//! The orbit camera, its projection and the basis a standard view uses.
//!
//! A camera looks at a target from a point on a sphere given by yaw and pitch; it is view state,
//! never document data, and an authored camera node pins it to an explicit basis instead (ADR 0029).
//!
//! The contract is millimetres and radians: the target, `distance`, the orthographic visible height
//! and the near and far planes are millimetres, the yaw, pitch, roll and field of view radians. The
//! near plane starts at 1 mm and the far plane is [`MAX_DISTANCE_MM`], both along the view axis, so
//! [`Camera::project_ndc`] refuses a point at or behind the near plane. Navigation is total: a
//! hostile delta is ignored, the pitch stays inside [`MIN_PITCH`]..=[`MAX_PITCH`], the distance
//! inside [`MIN_DISTANCE_MM`]..=[`MAX_DISTANCE_MM`], and the yaw and roll wrap into `0..TAU`.
//!
//! It refuses a non-finite target or a distance outside the supported range ([`CameraError`]), and
//! it refuses to move a camera pinned to an authored basis with [`Camera::looked_from`]: orbit,
//! pan, zoom and roll leave that camera alone.
//!
//! It does not own the frame, the grid or the picking result: [`Camera::ray_ndc`] produces a
//! [`Ray`], and picking itself lives in [`crate::ray`].
//!
//! The surface is over B6's 25 public items because it is one type's: a [`Camera`]'s constructor,
//! accessors, navigation calls and projection conversions change together with [`Projection`], and
//! every one of them is a question about the same value.
//!
//! Changes when: the orbit model, the projection or the authored basis changes.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, PI, TAU};

use furnicraft_geom::{DMat4, DVec2, DVec3};
use thiserror::Error;

use crate::ray::Ray;

/// The default vertical field of view, in radians (45 degrees).
pub const DEFAULT_FOV_Y: f64 = FRAC_PI_4;

/// Lowest camera pitch, in radians: a bottom view looks straight up the view plane.
pub const MIN_PITCH: f64 = -FRAC_PI_2;

/// Highest camera pitch, in radians: a top view looks straight down the view plane.
pub const MAX_PITCH: f64 = FRAC_PI_2;

/// How far outside the viewport a pointer ray is still accepted, in normalized device coordinates.
///
/// A drag can leave the viewport, so a little beyond ±1 is valid. The limit keeps a hostile or
/// accidental huge coordinate from turning the ray into an unusable vector.
const NDC_LIMIT: f64 = 16.0;
/// Below this horizontal length an eye direction counts as a pole, in millimetres.
///
/// A pole is a top or bottom view: the direction is parallel to `Z`, so it does not say which way
/// is up. The bound matches the one [`Camera::look_from`] uses to leave the yaw alone.
pub const POLE_TOLERANCE: f64 = 1.0e-9;

/// The canonical up of a standard view, as a world direction.
///
/// A top or bottom view looks along `±Z`, where a `+Z` up is parallel to the view and undefined, so
/// the elevation takes `+Y` up; every other view keeps `+Z` up. The shell passes this to
/// [`Camera::look_from_oriented`], so clicking the orientation widget and choosing a preset land on
/// the same basis.
pub fn canonical_up(direction: DVec3) -> DVec3 {
    if direction.x.abs() <= POLE_TOLERANCE && direction.y.abs() <= POLE_TOLERANCE { DVec3::Y } else { DVec3::Z }
}

/// Closest camera distance from its target, in millimetres.
pub const MIN_DISTANCE_MM: f64 = 10.0;

/// Farthest camera distance from its target, in millimetres.
pub const MAX_DISTANCE_MM: f64 = 1.0e6;

/// A reason why a camera could not be constructed.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum CameraError {
    /// The orbit target contains a NaN or an infinity.
    #[error("camera target must be finite")]
    NonFiniteTarget,
    /// The distance is outside the supported range.
    #[error("camera distance must be finite and between 10 and 1,000,000 mm, got {distance}")]
    InvalidDistance {
        /// The rejected distance.
        distance: f64,
    },
}

/// How a camera projects the scene onto the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// A perspective view with a vertical field of view in radians.
    Perspective {
        /// Vertical field of view in radians, strictly between zero and pi.
        fov_y: f64,
    },
    /// An orthographic view with a fixed visible height.
    Orthographic {
        /// Visible height in millimetres.
        height: f64,
    },
}

