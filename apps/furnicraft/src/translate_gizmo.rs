//! The viewport translate gizmo.
//!
//! Three world-axis handles are projected over the selection's manipulation pivot, the stored pivot
//! of its primary node. A drag never mutates the document: it moves a preview of every selected
//! panel, and the release dispatches one `node.placement.set_many`. Each node's world-space delta is
//! converted into its own parent frame and rounded to micrometres, so a selection whose nodes have
//! different parents still moves rigidly and the inspector agrees with the document after the drag.
//!
//! Handles keep a constant screen length. An axis within about ten degrees of the view direction is
//! not drawn: its projected direction is unstable and a pointer drag would move it by metres.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Response, Shape, Stroke, Ui, Vec2};
use furnicraft_geom::DVec3;
use furnicraft_model::Id;
use furnicraft_scene::{Camera, MAX_DISTANCE_MM};

use crate::gizmo_math::{
    ACTIVE_COLOR, Axis, MoveTarget, PlacementCommit, Preview, PreviewKey, Target, commit_translation, ray_to,
    screen_direction, screen_point,
};

/// Length of an axis handle, in points.
const HANDLE_LENGTH: f32 = 84.0;

/// Radius inside which a pointer hits a handle, in points.
const HIT_RADIUS: f32 = 8.0;

/// Part of a handle next to the origin that never starts a drag, in points.
const ORIGIN_DEAD_ZONE: f32 = 10.0;

/// Axes closer than this to the view direction are not drawn.
const VIEW_ALIGNMENT_LIMIT: f64 = 0.985;

/// A projected handle: the axis, its screen-space tip and its unit screen direction.
#[derive(Clone, Copy, Debug)]
struct Handle {
    axis: Axis,
    tip: Pos2,
    direction: Vec2,
}

/// One continuous axis drag.
#[derive(Debug)]
struct Drag {
    node: Id,
    axis: Axis,
    panels: Vec<Id>,
    targets: Vec<MoveTarget>,
    start_origin: DVec3,
    start_parameter: f64,
    delta: DVec3,
}

/// Persistent state of the translate gizmo.
#[derive(Debug, Default)]
pub(crate) struct TranslateGizmo {
    drag: Option<Drag>,
    hover: Option<Axis>,
    consumed_click: bool,
}

impl TranslateGizmo {
    /// Whether an axis drag is in progress.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether this frame's pointer release belongs to a gizmo drag and must not select.
    pub(crate) fn consumed_click(&self) -> bool {
        self.consumed_click
    }

