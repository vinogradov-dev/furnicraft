//! One task, with everything the plan says about it.
//!
//! A session start needs the short block ([`crate::status`]); starting the work needs the whole
//! entry, the note the last session left, the map rows that point at the code, the debt and the
//! open questions the task touches, and the ADRs it cites. This module collects the plan half of
//! that in one value, so a host adds only what the plan cannot know — git and the file sizes — and
//! nobody greps `plan/*.md` to reconstruct a task.
//!
//! The rendering is framed like the dashboard because a human reads both; an agent-facing form is
//! the host's, and it reads [`TaskView`] rather than the text.

use std::fmt::Write as _;
use std::path::Path;

use crate::notes::related_notes;
use crate::parse::{MapRow, markdown_bullets, parse_map};
use crate::section::{BOLD, GREEN, Section, Style, YELLOW, paint, sections, wrap};
use crate::{Git, ProcessGit, Session, Summary, Task, TaskStatus, note};

/// How a task is reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskOptions {
    /// The frame width in characters, at least 24.
    pub width: usize,
    /// Whether to emit ANSI escape codes.
    pub color: bool,
    /// Show every debt entry and every open question, not only the ones the task touches.
    pub all: bool,
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self { width: 80, color: false, all: false }
    }
}

/// What another task's note says about this task's files: its id and its first line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedNote {
    /// The task id the note belongs to.
    pub id: String,
    /// The note's first line, without its heading marker.
    pub title: String,
}

/// The commit a finished task landed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    /// The seven-character hash the entry names.
    pub hash: String,
    /// The author date of that commit, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// The commit's subject line.
    pub subject: Option<String>,
}

/// Everything the plan files say about one task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskView {
    /// The task itself, with its labelled fields and paths.
    pub task: Task,
    /// The next unfinished task in `## Now`, in file order: the work order, not the id order.
    pub next: Option<Task>,
    /// The raw note under `plan/notes/<ID>.md`, when the task has one.
    pub notes: Option<String>,
    /// Other tasks' notes that name one of this task's files.
    pub related: Vec<RelatedNote>,
    /// The commit a finished task landed in, when the entry names one.
    pub closed: Option<Closed>,
    /// The `plan/map.md` rows whose paths meet the task's `files:` hint.
    pub map: Vec<MapRow>,
    /// Every `plan/debt.md` entry, in file order; [`touches`] says which are the task's.
    pub debt: Vec<String>,
    /// Every `plan/questions.md` decision, in file order; [`touches`] says which are the task's.
    pub questions: Vec<String>,
    /// The live session note, while it is about this task.
    pub session: Option<Session>,
}

impl TaskView {
    /// The debt entries that name one of the task's paths, unless every entry was asked for.
    pub fn debt_for(&self, all: bool) -> Vec<String> {
        select(&self.debt, &self.task, all)
    }

    /// The open questions that name one of the task's paths, unless every entry was asked for.
    pub fn questions_for(&self, all: bool) -> Vec<String> {
        select(&self.questions, &self.task, all)
    }
}

/// The entries that name the task, or all of them when `all` is set.
fn select(entries: &[String], task: &Task, all: bool) -> Vec<String> {
    entries.iter().filter(|entry| all || touches(entry, task)).cloned().collect()
}

/// Whether a `plan/debt.md` or `plan/questions.md` entry is about this task: it names one of the
/// task's paths, the crate the path sits in, a file stem long enough to be a word of its own, or the
/// crate's own tag (`[engine]`).
pub fn touches(entry: &str, task: &Task) -> bool {
    let entry = entry.to_lowercase();
    let (needles, tags) = needles(task);
    needles.iter().any(|needle| entry.contains(needle.as_str()))
        || tags.iter().any(|tag| entry.contains(&format!("[{tag}]")))
}

