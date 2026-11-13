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

/// The width a section paragraph is wrapped at, so `## Next` stays as narrow as the rest of the plan.
const PROSE_WIDTH: usize = 100;

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
            .ok_or_else(|| format!("the plan has no `## {section}` section"))
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

/// Replaces the `files:` hint of `id`, keeping whatever a closed entry wrote after it.
pub fn set_files(backlog: &str, id: &str, hint: &str) -> Result<String, String> {
    let mut layout = Layout::parse(backlog);
    let entry = layout.entry(id)?.clone();
    let range = layout.field_range(&entry, "files").ok_or_else(|| format!("task {id} has no `files:` hint"))?;
    let tail = layout.lines.get(range.0).map(|line| hint_tail(line).to_owned()).unwrap_or_default();
    layout.lines.splice(range.0..range.1, [format!("{INDENT}`files: {hint}`{tail}")]);
    Ok(layout.render())
}

/// The text after the closing backtick of a `files:` hint, so the commit a closed entry writes on
/// the same line survives an edit that only meant to move the hint.
fn hint_tail(line: &str) -> &str {
    let Some(open) = line.find("`files:") else { return "" };
    let rest = line.get(open.saturating_add(1)..).unwrap_or_default();
    match rest.find('`') {
        Some(close) => rest.get(close.saturating_add(1)..).unwrap_or_default(),
        None => "",
    }
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
/// The prose under `## Next` and `## Where we are` is the part of `plan/STATUS.md` a command owns:
/// the gate requires the first to name the current task, and the second has to describe the tree
/// `STATE: base` points at.
pub fn set_section(text: &str, heading: &str, body: &str) -> Result<String, String> {
    let mut layout = Layout::parse(text);
    let at = layout.heading(heading)?;
    let end = layout.sections.iter().map(|(_, at)| *at).filter(|start| *start > at).min().unwrap_or(layout.lines.len());
    // A section that is already wrapped wider keeps its width: the file's own convention wins over
    // the constant, so rewriting one paragraph does not re-flow the section and its line budget.
    let width =
        layout.lines.get(at.saturating_add(1)..end).unwrap_or_default().iter().map(|line| line.chars().count()).max();
    let width = width.unwrap_or(PROSE_WIDTH).max(PROSE_WIDTH);
    let mut replacement: Vec<String> = vec![String::new()];
    replacement.extend(wrap_prose(body, width));
    if end < layout.lines.len() {
        replacement.push(String::new());
    }
    layout.lines.splice(at.saturating_add(1)..end, replacement);
    Ok(layout.render())
}

/// The line that opens the phase legend, so the reader and the parser find the same paragraph.
pub const LEGEND_LEAD: &str = "Phases come from the shape-base plan; `cargo xtask task phase` owns this list:";

/// The words that mark the legend paragraph, whatever its lead line says.
const LEGEND_MARK: &str = "Phases come from the shape-base plan";

/// The phase legend: one clause per phase letter, and whatever the parser could not read.
///
/// The legend is the intro paragraph that says what each id prefix means. Its canonical form is the
/// lead line followed by one clause per letter — `` `A` — contour `` — wrapped at [`PROSE_WIDTH`]
/// with two-space continuations, sorted by letter. The writer produces that form and the gate reads
/// it; an older hand-written prose legend parses as a problem, so a drift is reported instead of
/// silently keeping a second vocabulary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Legend {
    /// `(letter, text)` in file order.
    pub clauses: Vec<(char, String)>,
    /// One sentence per line the canonical form does not allow.
    pub problems: Vec<String>,
}

impl Legend {
    /// The clause of one letter, when the legend declares it.
    pub fn name(&self, letter: char) -> Option<&str> {
        self.clauses.iter().find(|(declared, _)| *declared == letter).map(|(_, name)| name.as_str())
    }
}

/// The legend paragraph of `plan/BACKLOG.md`, or `None` when the file declares no phases.
pub fn legend(text: &str) -> Option<Legend> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| line.trim_start().starts_with(LEGEND_MARK))?;
    let mut legend = Legend::default();
    if lines.get(start).map(|line| line.trim_end()) != Some(LEGEND_LEAD) {
        legend.problems.push(format!("phase legend must open with `{LEGEND_LEAD}`"));
    }
    let mut open: Option<char> = None;
    for line in lines.iter().skip(start.saturating_add(1)) {
        // A blank line or a heading ends the paragraph, even when a hand edit dropped the blank.
        if line.trim().is_empty() || line.starts_with("## ") {
            break;
        }
        if let Some((letter, body)) = parse_clause(line) {
            if legend.name(letter).is_some() {
                legend.problems.push(format!("phase legend declares `{letter}` twice"));
            } else {
                legend.clauses.push((letter, body));
            }
            open = Some(letter);
            continue;
        }
        let Some(letter) = open else {
            legend.problems.push(format!("phase legend line `{}` is not a `L` — text clause", line.trim()));
            continue;
        };
        match legend.clauses.iter_mut().find(|(declared, _)| *declared == letter) {
            Some((_, body)) if line.starts_with([' ', '\t']) => {
                body.push(' ');
                body.push_str(line.trim());
            }
            _ => legend.problems.push(format!("phase legend line `{}` is not a `L` — text clause", line.trim())),
        }
    }
    Some(legend)
}

