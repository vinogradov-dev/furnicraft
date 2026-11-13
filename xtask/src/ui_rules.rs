//! The interface's two ownership rules, read as Rust instead of as text.
//!
//! A colour literal in a feature module is a defect: it looks right in the dark palette and breaks
//! the light one, and the next palette change never finds it (ADR 0063). A raw `egui` host outside
//! the module that owns the surface is the same kind of defect: it escapes `Chrome::covers` and the
//! one overlay order, and then swallows a click that should have been a pick (ADR 0080). Both are
//! parse checks, not greps: `syn` resolves `use egui::Color32 as C` and `eframe::egui::Area`, so an
//! alias cannot slip past.
//!
//! The owner tables are the allow-list. Adding a surface is one row, and the row says what that
//! module owns; a feature module carries no allowance comment. Test code is not shipped code and is
//! not read: a test may drive an `egui::Context` with whatever host it needs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprCall, ExprPath, ExprStruct, ImplItem, Item, Path as SynPath, UseTree};

/// The two source trees the interface rules read.
const UI_TREES: &[&str] = &["crates/ui/src", "apps/furnicraft/src"];

/// The crate roots: a module declared under `#[cfg(test)]` is not shipped code.
const CRATE_ROOTS: &[&str] = &["crates/ui/src/lib.rs", "apps/furnicraft/src/main.rs"];

/// One module that owns a fact, and the fact it owns.
struct Owner {
    path: &'static str,
    owns: &'static str,
}

/// The modules that may build a colour, and what each one owns.
const COLOUR_OWNERS: &[Owner] = &[
    Owner { path: "crates/ui/src/theme.rs", owns: "the palette and the states mixed from it" },
    Owner { path: "crates/ui/src/icons.rs", owns: "the six semantic role inks an icon master writes (ADR 0090)" },
];

/// The modules that may create an `egui` host, and the surface each one owns.
///
/// Every entry is one floating surface or one band of the window skeleton. A new surface adds a row
/// here with the surface it draws, so the table is the inventory the reviewer reads, not a waiver.
const HOST_OWNERS: &[Owner] = &[
    Owner { path: "crates/ui/src/class_picker.rs", owns: "the class picker popup" },
    Owner { path: "crates/ui/src/command_bar.rs", owns: "the command bar over the viewport" },
    Owner { path: "crates/ui/src/home.rs", owns: "the Home start view" },
    Owner { path: "crates/ui/src/palette.rs", owns: "the command palette" },
    Owner { path: "crates/ui/src/settings.rs", owns: "the Settings dialog" },
    Owner { path: "crates/ui/src/toolbars.rs", owns: "the ribbon and the anchor of its variant list" },
    Owner { path: "crates/ui/src/toolbars/list.rs", owns: "the ribbon's variant list" },
    Owner { path: "apps/furnicraft/src/contour_gizmo.rs", owns: "the numeric entry beside a contour grip" },
    Owner { path: "apps/furnicraft/src/file_menu.rs", owns: "the file dialogs and the close question" },
    Owner {
        path: "apps/furnicraft/src/main.rs",
        owns: "the window skeleton: the central workspace and the header band",
    },
    Owner { path: "apps/furnicraft/src/side_panels.rs", owns: "the two floating side panels" },
    Owner { path: "apps/furnicraft/src/status_bar.rs", owns: "the status row" },
    Owner { path: "apps/furnicraft/src/strip.rs", owns: "the floating ribbon strip" },
    Owner { path: "apps/furnicraft/src/view_cluster.rs", owns: "the viewport cluster and its popovers" },
];

/// The `egui` hosts: a docked panel, a free window, a modal and an overlay area.
const HOSTS: &[&str] = &["Area", "Window", "Modal", "Panel", "SidePanel", "TopBottomPanel", "CentralPanel", "Popup"];

/// `Color32` values that mean "no colour" or "measure only", not a palette entry.
const SENTINELS: &[&str] = &["TRANSPARENT", "PLACEHOLDER", "NONE"];

/// How deep the source walk goes. The two trees are three levels; a runaway link is not a task.
const MAX_DEPTH: usize = 16;

