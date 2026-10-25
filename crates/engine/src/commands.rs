//! Command metadata, the static registry and dispatch errors.

use furnicraft_model::{Catalog, Id, MaterialId, Project, ProjectError};
use serde_json::Value;
use thiserror::Error;

use crate::node_cmds;
use crate::panel_cmds;
use crate::session::Session;

/// The result of a successful command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    created_id: Option<Id>,
}

impl CommandOutcome {
    /// A command that changed the document without creating a new entity.
    pub const fn changed() -> Self {
        Self { created_id: None }
    }

    /// A command that created an entity.
    pub const fn created(id: Id) -> Self {
        Self { created_id: Some(id) }
    }

    /// The created entity id, if the command created one.
    pub const fn created_id(self) -> Option<Id> {
        self.created_id
    }
}

/// Metadata and implementation of one document command.
///
/// The id is lowercase `<area>.<verb>` and stable forever. The label is user-facing English.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    id: &'static str,
    label: &'static str,
    enabled_fn: fn(&Session, &Value) -> bool,
    run_fn: fn(&mut Project, &Catalog, &Value) -> Result<CommandOutcome, CommandError>,
}

impl CommandSpec {
    /// Defines a command from its metadata and implementation.
    pub const fn new(
        id: &'static str,
        label: &'static str,
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
    pub const fn label(self) -> &'static str {
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
    node_cmds::ASSEMBLY_CREATE,
    panel_cmds::RENAME,
    node_cmds::ASSEMBLY_RENAME,
    panel_cmds::RESIZE,
    panel_cmds::MATERIAL_SET,
    panel_cmds::EDGE_BANDING_SET,
    panel_cmds::EDGE_BANDING_CLEAR,
    panel_cmds::CORNER_BANDING_SET,
    panel_cmds::CORNER_BANDING_CLEAR,
    panel_cmds::CORNER_SET,
    panel_cmds::CORNER_CLEAR,
    panel_cmds::BOW_SET,
    panel_cmds::BOW_CLEAR,
    panel_cmds::CUTOUT_CREATE,
    node_cmds::NODE_DELETE,
    node_cmds::NODE_DUPLICATE,
    node_cmds::NODE_GROUP,
    node_cmds::NODE_UNGROUP,
    node_cmds::NODE_PLACEMENT_SET,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_panel_commands() {
        let registry = CommandRegistry::new();
        let create = registry.get("panel.create").expect("panel.create is registered");
        assert_eq!(create.id(), "panel.create");
        assert_eq!(create.label(), "Create panel");
        let rename = registry.get("panel.rename").expect("panel.rename is registered");
        assert_eq!(rename.id(), "panel.rename");
        assert_eq!(rename.label(), "Rename panel");
        let resize = registry.get("panel.resize").expect("panel.resize is registered");
        assert_eq!(resize.id(), "panel.resize");
        assert_eq!(resize.label(), "Resize panel");
        let material = registry.get("panel.material.set").expect("panel.material.set is registered");
        assert_eq!(material.id(), "panel.material.set");
        assert_eq!(material.label(), "Set panel material");
        let band = registry.get("panel.edge_banding.set").expect("panel.edge_banding.set is registered");
        assert_eq!(band.id(), "panel.edge_banding.set");
        assert_eq!(band.label(), "Set edge banding");
        let clear = registry.get("panel.edge_banding.clear").expect("panel.edge_banding.clear is registered");
        assert_eq!(clear.id(), "panel.edge_banding.clear");
        assert_eq!(clear.label(), "Clear edge banding");
        let corner_band_set = registry.get("panel.corner_banding.set").expect("panel.corner_banding.set is registered");
        assert_eq!(corner_band_set.id(), "panel.corner_banding.set");
        assert_eq!(corner_band_set.label(), "Set corner banding");
        let corner_band_clear =
            registry.get("panel.corner_banding.clear").expect("panel.corner_banding.clear is registered");
        assert_eq!(corner_band_clear.id(), "panel.corner_banding.clear");
        assert_eq!(corner_band_clear.label(), "Clear corner banding");
        let corner_set = registry.get("panel.corner.set").expect("panel.corner.set is registered");
        assert_eq!(corner_set.id(), "panel.corner.set");
        assert_eq!(corner_set.label(), "Set corner feature");
        let corner_clear = registry.get("panel.corner.clear").expect("panel.corner.clear is registered");
        assert_eq!(corner_clear.id(), "panel.corner.clear");
        assert_eq!(corner_clear.label(), "Clear corner feature");
        let bow_set = registry.get("panel.bow.set").expect("panel.bow.set is registered");
        assert_eq!(bow_set.id(), "panel.bow.set");
        assert_eq!(bow_set.label(), "Set edge bow");
        let bow_clear = registry.get("panel.bow.clear").expect("panel.bow.clear is registered");
        assert_eq!(bow_clear.id(), "panel.bow.clear");
        assert_eq!(bow_clear.label(), "Clear edge bow");
        let cutout = registry.get("cutout.create").expect("cutout.create is registered");
        assert_eq!(cutout.id(), "cutout.create");
        assert_eq!(cutout.label(), "Create cutout");
        let assembly_create = registry.get("assembly.create").expect("assembly.create is registered");
        assert_eq!(assembly_create.id(), "assembly.create");
        assert_eq!(assembly_create.label(), "Create assembly");
        let assembly_rename = registry.get("assembly.rename").expect("assembly.rename is registered");
        assert_eq!(assembly_rename.id(), "assembly.rename");
        assert_eq!(assembly_rename.label(), "Rename assembly");
        let delete = registry.get("node.delete").expect("node.delete is registered");
        assert_eq!(delete.id(), "node.delete");
        assert_eq!(delete.label(), "Delete");
        let duplicate = registry.get("node.duplicate").expect("node.duplicate is registered");
        assert_eq!(duplicate.id(), "node.duplicate");
        assert_eq!(duplicate.label(), "Duplicate");
        let group = registry.get("node.group").expect("node.group is registered");
        assert_eq!(group.id(), "node.group");
        assert_eq!(group.label(), "Group");
        let ungroup = registry.get("node.ungroup").expect("node.ungroup is registered");
        assert_eq!(ungroup.id(), "node.ungroup");
        assert_eq!(ungroup.label(), "Ungroup");
        let placement = registry.get("node.placement.set").expect("node.placement.set is registered");
        assert_eq!(placement.id(), "node.placement.set");
        assert_eq!(placement.label(), "Place");
        assert!(registry.get("missing.command").is_none());
        assert_eq!(registry.specs().len(), 20);
    }
}
