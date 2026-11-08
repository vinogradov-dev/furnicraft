//! Renderer-agnostic scene description: what a frame contains.
//!
//! Drawables, world transforms, colours, camera and lights are plain data, so a scene can be
//! built and tested without a GPU. The renderer owns the GPU resources and interprets this
//! description; the document never stores a camera, a grid or a drawable.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, PI, TAU};
use std::sync::Arc;

use furnicraft_geom::{DMat4, DQuat, DVec2, DVec3, MeshData};
use furnicraft_model::Id;
use thiserror::Error;

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

/// Pick radius around an unlit line chain, in viewport points.
///
/// A path is drawn one pixel wide, so a ray rarely crosses it exactly; this radius makes a click
/// near the line select the path without making a whole viewport region its target.
pub const LINE_PICK_RADIUS_POINTS: f64 = 6.0;

/// Gamma-space colour of a selected line chain.
///
/// A tint mixed into the path colour moves too little to be seen, and the line pipeline cannot
/// widen a chain, so the selected chain takes this explicit colour and the shell repeats it as a
/// thick screen-space outline. Cyan stays visible over a brown panel, a light one and the yellow
/// unselected path.
pub const SELECTION_LINE_COLOR: [f32; 4] = [0.15, 0.85, 1.0, 1.0];

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
/// `pitch` (above the XY plane). Physical lengths are millimetres; the camera lives in view state,
/// never in the document. An authored camera pins an explicit basis with [`Camera::looked_from`];
/// orbit, pan, zoom and `look_from` then leave it alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    target: DVec3,
    yaw: f64,
    pitch: f64,
    distance: f64,
    projection: Projection,
    near: f64,
    far: f64,
    fixed: Option<FixedBasis>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            yaw: 2.8 * FRAC_PI_8,
            pitch: 30.0 * PI / 180.0,
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
        self.projection = match self.projection {
            Projection::Perspective { fov_y } => {
                Projection::Orthographic { height: 2.0 * self.distance * (fov_y * 0.5).tan() }
            }
            Projection::Orthographic { height } => {
                let fov_y = DEFAULT_FOV_Y;
                self.distance = (height / (2.0 * (fov_y * 0.5).tan())).clamp(MIN_DISTANCE_MM, MAX_DISTANCE_MM);
                Projection::Perspective { fov_y }
            }
        };
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
    /// It depends on yaw alone, so it is the exact limit of `forward × Z` and stays unit and
    /// horizontal at both poles, where the view direction is parallel to `Z`.
    pub fn right(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.right;
        }
        DVec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0)
    }

    /// The camera's up vector in world space.
    pub fn up(self) -> DVec3 {
        if let Some(fixed) = self.fixed {
            return fixed.up;
        }
        self.right().cross(self.forward())
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
    /// fix yaw, so the current yaw is kept.
    pub fn look_from(&mut self, direction: DVec3) {
        if self.fixed.is_some() || !direction.is_finite() {
            return;
        }
        let Some(direction) = direction.try_normalize() else {
            return;
        };
        if direction.x.hypot(direction.y) > 1.0e-9 {
            self.yaw = direction.y.atan2(direction.x).rem_euclid(TAU);
        }
        self.pitch = direction.z.clamp(MIN_PITCH.sin(), MAX_PITCH.sin()).asin();
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

/// A world-space ray used for picking.
///
/// The direction is unit length. Construction returns `None` for non-finite input or a zero
/// direction, so a ray that reaches the scene is always usable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    origin: DVec3,
    direction: DVec3,
}

impl Ray {
    /// Creates a ray, normalising the direction.
    pub fn new(origin: DVec3, direction: DVec3) -> Option<Self> {
        if !origin.is_finite() || !direction.is_finite() {
            return None;
        }
        let direction = direction.try_normalize()?;
        Some(Self { origin, direction })
    }

    /// The ray start point in world space.
    pub const fn origin(self) -> DVec3 {
        self.origin
    }

    /// The unit direction in world space.
    pub const fn direction(self) -> DVec3 {
        self.direction
    }

    /// The parameter along a line of the point closest to this ray.
    ///
    /// The line is `line_origin + parameter * line_direction`; the direction is normalized here.
    /// Returns `None` for non-finite input, a zero direction or a ray parallel to the line, where
    /// the closest point is not unique. This is the axis-drag primitive: the difference between two
    /// parameters is how far the pointer moved along the axis.
    pub fn closest_parameter_on_line(self, line_origin: DVec3, line_direction: DVec3) -> Option<f64> {
        if !line_origin.is_finite() || !line_direction.is_finite() {
            return None;
        }
        let line_direction = line_direction.try_normalize()?;
        let offset = self.origin - line_origin;
        let along = self.direction.dot(line_direction);
        let denominator = 1.0 - along * along;
        if denominator.abs() < 1.0e-9 {
            return None;
        }
        let parameter = (line_direction.dot(offset) - along * self.direction.dot(offset)) / denominator;
        parameter.is_finite().then_some(parameter)
    }
}

/// How the renderer shades the drawables of a frame.
///
/// The value is frame data, not document data: a viewport chooses it and the settings file remembers
/// the default, but a `.fcs` never carries it (ADR 0029, ADR 0070). The renderer turns each style
/// into pipelines; [`ShadingStyle`] only promises which passes are on, so a frame can be tested
/// without a GPU.
///
/// [ADR 0029]: ../docs/decisions/0029-view-state-belongs-to-the-session.md
/// [ADR 0070]: ../docs/decisions/0070-display-styles-are-frame-data.md
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadingStyle {
    /// Shaded faces with the deduplicated mesh edges drawn over them.
    #[default]
    ShadedWithEdges,
    /// Shaded faces with no edge pass.
    Shaded,
    /// The edge pass alone, with no faces.
    Wireframe,
    /// Edges drawn through the faces, so hidden geometry stays visible.
    ShadedWithHiddenEdges,
    /// Blended faces with no depth write, drawn back to front by object centre.
    Translucent,
}

impl ShadingStyle {
    /// Every style, in the order the viewport cluster draws its buttons.
    pub const ALL: [Self; 5] =
        [Self::ShadedWithEdges, Self::Shaded, Self::Wireframe, Self::ShadedWithHiddenEdges, Self::Translucent];

    /// Whether the style draws filled faces.
    pub const fn draws_faces(self) -> bool {
        !matches!(self, Self::Wireframe)
    }

    /// Whether the style draws the mesh edge pass.
    pub const fn draws_edges(self) -> bool {
        !matches!(self, Self::Shaded | Self::Translucent)
    }

    /// Whether the edge pass ignores the depth test, drawing edges through the faces.
    pub const fn edges_through_faces(self) -> bool {
        matches!(self, Self::ShadedWithHiddenEdges)
    }

    /// Whether the faces are blended and write no depth.
    pub const fn is_translucent(self) -> bool {
        matches!(self, Self::Translucent)
    }
}

/// A floor grid on the world XY plane.
///
/// The renderer derives line geometry from these values; the document knows nothing about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Whether the grid is drawn at all. A hidden grid costs no vertices.
    pub visible: bool,
    /// Distance between adjacent minor lines, in millimetres.
    pub spacing_mm: f64,
    /// Half-size of the square grid around the world origin, in millimetres.
    pub extent_mm: f64,
    /// Every `major_every`-th line is drawn with `major_color`.
    pub major_every: u32,
    /// Colour of ordinary lines, in gamma space.
    pub minor_color: [f32; 4],
    /// Colour of every major line, in gamma space.
    pub major_color: [f32; 4],
    /// Colour of the world X axis, in gamma space.
    pub axis_x_color: [f32; 4],
    /// Colour of the world Y axis, in gamma space.
    pub axis_y_color: [f32; 4],
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: true,
            spacing_mm: 100.0,
            extent_mm: 5000.0,
            major_every: 5,
            minor_color: [0.28, 0.30, 0.34, 1.0],
            major_color: [0.42, 0.45, 0.50, 1.0],
            axis_x_color: [0.72, 0.28, 0.28, 1.0],
            axis_y_color: [0.28, 0.62, 0.34, 1.0],
        }
    }
}