impl Projection {
    /// Whether the parameters are finite and inside their accepted range.
    pub fn is_valid(self) -> bool {
        match self {
            Self::Perspective { fov_y } => fov_y.is_finite() && fov_y > 1.0e-6 && fov_y < PI - 1.0e-6,
            Self::Orthographic { height } => height.is_finite() && height > 0.0,
        }
    }
}

/// An explicit view basis supplied by an authored camera node.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FixedBasis {
    eye: DVec3,
    right: DVec3,
    up: DVec3,
    forward: DVec3,
}

/// An orbit camera in a right-handed, Z-up world.
///
/// The camera looks at `target` from a point on a sphere described by `yaw` (around world Z) and
/// `pitch` (above the XY plane). A `roll` turns the view about its own axis without moving the eye,
/// the target or the forward direction. Physical lengths are millimetres; the camera lives in view
/// state, never in the document. An authored camera pins an explicit basis with
/// [`Camera::looked_from`]; orbit, pan, zoom and `look_from` then leave it alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    target: DVec3,
    yaw: f64,
    pitch: f64,
    roll: f64,
    distance: f64,
    projection: Projection,
    /// Near plane in millimetres along the forward axis; the picking tests build clip planes from it.
    pub(crate) near: f64,
    /// Far plane in millimetres along the forward axis, [`MAX_DISTANCE_MM`] by default.
    pub(crate) far: f64,
    fixed: Option<FixedBasis>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            yaw: 2.8 * FRAC_PI_8,
            pitch: 30.0 * PI / 180.0,
            roll: 0.0,
            distance: 1200.0,
            projection: Projection::Perspective { fov_y: DEFAULT_FOV_Y },
            near: 1.0,
            far: MAX_DISTANCE_MM,
            fixed: None,
        }
    }
}

impl Camera {
    /// Creates a camera looking at `target` from `distance` millimetres away.
    pub fn new(target: DVec3, distance: f64) -> Result<Self, CameraError> {
        if !target.is_finite() {
            return Err(CameraError::NonFiniteTarget);
        }
        if !distance.is_finite() || !(MIN_DISTANCE_MM..=MAX_DISTANCE_MM).contains(&distance) {
            return Err(CameraError::InvalidDistance { distance });
        }
        Ok(Self { target, distance, ..Self::default() })
    }

    /// The point the camera orbits around.
    pub const fn target(self) -> DVec3 {
        self.target
    }

    /// The horizontal orbit angle in radians.
    pub const fn yaw(self) -> f64 {
        self.yaw
    }

    /// The vertical orbit angle in radians.
    pub const fn pitch(self) -> f64 {
        self.pitch
    }

    /// The view roll about the camera's own forward axis, in radians, in `0..TAU`.
    ///
    /// A positive roll turns the image counter-clockwise. The roll is view state like yaw and
    /// pitch, never document data (ADR 0029, 0078).
    pub const fn roll(self) -> f64 {
        self.roll
    }

    /// The distance from the target in millimetres.
    pub const fn distance(self) -> f64 {
        self.distance
    }

    /// How the camera projects the scene.
    pub const fn projection(self) -> Projection {
        self.projection
    }

    /// Pins the camera to an explicit eye position and orthonormal basis.
    ///
    /// Used for an authored camera node, whose orientation is document data rather than an orbit.
    /// Non-finite or non-orthogonal input leaves the orbit view unchanged instead of producing a
    /// broken matrix.
    pub fn looked_from(mut self, eye: DVec3, right: DVec3, up: DVec3, forward: DVec3) -> Self {
        if !eye.is_finite() || !right.is_finite() || !up.is_finite() || !forward.is_finite() {
            return self;
        }
        let (Some(forward), Some(right), Some(up)) =
            (forward.try_normalize(), right.try_normalize(), up.try_normalize())
        else {
            return self;
        };
        let orthogonal = right.dot(up).abs() < 0.01 && right.dot(forward).abs() < 0.01 && up.dot(forward).abs() < 0.01;
        if !orthogonal {
            return self;
        }
        self.fixed = Some(FixedBasis { eye, right, up, forward });
        self
    }

    /// Replaces the projection when it is valid, keeping the current one otherwise.
    pub fn with_projection(mut self, projection: Projection) -> Self {
        if projection.is_valid() {
            self.projection = projection;
        }
        self
    }

