//! The writer half of the plan: the verbs that change `plan/*.md`.
//!
//! Every verb builds the new text with [`crate::edit`], then runs the gate over the whole plan as
//! it would be after the change, with the *real* repository's git facts, and only then lets the
//! bytes land through a temp file and a rename. A verb that would break the plan writes nothing and
//! prints the violations instead (ADR 0085).
//!
//! `--repair` is the one exception: a plan can carry several violations at once, and fixing them one
//! verb at a time would be refused at every step, because the others are still there. The flag lets
//! a write through when it strictly shrinks the set of violations — it removes at least one and adds
//! none — so the plan can be brought back with the commands instead of by hand.
//!
//! `--dry-run` prints the diff and stops. `plan/.lock` keeps two writers apart.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::{Git, ProcessGit, TaskStatus, edit};

/// The command line a verb that changes the plan accepts.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Print the diff and write nothing.
    pub dry_run: bool,
    /// Let the write through when it shrinks the set of violations, even though it does not empty it.
    pub repair: bool,
    /// The arguments that are not flags, in order.
    pub words: Vec<String>,
    /// `--key value` pairs, in order.
    pub values: Vec<(String, String)>,
    /// Flags that take no value.
    pub flags: Vec<String>,
}

impl Options {
    /// Parses `--key value`, `--flag` and bare words; `value_flags` names the flags that take a value.
    pub fn parse(args: &[String], value_flags: &[&str]) -> Result<Self, String> {
        let mut options = Options::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            if arg == "--dry-run" {
                options.dry_run = true;
                continue;
            }
            if arg == "--repair" {
                options.repair = true;
                continue;
            }
            if let Some(flag) = arg.strip_prefix("--") {
                if value_flags.contains(&flag) {
                    let value = args.get(index).ok_or_else(|| format!("--{flag} needs a value"))?;
                    index += 1;
                    options.values.push((flag.to_owned(), value.clone()));
                } else {
                    // A bare flag: `--tick`, `--stdin`, `--keep-note`. A misspelled value flag
                    // still fails later, because its value is then missing.
                    options.flags.push(flag.to_owned());
                }
                continue;
            }
            options.words.push(arg.to_owned());
        }
        Ok(options)
    }

    /// The value of `--key`, when it was given.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
    }

    /// The value of `--key` or the message that names the missing flag.
    pub fn required(&self, key: &str) -> Result<&str, String> {
        self.value(key).ok_or_else(|| format!("--{key} is required"))
    }

    /// Whether the flag was given.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    /// The body `--key` names: the value, or standard input with `--stdin`.
    pub fn body(&self, key: &str) -> Result<String, String> {
        if self.flag("stdin") {
            return read_stdin();
        }
        self.required(key).map(str::to_owned)
    }
}

/// Reads standard input to the end.
fn read_stdin() -> Result<String, String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|error| format!("cannot read stdin: {error}"))?;
    Ok(text)
}

/// `task add`, `start`, `set`, `step`, `note`, `close`, `reopen`, `take`, `defer`, `drop` and `phase`,
/// plus `state set`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    run_with(root, args, &ProcessGit::new(root))
}

/// [`run`] with the git facts supplied by `git`, so a test needs no repository.
pub fn run_with(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err("usage: cargo xtask task <add|start|set|step|note|close|reopen|take|defer|drop|phase> …".to_owned());
    };
    let rest = &args[1..];
    match verb {
        "add" => add(root, rest, git),
        "start" => start(root, rest, git),
        "set" => set(root, rest, git),
        "step" => step(root, rest, git),
        "note" => note(root, rest, git),
        "close" => close(root, rest, git),
        "reopen" => reopen(root, rest, git),
        "drop" => drop(root, rest, git),
        "take" => take(root, rest, git),
        "defer" => defer(root, rest, git),
        "phase" => phase(root, rest, git),
        other => Err(format!("unknown verb `{other}`")),
    }
}

/// `task add --phase <L> [--phase-name <text>] --files <paths> [--section Now] [--at top|end]
/// [--why/--shape/--done <text>] <lead>`
///
/// `--at top` puts the task at the head of the work order and points `STATE: task=` at it, because
/// the tool cannot leave a plan in which the current task is not the first open one: an owner-directed
/// interjection joins the order in one validated write instead of two that are each refused.
///
/// `--phase` is one capital letter, and `--phase-name` declares it in the legend in the same write,
/// so the first task of a new phase does not need a second command. Without a name the phase must
/// already be declared: a file whose ids use a letter its legend does not explain is the drift this
/// gate reports.
fn add(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options =
        Options::parse(args, &["phase", "phase-name", "section", "files", "why", "shape", "done", "at", "next"])?;
    let phase = options.required("phase")?.to_uppercase();
    let letter = one_letter(&phase)?;
    let files = options.required("files")?;
    let lead = options.words.join(" ");
    if lead.trim().is_empty() {
        return Err("a new task needs its opening sentence: `task add --phase S --files … \"what it is\"`".to_owned());
    }
    let section = options.value("section").unwrap_or("Now");
    let at = match options.value("at") {
        None | Some("end") => edit::At::End,
        Some("top") => edit::At::Top,
        Some(other) => return Err(format!("`--at` takes `top` or `end`, not `{other}`")),
    };
    if at == edit::At::Top && !section.eq_ignore_ascii_case("Now") {
        return Err(format!(
            "`--at top` puts the task in the work order, so it needs the `Now` section, not `{section}`"
        ));
    }
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = match options.value("phase-name") {
        Some(name) => edit::set_phases(&backlog, &[(letter, name.to_owned())])?,
        None => {
            if let Some(legend) = edit::legend(&backlog)
                && legend.problems.is_empty()
                && legend.name(letter).is_none()
            {
                return Err(format!(
                    "phase {letter} is not declared in the legend; `cargo xtask task phase {letter} --name \"…\"` or `--phase-name`"
                ));
            }
            backlog
        }
    };
    let id = edit::next_id(&backlog, &phase);
    let mut fields: Vec<(String, String)> = Vec::new();
    for (label, key) in [("Why", "why"), ("Shape", "shape"), ("Done", "done")] {
        if let Some(body) = options.value(key) {
            fields.push((label.to_owned(), body.to_owned()));
        }
    }
    let block = edit::new_entry(&id, lead.trim(), &fields, files);
    let backlog = edit::insert_entry(&backlog, section, at, &block)?;
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog);
    if at == edit::At::Top {
        let status = read(root, "plan/STATUS.md")?;
        let mut status = edit::set_state(&status, &[("task".to_owned(), id.clone())])?;
        if edit::has_section(&status, "Next") {
            let body = match options.value("next") {
                Some(next) => next.to_owned(),
                None => format!("{id} is next: {}", lead.trim()),
            };
            status = edit::set_section(&status, "Next", &body)?;
        }
        changes = changes.with("plan/STATUS.md", status);
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} added to `## {section}`");
    }
    Ok(())
}

/// `task phase <L> --name <text>`, or `task phase --declare "L=text" [--declare "M=text" …]`
///
/// The legend of `plan/BACKLOG.md` — what each id prefix means — is written here and nowhere else,
/// so a session can declare a phase when it opens one and the paragraph cannot drift from the ids
/// the file uses. Several letters may travel in one call because a file whose legend is still prose
/// has to gain every letter its ids use at once: the gate refuses a plan whose ids name a phase the
/// legend does not declare, so a one-letter-at-a-time migration could not land.
fn phase(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["name", "declare"])?;
    let mut declares: Vec<(char, String)> = Vec::new();
    for (key, value) in &options.values {
        if key != "declare" {
            continue;
        }
        let Some((letter, name)) = value.split_once('=') else {
            return Err(format!("`--declare` takes `L=text`, not `{value}`"));
        };
        declares.push((one_letter(&letter.trim().to_uppercase())?, name.trim().to_owned()));
    }
    if let Some(word) = options.words.first() {
        let letter = one_letter(&word.to_uppercase())?;
        declares.push((letter, options.required("name")?.to_owned()));
    }
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = edit::set_phases(&backlog, &declares)?;
    let changes = Changes::new(root).with("plan/BACKLOG.md", backlog);
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("the phase legend changed");
    }
    Ok(())
}

