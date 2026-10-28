//! The File menu and the open/save path dialog.
//!
//! The menu is a view over three pieces of shell state: the current scene path, the pending dialog
//! and the unsaved-changes flag the session owns. It never touches the document or the filesystem
//! itself; it returns a [`FileAction`] and the application performs it through the scene store.
//! A path dialog is a plain text field with a short list of nearby `.fcs` files, so the build needs
//! no native file-dialog dependency.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use eframe::egui::{Color32, Id, Modal, TextEdit, Ui};

/// One file operation the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileAction {
    /// Start a new, empty scene.
    New,
    /// Read the scene at this path.
    Open(PathBuf),
    /// Write the current scene to this path.
    Save(PathBuf),
}

/// What a File menu entry means before it becomes an action or a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuCommand {
    New,
    Open,
    Save,
    SaveAs,
}

/// Which operation the path dialog performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathMode {
    Open,
    SaveAs,
}

/// The open path dialog and what the user has typed into it.
#[derive(Clone, Debug)]
struct PathDialog {
    mode: PathMode,
    path: String,
    error: Option<String>,
}

/// The File menu, its current path and its pending modal dialogs.
#[derive(Debug, Default)]
pub(crate) struct FileMenu {
    current: Option<PathBuf>,
    dialog: Option<PathDialog>,
    confirm_new: bool,
}

impl FileMenu {
    /// Creates a menu with no current file.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records the file the scene was last opened from or saved to.
    pub(crate) fn set_path(&mut self, path: Option<PathBuf>) {
        self.current = path;
    }

