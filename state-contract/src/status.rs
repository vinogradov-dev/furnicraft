//! Rendering the block a session starts from.

use std::fmt::Write as _;

use crate::Summary;

/// The session-start block: the state line, the current task and the next three.
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
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, Task, TaskStatus};

    #[test]
    fn the_block_shows_the_current_task_and_the_next_three() {
        let summary = Summary {
            state: State {
                milestone: "M7".to_owned(),
                phase: None,
                task: "A3".to_owned(),
                status: "idle".to_owned(),
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
        };
        let text = render(&summary);
        assert!(text.starts_with("plan: M7 - · task A3 · idle · base abc1234"), "{text}");
        assert!(text.contains("-> [ ] A3 · arcs in the card  [files: crates/ui/src/object_card.rs]"), "{text}");
        assert!(text.contains("   [ ] A4 · stable ids\n"), "{text}");
    }
}
