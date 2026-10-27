//! The viewport rotate gizmo.
//!
//! Three rings are aligned with the world X, Y and Z axes and are projected over the selected node's
//! manipulation pivot. A drag never mutates the document: it turns a preview of the selected panels
//! about that pivot, and the release dispatches one `node.placement.set`. Because the rings are
//! world-aligned, the preview turn is applied on the left of the node's world rotation; the commit
//! converts it into the parent frame and pre-multiplies it onto the stored local quaternion. When
//! the pivot is not the node's origin the translation is adjusted too, so the ring centre stays a
//! fixed point of the object.
//!
//! Rings keep a roughly constant screen radius. A ring seen edge-on is not drawn: its plane is
//! nearly parallel to the view direction, the pointer cannot define an angle on it and a drag would
//! jump by half a turn.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Response, Stroke, Ui};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::{Id, Placement};
use furnicraft_scene::Camera;

use crate::gizmo_math::{
    ACTIVE_COLOR, Axis, PlacementCommit, Preview, PreviewKey, Target, distance_to_segment, ray_plane, ray_to,
    screen_point,
};

/// Radius of a ring relative to the camera distance.
const RING_RADIUS_FACTOR: f64 = 0.09;

/// Number of line segments in a ring.
const RING_SEGMENTS: usize = 64;

/// Radius inside which a pointer hits a ring, in points.
const HIT_RADIUS: f32 = 7.0;

/// Rings whose plane is closer than this to edge-on are not drawn or grabbed.
const EDGE_ON_LIMIT: f64 = 0.2;

/// One continuous rotation drag.
#[derive(Debug)]
struct Drag {
    node: Id,
    axis: Axis,
    panels: Vec<Id>,
    pivot: DVec3,
    pivot_local: DVec3,
    start_local: Placement,
    parent_rotation: DQuat,
    previous_angle: f64,
    angle: f64,
}

/// Persistent state of the rotate gizmo.
#[derive(Debug, Default)]
pub(crate) struct RotateGizmo {
    drag: Option<Drag>,
    hover: Option<Axis>,
    consumed_click: bool,
}

impl RotateGizmo {
    /// Whether a ring drag is in progress.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether this frame's pointer release belongs to a gizmo drag and must not select.
    pub(crate) fn consumed_click(&self) -> bool {
        self.consumed_click
    }