/// The lower-case strings a plan entry may use to name this task's area, and the crate tags it may
/// carry. A bare crate name is a tag only: `engine` as a word of prose names nothing.
fn needles(task: &Task) -> (Vec<String>, Vec<String>) {
    let mut needles = Vec::new();
    let mut tags = Vec::new();
    for path in &task.paths {
        let path = path.to_lowercase();
        needles.push(path.clone());
        if let Some((krate, _)) = path.split_once("/src/") {
            needles.push(krate.to_owned());
            if let Some(name) = krate.rsplit('/').next()
                && name.len() >= 4
            {
                tags.push(name.to_owned());
            }
        }
        if let Some(stem) = path.rsplit('/').next().and_then(|name| name.split('.').next())
            && stem.len() >= 5
            && !matches!(stem, "lib" | "main" | "mod")
        {
            needles.push(stem.to_owned());
        }
    }
    (needles, tags)
}

/// Reads the plan under `root` and returns what it says about `id`, or every violation.
pub fn task_view(root: &Path, id: &str) -> Result<TaskView, Vec<String>> {
    task_view_with(root, id, &ProcessGit::new(root))
}

/// [`task_view`] with the git facts supplied by `git`, so tests need no repository.
pub fn task_view_with(root: &Path, id: &str, git: &dyn Git) -> Result<TaskView, Vec<String>> {
    let summary = crate::check_with(root, git)?;
    let map = read_optional(root, "plan/map.md")?;
    let debt = read_optional(root, "plan/debt.md")?;
    let questions = read_optional(root, "plan/questions.md")?;
    let notes = note(root, id)?;
    let mut view = view_from(&summary, id, map.as_deref(), debt.as_deref(), questions.as_deref(), notes, git)?;
    view.related = related_notes(root, &view.task);
    Ok(view)
}

/// The view over a plan that is already checked, for a caller that holds the summary.
pub(crate) fn view_from(
    summary: &Summary,
    id: &str,
    map: Option<&str>,
    debt: Option<&str>,
    questions: Option<&str>,
    notes: Option<String>,
    git: &dyn Git,
) -> Result<TaskView, Vec<String>> {
    let Some(task) = summary.tasks.iter().find(|task| task.id.eq_ignore_ascii_case(id)).cloned() else {
        return Err(vec![format!("`plan/BACKLOG.md` has no task `{id}`")]);
    };
    // Only a task in `## Now` has a place in the work order; a bug, a future task and a done one
    // have no "next".
    let mut next = None;
    if task.section == "Now" {
        let mut passed = false;
        for other in &summary.tasks {
            if other.id == task.id {
                passed = true;
            } else if passed && other.section == "Now" && other.status != TaskStatus::Done {
                next = Some(other.clone());
                break;
            }
        }
    }
    let session = summary.session.clone().filter(|_| summary.state.status == "doing" && summary.state.task == task.id);
    Ok(TaskView {
        map: map
            .map(parse_map)
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.paths().iter().any(|row_path| task.paths.iter().any(|p| same(p, row_path))))
            .collect(),
        debt: debt.map(markdown_bullets).unwrap_or_default(),
        questions: questions.map(markdown_bullets).unwrap_or_default(),
        closed: closed_by(git, &task),
        related: Vec::new(),
        task,
        next,
        notes,
        session,
    })
}

/// The commit a finished task names in its entry, with what git knows about it.
fn closed_by(git: &dyn Git, task: &Task) -> Option<Closed> {
    if task.status != TaskStatus::Done {
        return None;
    }
    let hash = task.text.rsplit('`').nth(1)?;
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Closed { hash: hash.to_owned(), date: git.commit_date(hash), subject: git.subject(hash) })
}

/// Whether two plan paths name the same place, allowing a directory hint for a file row.
fn same(left: &str, right: &str) -> bool {
    std::path::Path::new(left).starts_with(right) || std::path::Path::new(right).starts_with(left)
}