/// The one capital letter a phase is, or the sentence that says what a phase is.
fn one_letter(text: &str) -> Result<char, String> {
    let mut chars = text.chars();
    let Some(letter) = chars.next() else {
        return Err("a phase is one capital letter, like `K`".to_owned());
    };
    if chars.next().is_some() || !letter.is_ascii_uppercase() {
        return Err(format!("`{text}` is not a phase: a phase is one capital letter, like `K`"));
    }
    Ok(letter)
}

/// `task start <ID> [--goal <text>] [--now <text>] [--run <cmd>]`
fn start(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["goal", "now", "run", "resume"])?;
    let id = options.words.first().ok_or("task start needs a task id")?.clone();
    let summary = crate::check_with(root, git).map_err(violations)?;
    let task = find(&summary.tasks, &id)?;
    if task.section != "Now" {
        return Err(format!("task {id} sits in `## {}`; only a task in `## Now` can start", task.section));
    }
    if let Some(other) = summary.tasks.iter().find(|task| task.status == TaskStatus::Doing && task.id != id) {
        return Err(format!(
            "task {} is still in progress; close it before starting {id} (`cargo xtask task close {}`)",
            other.id, other.id
        ));
    }
    // Starting the task that is already running would overwrite the note a session is working from.
    if task.status == TaskStatus::Doing && !options.flag("force") {
        return Err(format!(
            "task {id} is already in progress; `--force` rewrites `plan/SESSION.md`, `task step` adds to it"
        ));
    }
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = edit::set_marker(&backlog, &id, edit::DOING)?;
    let status = read(root, "plan/STATUS.md")?;
    let status =
        edit::set_state(&status, &[("task".to_owned(), id.clone()), ("state".to_owned(), "doing".to_owned())])?;
    let session = session_note(&id, task, &options);
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog).with("plan/STATUS.md", status);
    changes = changes.with("plan/SESSION.md", session);
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} started; `plan/SESSION.md` holds the note");
    }
    Ok(())
}

/// The note a start writes: the four fields the gate requires, filled from the entry.
fn session_note(id: &str, task: &crate::Task, options: &Options) -> String {
    let goal = options.value("goal").unwrap_or(&task.lead);
    let now = options.value("now").unwrap_or("starting the task");
    let run = options.value("run").unwrap_or("cargo xtask task");
    let resume = options.value("resume").unwrap_or("read the first `files:` path and its callers");
    format!(
        "# Session\n\n```text\ntask: {id}\ngoal: {goal}\n\nsteps:\n  [ ] {now}\n\nnow: {now}\nblocked: nothing\n\
         run: {run} {id}\nresume: {resume}\n```\n",
    )
}

/// `task set <ID> --field <Label> (--body <text>|--stdin) [--files <paths>]`
fn set(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["field", "body", "files"])?;
    let id = options.words.first().ok_or("task set needs a task id")?.clone();
    let field = options.required("field")?.to_owned();
    let mut backlog = read(root, "plan/BACKLOG.md")?;
    let mut changes = Changes::new(root);
    if let Some(files) = options.value("files") {
        backlog = edit::set_files(&backlog, &id, files)?;
        changes = changes.with("plan/BACKLOG.md", backlog.clone());
    }
    if field != "files" {
        let body = options.body("body")?;
        backlog = edit::set_field(&backlog, &id, &field, &body)?;
        changes = changes.with("plan/BACKLOG.md", backlog);
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id}: `{field}` updated");
    }
    Ok(())
}

/// `task step <ID> [--tick | --add <text>]`: the note's step list, one line at a time.
fn step(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["add"])?;
    let id = options.words.first().ok_or("task step needs a task id")?.clone();
    let session = read(root, "plan/SESSION.md")?;
    let session = if let Some(text) = options.value("add") {
        edit::add_step(&session, text)?
    } else {
        edit::tick_step(&session)?
    };
    let changes = Changes::new(root).with("plan/SESSION.md", session);
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id}: the session note changed");
    }
    Ok(())
}

/// `task note <ID> (--body <text>|--stdin)`: appends to the research note, never rewrites it.
fn note(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["body"])?;
    let id = options.words.first().ok_or("task note needs a task id")?.clone();
    let body = options.body("body")?;
    let summary = crate::check_with(root, git).map_err(violations)?;
    find(&summary.tasks, &id)?;
    let path = format!("plan/notes/{id}.md");
    let existing = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
    let text = if existing.trim().is_empty() {
        format!("# {id} — {}\n\n{}\n", first_line(&body), body.trim())
    } else {
        format!("{}\n{}\n", existing.trim_end(), body.trim())
    };
    let changes = Changes::new(root).with(&path, text);
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id}: `{path}` appended");
    }
    Ok(())
}

/// The first sentence of a note body, for the heading `note` writes.
fn first_line(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("research");
    line.trim_start_matches('#').trim().chars().take(80).collect()
}

/// `task close <ID> [--commit <hash|HEAD>] [--next <text>]`
fn close(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["commit", "next"])?;
    let id = options.words.first().ok_or("task close needs a task id")?.clone();
    let summary = crate::check_with(root, git).map_err(violations)?;
    let task = find(&summary.tasks, &id)?;
    if task.status == TaskStatus::Done {
        return Err(format!("task {id} is already done"));
    }
    let commit = options.value("commit").unwrap_or("HEAD");
    let hash = git.short(commit).ok_or_else(|| format!("`{commit}` is not a commit in this repository"))?;
    let suffix = if task.text.lines().count() > 1 { format!(" `[x]` `{hash}`") } else { format!(" — `{hash}`") };
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = edit::set_marker(&backlog, &id, edit::DONE)?;
    let backlog = edit::append_to_entry(&backlog, &id, &suffix)?;
    let backlog = edit::move_entry(&backlog, &id, "Done", edit::At::Top)?;
    let status = read(root, "plan/STATUS.md")?;
    let tasks = crate::parse_backlog(&backlog);
    let next = first_open(&tasks).ok_or_else(|| {
        "no unfinished task is left in `## Now`; set `STATE:` for the next milestone by hand".to_owned()
    })?;
    // The plan describes the tree that is checked out now, so `base` and `updated` follow `HEAD`,
    // not the commit the closed task names (a task may be closed long after it landed).
    let mut values = vec![("task".to_owned(), next.id.clone()), ("state".to_owned(), "idle".to_owned())];
    if let Some(head) = git.short("HEAD") {
        values.push(("base".to_owned(), head));
    }
    if let Some(date) = git.commit_date("HEAD") {
        values.push(("updated".to_owned(), date));
    }
    let status = edit::set_state(&status, &values)?;
    // The `## Next` paragraph names the task `STATE:` does, so a close that changes it must say what
    // the next paragraph is; otherwise the gate refuses and nothing is written.
    let status = match options.value("next") {
        Some(next) => edit::set_section(&status, "Next", next)?,
        None => status,
    };
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog).with("plan/STATUS.md", status);
    if root.join("plan/SESSION.md").exists() {
        changes = changes.removed("plan/SESSION.md");
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} closed into `## Done` at `{hash}`; `{next_id}` is next", next_id = next.id);
    }
    Ok(())
}

