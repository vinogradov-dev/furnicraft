//! The machining records of a panel: add, update, remove and reorder.
//!
//! A record is one anchored parameter set — a face or edge anchor plus tagged parameters — stored
//! in panel-local coordinates. The command validates the anchor and the parameters through
//! `Project`, and creation reports the new record id in [`CommandOutcome`]. Parameters are JSON
//! objects with `deny_unknown_fields`.
//!
//! It refuses an anchor outside the panel, parameters the anchor's kind does not accept, an
//! unknown machining id and a panel that is not in the project.
//!
//! Changes when: the shape or the order of a panel's machining records changes.
//!
//! It does not do: compute the machining effects or the outline they cut — `furnicraft-param`
//! owns that.

use furnicraft_model::{Catalog, Id, MachiningAnchor, MachiningId, MachiningParams, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;
use furnicraft_text::Message;

/// Stable id of the machining creation command.
pub const MACHINING_ADD_ID: &str = "panel.machining.add";

/// User-facing label of the machining creation command.
pub const MACHINING_ADD_LABEL: Message = Message::CmdAddMachining;

/// Parameters for `panel.machining.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMachiningAdd {
    /// Id of the panel that receives the operation.
    pub panel: Id,
    /// Face or edge the operation is anchored to.
    pub anchor: MachiningAnchor,
    /// What the operation does.
    pub params: MachiningParams,
}

impl PanelMachiningAdd {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: MACHINING_ADD_ID, message: error.to_string() })
    }
}

/// The `panel.machining.add` command.
pub(crate) const MACHINING_ADD: CommandSpec =
    CommandSpec::new(MACHINING_ADD_ID, MACHINING_ADD_LABEL, machining_add_enabled, machining_add_run);

fn machining_add_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelMachiningAdd::parse(params) else {
        return false;
    };
    session.project().validate_machining(params.panel, params.anchor, &params.params).is_ok()
}

fn machining_add_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelMachiningAdd::parse(params)?;
    let id = project.add_machining(params.panel, params.anchor, params.params)?;
    Ok(CommandOutcome::created_machining(id))
}

/// Stable id of the machining update command.
pub const MACHINING_UPDATE_ID: &str = "panel.machining.update";

/// User-facing label of the machining update command.
pub const MACHINING_UPDATE_LABEL: Message = Message::CmdUpdateMachining;

/// Parameters for `panel.machining.update`.
///
/// The record keeps its id and its position in the list; the anchor and parameters are replaced as
/// one validated unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMachiningUpdate {
    /// Id of the panel that owns the operation.
    pub panel: Id,
    /// Stable id of the record to replace.
    pub machining: MachiningId,
    /// Face or edge the operation is anchored to.
    pub anchor: MachiningAnchor,
    /// What the operation does.
    pub params: MachiningParams,
}

impl PanelMachiningUpdate {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: MACHINING_UPDATE_ID, message: error.to_string() })
    }
}

/// The `panel.machining.update` command.
pub(crate) const MACHINING_UPDATE: CommandSpec =
    CommandSpec::new(MACHINING_UPDATE_ID, MACHINING_UPDATE_LABEL, machining_update_enabled, machining_update_run);

fn machining_update_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelMachiningUpdate::parse(params) else {
        return false;
    };
    session.project().validate_machining_update(params.panel, params.machining, params.anchor, &params.params).is_ok()
}

fn machining_update_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelMachiningUpdate::parse(params)?;
    project.update_machining(params.panel, params.machining, params.anchor, params.params)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the machining removal command.
pub const MACHINING_REMOVE_ID: &str = "panel.machining.remove";

/// User-facing label of the machining removal command.
pub const MACHINING_REMOVE_LABEL: Message = Message::CmdRemoveMachining;

/// Parameters for `panel.machining.remove`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMachiningRemove {
    /// Id of the panel that owns the operation.
    pub panel: Id,
    /// Stable id of the record to remove.
    pub machining: MachiningId,
}

impl PanelMachiningRemove {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: MACHINING_REMOVE_ID, message: error.to_string() })
    }
}

/// The `panel.machining.remove` command.
pub(crate) const MACHINING_REMOVE: CommandSpec =
    CommandSpec::new(MACHINING_REMOVE_ID, MACHINING_REMOVE_LABEL, machining_remove_enabled, machining_remove_run);

fn machining_remove_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelMachiningRemove::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.machining_by_id(params.machining).is_some())
}

fn machining_remove_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelMachiningRemove::parse(params)?;
    project.remove_machining(params.panel, params.machining)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the machining reorder command.
pub const MACHINING_MOVE_ID: &str = "panel.machining.move";

/// User-facing label of the machining reorder command.
pub const MACHINING_MOVE_LABEL: Message = Message::CmdMoveMachining;

/// Parameters for `panel.machining.move`.
///
/// `index` is the position after the move, in `0..len`. The record keeps its id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMachiningMove {
    /// Id of the panel that owns the operation.
    pub panel: Id,
    /// Stable id of the record to move.
    pub machining: MachiningId,
    /// Final zero-based position in the list.
    pub index: usize,
}

impl PanelMachiningMove {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: MACHINING_MOVE_ID, message: error.to_string() })
    }
}

/// The `panel.machining.move` command.
pub(crate) const MACHINING_MOVE: CommandSpec =
    CommandSpec::new(MACHINING_MOVE_ID, MACHINING_MOVE_LABEL, machining_move_enabled, machining_move_run);

fn machining_move_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelMachiningMove::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| {
        panel.machining_by_id(params.machining).is_some() && params.index < panel.machining().len()
    })
}

fn machining_move_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelMachiningMove::parse(params)?;
    project.move_machining(params.panel, params.machining, params.index)?;
    Ok(CommandOutcome::changed())
}
