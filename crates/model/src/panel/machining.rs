//! Machining operations on a panel and the core trace that decides whether they are legal.
//!
//! A panel owns one ordered list of [`Machining`] records. Each record has a stable id, an anchor
//! that says which face or edge it acts on, and tagged parameters that say what it does. The
//! document stores the operation, never the removed solid. A through cutout may not empty or split
//! the remaining core; the trace subtracts the exact union of every through cutout from the core
//! region, so a rectangle and a contour ring take the same part in it.

use furnicraft_geom::{DVec2, Rect, Region, RegionError, subtract_regions};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Contour, ContourSegmentId, EdgeId, FaceSide, Id, Length, MachiningId, Panel, PanelGeometry, PanelRect, ProjectError,
};

/// One machining operation owned by a panel.
///
/// The record is the unit the card lists, the diagnostics name and the commands edit. Its id is
/// stable for the life of the panel, its anchor says where it acts, and its parameters say what it
/// does. The list order is the order the operations apply in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machining {
    id: MachiningId,
    anchor: MachiningAnchor,
    params: MachiningParams,
}

impl Machining {
    /// Builds a machining record the caller has already validated.
    pub const fn new(id: MachiningId, anchor: MachiningAnchor, params: MachiningParams) -> Self {
        Self { id, anchor, params }
    }

    /// The stable machining id.
    pub const fn id(&self) -> MachiningId {
        self.id
    }

    /// The face or edge this operation is anchored to.
    pub const fn anchor(&self) -> MachiningAnchor {
        self.anchor
    }

    /// The operation parameters.
    pub const fn params(&self) -> &MachiningParams {
        &self.params
    }

    /// The cutout parameters, when this record is a cutout.
    pub fn cutout(&self) -> Option<&Cutout> {
        match &self.params {
            MachiningParams::Cutout(cutout) => Some(cutout),
        }
    }

    /// The cutout parameters for an edit in place.
    pub(crate) fn cutout_mut(&mut self) -> Option<&mut Cutout> {
        match &mut self.params {
            MachiningParams::Cutout(cutout) => Some(cutout),
        }
    }
}

/// Where a machining operation acts on the panel.
///
/// A face anchor names the large face the operation enters from; an edge anchor names one edge of
/// the outline. Every operation declares which anchors it accepts, so a loaded file cannot pair a
/// cutout with an edge or an edge groove with a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MachiningAnchor {
    /// The operation acts through one large face.
    Face {
        /// The face it enters from.
        side: FaceSide,
    },
    /// The operation follows one edge of the outline.
    Edge {
        /// The outline edge or ring segment it follows.
        edge: EdgeRef,
    },
}

impl MachiningAnchor {
    /// An anchor on a large face.
    pub const fn face(side: FaceSide) -> Self {
        Self::Face { side }
    }

    /// An anchor on an outline edge.
    pub const fn edge(edge: EdgeRef) -> Self {
        Self::Edge { edge }
    }

    /// The face side, when this anchor is a face.
    pub const fn as_face(self) -> Option<FaceSide> {
        match self {
            Self::Face { side } => Some(side),
            Self::Edge { .. } => None,
        }
    }

    /// The edge, when this anchor is an edge.
    pub const fn as_edge(self) -> Option<EdgeRef> {
        match self {
            Self::Face { .. } => None,
            Self::Edge { edge } => Some(edge),
        }
    }
}

/// A reference to one edge of a panel outline.
///
/// A rectangular or shaped outline names a straight edge by its stable [`EdgeId`]; a contour panel
/// names one ring segment by its stable [`ContourSegmentId`]. The reference is a topology reference:
/// it travels with the outline edit and never carries coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EdgeRef {
    /// A straight edge of the nominal rectangle, ids 1..4.
    Rect {
        /// The edge id.
        edge: EdgeId,
    },
    /// One segment of a stored contour outline.
    Ring {
        /// The ring segment id.
        segment: ContourSegmentId,
    },
}

/// The typed parameters of a machining operation.
///
/// The variant is the operation kind; it is tagged in the file, so a newer file with an operation
/// this build does not know is a readable load error rather than a reinterpretation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MachiningParams {
    /// A volume removed from the panel.
    Cutout(Cutout),
}

/// A cutout in a panel.
///
/// Coordinates are nominal panel-local: `(0, 0)` is the minimum corner of the nominal rectangle,
/// with `+X` along the length and `+Y` along the width. The core origin and included edge banding
/// never move the panel frame. The entry face comes from the record's [`MachiningAnchor`]: a blind
/// cutout removes material from that face up to its depth, while a through cutout removes the full
/// thickness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cutout {
    shape: CutoutShape,
    depth: CutoutDepth,
}

