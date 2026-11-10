//! Parsing of the three plan files: text in, data out. No filesystem, no git.

use crate::{Field, State, Task, TaskStatus};

/// The marker that starts the state line.
pub(crate) const STATE_PREFIX: &str = "STATE:";

/// The labels a long backlog entry carries, in the order the template writes them.
///
/// Matching ignores case — `— rules: B1` closes the lead line — and needs a word start, so a body
/// that merely mentions the word keeps it.
pub(crate) const ENTRY_LABELS: &[&str] = &["Rules", "Why", "Shape", "Done", "Commits"];

/// The keys the `STATE:` line carries. Anything else is a key nobody reads, and a plan that grows
/// one by accident is a plan whose owner believes something is checked that is not.
pub(crate) const STATE_KEYS: &[&str] = &["milestone", "phase", "task", "state", "base", "updated"];

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
    let mut violations = Vec::new();
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
            other => violations.push(format!(
                "`STATE:` carries `{other}=`, which nobody reads; the keys are {}",
                STATE_KEYS.join(", ")
            )),
        }
    }
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
    if !violations.is_empty() {
        return Err(violations);
    }
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
            let (lead, fields) = split_fields(&task.text);
            let paths = task.files.as_deref().map(hint_paths).unwrap_or_default();
            Some(Task {
                id: task.id,
                status: task.status,
                section: entry.section,
                text: task.text,
                lead,
                fields,
                files: task.files,
                paths,
            })
        })
        .collect()
}

/// Splits one entry into its opening sentence and its labelled fields.
///
/// The lead keeps everything before the first label, so `— rules: B1` becomes a field and the lead
/// stays one short sentence. Each field runs from its label to the next one, so the order of the
/// file is the order of the result.
pub(crate) fn split_fields(text: &str) -> (String, Vec<Field>) {
    let mut cuts: Vec<(usize, usize, &'static str)> = Vec::new();
    for label in ENTRY_LABELS {
        let mut from = 0;
        while let Some(found) = find_label(text.get(from..).unwrap_or_default(), label) {
            let start = from.saturating_add(found);
            cuts.push((start, start.saturating_add(label.len()).saturating_add(1), label));
            from = start.saturating_add(label.len()).saturating_add(1);
        }
    }
    cuts.sort_by_key(|(start, _, _)| *start);
    let lead_end = cuts.first().map_or(text.len(), |(start, _, _)| *start);
    let lead = clean_lead(text.get(..lead_end).unwrap_or_default());
    let mut fields = Vec::new();
    for (index, (_, body_start, label)) in cuts.iter().enumerate() {
        let end = cuts.get(index.saturating_add(1)).map_or(text.len(), |(start, _, _)| *start);
        let body = text.get(*body_start..end).unwrap_or_default().trim();
        if !body.is_empty() {
            fields.push(Field { label: (*label).to_owned(), body: body.to_owned() });
        }
    }
    (lead, fields)
}

/// The byte offset of `label:` in `haystack`, when the label starts a word and is written in any
/// case. `None` when the entry does not carry that label at all.
fn find_label(haystack: &str, label: &str) -> Option<usize> {
    for (colon, _) in haystack.match_indices(':') {
        let word_start = haystack
            .get(..colon)
            .unwrap_or_default()
            .char_indices()
            .rev()
            .take_while(|(_, ch)| ch.is_ascii_alphabetic())
            .last()
            .map(|(start, _)| start);
        let Some(word_start) = word_start else { continue };
        if !haystack.get(word_start..colon).unwrap_or_default().eq_ignore_ascii_case(label) {
            continue;
        }
        let boundary = haystack
            .get(..word_start)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|ch| !ch.is_alphanumeric());
        if boundary {
            return Some(word_start);
        }
    }
    None
}

