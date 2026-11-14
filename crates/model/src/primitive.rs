//! Parametric primitives: the shape parameters a mesh is derived from.
//!
//! A primitive stores what the user can still change — a width, a depth, a height, a radius, the
//! number of sides and latitude bands — and never the mesh. The mesh is derived for the viewport and
//! for `object.collapse`, so raising a radius rebuilds one object rather than rewriting its vertices
//! ([ADR 0054]). A box keeps six faces at three independent sides; the face count is a parameter of
//! the curved primitives ([ADR 0101]).
//!
//! [ADR 0054]: ../docs/decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md
//! [ADR 0101]: ../docs/decisions/0101-a-frame-and-a-box-are-one-tool.md

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Largest accepted primitive dimension: ten metres, the same bound the panel rules use.
pub const MAX_PRIMITIVE_SIZE: Length = Length::from_micrometres(10_000_000);

/// Smallest accepted primitive dimension.
///
/// Two micrometres keep a positive half-extent when the mesh builder halves an edge or a height
/// into integer micrometres.
pub const MIN_PRIMITIVE_SIZE: Length = Length::from_micrometres(2);

/// Sides around a curved primitive when none is given.
pub const DEFAULT_SEGMENTS: u32 = 32;

/// Latitude bands between the two poles of a sphere when none is given.
pub const DEFAULT_BANDS: u32 = 16;

/// Fewest sides a curved primitive accepts.
pub const MIN_SEGMENTS: u32 = 3;

/// Most sides a curved primitive accepts.
pub const MAX_SEGMENTS: u32 = 256;

/// Fewest latitude bands a sphere accepts.
pub const MIN_BANDS: u32 = 2;

/// Most latitude bands a sphere accepts.
pub const MAX_BANDS: u32 = 128;

/// The primitive shapes the editor can build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind {
    /// A box with three independent sides, anchored at its base minimum corner.
    Cuboid,
    /// An upright cylinder centred on the object's local origin.
    Cylinder,
    /// A sphere centred on the object's local origin.
    Sphere,
}

impl PrimitiveKind {
    /// The shape name shown to the user.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cuboid => "Box",
            Self::Cylinder => "Cylinder",
            Self::Sphere => "Sphere",
        }
    }

    /// The lower-case shape name used in the scene tree.
    pub const fn shape_name(self) -> &'static str {
        match self {
            Self::Cuboid => "box",
            Self::Cylinder => "cylinder",
            Self::Sphere => "sphere",
        }
    }
}

/// A rejected primitive parameter set.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PrimitiveError {
    /// A dimension is not positive or does not fit the accepted range.
    #[error("a primitive dimension must be between {min} and {max}, got {value}")]
    InvalidLength {
        /// The rejected length.
        value: Length,
        /// The smallest accepted length.
        min: Length,
        /// The largest accepted length.
        max: Length,
    },
    /// A curved primitive has too few or too many sides.
    #[error("a curved primitive needs between {min} and {max} sides, got {count}")]
    SegmentCount {
        /// The supplied side count.
        count: u32,
        /// The smallest accepted count.
        min: u32,
        /// The largest accepted count.
        max: u32,
    },
    /// A sphere has too few or too many latitude bands.
    #[error("a sphere needs between {min} and {max} latitude bands, got {count}")]
    BandCount {
        /// The supplied band count.
        count: u32,
        /// The smallest accepted count.
        min: u32,
        /// The largest accepted count.
        max: u32,
    },
}

/// Validates one primitive dimension.
fn validate_length(value: Length) -> Result<(), PrimitiveError> {
    if value < MIN_PRIMITIVE_SIZE || value > MAX_PRIMITIVE_SIZE {
        return Err(PrimitiveError::InvalidLength { value, min: MIN_PRIMITIVE_SIZE, max: MAX_PRIMITIVE_SIZE });
    }
    Ok(())
}

/// Validates a curved primitive's side count.
fn validate_segments(segments: u32) -> Result<(), PrimitiveError> {
    if !(MIN_SEGMENTS..=MAX_SEGMENTS).contains(&segments) {
        return Err(PrimitiveError::SegmentCount { count: segments, min: MIN_SEGMENTS, max: MAX_SEGMENTS });
    }
    Ok(())
}

/// Validates a sphere's latitude band count.
fn validate_bands(bands: u32) -> Result<(), PrimitiveError> {
    if !(MIN_BANDS..=MAX_BANDS).contains(&bands) {
        return Err(PrimitiveError::BandCount { count: bands, min: MIN_BANDS, max: MAX_BANDS });
    }
    Ok(())
}

/// Serialized form of [`Cuboid`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CuboidWire {
    width: Length,
    depth: Length,
    height: Length,
}

