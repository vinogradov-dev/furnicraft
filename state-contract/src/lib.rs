//! A plan gate for agent-driven projects.
//!
//! A project keeps its plan in three files: `plan/STATUS.md` with exactly one `STATE:` line,
//! `plan/BACKLOG.md` with `## Now`, `## Next` and `## Done` sections, and `plan/SESSION.md` while a
//! task is in progress. This crate parses them, checks that they agree with each other and with the
//! files they name, and renders the block a session starts from.
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

mod git;
mod parse;
mod status;
mod validate;

use std::path::Path;

pub use git::{Git, ProcessGit};
pub use parse::parse_backlog;
pub use status::render;
pub use validate::{MAX_SESSION_LINES, MAX_STATUS_LINES, analyze};

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
    let session_path = root.join("plan/SESSION.md");
    let session = match std::fs::read_to_string(&session_path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(vec![format!("cannot read `plan/SESSION.md`: {error}")]),
    };

    let mut violations = Vec::new();
    let summary = match analyze(&status, &backlog, session.as_deref()) {
        Ok(summary) => Some(summary),
        Err(errors) => {
            violations.extend(errors);
            None
        }
    };
    if let Some(summary) = &summary {
        violations.extend(workspace_violations(root, summary, git));
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
    check_with(root, git).map(|summary| render(&summary))
}

fn read(path: &Path, name: &str) -> Result<String, Vec<String>> {
    std::fs::read_to_string(path).map_err(|error| vec![format!("cannot read `{name}`: {error}")])
}

/// Checks what only the filesystem and git can answer: the hinted paths and the `base` commit.
fn workspace_violations(root: &Path, summary: &Summary, git: &dyn Git) -> Vec<String> {
    let mut violations = Vec::new();

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

    let head = git.head();
    let base = summary.state.base.as_str();
    let exists = git.commit_exists(base);
    let ancestor = head.as_deref().is_some_and(|head| git.is_ancestor(base, head));
    if let Some(violation) = validate::base_violation(base, head.as_deref(), exists, ancestor) {
        violations.push(violation);
    }

    violations
}
