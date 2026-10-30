//! The plan gate: `plan/STATUS.md`, `plan/BACKLOG.md` and `plan/SESSION.md` must agree.
//!
//! A session starts by reading `STATE` and the `## Now` section of the backlog, so a stale or
//! contradictory plan file is a real defect: it sends the next session to the wrong task. The checks
//! are cheap string parsing plus a few filesystem and git lookups. Anything inconsistent fails the
//! gate, including a file over its line budget: a warning would be ignored, an error is not.

use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// Longest accepted `plan/STATUS.md`, in lines.
const MAX_STATUS_LINES: usize = 70;
/// Longest accepted `plan/SESSION.md`, in lines.
const MAX_SESSION_LINES: usize = 50;
/// The marker that starts the state line.
const STATE_PREFIX: &str = "STATE:";
/// The section a session starts from.
const NOW_SECTION: &str = "Now";
/// The section that holds finished tasks.
const DONE_SECTION: &str = "Done";

/// What the plan files say right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The `STATE:` fields.
    pub state: State,
    /// Backlog tasks, in file order.
    pub tasks: Vec<Task>,
}

/// The parsed `STATE:` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Milestone, for example `M7`.
    pub milestone: String,
    /// Shape-base phase, when the milestone has one.
    pub phase: Option<String>,
    /// The task id that is in progress, for example `A3`.
    pub task: String,
    /// `idle` or `doing`.
    pub status: String,
    /// The commit the plan describes; `HEAD` while the plan edit is itself uncommitted.
    pub base: String,
    /// The last update date.
    pub updated: String,
}

/// One backlog line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Stable id, for example `A3`.
    pub id: String,
    /// `open`, `doing` or `done`.
    pub status: TaskStatus,
    /// The `## ` section the line sits in, without the hashes: `Now`, `Next`, `Done`.
    pub section: String,
    /// The one-line text.
    pub text: String,
    /// The `files:` hint, when the line has one.
    pub files: Option<String>,
}

/// The state of one backlog task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    /// `[ ]`
    Open,
    /// `[~]`
    Doing,
    /// `[x]`
    Done,
}

impl TaskStatus {
    /// The marker as it appears in the file.
    const fn marker(self) -> &'static str {
        match self {
            Self::Open => "[ ]",
            Self::Doing => "[~]",
            Self::Done => "[x]",
        }
    }
}

/// Reads the real plan files and checks them.
pub fn check(workspace: &Path) -> Result<Summary, Vec<String>> {
    let status = read(&workspace.join("plan/STATUS.md"), "plan/STATUS.md")?;
    let backlog = read(&workspace.join("plan/BACKLOG.md"), "plan/BACKLOG.md")?;
    let session = workspace.join("plan/SESSION.md");
    let session_text = match std::fs::read_to_string(&session) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(vec![format!("cannot read `plan/SESSION.md`: {error}")]),
    };

    let mut violations = Vec::new();
    let summary = match analyze(&status, &backlog, session_text.as_deref()) {
        Ok(summary) => Some(summary),
        Err(errors) => {
            violations.extend(errors);
            None
        }
    };
    if let Some(summary) = &summary {
        violations.extend(workspace_violations(workspace, summary));
    }
    match summary {
        Some(summary) if violations.is_empty() => Ok(summary),
        _ => Err(violations),
    }
}

fn read(path: &Path, name: &str) -> Result<String, Vec<String>> {
    std::fs::read_to_string(path).map_err(|error| vec![format!("cannot read `{name}`: {error}")])
}

/// Checks what only the filesystem and git can answer: the hinted paths and the `base` commit.
fn workspace_violations(workspace: &Path, summary: &Summary) -> Vec<String> {
    let mut violations = Vec::new();

    for task in summary.tasks.iter().filter(|task| task.status != TaskStatus::Done) {
        let Some(hint) = &task.files else { continue };
        for path in missing_paths(hint, |path| workspace.join(path).exists()) {
            violations
                .push(format!("backlog task {} hints at `{path}` in `files:`, but that path does not exist", task.id));
        }
    }

    for task in summary.tasks.iter().filter(|task| task.status == TaskStatus::Done) {
        let Some(hash) = task.text.rsplit('`').nth(1) else {
            violations.push(format!("backlog task {} is done without a commit hash", task.id));
            continue;
        };
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            violations.push(format!("backlog task {} names `{hash}` as its commit, which is not a hash", task.id));
        } else if !commit_exists(workspace, hash) {
            violations.push(format!("backlog task {} names commit `{hash}`, which is not in this repository", task.id));
        }
    }

    let head = head(workspace);
    let base = summary.state.base.as_str();
    let exists = commit_exists(workspace, base);
    let ancestor = head.as_deref().is_some_and(|head| is_ancestor(workspace, base, head));
    if let Some(violation) = base_violation(base, head.as_deref(), exists, ancestor) {
        violations.push(violation);
    }

    violations
}

