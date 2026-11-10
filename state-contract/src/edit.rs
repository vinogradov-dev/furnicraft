//! Line splices on the plan files: text in, text out.
//!
//! A command owns the lines of the entry it changes. Every other line — the prose, the order, the
//! blank lines, the trailing newline — is copied byte for byte, so a plan commit stays a readable
//! diff and two sessions never fight over formatting (ADR 0085).
//!
//! Nothing here reads or writes a file: the caller stages the result, validates it with [`crate::check`]
//! and only then lets it land. A splice that cannot find what it was asked to change returns a
//! message instead of guessing, because a silent edit to the wrong entry is worse than a refusal.

/// The indent of a continuation line: two spaces, the only form `plan/BACKLOG.md` uses.
const INDENT: &str = "  ";

/// The width a field body is wrapped at, so a long `Why:` stays a readable diff.
const FIELD_WIDTH: usize = 95;

/// The three status markers, as the file spells them.
pub const OPEN: &str = "[ ]";
/// A task in progress.
pub const DOING: &str = "[~]";
/// A task that landed.
pub const DONE: &str = "[x]";

/// Where a moved or new entry lands in its section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// Above the entries already there: a close puts the newest first in `## Done`.
    Top,
    /// Below them: a new task joins the end of the work order.
    End,
}

/// Where one entry sits: the marker line and the exclusive end of its continuation lines.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    id: String,
    section: String,
    start: usize,
    end: usize,
}

/// A plan file split for editing: its lines, its entries and where each `## ` section starts.
#[derive(Clone, Debug)]
struct Layout {
    lines: Vec<String>,
    entries: Vec<Entry>,
    sections: Vec<(String, usize)>,
    trailing_newline: bool,
}

impl Layout {
    /// Scans `text` into the lines, the entries and the section headings.
    fn parse(text: &str) -> Self {
        let trailing_newline = text.ends_with('\n');
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let mut entries = Vec::new();
        let mut sections = Vec::new();
        let mut section = String::new();
        let mut index = 0;
        while index < lines.len() {
            let line = &lines[index];
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix("## ") {
                section = heading.trim().to_owned();
                sections.push((section.clone(), index));
                index += 1;
                continue;
            }
            if trimmed.is_empty() || line.starts_with([' ', '\t']) {
                index += 1;
                continue;
            }
            let Some(task) = crate::parse::parse_task_line(line) else {
                index += 1;
                continue;
            };
            let start = index;
            index += 1;
            while index < lines.len() && lines[index].starts_with([' ', '\t']) && !lines[index].trim().is_empty() {
                index += 1;
            }
            entries.push(Entry { id: task.id, section: section.clone(), start, end: index });
        }
        Self { lines, entries, sections, trailing_newline }
    }

    /// The entry `id`, or a message naming what the file does have.
    fn entry(&self, id: &str) -> Result<&Entry, String> {
        self.entries.iter().find(|entry| entry.id == id).ok_or_else(|| format!("`plan/BACKLOG.md` has no task `{id}`"))
    }

    /// The line index of the `## <section>` heading.
    fn heading(&self, section: &str) -> Result<usize, String> {
        self.sections
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(section))
            .map(|(_, at)| *at)
            .ok_or_else(|| format!("`plan/BACKLOG.md` has no `## {section}` section"))
    }

    /// The lines joined back into text, with the original trailing newline.
    fn render(&self) -> String {
        let mut text = self.lines.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        text
    }

    /// The line range of an entry's field, marker line excluded: `label:` and its continuation lines.
    ///
    /// A wrapped body is indented exactly like the next field, so the range ends at the next line
    /// that starts a known label, not at the first unindented one.
    fn field_range(&self, entry: &Entry, label: &str) -> Option<(usize, usize)> {
        let start = (entry.start..entry.end).find(|index| {
            self.lines
                .get(*index)
                .and_then(|line| label_of(line))
                .is_some_and(|found| found.eq_ignore_ascii_case(label))
        })?;
        let end = (start.saturating_add(1)..entry.end)
            .find(|index| self.lines.get(*index).and_then(|line| label_of(line)).is_some())
            .unwrap_or(entry.end);
        Some((start, end))
    }
}

