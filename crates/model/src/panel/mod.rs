//! The flat panel: its outline, material, banding, machining and typed properties.
//!
//! The panel stores the outline and the fallbacks, not the resolved core; [`Panel::geometry`]
//! derives that. Its local frame is anchored at the outline's minimum corner. Commands in
//! `furnicraft-engine` reach the panel through the crate API and mutate it through the
//! crate-internal setters here.

use furnicraft_geom::{DVec3, Region};
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod banding;
mod contour;
mod geometry;
mod machining;

pub use banding::{BandingSpec, CornerBanding, EdgeBanding, RingBanding};
pub use contour::{BakedOutline, ContourPath, PanelContourError, SegmentSource};
pub use geometry::{
    EntryFace, FaceSide, PanelDimensionError, PanelGeometry, PanelGeometryError, PanelNominalSize, PanelRect,
    PanelRectError, RectCorner, RectEdge,
};
pub use machining::{
    CoreViolation, Cutout, CutoutDepth, CutoutShape, EdgeRef, MACHINING_DESCRIPTORS, Machining, MachiningAnchor,
    MachiningCapabilities, MachiningCategory, MachiningDescriptor, MachiningKind, MachiningParams, MachiningStage,
};

pub(crate) use banding::validate_ring_banding;
pub(crate) use geometry::rects_overlap;
pub(crate) use machining::core_violation_of;

use crate::panel::banding::{normalize_band_order, validate_corner_banding};
use crate::panel::geometry::validate_dimension;
use crate::panel_features::{CornerFeature, EdgeBow, validate_contour_features};
use crate::{
    Contour, ContourError, ContourSegmentError, ContourSegmentId, EdgeId, Id, MachiningId, MaterialId, PivotLocal,
    Placement, ProjectError, Properties, PropertyError, PropertyValue, VertexId,
};

/// Where a panel's outline comes from: one source for the whole panel.
///
/// A panel is created either as the nominal rectangle or from a contour selected in the scene; the
/// variant never changes afterwards. The rectangle variant keeps its corner features and bows as
/// the parameters of a contour generator, while the contour variant keeps the finished ring. Both
/// produce one `geom::Region`, so no later stage branches on which variant a panel was created with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PanelOutline {
    /// The nominal rectangle, generated from its corner features and bowed edges.
    Rect {
        /// Corner radii and chamfers, at most one per nominal corner.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        corner_features: Vec<CornerFeature>,
        /// Signed bows, at most one per nominal edge.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edge_bows: Vec<EdgeBow>,
    },
    /// A closed contour ring anchored at the panel-local zero corner.
    Contour {
        /// The ring of directed lines and arcs, counter-clockwise.
        contour: Contour,
    },
}

impl Default for PanelOutline {
    fn default() -> Self {
        Self::Rect { corner_features: Vec::new(), edge_bows: Vec::new() }
    }
}

impl PanelOutline {
    /// The corner treatments of a generated rectangle outline, empty for a stored ring.
    pub fn corner_features(&self) -> &[CornerFeature] {
        match self {
            Self::Rect { corner_features, .. } => corner_features,
            Self::Contour { .. } => &[],
        }
    }

    /// The bowed edges of a generated rectangle outline, empty for a stored ring.
    pub fn edge_bows(&self) -> &[EdgeBow] {
        match self {
            Self::Rect { edge_bows, .. } => edge_bows,
            Self::Contour { .. } => &[],
        }
    }

    /// Whether the rectangle generator carries any corner treatment or bow.
    pub fn has_contour_features(&self) -> bool {
        !self.corner_features().is_empty() || !self.edge_bows().is_empty()
    }
}

/// A flat panel in the product tree.
///
/// The document stores the outline, the selected material and fallbacks, not the resolved core. The
/// core origin, core size, overall size and effective thickness are derived by [`Panel::geometry`]
/// from the panel, its edge banding and the catalogue value supplied by the caller. The panel-local
/// frame stays anchored at the outline's minimum corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PanelWire")]
pub struct Panel {
    id: Id,
    name: String,
    nominal_size: PanelNominalSize,
    outline: PanelOutline,
    material: Option<MaterialId>,
    last_known_thickness: Length,
    placement: Placement,
    pivot_local: PivotLocal,
    machining: Vec<Machining>,
    edge_banding: Vec<EdgeBanding>,
    corner_banding: Vec<CornerBanding>,
    ring_banding: Vec<RingBanding>,
    /// Banded segments in application order; the last one is drawn on top.
    band_order: Vec<SegmentSource>,
    properties: Properties,
}

impl Panel {
    /// Largest accepted nominal panel length or width: ten metres.
    pub const MAX_LENGTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted nominal panel width: ten metres.
    pub const MAX_WIDTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted panel thickness: one metre.
    pub const MAX_THICKNESS: Length = Length::from_micrometres(1_000_000);

    /// Creates a panel with a validated fallback thickness.
    ///
    /// Global id uniqueness and the catalogue lookup are checked later. A material id may be
    /// missing from the catalogue; the fallback thickness keeps the panel drawable.
    pub fn new(
        id: Id,
        name: impl Into<String>,
        nominal_size: PanelNominalSize,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", last_known_thickness, Self::MAX_THICKNESS)?;
        Ok(Self {
            id,
            name: name.into(),
            nominal_size,
            outline: PanelOutline::default(),
            material,
            last_known_thickness,
            placement,
            pivot_local: PivotLocal::ZERO,
            machining: Vec::new(),
            edge_banding: Vec::new(),
            corner_banding: Vec::new(),
            ring_banding: Vec::new(),
            band_order: Vec::new(),
            properties: Properties::new(),
        })
    }

