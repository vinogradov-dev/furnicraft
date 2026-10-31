//! Parsing of the three plan files: text in, data out. No filesystem, no git.

use crate::{State, Task, TaskStatus};

/// The marker that starts the state line.
pub(crate) const STATE_PREFIX: &str = "STATE:";

/// Parses the `STATE:` line and its required fields.
pub(crate) fn parse_state(status: &str) -> Result<State, Vec<String>> {
    let Some(line) = status.lines().find(|line| line.trim_start().starts_with(STATE_PREFIX)) else {
        return Err(vec![format!("`plan/STATUS.md` needs a `{STATE_PREFIX}` line")]);
    };
    let mut milestone = None;
    let mut phase = None;
    let mut task = None;
    let mut state = None;
    let mut base = None;
    let mut updated = None;
    for field in line.trim_start().trim_start_matches(STATE_PREFIX).split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "milestone" => milestone = Some(value.to_owned()),
            "phase" => phase = Some(value.to_owned()),
            "task" => task = Some(value.to_owned()),
            "state" => state = Some(value.to_owned()),
            "base" => base = Some(value.to_owned()),
            "updated" => updated = Some(value.to_owned()),
            _ => {}
        }
    }
    let mut violations = Vec::new();
    if milestone.is_none() {
        violations.push("`STATE:` needs `milestone=`".to_owned());
    }
    if task.is_none() {
        violations.push("`STATE:` needs `task=`".to_owned());
    }
    if state.is_none() {
        violations.push("`STATE:` needs `state=`".to_owned());
    }
    if base.is_none() {
        violations.push("`STATE:` needs `base=`".to_owned());
    }
    if updated.is_none() {
        violations.push("`STATE:` needs `updated=`".to_owned());
    }
    let (Some(milestone), Some(task), Some(state), Some(base), Some(updated)) = (milestone, task, state, base, updated)
    else {
        return Err(violations);
    };
    Ok(State { milestone, phase, task, status: state, base, updated })
}

/// Parses every backlog entry that starts with a task marker.
///
/// The marker may be wrapped in backticks for rendering; both forms are accepted. A task may wrap
/// onto indented lines (markdown's way of breaking a long bullet): the entry is joined first, so a
/// `files:` hint on the second line is still the task's hint.
pub fn parse_backlog(backlog: &str) -> Vec<Task> {
    backlog_entries(backlog)
        .into_iter()
        .filter_map(|entry| {
            let task = parse_task_line(&entry.text)?;
            Some(Task { id: task.id, status: task.status, section: entry.section, text: task.text, files: task.files })
        })
        .collect()
}

/// One logical backlog entry: the section it sits in and its wrapped text joined into one line.
pub(crate) struct Entry {
    pub(crate) section: String,
    pub(crate) text: String,
}

/// Groups the backlog into entries, joining indented continuation lines onto their task.
pub(crate) fn backlog_entries(backlog: &str) -> Vec<Entry> {
    let mut section = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut open = false;
    for line in backlog.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            section = heading.trim().to_owned();
            open = false;
            continue;
        }
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        if open
            && line.starts_with([' ', '\t'])
            && let Some(entry) = entries.last_mut()
        {
            entry.text.push(' ');
            entry.text.push_str(trimmed);
            continue;
        }
        if task_candidate(line) {
            entries.push(Entry { section: section.clone(), text: trimmed.to_owned() });
            open = true;
        } else {
            open = false;
        }
    }
    entries
}

/// One parsed backlog line, before its `## ` section is attached.
pub(crate) struct TaskLine {
    pub(crate) status: TaskStatus,
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) files: Option<String>,
}

/// Parses one backlog line, or `None` when it is not a well-formed task.
pub(crate) fn parse_task_line(line: &str) -> Option<TaskLine> {
    let line = line.trim_start().strip_prefix("- ")?;
    let (status, rest) = task_marker(line)?;
    let rest = rest.trim_start().strip_prefix('`').unwrap_or(rest.trim_start()).trim();
    let id = bold(rest)?;
    let files = files_hint(rest);
    let text = strip_files(rest);
    let text =
        text.trim_start().strip_prefix("**").and_then(|text| text.split_once("**")).map_or(text, |(_, text)| text);
    let text = text.trim_start().trim_start_matches(['—', '-', '·']).trim().to_owned();
    Some(TaskLine { status, id, text, files })
}

