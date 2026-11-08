//! The viewport gizmo for contour edit mode.
//!
//! The gizmo edits the editor's working copy, never the document: dragging a corner rewrites the
//! draft ring or chain, and double-clicking a side splits it there. Painting shows the draft over
//! the solid, with the original shape faintly behind it when the draft differs, so the user sees
//! the change before the single command that applies it.
//!
//! The pointer is mapped onto the node's local XY plane through its world placement. An arc corner
//! is projected onto its own circle first, because an edit may slide an arc corner along the arc but
//! never change its centre or radius.
//!
//! A ring names every corner by the segment that starts on it; an open chain has a free end as well,
//! so a handle is addressed with [`PathCorner`] and the last segment's end is reachable.

use eframe::egui::{self, Pos2, Response, Ui};
use furnicraft_geom::{DVec2, DVec3};
use furnicraft_model::{
    Contour, ContourSegment, ContourSegmentEntry, ContourSegmentId, CornerMove, PathCorner, Placement, Project,
};
use furnicraft_scene::Camera;
use furnicraft_ui::{ContourDraft, ContourEdit, ContourTarget, Editor};
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
    corner: PathCorner,
}

/// Pointer state of the contour gizmo; the working copy itself belongs to the editor.
#[derive(Debug, Default)]
pub(crate) struct ContourGizmo {
    drag: Option<CornerDrag>,
    hover_corner: Option<PathCorner>,
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
        let draft = edit.draft().clone();
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let corner = pointer.and_then(|pointer| corner_at(camera, rect, placement, &draft, pointer));
        let segment = pointer.and_then(|pointer| {
            if corner.is_some() { None } else { segment_at(camera, rect, placement, &draft, pointer) }
        });
        self.hover_corner = corner;
        self.hover_segment = segment;

        let mut changed = false;
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.drag = corner.map(|corner| CornerDrag { target, corner });
        }
        if let Some(drag) = self.drag {
            if drag.target != target {
                self.drag = None;
            } else if response.dragged_by(egui::PointerButton::Primary)
                && let Some(local) =
                    response.interact_pointer_pos().and_then(|pointer| local_point(pointer, rect, camera, placement))
                && let Some(local) = project_arc_corner(&draft, drag.corner, local)
                && let (Some(x), Some(y)) = (length_from_millimetres(local.x), length_from_millimetres(local.y))
                && editor.contour_edit_move_corner_at(drag.corner, [x, y], CornerMove::Free).is_ok()
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

    /// Paints the base shape, the working copy and the corner handles over the viewport image.
    pub(crate) fn paint(&self, ui: &Ui, rect: egui::Rect, camera: Camera, project: &Project, edit: &ContourEdit) {
        let Some(placement) = project.world_placement(edit.target().id()) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        if edit.is_dirty() {
            paint_draft(&painter, camera, rect, placement, edit.base(), egui::Stroke::new(1.0, BASE_COLOR));
        }
        paint_draft(&painter, camera, rect, placement, edit.draft(), egui::Stroke::new(2.0, DRAFT_COLOR));
        for corner in corner_handles(edit.draft()) {
            let Some(start) = corner_position(edit.draft(), corner) else {
                continue;
            };
            let world = placement.transform_point(local_world(start));
            let Some(position) = screen_point(camera, rect, world) else {
                continue;
            };
            let selected = self.drag.is_some_and(|drag| drag.corner == corner);
            let hovered = self.hover_corner == Some(corner);
            let radius = if selected || hovered { CORNER_RADIUS + 1.5 } else { CORNER_RADIUS };
            let color = if selected { ACTIVE_COLOR } else { CORNER_COLOR };
            painter.circle_filled(position, radius, color);
            painter.circle_stroke(position, radius, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)));
        }
    }
}

/// Paints one draft as a screen-space polyline, closed only when it is a ring.
fn paint_draft(
    painter: &egui::Painter,
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    stroke: egui::Stroke,
) {
    if let Some(shape) = draft_shape(camera, rect, placement, draft, stroke) {
        painter.add(shape);
    }
}

/// The screen-space polyline a draft paints, closed only when the draft is a ring.
///
/// The closed flag is what separates the two cases: a ring and a closed chain join their last
/// point back to the first, an open chain must not, or its free end would be drawn as a side.
fn draft_shape(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    stroke: egui::Stroke,
) -> Option<egui::Shape> {
    match draft {
        ContourDraft::Ring(contour) => ring_shape(camera, rect, placement, contour, stroke),
        ContourDraft::Chain(path) => chain_shape(camera, rect, placement, path.entries(), path.is_closed(), stroke),
    }
}

