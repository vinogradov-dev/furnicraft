//! The File menu, the open/save path dialog and the unsaved-changes question a close asks.
//!
//! The menu is a view over one piece of the document's shell state — the path the scene came from —
//! plus its pending dialog. It never touches the document or the filesystem itself; it returns a
//! [`FileAction`] and the application performs it through the scene store. A path dialog is a plain
//! text field with a short list of nearby `.fcs` files, so the build needs no native file-dialog
//! dependency.
//!
//! Opening and *New* both mean "a new tab" (U45), so the menu never discards a scene and never has
//! to ask: the one question left is a tab close with unsaved changes, which [`ConfirmClose`] draws
//! for the shell because a tab belongs to the window, not to one document.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use eframe::egui::{Color32, Id, Modal, TextEdit, Ui};

/// One file operation the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileAction {
    /// Open a new, empty scene in a new tab.
    New,
    /// Read the scene at this path.
    Open(PathBuf),
    /// Write the current scene to this path.
    Save(PathBuf),
    /// Close the active tab, asking first when its scene has unsaved changes.
    Close,
    /// Quit the application, asking once when any open scene has unsaved changes.
    Quit,
}

/// What a File menu entry means before it becomes an action or a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuCommand {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Quit,
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

/// What the unsaved-changes question was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseAnswer {
    /// Drop the tab and everything it holds.
    Discard,
    /// Leave the tab open.
    Cancel,
}

/// The question a close asks when the scene it would drop has unsaved changes.
///
/// The shell owns one of these while the question is up: it is about a tab, so it outlives the
/// document it names, and switching tabs must not lose it.
#[derive(Clone, Debug)]
pub(crate) struct ConfirmClose {
    /// The question, already built from what it is about.
    title: String,
    /// One sentence naming what is lost.
    body: String,
    /// The label of the button that discards.
    discard: String,
}

impl ConfirmClose {
    /// Asks about the scene `title` names, the way the tab row shows it.
    pub(crate) fn tab(title: impl Into<String>) -> Self {
        Self {
            title: format!("Close \u{201c}{}\u{201d}?", title.into()),
            body: "This scene has unsaved changes. Closing the tab discards them.".to_owned(),
            discard: "Discard and close".to_owned(),
        }
    }

    /// Asks about the whole window, when `unsaved` open scenes would go with it.
    pub(crate) fn quit(unsaved: usize) -> Self {
        let body = match unsaved {
            1 => "One open scene has unsaved changes. Quitting discards them.".to_owned(),
            count => format!("{count} open scenes have unsaved changes. Quitting discards them."),
        };
        Self { title: format!("Quit {}?", crate::theme::PRODUCT_NAME), body, discard: "Discard and quit".to_owned() }
    }

    /// Draws the question and returns the answer the user picked, if any.
    pub(crate) fn show(&self, ctx: &egui::Context) -> Option<CloseAnswer> {
        let mut answer = None;
        Modal::new(Id::unique("scene-confirm-close")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(&self.title);
            ui.label(&self.body);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(&self.discard).clicked() {
                    answer = Some(CloseAnswer::Discard);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(CloseAnswer::Cancel);
                }
            });
        });
        answer
    }
}

/// The File menu, its current path and its pending modal dialogs.
#[derive(Debug, Default)]
pub(crate) struct FileMenu {
    current: Option<PathBuf>,
    dialog: Option<PathDialog>,
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

