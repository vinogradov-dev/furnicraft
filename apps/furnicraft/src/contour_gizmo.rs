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
use furnicraft_ui::{ContourDraft, ContourEdit, ContourGrip, ContourTarget, Editor, GripNumber, GripValue};
use furnicraft_units::Length;

use crate::gizmo_math::{ACTIVE_COLOR, distance_to_segment, length_from_millimetres, ray_plane, ray_to, screen_point};

/// Radius of an unselected grip handle, in points.
const GRIP_RADIUS: f32 = 4.5;
/// Pointer distance that grabs a grip, in points.
const GRIP_HIT_RADIUS: f32 = 9.0;
/// Pointer distance that picks a side for a split, in points.
const SEGMENT_HIT_RADIUS: f32 = 7.0;
/// Sampling tolerance for the painted ring, in micrometres.
const RING_TOLERANCE_MICROMETRES: f64 = 500.0;
/// Fraction of a side that stays unsplittable next to each corner.
const SPLIT_MARGIN: f64 = 0.02;
/// Maximum number of screen segments used to draw one arc.
const MAX_ARC_STEPS: usize = 64;
/// How far from the pointer the numeric field sits, in points.
const NUMBER_OFFSET: egui::Vec2 = egui::vec2(18.0, 12.0);

/// Colour of the ring the edit started from.
const BASE_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(150, 150, 150, 160);
/// Colour of the working copy.
const DRAFT_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 200, 255);
/// Colour of an untouched grip handle.
const GRIP_COLOR: egui::Color32 = egui::Color32::from_rgb(240, 240, 240);

/// One grip drag that is in progress.
#[derive(Clone, Copy, Debug)]
struct GripDrag {
    target: ContourTarget,
    grip: ContourGrip,
    /// The motion resolved once at the drag start: free, or along one named segment.
    motion: CornerMove,
}

/// The numeric field that follows the cursor while a grip is dragged.
#[derive(Clone, Debug)]
struct NumericEntry {
    grip: ContourGrip,
    kind: GripNumber,
    label: &'static str,
    text: String,
    /// The value the drag started from, which Escape restores.
    start: Option<GripValue>,
    /// The motion segment for an along-contour drag, used to move the corner by a typed length.
    motion: Option<CornerMove>,
    pointer: Pos2,
    /// Whether the field still has to ask for keyboard focus.
    focus: bool,
    /// Whether the user typed into the field, so a drag must not overwrite the text.
    edited: bool,
    error: Option<String>,
}

/// What the numeric field asked the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericAction {
    /// Apply the parsed number to the grip.
    Apply(ContourGrip, GripValue),
    /// Put the value the drag started from back.
    Restore(ContourGrip, GripValue),
    /// Apply a typed length to an along-contour corner, moving it along the segment.
    ApplyAlong(ContourGrip, GripValue, CornerMove),
}

/// Pointer state of the contour gizmo; the working copy itself belongs to the editor.
#[derive(Debug, Default)]
pub(crate) struct ContourGizmo {
    drag: Option<GripDrag>,
    hover: Option<ContourGrip>,
    hover_segment: Option<ContourSegmentId>,
    numeric: Option<NumericEntry>,
    /// Whether a corner drag runs along the contour instead of freely.
    along: bool,
}

impl ContourGizmo {
    /// Drops every drag, hover and numeric state, for example when the mode ends.
    pub(crate) fn reset(&mut self) {
        self.drag = None;
        self.hover = None;
        self.hover_segment = None;
        self.numeric = None;
    }

