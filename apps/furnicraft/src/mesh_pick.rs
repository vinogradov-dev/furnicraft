//! Vertex picking for sub-object mesh editing.
//!
//! A mesh object's vertices are projected with the viewport camera and tested in screen space, the
//! same way the gizmo handles are hit-tested. Picking returns a vertex index of one object, never a
//! mesh pointer or a triangle index; the caller keeps the document immutable and only changes view
//! state.
//!
//! The object's world placement is composed once per pick rather than once per vertex, so a large
//! mesh does not walk the node tree for every index.

use eframe::egui::{Pos2, Rect};
use furnicraft_geom::DVec3;
use furnicraft_model::{Id, Mesh, Placement, Project};
use furnicraft_scene::Camera;
use furnicraft_units::Length;

use crate::gizmo_math::screen_point;

/// Radius around a projected vertex that counts as a hit, in viewport points.
pub(crate) const VERTEX_PICK_RADIUS: f32 = 10.0;

/// The mesh of an editable object and its world placement, resolved once for bulk work.
pub(crate) fn vertex_frame(project: &Project, object: Id) -> Option<(Placement, &Mesh)> {
    let mesh = project.object(object)?.geometry().as_mesh()?;
    let placement = project.world_placement(object)?;
    Some((placement, mesh))
}

/// The vertex closest to `pointer` within [`VERTEX_PICK_RADIUS`], if any.
///
/// Vertices behind the camera are skipped by the projection. A tie goes to the lower index, which
/// keeps picking deterministic when two vertices land on the same pixel.
pub(crate) fn pick_vertex(camera: Camera, viewport: Rect, project: &Project, object: Id, pointer: Pos2) -> Option<u32> {
    let (placement, mesh) = vertex_frame(project, object)?;
    let mut best: Option<(f32, u32)> = None;
    for (index, local) in mesh.positions().iter().enumerate() {
        let Ok(vertex) = u32::try_from(index) else {
            continue;
        };
        let Some(screen) = screen_point(camera, viewport, to_world(placement, *local)) else {
            continue;
        };
        let distance = screen.distance(pointer);
        if distance > VERTEX_PICK_RADIUS {
            continue;
        }
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, vertex));
        }
    }
    best.map(|(_, vertex)| vertex)
}

/// Applies the object's world placement to a vertex stored in micrometres.
pub(crate) fn to_world(placement: Placement, local: [Length; 3]) -> DVec3 {
    placement.transform_point(DVec3::new(
        local[0].millimetres_f64(),
        local[1].millimetres_f64(),
        local[2].millimetres_f64(),
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use eframe::egui::Vec2;
    use furnicraft_geom::DQuat;
    use furnicraft_model::{Geometry, Mesh, Placement};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    fn camera() -> Camera {
        Camera::new(DVec3::ZERO, 1000.0).expect("camera")
    }

    fn cube_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("object");
        (project, id)
    }

    fn millimetres(value: f64) -> Length {
        Length::from_micrometres((value * 1_000.0).round() as i64)
    }

    /// The screen position of one vertex, written out so the test does not use picking on itself.
    fn vertex_screen(camera: Camera, viewport: Rect, project: &Project, object: Id, vertex: u32) -> Option<Pos2> {
        let (placement, mesh) = vertex_frame(project, object)?;
        let local = *mesh.positions().get(usize::try_from(vertex).ok()?)?;
        screen_point(camera, viewport, to_world(placement, local))
    }

    #[test]
    fn a_vertex_is_projected_through_the_object_placement() {
        let (mut project, cube) = cube_project();
        let camera = camera();
        let identity = vertex_screen(camera, viewport(), &project, cube, 0).expect("vertex is visible");

        let placement = Placement::new(camera.right() * 250.0, DQuat::IDENTITY).expect("placement");
        project.set_node_placement(cube, placement).expect("move the object");
        let moved = vertex_screen(camera, viewport(), &project, cube, 0).expect("vertex is still visible");

        assert_ne!(moved, identity, "moving the object moves its projected vertices");
        assert!(moved.x > identity.x, "the object moved to the right");
        assert!((moved.y - identity.y).abs() < 1.0, "a sideways move keeps the height");
    }

    #[test]
    fn picking_finds_the_nearest_vertex_within_the_radius() {
        let (project, cube) = cube_project();
        let camera = camera();
        let viewport = viewport();
        let vertex = vertex_screen(camera, viewport, &project, cube, 4).expect("vertex is visible");

        assert_eq!(pick_vertex(camera, viewport, &project, cube, vertex), Some(4));
        let near = vertex + Vec2::new(3.0, -2.0);
        assert_eq!(pick_vertex(camera, viewport, &project, cube, near), Some(4));

        assert_eq!(pick_vertex(camera, viewport, &project, cube, Pos2::new(2.0, 2.0)), None);
    }

    #[test]
    fn an_unknown_object_or_vertex_is_not_picked() {
        let (project, cube) = cube_project();
        let camera = camera();
        let viewport = viewport();
        let pointer = vertex_screen(camera, viewport, &project, cube, 0).expect("vertex");

        assert_eq!(pick_vertex(camera, viewport, &project, Id::new(99).expect("id"), pointer), None);
        assert_eq!(vertex_screen(camera, viewport, &project, cube, 8), None);
    }

    #[test]
    fn a_vertex_behind_the_camera_is_not_picked() {
        let camera = camera();
        let behind = camera.eye() - camera.forward() * 100.0;
        let positions = vec![
            [millimetres(behind.x), millimetres(behind.y), millimetres(behind.z)],
            [Length::ZERO; 3],
            [millimetres(10.0), Length::ZERO, Length::ZERO],
        ];
        let mesh = Mesh::new(positions, vec![vec![0, 1, 2]]).expect("mesh");
        let mut project = sample_project();
        let root = project.root().id();
        let object = project.add_object(root, "Behind", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("object");

        assert_eq!(vertex_screen(camera, viewport(), &project, object, 0), None);
        assert_ne!(
            pick_vertex(camera, viewport(), &project, object, viewport().center()),
            Some(0),
            "a vertex behind the camera never wins a pick"
        );
    }
}