impl Cutout {
    /// Creates cutout parameters.
    pub const fn new(shape: CutoutShape, depth: CutoutDepth) -> Self {
        Self { shape, depth }
    }

    /// The cutout outline in nominal panel-local coordinates.
    pub const fn shape(&self) -> &CutoutShape {
        &self.shape
    }

    /// Replaces the shape; the caller has already validated it against the panel.
    pub(crate) fn set_shape(&mut self, shape: CutoutShape) {
        self.shape = shape;
    }

    /// How deep the cutout goes.
    pub const fn depth(&self) -> CutoutDepth {
        self.depth
    }
}

/// The outline of a cutout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoutShape {
    /// A rectangle whose minimum corner is at `rect.x`, `rect.y` from the nominal panel zero.
    Rectangle(PanelRect),
    /// A closed ring of directed lines and arcs in nominal panel-local coordinates.
    ///
    /// A through ring is subtracted from the core exactly like a rectangle, so it may touch or cross
    /// the core boundary and open a notch; the core trace decides whether the remaining material is
    /// still one positive component. A blind ring may touch the edge.
    Contour(Contour),
}

impl CutoutShape {
    /// The exact region this shape removes, in nominal panel-local coordinates.
    ///
    /// The region keeps a ring's lines and arcs, so the core trace, the fit rules and the cutout
    /// overlap rule all read the same geometry without sampling.
    pub fn region(&self) -> Result<Region, ProjectError> {
        match self {
            Self::Rectangle(rect) => {
                let min = DVec2::new(rect.x().millimetres_f64(), rect.y().millimetres_f64());
                let max = DVec2::new(min.x + rect.width().millimetres_f64(), min.y + rect.height().millimetres_f64());
                Ok(Region::from_rect(Rect::new(min, max).map_err(RegionError::from)?)?)
            }
            Self::Contour(contour) => Ok(Region::new(contour.to_geometry()?, Vec::new())?),
        }
    }
}

/// How much material a cutout removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoutDepth {
    /// Removes the full panel thickness.
    Through,
    /// Removes material from the entry face by `depth`.
    Blind {
        /// Depth measured from the entry face.
        depth: Length,
    },
}

/// Why the through cutouts of a panel do not leave a usable core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreViolation {
    /// The through cutouts remove the whole core.
    Removed,
    /// The remaining core is split into disconnected material pieces.
    Split,
}

/// The core minus the union of the through cutouts, traced into material regions.
///
/// The core is the resolved region itself, so a rectangle, a shaped outline and a contour ring take
/// the same path: a through cutout that touches or crosses the boundary opens a notch, and the
/// remaining material must be one positive component. `extra` is a candidate cutout a command is
/// about to add, and `exclude` names the record an update replaces so it is not counted twice.
pub(crate) fn core_violation_of(
    panel: &Panel,
    geometry: &PanelGeometry,
    extra: Option<&Region>,
    exclude: Option<MachiningId>,
) -> Result<Option<CoreViolation>, ProjectError> {
    let mut cuts = Vec::new();
    for machining in panel.machining() {
        if exclude == Some(machining.id()) {
            continue;
        }
        let Some(cutout) = machining.cutout() else {
            continue;
        };
        if matches!(cutout.depth(), CutoutDepth::Through) {
            cuts.push(cutout.shape().region()?);
        }
    }
    if let Some(extra) = extra {
        cuts.push(extra.clone());
    }
    if cuts.is_empty() {
        return Ok(None);
    }
    let remaining = subtract_regions(std::slice::from_ref(geometry.core_region()), &cuts)?;
    if remaining.is_empty() {
        Ok(Some(CoreViolation::Removed))
    } else if remaining.len() > 1 {
        Ok(Some(CoreViolation::Split))
    } else if remaining.first().is_some_and(|region| region.area() <= 0.0) {
        Ok(Some(CoreViolation::Removed))
    } else {
        Ok(None)
    }
}

