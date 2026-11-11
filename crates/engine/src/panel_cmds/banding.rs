//! The edge and corner trims of a panel outline: banding, corner features and edge bows.
//!
//! Each command resolves its material through the session catalogue once, validates the trimmed
//! record through `Project`, and then writes it; `enabled` runs the same validation without
//! mutating, so a disabled command is exactly one a run would reject.
//!
//! It refuses malformed parameters, a material the catalogue does not hold, a banding size outside
//! the panel's limit, a corner feature or an edge bow on a vertex or edge outside the rectangular
//! outline (vertices and edges `1..=4`), and a panel that is not in the project.
//!
//! Changes when: what a panel stores on one of its outline edges or corners changes.
//!
//! It does not do: derive the trimmed size, the outline ring or a mesh — `furnicraft-param` owns
//! that.

use furnicraft_model::{
    Catalog, ContourSegmentId, CornerBanding, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeBow, EdgeId, Id,
    MaterialId, Project, ProjectError, RingBanding, VertexId,
};
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;
use furnicraft_text::Message;

/// Stable id of the edge-banding command.
pub const EDGE_BANDING_SET_ID: &str = "panel.edge_banding.set";

/// User-facing label of the edge-banding command.
pub const EDGE_BANDING_SET_LABEL: Message = Message::CmdSetEdgeBanding;

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
pub const EDGE_BANDING_CLEAR_LABEL: Message = Message::CmdClearEdgeBanding;

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

/// Stable id of the corner-banding command.
pub const CORNER_BANDING_SET_ID: &str = "panel.corner_banding.set";

/// User-facing label of the corner-banding command.
pub const CORNER_BANDING_SET_LABEL: Message = Message::CmdSetCornerBanding;

/// Parameters for `panel.corner_banding.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCornerBandingSet {
    /// Id of the panel whose chamfer is banded.
    pub panel: Id,
    /// Chamfer corner to cover.
    pub corner: VertexId,
    /// Edge-banding material from the session catalogue.
    pub material: MaterialId,
    /// Whether the banding thickness is taken out of the nominal size.
    pub included_in_size: bool,
}

impl PanelCornerBandingSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: CORNER_BANDING_SET_ID, message: error.to_string() })
    }

    fn band(&self, catalog: &Catalog) -> Option<CornerBanding> {
        let material = catalog.edge_material(self.material)?;
        CornerBanding::new(self.corner, self.material, self.included_in_size, material.thickness(), material.width())
            .ok()
    }
}

/// The `panel.corner_banding.set` command.
pub(crate) const CORNER_BANDING_SET: CommandSpec = CommandSpec::new(
    CORNER_BANDING_SET_ID,
    CORNER_BANDING_SET_LABEL,
    corner_banding_set_enabled,
    corner_banding_set_run,
);

fn corner_banding_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCornerBandingSet::parse(params) else {
        return false;
    };
    let Some(band) = params.band(session.catalog()) else {
        return false;
    };
    session.project().validate_corner_banding(params.panel, band).is_ok()
}

fn corner_banding_set_run(
    project: &mut Project,
    catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelCornerBandingSet::parse(params)?;
    let Some(material) = catalog.edge_material(params.material) else {
        return Err(CommandError::UnknownMaterial { material: params.material });
    };
    let band = CornerBanding::new(
        params.corner,
        params.material,
        params.included_in_size,
        material.thickness(),
        material.width(),
    )
    .map_err(|error| CommandError::Model(ProjectError::PanelDimension(error)))?;
    project.set_corner_banding(params.panel, band)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the corner-banding clear command.
pub const CORNER_BANDING_CLEAR_ID: &str = "panel.corner_banding.clear";

/// User-facing label of the corner-banding clear command.
pub const CORNER_BANDING_CLEAR_LABEL: Message = Message::CmdClearCornerBanding;

/// Parameters for `panel.corner_banding.clear`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCornerBandingClear {
    /// Id of the panel whose corner is cleared.
    pub panel: Id,
    /// Chamfer corner to clear.
    pub corner: VertexId,
}

impl PanelCornerBandingClear {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone()).map_err(|error| CommandError::InvalidParams {
            command: CORNER_BANDING_CLEAR_ID,
            message: error.to_string(),
        })
    }
}

/// The `panel.corner_banding.clear` command.
pub(crate) const CORNER_BANDING_CLEAR: CommandSpec = CommandSpec::new(
    CORNER_BANDING_CLEAR_ID,
    CORNER_BANDING_CLEAR_LABEL,
    corner_banding_clear_enabled,
    corner_banding_clear_run,
);