    /// Whether a grip drag or its numeric field owns the keyboard right now.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some() || self.numeric.is_some()
    }

    /// Whether a corner drag follows the contour instead of moving freely.
    pub(crate) fn along(&self) -> bool {
        self.along
    }

    /// Switches between a free corner drag and one along the contour.
    pub(crate) fn toggle_along(&mut self) {
        self.along = !self.along;
    }

    /// The cursor the viewport should show while the mode is active.
    ///
    /// Always the default arrow: the enlarged hovered grip and the active drag state carry the
    /// feedback, and a hand or grab icon would fight the selection cursor and add noise.
    pub(crate) fn cursor(&self) -> egui::CursorIcon {
        egui::CursorIcon::Default
    }

    /// Applies the pointer to the working copy; returns whether the draft changed.
    ///
    /// The gizmo owns the left button while the mode is live: a drag moves the grabbed grip and a
    /// double-click splits the side under the pointer. A rejected edit leaves the draft at its last
    /// valid shape, so a grip simply stops when the ring would become invalid.
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
        let segment = pointer.and_then(|pointer| segment_at(camera, rect, placement, &draft, pointer));
        let grip = pointer.and_then(|pointer| grip_at(camera, rect, placement, &draft, pointer, segment));
        self.hover = grip;
        self.hover_segment = segment;

        let mut changed = false;
        if response.drag_started_by(egui::PointerButton::Primary) {
            // Resolve the grabbed grip from the press origin, not the current pointer.
            // The pointer may have moved a few points before egui fires the drag threshold,
            // and the user clearly intended to grab whatever was under the initial press.
            let press_origin = response.ctx.input(|i| i.pointer.press_origin());
            let press_grip = press_origin.and_then(|origin| {
                let seg = segment_at(camera, rect, placement, &draft, origin);
                grip_at(camera, rect, placement, &draft, origin, seg)
            });
            self.drag = press_grip.map(|grip| GripDrag {
                target,
                grip,
                motion: self.motion_for(camera, rect, placement, &draft, grip, press_origin),
            });
            if let Some(drag) = self.drag
                && let Some(origin) = press_origin
            {
                self.numeric = numeric_entry(&draft, drag.grip, origin, Some(drag.motion));
            }
        }
        if let Some(drag) = self.drag {
            if drag.target != target {
                self.drag = None;
                self.numeric = None;
            } else {
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                    && let Some(local) = local_point(pointer, rect, camera, placement)
                    && let Some(local) = self.dragged_point(&draft, drag, local)
                    && let (Some(x), Some(y)) = (length_from_millimetres(local.x), length_from_millimetres(local.y))
                    && editor.contour_edit_drag_grip(drag.grip, [x, y], drag.motion).is_ok()
                {
                    changed = true;
                    // An along-contour drag that reaches the far end records a collapse, which
                    // Apply turns into a segment removal. Dragging back clears it.
                    if let (ContourGrip::Corner(corner), CornerMove::Along(segment)) = (drag.grip, drag.motion) {
                        let collapsed = editor
                            .contour_edit()
                            .and_then(|edit| {
                                let start = edit.base().grip_position(ContourGrip::Corner(corner))?;
                                edit.draft().along_contour_length(segment, corner, start)
                            })
                            .is_some_and(|length| length.micrometres() <= 2);
                        if collapsed {
                            editor.contour_edit_record_collapse(segment);
                        } else {
                            editor.contour_edit_clear_collapse();
                        }
                    }
                }
                if let Some(entry) = &mut self.numeric
                    && let Some(pointer) = pointer
                {
                    entry.pointer = pointer;
                    if !entry.edited {
                        let value = if let Some(CornerMove::Along(segment)) = entry.motion
                            && let ContourGrip::Corner(corner) = entry.grip
                        {
                            editor.contour_edit().and_then(|edit| {
                                let start = edit.base().grip_position(ContourGrip::Corner(corner))?;
                                edit.draft().along_contour_length(segment, corner, start).map(GripValue::Length)
                            })
                        } else {
                            editor.contour_edit().and_then(|edit| edit.grip_value(entry.grip))
                        };
                        if let Some(value) = value {
                            entry.text = value.text();
                        }
                    }
                }
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    self.drag = None;
                }
            }
        }

        // An arc curve under the pointer also offers a radius grip, but a double-click there still
        // splits the arc: only a real handle blocks the split.
        let synthetic = matches!(grip, Some(ContourGrip::ArcRadius(..)));
        if self.drag.is_none()
            && self.numeric.is_none()
            && (grip.is_none() || synthetic)
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

    /// The point a grip really receives, after the arc projection an along-contour corner needs.
    ///
    /// A free corner takes the pointer point directly and the arc endpoint rebuilds the arc through
    /// the other end and its current midpoint. An along-contour corner projects onto the arc's
    /// circle so the centre and radius stay fixed.
    fn dragged_point(&self, draft: &ContourDraft, drag: GripDrag, local: DVec2) -> Option<DVec2> {
        let ContourGrip::Corner(corner) = drag.grip else {
            return Some(local);
        };
        if matches!(drag.motion, CornerMove::Along(_)) { project_arc_corner(draft, corner, local) } else { Some(local) }
    }

    /// The motion a drag takes, resolved once from the toggle and the junction under the pointer.
    fn motion_for(
        &self,
        camera: Camera,
        rect: egui::Rect,
        placement: Placement,
        draft: &ContourDraft,
        grip: ContourGrip,
        pointer: Option<Pos2>,
    ) -> CornerMove {
        if !self.along {
            return CornerMove::Free;
        }
        let (ContourGrip::Corner(corner), Some(pointer)) = (grip, pointer) else {
            return CornerMove::Free;
        };
        along_segment(camera, rect, placement, draft, corner, pointer)
            .map(CornerMove::Along)
            .unwrap_or(CornerMove::Free)
    }

    /// Paints the base shape, the working copy, the grips and the numeric field.
    ///
    /// The numeric field is an editable widget, so the frame returns the action Enter or Escape
    /// asked for and the shell applies it after the paint borrow ends.
    pub(crate) fn paint(
        &mut self,
        ui: &Ui,
        rect: egui::Rect,
        camera: Camera,
        project: &Project,
        edit: &ContourEdit,
    ) -> Option<NumericAction> {
        let placement = project.world_placement(edit.target().id())?;
        let painter = ui.painter().with_clip_rect(rect);
        if edit.is_dirty() {
            paint_draft(&painter, camera, rect, placement, edit.base(), egui::Stroke::new(1.0, BASE_COLOR));
        }
        paint_draft(&painter, camera, rect, placement, edit.draft(), egui::Stroke::new(2.0, DRAFT_COLOR));
        let grips = edit.draft().grips();
        for grip in &grips {
            let Some(local) = edit.draft().grip_position(*grip) else {
                continue;
            };
            let world = placement.transform_point(local_world(local));
            let Some(position) = screen_point(camera, rect, world) else {
                continue;
            };
            let selected = self.drag.is_some_and(|drag| drag.grip == *grip);
            let hovered = self.hover == Some(*grip);
            paint_grip(&painter, position, *grip, selected, hovered);
        }
        // The radius grip on a hovered arc curve is not part of the stable list, so paint it here.
        // The radius grip on a hovered arc curve or a circle rim is not part of the stable list, so paint it here.
        if let Some(grip @ (ContourGrip::ArcRadius(..) | ContourGrip::CircleRim(..))) = self.hover
            && !grips.contains(&grip)
            && let Some(local) = edit.draft().grip_position(grip)
        {
            let world = placement.transform_point(local_world(local));
            if let Some(position) = screen_point(camera, rect, world) {
                paint_grip(&painter, position, grip, false, true);
            }
        }
        self.paint_numeric(ui, rect)
    }

    /// Draws the editable number beside the cursor and reads Enter and Escape.
    fn paint_numeric(&mut self, ui: &Ui, rect: egui::Rect) -> Option<NumericAction> {
        let entry = self.numeric.as_mut()?;
        let dragging = self.drag.is_some();
        let position = Pos2::new(
            (entry.pointer.x + NUMBER_OFFSET.x).min(rect.max.x - 190.0),
            (entry.pointer.y + NUMBER_OFFSET.y).min(rect.max.y - 48.0),
        );
        let mut action = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("contour-grip-number"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(entry.label);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut entry.text)
                                .desired_width(84.0)
                                .hint_text(if matches!(entry.kind, GripNumber::Angle) { "°" } else { "mm" }),
                        );
                        if entry.focus {
                            response.request_focus();
                            entry.focus = false;
                        }
                        if response.changed() {
                            entry.edited = true;
                        }
                        if let Some(error) = &entry.error {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if escape {
                            if let Some(start) = entry.start {
                                action = Some(NumericAction::Restore(entry.grip, start));
                            }
                        } else if enter {
                            match entry.kind.parse(&entry.text) {
                                Ok(value) => {
                                    action = if let Some(motion) = entry.motion {
                                        Some(NumericAction::ApplyAlong(entry.grip, value, motion))
                                    } else {
                                        Some(NumericAction::Apply(entry.grip, value))
                                    };
                                }
                                Err(error) => entry.error = Some(error),
                            }
                        } else if response.lost_focus() && !dragging {
                            // A click outside the field drops it without touching the draft.
                            close = true;
                        }
                    });
                });
            });
        if action.is_some() || close {
            self.drag = None;
            self.numeric = None;
        }
        action
    }
}

