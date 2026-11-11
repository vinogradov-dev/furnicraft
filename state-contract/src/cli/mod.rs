//! The tool: one command table that drives dispatch and the help, and nothing hand-written twice.
//!
//! A project imports this crate and registers at most a few commands of its own — FurniCraft adds
//! only its layer rules and its `ci` step list — so the plan surface, its `--help` and its behaviour
//! come with the version of the crate, not with a copy in the host's runner.
//!
//! The crate still knows nothing about the product: the layout it reads is the plan convention plus
//! two defaults a project may override in `plan/state.conf` (the runner name the status block points
//! at and the decisions directory the ADR titles come from).

pub mod docs;
pub mod read;
pub mod write;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::ProcessGit;

/// The layout and wording a project may override; every field has the convention as its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The command line a session runs, printed in the status block and in the task report.
    pub runner: String,
    /// Where the architecture decisions live, relative to the project root.
    pub decisions: PathBuf,
    /// The project name the `agents` section speaks about.
    pub project: String,
    /// The name errors are prefixed with.
    pub name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runner: "state-contract".to_owned(),
            decisions: PathBuf::from("docs/decisions"),
            project: "the project".to_owned(),
            name: "state-contract".to_owned(),
        }
    }
}

impl Config {
    /// Reads `plan/state.conf` when the project has one.
    ///
    /// The file is `key = value` lines with `#` comments and three keys — `runner`, `decisions`,
    /// `project`. A key the file does not carry keeps its default, an unknown key is an error (a typo
    /// must not be ignored), and a missing file is not an error: every default is the convention.
    pub fn read(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join("plan/state.conf");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot read `plan/state.conf`: {error}")),
        };
        let mut config = Config::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("`plan/state.conf` line {} is not `key = value`", index.saturating_add(1)));
            };
            let value = value.trim().trim_matches('"').to_owned();
            match key.trim() {
                "runner" => config.runner = value,
                "decisions" => config.decisions = PathBuf::from(value),
                "project" => config.project = value,
                other => {
                    return Err(format!(
                        "`plan/state.conf` line {} names `{other}`; the keys are runner, decisions and project",
                        index.saturating_add(1)
                    ));
                }
            }
        }
        Ok(Some(config))
    }

    /// The usage line the status block and the task report point at.
    pub fn task_command(&self) -> String {
        format!("{} task", self.runner)
    }
}

/// One command: its name, its usage line, one sentence about it, and what it does.
///
/// The table is the single source of truth: `--help` is generated from it, and a host registers its
/// own commands into the same list, so one help lists everything the runner can do.
pub struct Command {
    /// The word that selects the command.
    pub name: &'static str,
    /// The usage line, printed by `help <name>`.
    pub usage: &'static str,
    /// What the command does, one line.
    pub summary: &'static str,
    /// Runs the command; the flags it does not know are its own business.
    pub run: fn(&Global, &Config, &[String]) -> Result<(), String>,
}

/// The flags every command accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    /// The project root that holds `plan/`.
    pub root: PathBuf,
    /// The frame width in characters.
    pub width: usize,
    /// Whether ANSI colour may be emitted.
    pub color: bool,
}

/// The commands this crate ships, in help order.
pub const COMMANDS: &[Command] = &[
    Command {
        name: "plan",
        usage: "plan",
        summary: "check that plan/STATUS.md, plan/BACKLOG.md, plan/SESSION.md and the reference files agree",
        run: |global, _, _| run_plan(global),
    },
    Command {
        name: "status",
        usage: "status",
        summary: "print the session-start block for the current task",
        run: |global, config, _| run_status(global, config),
    },
    Command {
        name: "task",
        usage: "task [<id>|list|add|start|set|step|note|close|reopen|take|drop] [options]",
        summary: "print one task in full, list tasks, or change the plan",
        run: run_task,
    },
    Command {
        name: "next",
        usage: "next",
        summary: "the report for the task `STATE:` names",
        run: |global, config, args| read::report(&global.root, config, global, args),
    },
    Command {
        name: "dashboard",
        usage: "dashboard [--task <id>]",
        summary: "print a styled overview of the plan; each section names its source file",
        run: run_dashboard,
    },
    Command {
        name: "docs",
        usage: "docs",
        summary: "check that the decisions README lists every decision and nothing else",
        run: |global, config, _| run_docs(global, config),
    },
    Command {
        name: "agents",
        usage: "agents",
        summary: "print the session contract a project pastes into its agent instructions",
        run: |_, config, _| run_agents(config),
    },
    Command {
        name: "init",
        usage: "init [--milestone <id>] [--task <id>] [--force]",
        summary: "scaffold `plan/` for a new project, or fail if a plan is already there",
        run: run_init,
    },
    Command {
        name: "state",
        usage: "state <set key=value …|next|where (--body <text>|--stdin)> [--task <id>] [--repair]",
        summary: "change the `STATE:` line, the `## Next` paragraph or the `## Where we are` prose",
        run: |global, _, args| write::state(&global.root, args, &ProcessGit::new(&global.root)),
    },
    Command {
        name: "file",
        usage: "file <plan/map.md|plan/debt.md|…> (--body <text>|--stdin) [--dry-run]",
        summary: "write one reference file of the plan through the gate, so two sessions never fight",
        run: |global, _, args| write::file(&global.root, args, &ProcessGit::new(&global.root)),
    },
];