    /// The panel ids, pivot and world turn the renderer should preview, if a drag turned something.
    pub(crate) fn preview(&self) -> Option<Preview<'_>> {
        let drag = self.drag.as_ref()?;
        if drag.angle == 0.0 || !drag.angle.is_finite() {
            return None;
        }
        Some(Preview::Rotation {
            panels: &drag.panels,
            pivot: drag.pivot,
            rotation: DQuat::from_axis_angle(drag.axis.direction(), drag.angle),
        })
    }

    /// The preview identity used to avoid re-uploading an unchanged drag.
    pub(crate) fn preview_key(&self) -> Option<PreviewKey> {
        let drag = self.drag.as_ref()?;
        if drag.angle == 0.0 || !drag.angle.is_finite() {
            return None;
        }
        Some(PreviewKey::Rotation { node: drag.node, angle: drag.angle.to_bits() })
    }

    /// Starts, continues or finishes a drag and returns a placement to dispatch on release.
    ///
    /// `pointer_over_widget` is true when the pointer is over the orientation widget, which owns
    /// its own clicks. The caller must run this before camera orbit and discard the pointer's click
    /// when [`Self::consumed_click`] is true.
    pub(crate) fn update(
        &mut self,
        ui: &Ui,
        response: &Response,
        viewport: egui::Rect,
        target: Target<'_>,
        pointer_over_widget: bool,
    ) -> Result<Option<PlacementCommit>, String> {
        self.consumed_click = false;
        if let Some(mut drag) = self.drag.take() {
            let (pointer, down, escape) = ui.input(|input| {
                (input.pointer.latest_pos(), input.pointer.primary_down(), input.key_pressed(egui::Key::Escape))
            });
            if escape {
                self.consumed_click = true;
                return Ok(None);
            }
            if let Some(pointer) = pointer
                && let Some(angle) = pointer_angle(pointer, viewport, target.camera, drag.pivot, drag.axis.direction())
            {
                let step = wrap_angle(angle - drag.previous_angle);
                if step.is_finite() {
                    drag.angle += step;
                    drag.previous_angle = angle;
                }
            }
            if !down {
                self.consumed_click = true;
                let commit = commit_rotation(&drag)?;
                return Ok(commit);
            }
            self.drag = Some(drag);
            return Ok(None);
        }

        self.hover = None;
        if pointer_over_widget {
            return Ok(None);
        }
        let Some(pivot) = target_rings(target) else {
            return Ok(None);
        };
        let Some(pointer) = response.hover_pos() else {
            return Ok(None);
        };
        let Some(axis) = hit_ring(target.camera, viewport, pivot, pointer) else {
            return Ok(None);
        };
        self.hover = Some(axis);
        if !ui.input(|input| input.pointer.primary_pressed()) {
            return Ok(None);
        }

        let axis_world = axis.direction();
        let Some(start_angle) = pointer_angle(pointer, viewport, target.camera, pivot, axis_world) else {
            return Ok(None);
        };
        let Some(selected) = target.selected else {
            return Ok(None);
        };
        let Some(start_local) = target.project.node(selected).map(|node| node.placement()) else {
            return Ok(None);
        };
        let Some(pivot_local) = target.project.pivot_local(selected) else {
            return Ok(None);
        };
        let Some(parent) = target.project.parent_of(selected) else {
            return Ok(None);
        };
        let Some(parent_rotation) = target.project.world_placement(parent).map(Placement::rotation) else {
            return Ok(None);
        };
        let Some(panels) = target.project.panel_ids_under(selected) else {
            return Ok(None);
        };

        self.drag = Some(Drag {
            node: selected,
            axis,
            panels,
            pivot,
            pivot_local,
            start_local,
            parent_rotation,
            previous_angle: start_angle,
            angle: 0.0,
        });
        if !ui.input(|input| input.pointer.primary_down()) {
            self.drag = None;
            self.consumed_click = true;
        }
        Ok(None)
    }

    /// Draws the rings over the viewport.
    pub(crate) fn paint(&self, ui: &Ui, viewport: egui::Rect, target: Target<'_>) {
        let Some(pivot) = target_rings(target) else {
            return;
        };
        let Some(origin) = screen_point(target.camera, viewport, pivot) else {
            return;
        };
        if !viewport.expand(64.0).contains(origin) {
            return;
        }
        let radius = world_radius(target.camera);
        let painter = ui.painter().with_clip_rect(viewport);
        let center_tone = if self.drag.is_some() { Color32::from_gray(140) } else { Color32::from_gray(220) };
        painter.circle_filled(origin, 3.0, center_tone);

        for axis in Axis::ALL {
            let axis_world = axis.direction();
            if axis_world.dot(target.camera.forward()).abs() < EDGE_ON_LIMIT {
                continue;
            }
            let Some(points) = ring_screen_points(target.camera, viewport, pivot, axis_world, radius) else {
                continue;
            };
            let active = self.drag.as_ref().is_some_and(|drag| drag.axis == axis);
            let hovered = self.hover == Some(axis);
            let mut color = if active || hovered { ACTIVE_COLOR } else { axis.color() };
            if self.drag.is_some() && !active {
                color = color.gamma_multiply(0.4);
            }
            let width = if active || hovered { 3.5 } else { 2.2 };
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], Stroke::new(width, color));
            }
            if let Some(label) = points.iter().min_by(|a, b| a.y.total_cmp(&b.y)) {
                painter.text(
                    *label - egui::vec2(0.0, 12.0),
                    Align2::CENTER_CENTER,
                    axis.label(),
                    FontId::proportional(12.0),
                    if active || hovered { ACTIVE_COLOR } else { Color32::from_gray(230) },
                );
            }
        }
    }
}

/// The world pivot of the node the rings belong to, if it has a placement to edit.
///
/// The rings are deliberately world-aligned: they do not follow the node's rotation, so a turn is
/// always about a world X, Y or Z axis and the ring centre is the stored pivot.
fn target_rings(target: Target<'_>) -> Option<DVec3> {
    let selected = target.selected?;
    // The root defines the world frame and has no placement to edit.
    target.project.parent_of(selected)?;
    target.project.pivot_world(selected)
}

