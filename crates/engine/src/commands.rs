//! Command metadata, the static registry and dispatch errors.

use furnicraft_model::{Catalog, Id, MaterialId, Project, ProjectError};
use serde_json::Value;
use thiserror::Error;

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
    panel_cmds::RESIZE,
    panel_cmds::MATERIAL_SET,
    panel_cmds::EDGE_BANDING_SET,
    panel_cmds::EDGE_BANDING_CLEAR,
    panel_cmds::CUTOUT_CREATE,
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
        let cutout = registry.get("cutout.create").expect("cutout.create is registered");
        assert_eq!(cutout.id(), "cutout.create");
        assert_eq!(cutout.label(), "Create cutout");
        assert!(registry.get("missing.command").is_none());
        assert_eq!(registry.specs().len(), 6);
    }
}
