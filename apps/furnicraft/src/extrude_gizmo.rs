//! The pull arrow a fresh extrusion shows.
//!
//! The ribbon creates a body at the height its field names; this gizmo then lets the user adjust
//! that height in place. Nothing is written while the pointer moves: the drag changes the height
//! this file remembers, the shell shows that height as a session preview of the body, and the
//! command bar's ✓ runs one `object.extrude.height.set`, so a whole adjustment is one undo entry
//! (M71).
//!
//! The preview is the body itself, evaluated at the height the arrow shows, so the user sees the
//! solid they are about to accept rather than a stand-in for it. The arrow sits on that top face,
//! at the contour's bounds centre, and points along the body's local `+Z`. A drag is read as a
//! position on that axis rather than as a screen distance, so the same gesture near the camera and
//! far from it produces the same number of millimetres.

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
pub(crate) const MIN_HEIGHT_MM: f64 = 1.0;

/// Colour of the arrow when the pointer is elsewhere.
const ARROW_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 200, 255);

/// The body a pull is adjusting, with the height the document holds and the previewed one.
#[derive(Clone, Copy, Debug)]
struct Pull {
    /// The constructive body.
    object: Id,
    /// The height the document holds.
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

/// What the pull asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtrudeEvent {
    /// The arrow moved: show this height without writing it.
    Preview {
        /// The body being pulled.
        object: Id,
        /// The height the preview shows.
        height: Length,
    },
    /// The pull is done: write this height once, then drop the preview.
    Finish {
        /// The body being pulled.
        object: Id,
        /// The height the pull shows.
        height: Length,
    },
}

/// Where the body sits: its frame, the contour centre and the axis the arrow follows.
#[derive(Clone, Debug, PartialEq)]
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
    fn top(&self, height: Length) -> DVec3 {
        self.placement.transform_point(DVec3::new(self.centre.x, self.centre.y, height.millimetres_f64()))
    }
}

/// Pointer state of the pull.
#[derive(Debug, Default)]
pub(crate) struct ExtrudeGizmo {
    pull: Option<Pull>,
    drag: Option<Drag>,
    hover: bool,
    /// Whether the bar's field still has to take the keyboard focus, so a fresh pull can be typed
    /// into without a click.
    focus: bool,
}

impl ExtrudeGizmo {
    /// Starts a pull on a body that was just extruded at `height`.
    pub(crate) fn start(&mut self, object: Id, height: Length) {
        self.pull = Some(Pull { object, committed: height, height });
        self.drag = None;
        self.hover = false;
        self.focus = true;
    }

    /// Drops the pull. The caller drops the preview overlay in the same breath.
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

