//! The viewport gizmo for contour edit mode.
//!
//! The gizmo edits the editor's working copy, never the document: dragging a corner rewrites the
//! draft ring, and double-clicking a side splits it there. Painting shows the draft over the solid,
//! with the original ring faintly behind it when the draft differs, so the user sees the change
//! before the single command that applies it.
//!
//! The pointer is mapped onto the node's local XY plane through its world placement. An arc corner
//! is projected onto its own circle first, because an edit may slide an arc corner along the arc but
//! never change its centre or radius.

use eframe::egui::{self, Pos2, Response, Ui};
use furnicraft_geom::{DVec2, DVec3};
use furnicraft_model::{Contour, ContourSegment, ContourSegmentId, Placement, Project};
use furnicraft_scene::Camera;
use furnicraft_ui::{ContourEdit, ContourTarget, Editor};
use furnicraft_units::Length;

use crate::gizmo_math::{ACTIVE_COLOR, distance_to_segment, length_from_millimetres, ray_plane, ray_to, screen_point};

/// Radius of an unselected corner handle, in points.
const CORNER_RADIUS: f32 = 4.5;
/// Pointer distance that grabs a corner, in points.
const CORNER_HIT_RADIUS: f32 = 9.0;
/// Pointer distance that picks a side for a split, in points.
const SEGMENT_HIT_RADIUS: f32 = 7.0;
/// Sampling tolerance for the painted ring, in micrometres.
const RING_TOLERANCE_MICROMETRES: f64 = 500.0;
/// Fraction of a side that stays unsplittable next to each corner.
const SPLIT_MARGIN: f64 = 0.02;
/// Maximum number of screen segments used to draw one arc.
const MAX_ARC_STEPS: usize = 64;

/// Colour of the ring the edit started from.
const BASE_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(150, 150, 150, 160);
/// Colour of the working copy.
const DRAFT_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 200, 255);
/// Colour of an untouched corner handle.
const CORNER_COLOR: egui::Color32 = egui::Color32::from_rgb(240, 240, 240);

/// One corner drag that is in progress.
#[derive(Clone, Copy, Debug)]
struct CornerDrag {
    target: ContourTarget,
    segment: ContourSegmentId,
}

/// Pointer state of the contour gizmo; the working copy itself belongs to the editor.
#[derive(Debug, Default)]
pub(crate) struct ContourGizmo {
    drag: Option<CornerDrag>,
    hover_corner: Option<ContourSegmentId>,
    hover_segment: Option<ContourSegmentId>,
}

impl ContourGizmo {
    /// Drops every drag and hover state, for example when the mode ends.
    pub(crate) fn reset(&mut self) {
        self.drag = None;
        self.hover_corner = None;
        self.hover_segment = None;
    }

    /// Whether a corner is being dragged right now.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The cursor the viewport should show while the mode is active.
    pub(crate) fn cursor(&self) -> egui::CursorIcon {
        if self.drag.is_some() {
            egui::CursorIcon::Grabbing
        } else if self.hover_corner.is_some() {
            egui::CursorIcon::Grab
        } else if self.hover_segment.is_some() {
            egui::CursorIcon::PointingHand
        } else {
            egui::CursorIcon::Default
        }
    }

    /// Applies the pointer to the working copy; returns whether the draft changed.
    ///
    /// The gizmo owns the left button while the mode is live: a drag moves the grabbed corner and a
    /// double-click splits the side under the pointer. A rejected edit leaves the draft at its last
    /// valid shape, so a corner simply stops when the ring would become invalid.
    pub(crate) fn update(
        &mut self,
        response: &Response,
        rect: egui::Rect,
        camera: Camera,
        project: &Project,
        editor: &mut Editor,
    ) -> Result<bool, String> {
        let Some(edit) = editor.contour_edit() else {
            self.reset();
            return Ok(false);
        };
        let target = edit.target();
        let Some(placement) = project.world_placement(target.id()) else {
            self.reset();
            return Ok(false);
        };
        // This gizmo owns a ring; a chain draft is drawn and dragged by the path gizmo.
        let Some(draft) = edit.draft().as_ring().cloned() else {
            self.hover_corner = None;
            self.hover_segment = None;
            return Ok(false);
        };
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let corner = pointer.and_then(|pointer| corner_at(camera, rect, placement, &draft, pointer));
        let segment = pointer.and_then(|pointer| {
            if corner.is_some() { None } else { segment_at(camera, rect, placement, &draft, pointer) }
        });
        self.hover_corner = corner;
        self.hover_segment = segment;

        let mut changed = false;
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.drag = corner.map(|segment| CornerDrag { target, segment });
        }
        if let Some(drag) = self.drag {
            if drag.target != target {
                self.drag = None;
            } else if response.dragged_by(egui::PointerButton::Primary)
                && let Some(local) =
                    response.interact_pointer_pos().and_then(|pointer| local_point(pointer, rect, camera, placement))
                && let Some(local) = project_arc_corner(&draft, drag.segment, local)
                && let (Some(x), Some(y)) = (length_from_millimetres(local.x), length_from_millimetres(local.y))
                && editor.contour_edit_move_corner(drag.segment, [x, y]).is_ok()
            {
                changed = true;
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) {
                self.drag = None;
            }
        }

