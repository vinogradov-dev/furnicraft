//! The decision index: `docs/decisions/README.md` must list every record and nothing else.
//!
//! The index is how a session finds a decision it did not know it needed; a record that is not in the
//! table is invisible, and a row that points at a missing file sends the reader nowhere. Both are the
//! same rot the plan gate rejects for `plan/map.md`, so they are checked the same way.

use std::path::Path;

/// The number of decisions the index and the directory agree on, or every mismatch.
pub fn check(root: &Path) -> Result<usize, Vec<String>> {
    let directory = root.join("docs/decisions");
    let entries =
        std::fs::read_dir(&directory).map_err(|error| vec![format!("cannot read `docs/decisions/`: {error}")])?;
    let mut files: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| number(&entry.file_name().to_string_lossy()))
        .collect();
    files.sort();
    let index = std::fs::read_to_string(directory.join("README.md"))
        .map_err(|error| vec![format!("cannot read `docs/decisions/README.md`: {error}")])?;
    let mut listed: Vec<String> = Vec::new();
    for (offset, _) in index.match_indices("](") {
        let rest = index.get(offset.saturating_add(2)..).unwrap_or_default();
        let target = rest.split([')', ' ']).next().unwrap_or_default();
        if let Some(number) = number(target)
            && !listed.contains(&number)
        {
            listed.push(number);
        }
    }
    let mut violations = Vec::new();
    for file in &files {
        if !listed.contains(file) {
            violations.push(format!("`docs/decisions/` has {file}, but `README.md` has no row for it"));
        }
    }
    for row in &listed {
        if !files.contains(row) {
            violations.push(format!("`docs/decisions/README.md` lists {row}, but no file carries that number"));
        }
    }
    if violations.is_empty() { Ok(files.len()) } else { Err(violations) }
}

/// The four-digit number a decision file or a link starts with.
fn number(name: &str) -> Option<String> {
    let digits: String = name.chars().take(4).collect();
    let rest = name.get(4..)?;
    (digits.len() == 4 && digits.chars().all(|ch| ch.is_ascii_digit()) && rest.starts_with('-')).then_some(digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch project with the given decision files and index rows.
    fn project(name: &str, files: &[&str], rows: &[&str]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/decisions")).expect("the scratch directory is writable");
        for file in files {
            std::fs::write(root.join("docs/decisions").join(file), "# a decision\n").expect("the file is writable");
        }
        let mut index = String::from("# Architecture decisions\n\n| # | Title | Status |\n|---|---|---|\n");
        for row in rows {
            index.push_str(&format!("| [{row}]({row}-a-decision.md) | a decision | Accepted |\n"));
        }
        std::fs::write(root.join("docs/decisions/README.md"), index).expect("the index is writable");
        root
    }

    #[test]
    fn an_index_that_matches_passes() {
        let root = project("ok", &["0001-a-decision.md", "0002-a-decision.md"], &["0001", "0002"]);
        assert_eq!(check(&root).expect("the index is complete"), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_record_without_a_row_and_a_row_without_a_record_are_both_reported() {
        let root = project("mismatch", &["0001-a-decision.md", "0007-a-decision.md"], &["0001", "0009"]);
        let violations = check(&root).expect_err("the index disagrees with the directory");
        assert!(violations.iter().any(|v| v.contains("0007") && v.contains("no row")), "{violations:?}");
        assert!(violations.iter().any(|v| v.contains("0009") && v.contains("no file")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_directory_or_index_is_an_error() {
        let root = std::env::temp_dir().join(format!("xtask-docs-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch directory is writable");
        assert!(check(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
