//! The shell's keyboard: the global shortcuts and the drawing tools' keys.
//!
//! [`take_shortcuts`] reads and consumes the modified pair and the view keys in one pass, so one key
//! does one thing; [`drawing_key`] reads the two keys a running draft obeys and
//! [`take_drawing_shortcut`] takes at most one tool key from the table the button hints print. Every
//! reader here reports what was pressed and the shell performs it: no shortcut mutates the document.
//!
//! Changes when: a key the Settings window advertises, or the table of tool keys, changes.

use eframe::egui;
use furnicraft_ui::Editor;

/// The global shortcuts, read in one pass and consumed so one key does one thing.
///
/// The list is the one the Settings window draws; a key added there has to be added here, and a key
/// removed here must leave the table. The drawing tools read their own keys, so only the modified
/// pair and the view keys live at this level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shortcuts {
    pub(crate) save: bool,
    /// Open an empty scene in a new tab.
    pub(crate) new_tab: bool,
    /// Close the active tab, asking first when it has unsaved changes.
    pub(crate) close_tab: bool,
    pub(crate) undo: bool,
    pub(crate) redo: bool,
    pub(crate) frame: bool,
    pub(crate) projection: bool,
    pub(crate) maximize: bool,
    pub(crate) palette: bool,
}

/// What one frame's keys ask of a running drawing draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftKey {
    /// Drop the draft; the tool leaves nothing in the document.
    Cancel,
    /// Commit the draft as one path.
    Commit,
}

/// The key a running draft obeys: Escape cancels, Enter commits (ADR 0076).
///
/// Escape wins a frame that somehow carried both, because the destructive key is the one the user
/// pressed to get out of the gesture. The right button is handled by the viewport separately.
pub(crate) const fn drawing_key(escape: bool, enter: bool) -> Option<DraftKey> {
    if escape {
        Some(DraftKey::Cancel)
    } else if enter {
        Some(DraftKey::Commit)
    } else {
        None
    }
}

pub(crate) fn take_shortcuts(ctx: &egui::Context) -> Shortcuts {
    ctx.input_mut(|input| Shortcuts {
        save: input.consume_key(egui::Modifiers::CTRL, egui::Key::S),
        new_tab: input.consume_key(egui::Modifiers::CTRL, egui::Key::N),
        close_tab: input.consume_key(egui::Modifiers::CTRL, egui::Key::W),
        undo: input.consume_key(egui::Modifiers::CTRL, egui::Key::Z),
        redo: input.consume_key(egui::Modifiers::CTRL, egui::Key::Y),
        frame: input.consume_key(egui::Modifiers::NONE, egui::Key::F),
        projection: input.consume_key(egui::Modifiers::NONE, egui::Key::O),
        maximize: input.consume_key(egui::Modifiers::ALT, egui::Key::W),
        palette: input.consume_key(egui::Modifiers::COMMAND, egui::Key::K),
    })
}