/// The editable numeric field a grip opens, when it takes a number at all.
fn numeric_entry(
    draft: &ContourDraft,
    grip: ContourGrip,
    pointer: Pos2,
    motion: Option<CornerMove>,
) -> Option<NumericEntry> {
    let (kind, value, label, motion) = if let Some(motion @ CornerMove::Along(segment)) = motion
        && let ContourGrip::Corner(corner) = grip
    {
        // Along-contour mode: show the length from the far end of the motion segment. At the
        // moment the drag starts the corner is still at its base position.
        let start = draft.grip_position(grip)?;
        let length = draft.along_contour_length(segment, corner, start)?;
        (GripNumber::Length, GripValue::Length(length), "L", Some(motion))
    } else {
        let kind = draft.grip_number(grip)?;
        let value = draft.grip_value(grip)?;
        let label = draft.grip_label(grip)?;
        (kind, value, label, None)
    };
    Some(NumericEntry {
        grip,
        kind,
        label,
        text: value.text(),
        start: Some(value),
        motion,
        pointer,
        focus: true,
        edited: false,
        error: None,
    })
}

/// Paints one grip: a square for a centre, a circle for a point on the figure.
fn paint_grip(painter: &egui::Painter, position: Pos2, grip: ContourGrip, selected: bool, hovered: bool) {
    let radius = if selected || hovered { GRIP_RADIUS + 1.5 } else { GRIP_RADIUS };
    let color = if selected { ACTIVE_COLOR } else { GRIP_COLOR };
    let outline = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160));
    if is_center(grip) {
        let box_rect = egui::Rect::from_center_size(position, egui::vec2(radius, radius) * 2.0);
        painter.rect_filled(box_rect, 1.0, color);
        painter.rect_stroke(box_rect, 1.0, outline, egui::StrokeKind::Inside);
    } else {
        painter.circle_filled(position, radius, color);
        painter.circle_stroke(position, radius, outline);
    }
}

