//! End-to-end checks on real directories.
//!
//! The git facts come from [`FakeGit`], except for one test that builds a throwaway repository and
//! runs the real binary through [`ProcessGit`].

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use state_contract::{Git, ProcessGit, TaskStatus, check, check_with, status_with};

/// The commit the fake repository calls `HEAD`.
const HEAD: &str = "fedcba9";

/// The git facts a plan needs, without a repository.
struct FakeGit {
    head: Option<&'static str>,
    commits: Vec<&'static str>,
    ancestors: Vec<(&'static str, &'static str)>,
    /// The author date `commit_date` answers with, when the fixture needs a specific one.
    base_date: Option<&'static str>,
    /// The paths `changed` answers with; `None` is a host that cannot compare two commits.
    changed: Option<Vec<String>>,
}

impl FakeGit {
    fn repository() -> Self {
        Self {
            head: Some(HEAD),
            commits: vec!["abc1234", "def5678"],
            ancestors: vec![("abc1234", HEAD), ("HEAD", HEAD)],
            base_date: Some("2025-12-01"),
            changed: None,
        }
    }
}

impl Git for FakeGit {
    fn head(&self) -> Option<String> {
        self.head.map(str::to_owned)
    }

    fn commit_exists(&self, hash: &str) -> bool {
        (hash == "HEAD" && self.head.is_some()) || self.commits.contains(&hash)
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.ancestors.iter().any(|(a, d)| *a == ancestor && *d == descendant)
    }

    fn changed(&self, _from: &str, _to: &str) -> Option<Vec<String>> {
        self.changed.clone()
    }

    fn commit_date(&self, hash: &str) -> Option<String> {
        // `abc1234` and `HEAD` were made before the fixtures' `updated=2026-01-01`.
        (self.commits.contains(&hash) || hash == "HEAD")
            .then_some(self.base_date.unwrap_or("2025-12-01"))
            .map(str::to_owned)
    }
}

/// A scratch plan with no git at all: the three plan files and one source file.
fn plan_dir(name: &str, status: &str, backlog: &str) -> PathBuf {
    let root = scratch(name);
    fs::create_dir_all(root.join("plan")).expect("the scratch plan is writable");
    fs::create_dir_all(root.join("src")).expect("the scratch sources are writable");
    fs::write(root.join("plan/STATUS.md"), status).expect("STATUS is writable");
    fs::write(root.join("plan/BACKLOG.md"), backlog).expect("BACKLOG is writable");
    fs::write(root.join("src/lib.rs"), "//! fixture\n").expect("the source is writable");
    root
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// A fresh directory outside any checkout, removed first if a previous run left it behind.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("state-contract-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the scratch directory is writable");
    root
}

/// A one-commit repository with a plan that names `src/lib.rs`; `None` when `git` cannot do it.
///
/// The commit carries a fixed author date so the plan's `updated=` stays comparable in any year.
fn scratch_repository(name: &str, status: &str, backlog: &str) -> Option<PathBuf> {
    let root = scratch(name);
    let git = |args: &[&str]| {
        Command::new("git").args(args).current_dir(&root).output().ok().filter(|output| output.status.success())
    };
    git(&["init", "-q"])?;
    Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ])
        .env("GIT_AUTHOR_DATE", "2025-12-01T00:00:00 +0000")
        .env("GIT_COMMITTER_DATE", "2025-12-01T00:00:00 +0000")
        .current_dir(&root)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    fs::create_dir_all(root.join("plan")).ok()?;
    fs::create_dir_all(root.join("src")).ok()?;
    fs::write(root.join("plan/STATUS.md"), status).ok()?;
    fs::write(root.join("plan/BACKLOG.md"), backlog).ok()?;
    fs::write(root.join("src/lib.rs"), "//! fixture\n").ok()?;
    Some(root)
}

const PLAN_BACKLOG: &str = "## Now\n\n- `[ ]` **T1** only `files: src/lib.rs`\n\n## Next\n\n## Done\n";