    /// The panel ids and world delta the renderer should preview, if a drag moved something.
    pub(crate) fn preview(&self) -> Option<Preview<'_>> {
        let drag = self.drag.as_ref()?;
        if drag.delta == DVec3::ZERO {
            return None;
        }
        Some(Preview::Translation { panels: &drag.panels, delta: drag.delta })
    }

    /// The preview identity used to avoid re-uploading an unchanged drag.
    pub(crate) fn preview_key(&self) -> Option<PreviewKey> {
        let drag = self.drag.as_ref()?;
        if drag.delta == DVec3::ZERO {
            return None;
        }
        Some(PreviewKey::Translation {
            node: drag.node,
            delta: [drag.delta.x.to_bits(), drag.delta.y.to_bits(), drag.delta.z.to_bits()],
        })
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
                && let Some(parameter) = ray_to(pointer, viewport, target.camera)
                    .closest_parameter_on_line(drag.start_origin, drag.axis.direction())
            {
                let scalar = (parameter - drag.start_parameter).clamp(-MAX_DISTANCE_MM, MAX_DISTANCE_MM);
                if scalar.is_finite() {
                    drag.delta = drag.axis.direction() * scalar;
                }
            }
            if !down {
                self.consumed_click = true;
                let commit = commit_translation(&drag.targets, drag.delta)?;
                return Ok(commit);
            }
            self.drag = Some(drag);
            return Ok(None);
        }

        self.hover = None;
        if pointer_over_widget {
            return Ok(None);
        }
        let targets = target.move_targets();
        let Some(anchor) = targets.last() else {
            return Ok(None);
        };
        let Some(origin_world) = target.project.pivot_world(anchor.node) else {
            return Ok(None);
        };
        let Some(pointer) = response.hover_pos() else {
            return Ok(None);
        };
        let Some(axis) = hit_axis(target.camera, viewport, origin_world, pointer) else {
            return Ok(None);
        };
        self.hover = Some(axis);
        if !ui.input(|input| input.pointer.primary_pressed()) {
            return Ok(None);
        }

        let Some(parameter) =
            ray_to(pointer, viewport, target.camera).closest_parameter_on_line(origin_world, axis.direction())
        else {
            return Ok(None);
        };

        self.drag = Some(Drag {
            node: anchor.node,
            axis,
            panels: target.panels(),
            targets,
            start_origin: origin_world,
            start_parameter: parameter,
            delta: DVec3::ZERO,
        });
        if !ui.input(|input| input.pointer.primary_down()) {
            self.drag = None;
            self.consumed_click = true;
        }
        Ok(None)
    }

    /// Draws the handles over the viewport.
    pub(crate) fn paint(&self, ui: &Ui, viewport: egui::Rect, target: Target<'_>) {
        let origin_world = if let Some(drag) = &self.drag {
            drag.start_origin + drag.delta
        } else {
            let Some(origin) = target.anchor_pivot() else {
                return;
            };
            origin
        };
        let Some(origin) = screen_point(target.camera, viewport, origin_world) else {
            return;
        };
        if !viewport.expand(64.0).contains(origin) {
            return;
        }
        let handles = handles(target.camera, viewport, origin_world, origin);
        let painter = ui.painter().with_clip_rect(viewport);
        let center_tone = if self.drag.is_some() { Color32::from_gray(140) } else { Color32::from_gray(220) };
        painter.circle_filled(origin, 3.0, center_tone);

        for handle in handles {
            let active = self.drag.as_ref().is_some_and(|drag| drag.axis == handle.axis);
            let hovered = self.hover == Some(handle.axis);
            let mut color = if active || hovered { ACTIVE_COLOR } else { handle.axis.color() };
            if self.drag.is_some() && !active {
                color = color.gamma_multiply(0.4);
            }
            let width = if active || hovered { 3.5 } else { 2.2 };
            painter.line_segment([origin, handle.tip], Stroke::new(width, color));

            let perpendicular = Vec2::new(-handle.direction.y, handle.direction.x);
            let base = handle.tip - handle.direction * 9.0;
            painter.add(Shape::convex_polygon(
                vec![handle.tip, base + perpendicular * 4.0, base - perpendicular * 4.0],
                color,
                Stroke::NONE,
            ));
            let label = handle.tip + handle.direction * 12.0;
            painter.text(
                label,
                Align2::CENTER_CENTER,
                handle.axis.label(),
                FontId::proportional(12.0),
                if active || hovered { ACTIVE_COLOR } else { Color32::from_gray(230) },
            );
        }
    }
}

/// The axis whose handle is nearest to `pointer`, ignoring the dead zone around the origin.
fn hit_axis(camera: Camera, viewport: egui::Rect, origin_world: DVec3, pointer: Pos2) -> Option<Axis> {
    let origin = screen_point(camera, viewport, origin_world)?;
    if !viewport.expand(64.0).contains(origin) {
        return None;
    }
    let mut best: Option<(f32, Axis)> = None;
    for handle in handles(camera, viewport, origin_world, origin) {
        let distance = distance_to_handle(pointer, origin, handle.tip);
        let Some(distance) = distance else {
            continue;
        };
        if distance <= HIT_RADIUS && best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, handle.axis));
        }
    }
    best.map(|(_, axis)| axis)
}

/// Distance from `point` to the segment, or `None` inside the dead zone next to the origin.
fn distance_to_handle(point: Pos2, start: Pos2, end: Pos2) -> Option<f32> {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let length = length_squared.sqrt();
    let along = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    if along * length < ORIGIN_DEAD_ZONE {
        return None;
    }
    Some((point - (start + segment * along)).length())
}