/// Whether a grip is a figure or arc centre, which paints as a square.
fn is_center(grip: ContourGrip) -> bool {
    matches!(
        grip,
        ContourGrip::CircleCenter
            | ContourGrip::ArcCenter(_)
            | ContourGrip::RectangleCenter
            | ContourGrip::PolygonCenter
    )
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

/// The corners a draft draws a handle on, for the tests that check the corner list.
///
/// A ring has one handle per segment start. A chain has the same starts plus the free end when it
/// is open; a closed chain's last end is the first start and would put two handles on one point.
#[cfg(test)]
fn corner_handles(draft: &ContourDraft) -> Vec<PathCorner> {
    let mut corners: Vec<PathCorner> = draft.entries().iter().map(|entry| PathCorner::Start(entry.id())).collect();
    let open = draft.as_chain().is_some_and(|path| !path.is_closed());
    if open && let Some(last) = draft.entries().last() {
        corners.push(PathCorner::End(last.id()));
    }
    corners
}

/// The point a corner sits on, in the node's local plane.
fn corner_position(draft: &ContourDraft, corner: PathCorner) -> Option<[Length; 2]> {
    let segment = draft.segment(corner.segment())?;
    if corner.is_end() { segment.end().ok() } else { segment.start().ok() }
}

/// The grip under the pointer, if any.
///
/// A real handle wins over the radius grip a hovered arc curve offers; the synthetic grip carries
/// the point on the curve so a drag that starts there measures the arc's own radius.
fn grip_at(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    pointer: Pos2,
    hover_segment: Option<ContourSegmentId>,
) -> Option<ContourGrip> {
    let mut best: Option<(f32, ContourGrip)> = None;
    for grip in draft.grips() {
        let Some(local) = draft.grip_position(grip) else {
            continue;
        };
        let Some(position) = screen_point(camera, rect, placement.transform_point(local_world(local))) else {
            continue;
        };
        let distance = position.distance(pointer);
        if distance <= GRIP_HIT_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, grip));
        }
    }
    if let Some((_, grip)) = best {
        return Some(grip);
    }
    // A ring has no arc grips yet, so it must not offer a radius handle it cannot drag.
    draft.as_chain()?;
    let segment = hover_segment?;
    let curve = draft.segment(segment)?;
    if !curve.is_arc() {
        return None;
    }
    // A circle-tagged path exposes a rim grip that scales the whole circle.
    if draft.as_chain().is_some_and(|path| path.shape() == Some(furnicraft_model::PathShape::Circle)) {
        let local = local_point(pointer, rect, camera, placement)?;
        let point = arc_nearest(curve, local)?;
        return Some(ContourGrip::CircleRim(point));
    }
    let local = local_point(pointer, rect, camera, placement)?;
    let point = arc_nearest(curve, local)?;
    Some(ContourGrip::ArcRadius(segment, point))
}