/// How close a bound must come to the floor to count as resting on it, in millimetres.
///
/// A panel placed at `z = 0` lands exactly on the plane, but a file edited elsewhere can leave a
/// sub-millimetre gap; the tolerance keeps that cabinet on the floor without giving a shelf mounted
/// ten millimetres up a shadow it should not have.
pub const FLOOR_TOLERANCE_MM: f64 = 0.5;

/// Fraction of an object's footprint half-extent the shadow ellipse covers.
///
/// Below one so the shadow stays inside the object's outline instead of spilling past it; the
/// falloff reaches zero at the ellipse rim, inside the footprint edge.
pub const SHADOW_FOOTPRINT_SCALE: f64 = 0.92;

/// Height of the shadow plane above the floor, in millimetres.
///
/// The grid is lifted to `0.02`, so a shadow at `0.01` sits under the lines and never z-fights with
/// the floor.
const SHADOW_GROUND_Z_MM: f64 = 0.01;

/// A soft contact shadow on the world XY plane under one object that rests on the floor.
///
/// The shadow is an axis-aligned ellipse derived from the object's world bounds. The renderer draws
/// it unlit with one transparent triangle pass and a radial falloff — no shadow map, no extra pass
/// and no texture — so a cabinet reads as standing on something instead of floating in the grid. It
/// is frame data, never document data (ADR 0071).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadow {
    /// The node whose footprint this shadow follows.
    pub id: Id,
    /// Centre of the ellipse on the ground plane, in world millimetres. `z` is the shadow plane.
    pub center: DVec3,
    /// Half-axes of the ellipse along world X and Y, in millimetres.
    pub radii: [f64; 2],
}

impl ContactShadow {
    /// Derives the shadow under a world-space bound, or `None` when the bound does not rest on the
    /// floor.
    ///
    /// A non-finite bound, a footprint with no area, or a lowest point further than
    /// [`FLOOR_TOLERANCE_MM`] from the floor casts nothing. The ellipse covers
    /// [`SHADOW_FOOTPRINT_SCALE`] of the footprint, so it always stays inside the object's outline.
    pub fn from_bounds(id: Id, min: DVec3, max: DVec3) -> Option<Self> {
        if !min.is_finite() || !max.is_finite() || min.z.abs() > FLOOR_TOLERANCE_MM {
            return None;
        }
        let radii = [(max.x - min.x) * 0.5 * SHADOW_FOOTPRINT_SCALE, (max.y - min.y) * 0.5 * SHADOW_FOOTPRINT_SCALE];
        if !radii[0].is_finite() || !radii[1].is_finite() || radii[0] <= 0.0 || radii[1] <= 0.0 {
            return None;
        }
        Some(Self { id, center: DVec3::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, SHADOW_GROUND_Z_MM), radii })
    }
}

/// The contact-shadow switch and strength, beside the grid in a frame.
///
/// A change is read at draw time like [`ShadingStyle`], so turning the pass off costs no upload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadows {
    /// Whether the pass draws at all.
    pub visible: bool,
    /// Peak opacity at the centre, from 0 to 1.
    pub strength: f32,
    /// Gamma-space RGB of the shadow, like [`Grid`] colours, because the pass is unlit.
    pub color: [f32; 3],
}

impl ContactShadows {
    /// Whether the pass draws anything: the switch is on and the strength is above zero.
    pub fn draws(&self) -> bool {
        self.visible && self.strength.is_finite() && self.strength > 0.0
    }
}

impl Default for ContactShadows {
    fn default() -> Self {
        Self { visible: true, strength: 0.4, color: [0.0, 0.0, 0.0] }
    }
}

/// The largest number of point lights one frame can shade.
///
/// A frame has one directional key light and this many point lights, which keeps the uniform small
/// and the fragment loop bounded. Further point lights are stored in the document but do not light
/// the viewport.
pub const MAX_POINT_LIGHTS: usize = 8;

/// A point light in world space.
///
/// The power is in watts, following Blender's point-light unit: a light of 1000 W at one metre
/// shades a white surface at about half intensity under the viewport's fixed exposure. The
/// renderer applies inverse-square falloff and no hard range cutoff ([ADR 0039]).
///
/// [ADR 0039]: ../docs/decisions/0039-point-lights-use-watts-and-inverse-square-falloff.md
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// World-space origin.
    pub position: DVec3,
    /// Linear RGB colour.
    pub color: [f32; 3],
    /// Radiant power in watts.
    pub power: f32,
}

/// The key light, point lights and ambient term used to shade solid drawables.
#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Unit direction from a surface towards the key light, in world space.
    pub key_direction: DVec3,
    /// Linear RGB intensity of the key light.
    pub key_color: [f32; 3],
    /// Fraction of the surface colour visible without direct light.
    pub ambient: f32,
    /// Point lights in tree order, at most [`MAX_POINT_LIGHTS`] of them when built from a scene.
    pub points: Vec<PointLight>,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            key_direction: DVec3::new(-0.35, 0.55, 0.75).try_normalize().unwrap_or(DVec3::Z),
            key_color: [1.0, 0.97, 0.92],
            ambient: 0.32,
            points: Vec::new(),
        }
    }
}

/// One shaded mesh in world space.
///
/// The mesh is shared so rebuilding a scene does not copy vertex data. `world_transform` is a
/// rigid transform composed from the assembly tree; the renderer applies it before uploading.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawable {
    /// The document object this mesh belongs to.
    pub id: Id,
    /// Local-space triangles.
    pub mesh: Arc<MeshData>,
    /// Boundary and crease edges of `mesh`, two vertex indices per edge.
    ///
    /// The list is derived beside the mesh and cached with it (ADR 0053), so the edge pass costs no
    /// search per frame. `None` means the drawable has no edge pass.
    pub edges: Option<Arc<[u32]>>,
    /// Every triangle edge of `mesh`, triangulation diagonals included.
    ///
    /// Only the optional *all surface edges* style reads it; the default edge pass uses [`Self::edges`].
    pub all_edges: Option<Arc<[u32]>>,
    /// Rigid local-to-world transform.
    pub world_transform: DMat4,
    /// Linear RGB colour and opacity.
    pub color: [f32; 4],
}

impl Drawable {
    /// Creates a drawable from a document id, a mesh, a rigid world transform and a colour.
    ///
    /// The drawable starts with no edge lists; [`Drawable::with_edges`] and
    /// [`Drawable::with_all_edges`] add them.
    pub fn new(id: Id, mesh: Arc<MeshData>, world_transform: DMat4, color: [f32; 4]) -> Self {
        Self { id, mesh, edges: None, all_edges: None, world_transform, color }
    }

