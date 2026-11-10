//! The task report: the plan half comes from `state-contract`, the code half from this workspace.
//!
//! `cargo xtask task <id>` is what a session starts with, so it prints everything a reader would
//! otherwise grep together: the entry with its fields, the note the previous session left, the map
//! rows, the debt and questions the task touches, the ADRs it cites — and the facts only the
//! checkout knows: production and test lines per hinted file, the last commit that touched it, and
//! the paths the task creates.
//!
//! Three forms: the default framed block for a human, `--md` for an agent (no box drawing, every
//! label explicit), and `--paths` for a shell loop. `cargo xtask next` is `task` with no id: it
//! takes the task `STATE:` names.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;

use state_contract::{Git, ProcessGit, Section, Style, Summary, TaskOptions, TaskView};

/// What the caller asked for, printed one way or another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// The framed blocks a human reads.
    Human,
    /// The labelled text an agent reads, with no box drawing to parse around.
    Markdown,
    /// One path per line: what the task names, existing first, then what it creates.
    Paths,
}

/// The flags `task` accepts.
struct Flags {
    id: Option<String>,
    form: Form,
    all: bool,
    width: usize,
    no_color: bool,
    /// Whether ANSI codes reach the terminal: `--no-color` or a pipe turns them off.
    color: bool,
}

/// One file the task names: what the checkout says about it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fact {
    path: String,
    exists: bool,
    /// Lines before the last `#[cfg(test)]`, the metric `plan/refactor.md` measures.
    production: usize,
    tests: usize,
    /// The short hash and subject of the last commit that touched the file.
    last: Option<String>,
}

impl Fact {
    /// How the human table spells the file: a missing path is what the task creates.
    fn human_row(&self) -> String {
        if self.exists {
            // The path comes first: it is what a reader looks for and what truncation may not eat.
            format!("{} · {}p {}t · {}", self.path, self.production, self.tests, self.short_hash())
        } else {
            format!("{} · to create", self.path)
        }
    }

    /// The commit hash alone: the subject is for the agent form, which has room for it.
    fn short_hash(&self) -> &str {
        self.last.as_deref().and_then(|last| last.split_once(' ').map(|(hash, _)| hash)).unwrap_or("-")
    }
}

/// `task [<id>] [--md|--paths] [--all] [--width <n>] [--no-color]`, or `next` with no arguments.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    let summary = state_contract::check(root).map_err(report)?;
    let id = flags.id.clone().unwrap_or_else(|| summary.state.task.clone());
    let view = state_contract::task_view_with(root, &id, &ProcessGit::new(root)).map_err(report)?;
    let facts = facts(root, &view);
    let decisions = decisions(root, &view);
    let text = match flags.form {
        Form::Paths => facts.iter().map(|fact| format!("{}\n", fact.path)).collect(),
        Form::Markdown => markdown(&summary, &view, &facts, &decisions, flags.all),
        Form::Human => human(&summary, &view, &facts, &decisions, &flags),
    };
    print!("{text}");
    Ok(())
}

/// The framed report: the plan blocks, the code facts after the files, the decisions last.
fn human(summary: &Summary, view: &TaskView, facts: &[Fact], decisions: &[String], flags: &Flags) -> String {
    let style = Style::new(flags.width, flags.color);
    let opts = TaskOptions { width: flags.width, color: flags.color, all: flags.all };
    let mut blocks = vec![Section::new("STATE", "plan/STATUS.md").line(plan_line(summary))];
    let mut task = state_contract::task_sections(view, &opts);

    // The code facts belong beside the files the plan names, before the map points anywhere.
    let code = code_section(facts);
    let at = task
        .iter()
        .position(|block| block.title == "FILES")
        .map_or_else(|| task.iter().position(|block| block.title == "MAP").unwrap_or(task.len()), |at| at + 1);
    task.insert(at, code);
    blocks.extend(task);

    if !decisions.is_empty() {
        blocks.push(Section::new("DECISIONS", "docs/decisions/").lines(decisions.to_vec()));
    }
    state_contract::sections(&blocks, style)
}

