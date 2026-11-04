//! Rendering the block a session starts from.

use std::fmt::Write as _;

use crate::{Session, Summary, TaskStatus};

/// The session-start block: the state line, the head commit, the session note while one runs, and
/// the current task with the next three.
///
/// The caller prints it; the crate does not write to stdout, so a host can put it in a log, a
/// diagnostic or a test.
pub fn render(summary: &Summary) -> String {
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
    let _ = writeln!(text, "head: {}", summary.head.as_deref().map_or("(no checkout)", short));
    if let Some(session) = &summary.session {
        render_session(&mut text, session);
    }
    let current = summary.tasks.iter().position(|task| task.id == state.task);
    let from = current.unwrap_or(0);
    let upcoming = summary.tasks.iter().skip(from).filter(|task| task.status != TaskStatus::Done);
    for (offset, task) in upcoming.take(4).enumerate() {
        let arrow = if offset == 0 { "->" } else { "  " };
        let _ = write!(text, "{arrow} {} {} · {}", task.status.marker(), task.id, task.text);
        if let Some(files) = &task.files {
            let _ = write!(text, "  [files: {files}]");
        }
        text.push('\n');
    }
    text
}

/// The seven-character form the plan uses for every commit, so `head` and `base` read alike.
fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

/// The four fields a fresh session resumes from; an absent one prints as `-`.
fn render_session(text: &mut String, session: &Session) {
    for (key, value) in
        [("task", &session.task), ("now", &session.now), ("blocked", &session.blocked), ("run", &session.run)]
    {
        let _ = writeln!(text, "  {key}: {}", value.as_deref().unwrap_or("-"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, Task, TaskStatus};

    fn summary(status: &str, head: Option<&str>, session: Option<Session>) -> Summary {
        Summary {
            state: State {
                milestone: "M7".to_owned(),
                phase: None,
                task: "A3".to_owned(),
                status: status.to_owned(),
                base: "abc1234".to_owned(),
                updated: "2026-10-30".to_owned(),
            },
            tasks: vec![
                Task {
                    id: "A3".to_owned(),
                    status: TaskStatus::Open,
                    section: "Now".to_owned(),
                    text: "arcs in the card".to_owned(),
                    files: Some("crates/ui/src/object_card.rs".to_owned()),
                },
                Task {
                    id: "A4".to_owned(),
                    status: TaskStatus::Open,
                    section: "Next".to_owned(),
                    text: "stable ids".to_owned(),
                    files: None,
                },
            ],
            session,
            head: head.map(str::to_owned),
        }
    }

    /// A note that carries every field the block prints.
    fn note() -> Session {
        Session {
            task: Some("A3".to_owned()),
            goal: Some("arcs in the card".to_owned()),
            now: Some("the arc arm".to_owned()),
            blocked: Some("nothing".to_owned()),
            run: Some("cargo test -p furnicraft-ui".to_owned()),
            resume: Some("finish the arc arm".to_owned()),
        }
    }

    #[test]
    fn the_block_skips_finished_tasks() {
        let mut list = summary("idle", Some("fedcba9"), None);
        list.tasks.push(Task {
            id: "A5".to_owned(),
            status: TaskStatus::Done,
            section: "Now".to_owned(),
            text: "already shipped".to_owned(),
            files: None,
        });
        let text = render(&list);
        assert!(!text.contains("A5"), "a `[x]` task is not part of what comes next: {text}");
        assert!(text.contains("-> [ ] A3"), "the current task stays first: {text}");
    }

    #[test]
    fn the_block_shows_the_current_task_and_the_next_three() {
        let text = render(&summary("idle", Some("fedcba9"), None));
        assert!(text.starts_with("plan: M7 - · task A3 · idle · base abc1234"), "{text}");
        assert!(text.contains("-> [ ] A3 · arcs in the card  [files: crates/ui/src/object_card.rs]"), "{text}");
        assert!(text.contains("   [ ] A4 · stable ids\n"), "{text}");
    }

    #[test]
    fn the_block_names_the_head_commit() {
        let text = render(&summary("idle", Some("fedcba9c0ffee"), None));
        assert!(text.contains("head: fedcba9\n"), "the head prints in the plan's short form: {text}");
        let short = render(&summary("idle", Some("abc"), None));
        assert!(short.contains("head: abc\n"), "a short hash is printed as it is: {short}");
    }

    #[test]
    fn outside_a_checkout_the_head_line_says_so() {
        let text = render(&summary("idle", None, None));
        assert!(text.contains("head: (no checkout)\n"), "{text}");
    }

    #[test]
    fn an_idle_plan_prints_no_session_block() {
        let text = render(&summary("idle", Some("fedcba9"), None));
        assert!(!text.contains("  task: "), "an idle plan has no note to resume: {text}");
    }

    #[test]
    fn a_session_note_prints_the_four_resume_fields() {
        let text = render(&summary("doing", Some("fedcba9"), Some(note())));
        for line in
            ["  task: A3\n", "  now: the arc arm\n", "  blocked: nothing\n", "  run: cargo test -p furnicraft-ui\n"]
        {
            assert!(text.contains(line), "the block must carry `{line}`: {text}");
        }
        assert!(!text.contains("finish the arc arm"), "`resume:` is not one of the four: {text}");
    }

    #[test]
    fn an_empty_session_field_prints_as_a_dash() {
        let note = Session { task: Some("A3".to_owned()), ..Session::default() };
        let text = render(&summary("doing", Some("fedcba9"), Some(note)));
        for line in ["  now: -\n", "  blocked: -\n", "  run: -\n"] {
            assert!(text.contains(line), "an empty field is still named: {text}");
        }
    }
}