/// `task take <ID> [--at top|end] [--next <text>]`: moves an open entry into `## Now`.
///
/// The order of work is the order of `## Now`, and `## Bugs` and `## Next` are not part of it, so a
/// session that wants to work on an entry filed there has to move the line first. Doing that by hand
/// is forbidden (ADR 0085), and no verb could do it, so the work order could only advance while
/// `## Now` already held the next task. Taking into an occupied `## Now` leaves `STATE:` alone; into
/// an empty one it makes the taken task current and says so in `## Next`, because the gate requires
/// the paragraph to name the task `STATE:` does.
fn take(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["at", "next"])?;
    let id = options.words.first().ok_or("task take needs a task id")?.clone();
    let at = match options.value("at") {
        None | Some("end") => edit::At::End,
        Some("top") => edit::At::Top,
        Some(other) => return Err(format!("`--at` takes `top` or `end`, not `{other}`")),
    };
    // Parsed straight from the file rather than through the gate: an empty `## Now` leaves the plan
    // with no valid current task, and taking an entry into it is the write that repairs that. The
    // gate still runs on the result, so `--repair` is needed while a violation stands.
    let backlog_text = read(root, "plan/BACKLOG.md")?;
    let tasks = crate::parse_backlog(&backlog_text);
    let task = find(&tasks, &id)?;
    if task.status == TaskStatus::Done {
        return Err(format!("task {id} is done; `cargo xtask task reopen {id}` puts it back in the order"));
    }
    if task.section == "Now" {
        return Err(format!("task {id} is already in `## Now`"));
    }
    // The head of the order is the slot `STATE:` names, so nothing may take it from a task in
    // progress; the end of `## Now` is behind that task and disturbs nothing.
    if matches!(at, edit::At::Top)
        && let Some(other) = tasks.iter().find(|task| task.status == TaskStatus::Doing)
    {
        return Err(format!(
            "task {} is still in progress; `task take {id}` joins the end of `## Now` without taking its place",
            other.id
        ));
    }
    let occupied = first_open(&tasks).is_some();
    let backlog = edit::move_entry(&backlog_text, &id, "Now", at)?;
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog);
    if !occupied {
        let status = read(root, "plan/STATUS.md")?;
        let status =
            edit::set_state(&status, &[("task".to_owned(), id.clone()), ("state".to_owned(), "idle".to_owned())])?;
        let status = match options.value("next") {
            Some(next) => edit::set_section(&status, "Next", next)?,
            None => edit::set_section(&status, "Next", &format!("{id} is next: {}", task.lead))?,
        };
        changes = changes.with("plan/STATUS.md", status);
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} taken into `## Now`");
    }
    Ok(())
}

/// `task defer <ID> [--next <text>]`: moves an open entry out of the work order into `## Next`.
///
/// The inverse of [`take`], and the one write an owner needs when a task is real but its moment is
/// later than the rest of the order (R1 waits for the release): the entry keeps its fields, its
/// hint and its note, and stops being scheduled, so no session-start block names it until somebody
/// takes it back. The order must keep a head, so deferring the current task points `STATE:` at the
/// next open entry and names it in `## Next` in the same write; deferring a later entry leaves the
/// state alone. A running task is not deferred: its session note is the record of the step in hand.
fn defer(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["next"])?;
    let id = options.words.first().ok_or("task defer needs a task id")?.clone();
    let backlog_text = read(root, "plan/BACKLOG.md")?;
    let tasks = crate::parse_backlog(&backlog_text);
    let task = find(&tasks, &id)?;
    if task.status == TaskStatus::Done {
        return Err(format!("task {id} is done; there is nothing to defer"));
    }
    if task.status == TaskStatus::Doing {
        return Err(format!(
            "task {id} is in progress; `cargo xtask task close {id}` or `cargo xtask task reopen {id}` \
             ends its session before it can be deferred"
        ));
    }
    if task.section != "Now" {
        return Err(format!("task {id} sits in `## {}`, not in the work order", task.section));
    }
    let backlog = edit::move_entry(&backlog_text, &id, "Next", edit::At::End)?;
    let moved = crate::parse_backlog(&backlog);
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog);
    if first_open(&tasks).is_some_and(|head| head.id == id) {
        let next = first_open(&moved).ok_or_else(|| {
            format!(
                "deferring {id} would leave `## Now` with nothing to work on; close the milestone or add a \
                 task first"
            )
        })?;
        let status = read(root, "plan/STATUS.md")?;
        let status = edit::set_state(&status, &[("task".to_owned(), next.id.clone())])?;
        let status = match options.value("next") {
            Some(next) => edit::set_section(&status, "Next", next)?,
            None => edit::set_section(&status, "Next", &format!("{} is next: {}", next.id, next.lead))?,
        };
        changes = changes.with("plan/STATUS.md", status);
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} deferred to `## Next`");
    }
    Ok(())
}

/// `task reopen <ID> [--next <text>]`
fn reopen(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["next"])?;
    let id = options.words.first().ok_or("task reopen needs a task id")?.clone();
    let summary = crate::check_with(root, git).map_err(violations)?;
    let task = find(&summary.tasks, &id)?;
    if task.status != TaskStatus::Done {
        return Err(format!("task {id} is not done"));
    }
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = edit::strip_commit(&backlog, &id)?;
    let backlog = edit::set_marker(&backlog, &id, edit::OPEN)?;
    let backlog = edit::move_entry(&backlog, &id, "Now", edit::At::Top)?;
    let status = read(root, "plan/STATUS.md")?;
    let status = edit::set_state(&status, &[("task".to_owned(), id.clone()), ("state".to_owned(), "idle".to_owned())])?;
    // The reopened task becomes the current one, so `## Next` has to name it in the same write;
    // otherwise the gate refuses, and two hand edits are the only way to say what the plan means.
    let status = match options.value("next") {
        Some(next) => edit::set_section(&status, "Next", next)?,
        None if crate::validate::next_violation(&status, &id).is_some() => {
            return Err(format!(
                "`plan/STATUS.md` › `## Next` must name the reopened task {id}; \
                 `task reopen {id} --next \"{id} is next: …\"` rewrites it in the same write"
            ));
        }
        None => status,
    };
    let changes = Changes::new(root).with("plan/BACKLOG.md", backlog).with("plan/STATUS.md", status);
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} reopened at the top of `## Now`");
    }
    Ok(())
}

/// `task drop <ID> [--keep-note]`
fn drop(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &[])?;
    let id = options.words.first().ok_or("task drop needs a task id")?.clone();
    let summary = crate::check_with(root, git).map_err(violations)?;
    if summary.state.task == id {
        return Err(format!("task {id} is the current task; move `STATE:` to another task first"));
    }
    let backlog = read(root, "plan/BACKLOG.md")?;
    let backlog = edit::remove_entry(&backlog, &id)?;
    let mut changes = Changes::new(root).with("plan/BACKLOG.md", backlog);
    let note = format!("plan/notes/{id}.md");
    if !options.flag("keep-note") && root.join(&note).exists() {
        changes = changes.removed(&note);
    }
    apply(root, changes, &options, git)?;
    if !options.dry_run {
        println!("task {id} dropped");
    }
    Ok(())
}

/// The task with this id, or a message naming the id that is missing.
fn find<'a>(tasks: &'a [crate::Task], id: &str) -> Result<&'a crate::Task, String> {
    tasks.iter().find(|task| task.id == id).ok_or_else(|| format!("`plan/BACKLOG.md` has no task `{id}`"))
}

/// The first unfinished task in `## Now`, which is the task `STATE:` must name.
fn first_open(tasks: &[crate::Task]) -> Option<&crate::Task> {
    tasks.iter().find(|task| task.section == "Now" && task.status != TaskStatus::Done)
}

/// The files a verb wants to change, and the files it wants gone.
struct Changes {
    root: PathBuf,
    files: Vec<(String, String)>,
    removed: Vec<String>,
}

impl Changes {
    fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf(), files: Vec::new(), removed: Vec::new() }
    }

    fn with(mut self, path: &str, text: String) -> Self {
        self.files.push((path.to_owned(), text));
        self
    }

    fn removed(mut self, path: &str) -> Self {
        self.removed.push(path.to_owned());
        self
    }

    /// Whether anything is asked for at all.
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.removed.is_empty()
    }

    /// The files whose text actually changes, with the text that is on disk now.
    fn changed(&self) -> Vec<(&str, Option<String>, &str)> {
        self.files
            .iter()
            .map(|(path, text)| (path.as_str(), std::fs::read_to_string(self.root.join(path)).ok(), text.as_str()))
            .filter(|(_, old, new)| old.as_deref() != Some(*new))
            .collect()
    }
}

