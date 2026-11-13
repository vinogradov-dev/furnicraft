//! The project skills stay honest about the tree and the commands they name.
//!
//! A skill that points at a path or a `cargo xtask` command that is gone sends the next session to
//! nothing, and the router line in `AGENTS.md` §1.3 is the only thing that makes the design skill
//! load reliably. `pi` also parses frontmatter as strict YAML, so an unquoted `:` in a description
//! silently loads no skill at all. Both are the same rot the plan gate rejects for `plan/map.md` and
//! the docs gate for `docs/decisions/`: read the tree, and fail with a sentence naming the file.
//!
//! What counts as a claim: a backticked token or a markdown link that starts with a repository root
//! (`apps`, `assets`, `crates`, `docs`, `plan`, `state-contract`, `vendor`, `xtask`, `.pi`) or with
//! `references/`, the one relative form a skill uses for its own siblings. A bare file name, a
//! command id and a `<placeholder>` are prose, not a claim, and are not checked.

use std::path::{Path, PathBuf};

use state_contract::cli::COMMANDS;

/// The router document: its §1.3 is what makes the design skill load reliably.
const AGENTS: &str = "AGENTS.md";
/// Where the skills and the slash prompts live.
const SKILLS: &str = ".pi/skills";
const PROMPTS: &str = ".pi/prompts";
/// The skill `AGENTS.md` §1.3 must name.
const DESIGN_SKILL: &str = ".pi/skills/ui-design/SKILL.md";
/// Line budgets: a skill is a procedure, not a document.
const SKILL_BUDGET: usize = 200;
const REFERENCE_BUDGET: usize = 150;
/// The repository roots a token may start with. Anything else is prose: `actions/delete` is a
/// command id, `n/a` a verdict, `contour/*` an area.
const ROOTS: &[&str] = &["apps", "assets", "crates", "docs", "plan", "state-contract", "vendor", "xtask", ".pi"];
/// Paths that exist only while a task is live; their absence from an idle tree is not rot.
const EPHEMERAL: &[&str] = &["plan/SESSION.md"];
/// How deep the walk goes. The skills tree is two levels; a runaway directory is not a task.
const MAX_DEPTH: usize = 4;

/// Checks the skills, their references and the prompts; returns how many files were read.
///
/// `host` is the runner's own command table, so `cargo xtask layers` in a skill is checked against
/// the commands this binary actually dispatches. Provides the plan gate for the same reason.
pub fn check(root: &Path, host: &[&str]) -> Result<usize, Vec<String>> {
    let files = collect(root)?;
    let skills = subdirectories(&root.join(SKILLS));
    let known = commands(host);
    let mut violations = Vec::new();

    for file in &files {
        let shown = shown(root, file);
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                violations.push(format!("cannot read `{shown}`: {error}"));
                continue;
            }
        };
        check_paths(root, file, &shown, &skills, &text, &mut violations);
        check_commands(&shown, &text, &known, &mut violations);
        check_frontmatter(file, &shown, &text, &mut violations);
        check_budget(&shown, &text, &mut violations);
    }
    check_router(root, &mut violations);

    if violations.is_empty() { Ok(files.len()) } else { Err(violations) }
}