/// The `base=` violation for the facts gathered from git.
///
/// `HEAD` is a legal `base`: the plan edit is committed on top of it (AGENTS §6.1).
fn base_violation(base: &str, head: Option<&str>, exists: bool, is_ancestor: bool) -> Option<String> {
    if !exists {
        return Some(format!("`STATE: base={base}` is not a commit in this repository"));
    }
    let Some(head) = head else {
        return Some("cannot resolve `HEAD`: `cargo xtask plan` needs a git checkout".to_owned());
    };
    (!is_ancestor).then(|| {
        format!(
            "`STATE: base={base}` is not an ancestor of `HEAD` ({head}); point `base` at a commit the plan describes"
        )
    })
}

/// Checks the three plan files against each other, without touching the filesystem or git.
pub fn analyze(status: &str, backlog: &str, session: Option<&str>) -> Result<Summary, Vec<String>> {
    let state = parse_state(status)?;
    let tasks = parse_backlog(backlog);
    let mut violations = parse_violations(status, backlog, &tasks);
    violations.extend(budget_violations(status, session));
    violations.extend(unparsed_task_lines(backlog));

    let current = tasks.iter().find(|task| task.id == state.task);
    match current {
        Some(task) if task.status == TaskStatus::Done => {
            violations.push(format!("backlog task {} is done, but `STATE: task={}` names it", state.task, state.task));
        }
        Some(task) => {
            if task.section != NOW_SECTION {
                violations.push(format!(
                    "`STATE: task={}` sits in `## {}`; the current task must be in `## {NOW_SECTION}`",
                    state.task, task.section
                ));
            }
        }
        None => violations.push(format!("`STATE: task={}` is not a task in `plan/BACKLOG.md`", state.task)),
    }

    let doing: Vec<&Task> = tasks.iter().filter(|task| task.status == TaskStatus::Doing).collect();
    match state.status.as_str() {
        "doing" => {
            if doing.len() != 1 {
                violations.push(format!(
                    "`STATE: state=doing` needs exactly one `[~]` task, the backlog has {}",
                    doing.len()
                ));
            }
            match doing.first() {
                Some(task) if task.id != state.task => violations
                    .push(format!("`STATE: task={}` does not match the `[~]` backlog task {}", state.task, task.id)),
                Some(task) if task.section != NOW_SECTION => violations
                    .push(format!("`[~]` task {} must sit in `## {NOW_SECTION}`, not `## {}`", task.id, task.section)),
                _ => {}
            }
            if session.is_none() {
                violations
                    .push("`STATE: state=doing` needs a `plan/SESSION.md`; copy `plan/SESSION.template.md`".into());
            }
            if let Some(session) = session {
                match session_field(session, "task") {
                    Some(task) if task == state.task => {}
                    Some(task) => violations.push(format!(
                        "`plan/SESSION.md` is about task {task}, but `STATE: task={}` names another",
                        state.task
                    )),
                    None => violations.push("`plan/SESSION.md` needs a `task: <id>` line".into()),
                }
                for key in ["goal", "now", "run"] {
                    if session_field(session, key).is_none() {
                        violations.push(format!("`plan/SESSION.md` needs a non-empty `{key}:` line"));
                    }
                }
            }
        }
        "idle" => {
            if !doing.is_empty() {
                let ids: Vec<&str> = doing.iter().map(|task| task.id.as_str()).collect();
                violations.push(format!(
                    "`STATE: state=idle` but the backlog marks {} still in progress; set `[~]` back to `[ ]` \
                     or start the task",
                    ids.join(", ")
                ));
            }
            if session.is_some() {
                violations.push("`STATE: state=idle` must not leave a `plan/SESSION.md` behind".into());
            }
        }
        other => violations.push(format!("`STATE: state={other}` must be `idle` or `doing`")),
    }

    if violations.is_empty() { Ok(Summary { state, tasks }) } else { Err(violations) }
}