/// The point of an arc that the pointer is closest to, when it lies inside the sweep.
fn arc_nearest(segment: ContourSegment, local: DVec2) -> Option<[Length; 2]> {
    let ContourSegment::Arc { center, radius, start_angle, sweep } = segment else {
        return None;
    };
    let center = DVec2::new(center[0].millimetres_f64(), center[1].millimetres_f64());
    let offset = local - center;
    if offset.length() <= f64::EPSILON {
        return None;
    }
    let point = center + offset.normalize() * radius.millimetres_f64();
    let angle = (point - center).y.atan2((point - center).x);
    let start = start_angle.radians_f64();
    let sweep = sweep.radians_f64();
    let turn = std::f64::consts::TAU;
    let mut travel = angle - start;
    travel = if sweep >= 0.0 { travel.rem_euclid(turn) } else { -((-travel).rem_euclid(turn)) };
    let inside = if sweep >= 0.0 { travel <= sweep } else { travel >= sweep };
    inside.then_some([length_from_millimetres(point.x)?, length_from_millimetres(point.y)?])
}

/// Whether two document points name the same joint, within the one-micrometre joint tolerance.
fn near(first: [Length; 2], second: [Length; 2]) -> bool {
    let dx = i128::from(first[0].micrometres()) - i128::from(second[0].micrometres());
    let dy = i128::from(first[1].micrometres()) - i128::from(second[1].micrometres());
    dx.abs() <= 1 && dy.abs() <= 1
}