/// The task as framed sections a human reads: the entry, its fields, files, map, notes, the debt
/// and questions it touches, the ADRs it cites and the live session note.
pub fn task_sections(view: &TaskView, opts: &TaskOptions) -> Vec<Section> {
    let style = Style::new(opts.width, opts.color);
    let width = style.text_width();
    let task = &view.task;
    let mut blocks = Vec::new();

    let mut head = Section::new("TASK", "plan/BACKLOG.md");
    head.push(format!(
        "{} {} · ## {}",
        paint(task.status.marker(), status_color(task.status, style.color)),
        paint(&task.id, style.color.then_some(BOLD)),
        task.section
    ));
    head.lines.extend(wrap(&task.lead, width));
    if let Some(closed) = &view.closed {
        let mut line = format!("closed: {}", closed.hash);
        if let Some(date) = &closed.date {
            let _ = write!(line, " · {date}");
        }
        if let Some(subject) = &closed.subject {
            let _ = write!(line, " · {subject}");
        }
        head.lines.extend(wrap(&line, width));
    }
    if let Some(next) = &view.next {
        head.push(format!("next: {} · {}", next.id, next.lead));
    }
    blocks.push(head);

    if !task.fields.is_empty() {
        blocks.push(Section::new("DETAIL", "plan/BACKLOG.md").lines(field_lines(task, width)));
    }

    if !task.paths.is_empty() {
        blocks.push(Section::new("FILES", "plan/BACKLOG.md").lines(task.paths.clone()));
    }

    let mut map = Section::new("MAP", "plan/map.md");
    for row in &view.map {
        map.push(format!("{} — {}", row.feature, row.where_));
    }
    if !map.is_empty() {
        blocks.push(map);
    }

    if let Some(notes) = &view.notes {
        blocks.push(Section::new("NOTES", format!("plan/notes/{}.md", task.id)).lines(note_lines(notes, width)));
    }

    if !view.related.is_empty() {
        blocks.push(
            Section::new("RELATED", "plan/notes/")
                .lines(view.related.iter().map(|note| format!("{} — {}", note.id, note.title))),
        );
    }

    for (title, source, entries, total) in [
        ("DEBT", "plan/debt.md", view.debt_for(opts.all), view.debt.len()),
        ("QUESTIONS", "plan/questions.md", view.questions_for(opts.all), view.questions.len()),
    ] {
        if entries.is_empty() {
            continue;
        }
        let mut block = Section::new(title, source);
        for entry in &entries {
            block.lines.extend(wrap(&format!("· {entry}"), width));
        }
        // What the tool filtered out is part of the answer: the reader must know it chose.
        if let Some(hidden) = total.checked_sub(entries.len()).filter(|hidden| *hidden > 0) {
            block.push(format!("+{hidden} other entries name other areas; `--all` prints them"));
        }
        blocks.push(block);
    }

    if let Some(session) = &view.session {
        blocks.push(Section::new("SESSION", "plan/SESSION.md").lines(session_lines(session, width)));
    }
    blocks
}

/// The framed report for one task, or every violation that prevents one.
pub fn task(root: &Path, id: &str, opts: &TaskOptions) -> Result<String, Vec<String>> {
    task_with(root, id, opts, &ProcessGit::new(root))
}

/// [`task`] with the git facts supplied by `git`.
pub fn task_with(root: &Path, id: &str, opts: &TaskOptions, git: &dyn Git) -> Result<String, Vec<String>> {
    task_view_with(root, id, git).map(|view| sections(&task_sections(&view, opts), Style::new(opts.width, opts.color)))
}

/// The labelled fields of the entry, wrapped, with a two-space continuation.
fn field_lines(task: &Task, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for field in &task.fields {
        let text = format!("{}: {}", field.label, field.body);
        for (index, line) in wrap(&text, width).into_iter().enumerate() {
            lines.push(if index == 0 { line } else { format!("  {line}") });
        }
    }
    lines
}

/// A note as framed lines: a paragraph is joined and wrapped to the frame, a fenced block and a
/// table are printed as they stand, and a bullet keeps its marker on its first line.
fn note_lines(notes: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut paragraph = Paragraph::default();
    let mut fenced = false;
    for line in notes.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            paragraph.flush(&mut lines, width);
            fenced = !fenced;
            lines.push(trimmed.to_owned());
            continue;
        }
        if fenced || trimmed.trim_start().starts_with('|') {
            paragraph.flush(&mut lines, width);
            lines.push(trimmed.to_owned());
            continue;
        }
        if trimmed.trim().is_empty() {
            paragraph.flush(&mut lines, width);
            lines.push(String::new());
            continue;
        }
        let (prefix, body) = split_prefix(trimmed);
        if !prefix.is_empty() {
            paragraph.flush(&mut lines, width);
        }
        paragraph.push(prefix, body);
    }
    paragraph.flush(&mut lines, width);
    lines
}