/// The screen-space polyline of one open or closed chain.
fn chain_shape(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    entries: &[ContourSegmentEntry],
    closed: bool,
    stroke: egui::Stroke,
) -> Option<egui::Shape> {
    let mut screen = Vec::new();
    for entry in entries {
        for position in segment_screen(camera, rect, placement, entry.segment()) {
            if screen.last() != Some(&position) {
                screen.push(position);
            }
        }
    }
    (screen.len() >= 2)
        .then(|| if closed { egui::Shape::closed_line(screen, stroke) } else { egui::Shape::line(screen, stroke) })
}

/// The screen-space polyline of one ring, always closed.
fn ring_shape(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    contour: &Contour,
    stroke: egui::Stroke,
) -> Option<egui::Shape> {
    let points = contour.sampled_points(RING_TOLERANCE_MICROMETRES).ok()?;
    let mut screen = Vec::with_capacity(points.len());
    for point in points {
        let Some(position) = screen_point(camera, rect, placement.transform_point(local_world(point))) else {
            continue;
        };
        screen.push(position);
    }
    (screen.len() >= 3).then(|| egui::Shape::closed_line(screen, stroke))
}

/// The corners a draft draws a handle on.
///
/// A ring has one handle per segment start. A chain has the same starts plus the free end when it
/// is open; a closed chain's last end is the first start and would put two handles on one point.
fn corner_handles(draft: &ContourDraft) -> Vec<PathCorner> {
    let mut corners: Vec<PathCorner> = draft.entries().iter().map(|entry| PathCorner::Start(entry.id())).collect();
    let open = draft.as_chain().is_some_and(|path| !path.is_closed());
    if open && let Some(last) = draft.entries().last() {
        corners.push(PathCorner::End(last.id()));
    }
    corners
}

/// The point a corner handle sits on, in the node's local plane.
fn corner_position(draft: &ContourDraft, corner: PathCorner) -> Option<[Length; 2]> {
    let segment = draft.segment(corner.segment())?;
    if corner.is_end() { segment.end().ok() } else { segment.start().ok() }
}

/// The corner handle under the pointer, if any.
fn corner_at(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    pointer: Pos2,
) -> Option<PathCorner> {
    let mut best: Option<(f32, PathCorner)> = None;
    for corner in corner_handles(draft) {
        let Some(start) = corner_position(draft, corner) else {
            continue;
        };
        let Some(position) = screen_point(camera, rect, placement.transform_point(local_world(start))) else {
            continue;
        };
        let distance = position.distance(pointer);
        if distance <= CORNER_HIT_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, corner));
        }
    }
    best.map(|(_, corner)| corner)
}