/// The router, every markdown file under `.pi/skills`, and every prompt.
fn collect(root: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let agents = root.join(AGENTS);
    if !agents.is_file() {
        return Err(vec![format!("`{AGENTS}` is missing")]);
    }
    let mut files = vec![agents];
    walk(&root.join(SKILLS), 0, &mut files).map_err(|error| vec![error])?;
    let prompts = root.join(PROMPTS);
    let entries = std::fs::read_dir(&prompts).map_err(|error| vec![format!("cannot read `{PROMPTS}/`: {error}")])?;
    for entry in entries.flatten() {
        let path = entry.path();
        if markdown(&path) {
            files.push(path);
        }
    }
    if files.len() < 2 {
        return Err(vec![format!("`{SKILLS}/` and `{PROMPTS}/` hold no skill or prompt")]);
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk(directory: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries =
        std::fs::read_dir(directory).map_err(|error| format!("cannot read `{}`: {error}", directory.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth.saturating_add(1), files)?;
        } else if markdown(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn markdown(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|extension| extension.to_str()) == Some("md")
}

/// The directories directly under `.pi/skills`, one per skill.
fn subdirectories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
    entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect()
}

fn commands<'a>(host: &'a [&'a str]) -> Vec<&'a str> {
    let mut known: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
    known.push("help");
    known.extend_from_slice(host);
    known
}

fn shown(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap_or(file).display().to_string()
}

/// Every claim about a path a file makes: backticked tokens and markdown link targets.
fn check_paths(root: &Path, file: &Path, shown: &str, skills: &[PathBuf], text: &str, violations: &mut Vec<String>) {
    let base = file.parent().unwrap_or(root);
    for token in backticks(text) {
        for expanded in expanded(token) {
            if !looks_like_path(&expanded) || EPHEMERAL.contains(&cleaned(&expanded)) {
                continue;
            }
            if !resolves(root, base, skills, &expanded, true) {
                violations.push(format!("`{shown}` names `{expanded}`, which does not exist"));
            }
        }
    }
    for target in links(text) {
        if !looks_like_path(target) || EPHEMERAL.contains(&cleaned(target)) {
            continue;
        }
        // A link is a real navigation, so it resolves against the file and nowhere else: a broken
        // relative link is exactly the rot a reader would hit.
        if !resolves(root, base, skills, target, false) {
            violations.push(format!("`{shown}` links to `{target}`, which does not exist"));
        }
    }
}

/// The contents of every inline code span, in order. An empty pair — the fences of a code block —
/// yields nothing.
fn backticks(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(offset) = rest.find('`') {
        let after = rest.get(offset.saturating_add(1)..).unwrap_or_default();
        let Some(end) = after.find('`') else { break };
        let token = after.get(..end).unwrap_or_default();
        if !token.is_empty() {
            tokens.push(token);
        }
        rest = after.get(end.saturating_add(1)..).unwrap_or_default();
    }
    tokens
}

/// The targets of every markdown link, with the anchor and the optional title cut off.
fn links(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(offset) = rest.find("](") {
        let after = rest.get(offset.saturating_add(2)..).unwrap_or_default();
        let Some(end) = after.find(')') else { break };
        let target = after.get(..end).unwrap_or_default().trim();
        let target = target.split('#').next().unwrap_or_default();
        let target = target.split_whitespace().next().unwrap_or_default();
        if !target.is_empty() {
            targets.push(target);
        }
        rest = after.get(end.saturating_add(1)..).unwrap_or_default();
    }
    targets
}

/// `a/{b,c}.rs` names two paths; the brace form is a list, not a claim.
fn expanded(token: &str) -> Vec<String> {
    let Some(open) = token.find('{') else { return vec![token.to_owned()] };
    let Some(close) = token.get(open..).and_then(|rest| rest.find('}')).map(|offset| open.saturating_add(offset))
    else {
        return vec![token.to_owned()];
    };
    let head = token.get(..open).unwrap_or_default();
    let body = token.get(open.saturating_add(1)..close).unwrap_or_default();
    let tail = token.get(close.saturating_add(1)..).unwrap_or_default();
    body.split(',').map(|part| format!("{head}{}{tail}", part.trim())).collect()
}

fn cleaned(token: &str) -> &str {
    token.trim_end_matches('/')
}

/// Whether a token claims to be a path at all. `assets/icons/<theme>/` is a shape, `actions/delete`
/// is a command id, `git log` is a command line: none of them is a claim about the tree.
fn looks_like_path(token: &str) -> bool {
    if token.is_empty() || token.len() > 200 {
        return false;
    }
    if token.starts_with('/') || token.starts_with('-') || token.contains(':') {
        return false;
    }
    if token.contains(char::is_whitespace) || token.contains('\\') {
        return false;
    }
    if ['<', '>', '{', '}', '`', '|'].iter().any(|ch| token.contains(*ch)) || token.contains("...") {
        return false;
    }
    // A relative link is the one form that resolves against the file itself, not against a root.
    if token.starts_with("./") || token.starts_with("../") {
        return true;
    }
    let Some((first, _)) = token.split_once('/') else { return false };
    ROOTS.contains(&first) || first == "references"
}

/// Whether a token names something that exists. A backticked token also resolves against the project
/// root and against a skill directory, because a prompt cites `references/checklist.md` of a skill.
fn resolves(root: &Path, base: &Path, skills: &[PathBuf], token: &str, relaxed: bool) -> bool {
    let token = cleaned(token);
    if token.is_empty() {
        return false;
    }
    let mut bases: Vec<&Path> = vec![base];
    if relaxed {
        bases.push(root);
        bases.extend(skills.iter().map(PathBuf::as_path));
    }
    if token.contains('*') {
        return bases.iter().any(|dir| matches_glob(dir, token));
    }
    bases.iter().any(|dir| dir.join(token).exists())
}

/// A path whose segments may carry a wildcard. A `**` segment matches anywhere below the segments
/// before it, which is the form a document uses to say "every skill file".
fn matches_glob(base: &Path, token: &str) -> bool {
    let segments: Vec<&str> = token.split('/').filter(|segment| !segment.is_empty()).collect();
    if let Some(index) = segments.iter().position(|segment| *segment == "**") {
        let (head, tail) = segments.split_at(index);
        let tail = tail.get(1..).unwrap_or_default();
        let [pattern] = tail else { return false };
        let start = head.iter().fold(base.to_path_buf(), |path, segment| path.join(segment));
        return deep_glob(&start, pattern, 0);
    }
    walk_glob(base, &segments, 0)
}

fn walk_glob(directory: &Path, segments: &[&str], depth: usize) -> bool {
    let Some((first, rest)) = segments.split_first() else { return directory.exists() };
    if depth > MAX_DEPTH {
        return false;
    }
    if !first.contains('*') {
        return walk_glob(&directory.join(first), rest, depth.saturating_add(1));
    }
    let Ok(entries) = std::fs::read_dir(directory) else { return false };
    entries.flatten().any(|entry| {
        wildcard(first, &entry.file_name().to_string_lossy()) && walk_glob(&entry.path(), rest, depth.saturating_add(1))
    })
}

fn deep_glob(directory: &Path, pattern: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(directory) else { return false };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if deep_glob(&path, pattern, depth.saturating_add(1)) {
                return true;
            }
        } else if wildcard(pattern, &entry.file_name().to_string_lossy()) {
            return true;
        }
    }
    false
}

fn wildcard(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.len() >= head.len().saturating_add(tail.len()) && name.starts_with(head) && name.ends_with(tail)
        }
    }
}