/// Dispatches one command line; `extra` is the host's own table, appended to [`COMMANDS`].
pub fn run(global: &Global, config: &Config, extra: &[Command], args: &[String]) -> Result<(), String> {
    let Some(word) = args.first().map(String::as_str) else {
        print_help(config, extra);
        return Ok(());
    };
    if word == "help" || word == "--help" || word == "-h" {
        match args.get(1) {
            Some(name) => return help_for(name, extra),
            None => {
                print_help(config, extra);
                return Ok(());
            }
        }
    }
    let command = COMMANDS.iter().chain(extra).find(|command| command.name == word);
    match command {
        Some(command) => (command.run)(global, config, &args[1..]),
        None => {
            print_help(config, extra);
            Err(format!("unknown command `{word}`"))
        }
    }
}

/// The entry point a host calls: the crate's table plus its own commands.
pub fn main_with(extra: &'static [Command], config: Config) -> i32 {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_main(&root, extra, config)
}

/// [`main_with`] for a root the caller names, so a test needs no working directory.
pub fn run_main(root: &Path, extra: &'static [Command], config: Config) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (global, rest) = match split_globals(root, &args) {
        Ok(parts) => parts,
        Err(message) => {
            eprintln!("{}: {message}", config.name);
            return 2;
        }
    };
    // The project's layout comes from the root the command line names, not from the working
    // directory: `--root` has to move the config with it.
    let config = match Config::read(&global.root) {
        Ok(Some(project)) => Config { name: config.name, ..project },
        Ok(None) => config,
        Err(message) => {
            eprintln!("{}: {message}", config.name);
            return 2;
        }
    };
    match run(&global, &config, extra, &rest) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{}: {message}", config.name);
            1
        }
    }
}

/// [`main_with`] with the crate's defaults and no host commands, which is what the crate's own
/// binary runs.
pub fn main() -> i32 {
    main_with(&[], Config::default())
}

/// Splits `--root`, `--width` and `--no-color` off the arguments, leaving the command's own.
pub fn split_globals(root: &Path, args: &[String]) -> Result<(Global, Vec<String>), String> {
    let mut global = Global {
        root: root.to_path_buf(),
        width: 80,
        color: !no_color_requested(args) && std::io::stdout().is_terminal(),
    };
    let mut rest = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--root" => {
                let value = args.get(index).ok_or("--root needs a value")?;
                index += 1;
                global.root = PathBuf::from(value);
            }
            "--width" => {
                let value = args.get(index).ok_or("--width needs a value")?;
                index += 1;
                global.width = value.parse::<usize>().map_err(|_| format!("--width needs a number, got `{value}`"))?;
            }
            "--no-color" => {}
            other => rest.push(other.to_owned()),
        }
    }
    if global.width < 24 {
        return Err(format!("--width must be at least 24, got {}", global.width));
    }
    Ok((global, rest))
}

/// Whether `--no-color` is anywhere on the line, before the globals are split.
fn no_color_requested(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--no-color")
}

/// The gate's violations as one message, each printed once for the reader.
fn violations(errors: Vec<String>) -> String {
    for error in &errors {
        eprintln!("error: {error}");
    }
    format!("{} violation(s)", errors.len())
}