/// Splits the start of a task line into its status marker and the text after it.
fn task_marker(line: &str) -> Option<(TaskStatus, &str)> {
    let (marker, rest) = match line.strip_prefix('`') {
        Some(rest) => rest.split_once('`')?,
        None => (line.get(..3)?, line.get(3..)?),
    };
    let status = match marker {
        "[ ]" => TaskStatus::Open,
        "[~]" => TaskStatus::Doing,
        "[x]" => TaskStatus::Done,
        _ => return None,
    };
    Some((status, rest))
}

/// Whether a line starts a task entry: a margin bullet with a task marker.
fn task_candidate(line: &str) -> bool {
    let Some(line) = line.strip_prefix("- ") else {
        return false;
    };
    let line = line.trim_start();
    ["[ ]", "[~]", "[x]", "`[ ]`", "`[~]`", "`[x]`"].iter().any(|marker| line.starts_with(marker))
}

/// The first `**bold**` span, used for a task id.
fn bold(text: &str) -> Option<String> {
    let rest = text.strip_prefix("**")?;
    let end = rest.find("**")?;
    let id = rest.get(..end)?.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The `files:` hint at the end of a task line.
pub(crate) fn files_hint(text: &str) -> Option<String> {
    let (_, hint) = text.rsplit_once("files:")?;
    let hint = hint.trim().trim_end_matches('`').trim();
    (!hint.is_empty()).then(|| hint.to_owned())
}

/// The task text without its trailing `files:` hint.
pub(crate) fn strip_files(text: &str) -> &str {
    match text.rsplit_once("files:") {
        Some((text, _)) => text.trim_end().trim_end_matches('`').trim_end(),
        None => text,
    }
}

/// The paths of a `files:` hint that the predicate rejects.
pub(crate) fn missing_paths<F: Fn(&str) -> bool>(hint: &str, exists: F) -> Vec<String> {
    hint.split(',').map(clean_path).filter(|path| !path.is_empty() && !exists(path)).map(str::to_owned).collect()
}

/// Strips the prose punctuation a hint may carry before or after a path.
fn clean_path(path: &str) -> &str {
    path.trim().trim_end_matches(['`', '.', ',', ';']).trim().trim_start_matches('`').trim()
}

/// One row of the `plan/map.md` feature table: the feature and where it lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRow {
    /// The feature name, as the first column spells it.
    pub feature: String,
    /// The path plus the anchor in parentheses; what to grep for.
    pub where_: String,
}

/// Parses the Markdown table in `plan/map.md`: one [`MapRow`] per data row.
///
/// The header row must mention both `Feature` and `Where`; anything else — prose lines, other
/// tables, the `Notes` column — is ignored. Leading and trailing pipes and surrounding whitespace
/// are tolerated, backticks around cells are stripped, and a row with an empty `Where` is skipped.
pub fn parse_map(text: &str) -> Vec<MapRow> {
    let mut rows = Vec::new();
    let mut columns: Option<(usize, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        // Only pipe-delimited lines belong to a table; prose between tables is ignored.
        if !trimmed.starts_with('|') && !trimmed.ends_with('|') {
            continue;
        }
        let cells: Vec<String> = split_cells(trimmed);
        if let Some((feature, where_)) = columns {
            // A separator row (`|---|---|`) has only dashes and colons in every cell: skip it.
            if cells.iter().all(|cell| cell.is_empty() || cell.chars().all(|c| matches!(c, '-' | ':'))) {
                continue;
            }
            let feature = cells.get(feature).cloned().unwrap_or_default();
            let where_ = cells.get(where_).cloned().unwrap_or_default();
            // A row without an anchor is no use to a reader looking for a place to start.
            if !where_.is_empty() {
                rows.push(MapRow { feature, where_ });
            }
            continue;
        }
        // The first table whose header names both columns is *the* map table; earlier ones lose.
        let lower: Vec<String> = cells.iter().map(|cell| cell.to_lowercase()).collect();
        if let (Some(f), Some(w)) = (lower.iter().position(|c| c == "feature"), lower.iter().position(|c| c == "where"))
        {
            columns = Some((f, w));
        }
    }
    rows
}