/// A plan file over its line budget is an error: an ignored warning is how the plan rots.
fn budget_violations(status: &str, session: Option<&str>) -> Vec<String> {
    let mut violations = Vec::new();
    for (name, text, max) in
        [("plan/STATUS.md", status, MAX_STATUS_LINES), ("plan/SESSION.md", session.unwrap_or(""), MAX_SESSION_LINES)]
    {
        let lines = text.lines().count();
        if lines > max {
            violations.push(format!("{name} has {lines} lines, over the {max}-line budget"));
        }
    }
    violations
}

/// Task-looking lines that did not parse, so a malformed line cannot silently drop a task.
fn unparsed_task_lines(backlog: &str) -> Vec<String> {
    backlog_entries(backlog)
        .into_iter()
        .filter(|entry| parse_task_line(&entry.text).is_none())
        .map(|entry| format!("`plan/BACKLOG.md` has a task line without a `**bold**` id: `{}`", entry.text))
        .collect()
}

/// Parses the `STATE:` line and its required fields.
fn parse_state(status: &str) -> Result<State, Vec<String>> {
    let Some(line) = status.lines().find(|line| line.trim_start().starts_with(STATE_PREFIX)) else {
        return Err(vec![format!("`plan/STATUS.md` needs a `{STATE_PREFIX}` line")]);
    };
    let mut milestone = None;
    let mut phase = None;
    let mut task = None;
    let mut state = None;
    let mut base = None;
    let mut updated = None;
    for field in line.trim_start().trim_start_matches(STATE_PREFIX).split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "milestone" => milestone = Some(value.to_owned()),
            "phase" => phase = Some(value.to_owned()),
            "task" => task = Some(value.to_owned()),
            "state" => state = Some(value.to_owned()),
            "base" => base = Some(value.to_owned()),
            "updated" => updated = Some(value.to_owned()),
            _ => {}
        }
    }
    let mut violations = Vec::new();
    if milestone.is_none() {
        violations.push("`STATE:` needs `milestone=`".to_owned());
    }
    if task.is_none() {
        violations.push("`STATE:` needs `task=`".to_owned());
    }
    if state.is_none() {
        violations.push("`STATE:` needs `state=`".to_owned());
    }
    if base.is_none() {
        violations.push("`STATE:` needs `base=`".to_owned());
    }
    if updated.is_none() {
        violations.push("`STATE:` needs `updated=`".to_owned());
    }
    let (Some(milestone), Some(task), Some(state), Some(base), Some(updated)) = (milestone, task, state, base, updated)
    else {
        return Err(violations);
    };
    Ok(State { milestone, phase, task, status: state, base, updated })
}

/// Parses every backlog line that starts with a task marker.
///
/// The marker may be wrapped in backticks for rendering; both forms are accepted. A task may wrap
/// onto indented lines (markdown's way of breaking a long bullet): the entry is joined first, so a
/// `files:` hint on the second line is still the task's hint.
pub fn parse_backlog(backlog: &str) -> Vec<Task> {
    backlog_entries(backlog)
        .into_iter()
        .filter_map(|entry| {
            let task = parse_task_line(&entry.text)?;
            Some(Task { id: task.id, status: task.status, section: entry.section, text: task.text, files: task.files })
        })
        .collect()
}

/// One logical backlog entry: the section it sits in and its wrapped text joined into one line.
struct Entry {
    section: String,
    text: String,
}

/// Groups the backlog into entries, joining indented continuation lines onto their task.
fn backlog_entries(backlog: &str) -> Vec<Entry> {
    let mut section = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut open = false;
    for line in backlog.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            section = heading.trim().to_owned();
            open = false;
            continue;
        }
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        if open
            && line.starts_with([' ', '\t'])
            && let Some(entry) = entries.last_mut()
        {
            entry.text.push(' ');
            entry.text.push_str(trimmed);
            continue;
        }
        if task_candidate(line) {
            entries.push(Entry { section: section.clone(), text: trimmed.to_owned() });
            open = true;
        } else {
            open = false;
        }
    }
    entries
}

/// One parsed backlog line, before its `## ` section is attached.
struct TaskLine {
    status: TaskStatus,
    id: String,
    text: String,
    files: Option<String>,
}