    /// Attaches the boundary and crease edge list derived from `mesh`.
    pub fn with_edges(mut self, edges: Arc<[u32]>) -> Self {
        self.edges = Some(edges);
        self
    }

    /// Attaches the full triangulation edge list derived from `mesh`.
    pub fn with_all_edges(mut self, all_edges: Arc<[u32]>) -> Self {
        self.all_edges = Some(all_edges);
        self
    }
}

/// One unlit polyline in world space.
///
/// A path is drawn as a chain of line segments, not a solid: it has no faces and no shading. The
/// points are local-space and `world_transform` is the rigid transform of the node that owns the
/// path, exactly like [`Drawable`], so a drag preview moves the matrix instead of copying the
/// points. The colour is gamma-space, like [`Grid`] colours, because the line pipeline is unlit.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    /// The document object this chain belongs to.
    pub id: Id,
    /// Local-space points in travel order; at least two.
    pub points: Arc<[DVec3]>,
    /// Rigid local-to-world transform.
    pub world_transform: DMat4,
    /// Gamma-space RGB and opacity.
    pub color: [f32; 4],
}

impl Polyline {
    /// Creates a polyline from a document id, local points, a rigid world transform and a colour.
    pub fn new(id: Id, points: Arc<[DVec3]>, world_transform: DMat4, color: [f32; 4]) -> Self {
        Self { id, points, world_transform, color }
    }
}

/// Everything needed to draw and light one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    /// The active orbit camera.
    pub camera: Camera,
    /// How the drawables are shaded.
    pub shading: ShadingStyle,
    /// Whether the edge pass draws every triangle edge instead of only the form edges.
    ///
    /// A diagnostic view state beside [`SceneFrame::shading`], never document data.
    pub all_edges: bool,
    /// The floor grid.
    pub grid: Grid,
    /// The contact-shadow switch and strength.
    pub contact_shadows: ContactShadows,
    /// Soft shadows under the objects that rest on the floor.
    pub shadows: Vec<ContactShadow>,
    /// The key light.
    pub lighting: Lighting,
    /// The clear colour, in gamma space.
    pub background: [f32; 4],
    /// Shaded meshes in world space.
    pub drawables: Vec<Drawable>,
    /// Unlit line chains in world space.
    pub polylines: Vec<Polyline>,
}

impl Default for SceneFrame {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            shading: ShadingStyle::default(),
            all_edges: false,
            grid: Grid::default(),
            contact_shadows: ContactShadows::default(),
            shadows: Vec::new(),
            lighting: Lighting::default(),
            background: [0.10, 0.11, 0.13, 1.0],
            drawables: Vec::new(),
            polylines: Vec::new(),
        }
    }
}

impl SceneFrame {
    /// Adds a drawable to the frame.
    pub fn push(&mut self, drawable: Drawable) {
        self.drawables.push(drawable);
    }

    /// Adds an unlit line chain to the frame.
    pub fn push_polyline(&mut self, polyline: Polyline) {
        self.polylines.push(polyline);
    }

    /// Adds a contact shadow to the frame.
    pub fn push_contact_shadow(&mut self, shadow: ContactShadow) {
        self.shadows.push(shadow);
    }

    /// The frame's drawables.
    pub fn drawables(&self) -> &[Drawable] {
        &self.drawables
    }

    /// The frame's contact shadows.
    pub fn shadows(&self) -> &[ContactShadow] {
        &self.shadows
    }

    /// The frame's unlit line chains.
    pub fn polylines(&self) -> &[Polyline] {
        &self.polylines
    }

    /// The document id of the nearest drawable or line chain hit by `ray`.
    ///
    /// A drawable is hit when one of its triangles crosses the ray. A line chain has no thickness,
    /// so it is hit when the ray passes within [`LINE_PICK_RADIUS_POINTS`] points of a segment at
    /// that segment's depth; `viewport_height` is the viewport's height in points, and a
    /// non-positive or non-finite one accepts only an exact crossing. A path wins a tie with a
    /// solid it lies on, so a contour drawn on a face stays selectable.
    ///
    /// Malformed vertices, indices and world transforms are skipped. Picking returns a document
    /// id, never a mesh index or a pointer: the caller can select it or send it to a command.
    pub fn pick(&self, ray: Ray, viewport_height: f64) -> Option<Id> {
        let mut nearest: Option<(f64, Id)> = None;
        for drawable in &self.drawables {
            let Some(distance) = ray_hit_drawable(ray, drawable) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, drawable.id));
            }
        }
        for polyline in &self.polylines {
            let Some(distance) = ray_hit_polyline(ray, polyline, self.camera, viewport_height) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| distance <= best) {
                nearest = Some((distance, polyline.id));
            }
        }
        nearest.map(|(_, id)| id)
    }

    /// A copy of the frame with every selected drawable and line chain highlighted for picking
    /// feedback.
    ///
    /// The base frame keeps its material colours, so clearing the selection restores them without
    /// rebuilding meshes. Passing the panels below a selected container highlights the container as
    /// one object. A line chain takes [`SELECTION_LINE_COLOR`] explicitly: a tint is invisible on a
    /// one-pixel line, and the shell widens it as an overlay.
    pub fn with_selection_highlight(&self, selected: &[Id]) -> Self {
        let mut frame = self.clone();
        if selected.is_empty() {
            return frame;
        }
        for drawable in &mut frame.drawables {
            if selected.contains(&drawable.id) {
                tint_selected(&mut drawable.color);
            }
        }
        for polyline in &mut frame.polylines {
            if selected.contains(&polyline.id) {
                polyline.color = SELECTION_LINE_COLOR;
            }
        }
        frame
    }

    /// A copy of the frame with the given drawables and line chains shifted by a world-space
    /// translation.
    ///
    /// This is a drag preview: it moves the listed primitives without touching the document, so the
    /// renderer can follow the pointer before one command commits the result. A non-finite delta
    /// leaves the frame unchanged.
    pub fn with_world_translation(&self, ids: &[Id], delta: DVec3) -> Self {
        let mut frame = self.clone();
        if ids.is_empty() || !delta.is_finite() {
            return frame;
        }
        let translation = DMat4::from_translation(delta);
        for drawable in &mut frame.drawables {
            if ids.contains(&drawable.id) {
                drawable.world_transform = translation * drawable.world_transform;
            }
        }
        for polyline in &mut frame.polylines {
            if ids.contains(&polyline.id) {
                polyline.world_transform = translation * polyline.world_transform;
            }
        }
        for shadow in &mut frame.shadows {
            if ids.contains(&shadow.id) {
                // A shadow stays on the ground plane: only its footprint slides with the object.
                shadow.center.x += delta.x;
                shadow.center.y += delta.y;
            }
        }
        frame
    }

    /// A copy of the frame with the given drawables and line chains rotated about a world-space
    /// pivot.
    ///
    /// This is a drag preview: it turns the listed primitives without touching the document, so the
    /// renderer can follow the pointer before one command commits the result. The rotation must be a
    /// finite unit quaternion and the pivot finite; anything else leaves the frame unchanged.
    pub fn with_world_rotation(&self, ids: &[Id], pivot: DVec3, rotation: DQuat) -> Self {
        let mut frame = self.clone();
        if ids.is_empty() || !pivot.is_finite() || !rotation.is_finite() || (rotation.length() - 1.0).abs() > 1.0e-9 {
            return frame;
        }
        let about_pivot = DMat4::from_translation(pivot) * DMat4::from_quat(rotation) * DMat4::from_translation(-pivot);
        for drawable in &mut frame.drawables {
            if ids.contains(&drawable.id) {
                drawable.world_transform = about_pivot * drawable.world_transform;
            }
        }
        for polyline in &mut frame.polylines {
            if ids.contains(&polyline.id) {
                polyline.world_transform = about_pivot * polyline.world_transform;
            }
        }
        for shadow in &mut frame.shadows {
            if ids.contains(&shadow.id) {
                // The ellipse stays world-axis aligned, so only its centre follows the turn; the
                // footprint is a soft blob and does not need a second orientation.
                let turned = about_pivot.transform_point3(shadow.center);
                shadow.center.x = turned.x;
                shadow.center.y = turned.y;
            }
        }
        frame
    }

    /// A copy of the frame with one drawable's local mesh replaced.
    ///
    /// This is a drag preview for sub-object editing: the shell builds a mesh with the moved
    /// vertices and swaps it in for one frame, so the document and the renderer cache stay untouched
    /// until the command commits. An unknown id leaves the frame unchanged.
    pub fn with_drawable_mesh(&self, id: Id, mesh: Arc<MeshData>) -> Self {
        let mut frame = self.clone();
        for drawable in &mut frame.drawables {
            if drawable.id == id {
                drawable.mesh = mesh.clone();
            }
        }
        frame
    }
}