#[test]
fn a_consistent_plan_passes() {
    let summary = check_with(&fixture("good"), &FakeGit::repository()).expect("the plan is consistent");
    assert_eq!(summary.state.task, "T1");
    assert_eq!(summary.state.base, "abc1234");
    let ids: Vec<&str> = summary.tasks.iter().map(|task| task.id.as_str()).collect();
    assert_eq!(ids, ["T1", "T2", "T0"]);
    assert_eq!(summary.tasks[0].section, "Now");
    assert_eq!(summary.tasks[0].files.as_deref(), Some("src/lib.rs"));
    assert_eq!(summary.tasks[2].status, TaskStatus::Done);
}

#[test]
fn a_doing_plan_with_a_full_note_passes() {
    let summary = check_with(&fixture("good-note"), &FakeGit::repository()).expect("a full note is enough");
    assert_eq!(summary.state.status, "doing");
    assert_eq!(summary.tasks[0].status, TaskStatus::Doing);
}

#[test]
fn a_hint_that_names_no_file_fails() {
    let error = check_with(&fixture("dead-hint"), &FakeGit::repository()).expect_err("a dead hint must fail");
    assert!(error.iter().any(|violation| violation.contains("src/missing.rs")), "{error:?}");
    assert!(error.iter().any(|violation| violation.contains("does not exist")), "{error:?}");
}

#[test]
fn a_thin_session_note_fails() {
    let error = check_with(&fixture("thin-note"), &FakeGit::repository()).expect_err("a note with only a task fails");
    for key in ["goal", "now", "run"] {
        assert!(
            error.iter().any(|violation| violation.contains(&format!("`{key}:` line"))),
            "the error must name `{key}:`: {error:?}"
        );
    }
}

#[test]
fn an_updated_date_earlier_than_the_base_commit_fails() {
    let mut git = FakeGit::repository();
    git.base_date = Some("2026-02-01");
    let error = check_with(&fixture("good"), &git).expect_err("a plan older than its base must fail");
    assert!(error.iter().any(|violation| violation.contains("earlier than")), "{error:?}");
    assert!(error.iter().any(|violation| violation.contains("2026-02-01")), "{error:?}");

    let summary = check_with(&fixture("good"), &FakeGit::repository()).expect("a later date is fine");
    assert_eq!(summary.state.updated, "2026-01-01");
}