/// One canonical clause line: a backticked capital letter, an em dash and its text.
fn parse_clause(line: &str) -> Option<(char, String)> {
    let rest = line.strip_prefix('`')?;
    let letter = rest.chars().next()?;
    if !letter.is_ascii_uppercase() {
        return None;
    }
    let rest = rest.get(letter.len_utf8()..)?.strip_prefix('`')?;
    let body = rest.strip_prefix(" — ")?.trim();
    (!body.is_empty()).then(|| (letter, body.to_owned()))
}

/// The lines of one clause: the letter, then the text wrapped under it.
fn clause_lines(letter: char, text: &str) -> Vec<String> {
    let prefix = format!("`{letter}` — ");
    let mut lines = Vec::new();
    let mut current = prefix.clone();
    for word in text.split_whitespace() {
        let at_start = current == prefix || current == INDENT;
        if !at_start && current.chars().count().saturating_add(1).saturating_add(word.chars().count()) > PROSE_WIDTH {
            lines.push(current);
            current = INDENT.to_owned();
        }
        if !current.ends_with(' ') {
            current.push(' ');
        }
        current.push_str(word);
    }
    if current != prefix {
        lines.push(current);
    }
    lines
}

/// Declares or updates phase letters in the legend, in one splice.
///
/// One call may carry several letters, because a file whose legend is still prose has to gain every
/// letter it uses at once: the gate refuses a plan whose ids name a phase the legend does not. A
/// letter that is already there keeps its position in the sort order and takes the new text; a new
/// letter is added. The paragraph is rewritten in the canonical form, so the writer and the reader
/// never have to agree on two grammars.
pub fn set_phases(text: &str, declares: &[(char, String)]) -> Result<String, String> {
    if declares.is_empty() {
        return Err("`task phase` needs a letter: `task phase K --name \"the order package\"`".to_owned());
    }
    let mut checked: Vec<(char, String)> = Vec::new();
    for (letter, name) in declares {
        let name = name.trim();
        if !letter.is_ascii_uppercase() {
            return Err(format!("`{letter}` is not a phase: a phase is one capital letter, like `K`"));
        }
        if name.is_empty() {
            return Err(format!("phase {letter} needs a name: `task phase {letter} --name \"…\"`"));
        }
        if name.contains('\n') {
            return Err(format!("phase {letter}'s name must be one line"));
        }
        checked.push((*letter, name.to_owned()));
    }
    let mut layout = Layout::parse(text);
    let mut clauses = legend(text).map(|legend| legend.clauses).unwrap_or_default();
    for (letter, name) in checked {
        match clauses.iter_mut().find(|(seen, _)| *seen == letter) {
            Some(entry) => entry.1 = name,
            None => clauses.push((letter, name)),
        }
    }
    clauses.sort_by_key(|(letter, _)| *letter);
    let mut block = vec![LEGEND_LEAD.to_owned()];
    for (letter, name) in &clauses {
        block.extend(clause_lines(*letter, name));
    }
    match layout.lines.iter().position(|line| line.trim_start().starts_with(LEGEND_MARK)) {
        Some(start) => {
            let mut end = start;
            while end < layout.lines.len() && !layout.lines[end].trim().is_empty() {
                end = end.saturating_add(1);
            }
            layout.lines.splice(start..end, block);
        }
        None => {
            // A file with no legend gains one above its first section, where the intro prose ends.
            let at = layout.sections.iter().map(|(_, at)| *at).min().unwrap_or(layout.lines.len());
            let mut inserted = Vec::new();
            // One blank line on each side: the intro already ends its paragraph with one.
            if at > 0 && !layout.lines.get(at.saturating_sub(1)).is_some_and(|line| line.trim().is_empty()) {
                inserted.push(String::new());
            }
            inserted.append(&mut block);
            inserted.push(String::new());
            layout.lines.splice(at..at, inserted);
        }
    }
    Ok(layout.render())
}