/// Linear RGB colour mixed into the selected drawable.
const SELECTION_TINT: [f64; 3] = [1.0, 0.55, 0.15];

/// Fraction of [`SELECTION_TINT`] mixed into the base colour.
const SELECTION_MIX: f64 = 0.35;

/// Mixes the selection tint into an RGB colour, leaving the alpha channel untouched.
fn tint_selected(color: &mut [f32; 4]) {
    for (channel, tint) in color.iter_mut().zip(SELECTION_TINT) {
        let base = f64::from(*channel);
        *channel = (base * (1.0 - SELECTION_MIX) + tint * SELECTION_MIX) as f32;
    }
}

fn ray_hit_drawable(ray: Ray, drawable: &Drawable) -> Option<f64> {
    let mesh = &drawable.mesh;
    let mut nearest = f64::INFINITY;
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let [i0, i1, i2] = *triangle;
        let (Some(v0), Some(v1), Some(v2)) = (
            world_vertex(mesh, i0, drawable.world_transform),
            world_vertex(mesh, i1, drawable.world_transform),
            world_vertex(mesh, i2, drawable.world_transform),
        ) else {
            continue;
        };
        if let Some(distance) = ray_triangle(ray, v0, v1, v2)
            && distance < nearest
        {
            nearest = distance;
        }
    }
    nearest.is_finite().then_some(nearest)
}

fn world_vertex(mesh: &MeshData, index: u32, transform: DMat4) -> Option<DVec3> {
    let position = mesh.positions.get(index as usize)?;
    let local = DVec3::new(f64::from(position[0]), f64::from(position[1]), f64::from(position[2]));
    if !local.is_finite() {
        return None;
    }
    let world = transform.transform_point3(local);
    world.is_finite().then_some(world)
}

