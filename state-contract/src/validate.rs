//! The rules: every check that compares the three files against each other.
//!
//! Everything here is pure: text in, violations out. The filesystem and git live in [`crate::check`].

use crate::parse::{STATE_PREFIX, backlog_entries, parse_backlog, parse_state, parse_task_line, session_field};
use crate::{Summary, Task, TaskStatus};

/// Longest accepted `plan/STATUS.md`, in lines.
pub const MAX_STATUS_LINES: usize = 70;
/// Longest accepted `plan/SESSION.md`, in lines.
pub const MAX_SESSION_LINES: usize = 50;
/// The section a session starts from.
const NOW_SECTION: &str = "Now";
/// The section that holds finished tasks.
const DONE_SECTION: &str = "Done";
/// The one place development state may live.
pub const README_RULE: &str = "README must not carry development status; it lives in plan/STATUS.md";

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

/// A `README.md` that carries a status line or a milestone is a second source of truth, and the
/// stale one. `plan/STATUS.md` is the only place development state lives.
pub fn readme_violations(readme: &str) -> Vec<String> {
    let mut violations = Vec::new();
    if readme.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("**Status:**") || line.starts_with("Status:")
    }) {
        violations.push(README_RULE.to_owned());
    }
    if let Some(token) = milestone(readme) {
        violations.push(format!("{README_RULE}: it names the milestone `{token}`"));
    }
    violations
}

/// The first `M<digit>` milestone token in `text`, ignoring the inside of a longer word.
fn milestone(text: &str) -> Option<&str> {
    text.match_indices('M').find_map(|(offset, _)| {
        let rest = &text[offset + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let preceded_by_word = text[..offset].chars().next_back().is_some_and(|ch| ch.is_alphanumeric());
        (digits > 0 && !preceded_by_word).then(|| &text[offset..offset + 1 + digits])
    })
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

/// The `base=` violation for the facts gathered from git.
///
/// `HEAD` is a legal `base`: the plan edit is committed on top of it while it is still uncommitted.
pub(crate) fn base_violation(base: &str, head: Option<&str>, exists: bool, is_ancestor: bool) -> Option<String> {
    if !exists {
        return Some(format!("`STATE: base={base}` is not a commit in this repository"));
    }
    let Some(head) = head else {
        return Some("cannot resolve `HEAD`: the plan check needs a git checkout".to_owned());
    };
    (!is_ancestor).then(|| {
        format!(
            "`STATE: base={base}` is not an ancestor of `HEAD` ({head}); point `base` at a commit the plan describes"
        )
    })
}

/// Duplicates, sections and an id that is not a plain label.
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
    fn the_base_must_be_a_commit_that_leads_to_head() {
        assert!(base_violation("HEAD", Some("db8374d"), true, true).is_none(), "`HEAD` is a legal base");
        let stale = base_violation("ea3153a", Some("db8374d"), true, false).expect("a side commit must fail");
        assert!(stale.contains("not an ancestor"), "{stale}");
        let unknown = base_violation("nope", Some("db8374d"), false, false).expect("an unknown commit must fail");
        assert!(unknown.contains("not a commit"), "{unknown}");
    }

    #[test]
    fn a_readme_may_describe_the_product_but_not_the_state() {
        let clean = "# FurniCraft\n\nA native editor. Build it with `cargo run -p furnicraft`.\n\
                     The state lives in `plan/STATUS.md`.\n";
        assert!(readme_violations(clean).is_empty(), "a product README is fine");

        for line in ["**Status:** M0 — scaffold", "Status: early"] {
            let readme = format!("# Project\n\n{line}\n");
            let violations = readme_violations(&readme);
            assert!(violations.iter().any(|violation| violation == README_RULE), "{violations:?}");
        }

        let milestone = readme_violations("# Project\n\nWe are at M7 now.\n");
        assert!(milestone.iter().any(|violation| violation.starts_with(README_RULE)), "{milestone:?}");
        assert!(milestone.iter().any(|violation| violation.contains("`M7`")), "{milestone:?}");
    }

    #[test]
    fn a_milestone_inside_a_longer_word_is_not_a_status() {
        assert!(readme_violations("An AMD64 build and a 16M2 room.\n").is_empty());
        assert_eq!(milestone("phase M10 starts"), Some("M10"));
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
    fn the_summary_holds_the_current_task_and_the_next_open_ones() {
        let summary = analyze(HEADER, backlog(), None).expect("the plan is consistent");
        assert_eq!(summary.state.milestone, "M7");
        assert_eq!(summary.tasks.iter().filter(|task| task.status == TaskStatus::Open).count(), 2);
    }

    #[test]
    fn the_state_may_not_point_at_a_finished_task() {
        let error = analyze(&HEADER.replace("task=A3", "task=A2"), backlog(), None)
            .expect_err("a finished task cannot be the current one");
        assert!(error.iter().any(|violation| violation.contains("is done")), "{error:?}");
    }
}
