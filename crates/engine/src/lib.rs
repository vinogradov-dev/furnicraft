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

mod class_rules;
mod commands;
mod diagnostics;
pub mod mesh_cmds;
pub mod node_cmds;
pub mod object_cmds;
pub mod panel_cmds;
mod preview;
pub mod product_cmds;
mod resolve;
pub mod scene_cmds;
mod session;

pub use class_rules::{
    ClassRefusal, CreationRefusal, Dimension, NodeRules, allow_creation, is_furniture, operation_of,
};
pub use commands::{CommandError, CommandOutcome, CommandRegistry, CommandSpec, Creation};
pub use diagnostics::{PanelDiagnostic, diagnose_panel};
pub use mesh_cmds::{MESH_VERTICES_MOVE_ID, MESH_VERTICES_MOVE_LABEL, MeshVertexMove, MeshVerticesMove};
pub use node_cmds::{
    AssemblyCreate, NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup, NodePivotCenter, NodePivotSet,
    NodePlacement, NodePlacementSet, NodePlacementSetMany, NodePlacementWorldZero, NodeProperty, NodePropertyClear,
    NodePropertySet, NodePropertySetMany, NodeRename, NodeUngroup,
};
pub use object_cmds::{
    ObjectCollapse, ObjectContourReplace, ObjectExtrudeCreate, ObjectExtrudeHeightSet, ObjectPathCreate,
    ObjectPathExtrude, ObjectPathReplace, ObjectPrimitiveCreate, ObjectPrimitiveReplace, PrimitiveKind,
};
pub use panel_cmds::{
    PanelBevelClear, PanelBevelSet, PanelBowClear, PanelBowSet, PanelCornerBandingClear, PanelCornerBandingSet,
    PanelCornerClear, PanelCornerSet, PanelCreate, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMachiningAdd,
    PanelMachiningMove, PanelMachiningRemove, PanelMachiningUpdate, PanelMaterialSet, PanelOutlineSet,
    PanelOutlineToContour, PanelRename, PanelResize, PanelRingBandingClear, PanelRingBandingSet,
};
pub use preview::Preview;
pub use product_cmds::{ProductCreate, ProductEnvelopeSet};
pub use scene_cmds::{CameraCreate, CameraProjectionSet, LightCreate, LightParamsSet, SceneActiveCameraSet};
pub use session::{EditScope, Session};