/// The table as help: every command, one line each, the host's included.
fn print_help(config: &Config, extra: &[Command]) {
    println!("usage: {} <command> [options]", config.runner);
    for command in COMMANDS.iter().chain(extra) {
        println!("  {:<12} {}", command.name, command.summary);
    }
    println!();
    println!("every command takes `--root <path>`, `--width <n>` and `--no-color`;");
    println!("`help <command>` prints one usage line; the plan lives in `plan/` under the project root");
}

/// One command's usage and summary, or a message naming what the table has.
fn help_for(name: &str, extra: &[Command]) -> Result<(), String> {
    match COMMANDS.iter().chain(extra).find(|command| command.name == name) {
        Some(command) => {
            println!("usage: {}", command.usage);
            println!("{}", command.summary);
            Ok(())
        }
        None => {
            let known: Vec<&str> = COMMANDS.iter().chain(extra).map(|command| command.name).collect();
            Err(format!("no command `{name}`; try one of {}", known.join(", ")))
        }
    }
}

/// `plan`: the gate, with the count of what it checked.
fn run_plan(global: &Global) -> Result<(), String> {
    match crate::check(&global.root) {
        Ok(summary) => {
            println!("plan: {} tasks, state {}", summary.tasks.len(), summary.state.status);
            println!("plan: ok");
            Ok(())
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            Err(format!("the plan has {} violation(s)", violations.len()))
        }
    }
}

/// `status`: the session-start block, pointing at this tool for the full task.
fn run_status(global: &Global, config: &Config) -> Result<(), String> {
    let detail = format!("{} task", config.runner);
    let text =
        crate::status_with_detail(&global.root, &ProcessGit::new(&global.root), Some(&detail)).map_err(violations)?;
    print!("{text}");
    Ok(())
}

/// `task`: one task, the list, or a verb that writes.
fn run_task(global: &Global, config: &Config, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("list") => read::list(&global.root, &args[1..]),
        Some("add" | "start" | "set" | "step" | "note" | "close" | "reopen" | "take" | "drop") => {
            write::run(&global.root, args)
        }
        _ => read::report(&global.root, config, global, args),
    }
}

/// `dashboard`: the framed overview, with one task optionally expanded.
fn run_dashboard(global: &Global, _config: &Config, args: &[String]) -> Result<(), String> {
    let mut options = crate::DashboardOptions { width: global.width, color: global.color, task: None };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--task" => {
                let value = args.get(index).ok_or("--task needs a task id")?;
                index += 1;
                options.task = Some(value.clone());
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    match crate::dashboard(&global.root, &options) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(errors) => Err(violations(errors)),
    }
}

/// `docs`: the ADR index against the files it indexes.
fn run_docs(global: &Global, config: &Config) -> Result<(), String> {
    match docs::check_in(&global.root, &config.decisions) {
        Ok(count) => {
            println!("docs: {count} decisions indexed");
            println!("docs: ok");
            Ok(())
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            Err(format!("the index has {} violation(s)", violations.len()))
        }
    }
}

/// The session contract, embedded from the template so the tool and the instructions share a version.
const AGENTS_SECTION: &str = include_str!("../../templates/AGENTS.section.md");
const STATUS_TEMPLATE: &str = include_str!("../../templates/STATUS.md");
const BACKLOG_TEMPLATE: &str = include_str!("../../templates/BACKLOG.md");
const SESSION_TEMPLATE: &str = include_str!("../../templates/SESSION.template.md");
const NOTE_TEMPLATE: &str = include_str!("../../templates/NOTE.md");

/// `agents`: the section a project pastes into the file its agent reads, with its own command names.
fn run_agents(config: &Config) -> Result<(), String> {
    print!("{}", agents_text(config));
    Ok(())
}

/// The section with the project's own names in it.
fn agents_text(config: &Config) -> String {
    AGENTS_SECTION.replace("{{PROJECT}}", &config.project).replace("{{TASK_COMMAND}}", &config.task_command())
}

/// `init`: write a plan that passes the gate, so a new project starts from a green one.
fn run_init(global: &Global, config: &Config, args: &[String]) -> Result<(), String> {
    run_init_with(global, config, args, &ProcessGit::new(&global.root))
}