/// The cells of one table line: outer pipes stripped, each cell trimmed of spaces and backticks.
fn split_cells(line: &str) -> Vec<String> {
    line.trim()
        .strip_prefix('|')
        .unwrap_or(line.trim())
        .strip_suffix('|')
        .unwrap_or(line.trim().strip_prefix('|').unwrap_or(line.trim()))
        .split('|')
        .map(|cell| cell.trim().trim_matches('`').trim().to_owned())
        .collect()
}

/// The value of a non-empty `key:` line of a session note.
pub(crate) fn session_field(session: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    session.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?;
        let value = value.trim().trim_matches('`').trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStatus;

    #[test]
    fn a_task_marker_may_be_wrapped_in_backticks() {
        let plain = parse_backlog("- [ ] **A3** text\n");
        assert_eq!(plain.len(), 1, "an unwrapped marker is a task too");
        assert_eq!(plain[0].id, "A3");
        assert_eq!(plain[0].status, TaskStatus::Open);

        let wrapped = parse_backlog("- `[~]` **A3** text\n");
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].status, TaskStatus::Doing);
    }

    #[test]
    fn a_wrapped_task_keeps_its_files_hint() {
        let wrapped = "## Now\n\n\
                       - `[ ]` **A3** a long sentence in the card\n  \
                       wrapped onto a second line. `files: crates/ui/src/object_card.rs, crates/ui/src/lib.rs`\n\n\
                       ## Done\n\n\
                       - `[x]` **A2** old — `0833b4c`\n";
        let tasks = parse_backlog(wrapped);
        let task = tasks.iter().find(|task| task.id == "A3").expect("A3 parses");
        assert_eq!(task.files.as_deref(), Some("crates/ui/src/object_card.rs, crates/ui/src/lib.rs"));
        assert!(task.text.contains("wrapped onto a second line"), "{:?}", task.text);
        assert_eq!(task.section, "Now");
    }

    #[test]
    fn a_hinted_path_that_does_not_exist_is_reported() {
        let missing =
            missing_paths("crates/ui/src/lib.rs, crates/ui/src/missing.rs", |path| path == "crates/ui/src/lib.rs");
        assert_eq!(missing, ["crates/ui/src/missing.rs"]);

        let cleaned =
            missing_paths("`crates/ui` , docs/decisions/.", |path| path == "crates/ui" || path == "docs/decisions/");
        assert!(cleaned.is_empty(), "{cleaned:?}");
    }

    #[test]
    fn a_session_field_is_read_only_when_it_has_a_value() {
        let note = "task: A3\ngoal:\nnow: the arc arm\nrun: cargo test\n";
        assert_eq!(session_field(note, "task").as_deref(), Some("A3"));
        assert_eq!(session_field(note, "now").as_deref(), Some("the arc arm"));
        assert_eq!(session_field(note, "goal"), None, "an empty field is not a value");
        assert_eq!(session_field(note, "resume"), None);
    }

    #[test]
    fn a_map_table_yields_one_row_per_feature() {
        let text = "# Map\n\nProse before the table.\n\n\
                    | Feature | Where | Notes |\n\
                    |---|---|---|\n\
                    | Contour ring | `crates/model/src/lib.rs` (`ContourRing`) | ~4600 lines |\n\
                    |  Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) |  small  |\n\
                    | No anchor here |  | — |\n";
        let rows = parse_map(text);
        assert_eq!(rows.len(), 2, "the row without a `Where` is skipped: {rows:?}");
        assert_eq!(rows[0].feature, "Contour ring");
        assert_eq!(rows[0].where_, "crates/model/src/lib.rs (`ContourRing`)");
        assert_eq!(rows[1].feature, "Panel commands", "surrounding whitespace is trimmed");
        assert_eq!(rows[1].where_, "crates/engine/src/panel_cmds.rs (`panel.*`)");
    }

    #[test]
    fn a_map_without_its_header_is_no_rows() {
        let missing_header = "| Anything | Else |\n|---|---|\n| a | b |\n";
        assert!(parse_map(missing_header).is_empty(), "a table without `Feature`/`Where` is not the map");
        assert!(parse_map("").is_empty(), "an empty file has no rows");
        assert!(parse_map("# Map\n\nNo table at all.\n").is_empty(), "prose alone has no rows");
    }
}
