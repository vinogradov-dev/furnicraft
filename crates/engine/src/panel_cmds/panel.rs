//! The panel itself: creation, stored outline, name, nominal size and body material.
//!
//! A panel is born from a nominal size or from another object's contour ring, and it stores its
//! outline as a ring once baked. Parameters are JSON objects with `deny_unknown_fields`; each
//! command validates through `Project` before it mutates and leaves the document untouched on
//! error, because the session restores its snapshot.
//!
//! It refuses an empty name, a nominal size outside the panel's limit, a material the catalogue
//! does not hold or whose thickness the panel's geometry rejects, a source object without a
//! contour, and a panel that is not in the project.
//!
//! Changes when: a panel's creation, stored outline, name, nominal size or body material changes.
//!
//! It does not do: the edge and corner trims (`banding.rs`), the machining records
//! (`machining.rs`), or any geometry derivation.

use furnicraft_model::{Catalog, Contour, Id, MaterialId, PanelNominalSize, Placement, Project, ProjectError};
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
    /// Id of the container that receives the panel.
    pub parent: Id,
    /// User-visible panel name; it must contain a non-whitespace character.
    pub name: String,
    /// Panel nominal length and width.
    pub nominal_size: PanelNominalSize,
    /// Panel-body material from the session catalogue.
    pub material: MaterialId,
    /// Placement relative to the parent container.
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

/// Stable id of the contour-panel creation command.
pub const CONTOUR_CREATE_ID: &str = "panel.contour.create";

/// User-facing label of the contour-panel creation command.
pub const CONTOUR_CREATE_LABEL: &str = "Create contour panel";

/// Parameters for `panel.contour.create`.
///
/// `source` is a constructive object in the scene: the new panel takes that object's contour ring
/// as its outline. The command always creates a panel; an existing panel never changes variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelContourCreate {
    /// Id of the container that receives the panel.
    pub parent: Id,
    /// User-visible panel name; it must contain a non-whitespace character.
    pub name: String,
    /// Constructive object whose contour becomes the panel outline.
    pub source: Id,
    /// Panel-body material from the session catalogue.
    pub material: MaterialId,
    /// Offset composed on top of the source's frame, in the parent frame.
    ///
    /// The identity means "exactly over the source": the panel's local zero, its contour's base
    /// corner, is placed on the source's world corner (ADR 0083).
    pub placement: Placement,
}

impl PanelContourCreate {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CONTOUR_CREATE_ID, message: error.to_string() })
    }

    fn validate_name(&self) -> Result<(), CommandError> {
        if self.name.trim().is_empty() {
            return Err(CommandError::InvalidParams {
                command: CONTOUR_CREATE_ID,
                message: "panel name must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

/// The `panel.contour.create` command.
pub(crate) const CONTOUR_CREATE: CommandSpec =
    CommandSpec::new(CONTOUR_CREATE_ID, CONTOUR_CREATE_LABEL, contour_create_enabled, contour_create_run);

fn contour_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelContourCreate::parse(params) else {
        return false;
    };
    params.validate_name().is_ok()
        && session.catalog().panel_material(params.material).is_some()
        && session.project().validate_panel_contour(params.parent, params.source).is_ok()
}

fn contour_create_run(
    project: &mut Project,
    catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelContourCreate::parse(params)?;
    params.validate_name()?;
    let material =
        catalog.panel_material(params.material).ok_or(CommandError::UnknownMaterial { material: params.material })?;
    let contour = project.object_contour(params.source)?.clone();
    let placement = over_source_placement(project, &params)?;
    let id = project.add_panel_contour(
        params.parent,
        params.name,
        &contour,
        Some(params.material),
        material.nominal_thickness(),
        placement,
    )?;
    Ok(CommandOutcome::created(id))
}

/// The parent-frame placement that puts the new panel exactly over its source.
///
/// A node stores a placement relative to its parent and a panel's local zero is its contour's base
/// corner, so `offset ∘ inverse(parent_world) ∘ source_world` is the pre-rebase frame: the model
/// then appends the base corner in the source frame, and no world point moves (ADR 0083).
fn over_source_placement(project: &Project, params: &PanelContourCreate) -> Result<Placement, CommandError> {
    let invalid = |error: furnicraft_model::PlacementError| CommandError::InvalidParams {
        command: CONTOUR_CREATE_ID,
        message: error.to_string(),
    };
    let parent_world =
        project.world_placement(params.parent).ok_or(ProjectError::ContainerNotFound { id: params.parent })?;
    let source_world =
        project.world_placement(params.source).ok_or(ProjectError::NodeNotFound { id: params.source })?;
    let parent_inverse = parent_world.inverse().map_err(invalid)?;
    let over_source = Placement::compose(parent_inverse, source_world).map_err(invalid)?;
    Placement::compose(params.placement, over_source).map_err(invalid)
}

/// Stable id of the contour-outline replacement command.
pub const OUTLINE_SET_ID: &str = "panel.outline.set";

/// User-facing label of the contour-outline replacement command.
pub const OUTLINE_SET_LABEL: &str = "Set panel outline";

/// Parameters for `panel.outline.set`.
///
/// The ring is in the panel-local plane and may sit anywhere; the command anchors it at the
/// panel-local zero and moves the panel frame with it, so the world-space shape stays fixed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelOutlineSet {
    /// Id of the contour panel whose outline changes.
    pub panel: Id,
    /// The new closed ring.
    pub contour: Contour,
}

impl PanelOutlineSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: OUTLINE_SET_ID, message: error.to_string() })
    }
}