    /// Makes the view orthographic without moving it.
    ///
    /// A perspective view becomes orthographic with the height it shows at the target plane, so the
    /// image keeps its size; an orthographic view keeps its visible height. The target, orbit angles
    /// and distance are untouched. An authored camera is pinned to its document projection and is
    /// left unchanged.
    pub fn set_orthographic(&mut self) {
        if self.fixed.is_some() {
            return;
        }
        if let Projection::Perspective { fov_y } = self.projection {
            self.projection = Projection::Orthographic { height: 2.0 * self.distance * (fov_y * 0.5).tan() };
        }
    }

    /// Switches between a perspective and an orthographic view.
    ///
    /// The visible height at the target is preserved: a perspective view becomes orthographic with
    /// the height it shows at the target plane, and an orthographic view becomes perspective with
    /// the default field of view and the distance that shows the same height. An authored camera is
    /// pinned to its document projection and is left unchanged.
    pub fn toggle_projection(&mut self) {
        if self.fixed.is_some() {
            return;
        }
        match self.projection {
            Projection::Perspective { .. } => self.set_orthographic(),
            Projection::Orthographic { height } => {
                let fov_y = DEFAULT_FOV_Y;
                self.distance = (height / (2.0 * (fov_y * 0.5).tan())).clamp(MIN_DISTANCE_MM, MAX_DISTANCE_MM);
                self.projection = Projection::Perspective { fov_y };
            }
        }
    }