    /// Creates a panel whose outline is a contour ring.
    ///
    /// The ring must be anchored at the panel-local zero corner: its bounds start at `0, 0`. The
    /// nominal size is taken from those bounds, so the caller does not pass a separate size. A ring
    /// that is not anchored, or is larger than a panel may be, is an error the user can read.
    pub fn new_contour(
        id: Id,
        name: impl Into<String>,
        contour: Contour,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Self, PanelError> {
        validate_dimension("thickness", last_known_thickness, Self::MAX_THICKNESS)?;
        let nominal_size = contour_nominal_size(&contour)?;
        Ok(Self {
            id,
            name: name.into(),
            nominal_size,
            outline: PanelOutline::Contour { contour },
            material,
            last_known_thickness,
            placement,
            pivot_local: PivotLocal::ZERO,
            machining: Vec::new(),
            edge_banding: Vec::new(),
            corner_banding: Vec::new(),
            ring_banding: Vec::new(),
            band_order: Vec::new(),
            properties: Properties::new(),
        })
    }

    /// Where the outline comes from: the nominal rectangle or a contour ring.
    pub const fn outline(&self) -> &PanelOutline {
        &self.outline
    }

    /// Whether the outline is a contour ring rather than the nominal rectangle.
    pub const fn is_contour(&self) -> bool {
        matches!(self.outline, PanelOutline::Contour { .. })
    }

    /// The contour ring, when the panel was created from one.
    pub const fn outline_contour(&self) -> Option<&Contour> {
        match &self.outline {
            PanelOutline::Contour { contour } => Some(contour),
            PanelOutline::Rect { .. } => None,
        }
    }

    /// The same outline moved so its minimum corner is the panel-local zero.
    ///
    /// A contour selected in the scene sits somewhere in that object's frame; the panel frame is
    /// anchored at the outline's minimum corner (ADR 0009), so the ring is translated there before
    /// it becomes the outline. Segment ids travel with their curves, so a band named by a segment
    /// keeps its meaning.
    pub fn anchored_contour(contour: &Contour) -> Result<Contour, ContourError> {
        let (min, _) = contour.bounds_micrometres();
        let offset = [
            Length::from_micrometres(min[0].checked_neg().ok_or(ContourSegmentError::NonFinite)?),
            Length::from_micrometres(min[1].checked_neg().ok_or(ContourSegmentError::NonFinite)?),
        ];
        contour.translated(offset)
    }

    /// Checks that a ring can become a panel outline and returns the size it occupies.
    ///
    /// A ring must be anchored at the panel-local zero and span a usable panel size; this is the
    /// same check [`Panel::new_contour`] applies, exposed so a command can validate before it
    /// allocates an id.
    pub fn contour_size(contour: &Contour) -> Result<PanelNominalSize, PanelError> {
        contour_nominal_size(contour)
    }

    /// Banded segments in application order; the last one is drawn on top.
    pub fn band_order(&self) -> &[SegmentSource] {
        &self.band_order
    }

    pub(crate) fn add_band_source(&mut self, source: SegmentSource) {
        if !self.band_order.contains(&source) {
            self.band_order.push(source);
        }
    }

    pub(crate) fn remove_band_source(&mut self, source: SegmentSource) {
        self.band_order.retain(|existing| *existing != source);
    }

    /// The stable panel id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The nominal panel length and width.
    ///
    /// For a contour panel this is the size of the ring's bounds, which start at the panel zero.
    pub const fn nominal_size(&self) -> PanelNominalSize {
        self.nominal_size
    }

    /// The selected panel material, if any.
    pub const fn material(&self) -> Option<MaterialId> {
        self.material
    }

    /// The fallback thickness used when the material is absent.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The placement relative to the parent frame, anchored at the nominal minimum corner.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The manipulation pivot in the panel-local frame.
    pub const fn pivot_local(&self) -> PivotLocal {
        self.pivot_local
    }

    /// The typed properties of this panel.
    pub const fn properties(&self) -> &Properties {
        &self.properties
    }

    /// Stores one validated property.
    pub fn set_property(&mut self, key: &str, value: PropertyValue) -> Result<(), PropertyError> {
        self.properties.insert(key, value).map(|_| ())
    }

    /// Removes one property; returns whether it was present.
    pub fn clear_property(&mut self, key: &str) -> bool {
        self.properties.remove(key).is_some()
    }

    /// Replaces the user-visible name; the caller has already rejected a blank one.
    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Replaces the nominal size; the caller has already validated the machining against it.
    pub(crate) fn set_nominal_size(&mut self, nominal_size: PanelNominalSize) {
        self.nominal_size = nominal_size;
    }

    /// Replaces the outline; the size that goes with it is set separately.
    pub(crate) fn set_outline(&mut self, outline: PanelOutline) {
        self.outline = outline;
    }

    /// Replaces every edge and corner band with the ring records it moves to.
    ///
    /// This is the banding half of a rectangle-to-ring conversion (ADR 0061); the new records name
    /// segments of the ring that is stored in the same change, so the panel never holds a record
    /// without its segment.
    pub(crate) fn set_contour_banding(&mut self, banding: Vec<RingBanding>, order: Vec<SegmentSource>) {
        self.edge_banding.clear();
        self.corner_banding.clear();
        self.ring_banding = banding;
        self.band_order = order;
    }

    /// Replaces the selected material.
    pub(crate) fn set_material(&mut self, material: Option<MaterialId>) {
        self.material = material;
    }

    /// Replaces the fallback thickness used when the catalogue has no entry.
    pub(crate) fn set_thickness(&mut self, thickness: Length) {
        self.last_known_thickness = thickness;
    }

    /// Replaces the placement relative to the parent container.
    pub(crate) fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    /// Replaces the stable id when a subtree is copied.
    pub(crate) fn set_node_id(&mut self, id: Id) {
        self.id = id;
    }

    /// Appends a machining record; the caller has already validated it.
    pub(crate) fn push_machining(&mut self, machining: Machining) {
        self.machining.push(machining);
    }

    /// Machining owned by this panel, in nominal panel-local coordinates, in application order.
    pub fn machining(&self) -> &[Machining] {
        &self.machining
    }

    /// The position and record of one machining id.
    pub(crate) fn machining_at(&self, id: MachiningId) -> Option<(usize, &Machining)> {
        self.machining.iter().enumerate().find(|(_index, machining)| machining.id() == id)
    }

    /// The machining record with this id.
    pub fn machining_by_id(&self, id: MachiningId) -> Option<&Machining> {
        self.machining_at(id).map(|(_index, machining)| machining)
    }

    /// Replaces one record in place; the caller has already validated it.
    pub(crate) fn replace_machining_at(&mut self, index: usize, machining: Machining) -> Option<Machining> {
        let slot = self.machining.get_mut(index)?;
        Some(std::mem::replace(slot, machining))
    }

    /// Removes one record and closes the gap.
    pub(crate) fn remove_machining_at(&mut self, index: usize) -> Option<Machining> {
        if index >= self.machining.len() {
            return None;
        }
        Some(self.machining.remove(index))
    }

    /// Moves one record to a final index; the caller has already checked the index.
    pub(crate) fn move_machining_to(&mut self, from: usize, to: usize) -> bool {
        if from >= self.machining.len() || to >= self.machining.len() {
            return false;
        }
        if from == to {
            return true;
        }
        let machining = self.machining.remove(from);
        self.machining.insert(to, machining);
        true
    }

    /// Checks whether the through cutouts leave one connected core with positive area.
    ///
    /// `Some` means the cutouts would empty the core or split it into disconnected pieces, so a
    /// command must be rejected and a loaded panel must be reported as a problem panel. Blind
    /// cutouts are not part of the 2D union. The core is the resolved region, so a shaped outline
    /// and a stored contour ring are traced exactly like a rectangle. An error means the exact trace
    /// refused the input, for example when the cutouts hold more segments than one boolean may
    /// examine.
    pub fn core_violation(&self, geometry: &PanelGeometry) -> Result<Option<CoreViolation>, ProjectError> {
        core_violation_of(self, geometry, None, None)
    }

    /// Edge banding in application order.
    pub fn edge_banding(&self) -> &[EdgeBanding] {
        &self.edge_banding
    }

    /// Corner banding in application order.
    pub fn corner_banding(&self) -> &[CornerBanding] {
        &self.corner_banding
    }

    /// The corner banding record on a chamfer corner, if present.
    pub fn corner_banding_at(&self, corner: VertexId) -> Option<CornerBanding> {
        self.corner_banding.iter().find(|band| band.corner() == corner).cloned()
    }

    /// Ring banding records in application order, one per banded ring segment.
    pub fn ring_banding(&self) -> &[RingBanding] {
        &self.ring_banding
    }

    /// The ring banding record on one ring segment, if present.
    pub fn ring_banding_at(&self, segment: ContourSegmentId) -> Option<RingBanding> {
        self.ring_banding.iter().find(|band| band.segment() == segment).cloned()
    }

    /// Whether any edge, corner or ring segment is banded.
    pub fn has_banding(&self) -> bool {
        !self.edge_banding.is_empty() || !self.corner_banding.is_empty() || !self.ring_banding.is_empty()
    }

    /// Corner treatments in application order.
    pub fn corner_features(&self) -> &[CornerFeature] {
        self.outline.corner_features()
    }

    /// Bowed edges in application order.
    pub fn edge_bows(&self) -> &[EdgeBow] {
        self.outline.edge_bows()
    }

    /// Whether the nominal outline is not a plain rectangle.
    pub fn has_contour_features(&self) -> bool {
        self.outline.has_contour_features()
    }

    /// Whether the panel has a shaped outline: a contour ring or a corner feature or a bow.
    pub fn has_shaped_outline(&self) -> bool {
        self.is_contour() || self.has_contour_features()
    }

    /// The corner treatment on a vertex, if present.
    pub fn corner_feature(&self, corner: VertexId) -> Option<CornerFeature> {
        self.corner_features().iter().copied().find(|feature| feature.corner() == corner)
    }

    /// The bow on an edge, if present.
    pub fn edge_bow(&self, edge: EdgeId) -> Option<EdgeBow> {
        self.edge_bows().iter().copied().find(|bow| bow.edge() == edge)
    }

    /// Resolves the geometry with the supplied panel thickness and fallback banding thicknesses.
    pub fn geometry(&self, thickness: Length) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry_for(self.nominal_size, thickness)
    }