    /// Whether the command bar's field must take the focus this frame.
    ///
    /// The flag is cleared by the read, so the field cannot ask for the focus on every frame and
    /// hold it against the user's next click.
    pub(crate) fn take_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus)
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

    /// Reads the pointer: a press on the arrow starts a drag, a move previews a height, a release
    /// keeps the preview open, and a click anywhere else finishes the pull.
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
            let mut moved = false;
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
                    moved = true;
                }
            }
            if ui.input(|input| input.pointer.primary_down()) {
                self.drag = Some(drag);
                return Ok(self
                    .pull
                    .as_ref()
                    .map(|pull| ExtrudeEvent::Preview { object: pull.object, height: pull.height })
                    .filter(|_| moved));
            }
            // The release keeps the pull open on the height it reached: the ✓ writes it, the ✕
            // drops it, and a drag never touches the document.
            return Ok(self
                .pull
                .as_ref()
                .map(|pull| ExtrudeEvent::Preview { object: pull.object, height: pull.height })
                .filter(|_| moved));
        }

        self.hover = pointer.is_some_and(|pointer| arrow_hit(camera, viewport, &body, pull.height, pointer));
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

        if response.clicked_by(egui::PointerButton::Primary) {
            return Ok(Some(ExtrudeEvent::Finish { object: pull.object, height: pull.height }));
        }
        Ok(None)
    }

    /// Paints the pull arrow on the body's top face.
    pub(crate) fn paint(&mut self, ui: &Ui, viewport: egui::Rect, camera: Camera, project: &Project) {
        let Some(pull) = self.pull.as_ref().copied() else {
            return;
        };
        let Some(body) = Body::read(project, pull.object) else {
            return;
        };
        let Some((base, tip)) = arrow_screen(camera, viewport, &body, pull.height) else {
            return;
        };
        let color = if self.hover || self.drag.is_some() { ACTIVE_COLOR } else { ARROW_COLOR };
        let painter = ui.painter().with_clip_rect(viewport);
        paint_arrow(&painter, base, tip, color);
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
    use furnicraft_testkit::millimetres;

    use super::*;

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
    fn the_field_asks_for_the_focus_once() {
        let object = Id::new(7).expect("id");
        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(object, millimetres(100));
        assert!(gizmo.take_focus(), "a fresh pull can be typed into without a click");
        assert!(!gizmo.take_focus(), "the second read holds the flag down");
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

    /// Runs the gizmo over one frame with the pointer at `pointer` and an optional button change.
    fn run_frame(
        ctx: &egui::Context,
        gizmo: &mut ExtrudeGizmo,
        project: &Project,
        frame: u32,
        pointer: Pos2,
        button: Option<(Pos2, bool)>,
    ) -> Option<ExtrudeEvent> {
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
            time: Some(f64::from(frame) * 0.1),
            predicted_dt: 0.1,
            events,
            ..Default::default()
        };
        let mut event = None;
        let output = ctx.run_ui(input, |ui| {
            let (_, response) = ui.allocate_exact_size(viewport().size(), egui::Sense::click_and_drag());
            event = gizmo.update(ui, &response, viewport(), Camera::default(), project).expect("update");
        });
        output.drop_without_applying_deltas();
        event
    }

    /// The height the document holds for an extruded body.
    fn committed_height(project: &Project, object: Id) -> Option<Length> {
        project.object(object).and_then(|node| node.geometry().as_constructive()).and_then(Constructive::height)
    }

    /// The pointer positions of one drag along the arrow, from the press to a point further out.
    fn drag_points(project: &Project, object: Id) -> (Pos2, Pos2) {
        let body = Body::read(project, object).expect("a body");
        let (base, tip) = arrow_screen(Camera::default(), viewport(), &body, millimetres(100)).expect("an arrow");
        let direction = (tip - base).normalized();
        (base + direction * 40.0, base + direction * 120.0)
    }

    #[test]
    fn a_drag_previews_the_height_without_touching_the_document() {
        let (project, id) = body_project();
        let revision = project.revision();
        let (start, end) = drag_points(&project, id);

        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(id, millimetres(100));
        let ctx = egui::Context::default();
        assert_eq!(run_frame(&ctx, &mut gizmo, &project, 0, start, None), None);
        assert_eq!(
            run_frame(&ctx, &mut gizmo, &project, 1, start, Some((start, true))),
            None,
            "a press starts the drag"
        );
        let Some(ExtrudeEvent::Preview { object, height }) = run_frame(&ctx, &mut gizmo, &project, 2, end, None) else {
            panic!("a drag previews the height it reached");
        };
        assert_eq!(object, id);
        assert!(height > millimetres(100), "the drag raised the preview, got {height}");
        assert_eq!(gizmo.height(), Some(height));
        assert_eq!(committed_height(&project, id), Some(millimetres(100)), "the body did not move");
        assert_eq!(project.revision(), revision, "a drag does not revise the document");
    }

    #[test]
    fn a_release_keeps_the_preview_and_writes_nothing() {
        let (project, id) = body_project();
        let revision = project.revision();
        let (start, end) = drag_points(&project, id);

        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(id, millimetres(100));
        let ctx = egui::Context::default();
        run_frame(&ctx, &mut gizmo, &project, 0, start, None);
        run_frame(&ctx, &mut gizmo, &project, 1, start, Some((start, true)));
        run_frame(&ctx, &mut gizmo, &project, 2, end, None);
        let released = run_frame(&ctx, &mut gizmo, &project, 3, end, Some((end, false)));
        assert_eq!(released, None, "the release adds no height the drag had not already shown");
        assert!(gizmo.is_active(), "the pull stays open until the check or the cross");
        assert_eq!(gizmo.committed(), Some(millimetres(100)), "the document still holds the created height");
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn a_click_outside_the_arrow_finishes_the_pull_at_its_height() {
        let (project, id) = body_project();
        let pointer = Pos2::new(700.0, 500.0);
        let mut gizmo = ExtrudeGizmo::default();
        gizmo.start(id, millimetres(100));
        let ctx = egui::Context::default();
        run_frame(&ctx, &mut gizmo, &project, 0, pointer, None);
        run_frame(&ctx, &mut gizmo, &project, 1, pointer, Some((pointer, true)));
        let event = run_frame(&ctx, &mut gizmo, &project, 2, pointer, Some((pointer, false)));
        assert_eq!(event, Some(ExtrudeEvent::Finish { object: id, height: millimetres(100) }));
        assert!(Body::read(&project, id).is_some(), "finishing reads the body, it does not touch it");
    }
}