/// The label a backlog line starts, when it starts one: `Rules`, `Why`, `Shape`, `Done`, `Commits`
/// or the `files:` hint, in any case, with or without the inline-code backtick.
fn label_of(line: &str) -> Option<String> {
    let body = line.trim_start().trim_start_matches('`');
    let (label, _) = body.split_once(':')?;
    let known = crate::parse::ENTRY_LABELS.iter().any(|known| known.eq_ignore_ascii_case(label)) || label == "files";
    known.then(|| label.to_owned())
}

/// Sets the status marker of `id`.
pub fn set_marker(backlog: &str, id: &str, marker: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let line = &mut layout.lines[entry.start];
    let head_len = line.len().min(12);
    let head = line.get(..head_len).unwrap_or_default();
    let replaced = [OPEN, DOING, DONE]
        .iter()
        .find_map(|current| head.find(current).map(|at| (at, current.len())))
        .ok_or_else(|| format!("task {id} has no status marker on its first line"));
    let (at, len) = replaced?;
    line.replace_range(at..at + len, marker);
    Ok(layout.render())
}

/// Appends `suffix` to the last line of the entry, where a close records its commit.
pub fn append_to_entry(backlog: &str, id: &str, suffix: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let last = entry.end.saturating_sub(1);
    layout.lines[last].push_str(suffix);
    Ok(layout.render())
}

/// Replaces the `files:` hint of `id`.
pub fn set_files(backlog: &str, id: &str, hint: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let range = layout.field_range(&entry, "files").ok_or_else(|| format!("task {id} has no `files:` hint"))?;
    layout.lines.splice(range.0..range.1, [format!("{INDENT}`files: {hint}`")]);
    Ok(layout.render())
}

/// Replaces, adds or removes one labelled field of `id`; an empty body removes the field.
pub fn set_field(backlog: &str, id: &str, label: &str, body: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let body = body.trim();
    let replacement = if body.is_empty() { Vec::new() } else { field_lines(label, body) };
    if let Some((start, end)) = layout.field_range(&entry, label) {
        layout.lines.splice(start..end, replacement);
        return Ok(layout.render());
    }
    if body.is_empty() {
        return Ok(layout.render());
    }
    // No such field: it goes before the `files:` hint, or at the end of the entry.
    let at = layout.field_range(&entry, "files").map_or(entry.end, |range| range.0);
    layout.lines.splice(at..at, replacement);
    Ok(layout.render())
}

/// Inserts a whole entry into `section`, at the top or after the entries already there.
pub fn insert_entry(backlog: &str, section: &str, at: At, block: &[String]) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let heading = layout.heading(section)?;
    let index = match at {
        At::Top => top_of_body(&layout, heading),
        At::End => end_of_body(&layout, heading),
    };
    let mut inserted: Vec<String> = block.to_vec();
    // One blank line between entries, never two: the section already ends with one.
    if !layout.lines.get(index).is_some_and(|line| line.trim().is_empty()) {
        inserted.push(String::new());
    }
    layout.lines.splice(index..index, inserted);
    Ok(layout.render())
}

/// The line where a new entry goes when it joins the end of a section: past the last entry, before
/// the blank lines that close the section.
fn end_of_body(layout: &Layout, heading: usize) -> usize {
    let next_heading = layout.sections.iter().map(|(_, at)| *at).filter(|at| *at > heading).min();
    let mut at = next_heading.unwrap_or(layout.lines.len());
    while at > heading.saturating_add(1)
        && layout.lines.get(at.saturating_sub(1)).is_some_and(|line| line.trim().is_empty())
    {
        at = at.saturating_sub(1);
    }
    at
}

/// Removes `id` and the blank line that followed it.
pub fn remove_entry(backlog: &str, id: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let end = blank_after(&layout, entry.end);
    layout.lines.drain(entry.start..end);
    Ok(layout.render())
}

/// Moves `id` into `section` at `at`, keeping its own lines untouched.
pub fn move_entry(backlog: &str, id: &str, section: &str, at: At) -> Result<String, String> {
    let layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let block: Vec<String> = layout.lines[entry.start..entry.end].to_vec();
    let without = remove_entry(backlog, id)?;
    insert_entry(&without, section, at, &block)
}