/// The lead sentence without the dash or colon that introduced the first label.
fn clean_lead(text: &str) -> String {
    text.trim().trim_end_matches(['—', '–', '-', '·', ':', ' ']).trim_end().to_owned()
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
    let text: &str = text
        .trim_start()
        .strip_prefix("**")
        .and_then(|text| text.split_once("**"))
        .map_or(text.as_str(), |(_, text)| text);
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

/// The `files:` hint of a task, which is the comma-separated path list that follows the label.
///
/// The list ends at the first character a path cannot hold, so an inline hint is followed by the
/// rest of the line (`— \`hash\``, a field) instead of swallowing it.
pub(crate) fn files_hint(text: &str) -> Option<String> {
    let (_, hint) = text.rsplit_once("files:")?;
    let hint = hint.get(..hint_len(hint)).unwrap_or_default().trim().trim_end_matches(['`', '.', ',']).trim();
    (!hint.is_empty()).then(|| hint.to_owned())
}

/// The byte length of the path list that starts `hint`: paths, commas, spaces and inline-code ticks.
fn hint_len(hint: &str) -> usize {
    let mut end = 0;
    for (index, ch) in hint.char_indices() {
        let allowed = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-' | ',' | ' ' | '\t' | '`');
        if !allowed {
            break;
        }
        end = index.saturating_add(ch.len_utf8());
    }
    end
}

/// The task text without its `files:` hint. Whatever follows the hint survives, so a done entry that
/// writes its commit after the hint keeps it.
pub(crate) fn strip_files(text: &str) -> String {
    let Some((before, hint)) = text.rsplit_once("files:") else { return text.trim_end().to_owned() };
    let head = before.trim_end().trim_end_matches('`').trim_end();
    let tail = hint.get(hint_len(hint)..).unwrap_or_default().trim();
    if tail.is_empty() { head.to_owned() } else { format!("{head} {tail}") }
}

/// The `files:` hint split into paths, in the order it names them.
pub(crate) fn hint_paths(hint: &str) -> Vec<String> {
    hint.split(',').map(clean_path).filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

/// The paths of a `files:` hint that the predicate rejects.
pub(crate) fn missing_paths<F: Fn(&str) -> bool>(hint: &str, exists: F) -> Vec<String> {
    hint_paths(hint).into_iter().filter(|path| !exists(path)).collect()
}

/// The four-digit ADR numbers `text` cites, including a written range (`ADRs 0034–0035`).
pub(crate) fn adr_numbers(text: &str) -> Vec<String> {
    let mut numbers: Vec<String> = Vec::new();
    for (offset, _) in text.match_indices("ADR") {
        let mut cursor = offset + 3;
        if text.get(cursor..).is_some_and(|rest| rest.starts_with('s')) {
            cursor += 1;
        }
        while text.get(cursor..).is_some_and(|rest| rest.starts_with(' ')) {
            cursor += 1;
        }
        let Some((first, mut after)) = take_number(text, cursor) else { continue };
        push_unique(&mut numbers, first);
        // A range names its second decision after a dash and no second `ADR`.
        if let Some(dash) = text.get(after..).and_then(|rest| rest.chars().next())
            && matches!(dash, '–' | '—' | '-')
        {
            after += dash.len_utf8();
            if let Some((second, _)) = take_number(text, after) {
                push_unique(&mut numbers, second);
            }
        }
    }
    numbers
}

/// Exactly four digits at `start`, when the run is not part of a longer number.
fn take_number(text: &str, start: usize) -> Option<(&str, usize)> {
    let run: String = text.get(start..)?.chars().take(5).take_while(char::is_ascii_digit).collect();
    (run.len() == 4).then(|| (text.get(start..start + 4).unwrap_or_default(), start + 4))
}

/// Pushes a number once, keeping the order the text cites them in.
fn push_unique(numbers: &mut Vec<String>, number: &str) {
    if !numbers.iter().any(|seen| seen == number) {
        numbers.push(number.to_owned());
    }
}

/// The path-like tokens of a map cell: a lower-case run that holds a `/`, so an anchor name or a
/// prose pair like `Line/Arc` never matches.
pub(crate) fn map_paths(cell: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current = String::new();
    for ch in cell.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-') {
            current.push(ch);
        } else {
            flush_path(&mut current, &mut paths);
        }
    }
    flush_path(&mut current, &mut paths);
    paths
}

/// Pushes the run collected so far when it reads as a path, then starts a new run.
fn flush_path(current: &mut String, paths: &mut Vec<String>) {
    let trimmed = current.trim_end_matches(['.', '-', '/']).trim_start_matches(['.', '/']);
    let lower_case = trimmed.chars().all(|ch| !ch.is_ascii_uppercase());
    if trimmed.contains('/') && lower_case && trimmed.chars().any(|ch| ch.is_ascii_alphabetic()) {
        paths.push(trimmed.to_owned());
    }
    current.clear();
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

impl MapRow {
    /// The paths the row names, in cell order — what a task's `files:` hint is matched against.
    pub fn paths(&self) -> Vec<String> {
        map_paths(&self.where_)
    }
}

/// Parses the Markdown table in `plan/map.md`: one [`MapRow`] per data row.
///
/// The header row must mention both `Feature` and `Where`; anything else — prose lines, other
/// tables, the `Notes` column — is ignored. Leading and trailing pipes and surrounding whitespace
/// are tolerated, backticks around cells are stripped, and a row with an empty `Where` is skipped.
pub(crate) fn parse_map(text: &str) -> Vec<MapRow> {
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

/// The top-level `- ` bullets of a markdown file, wrapped lines joined onto their bullet, so a
/// two-line entry reads as one sentence.
pub(crate) fn markdown_bullets(text: &str) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            bullets.push(rest.trim().to_owned());
            open = true;
        } else if open && line.starts_with([' ', '\t']) && !trimmed.is_empty() {
            if let Some(bullet) = bullets.last_mut() {
                bullet.push(' ');
                bullet.push_str(trimmed);
            }
        } else {
            open = false;
        }
    }
    bullets
}

