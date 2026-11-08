//! The pull arrow a fresh extrusion shows.
//!
//! The ribbon creates a body at the height its field names; this gizmo then lets the user adjust
//! that height in place. Nothing is written while the pointer moves: the drag and the typed field
//! only change the height this file remembers, and one `object.extrude.height.set` runs when a drag
//! is released or the pull is finished, so a whole adjustment is one undo entry.
//!
//! The arrow sits on the top face of the body, at the contour's bounds centre, and points along the
//! body's local `+Z`. A drag is read as a position on that axis rather than as a screen distance, so
//! the same gesture near the camera and far from it produces the same number of millimetres.

use eframe::egui::{self, Pos2, Response, Stroke, Ui, Vec2};
use furnicraft_geom::{DVec2, DVec3};
use furnicraft_model::{Id, Placement, Project};
use furnicraft_scene::Camera;
use furnicraft_units::Length;

use crate::gizmo_math::{ACTIVE_COLOR, HIT_RADIUS, distance_to_segment, ray_to, screen_direction, screen_point};

/// Length of the pull arrow, in points.
const ARROW_LENGTH: f32 = 96.0;
/// Half-width of the arrow head, in points.
const ARROW_HEAD: f32 = 5.0;
/// How far the pointer must move before a press becomes a drag, in points.
const CLICK_SLOP: f32 = 3.0;
/// The smallest height a pull may leave, in millimetres.
const MIN_HEIGHT_MM: f64 = 1.0;
/// How far from the arrow tip the numeric field sits, in points.
const NUMBER_OFFSET: Vec2 = Vec2::new(16.0, 12.0);

/// Colour of the arrow when the pointer is elsewhere.
const ARROW_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 200, 255);

/// The body a pull is adjusting, with the height the document holds and the previewed one.
#[derive(Clone, Copy, Debug)]
struct Pull {
    /// The constructive body.
    object: Id,
    /// The height the last command wrote.
    committed: Length,
    /// The height the arrow currently shows.
    height: Length,
}

/// One continuous drag on the arrow.
#[derive(Clone, Copy, Debug)]
struct Drag {
    /// Where the press landed, to tell a click from a drag.
    start_pointer: Pos2,
    /// The height at the press.
    start_height: Length,
    /// The axis position under the pointer at the press, in millimetres.
    start_parameter: f64,
    /// Whether the pointer moved far enough to change the height.
    moved: bool,
}

/// What the pull asks the shell to dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtrudeEvent {
    /// Write this height to the body and keep pulling.
    Set {
        /// The body being pulled.
        object: Id,
        /// The new height.
        height: Length,
    },
    /// Finish the pull; the height is written only when the body does not already hold it.
    Finish {
        /// The body being pulled.
        object: Id,
        /// The height the pull shows.
        height: Length,
    },
}

/// What the numeric field asked the shell to do after the paint borrow ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtrudeAction {
    /// Write the typed height and close the pull.
    Finish(Length),
    /// Drop the pull, keeping whatever height the document already holds.
    Cancel,
}

/// Where the body sits: its frame, the contour centre and the axis the arrow follows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Body {
    /// The node's world placement.
    placement: Placement,
    /// The contour's bounds centre in the node's local XY plane, in millimetres.
    centre: DVec2,
    /// The point on the contour plane that the axis runs through, in world millimetres.
    axis_origin: DVec3,
    /// The world direction of the body's local `+Z`.
    axis: DVec3,
}

impl Body {
    /// Reads the frame and contour centre of an extruded body.
    fn read(project: &Project, object: Id) -> Option<Self> {
        let node = project.object(object)?;
        let body = node.geometry().as_constructive()?;
        body.height()?;
        let contour = body.contour().ok()?;
        let ([min_x, min_y], [max_x, max_y]) = contour.bounds_micrometres();
        let centre = DVec2::new((min_x as f64 + max_x as f64) / 2_000.0, (min_y as f64 + max_y as f64) / 2_000.0);
        if !centre.is_finite() {
            return None;
        }
        let placement = project.world_placement(object)?;
        let axis_origin = placement.transform_point(DVec3::new(centre.x, centre.y, 0.0));
        let axis = placement.transform_vector(DVec3::Z).try_normalize()?;
        Some(Self { placement, centre, axis_origin, axis })
    }

