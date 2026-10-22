//! The FurniCraft document: assemblies, panels, machining, hardware, materials and dimensions.
//!
//! Pure serializable data plus its invariants. No geometry, no GPU, no UI, no file I/O. The exact
//! document schema is designed in M1; see `PROJECT.md`, "Open model questions".
//!
//! This module currently defines the vocabulary shared by every panel: document ids and
//! revisions, validated rectangular dimensions, rigid placements, the two large faces, stable
//! edge and vertex ids, and the local numbering of a rectangular outline. Geometry and the full
//! `Panel` record arrive with the panel milestones.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::num::NonZeroU64;

use furnicraft_geom::{DQuat, DVec3};
use furnicraft_units::Length;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// A stable document identifier.
///
/// Identifiers are never reused while a document is edited, so commands, hardware references and
/// the undo stack can refer to an assembly, panel or catalogue entry without holding a pointer.
/// Zero is reserved for "no entity" and is rejected on construction and deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(NonZeroU64);

impl Id {
    /// Creates an id from a non-zero raw value.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// A monotonically increasing document revision.
///
/// The revision changes after every document mutation and keys every derived cache. It starts at
/// zero and never wraps: [`Revision::checked_next`] returns `None` at the end of the range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// The initial revision of an unmodified document.
    pub const ZERO: Self = Self(0);

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next revision, or `None` if the counter would overflow.
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// A rigid placement relative to a parent frame.
///
/// Placement is translation plus rotation, never scale or shear. Rotations are stored as a unit
/// quaternion; construction and deserialization reject a zero or non-unit quaternion instead of
/// silently normalising it. The serialized rotation is `[x, y, z, w]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PlacementWire", into = "PlacementWire")]
pub struct Placement {
    translation: DVec3,
    rotation: DQuat,
}

impl Placement {
    /// The identity placement: no translation and no rotation.
    pub const IDENTITY: Self = Self { translation: DVec3::ZERO, rotation: DQuat::IDENTITY };

    /// Creates a placement after checking that every component is finite and the rotation is unit.
    pub fn new(translation: DVec3, rotation: DQuat) -> Result<Self, PlacementError> {
        if !translation.is_finite() {
            return Err(PlacementError::NonFiniteTranslation);
        }
        if !rotation.is_finite() {
            return Err(PlacementError::NonFiniteRotation);
        }
        let length = rotation.length();
        if !length.is_finite() || (length - 1.0).abs() > ROTATION_TOLERANCE {
            return Err(PlacementError::NonUnitRotation { length });
        }
        Ok(Self { translation, rotation })
    }

    /// The translation part, in the parent frame.
    pub const fn translation(self) -> DVec3 {
        self.translation
    }

    /// The unit rotation part, in the parent frame.
    pub const fn rotation(self) -> DQuat {
        self.rotation
    }

    /// Composes a child placement into its parent's frame: `parent ∘ child`.
    ///
    /// A point expressed in the child frame is first transformed by `child`, then by `parent`.
    pub fn compose(parent: Self, child: Self) -> Result<Self, PlacementError> {
        let rotation = (parent.rotation * child.rotation).normalize();
        let translation = parent.rotation * child.translation + parent.translation;
        Self::new(translation, rotation)
    }

    /// Applies the placement to a point.
    pub fn transform_point(self, point: DVec3) -> DVec3 {
        self.rotation * point + self.translation
    }

    /// Applies only the rotation to a direction vector.
    pub fn transform_vector(self, vector: DVec3) -> DVec3 {
        self.rotation * vector
    }
}

/// Why a placement is not a valid rigid transform.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum PlacementError {
    /// The translation contains a NaN or an infinity.
    #[error("placement translation must be finite")]
    NonFiniteTranslation,
    /// The rotation contains a NaN or an infinity.
    #[error("placement rotation must be finite")]
    NonFiniteRotation,
    /// The rotation quaternion is not unit length.
    #[error("placement rotation must be a unit quaternion, got length {length}")]
    NonUnitRotation {
        /// The length of the rejected quaternion.
        length: f64,
    },
}