/// An open markdown paragraph: the marker of its first line and the text collected so far.
#[derive(Default)]
struct Paragraph {
    prefix: String,
    text: String,
}

impl Paragraph {
    /// Adds one source line's text to the open paragraph.
    fn push(&mut self, prefix: String, body: String) {
        if self.prefix.is_empty() {
            self.prefix = prefix;
        }
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(body.trim());
    }

    /// Wraps what is collected and starts a new paragraph.
    fn flush(&mut self, lines: &mut Vec<String>, width: usize) {
        if self.text.is_empty() {
            self.prefix.clear();
            return;
        }
        let room = width.saturating_sub(self.prefix.chars().count());
        for (index, part) in wrap(&self.text, room).into_iter().enumerate() {
            lines.push(if index == 0 { format!("{}{part}", self.prefix) } else { part });
        }
        self.prefix.clear();
        self.text.clear();
    }
}

/// The markdown marker a line starts with — `### `, `## `, `# `, `- `, `> ` — and the text after it.
fn split_prefix(line: &str) -> (String, String) {
    let trimmed = line.trim_start();
    for marker in ["### ", "## ", "# ", "- ", "* ", "> "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return (marker.to_owned(), rest.to_owned());
        }
    }
    (String::new(), trimmed.to_owned())
}

/// The colour of a backlog marker: neutral open, yellow doing, green done.
fn status_color(status: TaskStatus, color: bool) -> Option<&'static str> {
    if !color {
        return None;
    }
    match status {
        TaskStatus::Open => None,
        TaskStatus::Doing => Some(YELLOW),
        TaskStatus::Done => Some(GREEN),
    }
}

/// The session fields a reader resumes from, wrapped to the frame.
fn session_lines(session: &Session, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let fields = [
        ("goal", &session.goal),
        ("now", &session.now),
        ("blocked", &session.blocked),
        ("run", &session.run),
        ("resume", &session.resume),
    ];
    for (key, value) in fields {
        let Some(value) = value else { continue };
        let text = format!("{key}: {value}");
        for (index, line) in wrap(&text, width).into_iter().enumerate() {
            lines.push(if index == 0 { line } else { format!("  {line}") });
        }
    }
    lines
}