/// [`run_init`] with the git facts supplied by `git`, so a test needs no repository.
fn run_init_with(global: &Global, config: &Config, args: &[String], git: &dyn crate::Git) -> Result<(), String> {
    let mut milestone = "M1".to_owned();
    let mut task = "T1".to_owned();
    let mut force = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--force" => force = true,
            "--milestone" => milestone = args.get(index).ok_or("--milestone needs a value")?.clone(),
            "--task" => task = args.get(index).ok_or("--task needs a value")?.clone(),
            other => return Err(format!("unknown option `{other}`")),
        }
        if matches!(arg, "--milestone" | "--task") {
            index += 1;
        }
    }
    let status = global.root.join("plan/STATUS.md");
    if status.exists() && !force {
        return Err("`plan/STATUS.md` exists; `--force` overwrites it".to_owned());
    }
    let today = git.commit_date("HEAD").unwrap_or_else(|| "1970-01-01".to_owned());
    let files = [
        ("plan/STATUS.md", filled(STATUS_TEMPLATE, &milestone, &task, &today, config)),
        ("plan/BACKLOG.md", filled(BACKLOG_TEMPLATE, &milestone, &task, &today, config)),
        ("plan/SESSION.template.md", SESSION_TEMPLATE.to_owned()),
        ("plan/NOTE.template.md", NOTE_TEMPLATE.to_owned()),
        ("plan/state.conf", config_file(config)),
    ];
    // The hint a fresh entry carries has to name a path that exists, so the scaffold creates it.
    let source = global.root.join("src/lib.rs");
    if !source.exists() {
        if let Some(parent) = source.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create `{}`: {error}", parent.display()))?;
        }
        std::fs::write(&source, "//! The first module.\n")
            .map_err(|error| format!("cannot write `src/lib.rs`: {error}"))?;
        println!("src/lib.rs");
    }
    for (path, text) in &files {
        let full = global.root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create `{}`: {error}", parent.display()))?;
        }
        std::fs::write(&full, text).map_err(|error| format!("cannot write `{path}`: {error}"))?;
        println!("{path}");
    }
    // Without a checkout the `base=` rule cannot be checked at all; say so instead of failing.
    if git.head().is_none() {
        println!("init: the plan is written; the gate needs a git checkout, run it once the first commit exists");
        return Ok(());
    }
    match crate::check_with(&global.root, git) {
        Ok(_) => {
            println!("init: the plan passes the gate");
            Ok(())
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            Err(format!("init: the fresh plan has {} violation(s)", violations.len()))
        }
    }
}

/// A template with the placeholders the scaffold fills.
fn filled(template: &str, milestone: &str, task: &str, date: &str, config: &Config) -> String {
    template
        .replace("{{MILESTONE}}", milestone)
        .replace("{{TASK_ID}}", task)
        .replace("{{DATE}}", date)
        .replace("{{PROJECT}}", &config.project)
        .replace("{{TASK_COMMAND}}", &config.task_command())
}