        if self.drag.is_none()
            && response.double_clicked_by(egui::PointerButton::Primary)
            && let Some(segment) = segment
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some(local) = local_point(pointer, rect, camera, placement)
            && let Some(curve) = draft.segment(segment)
            && let Some(point) = split_point(curve, local)
        {
            // A rejected split is a normal near-corner double-click; the side simply stays whole.
            if editor.contour_edit_split(segment, point).is_ok() {
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Paints the base ring, the working copy and the corner handles over the viewport image.
    ///
    /// A chain draft is left to the path gizmo; this one paints only a ring.
    pub(crate) fn paint(&self, ui: &Ui, rect: egui::Rect, camera: Camera, project: &Project, edit: &ContourEdit) {
        let Some(placement) = project.world_placement(edit.target().id()) else {
            return;
        };
        let (Some(base), Some(contour)) = (edit.base().as_ring(), edit.draft().as_ring()) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        if edit.is_dirty() {
            paint_ring(&painter, camera, rect, placement, base, egui::Stroke::new(1.0, BASE_COLOR));
        }
        paint_ring(&painter, camera, rect, placement, contour, egui::Stroke::new(2.0, DRAFT_COLOR));
        for entry in contour.entries() {
            let Ok(start) = entry.segment().start() else {
                continue;
            };
            let world = placement.transform_point(local_world(start));
            let Some(position) = screen_point(camera, rect, world) else {
                continue;
            };
            let selected = self.drag.is_some_and(|drag| drag.segment == entry.id());
            let hovered = self.hover_corner == Some(entry.id());
            let radius = if selected || hovered { CORNER_RADIUS + 1.5 } else { CORNER_RADIUS };
            let color = if selected { ACTIVE_COLOR } else { CORNER_COLOR };
            painter.circle_filled(position, radius, color);
            painter.circle_stroke(position, radius, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)));
        }
    }
}

/// Paints one ring as a closed screen-space polyline.
fn paint_ring(
    painter: &egui::Painter,
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    contour: &Contour,
    stroke: egui::Stroke,
) {
    let Ok(points) = contour.sampled_points(RING_TOLERANCE_MICROMETRES) else {
        return;
    };
    let mut screen = Vec::with_capacity(points.len());
    for point in points {
        let Some(position) = screen_point(camera, rect, placement.transform_point(local_world(point))) else {
            continue;
        };
        screen.push(position);
    }
    if screen.len() >= 3 {
        painter.add(egui::Shape::closed_line(screen, stroke));
    }
}

/// The segment whose start corner is under the pointer, if any.
fn corner_at(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    contour: &Contour,
    pointer: Pos2,
) -> Option<ContourSegmentId> {
    let mut best: Option<(f32, ContourSegmentId)> = None;
    for entry in contour.entries() {
        let Ok(start) = entry.segment().start() else {
            continue;
        };
        let Some(position) = screen_point(camera, rect, placement.transform_point(local_world(start))) else {
            continue;
        };
        let distance = position.distance(pointer);
        if distance <= CORNER_HIT_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, entry.id()));
        }
    }
    best.map(|(_, segment)| segment)
}