    /// Draws the File menu button on an existing row and any open dialog.
    ///
    /// The button is a plain menu: the header decides where it sits and draws its own dividers, so
    /// this function creates no panel of its own. *New* and *Open* open a tab, so neither of them
    /// discards the scene in front of the user and neither has to ask.
    pub(crate) fn show(&mut self, ui: &mut Ui) -> Option<FileAction> {
        let mut action = None;
        let button = egui::Button::new("File")
            .min_size(egui::vec2(crate::theme::HEADER_TEXT_WIDTH, crate::theme::HEADER_CONTROL_HEIGHT));
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            if ui.button("New").clicked() {
                ui.close();
                action = self.command(MenuCommand::New);
            }
            if ui.button("Open…").clicked() {
                ui.close();
                action = self.command(MenuCommand::Open);
            }
            ui.separator();
            if ui.button("Save").clicked() {
                ui.close();
                action = self.command(MenuCommand::Save);
            }
            if ui.button("Save As…").clicked() {
                ui.close();
                action = self.command(MenuCommand::SaveAs);
            }
            ui.separator();
            if ui.button("Close tab").clicked() {
                ui.close();
                action = self.command(MenuCommand::Close);
            }
            if ui.button("Exit").clicked() {
                ui.close();
                action = self.command(MenuCommand::Quit);
            }
        });
        action.or_else(|| self.show_dialogs(ui.ctx()))
    }

    /// Draws any open modal dialog and returns the operation it asked for.
    ///
    /// A dialog is a window over the whole application, not a child of the header, so the shell
    /// calls this once per frame however the header is laid out.
    pub(crate) fn show_dialogs(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        self.path_dialog_ui(ctx)
    }

    /// The Save path of the header's save button.
    ///
    /// The header calls the same [`MenuCommand::Save`] the File menu does, so the two cannot drift
    /// into one saving silently and the other opening a path dialog.
    pub(crate) fn save(&mut self) -> Option<FileAction> {
        self.command(MenuCommand::Save)
    }

    /// Opens the path dialog from the Home surface.
    ///
    /// Home has no menu of its own, so it asks for the same dialog the File menu opens; the dialog
    /// already warns when the current scene has unsaved changes.
    pub(crate) fn request_open(&mut self) {
        self.open_dialog();
    }

    /// The file the scene came from, if it has one.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    /// The bare file name the header's tab draws; the dirty marker is its own glyph there.
    pub(crate) fn file_name(&self) -> String {
        self.current
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned()
    }

    /// Turns a menu entry into an action or a dialog.
    fn command(&mut self, command: MenuCommand) -> Option<FileAction> {
        match command {
            MenuCommand::New => Some(FileAction::New),
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
            // Both are window operations, not file operations: the shell closes the tab or the
            // window, and the unsaved-changes question belongs there because it names a tab.
            MenuCommand::Close => Some(FileAction::Close),
            MenuCommand::Quit => Some(FileAction::Quit),
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

    fn path_dialog_ui(&mut self, ctx: &egui::Context) -> Option<FileAction> {
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
            if dialog.mode == PathMode::Open {
                // Opening a file adds a tab, so the scene in front of the user stays exactly where
                // it is: the dialog only says where the new one comes from.
                ui.label("The scene opens in a new tab; the open scenes are left as they are.");
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
        assert_eq!(menu.command(MenuCommand::Save), None);
        assert!(matches!(menu.dialog, Some(PathDialog { mode: PathMode::SaveAs, .. })));
    }

    #[test]
    fn save_with_a_path_returns_it() {
        let mut menu = FileMenu::new();
        let path = PathBuf::from("/tmp/scene.fcs");
        menu.set_path(Some(path.clone()));
        assert_eq!(menu.command(MenuCommand::Save), Some(FileAction::Save(path)));
        assert!(menu.dialog.is_none());
    }

    #[test]
    fn a_new_scene_opens_a_tab_without_asking() {
        // Opening a tab discards nothing, so neither New nor Open has a question to ask (U45); the
        // one question left is a tab close, which the shell draws through `ConfirmClose`.
        let mut menu = FileMenu::new();
        menu.set_path(Some(PathBuf::from("/tmp/cabinet.fcs")));
        assert_eq!(menu.command(MenuCommand::New), Some(FileAction::New));
        assert_eq!(menu.save(), Some(FileAction::Save(PathBuf::from("/tmp/cabinet.fcs"))));
        assert!(menu.dialog.is_none(), "the menu keeps the scene's path: it is another tab, not another file");
    }

    #[test]
    fn the_close_question_names_the_tab_and_offers_both_answers() {
        // The question belongs to the shell, so this test only proves it is the document's name it
        // asks about and that both answers exist; the modal itself needs an egui pass.
        let question = ConfirmClose::tab("cabinet.fcs");
        assert_eq!(question.title, "Close \u{201c}cabinet.fcs\u{201d}?");
        assert!(question.body.contains("unsaved changes"));
        assert_ne!(CloseAnswer::Discard, CloseAnswer::Cancel, "cancel and discard are different answers");
    }

    #[test]
    fn the_quit_question_counts_the_unsaved_scenes() {
        let one = ConfirmClose::quit(1);
        assert_eq!(one.title, format!("Quit {}?", crate::theme::PRODUCT_NAME));
        assert!(one.body.contains("One open scene"), "one scene reads as a sentence, not as `1 scenes`: {}", one.body);
        let many = ConfirmClose::quit(3);
        assert!(many.body.contains("3 open scenes"), "and a count reads as one: {}", many.body);
        assert_eq!(many.discard, "Discard and quit");
    }

    #[test]
    fn the_menu_names_close_and_exit_after_the_file_entries() {
        // *Close tab* and *Exit* are window operations, so the menu reports them as actions the
        // shell performs rather than opening a dialog of its own (X32).
        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::Close), Some(FileAction::Close));
        assert_eq!(menu.command(MenuCommand::Quit), Some(FileAction::Quit));
        assert!(menu.dialog.is_none(), "neither of them asks for a path");
    }

    #[test]
    fn open_opens_a_dialog_prefilled_with_a_scene_name() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.command(MenuCommand::Open), None);
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
    fn the_file_name_and_the_path_come_from_the_open_file() {
        let mut menu = FileMenu::new();
        assert_eq!(menu.file_name(), "Untitled");
        assert_eq!(menu.path(), None);
        menu.set_path(Some(PathBuf::from("/tmp/cabinet.fcs")));
        assert_eq!(menu.file_name(), "cabinet.fcs");
        assert_eq!(menu.path(), Some(Path::new("/tmp/cabinet.fcs")), "the path names the tab for a re-open");
        menu.set_path(None);
        assert_eq!(menu.file_name(), "Untitled", "a new scene has no file to overwrite");
    }
}