    /// The centre of the top face at `height`.
    fn top(self, height: Length) -> DVec3 {
        self.placement.transform_point(DVec3::new(self.centre.x, self.centre.y, height.millimetres_f64()))
    }
}

/// Pointer and keyboard state of the pull.
#[derive(Debug, Default)]
pub(crate) struct ExtrudeGizmo {
    pull: Option<Pull>,
    drag: Option<Drag>,
    hover: bool,
    /// The height typed into the field, prefilled with the current preview.
    text: String,
    /// Whether the field still has to ask for keyboard focus.
    focus: bool,
    /// Whether the user typed into the field, so a drag must not overwrite the text.
    edited: bool,
    error: Option<String>,
    /// The field's last painted rectangle, so a click meant for it does not finish the pull.
    number_rect: Option<egui::Rect>,
}

impl ExtrudeGizmo {
    /// Starts a pull on a body that was just extruded at `height`.
    pub(crate) fn start(&mut self, object: Id, height: Length) {
        self.pull = Some(Pull { object, committed: height, height });
        self.drag = None;
        self.hover = false;
        self.text = height.to_string();
        self.focus = true;
        self.edited = false;
        self.error = None;
        self.number_rect = None;
    }

    /// Drops the pull, keeping whatever height the document already holds.
    pub(crate) fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Whether a body is waiting for its height.
    pub(crate) fn is_active(&self) -> bool {
        self.pull.is_some()
    }

    /// Whether the arrow is being dragged.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The body being pulled.
    pub(crate) fn object(&self) -> Option<Id> {
        self.pull.as_ref().map(|pull| pull.object)
    }

    /// The height the document holds for the body.
    pub(crate) fn committed(&self) -> Option<Length> {
        self.pull.as_ref().map(|pull| pull.committed)
    }

    /// The height the arrow currently shows.
    pub(crate) fn height(&self) -> Option<Length> {
        self.pull.as_ref().map(|pull| pull.height)
    }

    /// Records the height a finished drag wrote into the document.
    ///
    /// A later finish then knows the body already holds that value and does not dispatch a
    /// duplicate command.
    pub(crate) fn committed_height(&mut self, height: Length) {
        if let Some(pull) = self.pull.as_mut() {
            pull.committed = height;
        }
    }

    /// The cursor the viewport should show while a pull is open.
    pub(crate) fn cursor(&self) -> egui::CursorIcon {
        if self.drag.is_some() {
            egui::CursorIcon::Grabbing
        } else if self.hover {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        }
    }

    /// Reads the pointer: a press on the arrow starts a drag, a release commits it, a click anywhere
    /// else finishes the pull.
    pub(crate) fn update(
        &mut self,
        ui: &Ui,
        response: &Response,
        viewport: egui::Rect,
        camera: Camera,
        project: &Project,
    ) -> Result<Option<ExtrudeEvent>, String> {
        let Some(pull) = self.pull.as_ref().copied() else {
            return Ok(None);
        };
        let Some(body) = Body::read(project, pull.object) else {
            // The body was deleted, undone or collapsed under the pull: there is nothing left to
            // adjust, so the arrow goes away instead of showing a stale number.
            self.cancel();
            return Ok(None);
        };
        let pointer = ui.input(|input| input.pointer.latest_pos());

        if let Some(mut drag) = self.drag.take() {
            if let Some(pointer) = pointer {
                if pointer.distance(drag.start_pointer) > CLICK_SLOP {
                    drag.moved = true;
                }
                if drag.moved
                    && let Some(parameter) =
                        ray_to(pointer, viewport, camera).closest_parameter_on_line(body.axis_origin, body.axis)
                    && let Some(height) = dragged_height(drag.start_height, drag.start_parameter, parameter)
                    && let Some(pull) = self.pull.as_mut()
                    && height != pull.height
                {
                    pull.height = height;
                    if !self.edited {
                        self.text = height.to_string();
                    }
                }
            }
            if ui.input(|input| input.pointer.primary_down()) {
                self.drag = Some(drag);
                return Ok(None);
            }
            let Some(pull) = self.pull.as_ref().copied() else {
                return Ok(None);
            };
            self.focus = true;
            self.edited = false;
            self.text = pull.height.to_string();
            if drag.moved {
                let event = (pull.height != pull.committed)
                    .then_some(ExtrudeEvent::Set { object: pull.object, height: pull.height });
                return Ok(event);
            }
            return Ok(Some(ExtrudeEvent::Finish { object: pull.object, height: pull.height }));
        }

        self.hover = pointer.is_some_and(|pointer| {
            !self.number_rect.is_some_and(|rect| rect.contains(pointer))
                && arrow_hit(camera, viewport, &body, pull.height, pointer)
        });
        let pressed = ui.input(|input| input.pointer.primary_pressed());
        if self.hover
            && pressed
            && let Some(pointer) = pointer
            && let Some(parameter) =
                ray_to(pointer, viewport, camera).closest_parameter_on_line(body.axis_origin, body.axis)
        {
            self.drag = Some(Drag {
                start_pointer: pointer,
                start_height: pull.height,
                start_parameter: parameter,
                moved: false,
            });
            return Ok(None);
        }

        let over_field = pointer.is_some_and(|pointer| self.number_rect.is_some_and(|rect| rect.contains(pointer)));
        if response.clicked_by(egui::PointerButton::Primary) && !over_field {
            return Ok(Some(ExtrudeEvent::Finish { object: pull.object, height: pull.height }));
        }
        Ok(None)
    }

