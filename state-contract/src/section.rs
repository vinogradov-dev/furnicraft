//! The framed blocks the dashboard and a task report are drawn in.
//!
//! A [`Section`] is a header that names the file it was read from plus the lines below it. The frame
//! is drawn to a fixed width and cuts a line that does not fit; colours are hand-written ANSI codes,
//! painted only when the caller asks for them, so a pipe or a log file stays plain.
//!
//! The renderer is public because a host adds its own sources — `xtask` puts the code facts beside
//! the plan files — and it must use the same frame rather than invent a second one.

use std::fmt::Write as _;

/// The styling: the frame width in columns and whether ANSI codes are painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// The box width in characters, at least 24.
    pub width: usize,
    /// Whether ANSI colour is emitted.
    pub color: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self { width: 80, color: false }
    }
}

impl Style {
    /// A style with the width clamped to what a frame needs.
    pub fn new(width: usize, color: bool) -> Self {
        Self { width: width.max(24), color }
    }

    /// The widest body line this style can hold without truncation.
    pub fn text_width(self) -> usize {
        self.width.saturating_sub(6)
    }
}

/// One framed block: the title, the source file, an optional flag and the body.
///
/// A section with an empty body still prints its header and frame: the source exists, it simply has
/// nothing to say — which is different from a caller that never opens the section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    /// The header title, for example `TASK` or `plan/BACKLOG.md`'s `NOW`.
    pub title: String,
    /// The file the block was read from, printed dimmed in the header.
    pub source: String,
    /// A flag printed bold red in the header, for example `INTERRUPTED`.
    pub marker: Option<String>,
    /// The body lines, printed in order.
    pub lines: Vec<String>,
}

impl Section {
    /// A section titled `title`, read from `source`.
    pub fn new(title: impl Into<String>, source: impl Into<String>) -> Self {
        Self { title: title.into(), source: source.into(), marker: None, lines: Vec::new() }
    }

    /// Adds the flag printed in the header.
    pub fn marker(mut self, marker: impl Into<String>) -> Self {
        self.marker = Some(marker.into());
        self
    }

    /// Appends one body line.
    pub fn line(mut self, line: impl Into<String>) -> Self {
        self.lines.push(line.into());
        self
    }

    /// Appends every line of a pre-formatted body.
    pub fn lines<I: IntoIterator<Item = String>>(mut self, lines: I) -> Self {
        self.lines.extend(lines);
        self
    }

    /// Builds the body by writing into a buffer, for a block with wrapping or per-line styling.
    pub fn body(mut self, body: impl FnOnce(&mut String)) -> Self {
        let mut text = String::new();
        body(&mut text);
        self.lines.extend(text.lines().map(str::to_owned));
        self
    }

    /// Appends one body line, for a caller that builds the block in a loop.
    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Whether the block has no body lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Draws the sections, joined by a blank line.
pub fn sections(blocks: &[Section], style: Style) -> String {
    let style = Style::new(style.width, style.color);
    let mut out: Vec<String> = Vec::with_capacity(blocks.len());
    for block in blocks {
        let mut text = String::new();
        // The frame costs two columns: `┌` and `┐`. A header line is `┌ text ┐`, so its inner field
        // is `width - 2`; a body line is `│ ` + text + pad + ` │`, whose field is two narrower.
        let inner = style.width.saturating_sub(2).max(1);
        let body_inner = inner.saturating_sub(2).max(1);
        let label = paint(&block.title, style.color.then_some(BOLD));
        let source = paint(&block.source, style.color.then_some(DIM));
        let mut head = format!(" {label} ─ {source} ");
        if let Some(marker) = block.marker.as_deref() {
            // Bold red: the flag must stand out inside the header line.
            let painted = paint(&paint(marker, style.color.then_some(RED)), style.color.then_some(BOLD));
            let _ = write!(head, "{painted} ");
        }
        let head = truncate_head(&head, inner);
        let filler = "─".repeat(inner.saturating_sub(display_width(&head)));
        let _ = writeln!(text, "┌{}{}┐", head, filler);

        for line in &block.lines {
            let line = truncate(line, body_inner);
            let pad = " ".repeat(body_inner.saturating_sub(display_width(&line)));
            let _ = writeln!(text, "│ {line}{pad} │");
        }
        let _ = writeln!(text, "└{}┘", "─".repeat(inner));
        out.push(text);
    }
    out.join("\n")
}

// Styling: hand-written ANSI codes, nothing else. `None` paints nothing.
pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BOLD: &str = "\x1b[1m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const YELLOW: &str = "\x1b[33m";

/// Wraps `text` in `code`, when `code` is `Some`; empty text paints nothing.
pub(crate) fn paint(text: &str, code: Option<&str>) -> String {
    match code {
        Some(code) if !text.is_empty() => format!("{code}{text}{RESET}"),
        _ => text.to_owned(),
    }
}

/// The visible width of `text`: ANSI escape sequences take no columns on screen.
pub(crate) fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            skip_escape(&mut chars);
            continue;
        }
        width += 1;
    }
    width
}

