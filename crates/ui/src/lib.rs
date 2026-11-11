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
mod command;
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
mod properties_card;
mod scene_tree;
mod section;
pub mod settings;
mod target;
pub mod theme;
pub mod toolbars;
pub mod ui_scale;
mod view_flags;

pub use contour_edit::{ContourDraft, ContourEdit, ContourTarget};
pub use contour_grips::{ContourGrip, GripNumber, GripValue};
pub use draw::{DrawDraft, DrawDraftError, DrawTool, DrawnPath};
pub use editor::Editor;
pub use mesh_edit::{MeshEdit, MeshElement};
pub use palette::{Palette, PaletteContext, PaletteKey};
pub use toolbars::{RibbonTab, RibbonView, RibbonViewAction, ToolGroup};
pub use view_flags::ViewFlags;