/// A stable orthonormal basis for the plane perpendicular to `normal`.
fn ring_basis(normal: DVec3) -> Option<(DVec3, DVec3)> {
    let seed = if normal.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let u = (seed - normal * seed.dot(normal)).try_normalize()?;
    Some((u, normal.cross(u)))
}

/// World radius that keeps a ring roughly [`RING_RADIUS_FACTOR`] of the camera distance.
fn world_radius(camera: Camera) -> f64 {
    (camera.distance() * RING_RADIUS_FACTOR).max(1.0)
}

/// The projected ring, closed by repeating its first point.
fn ring_screen_points(
    camera: Camera,
    viewport: egui::Rect,
    pivot: DVec3,
    axis_world: DVec3,
    radius: f64,
) -> Option<Vec<Pos2>> {
    let (u, v) = ring_basis(axis_world)?;
    let mut points = Vec::with_capacity(RING_SEGMENTS + 1);
    for index in 0..=RING_SEGMENTS {
        let turn = std::f64::consts::TAU * index as f64 / RING_SEGMENTS as f64;
        let point = pivot + (u * turn.cos() + v * turn.sin()) * radius;
        points.push(screen_point(camera, viewport, point)?);
    }
    Some(points)
}

/// The ring whose projected outline is nearest to `pointer`.
fn hit_ring(camera: Camera, viewport: egui::Rect, pivot: DVec3, pointer: Pos2) -> Option<Axis> {
    let radius = world_radius(camera);
    let mut best: Option<(f32, Axis)> = None;
    for axis in Axis::ALL {
        let axis_world = axis.direction();
        if axis_world.dot(camera.forward()).abs() < EDGE_ON_LIMIT {
            continue;
        }
        let Some(points) = ring_screen_points(camera, viewport, pivot, axis_world, radius) else {
            continue;
        };
        for pair in points.windows(2) {
            let Some(distance) = distance_to_segment(pointer, pair[0], pair[1]) else {
                continue;
            };
            if distance <= HIT_RADIUS && best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, axis));
            }
        }
    }
    best.map(|(_, axis)| axis)
}

/// The pointer's angle on the ring plane, measured from the basis of [`ring_basis`].
fn pointer_angle(pointer: Pos2, viewport: egui::Rect, camera: Camera, pivot: DVec3, axis_world: DVec3) -> Option<f64> {
    let hit = ray_plane(ray_to(pointer, viewport, camera), pivot, axis_world)?;
    let (u, v) = ring_basis(axis_world)?;
    let radial = hit - pivot;
    let radial = radial - axis_world * radial.dot(axis_world);
    let radial = radial.try_normalize()?;
    Some(radial.dot(v).atan2(radial.dot(u)))
}

/// Wraps an angle into `(-π, π]`, so a pointer crossing the seam does not jump a full turn.
fn wrap_angle(angle: f64) -> f64 {
    let wrapped = angle.rem_euclid(std::f64::consts::TAU);
    if wrapped > std::f64::consts::PI { wrapped - std::f64::consts::TAU } else { wrapped }
}