/// The next free id in `phase`: one past the highest number the file uses, so no id is reused.
pub fn next_id(backlog: &str, phase: &str) -> String {
    let layout = Layout::parse(backlog);
    let highest = layout
        .entries
        .iter()
        .filter_map(|entry| {
            let number = entry.id.get(phase.len()..)?;
            let prefix = entry.id.get(..phase.len())?;
            (prefix == phase && !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
                .then(|| number.parse::<u32>().ok())
                .flatten()
        })
        .max()
        .unwrap_or(0);
    format!("{phase}{}", highest.saturating_add(1))
}

/// The lines of a new entry: the marker line, its labelled fields and the `files:` hint.
pub fn new_entry(id: &str, lead: &str, fields: &[(String, String)], files: &str) -> Vec<String> {
    let mut lines = vec![format!("- `{OPEN}` **{id}** {lead}")];
    for (label, body) in fields {
        lines.extend(field_lines(label, body));
    }
    lines.push(format!("{INDENT}`files: {files}`"));
    lines
}

/// Removes the closing commit a [`crate::edit`] close appended — `` — `hash` `` or `` `[x]` `hash` ``.
pub fn strip_commit(backlog: &str, id: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let last = entry.end.saturating_sub(1);
    layout.lines[last] = strip_hash_suffix(&layout.lines[last]);
    Ok(layout.render())
}

/// The line without a trailing backticked commit hash, and without the close marker before it.
fn strip_hash_suffix(line: &str) -> String {
    let Some(end) = line.rfind('`') else { return line.to_owned() };
    let Some(start) = line.get(..end).and_then(|head| head.rfind('`')) else { return line.to_owned() };
    let hash = line.get(start.saturating_add(1)..end).unwrap_or_default();
    if hash.len() < 6 || !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return line.to_owned();
    }
    let mut head = line.get(..start).unwrap_or_default().trim_end();
    for marker in ["—", "`[x]`"] {
        if let Some(rest) = head.strip_suffix(marker) {
            head = rest.trim_end();
        }
    }
    head.to_owned()
}

/// Appends one open step to the `steps:` block of a session note.
pub fn add_step(session: &str, text: &str) -> Result<String, String> {
    let lines: Vec<String> = session.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|line| line.trim_end().ends_with("steps:"))
        .ok_or_else(|| "`plan/SESSION.md` has no `steps:` line".to_owned())?;
    let mut end = at.saturating_add(1);
    while end < lines.len() && is_step(&lines[end]) {
        end = end.saturating_add(1);
    }
    let mut out = lines;
    out.insert(end, format!("{INDENT}[ ] {}", text.trim()));
    Ok(join(out, session))
}

/// Marks the first open step of a session note done.
pub fn tick_step(session: &str) -> Result<String, String> {
    let mut lines: Vec<String> = session.lines().map(str::to_owned).collect();
    let index = lines
        .iter()
        .position(|line| line.contains(OPEN))
        .ok_or_else(|| "`plan/SESSION.md` has no open step to tick".to_owned())?;
    lines[index] = lines[index].replacen(OPEN, DONE, 1);
    Ok(join(lines, session))
}

/// Whether a line is one step of the `steps:` block.
fn is_step(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.starts_with([' ', '\t']) && (trimmed.starts_with(OPEN) || trimmed.starts_with(DONE))
}

