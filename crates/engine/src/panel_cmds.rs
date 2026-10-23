//! Document commands for panels and their cutouts.

use furnicraft_model::{
    Catalog, CornerFeature, CornerFeatureKind, CutoutDepth, CutoutShape, EdgeBanding, EdgeBow, EdgeId, FaceSide, Id,
    MaterialId, PanelNominalSize, Placement, Project, ProjectError, VertexId,
};
use furnicraft_units::Length;
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
    /// Cutout outline in core-local coordinates.
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

/// Stable id of the edge-banding command.
pub const EDGE_BANDING_SET_ID: &str = "panel.edge_banding.set";

/// User-facing label of the edge-banding command.
pub const EDGE_BANDING_SET_LABEL: &str = "Set edge banding";

/// Parameters for `panel.edge_banding.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelEdgeBandingSet {
    /// Id of the panel whose edge is banded.
    pub panel: Id,
    /// Edge to cover.
    pub edge: EdgeId,
    /// Edge-banding material from the session catalogue.
    pub material: MaterialId,
    /// Whether the banding thickness is taken out of the nominal size.
    pub included_in_size: bool,
}

impl PanelEdgeBandingSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: EDGE_BANDING_SET_ID, message: error.to_string() })
    }

    fn band(&self, catalog: &Catalog) -> Option<EdgeBanding> {
        let material = catalog.edge_material(self.material)?;
        EdgeBanding::new(self.edge, self.material, self.included_in_size, material.thickness(), material.width()).ok()
    }
}

/// The `panel.edge_banding.set` command.
pub(crate) const EDGE_BANDING_SET: CommandSpec =
    CommandSpec::new(EDGE_BANDING_SET_ID, EDGE_BANDING_SET_LABEL, edge_banding_set_enabled, edge_banding_set_run);

fn edge_banding_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelEdgeBandingSet::parse(params) else {
        return false;
    };
    let Some(band) = params.band(session.catalog()) else {
        return false;
    };
    session.project().validate_edge_banding(params.panel, band).is_ok()
}

fn edge_banding_set_run(
    project: &mut Project,
    catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelEdgeBandingSet::parse(params)?;
    let Some(material) = catalog.edge_material(params.material) else {
        return Err(CommandError::UnknownMaterial { material: params.material });
    };
    let band =
        EdgeBanding::new(params.edge, params.material, params.included_in_size, material.thickness(), material.width())
            .map_err(|error| CommandError::Model(ProjectError::PanelDimension(error)))?;
    project.set_edge_banding(params.panel, band)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the edge-banding clear command.
pub const EDGE_BANDING_CLEAR_ID: &str = "panel.edge_banding.clear";

/// User-facing label of the edge-banding clear command.
pub const EDGE_BANDING_CLEAR_LABEL: &str = "Clear edge banding";

/// Parameters for `panel.edge_banding.clear`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelEdgeBandingClear {
    /// Id of the panel whose edge is cleared.
    pub panel: Id,
    /// Edge to clear.
    pub edge: EdgeId,
}

impl PanelEdgeBandingClear {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: EDGE_BANDING_CLEAR_ID, message: error.to_string() })
    }
}

/// The `panel.edge_banding.clear` command.
pub(crate) const EDGE_BANDING_CLEAR: CommandSpec = CommandSpec::new(
    EDGE_BANDING_CLEAR_ID,
    EDGE_BANDING_CLEAR_LABEL,
    edge_banding_clear_enabled,
    edge_banding_clear_run,
);

fn edge_banding_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelEdgeBandingClear::parse(params) else {
        return false;
    };
    session
        .project()
        .panel(params.panel)
        .is_some_and(|panel| panel.edge_banding().iter().any(|band| band.edge() == params.edge))
}

fn edge_banding_clear_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelEdgeBandingClear::parse(params)?;
    project.clear_edge_banding(params.panel, params.edge)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the corner-feature command.
pub const CORNER_SET_ID: &str = "panel.corner.set";

/// User-facing label of the corner-feature command.
pub const CORNER_SET_LABEL: &str = "Set corner feature";

/// Parameters for `panel.corner.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCornerSet {
    /// Id of the panel whose corner changes.
    pub panel: Id,
    /// Corner vertex to treat.
    pub corner: VertexId,
    /// Radius or chamfer dimensions.
    pub feature: CornerFeatureKind,
}

