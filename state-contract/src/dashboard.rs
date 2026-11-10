//! A printed overview of the plan: every section names the file it was read from.
//!
//! [`dashboard`] is a pure string builder: it reads `plan/*` through the same parsers as [`crate::check`],
//! lays the result out in fixed-width boxes, and returns the text for the caller to print. It never
//! writes to `plan/*`, never enters raw mode or an alternate screen, and pulls in no dependency: the
//! styling is hand-written ANSI escape codes, disabled by [`DashboardOptions::color`] = `false`.

use std::path::Path;

use crate::parse::{markdown_bullets, parse_backlog, parse_map, parse_session, parse_state, session_field};
use crate::section::{DIM, GREEN, Section, Style, YELLOW, paint, sections, truncate};
use crate::task::{TaskOptions, task_sections, view_from};
use crate::{Git, ProcessGit, Summary, Task, TaskStatus};

/// The backlog tasks one section shows at most; the full list stays in `plan/BACKLOG.md`.
const MAX_TASKS_PER_SECTION: usize = 8;

/// How the dashboard is printed: line width, whether ANSI colors are on, and an optional task to
/// show in full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardOptions {
    /// The outer box width, in characters. Longer lines are truncated with `…`.
    pub width: usize,
    /// Whether to emit ANSI escape codes; `false` prints plain text.
    pub color: bool,
    /// A task id to expand in a `TASK` block, as `cargo xtask task <id>` would print it.
    pub task: Option<String>,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        Self { width: 80, color: true, task: None }
    }
}

/// Reads the plan files under `root` and returns the styled overview, or every violation that
/// prevents one — the same messages [`crate::check`] gives.
///
/// Every call re-reads the files: the dashboard is a live view, not a cache.
pub fn dashboard(root: &Path, opts: &DashboardOptions) -> Result<String, Vec<String>> {
    dashboard_with(root, opts, &ProcessGit::new(root))
}

/// [`dashboard`] with the git facts supplied by `git`, so tests need no repository.
pub fn dashboard_with(root: &Path, opts: &DashboardOptions, git: &dyn Git) -> Result<String, Vec<String>> {
    let status = read(root, "plan/STATUS.md")?;
    let backlog = read(root, "plan/BACKLOG.md")?;
    let (session_path, session) = match read_optional(root, "plan/SESSION.md")? {
        Some(text) => (Some("plan/SESSION.md"), Some(text)),
        None => (None, None),
    };
    let questions = read_optional(root, "plan/questions.md")?;
    let debt = read_optional(root, "plan/debt.md")?;
    let map = read_optional(root, "plan/map.md")?;

    let state = parse_state(&status)?;
    let head = git.head();
    let interrupted = is_interrupted(&state, session.as_deref(), head.as_deref(), git);
    let style = Style::new(opts.width, opts.color);
    let mut blocks: Vec<Section> = Vec::new();

    blocks.push(Section::new("STATE", "plan/STATUS.md").line(format!(
        "{} {} · task {} · {} · base {} · updated {}",
        state.milestone,
        state.phase.as_deref().unwrap_or("-"),
        state.task,
        state.status,
        state.base,
        state.updated
    )));

    if let Some(id) = opts.task.as_deref() {
        let summary = Summary {
            state: state.clone(),
            tasks: parse_backlog(&backlog),
            session: session.as_deref().map(parse_session),
            head: head.clone(),
        };
        let view = view_from(&summary, id, map.as_deref(), debt.as_deref(), questions.as_deref(), None)?;
        let notes = crate::note(root, &view.task.id)?;
        let mut view = view;
        view.notes = notes;
        let opts = TaskOptions { width: opts.width, color: opts.color, all: false };
        blocks.extend(task_sections(&view, &opts));
    }

    let tasks = parse_backlog(&backlog);
    for section in ["Now", "Bugs", "Next"] {
        let listed: Vec<&Task> = tasks.iter().filter(|task| task.section == section).collect();
        if listed.is_empty() {
            continue;
        }
        let mut block = Section::new(section.to_uppercase(), "plan/BACKLOG.md");
        render_tasks(&mut block, &listed, style);
        blocks.push(block);
    }

    if state.status == "doing"
        && let Some(session) = session.as_deref()
    {
        let name = session_path.unwrap_or("plan/SESSION.md");
        let mut block = Section::new("SESSION", name);
        if interrupted {
            block.marker = Some("INTERRUPTED".to_owned());
        }
        for key in ["task", "now", "blocked", "run", "resume"] {
            match session_field(session, key) {
                Some(value) => block.push(format!("{key}: {value}")),
                // An empty field still says something: the note left the question open.
                None => block.push(format!("{key}: -")),
            }
        }
        blocks.push(block);
    }

    if let Some(questions) = questions.as_deref() {
        let bullets = markdown_bullets(questions);
        if !bullets.is_empty() {
            blocks.push(
                Section::new("QUESTIONS", "plan/questions.md")
                    .lines(bullets.into_iter().take(MAX_TASKS_PER_SECTION).map(|bullet| format!("· {bullet}"))),
            );
        }
    }

    if let Some(debt) = debt.as_deref() {
        let counts = tag_counts(debt);
        if !counts.is_empty() {
            blocks.push(Section::new("DEBT", "plan/debt.md").line(counts.join(" · ")));
        }
    }

    // The feature map is an optional starting point: no file, or no table rows, means no block.
    if let Some(map) = map.as_deref() {
        let rows = parse_map(map);
        if !rows.is_empty() {
            blocks.push(
                Section::new("MAP", "plan/map.md")
                    .lines(rows.into_iter().map(|row| format!("{} — {}", row.feature, row.where_))),
            );
        }
    }

    Ok(sections(&blocks, style))
}