    /// Resolves the geometry with a caller-provided banding thickness.
    ///
    /// The engine uses this to pass catalogue-resolved profiles without making the model read the
    /// catalogue. `band_thickness` is called once per banding path; a rectangular panel has one band
    /// per edge, while a tangent path counts once.
    pub fn geometry_with_banding<F>(
        &self,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        self.geometry_for_with(self.nominal_size, thickness, band_thickness)
    }

    /// Resolves the geometry for a different nominal size, used by a resize before mutation.
    ///
    /// Banding thickness comes from the stored fallbacks.
    pub fn geometry_for(
        &self,
        nominal_size: PanelNominalSize,
        thickness: Length,
    ) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry_for_with(nominal_size, thickness, |spec| spec.last_known_thickness())
    }

    /// Resolves geometry for a nominal size with a caller-provided banding thickness.
    ///
    /// The nominal outline is generated from the rectangle parameters or read from the stored ring,
    /// then offset once for included banding (the core) and once for non-included banding (the outer
    /// envelope). Both are kept as exact regions, so every later stage reads one representation.
    pub fn geometry_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        validate_dimension("thickness", thickness, Panel::MAX_THICKNESS).map_err(PanelGeometryError::Dimension)?;
        for band in &self.edge_banding {
            if !(1..=4).contains(&band.edge().get()) {
                return Err(PanelGeometryError::UnsupportedEdge { edge: band.edge() });
            }
        }
        let core_contour =
            self.core_contour_for_with(nominal_size, &band_thickness).map_err(|_| PanelGeometryError::Contour)?;
        let overall_contour =
            self.outer_contour_for_with(nominal_size, &band_thickness).map_err(|_| PanelGeometryError::Contour)?;
        let core = Region::new(core_contour, Vec::new()).map_err(|_| PanelGeometryError::Contour)?;
        let overall = Region::new(overall_contour, Vec::new()).map_err(|_| PanelGeometryError::Contour)?;
        PanelGeometry::from_regions(core, overall, thickness)
    }

    /// Resolves geometry using the stored fallback thickness.
    pub fn geometry_with_fallbacks(&self) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry(self.last_known_thickness)
    }

    /// Moves the panel-local frame so its origin sits at `min`, keeping the world shape fixed.
    ///
    /// A contour edit can move the ring's minimum corner. The frame is anchored there (ADR 0009), so
    /// the ring is translated to zero, the placement moves by the inverse offset and the machining
    /// shifts with it. The pivot is derived from the frame: it is the base corner, always zero, so
    /// it is reset rather than shifted (ADR 0083). The world-space geometry and every machining
    /// point therefore do not move.
    pub(crate) fn move_frame_to(&mut self, min: [i64; 2]) -> Result<(), ProjectError> {
        if min == [0, 0] {
            // The frame did not move; the pivot is symbolic anyway, so make the invariant explicit.
            self.pivot_local = PivotLocal::ZERO;
            return Ok(());
        }
        let x = Length::from_micrometres(min[0]);
        let y = Length::from_micrometres(min[1]);
        let offset = DVec3::new(x.millimetres_f64(), y.millimetres_f64(), 0.0);
        let translation = self.placement.translation() + self.placement.rotation() * offset;
        let placement = Placement::new(translation, self.placement.rotation())
            .map_err(|_| ProjectError::PanelOutlineFrame { panel: self.id })?;
        // Build the shifted machining beside the old one so a refusal leaves the panel untouched.
        let mut machining = self.machining.clone();
        let shift = [
            Length::ZERO.checked_sub(x).ok_or(ProjectError::PanelOutlineFrame { panel: self.id })?,
            Length::ZERO.checked_sub(y).ok_or(ProjectError::PanelOutlineFrame { panel: self.id })?,
        ];
        for entry in &mut machining {
            let Some(cutout) = entry.cutout_mut() else {
                continue;
            };
            let shape = match cutout.shape() {
                CutoutShape::Rectangle(rect) => {
                    let cut_x = rect.x().checked_sub(x).ok_or(ProjectError::PanelOutlineFrame { panel: self.id })?;
                    let cut_y = rect.y().checked_sub(y).ok_or(ProjectError::PanelOutlineFrame { panel: self.id })?;
                    CutoutShape::Rectangle(
                        PanelRect::new(cut_x, cut_y, rect.width(), rect.height())
                            .map_err(|_| ProjectError::PanelOutlineFrame { panel: self.id })?,
                    )
                }
                CutoutShape::Contour(contour) => CutoutShape::Contour(
                    contour.translated(shift).map_err(|_| ProjectError::PanelOutlineFrame { panel: self.id })?,
                ),
            };
            cutout.set_shape(shape);
        }
        self.placement = placement;
        self.pivot_local = PivotLocal::ZERO;
        self.machining = machining;
        Ok(())
    }

    /// Translates version-2 core-local cutouts into the nominal panel frame.
    ///
    /// The legacy core origin is derived from the included bottom and left banding fallbacks. A
    /// panel with a shaped contour has no stored core origin, so its machining is left unchanged.
    pub(crate) fn migrate_machining_from_core(&mut self) -> Result<(), ProjectError> {
        if self.machining.is_empty() || self.has_shaped_outline() {
            return Ok(());
        }
        let mut core_x = Length::ZERO;
        let mut core_y = Length::ZERO;
        for band in &self.edge_banding {
            if !band.included_in_size() {
                continue;
            }
            match band.edge().get() {
                1 => {
                    core_y = core_y
                        .checked_add(band.last_known_thickness())
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                }
                4 => {
                    core_x = core_x
                        .checked_add(band.last_known_thickness())
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                }
                _ => {}
            }
        }
        if core_x == Length::ZERO && core_y == Length::ZERO {
            return Ok(());
        }
        for machining in &mut self.machining {
            let Some(cutout) = machining.cutout_mut() else {
                continue;
            };
            let shape = match cutout.shape() {
                CutoutShape::Rectangle(rect) => {
                    let x = rect
                        .x()
                        .checked_add(core_x)
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                    let y = rect
                        .y()
                        .checked_add(core_y)
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                    CutoutShape::Rectangle(
                        PanelRect::new(x, y, rect.width(), rect.height())
                            .map_err(|_| ProjectError::MachiningCoordinateOverflow { panel: self.id })?,
                    )
                }
                // A version-2 file predates contour cutouts; a ring that somehow carries one keeps
                // its place in the nominal frame.
                CutoutShape::Contour(contour) => CutoutShape::Contour(
                    contour
                        .translated([core_x, core_y])
                        .map_err(|_| ProjectError::MachiningCoordinateOverflow { panel: self.id })?,
                ),
            };
            cutout.set_shape(shape);
        }
        Ok(())
    }

    pub(crate) fn upsert_corner_feature(&mut self, feature: CornerFeature) {
        let PanelOutline::Rect { corner_features, .. } = &mut self.outline else {
            return;
        };
        let corner = feature.corner();
        corner_features.retain(|existing| existing.corner() != corner);
        corner_features.push(feature);
    }

    pub(crate) fn remove_corner_feature(&mut self, corner: VertexId) -> bool {
        let PanelOutline::Rect { corner_features, .. } = &mut self.outline else {
            return false;
        };
        let before = corner_features.len();
        corner_features.retain(|existing| existing.corner() != corner);
        corner_features.len() != before
    }

    pub(crate) fn upsert_edge_bow(&mut self, bow: EdgeBow) {
        let PanelOutline::Rect { edge_bows, .. } = &mut self.outline else {
            return;
        };
        let edge = bow.edge();
        edge_bows.retain(|existing| existing.edge() != edge);
        edge_bows.push(bow);
    }

    pub(crate) fn remove_edge_bow(&mut self, edge: EdgeId) -> bool {
        let PanelOutline::Rect { edge_bows, .. } = &mut self.outline else {
            return false;
        };
        let before = edge_bows.len();
        edge_bows.retain(|existing| existing.edge() != edge);
        edge_bows.len() != before
    }

    pub(crate) fn upsert_corner_banding(&mut self, band: CornerBanding) {
        let corner = band.corner();
        self.corner_banding.retain(|existing| existing.corner() != corner);
        self.corner_banding.push(band);
    }

    pub(crate) fn remove_corner_banding(&mut self, corner: VertexId) -> bool {
        let before = self.corner_banding.len();
        self.corner_banding.retain(|existing| existing.corner() != corner);
        self.corner_banding.len() != before
    }

    pub(crate) fn remove_ring_banding(&mut self, segment: ContourSegmentId) -> bool {
        let before = self.ring_banding.len();
        self.ring_banding.retain(|existing| existing.segment() != segment);
        self.ring_banding.len() != before
    }
}