/// The segment whose projected curve is closest to the pointer, if within the hit radius.
fn segment_at(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    contour: &Contour,
    pointer: Pos2,
) -> Option<ContourSegmentId> {
    let mut best: Option<(f32, ContourSegmentId)> = None;
    for entry in contour.entries() {
        let points = segment_screen(camera, rect, placement, entry.segment());
        for pair in points.windows(2) {
            let Some(distance) = distance_to_segment(pointer, pair[0], pair[1]) else {
                continue;
            };
            if distance <= SEGMENT_HIT_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, entry.id()));
            }
        }
    }
    best.map(|(_, segment)| segment)
}

/// A segment sampled into screen points; a line needs two, an arc enough to look round.
fn segment_screen(camera: Camera, rect: egui::Rect, placement: Placement, segment: ContourSegment) -> Vec<Pos2> {
    let steps = match segment {
        ContourSegment::Line { .. } => 1,
        ContourSegment::Arc { sweep, .. } => {
            let degrees = (sweep.millidegrees().unsigned_abs() as f64) / 1_000.0;
            ((degrees / 10.0).ceil() as usize).clamp(8, MAX_ARC_STEPS)
        }
    };
    let Ok(geometry) = segment.to_geom() else {
        return Vec::new();
    };
    let mut points = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let fraction = step as f64 / steps as f64;
        let Some(local) = geometry.point_at(fraction) else {
            continue;
        };
        let Some(position) = screen_point(camera, rect, placement.transform_point(DVec3::new(local.x, local.y, 0.0)))
        else {
            continue;
        };
        points.push(position);
    }
    points
}

/// The world point under a viewport position on the node's local XY plane, as local millimetres.
fn local_point(pointer: Pos2, rect: egui::Rect, camera: Camera, placement: Placement) -> Option<DVec2> {
    let ray = ray_to(pointer, rect, camera);
    let origin = placement.transform_point(DVec3::ZERO);
    let normal = placement.transform_vector(DVec3::Z);
    let hit = ray_plane(ray, origin, normal)?;
    let local = placement.rotation().inverse() * (hit - origin);
    let point = DVec2::new(local.x, local.y);
    point.is_finite().then_some(point)
}

/// Keeps an arc corner on its circle, or returns a line corner unchanged.
///
/// A corner joins two segments; when either side is an arc the corner is constrained by that arc's
/// circle, so the pointer point is projected onto it before the edit sees it.
fn project_arc_corner(contour: &Contour, segment: ContourSegmentId, point: DVec2) -> Option<DVec2> {
    let index = contour.index_of(segment)?;
    let count = contour.entries().len();
    let current = contour.segment(segment)?;
    let previous = contour.entries().get((index + count - 1) % count)?.segment();
    for candidate in [current, previous] {
        if let ContourSegment::Arc { center, radius, .. } = candidate {
            let center = DVec2::new(center[0].millimetres_f64(), center[1].millimetres_f64());
            let offset = point - center;
            if offset.length() <= f64::EPSILON {
                return None;
            }
            return Some(center + offset.normalize() * radius.millimetres_f64());
        }
    }
    Some(point)
}

/// The point on `segment` where a split is allowed, or `None` next to a corner.
///
/// The point is the pointer projected onto the segment's own curve; the margin keeps a double-click
/// near a corner from minting a segment a few micrometres long.
fn split_point(segment: ContourSegment, local: DVec2) -> Option<[Length; 2]> {
    let point = match segment {
        ContourSegment::Line { start, end } => {
            let start = DVec2::new(start[0].millimetres_f64(), start[1].millimetres_f64());
            let end = DVec2::new(end[0].millimetres_f64(), end[1].millimetres_f64());
            let direction = end - start;
            let length_squared = direction.length_squared();
            if length_squared <= f64::EPSILON {
                return None;
            }
            let along = (local - start).dot(direction) / length_squared;
            if along <= SPLIT_MARGIN || along >= 1.0 - SPLIT_MARGIN {
                return None;
            }
            start + direction * along
        }
        ContourSegment::Arc { center, radius, start_angle, sweep } => {
            let center = DVec2::new(center[0].millimetres_f64(), center[1].millimetres_f64());
            let offset = local - center;
            if offset.length() <= f64::EPSILON {
                return None;
            }
            let point = center + offset.normalize() * radius.millimetres_f64();
            let angle = (point - center).y.atan2((point - center).x);
            let start = start_angle.radians_f64();
            let sweep = sweep.radians_f64();
            let mut travel = angle - start;
            let turn = std::f64::consts::TAU;
            travel = if sweep >= 0.0 { travel.rem_euclid(turn) } else { -((-travel).rem_euclid(turn)) };
            let margin = sweep.abs() * SPLIT_MARGIN;
            let outside = if sweep >= 0.0 {
                travel <= margin || travel >= sweep - margin
            } else {
                travel >= -margin || travel <= sweep + margin
            };
            if outside {
                return None;
            }
            point
        }
    };
    Some([length_from_millimetres(point.x)?, length_from_millimetres(point.y)?])
}

