//! Document commands for panels and their cutouts.

use furnicraft_model::{
    Catalog, CutoutDepth, CutoutShape, FaceSide, Id, MaterialId, PanelNominalSize, Placement, Project,
};
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
    /// Panel nominal length and width.
    pub nominal_size: PanelNominalSize,
    /// Panel-body material from the session catalogue.
    pub material: MaterialId,
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
    params.validate_name().is_ok()
        && session.project().assembly(params.parent).is_some()
        && session.catalog().panel_material(params.material).is_some()
}

fn create_run(project: &mut Project, catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelCreate::parse(params)?;
    params.validate_name()?;
    let material =
        catalog.panel_material(params.material).ok_or(CommandError::UnknownMaterial { material: params.material })?;
    let id = project.add_panel(
        params.parent,
        params.name,
        params.nominal_size,
        Some(params.material),
        material.nominal_thickness(),
        params.placement,
    )?;
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
    /// New nominal panel length and width.
    pub nominal_size: PanelNominalSize,
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
    session.project().validate_panel_resize(params.panel, params.nominal_size).is_ok()
}

fn resize_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelResize::parse(params)?;
    project.set_panel_nominal_size(params.panel, params.nominal_size)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the cutout creation command.
pub const CUTOUT_CREATE_ID: &str = "cutout.create";

/// User-facing label of the cutout creation command.
pub const CUTOUT_CREATE_LABEL: &str = "Create cutout";

/// Parameters for `cutout.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoutCreate {
    /// Id of the panel that receives the cutout.
    pub panel: Id,
    /// Cutout outline in panel-local coordinates.
    pub shape: CutoutShape,
    /// Through or blind depth.
    pub depth: CutoutDepth,
    /// Face the cut enters from.
    pub entry: FaceSide,
}

impl CutoutCreate {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CUTOUT_CREATE_ID, message: error.to_string() })
    }
}

/// The `cutout.create` command.
pub(crate) const CUTOUT_CREATE: CommandSpec =
    CommandSpec::new(CUTOUT_CREATE_ID, CUTOUT_CREATE_LABEL, cutout_create_enabled, cutout_create_run);

fn cutout_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = CutoutCreate::parse(params) else {
        return false;
    };
    session.project().validate_cutout(params.panel, params.shape, params.depth).is_ok()
}

fn cutout_create_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = CutoutCreate::parse(params)?;
    let id = project.add_cutout(params.panel, params.shape, params.depth, params.entry)?;
    Ok(CommandOutcome::created(id))
}
