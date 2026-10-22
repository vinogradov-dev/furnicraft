//! `panel.*` commands.

use furnicraft_model::{Id, PanelSize, Placement, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;

/// Stable id of the panel creation command.
pub const CREATE_ID: &str = "panel.create";

/// User-facing label of the panel creation command.
pub const CREATE_LABEL: &str = "Create panel";

/// Parameters for `panel.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCreate {
    /// Id of the assembly that receives the panel.
    pub parent: Id,
    /// User-visible panel name; it must contain a non-whitespace character.
    pub name: String,
    /// Panel dimensions.
    pub size: PanelSize,
    /// Placement relative to the parent assembly.
    pub placement: Placement,
}

impl PanelCreate {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CREATE_ID, message: error.to_string() })
    }

    fn validate_name(&self) -> Result<(), CommandError> {
        if self.name.trim().is_empty() {
            return Err(CommandError::InvalidParams {
                command: CREATE_ID,
                message: "panel name must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

/// The `panel.create` command.
pub(crate) const CREATE: CommandSpec = CommandSpec::new(CREATE_ID, CREATE_LABEL, create_enabled, create_run);

fn create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCreate::parse(params) else {
        return false;
    };
    params.validate_name().is_ok() && session.project().assembly(params.parent).is_some()
}

fn create_run(project: &mut Project, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelCreate::parse(params)?;
    params.validate_name()?;
    let id = project.add_panel(params.parent, params.name, params.size, params.placement)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the panel resize command.
pub const RESIZE_ID: &str = "panel.resize";

/// User-facing label of the panel resize command.
pub const RESIZE_LABEL: &str = "Resize panel";

/// Parameters for `panel.resize`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelResize {
    /// Id of the panel to resize.
    pub panel: Id,
    /// New panel dimensions.
    pub size: PanelSize,
}

impl PanelResize {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: RESIZE_ID, message: error.to_string() })
    }
}

/// The `panel.resize` command.
pub(crate) const RESIZE: CommandSpec = CommandSpec::new(RESIZE_ID, RESIZE_LABEL, resize_enabled, resize_run);

fn resize_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelResize::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some()
}

fn resize_run(project: &mut Project, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelResize::parse(params)?;
    project.set_panel_size(params.panel, params.size)?;
    Ok(CommandOutcome::changed())
}
