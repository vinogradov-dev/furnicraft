//! The editor: the view state a UI session owns, split by the mode that owns it.
//!
//! `Editor` holds no document data. Every entry point reads a `Project` or a `Session` and
//! dispatches a command for anything that changes the scene; the struct itself is the one place
//! where the modes meet. Each mode keeps its state and the methods that enter, feed and leave it in
//! its own module: `selection.rs` (the selection and the open set), `mesh.rs` (sub-object editing),
//! `contour.rs` (the contour working copy), `drawing.rs` (the drawing draft and its commit),
//! `creation.rs` (the armed container and the `create` commands), `ribbon.rs` (the ribbon and the
//! extrusion it offers) and `cards.rs` (the scene tree and the inspector).
//!
//! Refuses nothing at this level: a mode that cannot start returns `false` and leaves the state
//! untouched, so the shell can ignore a gesture it cannot honour.
//!
//! Changes when: the editor gains or loses a pointer mode, a card or a field they share.

use std::collections::HashSet;

use furnicraft_model::Id;

use crate::camera_card::CameraCard;
use crate::container_card::ContainerCard;
use crate::contour_edit::{ContourEdit, DrawDraft};
use crate::light_card::LightCard;
use crate::mesh_edit::MeshEdit;
use crate::object_card::ObjectCard;
use crate::panel_card::PanelCard;
use crate::pivot_card::PivotCard;
use crate::placement_card::PlacementCard;
use crate::properties_card::PropertiesCard;
use crate::scene_tree::Selection;
use crate::toolbars;

use self::creation::PendingPrimitive;

mod cards;
mod contour;
mod creation;
mod drawing;
mod mesh;
mod ribbon;
mod selection;

#[cfg(test)]
mod tests;

/// Selection and draft state owned by the editor panels.
///
/// The composition root of the modes above: each method touches the state of one mode and no
/// method takes two modes at once, so the field list is the list of modes, not a god object.
#[derive(Debug, Default)]
pub struct Editor {
    selection: Selection,
    open: HashSet<Id>,
    mesh_edit: Option<MeshEdit>,
    contour_edit: Option<ContourEdit>,
    drawing: Option<DrawDraft>,
    pending_primitive: Option<PendingPrimitive>,
    /// The container the next created nodes land in, or `None` for the scene root.
    creation_parent: Option<Id>,
    toolbars: toolbars::Toolbars,
    card: PanelCard,
    container_card: ContainerCard,
    object_card: ObjectCard,
    camera_card: CameraCard,
    light_card: LightCard,
    placement_card: PlacementCard,
    pivot_card: PivotCard,
    properties_card: PropertiesCard,
    error: Option<String>,
    initialized: bool,
}

impl Editor {
    /// Creates an editor with no node selected; the scene tree selects the first panel on first show.
    pub fn new() -> Self {
        Self::default()
    }
}
