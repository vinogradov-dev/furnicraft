//! The nominal size, core geometry and local labels of a flat panel.
//!
//! A panel stores a nominal size, a thickness, an outline and banding; the core and overall regions,
//! the sizes derived from their bounds and the cutout fit rules are computed here. Lengths are
//! integer micrometres except in the predicates that hand a region to `furnicraft_geom`, which is the
//! only floating-point edge.

use furnicraft_geom::{DVec2, Rect, Region, RegionError};
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EdgeId, Panel, VertexId};

/// A rectangle in nominal panel-local XY coordinates.
///
/// `x` and `y` are the minimum corner relative to the nominal panel zero; `width` and `height`
/// are positive. Cutout validation checks that the rectangle stays within the core outline, which
/// may start at a positive `(core_x, core_y)` when an included band trims the low edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PanelRectWire")]
pub struct PanelRect {
    x: Length,
    y: Length,
    width: Length,
    height: Length,
}

impl PanelRect {
    /// Creates a rectangle with positive width and height.
    pub fn new(x: Length, y: Length, width: Length, height: Length) -> Result<Self, PanelRectError> {
        if !width.is_positive() {
            return Err(PanelRectError::NonPositive { field: "width", value: width });
        }
        if !height.is_positive() {
            return Err(PanelRectError::NonPositive { field: "height", value: height });
        }
        Ok(Self { x, y, width, height })
    }

    /// The minimum X coordinate.
    pub const fn x(self) -> Length {
        self.x
    }

    /// The minimum Y coordinate.
    pub const fn y(self) -> Length {
        self.y
    }

    /// The extent along X.
    pub const fn width(self) -> Length {
        self.width
    }

    /// The extent along Y.
    pub const fn height(self) -> Length {
        self.height
    }
}

/// Why a panel rectangle is not usable.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelRectError {
    /// Width or height is zero or negative.
    #[error("cutout {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
}

/// Serialized form of [`PanelRect`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelRectWire {
    x: Length,
    y: Length,
    width: Length,
    height: Length,
}

impl TryFrom<PanelRectWire> for PanelRect {
    type Error = PanelRectError;

    fn try_from(wire: PanelRectWire) -> Result<Self, Self::Error> {
        Self::new(wire.x, wire.y, wire.width, wire.height)
    }
}

/// One of the two large faces of a flat panel.
///
/// `A` is the face at `z = thickness`; `B` is the face at `z = 0`. `+Z` points from `B` to `A`.
/// The labels are local to the panel: they travel with it through a placement and never mean
/// "world up" or "world front".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaceSide {
    /// The face at `z = thickness` (+Z).
    A,
    /// The face at `z = 0` (-Z).
    B,
}

impl FaceSide {
    /// The other face.
    pub const fn opposite(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// Human-readable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }

    /// Sign of the face normal along the panel-local Z axis.
    pub const fn normal_sign(self) -> i8 {
        match self {
            Self::A => 1,
            Self::B => -1,
        }
    }
}

/// The nominal length and width stored on a panel.
///
/// Thickness is not part of the nominal size: it comes from the material, or from the panel's
/// fallback when the material is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PanelNominalSizeWire")]
pub struct PanelNominalSize {
    length: Length,
    width: Length,
}

impl PanelNominalSize {
    /// Creates a nominal size after checking that both sides are positive and within limits.
    pub fn new(length: Length, width: Length) -> Result<Self, PanelDimensionError> {
        validate_dimension("length", length, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_WIDTH)?;
        Ok(Self { length, width })
    }

    /// The nominal length along local X.
    pub const fn length(self) -> Length {
        self.length
    }

    /// The nominal width along local Y.
    pub const fn width(self) -> Length {
        self.width
    }
}

/// The resolved dimensions of a panel.
///
/// The panel-local frame is anchored at the minimum corner of the **nominal** outline. The core and
/// the overall envelope are kept as exact [`Region`]s in that frame, so a rectangle, a shaped outline
/// and a stored contour ring resolve through the same value and no machining rule has to ask which
/// kind of outline it came from. The cached sizes below are the bounds of those regions: the core
/// occupies `x` from `core_x` to `core_x + core_length` and `y` from `core_y` to
/// `core_y + core_width`, while `overall_length` and `overall_width` are the extents of the finished
/// panel including every band. A rectangle with included left and bottom banding has a positive core
/// origin; a shaped or contour panel builds its core ring directly and may report a negative origin
/// when an outward bow carries the material past the nominal minimum corner.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelGeometry {
    core: Region,
    overall: Region,
    core_x: Length,
    core_y: Length,
    core_length: Length,
    core_width: Length,
    thickness: Length,
    overall_length: Length,
    overall_width: Length,
}