/// The segment a corner should follow when the drag is constrained to the contour.
///
/// The corner joins up to two segments; the one whose projected curve lies nearest the pointer is
/// the one the user grabbed. The choice is made once, when the drag starts, so a drag that moves
/// past the other segment does not switch half way.
fn along_segment(
    camera: Camera,
    rect: egui::Rect,
    placement: Placement,
    draft: &ContourDraft,
    corner: PathCorner,
    pointer: Pos2,
) -> Option<ContourSegmentId> {
    let point = corner_position(draft, corner)?;
    let mut best: Option<(f32, ContourSegmentId)> = None;
    for entry in draft.entries() {
        let segment = entry.segment();
        let (Ok(start), Ok(end)) = (segment.start(), segment.end()) else {
            continue;
        };
        if !near(start, point) && !near(end, point) {
            continue;
        }
        let points = segment_screen(camera, rect, placement, segment);
        let mut distance = f32::INFINITY;
        for pair in points.windows(2) {
            if let Some(found) = distance_to_segment(pointer, pair[0], pair[1]) {
                distance = distance.min(found);
            }
        }
        if distance.is_finite() && best.is_none_or(|(closest, _)| distance < closest) {
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
    use furnicraft_testkit::{millimetres, open_path};
    use furnicraft_units::Degrees;

    use super::*;

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

    /// A 100 × 60 mm three-point chain, open at both ends.
    fn l_path() -> Path {
        open_path(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(100), millimetres(0)],
            [millimetres(100), millimetres(60)],
        ])
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
        let path = l_path();
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
        let path = l_path();
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
                &ContourDraft::Chain(l_path()),
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
        let path = l_path();
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

    fn screen_of(camera: Camera, point: [Length; 2]) -> Pos2 {
        screen_point(camera, viewport(), Placement::IDENTITY.transform_point(local_world(point))).expect("in front")
    }

    #[test]
    fn an_along_contour_drag_picks_the_segment_nearest_the_pointer() {
        let path =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(60)]).expect("rectangle");
        let draft = ContourDraft::Chain(path.clone());
        let bottom = path.id_at(0).expect("bottom side");
        let left = path.id_at(3).expect("left side");
        let corner = PathCorner::Start(bottom);
        let camera = Camera::default();

        let near_bottom = screen_of(camera, [millimetres(50), millimetres(0)]);
        assert_eq!(along_segment(camera, viewport(), Placement::IDENTITY, &draft, corner, near_bottom), Some(bottom));
        let near_left = screen_of(camera, [millimetres(0), millimetres(30)]);
        assert_eq!(along_segment(camera, viewport(), Placement::IDENTITY, &draft, corner, near_left), Some(left));
    }

    #[test]
    fn the_along_toggle_resolves_a_corner_motion_and_defaults_to_free() {
        let mut gizmo = ContourGizmo::default();
        assert!(!gizmo.along(), "free movement is the default");
        let path =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(60)]).expect("rectangle");
        let draft = ContourDraft::Chain(path.clone());
        let corner = PathCorner::Start(path.id_at(0).expect("bottom"));
        let camera = Camera::default();
        let pointer = Some(screen_of(camera, [millimetres(50), millimetres(0)]));
        assert_eq!(
            gizmo.motion_for(camera, viewport(), Placement::IDENTITY, &draft, ContourGrip::Corner(corner), pointer),
            CornerMove::Free
        );
        gizmo.toggle_along();
        assert_eq!(
            gizmo.motion_for(camera, viewport(), Placement::IDENTITY, &draft, ContourGrip::Corner(corner), pointer),
            CornerMove::Along(path.id_at(0).expect("bottom"))
        );
    }

    #[test]
    fn a_circle_paints_a_square_centre_and_four_round_quadrants() {
        let path = Path::circle([millimetres(10), millimetres(20)], millimetres(40)).expect("circle");
        let (project, object) = path_project(path);
        let edit = ContourEdit::start(&project, ContourTarget::Path(object)).expect("editable");
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ContourGizmo::default().paint(ui, viewport(), Camera::default(), &project, &edit);
        });
        let mut filled_circles = 0;
        let mut filled_rects = 0;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::Circle(circle) if circle.fill.a() > 0 => filled_circles += 1,
                egui::Shape::Rect(rect) if rect.fill.a() > 0 => filled_rects += 1,
                _ => {}
            }
        }
        assert_eq!(filled_circles, 4, "one round handle per quadrant");
        assert_eq!(filled_rects, 1, "the centre is the only square handle");
        output.drop_without_applying_deltas();
    }

    #[test]
    fn an_arc_curve_offers_a_radius_grip_away_from_its_midpoint() {
        let path = Path::new(
            vec![
                ContourSegment::arc(
                    [millimetres(0), millimetres(0)],
                    millimetres(200),
                    Degrees::ZERO,
                    Degrees::from_millidegrees(180_000),
                )
                .expect("a half circle"),
            ],
            false,
        )
        .expect("an arc chain");
        let draft = ContourDraft::Chain(path.clone());
        let arc = path.id_at(0).expect("arc");
        let camera = Camera::default();
        // A quarter of the way along the sweep, away from the persistent midpoint handle.
        let angle = 45.0_f64.to_radians();
        let point =
            [millimetres((200.0 * angle.cos()).round() as i64), millimetres((200.0 * angle.sin()).round() as i64)];
        let pointer = screen_of(camera, point);
        let grip = grip_at(camera, viewport(), Placement::IDENTITY, &draft, pointer, Some(arc));
        assert!(
            matches!(grip, Some(ContourGrip::ArcRadius(segment, _)) if segment == arc),
            "the hovered curve offers its radius: {grip:?}"
        );

        // The persistent midpoint handle wins when the pointer is right on it.
        let midpoint = draft.grip_position(ContourGrip::ArcPoint(arc)).expect("an arc midpoint");
        let pointer = screen_of(camera, midpoint);
        assert_eq!(
            grip_at(camera, viewport(), Placement::IDENTITY, &draft, pointer, Some(arc)),
            Some(ContourGrip::ArcPoint(arc))
        );
    }

    #[test]
    fn a_numeric_entry_starts_from_the_grip_value_and_its_kind() {
        let circle =
            ContourDraft::Chain(Path::circle([millimetres(0), millimetres(0)], millimetres(40)).expect("circle"));
        let entry = numeric_entry(&circle, ContourGrip::CircleQuadrant(0), Pos2::ZERO, None).expect("a radius field");
        assert_eq!(entry.kind, GripNumber::Length);
        assert_eq!(entry.text, "40 mm");
        assert_eq!(entry.start, Some(GripValue::Length(millimetres(40))));

        let arc = ContourDraft::Chain(line_and_arc());
        let id = arc.entries().last().expect("arc").id();
        let entry =
            numeric_entry(&arc, ContourGrip::Corner(PathCorner::End(id)), Pos2::ZERO, None).expect("an angle field");
        assert_eq!(entry.kind, GripNumber::Angle);
        assert_eq!(entry.text, "0°");
        assert!(entry.label.contains('∠') || entry.label.contains('R'), "the field is labelled: {entry:?}");

        assert!(
            numeric_entry(&circle, ContourGrip::CircleCenter, Pos2::ZERO, None).is_none(),
            "a centre has no number"
        );
    }

    #[test]
    fn an_along_contour_numeric_entry_shows_the_length_from_the_far_end() {
        let draft = ContourDraft::Chain(l_path());
        let first = draft.id_at(0).expect("first segment");
        let second = draft.id_at(1).expect("second segment");
        let corner = PathCorner::Start(second);
        let motion = Some(CornerMove::Along(first));
        let entry = numeric_entry(&draft, ContourGrip::Corner(corner), Pos2::ZERO, motion).expect("a length field");
        assert_eq!(entry.kind, GripNumber::Length);
        assert_eq!(entry.label, "L", "the along-contour field is labelled L");
        assert_eq!(entry.text, "100 mm", "the corner sits at the far end of a 100 mm segment");
        assert_eq!(entry.motion, Some(CornerMove::Along(first)));

        // An arc's value is its arc length, not its chord.
        let arc_draft = ContourDraft::Chain(line_and_arc());
        let arc = arc_draft.id_at(1).expect("arc segment");
        let arc_corner = PathCorner::End(arc);
        let entry =
            numeric_entry(&arc_draft, ContourGrip::Corner(arc_corner), Pos2::ZERO, Some(CornerMove::Along(arc)))
                .expect("an arc length field");
        // A quarter arc of radius 40 is 40·π/2 ≈ 62.83 mm.
        let expected = (40.0 * std::f64::consts::FRAC_PI_2 * 1_000.0).round() as i64;
        assert_eq!(entry.text, Length::from_micrometres(expected).to_string(), "the arc length is 62.83 mm");
    }

    #[test]
    fn the_cursor_is_always_the_default_arrow() {
        let mut gizmo = ContourGizmo::default();
        assert_eq!(gizmo.cursor(), egui::CursorIcon::Default, "fresh gizmo shows the default arrow");
        gizmo.hover = Some(ContourGrip::CircleCenter);
        assert_eq!(gizmo.cursor(), egui::CursorIcon::Default, "hover over a grip is still the default arrow");
        gizmo.drag = Some(GripDrag {
            target: ContourTarget::Path(furnicraft_model::Id::new(1).expect("id")),
            grip: ContourGrip::RectangleCenter,
            motion: CornerMove::Free,
        });
        assert_eq!(gizmo.cursor(), egui::CursorIcon::Default, "a running drag is still the default arrow");
    }

    #[test]
    fn a_grip_is_found_within_the_hit_radius_and_refused_outside_it() {
        let path = Path::circle([millimetres(0), millimetres(0)], millimetres(40)).expect("circle");
        let draft = ContourDraft::Chain(path.clone());
        let camera = Camera::default();
        let circle_centre_in_screen = screen_of(camera, [millimetres(0), millimetres(0)]);

        let right_on = grip_at(camera, viewport(), Placement::IDENTITY, &draft, circle_centre_in_screen, None);
        assert!(matches!(right_on, Some(ContourGrip::CircleCenter)), "exactly on the centre grabs it");

        let far_away = circle_centre_in_screen + egui::vec2(100.0, 0.0);
        let outside = grip_at(camera, viewport(), Placement::IDENTITY, &draft, far_away, None);
        assert!(outside.is_none(), "100 pt from the centre is well beyond the 9 pt hit radius");
    }

    #[test]
    fn a_grip_within_9_pt_is_still_grabbed_after_a_small_move() {
        let path = Path::circle([millimetres(0), millimetres(0)], millimetres(40)).expect("circle");
        let draft = ContourDraft::Chain(path.clone());
        let camera = Camera::default();
        let centre_screen = screen_of(camera, [millimetres(0), millimetres(0)]);

        // The press origin is right on the centre, so the grip resolves even if the drag
        // fires at a position 5 pt away (below the 9 pt hit radius from the centre).
        let at_origin = grip_at(camera, viewport(), Placement::IDENTITY, &draft, centre_screen, None);
        assert!(
            matches!(at_origin, Some(ContourGrip::CircleCenter)),
            "the grip resolves from the press origin, not the moved pointer"
        );

        // A pointer 5 pt from the centre is still within the 9 pt hit radius, so a grip that
        // started at the centre would still be held.
        let moved = centre_screen + egui::vec2(5.0, 0.0);
        let near_origin = grip_at(camera, viewport(), Placement::IDENTITY, &draft, moved, None);
        assert!(near_origin.is_some(), "5 pt from the centre is inside the hit radius");
    }

    #[test]
    fn length_rounding_refuses_values_that_cannot_be_stored() {
        assert_eq!(length_from_millimetres(1.2345), Some(Length::from_micrometres(1235)));
        assert!(length_from_millimetres(f64::NAN).is_none());
        assert!(length_from_millimetres(f64::INFINITY).is_none());
    }
}