/// A box with three independent sides, anchored at its base minimum corner.
///
/// The local zero is the corner of the base rectangle, so the box occupies
/// `[0..width] × [0..depth] × [0..height]` in its own frame — the same anchor a panel has and the
/// space a product frame reserves (ADR 0083, ADR 0101).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CuboidWire")]
pub struct Cuboid {
    width: Length,
    depth: Length,
    height: Length,
}

impl Cuboid {
    /// Validates the three sides and stores them.
    pub fn new(width: Length, depth: Length, height: Length) -> Result<Self, PrimitiveError> {
        validate_length(width)?;
        validate_length(depth)?;
        validate_length(height)?;
        Ok(Self { width, depth, height })
    }

    /// The side along local X.
    pub const fn width(&self) -> Length {
        self.width
    }

    /// The side along local Y.
    pub const fn depth(&self) -> Length {
        self.depth
    }

    /// The side along local Z.
    pub const fn height(&self) -> Length {
        self.height
    }

    /// The three sides as primitives build them.
    pub const fn sides(&self) -> [Length; 3] {
        [self.width, self.depth, self.height]
    }
}

impl TryFrom<CuboidWire> for Cuboid {
    type Error = PrimitiveError;

    fn try_from(wire: CuboidWire) -> Result<Self, Self::Error> {
        Self::new(wire.width, wire.depth, wire.height)
    }
}

/// Serialized form of [`Cylinder`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CylinderWire {
    radius: Length,
    height: Length,
    segments: u32,
}

/// An upright cylinder centred on the local origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CylinderWire")]
pub struct Cylinder {
    radius: Length,
    height: Length,
    segments: u32,
}

impl Cylinder {
    /// Validates the dimensions and the side count, then stores them.
    pub fn new(radius: Length, height: Length, segments: u32) -> Result<Self, PrimitiveError> {
        validate_length(radius)?;
        validate_length(height)?;
        validate_segments(segments)?;
        Ok(Self { radius, height, segments })
    }

    /// The radius in the local XY plane.
    pub const fn radius(&self) -> Length {
        self.radius
    }

    /// The full height along local Z.
    pub const fn height(&self) -> Length {
        self.height
    }

    /// Sides around the tube.
    pub const fn segments(&self) -> u32 {
        self.segments
    }
}

impl TryFrom<CylinderWire> for Cylinder {
    type Error = PrimitiveError;

    fn try_from(wire: CylinderWire) -> Result<Self, Self::Error> {
        Self::new(wire.radius, wire.height, wire.segments)
    }
}

/// Serialized form of [`Sphere`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphereWire {
    radius: Length,
    segments: u32,
    bands: u32,
}

/// A sphere centred on the local origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SphereWire")]
pub struct Sphere {
    radius: Length,
    segments: u32,
    bands: u32,
}

impl Sphere {
    /// Validates the radius, the side count and the band count, then stores them.
    pub fn new(radius: Length, segments: u32, bands: u32) -> Result<Self, PrimitiveError> {
        validate_length(radius)?;
        validate_segments(segments)?;
        validate_bands(bands)?;
        Ok(Self { radius, segments, bands })
    }

    /// The radius in every direction.
    pub const fn radius(&self) -> Length {
        self.radius
    }

    /// Sides around each latitude ring.
    pub const fn segments(&self) -> u32 {
        self.segments
    }

    /// Latitude bands between the two poles.
    pub const fn bands(&self) -> u32 {
        self.bands
    }
}

impl TryFrom<SphereWire> for Sphere {
    type Error = PrimitiveError;

    fn try_from(wire: SphereWire) -> Result<Self, Self::Error> {
        Self::new(wire.radius, wire.segments, wire.bands)
    }
}

/// A parametric primitive: the shape and the parameters that build its mesh.
///
/// The inner tag is `shape`, because the outer [`Geometry`](crate::Geometry) payload already uses
/// `kind`. Parameters are the source of truth; the mesh is never stored beside them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Primitive {
    /// A box with three independent sides.
    Cuboid(Cuboid),
    /// An upright cylinder.
    Cylinder(Cylinder),
    /// A sphere built from latitude bands.
    Sphere(Sphere),
}

impl Primitive {
    /// A box with the given width, depth and height, anchored at its base minimum corner.
    pub fn cuboid(width: Length, depth: Length, height: Length) -> Result<Self, PrimitiveError> {
        Ok(Self::Cuboid(Cuboid::new(width, depth, height)?))
    }

    /// A cylinder with the given radius, height and side count.
    pub fn cylinder(radius: Length, height: Length, segments: u32) -> Result<Self, PrimitiveError> {
        Ok(Self::Cylinder(Cylinder::new(radius, height, segments)?))
    }

