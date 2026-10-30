//! The git facts the gate needs, behind a trait so tests need no repository.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// What the gate needs to know about the repository a plan lives in.
///
/// A host that already resolved these facts — or a test — implements this instead of running git.
pub trait Git {
    /// The `HEAD` commit, when the root is a checkout.
    fn head(&self) -> Option<String>;
    /// Whether `hash` names a commit; `HEAD` is accepted as a literal.
    fn commit_exists(&self, hash: &str) -> bool;
    /// Whether `ancestor` is reachable from `descendant`.
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool;
}

/// [`Git`] implemented by running the `git` binary in a directory.
#[derive(Clone, Debug)]
pub struct ProcessGit {
    root: PathBuf,
}

impl ProcessGit {
    /// Runs git with `root` as its working directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The trimmed stdout of a successful `git` call.
    fn output(&self, args: &[&str]) -> Option<String> {
        let output = Command::new("git").args(args).current_dir(&self.root).stdin(Stdio::null()).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Whether `git` exited successfully.
    fn success(&self, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

impl Git for ProcessGit {
    fn head(&self) -> Option<String> {
        self.output(&["rev-parse", "HEAD"]).filter(|hash| !hash.is_empty())
    }

    fn commit_exists(&self, hash: &str) -> bool {
        if hash == "HEAD" {
            return self.head().is_some();
        }
        if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
        self.success(&["cat-file", "-e", &format!("{hash}^{{commit}}")])
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.success(&["merge-base", "--is-ancestor", ancestor, descendant])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory outside any checkout.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("state-contract-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch directory is writable");
        root
    }

    #[test]
    fn an_empty_directory_is_not_a_repository() {
        let root = scratch("empty");
        let git = ProcessGit::new(&root);
        assert_eq!(git.head(), None, "`/tmp/...` is not a checkout");
        assert!(!git.commit_exists("HEAD"));
        assert!(!git.commit_exists("0123456"));
        assert!(!git.commit_exists("not a hash"));
        assert!(!git.is_ancestor("HEAD", "HEAD"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