/// Checks the interface rules; returns how many shipped files were read.
///
/// `root` is the repository root, the same path the other host checks take.
pub fn check(root: &Path) -> Result<usize, Vec<String>> {
    let test_only = test_only_files(root);
    let mut violations = Vec::new();
    let mut checked = 0usize;

    for tree in UI_TREES {
        let mut files = Vec::new();
        if let Err(error) = walk(&root.join(tree), 0, &mut files) {
            violations.push(error);
            continue;
        }
        files.sort();
        for file in files {
            let shown = shown(root, &file);
            if test_only.contains(&shown) {
                continue;
            }
            checked = checked.saturating_add(1);
            check_file(&shown, &file, &mut violations);
        }
    }

    if violations.is_empty() { Ok(checked) } else { Err(violations) }
}

fn check_file(shown: &str, path: &Path, violations: &mut Vec<String>) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            violations.push(format!("cannot read `{shown}`: {error}"));
            return;
        }
    };
    let file = match syn::parse_file(&source) {
        Ok(file) => file,
        Err(error) => {
            violations.push(format!("`{shown}` does not parse as Rust: {error}"));
            return;
        }
    };
    let mut scan = Scan {
        shown,
        imports: Imports::collect(&file),
        // The palette belongs to `crates/ui`; the shell's gizmo inks are a known debt and are not
        // this rule's claim until that debt is paid.
        colour: shown.starts_with("crates/ui/") && !owned(COLOUR_OWNERS, shown),
        host: !owned(HOST_OWNERS, shown),
        violations,
    };
    scan.visit_file(&file);
}

fn owned(owners: &[Owner], shown: &str) -> bool {
    owners.iter().any(|owner| owner.path == shown)
}

/// Every `.rs` file under `directory`, to [`MAX_DEPTH`].
fn walk(directory: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("the source walk under `{}` is deeper than {MAX_DEPTH}", directory.display()));
    }
    let entries =
        std::fs::read_dir(directory).map_err(|error| format!("cannot read `{}`: {error}", directory.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth.saturating_add(1), files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn shown(root: &Path, file: &Path) -> String {
    // A stable forward-slash form: the owner table is written with `/`, and the check runs on
    // Windows too, where `display()` would separate the segments with `\`.
    file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

/// The files declared by a crate root under `#[cfg(test)]`, which are not shipped code.
fn test_only_files(root: &Path) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    for root_file in CRATE_ROOTS {
        let path = root.join(root_file);
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        let Ok(file) = syn::parse_file(&source) else { continue };
        collect_test_modules(root, &module_dir(&path), &file.items, &mut files);
    }
    files
}

/// Walks the module tree from one directory, recording the files a `#[cfg(test)]` module names.
fn collect_test_modules(root: &Path, directory: &Path, items: &[Item], out: &mut BTreeSet<String>) {
    for item in items {
        let Item::Mod(module) = item else { continue };
        let name = module.ident.to_string();
        let test = mentions_test(&module.attrs);
        match &module.content {
            Some((_, items)) => {
                if !test {
                    collect_test_modules(root, &directory.join(&name), items, out);
                }
            }
            None => {
                let Some(found) = external_module(directory, &name) else { continue };
                if test {
                    out.insert(shown(root, &found));
                } else if let Ok(source) = std::fs::read_to_string(&found)
                    && let Ok(parsed) = syn::parse_file(&source)
                {
                    collect_test_modules(root, &module_dir(&found), &parsed.items, out);
                }
            }
        }
    }
}

/// The directory a module declared in `file` resolves its children against.
fn module_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    let stem = file.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    if matches!(stem, "lib" | "main" | "mod") { parent.to_path_buf() } else { parent.join(stem) }
}

fn external_module(directory: &Path, name: &str) -> Option<PathBuf> {
    let direct = directory.join(format!("{name}.rs"));
    if direct.is_file() {
        return Some(direct);
    }
    let nested = directory.join(name).join("mod.rs");
    nested.is_file().then_some(nested)
}

fn mentions_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        match &attribute.meta {
            syn::Meta::List(list) => tokens_mention_test(list.tokens.clone()),
            _ => false,
        }
    })
}

fn tokens_mention_test(tokens: proc_macro2::TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(ident) => ident == "test",
        TokenTree::Group(group) => tokens_mention_test(group.stream()),
        _ => false,
    })
}