impl PanelGeometry {
    /// Creates resolved geometry from the core and overall regions in the panel-local frame.
    ///
    /// Every size is derived from the region bounds and validated once here, so the accessors stay
    /// infallible. A non-positive core or an overall envelope smaller than the core is a typed error;
    /// a negative core origin is kept, because a bow may carry material past the nominal corner.
    pub fn from_regions(core: Region, overall: Region, thickness: Length) -> Result<Self, PanelGeometryError> {
        validate_dimension("thickness", thickness, Panel::MAX_THICKNESS).map_err(PanelGeometryError::Dimension)?;
        let (core_min, core_max) = core.bounds().ok_or(PanelGeometryError::Contour)?;
        let (overall_min, overall_max) = overall.bounds().ok_or(PanelGeometryError::Contour)?;
        let core_x = signed_length_from_millimetres(core_min.x)?;
        let core_y = signed_length_from_millimetres(core_min.y)?;
        let core_length = length_from_millimetres(core_max.x - core_min.x)?;
        let core_width = length_from_millimetres(core_max.y - core_min.y)?;
        require_positive("core_length", core_length)?;
        require_positive("core_width", core_width)?;
        let overall_length = length_from_millimetres(overall_max.x - overall_min.x)?;
        let overall_width = length_from_millimetres(overall_max.y - overall_min.y)?;
        if overall_length < core_length {
            return Err(PanelGeometryError::OverallTooSmall {
                field: "overall_length",
                value: overall_length,
                core: core_length,
            });
        }
        if overall_width < core_width {
            return Err(PanelGeometryError::OverallTooSmall {
                field: "overall_width",
                value: overall_width,
                core: core_width,
            });
        }
        Ok(Self { core, overall, core_x, core_y, core_length, core_width, thickness, overall_length, overall_width })
    }

    /// Creates resolved geometry from rectangular core and overall sizes.
    ///
    /// This is the rectangle constructor kept for callers that already know the two boxes; the
    /// document itself goes through [`PanelGeometry::from_regions`] so a shaped or contour panel
    /// resolves the same way.
    pub fn new(
        core_x: Length,
        core_y: Length,
        core_length: Length,
        core_width: Length,
        thickness: Length,
        overall_length: Length,
        overall_width: Length,
    ) -> Result<Self, PanelGeometryError> {
        require_non_negative("core_x", core_x)?;
        require_non_negative("core_y", core_y)?;
        let core_min = DVec2::new(core_x.millimetres_f64(), core_y.millimetres_f64());
        let core_max =
            DVec2::new(core_min.x + core_length.millimetres_f64(), core_min.y + core_width.millimetres_f64());
        let overall_max = DVec2::new(overall_length.millimetres_f64(), overall_width.millimetres_f64());
        let core = Rect::new(core_min, core_max).map_err(|_| PanelGeometryError::Contour)?;
        let core = Region::from_rect(core).map_err(|_| PanelGeometryError::Contour)?;
        let overall = Rect::new(DVec2::ZERO, overall_max).map_err(|_| PanelGeometryError::Contour)?;
        let overall = Region::from_rect(overall).map_err(|_| PanelGeometryError::Contour)?;
        Self::from_regions(core, overall, thickness)
    }

    /// The exact material region of the core, in the panel-local frame.
    pub const fn core_region(&self) -> &Region {
        &self.core
    }

    /// The exact region of the finished panel including every band, in the panel-local frame.
    pub const fn overall_region(&self) -> &Region {
        &self.overall
    }

