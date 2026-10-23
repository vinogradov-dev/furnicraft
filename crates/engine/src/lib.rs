//! Session state, the command registry and undo/redo.
//!
//! Every user-visible change to the document is a command with a stable id, typed parameters and a
//! `Result`. Commands mutate the project only through [`Session::dispatch`], which snapshots the
//! document for undo and restores it if the command fails. This crate must not depend on `wgpu` or
//! `egui`.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod commands;
mod diagnostics;
pub mod panel_cmds;
mod resolve;
mod session;

pub use commands::{CommandError, CommandOutcome, CommandRegistry, CommandSpec};
pub use diagnostics::{PanelDiagnostic, diagnose_panel};
pub use panel_cmds::{
    CutoutCreate, PanelBowClear, PanelBowSet, PanelCornerClear, PanelCornerSet, PanelCreate, PanelEdgeBandingClear,
    PanelEdgeBandingSet, PanelMaterialSet, PanelRename, PanelResize,
};
pub use session::Session;