/// The cells of one table line: outer pipes stripped, each cell trimmed of spaces, with every
/// inline-code span unwrapped — `` `path` (`anchor`) `` reads as `path (anchor)`, so no cell can
/// carry a stray backtick.
fn split_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let body = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let body = body.strip_suffix('|').unwrap_or(body);
    body.split('|').map(|cell| unwrap_code(cell.trim())).collect()
}

/// Replaces every balanced `` `code` `` span in `text` by its contents; an unpaired backtick stays.
fn unwrap_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        match after[1..].find('`') {
            Some(end) => {
                out.push_str(&after[1..end + 1]);
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&after[1..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_owned()
}

/// The value of a non-empty `key:` line of a session note.
pub(crate) fn session_field(session: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    session.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?;
        let value = unwrap_field(value.trim());
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Strips one pair of backticks that wraps the whole value (`` `A3` `` reads as `A3`).
///
/// A value that merely *ends* in inline code — ``now: the arm in `src/lib.rs` `` — keeps its code
/// span: cutting every trailing backtick would leave the sentence broken.
fn unwrap_field(value: &str) -> &str {
    match value.strip_prefix('`').and_then(|inner| inner.strip_suffix('`')) {
        Some(inner) if !inner.contains('`') => inner,
        _ => value,
    }
}

/// Parses the fields a session resumes from; the note's free prose and step list stay unread.
pub(crate) fn parse_session(session: &str) -> crate::Session {
    crate::Session {
        task: session_field(session, "task"),
        goal: session_field(session, "goal"),
        now: session_field(session, "now"),
        blocked: session_field(session, "blocked"),
        run: session_field(session, "run"),
        resume: session_field(session, "resume"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStatus;

    #[test]
    fn a_state_line_may_not_carry_a_key_nobody_reads() {
        let line = "STATE: milestone=M1 phase=P task=P1 state=idle base=abc1234 updated=2026-11-10 next=P2\n";
        let error = parse_state(line).expect_err("`next=` is not a key of the state line");
        assert!(error.iter().any(|violation| violation.contains("`next=`")), "{error:?}");
        assert!(error.iter().any(|violation| violation.contains("milestone, phase, task")), "{error:?}");

        let plain = "STATE: milestone=M1 phase=P task=P1 state=idle base=abc1234 updated=2026-11-10\n";
        parse_state(plain).expect("the six keys are the state");
    }

    #[test]
    fn an_entry_splits_into_its_lead_and_its_labelled_fields() {
        let (lead, fields) = split_fields(
            "`panel_cmds.rs` gives up its banding commands — rules: B1, B4. Why: 1143 lines. \
             Shape: the ten commands move. Done: the registry test. Commits: one move.",
        );
        assert_eq!(lead, "`panel_cmds.rs` gives up its banding commands");
        let labels: Vec<&str> = fields.iter().map(|field| field.label.as_str()).collect();
        assert_eq!(labels, ["Rules", "Why", "Shape", "Done", "Commits"]);
        assert_eq!(fields[0].body, "B1, B4.");
        assert_eq!(fields[2].body, "the ten commands move.");
        assert!(fields[4].body.starts_with("one move"), "{:?}", fields[4].body);
    }

    #[test]
    fn a_label_needs_a_word_start_and_any_case() {
        let (lead, fields) = split_fields("a task without fields, and no colon either");
        assert_eq!(lead, "a task without fields, and no colon either");
        assert!(fields.is_empty(), "{fields:?}");

        let (_, fields) = split_fields("a task why: lower case counts");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].label, "Why");
        assert_eq!(fields[0].body, "lower case counts");

        let (lead, fields) = split_fields("a task with `Done: now` in code is still a field");
        assert_eq!(lead, "a task with `");
        assert_eq!(fields[0].label, "Done", "a label is a label wherever it starts a word");
    }

    #[test]
    fn a_bug_entry_keeps_its_whole_observation_as_the_lead() {
        let bugs = "## Bugs\n\n- `[ ]` **X31** a drag on a short segment panics: the corner stops neither \
                    way. Expectation: refuse it. Suspected cause: the clamp. `files: crates/geom/src/segment.rs`\n";
        let task = &parse_backlog(bugs)[0];
        assert!(task.fields.is_empty(), "a bug has no labelled fields: {:?}", task.fields);
        assert!(task.lead.starts_with("a drag on a short segment panics"), "{:?}", task.lead);
        assert!(!task.lead.contains("files:"), "the hint is not part of the lead: {:?}", task.lead);
        assert_eq!(task.paths, ["crates/geom/src/segment.rs"]);
    }

    #[test]
    fn paths_come_from_the_hint_and_from_a_map_cell() {
        assert_eq!(
            hint_paths("`crates/model/src/lib.rs`, crates/ui/src/object_card.rs."),
            ["crates/model/src/lib.rs", "crates/ui/src/object_card.rs"]
        );
        assert_eq!(
            map_paths("crates/scene/src/lib.rs (Polyline, SceneFrame::pick), docs/decisions/"),
            ["crates/scene/src/lib.rs", "docs/decisions"]
        );
        assert!(map_paths("(PanelOutlineSet, PanelOutlineToContour)").is_empty(), "an anchor alone is no path");
        assert!(
            map_paths("`crates/model/src/contour.rs` (ContourSegment, Line/Arc)")
                .iter()
                .all(|path| path.contains("crates")),
            "a prose pair with a slash is not a path"
        );
    }

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
    fn a_backticked_field_loses_only_its_wrapping_pair() {
        let note = "task: `A3`\nnow: the arm in `src/lib.rs`\n";
        assert_eq!(session_field(note, "task").as_deref(), Some("A3"), "a wrapped value unwraps");
        assert_eq!(
            session_field(note, "now").as_deref(),
            Some("the arm in `src/lib.rs`"),
            "an inner code span keeps its closing backtick"
        );
    }

    #[test]
    fn a_session_note_parses_into_the_fields_a_reader_resumes_from() {
        let note = "task: A3\ngoal: arcs\n\nsteps:\n  [ ] the arm\n\nnow: the arm\nblocked: nothing\n\
                    run: cargo test\nresume: finish the arm\n";
        let session = parse_session(note);
        assert_eq!(session.task.as_deref(), Some("A3"));
        assert_eq!(session.goal.as_deref(), Some("arcs"));
        assert_eq!(session.now.as_deref(), Some("the arm"));
        assert_eq!(session.blocked.as_deref(), Some("nothing"));
        assert_eq!(session.run.as_deref(), Some("cargo test"));
        assert_eq!(session.resume.as_deref(), Some("finish the arm"));
        assert_eq!(parse_session("# empty\n"), crate::Session::default(), "a note without fields parses empty");
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
        assert_eq!(
            rows[0].where_, "crates/model/src/lib.rs (ContourRing)",
            "inline code is unwrapped, no stray backtick"
        );
        assert!(!rows[0].where_.contains('`'), "{:?}", rows[0].where_);
        assert_eq!(rows[1].feature, "Panel commands", "surrounding whitespace is trimmed");
        assert_eq!(rows[1].where_, "crates/engine/src/panel_cmds.rs (panel.*)");
    }

    #[test]
    fn a_map_without_its_header_is_no_rows() {
        let missing_header = "| Anything | Else |\n|---|---|\n| a | b |\n";
        assert!(parse_map(missing_header).is_empty(), "a table without `Feature`/`Where` is not the map");
        assert!(parse_map("").is_empty(), "an empty file has no rows");
        assert!(parse_map("# Map\n\nNo table at all.\n").is_empty(), "prose alone has no rows");
    }
}
