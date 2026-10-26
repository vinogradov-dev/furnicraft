//! Shared math and presentation for the viewport transform gizmos.
//!
//! The translate and rotate gizmos project world points with the camera, hit-test their handles in
//! screen space and dispatch one `node.placement.set` per finished drag. Keeping the axis colours,
//! the projection and the commit type here means the two gizmos cannot drift apart.

use eframe::egui;
use egui::{Color32, Pos2, Vec2};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::{Id, Placement, Project};
use furnicraft_scene::{Camera, Ray};

/// Gamma-space colours for the three axes.
const X_COLOR: Color32 = Color32::from_rgb(232, 80, 80);
const Y_COLOR: Color32 = Color32::from_rgb(92, 204, 116);
const Z_COLOR: Color32 = Color32::from_rgb(92, 140, 255);

/// Colour of the handle under the pointer or being dragged.
pub(crate) const ACTIVE_COLOR: Color32 = Color32::from_rgb(255, 220, 96);

/// One local or world axis of a gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// All three axes in display order.
    pub(crate) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The unit vector of this axis in the frame the gizmo draws it in.
    pub(crate) fn direction(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }

    /// The one-letter label shown next to a handle.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    /// The gamma-space colour of this axis.
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::X => X_COLOR,
            Self::Y => Y_COLOR,
            Self::Z => Z_COLOR,
        }
    }
}

/// What a gizmo points at in one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Target<'a> {
    /// The camera that projects the handles.
    pub(crate) camera: Camera,
    /// The document whose node is being placed.
    pub(crate) project: &'a Project,
    /// The selected node, if any.
    pub(crate) selected: Option<Id>,
}

/// The placement a finished drag asks the shell to dispatch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacementCommit {
    /// The node whose placement changed.
    pub(crate) node: Id,
    /// The new placement relative to its parent.
    pub(crate) placement: Placement,
}

/// The scene change a gizmo drag is previewing, without touching the document.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Preview<'a> {
    /// Shift the listed panels by a world-space delta.
    Translation {
        /// Panels to move.
        panels: &'a [Id],
        /// World-space shift.
        delta: DVec3,
    },
    /// Turn the listed panels about a world-space pivot.
    Rotation {
        /// Panels to turn.
        panels: &'a [Id],
        /// World-space point the turn is centred on.
        pivot: DVec3,
        /// World-space turn.
        rotation: DQuat,
    },
}

/// Bit-exact identity of a preview, used to skip an unchanged re-upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewKey {
    /// A translation preview: node and delta bits.
    Translation {
        /// The node being dragged.
        node: Id,
        /// `x`, `y`, `z` of the delta as raw bits.
        delta: [u64; 3],
    },
    /// A rotation preview: node and turn bits.
    Rotation {
        /// The node being dragged.
        node: Id,
        /// The accumulated local turn as raw bits.
        angle: u64,
    },
}

/// The world point under a viewport position.
pub(crate) fn ray_to(pointer: Pos2, viewport: egui::Rect, camera: Camera) -> Ray {
    let (x, y) = normalized_pointer(pointer, viewport);
    camera.ray_ndc(x, y, aspect(viewport))
}

/// A world point in viewport points, or `None` when it is behind the camera.
pub(crate) fn screen_point(camera: Camera, viewport: egui::Rect, point: DVec3) -> Option<Pos2> {
    let ndc = camera.project_ndc(point, aspect(viewport))?;
    let x = viewport.left() + (ndc.x as f32 + 1.0) * 0.5 * viewport.width();
    let y = viewport.top() + (1.0 - ndc.y as f32) * 0.5 * viewport.height();
    (x.is_finite() && y.is_finite()).then_some(Pos2::new(x, y))
}

/// Distance from `point` to a screen-space segment.
pub(crate) fn distance_to_segment(point: Pos2, start: Pos2, end: Pos2) -> Option<f32> {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let along = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    Some((point - (start + segment * along)).length())
}

/// The point where a ray meets a plane, or `None` when they are parallel or the plane is behind.
pub(crate) fn ray_plane(ray: Ray, plane_point: DVec3, normal: DVec3) -> Option<DVec3> {
    let denominator = ray.direction().dot(normal);
    if denominator.abs() < 1.0e-9 {
        return None;
    }
    let distance = (plane_point - ray.origin()).dot(normal) / denominator;
    if !distance.is_finite() || distance <= 0.0 {
        return None;
    }
    Some(ray.origin() + ray.direction() * distance)
}

fn normalized_pointer(pointer: Pos2, viewport: egui::Rect) -> (f64, f64) {
    let width = f64::from(viewport.width());
    let height = f64::from(viewport.height());
    if width <= 0.0 || height <= 0.0 {
        return (0.0, 0.0);
    }
    let x = (f64::from(pointer.x - viewport.left()) / width) * 2.0 - 1.0;
    let y = 1.0 - (f64::from(pointer.y - viewport.top()) / height) * 2.0;
    (x, y)
}

fn aspect(viewport: egui::Rect) -> f64 {
    let width = f64::from(viewport.width());
    let height = f64::from(viewport.height());
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 { width / height } else { 1.0 }
}

/// A unit screen direction, or `None` when the projected segment is too short to be stable.
pub(crate) fn screen_direction(delta: Vec2) -> Option<Vec2> {
    (delta.length() >= 0.5).then(|| delta.normalized())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn axes_have_distinct_directions_and_colours() {
        assert_eq!(Axis::ALL.len(), 3);
        assert_eq!(Axis::X.direction(), DVec3::X);
        assert_eq!(Axis::Y.direction(), DVec3::Y);
        assert_eq!(Axis::Z.direction(), DVec3::Z);
        assert_ne!(Axis::X.color(), Axis::Y.color());
        assert_ne!(Axis::Y.color(), Axis::Z.color());
    }

    #[test]
    fn a_ray_meets_a_plane_only_in_front_of_the_camera() {
        let ray = Ray::new(DVec3::new(0.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        let hit = ray_plane(ray, DVec3::ZERO, DVec3::Z).expect("hit");
        assert!(hit.length() < 1.0e-12);

        let behind = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::Z).expect("ray");
        assert!(ray_plane(behind, DVec3::ZERO, DVec3::Z).is_none());

        let parallel = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::X).expect("ray");
        assert!(ray_plane(parallel, DVec3::ZERO, DVec3::Z).is_none());
    }

    #[test]
    fn screen_segments_reject_degenerate_input() {
        assert!(distance_to_segment(Pos2::ZERO, Pos2::ZERO, Pos2::ZERO).is_none());
        assert_eq!(distance_to_segment(Pos2::new(0.0, 3.0), Pos2::ZERO, Pos2::new(10.0, 0.0)), Some(3.0));
        assert!(screen_direction(Vec2::ZERO).is_none());
        assert_eq!(screen_direction(Vec2::new(0.0, 4.0)), Some(Vec2::new(0.0, 1.0)));
    }
}