/// Parses one backlog line, or `None` when it is not a well-formed task.
fn parse_task_line(line: &str) -> Option<TaskLine> {
    let line = line.trim_start().strip_prefix("- ")?;
    let (status, rest) = task_marker(line)?;
    let rest = rest.trim_start().strip_prefix('`').unwrap_or(rest.trim_start()).trim();
    let id = bold(rest)?;
    let files = files_hint(rest);
    let text = strip_files(rest);
    let text =
        text.trim_start().strip_prefix("**").and_then(|text| text.split_once("**")).map_or(text, |(_, text)| text);
    let text = text.trim_start().trim_start_matches(['—', '-', '·']).trim().to_owned();
    Some(TaskLine { status, id, text, files })
}

/// Splits the start of a task line into its status marker and the text after it.
fn task_marker(line: &str) -> Option<(TaskStatus, &str)> {
    let (marker, rest) = match line.strip_prefix('`') {
        Some(rest) => rest.split_once('`')?,
        None => (line.get(..3)?, line.get(3..)?),
    };
    let status = match marker {
        "[ ]" => TaskStatus::Open,
        "[~]" => TaskStatus::Doing,
        "[x]" => TaskStatus::Done,
        _ => return None,
    };
    Some((status, rest))
}

/// Whether a line starts a task entry: a margin bullet with a task marker.
fn task_candidate(line: &str) -> bool {
    let Some(line) = line.strip_prefix("- ") else {
        return false;
    };
    let line = line.trim_start();
    ["[ ]", "[~]", "[x]", "`[ ]`", "`[~]`", "`[x]`"].iter().any(|marker| line.starts_with(marker))
}