/// Every `cargo xtask <word>` a file names must be a command the runner dispatches.
fn check_commands(shown: &str, text: &str, known: &[&str], violations: &mut Vec<String>) {
    const PREFIX: &str = "cargo xtask";
    let mut rest = text;
    while let Some(offset) = rest.find(PREFIX) {
        let after = rest.get(offset.saturating_add(PREFIX.len())..).unwrap_or_default();
        let trimmed = after.trim_start();
        let length = trimmed.bytes().take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'-').count();
        let word = trimmed.get(..length).unwrap_or_default();
        if !word.is_empty() && !known.contains(&word) {
            violations.push(format!(
                "`{shown}` names `{PREFIX} {word}`, which is not a command of the tool (see `cargo xtask help`)"
            ));
        }
        rest = trimmed.get(length..).unwrap_or_default();
    }
}

/// The frontmatter the loader reads: a skill's `name` is its folder, and every `description` is a
/// quoted, non-empty scalar.
fn check_frontmatter(file: &Path, shown: &str, text: &str, violations: &mut Vec<String>) {
    let skill = file.file_name().and_then(|name| name.to_str()) == Some("SKILL.md");
    let Some(front) = frontmatter(text) else {
        if skill {
            violations.push(format!("`{shown}` has no `---` frontmatter block"));
        }
        return;
    };
    if skill {
        let folder = file.parent().and_then(Path::file_name).and_then(|name| name.to_str());
        match field(front, "name") {
            None => violations.push(format!("`{shown}` has no `name` in its frontmatter")),
            Some(raw) if Some(unquote(raw)) != folder => violations.push(format!(
                "`{shown}` declares `name: {}`, but its folder is `{}`",
                unquote(raw),
                folder.unwrap_or_default()
            )),
            Some(_) => {}
        }
    }
    match field(front, "description") {
        None => violations.push(format!("`{shown}` has no `description` in its frontmatter")),
        Some("") => violations.push(format!("`{shown}` has an empty `description`")),
        Some(raw) if !quoted(raw) => violations.push(format!(
            "`{shown}` has an unquoted `description`; the loader parses strict YAML, so a bare `:` makes it unloadable"
        )),
        Some(_) => {}
    }
}