fn item_is_test(item: &Item) -> bool {
    match item {
        Item::Const(item) => mentions_test(&item.attrs),
        Item::Enum(item) => mentions_test(&item.attrs),
        Item::ExternCrate(item) => mentions_test(&item.attrs),
        Item::Fn(item) => mentions_test(&item.attrs),
        Item::ForeignMod(item) => mentions_test(&item.attrs),
        Item::Impl(item) => mentions_test(&item.attrs),
        Item::Macro(item) => mentions_test(&item.attrs),
        Item::Mod(item) => mentions_test(&item.attrs),
        Item::Static(item) => mentions_test(&item.attrs),
        Item::Struct(item) => mentions_test(&item.attrs),
        Item::Trait(item) => mentions_test(&item.attrs),
        Item::TraitAlias(item) => mentions_test(&item.attrs),
        Item::Type(item) => mentions_test(&item.attrs),
        Item::Union(item) => mentions_test(&item.attrs),
        Item::Use(item) => mentions_test(&item.attrs),
        _ => false,
    }
}

fn impl_item_is_test(item: &ImplItem) -> bool {
    match item {
        ImplItem::Const(item) => mentions_test(&item.attrs),
        ImplItem::Fn(item) => mentions_test(&item.attrs),
        ImplItem::Macro(item) => mentions_test(&item.attrs),
        ImplItem::Type(item) => mentions_test(&item.attrs),
        _ => false,
    }
}

/// The names a file imports, resolved to the path they came from.
#[derive(Default)]
struct Imports {
    aliases: BTreeMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
}

impl Imports {
    fn collect(file: &syn::File) -> Self {
        struct Collect(Imports);
        impl<'ast> Visit<'ast> for Collect {
            fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
                self.0.add(&[], &item.tree);
            }
        }
        let mut collect = Collect(Self::default());
        collect.visit_file(file);
        collect.0
    }

    fn add(&mut self, prefix: &[String], tree: &UseTree) {
        match tree {
            UseTree::Path(path) => {
                let mut prefix = prefix.to_vec();
                prefix.push(path.ident.to_string());
                self.add(&prefix, &path.tree);
            }
            UseTree::Name(name) => {
                let mut full = prefix.to_vec();
                full.push(name.ident.to_string());
                self.aliases.insert(name.ident.to_string(), full);
            }
            UseTree::Rename(rename) => {
                let mut full = prefix.to_vec();
                if rename.ident == "self" {
                    self.aliases.insert(rename.rename.to_string(), full);
                    return;
                }
                full.push(rename.ident.to_string());
                self.aliases.insert(rename.rename.to_string(), full);
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.add(prefix, tree);
                }
            }
            UseTree::Glob(_) => self.globs.push(prefix.to_vec()),
        }
    }

    fn resolve(&self, path: &SynPath) -> Vec<String> {
        let segments: Vec<String> = path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        let Some(first) = segments.first().cloned() else { return segments };
        if let Some(mapped) = self.aliases.get(&first) {
            let mut canonical = mapped.clone();
            canonical.extend(segments.into_iter().skip(1));
            return canonical;
        }
        if segments.len() == 1 {
            for glob in &self.globs {
                let mut canonical = glob.clone();
                canonical.push(first.clone());
                if toolkit(&canonical) {
                    return canonical;
                }
            }
        }
        segments
    }
}

/// Whether a canonical path came from the UI toolkit rather than from this crate.
fn toolkit(canonical: &[String]) -> bool {
    canonical.iter().any(|segment| segment == "egui" || segment == "eframe")
}

/// The host type a path names, if it names one (`egui::Area`, `eframe::egui::Panel::top`, …).
fn host(canonical: &[String]) -> Option<&str> {
    if !toolkit(canonical) {
        return None;
    }
    canonical.iter().map(String::as_str).find(|segment| HOSTS.contains(segment))
}

/// The `Color32` member a path names: a constant or one of its `from_*` constructors.
fn colour_member(canonical: &[String]) -> Option<&str> {
    let index = canonical.iter().position(|segment| segment == "Color32")?;
    if !toolkit(canonical.get(..index)?) {
        return None;
    }
    canonical.get(index.saturating_add(1)).map(String::as_str)
}

