//! World-space rays and the casts picking uses.
//!
//! A [`Ray`] starts at an origin in world millimetres and carries a unit direction; every hit
//! reports a distance in millimetres along it, and the camera that made the ray lives in
//! [`crate::camera`]. This module does not choose what a hit means: it returns a distance, an id
//! or a pair of numbers, and the caller orders and selects.
//!
//! The contract: a distance is positive, measured from the ray origin, and a hit behind the origin
//! is `None` rather than a negative number. The pick radius tolerance of a line chain is
//! [`LINE_PICK_RADIUS_POINTS`] viewport points at the hit depth, which [`crate::Camera::world_per_point`]
//! turns into millimetres, so a far path needs a wider world tolerance than a near one. A triangle
//! counts as crossed when its Möller–Trumbore determinant exceeds `1e-12`, and a segment whose
//! closest approach is not unique (a ray parallel to it) is no hit.
//!
//! It refuses non-finite input, a zero-length direction, an index outside a mesh and a world
//! transform that produces a non-finite vertex: each is skipped, never turned into a NaN distance.
//!
//! It does not test containment, occlusion or self-intersection: a ray cast answers only what it
//! crosses and how far. Picking a whole frame lives in [`crate::SceneFrame::pick`].
//!
//! Changes when: the ray primitive, a cast primitive or the pick tolerance changes.

use furnicraft_geom::{DMat4, DVec3, MeshData};

use crate::camera::Camera;
use crate::{Drawable, Polyline};

/// Pick radius around an unlit line chain, in viewport points.
///
/// A path is drawn one pixel wide, so a ray rarely crosses it exactly; this radius makes a click
/// near the line select the path without making a whole viewport region its target.
pub const LINE_PICK_RADIUS_POINTS: f64 = 6.0;
/// A world-space ray used for picking.
///
/// The direction is unit length. Construction returns `None` for non-finite input or a zero
/// direction, so a ray that reaches the scene is always usable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Readable inside the crate so `Camera` can build a fallback ray without re-normalising.
    pub(crate) origin: DVec3,
    /// See [`Ray::direction`]; the field is set only by a constructor that normalised it.
    pub(crate) direction: DVec3,
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
/// Distance along `ray` to the nearest triangle of `drawable`, if it crosses one.
///
/// A malformed vertex, index or transform is skipped rather than ending the cast.
pub(crate) fn ray_hit_drawable(ray: Ray, drawable: &Drawable) -> Option<f64> {
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
pub(crate) fn ray_hit_polyline(ray: Ray, polyline: &Polyline, camera: Camera, viewport_height: f64) -> Option<f64> {
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