    /// A sphere with the given radius, side count and latitude bands.
    pub fn sphere(radius: Length, segments: u32, bands: u32) -> Result<Self, PrimitiveError> {
        Ok(Self::Sphere(Sphere::new(radius, segments, bands)?))
    }

    /// Builds the default primitive of one kind from the creation size.
    ///
    /// The size is the full extent: the side of a cubic box, or the diameter of a cylinder or
    /// sphere. Cylinder height equals the size, and both curved shapes use the default
    /// tessellation. A box drawn with a base rectangle and a height is built by its own
    /// constructor, because one extent cannot name three sides; this one is the equal-sided box the
    /// size still describes.
    pub fn from_kind(kind: PrimitiveKind, size: Length) -> Result<Self, PrimitiveError> {
        validate_length(size)?;
        let radius = Length::from_micrometres(size.micrometres() / 2);
        match kind {
            PrimitiveKind::Cuboid => Self::cuboid(size, size, size),
            PrimitiveKind::Cylinder => Self::cylinder(radius, size, DEFAULT_SEGMENTS),
            PrimitiveKind::Sphere => Self::sphere(radius, DEFAULT_SEGMENTS, DEFAULT_BANDS),
        }
    }

    /// Which shape this primitive is.
    pub const fn kind(&self) -> PrimitiveKind {
        match self {
            Self::Cuboid(_) => PrimitiveKind::Cuboid,
            Self::Cylinder(_) => PrimitiveKind::Cylinder,
            Self::Sphere(_) => PrimitiveKind::Sphere,
        }
    }

