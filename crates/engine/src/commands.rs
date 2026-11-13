//! Command metadata, the static registry and dispatch errors.

use furnicraft_model::{Catalog, Id, MachiningId, MaterialId, Project, ProjectError};
use furnicraft_text::Message;
use serde_json::Value;
use thiserror::Error;

use crate::class_rules::ClassRefusal;
use crate::mesh_cmds;
use crate::node_cmds;
use crate::object_cmds;
use crate::panel_cmds;
use crate::product_cmds;
use crate::scene_cmds;
use crate::session::Session;

/// The result of a successful command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    created_id: Option<Id>,
    created_machining: Option<MachiningId>,
}

impl CommandOutcome {
    /// A command that changed the document without creating a new entity.
    pub const fn changed() -> Self {
        Self { created_id: None, created_machining: None }
    }

    /// A command that created a node.
    pub const fn created(id: Id) -> Self {
        Self { created_id: Some(id), created_machining: None }
    }

    /// A command that created a machining record.
    pub const fn created_machining(id: MachiningId) -> Self {
        Self { created_id: None, created_machining: Some(id) }
    }

    /// The created node id, if the command created one.
    pub const fn created_id(self) -> Option<Id> {
        self.created_id
    }

    /// The created machining id, if the command created one.
    pub const fn created_machining_id(self) -> Option<MachiningId> {
        self.created_machining
    }
}

/// Metadata and implementation of one document command.
///
/// The id is lowercase `<area>.<verb>` and stable forever. The label is a [`Message`], so the
/// palette, a CLI and the assistant print the same words in the user's language.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    id: &'static str,
    label: Message,
    enabled_fn: fn(&Session, &Value) -> bool,
    run_fn: fn(&mut Project, &Catalog, &Value) -> Result<CommandOutcome, CommandError>,
}

impl CommandSpec {
    /// Defines a command from its metadata and implementation.
    pub const fn new(
        id: &'static str,
        label: Message,
        enabled_fn: fn(&Session, &Value) -> bool,
        run_fn: fn(&mut Project, &Catalog, &Value) -> Result<CommandOutcome, CommandError>,
    ) -> Self {
        Self { id, label, enabled_fn, run_fn }
    }

    /// The stable command id.
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// The user-facing command label.
    pub const fn label(self) -> Message {
        self.label
    }

    /// Whether the command applies to this session and these parameters.
    ///
    /// This is a pure check for menus, toolbars and enabled state. It never mutates the document.
    pub fn enabled(self, session: &Session, params: &Value) -> bool {
        (self.enabled_fn)(session, params)
    }

    pub(crate) fn run(
        self,
        project: &mut Project,
        catalog: &Catalog,
        params: &Value,
    ) -> Result<CommandOutcome, CommandError> {
        (self.run_fn)(project, catalog, params)
    }
}

/// The commands compiled into this build.
static COMMANDS: &[CommandSpec] = &[
    panel_cmds::CREATE,
    panel_cmds::CONTOUR_CREATE,
    panel_cmds::OUTLINE_SET,
    panel_cmds::OUTLINE_CONTOUR,
    node_cmds::ASSEMBLY_CREATE,
    product_cmds::PRODUCT_CREATE,
    product_cmds::PRODUCT_ENVELOPE_SET,
    panel_cmds::RENAME,
    node_cmds::NODE_RENAME,
    panel_cmds::RESIZE,
    panel_cmds::MATERIAL_SET,
    panel_cmds::EDGE_BANDING_SET,
    panel_cmds::EDGE_BANDING_CLEAR,
    panel_cmds::CORNER_BANDING_SET,
    panel_cmds::CORNER_BANDING_CLEAR,
    panel_cmds::RING_BANDING_SET,
    panel_cmds::RING_BANDING_CLEAR,
    panel_cmds::CORNER_SET,
    panel_cmds::CORNER_CLEAR,
    panel_cmds::BOW_SET,
    panel_cmds::BOW_CLEAR,
    panel_cmds::MACHINING_ADD,
    panel_cmds::MACHINING_UPDATE,
    panel_cmds::MACHINING_REMOVE,
    panel_cmds::MACHINING_MOVE,
    node_cmds::NODE_DELETE,
    node_cmds::NODE_DUPLICATE,
    node_cmds::NODE_GROUP,
    node_cmds::NODE_UNGROUP,
    node_cmds::NODE_PLACEMENT_SET,
    node_cmds::NODE_PLACEMENT_WORLD_ZERO,
    node_cmds::NODE_PLACEMENT_SET_MANY,
    node_cmds::NODE_PIVOT_SET,
    node_cmds::NODE_PIVOT_CENTER,
    node_cmds::NODE_ORIGIN_REBASE,
    node_cmds::NODE_PROPERTY_SET,
    node_cmds::NODE_PROPERTY_SET_MANY,
    node_cmds::NODE_PROPERTY_CLEAR,
    node_cmds::NODE_COMPONENT_CONVERT_TO_GROUP,
    object_cmds::OBJECT_PRIMITIVE_CREATE,
    object_cmds::OBJECT_PRIMITIVE_REPLACE,
    object_cmds::OBJECT_EXTRUDE_CREATE,
    object_cmds::OBJECT_EXTRUDE_HEIGHT_SET,
    object_cmds::OBJECT_CONTOUR_REPLACE,
    object_cmds::OBJECT_PATH_CREATE,
    object_cmds::OBJECT_PATH_EXTRUDE,
    object_cmds::OBJECT_PATH_REPLACE,
    object_cmds::OBJECT_COLLAPSE,
    mesh_cmds::MESH_VERTICES_MOVE,
    scene_cmds::CAMERA_CREATE,
    scene_cmds::LIGHT_CREATE,
    scene_cmds::SCENE_ACTIVE_CAMERA_SET,
    scene_cmds::CAMERA_PROJECTION_SET,
    scene_cmds::LIGHT_PARAMS_SET,
];