/// Why a serialized panel cannot be read as the current schema.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PanelError {
    /// A current panel is missing its nominal size.
    #[error("panel has no nominal size")]
    MissingNominalSize,
    /// A current panel is missing its fallback thickness.
    #[error("panel has no last_known_thickness")]
    MissingThickness,
    /// A panel mixes the legacy `size` field with current fields.
    #[error("panel mixes the legacy size field with the current schema")]
    AmbiguousSize,
    /// A panel stores contour features both in its outline and in the legacy top-level fields.
    #[error("panel stores corner features both in its outline and in the legacy top-level fields")]
    AmbiguousOutlineFeatures,
    /// A stored dimension is not usable.
    #[error(transparent)]
    Dimension(#[from] PanelDimensionError),
    /// A corner feature was stored twice for the same corner.
    #[error("panel has duplicate corner feature on corner {corner}")]
    DuplicateCornerFeature {
        /// The duplicated corner.
        corner: VertexId,
    },
    /// An edge bow was stored twice for the same edge.
    #[error("panel has duplicate bow on edge {edge}")]
    DuplicateEdgeBow {
        /// The duplicated edge.
        edge: EdgeId,
    },
    /// A corner feature uses a corner outside the rectangular outline.
    #[error("panel corner feature uses unsupported corner {corner}")]
    UnsupportedCornerFeature {
        /// The rejected corner.
        corner: VertexId,
    },
    /// A bow uses an edge outside the rectangular outline.
    #[error("panel bow uses unsupported edge {edge}")]
    UnsupportedEdgeBow {
        /// The rejected edge.
        edge: EdgeId,
    },
    /// Corner banding was stored twice for the same corner.
    #[error("panel has duplicate corner banding on corner {corner}")]
    DuplicateCornerBanding {
        /// The duplicated corner.
        corner: VertexId,
    },
    /// Corner banding uses a corner outside the rectangular outline.
    #[error("panel corner banding uses unsupported corner {corner}")]
    UnsupportedCornerBanding {
        /// The rejected corner.
        corner: VertexId,
    },
    /// A contour outline does not start at the panel-local zero corner.
    #[error("panel contour starts at {start:?} micrometres, not at the panel zero corner")]
    ContourNotAnchored {
        /// The minimum corner of the ring, in micrometres.
        start: [i64; 2],
    },
    /// A contour outline is not a usable panel size.
    #[error("panel contour measures {length} by {width} micrometres, which is not a usable panel size")]
    ContourSize {
        /// The ring's extent along X, in micrometres.
        length: i64,
        /// The ring's extent along Y, in micrometres.
        width: i64,
    },
    /// A contour panel stores a nominal size that its ring does not have.
    #[error("panel contour bounds are {contour:?}, but the panel stores {stored:?}")]
    ContourSizeMismatch {
        /// The size the panel stores.
        stored: PanelNominalSize,
        /// The size of the ring's bounds.
        contour: PanelNominalSize,
    },
    /// A contour panel stored corner features or bows, which describe a rectangle.
    #[error("a contour panel cannot carry corner features or bows")]
    ContourWithFeatures,
    /// A contour panel stored edge or corner banding, which belongs to a rectangle.
    #[error("a contour panel cannot carry edge or corner banding, only ring banding")]
    ContourWithBanding,
    /// A rectangular panel stored banding against contour ring segments.
    #[error("a rectangular panel cannot carry banding on a contour ring segment")]
    RectWithRingBanding,
    /// Two ring banding records name the same segment.
    #[error("panel has duplicate ring banding on segment {segment}")]
    DuplicateRingBanding {
        /// The duplicated ring segment.
        segment: ContourSegmentId,
    },
    /// A ring banding record names a segment the stored ring does not have.
    #[error("panel ring banding uses unsupported segment {segment}")]
    UnsupportedRingBanding {
        /// The rejected ring segment.
        segment: ContourSegmentId,
    },
    /// A stored machining record pairs an operation with an anchor it cannot use.
    #[error("machining {machining} cannot be anchored to {anchor:?}")]
    MachiningAnchorUnsupported {
        /// The record that cannot be anchored.
        machining: MachiningId,
        /// The rejected anchor.
        anchor: MachiningAnchor,
    },
}