/// The projected handles of the selected node, skipping axes that point at the camera.
fn handles(camera: Camera, viewport: egui::Rect, origin_world: DVec3, origin: Pos2) -> Vec<Handle> {
    let step = (camera.distance() * 0.01).max(1.0);
    let mut handles = Vec::with_capacity(Axis::ALL.len());
    for axis in Axis::ALL {
        if axis.direction().dot(camera.forward()).abs() > VIEW_ALIGNMENT_LIMIT {
            continue;
        }
        let Some(end) = screen_point(camera, viewport, origin_world + axis.direction() * step) else {
            continue;
        };
        let Some(direction) = screen_direction(end - origin) else {
            continue;
        };
        handles.push(Handle { axis, tip: origin + direction * HANDLE_LENGTH, direction });
    }
    handles
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::f64::consts::FRAC_PI_2;

    use furnicraft_geom::DQuat;
    use furnicraft_model::Placement;

    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    fn origin_screen(camera: Camera) -> Pos2 {
        screen_point(camera, viewport(), camera.target()).expect("the target is in front")
    }

    fn move_target(node: u64, parent_rotation: DQuat, start_local: Placement) -> MoveTarget {
        MoveTarget {
            node: Id::new(node).expect("id"),
            start_local,
            parent: Placement::new(DVec3::ZERO, parent_rotation).expect("placement"),
            pivot_local: DVec3::ZERO,
        }
    }

    #[test]
    fn the_default_view_draws_three_distinct_handles() {
        let camera = Camera::default();
        let handles = handles(camera, viewport(), camera.target(), origin_screen(camera));
        assert_eq!(handles.len(), 3);
        for handle in &handles {
            assert!(handle.tip.is_finite());
            assert!((handle.direction.length() - 1.0).abs() < 1.0e-5);
            assert!((handle.tip - origin_screen(camera)).length() > 80.0);
        }
        let z = handles.iter().find(|handle| handle.axis == Axis::Z).expect("Z handle");
        assert!(z.direction.y < -0.5, "Z should point up, direction was {:?}", z.direction);
    }

    #[test]
    fn a_view_aligned_axis_has_no_handle() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        let handles = handles(camera, viewport(), camera.target(), origin_screen(camera));
        assert!(handles.iter().all(|handle| handle.axis != Axis::Z), "handles were {handles:?}");
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn the_handle_nearest_the_pointer_wins_and_the_origin_is_dead() {
        let camera = Camera::default();
        let origin_world = camera.target();
        let origin = origin_screen(camera);
        let handles = handles(camera, viewport(), origin_world, origin);
        let x = handles.iter().find(|handle| handle.axis == Axis::X).expect("X handle");
        let pointer = origin + (x.tip - origin) * 0.6;
        assert_eq!(hit_axis(camera, viewport(), origin_world, pointer), Some(Axis::X));
        assert_eq!(hit_axis(camera, viewport(), origin_world, origin), None);
        assert_eq!(hit_axis(camera, viewport(), origin_world, origin + Vec2::new(300.0, 300.0)), None);
    }

    #[test]
    fn a_world_delta_is_converted_into_a_rotated_parent_frame() {
        let target = move_target(2, DQuat::from_rotation_z(FRAC_PI_2), Placement::IDENTITY);
        let commit =
            commit_translation(&[target], DVec3::new(10.0, 0.0, 0.0)).expect("valid placement").expect("a move");
        assert_eq!(commit.changes.len(), 1);
        let change = commit.changes[0];
        let expected = DVec3::new(0.0, -10.0, 0.0);
        assert!(
            (change.placement.translation() - expected).length() < 1.0e-9,
            "translation was {:?}",
            change.placement.translation()
        );
        assert_eq!(change.placement.rotation(), Placement::IDENTITY.rotation());
    }

    #[test]
    fn a_batch_moves_a_selection_rigidly_across_different_parents() {
        let turned = Placement::new(DVec3::new(500.0, 0.0, 0.0), DQuat::from_rotation_z(FRAC_PI_2)).expect("placement");
        let targets = [
            move_target(2, DQuat::IDENTITY, Placement::IDENTITY),
            move_target(
                3,
                turned.rotation(),
                Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::IDENTITY).expect("placement"),
            ),
        ];
        let commit = commit_translation(&targets, DVec3::new(10.0, 4.0, 0.0)).expect("valid").expect("a move");
        assert_eq!(commit.changes.len(), 2);
        assert_eq!(commit.changes[0].node, Id::new(2).expect("id"));
        assert_eq!(commit.changes[1].node, Id::new(3).expect("id"));

        // Every node receives the same world delta, expressed in its own parent frame.
        for (change, target) in commit.changes.iter().zip(targets) {
            let world_before = target.parent.transform_point(target.start_local.translation());
            let world_after = target.parent.transform_point(change.placement.translation());
            assert!(
                (world_after - world_before - DVec3::new(10.0, 4.0, 0.0)).length() < 1.0e-9,
                "world shift was {:?}",
                world_after - world_before
            );
        }
    }

    #[test]
    fn a_zero_delta_or_a_rounded_out_delta_dispatches_nothing() {
        let target = move_target(2, DQuat::IDENTITY, Placement::IDENTITY);
        assert_eq!(commit_translation(&[target], DVec3::ZERO).expect("no error"), None);
        assert_eq!(commit_translation(&[target], DVec3::new(1.0e-7, 0.0, 0.0)).expect("no error"), None);
        assert_eq!(commit_translation(&[], DVec3::new(1.0, 0.0, 0.0)).expect("no error"), None);
    }

    #[test]
    fn a_parallel_axis_ray_has_no_start_parameter() {
        let camera = Camera::default();
        let origin = camera.target();
        let pointer = origin_screen(camera);
        let ray = ray_to(pointer, viewport(), camera);
        assert!(ray.closest_parameter_on_line(origin, ray.direction()).is_none());
    }
}