    /// The camera position in world space.
    pub fn eye(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.eye;
        }
        let horizontal = self.pitch.cos();
        let offset = DVec3::new(horizontal * self.yaw.cos(), horizontal * self.yaw.sin(), self.pitch.sin());
        self.target + offset * self.distance
    }

    /// The unit direction from the camera towards its target.
    pub fn forward(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.forward;
        }
        (self.target - self.eye()).try_normalize().unwrap_or(-DVec3::Z)
    }

    /// The camera's right vector in world space.
    ///
    /// The base vector depends on yaw alone, so it is the exact limit of `forward × Z` and stays
    /// unit and horizontal at both poles, where the view direction is parallel to `Z`. The roll then
    /// turns it about `forward`, so a rolled view's right vector leaves the horizon while `up`
    /// stays `right × forward` (ADR 0078).
    pub fn right(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.right;
        }
        let (sin, cos) = self.roll.sin_cos();
        let (base_right, base_up) = self.base_basis();
        base_right * cos - base_up * sin
    }

    /// The camera's up vector in world space.
    pub fn up(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.up;
        }
        let (sin, cos) = self.roll.sin_cos();
        let (base_right, base_up) = self.base_basis();
        base_right * sin + base_up * cos
    }

    /// The unrolled right and up vectors, before the roll turns them about `forward`.
    fn base_basis(self) -> (DVec3, DVec3) {
        let right = DVec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0);
        (right, right.cross(self.forward()))
    }

    /// The world-to-view matrix.
    pub fn view_matrix(self) -> DMat4 {
        let eye = self.eye();
        let forward = self.forward();
        let right = self.right();
        let up = self.up();
        DMat4::from_cols_array(&[
            right.x,
            up.x,
            -forward.x,
            0.0,
            right.y,
            up.y,
            -forward.y,
            0.0,
            right.z,
            up.z,
            -forward.z,
            0.0,
            -right.dot(eye),
            -up.dot(eye),
            forward.dot(eye),
            1.0,
        ])
    }

    /// The projection matrix for a viewport with the given aspect ratio.
    ///
    /// The matrix matches wgpu's clip-space convention: right-handed view space, depth in `0..1`.
    /// A broken aspect ratio falls back to `1.0` instead of producing NaNs.
    pub fn projection_matrix(self, aspect: f64) -> DMat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(1.0e-4, 1.0e4) } else { 1.0 };
        match self.projection {
            Projection::Perspective { fov_y } => {
                let f = 1.0 / (fov_y * 0.5).tan();
                DMat4::from_cols_array(&[
                    f / aspect,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    f,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    self.far / (self.near - self.far),
                    -1.0,
                    0.0,
                    0.0,
                    self.far * self.near / (self.near - self.far),
                    0.0,
                ])
            }
            Projection::Orthographic { height } => {
                let half_height = (height.max(1.0e-6)) * 0.5;
                let half_width = half_height * aspect;
                DMat4::from_cols_array(&[
                    1.0 / half_width,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0 / half_height,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0 / (self.near - self.far),
                    0.0,
                    0.0,
                    0.0,
                    self.near / (self.near - self.far),
                    1.0,
                ])
            }
        }
    }

    /// The combined view-projection matrix.
    pub fn view_projection(self, aspect: f64) -> DMat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// Orbits the camera by the given deltas in radians.
    ///
    /// Yaw wraps into `0..TAU`; pitch is clamped to the poles, where a yaw delta still turns the
    /// view about the vertical axis. Non-finite deltas are ignored. A camera pinned to an authored
    /// view does not move.
    pub fn orbit(&mut self, delta_yaw: f64, delta_pitch: f64) {
        if self.fixed.is_some() || !delta_yaw.is_finite() || !delta_pitch.is_finite() {
            return;
        }
        self.yaw = (self.yaw + delta_yaw).rem_euclid(TAU);
        self.pitch = (self.pitch + delta_pitch).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Sets the view roll about the camera's own forward axis, in radians.
    ///
    /// The angle wraps into `0..TAU`; non-finite input is ignored, and a camera pinned to an
    /// authored view keeps its own basis and ignores the call, exactly as [`Camera::orbit`] does.
    pub fn roll_to(&mut self, roll: f64) {
        if self.fixed.is_some() || !roll.is_finite() {
            return;
        }
        self.roll = roll.rem_euclid(TAU);
    }

    /// Turns the view about its own forward axis by `delta` radians.
    ///
    /// Non-finite input is ignored and a pinned camera does not move; a multi-turn delta is
    /// normalised into `0..TAU`.
    pub fn roll_by(&mut self, delta: f64) {
        if !delta.is_finite() {
            return;
        }
        self.roll_to(self.roll + delta);
    }

    /// A world-space ray through normalized device coordinates.
    ///
    /// `(-1, -1)` is the bottom-left corner of the viewport and `(1, 1)` the top-right. Coordinates
    /// just outside the viewport are valid, so a drag can leave the viewport; extreme values are
    /// clamped to a sane range and non-finite coordinates fall back to the centre ray. The
    /// direction is unit length.
    pub fn ray_ndc(self, ndc_x: f64, ndc_y: f64, aspect: f64) -> Ray {
        let x = if ndc_x.is_finite() { ndc_x.clamp(-NDC_LIMIT, NDC_LIMIT) } else { 0.0 };
        let y = if ndc_y.is_finite() { ndc_y.clamp(-NDC_LIMIT, NDC_LIMIT) } else { 0.0 };
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        let (origin, direction) = match self.projection {
            Projection::Perspective { fov_y } => {
                let half_height = (fov_y * 0.5).tan();
                (self.eye(), self.forward() + self.right() * (x * half_height * aspect) + self.up() * (y * half_height))
            }
            Projection::Orthographic { height } => {
                let half_height = (height.max(1.0e-6)) * 0.5;
                let half_width = half_height * aspect;
                (self.eye() + self.right() * (x * half_width) + self.up() * (y * half_height), self.forward())
            }
        };
        Ray::new(origin, direction).unwrap_or(Ray { origin, direction: self.forward() })
    }

    /// Projects a world point to normalized device coordinates.
    ///
    /// `(-1, -1)` is the bottom-left corner of the viewport and `(1, 1)` the top-right. Returns
    /// `None` for a non-finite point or a point at or behind the camera plane. A broken aspect
    /// ratio is treated as 1.
    pub fn project_ndc(self, point: DVec3, aspect: f64) -> Option<DVec2> {
        if !point.is_finite() {
            return None;
        }
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        // An orthographic view has no perspective divide, so a point behind the camera still has a
        // finite clip position. The view-space depth rejects it explicitly.
        if matches!(self.projection, Projection::Orthographic { .. }) {
            let view = self.view_matrix() * point.extend(1.0);
            if !view.is_finite() || view.z > -self.near {
                return None;
            }
        }
        let clip = self.view_projection(aspect) * point.extend(1.0);
        if !clip.is_finite() || clip.w <= 1.0e-9 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let ndc = DVec2::new(ndc.x, ndc.y);
        ndc.is_finite().then_some(ndc)
    }

    /// World millimetres covered by one viewport point at `depth` millimetres from the eye.
    ///
    /// A perspective view widens with depth; an orthographic one does not, so its value is the
    /// same everywhere. Broken input yields zero: a tolerance built from it can then only accept an
    /// exact hit instead of rejecting a valid one.
    pub fn world_per_point(self, depth: f64, viewport_height: f64) -> f64 {
        if !depth.is_finite() || !viewport_height.is_finite() || viewport_height <= 0.0 {
            return 0.0;
        }
        match self.projection {
            Projection::Perspective { fov_y } => 2.0 * depth.max(0.0) * (fov_y * 0.5).tan() / viewport_height,
            Projection::Orthographic { height } => height.max(1.0e-6) / viewport_height,
        }
    }

    /// Pans the target across the screen plane, in points.
    ///
    /// The world distance per point is measured at the target, so the scene follows the pointer.
    /// Non-finite input is ignored.
    pub fn pan_pixels(&mut self, delta_x: f64, delta_y: f64, viewport_height: f64) {
        if self.fixed.is_some() || !delta_x.is_finite() || !delta_y.is_finite() || !viewport_height.is_finite() {
            return;
        }
        let height = viewport_height.max(1.0);
        let world_per_point = match self.projection {
            Projection::Perspective { fov_y } => 2.0 * self.distance * (fov_y * 0.5).tan() / height,
            Projection::Orthographic { height: visible } => visible.max(1.0e-6) / height,
        };
        self.target += (-self.right() * delta_x + self.up() * delta_y) * world_per_point;
    }

    /// Points the camera at its target from the given world direction.
    ///
    /// `direction` is the offset from the target to the eye, not the viewing direction. A zero or
    /// non-finite direction leaves the camera unchanged. Pitch is clamped to the poles, so a top or
    /// bottom view looks straight down or up the view plane; at a pole the direction alone does not
    /// fix yaw, so the current yaw is kept. A valid direction resets the roll: a standard view is
    /// upright (ADR 0078).
    pub fn look_from(&mut self, direction: DVec3) {
        if self.fixed.is_some() || !direction.is_finite() {
            return;
        }
        let Some(direction) = direction.try_normalize() else {
            return;
        };
        if direction.x.hypot(direction.y) > POLE_TOLERANCE {
            self.yaw = direction.y.atan2(direction.x).rem_euclid(TAU);
        }
        self.pitch = direction.z.clamp(MIN_PITCH.sin(), MAX_PITCH.sin()).asin();
        self.roll = 0.0;
    }

    /// Points the camera at its target from `direction`, taking the roll at a pole from `up`.
    ///
    /// A non-pole direction fixes the yaw on its own, so `up` is ignored there and the rule stays
    /// the one [`Camera::look_from`] uses. A top or bottom view cannot be rolled by its direction:
    /// the yaw comes from the world direction `up` should point along, so the camera's own up is that
    /// direction projected onto the horizontal plane. A zero or non-finite `direction` leaves the
    /// camera unchanged; a `up` parallel to the view direction has no roll to offer, so the current
    /// yaw stays.
    pub fn look_from_oriented(&mut self, direction: DVec3, up: DVec3) {
        self.look_from(direction);
        if self.fixed.is_some() || !direction.is_finite() || !up.is_finite() {
            return;
        }
        let Some(direction) = direction.try_normalize() else {
            return;
        };
        if direction.x.hypot(direction.y) > POLE_TOLERANCE {
            return;
        }
        // `right = forward × up`, and `right = (-sin yaw, cos yaw, 0)`, so the requested up names the
        // one yaw that puts the camera's own up on it.
        let Some(right) = (-direction).cross(up).try_normalize() else {
            return;
        };
        if right.x.hypot(right.y) <= POLE_TOLERANCE {
            return;
        }
        self.yaw = (-right.x).atan2(right.y).rem_euclid(TAU);
    }

    /// Scales the orbit distance by `factor`.
    ///
    /// A factor below one moves closer; values outside the supported range are clamped and invalid
    /// factors are ignored. An orthographic view scales its visible height with the distance, so
    /// zooming works there too and a later toggle back shows the same frame.
    pub fn zoom(&mut self, factor: f64) {
        if self.fixed.is_some() || !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let previous = self.distance;
        self.distance = (previous * factor).clamp(MIN_DISTANCE_MM, MAX_DISTANCE_MM);
        if let Projection::Orthographic { height } = self.projection {
            let scaled = height * (self.distance / previous);
            if scaled.is_finite() && scaled > 0.0 {
                self.projection = Projection::Orthographic { height: scaled };
            }
        }
    }
}