/// Consumes at most one drawing-tool key and starts its tool.
///
/// The table is [`furnicraft_ui::toolbars::shortcut`], the same one the button hints print, so the
/// key and the tooltip cannot drift apart. Returns whether a key was taken. A running draft is
/// replaced, because of ADR 0076: the old draft wrote nothing and the new tool owns the next click.
pub(crate) fn take_drawing_shortcut(ctx: &egui::Context, editor: &mut Editor, sides: u32) -> bool {
    for tool in furnicraft_ui::toolbars::CONTOUR_TOOLS {
        let Some(shortcut) = furnicraft_ui::toolbars::shortcut(tool) else {
            continue;
        };
        let modifiers = if shortcut.shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
        if ctx.input_mut(|input| input.consume_key(modifiers, shortcut.key)) {
            editor.start_drawing_with(tool, sides);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_scene::Camera;
    use furnicraft_ui::display::{OpeningView, ShadingStyle};
    use furnicraft_ui::{DrawDraft, DrawTool};
    use furnicraft_units::Length;

    use super::*;

    /// One key press, as the platform would hand it to `egui`.
    fn key_input(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        }
    }

    #[test]
    fn escape_cancels_a_draft_and_enter_commits_it() {
        assert_eq!(drawing_key(true, false), Some(DraftKey::Cancel), "Escape cancels");
        assert_eq!(drawing_key(false, true), Some(DraftKey::Commit), "Enter commits");
        assert_eq!(drawing_key(true, true), Some(DraftKey::Cancel), "the way out wins a frame with both");
        assert_eq!(drawing_key(false, false), None, "no key changes the draft");
    }

    #[test]
    fn a_drawing_shortcut_replaces_the_running_draft() {
        // `R` while a polyline runs starts the rectangle tool with an empty draft (ADR 0076).
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        editor.start_drawing(DrawTool::Polyline);
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
        editor.drawing_point([Length::from_millimetres(600).expect("x"), Length::ZERO]).expect("end");

        let mut started = false;
        let output = ctx.run_ui(key_input(egui::Key::R, egui::Modifiers::NONE), |ui| {
            let sides = editor.polygon_sides();
            started = take_drawing_shortcut(ui.ctx(), &mut editor, sides);
        });
        output.drop_without_applying_deltas();
        assert!(started, "the key is taken");
        let draft = editor.drawing().expect("the rectangle tool is live");
        assert_eq!(draft.tool(), DrawTool::Rectangle);
        assert!(draft.points().is_empty(), "the running polyline is gone");
    }

    #[test]
    fn a_drawing_shortcut_takes_one_key_at_a_time() {
        // A plain letter consumes the frame; a second tool would otherwise start from the same
        // key press through the remainder of the table.
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut started = false;
        let output = ctx.run_ui(key_input(egui::Key::N, egui::Modifiers::NONE), |ui| {
            let sides = editor.polygon_sides();
            started = take_drawing_shortcut(ui.ctx(), &mut editor, sides);
        });
        output.drop_without_applying_deltas();
        assert!(started);
        assert_eq!(editor.drawing().map(DrawDraft::tool), Some(DrawTool::RegularPolygon));

        // The same frame cannot start a second tool: the table already consumed its key.
        let output = ctx.run_ui(Default::default(), |ui| {
            let sides = editor.polygon_sides();
            assert!(!take_drawing_shortcut(ui.ctx(), &mut editor, sides), "no new key");
        });
        output.drop_without_applying_deltas();
        assert_eq!(editor.drawing().map(DrawDraft::tool), Some(DrawTool::RegularPolygon));
    }

    #[test]
    fn the_advertised_shortcuts_are_consumed() {
        let ctx = egui::Context::default();
        let mut read = Shortcuts::default();
        let output = ctx.run_ui(key_input(egui::Key::Z, egui::Modifiers::CTRL), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { undo: true, ..Shortcuts::default() });

        let output = ctx.run_ui(key_input(egui::Key::F, egui::Modifiers::NONE), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { frame: true, ..Shortcuts::default() });

        // `F` with a modifier is a different shortcut and must not reframe.
        let output = ctx.run_ui(key_input(egui::Key::F, egui::Modifiers::CTRL), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts::default());

        let output = ctx.run_ui(key_input(egui::Key::K, egui::Modifiers::COMMAND), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { palette: true, ..Shortcuts::default() });
    }

    #[test]
    fn alt_w_is_consumed_and_a_single_layout_refuses_it() {
        let ctx = egui::Context::default();
        let mut read = Shortcuts::default();
        let output = ctx.run_ui(key_input(egui::Key::W, egui::Modifiers::ALT), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { maximize: true, ..Shortcuts::default() });

        // A plain `W` is a different key and must not expand anything.
        let output = ctx.run_ui(key_input(egui::Key::W, egui::Modifiers::NONE), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts::default());

        // One view already fills the workspace, so the key is a no-op in the single layout.
        let mut layout =
            crate::view_layout::Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        layout.toggle_maximized();
        assert!(!layout.maximized());
    }
}