/// Serialized form of [`Panel`]; it accepts the v1 `size` field for migration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelWire {
    id: Id,
    name: String,
    #[serde(default)]
    nominal_size: Option<PanelNominalSize>,
    #[serde(default)]
    outline: PanelOutline,
    #[serde(default)]
    material: Option<MaterialId>,
    #[serde(default)]
    last_known_thickness: Option<Length>,
    placement: Placement,
    #[serde(default, alias = "pivot")]
    pivot_local: PivotLocal,
    #[serde(default)]
    machining: Vec<Machining>,
    #[serde(default)]
    edge_banding: Vec<EdgeBanding>,
    #[serde(default)]
    corner_banding: Vec<CornerBanding>,
    #[serde(default)]
    ring_banding: Vec<RingBanding>,
    #[serde(default)]
    band_order: Vec<SegmentSource>,
    /// Legacy layout: corner features lived on the panel, not in the outline.
    #[serde(default)]
    corner_features: Vec<CornerFeature>,
    /// Legacy layout: bowed edges lived on the panel, not in the outline.
    #[serde(default)]
    edge_bows: Vec<EdgeBow>,
    #[serde(default)]
    properties: Properties,
    #[serde(default)]
    size: Option<LegacyPanelSize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPanelSize {
    length: Length,
    width: Length,
    thickness: Length,
}

impl TryFrom<PanelWire> for Panel {
    type Error = PanelError;

    fn try_from(wire: PanelWire) -> Result<Self, Self::Error> {
        let (nominal_size, material, thickness, edge_banding) = match wire.size {
            Some(size) => {
                if wire.nominal_size.is_some()
                    || wire.material.is_some()
                    || wire.last_known_thickness.is_some()
                    || !wire.edge_banding.is_empty()
                    || !wire.corner_banding.is_empty()
                    || !wire.ring_banding.is_empty()
                    || !wire.band_order.is_empty()
                    || !wire.corner_features.is_empty()
                    || !wire.edge_bows.is_empty()
                    || wire.outline != PanelOutline::default()
                {
                    return Err(PanelError::AmbiguousSize);
                }
                let nominal_size = PanelNominalSize::new(size.length, size.width)?;
                (nominal_size, None, size.thickness, Vec::new())
            }
            None => {
                let nominal_size = wire.nominal_size.ok_or(PanelError::MissingNominalSize)?;
                let thickness = wire.last_known_thickness.ok_or(PanelError::MissingThickness)?;
                (nominal_size, wire.material, thickness, wire.edge_banding)
            }
        };

        let outline = match wire.outline {
            PanelOutline::Rect { mut corner_features, mut edge_bows } => {
                if !wire.corner_features.is_empty() || !wire.edge_bows.is_empty() {
                    if !corner_features.is_empty() || !edge_bows.is_empty() {
                        return Err(PanelError::AmbiguousOutlineFeatures);
                    }
                    corner_features = wire.corner_features;
                    edge_bows = wire.edge_bows;
                }
                PanelOutline::Rect { corner_features, edge_bows }
            }
            PanelOutline::Contour { contour } => {
                if !wire.corner_features.is_empty() || !wire.edge_bows.is_empty() {
                    return Err(PanelError::ContourWithFeatures);
                }
                PanelOutline::Contour { contour }
            }
        };
        validate_contour_features(outline.corner_features(), outline.edge_bows())?;
        validate_corner_banding(&wire.corner_banding)?;
        match &outline {
            PanelOutline::Contour { contour } => {
                if !edge_banding.is_empty() || !wire.corner_banding.is_empty() {
                    return Err(PanelError::ContourWithBanding);
                }
                validate_ring_banding(contour, &wire.ring_banding)?;
                let contour_size = contour_nominal_size(contour)?;
                if contour_size != nominal_size {
                    return Err(PanelError::ContourSizeMismatch { stored: nominal_size, contour: contour_size });
                }
            }
            PanelOutline::Rect { .. } => {
                if !wire.ring_banding.is_empty() {
                    return Err(PanelError::RectWithRingBanding);
                }
            }
        }
        let mut panel = Self::new(wire.id, wire.name, nominal_size, material, thickness, wire.placement)?;
        validate_machining_records(&wire.machining)?;
        panel.outline = outline;
        panel.pivot_local = wire.pivot_local;
        panel.machining = wire.machining;
        panel.edge_banding = edge_banding;
        panel.corner_banding = wire.corner_banding;
        panel.ring_banding = wire.ring_banding;
        panel.band_order =
            normalize_band_order(wire.band_order, &panel.edge_banding, &panel.corner_banding, &panel.ring_banding);
        panel.properties = wire.properties;
        Ok(panel)
    }
}

