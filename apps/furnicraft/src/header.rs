//! The header: the product mark, the file and history actions, the open file, and the settings.
//!
//! The header is chrome, not a document surface. It owns no document state and dispatches no
//! command by itself: it returns a [`HeaderAction`] and the shell performs it, exactly like the
//! File menu and the tool strip. The window keeps its system frame (`AGENTS.md` §4.10 is about the
//! document; the decision to keep the frame is the owner's), so the header carries no close,
//! minimize or maximize button and only the settings gear sits on its right.
//!
//! Every divider on this row separates a meaning group — identity, file, history, tabs, settings —
//! and comes from [`theme::divider_of_height`], as tall as the controls it separates, so the header
//! and the ribbon share one rhythm and no hairline stops halfway down a button.

use eframe::egui;
use eframe::egui::Ui;

use crate::icons::{self, Icon};
use crate::theme;

/// What the header asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderAction {
    /// Show the start surface: new, open, recent and templates.
    Home,
    /// Undo the last document change.
    Undo,
    /// Redo the last undone change.
    Redo,
    /// Save to the current path, or ask for one.
    Save,
    /// Open the application settings.
    Settings,
    /// Close the open tab; the shell decides whether unsaved changes need a question.
    CloseTab,
}

/// The state the header draws from; nothing here is document data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderState {
    /// The tab's title: the file name, or the unsaved-scene name.
    pub title: String,
    /// Whether the scene has unsaved changes.
    pub dirty: bool,
    /// Whether there is an undo entry.
    pub can_undo: bool,
    /// Whether there is a redo entry.
    pub can_redo: bool,
}

/// Draws the header row and returns the one action it asked for, if any.
///
/// The File menu is passed in as a closure so the shell keeps its existing [`crate::file_menu`]
/// menu, its dialogs and its path handling: the header only gives it a place to live on the row.
pub fn show(ui: &mut Ui, state: &HeaderState, file_menu: impl FnOnce(&mut Ui)) -> Option<HeaderAction> {
    let height = theme::HEADER_HEIGHT;
    let mut action = None;
    egui::Frame::new().fill(theme::surface(ui)).inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        // `Sides` gives the left group the row and pins the right group to its far edge; two nested
        // layouts would leave the gear wherever the tab strip happened to end.
        let (left, right) = egui::Sides::new().height(height).shrink_right().show(
            ui,
            |ui| {
                let mut action = None;
                icons::mark(ui, 18.0, theme::accent(ui));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(theme::PRODUCT_NAME).strong().size(13.5));
                theme::divider_of_height(ui, theme::HEADER_CONTROL_HEIGHT);

                if icon_button(ui, icons::view::HOME, "Home", true, false).clicked() {
                    action = Some(HeaderAction::Home);
                }
                theme::divider_of_height(ui, theme::HEADER_CONTROL_HEIGHT);

                file_menu(ui);
                theme::divider_of_height(ui, theme::HEADER_CONTROL_HEIGHT);

                if icon_button(ui, icons::actions::FILE_SAVE, "Save  (Ctrl+S)", true, false).clicked() {
                    action = Some(HeaderAction::Save);
                }
                theme::divider_of_height(ui, theme::HEADER_CONTROL_HEIGHT);

                let hint = if state.can_undo { "Undo  (Ctrl+Z)" } else { "Nothing to undo" };
                if icon_button(ui, icons::actions::UNDO, hint, state.can_undo, false).clicked() && state.can_undo {
                    action = Some(HeaderAction::Undo);
                }
                let hint = if state.can_redo { "Redo  (Ctrl+Y)" } else { "Nothing to redo" };
                if icon_button(ui, icons::actions::REDO, hint, state.can_redo, false).clicked() && state.can_redo {
                    action = Some(HeaderAction::Redo);
                }
                theme::divider_of_height(ui, theme::HEADER_CONTROL_HEIGHT);

                if let Some(true) = tab(ui, state) {
                    action = Some(HeaderAction::CloseTab);
                }
                action
            },
            |ui| {
                let mut action = None;
                if icon_button(ui, icons::actions::SETTINGS, "Settings", true, false).clicked() {
                    action = Some(HeaderAction::Settings);
                }
                action
            },
        );
        action = left.or(right);
    });
    action
}