fn corner_banding_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelCornerBandingClear::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.corner_banding_at(params.corner).is_some())
}

fn corner_banding_clear_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelCornerBandingClear::parse(params)?;
    project.clear_corner_banding(params.panel, params.corner)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the ring-banding command.
pub const RING_BANDING_SET_ID: &str = "panel.ring_banding.set";

/// User-facing label of the ring-banding command.
pub const RING_BANDING_SET_LABEL: Message = Message::CmdSetContourBanding;

/// Parameters for `panel.ring_banding.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelRingBandingSet {
    /// Id of the contour panel whose ring segment is banded.
    pub panel: Id,
    /// Stable id of the ring segment to cover.
    pub segment: ContourSegmentId,
    /// Edge-banding material from the session catalogue.
    pub material: MaterialId,
    /// Whether the banding thickness is taken out of the nominal size.
    pub included_in_size: bool,
}

impl PanelRingBandingSet {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: RING_BANDING_SET_ID, message: error.to_string() })
    }

    fn band(&self, catalog: &Catalog) -> Option<RingBanding> {
        let material = catalog.edge_material(self.material)?;
        RingBanding::new(self.segment, self.material, self.included_in_size, material.thickness(), material.width())
            .ok()
    }
}

/// The `panel.ring_banding.set` command.
pub(crate) const RING_BANDING_SET: CommandSpec =
    CommandSpec::new(RING_BANDING_SET_ID, RING_BANDING_SET_LABEL, ring_banding_set_enabled, ring_banding_set_run);

fn ring_banding_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelRingBandingSet::parse(params) else {
        return false;
    };
    let Some(band) = params.band(session.catalog()) else {
        return false;
    };
    session.project().validate_ring_banding(params.panel, band).is_ok()
}

fn ring_banding_set_run(
    project: &mut Project,
    catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelRingBandingSet::parse(params)?;
    let Some(material) = catalog.edge_material(params.material) else {
        return Err(CommandError::UnknownMaterial { material: params.material });
    };
    let band = RingBanding::new(
        params.segment,
        params.material,
        params.included_in_size,
        material.thickness(),
        material.width(),
    )
    .map_err(|error| CommandError::Model(ProjectError::PanelDimension(error)))?;
    project.set_ring_banding(params.panel, band)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the ring-banding clear command.
pub const RING_BANDING_CLEAR_ID: &str = "panel.ring_banding.clear";

/// User-facing label of the ring-banding clear command.
pub const RING_BANDING_CLEAR_LABEL: Message = Message::CmdClearContourBanding;

/// Parameters for `panel.ring_banding.clear`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelRingBandingClear {
    /// Id of the contour panel whose ring segment is cleared.
    pub panel: Id,
    /// Stable id of the ring segment to clear.
    pub segment: ContourSegmentId,
}

impl PanelRingBandingClear {
    fn parse(params: &Value) -> Result<Self, CommandError> {
        serde_json::from_value(params.clone())
            .map_err(|error| CommandError::InvalidParams { command: RING_BANDING_CLEAR_ID, message: error.to_string() })
    }
}

/// The `panel.ring_banding.clear` command.
pub(crate) const RING_BANDING_CLEAR: CommandSpec = CommandSpec::new(
    RING_BANDING_CLEAR_ID,
    RING_BANDING_CLEAR_LABEL,
    ring_banding_clear_enabled,
    ring_banding_clear_run,
);

fn ring_banding_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = PanelRingBandingClear::parse(params) else {
        return false;
    };
    session.project().panel(params.panel).is_some_and(|panel| panel.ring_banding_at(params.segment).is_some())
}

fn ring_banding_clear_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = PanelRingBandingClear::parse(params)?;
    project.clear_ring_banding(params.panel, params.segment)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the corner-feature command.
pub const CORNER_SET_ID: &str = "panel.corner.set";

/// User-facing label of the corner-feature command.
pub const CORNER_SET_LABEL: Message = Message::CmdSetCornerFeature;

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
pub const CORNER_CLEAR_LABEL: Message = Message::CmdClearCornerFeature;

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
pub const BOW_SET_LABEL: Message = Message::CmdSetEdgeBow;

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
pub const BOW_CLEAR_LABEL: Message = Message::CmdClearEdgeBow;

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