/// The agent form: every label on its own line, no box drawing, nothing to un-wrap.
fn markdown(summary: &Summary, view: &TaskView, facts: &[Fact], decisions: &[String], all: bool) -> String {
    let task = &view.task;
    let mut out = format!("# {} — {}\n\n", task.id, task.lead);
    out.push_str(&format!("plan: {}\n", plan_line(summary)));
    out.push_str(&format!("status: {} · `## {}`\n", task.status.marker(), task.section));
    if let Some(next) = &view.next {
        out.push_str(&format!("next: {} — {}\n", next.id, next.lead));
    }
    if let Some(files) = &task.files {
        out.push_str(&format!("files: {files}\n"));
    }
    out.push('\n');

    if !task.fields.is_empty() {
        out.push_str("## Detail — plan/BACKLOG.md\n\n");
        for field in &task.fields {
            out.push_str(&format!("**{}** {}\n\n", field.label, field.body));
        }
    }

    out.push_str("## Code — the workspace\n\n");
    out.push_str("| path | production | tests | last commit |\n|---|---|---|---|\n");
    for fact in facts {
        if fact.exists {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                fact.path,
                fact.production,
                fact.tests,
                fact.last.as_deref().unwrap_or("-")
            ));
        } else {
            out.push_str(&format!("| `{}` | - | - | to create |\n", fact.path));
        }
    }
    out.push('\n');

    if view.map.is_empty() {
        out.push_str("## Map — plan/map.md\n\nno row names the task's paths; add one when the feature moves.\n\n");
    } else {
        out.push_str("## Map — plan/map.md\n\n");
        for row in &view.map {
            out.push_str(&format!("- {} — {}\n", row.feature, row.where_));
        }
        out.push('\n');
    }

    for (title, source, entries) in
        [("Debt", "plan/debt.md", view.debt_for(all)), ("Questions", "plan/questions.md", view.questions_for(all))]
    {
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("## {title} — {source}\n\n"));
        for entry in entries {
            out.push_str(&format!("- {entry}\n"));
        }
        out.push('\n');
    }

    if !decisions.is_empty() {
        out.push_str("## Decisions — docs/decisions/\n\n");
        for decision in decisions {
            out.push_str(&format!("- {decision}\n"));
        }
        out.push('\n');
    }

    match &view.notes {
        Some(notes) => out.push_str(&format!("## Notes — plan/notes/{}.md\n\n{notes}\n", task.id)),
        None => out.push_str(&format!("## Notes — plan/notes/{}.md\n\nnone\n\n", task.id)),
    }

    if let Some(session) = &view.session {
        out.push_str("## Session — plan/SESSION.md\n\n");
        for (key, value) in [
            ("goal", &session.goal),
            ("now", &session.now),
            ("blocked", &session.blocked),
            ("run", &session.run),
            ("resume", &session.resume),
        ] {
            if let Some(value) = value {
                out.push_str(&format!("{key}: {value}\n"));
            }
        }
        out.push('\n');
    }
    out
}

/// The `STATE` line the report repeats, so a reader knows which plan the task came from.
fn plan_line(summary: &Summary) -> String {
    let state = &summary.state;
    format!(
        "{} {} · task {} · {} · base {} · updated {} · head {}",
        state.milestone,
        state.phase.as_deref().unwrap_or("-"),
        state.task,
        state.status,
        state.base,
        state.updated,
        summary.head.as_deref().map_or("(no checkout)", short)
    )
}

/// The seven-character form the plan uses for every commit.
fn short(hash: &str) -> &str {
    hash.get(..7).unwrap_or(hash)
}

/// The code facts as one framed block.
fn code_section(facts: &[Fact]) -> Section {
    Section::new("CODE", "the workspace").lines(facts.iter().map(Fact::human_row))
}

/// Every file the task names, with what the checkout says about it.
///
/// The `files:` hint comes first; a path the entry mentions but the hint does not name is added when
/// it exists or looks like a source file, so a `Shape:` sentence that creates a module shows up as
/// "to create" instead of staying a word in a sentence.
fn facts(root: &Path, view: &TaskView) -> Vec<Fact> {
    let git = ProcessGit::new(root);
    let mut paths: Vec<String> = view.task.paths.clone();
    let mentioned = view
        .task
        .fields
        .iter()
        .map(|field| field.body.as_str())
        .chain(view.notes.as_deref().into_iter().flat_map(str::lines))
        .flat_map(path_tokens);
    for path in mentioned {
        let known = root.join(&path);
        if !paths.contains(&path) && path.ends_with(".rs") && (!known.exists() || known.is_file()) {
            paths.push(path);
        }
    }
    paths.into_iter().map(|path| fact(root, &git, path)).collect()
}

/// One file: its size, its test tail and the last commit that touched it.
fn fact(root: &Path, git: &dyn Git, path: String) -> Fact {
    let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
        return Fact { path, exists: false, production: 0, tests: 0, last: None };
    };
    let (production, tests) = line_counts(&text);
    let last = git.last_touch(&path);
    Fact { path, exists: true, production, tests, last }
}