/// The lines between the two `---` fences.
fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    rest.get(..end)
}

/// The value of one top-level frontmatter key.
fn field<'a>(front: &'a str, key: &str) -> Option<&'a str> {
    front.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim())
    })
}

fn unquote(value: &str) -> &str {
    value.trim_matches(['"', '\''])
}

fn quoted(value: &str) -> bool {
    let first = value.chars().next();
    let last = value.chars().last();
    first == last && matches!(first, Some('"' | '\''))
}

/// A skill is a procedure and a reference a lookup: both have a budget.
fn check_budget(shown: &str, text: &str, violations: &mut Vec<String>) {
    if !shown.starts_with(SKILLS) {
        return;
    }
    let lines = text.lines().count();
    let (kind, budget) =
        if shown.ends_with("SKILL.md") { ("a `SKILL.md`", SKILL_BUDGET) } else { ("a reference", REFERENCE_BUDGET) };
    if lines > budget {
        violations.push(format!("`{shown}` is {lines} lines; {kind} is at most {budget}"));
    }
}

/// `AGENTS.md` §1.3 is the only thing that makes the design skill load reliably.
fn check_router(root: &Path, violations: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(root.join(AGENTS)) else { return };
    let Some(section) = section(&text, "### 1.3") else {
        violations.push(format!("`{AGENTS}` has no §1.3 section"));
        return;
    };
    if !section.contains(DESIGN_SKILL) {
        violations.push(format!(
            "`{AGENTS}` §1.3 does not name `{DESIGN_SKILL}`, so the design skill does not load reliably"
        ));
    }
}