/// Validates the plan as it would be, then lands the change — or prints why it refused and writes
/// nothing.
///
/// A plan that is already broken cannot be repaired one verb at a time, because every verb is refused
/// while the other violations stand: `--repair` lets a write through when it strictly shrinks the set
/// of violations, so `plan/BACKLOG.md` and `plan/STATUS.md` can be brought back with commands.
fn apply(root: &Path, changes: Changes, options: &Options, git: &dyn Git) -> Result<(), String> {
    if changes.is_empty() {
        return Err("nothing to change".to_owned());
    }
    let changed = changes.changed();
    if changed.is_empty() && changes.removed.is_empty() {
        println!("no change");
        return Ok(());
    }
    let status = resulting(root, &changes, "plan/STATUS.md").ok_or("`plan/STATUS.md` is missing")?;
    let backlog = resulting(root, &changes, "plan/BACKLOG.md").ok_or("`plan/BACKLOG.md` is missing")?;
    let session = resulting(root, &changes, "plan/SESSION.md");
    let notes = notes_after(root, &changes)?;
    let plan = crate::PlanText { status: &status, backlog: &backlog, session: session.as_deref(), notes: Some(&notes) };
    let after = violations_of(root, &plan, git);
    let repairing = if after.is_empty() {
        false
    } else {
        let before = crate::check_with(root, git).err().unwrap_or_default();
        if options.repair && shrinks(&before, &after) {
            true
        } else {
            return Err(refusal(&after, options.repair, &before));
        }
    };

    for (path, old, new) in &changed {
        println!("{}", diff(path, old.as_deref().unwrap_or(""), new));
    }
    for path in &changes.removed {
        println!("--- {path}\n(deleted)");
    }
    if repairing {
        println!("repair: {} violation(s) still stand", after.len());
    }
    if options.dry_run {
        println!("dry run: nothing written");
        return Ok(());
    }

    let _lock = Lock::acquire(root)?;
    for (path, _, text) in &changed {
        write_atomic(&root.join(path), text)?;
    }
    for path in &changes.removed {
        std::fs::remove_file(root.join(path)).map_err(|error| format!("cannot delete `{path}`: {error}"))?;
    }
    Ok(())
}

/// The text `path` will have once the change lands, or `None` when the change deletes it.
fn resulting(root: &Path, changes: &Changes, path: &str) -> Option<String> {
    if changes.removed.iter().any(|removed| removed == path) {
        return None;
    }
    changes
        .files
        .iter()
        .find(|(candidate, _)| candidate == path)
        .map(|(_, text)| text.clone())
        .or_else(|| std::fs::read_to_string(root.join(path)).ok())
}

/// The notes directory as it will be: what is on disk, with this change applied.
fn notes_after(root: &Path, changes: &Changes) -> Result<Vec<(String, String)>, String> {
    let mut notes: Vec<(String, String)> = match std::fs::read_dir(root.join("plan/notes")) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                (name, text)
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("cannot read `plan/notes/`: {error}")),
    };
    notes.retain(|(name, _)| !changes.removed.iter().any(|path| path == &format!("plan/notes/{name}")));
    for (path, text) in &changes.files {
        let Some(name) = path.strip_prefix("plan/notes/") else { continue };
        notes.retain(|(candidate, _)| candidate != name);
        notes.push((name.to_owned(), text.clone()));
    }
    notes.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(notes)
}

/// The one-writer lock: `plan/.lock`, created exclusively and removed when the verb ends.
struct Lock {
    path: PathBuf,
}

impl Lock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let path = root.join("plan/.lock");
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Self { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err("`plan/.lock` exists: another session is writing the plan; delete it only if no session runs"
                    .to_owned())
            }
            Err(error) => Err(format!("cannot create `plan/.lock`: {error}")),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Writes through a sibling temp file and a rename, so a half-written plan never reaches the disk.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("plan");
    let temporary = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&temporary, text).map_err(|error| format!("cannot write `{}`: {error}", path.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| format!("cannot replace `{}`: {error}", path.display()))
}

/// The diff of one file: the lines that changed, with two lines of context either side.
fn diff(path: &str, old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let prefix = old_lines.iter().zip(&new_lines).take_while(|(a, b)| a == b).count();
    let suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old_lines.len().saturating_sub(prefix))
        .min(new_lines.len().saturating_sub(prefix));
    let context_start = prefix.saturating_sub(2);
    let mut out = format!("--- {path}\n");
    for line in &old_lines[context_start..prefix] {
        out.push_str(&format!("  {line}\n"));
    }
    for line in &old_lines[prefix..old_lines.len().saturating_sub(suffix)] {
        out.push_str(&format!("- {line}\n"));
    }
    for line in &new_lines[prefix..new_lines.len().saturating_sub(suffix)] {
        out.push_str(&format!("+ {line}\n"));
    }
    for line in new_lines.iter().skip(new_lines.len().saturating_sub(suffix)).take(2) {
        out.push_str(&format!("  {line}\n"));
    }
    out
}

/// The gate's violations as one message: they are printed once and the verb writes nothing.
fn violations(errors: Vec<String>) -> String {
    for error in &errors {
        eprintln!("plan: {error}");
    }
    format!("refused: the plan has {} violation(s); nothing was written", errors.len())
}

/// The violations the plan has once the change lands, or none when it is valid.
fn violations_of(root: &Path, plan: &crate::PlanText<'_>, git: &dyn Git) -> Vec<String> {
    crate::check_text(root, plan, git).err().unwrap_or_default()
}

/// Whether `after` is a strict subset of `before`: at least one violation gone, none new.
///
/// A repair may not trade one violation for another, and may not repeat one it claims to fix, so
/// `--repair` cannot be used to land a change that leaves the plan in a state nobody looked at.
fn shrinks(before: &[String], after: &[String]) -> bool {
    after.len() < before.len() && after.iter().all(|violation| before.contains(violation))
}

/// The refusal for a write that would leave violations, with the one line that says how to proceed.
///
/// `before` is what the plan has now: when the write takes a violation away and leaves the rest, the
/// message names `--repair`, which is the only way to bring a plan with several violations back.
fn refusal(after: &[String], repair: bool, before: &[String]) -> String {
    for error in after {
        eprintln!("plan: {error}");
    }
    if repair {
        eprintln!(
            "plan: `--repair` passes only a write that removes a violation and adds none; this one \
             leaves {} where the plan has {}",
            after.len(),
            before.len()
        );
    } else if after.len() < before.len() {
        eprintln!("plan: this write removes a violation; `--repair` passes it and leaves the rest");
    }
    format!("refused: the plan has {} violation(s); nothing was written", after.len())
}

/// Reads a required plan file.
fn read(root: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(path)).map_err(|error| format!("cannot read `{path}`: {error}"))
}

/// `state set key=value …`, `state next (--body <text>|--stdin) [--task <id>]` and
/// `state where (--body <text>|--stdin)`.
///
/// `state next --task <id>` names a new current task in the same write: the gate requires the
/// paragraph to name the task `STATE:` does, so a paragraph that moves on its own would be refused,
/// and the only way out would be a hand edit. `state where` owns the prose that describes the tree
/// `base` points at, so a session that sets `base=HEAD` can say what changed in the same command.
pub fn state(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["body", "task"])?;
    match options.words.first().map(String::as_str) {
        Some("next") => {
            let body = options.body("body")?;
            let status = read(root, "plan/STATUS.md")?;
            let status = edit::set_section(&status, "Next", &body)?;
            let status = match options.value("task") {
                Some(task) => edit::set_state(&status, &[("task".to_owned(), task.to_owned())])?,
                None => status,
            };
            return apply(root, Changes::new(root).with("plan/STATUS.md", status), &options, git);
        }
        Some("where") => {
            let body = options.body("body")?;
            let status = read(root, "plan/STATUS.md")?;
            let status = edit::set_section(&status, "Where we are", &body)?;
            return apply(root, Changes::new(root).with("plan/STATUS.md", status), &options, git);
        }
        Some("set") => {}
        _ => {
            return Err("usage: cargo xtask state <set key=value …|next|where (--body <text>|--stdin)>".to_owned());
        }
    }
    let mut values: Vec<(String, String)> = Vec::new();
    for word in options.words.iter().skip(1) {
        let (key, value) = word.split_once('=').ok_or_else(|| format!("`{word}` is not key=value"))?;
        values.push((key.to_owned(), value.to_owned()));
    }
    if values.is_empty() {
        return Err("state set needs at least one key=value".to_owned());
    }
    let status = read(root, "plan/STATUS.md")?;
    let status = edit::set_state(&status, &values)?;
    apply(root, Changes::new(root).with("plan/STATUS.md", status), &options, git)
}