/// Whether the session looks cut short: work was running, the plan edit rides on commits past the
/// `base`, and the session note still has unchecked steps.
fn is_interrupted(state: &crate::State, session: Option<&str>, head: Option<&str>, git: &dyn Git) -> bool {
    if state.status != "doing" || session.is_none() {
        return false;
    }
    let Some(head) = head else {
        return false;
    };
    if state.base == head || !git.is_ancestor(&state.base, head) {
        return false;
    }
    has_open_steps(session.unwrap_or(""))
}

/// Whether the `steps:` block of a session note still lists `[ ]` items.
fn has_open_steps(session: &str) -> bool {
    let mut in_steps = false;
    for line in session.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with("steps:") {
            in_steps = true;
            continue;
        }
        if in_steps {
            if trimmed.is_empty() {
                break;
            }
            if !line.starts_with([' ', '\t']) && !trimmed.starts_with("[ ]") && !trimmed.starts_with("[x]") {
                break;
            }
            if trimmed.starts_with("[ ]") {
                return true;
            }
        }
    }
    false
}

/// Writes each task as a marker line plus its dim `files:` hint, capped per section.
fn render_tasks(block: &mut Section, tasks: &[&Task], style: Style) {
    let inner = style.width.saturating_sub(4).max(12);
    for task in tasks.iter().take(MAX_TASKS_PER_SECTION) {
        let painted = paint(task.status.marker(), status_color(task.status, style.color));
        let room = inner.saturating_sub(task.id.len().saturating_add(6));
        block.push(format!("{painted} {}  {}", task.id, truncate(&task.lead, room)));
        if let Some(files) = &task.files {
            let shown = truncate(files, inner.saturating_sub(9));
            block.push(format!("         {}", paint(&shown, if style.color { Some(DIM) } else { None })));
        }
    }
}

/// One `[tag]` count per tag found in the file, in order of appearance.
fn tag_counts(text: &str) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for tag in text.match_indices('[').filter_map(|(offset, _)| {
        let rest = &text[offset + 1..];
        let end = rest.find(']')?;
        let tag = &rest[..end];
        (!tag.is_empty() && tag.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')).then(|| tag.to_owned())
    }) {
        match counts.iter_mut().find(|(found, _)| *found == tag) {
            Some((_, seen)) => *seen += 1,
            None => counts.push((tag, 1)),
        }
    }
    counts.into_iter().map(|(tag, seen)| format!("[{tag}] {seen}")).collect()
}