/// Joins lines back into text, keeping the trailing newline the original had.
fn join(lines: Vec<String>, original: &str) -> String {
    let mut text = lines.join("\n");
    if original.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Replaces the body of a `## <heading>` section, keeping the heading and the blank lines around it.
///
/// The prose under `## Next` is the one part of `plan/STATUS.md` a command has to own, because the
/// gate requires it to name the current task and a close changes which task that is.
pub fn set_section(text: &str, heading: &str, body: &str) -> Result<String, String> {
    let mut layout = Layout::parse(text);
    let at = layout.heading(heading)?;
    let end = layout.sections.iter().map(|(_, at)| *at).filter(|start| *start > at).min().unwrap_or(layout.lines.len());
    let mut replacement: Vec<String> = vec![String::new()];
    replacement.extend(body.lines().map(str::trim_end).filter(|line| !line.is_empty()).map(str::to_owned));
    if end < layout.lines.len() {
        replacement.push(String::new());
    }
    layout.lines.splice(at.saturating_add(1)..end, replacement);
    Ok(layout.render())
}

/// Sets one or more `key=value` fields on the single `STATE:` line.
///
/// A key the line does not carry is appended, so a new field costs one command and no rewrite.
pub fn set_state(status: &str, values: &[(String, String)]) -> Result<String, String> {
    let mut lines: Vec<String> = status.lines().map(str::to_owned).collect();
    let index = lines
        .iter()
        .position(|line| line.trim_start().starts_with(crate::parse::STATE_PREFIX))
        .ok_or_else(|| "`plan/STATUS.md` has no `STATE:` line".to_owned())?;
    let line = &mut lines[index];
    for (key, value) in values {
        let prefix = format!("{key}=");
        match line.split_whitespace().find(|word| word.starts_with(&prefix)) {
            Some(word) => {
                let word = word.to_owned();
                *line = line.replacen(&word, &format!("{prefix}{value}"), 1);
            }
            None => {
                line.push(' ');
                line.push_str(&prefix);
                line.push_str(value);
            }
        }
    }
    let mut text = lines.join("\n");
    if status.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// The wrapped lines of one labelled field, `Label: body` on the first.
fn field_lines(label: &str, body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = format!("{INDENT}{label}:");
    for word in body.split_whitespace() {
        // `+ 1` for the space before a word; the first line also carries `Label:`.
        if current.chars().count() + 1 + word.chars().count() > FIELD_WIDTH && current.trim() != format!("{label}:") {
            lines.push(current);
            current = INDENT.to_owned();
        }
        current.push(' ');
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        lines.push(current);
    }
    lines
}

/// The line after `## <section>` where a new entry goes: the heading, one blank line, the entry.
///
/// A section with prose under its heading is not a home for a machine-inserted entry; the handler
/// refuses rather than guessing where the prose ends.
fn top_of_body(layout: &Layout, heading: usize) -> usize {
    let after = heading.saturating_add(1);
    if layout.lines.get(after).is_some_and(|line| line.trim().is_empty()) { heading.saturating_add(2) } else { after }
}

/// The exclusive end of an entry including the blank line that follows it.
fn blank_after(layout: &Layout, end: usize) -> usize {
    if layout.lines.get(end).is_some_and(|line| line.trim().is_empty()) { end.saturating_add(1) } else { end }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const BACKLOG: &str = "## Now

- `[~]` **S14** the banding commands move — rules: B1.
  Why: 1143 lines and
  two reasons to change.
  `files: crates/engine/src/panel_cmds.rs`

- `[ ]` **S15** the camera and the ray move — rules: B1.
  `files: crates/scene/src/lib.rs`

## Bugs

- `[ ]` **X31** a short segment panics `files: crates/geom/src/segment.rs`

## Done

- `[x]` **S13** the frames move `[x]` `d8864be`
";

    fn lines(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    #[test]
    fn a_marker_changes_in_place_and_nothing_else_moves() {
        let before = lines(BACKLOG);
        let after = set_marker(BACKLOG, "S15", DOING).expect("S15 is in the file");
        let after_lines = lines(&after);
        assert_eq!(after_lines[7], "- `[~]` **S15** the camera and the ray move — rules: B1.");
        for (index, line) in before.iter().enumerate() {
            if index != 7 {
                assert_eq!(after_lines[index], *line, "line {index} must stay byte-identical");
            }
        }
    }

    #[test]
    fn an_unknown_task_or_a_markerless_line_is_refused() {
        let error = set_marker(BACKLOG, "S99", DOING).expect_err("S99 does not exist");
        assert!(error.contains("has no task `S99`"), "{error}");
    }

    #[test]
    fn a_close_appends_the_commit_to_the_last_line() {
        let text = append_to_entry(BACKLOG, "S15", " `[x]` `abc1234`").expect("S15 is in the file");
        assert!(text.contains("  `files: crates/scene/src/lib.rs` `[x]` `abc1234`"), "{text}");
    }

    #[test]
    fn a_field_is_replaced_and_its_continuation_lines_go_with_it() {
        let text = set_field(BACKLOG, "S14", "Why", "measured again: 900 lines").expect("S14 is in the file");
        assert!(text.contains("  Why: measured again: 900 lines\n"), "{text}");
        assert!(!text.contains("1143 lines"), "the old body is gone: {text}");
        assert!(text.contains("  `files: crates/engine/src/panel_cmds.rs`"), "the next field survives: {text}");
        assert_eq!(text.lines().count(), BACKLOG.lines().count() - 1, "two body lines became one");
    }

    #[test]
    fn a_new_field_goes_before_the_files_hint() {
        let text = set_field(BACKLOG, "S15", "Done", "the suite passes").expect("S15 is in the file");
        let body: Vec<&str> = text.lines().collect();
        let done = body.iter().position(|line| line.trim_start().starts_with("Done:")).expect("the field is there");
        let files = body.iter().position(|line| line.contains("`files: crates/scene")).expect("the hint is there");
        assert!(done < files, "the hint stays last: {text}");
    }

    #[test]
    fn an_empty_body_removes_the_field() {
        let text = set_field(BACKLOG, "S14", "Why", "  ").expect("S14 is in the file");
        assert!(!text.contains("Why:"), "{text}");
        assert!(text.contains("Rules") || text.contains("rules:"), "the rest of the entry stays: {text}");
    }

    #[test]
    fn a_long_body_is_wrapped_with_the_indent() {
        let body = "word ".repeat(60);
        let text = set_field(BACKLOG, "S15", "Shape", &body).expect("S15 is in the file");
        let field: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with("Shape:") || line.starts_with("  ") && !line.contains(':'))
            .collect();
        assert!(field.len() > 1, "the body wraps: {text}");
        for line in text.lines().filter(|line| line.starts_with("  ") && !line.contains(':')) {
            assert!(line.chars().count() <= FIELD_WIDTH, "a wrapped line fits: {line:?}");
        }
    }

    #[test]
    fn a_files_hint_is_replaced_without_touching_the_entry() {
        let text = set_files(BACKLOG, "S14", "crates/a.rs, crates/b.rs").expect("S14 is in the file");
        assert!(text.contains("  `files: crates/a.rs, crates/b.rs`\n"), "{text}");
    }

    #[test]
    fn an_entry_is_inserted_at_the_top_of_its_section() {
        let block =
            vec!["- `[ ]` **P6** the writer — rules: B1.".to_owned(), "  `files: xtask/src/main.rs`".to_owned()];
        let text = insert_entry(BACKLOG, "Now", At::Top, &block).expect("Now exists");
        let body: Vec<&str> = text.lines().collect();
        assert_eq!(body[0], "## Now");
        assert_eq!(body[1], "");
        assert_eq!(body[2], "- `[ ]` **P6** the writer — rules: B1.");
        assert!(text.contains(&BACKLOG[BACKLOG.find("- `[~]` **S14**").expect("fixture has S14")..]), "{text}");
    }

    #[test]
    fn removing_an_entry_takes_its_blank_line_and_leaves_the_rest() {
        let text = remove_entry(BACKLOG, "S15").expect("S15 exists");
        assert!(!text.contains("S15"), "{text}");
        assert!(text.contains("## Bugs"), "{text}");
        assert!(!text.contains("\n\n\n"), "no double blank line is left behind: {text}");
    }

    #[test]
    fn moving_keeps_the_entry_lines_and_lands_at_the_top() {
        let text = move_entry(BACKLOG, "S15", "Done", At::Top).expect("S15 exists");
        let body: Vec<&str> = text.lines().collect();
        let done = body.iter().position(|line| line == &"## Done").expect("Done exists");
        assert_eq!(body[done + 2], "- `[ ]` **S15** the camera and the ray move — rules: B1.");
        assert!(text.starts_with("## Now\n\n- `[~]` **S14**"), "Now keeps its first entry: {text}");
    }

    #[test]
    fn an_entry_that_joins_the_end_of_a_section_lands_below_the_last_one() {
        let block = vec!["- `[ ]` **S16** a tenth task `files: crates/render/src/lib.rs`".to_owned()];
        let text = insert_entry(BACKLOG, "Now", At::End, &block).expect("Now exists");
        let body: Vec<&str> = text.lines().collect();
        let added = body.iter().position(|line| line.contains("**S16**")).expect("S16 is there");
        let last = body.iter().position(|line| line.contains("**S15**")).expect("S15 is there");
        assert!(added > last, "a new task joins the end of the order: {text}");
        assert_eq!(body[added + 1], "");
        assert_eq!(body[added + 2], "## Bugs", "the next section keeps its blank line: {text}");
    }

    #[test]
    fn the_next_id_is_one_past_the_highest_in_the_phase() {
        assert_eq!(next_id(BACKLOG, "S"), "S16");
        assert_eq!(next_id(BACKLOG, "P"), "P1");
        assert_eq!(next_id(BACKLOG, "X"), "X32");
        let with_gap = "## Now\n\n- `[ ]` **A3** one `files: src/a.rs`\n- `[ ]` **A7** two `files: src/b.rs`\n";
        assert_eq!(next_id(with_gap, "A"), "A8", "an id is never reused, even with a gap");
    }

    #[test]
    fn a_new_entry_carries_its_fields_and_hint() {
        let block = new_entry(
            "S16",
            "the pipelines move",
            &[("Why".to_owned(), "1446 lines".to_owned()), ("Done".to_owned(), "the suite passes".to_owned())],
            "crates/render/src/lib.rs",
        );
        assert_eq!(block[0], "- `[ ]` **S16** the pipelines move");
        assert_eq!(block[1], "  Why: 1446 lines");
        assert_eq!(block[2], "  Done: the suite passes");
        assert_eq!(block[3], "  `files: crates/render/src/lib.rs`");
    }

    #[test]
    fn a_reopen_takes_the_closing_commit_back_out() {
        let closed = append_to_entry(BACKLOG, "S15", " `[x]` `abc1234`").expect("S15 exists");
        let reopened = strip_commit(&closed, "S15").expect("S15 exists");
        assert!(reopened.contains("  `files: crates/scene/src/lib.rs`\n"), "{reopened}");
        assert!(!reopened.contains("abc1234"), "{reopened}");

        let one_line = "- `[x]` **A2** old task — `0833b4c`\n";
        assert_eq!(strip_commit(one_line, "A2").expect("A2 exists"), "- `[x]` **A2** old task\n");
    }

    #[test]
    fn a_step_is_added_and_ticked() {
        let session =
            "# Session\n\n```text\ntask: P6\n\nsteps:\n  [x] the layout\n  [ ] the writer\n\nnow: the writer\n```\n";
        let added = add_step(session, "the verbs").expect("the note has a steps block");
        assert!(added.contains("  [ ] the writer\n  [ ] the verbs\n"), "{added}");
        let ticked = tick_step(&added).expect("there is an open step");
        assert!(ticked.contains("  [x] the writer"), "{ticked}");
        assert!(ticked.contains("  [ ] the verbs"), "only one step is ticked: {ticked}");
        assert!(add_step("no steps here\n", "x").is_err());
        assert!(tick_step("task: P6\n").is_err(), "nothing to tick is a refusal");
    }

    #[test]
    fn a_section_body_is_replaced_and_the_headings_around_it_stay() {
        let status = "STATE: milestone=M1 task=A3 state=idle base=abc1234 updated=2026-01-01\n\n\
                      ## Where we are\n\nProse.\n\n## Next\n\nA3 is next.\n\n## Index\n\n| File |\n";
        let text = set_section(status, "Next", "A4 is next: the card.\n\nThen A5.").expect("Next exists");
        assert!(text.contains("## Next\n\nA4 is next: the card.\nThen A5.\n\n## Index"), "{text}");
        assert!(text.contains("## Where we are\n\nProse.\n"), "the section above stays: {text}");
        assert!(set_section(status, "Gone", "x").is_err());
    }

    #[test]
    fn state_fields_are_replaced_or_appended() {
        let status = "# Status\n\nSTATE: milestone=M7 task=S14 state=idle base=abc1234 updated=2026-01-01\n";
        let text = set_state(status, &[("task".to_owned(), "P6".to_owned()), ("state".to_owned(), "doing".to_owned())])
            .expect("the state line parses");
        assert!(text.contains("STATE: milestone=M7 task=P6 state=doing base=abc1234 updated=2026-01-01"), "{text}");
        let added = set_state(status, &[("phase".to_owned(), "A".to_owned())]).expect("the state line parses");
        assert!(added.contains("updated=2026-01-01 phase=A"), "a new key is appended: {added}");
        assert!(set_state("# no state\n", &[]).is_err());
    }
}