    /// Paints the arrow and the height field; returns what Enter or Escape asked for.
    pub(crate) fn paint(
        &mut self,
        ui: &Ui,
        viewport: egui::Rect,
        camera: Camera,
        project: &Project,
    ) -> Option<ExtrudeAction> {
        let pull = self.pull.as_ref().copied()?;
        let body = Body::read(project, pull.object)?;
        let Some((base, tip)) = arrow_screen(camera, viewport, &body, pull.height) else {
            self.number_rect = None;
            return None;
        };
        let color = if self.hover || self.drag.is_some() { ACTIVE_COLOR } else { ARROW_COLOR };
        let painter = ui.painter().with_clip_rect(viewport);
        paint_arrow(&painter, base, tip, color);
        let position = Pos2::new(
            (tip.x + NUMBER_OFFSET.x).min(viewport.max.x - 200.0),
            (tip.y + NUMBER_OFFSET.y).min(viewport.max.y - 56.0),
        );
        self.paint_number(ui, position)
    }

    /// Draws the editable height beside the arrow and reads Enter and Escape.
    fn paint_number(&mut self, ui: &Ui, position: Pos2) -> Option<ExtrudeAction> {
        let mut text = std::mem::take(&mut self.text);
        let mut focus = self.focus;
        let mut edited = self.edited;
        let mut error = self.error.take();
        let mut action = None;
        let area = egui::Area::new(egui::Id::unique("extrude-height"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("H");
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(84.0).hint_text("mm"));
                        if focus {
                            response.request_focus();
                            focus = false;
                        }
                        if response.changed() {
                            edited = true;
                        }
                        if let Some(error) = &error {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if escape {
                            action = Some(ExtrudeAction::Cancel);
                        } else if enter {
                            match Length::parse_millimetres(text.trim()) {
                                Ok(height) if height.is_positive() => {
                                    action = Some(ExtrudeAction::Finish(height));
                                }
                                Ok(_) => error = Some("The height must be positive.".to_owned()),
                                Err(reason) => error = Some(reason.to_string()),
                            }
                        }
                    });
                });
            });
        self.number_rect = Some(area.response.rect);
        self.text = text;
        self.focus = focus;
        self.edited = edited;
        self.error = error;
        action
    }
}

/// The screen ends of the pull arrow on the top face at `height`.
fn arrow_screen(camera: Camera, viewport: egui::Rect, body: &Body, height: Length) -> Option<(Pos2, Pos2)> {
    let base_world = body.top(height);
    let base = screen_point(camera, viewport, base_world)?;
    let step = (camera.distance() * 0.01).max(1.0);
    let ahead = screen_point(camera, viewport, base_world + body.axis * step)?;
    let direction = screen_direction(ahead - base)?;
    Some((base, base + direction * ARROW_LENGTH))
}