/// A lookup table for the compiled command specs.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandRegistry;

impl CommandRegistry {
    /// Creates a registry handle. The specs themselves are static.
    pub const fn new() -> Self {
        Self
    }

    /// All registered commands, in display order.
    pub const fn specs(&self) -> &'static [CommandSpec] {
        COMMANDS
    }

    /// Finds a command by its stable id.
    pub fn get(&self, id: &str) -> Option<&'static CommandSpec> {
        COMMANDS.iter().find(|spec| spec.id == id)
    }
}

/// Why a command could not be dispatched or a history operation failed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    /// No command with this id is registered.
    #[error("unknown command `{id}`")]
    UnknownCommand {
        /// The id that was requested.
        id: String,
    },
    /// The parameters do not match the command's typed parameter struct.
    #[error("invalid parameters for `{command}`: {message}")]
    InvalidParams {
        /// The stable command id.
        command: &'static str,
        /// A human-readable parse or validation message.
        message: String,
    },
    /// The document rejected the change.
    #[error(transparent)]
    Model(#[from] ProjectError),
    /// The node's class forbids what the command would do.
    #[error("`{command}` is not allowed for this object: {refusal}")]
    ClassRefused {
        /// The stable command id.
        command: &'static str,
        /// What the class forbids.
        refusal: ClassRefusal,
    },
    /// Deriving geometry for the command failed.
    #[error("cannot evaluate `{command}`: {message}")]
    Evaluation {
        /// The stable command id.
        command: &'static str,
        /// A human-readable evaluation message.
        message: String,
    },
    /// The catalogue does not contain the requested material.
    #[error("unknown material {material}")]
    UnknownMaterial {
        /// The missing material id.
        material: MaterialId,
    },
    /// There is no earlier document state to restore.
    #[error("nothing to undo")]
    NothingToUndo,
    /// There is no undone command to reapply.
    #[error("nothing to redo")]
    NothingToRedo,
}