/// Accepted deviation from unit length for a rotation quaternion.
const ROTATION_TOLERANCE: f64 = 1e-9;

/// Serialized form of [`Placement`]; validation happens in its `TryFrom` implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementWire {
    translation: [f64; 3],
    rotation: [f64; 4],
}

impl From<Placement> for PlacementWire {
    fn from(placement: Placement) -> Self {
        let rotation = placement.rotation;
        Self {
            translation: placement.translation.to_array(),
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
        }
    }
}

impl TryFrom<PlacementWire> for Placement {
    type Error = PlacementError;

    fn try_from(wire: PlacementWire) -> Result<Self, Self::Error> {
        let translation = DVec3::from_array(wire.translation);
        let rotation = DQuat::from_xyzw(wire.rotation[0], wire.rotation[1], wire.rotation[2], wire.rotation[3]);
        Self::new(translation, rotation)
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

/// The validated dimensions of a rectangular panel.
///
/// In the panel-local frame the panel occupies `x` from `0` to the length, `y` from `0` to the
/// width and `z` from `0` to the thickness. The limits are validation rules, not part of the
/// stored value: every dimension stays an `i64` micrometre count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PanelSizeWire")]
pub struct PanelSize {
    length: Length,
    width: Length,
    thickness: Length,
}

impl PanelSize {
    /// Largest accepted panel length or width: ten metres.
    pub const MAX_LENGTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted panel width: ten metres.
    pub const MAX_WIDTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted panel thickness: one metre.
    pub const MAX_THICKNESS: Length = Length::from_micrometres(1_000_000);

    /// Creates dimensions after checking that every side is positive and within its limit.
    pub fn new(length: Length, width: Length, thickness: Length) -> Result<Self, PanelSizeError> {
        validate_dimension("length", length, Self::MAX_LENGTH)?;
        validate_dimension("width", width, Self::MAX_WIDTH)?;
        validate_dimension("thickness", thickness, Self::MAX_THICKNESS)?;
        Ok(Self { length, width, thickness })
    }

    /// The panel length along local X.
    pub const fn length(self) -> Length {
        self.length
    }

    /// The panel width along local Y.
    pub const fn width(self) -> Length {
        self.width
    }

    /// The panel thickness along local Z.
    pub const fn thickness(self) -> Length {
        self.thickness
    }
}

/// Why a set of panel dimensions is not usable.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelSizeError {
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

fn validate_dimension(field: &'static str, value: Length, max: Length) -> Result<(), PanelSizeError> {
    if !value.is_positive() {
        return Err(PanelSizeError::NonPositive { field, value });
    }
    if value > max {
        return Err(PanelSizeError::TooLarge { field, value, max });
    }
    Ok(())
}

/// Serialized form of [`PanelSize`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelSizeWire {
    length: Length,
    width: Length,
    thickness: Length,
}

impl TryFrom<PanelSizeWire> for PanelSize {
    type Error = PanelSizeError;