/// Checks that every stored machining record accepts the anchor it carries.
///
/// The rule comes from the kind's descriptor, so a cutout cannot be stored on an edge and a later
/// edge operation is accepted without touching this function. A file that pairs an operation with a
/// foreign anchor is a readable error, not an operation the evaluator silently ignores.
fn validate_machining_records(machining: &[Machining]) -> Result<(), PanelError> {
    for entry in machining {
        if !entry.params().descriptor().capabilities().accepts_anchor(entry.anchor()) {
            return Err(PanelError::MachiningAnchorUnsupported { machining: entry.id(), anchor: entry.anchor() });
        }
    }
    Ok(())
}

/// The nominal size a contour ring occupies, refusing a ring that is not anchored at zero.
///
/// The ring is the outline of the panel, so its minimum corner is the panel-local zero and its
/// bounds are the nominal size. The constructor and the file reader both go through this check, so
/// a hand-edited file cannot anchor a panel somewhere else.
fn contour_nominal_size(contour: &Contour) -> Result<PanelNominalSize, PanelError> {
    let (min, max) = contour.bounds_micrometres();
    if min != [0, 0] {
        return Err(PanelError::ContourNotAnchored { start: min });
    }
    let length = max[0].checked_sub(min[0]).ok_or(PanelError::ContourSize { length: max[0], width: max[1] })?;
    let width = max[1].checked_sub(min[1]).ok_or(PanelError::ContourSize { length: max[0], width: max[1] })?;
    PanelNominalSize::new(Length::from_micrometres(length), Length::from_micrometres(width))
        .map_err(|_| PanelError::ContourSize { length, width })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    use furnicraft_geom::{DQuat, DVec3};

    use crate::fixtures::*;

    #[test]
    fn a_panel_created_from_a_contour_keeps_its_ring() {
        let contour = rectangle_contour(700, 500);
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Shelf",
            contour.clone(),
            None,
            sample_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");

        assert!(panel.is_contour());
        assert_eq!(panel.outline_contour(), Some(&contour));
        assert_eq!(panel.nominal_size(), sample_nominal(), "the nominal size is the ring's bounds");
        assert!(!panel.has_contour_features(), "a ring is not shaped by corner features");
        assert!(!panel.has_banding());

        let geometry = panel.geometry_with_fallbacks().expect("contour geometry");
        assert_eq!(geometry.core_x(), Length::ZERO);
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let text = serde_json::to_string(&panel).expect("serialize panel");
        assert!(text.contains("\"kind\":\"contour\""), "the ring is written as a contour outline: {text}");
        let decoded: Panel = serde_json::from_str(&text).expect("deserialize panel");
        assert_eq!(decoded, panel);
    }

    #[test]
    fn a_contour_panel_rejects_a_ring_that_does_not_start_at_zero() {
        let moved = Contour::from_polygon(vec![
            [millimetres(10), millimetres(10)],
            [millimetres(110), millimetres(10)],
            [millimetres(110), millimetres(60)],
            [millimetres(10), millimetres(60)],
        ])
        .expect("ring");

        assert_eq!(
            Panel::new_contour(Id::new(2).expect("id"), "Moved", moved, None, sample_thickness(), Placement::IDENTITY),
            Err(PanelError::ContourNotAnchored { start: [10_000, 10_000] })
        );
    }

    #[test]
    fn replacing_a_contour_outline_reanchors_the_frame_and_keeps_the_world_shape() {
        let contour = rectangle_contour(700, 500);
        let panel_id = Id::new(2).expect("id");
        let placement = Placement::new(DVec3::new(100.0, 20.0, 30.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let panel = Panel::new_contour(panel_id, "Shelf", contour.clone(), None, sample_thickness(), placement)
            .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let mut project = Project::new(root).expect("valid project");
        let cutout_rect =
            PanelRect::new(millimetres(10), millimetres(10), millimetres(100), millimetres(50)).expect("rect");
        project
            .add_cutout(panel_id, CutoutShape::Rectangle(cutout_rect), CutoutDepth::Through, FaceSide::A)
            .expect("cutout");

        let before_world = project.world_placement(panel_id).expect("world placement");
        let before_cutout = cutout_world_corners(cutout_rect, before_world);

        // The user edits the ring in the panel's current frame: the ring grows past its old zero, so
        // its minimum corner lands at (-100, -50) and the panel must re-anchor there.
        let edited = ring(-100, -50, 800, 600);
        let input_points = edited.sampled_points(1_000.0).expect("sampled points");
        let input_world: Vec<DVec3> = input_points
            .iter()
            .map(|point| {
                before_world.transform_point(DVec3::new(point[0].millimetres_f64(), point[1].millimetres_f64(), 0.0))
            })
            .collect();
        let min = [-100.0, -50.0];
        let new_base = before_world.transform_point(DVec3::new(min[0], min[1], 0.0));
        project.set_panel_outline(panel_id, edited).expect("replace the outline");

        let panel = project.panel(panel_id).expect("panel");
        let ring = panel.outline_contour().expect("ring");
        assert_eq!(ring.bounds_micrometres().0, [0, 0], "the ring is anchored at the panel zero");
        assert_eq!(panel.nominal_size(), PanelNominalSize::new(millimetres(800), millimetres(600)).expect("new size"));
        assert!(panel.pivot_local().is_zero(), "a panel's pivot is derived and always its base corner");
        let after_world = project.world_placement(panel_id).expect("world placement");
        assert!(
            (after_world.translation() - new_base).length() < 1.0e-9,
            "the translation is the world position of the new base corner: {:?} vs {new_base:?}",
            after_world.translation()
        );
        assert!((after_world.rotation() - before_world.rotation()).length() < 1.0e-9, "the turn is untouched");
        let after_pivot = project.pivot_world(panel_id).expect("world pivot");
        assert!((after_pivot - new_base).length() < 1.0e-9, "the world pivot is the world base corner");
        let old_zero = after_world.transform_point(DVec3::new(100.0, 50.0, 0.0));
        assert!((old_zero - before_world.translation()).length() < 1.0e-9, "the old zero corner keeps its world place");

        // Every point the user drew keeps its world position after the re-anchoring.
        let after_points = ring.sampled_points(1_000.0).expect("sampled points");
        assert_eq!(input_world.len(), after_points.len());
        for expected_world in &input_world {
            let closest = after_points
                .iter()
                .map(|point| {
                    let world = after_world.transform_point(DVec3::new(
                        point[0].millimetres_f64(),
                        point[1].millimetres_f64(),
                        0.0,
                    ));
                    (*expected_world - world).length()
                })
                .fold(f64::INFINITY, f64::min);
            assert!(closest < 1.0e-6, "a world vertex moved by {closest} mm at {expected_world:?}");
        }
        let moved_rect = project.panel(panel_id).expect("panel").machining()[0].cutout().expect("cutout").shape();
        let CutoutShape::Rectangle(moved_rect) = moved_rect else {
            panic!("the cutout stayed a rectangle");
        };
        let after_cutout = cutout_world_corners(*moved_rect, after_world);
        for (before, after) in before_cutout.iter().zip(&after_cutout) {
            assert!((*before - *after).length() < 1.0e-6, "a machining point moved: {before:?} -> {after:?}");
        }
    }

    fn cutout_world_corners(rect: PanelRect, placement: Placement) -> Vec<DVec3> {
        let x = rect.x().millimetres_f64();
        let y = rect.y().millimetres_f64();
        let width = rect.width().millimetres_f64();
        let height = rect.height().millimetres_f64();
        [[x, y], [x + width, y], [x + width, y + height], [x, y + height]]
            .iter()
            .map(|point| placement.transform_point(DVec3::new(point[0], point[1], 0.0)))
            .collect()
    }

    #[test]
    fn a_rectangle_panel_never_takes_a_contour_outline() {
        let mut project = sample_project();
        let id = Id::new(2).expect("id");
        assert_eq!(
            project.set_panel_outline(id, rectangle_contour(100, 100)),
            Err(ProjectError::PanelNotContour { id })
        );
        assert_eq!(project.panel(id).expect("panel").outline(), &PanelOutline::default());
    }

    #[test]
    fn a_bad_outline_edit_leaves_the_panel_and_revision_alone() {
        let contour = rectangle_contour(700, 500);
        let panel_id = Id::new(2).expect("id");
        let panel =
            Panel::new_contour(panel_id, "Shelf", contour.clone(), None, sample_thickness(), Placement::IDENTITY)
                .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let mut project = Project::new(root).expect("valid project");
        let revision = project.revision();
        let too_long = Contour::from_polygon(vec![
            [millimetres(-10_000), millimetres(0)],
            [millimetres(10_000), millimetres(0)],
            [millimetres(10_000), millimetres(500)],
            [millimetres(-10_000), millimetres(500)],
        ])
        .expect("a ring inside the coordinate limit but wider than a panel");
        assert!(project.set_panel_outline(panel_id, too_long).is_err(), "a ring wider than a panel is refused");
        assert_eq!(project.revision(), revision);
        assert_eq!(project.panel(panel_id).expect("panel").outline_contour(), Some(&contour));
    }

    #[test]
    fn an_outline_edit_may_not_drop_a_banded_segment() {
        let contour = rectangle_contour(700, 500);
        let panel_id = Id::new(2).expect("id");
        let panel =
            Panel::new_contour(panel_id, "Shelf", contour.clone(), None, sample_thickness(), Placement::IDENTITY)
                .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let mut project = Project::new(root).expect("valid project");
        let segment = contour.id_at(0).expect("segment");
        let band =
            RingBanding::new(segment, sample_material(), false, millimetres(2), millimetres(20)).expect("ring band");
        project.set_ring_banding(panel_id, band).expect("band the ring");
        let (split, inserted) =
            contour.split_segment(segment, [millimetres(350), millimetres(0)]).expect("split the bottom edge");
        project.set_panel_outline(panel_id, split.clone()).expect("apply the split");
        let band =
            RingBanding::new(inserted, sample_material(), false, millimetres(2), millimetres(20)).expect("ring band");
        project.set_ring_banding(panel_id, band).expect("band the new segment");
        assert_eq!(
            project.set_panel_outline(panel_id, contour),
            Err(ProjectError::Panel(PanelError::UnsupportedRingBanding { segment: inserted }))
        );
        assert_eq!(project.panel(panel_id).expect("panel").outline_contour(), Some(&split));
        assert!(project.panel(panel_id).expect("panel").ring_banding_at(inserted).is_some());
    }

    #[test]
    fn a_contour_panel_refuses_the_rectangle_controls_on_load() {
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Shelf",
            rectangle_contour(700, 500),
            None,
            sample_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");
        let text = serde_json::to_string(&panel).expect("serialize panel");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");

        let mut sized = value.clone();
        sized["nominal_size"]["length"] = serde_json::json!(800_000);
        let error = serde_json::from_value::<Panel>(sized).expect_err("the size must match the ring");
        assert!(error.to_string().contains("contour bounds"), "error was {error}");

        let mut shaped = value.clone();
        shaped["corner_features"] =
            serde_json::json!([{ "corner": 2, "feature": { "kind": "radius", "radius": 25_000 } }]);
        let error = serde_json::from_value::<Panel>(shaped).expect_err("a ring has no corners to shape");
        assert!(error.to_string().contains("corner features or bows"), "error was {error}");

        let mut banded = value;
        banded["edge_banding"] = serde_json::json!([{
            "edge": 1,
            "material": 1,
            "included_in_size": true,
            "last_known_thickness": 2_000,
            "last_known_width": 20_000,
        }]);
        let error = serde_json::from_value::<Panel>(banded).expect_err("rectangle banding is not a ring");
        assert!(error.to_string().contains("contour panel cannot carry edge or corner banding"), "error was {error}");
    }

    #[test]
    fn a_panel_created_from_a_moved_scene_contour_is_anchored() {
        let moved = Contour::from_polygon(vec![
            [millimetres(10), millimetres(10)],
            [millimetres(110), millimetres(10)],
            [millimetres(110), millimetres(60)],
            [millimetres(10), millimetres(60)],
        ])
        .expect("ring");
        let source_id = Id::new(5).expect("id");
        let constructive = Constructive::extrude(moved.clone().into(), millimetres(18)).expect("constructive object");
        let source = Object::new(source_id, "Counter", Placement::IDENTITY, Geometry::Constructive(constructive));
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![source.into()]);
        let mut project = Project::new(root).expect("valid project");

        assert_eq!(project.object_contour(source_id).expect("source contour"), moved);
        project.validate_panel_contour(project.root().id(), source_id).expect("a valid ring");

        let panel_id = project
            .add_panel_contour(project.root().id(), "Top", &moved, None, sample_thickness(), Placement::IDENTITY)
            .expect("contour panel");
        let panel = project.panel(panel_id).expect("panel");
        let ring = panel.outline_contour().expect("ring");
        assert_eq!(ring.bounds_micrometres(), ([0, 0], [100_000, 50_000]));
        assert_eq!(panel.nominal_size(), PanelNominalSize::new(millimetres(100), millimetres(50)).expect("size"));
        assert_eq!(panel.placement().translation(), DVec3::new(10.0, 10.0, 0.0), "the base corner is the local zero");
        assert!(panel.pivot_local().is_zero(), "the local zero is the pivot");
        assert_eq!(
            project.pivot_world(panel_id).expect("world pivot"),
            DVec3::new(10.0, 10.0, 0.0),
            "the world pivot sits on the source contour's base corner"
        );
        assert_eq!(
            ring.entries().iter().map(|entry| entry.id()).collect::<Vec<_>>(),
            moved.entries().iter().map(|entry| entry.id()).collect::<Vec<_>>(),
            "segment ids travel with their curves"
        );
        let panel_world = project.world_placement(panel_id).expect("world placement");
        assert_eq!(
            panel_world.transform_point(DVec3::ZERO),
            DVec3::new(10.0, 10.0, 0.0),
            "the placement places the base corner, so no world point moved"
        );
        let source_world = project.world_placement(source_id).expect("source world");
        let source_points = moved.sampled_points(1_000.0).expect("source points");
        let panel_points = ring.sampled_points(1_000.0).expect("panel points");
        assert_eq!(source_points.len(), panel_points.len());
        for (source, panel) in source_points.iter().zip(&panel_points) {
            let world = |point: [Length; 2], placement: Placement| {
                placement.transform_point(DVec3::new(point[0].millimetres_f64(), point[1].millimetres_f64(), 0.0))
            };
            assert!(
                (world(*source, source_world) - world(*panel, panel_world)).length() < 1e-3,
                "{source:?} moved to {panel:?}"
            );
        }

        let mesh_id = project
            .add_object(project.root().id(), "Mesh", Placement::IDENTITY, Geometry::Mesh(box_mesh()))
            .expect("mesh object");
        assert_eq!(
            project.validate_panel_contour(project.root().id(), mesh_id),
            Err(ProjectError::NotConstructiveObject { id: mesh_id })
        );
        let missing = Id::new(999).expect("id");
        assert_eq!(
            project.validate_panel_contour(project.root().id(), missing),
            Err(ProjectError::NodeNotFound { id: missing })
        );
        assert_eq!(
            project.validate_panel_contour(missing, source_id),
            Err(ProjectError::ContainerNotFound { id: missing })
        );
    }

    #[test]
    fn a_panel_file_without_an_outline_loads_as_a_rectangle() {
        let text = serde_json::to_string(&sample_panel(Id::new(2).expect("id"), "Shelf")).expect("serialize panel");
        let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["outline"], serde_json::json!({ "kind": "rect" }));
        value.as_object_mut().expect("panel object").remove("outline");

        let panel: Panel = serde_json::from_value(value).expect("a panel without an outline is a rectangle");
        assert!(!panel.is_contour());
        assert_eq!(panel.nominal_size(), sample_nominal());
        assert_eq!(panel.outline_contour(), None);
    }

    #[test]
    fn corner_features_and_bows_survive_a_project_round_trip() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner =
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(25) })
                .expect("corner feature");
        project.set_corner_feature(panel_id, corner).expect("set corner");
        project
            .set_edge_bow(panel_id, EdgeBow::new(RectEdge::Top.id(), millimetres(12)).expect("bow"))
            .expect("set bow");

        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert_eq!(panel.corner_feature(RectCorner::TopRight.id()), Some(corner));
        assert_eq!(panel.edge_bow(RectEdge::Top.id()).expect("bow").sagitta(), millimetres(12));

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);

        let geometry = panel.geometry_with_fallbacks().expect("contour geometry");
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(512));
        assert_eq!(geometry.overall_width(), millimetres(512));
    }

    #[test]
    fn shaping_keeps_banding_and_machining() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = CornerFeature::new(
            RectCorner::BottomLeft.id(),
            CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(20) },
        )
        .expect("corner feature");

        project
            .add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
                CutoutDepth::Through,
                FaceSide::A,
            )
            .expect("cutout on the rectangular panel");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(20))
                    .expect("banding"),
            )
            .expect("set banding");
        project.set_corner_feature(panel_id, corner).expect("shape a banded panel with machining");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert_eq!(panel.machining().len(), 1);
        assert!(panel.edge_banding().iter().any(|band| band.edge() == RectEdge::Bottom.id()));
        panel.geometry_with_fallbacks().expect("shaped geometry with machining");

        // Machining no longer depends on the outline kind: a shaped, banded panel takes a new
        // cutout through the same validation as a rectangle.
        let added = project
            .add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(200, 200, 100, 50)),
                CutoutDepth::Through,
                FaceSide::A,
            )
            .expect("cutout on a shaped panel");
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.machining().len(), 2);
        assert!(panel.machining().iter().any(|machining| machining.id() == added));
    }

    #[test]
    fn panels_load_without_machining() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let mut value = serde_json::to_value(&panel).expect("serialize panel");
        value.as_object_mut().expect("panel is an object").remove("machining");
        let decoded: Panel = serde_json::from_value(value).expect("older panel without machining");
        assert_eq!(decoded, panel);
        assert!(decoded.machining().is_empty());
    }

    #[test]
    fn a_cutout_stored_on_an_edge_is_a_readable_error() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let mut value = serde_json::to_value(&panel).expect("serialize panel");
        value["machining"] = serde_json::json!([{
            "id": 5,
            "anchor": {"kind": "edge", "edge": {"kind": "rect", "edge": 1}},
            "params": {
                "kind": "cutout",
                "shape": {"rectangle": {"x": 10_000, "y": 10_000, "width": 100_000, "height": 50_000}},
                "depth": "through"
            }
        }]);
        let error = serde_json::from_value::<Panel>(value).expect_err("a cutout cannot follow an edge");
        assert!(error.to_string().contains("cannot be anchored"), "{error}");
    }
}
