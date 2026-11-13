//! The egui shell: panels, widgets and input wiring.
//!
//! Thin by design: actions are dispatched to the engine as commands, and the UI never mutates the
//! document directly. This file is the crate's surface — the modules and the re-exports — while the
//! editor's view state and its per-mode operations live in `editor/`. Which node a viewport click
//! selects and where a creation tool lands are the rules of `target.rs`.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod camera_card;
mod class_picker;
mod command;
pub mod command_bar;
mod container_card;
mod contour_edit;
mod contour_grips;
mod coords;
pub mod display;
mod draw;
mod editor;
pub mod home;
pub mod icons;
mod light_card;
mod machining;
mod mesh_edit;
mod object_card;
mod palette;
mod panel_card;
mod pivot_card;
mod placement_card;
pub mod product_card;
mod properties_card;
mod scene_tree;
mod scrub;
mod section;
pub mod settings;
mod target;
pub mod theme;
pub mod toolbars;
pub mod ui_scale;
mod view_flags;

pub use command_bar::{CommandBar, CommandBarAction};
pub use contour_edit::{ContourDraft, ContourEdit, ContourTarget};
pub use contour_grips::{ContourGrip, GripNumber, GripValue};
pub use draw::{DrawDraft, DrawDraftError, DrawTool, DrawnPath};
pub use editor::Editor;
pub use furnicraft_text::{Counted, Lang, Message};
pub use mesh_edit::{MeshEdit, MeshElement};
pub use palette::{Palette, PaletteContext, PaletteKey};
pub use toolbars::{RibbonView, RibbonViewAction};
pub use view_flags::ViewFlags;

/// The `egui` memory key the shell publishes the workspace language under.
///
/// The language is workspace settings, not document data: the shell writes it once per frame and
/// every surface reads it, so a new panel never has to thread a `Lang` through five signatures to
/// draw one caption. A test publishes its own language the same way.
const LANGUAGE_KEY: &str = "furnicraft.language";

/// Publishes the language every surface draws in.
pub fn set_language(ctx: &egui::Context, lang: Lang) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(LANGUAGE_KEY), lang));
}

/// The language every surface draws in.
pub fn language(ctx: &egui::Context) -> Lang {
    ctx.data(|data| data.get_temp(egui::Id::unique(LANGUAGE_KEY))).unwrap_or_default()
}

/// The `egui` memory key the shell publishes the workspace icon-colour mode under.
const ICON_COLOR_KEY: &str = "furnicraft.icon_color";

/// Publishes how the icon pipeline colours a glyph.
///
/// The mode is workspace settings, not document data, so the shell writes it once per frame and the
/// rasteriser reads it, exactly like the language: a new icon surface never threads a mode through
/// its signature. Before the shell publishes one, [`settings::IconColor::Auto`] applies.
pub fn set_icon_color(ctx: &egui::Context, mode: settings::IconColor) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ICON_COLOR_KEY), mode));
}

/// How the icon pipeline colours a glyph.
pub fn icon_color(ctx: &egui::Context) -> settings::IconColor {
    ctx.data(|data| data.get_temp(egui::Id::unique(ICON_COLOR_KEY))).unwrap_or_default()
}

/// The sentence for `message` in the language of this context.
pub fn text(ctx: &egui::Context, message: Message) -> &'static str {
    message.text(language(ctx))
}

/// The interface's words for a refused length, so a raw `thiserror` sentence never reaches the
/// screen. The error itself stays English for logs and tests.
pub fn length_error(error: furnicraft_units::LengthParseError) -> Message {
    use furnicraft_units::LengthParseError as Error;
    match error {
        Error::Empty => Message::LengthErrorEmpty,
        Error::Invalid => Message::LengthErrorInvalid,
        Error::TooPrecise => Message::LengthErrorTooPrecise,
        Error::OutOfRange => Message::LengthErrorOutOfRange,
    }
}

/// The interface's words for a refused angle, for the same reason as [`length_error`].
pub fn degrees_error(error: furnicraft_units::DegreesParseError) -> Message {
    use furnicraft_units::DegreesParseError as Error;
    match error {
        Error::Empty => Message::DegreesErrorEmpty,
        Error::Invalid => Message::DegreesErrorInvalid,
        Error::TooPrecise => Message::DegreesErrorTooPrecise,
        Error::OutOfRange => Message::DegreesErrorOutOfRange,
    }
}