impl PanelCornerSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CORNER_SET_ID, message: error.to_string() })
    }

    fn feature(&self) -> Result<CornerFeature, CommandError> {
        if !is_rect_corner(self.corner) {
            return Err(CommandError::InvalidParams {
                command: CORNER_SET_ID,
                message: format!("corner {} is outside the rectangular outline", self.corner),
            });
        }
        CornerFeature::new(self.corner, self.feature)
            .map_err(|error| CommandError::Model(ProjectError::PanelDimension(error)))
    }
}

/// The `panel.corner.set` command.
pub(crate) const CORNER_SET: CommandSpec =
    CommandSpec::new(CORNER_SET_ID, CORNER_SET_LABEL, corner_set_enabled, corner_set_run);

fn corner_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCornerSet::parse(params) else {
        return false;
    };
    let Ok(feature) = params.feature() else {
        return false;
    };
    session.project().validate_corner_feature(params.panel, feature).is_ok()
}

fn corner_set_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelCornerSet::parse(params)?;
    project.set_corner_feature(params.panel, params.feature()?)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the corner-feature clear command.
pub const CORNER_CLEAR_ID: &str = "panel.corner.clear";

/// User-facing label of the corner-feature clear command.
pub const CORNER_CLEAR_LABEL: &str = "Clear corner feature";

/// Parameters for `panel.corner.clear`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCornerClear {
    /// Id of the panel whose corner changes.
    pub panel: Id,
    /// Corner vertex to clear.
    pub corner: VertexId,
}

impl PanelCornerClear {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CORNER_CLEAR_ID, message: error.to_string() })
    }
}

/// The `panel.corner.clear` command.
pub(crate) const CORNER_CLEAR: CommandSpec =
    CommandSpec::new(CORNER_CLEAR_ID, CORNER_CLEAR_LABEL, corner_clear_enabled, corner_clear_run);

fn corner_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCornerClear::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.corner_feature(params.corner).is_some())
}

fn corner_clear_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelCornerClear::parse(params)?;
    project.clear_corner_feature(params.panel, params.corner)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the edge-bow command.
pub const BOW_SET_ID: &str = "panel.bow.set";

/// User-facing label of the edge-bow command.
pub const BOW_SET_LABEL: &str = "Set edge bow";

/// Parameters for `panel.bow.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBowSet {
    /// Id of the panel whose edge changes.
    pub panel: Id,
    /// Edge to bow.
    pub edge: EdgeId,
    /// Signed sagitta: positive outward, negative inward.
    pub sagitta: Length,
}

impl PanelBowSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: BOW_SET_ID, message: error.to_string() })
    }

    fn bow(&self) -> Result<EdgeBow, CommandError> {
        if !is_rect_edge(self.edge) {
            return Err(CommandError::InvalidParams {
                command: BOW_SET_ID,
                message: format!("edge {} is outside the rectangular outline", self.edge),
            });
        }
        EdgeBow::new(self.edge, self.sagitta).map_err(|error| CommandError::Model(ProjectError::PanelDimension(error)))
    }
}

/// The `panel.bow.set` command.
pub(crate) const BOW_SET: CommandSpec = CommandSpec::new(BOW_SET_ID, BOW_SET_LABEL, bow_set_enabled, bow_set_run);

fn bow_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelBowSet::parse(params) else {
        return false;
    };
    let Ok(bow) = params.bow() else {
        return false;
    };
    session.project().validate_edge_bow(params.panel, bow).is_ok()
}

fn bow_set_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelBowSet::parse(params)?;
    project.set_edge_bow(params.panel, params.bow()?)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the edge-bow clear command.
pub const BOW_CLEAR_ID: &str = "panel.bow.clear";

/// User-facing label of the edge-bow clear command.
pub const BOW_CLEAR_LABEL: &str = "Clear edge bow";

/// Parameters for `panel.bow.clear`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBowClear {
    /// Id of the panel whose edge changes.
    pub panel: Id,
    /// Edge to clear.
    pub edge: EdgeId,
}

impl PanelBowClear {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: BOW_CLEAR_ID, message: error.to_string() })
    }
}

/// The `panel.bow.clear` command.
pub(crate) const BOW_CLEAR: CommandSpec =
    CommandSpec::new(BOW_CLEAR_ID, BOW_CLEAR_LABEL, bow_clear_enabled, bow_clear_run);

fn bow_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelBowClear::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.edge_bow(params.edge).is_some())
}

fn bow_clear_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = PanelBowClear::parse(params)?;
    project.clear_edge_bow(params.panel, params.edge)?;
    Ok(CommandOutcome::changed())
}

fn is_rect_corner(corner: VertexId) -> bool {
    (1..=4).contains(&corner.get())
}

fn is_rect_edge(edge: EdgeId) -> bool {
    (1..=4).contains(&edge.get())
}
