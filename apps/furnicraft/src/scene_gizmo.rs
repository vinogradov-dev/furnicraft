//! Screen-space icons for authored cameras and lights.
//!
//! A camera or a light has no shaded mesh, so the viewport cannot pick it through a ray against a
//! drawable. Instead it projects the node's world frame and paints a glyph that shows its kind and
//! its direction: a camera frustum whose apex is the node's zero, a star for a light, and a dashed
//! arrow for a sun. A click on the glyph, including its frustum or rays, returns the node id
//! ([ADR 0036]). Picking stays in screen space and never touches the document.
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use eframe::egui;
use egui::{Color32, Pos2, Shape, Stroke, Ui, Vec2};
use furnicraft_geom::DVec3;
use furnicraft_model::{Id, LightKind, NodeQuery, Placement, Project, StructuralKind};
use furnicraft_scene::{Camera, Projection};

use crate::gizmo_math::{distance_to_segment, screen_point};

/// Length of a camera glyph from its apex to its base, in screen points.
const ICON_SIZE_POINTS: f64 = 30.0;

/// A camera glyph is twice as long as a light ray, so its direction reads at a glance.
const CAMERA_ICON_SIZE_POINTS: f64 = ICON_SIZE_POINTS * 2.0;

/// Radius around an icon centre that counts as a click, in points.
const ICON_PICK_RADIUS: f32 = 10.0;

/// Distance from a glyph line that still counts as a click, in points.
const EDGE_PICK_RADIUS: f32 = 4.0;

/// Radius of a light circle, in points.
const LIGHT_RADIUS: f32 = 5.0;

/// Inner radius of a light ray, in points.
const RAY_INNER: f32 = 8.0;

/// Outer radius of a light ray, in points.
const RAY_OUTER: f32 = 12.0;

/// Length of a sun direction arrow, in points.
const SUN_ARROW_LENGTH: f32 = 30.0;

/// The fill colour of a camera icon.
const CAMERA_COLOR: Color32 = Color32::from_rgb(120, 170, 255);

/// The fill colour of a light icon.
const LIGHT_COLOR: Color32 = Color32::from_rgb(255, 198, 92);

/// The outline of an unselected icon.
const OUTLINE_COLOR: Color32 = Color32::from_gray(42);

/// The outline and lines of a selected icon.
const SELECTED_COLOR: Color32 = Color32::WHITE;

/// Which glyph an icon uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconKind {
    /// An authored camera.
    Camera,
    /// An authored light.
    Light,
}

/// One authored camera or light projected into the viewport.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneIcon {
    /// The document node the icon stands for.
    pub(crate) id: Id,
    /// The glyph to draw.
    pub(crate) kind: IconKind,
    /// The projected world origin, in viewport points.
    pub(crate) position: Pos2,
    /// The aimed-at point: a camera base centre or a sun arrow tip.
    pub(crate) tip: Option<Pos2>,
    /// The filled camera body, when the glyph has one.
    body: Option<[Pos2; 4]>,
    /// The filled light circle radius, when the glyph has one.
    circle: Option<f32>,
    /// Every glyph line, used for painting and picking.
    segments: Vec<(Pos2, Pos2)>,
}

/// Projects every authored camera and light that is in front of the camera.
///
/// `previewed` carries the world placements of cameras and lights inside the nodes an active drag
/// moves. It wins over the document so a glyph follows the pointer in the same frame as the axis
/// handles; an empty slice reads the document.
pub(crate) fn icons(
    camera: Camera,
    viewport: egui::Rect,
    project: &Project,
    previewed: &[(Id, Placement)],
) -> Vec<SceneIcon> {
    let mut icons = Vec::new();
    for (kind, structural) in [(IconKind::Camera, StructuralKind::Camera), (IconKind::Light, StructuralKind::Light)] {
        for id in project.query(&NodeQuery::any().with_kind(structural)) {
            let Some(world) = previewed_world(project, id, previewed) else {
                continue;
            };
            let Some(position) = screen_point(camera, viewport, world.translation()) else {
                continue;
            };
            let icon = match kind {
                IconKind::Camera => camera_icon(camera, viewport, id, world, position),
                IconKind::Light => light_icon(camera, viewport, project, id, world, position),
            };
            if let Some(icon) = icon {
                icons.push(icon);
            }
        }
    }
    icons
}