    /// The lower-case shape name used in the scene tree.
    pub const fn shape_name(&self) -> &'static str {
        self.kind().shape_name()
    }

    /// The number of polygon faces the derived mesh has.
    ///
    /// It is computed from the parameters, so the card can show a face count without building the
    /// mesh: a box has six, a cylinder one quad per side plus two caps, and a sphere one face per
    /// side and band.
    pub const fn face_count(&self) -> usize {
        match self {
            Self::Cuboid(_) => 6,
            Self::Cylinder(cylinder) => cylinder.segments as usize + 2,
            Self::Sphere(sphere) => sphere.segments as usize * sphere.bands as usize,
        }
    }

    /// The number of vertices the derived mesh has.
    pub const fn vertex_count(&self) -> usize {
        match self {
            Self::Cuboid(_) => 8,
            Self::Cylinder(cylinder) => cylinder.segments as usize * 2,
            Self::Sphere(sphere) => 2 + sphere.segments as usize * (sphere.bands as usize - 1),
        }
    }

    /// The vertex and face counts of the derived mesh.
    pub const fn mesh_counts(&self) -> (usize, usize) {
        (self.vertex_count(), self.face_count())
    }

    /// The local bounding box of the derived mesh, in micrometres.
    ///
    /// It is analytic, so the tree and the pivot do not need the evaluated mesh. A box starts at the
    /// local zero and a curved shape is centred on it.
    pub const fn bounds_micrometres(&self) -> ([i64; 3], [i64; 3]) {
        match self {
            Self::Cuboid(cuboid) => {
                ([0; 3], [cuboid.width.micrometres(), cuboid.depth.micrometres(), cuboid.height.micrometres()])
            }
            Self::Cylinder(cylinder) => {
                let radius = cylinder.radius.micrometres();
                let half = cylinder.height.micrometres() / 2;
                ([-radius, -radius, -half], [radius, radius, half])
            }
            Self::Sphere(sphere) => {
                let radius = sphere.radius.micrometres();
                ([-radius; 3], [radius; 3])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::millimetres;

    use super::*;

    #[test]
    fn the_creation_size_builds_the_default_shape() {
        let size = millimetres(100);
        let cuboid = Primitive::from_kind(PrimitiveKind::Cuboid, size).expect("box");
        assert_eq!(cuboid, Primitive::cuboid(size, size, size).expect("box"));

        let cylinder = Primitive::from_kind(PrimitiveKind::Cylinder, size).expect("cylinder");
        let Primitive::Cylinder(cylinder) = cylinder else { panic!("a cylinder") };
        assert_eq!(cylinder.radius(), millimetres(50), "the diameter is the creation size");
        assert_eq!(cylinder.height(), size);
        assert_eq!(cylinder.segments(), DEFAULT_SEGMENTS);

        let sphere = Primitive::from_kind(PrimitiveKind::Sphere, size).expect("sphere");
        let Primitive::Sphere(sphere) = sphere else { panic!("a sphere") };
        assert_eq!(sphere.radius(), millimetres(50));
        assert_eq!(sphere.segments(), DEFAULT_SEGMENTS);
        assert_eq!(sphere.bands(), DEFAULT_BANDS);
    }

    #[test]
    fn a_box_keeps_three_independent_sides_from_its_base_corner() {
        let cuboid = Primitive::cuboid(millimetres(600), millimetres(400), millimetres(720)).expect("a box");
        let Primitive::Cuboid(box_) = &cuboid else { panic!("a box") };
        assert_eq!(box_.sides(), [millimetres(600), millimetres(400), millimetres(720)]);
        assert_eq!(cuboid.mesh_counts(), (8, 6), "a box keeps eight corners and six faces");
        assert_eq!(
            cuboid.bounds_micrometres(),
            ([0; 3], [600_000, 400_000, 720_000]),
            "the local zero is the corner of the base rectangle"
        );
    }

    #[test]
    fn counts_and_bounds_follow_the_parameters() {
        let cuboid = Primitive::cuboid(millimetres(100), millimetres(100), millimetres(100)).expect("box");
        assert_eq!(cuboid.mesh_counts(), (8, 6));
        assert_eq!(cuboid.bounds_micrometres(), ([0; 3], [100_000; 3]));

        let cylinder = Primitive::cylinder(millimetres(20), millimetres(100), 12).expect("cylinder");
        assert_eq!(cylinder.mesh_counts(), (24, 14));
        assert_eq!(cylinder.bounds_micrometres(), ([-20_000, -20_000, -50_000], [20_000, 20_000, 50_000]));

        let sphere = Primitive::sphere(millimetres(30), 8, 4).expect("sphere");
        assert_eq!(sphere.mesh_counts(), (2 + 8 * 3, 8 * 4));
        assert_eq!(sphere.bounds_micrometres(), ([-30_000; 3], [30_000; 3]));
    }

    #[test]
    fn a_bad_dimension_is_rejected() {
        for value in [
            Length::ZERO,
            Length::from_micrometres(1),
            MAX_PRIMITIVE_SIZE.checked_add(Length::from_micrometres(1)).expect("sum"),
        ] {
            let error = Primitive::cuboid(value, millimetres(100), millimetres(100)).expect_err("a bad width");
            assert!(matches!(error, PrimitiveError::InvalidLength { .. }), "error was {error:?}");
            let error = Primitive::cuboid(millimetres(100), millimetres(100), value).expect_err("a bad height");
            assert!(matches!(error, PrimitiveError::InvalidLength { .. }), "error was {error:?}");
        }
    }

    #[test]
    fn a_bad_tessellation_is_rejected() {
        assert_eq!(
            Primitive::cylinder(millimetres(10), millimetres(10), 2),
            Err(PrimitiveError::SegmentCount { count: 2, min: MIN_SEGMENTS, max: MAX_SEGMENTS })
        );
        assert_eq!(
            Primitive::sphere(millimetres(10), MAX_SEGMENTS + 1, DEFAULT_BANDS),
            Err(PrimitiveError::SegmentCount { count: MAX_SEGMENTS + 1, min: MIN_SEGMENTS, max: MAX_SEGMENTS })
        );
        assert_eq!(
            Primitive::sphere(millimetres(10), DEFAULT_SEGMENTS, 1),
            Err(PrimitiveError::BandCount { count: 1, min: MIN_BANDS, max: MAX_BANDS })
        );
    }

    #[test]
    fn a_primitive_round_trips_with_its_shape_tag() {
        let sphere = Primitive::sphere(millimetres(100), 24, 12).expect("sphere");
        let text = serde_json::to_string(&sphere).expect("serialize");
        assert!(text.contains("\"shape\":\"sphere\""), "the shape is the inner tag: {text}");
        assert!(!text.contains("\"kind\""), "the geometry tag is not repeated: {text}");
        let decoded: Primitive = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(decoded, sphere);

        let cuboid = Primitive::cuboid(millimetres(600), millimetres(400), millimetres(720)).expect("box");
        let text = serde_json::to_string(&cuboid).expect("serialize");
        assert!(text.contains("\"shape\":\"cuboid\""), "a box is tagged cuboid: {text}");
        let decoded: Primitive = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(decoded, cuboid);
    }

    #[test]
    fn a_box_with_a_missing_side_is_not_a_shape() {
        let text = r#"{"shape":"cuboid","width":600000,"depth":400000}"#;
        assert!(serde_json::from_str::<Primitive>(text).is_err(), "three sides are required");
        let text = r#"{"shape":"cube","edge":100000}"#;
        assert!(serde_json::from_str::<Primitive>(text).is_err(), "the cube shape is gone with schema 22");
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let text = r#"{"shape":"sphere","radius":100000,"segments":32,"bands":16,"thickness":1}"#;
        assert!(serde_json::from_str::<Primitive>(text).is_err(), "an unknown parameter is an error");
    }
}