    fn try_from(wire: PanelSizeWire) -> Result<Self, Self::Error> {
        Self::new(wire.length, wire.width, wire.thickness)
    }
}

macro_rules! panel_id {
    ($name:ident, $description:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates an id from a non-zero value.
            pub const fn new(value: u32) -> Option<Self> {
                if value == 0 { None } else { Some(Self(value)) }
            }

            /// The numeric value.
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Builds an id from a value that is known to be non-zero inside this crate.
            pub(crate) const fn from_known(value: u32) -> Self {
                Self(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u32::deserialize(deserializer)?;
                Self::new(value)
                    .ok_or_else(|| <D::Error as serde::de::Error>::custom(concat!($description, " must be non-zero")))
            }
        }
    };
}

panel_id!(VertexId, "vertex id");
panel_id!(EdgeId, "edge id");

/// A corner of a rectangular panel, numbered counter-clockwise when viewed from face `A`.
///
/// Corner 1 is the panel zero at `(0, 0)`. The enum is a convenience for rectangular panels;
/// contour panels use [`VertexId`] directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RectCorner {
    /// `(0, 0)`, the panel zero.
    BottomLeft = 1,
    /// `(length, 0)`.
    BottomRight = 2,
    /// `(length, width)`.
    TopRight = 3,
    /// `(0, width)`.
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

#[cfg(test)]
mod tests {
    use super::*;

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

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    #[test]
    fn ids_are_non_zero_and_serialize_as_numbers() {
        assert!(Id::new(0).is_none());
        let id = Id::new(42).expect("non-zero id");
        assert_eq!(id.get(), 42);
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "42");
        assert_eq!(serde_json::from_str::<Id>("42").expect("deserialize id"), id);
        assert!(serde_json::from_str::<Id>("0").is_err());
        assert_eq!(std::mem::size_of::<Option<Id>>(), std::mem::size_of::<u64>());
    }

    #[test]
    fn revisions_are_monotonic_and_do_not_wrap() {
        assert_eq!(Revision::ZERO.get(), 0);
        let one = Revision::ZERO.checked_next().expect("first revision");
        assert_eq!(one.get(), 1);
        assert_eq!(one.checked_next().expect("second revision").get(), 2);

        assert_eq!(Revision(u64::MAX).checked_next(), None);
        assert_eq!(serde_json::from_str::<Revision>("7").expect("deserialize revision").get(), 7);
    }

    #[test]
    fn identity_placement_preserves_points_and_vectors() {
        let point = DVec3::new(1.0, 2.0, 3.0);
        assert_eq!(Placement::IDENTITY.transform_point(point), point);
        assert_eq!(Placement::IDENTITY.transform_vector(DVec3::X), DVec3::X);
    }

    #[test]
    fn placements_compose_translation_and_rotation() {
        let parent = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("parent placement");
        let child = Placement::new(DVec3::new(0.0, 5.0, 0.0), DQuat::IDENTITY).expect("child placement");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::ZERO);
        assert!((point - DVec3::new(10.0, 5.0, 0.0)).length() < 1e-12, "point was {point:?}");

