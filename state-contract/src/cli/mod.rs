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
    /// The name errors are prefixed with.
    pub name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runner: "state-contract".to_owned(),
            decisions: PathBuf::from("docs/decisions"),
            name: "state-contract".to_owned(),
        }
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
        usage: "task [<id>|list|add|start|set|step|note|close|reopen|drop] [options]",
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
        name: "state",
        usage: "state <set key=value …|next (--body <text>|--stdin)>",
        summary: "change the `STATE:` line, or the `## Next` paragraph",
        run: |global, _, args| write::state(&global.root, args, &ProcessGit::new(&global.root)),
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
        Some("add" | "start" | "set" | "step" | "note" | "close" | "reopen" | "drop") => write::run(&global.root, args),
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const HOST: &[Command] =
        &[Command { name: "layers", usage: "layers", summary: "the host's own rule", run: |_, _, _| Ok(()) }];

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