impl CommandError {
    /// A class gate's refusal, tagged with the command that asked it.
    pub(crate) fn class(command: &'static str, refusal: ClassRefusal) -> Self {
        Self::ClassRefused { command, refusal }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use furnicraft_text::Lang;

    #[test]
    fn registry_exposes_panel_commands() {
        let registry = CommandRegistry::new();
        let create = registry.get("panel.create").expect("panel.create is registered");
        assert_eq!(create.id(), "panel.create");
        assert_eq!(create.label().text(Lang::En), "Create panel");
        let rename = registry.get("panel.rename").expect("panel.rename is registered");
        assert_eq!(rename.id(), "panel.rename");
        assert_eq!(rename.label().text(Lang::En), "Rename panel");
        let resize = registry.get("panel.resize").expect("panel.resize is registered");
        assert_eq!(resize.id(), "panel.resize");
        assert_eq!(resize.label().text(Lang::En), "Resize panel");
        let material = registry.get("panel.material.set").expect("panel.material.set is registered");
        assert_eq!(material.id(), "panel.material.set");
        assert_eq!(material.label().text(Lang::En), "Set panel material");
        let band = registry.get("panel.edge_banding.set").expect("panel.edge_banding.set is registered");
        assert_eq!(band.id(), "panel.edge_banding.set");
        assert_eq!(band.label().text(Lang::En), "Set edge banding");
        let clear = registry.get("panel.edge_banding.clear").expect("panel.edge_banding.clear is registered");
        assert_eq!(clear.id(), "panel.edge_banding.clear");
        assert_eq!(clear.label().text(Lang::En), "Clear edge banding");
        let corner_band_set = registry.get("panel.corner_banding.set").expect("panel.corner_banding.set is registered");
        assert_eq!(corner_band_set.id(), "panel.corner_banding.set");
        assert_eq!(corner_band_set.label().text(Lang::En), "Set corner banding");
        let corner_band_clear =
            registry.get("panel.corner_banding.clear").expect("panel.corner_banding.clear is registered");
        assert_eq!(corner_band_clear.id(), "panel.corner_banding.clear");
        assert_eq!(corner_band_clear.label().text(Lang::En), "Clear corner banding");
        let corner_set = registry.get("panel.corner.set").expect("panel.corner.set is registered");
        assert_eq!(corner_set.id(), "panel.corner.set");
        assert_eq!(corner_set.label().text(Lang::En), "Set corner feature");
        let corner_clear = registry.get("panel.corner.clear").expect("panel.corner.clear is registered");
        assert_eq!(corner_clear.id(), "panel.corner.clear");
        assert_eq!(corner_clear.label().text(Lang::En), "Clear corner feature");
        let bow_set = registry.get("panel.bow.set").expect("panel.bow.set is registered");
        assert_eq!(bow_set.id(), "panel.bow.set");
        assert_eq!(bow_set.label().text(Lang::En), "Set edge bow");
        let bow_clear = registry.get("panel.bow.clear").expect("panel.bow.clear is registered");
        assert_eq!(bow_clear.id(), "panel.bow.clear");
        assert_eq!(bow_clear.label().text(Lang::En), "Clear edge bow");
        let add = registry.get("panel.machining.add").expect("panel.machining.add is registered");
        assert_eq!(add.id(), "panel.machining.add");
        assert_eq!(add.label().text(Lang::En), "Add machining");
        let update = registry.get("panel.machining.update").expect("panel.machining.update is registered");
        assert_eq!(update.id(), "panel.machining.update");
        assert_eq!(update.label().text(Lang::En), "Update machining");
        let remove = registry.get("panel.machining.remove").expect("panel.machining.remove is registered");
        assert_eq!(remove.id(), "panel.machining.remove");
        assert_eq!(remove.label().text(Lang::En), "Remove machining");
        let reposition = registry.get("panel.machining.move").expect("panel.machining.move is registered");
        assert_eq!(reposition.id(), "panel.machining.move");
        assert_eq!(reposition.label().text(Lang::En), "Move machining");
        let outline_set = registry.get("panel.outline.set").expect("panel.outline.set is registered");
        assert_eq!(outline_set.id(), "panel.outline.set");
        assert_eq!(outline_set.label().text(Lang::En), "Set panel outline");
        let outline_contour = registry.get("panel.outline.to_contour").expect("panel.outline.to_contour is registered");
        assert_eq!(outline_contour.id(), "panel.outline.to_contour");
        assert_eq!(outline_contour.label().text(Lang::En), "Bake panel outline into a contour");
        let assembly_create = registry.get("assembly.create").expect("assembly.create is registered");
        assert_eq!(assembly_create.id(), "assembly.create");
        assert_eq!(assembly_create.label().text(Lang::En), "Create assembly");
        let product_create = registry.get("product.create").expect("product.create is registered");
        assert_eq!(product_create.id(), "product.create");
        assert_eq!(product_create.label().text(Lang::En), "Create product frame");
        let envelope_set = registry.get("product.envelope.set").expect("product.envelope.set is registered");
        assert_eq!(envelope_set.id(), "product.envelope.set");
        assert_eq!(envelope_set.label().text(Lang::En), "Set product frame size");
        let node_rename = registry.get("node.rename").expect("node.rename is registered");
        assert_eq!(node_rename.id(), "node.rename");
        assert_eq!(node_rename.label().text(Lang::En), "Rename");
        let delete = registry.get("node.delete").expect("node.delete is registered");
        assert_eq!(delete.id(), "node.delete");
        assert_eq!(delete.label().text(Lang::En), "Delete");
        let duplicate = registry.get("node.duplicate").expect("node.duplicate is registered");
        assert_eq!(duplicate.id(), "node.duplicate");
        assert_eq!(duplicate.label().text(Lang::En), "Duplicate");
        let group = registry.get("node.group").expect("node.group is registered");
        assert_eq!(group.id(), "node.group");
        assert_eq!(group.label().text(Lang::En), "Group");
        let ungroup = registry.get("node.ungroup").expect("node.ungroup is registered");
        assert_eq!(ungroup.id(), "node.ungroup");
        assert_eq!(ungroup.label().text(Lang::En), "Ungroup");
        let placement = registry.get("node.placement.set").expect("node.placement.set is registered");
        assert_eq!(placement.id(), "node.placement.set");
        assert_eq!(placement.label().text(Lang::En), "Place");
        let placement_world_zero =
            registry.get("node.placement.world_zero").expect("node.placement.world_zero is registered");
        assert_eq!(placement_world_zero.id(), "node.placement.world_zero");
        assert_eq!(placement_world_zero.label().text(Lang::En), "Place at world zero");
        let placement_many = registry.get("node.placement.set_many").expect("node.placement.set_many is registered");
        assert_eq!(placement_many.id(), "node.placement.set_many");
        assert_eq!(placement_many.label().text(Lang::En), "Place selection");
        let pivot_set = registry.get("node.pivot.set").expect("node.pivot.set is registered");
        assert_eq!(pivot_set.id(), "node.pivot.set");
        assert_eq!(pivot_set.label().text(Lang::En), "Set pivot");
        let pivot_center = registry.get("node.pivot.center").expect("node.pivot.center is registered");
        assert_eq!(pivot_center.id(), "node.pivot.center");
        assert_eq!(pivot_center.label().text(Lang::En), "Center pivot");
        let origin_rebase = registry.get("node.origin.rebase").expect("node.origin.rebase is registered");
        assert_eq!(origin_rebase.id(), "node.origin.rebase");
        assert_eq!(origin_rebase.label().text(Lang::En), "Rebase origin");
        let property_set = registry.get("node.property.set").expect("node.property.set is registered");
        assert_eq!(property_set.id(), "node.property.set");
        assert_eq!(property_set.label().text(Lang::En), "Set property");
        let property_many = registry.get("node.property.set_many").expect("node.property.set_many is registered");
        assert_eq!(property_many.id(), "node.property.set_many");
        assert_eq!(property_many.label().text(Lang::En), "Set properties");
        let property_clear = registry.get("node.property.clear").expect("node.property.clear is registered");
        assert_eq!(property_clear.id(), "node.property.clear");
        assert_eq!(property_clear.label().text(Lang::En), "Clear property");
        let convert =
            registry.get("node.component.convert_to_group").expect("node.component.convert_to_group is registered");
        assert_eq!(convert.id(), "node.component.convert_to_group");
        assert_eq!(convert.label().text(Lang::En), "Convert to group");
        let create_object = registry.get("object.primitive.create").expect("object.primitive.create is registered");
        assert_eq!(create_object.id(), "object.primitive.create");
        assert_eq!(create_object.label().text(Lang::En), "Create primitive");
        let replace_primitive =
            registry.get("object.primitive.replace").expect("object.primitive.replace is registered");
        assert_eq!(replace_primitive.id(), "object.primitive.replace");
        assert_eq!(replace_primitive.label().text(Lang::En), "Replace primitive parameters");
        let create_path = registry.get("object.path.create").expect("object.path.create is registered");
        assert_eq!(create_path.id(), "object.path.create");
        assert_eq!(create_path.label().text(Lang::En), "Create path");
        let replace_path = registry.get("object.path.replace").expect("object.path.replace is registered");
        assert_eq!(replace_path.id(), "object.path.replace");
        assert_eq!(replace_path.label().text(Lang::En), "Replace path");
        let move_vertices = registry.get("mesh.vertices.move").expect("mesh.vertices.move is registered");
        assert_eq!(move_vertices.id(), "mesh.vertices.move");
        assert_eq!(move_vertices.label().text(Lang::En), "Move vertices");
        assert!(registry.get("missing.command").is_none());
        assert_eq!(registry.specs().len(), 54);
    }

    #[test]
    fn every_command_label_resolves_in_every_language() {
        let registry = CommandRegistry::new();
        for spec in registry.specs() {
            for lang in Lang::ALL {
                assert!(!spec.label().text(lang).trim().is_empty(), "{} has no label in {lang:?}", spec.id());
            }
        }
    }
}
