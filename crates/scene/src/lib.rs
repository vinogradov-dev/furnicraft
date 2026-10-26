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

use std::f64::consts::{FRAC_PI_4, FRAC_PI_8, PI, TAU};
use std::sync::Arc;

use furnicraft_geom::{DMat4, DVec2, DVec3, MeshData};
use furnicraft_model::Id;
use thiserror::Error;

/// The default vertical field of view, in radians (45 degrees).
pub const DEFAULT_FOV_Y: f64 = FRAC_PI_4;

/// Lowest camera pitch, in radians. The camera never reaches the poles, so the up vector stays
/// well-defined.
pub const MIN_PITCH: f64 = -85.0 * PI / 180.0;

/// Highest camera pitch, in radians.
pub const MAX_PITCH: f64 = 85.0 * PI / 180.0;

/// How far outside the viewport a pointer ray is still accepted, in normalized device coordinates.
///
/// A drag can leave the viewport, so a little beyond ±1 is valid. The limit keeps a hostile or
/// accidental huge coordinate from turning the ray into an unusable vector.
const NDC_LIMIT: f64 = 16.0;

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
            yaw: 2.8 * FRAC_PI_8,
            pitch: 30.0 * PI / 180.0,
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
        let half_height = (self.fov_y * 0.5).tan();
        let direction = self.forward() + self.right() * (x * half_height * aspect) + self.up() * (y * half_height);
        let origin = self.eye();
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
        let clip = self.view_projection(aspect) * point.extend(1.0);
        if !clip.is_finite() || clip.w <= 1.0e-9 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let ndc = DVec2::new(ndc.x, ndc.y);
        ndc.is_finite().then_some(ndc)
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

    /// Points the camera at its target from the given world direction.
    ///
    /// `direction` is the offset from the target to the eye, not the viewing direction. A zero or
    /// non-finite direction leaves the camera unchanged. Pitch is clamped to the camera limits, so
    /// top and bottom views stop five degrees short of the pole and keep a defined up vector.
    pub fn look_from(&mut self, direction: DVec3) {
        if !direction.is_finite() {
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
    /// factors are ignored.
    pub fn zoom(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE_MM, MAX_DISTANCE_MM);
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
            key_direction: DVec3::new(-0.35, 0.55, 0.75).try_normalize().unwrap_or(DVec3::Z),
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

    /// The document id of the nearest drawable hit by `ray`.
    ///
    /// Only triangles are tested, so a malformed mesh or a drawable that misses the ray is skipped.
    /// Picking returns a document id, never a mesh index or a pointer: the caller can select it or
    /// send it to a command.
    pub fn pick(&self, ray: Ray) -> Option<Id> {
        let mut nearest: Option<(f64, Id)> = None;
        for drawable in &self.drawables {
            let Some(distance) = ray_hit_drawable(ray, drawable) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, drawable.id));
            }
        }
        nearest.map(|(_, id)| id)
    }

    /// A copy of the frame with every selected drawable tinted for picking feedback.
    ///
    /// The base frame keeps its material colours, so clearing the selection restores them without
    /// rebuilding meshes. Passing the panels below a selected container highlights the container as
    /// one object.
    pub fn with_selection_highlight(&self, selected: &[Id]) -> Self {
        let mut frame = self.clone();
        if selected.is_empty() {
            return frame;
        }
        for drawable in &mut frame.drawables {
            if !selected.contains(&drawable.id) {
                continue;
            }
            for (channel, tint) in drawable.color.iter_mut().zip(SELECTION_TINT) {
                let base = f64::from(*channel);
                *channel = (base * (1.0 - SELECTION_MIX) + tint * SELECTION_MIX) as f32;
            }
        }
        frame
    }

    /// A copy of the frame with the given drawables shifted by a world-space translation.
    ///
    /// This is a drag preview: it moves the listed drawables without touching the document, so the
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
        frame
    }
}

/// Linear RGB colour mixed into the selected drawable.
const SELECTION_TINT: [f64; 3] = [1.0, 0.55, 0.15];

/// Fraction of [`SELECTION_TINT`] mixed into the base colour.
const SELECTION_MIX: f64 = 0.35;

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

/// Möller–Trumbore intersection; returns the positive distance along the ray.
fn ray_triangle(ray: Ray, v0: DVec3, v1: DVec3, v2: DVec3) -> Option<f64> {
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
    fn picking_returns_the_nearest_document_id() {
        let near = Id::new(2).expect("id");
        let far = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(near, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(far, quad_mesh(), DMat4::from_translation(DVec3::new(0.0, 0.0, 1.0)), [1.0; 4]));

        let from_above = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_above), Some(far));
        let from_below = Ray::new(DVec3::new(0.5, 0.5, -10.0), DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_below), Some(near));

        let miss = Ray::new(DVec3::new(2.0, 2.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(miss), None);
    }

    #[test]
    fn picking_follows_the_world_transform() {
        let id = Id::new(4).expect("id");
        let mut frame = SceneFrame::default();
        let transform = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        frame.push(Drawable::new(id, quad_mesh(), transform, [1.0; 4]));

        let hit = Ray::new(DVec3::new(10.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(hit), Some(id));
        let original_position = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(original_position), None);
    }

    #[test]
    fn malformed_meshes_are_skipped_instead_of_panicking() {
        let id = Id::new(5).expect("id");
        let mesh =
            Arc::new(MeshData { positions: vec![[0.0; 3]], normals: Vec::new(), indices: vec![0, 1, 2, 0, 0, 0] });
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        let ray = Ray::new(DVec3::new(0.0, 0.0, 1.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(ray), None);
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
