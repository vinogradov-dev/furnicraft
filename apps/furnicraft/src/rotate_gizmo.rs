//! The viewport rotate gizmo.
//!
//! Three rings are aligned with the world X, Y and Z axes and are projected over the selection's
//! manipulation pivot, the stored pivot of its primary node. A drag never mutates the document: it
//! turns a preview of every selected panel about that pivot, and the release dispatches one
//! `node.placement.set_many`. Because the rings are world-aligned, the preview turn is applied on
//! the left of each node's world rotation; the commit converts it into that node's parent frame and
//! pre-multiplies it onto the stored local quaternion. When the pivot is not the node's origin the
//! translation is adjusted too, so the ring centre stays a fixed point of the whole selection.
//!
//! Rings keep a roughly constant screen radius. A ring seen edge-on is not drawn: its plane is
//! nearly parallel to the view direction, the pointer cannot define an angle on it and a drag would
//! jump by half a turn.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Response, Stroke, Ui};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::Id;
use furnicraft_scene::Camera;

use crate::gizmo_math::{
    ACTIVE_COLOR, Axis, MoveTarget, PlacementCommit, Preview, PreviewKey, Target, commit_rotation, distance_to_segment,
    ray_plane, ray_to, screen_point,
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
    drawables: Vec<Id>,
    targets: Vec<MoveTarget>,
    pivot: DVec3,
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

    /// The drawable ids, pivot and world turn the renderer should preview, if a drag turned something.
    pub(crate) fn preview(&self) -> Option<Preview<'_>> {
        let drag = self.drag.as_ref()?;
        if drag.angle == 0.0 || !drag.angle.is_finite() {
            return None;
        }
        Some(Preview::Rotation {
            drawables: &drag.drawables,
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
                let turn = DQuat::from_axis_angle(drag.axis.direction(), drag.angle);
                let commit = commit_rotation(&drag.targets, drag.pivot, turn)?;
                return Ok(commit);
            }
            self.drag = Some(drag);
            return Ok(None);
        }

        self.hover = None;
        if pointer_over_widget {
            return Ok(None);
        }
        let Some(pivot) = target.anchor_pivot() else {
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
        let targets = target.move_targets();
        let Some(anchor) = targets.last() else {
            return Ok(None);
        };

        self.drag = Some(Drag {
            node: anchor.node,
            axis,
            drawables: target.drawables(),
            targets,
            pivot,
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
        let Some(pivot) = target.anchor_pivot() else {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    use furnicraft_model::Placement;

    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(800.0, 600.0))
    }

    fn move_target(node: u64, start_local: Placement, parent: Placement, pivot_local: DVec3) -> MoveTarget {
        MoveTarget { node: Id::new(node).expect("id"), start_local, parent, pivot_local }
    }

    fn world_pivot(target: &MoveTarget) -> DVec3 {
        target.parent.transform_point(target.start_local.transform_point(target.pivot_local))
    }

    fn drag(axis: Axis, angle: f64) -> Drag {
        Drag {
            node: Id::new(2).expect("id"),
            axis,
            drawables: Vec::new(),
            targets: Vec::new(),
            pivot: DVec3::ZERO,
            previous_angle: 0.0,
            angle,
        }
    }

    #[test]
    fn a_quarter_turn_premultiplies_the_world_turn_onto_the_local_frame() {
        let start = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(FRAC_PI_4)).expect("placement");
        let target = move_target(2, start, Placement::IDENTITY, DVec3::ZERO);
        let turn = DQuat::from_axis_angle(Axis::Y.direction(), FRAC_PI_2);
        let commit = commit_rotation(&[target], world_pivot(&target), turn).expect("valid rotation").expect("a turn");
        assert_eq!(commit.changes.len(), 1);
        let change = commit.changes[0];
        assert_eq!(change.placement.translation(), start.translation());
        let expected = (turn * start.rotation()).normalize();
        assert!(change.placement.rotation().dot(expected).abs() > 1.0 - 1.0e-12);
    }

    #[test]
    fn a_turn_is_expressed_in_the_parent_frame() {
        let parent = Placement::new(DVec3::ZERO, DQuat::from_rotation_z(FRAC_PI_4)).expect("placement");
        let start = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::from_rotation_z(0.2)).expect("placement");
        let target = move_target(2, start, parent, DVec3::new(1.0, 2.0, 3.0));
        let angle = FRAC_PI_2;
        let turn = DQuat::from_axis_angle(Axis::Y.direction(), angle);
        let commit = commit_rotation(&[target], world_pivot(&target), turn).expect("valid rotation").expect("a turn");
        let change = commit.changes[0];

        // In world space the preview and the commit must be the same left-multiplied turn.
        let world_before = parent.rotation() * start.rotation();
        let world_after = parent.rotation() * change.placement.rotation();
        let expected = turn * world_before;
        assert!(world_after.dot(expected).abs() > 1.0 - 1.0e-12, "world rotations diverged");

        // The pivot is fixed in the parent frame and therefore in world space too.
        let pivot_before = start.transform_point(target.pivot_local);
        let pivot_after = change.placement.transform_point(target.pivot_local);
        assert!(
            (pivot_before - pivot_after).length() < 2.0e-3,
            "pivot moved from {pivot_before:?} to {pivot_after:?} by more than the micrometre rounding"
        );
    }

    #[test]
    fn a_zero_or_full_turn_dispatches_nothing() {
        let target = move_target(2, Placement::IDENTITY, Placement::IDENTITY, DVec3::ZERO);
        let pivot = world_pivot(&target);
        for angle in [0.0, std::f64::consts::TAU, -std::f64::consts::TAU] {
            let turn = DQuat::from_axis_angle(Axis::X.direction(), angle);
            assert_eq!(commit_rotation(&[target], pivot, turn).expect("no error"), None);
        }
        assert_eq!(
            commit_rotation(&[], pivot, DQuat::from_axis_angle(Axis::X.direction(), 0.5)).expect("no error"),
            None
        );
    }

    #[test]
    fn a_turn_about_a_non_zero_pivot_keeps_that_point_fixed() {
        let start = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("placement");
        let target = move_target(2, start, Placement::IDENTITY, DVec3::new(1.0, 2.0, 3.0));
        let turn = DQuat::from_axis_angle(Axis::Z.direction(), FRAC_PI_2);
        let commit = commit_rotation(&[target], world_pivot(&target), turn).expect("valid rotation").expect("a turn");
        let change = commit.changes[0];

        let before = start.transform_point(target.pivot_local);
        let after = change.placement.transform_point(target.pivot_local);
        assert!((before - after).length() < 1.0e-9, "pivot moved from {before:?} to {after:?}");
        assert!(
            (change.placement.translation() - start.translation()).length() > 1.0,
            "a turn about a corner must adjust the translation"
        );
    }

    #[test]
    fn a_zero_pivot_preserves_the_translation_bit_for_bit() {
        let start = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(FRAC_PI_4)).expect("placement");
        let target = move_target(2, start, Placement::IDENTITY, DVec3::ZERO);
        let turn = DQuat::from_axis_angle(Axis::Y.direction(), FRAC_PI_2);
        let commit = commit_rotation(&[target], world_pivot(&target), turn).expect("valid rotation").expect("a turn");
        assert_eq!(commit.changes[0].placement.translation(), start.translation());
    }

    #[test]
    fn a_multi_selection_turns_every_node_about_the_anchor_pivot() {
        let first = Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let anchor = Placement::new(DVec3::new(0.0, 200.0, 0.0), DQuat::IDENTITY).expect("placement");
        let targets = [
            move_target(2, first, Placement::IDENTITY, DVec3::ZERO),
            move_target(3, anchor, Placement::IDENTITY, DVec3::new(0.0, 10.0, 0.0)),
        ];
        let pivot = world_pivot(&targets[1]);
        assert_eq!(pivot, DVec3::new(0.0, 210.0, 0.0));
        let turn = DQuat::from_axis_angle(Axis::Z.direction(), FRAC_PI_2);
        let commit = commit_rotation(&targets, pivot, turn).expect("valid rotation").expect("a turn");
        assert_eq!(commit.changes.len(), 2);

        // The anchor's stored pivot stays where it was; the other node orbits the same point.
        let anchor_after = commit.changes[1].placement.transform_point(targets[1].pivot_local);
        assert!((anchor_after - pivot).length() < 1.0e-9, "anchor pivot moved to {anchor_after:?}");
        let first_after = commit.changes[0].placement.translation();
        let expected = pivot + turn * (first.translation() - pivot);
        assert!((first_after - expected).length() < 1.0e-6, "first moved to {first_after:?}");
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