/// A node's world placement, preferring a previewed one during a drag.
fn previewed_world(project: &Project, id: Id, previewed: &[(Id, Placement)]) -> Option<Placement> {
    previewed
        .iter()
        .find(|(node, _)| *node == id)
        .map(|(_, placement)| *placement)
        .or_else(|| project.world_placement(id))
}

/// Builds the camera frustum.
///
/// The node's zero is the apex of the view cone, so the glyph grows along the camera's local `-Z`
/// and its base plane sits one icon length ahead. A perspective camera ends in one apex; an
/// orthographic camera opens from a near rectangle at the origin to a second rectangle, so the two
/// projections read differently on screen.
fn camera_icon(camera: Camera, viewport: egui::Rect, id: Id, world: Placement, position: Pos2) -> Option<SceneIcon> {
    let origin = world.translation();
    let rotation = world.rotation();
    let right = rotation * DVec3::X;
    let up = rotation * DVec3::Y;
    let forward = rotation * -DVec3::Z;
    let size = icon_world_size(camera, viewport, origin, CAMERA_ICON_SIZE_POINTS);
    if !size.is_finite() || size <= 0.0 {
        return None;
    }

    let half_width = size * 0.16;
    let half_height = size * 0.12;
    let centre = origin + forward * size;
    let base = [
        centre + right * half_width + up * half_height,
        centre - right * half_width + up * half_height,
        centre - right * half_width - up * half_height,
        centre + right * half_width - up * half_height,
    ];
    let mut world_segments = vec![(base[0], base[1]), (base[1], base[2]), (base[2], base[3]), (base[3], base[0])];
    if matches!(camera.projection(), Projection::Orthographic { .. }) {
        let near = base.map(|corner| corner - forward * size);
        world_segments.extend([(near[0], near[1]), (near[1], near[2]), (near[2], near[3]), (near[3], near[0])]);
        world_segments.extend(near.iter().zip(base.iter()).map(|(start, end)| (*start, *end)));
    } else {
        world_segments.extend(base.into_iter().map(|corner| (corner, origin)));
    }

    let projected_body: [Pos2; 4] = project_points(camera, viewport, &base)?.try_into().ok()?;
    let tip = screen_point(camera, viewport, centre)?;
    let segments = project_segments(camera, viewport, &world_segments);
    Some(SceneIcon {
        id,
        kind: IconKind::Camera,
        position,
        tip: Some(tip),
        body: Some(projected_body),
        circle: None,
        segments,
    })
}

/// Builds a light star and, for a sun, a dashed arrow along its travel direction.
fn light_icon(
    camera: Camera,
    viewport: egui::Rect,
    project: &Project,
    id: Id,
    world: Placement,
    position: Pos2,
) -> Option<SceneIcon> {
    let light = project.light(id)?;
    let mut segments = Vec::new();
    for step in 0..8 {
        let angle = std::f32::consts::TAU * step as f32 / 8.0;
        let direction = Vec2::new(angle.cos(), angle.sin());
        segments.push((position + direction * RAY_INNER, position + direction * RAY_OUTER));
    }

    let tip = match light.kind() {
        LightKind::Sun => {
            let travel = world.rotation() * -DVec3::Z;
            let size = icon_world_size(camera, viewport, world.translation(), ICON_SIZE_POINTS);
            screen_point(camera, viewport, world.translation() + travel * size).and_then(|end| {
                let delta = end - position;
                let length = delta.length();
                if !length.is_finite() || length <= f32::EPSILON {
                    return None;
                }
                let direction = delta / length;
                let start = position + direction * (RAY_OUTER + 3.0);
                let arrow_tip = position + direction * SUN_ARROW_LENGTH;
                segments.extend(dashed(start, arrow_tip));
                let perpendicular = Vec2::new(-direction.y, direction.x);
                let base = arrow_tip - direction * 6.0;
                segments.push((arrow_tip, base + perpendicular * 3.0));
                segments.push((arrow_tip, base - perpendicular * 3.0));
                Some(arrow_tip)
            })
        }
        LightKind::Point => None,
    };

    Some(SceneIcon { id, kind: IconKind::Light, position, tip, body: None, circle: Some(LIGHT_RADIUS), segments })
}