    /// Draws the menu bar and any open dialog, returning the operation to perform.
    ///
    /// `dirty` marks the current scene as having unsaved changes: Save is still allowed, New asks
    /// for confirmation and Open warns that it replaces the document.
    pub(crate) fn show(&mut self, ui: &mut Ui, dirty: bool) -> Option<FileAction> {
        let mut action = None;
        egui::Panel::top("file_menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        ui.close();
                        action = self.command(MenuCommand::New, dirty);
                    }
                    if ui.button("Open…").clicked() {
                        ui.close();
                        action = self.command(MenuCommand::Open, dirty);
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        ui.close();
                        action = self.command(MenuCommand::Save, dirty);
                    }
                    if ui.button("Save As…").clicked() {
                        ui.close();
                        action = self.command(MenuCommand::SaveAs, dirty);
                    }
                });
                ui.separator();
                ui.label(self.display_name(dirty));
            });
        });
        if action.is_some() {
            return action;
        }
        let ctx = ui.ctx().clone();
        if self.confirm_new { self.confirm_new_ui(&ctx) } else { self.path_dialog_ui(&ctx, dirty) }
    }

    /// Turns a menu entry into an action or a dialog.
    fn command(&mut self, command: MenuCommand, dirty: bool) -> Option<FileAction> {
        match command {
            MenuCommand::New => {
                if dirty {
                    self.confirm_new = true;
                    None
                } else {
                    Some(FileAction::New)
                }
            }
            MenuCommand::Open => {
                self.open_dialog();
                None
            }
            MenuCommand::Save => match &self.current {
                Some(path) => Some(FileAction::Save(path.clone())),
                None => {
                    self.save_dialog();
                    None
                }
            },
            MenuCommand::SaveAs => {
                self.save_dialog();
                None
            }
        }
    }

    fn open_dialog(&mut self) {
        self.dialog = Some(PathDialog { mode: PathMode::Open, path: self.default_path(), error: None });
    }

    fn save_dialog(&mut self) {
        self.dialog = Some(PathDialog { mode: PathMode::SaveAs, path: self.default_path(), error: None });
    }

    /// The path a dialog starts with: the current file, or `scene.fcs` in the working directory.
    fn default_path(&self) -> String {
        if let Some(current) = &self.current {
            return current.display().to_string();
        }
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        directory.join("scene.fcs").display().to_string()
    }

    /// The name shown in the menu bar, with a dirty marker.
    fn display_name(&self, dirty: bool) -> String {
        let name =
            self.current.as_deref().and_then(Path::file_name).and_then(|name| name.to_str()).unwrap_or("Untitled");
        if dirty { format!("{name}  *") } else { name.to_owned() }
    }

    fn confirm_new_ui(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        let mut discard = false;
        let mut cancel = false;
        Modal::new(Id::unique("scene-confirm-new")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Start a new scene?");
            ui.label("The current scene has unsaved changes. Starting a new scene discards them.");
            ui.separator();
            ui.horizontal(|ui| {
                discard = ui.button("Discard and start new").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if discard {
            self.confirm_new = false;
            return Some(FileAction::New);
        }
        if cancel {
            self.confirm_new = false;
        }
        None
    }

    fn path_dialog_ui(&mut self, ctx: &egui::Context, dirty: bool) -> Option<FileAction> {
        let mut dialog = self.dialog.take()?;
        let mut action = None;
        let mut keep = true;
        Modal::new(Id::unique("scene-file-path")).show(ctx, |ui| {
            ui.set_width(460.0);
            let (title, primary) = match dialog.mode {
                PathMode::Open => ("Open scene", "Open"),
                PathMode::SaveAs => ("Save scene as", "Save"),
            };
            ui.heading(title);
            if dirty && dialog.mode == PathMode::Open {
                ui.colored_label(
                    Color32::YELLOW,
                    "The current scene has unsaved changes; opening another file discards them.",
                );
            }
            ui.horizontal(|ui| {
                ui.label("Path");
                ui.add(TextEdit::singleline(&mut dialog.path).desired_width(f32::INFINITY));
            });
            if dialog.mode == PathMode::Open {
                let files = scene_files_near(&dialog.path);
                if !files.is_empty() {
                    ui.label("Nearby scenes");
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        for file in files {
                            let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
                            if ui.selectable_label(false, name).clicked() {
                                dialog.path = file.display().to_string();
                            }
                        }
                    });
                }
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            ui.separator();
            let mut submit = false;
            ui.horizontal(|ui| {
                submit = ui.button(primary).clicked();
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
            if submit {
                match submit_dialog(&mut dialog) {
                    Some(chosen) => {
                        action = Some(chosen);
                        keep = false;
                    }
                    None => keep = true,
                }
            }
        });
        if keep {
            self.dialog = Some(dialog);
        }
        action
    }
}

/// Validates the typed path and turns it into an action, or records why it cannot.
fn submit_dialog(dialog: &mut PathDialog) -> Option<FileAction> {
    let path = dialog.path.trim();
    if path.is_empty() {
        dialog.error = Some("Enter a file path.".to_owned());
        return None;
    }
    let path = PathBuf::from(path);
    dialog.error = None;
    match dialog.mode {
        PathMode::Open => Some(FileAction::Open(path)),
        PathMode::SaveAs => Some(FileAction::Save(path)),
    }
}

/// Up to fifty `.fcs` files beside the typed path, sorted, for the Open dialog.
fn scene_files_near(path: &str) -> Vec<PathBuf> {
    let path = Path::new(path);
    let directory = if path.is_dir() {
        path
    } else {
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."))
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .take(500)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate.is_file()
                && candidate
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("fcs"))
        })
        .collect();
    files.sort();
    files.truncate(50);
    files
}

/// Top-level sections of a loaded scene that this build does not understand.
pub(crate) type PreservedSections = BTreeMap<String, serde_json::Value>;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn save_without_a_path_opens_the_save_dialog() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::Save, false), None);
        assert!(matches!(menu.dialog, Some(PathDialog { mode: PathMode::SaveAs, .. })));
    }

    #[test]
    fn save_with_a_path_returns_it() {
        let mut menu = FileMenu::new();
        let path = PathBuf::from("/tmp/scene.fcs");
        menu.set_path(Some(path.clone()));
        assert_eq!(menu.command(MenuCommand::Save, false), Some(FileAction::Save(path)));
        assert!(menu.dialog.is_none());
    }

    #[test]
    fn a_new_scene_asks_before_discarding_unsaved_changes() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::New, true), None);
        assert!(menu.confirm_new);

        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::New, false), Some(FileAction::New));
        assert!(!menu.confirm_new);
    }

    #[test]
    fn open_opens_a_dialog_prefilled_with_a_scene_name() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::Open, false), None);
        let dialog = menu.dialog.as_ref().expect("the open dialog");
        assert_eq!(dialog.mode, PathMode::Open);
        assert!(dialog.path.ends_with("scene.fcs"), "path was {}", dialog.path);
    }

    #[test]
    fn an_empty_path_is_rejected_by_the_dialog() {
        let mut dialog = PathDialog { mode: PathMode::Open, path: "   ".to_owned(), error: None };
        assert_eq!(submit_dialog(&mut dialog), None);
        assert!(dialog.error.is_some());
    }

    #[test]
    fn nearby_scenes_list_only_scene_files() {
        let dir = std::env::temp_dir().join(format!("furnicraft-scene-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the fixture directory");
        for name in ["b.fcs", "a.fcs", "notes.txt", "save.fcs.tmp"] {
            fs::write(dir.join(name), "{}").expect("write a fixture");
        }

        let files = scene_files_near(&dir.display().to_string());
        let names: Vec<_> = files.iter().filter_map(|path| path.file_name()).filter_map(|name| name.to_str()).collect();
        assert_eq!(names, ["a.fcs", "b.fcs"], "only scene files are listed, sorted");

        fs::remove_dir_all(&dir).expect("remove the fixture directory");
    }

    #[test]
    fn the_dirty_marker_is_part_of_the_display_name() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.display_name(false), "Untitled");
        assert_eq!(menu.display_name(true), "Untitled  *");
        menu.set_path(Some(PathBuf::from("/tmp/cabinet.fcs")));
        assert_eq!(menu.display_name(false), "cabinet.fcs");
    }
}