/// The placement to dispatch at the end of a drag, or `None` when nothing turned.
fn commit_rotation(drag: &Drag) -> Result<Option<PlacementCommit>, String> {
    if drag.angle == 0.0 {
        return Ok(None);
    }
    if !drag.angle.is_finite() {
        return Err("the drag produced a non-finite rotation".to_owned());
    }
    // The preview turns the node about a world axis. Convert that world turn into the parent frame
    // so it can be pre-multiplied onto the stored local rotation.
    let delta_world = DQuat::from_axis_angle(drag.axis.direction(), drag.angle);
    let delta_parent = (drag.parent_rotation.inverse() * delta_world * drag.parent_rotation).normalize();
    if !delta_parent.is_finite() {
        return Err("the drag produced a non-finite rotation".to_owned());
    }
    let start_rotation = drag.start_local.rotation();
    let rotation = (delta_parent * start_rotation).normalize();
    if !rotation.is_finite() {
        return Err("the drag produced a non-finite rotation".to_owned());
    }
    let placement = if drag.pivot_local == DVec3::ZERO {
        // A zero pivot keeps the stored translation exact.
        Placement::new(drag.start_local.translation(), rotation).map_err(|error| error.to_string())?
    } else {
        // Keep the pivot fixed in the parent frame and round the new translation to micrometres.
        let pivot_parent = drag.start_local.transform_point(drag.pivot_local);
        let translation = pivot_parent + delta_parent * (drag.start_local.translation() - pivot_parent);
        if !translation.is_finite() {
            return Err("the drag moved the node to a non-finite position".to_owned());
        }
        let placement = Placement::new(translation, rotation).map_err(|error| error.to_string())?;
        let [x, y, z] = placement
            .translation_lengths()
            .ok_or_else(|| "the drag moved the node outside the editable millimetre range".to_owned())?;
        placement.with_translation(x, y, z).map_err(|error| error.to_string())?
    };
    if placement.same_transform(drag.start_local) {
        Ok(None)
    } else {
        Ok(Some(PlacementCommit { node: drag.node, placement }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(800.0, 600.0))
    }

    fn drag(axis: Axis, angle: f64) -> Drag {
        Drag {
            node: Id::new(2).expect("id"),
            axis,
            panels: Vec::new(),
            pivot: DVec3::ZERO,
            pivot_local: DVec3::ZERO,
            start_local: Placement::IDENTITY,
            parent_rotation: DQuat::IDENTITY,
            previous_angle: 0.0,
            angle,
        }
    }

    #[test]
    fn a_quarter_turn_premultiplies_the_world_turn_onto_the_local_frame() {
        let start = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(FRAC_PI_4)).expect("placement");
        let drag = Drag { start_local: start, ..drag(Axis::Y, FRAC_PI_2) };
        let commit = commit_rotation(&drag).expect("valid rotation").expect("a turn");
        assert_eq!(commit.placement.translation(), start.translation());
        let expected = (DQuat::from_axis_angle(Axis::Y.direction(), FRAC_PI_2) * start.rotation()).normalize();
        assert!(commit.placement.rotation().dot(expected).abs() > 1.0 - 1.0e-12);
    }

    #[test]
    fn a_turn_is_expressed_in_the_parent_frame() {
        let parent_rotation = DQuat::from_rotation_z(FRAC_PI_4);
        let start = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::from_rotation_z(0.2)).expect("placement");
        let pivot_local = DVec3::new(1.0, 2.0, 3.0);
        let angle = FRAC_PI_2;
        let drag = Drag { start_local: start, pivot_local, parent_rotation, ..drag(Axis::Y, angle) };
        let commit = commit_rotation(&drag).expect("valid rotation").expect("a turn");

        // In world space the preview and the commit must be the same left-multiplied turn.
        let world_before = parent_rotation * start.rotation();
        let world_after = parent_rotation * commit.placement.rotation();
        let expected = DQuat::from_axis_angle(Axis::Y.direction(), angle) * world_before;
        assert!(world_after.dot(expected).abs() > 1.0 - 1.0e-12, "world rotations diverged");

        // The pivot is fixed in the parent frame and therefore in world space too.
        let pivot_before = start.transform_point(pivot_local);
        let pivot_after = commit.placement.transform_point(pivot_local);
        assert!(
            (pivot_before - pivot_after).length() < 2.0e-3,
            "pivot moved from {pivot_before:?} to {pivot_after:?} by more than the micrometre rounding"
        );
    }

    #[test]
    fn a_zero_or_full_turn_dispatches_nothing() {
        assert_eq!(commit_rotation(&drag(Axis::X, 0.0)).expect("no error"), None);
        assert_eq!(commit_rotation(&drag(Axis::X, std::f64::consts::TAU)).expect("no error"), None);
        assert_eq!(commit_rotation(&drag(Axis::X, -std::f64::consts::TAU)).expect("no error"), None);
    }

    #[test]
    fn a_turn_about_a_non_zero_pivot_keeps_that_point_fixed() {
        let start = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("placement");
        let pivot_local = DVec3::new(1.0, 2.0, 3.0);
        let drag = Drag { start_local: start, pivot_local, ..drag(Axis::Z, FRAC_PI_2) };
        let commit = commit_rotation(&drag).expect("valid rotation").expect("a turn");

        let before = start.transform_point(pivot_local);
        let after = commit.placement.transform_point(pivot_local);
        assert!((before - after).length() < 1.0e-9, "pivot moved from {before:?} to {after:?}");
        assert!(
            (commit.placement.translation() - start.translation()).length() > 1.0,
            "a turn about a corner must adjust the translation"
        );
    }

    #[test]
    fn a_zero_pivot_preserves_the_translation_bit_for_bit() {
        let start = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(FRAC_PI_4)).expect("placement");
        let drag = Drag { start_local: start, ..drag(Axis::Y, FRAC_PI_2) };
        let commit = commit_rotation(&drag).expect("valid rotation").expect("a turn");
        assert_eq!(commit.placement.translation(), start.translation());
    }

    #[test]
    fn the_preview_turns_about_the_world_axis_and_keeps_the_pivot() {
        let gizmo = RotateGizmo { drag: Some(drag(Axis::Z, FRAC_PI_2)), ..Default::default() };
        let Some(Preview::Rotation { pivot, rotation, .. }) = gizmo.preview() else {
            return;
        };
        assert_eq!(pivot, DVec3::ZERO);
        let point = rotation * DVec3::X;
        assert!((point - DVec3::Y).length() < 1.0e-12, "point was {point:?}");
        assert!(gizmo.preview_key().is_some());
    }

    #[test]
    fn the_pointer_angle_is_measured_on_the_ring_plane() {
        let camera = Camera::default();
        let pivot = camera.target();
        let center = screen_point(camera, viewport(), pivot).expect("centre in front");
        assert!(pointer_angle(center, viewport(), camera, pivot, DVec3::Z).is_none());

        let axis_world = DVec3::Z;
        let (u, _) = ring_basis(axis_world).expect("basis");
        let point = pivot + u * world_radius(camera);
        let pointer = screen_point(camera, viewport(), point).expect("ring point in front");
        let angle = pointer_angle(pointer, viewport(), camera, pivot, axis_world).expect("angle");
        assert!(angle.abs() < 1.0e-5, "angle was {angle}");
    }

    #[test]
    fn wrap_angle_crosses_the_seam_without_a_full_turn_jump() {
        assert!((wrap_angle(std::f64::consts::PI + 0.1) - (-std::f64::consts::PI + 0.1)).abs() < 1.0e-12);
        assert!((wrap_angle(0.3) - 0.3).abs() < 1.0e-12);
        assert!((wrap_angle(-0.3) + 0.3).abs() < 1.0e-12);
    }

    #[test]
    fn the_default_view_shows_three_world_aligned_rings_and_hides_an_edge_on_one() {
        let camera = Camera::default();
        let pivot = camera.target();
        let mut visible = 0;
        for axis in Axis::ALL {
            let axis_world = axis.direction();
            if axis_world.dot(camera.forward()).abs() >= EDGE_ON_LIMIT {
                visible += 1;
                assert!(ring_screen_points(camera, viewport(), pivot, axis_world, world_radius(camera)).is_some());
            }
        }
        assert_eq!(visible, 3, "the default isometric view should show all rings");

        let mut top = Camera::default();
        top.look_from(DVec3::Z);
        let z = Axis::Z.direction();
        assert!(z.dot(top.forward()).abs() >= EDGE_ON_LIMIT);
        let x = Axis::X.direction();
        assert!(x.dot(top.forward()).abs() < EDGE_ON_LIMIT, "the X ring is edge-on from above");
    }

    #[test]
    fn a_pointer_on_a_projected_ring_hits_that_world_axis() {
        let camera = Camera::default();
        let pivot = camera.target();
        let radius = world_radius(camera);
        let axis_world = Axis::Z.direction();
        let points = ring_screen_points(camera, viewport(), pivot, axis_world, radius).expect("ring");
        let pointer = points[1];
        assert_eq!(hit_ring(camera, viewport(), pivot, pointer), Some(Axis::Z));
        assert_eq!(hit_ring(camera, viewport(), pivot, Pos2::new(790.0, 590.0)), None);
    }
}