/// A world size that projects to roughly `points` screen points at the node's depth.
fn icon_world_size(camera: Camera, viewport: egui::Rect, origin: DVec3, points: f64) -> f64 {
    let height = viewport.height().max(1.0) as f64;
    match camera.projection() {
        Projection::Perspective { fov_y } => {
            let distance = (origin - camera.eye()).length().max(1.0);
            2.0 * distance * (fov_y * 0.5).tan() / height * points
        }
        Projection::Orthographic { height: visible } => visible / height * points,
    }
}

/// Projects every point, returning `None` when any point is behind the camera.
fn project_points(camera: Camera, viewport: egui::Rect, points: &[DVec3]) -> Option<Vec<Pos2>> {
    points.iter().map(|point| screen_point(camera, viewport, *point)).collect()
}

/// Projects as many segments as the camera can see; a segment with a hidden endpoint is dropped.
fn project_segments(camera: Camera, viewport: egui::Rect, world_segments: &[(DVec3, DVec3)]) -> Vec<(Pos2, Pos2)> {
    world_segments
        .iter()
        .filter_map(|(start, end)| {
            Some((screen_point(camera, viewport, *start)?, screen_point(camera, viewport, *end)?))
        })
        .collect()
}

/// Splits a screen-space line into dashes.
fn dashed(start: Pos2, end: Pos2) -> Vec<(Pos2, Pos2)> {
    let delta = end - start;
    let length = delta.length();
    if !length.is_finite() || length <= f32::EPSILON {
        return Vec::new();
    }
    let direction = delta / length;
    let mut segments = Vec::new();
    let mut travelled = 0.0;
    while travelled < length {
        let dash_end = (travelled + 4.0).min(length);
        segments.push((start + direction * travelled, start + direction * dash_end));
        travelled += 8.0;
    }
    segments
}

/// The distance from `pointer` to the icon glyph, or `None` when the click misses it.
fn hit_distance(icon: &SceneIcon, pointer: Pos2) -> Option<f32> {
    let mut best = None;
    let centre = icon.position.distance(pointer);
    if centre <= ICON_PICK_RADIUS + icon.circle.unwrap_or(0.0) {
        best = Some(centre);
    }
    for (start, end) in &icon.segments {
        let Some(distance) = distance_to_segment(pointer, *start, *end) else {
            continue;
        };
        if distance <= EDGE_PICK_RADIUS {
            best = Some(best.map_or(distance, |closest: f32| closest.min(distance)));
        }
    }
    best
}

/// The closest camera or light icon under `pointer`, if any.
///
/// An icon is small and easy to miss behind a large panel, so it is tested before the mesh ray.
/// A tie goes to the first icon in tree order, keeping the result deterministic.
pub(crate) fn pick(
    camera: Camera,
    viewport: egui::Rect,
    project: &Project,
    pointer: Pos2,
    previewed: &[(Id, Placement)],
) -> Option<Id> {
    let mut best: Option<(f32, Id)> = None;
    for icon in icons(camera, viewport, project, previewed) {
        let Some(distance) = hit_distance(&icon, pointer) else {
            continue;
        };
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, icon.id));
        }
    }
    best.map(|(_, id)| id)
}

