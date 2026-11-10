//! Rigid placements, manipulation pivots and the Euler-angle view used by the inspector.
//!
//! A placement is translation plus rotation, never scale or shear, and the rotation is a unit
//! quaternion. This module is the only place where placement coordinates meet floating-point
//! millimetres: every persisted length is a [`Length`] in micrometres and every angle is a
//! [`Degrees`] in millidegrees.

use furnicraft_geom::{DQuat, DVec3, EulerRot};
use furnicraft_units::{Degrees, Length};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Intrinsic XYZ Euler angles in degrees, the inspector's view of a placement rotation.
///
/// The document stores a unit quaternion; Euler angles are derived on read and never serialized.
/// Each angle is in millidegrees, so text entered in the inspector round-trips exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EulerAngles {
    /// Rotation about the local X axis, in degrees.
    pub x: Degrees,
    /// Rotation about the local Y axis, in degrees.
    pub y: Degrees,
    /// Rotation about the local Z axis, in degrees.
    pub z: Degrees,
}

impl EulerAngles {
    /// The axis order used by [`Placement::rotation_euler`] and [`Placement::with_rotation_euler`].
    pub const ORDER_LABEL: &'static str = "XYZ";

    /// Wraps each angle into `(-180°, 180°]`, the principal range the inspector displays.
    ///
    /// A full turn about one axis is the identity, so wrapping each angle does not change the
    /// rotation the triple describes.
    pub fn wrapped(self) -> Self {
        Self { x: wrap_degrees(self.x), y: wrap_degrees(self.y), z: wrap_degrees(self.z) }
    }
}

