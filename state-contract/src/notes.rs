//! Per-task notes: `plan/notes/<ID>.md`.
//!
//! A note is the working memory of one task — what a session measured, which file and line it read,
//! which road it ruled out. It lives beside the entry so a task report can print the plan and the
//! research together, and so the next session finds it without a grep.
//!
//! The note is not a second plan: it carries no status, no `files:` hint and no commit. It survives
//! the task, because "what the research ruled out" stays useful after the work lands; the outcome
//! itself belongs in the backlog entry, where the reader looks first.

use std::path::{Path, PathBuf};

use crate::Task;

/// Longest accepted note, in lines: a note is a working memory, not a report.
pub const MAX_NOTE_LINES: usize = 150;

/// The file a note for `id` lives in.
pub fn note_path(root: &Path, id: &str) -> PathBuf {
    root.join("plan").join("notes").join(format!("{id}.md"))
}

/// The note for `id`, or `None` when the task has none.
///
/// A missing `plan/notes/` directory is not an error: notes are optional for every task.
pub fn note(root: &Path, id: &str) -> Result<Option<String>, Vec<String>> {
    match std::fs::read_to_string(note_path(root, id)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(vec![format!("cannot read `plan/notes/{id}.md`: {error}")]),
    }
}

/// Every violation the notes directory carries: a name that is not `<task id>.md`, a note whose task
/// is not in the backlog, an empty note or one over [`MAX_NOTE_LINES`].
pub(crate) fn note_violations(root: &Path, tasks: &[Task]) -> Vec<String> {
    let directory = root.join("plan").join("notes");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => return vec![format!("cannot read `plan/notes/`: {error}")],
    };
    let mut violations = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".md") else {
            violations.push(format!("`plan/notes/{name}` is not a note: a note is named `<task id>.md`"));
            continue;
        };
        if !tasks.iter().any(|task| task.id == id) {
            violations.push(format!("`plan/notes/{name}` is named after {id}, which `plan/BACKLOG.md` does not have"));
        }
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        if text.trim().is_empty() {
            violations.push(format!(
                "`plan/notes/{name}` is empty; a note says what the research found, or it does not exist"
            ));
        } else if text.lines().count() > MAX_NOTE_LINES {
            violations.push(format!(
                "`plan/notes/{name}` has {} lines, over the {MAX_NOTE_LINES}-line budget",
                text.lines().count()
            ));
        }
    }
    // The directory order is the filesystem's; a report must not shuffle between runs.
    violations.sort();
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Field, Task, TaskStatus};

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: id.to_owned(),
            status,
            section: "Now".to_owned(),
            text: "a task".to_owned(),
            lead: "a task".to_owned(),
            fields: vec![Field { label: "Why".to_owned(), body: "a reason".to_owned() }],
            files: Some("crates/ui/src/lib.rs".to_owned()),
            paths: vec!["crates/ui/src/lib.rs".to_owned()],
        }
    }

    /// A scratch project with the given notes, keyed by file name.
    fn project(name: &str, notes: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("state-contract-notes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("plan/notes")).expect("the scratch directory is writable");
        for (file, text) in notes {
            std::fs::write(root.join("plan/notes").join(file), text).expect("the note is writable");
        }
        root
    }

    #[test]
    fn a_note_is_found_by_its_task_id() {
        let root = project("read", &[("A3.md", "# A3\n\nmeasured `object_card.rs`\n")]);
        let text = note(&root, "A3").expect("the note reads");
        assert!(text.as_deref().is_some_and(|text| text.contains("measured")));
        assert_eq!(note(&root, "A4").expect("a missing note is not an error"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_without_a_notes_directory_has_no_violations() {
        let root = std::env::temp_dir().join(format!("state-contract-notes-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("plan")).expect("the scratch directory is writable");
        assert!(note_violations(&root, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_note_for_a_live_task_passes() {
        let root = project("live", &[("A3.md", "a note\n")]);
        let violations = note_violations(&root, &[task("A3", TaskStatus::Doing)]);
        assert!(violations.is_empty(), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_note_outlives_its_finished_task_but_not_a_removed_one() {
        let root = project("stale", &[("A3.md", "a note\n"), ("A9.md", "another\n")]);
        let violations = note_violations(&root, &[task("A3", TaskStatus::Done)]);
        assert!(violations.iter().any(|v| v.contains("does not have") && v.contains("A9")), "{violations:?}");
        assert!(!violations.iter().any(|v| v.contains("A3")), "a finished task keeps its research: {violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_note_and_a_misnamed_file_are_reported() {
        let root = project("bad", &[("A3.md", "  \n\n"), ("notes.md.bak", "x\n")]);
        let empty = note_violations(&root, &[task("A3", TaskStatus::Open)]);
        assert!(empty.iter().any(|v| v.contains("is empty")), "{empty:?}");
        let _ = std::fs::remove_dir_all(&root);

        let root = project("named", &[("scratch.md", "x\n")]);
        let named = note_violations(&root, &[task("A3", TaskStatus::Open)]);
        assert!(named.iter().any(|v| v.contains("is named after scratch")), "{named:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_over_long_note_is_reported() {
        let long = "line\n".repeat(MAX_NOTE_LINES + 1);
        let root = project("long", &[("A3.md", &long)]);
        let violations = note_violations(&root, &[task("A3", TaskStatus::Open)]);
        assert!(violations.iter().any(|v| v.contains("line budget")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
