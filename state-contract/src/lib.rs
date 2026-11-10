//! A plan gate for agent-driven projects.
//!
//! A project keeps its plan in three files: `plan/STATUS.md` with exactly one `STATE:` line,
//! `plan/BACKLOG.md` with `## Now`, `## Next` and `## Done` sections, and `plan/SESSION.md` while a
//! task is in progress; `plan/notes/<ID>.md` holds the research note of one task. This crate parses
//! them, checks that they agree with each other and with the files they name, renders the block a
//! session starts from, and prints one task in full — its fields, map rows, notes, debt, questions
//! and the session fields — so a host need not grep the plan files. It changes nothing by itself:
//! [`edit`] splices the lines a command owns and [`check_text`] validates the result before a host
//! writes it.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
//!
//! It has no dependencies and no knowledge of any product: the inputs are text, the filesystem and
//! `git`. [`check`] does all of it, [`analyze`] is the pure part, and [`Git`] is the injection point
//! for tests or for a host that already knows the repository.
//!
//! ```no_run
//! # use std::path::Path;
//! match state_contract::check(Path::new(".")) {
//!     Ok(summary) => println!("{}", state_contract::render(&summary)),
//!     Err(violations) => {
//!         for violation in violations {
//!             eprintln!("error: {violation}");
//!         }
//!     }
//! }
//! ```

pub mod cli;
mod dashboard;
pub mod edit;
mod git;
mod notes;
mod parse;
mod section;
mod status;
mod task;
mod validate;

use std::path::Path;

pub use cli::{Command, Config, Global};
pub use dashboard::{DashboardOptions, dashboard, dashboard_with};
pub use git::{Git, ProcessGit};
pub use notes::{MAX_NOTE_LINES, note, note_path};
pub use parse::parse_backlog;
pub use section::{Section, Style, sections, wrap};
pub use status::{render, render_with_detail};
pub use task::{
    Closed, RelatedNote, TaskOptions, TaskView, task, task_sections, task_view, task_view_with, task_with, touches,
};
pub use validate::{MAX_SESSION_LINES, MAX_STATUS_LINES, README_RULE, analyze};

/// What the plan files say right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The `STATE:` fields.
    pub state: State,
    /// Backlog tasks, in file order.
    pub tasks: Vec<Task>,
    /// The parsed session note, when `plan/SESSION.md` exists.
    pub session: Option<Session>,
    /// The `HEAD` commit, filled in by whichever caller resolved it; `None` outside a checkout.
    pub head: Option<String>,
    /// Whether the session looks cut short: `doing`, a note with open steps, and commits past `base`.
    pub interrupted: bool,
}

/// The parts of `plan/SESSION.md` a fresh session resumes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// `task:` — the task the note is about.
    pub task: Option<String>,
    /// `goal:` — the user-visible result, in one sentence.
    pub goal: Option<String>,
    /// `now:` — the exact sub-step in progress.
    pub now: Option<String>,
    /// `blocked:` — what the session waits for, or `nothing`.
    pub blocked: Option<String>,
    /// `run:` — the command that reproduces the current state.
    pub run: Option<String>,
    /// `resume:` — the first action a fresh session takes.
    pub resume: Option<String>,
}

/// The parsed `STATE:` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Milestone, for example `M7`.
    pub milestone: String,
    /// The phase inside the milestone, when the project has one.
    pub phase: Option<String>,
    /// The task id that is current, for example `A3`.
    pub task: String,
    /// `idle` or `doing`.
    pub status: String,
    /// The commit the plan describes; `HEAD` while the plan edit itself is uncommitted.
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
    /// The whole entry, continuation lines joined into one: what a reader quotes.
    pub text: String,
    /// The opening sentence, before the first labelled field — the line a dashboard shows.
    pub lead: String,
    /// The labelled parts of the entry, in file order: `Rules`, `Why`, `Shape`, `Done`, `Commits`.
    pub fields: Vec<Field>,
    /// The `files:` hint, when the line has one.
    pub files: Option<String>,
    /// The `files:` hint split into paths, in the order it names them.
    pub paths: Vec<String>,
}

/// One labelled part of a backlog entry, for example `Why: …`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// The label as the entry spells it, in its canonical form: `Rules`, `Why`, `Shape`, `Done`,
    /// `Commits`.
    pub label: String,
    /// The text after the label, up to the next label.
    pub body: String,
}

impl Task {
    /// The body of the field with this label, if the entry carries one.
    pub fn field(&self, label: &str) -> Option<&str> {
        self.fields.iter().find(|field| field.label.eq_ignore_ascii_case(label)).map(|field| field.body.as_str())
    }
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
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Open => "[ ]",
            Self::Doing => "[~]",
            Self::Done => "[x]",
        }
    }
}

/// Reads the plan files under `root`, checks them and returns what they say.
///
/// A single error vector holds every violation, so a caller can print all of them in one run; each
/// message is a complete sentence meant for the user.
pub fn check(root: &Path) -> Result<Summary, Vec<String>> {
    check_with(root, &ProcessGit::new(root))
}

/// [`check`] with the git facts supplied by `git`, so tests need no repository.
pub fn check_with(root: &Path, git: &dyn Git) -> Result<Summary, Vec<String>> {
    let status = read(&root.join("plan/STATUS.md"), "plan/STATUS.md")?;
    let backlog = read(&root.join("plan/BACKLOG.md"), "plan/BACKLOG.md")?;
    let session = match std::fs::read_to_string(root.join("plan/SESSION.md")) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(vec![format!("cannot read `plan/SESSION.md`: {error}")]),
    };
    let plan = PlanText { status: &status, backlog: &backlog, session: session.as_deref(), notes: None };
    check_text(root, &plan, git)
}

