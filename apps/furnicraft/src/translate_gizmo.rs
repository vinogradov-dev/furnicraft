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
use egui::{Response, Ui};
use furnicraft_geom::DVec3;
use furnicraft_model::Id;
use furnicraft_scene::MAX_DISTANCE_MM;

use crate::gizmo_math::{
    Axis, MoveTarget, PlacementCommit, Preview, PreviewKey, Target, commit_translation, hit_axis, paint_axis_handles,
    ray_to,
};

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
        paint_axis_handles(
            ui,
            viewport,
            target.camera,
            origin_world,
            self.hover,
            self.drag.as_ref().map(|drag| drag.axis),
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::f64::consts::FRAC_PI_2;

    use egui::Pos2;
    use furnicraft_geom::DQuat;
    use furnicraft_model::Placement;
    use furnicraft_scene::Camera;

    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(800.0, 600.0))
    }

    fn origin_screen(camera: Camera) -> Pos2 {
        crate::gizmo_math::screen_point(camera, viewport(), camera.target()).expect("the target is in front")
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