/// Whether every argument is a literal, so the colour was hand-picked rather than derived.
fn all_literals(arguments: &syn::punctuated::Punctuated<Expr, syn::Token![,]>) -> bool {
    !arguments.is_empty() && arguments.iter().all(literal)
}

fn literal(expression: &Expr) -> bool {
    match expression {
        Expr::Lit(_) => true,
        Expr::Unary(unary) => matches!(unary.op, syn::UnOp::Neg(_)) && literal(&unary.expr),
        Expr::Group(group) => literal(&group.expr),
        Expr::Paren(paren) => literal(&paren.expr),
        _ => false,
    }
}

/// Reads one shipped file and reports the rules it breaks.
struct Scan<'a> {
    shown: &'a str,
    imports: Imports,
    colour: bool,
    host: bool,
    violations: &'a mut Vec<String>,
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test(item) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if impl_item_is_test(item) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = call.func.as_ref() {
            let canonical = self.imports.resolve(&path.path);
            if let Some(member) = colour_member(&canonical)
                && self.colour
                && member.starts_with("from_")
                && all_literals(&call.args)
            {
                let span = call.func.span();
                self.colour_literal(span, &format!("Color32::{member}"));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast ExprPath) {
        let canonical = self.imports.resolve(&path.path);
        if self.host
            && let Some(host) = host(&canonical)
        {
            self.host_literal(path.span(), host);
        }
        if let Some(member) = colour_member(&canonical)
            && self.colour
            && member.chars().next().is_some_and(char::is_uppercase)
            && !SENTINELS.contains(&member)
        {
            self.colour_literal(path.span(), &format!("Color32::{member}"));
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        let canonical = self.imports.resolve(&expression.path);
        let writes_a_colour = canonical.iter().any(|segment| segment == "Color32") && toolkit(&canonical);
        if self.colour && writes_a_colour {
            self.colour_literal(expression.path.span(), "Color32 { .. }");
        }
        visit::visit_expr_struct(self, expression);
    }
}

impl Scan<'_> {
    fn colour_literal(&mut self, span: Span, what: &str) {
        self.violations.push(format!(
            "`{}:{}` builds `{what}`, a colour literal; a feature module takes its colour from \
             `crates/ui/src/theme.rs` (ADR 0063)",
            self.shown,
            span.start().line
        ));
    }

    fn host_literal(&mut self, span: Span, host: &str) {
        let owner = HOST_OWNERS.iter().map(|owner| owner.owns).collect::<Vec<_>>().join("; ");
        self.violations.push(format!(
            "`{}:{}` creates `egui::{host}`, a host only the module that owns the surface may draw; \
             add the module to `HOST_OWNERS` in `xtask/src/ui_rules.rs` with the surface it owns \
             (owned today: {owner})",
            self.shown,
            span.start().line
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTER: &str = "fn main() {}\n";

    /// A scratch project with the given `(relative path, contents)` files.
    fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("xtask-ui-rules-{name}-{}", std::process::id()));
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

    fn healthy() -> Vec<(&'static str, &'static str)> {
        vec![
            ("crates/ui/src/lib.rs", ROUTER),
            ("apps/furnicraft/src/main.rs", ROUTER),
            ("crates/ui/src/panel_card.rs", "pub fn card() { ui.label(\"ok\"); }\n"),
        ]
    }

    fn violations_of(root: &Path) -> Vec<String> {
        check(root).expect_err("the tree breaks a rule")
    }

    #[test]
    fn a_healthy_tree_passes() {
        let root = project("ok", &healthy());
        assert_eq!(check(&root).expect("the interface keeps its owners"), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_literal_colour_in_a_card_names_its_file_and_line() {
        let mut files = healthy();
        files[2].1 = "use egui::Color32;\npub fn card() {\n    ui.label(Color32::LIGHT_RED);\n}\n";
        let root = project("colour", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("`crates/ui/src/panel_card.rs:3`")
                && violation.contains("Color32::LIGHT_RED")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_raw_host_in_a_card_is_named() {
        let mut files = healthy();
        files[2].1 = "pub fn card(ctx: &egui::Context) {\n    egui::Window::new(\"card\").show(ctx, |_ui| {});\n}\n";
        let root = project("host", &files);
        let violations = violations_of(&root);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("`crates/ui/src/panel_card.rs:2`")
                    && violation.contains("egui::Window")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_alias_cannot_slip_past() {
        let mut files = healthy();
        files[2].1 = "use egui::Color32 as C;\npub fn card() {\n    ui.label(C::LIGHT_RED);\n}\n";
        let root = project("alias", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("Color32::LIGHT_RED")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);

        let mut files = healthy();
        files[2].1 = "use egui::{self as gui};\npub fn card(ctx: &gui::Context) {\n    gui::Window::new(\"x\").show(ctx, |_ui| {});\n}\n";
        let root = project("module-alias", &files);
        let violations = violations_of(&root);
        assert!(violations.iter().any(|violation| violation.contains("egui::Window")), "{violations:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hand_picked_channel_and_a_raw_struct_are_colours_too() {
        let mut files = healthy();
        files[2].1 = "use egui::Color32;\npub fn card() {\n    let _ = Color32::from_rgb(1, 2, 3);\n    let _ = Color32 { r: 1, g: 2, b: 3, a: 255 };\n}\n";
        let root = project("channels", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("`crates/ui/src/panel_card.rs:3`")
                && violation.contains("Color32::from_rgb")),
            "{violations:?}"
        );
        assert!(
            violations.iter().any(|violation| violation.contains("`crates/ui/src/panel_card.rs:4`")
                && violation.contains("Color32 { .. }")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_derived_colour_is_not_a_literal() {
        let mut files = healthy();
        files[2].1 =
            "use egui::Color32;\npub fn card(r: u8, g: u8, b: u8) -> Color32 {\n    Color32::from_rgb(r, g, b)\n}\n";
        let root = project("derived", &files);
        assert!(check(&root).is_ok(), "a colour mapped from data is not a hand-picked literal");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_owner_in_the_table_may_do_what_it_owns() {
        let mut files = Vec::new();
        for owner in COLOUR_OWNERS {
            files.push((owner.path, "use egui::Color32;\npub const INK: Color32 = Color32::from_rgb(1, 2, 3);\n"));
        }
        for owner in HOST_OWNERS {
            files.push((
                owner.path,
                "pub fn show(ctx: &egui::Context) {\n    egui::Window::new(\"owned\").show(ctx, |_ui| {});\n}\n",
            ));
        }
        files.push(("crates/ui/src/lib.rs", ROUTER));
        files.push(("apps/furnicraft/src/main.rs", ROUTER));
        let root = project("owners", &files);
        assert!(check(&root).is_ok(), "{:?}", check(&root).err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_code_is_not_shipped_code() {
        let mut files = healthy();
        files[2].1 = "use egui::Color32;\n#[cfg(test)]\nmod tests {\n    fn card() {\n        let _ = Color32::LIGHT_RED;\n    }\n}\n";
        let root = project("inline-test", &files);
        assert!(check(&root).is_ok(), "{:?}", check(&root).err());
        let _ = std::fs::remove_dir_all(&root);

        let mut files = healthy();
        files[0] = ("crates/ui/src/lib.rs", "#[cfg(test)]\nmod panel_card_tests;\n");
        files.push((
            "crates/ui/src/panel_card_tests.rs",
            "use egui::Color32;\npub fn card() {\n    let _ = Color32::LIGHT_RED;\n}\n",
        ));
        let root = project("file-test", &files);
        assert!(check(&root).is_ok(), "{:?}", check(&root).err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_does_not_parse_is_named() {
        let mut files = healthy();
        files[2].1 = "pub fn card( {";
        let root = project("parse", &files);
        let violations = violations_of(&root);
        assert!(
            violations.iter().any(|violation| violation.contains("`crates/ui/src/panel_card.rs` does not parse")),
            "{violations:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The tree this repository ships: a new colour or host in a feature module fails the gate.
    #[test]
    fn the_real_tree_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for owner in COLOUR_OWNERS.iter().chain(HOST_OWNERS) {
            assert!(root.join(owner.path).is_file(), "`{}` owns `{}` and does not exist", owner.path, owner.owns);
        }
        let count = check(&root).expect("the interface keeps its colours and its hosts where they are owned");
        assert!(count > 40, "{count} files checked");
    }
}
