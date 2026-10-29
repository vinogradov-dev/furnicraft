//! The plan gate: `plan/STATUS.md`, `plan/BACKLOG.md` and `plan/SESSION.md` must agree.
//!
//! A session starts by reading `STATE` and the backlog, so a stale or contradictory plan file is a
//! real defect: it sends the next session to the wrong task. The checks are cheap string parsing plus
//! a few `git cat-file` lookups; only an inconsistency fails, a length over the budget is a warning.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// Longest accepted `plan/STATUS.md`, in lines.
const MAX_STATUS_LINES: usize = 70;
/// Longest accepted `plan/SESSION.md`, in lines.
const MAX_SESSION_LINES: usize = 50;
/// The marker that starts the state line.
const STATE_PREFIX: &str = "STATE:";

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
    /// The commit the plan was last written against.
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
pub fn check(workspace: &Path) -> Result<(Summary, Vec<String>), Vec<String>> {
    let status = read(&workspace.join("plan/STATUS.md"), "plan/STATUS.md")?;
    let backlog = read(&workspace.join("plan/BACKLOG.md"), "plan/BACKLOG.md")?;
    let session = workspace.join("plan/SESSION.md");
    let session_text = match std::fs::read_to_string(&session) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(vec![format!("cannot read `plan/SESSION.md`: {error}")]),
    };
    let summary = analyze(&status, &backlog, session_text.as_deref())?;

    let mut warnings = Vec::new();
    for (name, text, max) in [
        ("plan/STATUS.md", status.as_str(), MAX_STATUS_LINES),
        ("plan/SESSION.md", session_text.as_deref().unwrap_or(""), MAX_SESSION_LINES),
    ] {
        let lines = text.lines().count();
        if lines > max {
            warnings.push(format!("{name} has {lines} lines, over the {max}-line budget"));
        }
    }

    let mut violations = Vec::new();
    for task in summary.tasks.iter().filter(|task| task.status == TaskStatus::Done) {
        let Some(hash) = task.text.rsplit('`').nth(1) else {
            violations.push(format!("backlog task {} is done without a commit hash", task.id));
            continue;
        };
        if !commit_exists(workspace, hash) {
            violations.push(format!("backlog task {} names commit `{hash}`, which is not in this repository", task.id));
        }
    }
    if !commit_exists(workspace, &summary.state.base) {
        violations.push(format!("`STATE: base={}` is not a commit in this repository", summary.state.base));
    }
    if violations.is_empty() { Ok((summary, warnings)) } else { Err(violations) }
}

fn read(path: &Path, name: &str) -> Result<String, Vec<String>> {
    std::fs::read_to_string(path).map_err(|error| vec![format!("cannot read `{name}`: {error}")])
}

/// Checks the three plan files against each other, without touching the filesystem.
pub fn analyze(status: &str, backlog: &str, session: Option<&str>) -> Result<Summary, Vec<String>> {
    let state = parse_state(status)?;
    let tasks = parse_backlog(backlog);
    let mut violations = parse_violations(status, backlog, &tasks);

    let current = tasks.iter().find(|task| task.id == state.task);
    match current {
        Some(Task { status: TaskStatus::Open | TaskStatus::Doing, .. }) => {}
        Some(Task { status: TaskStatus::Done, .. }) => {
            violations.push(format!("backlog task {} is done, but `STATE: task={}` names it", state.task, state.task));
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
                _ => {}
            }
            if session.is_none() {
                violations
                    .push("`STATE: state=doing` needs a `plan/SESSION.md`; copy `plan/SESSION.template.md`".into());
            }
            if let Some(session) = session {
                match session_task(session) {
                    Some(task) if task == state.task => {}
                    Some(task) => violations.push(format!(
                        "`plan/SESSION.md` is about task {task}, but `STATE: task={}` names another",
                        state.task
                    )),
                    None => violations.push("`plan/SESSION.md` needs a `task: <id>` line".into()),
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
/// The marker may be wrapped in backticks for rendering; both forms are accepted.
pub fn parse_backlog(backlog: &str) -> Vec<Task> {
    backlog
        .lines()
        .filter_map(|line| {
            let line = line.trim_start().strip_prefix("- ")?;
            let line = line.trim().strip_prefix('`').unwrap_or(line.trim());
            let (status, rest) = if let Some(rest) = line.strip_prefix(TaskStatus::Open.marker()) {
                (TaskStatus::Open, rest)
            } else if let Some(rest) = line.strip_prefix(TaskStatus::Doing.marker()) {
                (TaskStatus::Doing, rest)
            } else {
                (TaskStatus::Done, line.strip_prefix(TaskStatus::Done.marker())?)
            };
            let rest = rest.trim_start().strip_prefix('`').unwrap_or(rest.trim_start()).trim();
            let id = bold(rest)?;
            let files = files_hint(rest);
            let text = strip_files(rest);
            let text = text
                .trim_start()
                .strip_prefix("**")
                .and_then(|text| text.split_once("**"))
                .map_or(text, |(_, text)| text);
            let text = text.trim_start().trim_start_matches(['—', '-', '·']).trim().to_owned();
            Some(Task { id, status, text, files })
        })
        .collect()
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
        Some((text, _)) => text,
        None => text,
    }
}

/// The task id named by a session note's `task:` line.
fn session_task(session: &str) -> Option<String> {
    for line in session.lines() {
        let line = line.trim();
        if let Some(task) = line.strip_prefix("task:") {
            let task = task.trim().trim_start_matches('`').trim_end_matches('`').trim();
            if !task.is_empty() {
                return Some(task.to_owned());
            }
        }
    }
    None
}

/// Report-only checks: duplicates and an id that is not a plain label.
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
fn commit_exists(workspace: &Path, hash: &str) -> bool {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    Command::new("git")
        .args(["cat-file", "-e", &format!("{hash}^{{commit}}")])
        .current_dir(workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
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
        let summary = analyze(&doing, &marked, Some("task: A3\n")).expect("every file agrees");
        assert_eq!(summary.state.status, "doing");
    }

    #[test]
    fn a_session_note_about_another_task_is_rejected() {
        let doing = HEADER.replace("state=idle", "state=doing");
        let marked = backlog().replace("- `[ ]` **A3**", "- `[~]` **A3**");
        let error = analyze(&doing, &marked, Some("task: A4\n")).expect_err("a foreign session note must fail");
        assert!(error.iter().any(|violation| violation.contains("is about task A4")), "{error:?}");

        let error = analyze(&doing, &marked, Some("# note without a task line\n"))
            .expect_err("a session note without a task line must fail");
        assert!(error.iter().any(|violation| violation.contains("needs a `task: <id>` line")), "{error:?}");
    }

    #[test]
    fn a_doing_task_that_the_backlog_does_not_mark_is_rejected() {
        let doing = HEADER.replace("state=idle", "state=doing");
        let error =
            analyze(&doing, backlog(), Some("task: A3\n")).expect_err("a doing state without a `[~]` marker must fail");
        assert!(error.iter().any(|violation| violation.contains("exactly one `[~]`")), "{error:?}");
    }

    #[test]
    fn an_idle_plan_must_not_hide_a_session_note_or_an_open_task() {
        let error = analyze(HEADER, backlog(), Some("task: A3\n")).expect_err("an idle session note must fail");
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
    fn the_state_may_not_point_at_a_finished_task() {
        let error = analyze(&HEADER.replace("task=A3", "task=A2"), backlog(), None)
            .expect_err("a finished task cannot be the current one");
        assert!(error.iter().any(|violation| violation.contains("is done")), "{error:?}");
    }
}