/// A plan as text, for a writer that has not written it yet.
///
/// Everything the gate needs from the filesystem — the README, the paths a `files:` hint names, the
/// commits and the base — still comes from `root`; only the plan files come from the caller.
pub struct PlanText<'a> {
    /// `plan/STATUS.md`.
    pub status: &'a str,
    /// `plan/BACKLOG.md`.
    pub backlog: &'a str,
    /// `plan/SESSION.md`, or `None` when the plan has no note.
    pub session: Option<&'a str>,
    /// `plan/notes/` as `(file name, text)`, when the caller changed it; `None` reads the disk.
    pub notes: Option<&'a [(String, String)]>,
}

/// Checks a plan given as text against the workspace under `root`: the same rules [`check_with`]
/// applies, with the plan files supplied instead of read.
pub fn check_text(root: &Path, plan: &PlanText<'_>, git: &dyn Git) -> Result<Summary, Vec<String>> {
    let mut violations = Vec::new();
    let mut summary = match analyze(plan.status, plan.backlog, plan.session) {
        Ok(summary) => Some(summary),
        Err(errors) => {
            violations.extend(errors);
            None
        }
    };
    if let Some(summary) = summary.as_mut() {
        // One git call answers both the `base` rule and the head line the block prints.
        summary.head = git.head();
        summary.interrupted = session_interrupted(&summary.state, plan.session, summary.head.as_deref(), git);
        violations.extend(workspace_violations(root, summary, git));
        if let Some(notes) = plan.notes {
            violations.extend(notes::note_violations_in(notes, &summary.tasks));
        }
        // The reference files are the plan too: a stale map row or an undeclared debt tag rots it.
        if let Ok(map) = std::fs::read_to_string(root.join("plan/map.md")) {
            violations.extend(validate::map_violations(&map, |path| root.join(path).exists()));
        }
        if let Ok(debt) = std::fs::read_to_string(root.join("plan/debt.md")) {
            violations.extend(validate::debt_violations(&debt));
        }
    }
    match std::fs::read_to_string(root.join("README.md")) {
        Ok(readme) => violations.extend(validate::readme_violations(&readme)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => violations.push(format!("cannot read `README.md`: {error}")),
    }
    match summary {
        Some(summary) if violations.is_empty() => Ok(summary),
        _ => Err(violations),
    }
}

/// The session-start block for `root`, or the violations that prevent one.
pub fn status(root: &Path) -> Result<String, Vec<String>> {
    status_with(root, &ProcessGit::new(root))
}

/// [`status`] with the git facts supplied by `git`.
pub fn status_with(root: &Path, git: &dyn Git) -> Result<String, Vec<String>> {
    status_with_detail(root, git, None)
}

/// [`status_with`] plus the command a host points at for the full task, for example
/// `cargo xtask task`: the block names it once, under the current task, instead of printing the
/// whole entry. `None` prints no hint, for a project without a task command.
pub fn status_with_detail(root: &Path, git: &dyn Git, detail: Option<&str>) -> Result<String, Vec<String>> {
    check_with(root, git).map(|summary| status::render_with_detail(&summary, detail))
}

fn read(path: &Path, name: &str) -> Result<String, Vec<String>> {
    std::fs::read_to_string(path).map_err(|error| vec![format!("cannot read `{name}`: {error}")])
}

/// Whether the session looks cut short: work was running, the plan edit rides on commits past the
/// `base`, and the note still has an open step. One definition, two readers — the status block and
/// the dashboard flag.
pub(crate) fn session_interrupted(state: &State, session: Option<&str>, head: Option<&str>, git: &dyn Git) -> bool {
    if state.status != "doing" {
        return false;
    }
    let (Some(session), Some(head)) = (session, head) else { return false };
    if state.base == head || !git.is_ancestor(&state.base, head) {
        return false;
    }
    has_open_steps(session)
}

/// Whether the `steps:` block of a session note still lists `[ ]` items.
fn has_open_steps(session: &str) -> bool {
    let mut in_steps = false;
    for line in session.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with("steps:") {
            in_steps = true;
            continue;
        }
        if in_steps {
            if trimmed.is_empty() {
                break;
            }
            if !line.starts_with([' ', '\t']) && !trimmed.starts_with("[ ]") && !trimmed.starts_with("[x]") {
                break;
            }
            if trimmed.starts_with("[ ]") {
                return true;
            }
        }
    }
    false
}

/// Checks what only the filesystem and git can answer: the hinted paths and the `base` commit.
fn workspace_violations(root: &Path, summary: &Summary, git: &dyn Git) -> Vec<String> {
    let mut violations = notes::note_violations(root, &summary.tasks);

    for task in summary.tasks.iter().filter(|task| task.status != TaskStatus::Done) {
        let Some(hint) = &task.files else { continue };
        for path in parse::missing_paths(hint, |path| root.join(path).exists()) {
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
        } else if !git.commit_exists(hash) {
            violations.push(format!("backlog task {} names commit `{hash}`, which is not in this repository", task.id));
        }
    }

    let head = summary.head.as_deref();
    let base = summary.state.base.as_str();
    let exists = git.commit_exists(base);
    let ancestor = head.is_some_and(|head| git.is_ancestor(base, head));
    if let Some(violation) = validate::base_violation(base, head, exists, ancestor) {
        violations.push(violation);
    } else if let Some(violation) =
        validate::updated_base_violation(&summary.state.updated, base, git.commit_date(base).as_deref())
    {
        // Only a base that resolves can have a date; an unknown base already has its own error.
        violations.push(violation);
    }

    violations
}