/// The reference files a command owns: the plan's data lives here too, and a session that edits one
/// by hand leaves no diff through the tool and no gate run to catch a stale row.
const REFERENCE_FILES: &[&str] = &[
    "plan/map.md",
    "plan/debt.md",
    "plan/questions.md",
    "plan/refactor.md",
    "plan/rename.md",
    "plan/archive/done.md",
    "plan/SESSION.template.md",
];

/// `file <path> (--body <text>|--stdin) [--dry-run]`: one reference file, written through the gate.
///
/// `plan/BACKLOG.md`, `plan/STATUS.md` and `plan/SESSION.md` are not on the list: the `task` and
/// `state` verbs own their lines, and a whole-file rewrite would lose the entry formatting their
/// splices preserve.
pub fn file(root: &Path, args: &[String], git: &dyn Git) -> Result<(), String> {
    let options = Options::parse(args, &["body"])?;
    let path = options.words.first().ok_or("`file` needs the path of a reference file")?;
    if !REFERENCE_FILES.contains(&path.as_str()) {
        return Err(format!("`{path}` is not a reference file a command owns; one of {}", REFERENCE_FILES.join(", ")));
    }
    let body = options.body("body")?;
    if body.trim().is_empty() {
        return Err(format!("`{path}` would be empty; a reference file says something or it does not exist"));
    }
    let mut text = body.trim_end().to_owned();
    text.push('\n');
    apply(root, Changes::new(root).with(path, text), &options, git)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{Git, TaskStatus};

    /// The git facts a verb needs, without a repository.
    struct FakeGit;

    impl Git for FakeGit {
        fn head(&self) -> Option<String> {
            Some("abc1234".to_owned())
        }

        fn commit_exists(&self, hash: &str) -> bool {
            !hash.is_empty() && hash.chars().all(|ch| ch.is_ascii_hexdigit())
        }

        fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
            ancestor == "abc1234" && (descendant == "abc1234" || descendant == "fedcba9")
        }

        fn commit_date(&self, hash: &str) -> Option<String> {
            (hash == "HEAD" || self.commit_exists(hash)).then(|| "2026-11-10".to_owned())
        }

        fn short(&self, hash: &str) -> Option<String> {
            (hash == "HEAD" || self.commit_exists(hash)).then(|| "abc1234".to_owned())
        }

        fn subject(&self, hash: &str) -> Option<String> {
            self.commit_exists(hash).then(|| "a commit".to_owned())
        }
    }

    const STATUS: &str = "# Status\n\nSTATE: milestone=M1 phase=P task=P6 state=idle base=abc1234 updated=2026-11-10\n";
    const BACKLOG: &str = "## Now\n\n\
- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n\
- `[ ]` **S14** the banding\n  `files: src/b.rs`\n\n\
## Bugs\n\n\
## Next\n\n\
## Done\n";
    const NOTE: &str = "task: P6\ngoal: the writer\n\nsteps:\n  [x] the layout\n  [ ] the writer\n\n\
now: the writer\nblocked: nothing\nrun: cargo test\nresume: the writer\n";

    /// A backlog that declares its phases the canonical way and holds one task of each.
    const LEGEND: &str = "# Backlog\n\nIntro prose.\n\n\
Phases come from the shape-base plan; `cargo xtask task phase` owns this list:\n\
`P` — the plan and `xtask` itself\n\
`S` — a structural refactor that changes module boundaries only\n\n\
## Now\n\n\
- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n\
- `[ ]` **S14** the banding\n  `files: src/b.rs`\n\n\
## Bugs\n\n\
## Next\n\n\
## Done\n";

    #[test]
    fn a_phase_is_declared_and_then_updated_by_one_verb() {
        let root = project("phase", STATUS, LEGEND, None);
        run(&root, &["phase", "K", "--name", "the order package"]).expect("K is declared");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("`K` — the order package\n"), "the clause added: {backlog}");
        assert!(backlog.contains("`P` — the plan and `xtask` itself\n"), "the others stayed: {backlog}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");

        run(&root, &["phase", "K", "--name", "the order package and its money"]).expect("K is updated");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("`K` — the order package and its money\n"), "{backlog}");
        assert!(!backlog.contains("`K` — the order package\n"), "the old clause is gone: {backlog}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn several_phases_travel_in_one_write() {
        let root = project("phase-many", STATUS, LEGEND, None);
        run(&root, &["phase", "--declare", "K=the order package", "--declare", "R=the pre-release rename"])
            .expect("both letters are declared");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("`K` — the order package\n"), "{backlog}");
        assert!(backlog.contains("`R` — the pre-release rename\n"), "{backlog}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_partial_legend_that_drops_a_used_phase_is_refused() {
        // The prose legend declares P and K; rewriting it with P alone would leave task K4 unexplained.
        let prose = LEGEND
            .replace(
                "`P` — the plan and `xtask` itself\n`S` — a structural refactor that changes module boundaries only",
                "P is the plan, K is the order package",
            )
            .replace("**S14** the banding", "**K4** the package");
        let root = project("phase-drop", STATUS, &prose, None);
        let before = std::fs::read_to_string(root.join("plan/BACKLOG.md")).expect("the backlog is readable");
        let error = run(&root, &["phase", "P", "--name", "the plan"]).expect_err("K would lose its clause");
        assert!(error.contains("nothing was written"), "{error}");
        // The sentence itself belongs to the gate, so it is asserted there: this verb only refuses.
        assert!(
            crate::check_with(&root, &FakeGit).is_err(),
            "the fixture's prose legend is what the gate reports, and the rewrite would have left K undeclared"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("plan/BACKLOG.md")).expect("readable"),
            before,
            "a refusal writes nothing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_task_in_an_undeclared_phase_is_refused_unless_it_names_one() {
        let root = project("phase-add", STATUS, LEGEND, None);
        let error =
            run(&root, &["add", "--phase", "K", "--files", "src/a.rs", "the package"]).expect_err("K is not declared");
        assert!(error.contains("task phase K"), "the sentence names the verb: {error}");

        run(&root, &["add", "--phase", "K", "--phase-name", "the order package", "--files", "src/a.rs", "the package"])
            .expect("the declaration travels with the task");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("`K` — the order package\n"), "{backlog}");
        assert!(backlog.contains("**K1** the package"), "the task landed in its phase: {backlog}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_phase_is_one_capital_letter() {
        let root = project("phase-letter", STATUS, LEGEND, None);
        let error = run(&root, &["phase", "KK", "--name", "two letters"]).expect_err("two letters is no phase");
        assert!(error.contains("one capital letter"), "{error}");
        // A lowercase letter is read as the capital it names, the same way `add --phase` reads one.
        run(&root, &["phase", "k", "--name", "lowercase but read as K"]).expect("a lowercase letter names its phase");
        assert!(text(&root, "plan/BACKLOG.md").contains("`K` — lowercase but read as K\n"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A scratch project: two plan files, two source files, a note only when the test needs one.
    fn project(name: &str, status: &str, backlog: &str, session: Option<&str>) -> PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-write-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("plan/notes")).expect("the scratch plan is writable");
        std::fs::create_dir_all(root.join("src")).expect("the scratch sources are writable");
        std::fs::write(root.join("plan/STATUS.md"), status).expect("STATUS is writable");
        std::fs::write(root.join("plan/BACKLOG.md"), backlog).expect("BACKLOG is writable");
        std::fs::write(root.join("src/a.rs"), "fn a() {}\n").expect("a.rs is writable");
        std::fs::write(root.join("src/b.rs"), "fn b() {}\n").expect("b.rs is writable");
        if let Some(session) = session {
            std::fs::write(root.join("plan/SESSION.md"), session).expect("SESSION is writable");
        }
        root
    }

    fn run(root: &Path, args: &[&str]) -> Result<(), String> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        run_with(root, &args, &FakeGit)
    }

    fn text(root: &Path, path: &str) -> String {
        std::fs::read_to_string(root.join(path)).unwrap_or_default()
    }

    /// Every file under `plan/`, so a refusal can be checked byte for byte.
    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = std::fs::read_dir(root.join("plan"))
            .expect("the plan directory is readable")
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    text(root, &format!("plan/{}", entry.file_name().to_string_lossy())),
                )
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn a_repair_passes_only_a_write_that_shrinks_the_violations() {
        let stale = |left: &str, right: &str| format!("before {left} after {right}");
        let (before, after) = (vec![stale("a", "b")], vec![stale("a", "c")]);
        assert!(!shrinks(&before, &after), "a trade is not a repair");
        assert!(!shrinks(&before, &before), "a write that fixes nothing is not a repair");
        let two = vec![stale("a", "b"), stale("c", "d")];
        assert!(shrinks(&two, &[stale("c", "d")]), "one violation gone, none new");
        assert!(!shrinks(&two, &[stale("c", "d"), stale("e", "f")]), "one gone, one new is a trade");
        assert!(shrinks(&two, &Vec::new()), "an empty plan is a strict subset");
    }

    #[test]
    fn a_verb_repairs_a_plan_with_several_violations_only_when_told_to() {
        // Two hints name files that are gone: one verb can fix one of them.
        let backlog = BACKLOG.replace("src/a.rs", "src/gone.rs").replace("src/b.rs", "src/also_gone.rs");
        let root = project("repair", STATUS, &backlog, None);
        let before = snapshot(&root);
        let error =
            run(&root, &["set", "P6", "--field", "files", "--files", "src/a.rs"]).expect_err("two violations stand");
        assert!(error.contains("refused"), "{error}");
        assert_eq!(snapshot(&root), before, "a refused repair writes nothing");

        run(&root, &["set", "P6", "--field", "files", "--files", "src/a.rs", "--repair"])
            .expect("one violation goes, none comes");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("`files: src/a.rs`"), "the hint is fixed: {backlog}");
        let remaining = crate::check_with(&root, &FakeGit).expect_err("the other violation stands");
        assert_eq!(remaining.len(), 1, "{remaining:?}");
        assert!(remaining[0].contains("also_gone.rs"), "{remaining:?}");

        // The last one is repairable the same way, and then the plan is valid again.
        run(&root, &["set", "S14", "--field", "files", "--files", "src/b.rs", "--repair"])
            .expect("the second violation goes");
        crate::check_with(&root, &FakeGit).expect("the plan is whole again");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn starting_a_task_marks_it_and_writes_the_note() {
        let root = project("start", STATUS, BACKLOG, None);
        run(&root, &["start", "P6"]).expect("P6 starts");
        assert!(text(&root, "plan/BACKLOG.md").contains("- `[~]` **P6** the writer"), "the marker moved");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=P6 state=doing"), "{status}");
        let session = text(&root, "plan/SESSION.md");
        assert!(session.contains("task: P6") && session.contains("now: "), "{session}");
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_start_while_another_task_runs_is_refused_and_writes_nothing() {
        let running = STATUS.replace("state=idle", "state=doing");
        let backlog = BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("start-refused", &running, &backlog, Some(NOTE));
        let before = snapshot(&root);
        let error = run(&root, &["start", "S14"]).expect_err("P6 is still in progress");
        assert!(error.contains("still in progress"), "{error}");
        assert_eq!(snapshot(&root), before, "a refused verb writes nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_close_lands_the_commit_moves_the_entry_and_drops_the_note() {
        let running = STATUS.replace("state=idle", "state=doing");
        let backlog = BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("close", &running, &backlog, Some(NOTE));
        run(&root, &["close", "P6"]).expect("P6 closes");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("- `[x]` **P6** the writer"), "{backlog}");
        assert!(backlog.contains("abc1234"), "the commit is recorded: {backlog}");
        let done = backlog.find("## Done").expect("Done is there");
        assert!(backlog[done..].contains("**P6**"), "the entry moved to Done: {backlog}");
        assert!(!backlog[..done].contains("**P6**"), "and left Now: {backlog}");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=S14 state=idle"), "{status}");
        assert!(status.contains("base=abc1234"), "the plan describes the checkout: {status}");
        assert!(!root.join("plan/SESSION.md").exists(), "the note is gone");
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_task_takes_the_next_id_and_joins_the_end_of_the_order() {
        let root = project("add", STATUS, BACKLOG, None);
        run(&root, &["add", "--phase", "S", "--files", "src/b.rs", "--why", "1446 lines", "the pipelines move"])
            .expect("a task is added");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("- `[ ]` **S15** the pipelines move"), "{backlog}");
        assert!(backlog.contains("  Why: 1446 lines"), "{backlog}");
        let added = backlog.find("**S15**").expect("S15 is there");
        let last = backlog.find("**S14**").expect("S14 is there");
        assert!(added > last, "the new task joins the end: {backlog}");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=P6"), "the current task does not change: {status}");
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_field_and_a_files_hint_are_set_in_place() {
        let root = project("set", STATUS, BACKLOG, None);
        run(&root, &["set", "P6", "--field", "Done", "--body", "a test per refusal"]).expect("the field is set");
        run(&root, &["set", "P6", "--field", "files", "--files", "src/a.rs, src/b.rs"]).expect("the hint is set");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("  Done: a test per refusal"), "{backlog}");
        assert!(backlog.contains("  `files: src/a.rs, src/b.rs`"), "{backlog}");
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_verb_on_an_unknown_task_is_refused_and_writes_nothing() {
        let root = project("unknown", STATUS, BACKLOG, None);
        let before = snapshot(&root);
        for verb in
            [&["start", "S99"][..], &["set", "S99", "--field", "Why", "--body", "x"], &["note", "S99", "--body", "x"]]
        {
            let error = run(&root, verb).expect_err("S99 does not exist");
            assert!(error.contains("no task `S99`") || error.contains("has no task"), "{verb:?}: {error}");
            assert_eq!(snapshot(&root), before, "{verb:?} writes nothing");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_note_is_created_with_a_heading_and_appended_to() {
        let root = project("note", STATUS, BACKLOG, None);
        run(&root, &["note", "P6", "--body", "the layout scanner\n\nit walks the lines"]).expect("the note is added");
        let note = text(&root, "plan/notes/P6.md");
        assert!(note.starts_with("# P6 — the layout scanner\n"), "{note}");
        run(&root, &["note", "P6", "--body", "a second thought"]).expect("the note is appended");
        let note = text(&root, "plan/notes/P6.md");
        assert!(note.contains("a second thought"), "{note}");
        assert_eq!(note.matches("# P6").count(), 1, "the heading is written once: {note}");
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reopen_returns_the_task_to_the_top_of_now() {
        let done = BACKLOG.replace("- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n", "");
        let done = format!("{done}\n- `[x]` **P6** the writer\n  `files: src/a.rs` — `abc1234`\n");
        let status = STATUS.replace("task=P6", "task=S14");
        let root = project("reopen", &status, &done, None);
        run(&root, &["reopen", "P6"]).expect("P6 reopens");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(backlog.contains("- `[ ]` **P6** the writer\n"), "{backlog}");
        assert!(!backlog.contains("abc1234"), "the commit is gone: {backlog}");
        assert!(text(&root, "plan/STATUS.md").contains("task=P6 state=idle"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A backlog with a bug, a next-order entry and the current task.
    const FILED_BACKLOG: &str = "## Now\n\n\
- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n\
## Bugs\n\n\
- `[ ]` **X2** a defect\n  Why: it breaks on 1 µm\n  `files: src/b.rs`\n\n\
## Next\n\n\
- `[ ]` **R1** the rename\n  Why: the name is in three places\n  `files: src/a.rs`\n\n\
## Done\n";

    #[test]
    fn taking_a_bug_moves_the_entry_whole() {
        let root = project("take-bug", STATUS, FILED_BACKLOG, None);
        run(&root, &["take", "X2"]).expect("the bug joins the end of Now");
        let backlog = text(&root, "plan/BACKLOG.md");
        assert!(
            backlog.contains("- `[ ]` **X2** a defect\n  Why: it breaks on 1 µm\n  `files: src/b.rs`"),
            "the entry keeps its text, fields and hint: {backlog}"
        );
        let now = backlog.find("## Now").expect("Now is there");
        let bugs = backlog.find("## Bugs").expect("Bugs is there");
        let order = &backlog[now..bugs];
        let (current, taken) = (order.find("**P6**").expect("P6 stays"), order.find("**X2**").expect("X2 arrived"));
        assert!(current < taken, "the taken entry sits behind the current one: {order}");
        assert!(!backlog[bugs..].contains("**X2**"), "and left `## Bugs`: {backlog}");
        assert!(
            text(&root, "plan/STATUS.md").contains("task=P6 state=idle"),
            "an occupied `## Now` keeps its `STATE:`"
        );
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn taking_into_an_empty_now_makes_the_task_current() {
        // The state a plan is left in when `## Now` runs out: no valid current task until an entry
        // is taken into the empty order, which is the write that repairs it.
        let status = format!("{}\n## Next\n\nP6 is next: the writer.\n", STATUS.replace("task=P6", "task=R1"));
        let backlog = FILED_BACKLOG.replace("- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n", "");
        let root = project("take-empty", &status, &backlog, None);
        assert!(crate::check_with(&root, &FakeGit).is_err(), "the plan starts with no valid current task");
        run(&root, &["take", "R1"]).expect("the take fills the empty order and repairs the plan");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=R1 state=idle"), "{status}");
        assert!(status.contains("R1 is next: the rename"), "the paragraph is rewritten: {status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn taking_a_done_or_unknown_task_is_refused() {
        let backlog =
            FILED_BACKLOG.replace("## Done\n", "## Done\n\n- `[x]` **X1** older\n  `files: src/a.rs` — `abc1234`\n");
        let root = project("take-refused", STATUS, &backlog, None);
        let before = snapshot(&root);
        let error = run(&root, &["take", "X1"]).expect_err("a done task is not taken");
        assert!(error.contains("is done"), "{error}");
        let error = run(&root, &["take", "X9"]).expect_err("an unknown id is refused");
        assert!(error.contains("no task `X9`"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn taking_at_the_top_while_a_task_runs_is_refused() {
        let running = STATUS.replace("state=idle", "state=doing");
        let backlog = FILED_BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("take-top", &running, &backlog, Some(NOTE));
        let before = snapshot(&root);
        let error = run(&root, &["take", "X2", "--at", "top"]).expect_err("the top belongs to the running task");
        assert!(error.contains("still in progress"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");

        run(&root, &["take", "X2"]).expect("the end of `## Now` does not disturb the running task");
        assert!(text(&root, "plan/STATUS.md").contains("task=P6 state=doing"));
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A status with the `## Next` paragraph the gate wants when `STATE:` moves.
    fn with_next(status: &str) -> String {
        format!("{status}\n## Next\n\nP6 is next: the writer.\n")
    }

    #[test]
    fn deferring_the_current_task_points_the_state_at_the_next_one() {
        let root = project("defer-head", &with_next(STATUS), BACKLOG, None);
        run(&root, &["defer", "P6"]).expect("P6 leaves the order");
        let backlog = text(&root, "plan/BACKLOG.md");
        let now = backlog.find("## Now").expect("Now is there");
        let next = backlog.find("## Next").expect("Next is there");
        assert!(!backlog[now..next].contains("**P6**"), "P6 left the order: {backlog}");
        assert!(
            backlog[next..].contains("- `[ ]` **P6** the writer\n  `files: src/a.rs`"),
            "the entry moves whole, fields and hint included: {backlog}"
        );
        assert!(backlog[now..next].contains("**S14**"), "the rest of the order is untouched: {backlog}");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=S14 state=idle"), "{status}");
        assert!(status.contains("S14 is next: the banding"), "the paragraph names the new head: {status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deferring_a_later_task_leaves_the_state_alone() {
        let root = project("defer-later", &with_next(STATUS), BACKLOG, None);
        run(&root, &["defer", "S14"]).expect("S14 leaves the order");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=P6 state=idle"), "the head did not move: {status}");
        assert!(status.contains("P6 is next: the writer"), "the paragraph still names it: {status}");
        assert!(!text(&root, "plan/BACKLOG.md").contains("**S14** the banding\n  `files: src/b.rs`\n\n## Bugs"));
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_deferred_task_keeps_its_note_and_stops_being_scheduled() {
        let done = format!("{STATUS}\n## Next\n\nP6 is next: the writer.\n");
        let root = project("defer-note", &done, FILED_BACKLOG, None);
        run(&root, &["note", "X2", "--body", "a measured defect"]).expect("the bug has a note");
        run(&root, &["defer", "X2"]).expect_err("X2 is filed under `## Bugs`, not the order");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deferring_a_running_or_unknown_task_is_refused() {
        let running = with_next(&STATUS.replace("state=idle", "state=doing"));
        let backlog = BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("defer-running", &running, &backlog, Some(NOTE));
        let before = snapshot(&root);
        let error = run(&root, &["defer", "P6"]).expect_err("a running task keeps its session");
        assert!(error.contains("in progress"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");

        let status = with_next(STATUS);
        let root = project("defer-unknown", &status, BACKLOG, None);
        let before = snapshot(&root);
        let error = run(&root, &["defer", "P9"]).expect_err("P9 does not exist");
        assert!(error.contains("no task `P9`"), "{error}");
        assert_eq!(snapshot(&root), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_order_cannot_be_emptied_by_deferring_its_last_task() {
        let backlog = "## Now\n\n- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n## Bugs\n\n## Next\n\n## Done\n";
        let root = project("defer-last", &with_next(STATUS), backlog, None);
        let before = snapshot(&root);
        let error = run(&root, &["defer", "P6"]).expect_err("the order must keep a head");
        assert!(error.contains("nothing to work on"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_current_task_cannot_be_dropped() {
        let root = project("drop-current", STATUS, BACKLOG, None);
        let before = snapshot(&root);
        let error = run(&root, &["drop", "P6"]).expect_err("the current task stays");
        assert!(error.contains("current task"), "{error}");
        assert_eq!(snapshot(&root), before);
        run(&root, &["drop", "S14"]).expect("S14 is not current");
        assert!(!text(&root, "plan/BACKLOG.md").contains("S14"), "the entry is gone");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_start_on_a_running_task_does_not_rewrite_its_note() {
        let running = STATUS.replace("state=idle", "state=doing");
        let backlog = BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("start-again", &running, &backlog, Some(NOTE));
        let before = snapshot(&root);
        let error = run(&root, &["start", "P6"]).expect_err("a running task is not restarted");
        assert!(error.contains("already in progress") && error.contains("--force"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");
        run(&root, &["start", "P6", "--force"]).expect("--force rewrites the note");
        assert!(text(&root, "plan/SESSION.md").contains("now: "), "the note is rebuilt");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn closing_a_done_task_and_reopening_an_open_one_are_refused() {
        let done = STATUS.replace("task=P6", "task=S14");
        let backlog = BACKLOG.replace("- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n", "");
        let backlog = format!("{backlog}\n- `[x]` **P6** the writer\n  `files: src/a.rs` — `abc1234`\n");
        let root = project("transitions", &done, &backlog, None);
        let before = snapshot(&root);
        let error = run(&root, &["close", "P6"]).expect_err("P6 is already done");
        assert!(error.contains("already done"), "{error}");
        assert_eq!(snapshot(&root), before);

        let error = run(&root, &["reopen", "S14"]).expect_err("S14 is not done");
        assert!(error.contains("is not done"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reopen_that_moves_the_current_task_names_the_next_paragraph() {
        let done = BACKLOG.replace("- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n", "");
        let done = format!("{done}\n- `[x]` **P6** the writer\n  `files: src/a.rs` — `abc1234`\n");
        let status = format!("{}\n## Next\n\nS14 is next: the banding.\n", STATUS.replace("task=P6", "task=S14"));
        let root = project("reopen-next", &status, &done, None);
        let before = snapshot(&root);
        let error = run(&root, &["reopen", "P6"]).expect_err("the paragraph names another task");
        assert!(error.contains("--next") && error.contains("P6"), "{error}");
        assert_eq!(snapshot(&root), before, "a refusal writes nothing");

        run(&root, &["reopen", "P6", "--next", "P6 is back: the writer."]).expect("both atoms in one write");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=P6 state=idle"), "{status}");
        let next = status.split("## Next").nth(1).unwrap_or_default();
        assert!(next.contains("P6 is back"), "the paragraph moved: {status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_task_added_at_the_top_becomes_the_current_one() {
        let status = format!("{}\n## Next\n\nP6 is next: the writer.\n", STATUS);
        let root = project("add-top", &status, BACKLOG, None);
        run(&root, &["add", "--phase", "X", "--at", "top", "--files", "src/b.rs", "an interjection"])
            .expect("the task joins the head of the order");
        let backlog = text(&root, "plan/BACKLOG.md");
        let added = backlog.find("**X1**").expect("X1 is there");
        let first = backlog.find("**P6**").expect("P6 is there");
        assert!(added < first, "the new task is first in `## Now`: {backlog}");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=X1 state=idle"), "the plan points at it: {status}");
        assert!(status.contains("X1 is next"), "the paragraph follows: {status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");

        // A task that cannot be the current one may not take the top of a section.
        let error =
            run(&root, &["add", "--phase", "X", "--at", "top", "--section", "Bugs", "--files", "src/b.rs", "a defect"])
                .expect_err("only `## Now` orders the work");
        assert!(error.contains("`Now`"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_next_paragraph_moves_with_the_task_it_names() {
        // An interrupted session: `STATE:` still names P6, which already landed, and the paragraph
        // has to move to S14 with it — one write, or the gate refuses either half.
        let done = BACKLOG.replace("- `[ ]` **P6** the writer\n  `files: src/a.rs`\n\n", "");
        let done = format!("{done}\n- `[x]` **P6** the writer\n  `files: src/a.rs` — `abc1234`\n");
        let status = format!("{}\n## Next\n\nP6 is next: the writer.\n", STATUS);
        let root = project("state-next", &status, &done, None);
        let before = snapshot(&root);
        let error =
            state(&root, &["next".to_owned(), "--body".to_owned(), "S14 is next: the banding.".to_owned()], &FakeGit)
                .expect_err("the paragraph must name the current task");
        assert!(error.contains("refused"), "{error}");
        assert_eq!(snapshot(&root), before);

        state(
            &root,
            &[
                "next".to_owned(),
                "--body".to_owned(),
                "S14 is next: the banding.".to_owned(),
                "--task".to_owned(),
                "S14".to_owned(),
            ],
            &FakeGit,
        )
        .expect("the paragraph and the current task move together");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("task=S14"), "{status}");
        assert!(status.contains("S14 is next"), "{status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_where_we_are_prose_is_written_by_a_command() {
        let status = format!("{}\n## Where we are\n\nthe old tree.\n\n## Next\n\nP6 is next.\n", STATUS);
        let root = project("state-where", &status, BACKLOG, None);
        state(&root, &["where".to_owned(), "--body".to_owned(), "the tree at abc1234.".to_owned()], &FakeGit)
            .expect("the prose is replaced");
        let status = text(&root, "plan/STATUS.md");
        assert!(status.contains("the tree at abc1234."), "{status}");
        assert!(!status.contains("the old tree."), "{status}");
        crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reference_file_is_written_through_the_gate() {
        let root = project("file", STATUS, BACKLOG, None);
        file(
            &root,
            &["plan/map.md".to_owned(), "--body".to_owned(), "# Map\n\n| Feature | Where |\n|---|---|\n".to_owned()],
            &FakeGit,
        )
        .expect("a reference file is written");
        assert!(text(&root, "plan/map.md").contains("| Feature |"), "the file landed");

        let error = file(&root, &["plan/BACKLOG.md".to_owned(), "--body".to_owned(), "gone\n".to_owned()], &FakeGit)
            .expect_err("the task verbs own the backlog");
        assert!(error.contains("not a reference file"), "{error}");
        assert!(text(&root, "plan/BACKLOG.md").contains("**P6**"), "the backlog is untouched");

        let error = file(&root, &["plan/debt.md".to_owned(), "--body".to_owned(), "   \n".to_owned()], &FakeGit)
            .expect_err("an empty file says nothing");
        assert!(error.contains("would be empty"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_step_without_a_session_note_is_refused() {
        let root = project("step-none", STATUS, BACKLOG, None);
        let before = snapshot(&root);
        let error = run(&root, &["step", "P6", "--tick"]).expect_err("there is no note");
        assert!(error.contains("cannot read `plan/SESSION.md`"), "{error}");
        assert_eq!(snapshot(&root), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dry_run_reports_and_writes_nothing() {
        let root = project("dry", STATUS, BACKLOG, None);
        let before = snapshot(&root);
        run(&root, &["start", "P6", "--dry-run"]).expect("the dry run succeeds");
        assert_eq!(snapshot(&root), before, "--dry-run writes nothing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_state_field_is_set_one_at_a_time() {
        let root = project("state", STATUS, BACKLOG, None);
        state(&root, &["set".to_owned(), "phase=A".to_owned()], &FakeGit).expect("phase moves to A");
        assert!(text(&root, "plan/STATUS.md").contains("phase=A"));
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_step_is_ticked_and_added() {
        let running = STATUS.replace("state=idle", "state=doing");
        let backlog = BACKLOG.replace("- `[ ]` **P6**", "- `[~]` **P6**");
        let root = project("step", &running, &backlog, Some(NOTE));
        run(&root, &["step", "P6", "--tick"]).expect("the step is ticked");
        assert!(text(&root, "plan/SESSION.md").contains("  [x] the writer"));
        run(&root, &["step", "P6", "--add", "the verbs"]).expect("a step is added");
        assert!(text(&root, "plan/SESSION.md").contains("  [ ] the verbs"));
        crate::check_with(&root, &FakeGit).expect("the plan is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_drift_the_gate_would_reject_is_refused_before_it_lands() {
        // A task without a `files:` hint cannot be added: the gate rejects it, so the verb refuses.
        let root = project("refuse-gate", STATUS, BACKLOG, None);
        let before = snapshot(&root);
        let error = run(&root, &["add", "--phase", "S", "--why", "x", "a task with no hint"])
            .expect_err("the gate requires a hint");
        assert!(error.contains("--files is required"), "{error}");
        assert_eq!(snapshot(&root), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_task_status_is_read_back_as_the_gate_sees_it() {
        let root = project("status", STATUS, BACKLOG, None);
        run(&root, &["start", "P6"]).expect("P6 starts");
        let summary = crate::check_with(&root, &FakeGit).expect("the plan is valid");
        let task = summary.tasks.iter().find(|task| task.id == "P6").expect("P6 is in the backlog");
        assert_eq!(task.status, TaskStatus::Doing);
        let _ = std::fs::remove_dir_all(&root);
    }
}