/// Whether the pointer grabs the arrow.
fn arrow_hit(camera: Camera, viewport: egui::Rect, body: &Body, height: Length, pointer: Pos2) -> bool {
    arrow_screen(camera, viewport, body, height)
        .and_then(|(base, tip)| distance_to_segment(pointer, base, tip))
        .is_some_and(|distance| distance <= HIT_RADIUS)
}

/// The height a drag reaches, clamped to a positive document length and rounded to whole
/// millimetres.
fn dragged_height(start: Length, start_parameter: f64, parameter: f64) -> Option<Length> {
    let millimetres = start.millimetres_f64() + (parameter - start_parameter);
    if !millimetres.is_finite() {
        return None;
    }
    let maximum = (furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000) as f64;
    let whole = millimetres.round().clamp(MIN_HEIGHT_MM, maximum);
    Length::from_millimetres(whole as i64)
}

/// Paints one arrow with a solid head.
fn paint_arrow(painter: &egui::Painter, base: Pos2, tip: Pos2, color: egui::Color32) {
    let direction = (tip - base).normalized();
    let perpendicular = Vec2::new(-direction.y, direction.x);
    painter.line_segment([base, tip], Stroke::new(2.5, color));
    painter.circle_filled(base, 2.5, color);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip - direction * 12.0 + perpendicular * ARROW_HEAD,
            tip - direction * 12.0 - perpendicular * ARROW_HEAD,
        ],
        color,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Constructive, Extrude, Feature, Geometry, Path};

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    /// A rectangle 100 × 50 mm extruded 100 mm at the scene root.
    fn body_project() -> (Project, Id) {
        let mut project = furnicraft_testkit::sample_project();
        let root = project.root().id();
        let path = Path::rectangle([millimetres(20), millimetres(10)], [millimetres(120), millimetres(60)])
            .expect("a rectangle");
        let body = Constructive::new(path, vec![Feature::Extrude(Extrude::new(millimetres(100)).expect("a height"))])
            .expect("a constructive body");
        let id = project.add_object(root, "Body", Placement::IDENTITY, Geometry::Constructive(body)).expect("body");
        (project, id)
    }

    #[test]
    fn a_body_reads_its_contour_centre_and_its_own_axis() {
        let (project, id) = body_project();
        let body = Body::read(&project, id).expect("a body");
        assert_eq!(body.centre, DVec2::new(70.0, 35.0), "the centre of the bounds");
        assert!((body.axis - DVec3::Z).length() < 1.0e-9);
        let top = body.top(millimetres(100));
        assert!((top - DVec3::new(70.0, 35.0, 100.0)).length() < 1.0e-9, "the arrow starts on the top face");
    }

    #[test]
    fn a_drawn_path_has_no_pull() {
        let mut project = furnicraft_testkit::sample_project();
        let root = project.root().id();
        let path = Path::rectangle([millimetres(0), millimetres(0)], [millimetres(50), millimetres(50)]).expect("path");
        let id = project.add_object(root, "Drawing", Placement::IDENTITY, Geometry::Path(path)).expect("path");
        assert!(Body::read(&project, id).is_none(), "only an extrusion has a height to pull");
    }

    #[test]
    fn a_drag_reads_millimetres_along_the_axis_not_screen_points() {
        // The parameter is a position on the body's own axis, so one parameter unit is one
        // millimetre whether the view is close or far.
        assert_eq!(dragged_height(millimetres(100), 100.0, 250.0), Some(millimetres(250)));
        assert_eq!(dragged_height(millimetres(100), 100.0, 40.0), Some(millimetres(40)));
        assert_eq!(dragged_height(millimetres(100), 100.0, -500.0), Some(millimetres(1)), "never zero or negative");
        assert_eq!(dragged_height(millimetres(100), 100.0, f64::NAN), None);
        assert_eq!(dragged_height(millimetres(100), 100.0, f64::INFINITY), None);
    }

    #[test]
    fn a_pull_starts_at_the_created_height_and_can_be_dropped() {
        let mut gizmo = ExtrudeGizmo::default();
        assert!(!gizmo.is_active());
        let object = Id::new(7).expect("id");
        gizmo.start(object, millimetres(100));
        assert!(gizmo.is_active());
        assert_eq!(gizmo.object(), Some(object));
        assert_eq!(gizmo.height(), Some(millimetres(100)));
        assert_eq!(gizmo.committed(), Some(millimetres(100)));
        gizmo.cancel();
        assert!(!gizmo.is_active());
        assert_eq!(gizmo.height(), None);
    }

    #[test]
    fn the_arrow_projects_above_the_top_face_and_is_grabbable() {
        let (project, id) = body_project();
        let body = Body::read(&project, id).expect("a body");
        let camera = Camera::default();
        let (base, tip) = arrow_screen(camera, viewport(), &body, millimetres(100)).expect("an arrow");
        assert!(viewport().contains(base));
        assert!(viewport().contains(tip));
        assert!((tip - base).length() > 60.0, "the arrow is a readable length");
        let mid = Pos2::new((base.x + tip.x) * 0.5, (base.y + tip.y) * 0.5);
        assert!(arrow_hit(camera, viewport(), &body, millimetres(100), mid));
        assert!(!arrow_hit(camera, viewport(), &body, millimetres(100), mid + Vec2::new(60.0, 0.0)));
    }

    /// Runs the gizmo over three frames — hover, press, release — and returns the last event.
    fn click_frames(project: &Project, object: Id, pointer: Pos2) -> Option<ExtrudeEvent> {
        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(object, millimetres(100));
        let ctx = egui::Context::default();
        let mut event = None;
        for (frame, pressed) in [(0, None), (1, Some(true)), (2, Some(false))] {
            let mut events = vec![egui::Event::PointerMoved(pointer)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(viewport()),
                time: Some(f64::from(frame) * 0.1),
                predicted_dt: 0.1,
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let (_, response) = ui.allocate_exact_size(viewport().size(), egui::Sense::click_and_drag());
                event = gizmo.update(ui, &response, viewport(), Camera::default(), project).expect("update");
            });
            output.drop_without_applying_deltas();
        }
        event
    }

    /// Runs the gizmo over four frames: hover, press, move and release.
    fn drag_frames(project: &Project, object: Id, start: Pos2, end: Pos2) -> Option<ExtrudeEvent> {
        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(object, millimetres(100));
        let ctx = egui::Context::default();
        let mut event = None;
        let steps = [(start, None), (start, Some((start, true))), (end, None), (end, Some((end, false)))];
        for (frame, (pointer, button)) in steps.into_iter().enumerate() {
            let mut events = vec![egui::Event::PointerMoved(pointer)];
            if let Some((pos, pressed)) = button {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(viewport()),
                time: Some(f64::from(frame as u32) * 0.1),
                predicted_dt: 0.1,
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let (_, response) = ui.allocate_exact_size(viewport().size(), egui::Sense::click_and_drag());
                event = gizmo.update(ui, &response, viewport(), Camera::default(), project).expect("update");
            });
            output.drop_without_applying_deltas();
        }
        event
    }

    #[test]
    fn a_drag_on_the_arrow_writes_the_height_it_reaches() {
        let (project, id) = body_project();
        let body = Body::read(&project, id).expect("a body");
        let (base, tip) = arrow_screen(Camera::default(), viewport(), &body, millimetres(100)).expect("an arrow");
        let direction = (tip - base).normalized();
        let event = drag_frames(&project, id, base + direction * 40.0, base + direction * 120.0);
        match event {
            Some(ExtrudeEvent::Set { object, height }) => {
                assert_eq!(object, id);
                assert!(
                    height.micrometres() > millimetres(100).micrometres(),
                    "pulling along the axis raises the height, got {height}"
                );
            }
            other => panic!("a released drag writes the height, got {other:?}"),
        }
    }

    #[test]
    fn a_click_outside_the_arrow_finishes_the_pull_at_its_height() {
        let (project, id) = body_project();
        let event = click_frames(&project, id, Pos2::new(700.0, 500.0));
        assert_eq!(event, Some(ExtrudeEvent::Finish { object: id, height: millimetres(100) }));
        assert!(Body::read(&project, id).is_some(), "finishing reads the body, it does not touch it");
    }
}