/// The file form of one machining record.
///
/// The current form is a record with an id, an anchor and tagged parameters. The legacy form is the
/// externally tagged `{"cutout": {"id", "shape", "depth", "entry"}}` record that schemas up to 15
/// wrote; it loads as a face-anchored cutout with the same id, so an old scene keeps every hole in
/// place.
#[derive(Deserialize)]
#[serde(untagged)]
enum MachiningWire {
    Current(CurrentMachining),
    Legacy(LegacyMachining),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentMachining {
    id: MachiningId,
    anchor: MachiningAnchor,
    params: MachiningParams,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyMachining {
    Cutout(LegacyCutout),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyCutout {
    id: Id,
    shape: CutoutShape,
    depth: CutoutDepth,
    entry: FaceSide,
}

impl From<MachiningWire> for Machining {
    fn from(wire: MachiningWire) -> Self {
        match wire {
            MachiningWire::Current(current) => Self { id: current.id, anchor: current.anchor, params: current.params },
            MachiningWire::Legacy(LegacyMachining::Cutout(cutout)) => Self {
                id: MachiningId::from_id(cutout.id),
                anchor: MachiningAnchor::face(cutout.entry),
                params: MachiningParams::Cutout(Cutout::new(cutout.shape, cutout.depth)),
            },
        }
    }
}

impl<'de> Deserialize<'de> for Machining {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        MachiningWire::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    use crate::fixtures::*;

    fn record(id: u64, anchor: MachiningAnchor, params: MachiningParams) -> Machining {
        Machining::new(MachiningId::new(id).expect("machining id"), anchor, params)
    }

    fn cutout_record(id: u64, shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Machining {
        record(id, MachiningAnchor::face(entry), MachiningParams::Cutout(Cutout::new(shape, depth)))
    }

    #[test]
    fn shaped_panel_with_machining_resizes() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: millimetres(8) })
                    .expect("radius"),
            )
            .expect("shape the panel");
        let bigger = PanelNominalSize::new(millimetres(900), millimetres(600)).expect("valid size");
        project.set_panel_nominal_size(panel_id, bigger).expect("resize a shaped panel with machining");
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.nominal_size(), bigger);
        assert!(panel.geometry_with_fallbacks().is_ok());
    }

    #[test]
    fn shaped_panel_with_machining_resolves_geometry() {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Shaped");
        panel.upsert_corner_feature(
            CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: millimetres(8) })
                .expect("radius"),
        );
        panel.push_machining(cutout_record(
            5,
            CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
            CutoutDepth::Through,
            FaceSide::A,
        ));
        let geometry = panel.geometry_with_fallbacks().expect("shaped geometry with machining");
        assert!(geometry.core_length().is_positive());
        assert!(geometry.core_width().is_positive());
    }

    #[test]
    fn a_record_round_trips_with_id_anchor_and_params() {
        let machining = cutout_record(
            5,
            CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
            CutoutDepth::Blind { depth: millimetres(8) },
            FaceSide::B,
        );
        let text = serde_json::to_string(&machining).expect("serialize machining");
        assert!(text.contains("\"kind\":\"cutout\""), "the parameters are tagged: {text}");
        assert!(text.contains("\"kind\":\"face\""), "the anchor is tagged: {text}");
        let decoded: Machining = serde_json::from_str(&text).expect("deserialize machining");
        assert_eq!(decoded, machining);
        assert_eq!(decoded.id(), MachiningId::new(5).expect("id"));
        assert_eq!(decoded.anchor().as_face(), Some(FaceSide::B));
    }

    #[test]
    fn a_legacy_cutout_record_loads_as_a_face_anchored_cutout() {
        let legacy = serde_json::json!({
            "cutout": {
                "id": 5,
                "shape": {"rectangle": {"x": 10_000, "y": 10_000, "width": 100_000, "height": 50_000}},
                "depth": "through",
                "entry": "A"
            }
        });
        let machining: Machining = serde_json::from_value(legacy).expect("legacy cutout");
        assert_eq!(machining.id(), MachiningId::new(5).expect("id"));
        assert_eq!(machining.anchor().as_face(), Some(FaceSide::A));
        let Some(cutout) = machining.cutout() else {
            panic!("a legacy record is a cutout");
        };
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)));
        assert_eq!(cutout.depth(), CutoutDepth::Through);
    }

    fn panel_with_ring(shape: Contour) -> Panel {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Ring cut");
        panel.push_machining(cutout_record(5, CutoutShape::Contour(shape), CutoutDepth::Through, FaceSide::A));
        panel
    }

    #[test]
    fn a_through_ring_inside_the_core_keeps_one_component() {
        let panel = panel_with_ring(ring(100, 100, 100, 60));
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(&geometry), Ok(None));
    }

    #[test]
    fn a_through_ring_across_the_core_splits_it() {
        let panel = panel_with_ring(ring(-10, 200, 720, 60));
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(&geometry), Ok(Some(CoreViolation::Split)));
    }

    #[test]
    fn a_through_ring_that_covers_the_core_removes_it() {
        let panel = panel_with_ring(ring(-10, -10, 720, 520));
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(&geometry), Ok(Some(CoreViolation::Removed)));
    }
}