/// The `panel.outline.set` command.
pub(crate) const OUTLINE_SET: CommandSpec =
    CommandSpec::new(OUTLINE_SET_ID, OUTLINE_SET_LABEL, outline_set_enabled, outline_set_run);

fn outline_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelOutlineSet::parse(params) else {
        return false;
    };
    session.project().validate_panel_outline(params.panel, &params.contour).is_ok()
}

fn outline_set_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelOutlineSet::parse(params)?;
    project.set_panel_outline(params.panel, params.contour)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the outline-bake command.
pub const OUTLINE_CONTOUR_ID: &str = "panel.outline.to_contour";

/// User-facing label of the outline-bake command.
pub const OUTLINE_CONTOUR_LABEL: &str = "Bake panel outline into a contour";

/// Parameters for `panel.outline.to_contour`.
///
/// The command bakes a rectangle outline into the ring it draws, in one revision and one undo
/// entry, so *Edit contour* can reach a panel that is not a ring yet. Corner and bow parameters are
/// replaced by their curve segments; undo restores them (ADR 0061).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelOutlineToContour {
    /// Id of the rectangle panel whose outline is baked.
    pub panel: Id,
}

impl PanelOutlineToContour {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: OUTLINE_CONTOUR_ID, message: error.to_string() })
    }
}

/// The `panel.outline.to_contour` command.
pub(crate) const OUTLINE_CONTOUR: CommandSpec =
    CommandSpec::new(OUTLINE_CONTOUR_ID, OUTLINE_CONTOUR_LABEL, outline_contour_enabled, outline_contour_run);

fn outline_contour_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelOutlineToContour::parse(params) else {
        return false;
    };
    session.project().validate_panel_contour_conversion(params.panel).is_ok()
}

fn outline_contour_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelOutlineToContour::parse(params)?;
    project.convert_panel_to_contour(params.panel)?;
    Ok(CommandOutcome::changed())
}

fn create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCreate::parse(params) else {
        return false;
    };
    params.validate_name().is_ok()
        && session.project().is_container(params.parent)
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

/// Stable id of the panel rename command.
pub const RENAME_ID: &str = "panel.rename";

/// User-facing label of the panel rename command.
pub const RENAME_LABEL: &str = "Rename panel";

/// Parameters for `panel.rename`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelRename {
    /// Id of the panel to rename.
    pub panel: Id,
    /// New user-visible panel name; it must contain a non-whitespace character.
    pub name: String,
}

impl PanelRename {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: RENAME_ID, message: error.to_string() })
    }

    fn validate_name(&self) -> Result<(), CommandError> {
        if self.name.trim().is_empty() {
            return Err(CommandError::InvalidParams {
                command: RENAME_ID,
                message: "panel name must not be empty".to_owned(),
            });
        }
        Ok(())
    }
}

/// The `panel.rename` command.
pub(crate) const RENAME: CommandSpec = CommandSpec::new(RENAME_ID, RENAME_LABEL, rename_enabled, rename_run);

fn rename_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelRename::parse(params) else {
        return false;
    };
    params.validate_name().is_ok() && session.project().panel(params.panel).is_some()
}

fn rename_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelRename::parse(params)?;
    params.validate_name()?;
    project.rename_panel(params.panel, params.name)?;
    Ok(CommandOutcome::changed())
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

/// Stable id of the panel material command.
pub const MATERIAL_SET_ID: &str = "panel.material.set";

/// User-facing label of the panel material command.
pub const MATERIAL_SET_LABEL: &str = "Set panel material";

/// Parameters for `panel.material.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelMaterialSet {
    /// Id of the panel whose material changes.
    pub panel: Id,
    /// Panel-body material from the session catalogue.
    pub material: MaterialId,
}

impl PanelMaterialSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: MATERIAL_SET_ID, message: error.to_string() })
    }
}

/// The `panel.material.set` command.
pub(crate) const MATERIAL_SET: CommandSpec =
    CommandSpec::new(MATERIAL_SET_ID, MATERIAL_SET_LABEL, material_set_enabled, material_set_run);

fn material_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelMaterialSet::parse(params) else {
        return false;
    };
    let Some(material) = session.catalog().panel_material(params.material) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.geometry(material.nominal_thickness()).is_ok())
}

fn material_set_run(project: &mut Project, catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelMaterialSet::parse(params)?;
    let material =
        catalog.panel_material(params.material).ok_or(CommandError::UnknownMaterial { material: params.material })?;
    project.set_panel_material(params.panel, params.material, material.nominal_thickness())?;
    Ok(CommandOutcome::changed())
}
