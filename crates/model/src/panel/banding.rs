//! Edge, corner and ring banding records and the order they are applied in.
//!
//! A band is stored as data: the material id plus fallback dimensions, never a resolved thickness.
//! The order list is the draw order; the last applied band is drawn on top. Reloading validates that
//! every record still names a segment the panel has.

use std::collections::HashSet;

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};

use crate::panel::geometry::{PanelDimensionError, validate_dimension};
use crate::{Contour, ContourSegmentId, EdgeId, MaterialId, Panel, PanelError, SegmentSource, VertexId};

/// The catalogued and fallback values that describe one band.
///
/// Edge and corner banding share these values but keep separate stable owners. The catalogued
/// material is resolved by the engine; the fallback dimensions keep the panel readable when the
/// material is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandingSpec {
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl BandingSpec {
    /// Creates a spec with validated fallback dimensions.
    pub fn new(
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The banding material id.
    pub const fn material(self) -> MaterialId {
        self.material
    }

    /// Whether this band is included in the panel's nominal size.
    pub const fn included_in_size(self) -> bool {
        self.included_in_size
    }

    /// The fallback band thickness.
    pub const fn last_known_thickness(self) -> Length {
        self.last_known_thickness
    }

    /// The fallback band width.
    pub const fn last_known_width(self) -> Length {
        self.last_known_width
    }
}

/// One edge-banding strip in application order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EdgeBandingWire")]
pub struct EdgeBanding {
    edge: EdgeId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl EdgeBanding {
    /// Creates a banding record with validated fallback dimensions.
    pub fn new(
        edge: EdgeId,
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { edge, material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The edge this strip covers.
    pub const fn edge(&self) -> EdgeId {
        self.edge
    }

    /// The banding material id.
    pub const fn material(&self) -> MaterialId {
        self.material
    }

    /// Whether this strip is included in the panel's nominal size.
    pub const fn included_in_size(&self) -> bool {
        self.included_in_size
    }

    /// The fallback banding thickness.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The fallback banding width.
    pub const fn last_known_width(&self) -> Length {
        self.last_known_width
    }

    /// The shared banding values without the edge owner.
    pub const fn spec(&self) -> BandingSpec {
        BandingSpec {
            material: self.material,
            included_in_size: self.included_in_size,
            last_known_thickness: self.last_known_thickness,
            last_known_width: self.last_known_width,
        }
    }
}

/// One corner-banding strip on a corner segment.
///
/// A chamfer or radius is its own contour segment, so its banding is keyed by the corner vertex.
/// The record is removed together with the segment when the corner becomes a point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CornerBandingWire")]
pub struct CornerBanding {
    corner: VertexId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl CornerBanding {
    /// Creates a corner banding record with validated fallback dimensions.
    pub fn new(
        corner: VertexId,
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { corner, material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The chamfer corner this strip covers.
    pub const fn corner(&self) -> VertexId {
        self.corner
    }

    /// The banding material id.
    pub const fn material(&self) -> MaterialId {
        self.material
    }

    /// Whether this strip is included in the panel's nominal size.
    pub const fn included_in_size(&self) -> bool {
        self.included_in_size
    }

    /// The fallback band thickness.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The fallback band width.
    pub const fn last_known_width(&self) -> Length {
        self.last_known_width
    }

    /// The shared banding values without the corner owner.
    pub const fn spec(&self) -> BandingSpec {
        BandingSpec {
            material: self.material,
            included_in_size: self.included_in_size,
            last_known_thickness: self.last_known_thickness,
            last_known_width: self.last_known_width,
        }
    }
}

/// One banding strip on one segment of a contour ring.
///
/// A contour panel has no rectangle edges: every side is a ring segment with its own stable
/// [`ContourSegmentId`], so banding is keyed by that id. The record is removed with its segment when
/// the outline is edited (ADR 0041).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RingBandingWire")]
pub struct RingBanding {
    segment: ContourSegmentId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl RingBanding {
    /// Creates a ring banding record with validated fallback dimensions.
    pub fn new(
        segment: ContourSegmentId,
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { segment, material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The ring segment this strip covers.
    pub const fn segment(&self) -> ContourSegmentId {
        self.segment
    }

    /// The banding material id.
    pub const fn material(&self) -> MaterialId {
        self.material
    }

    /// Whether this strip is included in the panel's nominal size.
    pub const fn included_in_size(&self) -> bool {
        self.included_in_size
    }

    /// The fallback band thickness.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The fallback band width.
    pub const fn last_known_width(&self) -> Length {
        self.last_known_width
    }

    /// The shared banding values without the segment owner.
    pub const fn spec(&self) -> BandingSpec {
        BandingSpec {
            material: self.material,
            included_in_size: self.included_in_size,
            last_known_thickness: self.last_known_thickness,
            last_known_width: self.last_known_width,
        }
    }
}

/// Serialized form of [`EdgeBanding`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBandingWire {
    edge: EdgeId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl TryFrom<EdgeBandingWire> for EdgeBanding {
    type Error = PanelDimensionError;

    fn try_from(wire: EdgeBandingWire) -> Result<Self, Self::Error> {
        Self::new(wire.edge, wire.material, wire.included_in_size, wire.last_known_thickness, wire.last_known_width)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CornerBandingWire {
    corner: VertexId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl TryFrom<CornerBandingWire> for CornerBanding {
    type Error = PanelDimensionError;

    fn try_from(wire: CornerBandingWire) -> Result<Self, Self::Error> {
        Self::new(wire.corner, wire.material, wire.included_in_size, wire.last_known_thickness, wire.last_known_width)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RingBandingWire {
    segment: ContourSegmentId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl TryFrom<RingBandingWire> for RingBanding {
    type Error = PanelDimensionError;

    fn try_from(wire: RingBandingWire) -> Result<Self, Self::Error> {
        Self::new(wire.segment, wire.material, wire.included_in_size, wire.last_known_thickness, wire.last_known_width)
    }
}

pub(crate) fn validate_ring_banding(contour: &Contour, bands: &[RingBanding]) -> Result<(), PanelError> {
    let mut seen = HashSet::with_capacity(bands.len());
    for band in bands {
        let segment = band.segment();
        if !seen.insert(segment) {
            return Err(PanelError::DuplicateRingBanding { segment });
        }
        if contour.index_of(segment).is_none() {
            return Err(PanelError::UnsupportedRingBanding { segment });
        }
    }
    Ok(())
}

pub(crate) fn normalize_band_order(
    order: Vec<SegmentSource>,
    edges: &[EdgeBanding],
    corners: &[CornerBanding],
    ring: &[RingBanding],
) -> Vec<SegmentSource> {
    let mut normalized = Vec::new();
    for source in order {
        let present = match source {
            SegmentSource::Edge(edge) => edges.iter().any(|band| band.edge() == edge),
            SegmentSource::Corner(corner) => corners.iter().any(|band| band.corner() == corner),
            SegmentSource::Ring(segment) => ring.iter().any(|band| band.segment() == segment),
        };
        if present && !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    for band in edges {
        let source = SegmentSource::Edge(band.edge());
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    for band in corners {
        let source = SegmentSource::Corner(band.corner());
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    for band in ring {
        let source = SegmentSource::Ring(band.segment());
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    normalized
}

pub(crate) fn validate_corner_banding(bands: &[CornerBanding]) -> Result<(), PanelError> {
    let mut corners = HashSet::new();
    for band in bands {
        let corner = band.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(PanelError::UnsupportedCornerBanding { corner });
        }
        if !corners.insert(corner.get()) {
            return Err(PanelError::DuplicateCornerBanding { corner });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::PanelWire;
    use crate::*;

    use crate::fixtures::*;

    #[test]
    fn a_contour_panel_bands_one_ring_segment() {
        let contour = rectangle_contour(700, 500);
        let segment = contour.entries().first().expect("ring segment").id();
        let panel_id = Id::new(2).expect("id");
        let panel = Panel::new_contour(panel_id, "Shelf", contour, None, sample_thickness(), Placement::IDENTITY)
            .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let mut project = Project::new(root).expect("valid project");

        let band =
            RingBanding::new(segment, sample_material(), true, millimetres(2), millimetres(20)).expect("ring band");
        project.set_ring_banding(panel_id, band).expect("set ring banding");

        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.ring_banding().len(), 1);
        assert_eq!(panel.ring_banding_at(segment).map(|band| band.spec().last_known_thickness()), Some(millimetres(2)));
        assert_eq!(panel.band_order(), &[SegmentSource::Ring(segment)]);
        assert_eq!(
            panel.segment_banding(SegmentSource::Ring(segment)),
            panel.ring_banding_at(segment).map(|band| band.spec())
        );

        let geometry = panel.geometry_with_fallbacks().expect("contour geometry");
        assert_eq!(geometry.core_width(), millimetres(498), "the included band narrows the core");
        assert_eq!(geometry.overall_width(), millimetres(500));

        project.clear_ring_banding(panel_id, segment).expect("clear ring banding");
        let panel = project.panel(panel_id).expect("panel");
        assert!(!panel.has_banding());
        assert!(panel.band_order().is_empty());
    }

    #[test]
    fn a_rectangular_panel_refuses_ring_banding() {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let segment = ContourSegmentId::new(1).expect("segment id");
        let band =
            RingBanding::new(segment, sample_material(), false, millimetres(2), millimetres(20)).expect("ring band");
        assert_eq!(panel.apply_ring_banding(band), Err(PanelContourError::UnsupportedRingSegment { segment }));
    }

    #[test]
    fn ring_banding_survives_a_round_trip_and_is_refused_on_a_rectangle() {
        let contour = rectangle_contour(700, 500);
        let segment = contour.entries().first().expect("ring segment").id();
        let panel_id = Id::new(2).expect("id");
        let panel = Panel::new_contour(panel_id, "Shelf", contour, None, sample_thickness(), Placement::IDENTITY)
            .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let mut project = Project::new(root).expect("valid project");
        project
            .set_ring_banding(
                panel_id,
                RingBanding::new(segment, sample_material(), true, millimetres(2), millimetres(20)).expect("band"),
            )
            .expect("set ring banding");

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
        let decoded_panel = decoded.panel(panel_id).expect("panel");
        assert_eq!(decoded_panel.ring_banding_at(segment).map(|band| band.material()), Some(sample_material()));

        let rect = serde_json::to_value(sample_panel(Id::new(2).expect("id"), "Shelf")).expect("serialize panel");
        let mut ring_on_rect = rect.clone();
        ring_on_rect["ring_banding"] = serde_json::json!([{
            "segment": 1,
            "material": 1,
            "included_in_size": false,
            "last_known_thickness": 2_000,
            "last_known_width": 20_000,
        }]);
        let error = serde_json::from_value::<Panel>(ring_on_rect).expect_err("a rectangle has no ring segments");
        assert!(error.to_string().contains("contour ring segment"), "error was {error}");

        let mut duplicate = serde_json::to_value(decoded_panel).expect("serialize panel");
        let one = serde_json::json!({
            "segment": segment.get(),
            "material": 1,
            "included_in_size": false,
            "last_known_thickness": 2_000,
            "last_known_width": 20_000,
        });
        duplicate["ring_banding"] = serde_json::json!([one.clone(), one]);
        let error = serde_json::from_value::<Panel>(duplicate).expect_err("one record per segment");
        assert!(error.to_string().contains("duplicate ring banding"), "error was {error}");

        let mut unknown = serde_json::to_value(decoded_panel).expect("serialize panel");
        unknown["ring_banding"] = serde_json::json!([{
            "segment": 99,
            "material": 1,
            "included_in_size": false,
            "last_known_thickness": 2_000,
            "last_known_width": 20_000,
        }]);
        let error = serde_json::from_value::<Panel>(unknown).expect_err("the segment must exist in the ring");
        assert!(error.to_string().contains("unsupported segment"), "error was {error}");
    }

    #[test]
    fn merging_paths_keeps_the_existing_banding() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(20))
                    .expect("banding"),
            )
            .expect("band the right edge");
        let radius =
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius");
        project.set_corner_feature(panel_id, radius).expect("shape a banded panel");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert!(panel.edge_banding().iter().any(|band| band.edge() == RectEdge::Right.id()));
        panel.geometry_with_fallbacks().expect("rounded banded geometry");
    }

    #[test]
    fn corner_banding_requires_a_segment_and_goes_with_it() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::BottomLeft.id();
        let band = CornerBanding::new(corner, sample_material(), false, millimetres(2), millimetres(22))
            .expect("corner banding");
        assert_eq!(
            project.set_corner_banding(panel_id, band.clone()),
            Err(ProjectError::CornerFeatureNotFound { panel: panel_id, corner })
        );

        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(15) },
                )
                .expect("chamfer"),
            )
            .expect("shape the corner");
        project.set_corner_banding(panel_id, band).expect("band the chamfer segment");
        assert!(project.panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        project.clear_corner_feature(panel_id, corner).expect("remove the segment");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.corner_feature(corner).is_none());
        assert!(panel.corner_banding_at(corner).is_none());
    }

    #[test]
    fn per_segment_banding_insets_only_the_banded_segments() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        for corner in RectCorner::ALL {
            project
                .set_corner_feature(
                    panel_id,
                    CornerFeature::new(corner.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                        .expect("radius"),
                )
                .expect("set radius");
        }
        for edge in RectEdge::ALL {
            project
                .set_edge_banding(
                    panel_id,
                    EdgeBanding::new(edge.id(), sample_material(), true, millimetres(2), millimetres(22))
                        .expect("band"),
                )
                .expect("band the edge");
        }
        for corner in RectCorner::ALL {
            project
                .set_corner_banding(
                    panel_id,
                    CornerBanding::new(corner.id(), sample_material(), true, millimetres(2), millimetres(22))
                        .expect("corner band"),
                )
                .expect("band the corner");
        }
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.band_order().len(), 8);
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("non-included band");
        let panel = project.panel(panel_id).expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(502));
    }

    #[test]
    fn band_order_tracks_application_and_parameter_edits() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let bottom = EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(22))
            .expect("bottom");
        let top = EdgeBanding::new(RectEdge::Top.id(), sample_material(), true, millimetres(2), millimetres(22))
            .expect("top");
        project.set_edge_banding(panel_id, bottom.clone()).expect("band bottom");
        project.set_edge_banding(panel_id, top.clone()).expect("band top");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Edge(RectEdge::Top.id())]
        );

        let edited = EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), false, millimetres(2), millimetres(22))
            .expect("bottom");
        project.set_edge_banding(panel_id, edited.clone()).expect("edit bottom");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Edge(RectEdge::Top.id())],
            "a parameter edit keeps the order"
        );

        project.clear_edge_banding(panel_id, RectEdge::Bottom.id()).expect("clear bottom");
        assert_eq!(project.panel(panel_id).expect("panel").band_order(), &[SegmentSource::Edge(RectEdge::Top.id())]);
        project.set_edge_banding(panel_id, edited).expect("re-add bottom");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Top.id()), SegmentSource::Edge(RectEdge::Bottom.id())]
        );
    }

    #[test]
    fn corner_banding_joins_the_order_and_leaves_with_its_segment() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(22))
                    .expect("bottom"),
            )
            .expect("band bottom");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(15) },
                )
                .expect("chamfer"),
            )
            .expect("shape the corner");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, sample_material(), false, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("band the corner");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Corner(corner)]
        );

        project.clear_corner_feature(panel_id, corner).expect("remove the corner segment");
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.band_order(), &[SegmentSource::Edge(RectEdge::Bottom.id())]);
        assert!(panel.corner_banding_at(corner).is_none());
    }

    #[test]
    fn a_partially_banded_path_is_reported() {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Shaped");
        panel.upsert_corner_feature(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius"),
        );
        panel.edge_banding.push(
            EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                .expect("band"),
        );
        assert!(panel.has_path_banding_conflict().expect("paths"));
        panel.edge_banding.push(
            EdgeBanding::new(RectEdge::Top.id(), sample_material(), false, millimetres(2), millimetres(22))
                .expect("band"),
        );
        assert!(!panel.has_path_banding_conflict().expect("paths"));
    }

    #[test]
    fn corner_banding_load_validation_rejects_inconsistent_records() {
        let base = |corner_features: serde_json::Value, corner_banding: serde_json::Value| {
            serde_json::json!({
                "id": 2,
                "name": "Panel",
                "nominal_size": {"length": 700000, "width": 500000},
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "last_known_thickness": 18000,
                "outline": {"kind": "rect", "corner_features": corner_features},
                "corner_banding": corner_banding,
            })
        };
        let chamfer =
            serde_json::json!([{"corner": 2, "feature": {"kind": "chamfer", "incoming": 10000, "outgoing": 20000}}]);
        let band = serde_json::json!([{
            "corner": 2,
            "material": 1,
            "included_in_size": true,
            "last_known_thickness": 2000,
            "last_known_width": 22000
        }]);
        let panel: Panel = serde_json::from_value(base(chamfer.clone(), band.clone())).expect("valid panel");
        let corner = RectCorner::BottomRight.id();
        assert!(panel.corner_banding_at(corner).is_some());

        let duplicate = serde_json::json!([band[0].clone(), band[0].clone(),]);
        let duplicate: PanelWire = serde_json::from_value(base(chamfer.clone(), duplicate)).expect("panel wire");
        assert_eq!(Panel::try_from(duplicate), Err(PanelError::DuplicateCornerBanding { corner }));

        let radius = serde_json::json!([{"corner": 2, "feature": {"kind": "radius", "radius": 10000}}]);
        let radius: PanelWire = serde_json::from_value(base(radius, band.clone())).expect("panel wire");
        let radius = Panel::try_from(radius).expect("corner banding survives a radius");
        assert!(radius.corner_banding_at(corner).is_some());

        let outside = serde_json::json!({"corner": 5, "material": 1, "included_in_size": true, "last_known_thickness": 2000, "last_known_width": 22000});
        let outside: PanelWire =
            serde_json::from_value(base(serde_json::json!([]), serde_json::json!([outside]))).expect("panel wire");
        assert_eq!(
            Panel::try_from(outside),
            Err(PanelError::UnsupportedCornerBanding { corner: VertexId::new(5).expect("corner") })
        );
    }

    #[test]
    fn panel_banding_cannot_make_the_core_non_positive() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        let too_wide =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(500), millimetres(22))
                .expect("valid banding dimensions");
        // A band wider than half the panel folds the core contour on itself, so the ring no longer
        // closes into usable material. The edit is rejected and nothing is stored.
        assert!(matches!(
            project.set_edge_banding(panel_id, too_wide),
            Err(ProjectError::PanelGeometry(PanelGeometryError::Contour))
        ));
        assert!(project.panel(panel_id).expect("panel").edge_banding().is_empty());
    }

    #[test]
    fn panel_banding_serde_round_trip_and_validation() {
        let band = EdgeBanding::new(
            EdgeId::new(1).expect("edge id"),
            MaterialId::new(99).expect("material id"),
            true,
            millimetres(2),
            millimetres(22),
        )
        .expect("valid banding");
        let text = serde_json::to_string(&band).expect("serialize banding");
        assert_eq!(serde_json::from_str::<EdgeBanding>(&text).expect("deserialize banding"), band);
        assert!(
            serde_json::from_str::<EdgeBanding>(
                r#"{"edge":1,"material":99,"included_in_size":true,"last_known_thickness":0,"last_known_width":22000}"#
            )
            .is_err()
        );
    }
}