fn wrap_degrees(degrees: Degrees) -> Degrees {
    let wrapped = degrees.millidegrees().rem_euclid(360_000);
    Degrees::from_millidegrees(if wrapped > 180_000 { wrapped - 360_000 } else { wrapped })
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

    /// Inverts a rigid frame, so `compose(frame, frame.inverse())` is the identity.
    ///
    /// The inverse of a rigid transform is its transposed rotation and the negated rotated origin:
    /// the conjugate quaternion and `-R⁻¹t`. `inverse(parent) ∘ world` is the world frame expressed
    /// in the parent's frame, the missing half of [`Self::compose`]: it is how a shell that knows only
    /// a world placement hands a command the parent-relative placement the document stores.
    pub fn inverse(self) -> Result<Self, PlacementError> {
        let rotation = self.rotation.conjugate();
        let translation = rotation * -self.translation;
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

    /// The translation as exact micrometre lengths, rounded to the nearest micrometre.
    ///
    /// This is the boundary conversion for editing a placement as integer lengths. It returns
    /// `None` when a component is NaN, infinite or outside the range of [`Length`].
    pub fn translation_lengths(self) -> Option<[Length; 3]> {
        lengths_from_millimetres(self.translation)
    }

    /// Replaces the translation from exact lengths, keeping the rotation.
    ///
    /// This converts to millimetres at the geometry boundary; callers keep placement coordinates
    /// as [`Length`] until this point.
    pub fn with_translation(self, x: Length, y: Length, z: Length) -> Result<Self, PlacementError> {
        Self::new(DVec3::new(x.millimetres_f64(), y.millimetres_f64(), z.millimetres_f64()), self.rotation)
    }

    /// The rotation as intrinsic XYZ Euler angles, rounded to the nearest millidegree.
    ///
    /// Euler angles are a presentation of the stored unit quaternion; the quaternion remains the
    /// source of truth. At Y = ±90° the X and Z angles are not unique.
    pub fn rotation_euler(self) -> EulerAngles {
        let (x, y, z) = self.rotation.to_euler(EulerRot::XYZ);
        EulerAngles {
            x: Degrees::from_radians(x).unwrap_or_default(),
            y: Degrees::from_radians(y).unwrap_or_default(),
            z: Degrees::from_radians(z).unwrap_or_default(),
        }
        .wrapped()
    }

    /// Replaces the rotation from intrinsic XYZ Euler angles, keeping the translation.
    ///
    /// The quaternion is re-derived from the angles, so values finer than a millidegree are lost;
    /// this is the write side of the inspector. The angles are wrapped to `(-180°, 180°]` first.
    pub fn with_rotation_euler(self, angles: EulerAngles) -> Result<Self, PlacementError> {
        let angles = angles.wrapped();
        let rotation =
            DQuat::from_euler(EulerRot::XYZ, angles.x.radians_f64(), angles.y.radians_f64(), angles.z.radians_f64())
                .normalize();
        Self::new(self.translation, rotation)
    }

    /// Whether two placements describe the same rigid transform.
    ///
    /// Quaternions `q` and `-q` are the same rotation, so a sign-only difference is not a change.
    /// The translation must match exactly; the rotation tolerance is well below the inspector's
    /// millidegree display precision.
    pub fn same_transform(self, other: Self) -> bool {
        self.translation == other.translation && self.rotation.dot(other.rotation).abs() >= 1.0 - 1.0e-12
    }
}

/// Rounds a millimetre point to exact micrometre lengths.
///
/// Returns `None` when a component is NaN, infinite or outside the range of [`Length`]. This is the
/// geometry-boundary conversion used by placement and pivot editing and by a frame anchored on a
/// mixed collection of children.
pub(crate) fn lengths_from_millimetres(point: DVec3) -> Option<[Length; 3]> {
    fn to_length(value: f64) -> Option<Length> {
        if !value.is_finite() {
            return None;
        }
        let micrometres = (value * 1_000.0).round();
        if !(micrometres >= i64::MIN as f64 && micrometres < i64::MAX as f64) {
            return None;
        }
        Some(Length::from_micrometres(micrometres as i64))
    }
    Some([to_length(point.x)?, to_length(point.y)?, to_length(point.z)?])
}

/// The manipulation pivot of a node, in that node's local frame.
///
/// The pivot is stored document data rather than a value derived from the current geometry: it is
/// created once with a sensible default, then moves and turns with the node and survives resizing,
/// adding or removing children, and save/load. Coordinates are integer micrometres like every other
/// persisted length; [`Pivot::to_local`] is the geometry-boundary view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pivot([Length; 3]);

impl Pivot {
    /// The local origin.
    pub const ZERO: Self = Self([Length::ZERO; 3]);

    /// Builds a pivot from exact micrometre lengths.
    pub const fn new(x: Length, y: Length, z: Length) -> Self {
        Self([x, y, z])
    }

    /// Rounds a millimetre point into a pivot, or `None` when it is not representable.
    pub fn from_local(point: DVec3) -> Option<Self> {
        Some(Self(lengths_from_millimetres(point)?))
    }

    /// The three components in micrometres.
    pub const fn components(self) -> [Length; 3] {
        self.0
    }

    /// The pivot in millimetres for geometry and rendering.
    pub fn to_local(self) -> DVec3 {
        DVec3::new(self.0[0].millimetres_f64(), self.0[1].millimetres_f64(), self.0[2].millimetres_f64())
    }

    /// Whether the pivot is the node's local origin.
    pub const fn is_zero(self) -> bool {
        self.0[0].is_zero() && self.0[1].is_zero() && self.0[2].is_zero()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn inverse_undoes_compose_and_maps_a_world_point_back_to_its_local_one() {
        let parent = Placement::new(DVec3::new(120.0, -40.0, 900.0), DQuat::from_rotation_z(0.7)).expect("parent");
        let child = Placement::new(DVec3::new(15.0, 2.5, -3.0), DQuat::from_rotation_x(-0.4)).expect("child");
        let world = Placement::compose(parent, child).expect("world placement");
        let parent_inverse = parent.inverse().expect("inverse parent");

        let back = Placement::compose(parent_inverse, world).expect("parent-frame placement");
        assert!((back.translation() - child.translation()).length() < 1e-9, "was {back:?}, expected {child:?}");
        assert!(back.rotation().dot(child.rotation()).abs() >= 1.0 - 1e-12, "the rotation came back as {back:?}");
        let identity = Placement::compose(parent, parent_inverse).expect("identity");
        assert!(identity.translation().length() < 1e-9, "a frame composed with its inverse is the identity");
        assert!(identity.rotation().dot(DQuat::IDENTITY).abs() >= 1.0 - 1e-12);

        let local = DVec3::new(1.5, -2.0, 0.25);
        let round_trip = world.inverse().expect("inverse world").transform_point(world.transform_point(local));
        assert!((round_trip - local).length() < 1e-9, "the point came back as {round_trip:?}");
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
    fn placements_round_trip_through_exact_translation_lengths() {
        let placement =
            Placement::new(DVec3::new(-2.5, 0.001, 1200.0), DQuat::from_rotation_y(0.5)).expect("placement");
        let [x, y, z] = placement.translation_lengths().expect("representable translation");
        assert_eq!(x, Length::from_micrometres(-2_500));
        assert_eq!(y, Length::from_micrometres(1));
        assert_eq!(z, Length::from_micrometres(1_200_000));

        let edited = placement.with_translation(x, y, z).expect("rebuild placement");
        assert_eq!(edited, placement);
        assert_eq!(edited.rotation(), placement.rotation());

        let moved = placement
            .with_translation(Length::from_millimetres(10).expect("length"), Length::ZERO, Length::ZERO)
            .expect("move placement");
        assert_eq!(moved.translation(), DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.rotation(), placement.rotation());
    }

    #[test]
    fn translation_lengths_reject_values_outside_the_editable_range() {
        let huge = Placement::new(DVec3::new(1e300, 0.0, 0.0), DQuat::IDENTITY).expect("finite placement");
        assert!(huge.translation_lengths().is_none());
        assert!(Placement::IDENTITY.translation_lengths().is_some());
    }

    #[test]
    fn identity_rotation_has_zero_euler_angles() {
        let angles = Placement::IDENTITY.rotation_euler();
        assert_eq!(angles, EulerAngles::default());
        assert_eq!(angles.x, Degrees::ZERO);
        assert_eq!(angles.y, Degrees::ZERO);
        assert_eq!(angles.z, Degrees::ZERO);
    }

    #[test]
    fn euler_angles_use_intrinsic_xyz_order() {
        let rotation = DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)
            * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_4)
            * DQuat::from_rotation_z(std::f64::consts::FRAC_PI_8);
        let placement = Placement::new(DVec3::ZERO, rotation).expect("placement");
        let angles = placement.rotation_euler();
        assert_eq!(angles.x, Degrees::from_millidegrees(90_000));
        assert_eq!(angles.y, Degrees::from_millidegrees(45_000));
        assert_eq!(angles.z, Degrees::from_millidegrees(22_500));
    }

    #[test]
    fn euler_angles_round_trip_and_preserve_translation() {
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let angles = placement.rotation_euler();
        let rebuilt = placement.with_rotation_euler(angles).expect("rebuild rotation");
        assert_eq!(rebuilt.translation(), placement.translation());
        assert!(rebuilt.rotation().dot(placement.rotation()).abs() > 1.0 - 1.0e-9);

        let turned = placement
            .with_rotation_euler(EulerAngles {
                x: Degrees::from_millidegrees(30_000),
                y: Degrees::from_millidegrees(-15_500),
                z: Degrees::from_millidegrees(90_000),
            })
            .expect("turn placement");
        assert_eq!(turned.translation(), placement.translation());
        assert!((turned.rotation().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn euler_angles_at_gimbal_lock_recompose_the_same_rotation() {
        let rotation = DQuat::from_rotation_x(0.4)
            * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2)
            * DQuat::from_rotation_z(0.7);
        let placement = Placement::new(DVec3::ZERO, rotation).expect("placement");
        let angles = placement.rotation_euler();
        let rebuilt = placement.with_rotation_euler(angles).expect("rebuild rotation");
        assert!(rebuilt.rotation().dot(placement.rotation()).abs() > 1.0 - 1.0e-9);
    }

    #[test]
    fn euler_angles_wrap_full_turns_into_the_principal_range() {
        let full_turn = EulerAngles {
            x: Degrees::from_millidegrees(360_000),
            y: Degrees::from_millidegrees(-360_000),
            z: Degrees::from_millidegrees(720_000),
        };
        assert_eq!(full_turn.wrapped(), EulerAngles::default());
        let placement = Placement::IDENTITY.with_rotation_euler(full_turn).expect("wrap placement");
        assert!(placement.same_transform(Placement::IDENTITY));

        let wrapped = EulerAngles {
            x: Degrees::from_millidegrees(540_000),
            y: Degrees::from_millidegrees(-181_000),
            z: Degrees::from_millidegrees(180_000),
        }
        .wrapped();
        assert_eq!(wrapped.x, Degrees::from_millidegrees(180_000));
        assert_eq!(wrapped.y, Degrees::from_millidegrees(179_000));
        assert_eq!(wrapped.z, Degrees::from_millidegrees(180_000));
    }

    #[test]
    fn same_transform_ignores_the_quaternion_sign_and_rejects_real_changes() {
        let rotation = DQuat::from_rotation_z(0.3);
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), rotation).expect("placement");
        let flipped = Placement::new(DVec3::new(1.0, 2.0, 3.0), -rotation).expect("flipped placement");
        assert!(placement.same_transform(flipped));

        let moved = Placement::new(DVec3::new(1.0, 2.0, 4.0), rotation).expect("moved placement");
        assert!(!placement.same_transform(moved));
        let turned = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.31)).expect("turned placement");
        assert!(!placement.same_transform(turned));
    }
}