/// Consumes the rest of an ANSI escape sequence whose `ESC` was just read, keeping nothing.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if chars.peek() == Some(&'[') {
        chars.next();
        while matches!(chars.peek(), Some(digit) if digit.is_ascii_digit() || *digit == ';') {
            chars.next();
        }
        chars.next();
    }
}

/// The same, keeping the consumed sequence so colours survive a cut.
fn skip_escape_keeping(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, kept: &mut String) {
    if chars.peek() == Some(&'[') {
        kept.extend(chars.next());
        while matches!(chars.peek(), Some(digit) if digit.is_ascii_digit() || *digit == ';') {
            kept.extend(chars.next());
        }
        kept.extend(chars.next());
    }
}

/// Truncates `text` to `width` display columns, ending it with `…`; ANSI codes pass through whole.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            kept.push(ch);
            skip_escape_keeping(&mut chars, &mut kept);
            continue;
        }
        if used + 1 >= width {
            break;
        }
        kept.push(ch);
        used += 1;
    }
    format!("{kept}…")
}

/// Trims a section header to `width` display columns without an ellipsis: the box rule that follows
/// is what fills the line, and cutting off mid-word with `…` would only hide the source path.
fn truncate_head(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            kept.push(ch);
            skip_escape_keeping(&mut chars, &mut kept);
            continue;
        }
        if used + 1 > width {
            break;
        }
        kept.push(ch);
        used += 1;
    }
    // Never leave a dangling reset behind a cut escape sequence: close every open color.
    if kept.contains('\x1b') && !kept.ends_with(RESET) {
        kept.push_str(RESET);
    }
    kept
}

/// Wraps `text` at `width` columns on spaces, so a paragraph fits a frame instead of being cut.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let extra = if current.is_empty() { word.chars().count() } else { 1 + word.chars().count() };
        if !current.is_empty() && current.chars().count() + extra > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sections_prints_nothing() {
        assert_eq!(sections(&[], Style::default()), "");
    }

    #[test]
    fn a_section_names_its_source_and_pads_every_line() {
        let block = Section::new("STATE", "plan/STATUS.md").line("M7 A · task A3").line("second line");
        let text = sections(&[block], Style::new(30, false));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("┌ STATE ─ plan/STATUS.md "), "{text}");
        assert_eq!(display_width(lines[0]), 30, "the frame closes at the width: {text}");
        for line in &lines[1..3] {
            assert_eq!(display_width(line), 30, "every body line is padded to the width: {line:?}");
        }
        assert!(text.contains("│ M7 A · task A3"), "the body is kept: {text}");
        assert!(lines.last().is_some_and(|line| line.starts_with('└')), "{text}");
    }

    #[test]
    fn a_marker_sits_in_the_header() {
        let block = Section::new("SESSION", "plan/SESSION.md").marker("INTERRUPTED").line("task: A3");
        let text = sections(&[block], Style::new(60, true));
        assert!(text.contains("INTERRUPTED"), "{text}");
        assert!(text.contains("\x1b[31m"), "the flag is red when colour is on: {text}");
        assert!(text.contains("\x1b[1m"), "the title is bold when colour is on: {text}");
    }

    #[test]
    fn a_body_built_by_writing_keeps_its_lines() {
        let block = Section::new("LONG", "plan/X.md").body(|out| {
            out.push_str("one\n");
            out.push_str("two");
        });
        assert_eq!(block.lines, ["one", "two"]);
        assert!(!block.is_empty());
    }

    #[test]
    fn a_long_line_is_cut_to_the_frame() {
        let long = "x".repeat(200);
        let text = sections(&[Section::new("LONG", "plan/X.md").line(long)], Style::new(24, false));
        assert!(text.contains('…'), "a line that does not fit is cut with an ellipsis: {text}");
        for line in text.lines() {
            assert!(display_width(line) <= 24, "no line may exceed the width: {line:?}");
        }
    }

    #[test]
    fn colour_sequences_take_no_columns() {
        let painted = paint("STATE", Some(BOLD));
        assert_eq!(display_width(&painted), 5, "STATE is five columns, the codes are none");
        assert_eq!(truncate("abcdef", 3), "ab…");
    }

    #[test]
    fn a_style_never_goes_below_the_frame_minimum() {
        let style = Style::new(4, false);
        assert_eq!(style.width, 24);
        assert_eq!(style.text_width(), 18);
    }

    #[test]
    fn wrap_breaks_on_spaces_and_keeps_every_word() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("", 10), Vec::<String>::new());
        assert_eq!(wrap("one  two", 10), ["one two"], "runs of spaces collapse");
        let joined: String = wrap("a word that will not fit in ten", 10).join(" ");
        assert_eq!(joined, "a word that will not fit in ten", "wrapping loses no word");
    }
}