/// Reads an optional plan file; `None` when it does not exist.
fn read_optional(root: &Path, name: &str) -> Result<Option<String>, Vec<String>> {
    match std::fs::read_to_string(root.join(name)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(vec![format!("cannot read `{name}`: {error}")]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{Field, Git, State};

    /// The git facts a view needs, without a repository.
    struct FakeGit;

    impl Git for FakeGit {
        fn head(&self) -> Option<String> {
            Some("fedcba9".to_owned())
        }

        fn commit_exists(&self, hash: &str) -> bool {
            hash.chars().all(|ch| ch.is_ascii_hexdigit())
        }

        fn is_ancestor(&self, _ancestor: &str, _descendant: &str) -> bool {
            false
        }

        fn commit_date(&self, _hash: &str) -> Option<String> {
            Some("2026-11-10".to_owned())
        }

        fn subject(&self, hash: &str) -> Option<String> {
            Some(format!("a commit {hash}"))
        }
    }

    /// A summary holding `tasks` with the state line the tests need.
    fn summary(tasks: Vec<Task>) -> Summary {
        Summary {
            state: State {
                milestone: "M7".to_owned(),
                phase: None,
                task: "S14".to_owned(),
                status: "idle".to_owned(),
                base: "abc1234".to_owned(),
                updated: "2026-11-10".to_owned(),
            },
            tasks,
            session: None,
            head: Some("fedcba9".to_owned()),
            interrupted: false,
        }
    }

    fn in_section(id: &str, section: &str, status: TaskStatus) -> Task {
        Task { id: id.to_owned(), section: section.to_owned(), status, ..task() }
    }

    #[test]
    fn only_a_task_in_now_has_a_next() {
        let tasks = vec![
            in_section("S14", "Now", TaskStatus::Open),
            in_section("X31", "Bugs", TaskStatus::Open),
            in_section("S15", "Now", TaskStatus::Open),
        ];
        let view = view_from(&summary(tasks.clone()), "S14", None, None, None, None, &FakeGit).expect("S14 reads");
        assert_eq!(view.next.as_ref().map(|task| task.id.as_str()), Some("S15"));

        let bug = view_from(&summary(tasks), "X31", None, None, None, None, &FakeGit).expect("X31 reads");
        assert!(bug.next.is_none(), "a bug has no place in the work order: {:?}", bug.next);
    }

    #[test]
    fn a_done_task_reports_the_commit_that_closed_it() {
        let done = Task {
            id: "S13".to_owned(),
            section: "Done".to_owned(),
            status: TaskStatus::Done,
            text: "a task. `[x]` `7df9401`".to_owned(),
            lead: "a task".to_owned(),
            ..task()
        };
        let view = view_from(&summary(vec![done]), "S13", None, None, None, None, &FakeGit).expect("S13 reads");
        let closed = view.closed.expect("the entry names a commit");
        assert_eq!(closed.hash, "7df9401");
        assert_eq!(closed.date.as_deref(), Some("2026-11-10"));
        assert_eq!(closed.subject.as_deref(), Some("a commit 7df9401"));
    }

    #[test]
    fn an_open_task_is_not_closed() {
        let view = view_from(
            &summary(vec![in_section("S14", "Now", TaskStatus::Open)]),
            "S14",
            None,
            None,
            None,
            None,
            &FakeGit,
        )
        .expect("S14 reads");
        assert!(view.closed.is_none());
    }

    fn task() -> Task {
        Task {
            id: "S14".to_owned(),
            status: TaskStatus::Doing,
            section: "Now".to_owned(),
            text: "`panel_cmds.rs` gives up its banding commands".to_owned(),
            lead: "`panel_cmds.rs` gives up its banding commands".to_owned(),
            fields: vec![Field { label: "Why".to_owned(), body: "a reason".to_owned() }],
            files: Some("crates/engine/src/panel_cmds.rs".to_owned()),
            paths: vec!["crates/engine/src/panel_cmds.rs".to_owned()],
        }
    }

    #[test]
    fn an_entry_touches_the_task_that_names_its_file_crate_or_stem() {
        let task = task();
        assert!(touches("`crates/engine/src/panel_cmds.rs` has no test module", &task));
        assert!(touches("`[engine]` the registry is a table", &task), "the crate tag is enough");
        assert!(touches("the panel_cmds surface is wide", &task), "the file stem is enough");
        assert!(touches("crates/engine holds it", &task), "the crate directory is enough");
        assert!(!touches("[render] the frame has no tone mapper", &task));
        assert!(!touches("[types] an engine command or report", &task), "a bare crate name is a word, not a tag");
        assert!(!touches("[panel] banding offsets", &task), "a short stem is not a needle");
    }

    #[test]
    fn a_directory_hint_meets_a_file_row() {
        assert!(same("crates/ui/src", "crates/ui/src/lib.rs"));
        assert!(same("crates/ui/src/lib.rs", "crates/ui/src/lib.rs"));
        assert!(!same("crates/ui/src/lib.rs", "crates/ui/src/object_card.rs"));
        assert!(!same("crates/engine", "crates/ui/src/lib.rs"));
    }

    #[test]
    fn the_entries_of_a_task_are_selected_unless_all_was_asked_for() {
        let entries = vec!["[engine] one".to_owned(), "[render] two".to_owned()];
        let task = task();
        assert_eq!(select(&entries, &task, false), ["[engine] one"]);
        assert_eq!(select(&entries, &task, true), entries);
    }
}