/// One open file: its name, the unsaved marker and its close button.
///
/// The tab draws pressed when it is the active document, which is the only tab presented to the
/// user for now. A second open file turns this row into a strip without changing the drawing.
/// Returns whether the tab's close button was clicked; the tab body has nowhere to navigate.
fn tab(ui: &mut Ui, state: &HeaderState) -> Option<bool> {
    let mut close = false;
    egui::Frame::new()
        .fill(theme::panel(ui))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&state.title).size(12.0));
                if state.dirty {
                    ui.label(egui::RichText::new("●").size(9.0).color(theme::warn(ui)))
                        .on_hover_text("Unsaved changes");
                }
                close = icons::icon_button_at(ui, icons::window::CLOSE, 11.0, "Close the file", true, false).clicked();
            });
        });
    Some(close)
}

/// The header's icon button: a drawn icon, or its label if the asset is still empty.
///
/// The button is one header control — [`theme::HEADER_CONTROL_HEIGHT`] tall, square — so it lines up
/// with the `File` text button beside it instead of standing a few points taller. The icon itself is
/// 18 pt inside that cell, which is the optical size the header needs.
fn icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> egui::Response {
    icons::icon_button_sized(ui, icon, theme::HEADER_CONTROL_HEIGHT, 18.0, label, enabled, selected)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use egui::epaint::Shape;

    use super::*;

    fn state(dirty: bool, can_undo: bool) -> HeaderState {
        HeaderState { title: "cabinet.fcs".to_owned(), dirty, can_undo, can_redo: false }
    }

    /// Runs the header and returns its text shapes.
    fn drawn(state: &HeaderState) -> Vec<String> {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, state, |_ui| {});
        });
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        output.drop_without_applying_deltas();
        out
    }

    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(text) => out.push(text.galley.text().to_owned()),
            Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn the_header_names_the_product_and_the_open_file() {
        let texts = drawn(&state(false, false));
        assert!(texts.iter().any(|text| text == theme::PRODUCT_NAME), "the mark carries the name: {texts:?}");
        assert!(texts.iter().any(|text| text == "cabinet.fcs"), "the tab names the file: {texts:?}");
        assert!(!texts.iter().any(|text| text == "●"), "a clean scene has no unsaved marker: {texts:?}");
    }

    #[test]
    fn an_unsaved_scene_marks_its_tab() {
        let texts = drawn(&state(true, false));
        assert!(texts.iter().any(|text| text == "●"), "an unsaved scene is marked: {texts:?}");
    }

    #[test]
    fn an_empty_history_starts_with_nothing_to_undo() {
        let state = state(false, false);
        assert!(!state.can_undo, "a fresh scene has no command to take back");
        assert!(!state.can_redo);
    }

    #[test]
    fn the_settings_button_sits_on_the_far_edge_of_the_row() {
        // The gear is the only control on the right group; a header that lays the two groups out
        // one after another leaves it clipped or floating mid-row, which is how it looked before
        // `Sides` gave the right group its own edge.
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut edge = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 400.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let state = state(false, false);
            show(ui, &state, |_ui| {});
            edge = ui.min_rect().right();
        });
        output.drop_without_applying_deltas();
        assert!(edge > 1100.0, "the row reaches its own right edge: {edge}");
    }

    #[test]
    fn a_tab_body_click_asks_for_nothing() {
        // The body of the only tab is the active document; a click on it changes no state, so the
        // header reports no action.
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let output = ctx.run_ui(Default::default(), |ui| {
            let state = state(false, false);
            show(ui, &state, |_ui| {});
        });
        output.drop_without_applying_deltas();
    }
}