    /// Whether a cutout region shares positive area with the core.
    ///
    /// A through cutout that does not reach the core would remove nothing and is rejected. The test
    /// is exact for a rectangle and a curved ring alike.
    pub fn cutout_reaches_core(&self, cutout: &Region) -> Result<bool, RegionError> {
        let overlap =
            furnicraft_geom::intersect_regions(std::slice::from_ref(&self.core), std::slice::from_ref(cutout))?;
        Ok(overlap.iter().any(|region| region.area() > 0.0))
    }

    /// Whether a cutout region lies inside the core, touching its edge allowed.
    ///
    /// This is the rule for a blind cutout: it removes no through material, so it may open at the
    /// edge like a blind rectangle.
    pub fn cutout_fits_core(&self, cutout: &Region) -> Result<bool, RegionError> {
        let outside =
            furnicraft_geom::subtract_regions(std::slice::from_ref(cutout), std::slice::from_ref(&self.core))?;
        Ok(outside.iter().all(|region| region.area() <= 0.0))
    }

    /// The core origin along local X, relative to the nominal minimum corner.
    pub const fn core_x(&self) -> Length {
        self.core_x
    }

    /// The core origin along local Y, relative to the nominal minimum corner.
    pub const fn core_y(&self) -> Length {
        self.core_y
    }

    /// The core length along local X.
    pub const fn core_length(&self) -> Length {
        self.core_length
    }

    /// The core width along local Y.
    pub const fn core_width(&self) -> Length {
        self.core_width
    }

    /// The effective panel thickness along local Z.
    pub const fn thickness(&self) -> Length {
        self.thickness
    }

    /// The overall length including edge banding.
    pub const fn overall_length(&self) -> Length {
        self.overall_length
    }

    /// The overall width including edge banding.
    pub const fn overall_width(&self) -> Length {
        self.overall_width
    }
}

/// Why a nominal panel size or a banding profile is not usable.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelDimensionError {
    /// A dimension is zero or negative.
    #[error("panel {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// A dimension exceeds the supported maximum.
    #[error("panel {field} must not exceed {max}, got {value}")]
    TooLarge {
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
        /// The largest accepted value for this dimension.
        max: Length,
    },
}

/// Why a panel cannot be resolved into core, overall and thickness.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelGeometryError {
    /// A stored dimension is not usable.
    #[error(transparent)]
    Dimension(#[from] PanelDimensionError),
    /// The resulting core is zero or negative.
    #[error("panel {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// The core origin is negative relative to the nominal minimum corner.
    #[error("panel {field} must not be negative, got {value}")]
    NegativeOrigin {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// An overall dimension is smaller than its core.
    #[error("panel {field} {value} is smaller than the core {core}")]
    OverallTooSmall {
        /// The rejected field.
        field: &'static str,
        /// The rejected overall value.
        value: Length,
        /// The core value.
        core: Length,
    },
    /// A sum or difference overflowed.
    #[error("panel {field} overflows")]
    Overflow {
        /// The overflowing field.
        field: &'static str,
    },
    /// Edge banding refers to an edge outside the rectangular v1 outline.
    #[error("edge banding on unsupported edge {edge}")]
    UnsupportedEdge {
        /// The rejected edge.
        edge: EdgeId,
    },
    /// The outline or its edge banding does not form a valid contour ring.
    #[error("the panel outline or its edge banding does not form a valid contour")]
    Contour,
}

/// A corner of a rectangular panel, numbered counter-clockwise when viewed from face `A`.
///
/// Corner 1 is the nominal minimum corner at `(0, 0)`. The core may start at a positive
/// `(core_x, core_y)` when the bottom or left banding is included in the size. The enum is a
/// convenience for rectangular panels; contour panels use [`VertexId`] directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RectCorner {
    /// `(0, 0)`, the nominal zero.
    BottomLeft = 1,
    /// `(nominal length, 0)`.
    BottomRight = 2,
    /// `(nominal length, nominal width)`.
    TopRight = 3,
    /// `(0, nominal width)`.
    TopLeft = 4,
}

impl RectCorner {
    /// Corners in counter-clockwise order.
    pub const ALL: [Self; 4] = [Self::BottomLeft, Self::BottomRight, Self::TopRight, Self::TopLeft];

    /// The stable id, equal to the human number.
    pub const fn id(self) -> VertexId {
        VertexId::from_known(self as u32)
    }

