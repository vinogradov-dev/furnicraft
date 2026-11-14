//! Rendering the block a session starts from.

use std::fmt::Write as _;

use crate::{NOTHING_SCHEDULED, Session, Summary, TaskStatus};

/// The session-start block: the state line, the head commit, the session note while one runs, and
/// the current task with the next three.
///
/// The caller prints it; the crate does not write to stdout, so a host can put it in a log, a
/// diagnostic or a test.
pub fn render(summary: &Summary) -> String {
    render_with_detail(summary, None)
}

/// [`render`] with the command a host points at for the full task.
///
/// The crate has no opinion about the host's command line, so the hint is passed in; `None` prints
/// no hint, which is what a project without a task command wants.
pub fn render_with_detail(summary: &Summary, detail: Option<&str>) -> String {
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
    if summary.interrupted {
        // The one fact `AGENTS.md` §1.1 otherwise asks the reader to derive from `base`, `HEAD` and
        // the note's steps.
        text.push_str("  interrupted: yes\n");
    }
    let scheduled = state.current().is_some();
    if !scheduled {
        // The order is empty: saying so is the one fact the reader needs, and no parked task is
        // marked with the arrow that means "work on this".
        let _ = writeln!(text, "-> {NOTHING_SCHEDULED}: `## Now` is empty, the backlog keeps the work");
    }
    let current = state.current().and_then(|id| summary.tasks.iter().position(|task| task.id == id));
    let from = current.unwrap_or(0);
    let upcoming = summary.tasks.iter().skip(from).filter(|task| task.status != TaskStatus::Done);
    // Four entries either way: the block is one screen, and with an empty order the reader sees the
    // sentence and the parked work in the same glance.
    for (offset, task) in upcoming.take(4).enumerate() {
        let arrow = if scheduled && offset == 0 { "->" } else { "  " };
        // The lead sentence, not the whole entry: the fields are one command away.
        let _ = write!(text, "{arrow} {} {} · {}", task.status.marker(), task.id, task.lead);
        if let Some(files) = &task.files {
            let _ = write!(text, "  [files: {files}]");
        }
        text.push('\n');
        if offset == 0
            && let Some(detail) = detail
        {
            let _ = writeln!(text, "   detail: {detail} {}", task.id);
        }
    }
    text
}

/// The seven-character form the plan uses for every commit, so `head` and `base` read alike.
fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

/// The fields a fresh session resumes from; an absent one prints as `-`.
fn render_session(text: &mut String, session: &Session) {
    for (key, value) in [
        ("task", &session.task),
        ("goal", &session.goal),
        ("now", &session.now),
        ("blocked", &session.blocked),
        ("run", &session.run),
        ("resume", &session.resume),
    ] {
        let _ = writeln!(text, "  {key}: {}", value.as_deref().unwrap_or("-"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Field, State, Task, TaskStatus};

    /// One backlog task as the parser would hand it over.
    fn task(id: &str, status: TaskStatus, section: &str, lead: &str, files: Option<&str>) -> Task {
        Task {
            id: id.to_owned(),
            status,
            section: section.to_owned(),
            text: lead.to_owned(),
            lead: lead.to_owned(),
            fields: vec![Field { label: "Why".to_owned(), body: "because".to_owned() }],
            files: files.map(str::to_owned),
            paths: files.map(|files| vec![files.to_owned()]).unwrap_or_default(),
        }
    }

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
                task("A3", TaskStatus::Open, "Now", "arcs in the card", Some("crates/ui/src/object_card.rs")),
                task("A4", TaskStatus::Open, "Next", "stable ids", None),
            ],
            session,
            head: head.map(str::to_owned),
            interrupted: false,
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
        list.tasks.push(task("A5", TaskStatus::Done, "Now", "already shipped", None));
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
    fn the_block_prints_the_lead_and_not_the_whole_entry() {
        let mut list = summary("idle", Some("fedcba9"), None);
        list.tasks[0].lead = "a short lead".to_owned();
        list.tasks[0].text = "a short lead Why: the whole entry, far too long for this block".to_owned();
        let text = render(&list);
        assert!(text.contains("-> [ ] A3 · a short lead"), "{text}");
        assert!(!text.contains("far too long"), "the fields are one command away: {text}");
    }

    #[test]
    fn a_detail_hint_follows_the_current_task_only() {
        let text = render_with_detail(&summary("idle", Some("fedcba9"), None), Some("cargo xtask task"));
        assert!(text.contains("   detail: cargo xtask task A3\n"), "{text}");
        assert!(!text.contains("cargo xtask task A4"), "only the current task is pointed at: {text}");
    }

    #[test]
    fn a_block_without_a_hint_names_no_command() {
        let text = render(&summary("idle", Some("fedcba9"), None));
        assert!(!text.contains("detail:"), "a host without a task command prints no hint: {text}");
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
    fn a_session_note_prints_every_field_a_session_resumes_from() {
        let text = render(&summary("doing", Some("fedcba9"), Some(note())));
        for line in [
            "  task: A3\n",
            "  goal: arcs in the card\n",
            "  now: the arc arm\n",
            "  blocked: nothing\n",
            "  run: cargo test -p furnicraft-ui\n",
            "  resume: finish the arc arm\n",
        ] {
            assert!(text.contains(line), "the block must carry `{line}`: {text}");
        }
    }

    #[test]
    fn an_interrupted_session_says_so() {
        let mut list = summary("doing", Some("fedcba9"), Some(note()));
        list.interrupted = true;
        let text = render(&list);
        assert!(text.contains("  interrupted: yes\n"), "the flag the reader would derive is printed: {text}");
        assert!(!render(&summary("idle", Some("fedcba9"), None)).contains("interrupted"), "an idle plan says nothing");
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
