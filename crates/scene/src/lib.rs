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

use std::f64::consts::{FRAC_PI_4, TAU};
use std::sync::Arc;

use furnicraft_geom::{DMat4, DVec3, MeshData};
use furnicraft_model::Id;
use thiserror::Error;

/// The default vertical field of view, in radians (45 degrees).
pub const DEFAULT_FOV_Y: f64 = std::f64::consts::FRAC_PI_4;

/// Lowest camera pitch, in radians. The camera never reaches the poles, so the up vector stays
/// well-defined.
pub const MIN_PITCH: f64 = -85.0 * std::f64::consts::PI / 180.0;

/// Highest camera pitch, in radians.
pub const MAX_PITCH: f64 = 85.0 * std::f64::consts::PI / 180.0;

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

/// An orbit camera in a right-handed, Z-up world.
///
/// The camera looks at `target` from a point on a sphere described by `yaw` (around world Z) and
/// `pitch` (above the XY plane). Physical lengths are millimetres; the camera lives in view state,
/// never in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    target: DVec3,
    yaw: f64,
    pitch: f64,
    distance: f64,
    fov_y: f64,
    near: f64,
    far: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            yaw: -FRAC_PI_4,
            pitch: 30.0 * std::f64::consts::PI / 180.0,
            distance: 1200.0,
            fov_y: DEFAULT_FOV_Y,
            near: 1.0,
            far: MAX_DISTANCE_MM,
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

    /// The vertical field of view in radians.
    pub const fn fov_y(self) -> f64 {
        self.fov_y
    }

    /// The camera position in world space.
    pub fn eye(self) -> DVec3 {
        let horizontal = self.pitch.cos();
        let offset = DVec3::new(horizontal * self.yaw.cos(), horizontal * self.yaw.sin(), self.pitch.sin());
        self.target + offset * self.distance
    }

    /// The unit direction from the camera towards its target.
    pub fn forward(self) -> DVec3 {
        (self.target - self.eye()).try_normalize().unwrap_or(-DVec3::Z)
    }

    /// The camera's right vector in world space.
    pub fn right(self) -> DVec3 {
        self.forward().cross(DVec3::Z).try_normalize().unwrap_or(DVec3::X)
    }

    /// The camera's up vector in world space.
    pub fn up(self) -> DVec3 {
        self.right().cross(self.forward()).try_normalize().unwrap_or(DVec3::Z)
    }

    /// The world-to-view matrix.
    pub fn view_matrix(self) -> DMat4 {
        let eye = self.eye();
        let forward = self.forward();
        let right = self.right();
        let up = right.cross(forward);
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

    /// The perspective projection matrix for a viewport with the given aspect ratio.
    ///
    /// The matrix matches wgpu's clip-space convention: right-handed view space, depth in `0..1`.
    /// A broken aspect ratio falls back to `1.0` instead of producing NaNs.
    pub fn projection_matrix(self, aspect: f64) -> DMat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(1.0e-4, 1.0e4) } else { 1.0 };
        let f = 1.0 / (self.fov_y * 0.5).tan();
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

    /// The combined view-projection matrix.
    pub fn view_projection(self, aspect: f64) -> DMat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// Orbits the camera by the given deltas in radians.
    ///
    /// Yaw wraps into `0..TAU`; pitch is clamped just short of the poles. Non-finite deltas are
    /// ignored.
    pub fn orbit(&mut self, delta_yaw: f64, delta_pitch: f64) {
        if !delta_yaw.is_finite() || !delta_pitch.is_finite() {
            return;
        }
        self.yaw = (self.yaw + delta_yaw).rem_euclid(TAU);
        self.pitch = (self.pitch + delta_pitch).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Pans the target across the screen plane, in points.
    ///
    /// The world distance per point is measured at the target, so the scene follows the pointer.
    /// Non-finite input is ignored.
    pub fn pan_pixels(&mut self, delta_x: f64, delta_y: f64, viewport_height: f64) {
        if !delta_x.is_finite() || !delta_y.is_finite() || !viewport_height.is_finite() {
            return;
        }
        let height = viewport_height.max(1.0);
        let world_per_point = 2.0 * self.distance * (self.fov_y * 0.5).tan() / height;
        self.target += (-self.right() * delta_x + self.up() * delta_y) * world_per_point;
    }

    /// Scales the orbit distance by `factor`.
    ///
    /// A factor below one moves closer; values outside the supported range are clamped and invalid
    /// factors are ignored.
    pub fn zoom(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE_MM, MAX_DISTANCE_MM);
    }
}

/// A floor grid on the world XY plane.
///
/// The renderer derives line geometry from these values; the document knows nothing about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
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

/// The key light and ambient term used to shade solid drawables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Unit direction from a surface towards the key light, in world space.
    pub key_direction: DVec3,
    /// Linear RGB intensity of the key light.
    pub key_color: [f32; 3],
    /// Fraction of the surface colour visible without direct light.
    pub ambient: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            key_direction: DVec3::new(0.35, -0.55, 0.75).try_normalize().unwrap_or(DVec3::Z),
            key_color: [1.0, 0.97, 0.92],
            ambient: 0.32,
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
    /// Rigid local-to-world transform.
    pub world_transform: DMat4,
    /// Linear RGB colour and opacity.
    pub color: [f32; 4],
}

impl Drawable {
    /// Creates a drawable from a document id, a mesh, a rigid world transform and a colour.
    pub fn new(id: Id, mesh: Arc<MeshData>, world_transform: DMat4, color: [f32; 4]) -> Self {
        Self { id, mesh, world_transform, color }
    }
}

/// Everything needed to draw and light one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    /// The active orbit camera.
    pub camera: Camera,
    /// The floor grid.
    pub grid: Grid,
    /// The key light.
    pub lighting: Lighting,
    /// The clear colour, in gamma space.
    pub background: [f32; 4],
    /// Shaded meshes in world space.
    pub drawables: Vec<Drawable>,
}

impl Default for SceneFrame {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            grid: Grid::default(),
            lighting: Lighting::default(),
            background: [0.10, 0.11, 0.13, 1.0],
            drawables: Vec::new(),
        }
    }
}

impl SceneFrame {
    /// Adds a drawable to the frame.
    pub fn push(&mut self, drawable: Drawable) {
        self.drawables.push(drawable);
    }

    /// The frame's drawables.
    pub fn drawables(&self) -> &[Drawable] {
        &self.drawables
    }
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
    fn scene_accepts_drawables() {
        let mut frame = SceneFrame::default();
        let mesh = Arc::new(MeshData::default());
        let id = Id::new(7).expect("id");
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.drawables().len(), 1);
        assert_eq!(frame.drawables()[0].id, id);
    }
}