/// One section of a markdown file: from its heading to the next one of the same or a higher level.
fn section<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let start = text.find(heading)?;
    let body = text.get(start..)?;
    let rest = body.get(heading.len()..)?;
    let end = rest
        .match_indices("\n## ")
        .chain(rest.match_indices("\n### "))
        .map(|(offset, _)| offset)
        .min()
        .unwrap_or(rest.len());
    body.get(..heading.len().saturating_add(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTER: &str = "# Agents\n\n### 1.3 Designing the interface\n\nRead `.pi/skills/ui-design/SKILL.md`.\n\n### 1.4 Planning\n\nNothing here.\n";

    /// A scratch project with the given files, written as `(relative path, contents)`.
    fn project(name: &str, files: &[(&str, String)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-skills-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch directory is writable");
        for (relative, contents) in files {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("the scratch directory is writable");
            }
            std::fs::write(path, contents).expect("the file is writable");
        }
        root
    }

    fn skill(name: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: \"a skill\"\nlicense: MIT\n---\n\n# A skill\n\n{body}\n")
    }

    fn healthy() -> Vec<(&'static str, String)> {
        vec![
            ("AGENTS.md", ROUTER.to_owned()),
            (".pi/skills/ui-design/SKILL.md", skill("ui-design", "Read `references/checklist.md`.")),
            (".pi/skills/ui-design/references/checklist.md", "- a rubric line\n".to_owned()),
            (".pi/prompts/review.md", "---\ndescription: \"review it\"\n---\n\nRun `cargo xtask layers`.\n".to_owned()),
        ]
    }

    fn violations_of(root: &Path) -> Vec<String> {
        check(root, &["layers", "ci"]).expect_err("the tree is refused")
    }

    #[test]
    fn a_healthy_tree_passes() {
        let root = project("ok", &healthy());
        assert_eq!(check(&root, &["layers", "ci"]).expect("the tree is honest"), 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_that_is_gone_is_named_with_its_file() {
        let mut files = healthy();
        files[1].1 = skill("ui-design", "See `crates/ui/src/gone.rs` and `references/checklist.md`.");
        let root = project("gone", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("`.pi/skills/ui-design/SKILL.md`")
                && violation.contains("`crates/ui/src/gone.rs`")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reference_the_skill_does_not_have_is_named() {
        let mut files = healthy();
        files[1].1 = skill("ui-design", "Load `references/human-factors.md`.");
        let root = project("reference", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("`references/human-factors.md`")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_prompt_resolves_a_skill_reference_but_a_link_stays_relative() {
        let mut files = healthy();
        files[3].1 = "---\ndescription: \"review it\"\n---\n\nLoad `references/checklist.md`.\n".to_owned();
        let root = project("prompt", &files);
        assert!(check(&root, &["layers", "ci"]).is_ok(), "a prompt may cite a skill's reference");
        let _ = std::fs::remove_dir_all(&root);

        let mut files = healthy();
        files[2].1 = "- a rubric line\n\nSee [`settings`](../../../crates/ui/src/settings.rs).\n".to_owned();
        let root = project("link", &files);
        std::fs::create_dir_all(root.join("crates/ui/src")).expect("the scratch directory is writable");
        std::fs::write(root.join("crates/ui/src/settings.rs"), "// a file\n").expect("the file is writable");
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("links to `../../../crates/ui/src/settings.rs`")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_command_that_does_not_exist_is_named() {
        let mut files = healthy();
        files[3].1 = "---\ndescription: \"review it\"\n---\n\nRun `cargo xtask layerz`.\n".to_owned();
        let root = project("command", &files);
        let violations = violations_of(&root);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("`cargo xtask layerz`") && violation.contains("is not a command")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_frontmatter_must_name_the_folder_and_quote_the_description() {
        let mut files = healthy();
        files[1].1 = skill("ui", "Read `references/checklist.md`.");
        let root = project("name", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("declares `name: ui`")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);

        let mut files = healthy();
        files[1].1 = "---\nname: ui-design\ndescription: Design the interface: a panel and a dialog\n---\n\nA skill.\n"
            .to_owned();
        let root = project("unquoted", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("unquoted `description`")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);

        let mut files = healthy();
        files[1].1 = "---\nname: ui-design\n---\n\nA skill.\n".to_owned();
        let root = project("missing", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("no `description`")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_skill_over_its_budget_is_named() {
        let mut files = healthy();
        let long = "a line\n".repeat(200);
        files[1].1 = skill("ui-design", &long);
        let root = project("budget", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("at most 200") && violation.contains("SKILL.md` is")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_router_must_name_the_design_skill() {
        let mut files = healthy();
        files[0].1 = "# Agents\n\n### 1.3 Designing the interface\n\nRead the design skill.\n".to_owned();
        let root = project("router", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("§1.3 does not name")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prose_is_not_a_path_claim() {
        for token in [
            "actions/delete",
            "contour/*",
            "assets/icons/<theme>/",
            "crates/engine/src/<area>_cmds.rs",
            "plan/notes/<ID>.md",
            "toolbars/{spec,list,state,ready}.rs",
            "references/*.md",
            "git log",
            "n/a",
        ] {
            assert!(!looks_like_path(token) || token == "references/*.md", "{token} is prose or a shape");
        }
        assert!(looks_like_path("crates/ui/src/theme.rs"));
        assert!(looks_like_path(".pi/skills/ui-design/SKILL.md"));
        assert!(looks_like_path("../../../crates/ui/src/settings.rs"), "a relative link is a claim");
    }

    #[test]
    fn a_brace_names_each_path_and_a_star_matches_a_file() {
        let expanded = expanded("crates/ui/src/{theme,icons}.rs");
        assert_eq!(expanded, ["crates/ui/src/theme.rs", "crates/ui/src/icons.rs"]);
        assert!(wildcard("*.md", "STATUS.md"));
        assert!(wildcard("0*-a.md", "0001-a.md"));
        assert!(!wildcard("*.md", "STATUS.toml"));
    }

    #[test]
    fn a_deep_glob_looks_under_the_directory() {
        let root = project("deep", &healthy());
        assert!(resolves(&root, &root, &[], ".pi/skills/**/*.md", true));
        assert!(resolves(&root, &root, &[], ".pi/skills/*/*.md", true));
        assert!(!resolves(&root, &root, &[], ".pi/prompts/**/*.rs", true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The tree this repository ships: a renamed path in a skill fails `cargo test` and `cargo xtask ci`.
    #[test]
    fn the_real_tree_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let host: Vec<&str> = crate::HOST.iter().map(|command| command.name).collect();
        let count = check(&root, &host).expect("the skills name paths, commands and frontmatter that exist");
        assert!(count >= 8, "{count} files checked");
    }
}