/// The segment whose projected curve is closest to the pointer, if within the hit radius.
fn segment_at(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    pointer: Pos2,
) -> Option<ContourSegmentId> {
    let mut best: Option<(f32, ContourSegmentId)> = None;
    for entry in draft.entries() {
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
fn project_arc_corner(draft: &ContourDraft, corner: PathCorner, point: DVec2) -> Option<DVec2> {
    for candidate in adjacent_segments(draft, corner).into_iter().flatten() {
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

/// The segments that share `corner`, in draft travel order.
///
/// A ring resolves an end to the next segment's start, so both slots exist. An open chain leaves
/// the slot across its free start or free end empty; a closed chain wraps to the other end.
fn adjacent_segments(draft: &ContourDraft, corner: PathCorner) -> [Option<ContourSegment>; 2] {
    let entries = draft.entries();
    let Some(index) = draft.index_of(corner.segment()) else {
        return [None, None];
    };
    let count = entries.len();
    if count == 0 {
        return [None, None];
    }
    match draft {
        ContourDraft::Ring(_) => {
            let start = if corner.is_end() { (index + 1) % count } else { index };
            let previous = (start + count - 1) % count;
            [entries.get(start).map(|entry| entry.segment()), entries.get(previous).map(|entry| entry.segment())]
        }
        ContourDraft::Chain(path) => {
            let Some(current) = entries.get(index).map(|entry| entry.segment()) else {
                return [None, None];
            };
            let neighbour = if corner.is_end() {
                if index + 1 < count {
                    Some(index + 1)
                } else if path.is_closed() {
                    Some(0)
                } else {
                    None
                }
            } else if index > 0 {
                Some(index - 1)
            } else if path.is_closed() {
                Some(count - 1)
            } else {
                None
            };
            [Some(current), neighbour.and_then(|neighbour| entries.get(neighbour)).map(|entry| entry.segment())]
        }
    }
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
    use furnicraft_model::{ContourSegment, Path};
    use furnicraft_units::Degrees;

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    /// The viewport the gizmo projects into.
    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
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

    fn open_path() -> Path {
        Path::polyline(
            vec![
                [millimetres(0), millimetres(0)],
                [millimetres(100), millimetres(0)],
                [millimetres(100), millimetres(60)],
            ],
            false,
        )
        .expect("open path")
    }

    /// A line joined to a quarter arc of radius 40, open at both ends.
    fn line_and_arc() -> Path {
        Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::arc(
                    [millimetres(100), millimetres(40)],
                    millimetres(40),
                    Degrees::from_millidegrees(-90_000),
                    Degrees::from_millidegrees(90_000),
                )
                .expect("arc"),
            ],
            false,
        )
        .expect("line and arc")
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
    fn an_open_chain_puts_a_handle_on_its_free_end() {
        let path = open_path();
        let draft = ContourDraft::Chain(path.clone());
        let handles = corner_handles(&draft);
        let first = path.id_at(0).expect("first segment");
        let last = path.id_at(1).expect("last segment");
        assert_eq!(handles, vec![PathCorner::Start(first), PathCorner::Start(last), PathCorner::End(last)]);
        assert_eq!(corner_position(&draft, PathCorner::Start(first)), Some([millimetres(0), millimetres(0)]));
        assert_eq!(corner_position(&draft, PathCorner::Start(last)), Some([millimetres(100), millimetres(0)]));
        assert_eq!(
            corner_position(&draft, PathCorner::End(last)),
            Some([millimetres(100), millimetres(60)]),
            "the last point is the last segment's end"
        );
    }

    #[test]
    fn a_closed_chain_has_one_handle_per_corner() {
        let path =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(60)]).expect("rectangle");
        let draft = ContourDraft::Chain(path);
        let handles = corner_handles(&draft);
        assert_eq!(handles.len(), 4, "the last end is the first start and draws no second handle");
        assert!(handles.iter().all(|corner| !corner.is_end()));
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
        let draft = ContourDraft::Ring(contour);
        let id = draft.entries().iter().find(|entry| entry.segment().is_arc()).map(|entry| entry.id()).expect("arc");
        let projected = project_arc_corner(&draft, PathCorner::Start(id), DVec2::new(125.0, 35.0)).expect("projection");
        let center = DVec2::new(100.0, 20.0);
        assert!(((projected - center).length() - 20.0).abs() < 1.0e-9, "the corner stays on the circle");
        assert!(projected.x > 100.0 && projected.y > 20.0, "the projection follows the pointer's quadrant");
    }

    #[test]
    fn an_open_chain_paints_as_an_open_polyline_ending_on_its_free_end() {
        let camera = Camera::default();
        let path = open_path();
        let draft = ContourDraft::Chain(path.clone());
        let shape = draft_shape(camera, viewport(), Placement::IDENTITY, &draft, egui::Stroke::new(2.0, DRAFT_COLOR))
            .expect("three points make a polyline");
        let egui::Shape::Path(painted) = shape else {
            panic!("a chain paints as a path, got {shape:?}");
        };
        assert!(!painted.closed, "an open chain must not join its last point back to the first");
        assert_eq!(painted.points.len(), 3, "one screen point per corner");

        let last = path.id_at(1).expect("last segment");
        let free_end = corner_position(&draft, PathCorner::End(last)).expect("the free end");
        let expected = screen_point(camera, viewport(), Placement::IDENTITY.transform_point(local_world(free_end)))
            .expect("the free end is in front");
        assert_eq!(painted.points.last().copied(), Some(expected), "the polyline ends on the last point");
    }

    #[test]
    fn a_closed_chain_and_a_ring_paint_closed() {
        let camera = Camera::default();
        let rectangle =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(60)]).expect("rectangle");
        let chain = draft_shape(
            camera,
            viewport(),
            Placement::IDENTITY,
            &ContourDraft::Chain(rectangle.clone()),
            egui::Stroke::new(2.0, DRAFT_COLOR),
        )
        .expect("a closed chain paints");
        assert!(matches!(chain, egui::Shape::Path(ref painted) if painted.closed), "a closed chain closes");

        let ring = draft_shape(
            camera,
            viewport(),
            Placement::IDENTITY,
            &ContourDraft::Ring(rectangle.to_contour().expect("ring")),
            egui::Stroke::new(2.0, DRAFT_COLOR),
        )
        .expect("a ring paints");
        assert!(matches!(ring, egui::Shape::Path(ref painted) if painted.closed), "a ring always closes");
    }

    #[test]
    fn a_draft_without_enough_screen_points_paints_nothing() {
        let camera = Camera::default();
        let single = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
            ],
            false,
        )
        .expect("one line");
        assert!(
            draft_shape(
                camera,
                viewport(),
                Placement::IDENTITY,
                &ContourDraft::Chain(single),
                egui::Stroke::new(2.0, DRAFT_COLOR)
            )
            .is_some(),
            "one line still has two screen points and paints"
        );
        assert!(
            draft_shape(
                camera,
                egui::Rect::from_min_size(Pos2::ZERO, egui::Vec2::ZERO),
                Placement::IDENTITY,
                &ContourDraft::Chain(open_path()),
                egui::Stroke::new(2.0, DRAFT_COLOR)
            )
            .is_none(),
            "a zero-size viewport collapses every corner onto one point, so there is no line to draw"
        );
    }

    /// A project whose root holds one path object.
    fn path_project(path: Path) -> (Project, furnicraft_model::Id) {
        let mut project = furnicraft_testkit::sample_project();
        let root = project.root().id();
        let id = project
            .add_object(root, "Sketch", Placement::IDENTITY, furnicraft_model::Geometry::Path(path))
            .expect("path object");
        (project, id)
    }

    #[test]
    fn painting_an_open_chain_adds_one_open_polyline_and_a_handle_on_every_corner() {
        let path = open_path();
        let (project, object) = path_project(path.clone());
        let edit = ContourEdit::start(&project, ContourTarget::Path(object)).expect("a path is editable");
        let camera = Camera::default();
        let rect = viewport();

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ContourGizmo::default().paint(ui, rect, camera, &project, &edit);
        });

        let mut paths = Vec::new();
        let mut filled = 0;
        let mut outlined = 0;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::Path(painted) => paths.push(painted.clone()),
                egui::Shape::Circle(circle) if circle.fill.a() > 0 => filled += 1,
                egui::Shape::Circle(_) => outlined += 1,
                _ => {}
            }
        }
        assert_eq!(paths.len(), 1, "an untouched draft paints one polyline, not a base and a copy");
        assert!(!paths[0].closed, "the painted chain is open");
        assert_eq!(paths[0].points.len(), 3);
        assert_eq!(filled, 3, "two segment starts and the free end, and nothing else");
        assert_eq!(outlined, 3, "every handle keeps its dark outline");
        output.drop_without_applying_deltas();

        // A dirty draft paints the stored shape faintly behind the working copy.
        let mut edit = edit;
        let last = edit.draft().id_at(1).expect("last segment");
        edit.move_corner_at(PathCorner::End(last), [millimetres(140), millimetres(60)], CornerMove::Free)
            .expect("move the free end");
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ContourGizmo::default().paint(ui, rect, camera, &project, &edit);
        });
        let paths: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Path(painted) => Some(painted.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2, "the base chain and the working copy");
        assert!(paths.iter().all(|painted| !painted.closed), "both paint open");
        output.drop_without_applying_deltas();
    }

    #[test]
    fn a_chain_corner_next_to_an_arc_is_projected_onto_the_arc() {
        let path = line_and_arc();
        let draft = ContourDraft::Chain(path.clone());
        let line = path.id_at(0).expect("line");
        let arc = path.id_at(1).expect("arc");
        let center = DVec2::new(100.0, 40.0);

        let shared = project_arc_corner(&draft, PathCorner::End(line), DVec2::new(125.0, 35.0)).expect("projection");
        assert!(((shared - center).length() - 40.0).abs() < 1.0e-9, "the shared corner stays on the arc");
        let free_end = project_arc_corner(&draft, PathCorner::End(arc), DVec2::new(125.0, 35.0)).expect("projection");
        assert!(((free_end - center).length() - 40.0).abs() < 1.0e-9, "the free arc end stays on the circle");

        let line_start = PathCorner::Start(line);
        assert_eq!(
            project_arc_corner(&draft, line_start, DVec2::new(-10.0, 5.0)),
            Some(DVec2::new(-10.0, 5.0)),
            "a corner with no arc neighbour takes the pointer point"
        );
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