/// The colour of a backlog marker: neutral open, yellow doing, green done.
fn status_color(status: TaskStatus, color: bool) -> Option<&'static str> {
    if !color {
        return None;
    }
    match status {
        TaskStatus::Open => None,
        TaskStatus::Doing => Some(YELLOW),
        TaskStatus::Done => Some(GREEN),
    }
}

/// Reads a required plan file, reporting the same message shape as [`crate::check`].
fn read(root: &Path, name: &str) -> Result<String, Vec<String>> {
    std::fs::read_to_string(root.join(name)).map_err(|error| vec![format!("cannot read `{name}`: {error}")])
}

/// Reads an optional plan file; `None` when it does not exist.
fn read_optional(root: &Path, name: &str) -> Result<Option<String>, Vec<String>> {
    match std::fs::read_to_string(root.join(name)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(vec![format!("cannot read `{name}`: {error}")]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::Git;
    use crate::section::display_width;

    /// A fake repository: `HEAD` is ahead of the plan's `base` whenever the test asks for it.
    struct FakeGit {
        head: Option<&'static str>,
    }

    impl Git for FakeGit {
        fn head(&self) -> Option<String> {
            self.head.map(str::to_owned)
        }

        fn commit_exists(&self, hash: &str) -> bool {
            // The scratch fixtures name commits that live only in the fixture text; a repository
            // would know them, so every well-formed hash is treated as present.
            !hash.is_empty()
        }

        fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
            ancestor == "abc1234" && Some(ancestor) != Some(descendant) && self.head == Some(descendant)
        }

        fn commit_date(&self, hash: &str) -> Option<String> {
            // The fixtures are written on a date after their base, so the `updated` rule holds.
            (hash == "abc1234").then(|| "2025-12-01".to_owned())
        }
    }

    const IDLE_STATUS: &str = "STATE: milestone=M7 phase=D task=A5 state=idle base=abc1234 updated=2026-10-30\n";
    const DOING_STATUS: &str = "STATE: milestone=M7 phase=D task=D1 state=doing base=abc1234 updated=2026-10-30\n";
    const BACKLOG: &str = "## Now\n\n\
                           - `[~]` **D1** panel from a contour `files: crates/model/src/lib.rs`\n\n\
                           ## Next\n\n\
                           - `[ ]` **A5** contour edit math `files: crates/model/src/contour.rs`\n\n\
                           ## Done\n\n\
                           - `[x]` **A2** ring of segments — `0833b4c`\n";
    const NOTE: &str = "task: D1\ngoal: a contour panel\n\nsteps:\n  [x] outline enum\n  [ ] banding by ring\n\n\
                        now: `nominal_contour` in `crates/model/src/panel_contour.rs`\nblocked: nothing\n\
                        run: cargo test -p furnicraft-model --lib\nresume: read `Panel::geometry_for_with`\n";

    /// A scratch project directory with the given plan files.
    fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("state-contract-dash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("plan")).expect("the scratch directory is writable");
        for (path, text) in files {
            let full = root.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("the parent directory is writable");
            }
            std::fs::write(full, text).expect("the plan file is writable");
        }
        root
    }

    fn opts(width: usize, color: bool) -> DashboardOptions {
        DashboardOptions { width, color, task: None }
    }

    fn head_ahead() -> FakeGit {
        FakeGit { head: Some("fedcba9") }
    }

    /// A repository whose `HEAD` equals the plan's `base`: nothing has landed since the plan edit.
    fn head_same() -> FakeGit {
        FakeGit { head: Some("abc1234") }
    }

    #[test]
    fn an_idle_plan_shows_no_session_block() {
        let root = project("idle", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG)]);
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(text.contains("┌ STATE ─ plan/STATUS.md"), "{text}");
        assert!(text.contains("M7 D · task A5 · idle · base abc1234 · updated 2026-10-30"), "{text}");
        assert!(text.contains("┌ NOW ─ plan/BACKLOG.md"), "{text}");
        assert!(!text.contains("SESSION"), "an idle plan must not show a session block: {text}");
        assert!(!text.contains("QUESTIONS"), "a missing questions.md omits the section: {text}");
        assert!(!text.contains("DEBT"), "a missing debt.md omits the section: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_bug_section_is_its_own_block() {
        let backlog = "## Now\n\n\
                       - `[ ]` **A6** contour edit commands `files: crates/engine/src`\n\n\
                       ## Bugs\n\n\
                       - `[ ]` **X1** a flap triangle across a concave cap `files: crates/mesh/src/lib.rs`\n\n\
                       ## Next\n\n\
                       - `[ ]` **D2** the contour panel card `files: crates/ui/src/panel_card.rs`\n\n\
                       ## Done\n\n\
                       - `[x]` **A2** ring of segments — `0833b4c`\n";
        let status = "STATE: milestone=M7 phase=A task=A6 state=idle base=abc1234 updated=2026-10-30\n";
        let root = project("bugs", &[("plan/STATUS.md", status), ("plan/BACKLOG.md", backlog)]);
        let text = dashboard_with(&root, &opts(80, false), &head_same()).expect("the plan parses");
        assert!(text.contains("┌ BUGS ─ plan/BACKLOG.md"), "a defect needs its own block: {text}");
        assert!(text.contains("X1"), "the defect is listed with its id: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_doing_plan_shows_the_session_fields() {
        let root = project(
            "doing",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/SESSION.md", NOTE)],
        );
        let text = dashboard_with(&root, &opts(80, false), &head_same()).expect("the plan parses");
        assert!(text.contains("┌ SESSION ─ plan/SESSION.md"), "{text}");
        assert!(!text.contains("INTERRUPTED"), "a base equal to HEAD is not an interruption: {text}");
        for line in ["task: D1", "blocked: nothing", "run: cargo test", "resume: read"] {
            assert!(text.contains(line), "the session block must carry `{line}`: {text}");
        }
        assert!(text.contains("nominal_contour"), "the `now:` field names the symbol it stopped at: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stale_base_with_open_steps_is_marked_interrupted_in_red() {
        let root = project(
            "interrupted",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/SESSION.md", NOTE)],
        );
        let text = dashboard_with(&root, &opts(80, true), &head_ahead()).expect("the plan parses");
        assert!(text.contains("\x1b[31mINTERRUPTED\x1b[0m"), "the marker is red: {text}");
        let plain = dashboard_with(&root, &opts(80, false), &head_ahead()).expect("the plan parses");
        assert!(plain.contains("INTERRUPTED"), "the marker survives --no-color: {plain}");
        assert!(!plain.contains('\x1b'), "no escape codes without color: {plain:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_finished_step_list_is_not_an_interruption() {
        let done_steps = NOTE.replace("  [ ] banding by ring", "  [x] banding by ring");
        let root = project(
            "clean",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/SESSION.md", &done_steps)],
        );
        let text = dashboard_with(&root, &opts(80, false), &head_ahead()).expect("the plan parses");
        assert!(!text.contains("INTERRUPTED"), "every step closed means the session ended whole: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn questions_and_debt_appear_only_when_the_files_exist() {
        let files = [
            ("plan/STATUS.md", IDLE_STATUS),
            ("plan/BACKLOG.md", BACKLOG),
            ("plan/questions.md", "# Open questions\n\n- Folder naming in the order registry.\n"),
            ("plan/debt.md", "# Known debt\n\n- `[render]` no shadows.\n- `[render]` no culling.\n"),
        ];
        let root = project("extra", &files);
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(text.contains("┌ QUESTIONS ─ plan/questions.md"), "{text}");
        assert!(text.contains("· Folder naming in the order registry."), "{text}");
        assert!(text.contains("┌ DEBT ─ plan/debt.md"), "{text}");
        assert!(text.contains("[render] 2"), "{text}");
        let _ = std::fs::remove_dir_all(&root);

        let root = project("missing-extra", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG)]);
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(!text.contains("QUESTIONS"), "{text}");
        assert!(!text.contains("DEBT"), "{text}");
        assert!(!text.contains("MAP"), "a missing plan/map.md omits the map section too: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_map_file_becomes_a_block_of_feature_rows() {
        let map = "# Map\n\n| Feature | Where | Notes |\n|---|---|---|\n\
                   | Contour ring | `crates/model/src/lib.rs` (`ContourRing`, `assemble`) | ~4600 lines |\n\
                   | Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) | small |\n";
        let root =
            project("map", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/map.md", map)]);
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(text.contains("┌ MAP ─ plan/map.md"), "{text}");
        assert!(text.contains("Contour ring — crates/model/src/lib.rs (ContourRing, assemble)"), "{text}");
        assert!(text.contains("Panel commands — crates/engine/src/panel_cmds.rs (panel.*)"), "{text}");
        assert!(!text.contains("~4600"), "the `Notes` column stays out of the block: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_or_headerless_map_prints_no_block() {
        let root = project(
            "map-empty",
            &[
                ("plan/STATUS.md", IDLE_STATUS),
                ("plan/BACKLOG.md", BACKLOG),
                ("plan/map.md", "# Map\n\nNothing yet.\n"),
            ],
        );
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(!text.contains("MAP"), "a map with no table rows prints no block: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn long_map_rows_are_truncated_at_the_width() {
        let map = "| Feature | Where | Notes |\n|---|---|---|\n\
                   | A very long feature name indeed | `crates/model/src/some_deeply_nested_module.rs` (`a_long_anchor`) | — |\n";
        let root =
            project("map-cut", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/map.md", map)]);
        let text = dashboard_with(&root, &opts(40, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(text.contains("┌ MAP ─ plan/map.md"), "{text}");
        assert!(text.contains('…'), "a row longer than the width is cut with an ellipsis: {text}");
        for line in text.lines().filter(|line| !line.is_empty()) {
            assert_eq!(display_width(line), 40, "every box line keeps the requested width: {line:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_colored_map_wraps_its_header_but_no_color_stays_plain() {
        let map =
            "| Feature | Where | Notes |\n|---|---|---|\n| Ring | `crates/model/src/lib.rs` (`ContourRing`) | — |\n";
        let files = [("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/map.md", map)];
        let root = project("map-color", &files);
        let colored = dashboard_with(&root, &opts(80, true), &FakeGit { head: None }).expect("the plan parses");
        assert!(colored.contains("\x1b[1mMAP\x1b[0m"), "the header title is bold: {colored}");
        assert!(
            colored.contains("│ Ring — crates/model/src/lib.rs (ContourRing)"),
            "the body line carries no escapes: {colored}"
        );
        let plain = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(plain.contains("┌ MAP ─ plan/map.md"), "--no-color still prints the block: {plain}");
        assert!(!plain.contains('\x1b'), "no escape codes without color: {plain:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn long_lines_are_truncated_at_the_width_with_an_ellipsis() {
        let root = project("wide", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG)]);
        let narrow = dashboard_with(&root, &opts(40, false), &FakeGit { head: None }).expect("the plan parses");
        for line in narrow.lines() {
            assert!(display_width(line) <= 40, "line over the width: {:?}", display_width(line));
        }
        assert!(narrow.contains("…"), "a truncated line ends with the ellipsis: {narrow}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_line_of_a_plain_box_is_exactly_the_requested_width() {
        // The padding counts characters, not display columns: `·` and `─` must each cost one column.
        let root = project(
            "boxwidth",
            &[
                ("plan/STATUS.md", IDLE_STATUS),
                ("plan/BACKLOG.md", BACKLOG),
                ("plan/questions.md", "# Open questions\n\n- Folder naming · version statuses.\n"),
            ],
        );
        let text = dashboard_with(&root, &opts(50, false), &FakeGit { head: None }).expect("the plan parses");
        for line in text.lines().filter(|line| !line.is_empty()) {
            assert_eq!(line.chars().count(), 50, "line is not flush at the width: {line:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_line_of_a_colored_box_is_exactly_the_requested_width() {
        // With ANSI on, the same rule holds over display columns: escape codes take no space.
        let root = project(
            "colorwidth",
            &[
                ("plan/STATUS.md", DOING_STATUS),
                ("plan/BACKLOG.md", BACKLOG),
                ("plan/SESSION.md", NOTE),
                ("plan/questions.md", "# Open questions\n\n- Folder naming · version statuses.\n"),
            ],
        );
        let text = dashboard_with(&root, &opts(50, true), &head_ahead()).expect("the plan parses");
        for line in text.lines().filter(|line| !line.is_empty()) {
            assert_eq!(display_width(line), 50, "colored line is not flush at the width: {line:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_ansi_colored_line_is_measured_without_its_escape_codes() {
        // A colored header must keep the box at exactly `width` visible columns.
        let root = project(
            "colorbox",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/SESSION.md", NOTE)],
        );
        let text = dashboard_with(&root, &opts(60, true), &head_ahead()).expect("the plan parses");
        for line in text.lines() {
            assert!(display_width(line) <= 60, "colored line over the width: {:?}", display_width(line));
        }
        assert!(text.contains("\x1b["), "the output is styled: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn color_wraps_the_marker_but_no_color_prints_plain_text() {
        let root = project(
            "color",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", BACKLOG), ("plan/SESSION.md", NOTE)],
        );
        let colored = dashboard_with(&root, &opts(80, true), &head_ahead()).expect("the plan parses");
        assert!(colored.contains("\x1b[33m[~]\x1b[0m"), "the doing marker is yellow: {colored}");
        // The overview shows `Now` and `Next`; the closed `[x]` markers stay in `plan/BACKLOG.md`,
        // so green is checked against a done task placed in `## Now` by hand.
        let done_now = BACKLOG.replace("- `[~]` **D1**", "- `[x]` **D1**");
        let done_root = project(
            "done-color",
            &[("plan/STATUS.md", DOING_STATUS), ("plan/BACKLOG.md", &done_now), ("plan/SESSION.md", NOTE)],
        );
        let done_colored = dashboard_with(&done_root, &opts(80, true), &head_ahead()).expect("the plan parses");
        assert!(done_colored.contains("\x1b[32m[x]\x1b[0m"), "a done marker is green: {done_colored}");
        assert!(colored.contains("\x1b[2mplan/STATUS.md\x1b[0m"), "the path is dim: {colored}");

        let plain = dashboard_with(&root, &opts(80, false), &head_ahead()).expect("the plan parses");
        assert!(!plain.contains('\x1b'), "--no-color must print plain text: {plain:?}");
        assert!(plain.contains("[~] D1"), "{plain}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&done_root);
    }

    #[test]
    fn a_missing_status_file_reports_the_read_error() {
        let root = project("empty", &[]);
        let errors = dashboard_with(&root, &opts(80, false), &FakeGit { head: None })
            .expect_err("a project without a plan cannot produce a dashboard");
        assert!(errors[0].contains("cannot read `plan/STATUS.md`"), "{errors:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn truncation_keeps_the_char_count_and_marks_the_cut() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("hello world", 1), "…");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn the_next_section_is_labeled_by_the_backlog_it_came_from() {
        let root = project("sections", &[("plan/STATUS.md", IDLE_STATUS), ("plan/BACKLOG.md", BACKLOG)]);
        let text = dashboard_with(&root, &opts(80, false), &FakeGit { head: None }).expect("the plan parses");
        assert!(text.contains("┌ NOW ─ plan/BACKLOG.md"), "{text}");
        assert!(text.contains("┌ NEXT ─ plan/BACKLOG.md"), "{text}");
        assert!(text.contains("[ ] A5  contour edit math"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