        let parent =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)).expect("rotated parent");
        let child = Placement::new(DVec3::new(1.0, 0.0, 0.0), DQuat::IDENTITY).expect("translated child");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::ZERO);
        assert!((point - DVec3::new(0.0, 1.0, 0.0)).length() < 1e-12, "point was {point:?}");

        let parent =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)).expect("rotated parent");
        let child =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)).expect("rotated child");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::Y);
        assert!((point - DVec3::Z).length() < 1e-12, "point was {point:?}");
    }

    #[test]
    fn placements_reject_non_finite_and_non_unit_input() {
        assert_eq!(
            Placement::new(DVec3::new(f64::NAN, 0.0, 0.0), DQuat::IDENTITY),
            Err(PlacementError::NonFiniteTranslation)
        );
        assert_eq!(
            Placement::new(DVec3::ZERO, DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0)),
            Err(PlacementError::NonFiniteRotation)
        );
        assert!(matches!(
            Placement::new(DVec3::ZERO, DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0)),
            Err(PlacementError::NonUnitRotation { length }) if (length - 2.0).abs() < 1e-12
        ));
    }

    #[test]
    fn placements_round_trip_and_validate_on_load() {
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let text = serde_json::to_string(&placement).expect("serialize placement");
        assert!(text.contains(r#""translation":[1.0,2.0,3.0]"#), "json was {text}");
        assert_eq!(serde_json::from_str::<Placement>(&text).expect("deserialize placement"), placement);
        assert!(
            serde_json::from_str::<Placement>(r#"{"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,2.0]}"#).is_err()
        );
        assert!(
            serde_json::from_str::<Placement>(
                r#"{"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0],"extra":1}"#
            )
            .is_err()
        );
    }

    #[test]
    fn panel_size_accepts_positive_dimensions() {
        let size = PanelSize::new(millimetres(700), millimetres(500), millimetres(18)).expect("valid panel size");
        assert_eq!(size.length(), millimetres(700));
        assert_eq!(size.width(), millimetres(500));
        assert_eq!(size.thickness(), millimetres(18));
    }

    #[test]
    fn panel_size_rejects_non_positive_and_oversized_dimensions() {
        let zero = Length::ZERO;
        let negative = millimetres(-1);
        assert_eq!(
            PanelSize::new(zero, millimetres(500), millimetres(18)),
            Err(PanelSizeError::NonPositive { field: "length", value: zero })
        );
        assert_eq!(
            PanelSize::new(millimetres(700), negative, millimetres(18)),
            Err(PanelSizeError::NonPositive { field: "width", value: negative })
        );
        assert_eq!(
            PanelSize::new(millimetres(700), millimetres(500), negative),
            Err(PanelSizeError::NonPositive { field: "thickness", value: negative })
        );

        let too_long = Length::from_micrometres(PanelSize::MAX_LENGTH.micrometres() + 1);
        assert_eq!(
            PanelSize::new(too_long, millimetres(500), millimetres(18)),
            Err(PanelSizeError::TooLarge { field: "length", value: too_long, max: PanelSize::MAX_LENGTH })
        );

        let too_wide = Length::from_micrometres(PanelSize::MAX_WIDTH.micrometres() + 1);
        assert_eq!(
            PanelSize::new(millimetres(700), too_wide, millimetres(18)),
            Err(PanelSizeError::TooLarge { field: "width", value: too_wide, max: PanelSize::MAX_WIDTH })
        );

        let too_thick = Length::from_micrometres(PanelSize::MAX_THICKNESS.micrometres() + 1);
        assert_eq!(
            PanelSize::new(millimetres(700), millimetres(500), too_thick),
            Err(PanelSizeError::TooLarge { field: "thickness", value: too_thick, max: PanelSize::MAX_THICKNESS })
        );

        assert!(PanelSize::new(PanelSize::MAX_LENGTH, PanelSize::MAX_WIDTH, PanelSize::MAX_THICKNESS).is_ok());
    }

    #[test]
    fn panel_size_serde_round_trip_and_validation() {
        let size = PanelSize::new(millimetres(700), millimetres(500), millimetres(18)).expect("valid panel size");
        let text = serde_json::to_string(&size).expect("serialize panel size");
        assert_eq!(text, r#"{"length":700000,"width":500000,"thickness":18000}"#);
        assert_eq!(serde_json::from_str::<PanelSize>(&text).expect("deserialize panel size"), size);

        assert!(serde_json::from_str::<PanelSize>(r#"{"length":0,"width":500000,"thickness":18000}"#).is_err());
        assert!(serde_json::from_str::<PanelSize>(r#"{"length":700000,"width":500000,"thickness":-1}"#).is_err());
        assert!(serde_json::from_str::<PanelSize>(r#"{"length":10000001,"width":500000,"thickness":18000}"#).is_err());
        assert!(
            serde_json::from_str::<PanelSize>(r#"{"length":700000,"width":500000,"thickness":18000,"unexpected":1}"#)
                .is_err()
        );
    }

    #[test]
    fn ids_reject_zero_and_serialize_as_numbers() {
        assert!(VertexId::new(0).is_none());
        assert!(EdgeId::new(0).is_none());
        assert!(serde_json::from_str::<VertexId>("0").is_err());
        assert!(serde_json::from_str::<EdgeId>("0").is_err());

        let id = EdgeId::new(3).expect("non-zero edge id");
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "3");
        let decoded: EdgeId = serde_json::from_str("3").expect("deserialize id");
        assert_eq!(decoded, id);
    }
}