/// The first `**bold**` span, used for a task id.
fn bold(text: &str) -> Option<String> {
    let rest = text.strip_prefix("**")?;
    let end = rest.find("**")?;
    let id = rest.get(..end)?.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The `files:` hint at the end of a task line.
fn files_hint(text: &str) -> Option<String> {
    let (_, hint) = text.rsplit_once("files:")?;
    let hint = hint.trim().trim_end_matches('`').trim();
    (!hint.is_empty()).then(|| hint.to_owned())
}

/// The task text without its trailing `files:` hint.
fn strip_files(text: &str) -> &str {
    match text.rsplit_once("files:") {
        Some((text, _)) => text.trim_end().trim_end_matches('`').trim_end(),
        None => text,
    }
}

/// The paths of a `files:` hint that the predicate rejects.
fn missing_paths<F: Fn(&str) -> bool>(hint: &str, exists: F) -> Vec<String> {
    hint.split(',').map(clean_path).filter(|path| !path.is_empty() && !exists(path)).map(str::to_owned).collect()
}

/// Strips the prose punctuation a hint may carry before or after a path.
fn clean_path(path: &str) -> &str {
    path.trim().trim_end_matches(['`', '.', ',', ';']).trim().trim_start_matches('`').trim()
}

/// The value of a non-empty `key:` line of a session note.
fn session_field(session: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    session.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?;
        let value = value.trim().trim_matches('`').trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Report-only checks: duplicates, sections and an id that is not a plain label.
fn parse_violations(status: &str, backlog: &str, tasks: &[Task]) -> Vec<String> {
    let mut violations = Vec::new();
    if !backlog.contains("## Now") || !backlog.contains("## Done") {
        violations.push("`plan/BACKLOG.md` needs `## Now` and `## Done` sections".to_owned());
    }
    let mut seen = Vec::new();
    for task in tasks {
        if seen.contains(&task.id) {
            violations.push(format!("backlog id {} appears twice", task.id));
        } else {
            seen.push(task.id.clone());
        }
        if task.status != TaskStatus::Done && task.files.is_none() {
            violations.push(format!("backlog task {} has no `files:` hint; name where the work starts", task.id));
        }
        match (task.status, task.section.as_str()) {
            (TaskStatus::Done, DONE_SECTION) => {}
            (TaskStatus::Done, section) => violations.push(format!(
                "backlog task {} is `[x]` in `## {section}`; a finished task belongs in `## {DONE_SECTION}`",
                task.id
            )),
            (_, DONE_SECTION) => violations.push(format!(
                "`## {DONE_SECTION}` holds {} without `[x]`; move it back to `## {NOW_SECTION}` or mark it done",
                task.id
            )),
            _ => {}
        }
    }
    let mut markers = 0usize;
    let mut state_line = None;
    for line in status.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(STATE_PREFIX) {
            markers += 1;
            state_line = Some(line);
        }
    }
    if markers != 1 {
        violations.push(format!("`plan/STATUS.md` has {markers} `STATE:` lines, expected exactly one"));
    }
    if let Some(line) = state_line
        && line.trim() != line.trim_start()
    {
        violations.push("`STATE:` must start at the beginning of its line, without indentation".to_owned());
    }
    violations
}

/// Whether `hash` names a commit reachable from the repository at `workspace`.
///
/// `HEAD` is accepted as a literal because `plan/STATUS.md` carries it while the plan edit is still
/// uncommitted (AGENTS §6.1); the shell resolves it, we only need to know it is a commit.
fn commit_exists(workspace: &Path, hash: &str) -> bool {
    if hash == "HEAD" {
        return head(workspace).is_some();
    }
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    Command::new("git")
        .args(["cat-file", "-e", &format!("{hash}^{{commit}}")])
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The `HEAD` commit, when the workspace is a git checkout.
fn head(workspace: &Path) -> Option<String> {
    let output =
        Command::new("git").args(["rev-parse", "HEAD"]).current_dir(workspace).stdin(Stdio::null()).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|hash| !hash.is_empty())
}

/// Whether `ancestor` is reachable from `descendant`.
fn is_ancestor(workspace: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Prints the state, the current task and what follows, so a session start is one command.
pub fn print(summary: &Summary) {
    let state = &summary.state;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "plan: {} {} · task {} · {} · base {} · updated {}",
        state.milestone,
        state.phase.as_deref().unwrap_or("-"),
        state.task,
        state.status,
        state.base,
        state.updated
    );
    let current = summary.tasks.iter().position(|task| task.id == state.task);
    let from = current.unwrap_or(0);
    for (offset, task) in summary.tasks.iter().skip(from).take(4).enumerate() {
        let arrow = if offset == 0 { "->" } else { "  " };
        let _ = write!(text, "{arrow} {} {} · {}", task.status.marker(), task.id, task.text);
        if let Some(files) = &task.files {
            let _ = write!(text, "  [files: {files}]");
        }
        text.push('\n');
    }
    print!("{text}");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const HEADER: &str = "STATE: milestone=M7 phase=A task=A3 state=idle base=32c659f updated=2026-10-30\n";
    /// A session note a fresh session can resume from.
    const NOTE: &str = "task: A3\ngoal: edit arcs in the card\nnow: the arc arm in `object_card.rs`\nrun: cargo test -p furnicraft-ui\n";

    fn backlog() -> &'static str {
        "## Now\n\n\
         - `[ ]` **A3** arcs in the card `files: crates/ui/src/object_card.rs`\n\
         - `[ ]` **A4** stable ids `files: crates/model/src/lib.rs`\n\n\
         ## Done\n\n\
         - `[x]` **A2** ring of segments — `0833b4c`\n"
    }

    #[test]
    fn an_idle_plan_with_a_matching_task_is_consistent() {
        let summary = analyze(HEADER, backlog(), None).expect("the plan is consistent");
        assert_eq!(summary.state.task, "A3");
        assert_eq!(summary.tasks.len(), 3);
        assert_eq!(summary.tasks[0].status, TaskStatus::Open);
        assert_eq!(summary.tasks[0].id, "A3");
        assert_eq!(summary.tasks[0].section, "Now");
        assert_eq!(summary.tasks[0].files.as_deref(), Some("crates/ui/src/object_card.rs"));
        assert_eq!(summary.tasks[2].status, TaskStatus::Done);
    }

    #[test]
    fn a_doing_plan_needs_a_session_note_and_one_marked_task() {
        let doing = HEADER.replace("state=idle", "state=doing");
        assert!(
            analyze(&doing, backlog(), None)
                .expect_err("a doing task without a session note must fail")
                .iter()
                .any(|violation| violation.contains("needs a `plan/SESSION.md`"))
        );

        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let summary = analyze(&doing, &marked, Some(NOTE)).expect("every file agrees");
        assert_eq!(summary.state.status, "doing");
    }

    #[test]
    fn a_session_note_about_another_task_is_rejected() {
        let doing = HEADER.replace("state=idle", "state=doing");
        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let error = analyze(&doing, &marked, Some("task: A4\ngoal: x\nnow: y\nrun: z\n"))
            .expect_err("a foreign session note must fail");
        assert!(error.iter().any(|violation| violation.contains("is about task A4")), "{error:?}");

        let error = analyze(&doing, &marked, Some("# note without a task line\n"))
            .expect_err("a session note without a task line must fail");
        assert!(error.iter().any(|violation| violation.contains("needs a `task: <id>` line")), "{error:?}");
    }

    #[test]
    fn a_session_note_needs_a_goal_the_current_step_and_a_command() {
        let doing = HEADER.replace("state=idle", "state=doing");
        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let error = analyze(&doing, &marked, Some("task: A3\ngoal:\nrun:   \n")).expect_err("an empty note must fail");
        for key in ["goal", "now", "run"] {
            assert!(
                error.iter().any(|violation| violation.contains(&format!("`{key}:` line"))),
                "the error must name `{key}:`: {error:?}"
            );
        }
    }

    #[test]
    fn a_doing_task_that_the_backlog_does_not_mark_is_rejected() {
        let doing = HEADER.replace("state=idle", "state=doing");
        let error = analyze(&doing, backlog(), Some(NOTE)).expect_err("a doing state without a `[~]` marker must fail");
        assert!(error.iter().any(|violation| violation.contains("exactly one `[~]`")), "{error:?}");
    }

    #[test]
    fn an_idle_plan_must_not_hide_a_session_note_or_an_open_task() {
        let error = analyze(HEADER, backlog(), Some(NOTE)).expect_err("an idle session note must fail");
        assert!(error.iter().any(|violation| violation.contains("must not leave")), "{error:?}");

        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let error = analyze(HEADER, &marked, None).expect_err("an idle open task must fail");
        assert!(error.iter().any(|violation| violation.contains("still in progress")), "{error:?}");
    }

    #[test]
    fn the_state_must_name_a_task_in_the_backlog() {
        let error =
            analyze(&HEADER.replace("task=A3", "task=A9"), backlog(), None).expect_err("an unknown task id must fail");
        assert!(error.iter().any(|violation| violation.contains("not a task in `plan/BACKLOG.md`")), "{error:?}");
    }

    #[test]
    fn the_current_task_must_sit_in_the_now_section() {
        let split = "## Now\n\n\
                     - `[ ]` **A3** first `files: crates/ui`\n\n\
                     ## Next\n\n\
                     - `[ ]` **A4** later `files: crates/model`\n\n\
                     ## Done\n\n\
                     - `[x]` **A2** ring of segments — `0833b4c`\n";
        let error = analyze(&HEADER.replace("task=A3", "task=A4"), split, None)
            .expect_err("a `## Next` task cannot be the current one");
        assert!(error.iter().any(|violation| violation.contains("must be in `## Now`")), "{error:?}");
    }

    #[test]
    fn a_finished_marker_only_belongs_in_the_done_section() {
        let stray = backlog().replace("- `[ ]` **A4**", "- `[x]` **A4**");
        let error = analyze(HEADER, &stray, None).expect_err("`[x]` outside `## Done` must fail");
        assert!(error.iter().any(|violation| violation.contains("belongs in `## Done`")), "{error:?}");

        let reopened = backlog().replace("- `[x]` **A2**", "- `[ ]` **A2**");
        let error = analyze(HEADER, &reopened, None).expect_err("`## Done` holding an open task must fail");
        assert!(error.iter().any(|violation| violation.contains("without `[x]`")), "{error:?}");
    }

    #[test]
    fn an_unfinished_task_needs_a_files_hint() {
        let hintless = backlog().replace(" `files: crates/model/src/lib.rs`", "");
        let error = analyze(HEADER, &hintless, None).expect_err("a task without a hint must fail");
        assert!(error.iter().any(|violation| violation.contains("has no `files:` hint")), "{error:?}");
    }

    #[test]
    fn a_hinted_path_that_does_not_exist_is_reported() {
        let missing =
            missing_paths("crates/ui/src/lib.rs, crates/ui/src/missing.rs", |path| path == "crates/ui/src/lib.rs");
        assert_eq!(missing, ["crates/ui/src/missing.rs"]);

        let cleaned =
            missing_paths("`crates/ui` , docs/decisions/.", |path| path == "crates/ui" || path == "docs/decisions/");
        assert!(cleaned.is_empty(), "{cleaned:?}");
    }

    #[test]
    fn the_base_must_be_a_commit_that_leads_to_head() {
        assert!(base_violation("HEAD", Some("db8374d"), true, true).is_none(), "`HEAD` is a legal base");
        let stale = base_violation("ea3153a", Some("db8374d"), true, false).expect("a side commit must fail");
        assert!(stale.contains("not an ancestor"), "{stale}");
        let unknown = base_violation("nope", Some("db8374d"), false, false).expect("an unknown commit must fail");
        assert!(unknown.contains("not a commit"), "{unknown}");
    }

    #[test]
    fn an_over_long_plan_file_fails_the_gate() {
        let long_status = format!("{HEADER}{}", "filler\n".repeat(MAX_STATUS_LINES));
        let error = analyze(&long_status, backlog(), None).expect_err("the budget is an error, not a warning");
        assert!(error.iter().any(|violation| violation.contains("over the 70-line budget")), "{error:?}");

        let doing = HEADER.replace("state=idle", "state=doing");
        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let long_note = format!("{NOTE}{}", "filler\n".repeat(MAX_SESSION_LINES));
        let error = analyze(&doing, &marked, Some(&long_note)).expect_err("an over-long note must fail");
        assert!(error.iter().any(|violation| violation.contains("over the 50-line budget")), "{error:?}");
    }

    #[test]
    fn a_task_line_without_a_bold_id_fails_the_gate() {
        let broken = format!("{}- `[ ]` A9 an editor without a bold id\n", backlog());
        let error = analyze(HEADER, &broken, None).expect_err("a malformed task line must fail");
        assert!(error.iter().any(|violation| violation.contains("without a `**bold**` id")), "{error:?}");
    }

    #[test]
    fn a_missing_state_line_or_field_is_reported() {
        let error = analyze("# Status\n", backlog(), None).expect_err("a missing state line must fail");
        assert!(error.iter().any(|violation| violation.contains("needs a `STATE:` line")), "{error:?}");

        let error = analyze("STATE: milestone=M7 task=A3\n", backlog(), None).expect_err("missing fields must fail");
        assert!(error.iter().any(|violation| violation.contains("needs `state=`")), "{error:?}");
        assert!(error.iter().any(|violation| violation.contains("needs `base=`")), "{error:?}");
    }

    #[test]
    fn a_duplicate_backlog_id_is_rejected() {
        let doubled = backlog().replace("- `[ ]` **A4**", "- `[ ]` **A3**");
        let error = analyze(HEADER, &doubled, None).expect_err("a duplicate id must fail");
        assert!(error.iter().any(|violation| violation.contains("appears twice")), "{error:?}");
    }

    #[test]
    fn the_print_summary_shows_the_current_task_and_the_next_three() {
        let summary = analyze(HEADER, backlog(), None).expect("the plan is consistent");
        assert_eq!(summary.state.milestone, "M7");
        // `print` writes to stdout; only its input is checked here.
        assert_eq!(summary.tasks.iter().filter(|task| task.status == TaskStatus::Open).count(), 2);
    }

    #[test]
    fn a_task_marker_may_be_wrapped_in_backticks() {
        let plain = parse_backlog("- [ ] **A3** text\n");
        assert_eq!(plain.len(), 1, "an unwrapped marker is a task too");
        assert_eq!(plain[0].id, "A3");
    }

    #[test]
    fn a_wrapped_task_keeps_its_files_hint() {
        let wrapped = "## Now\n\n\
                       - `[ ]` **A3** a long sentence in the card\n  \
                       wrapped onto a second line. `files: crates/ui/src/object_card.rs, crates/ui/src/lib.rs`\n\n\
                       ## Done\n\n\
                       - `[x]` **A2** old — `0833b4c`\n";
        let tasks = parse_backlog(wrapped);
        let task = tasks.iter().find(|task| task.id == "A3").expect("A3 parses");
        assert_eq!(task.files.as_deref(), Some("crates/ui/src/object_card.rs, crates/ui/src/lib.rs"));
        assert!(task.text.contains("wrapped onto a second line"), "{:?}", task.text);
    }

    #[test]
    fn the_state_may_not_point_at_a_finished_task() {
        let error = analyze(&HEADER.replace("task=A3", "task=A2"), backlog(), None)
            .expect_err("a finished task cannot be the current one");
        assert!(error.iter().any(|violation| violation.contains("is done")), "{error:?}");
    }
}