/// The distance along `ray` to its closest approach to a line chain, if the ray passes near it.
///
/// A segment counts as hit when the separation at its depth is within the pick radius the camera
/// gives for that depth, so a near path does not need as wide a world-space tolerance as a far one.
fn ray_hit_polyline(ray: Ray, polyline: &Polyline, camera: Camera, viewport_height: f64) -> Option<f64> {
    let mut nearest = f64::INFINITY;
    for pair in polyline.points.windows(2) {
        let (Some(&start), Some(&end)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let start = polyline.world_transform.transform_point3(start);
        let end = polyline.world_transform.transform_point3(end);
        if !start.is_finite() || !end.is_finite() {
            continue;
        }
        let Some((along, separation)) = ray_segment(ray, start, end) else {
            continue;
        };
        let tolerance = camera.world_per_point(along, viewport_height) * LINE_PICK_RADIUS_POINTS;
        if separation <= tolerance && along < nearest {
            nearest = along;
        }
    }
    nearest.is_finite().then_some(nearest)
}

/// Distance along `ray` to its closest approach to the segment `a..b`, and the separation there.
///
/// A line has no thickness, so the caller compares the second value with its own tolerance while
/// the first orders the hit against a solid. The closest approach must be in front of the ray; a
/// segment entirely behind the camera yields `None`. A zero-length segment is treated as the point
/// `a`, and a non-finite endpoint yields `None`.
pub fn ray_segment(ray: Ray, a: DVec3, b: DVec3) -> Option<(f64, f64)> {
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let offset = ray.origin - a;
    let along_offset = ray.direction.dot(offset);
    let segment = b - a;
    let length_squared = segment.length_squared();
    let parameter = if length_squared <= 1.0e-12 {
        0.0
    } else {
        let along_segment = ray.direction.dot(segment);
        let denominator = length_squared - along_segment * along_segment;
        let raw = if denominator.abs() <= 1.0e-12 {
            0.0
        } else {
            (segment.dot(offset) - along_segment * along_offset) / denominator
        };
        raw.clamp(0.0, 1.0)
    };
    let along = parameter.mul_add(ray.direction.dot(segment), -along_offset);
    if along < 0.0 || !along.is_finite() {
        return None;
    }
    let closest = ray.origin + ray.direction * along;
    let point = a + segment * parameter;
    let separation = point.distance(closest);
    separation.is_finite().then_some((along, separation))
}

/// Möller–Trumbore intersection; returns the positive distance along the ray.
///
/// Shared with viewport sub-object picking, which tests the polygons of an unsimplified mesh
/// before anything is tessellated for the renderer.
pub fn ray_triangle(ray: Ray, v0: DVec3, v1: DVec3, v2: DVec3) -> Option<f64> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let p = ray.direction.cross(edge2);
    let determinant = edge1.dot(p);
    if determinant.abs() < 1.0e-12 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let offset = ray.origin - v0;
    let u = offset.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = offset.cross(edge1);
    let v = ray.direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge2.dot(q) * inverse;
    (distance > 0.0).then_some(distance)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use furnicraft_geom::DVec4;
    use furnicraft_model::Id;

    fn assert_close(actual: DVec3, expected: DVec3) {
        assert!((actual - expected).length() < 1.0e-9, "actual {actual:?}, expected {expected:?}");
    }

    #[test]
    fn a_display_style_is_frame_data_with_a_default() {
        assert_eq!(SceneFrame::default().shading, ShadingStyle::ShadedWithEdges);
        assert_eq!(ShadingStyle::ALL.len(), 5, "the cluster offers every style the renderer interprets");
        assert!(ShadingStyle::Wireframe.draws_edges() && !ShadingStyle::Wireframe.draws_faces());
        assert!(!ShadingStyle::Translucent.draws_edges() && ShadingStyle::Translucent.is_translucent());
        assert!(!ShadingStyle::Shaded.draws_edges() && ShadingStyle::Shaded.draws_faces());
        assert!(ShadingStyle::ShadedWithHiddenEdges.edges_through_faces());
        assert!(!ShadingStyle::ShadedWithEdges.edges_through_faces());
    }

    #[test]
    fn a_contact_shadow_rests_on_the_floor_inside_the_footprint() {
        let id = Id::new(7).expect("id");
        let shadow = ContactShadow::from_bounds(id, DVec3::new(-100.0, -50.0, 0.0), DVec3::new(100.0, 50.0, 600.0))
            .expect("a bound on the floor casts a shadow");
        assert_eq!(shadow.id, id);
        assert!((shadow.center - DVec3::new(0.0, 0.0, SHADOW_GROUND_Z_MM)).length() < 1.0e-9);
        assert!((shadow.radii[0] - 92.0).abs() < 1.0e-9, "the ellipse is narrower than the footprint");
        assert!((shadow.radii[1] - 46.0).abs() < 1.0e-9);
        assert!(shadow.radii[0] < 100.0 && shadow.radii[1] < 50.0, "the ellipse stays inside the footprint");
        assert!(shadow.center.z < 0.02, "the shadow sits under the grid lines");
    }

    #[test]
    fn a_bound_without_floor_contact_casts_no_shadow() {
        let id = Id::new(8).expect("id");
        let floating = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 10.0), DVec3::new(600.0, 400.0, 700.0));
        assert!(floating.is_none(), "a shelf ten millimetres up floats, it does not rest");
        let within = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.4), DVec3::new(600.0, 400.0, 18.0));
        assert!(within.is_some(), "a sub-millimetre gap still reads as the floor");
        let flat = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 0.0));
        assert!(flat.is_none(), "a footprint with no area casts nothing");
        let hostile = ContactShadow::from_bounds(id, DVec3::new(f64::NAN, 0.0, 0.0), DVec3::new(1.0, 1.0, 1.0));
        assert!(hostile.is_none(), "a non-finite bound casts nothing");
    }

    #[test]
    fn the_contact_shadow_switch_removes_the_pass() {
        let mut frame = SceneFrame::default();
        assert!(frame.contact_shadows.draws(), "a frame draws the shadows by default");
        assert!(frame.shadows().is_empty());
        let id = Id::new(9).expect("id");
        let shadow = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 500.0));
        frame.push_contact_shadow(shadow.expect("shadow"));
        assert_eq!(frame.shadows().len(), 1);

        frame.contact_shadows.visible = false;
        assert!(!frame.contact_shadows.draws(), "the switch removes the pass from the frame");
        frame.contact_shadows.visible = true;
        frame.contact_shadows.strength = 0.0;
        assert!(!frame.contact_shadows.draws(), "a zero strength draws nothing either");
    }

    #[test]
    fn default_camera_orbits_at_a_finite_distance() {
        let camera = Camera::default();
        assert!(camera.eye().is_finite());
        assert!(((camera.eye() - camera.target()).length() - camera.distance()).abs() < 1.0e-9);
        assert!((camera.forward().length() - 1.0).abs() < 1.0e-12);
        assert!((camera.right().dot(camera.up())).abs() < 1.0e-12);
    }

    #[test]
    fn camera_rejects_hostile_constructor_input() {
        assert!(matches!(Camera::new(DVec3::new(f64::NAN, 0.0, 0.0), 1000.0), Err(CameraError::NonFiniteTarget)));
        assert!(matches!(Camera::new(DVec3::ZERO, f64::INFINITY), Err(CameraError::InvalidDistance { .. })));
        assert!(matches!(Camera::new(DVec3::ZERO, MIN_DISTANCE_MM * 0.5), Err(CameraError::InvalidDistance { .. })));
    }

    #[test]
    fn camera_looks_at_the_target() {
        let camera = Camera::new(DVec3::new(10.0, 20.0, 30.0), 1000.0).expect("valid camera");
        let eye = camera.eye();
        let view = camera.view_matrix();
        assert_close(view.transform_point3(eye), DVec3::ZERO);
        assert_close(view.transform_point3(camera.target()), DVec3::new(0.0, 0.0, -1000.0));
    }

    #[test]
    fn orbit_wraps_yaw_and_clamps_pitch() {
        let mut camera = Camera::default();
        camera.orbit(TAU + 0.25, 100.0);
        assert!((0.0..TAU).contains(&camera.yaw()));
        assert_eq!(camera.pitch(), MAX_PITCH);
        camera.orbit(0.0, -10_000.0);
        assert_eq!(camera.pitch(), MIN_PITCH);
        let yaw = camera.yaw();
        camera.orbit(f64::NAN, 0.1);
        assert_eq!(camera.yaw(), yaw);
    }

    #[test]
    fn zoom_clamps_and_ignores_invalid_factors() {
        let mut camera = Camera::default();
        camera.zoom(0.5);
        assert_eq!(camera.distance(), 600.0);
        camera.zoom(1.0e12);
        assert_eq!(camera.distance(), MAX_DISTANCE_MM);
        camera.zoom(1.0e-12);
        assert_eq!(camera.distance(), MIN_DISTANCE_MM);
        let distance = camera.distance();
        camera.zoom(f64::NAN);
        camera.zoom(0.0);
        assert_eq!(camera.distance(), distance);
    }

    #[test]
    fn pan_moves_the_target_with_the_pointer() {
        let mut camera = Camera::default();
        let before = camera.target();
        camera.pan_pixels(100.0, 0.0, 800.0);
        let moved = camera.target() - before;
        assert!(moved.length() > 0.0);
        assert!(moved.dot(camera.right()) < 0.0, "dragging right moves the target left");
        camera.pan_pixels(f64::INFINITY, 0.0, 800.0);
        assert!(camera.target().is_finite());
    }

    #[test]
    fn look_from_snaps_to_axis_views_and_keeps_the_target() {
        let default = Camera::default();
        let mut camera = Camera::default();
        camera.look_from(DVec3::X);
        let offset = camera.eye() - camera.target();
        assert!((offset.try_normalize().expect("eye offset") - DVec3::X).length() < 1.0e-9);
        assert!(camera.forward().dot(-DVec3::X) > 1.0 - 1.0e-9);
        assert_eq!(camera.target(), default.target());
        assert_eq!(camera.distance(), default.distance());

        camera.look_from(-DVec3::Y);
        let offset = camera.eye() - camera.target();
        assert!((offset.try_normalize().expect("eye offset") - -DVec3::Y).length() < 1.0e-9);

        camera.look_from(DVec3::Z);
        assert_eq!(camera.pitch(), MAX_PITCH);
        let yaw = camera.yaw();
        camera.look_from(DVec3::ZERO);
        assert_eq!(camera.yaw(), yaw);
        assert_eq!(camera.pitch(), MAX_PITCH);
        camera.look_from(DVec3::new(f64::NAN, 0.0, 0.0));
        assert_eq!(camera.yaw(), yaw);
        assert_eq!(camera.pitch(), MAX_PITCH);
    }

    #[test]
    fn a_pole_view_looks_straight_down_and_projects_its_target() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        assert_eq!(camera.pitch(), FRAC_PI_2);
        assert!((camera.forward() - -DVec3::Z).length() < 1.0e-12, "forward was {:?}", camera.forward());
        let up = camera.up();
        assert!(up.is_finite());
        assert!((up.length() - 1.0).abs() < 1.0e-12, "up was {up:?}");
        assert!(up.dot(camera.forward()).abs() < 1.0e-12, "up is perpendicular to forward");

        let aspect = 16.0 / 9.0;
        let centre = camera.ray_ndc(0.0, 0.0, aspect);
        assert!((centre.direction() - -DVec3::Z).length() < 1.0e-12, "the centre ray looks straight down");
        let ndc = camera.project_ndc(camera.target(), aspect).expect("the target is in front");
        assert!(ndc.length() < 1.0e-12, "the target projects to the viewport centre, got {ndc:?}");

        camera.look_from(-DVec3::Z);
        assert_eq!(camera.pitch(), MIN_PITCH);
        assert!((camera.forward() - DVec3::Z).length() < 1.0e-12, "forward was {:?}", camera.forward());
        assert!((camera.up().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn orbiting_off_a_pole_moves_the_view() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        let before = camera.forward();
        camera.orbit(0.0, -0.1);
        assert!((camera.pitch() - (FRAC_PI_2 - 0.1)).abs() < 1.0e-12);
        assert!((camera.forward() - before).length() > 0.05, "an orbit leaves the pole");
        assert!((camera.up().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn the_view_matrix_keeps_an_authored_up_vector() {
        let up = DVec3::new(0.0, 1.0, 0.005).try_normalize().expect("a finite up vector");
        let camera = Camera::default().looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, up, -DVec3::Z);
        assert_close(camera.up(), up);
        let columns = camera.view_matrix().to_cols_array();
        assert_close(DVec3::new(columns[1], columns[5], columns[9]), up);
    }

    #[test]
    fn projection_is_finite_at_both_depth_planes() {
        let camera = Camera::default();
        let projection = camera.projection_matrix(16.0 / 9.0);
        assert!(projection.is_finite());

        let near = projection * DVec4::new(0.0, 0.0, -camera.near, 1.0);
        let far = projection * DVec4::new(0.0, 0.0, -camera.far, 1.0);
        assert!((near.z / near.w - 0.0).abs() < 1.0e-9);
        assert!((far.z / far.w - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn toggling_the_projection_keeps_the_visible_height() {
        let mut camera = Camera::default();
        let perspective = camera;
        camera.toggle_projection();
        let Projection::Orthographic { height } = camera.projection() else {
            panic!("a perspective view becomes orthographic");
        };
        let expected = 2.0 * perspective.distance() * (DEFAULT_FOV_Y * 0.5).tan();
        assert!((height - expected).abs() < 1.0e-9, "the orthographic height is the frame at the target");
        assert_eq!(camera.target(), perspective.target());
        assert_eq!(camera.yaw(), perspective.yaw());
        assert_eq!(camera.pitch(), perspective.pitch());
        assert_eq!(camera.distance(), perspective.distance());

        camera.toggle_projection();
        assert!(matches!(camera.projection(), Projection::Perspective { .. }));
        assert!((camera.distance() - perspective.distance()).abs() < 1.0e-9, "the frame comes back");
        assert_eq!(camera.target(), perspective.target());
        assert_eq!(camera.yaw(), perspective.yaw());
        assert_eq!(camera.pitch(), perspective.pitch());
    }

    #[test]
    fn zooming_an_orthographic_view_scales_the_visible_height() {
        let mut camera = Camera::default();
        camera.toggle_projection();
        let Projection::Orthographic { height } = camera.projection() else {
            panic!("the view is orthographic");
        };
        camera.zoom(0.5);
        let Projection::Orthographic { height: zoomed } = camera.projection() else {
            panic!("zooming keeps the projection");
        };
        assert!((zoomed - height * 0.5).abs() < 1.0e-9, "zooming in halves the visible height");

        camera.toggle_projection();
        camera.toggle_projection();
        let Projection::Orthographic { height: returned } = camera.projection() else {
            panic!("the round trip returns an orthographic view");
        };
        assert!((returned - zoomed).abs() < 1.0e-9, "a toggle pair keeps the zoomed frame");
    }

    #[test]
    fn an_authored_camera_keeps_its_projection() {
        let authored = Camera::default()
            .looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, DVec3::Y, -DVec3::Z)
            .with_projection(Projection::Orthographic { height: 500.0 });
        let mut camera = authored;
        camera.toggle_projection();
        camera.zoom(4.0);
        assert_eq!(camera, authored, "a pinned camera is not view state the viewport may change");
    }

    #[test]
    fn an_orthographic_camera_projects_without_perspective() {
        let camera = Camera::default().with_projection(Projection::Orthographic { height: 100.0 });
        let aspect = 2.0;
        let eye = camera.eye();
        let point = eye + camera.forward() * 500.0 + camera.right() * 100.0;
        let ndc = camera.project_ndc(point, aspect).expect("a point in front of the camera");
        assert!((ndc.x - 1.0).abs() < 1.0e-9, "ndc was {ndc:?}");
        assert!(ndc.y.abs() < 1.0e-9);

        let centre = camera.ray_ndc(0.0, 0.0, aspect);
        let right = camera.ray_ndc(1.0, 0.0, aspect);
        assert_close(centre.direction(), camera.forward());
        assert_close(right.direction(), camera.forward());
        assert!((right.origin() - centre.origin()).dot(camera.right()) > 99.0, "rays are parallel and offset");

        assert!(camera.project_ndc(eye - camera.forward() * 10.0, aspect).is_none(), "behind is rejected");

        let projection = camera.projection_matrix(aspect);
        let near = projection * DVec4::new(0.0, 0.0, -camera.near, 1.0);
        let far = projection * DVec4::new(0.0, 0.0, -camera.far, 1.0);
        assert!((near.z / near.w - 0.0).abs() < 1.0e-9);
        assert!((far.z / far.w - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_fixed_view_keeps_its_basis_and_ignores_navigation() {
        let mut camera = Camera::default().looked_from(DVec3::new(1.0, 2.0, 3.0), DVec3::X, DVec3::Y, -DVec3::Z);
        assert_close(camera.eye(), DVec3::new(1.0, 2.0, 3.0));
        assert_close(camera.forward(), -DVec3::Z);
        assert_close(camera.right(), DVec3::X);
        assert_close(camera.up(), DVec3::Y);

        let before = camera;
        camera.orbit(1.0, 1.0);
        camera.zoom(0.5);
        camera.pan_pixels(10.0, 10.0, 800.0);
        camera.look_from(DVec3::X);
        assert_eq!(camera, before, "an authored view is not navigated by the viewport");

        let orbit = Camera::default();
        assert_eq!(orbit.looked_from(DVec3::NAN, DVec3::X, DVec3::Y, DVec3::Z), orbit);
        assert_eq!(orbit.looked_from(DVec3::ZERO, DVec3::X, DVec3::X, DVec3::Z), orbit);
    }

    #[test]
    fn scene_accepts_drawables() {
        let mut frame = SceneFrame::default();
        let mesh = Arc::new(MeshData::default());
        let id = Id::new(7).expect("id");
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.drawables().len(), 1);
        assert_eq!(frame.drawables()[0].id, id);
    }

    fn line_chain(id: Id) -> Polyline {
        Polyline::new(id, Arc::from(vec![DVec3::ZERO, DVec3::X]), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0])
    }

    #[test]
    fn scene_accepts_polylines() {
        let mut frame = SceneFrame::default();
        let id = Id::new(7).expect("id");
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)]);
        frame.push_polyline(Polyline::new(id, points.clone(), DMat4::IDENTITY, [1.0; 4]));

        assert_eq!(frame.polylines().len(), 1);
        assert_eq!(frame.polylines()[0].id, id);
        assert_eq!(frame.polylines()[0].points, points);
        assert!(frame.drawables().is_empty(), "a line chain is not a solid drawable");
    }

    #[test]
    fn a_world_translation_preview_moves_only_the_listed_polylines() {
        let moved = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push_polyline(line_chain(moved));
        frame.push_polyline(line_chain(other));

        let translated = frame.with_world_translation(&[moved], DVec3::new(1.0, 2.0, 3.0));
        let position = translated.polylines()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 2.0, 3.0)).length() < 1.0e-12);
        assert_eq!(translated.polylines()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.polylines()[0].world_transform, DMat4::IDENTITY, "the base frame keeps its transform");
    }

    #[test]
    fn a_world_rotation_preview_turns_only_the_listed_polylines() {
        let turned = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        let offset = DMat4::from_translation(DVec3::new(2.0, 0.0, 0.0));
        frame.push_polyline(Polyline::new(turned, Arc::from(vec![DVec3::ZERO, DVec3::X]), offset, [1.0; 4]));
        frame.push_polyline(line_chain(other));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let rotated = frame.with_world_rotation(&[turned], DVec3::new(1.0, 0.0, 0.0), quarter_turn);
        let position = rotated.polylines()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 1.0, 0.0)).length() < 1.0e-12, "position was {position:?}");
        assert_eq!(rotated.polylines()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.polylines()[0].world_transform, offset);
    }

    #[test]
    fn selection_highlight_gives_every_selected_polyline_the_explicit_colour() {
        let selected = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push_polyline(line_chain(selected));
        frame.push_polyline(line_chain(other));

        let highlighted = frame.with_selection_highlight(&[selected]);
        assert_eq!(highlighted.polylines()[0].color, SELECTION_LINE_COLOR);
        assert_ne!(SELECTION_LINE_COLOR, frame.polylines()[0].color, "picking the colour must be visible");
        assert_eq!(highlighted.polylines()[1].color, frame.polylines()[1].color);
        assert_eq!(frame.polylines()[0].color, [0.5, 0.5, 0.5, 1.0], "the base frame keeps its colour");
    }

    fn quad_mesh() -> Arc<MeshData> {
        Arc::new(MeshData {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
        })
    }

    #[test]
    fn the_centre_ray_looks_where_the_camera_does() {
        let camera = Camera::default();
        let ray = camera.ray_ndc(0.0, 0.0, 16.0 / 9.0);
        assert_close(ray.origin(), camera.eye());
        assert_close(ray.direction(), camera.forward());

        let invalid = camera.ray_ndc(f64::NAN, f64::INFINITY, -1.0);
        assert_close(invalid.direction(), camera.forward());
        assert!(invalid.origin().is_finite());
    }

    #[test]
    fn projection_and_ray_agree_inside_and_outside_the_viewport() {
        let camera = Camera::default();
        let aspect = 16.0 / 9.0;
        let point = camera.target() + camera.right() * 100.0 + camera.up() * 50.0;
        let ndc = camera.project_ndc(point, aspect).expect("point in front of the camera");
        assert!(ndc.x > 0.0 && ndc.y > 0.0, "ndc was {ndc:?}");
        let ray = camera.ray_ndc(ndc.x, ndc.y, aspect);
        let to_point = (point - ray.origin()).try_normalize().expect("direction");
        assert!(ray.direction().dot(to_point) > 1.0 - 1.0e-9);

        let centre = camera.project_ndc(camera.target(), aspect).expect("target");
        assert!(centre.length() < 1.0e-9, "centre was {centre:?}");

        let behind = camera.eye() + (camera.eye() - camera.target());
        assert!(camera.project_ndc(behind, aspect).is_none());
        assert!(camera.project_ndc(DVec3::new(f64::NAN, 0.0, 0.0), aspect).is_none());

        let inside = camera.ray_ndc(1.0, 0.0, aspect);
        let outside = camera.ray_ndc(2.0, 0.0, aspect);
        assert!(outside.direction().dot(camera.right()) > inside.direction().dot(camera.right()));
        assert!(camera.ray_ndc(1.0e300, 0.0, aspect).direction().is_finite());
    }

    #[test]
    fn ray_closest_point_on_an_axis_line_is_a_drag_parameter() {
        let ray = Ray::new(DVec3::new(5.0, 10.0, 0.0), -DVec3::Y).expect("ray");
        let parameter = ray.closest_parameter_on_line(DVec3::ZERO, DVec3::X).expect("line");
        assert!((parameter - 5.0).abs() < 1.0e-12, "parameter was {parameter}");

        let through_origin = Ray::new(DVec3::new(0.0, 1.0, 0.0), -DVec3::Y).expect("ray");
        assert!(through_origin.closest_parameter_on_line(DVec3::ZERO, DVec3::X).expect("line").abs() < 1.0e-12);

        let parallel = Ray::new(DVec3::ZERO, DVec3::X).expect("ray");
        assert!(parallel.closest_parameter_on_line(DVec3::ZERO, DVec3::X).is_none());
        assert!(ray.closest_parameter_on_line(DVec3::ZERO, DVec3::ZERO).is_none());
        assert!(ray.closest_parameter_on_line(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X).is_none());
    }

    #[test]
    fn a_world_translation_preview_moves_only_the_listed_drawables() {
        let moved = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(moved, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let translated = frame.with_world_translation(&[moved], DVec3::new(1.0, 2.0, 3.0));
        let position = translated.drawables()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 2.0, 3.0)).length() < 1.0e-12);
        assert_eq!(translated.drawables()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.drawables()[0].world_transform, DMat4::IDENTITY);

        assert_eq!(frame.with_world_translation(&[], DVec3::X), frame);
        assert_eq!(frame.with_world_translation(&[moved], DVec3::new(f64::NAN, 0.0, 0.0)), frame);
    }

    #[test]
    fn a_world_rotation_preview_turns_only_the_listed_drawables_about_the_pivot() {
        let turned = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        let offset = DMat4::from_translation(DVec3::new(2.0, 0.0, 0.0));
        frame.push(Drawable::new(turned, quad_mesh(), offset, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let rotated = frame.with_world_rotation(&[turned], DVec3::new(1.0, 0.0, 0.0), quarter_turn);
        let position = rotated.drawables()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 1.0, 0.0)).length() < 1.0e-12, "position was {position:?}");
        assert_eq!(rotated.drawables()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.drawables()[0].world_transform, offset);
    }

    #[test]
    fn a_world_rotation_preview_rejects_empty_ids_and_bad_input() {
        let turned = Id::new(2).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(turned, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        assert_eq!(frame.with_world_rotation(&[], DVec3::ZERO, quarter_turn), frame);
        assert_eq!(frame.with_world_rotation(&[turned], DVec3::new(f64::NAN, 0.0, 0.0), quarter_turn), frame);
        assert_eq!(frame.with_world_rotation(&[turned], DVec3::ZERO, DQuat::from_xyzw(2.0, 0.0, 0.0, 0.0)), frame);
    }

    #[test]
    fn a_mesh_preview_replaces_only_the_named_drawable() {
        let edited = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(edited, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let mut replacement = (*quad_mesh()).clone();
        replacement.positions[0] = [9.0, 9.0, 9.0];
        let replacement = Arc::new(replacement);
        let preview = frame.with_drawable_mesh(edited, replacement.clone());

        assert_eq!(preview.drawables()[0].mesh, replacement);
        assert_eq!(preview.drawables()[1].mesh, quad_mesh());
        assert_eq!(frame.drawables()[0].mesh, quad_mesh(), "the base frame keeps its mesh");
        assert_eq!(frame.with_drawable_mesh(Id::new(99).expect("id"), replacement), frame);
    }

    #[test]
    fn picking_returns_the_nearest_document_id() {
        let near = Id::new(2).expect("id");
        let far = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(near, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(far, quad_mesh(), DMat4::from_translation(DVec3::new(0.0, 0.0, 1.0)), [1.0; 4]));

        let from_above = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_above, 600.0), Some(far));
        let from_below = Ray::new(DVec3::new(0.5, 0.5, -10.0), DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_below, 600.0), Some(near));

        let miss = Ray::new(DVec3::new(2.0, 2.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(miss, 600.0), None);
    }

    #[test]
    fn picking_follows_the_world_transform() {
        let id = Id::new(4).expect("id");
        let mut frame = SceneFrame::default();
        let transform = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        frame.push(Drawable::new(id, quad_mesh(), transform, [1.0; 4]));

        let hit = Ray::new(DVec3::new(10.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(hit, 600.0), Some(id));
        let original_position = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(original_position, 600.0), None);
    }

    #[test]
    fn malformed_meshes_are_skipped_instead_of_panicking() {
        let id = Id::new(5).expect("id");
        let mesh =
            Arc::new(MeshData { positions: vec![[0.0; 3]], normals: Vec::new(), indices: vec![0, 1, 2, 0, 0, 0] });
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        let ray = Ray::new(DVec3::new(0.0, 0.0, 1.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(ray, 600.0), None);
    }

    #[test]
    fn world_per_point_follows_the_projection_and_the_depth() {
        let camera = Camera::new(DVec3::ZERO, 1000.0).expect("camera");
        let expected = 2.0 * 1000.0 * (DEFAULT_FOV_Y * 0.5).tan() / 1000.0;
        assert!((camera.world_per_point(1000.0, 1000.0) - expected).abs() < 1.0e-12);
        assert!(camera.world_per_point(2000.0, 1000.0) > camera.world_per_point(1000.0, 1000.0));

        let orthographic = camera.with_projection(Projection::Orthographic { height: 500.0 });
        assert_eq!(orthographic.world_per_point(10.0, 1000.0), 0.5);
        assert_eq!(orthographic.world_per_point(900.0, 1000.0), 0.5);

        assert_eq!(camera.world_per_point(f64::NAN, 1000.0), 0.0);
        assert_eq!(camera.world_per_point(10.0, 0.0), 0.0);
    }

    #[test]
    fn ray_segment_measures_the_closest_approach() {
        let ray = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        let (along, separation) = ray_segment(ray, DVec3::ZERO, DVec3::X).expect("crossing");
        assert!((along - 10.0).abs() < 1.0e-9);
        assert!(separation < 1.0e-9);

        let offset = DVec3::new(0.0, 0.25, 0.0);
        let (_, separation) = ray_segment(ray, offset, offset + DVec3::X).expect("near miss");
        assert!((separation - 0.25).abs() < 1.0e-9);

        let past_the_end = Ray::new(DVec3::new(2.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        let (_, separation) = ray_segment(past_the_end, DVec3::ZERO, DVec3::X).expect("clamped to the end");
        assert!((separation - 1.0).abs() < 1.0e-9);

        let (along, separation) = ray_segment(ray, DVec3::ZERO, DVec3::ZERO).expect("a point");
        assert!((along - 10.0).abs() < 1.0e-9);
        assert!((separation - 0.5).abs() < 1.0e-9, "the zero-length segment is its own start point");

        let behind = Ray::new(DVec3::new(0.5, 0.0, -10.0), -DVec3::Z).expect("ray");
        assert_eq!(ray_segment(behind, DVec3::ZERO, DVec3::X), None);
        assert_eq!(ray_segment(ray, DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X), None);
    }

    #[test]
    fn picking_a_path_returns_its_document_id_near_the_line() {
        let id = Id::new(6).expect("id");
        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        frame.push_polyline(Polyline::new(id, points, DMat4::IDENTITY, [1.0; 4]));

        // At ten millimetres deep and a 600-point viewport, six points are about 0.083 mm.
        let near = Ray::new(DVec3::new(0.5, 0.04, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(near, 600.0), Some(id));
        let far = Ray::new(DVec3::new(0.5, 0.4, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(far, 600.0), None);

        let exact = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(exact, 0.0), Some(id), "an exact crossing needs no tolerance");
        assert_eq!(frame.pick(near, 0.0), None, "a broken viewport height means no radius");

        let miss = Ray::new(DVec3::new(5.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(miss, 600.0), None, "past the end of the chain");
    }

    #[test]
    fn picking_follows_the_polyline_world_transform() {
        let id = Id::new(7).expect("id");
        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        let transform = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        frame.push_polyline(Polyline::new(id, points, transform, [1.0; 4]));

        let hit = Ray::new(DVec3::new(10.5, 0.01, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(hit, 600.0), Some(id));
        let original_position = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(original_position, 600.0), None);
    }

    #[test]
    fn the_nearest_of_a_solid_and_a_path_wins_and_a_path_wins_a_tie() {
        let solid = Id::new(8).expect("id");
        let path = Id::new(9).expect("id");
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::new(0.0, 0.5, 0.0), DVec3::new(1.0, 0.5, 0.0)]);
        let above = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");

        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(solid, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push_polyline(Polyline::new(path, points.clone(), DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(above, 600.0), Some(path), "a path on a face stays selectable");

        frame = SceneFrame::default();
        frame.push(Drawable::new(solid, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        let below = DMat4::from_translation(DVec3::new(0.0, 0.0, -1.0));
        frame.push_polyline(Polyline::new(path, points, below, [1.0; 4]));
        assert_eq!(frame.pick(above, 600.0), Some(solid), "an occluded path loses to the solid");
    }

    #[test]
    fn malformed_polylines_are_skipped_instead_of_panicking() {
        let id = Id::new(10).expect("id");
        let ray = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        let mut frame = SceneFrame::default();
        let broken: Arc<[DVec3]> = Arc::from(vec![DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X]);
        frame.push_polyline(Polyline::new(id, broken, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None);

        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        let broken = DMat4::from_translation(DVec3::new(f64::INFINITY, 0.0, 0.0));
        frame.push_polyline(Polyline::new(id, points, broken, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None);

        let mut frame = SceneFrame::default();
        let single: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO]);
        frame.push_polyline(Polyline::new(id, single, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None, "one point has no segment");
    }

    #[test]
    fn selection_highlight_tints_every_selected_drawable() {
        let selected = Id::new(2).expect("id");
        let also_selected = Id::new(4).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(selected, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));
        frame.push(Drawable::new(also_selected, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));

        let highlighted = frame.with_selection_highlight(&[selected, also_selected]);
        assert_ne!(highlighted.drawables()[0].color, frame.drawables()[0].color);
        assert_ne!(highlighted.drawables()[2].color, frame.drawables()[2].color);
        assert_eq!(highlighted.drawables()[1].color, frame.drawables()[1].color);
        assert_eq!(frame.drawables()[0].color, [0.5, 0.5, 0.5, 1.0]);

        assert_eq!(frame.with_selection_highlight(&[]), frame);
        let missing = Id::new(99).expect("id");
        assert_eq!(frame.with_selection_highlight(&[missing]), frame);
    }
}