    /// The human number as a string.
    pub const fn label(self) -> &'static str {
        match self {
            Self::BottomLeft => "1",
            Self::BottomRight => "2",
            Self::TopRight => "3",
            Self::TopLeft => "4",
        }
    }

    /// The next corner counter-clockwise.
    pub const fn next(self) -> Self {
        match self {
            Self::BottomLeft => Self::BottomRight,
            Self::BottomRight => Self::TopRight,
            Self::TopRight => Self::TopLeft,
            Self::TopLeft => Self::BottomLeft,
        }
    }
}

/// A side face (end) of a rectangular panel.
///
/// Edge `i` runs from corner `i` to corner `i + 1`, wrapping at four. Thus:
/// edge 1 is the bottom (`-Y`), edge 2 the right (`+X`), edge 3 the top (`+Y`), edge 4 the left
/// (`-X`), viewed from face `A`. Contour panels use [`EdgeId`] directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RectEdge {
    /// Between corners 1 and 2 (`-Y`).
    Bottom = 1,
    /// Between corners 2 and 3 (`+X`).
    Right = 2,
    /// Between corners 3 and 4 (`+Y`).
    Top = 3,
    /// Between corners 4 and 1 (`-X`).
    Left = 4,
}

impl RectEdge {
    /// Edges in counter-clockwise order, following their start corners.
    pub const ALL: [Self; 4] = [Self::Bottom, Self::Right, Self::Top, Self::Left];

    /// The stable id, equal to the human number.
    pub const fn id(self) -> EdgeId {
        EdgeId::from_known(self as u32)
    }

    /// The human number as a string.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bottom => "1",
            Self::Right => "2",
            Self::Top => "3",
            Self::Left => "4",
        }
    }

    /// The corner at which the edge starts.
    pub const fn start(self) -> RectCorner {
        match self {
            Self::Bottom => RectCorner::BottomLeft,
            Self::Right => RectCorner::BottomRight,
            Self::Top => RectCorner::TopRight,
            Self::Left => RectCorner::TopLeft,
        }
    }

    /// The corner at which the edge ends.
    pub const fn end(self) -> RectCorner {
        match self {
            Self::Bottom => RectCorner::BottomRight,
            Self::Right => RectCorner::TopRight,
            Self::Top => RectCorner::TopLeft,
            Self::Left => RectCorner::BottomLeft,
        }
    }

    /// The edge between two adjacent corners, or `None` for a diagonal or equal pair.
    pub const fn between(start: RectCorner, end: RectCorner) -> Option<Self> {
        match (start, end) {
            (RectCorner::BottomLeft, RectCorner::BottomRight) => Some(Self::Bottom),
            (RectCorner::BottomRight, RectCorner::TopRight) => Some(Self::Right),
            (RectCorner::TopRight, RectCorner::TopLeft) => Some(Self::Top),
            (RectCorner::TopLeft, RectCorner::BottomLeft) => Some(Self::Left),
            _ => None,
        }
    }
}

/// The face through which a machining operation enters the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryFace {
    /// A large face, `A` or `B`.
    Face(FaceSide),
    /// A side face identified by its stable edge id.
    Edge(EdgeId),
}

pub(crate) fn rects_overlap(a: PanelRect, b: PanelRect) -> bool {
    let a_x0 = a.x().micrometres() as i128;
    let a_y0 = a.y().micrometres() as i128;
    let a_x1 = a_x0 + a.width().micrometres() as i128;
    let a_y1 = a_y0 + a.height().micrometres() as i128;
    let b_x0 = b.x().micrometres() as i128;
    let b_y0 = b.y().micrometres() as i128;
    let b_x1 = b_x0 + b.width().micrometres() as i128;
    let b_y1 = b_y0 + b.height().micrometres() as i128;
    a_x0 < b_x1 && b_x0 < a_x1 && a_y0 < b_y1 && b_y0 < a_y1
}

/// Rounds a millimetre value to the nearest micrometre, or reports why it is not representable.
///
/// Unlike [`length_from_millimetres`], a negative value is kept: a core region may start left of or
/// below the nominal minimum corner when an outward bow carries the material past it.
fn signed_length_from_millimetres(value: f64) -> Result<Length, PanelGeometryError> {
    let micrometres = (value * 1000.0).round();
    if !micrometres.is_finite() || micrometres.abs() > i64::MAX as f64 {
        return Err(PanelGeometryError::Contour);
    }
    Ok(Length::from_micrometres(micrometres as i64))
}

