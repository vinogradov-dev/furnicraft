//! Screen-space icons for authored cameras and lights.
//!
//! A camera or a light has no shaded mesh, so the viewport cannot pick it through a ray against a
//! drawable. Instead it projects the node's world origin and paints a small glyph; a click within
//! the glyph radius returns the node id ([ADR 0036]). Picking stays in screen space and never
//! touches the document.
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Stroke, Ui};
use furnicraft_model::{Id, NodeQuery, Project, StructuralKind};
use furnicraft_scene::Camera;

use crate::gizmo_math::screen_point;

/// Radius of a painted icon, in points.
const ICON_RADIUS: f32 = 9.0;

/// Radius around an icon centre that counts as a click, in points.
pub(crate) const ICON_PICK_RADIUS: f32 = 12.0;

/// The fill colour of a camera icon.
const CAMERA_COLOR: Color32 = Color32::from_rgb(120, 170, 255);

/// The fill colour of a light icon.
const LIGHT_COLOR: Color32 = Color32::from_rgb(255, 198, 92);

/// Which glyph an icon uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconKind {
    /// An authored camera.
    Camera,
    /// An authored light.
    Light,
}

/// One authored camera or light projected into the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneIcon {
    /// The document node the icon stands for.
    pub(crate) id: Id,
    /// The glyph to draw.
    pub(crate) kind: IconKind,
    /// The projected world origin, in viewport points.
    pub(crate) position: Pos2,
}

/// Projects every authored camera and light that is in front of the camera.
pub(crate) fn icons(camera: Camera, viewport: egui::Rect, project: &Project) -> Vec<SceneIcon> {
    let mut icons = Vec::new();
    for (kind, structural) in [(IconKind::Camera, StructuralKind::Camera), (IconKind::Light, StructuralKind::Light)] {
        for id in project.query(&NodeQuery::any().with_kind(structural)) {
            let Some(world) = project.world_placement(id) else {
                continue;
            };
            let Some(position) = screen_point(camera, viewport, world.translation()) else {
                continue;
            };
            icons.push(SceneIcon { id, kind, position });
        }
    }
    icons
}

/// The closest camera or light icon under `pointer`, if any.
///
/// An icon is small and easy to miss behind a large panel, so it is tested before the mesh ray.
/// A tie goes to the first icon in tree order, keeping the result deterministic.
pub(crate) fn pick(camera: Camera, viewport: egui::Rect, project: &Project, pointer: Pos2) -> Option<Id> {
    let mut best: Option<(f32, Id)> = None;
    for icon in icons(camera, viewport, project) {
        let distance = icon.position.distance(pointer);
        if distance > ICON_PICK_RADIUS {
            continue;
        }
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, icon.id));
        }
    }
    best.map(|(_, id)| id)
}

/// Paints every camera and light icon over the viewport image.
pub(crate) fn paint(ui: &Ui, viewport: egui::Rect, camera: Camera, project: &Project, selection: &[Id]) {
    let painter = ui.painter().with_clip_rect(viewport);
    for icon in icons(camera, viewport, project) {
        let selected = selection.contains(&icon.id);
        let (fill, label) = match icon.kind {
            IconKind::Camera => (CAMERA_COLOR, "C"),
            IconKind::Light => (LIGHT_COLOR, "L"),
        };
        let outline = if selected { Color32::WHITE } else { Color32::from_gray(42) };
        let width = if selected { 2.0 } else { 1.0 };
        painter.circle_filled(icon.position, ICON_RADIUS, fill);
        painter.circle_stroke(icon.position, ICON_RADIUS, Stroke::new(width, outline));
        painter.text(icon.position, Align2::CENTER_CENTER, label, FontId::proportional(11.0), Color32::from_gray(24));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use eframe::egui;
    use egui::{Pos2, Rect, Vec2};
    use furnicraft_geom::DVec3;
    use furnicraft_model::{CameraProjection, LightKind, Placement};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    fn camera() -> Camera {
        Camera::new(DVec3::ZERO, 1000.0).expect("camera")
    }

    fn scene_project() -> (Project, Id, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let light = project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("light");
        (project, camera, light)
    }

    #[test]
    fn both_kinds_project_to_icons_and_pick_by_screen_distance() {
        let (project, camera_id, light_id) = scene_project();
        let icons = icons(camera(), viewport(), &project);
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].id, camera_id);
        assert_eq!(icons[0].kind, IconKind::Camera);
        assert_eq!(icons[1].id, light_id);

        // A click on the icon hits the exact node, not its parent container.
        assert_eq!(pick(camera(), viewport(), &project, icons[0].position), Some(camera_id));
        assert_eq!(pick(camera(), viewport(), &project, Pos2::new(5.0, 5.0)), None);
    }

    #[test]
    fn a_node_behind_the_camera_has_no_icon() {
        let (mut project, camera_id, _) = scene_project();
        let behind = camera().eye() - camera().forward() * 100.0;
        project
            .set_node_placement(camera_id, Placement::new(behind, furnicraft_geom::DQuat::IDENTITY).expect("placement"))
            .expect("move the camera");
        let icons = icons(camera(), viewport(), &project);
        assert!(icons.iter().all(|icon| icon.id != camera_id));
    }
}
