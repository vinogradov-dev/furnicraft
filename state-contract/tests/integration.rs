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
}

impl FakeGit {
    fn repository() -> Self {
        Self {
            head: Some(HEAD),
            commits: vec!["abc1234", "def5678"],
            ancestors: vec![("abc1234", HEAD), ("HEAD", HEAD)],
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
fn scratch_repository(name: &str, status: &str, backlog: &str) -> Option<PathBuf> {
    let root = scratch(name);
    let git = |args: &[&str]| {
        Command::new("git").args(args).current_dir(&root).output().ok().filter(|output| output.status.success())
    };
    git(&["init", "-q"])?;
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ])?;
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