/// Paints every camera and light icon over the viewport image.
pub(crate) fn paint(
    ui: &Ui,
    viewport: egui::Rect,
    camera: Camera,
    project: &Project,
    selection: &[Id],
    previewed: &[(Id, Placement)],
) {
    let painter = ui.painter().with_clip_rect(viewport);
    for icon in icons(camera, viewport, project, previewed) {
        let selected = selection.contains(&icon.id);
        let color = match icon.kind {
            IconKind::Camera => CAMERA_COLOR,
            IconKind::Light => LIGHT_COLOR,
        };
        let outline = if selected { SELECTED_COLOR } else { OUTLINE_COLOR };
        let width = if selected { 2.0 } else { 1.0 };
        if let Some(body) = icon.body {
            painter.add(Shape::convex_polygon(body.to_vec(), color, Stroke::new(width, outline)));
        }
        if let Some(radius) = icon.circle {
            painter.circle_filled(icon.position, radius, color);
            painter.circle_stroke(icon.position, radius, Stroke::new(width, outline));
        }
        for (start, end) in &icon.segments {
            let line = if selected { SELECTED_COLOR } else { color };
            painter.line_segment([*start, *end], Stroke::new(width, line));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use eframe::egui;
    use egui::{Pos2, Rect, Vec2};
    use furnicraft_geom::{DQuat, DVec3};
    use furnicraft_model::{CameraProjection, LightKind, Placement};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    /// A view from `+Z` looking at the origin, so screen right is world `+X`.
    fn camera() -> Camera {
        Camera::new(DVec3::ZERO, 1000.0).expect("camera").looked_from(
            DVec3::new(0.0, 0.0, 1000.0),
            DVec3::X,
            DVec3::Y,
            -DVec3::Z,
        )
    }

    fn scene_project() -> (Project, Id, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let away = Placement::new(DVec3::new(200.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let light = project.add_light(root, "Key", away, LightKind::Sun, [1.0; 3], 1.0).expect("light");
        (project, camera, light)
    }

    #[test]
    fn both_kinds_project_to_icons_and_pick_by_screen_distance() {
        let (project, camera_id, light_id) = scene_project();
        let icons = icons(camera(), viewport(), &project, &[]);
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].id, camera_id);
        assert_eq!(icons[0].kind, IconKind::Camera);
        assert!(icons[0].body.is_some(), "a camera shows its body");
        assert_eq!(icons[1].id, light_id);
        assert!(icons[1].circle.is_some(), "a light shows a circle");

        // A click on the icon hits the exact node, not its parent container.
        assert_eq!(pick(camera(), viewport(), &project, icons[0].position, &[]), Some(camera_id));
        assert_eq!(pick(camera(), viewport(), &project, icons[1].position, &[]), Some(light_id));
        assert_eq!(pick(camera(), viewport(), &project, Pos2::new(5.0, 5.0), &[]), None);
    }

    #[test]
    fn a_camera_icon_points_along_its_view_direction_and_picks_at_its_base() {
        let mut project = sample_project();
        let root = project.root().id();
        let turn = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let id = project
            .add_camera(
                root,
                "Camera",
                Placement::new(DVec3::ZERO, turn).expect("placement"),
                CameraProjection::DEFAULT,
            )
            .expect("camera");

        let icons = icons(camera(), viewport(), &project, &[]);
        let icon = icons.iter().find(|icon| icon.id == id).expect("camera icon");
        let tip = icon.tip.expect("a camera has an aimed-at base");
        assert!(tip.x < icon.position.x - 5.0, "turning the camera about Y points its frustum to screen left");
        assert_eq!(pick(camera(), viewport(), &project, tip, &[]), Some(id), "a click on the base picks the camera");
    }

    #[test]
    fn the_camera_node_zero_is_the_apex_and_the_glyph_opens_forward() {
        let mut project = sample_project();
        let root = project.root().id();
        let placement = Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let id = project.add_camera(root, "Camera", placement, CameraProjection::DEFAULT).expect("camera");

        let icons = icons(camera(), viewport(), &project, &[]);
        let icon = icons.iter().find(|icon| icon.id == id).expect("camera icon");
        let origin = screen_point(camera(), viewport(), placement.translation()).expect("origin in front");
        assert!((icon.position - origin).length() < 1.0e-3, "the icon anchor is the node's zero");

        // A perspective glyph draws four edges from the base rectangle back to the apex, which is
        // the node origin; no other point of the glyph sits there.
        let edges_to_origin = icon.segments.iter().filter(|(_, end)| (*end - origin).length() < 1.0e-3).count();
        assert_eq!(edges_to_origin, 4, "the cone converges on the node origin, not on a body plane");

        // The base plane is one icon length ahead along the camera's local `-Z`.
        let tip = icon.tip.expect("a camera has an aimed-at base");
        let forward = placement.rotation() * -DVec3::Z;
        let size = icon_world_size(camera(), viewport(), placement.translation(), CAMERA_ICON_SIZE_POINTS);
        let expected =
            screen_point(camera(), viewport(), placement.translation() + forward * size).expect("base centre in front");
        assert!((tip - expected).length() < 1.0e-3, "the aimed-at point is the centre of the base plane");
        assert!(size > 0.0, "the base plane is ahead of the apex");
    }

    #[test]
    fn a_previewed_placement_moves_the_icon_before_the_command_lands() {
        let (project, _, light_id) = scene_project();
        let world = project.world_placement(light_id).expect("light world");
        let before = icons(camera(), viewport(), &project, &[])
            .into_iter()
            .find(|icon| icon.id == light_id)
            .expect("light icon")
            .position;

        let delta = DVec3::new(150.0, 0.0, 0.0);
        let moved = Placement::new(world.translation() + delta, world.rotation()).expect("placement");
        let after = icons(camera(), viewport(), &project, &[(light_id, moved)])
            .into_iter()
            .find(|icon| icon.id == light_id)
            .expect("light icon")
            .position;

        let expected = screen_point(camera(), viewport(), world.translation() + delta).expect("moved origin in front");
        assert!((after - expected).length() < 1.0e-3, "the icon follows the previewed world placement");
        assert!((after - before).length() > 10.0, "the preview moved the icon on screen");
    }

    #[test]
    fn a_sun_icon_shows_where_it_shines_and_a_point_light_has_no_direction() {
        let mut project = sample_project();
        let root = project.root().id();
        let turn = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let placement = Placement::new(DVec3::ZERO, turn).expect("placement");
        let sun = project.add_light(root, "Sun", placement, LightKind::Sun, [1.0; 3], 1.0).expect("sun");
        let point =
            project.add_light(root, "Fill", Placement::IDENTITY, LightKind::Point, [1.0; 3], 1.0).expect("point light");

        let icons = icons(camera(), viewport(), &project, &[]);
        let sun_icon = icons.iter().find(|icon| icon.id == sun).expect("sun icon");
        let point_icon = icons.iter().find(|icon| icon.id == point).expect("point icon");
        let tip = sun_icon.tip.expect("a sun has a direction arrow");
        assert!(tip.x < sun_icon.position.x - 20.0, "turning the sun about Y points its rays to screen left");
        assert!(point_icon.tip.is_none(), "a point light radiates in every direction");
    }

    #[test]
    fn a_node_behind_the_camera_has_no_icon() {
        let (mut project, camera_id, _) = scene_project();
        let behind = camera().eye() - camera().forward() * 100.0;
        project
            .set_node_placement(camera_id, Placement::new(behind, DQuat::IDENTITY).expect("placement"))
            .expect("move the camera");
        let icons = icons(camera(), viewport(), &project, &[]);
        assert!(icons.iter().all(|icon| icon.id != camera_id));
    }
}