/// Production lines — everything before the last test module — and the test lines that follow.
///
/// The last module, not the first: a file may keep a small test module beside a type and the bulk
/// of its tests at the bottom, and both tails are test lines.
fn line_counts(text: &str) -> (usize, usize) {
    let total = text.lines().count();
    let production =
        text.lines().collect::<Vec<_>>().iter().rposition(|line| line.contains("#[cfg(test)]")).unwrap_or(total);
    (production, total - production)
}

/// The path-like tokens of a sentence: a run with a `/` in it, so a type name never matches.
fn path_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-') {
            current.push(ch);
        } else {
            push_token(&mut current, &mut tokens);
        }
    }
    push_token(&mut current, &mut tokens);
    tokens
}

/// Keeps the run collected so far when it looks like a path, then starts a new one.
fn push_token(current: &mut String, tokens: &mut Vec<String>) {
    let trimmed = current.trim_end_matches(['.', ',', ';', ':', '-', '/']).trim_start_matches(['.', '/']);
    if trimmed.contains('/') && trimmed.chars().any(|ch| ch.is_ascii_alphabetic()) {
        tokens.push(trimmed.to_owned());
    }
    current.clear();
}

/// The ADRs the entry and the note cite, with their titles read from `docs/decisions/`.
fn decisions(root: &Path, view: &TaskView) -> Vec<String> {
    let mut text = view.task.text.clone();
    if let Some(notes) = &view.notes {
        text.push('\n');
        text.push_str(notes);
    }
    let titles = decision_titles(root);
    let mut found: Vec<String> = Vec::new();
    for number in adr_numbers(&text) {
        let title = titles.get(&number).cloned().unwrap_or_else(|| format!("{number}. (no file in docs/decisions/)"));
        if !found.contains(&title) {
            found.push(title);
        }
    }
    found
}

/// The first line of every `docs/decisions/NNNN-*.md`, keyed by the number.
fn decision_titles(root: &Path) -> BTreeMap<String, String> {
    let mut titles = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(root.join("docs/decisions")) else {
        return titles;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = name.split('-').next() else { continue };
        if number.len() != 4 || !number.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Some(first) = text.lines().next() else { continue };
        let title = first.trim_start_matches('#').trim();
        if !title.is_empty() {
            titles.insert(number.to_owned(), title.to_owned());
        }
    }
    titles
}