/// A contour point in the node's local frame.
fn local_world(point: [Length; 2]) -> DVec3 {
    DVec3::new(point[0].millimetres_f64(), point[1].millimetres_f64(), 0.0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::DVec2;
    use furnicraft_model::ContourSegment;
    use furnicraft_units::Degrees;

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    fn rectangle() -> Contour {
        Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(100), millimetres(0)],
            [millimetres(100), millimetres(60)],
            [millimetres(0), millimetres(60)],
        ])
        .expect("rectangle")
    }

    #[test]
    fn a_line_split_projects_the_pointer_and_refuses_the_corners() {
        let segment = rectangle().id_at(0).and_then(|id| rectangle().segment(id)).expect("bottom edge");
        let point = split_point(segment, DVec2::new(40.0, 3.0)).expect("inside the edge");
        assert_eq!(point, [millimetres(40), millimetres(0)], "the point sits on the line");

        assert!(split_point(segment, DVec2::new(0.5, 0.0)).is_none(), "too close to the start corner");
        assert!(split_point(segment, DVec2::new(99.5, 0.0)).is_none(), "too close to the end corner");
        assert!(split_point(segment, DVec2::new(140.0, 0.0)).is_none(), "past the end corner");
    }

    #[test]
    fn an_arc_corner_is_projected_onto_its_circle() {
        let arc = ContourSegment::arc(
            [millimetres(100), millimetres(20)],
            millimetres(20),
            Degrees::from_millidegrees(-90_000),
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let contour = Contour::new(vec![
            ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]).expect("line"),
            arc,
            ContourSegment::line([millimetres(120), millimetres(20)], [millimetres(120), millimetres(60)])
                .expect("line"),
            ContourSegment::line([millimetres(120), millimetres(60)], [millimetres(0), millimetres(60)]).expect("line"),
            ContourSegment::line([millimetres(0), millimetres(60)], [millimetres(0), millimetres(0)]).expect("line"),
        ])
        .expect("ring");
        let id = contour.entries().iter().find(|entry| entry.segment().is_arc()).map(|entry| entry.id()).expect("arc");
        let projected = project_arc_corner(&contour, id, DVec2::new(125.0, 35.0)).expect("projection");
        let center = DVec2::new(100.0, 20.0);
        assert!(((projected - center).length() - 20.0).abs() < 1.0e-9, "the corner stays on the circle");
        assert!(projected.x > 100.0 && projected.y > 20.0, "the projection follows the pointer's quadrant");
    }

    #[test]
    fn an_arc_split_needs_a_point_inside_the_sweep() {
        let arc = ContourSegment::arc(
            [millimetres(100), millimetres(20)],
            millimetres(20),
            Degrees::from_millidegrees(-90_000),
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let inside = split_point(arc, DVec2::new(115.0, 5.0)).expect("inside the sweep");
        let point = DVec2::new(inside[0].millimetres_f64(), inside[1].millimetres_f64());
        assert!(((point - DVec2::new(100.0, 20.0)).length() - 20.0).abs() < 1.0e-3, "on the circle");
        assert!(split_point(arc, DVec2::new(120.0, 25.0)).is_none(), "too close to the end corner");
        assert!(split_point(arc, DVec2::new(100.0, 20.0)).is_none(), "the centre has no direction");
    }

    #[test]
    fn length_rounding_refuses_values_that_cannot_be_stored() {
        assert_eq!(length_from_millimetres(1.2345), Some(Length::from_micrometres(1235)));
        assert!(length_from_millimetres(f64::NAN).is_none());
        assert!(length_from_millimetres(f64::INFINITY).is_none());
    }
}