/// Rounds a millimetre value to the nearest micrometre, or reports why it is not representable.
pub(crate) fn length_from_millimetres(value: f64) -> Result<Length, PanelGeometryError> {
    let micrometres = (value * 1000.0).round();
    if !micrometres.is_finite() || micrometres < 0.0 || micrometres > i64::MAX as f64 {
        return Err(PanelGeometryError::Contour);
    }
    Ok(Length::from_micrometres(micrometres as i64))
}

/// Rejects a dimension that is non-positive or over its limit.
pub(crate) fn validate_dimension(field: &'static str, value: Length, max: Length) -> Result<(), PanelDimensionError> {
    if !value.is_positive() {
        return Err(PanelDimensionError::NonPositive { field, value });
    }
    if value > max {
        return Err(PanelDimensionError::TooLarge { field, value, max });
    }
    Ok(())
}

fn require_positive(field: &'static str, value: Length) -> Result<(), PanelGeometryError> {
    if !value.is_positive() {
        return Err(PanelGeometryError::NonPositive { field, value });
    }
    Ok(())
}

fn require_non_negative(field: &'static str, value: Length) -> Result<(), PanelGeometryError> {
    if value.is_negative() {
        return Err(PanelGeometryError::NegativeOrigin { field, value });
    }
    Ok(())
}

/// Serialized form of [`PanelNominalSize`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelNominalSizeWire {
    length: Length,
    width: Length,
}

impl TryFrom<PanelNominalSizeWire> for PanelNominalSize {
    type Error = PanelDimensionError;