/// Whether `text` has a `## <heading>` section, so a verb knows whether it has to own it.
pub fn has_section(text: &str, heading: &str) -> bool {
    Layout::parse(text).sections.iter().any(|(name, _)| name.eq_ignore_ascii_case(heading))
}

/// Sets one or more `key=value` fields on the single `STATE:` line.///
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

/// The prose of a section, with the blank lines between paragraphs kept.
///
/// A line that already fits [`PROSE_WIDTH`] is copied as it is: a caller that hands the tool the
/// plan's own text gets it back byte for byte, so a rewrite of one paragraph does not re-flow the
/// section and push a file over its line budget. Only a line that is too long is wrapped.
fn wrap_prose(body: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        // The indentation a bullet's continuation lines carry is part of the file, not noise.
        let indent = raw.get(..raw.len().saturating_sub(raw.trim_start().len())).unwrap_or_default();
        if raw.trim_end().chars().count() <= width {
            lines.push(raw.trim_end().to_owned());
            continue;
        }
        let room = width.saturating_sub(indent.chars().count()).max(1);
        let mut current = String::new();
        for word in trimmed.split_whitespace() {
            if !current.is_empty()
                && current.chars().count().saturating_add(1).saturating_add(word.chars().count()) > room
            {
                lines.push(format!("{indent}{current}"));
                current = String::new();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(format!("{indent}{current}"));
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The wrapped lines of one labelled field, `Label: body` on the first.
///
/// A body whose lines already fit [`FIELD_WIDTH`] is copied line for line, so setting one field
/// does not re-flow the whole of it: the diff a session reviews is the change it made.
fn field_lines(label: &str, body: &str) -> Vec<String> {
    let fits = body.lines().all(|line| line.chars().count() <= FIELD_WIDTH);
    if fits && body.lines().count() > 1 {
        let mut lines: Vec<String> = body.lines().map(|line| format!("{INDENT}{}", line.trim_end())).collect();
        if let Some(first) = lines.first_mut() {
            *first = format!("{INDENT}{label}: {}", first.trim_start_matches(INDENT));
        }
        return lines;
    }
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
    fn a_closed_entry_keeps_the_commit_written_after_its_hint() {
        // The hint and the commit share a line in `## Done`; moving the hint may not drop the hash.
        let done = "## Now\n\n## Done\n\n- `[x]` **S10** a move — `0bd1f6e`.\n  \
                     `files: crates/geom/src/old.rs, crates/geom/src/lib.rs` — `0bd1f6e`\n";
        let text = set_files(done, "S10", "crates/geom/src/rect.rs, crates/geom/src/lib.rs").expect("S10 has a hint");
        assert!(text.contains("`files: crates/geom/src/rect.rs, crates/geom/src/lib.rs` — `0bd1f6e`"), "{text}");
        assert_eq!(text.matches("0bd1f6e").count(), 2, "the subject and the hint line keep their hash: {text}");
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
    fn a_section_rewrite_keeps_each_line_that_already_fits() {
        // A bulleted section, wrapped by hand at 100, keeps its indent, its breaks and its width:
        // a session that changes one paragraph must not re-flow the rest of the file.
        let status = "STATE: milestone=M1 task=A3 state=idle base=abc1234 updated=2026-01-01\n\n\
                      ## Where we are\n\n\
                      - M7 — objects. `Container` is the only container kind, every node carries typed\n\
                      \x20 properties.\n\n- S7 audited the rules.\n\n## Next\n\nA3 is next.\n";
        let body = "- M7 — objects. `Container` is the only container kind, every node carries typed\n  properties.\n\n- S7 audited the rules.\n- S8 gave the fixtures one definition.\n";
        let text = set_section(status, "Where we are", body).expect("the section exists");
        assert!(text.contains("  properties.\n\n- S7 audited the rules.\n"), "{text}");
        assert!(text.contains("- S8 gave the fixtures one definition."), "{text}");
    }

    #[test]
    fn a_section_written_wider_keeps_its_own_width() {
        let wide = "w".repeat(105);
        let status =
            format!("STATE: milestone=M1 task=A3 state=idle base=abc1234 updated=2026-01-01\n\n## Next\n\n{wide}\n");
        let text = set_section(&status, "Next", &format!("{wide} {wide}")).expect("Next exists");
        let body = text.split("## Next\n\n").nth(1).unwrap_or_default();
        assert!(body.lines().count() >= 2, "the paragraph wraps: {text}");
        assert!(
            body.lines().take(2).all(|line| line.chars().count() <= 105),
            "the section's own width wins over the constant: {text}"
        );
    }

    #[test]
    fn a_section_body_is_replaced_and_the_headings_around_it_stay() {
        let status = "STATE: milestone=M1 task=A3 state=idle base=abc1234 updated=2026-01-01\n\n\
                      ## Where we are\n\nProse.\n\n## Next\n\nA3 is next.\n\n## Index\n\n| File |\n";
        let text = set_section(status, "Next", "A4 is next: the card.\n\nThen A5.").expect("Next exists");
        assert!(text.contains("## Next\n\nA4 is next: the card.\n\nThen A5.\n\n## Index"), "{text}");
        assert!(text.contains("## Where we are\n\nProse.\n"), "the section above stays: {text}");
        assert!(set_section(status, "Gone", "x").is_err());
    }

    /// The intro of a plan that declares two phases the canonical way.
    fn legend_backlog() -> String {
        format!(
            "# Backlog\n\nIntro prose.\n\n{LEGEND_LEAD}\n`A` — contour\n`K` — the order package, its writer and the \
             money it carries\n\n## Now\n\n- `[ ]` **A3** arcs `files: src/a.rs`\n\n## Done\n"
        )
    }

    #[test]
    fn the_legend_parses_its_clauses_and_their_continuations() {
        let parsed = legend(&legend_backlog()).expect("the file declares phases");
        assert!(parsed.problems.is_empty(), "{parsed:?}");
        assert_eq!(parsed.clauses.len(), 2);
        assert_eq!(parsed.name('A'), Some("contour"));
        assert_eq!(parsed.name('K'), Some("the order package, its writer and the money it carries"));
        assert!(legend("# Backlog\n\nNo phases here.\n").is_none(), "no legend is not an error");
    }

    #[test]
    fn a_prose_legend_is_reported_rather_than_guessed() {
        let prose = format!(
            "# Backlog\n\n{LEGEND_LEAD}\nPhases: A contour, B contour operations.\n\n## Now\n\n- `[ ]` **A3** arcs \
             `files: src/a.rs`\n\n## Done\n"
        );
        let parsed = legend(&prose).expect("the paragraph is there");
        assert_eq!(parsed.clauses.len(), 0);
        assert!(!parsed.problems.is_empty(), "a prose clause line names what it wants");
    }

    #[test]
    fn a_declaration_updates_one_clause_and_adds_another() {
        let text =
            set_phases(&legend_backlog(), &[('A', "the contour, drawn".to_owned()), ('M', "milestone".to_owned())])
                .expect("the legend is written");
        let parsed = legend(&text).expect("the legend is still there");
        assert_eq!(parsed.name('A'), Some("the contour, drawn"));
        assert_eq!(parsed.name('M'), Some("milestone"));
        assert_eq!(parsed.clauses.iter().map(|(letter, _)| *letter).collect::<Vec<_>>(), vec!['A', 'K', 'M']);
        assert!(text.contains("`A` — the contour, drawn\n"), "the clause is one line: {text}");
        assert!(text.contains("## Now\n\n- `[ ]` **A3**"), "the rest of the file is untouched: {text}");
    }

    #[test]
    fn a_file_without_a_legend_gains_one_above_its_first_section() {
        let plain = "# Backlog\n\nIntro.\n\n## Now\n\n- `[ ]` **A3** arcs `files: src/a.rs`\n\n## Done\n";
        let text = set_phases(plain, &[('A', "contour".to_owned())]).expect("the legend is added");
        assert!(text.contains(&format!("Intro.\n\n{LEGEND_LEAD}\n`A` — contour\n\n## Now")), "{text}");
        assert_eq!(legend(&text).and_then(|legend| legend.name('A').map(str::to_owned)), Some("contour".to_owned()));
    }

    #[test]
    fn a_long_clause_wraps_under_its_letter_and_reads_back_whole() {
        let long = "the order package: one manifest and ten flat tables other programs read, with a price per line "
            .to_owned()
            + "and a cutting plan per sheet, so a workshop can price the piece";
        let text = set_phases(&legend_backlog(), &[('K', long.to_owned())]).expect("the legend is written");
        let parsed = legend(&text).expect("the legend reads back");
        assert_eq!(parsed.name('K'), Some(long.as_str()));
        assert!(text.lines().all(|line| line.chars().count() <= PROSE_WIDTH), "every line fits: {text}");
    }

    #[test]
    fn a_phase_needs_one_letter_and_a_name() {
        let backlog = legend_backlog();
        assert!(set_phases(&backlog, &[]).is_err());
        assert!(set_phases(&backlog, &[('k', "lowercase".to_owned())]).is_err());
        assert!(set_phases(&backlog, &[('K', "  ".to_owned())]).is_err());
        assert!(set_phases(&backlog, &[('K', "two\nlines".to_owned())]).is_err());
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