/// The `plan/state.conf` a scaffold writes: the defaults spelled out, so a reader can change them.
fn config_file(config: &Config) -> String {
    format!(
        "# The plan tool's layout. Every key has this value as its default; delete a line to keep it.\n\
         # The command line a session runs, printed by `status` and `task`.\n\
         runner = {}\n\
         # Where the architecture decisions live.\n\
         decisions = {}\n\
         # The project name the `agents` section speaks about.\n\
         project = {}\n",
        config.runner,
        config.decisions.display(),
        config.project
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const HOST: &[Command] =
        &[Command { name: "layers", usage: "layers", summary: "the host's own rule", run: |_, _, _| Ok(()) }];

    /// The git facts a scaffold needs, without a repository.
    struct FakeGit;

    impl crate::Git for FakeGit {
        fn head(&self) -> Option<String> {
            Some("abc1234".to_owned())
        }

        fn commit_exists(&self, hash: &str) -> bool {
            hash == "HEAD" || hash == "abc1234"
        }

        fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
            (ancestor == "HEAD" || ancestor == "abc1234") && (descendant == "HEAD" || descendant == "abc1234")
        }

        fn commit_date(&self, hash: &str) -> Option<String> {
            (hash == "HEAD" || hash == "abc1234").then(|| "2026-11-10".to_owned())
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("state-contract-init-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch directory is writable");
        root
    }

    #[test]
    fn a_project_without_a_config_gets_the_conventions() {
        let root = scratch("no-config");
        assert_eq!(Config::read(&root).expect("a missing file is not an error"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_config_overrides_the_defaults_and_rejects_a_typo() {
        let root = scratch("config");
        std::fs::create_dir_all(root.join("plan")).expect("plan is writable");
        std::fs::write(
            root.join("plan/state.conf"),
            "# a comment\nrunner = cargo xtask\ndecisions = docs/adr\nproject = FurniCraft\n",
        )
        .expect("the config is writable");
        let config = Config::read(&root).expect("the config parses").expect("the file exists");
        assert_eq!(config.runner, "cargo xtask");
        assert_eq!(config.decisions, PathBuf::from("docs/adr"));
        assert_eq!(config.project, "FurniCraft");
        assert_eq!(config.task_command(), "cargo xtask task");

        std::fs::write(root.join("plan/state.conf"), "runnerr = oops\n").expect("the config is writable");
        let error = Config::read(&root).expect_err("a typo is an error");
        assert!(error.contains("runnerr"), "{error}");
        std::fs::write(root.join("plan/state.conf"), "no equals sign\n").expect("the config is writable");
        assert!(Config::read(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_agents_section_speaks_about_the_project_and_its_command() {
        let config = Config { project: "FurniCraft".to_owned(), runner: "cargo xtask".to_owned(), ..Config::default() };
        let text = agents_text(&config);
        assert!(text.contains("FurniCraft"), "{text}");
        assert!(text.contains("cargo xtask task"), "{text}");
        assert!(!text.contains("{{"), "every placeholder is filled: {text}");
    }

    #[test]
    fn a_scaffold_writes_a_plan_that_passes_the_gate() {
        let root = scratch("init");
        let global = Global { root: root.clone(), width: 80, color: false };
        let config = Config::default();
        run_init_with(&global, &config, &[], &FakeGit).expect("the scaffold succeeds");
        for path in [
            "src/lib.rs",
            "plan/STATUS.md",
            "plan/BACKLOG.md",
            "plan/SESSION.template.md",
            "plan/NOTE.template.md",
            "plan/state.conf",
        ] {
            assert!(root.join(path).is_file(), "{path} is written");
        }
        crate::check_with(&root, &FakeGit).expect("the scaffolded plan passes the gate");
        let error = run_init_with(&global, &config, &[], &FakeGit).expect_err("a second init refuses");
        assert!(error.contains("--force"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_table_holds_every_command_once() {
        let names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "a command appears twice: {names:?}");
        for command in COMMANDS {
            assert!(!command.summary.is_empty() && !command.usage.is_empty(), "{} is undocumented", command.name);
            assert!(command.usage.starts_with(command.name), "{} usage starts with its name", command.name);
        }
    }

    #[test]
    fn a_host_command_joins_the_same_help() {
        let error = run(
            &Global { root: PathBuf::from("."), width: 80, color: false },
            &Config::default(),
            HOST,
            &["help".to_owned(), "layers".to_owned()],
        );
        assert!(error.is_ok(), "the host command is known to the help");
    }

    #[test]
    fn an_unknown_command_names_what_the_table_has() {
        let error = run(
            &Global { root: PathBuf::from("."), width: 80, color: false },
            &Config::default(),
            HOST,
            &["nothing".to_owned()],
        )
        .expect_err("nothing is not a command");
        assert!(error.contains("unknown command `nothing`"), "{error}");
        let error = help_for("nothing", HOST).expect_err("nothing is not a command either");
        assert!(error.contains("layers") && error.contains("task"), "the message lists the table: {error}");
    }

    #[test]
    fn the_globals_are_split_off_and_the_rest_is_the_command() {
        let args: Vec<String> =
            ["task", "--width", "100", "S14", "--no-color", "--md"].iter().map(|arg| (*arg).to_owned()).collect();
        let (global, rest) = split_globals(Path::new("/project"), &args).expect("the globals parse");
        assert_eq!(global.root, PathBuf::from("/project"));
        assert_eq!(global.width, 100);
        assert!(!global.color, "--no-color wins");
        assert_eq!(rest, ["task", "S14", "--md"]);
    }

    #[test]
    fn a_bad_width_is_a_usage_error() {
        let args: Vec<String> = ["--width", "10"].iter().map(|arg| (*arg).to_owned()).collect();
        assert!(split_globals(Path::new("."), &args).is_err());
        let args: Vec<String> = ["--width"].iter().map(|arg| (*arg).to_owned()).collect();
        assert!(split_globals(Path::new("."), &args).is_err());
    }
}