    fn try_from(wire: PanelNominalSizeWire) -> Result<Self, Self::Error> {
        Self::new(wire.length, wire.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    use crate::fixtures::*;

    #[test]
    fn panel_nominal_size_accepts_positive_dimensions() {
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        assert_eq!(size.length(), millimetres(700));
        assert_eq!(size.width(), millimetres(500));
    }

    #[test]
    fn panel_nominal_size_rejects_non_positive_and_oversized_dimensions() {
        let zero = Length::ZERO;
        let negative = millimetres(-1);
        assert_eq!(
            PanelNominalSize::new(zero, millimetres(500)),
            Err(PanelDimensionError::NonPositive { field: "length", value: zero })
        );
        assert_eq!(
            PanelNominalSize::new(millimetres(700), negative),
            Err(PanelDimensionError::NonPositive { field: "width", value: negative })
        );

        let too_long = Length::from_micrometres(Panel::MAX_LENGTH.micrometres() + 1);
        assert_eq!(
            PanelNominalSize::new(too_long, millimetres(500)),
            Err(PanelDimensionError::TooLarge { field: "length", value: too_long, max: Panel::MAX_LENGTH })
        );

        let too_wide = Length::from_micrometres(Panel::MAX_WIDTH.micrometres() + 1);
        assert_eq!(
            PanelNominalSize::new(millimetres(700), too_wide),
            Err(PanelDimensionError::TooLarge { field: "width", value: too_wide, max: Panel::MAX_WIDTH })
        );

        assert!(PanelNominalSize::new(Panel::MAX_LENGTH, Panel::MAX_WIDTH).is_ok());
    }

    #[test]
    fn panel_nominal_size_serde_round_trip_and_validation() {
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        let text = serde_json::to_string(&size).expect("serialize panel size");
        assert_eq!(text, r#"{"length":700000,"width":500000}"#);
        assert_eq!(serde_json::from_str::<PanelNominalSize>(&text).expect("deserialize panel size"), size);

        assert!(serde_json::from_str::<PanelNominalSize>(r#"{"length":0,"width":500000}"#).is_err());
        assert!(serde_json::from_str::<PanelNominalSize>(r#"{"length":10000001,"width":500000}"#).is_err());
        assert!(
            serde_json::from_str::<PanelNominalSize>(r#"{"length":700000,"width":500000,"unexpected":1}"#).is_err()
        );
    }

    #[test]
    fn panel_geometry_uses_nominal_size_and_thickness() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let geometry = panel.geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(500));
        assert_eq!(geometry.thickness(), millimetres(18));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn all_included_edges_inset_the_core_inside_the_nominal_envelope() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        for edge in 1..=4 {
            let band =
                EdgeBanding::new(EdgeId::new(edge).expect("edge id"), material, true, millimetres(2), millimetres(22))
                    .expect("valid banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }

        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn panel_geometry_rejects_a_negative_core_origin() {
        let error = PanelGeometry::new(
            millimetres(-1),
            Length::ZERO,
            millimetres(700),
            millimetres(500),
            sample_thickness(),
            millimetres(700),
            millimetres(500),
        )
        .expect_err("negative core origin");
        assert!(matches!(error, PanelGeometryError::NegativeOrigin { field: "core_x", .. }));
    }

    #[test]
    fn panel_geometry_accepts_catalogue_resolved_banding_thickness() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        let bottom =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(2), millimetres(22))
                .expect("valid bottom banding");
        project.set_edge_banding(panel_id, bottom).expect("set bottom banding");
        let panel = project.panel(panel_id).expect("panel");

        let geometry = panel.geometry_with_banding(sample_thickness(), |_| millimetres(4)).expect("resolved geometry");
        assert_eq!(geometry.core_y(), millimetres(4));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn panel_geometry_respects_included_banding_and_application_order() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");

        let bottom =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(2), millimetres(22))
                .expect("valid bottom banding");
        project.set_edge_banding(panel_id, bottom).expect("set bottom banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), Length::ZERO);
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let left = EdgeBanding::new(EdgeId::new(4).expect("edge id"), material, false, millimetres(1), millimetres(22))
            .expect("valid left banding");
        project.set_edge_banding(panel_id, left).expect("set left banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), Length::ZERO);
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(701));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let left_included =
            EdgeBanding::new(EdgeId::new(4).expect("edge id"), material, true, millimetres(1), millimetres(22))
                .expect("valid left banding");
        project.set_edge_banding(panel_id, left_included).expect("set left banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), millimetres(1));
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(699));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let bands = project.panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0].edge().get(), 1);
        assert_eq!(bands[1].edge().get(), 4);
    }

    #[test]
    fn face_sides_are_opposites() {
        assert_eq!(FaceSide::A.opposite(), FaceSide::B);
        assert_eq!(FaceSide::B.opposite(), FaceSide::A);
        assert_eq!(FaceSide::A.normal_sign(), 1);
        assert_eq!(FaceSide::B.normal_sign(), -1);
        assert_eq!(FaceSide::A.label(), "A");
    }

    #[test]
    fn rectangle_corners_are_counter_clockwise_from_zero() {
        assert_eq!(RectCorner::ALL.map(RectCorner::label), ["1", "2", "3", "4"]);
        assert_eq!(RectCorner::ALL.map(|corner| corner.id().get()), [1, 2, 3, 4]);
        assert_eq!(RectCorner::BottomLeft.next(), RectCorner::BottomRight);
        assert_eq!(RectCorner::BottomRight.next(), RectCorner::TopRight);
        assert_eq!(RectCorner::TopRight.next(), RectCorner::TopLeft);
        assert_eq!(RectCorner::TopLeft.next(), RectCorner::BottomLeft);
    }

    #[test]
    fn rectangle_edges_connect_consecutive_corners() {
        for edge in RectEdge::ALL {
            assert_eq!(edge.start().next(), edge.end());
            assert_eq!(RectEdge::between(edge.start(), edge.end()), Some(edge));
        }
        assert_eq!(RectEdge::between(RectCorner::BottomLeft, RectCorner::TopLeft), None);
    }

    #[test]
    fn panel_rects_reject_non_positive_dimensions() {
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, Length::ZERO, millimetres(10)),
            Err(PanelRectError::NonPositive { field: "width", value: Length::ZERO })
        );
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, millimetres(10), Length::ZERO),
            Err(PanelRectError::NonPositive { field: "height", value: Length::ZERO })
        );
        assert!(serde_json::from_str::<PanelRect>(r#"{"x":0,"y":0,"width":0,"height":10000}"#).is_err());
    }
}