/// The four-digit ADR numbers `text` cites, including a written range (`ADRs 0034–0035`).
fn adr_numbers(text: &str) -> Vec<String> {
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

/// Prints every violation on its own line and returns the summary as the error.
fn report(violations: Vec<String>) -> String {
    if violations.is_empty() {
        return "the plan is not readable".to_owned();
    }
    for violation in &violations {
        eprintln!("plan: {violation}");
    }
    format!("the plan has {} violation(s)", violations.len())
}

impl Flags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut flags = Flags { id: None, form: Form::Human, all: false, width: 80, no_color: false, color: false };
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            match arg {
                "--md" => flags.form = Form::Markdown,
                "--paths" => flags.form = Form::Paths,
                "--all" => flags.all = true,
                "--no-color" => flags.no_color = true,
                "--width" => {
                    let value = args.get(index).ok_or("--width needs a value")?;
                    index += 1;
                    flags.width =
                        value.parse::<usize>().map_err(|_| format!("--width needs a number, got `{value}`"))?;
                }
                other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
                other => {
                    if flags.id.is_some() {
                        return Err(format!("task takes one id, got `{other}` too"));
                    }
                    flags.id = Some(other.to_owned());
                }
            }
        }
        if flags.width < 24 {
            return Err(format!("--width must be at least 24, got {}", flags.width));
        }
        flags.color = !flags.no_color && std::io::stdout().is_terminal();
        Ok(flags)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use state_contract::{Field, Task, TaskStatus};

    #[test]
    fn production_lines_stop_at_the_last_test_module() {
        let text = "//! module\nfn a() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn b() {}\n}\n";
        assert_eq!(line_counts(text), (3, 5));
        let split = "fn a() {}\n\n#[cfg(test)]\nmod first {}\n\nfn b() {}\n\n#[cfg(test)]\nmod last {}\n";
        assert_eq!(line_counts(split), (7, 2), "a second test module is still test lines");
        assert_eq!(line_counts("fn a() {}\n"), (1, 0), "a file without tests is all production");
    }

    #[test]
    fn only_a_run_with_a_slash_reads_as_a_path() {
        let tokens = path_tokens("move `crates/model/src/lib.rs` and `CornerMove` into plan/notes/A3.md.");
        assert_eq!(tokens, ["crates/model/src/lib.rs", "plan/notes/A3.md"]);
        assert!(path_tokens("`Panel::new` and `--files:` stay words").is_empty());
    }

    #[test]
    fn adr_citations_are_four_digits_and_survive_a_range() {
        assert_eq!(adr_numbers("see ADR 0073 and ADRs 0034–0035, not 12345"), ["0073", "0034", "0035"]);
        assert_eq!(adr_numbers("ADR 0072"), ["0072"]);
        assert!(adr_numbers("ADR 12").is_empty(), "a two-digit number is not an ADR");
        assert!(adr_numbers("no decision here").is_empty());
    }

    #[test]
    fn a_missing_file_is_a_file_the_task_creates() {
        let root = std::env::temp_dir().join(format!("xtask-task-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fact = fact(&root, &ProcessGit::new(&root), "crates/x/src/new.rs".to_owned());
        assert!(!fact.exists);
        assert!(fact.human_row().ends_with("· to create"), "{:?}", fact.human_row());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_exists_reports_its_lines() {
        let root = std::env::temp_dir().join(format!("xtask-task-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/x/src")).expect("the scratch directory is writable");
        std::fs::write(root.join("crates/x/src/old.rs"), "fn a() {}\n\n#[cfg(test)]\nmod tests {}\n")
            .expect("the file is writable");
        let fact = fact(&root, &ProcessGit::new(&root), "crates/x/src/old.rs".to_owned());
        assert!(fact.exists);
        assert_eq!((fact.production, fact.tests), (2, 2));
        assert!(fact.human_row().contains("· 2p 2t ·"), "{:?}", fact.human_row());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_flags_choose_one_form_and_one_id() {
        let flags = Flags::parse(&["S14".to_owned(), "--md".to_owned(), "--all".to_owned()]).expect("the flags parse");
        assert_eq!(flags.id.as_deref(), Some("S14"));
        assert_eq!(flags.form, Form::Markdown);
        assert!(flags.all);
        assert_eq!(Flags::parse(&["--paths".to_owned()]).expect("paths parses").form, Form::Paths);
        assert!(Flags::parse(&["A".to_owned(), "B".to_owned()]).is_err(), "one id is enough");
        assert!(Flags::parse(&["--width".to_owned()]).is_err(), "--width needs a value");
    }

    /// The task the report reads: one field, one path, nothing else.
    fn task() -> Task {
        Task {
            id: "S14".to_owned(),
            status: TaskStatus::Doing,
            section: "Now".to_owned(),
            text: "a task".to_owned(),
            lead: "a task".to_owned(),
            fields: vec![Field { label: "Why".to_owned(), body: "because".to_owned() }],
            files: Some("crates/x/src/old.rs".to_owned()),
            paths: vec!["crates/x/src/old.rs".to_owned()],
        }
    }

    #[test]
    fn the_markdown_form_labels_every_part_and_keeps_the_note() {
        let task = task();
        let view = TaskView {
            task: task.clone(),
            next: None,
            notes: Some("measured `old.rs`\n".to_owned()),
            map: Vec::new(),
            debt: vec!["[mesh] a gap".to_owned()],
            questions: Vec::new(),
            session: None,
        };
        let summary = Summary {
            state: state_contract::State {
                milestone: "M7".to_owned(),
                phase: None,
                task: "S14".to_owned(),
                status: "doing".to_owned(),
                base: "abc1234".to_owned(),
                updated: "2026-11-10".to_owned(),
            },
            tasks: vec![task],
            session: None,
            head: Some("fedcba9c0ffee".to_owned()),
        };
        let facts = vec![Fact {
            path: "crates/x/src/old.rs".to_owned(),
            exists: true,
            production: 2,
            tests: 2,
            last: Some("fedcba9 split".to_owned()),
        }];
        let text = markdown(&summary, &view, &facts, &["0073. A shape".to_owned()], true);
        assert!(text.starts_with("# S14 — a task\n"), "{text}");
        assert!(text.contains("**Why** because"), "{text}");
        assert!(text.contains("| `crates/x/src/old.rs` | 2 | 2 | fedcba9 split |"), "{text}");
        assert!(text.contains("## Map — plan/map.md\n\nno row names"), "{text}");
        assert!(text.contains("- [mesh] a gap"), "{text}");
        assert!(text.contains("- 0073. A shape"), "{text}");
        assert!(text.contains("## Notes — plan/notes/S14.md\n\nmeasured `old.rs`"), "{text}");
        assert!(!text.contains('┌'), "the agent form carries no box drawing: {text}");
    }
}