#[test]
fn an_updated_value_that_is_not_a_date_fails() {
    let root = scratch("bad-date");
    fs::create_dir_all(root.join("plan")).expect("plan dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("plan/STATUS.md"), "STATE: milestone=M1 task=T1 state=idle base=abc1234 updated=2026-1-1\n")
        .expect("STATUS.md");
    fs::write(root.join("plan/BACKLOG.md"), PLAN_BACKLOG).expect("BACKLOG.md");
    fs::write(root.join("src/lib.rs"), "//! fixture\n").expect("src/lib.rs");
    let error = check_with(&root, &FakeGit::repository()).expect_err("a malformed date must fail");
    assert!(error.iter().any(|violation| violation.contains("`YYYY-MM-DD`")), "{error:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_base_that_does_not_lead_to_head_fails() {
    let mut git = FakeGit::repository();
    git.ancestors.clear();
    let error = check_with(&fixture("good"), &git).expect_err("a side commit cannot be the base");
    assert!(error.iter().any(|violation| violation.contains("not an ancestor")), "{error:?}");
}

#[test]
fn head_is_a_legal_base() {
    let summary = check_with(&fixture("head-base"), &FakeGit::repository()).expect("`HEAD` is a legal base");
    assert_eq!(summary.state.base, "HEAD");
}

#[test]
fn the_status_block_names_the_current_task_and_its_files() {
    let text = status_with(&fixture("good"), &FakeGit::repository()).expect("a consistent plan renders");
    assert!(text.contains("task T1"), "{text}");
    assert!(text.contains("-> [ ] T1 · first task"), "{text}");
    assert!(text.contains("[files: src/lib.rs]"), "{text}");
    assert!(text.contains("head: fedcba9\n"), "the block names the head commit: {text}");
}

#[test]
fn the_status_block_carries_every_field_a_session_resumes_from() {
    let text = status_with(&fixture("good-note"), &FakeGit::repository()).expect("a consistent plan renders");
    for line in [
        "  task: T1\n",
        "  goal: the first user-visible result\n",
        "  now: the parser arm in `src/lib.rs`\n",
        "  blocked: nothing\n",
        "  run: cargo test -p state-contract\n",
        "  resume: finish the parser arm",
    ] {
        assert!(text.contains(line), "the block must carry `{line}`: {text}");
    }
    assert!(text.contains("  interrupted: yes\n"), "the note has an open step and `HEAD` is past `base`: {text}");
}

/// A plan whose work order is empty: `task=-`, and `## Next` says so in words.
fn empty_order_plan(name: &str) -> PathBuf {
    let status = "STATE: milestone=M1 phase=K task=- state=idle base=abc1234 updated=2026-01-01\n\n\
                  ## Next\n\nNothing is scheduled: 1 open task waits in `## Next`.\n";
    let backlog = "## Now\n\n## Bugs\n\n## Next\n\n\
                   - `[ ]` **K2** the package writer\n  `files: src/lib.rs`\n\n## Done\n";
    plan_dir(name, status, backlog)
}

#[test]
fn an_empty_work_order_is_a_valid_plan() {
    let root = empty_order_plan("empty-order");
    let summary = check_with(&root, &FakeGit::repository()).expect("nothing scheduled is a legal state");
    assert_eq!(summary.state.task, "-");
    assert_eq!(summary.state.current(), None);
}

#[test]
fn the_status_block_says_nothing_is_scheduled_and_marks_no_parked_task() {
    let text = status_with(&empty_order_plan("empty-order-status"), &FakeGit::repository())
        .expect("a consistent plan renders");
    assert!(text.contains("task - · idle"), "{text}");
    assert!(text.contains("-> Nothing is scheduled"), "the block says it in words: {text}");
    assert!(!text.contains("-> [ ] K2"), "a parked task is not marked as the work in hand: {text}");
    assert!(text.contains("   [ ] K2 · the package writer"), "the parked work is still listed: {text}");
}

#[test]
fn a_task_line_under_an_empty_order_is_a_violation() {
    let root = empty_order_plan("empty-order-task");
    let backlog = fs::read_to_string(root.join("plan/BACKLOG.md")).expect("the backlog is readable");
    fs::write(
        root.join("plan/BACKLOG.md"),
        backlog.replace("## Now\n\n", "## Now\n\n- `[ ]` **K2** the writer\n  `files: src/lib.rs`\n\n"),
    )
    .expect("the backlog is writable");
    let error = check_with(&root, &FakeGit::repository()).expect_err("`task=-` cannot hold a task");
    assert!(error.iter().any(|violation| violation.contains("still holds K2")), "{error:?}");
}

#[test]
fn an_over_long_status_file_fails() {
    let root = scratch("long");
    fs::create_dir_all(root.join("plan")).expect("plan dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    let mut status = "STATE: milestone=M1 task=T1 state=idle base=abc1234 updated=2026-01-01\n".to_owned();
    for _ in 0..80 {
        status.push_str("filler\n");
    }
    fs::write(root.join("plan/STATUS.md"), &status).expect("STATUS.md");
    fs::write(root.join("plan/BACKLOG.md"), PLAN_BACKLOG).expect("BACKLOG.md");
    fs::write(root.join("src/lib.rs"), "//! fixture\n").expect("src/lib.rs");
    let error = check_with(&root, &FakeGit::repository()).expect_err("the budget is an error");
    assert!(error.iter().any(|violation| violation.contains("over the 70-line budget")), "{error:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_state_naming_a_task_behind_the_first_open_one_fails() {
    let backlog = "## Now\n\n\
                   - `[ ]` **T1** first `files: src/lib.rs`\n\
                   - `[ ]` **T2** later `files: src/lib.rs`\n\n\
                   ## Next\n\n## Done\n";
    let root = scratch("skipped");
    fs::create_dir_all(root.join("plan")).expect("plan dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("plan/STATUS.md"), "STATE: milestone=M1 task=T2 state=idle base=abc1234 updated=2026-01-01\n")
        .expect("STATUS.md");
    fs::write(root.join("plan/BACKLOG.md"), backlog).expect("BACKLOG.md");
    fs::write(root.join("src/lib.rs"), "//! fixture\n").expect("src/lib.rs");
    let error = check_with(&root, &FakeGit::repository()).expect_err("a skipped task must fail");
    assert!(error.iter().any(|violation| violation.contains("is not the first unfinished task")), "{error:?}");
    assert!(error.iter().any(|violation| violation.contains("`T1` comes before it")), "{error:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_readme_without_a_status_passes_and_one_with_it_fails() {
    let summary = check_with(&fixture("readme-clean"), &FakeGit::repository()).expect("a product README is fine");
    assert_eq!(summary.state.task, "T1");

    let error = check_with(&fixture("readme-status"), &FakeGit::repository())
        .expect_err("a README carrying the development status must fail");
    assert!(error.iter().any(|violation| violation.contains("must not carry development status")), "{error:?}");
    assert!(error.iter().any(|violation| violation.contains("`M7`")), "the milestone is named: {error:?}");
}

#[test]
fn a_fixture_without_a_readme_still_passes() {
    check_with(&fixture("good"), &FakeGit::repository()).expect("a README is optional");
}

#[test]
fn head_is_a_legal_base_in_a_real_repository() {
    let status = "STATE: milestone=M1 phase=A task=T1 state=idle base=HEAD updated=2026-01-01\n";
    let Some(root) = scratch_repository("head", status, PLAN_BACKLOG) else {
        // No usable `git` binary: the fake covers the same rule above.
        return;
    };
    let summary = check(&root).expect("`HEAD` resolves in a real repository");
    assert_eq!(summary.state.base, "HEAD");
    assert!(ProcessGit::new(&root).head().is_some(), "the scratch repository has a commit");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_note_for_a_task_the_backlog_has_is_a_valid_plan() {
    let summary = check_with(&fixture("good"), &FakeGit::repository()).expect("a note is not a violation");
    assert_eq!(summary.tasks.len(), 3, "the note adds no task");

    let error = check_with(&fixture("notes-unknown"), &FakeGit::repository())
        .expect_err("a note for an unknown task must fail");
    assert!(error.iter().any(|violation| violation.contains("does not have")), "{error:?}");

    let error = check_with(&fixture("notes-empty"), &FakeGit::repository()).expect_err("an empty note must fail");
    assert!(error.iter().any(|violation| violation.contains("is empty")), "{error:?}");
}

#[test]
fn the_task_report_carries_the_entry_the_note_and_the_paths() {
    use state_contract::{TaskOptions, task_view_with};

    let view = task_view_with(&fixture("good"), "T1", &FakeGit::repository()).expect("the task reads");
    assert_eq!(view.task.id, "T1");
    assert_eq!(view.task.paths, ["src/lib.rs"], "the hint is split into paths");
    assert_eq!(view.task.lead, "first task");
    assert!(view.notes.as_deref().is_some_and(|note| note.contains("road ruled out")), "{:?}", view.notes);
    assert_eq!(view.next.as_ref().map(|task| task.id.as_str()), Some("T2"), "the next task in file order");
    assert!(view.session.is_none(), "the plan is idle");

    let error = task_view_with(&fixture("good"), "T9", &FakeGit::repository()).expect_err("T9 does not exist");
    assert!(error.iter().any(|violation| violation.contains("has no task `T9`")), "{error:?}");

    let text = state_contract::task_with(&fixture("good"), "T1", &TaskOptions::default(), &FakeGit::repository())
        .expect("the task renders");
    assert!(text.contains("TASK ─ plan/BACKLOG.md"), "{text}");
    assert!(text.contains("NOTES ─ plan/notes/T1.md"), "{text}");
    assert!(text.contains("road ruled out"), "the note is printed in full: {text}");
    assert!(text.contains("src/lib.rs"), "{text}");
}

#[test]
fn the_dashboard_expands_one_task_when_asked() {
    use state_contract::{DashboardOptions, dashboard_with};

    let opts = DashboardOptions { task: Some("T1".to_owned()), width: 80, color: false };
    let text = dashboard_with(&fixture("good"), &opts, &FakeGit::repository()).expect("the dashboard renders");
    let task = text.find("TASK ─ plan/BACKLOG.md").expect("the task block is there");
    let now = text.find("NOW ─ plan/BACKLOG.md").expect("the backlog blocks stay");
    assert!(task < now, "the task block sits above the backlog sections: {text}");
    assert!(text.contains("NOTES ─ plan/notes/T1.md"), "{text}");

    let opts = DashboardOptions { task: Some("T9".to_owned()), width: 80, color: false };
    let error = dashboard_with(&fixture("good"), &opts, &FakeGit::repository())
        .expect_err("a task the backlog does not have is an error");
    assert!(error.iter().any(|violation| violation.contains("has no task `T9`")), "{error:?}");
}

/// The `STATE:` line every scratch plan writes: idle, based on `abc1234`.
const STATUS_T1: &str = "STATE: milestone=M1 phase=P task=T1 state=idle base=abc1234 updated=2026-11-10\n";

#[test]
fn an_idle_plan_whose_base_lags_the_code_fails() {
    let root = plan_dir("base-stale", STATUS_T1, PLAN_BACKLOG);
    let moved = FakeGit { changed: Some(vec!["crates/x/src/lib.rs".to_owned()]), ..FakeGit::repository() };
    let error = check_with(&root, &moved).expect_err("an idle plan describes the checkout");
    assert!(error.iter().any(|violation| violation.contains("is not the tree this plan describes")), "{error:?}");
    assert!(error.iter().any(|violation| violation.contains("crates/x/src/lib.rs")), "{error:?}");

    // A plan edit that rides on top of `base` touches `plan/` only, so that base is legal.
    let planned = FakeGit { changed: Some(vec!["plan/BACKLOG.md".to_owned()]), ..FakeGit::repository() };
    check_with(&root, &planned).expect("a plan-only commit is what `base` may lag by");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_doing_plan_may_leave_its_base_behind() {
    let error = check_with(&fixture("good-note"), &FakeGit::repository()).expect("a doing plan resumes");
    assert_eq!(error.state.status, "doing");
    let running = FakeGit { changed: Some(vec!["crates/x/src/lib.rs".to_owned()]), ..FakeGit::repository() };
    let summary = check_with(&fixture("good-note"), &running).expect("work in progress moves ahead of the plan");
    assert_eq!(summary.state.status, "doing");
}

#[test]
fn a_closed_task_citing_a_missing_decision_fails() {
    let backlog = "## Now\n\n- `[ ]` **T1** only `files: src/lib.rs`\n\n## Done\n\n\
                   - `[x]` **T0** it landed (ADR 0099) — `abc1234`\n";
    let root = plan_dir("adr-cite", STATUS_T1, backlog);
    fs::create_dir_all(root.join("docs/decisions")).expect("the decisions are writable");
    fs::write(root.join("docs/decisions/0001-a-decision.md"), "# A decision\n").expect("a decision is writable");
    let error = check_with(&root, &FakeGit::repository()).expect_err("a closed citation must resolve");
    assert!(error.iter().any(|violation| violation.contains("ADR 0099")), "{error:?}");

    fs::write(root.join("docs/decisions/0099-a-decision.md"), "# A decision\n").expect("the decision is writable");
    check_with(&root, &FakeGit::repository()).expect("the record is whole");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_reference_file_may_not_carry_plan_state() {
    let root = plan_dir("reference-state", STATUS_T1, PLAN_BACKLOG);
    let questions = root.join("plan/questions.md");
    fs::write(&questions, "# Open questions\n\n- `[ ]` a question\n").expect("questions are writable");
    let error = check_with(&root, &FakeGit::repository()).expect_err("a marker in `questions.md` must fail");
    assert!(error.iter().any(|violation| violation.contains("questions.md")), "{error:?}");

    fs::write(&questions, "# Open questions\n\nSTATE: task=T1\n").expect("questions are writable");
    let error = check_with(&root, &FakeGit::repository()).expect_err("a state line in `questions.md` must fail");
    assert!(error.iter().any(|violation| violation.contains("the state lives in")), "{error:?}");

    fs::write(&questions, "# Open questions\n\n- a question\n").expect("questions are writable");
    check_with(&root, &FakeGit::repository()).expect("a reference file passes the gate");
    let _ = fs::remove_dir_all(&root);
}
