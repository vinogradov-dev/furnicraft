//! The viewport tool that draws on the work plane.
//!
//! While a drawing draft is live the gizmo owns the pointer: it maps the cursor onto the work
//! plane, snaps it to a draft end, an axis or the 10 mm grid, appends a point on a click and closes
//! a polyline through its first point. Tab chooses whether the next polyline segment is straight or
//! an arc, so one chain carries both kinds; Alt drops every snap and takes the pointer as it is.
//! Painting shows the placed chain, the shape the next click would make, the snap marker and a live
//! size. A preview that cannot become a path keeps the last valid one, so a chain never blinks out;
//! the reason is on [`DrawGizmo::preview_error`]. The dummy document is never touched here; the
//! commit is a separate step, so Escape costs nothing.

use eframe::egui::{self, Color32, FontId, Pos2, Shape, Stroke, Ui};
use furnicraft_geom::DVec2;
use furnicraft_model::{ContourSegment, Path};
use furnicraft_scene::Camera;
use furnicraft_ui::{DrawDraft, DrawTool, Editor};
use furnicraft_units::Length;

use crate::gizmo_math::{
    SnapMode, SnapRequest, SnapTarget, WorkPlane, length_from_millimetres, ray_to, screen_point, snap_point,
};

/// Pointer distance inside which a draft end captures the pointer, in points.
const SNAP_RADIUS_POINTS: f32 = 10.0;
/// Radius of a placed draft point marker, in points.
const POINT_RADIUS: f32 = 3.5;
/// Sampling tolerance for a painted preview, in millimetres.
const PREVIEW_TOLERANCE_MM: f64 = 0.25;

/// Colour of the points already placed.
const DRAFT_COLOR: Color32 = Color32::from_rgb(120, 200, 255);
/// Colour of the shape the next click would make.
const PREVIEW_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
/// Colour of a live-size label.
const SIZE_COLOR: Color32 = Color32::from_rgb(245, 245, 245);
/// Fill behind a live-size label.
const SIZE_BACKGROUND: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 190);
/// Colour of a draft end that would close the chain.
const CLOSE_COLOR: Color32 = Color32::from_rgb(120, 230, 140);

/// The keyboard state a click carries while a drawing tool runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawModifiers {
    /// Shift locks the next segment to the nearest axis.
    pub axis_lock: bool,
    /// Alt takes the pointer without snapping it to a draft end or the grid.
    pub ignore_snaps: bool,
}

/// What a click on the snapped point does to the live draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawClick {
    /// Append the point to the chain as a straight segment.
    Append([Length; 2]),
    /// Append the point as an arc through it, waiting for the next click.
    StartArc([Length; 2]),
    /// Close the chain through its first point.
    Close,
    /// The click cannot change the draft.
    Ignore,
}

/// How the live pointer is snapped to the work plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapSettings {
    /// The 10 mm grid is on.
    grid: bool,
    /// Shift locks the next point to an axis through the anchor.
    axis_lock: bool,
    /// Alt drops the end and grid snaps.
    ignore_snaps: bool,
}

/// Pointer state of the drawing tool; the draft itself belongs to the editor.
#[derive(Debug)]
pub(crate) struct DrawGizmo {
    grid: bool,
    hover: Option<SnapOutcome>,
    /// The plane the draft started on, frozen while the tool runs.
    ///
    /// Orbiting the view mid-drawing must not reinterpret the points already placed, so the plane
    /// is chosen once and kept until the tool ends.
    plane: Option<WorkPlane>,
    /// The last preview that became a path, kept while the pointer sits on a hostile point.
    preview: Option<Path>,
    /// Why the current pointer cannot extend the draft, if it cannot.
    preview_error: Option<String>,
}

/// The snapped cursor and where on screen it landed.
#[derive(Clone, Copy, Debug)]
struct SnapOutcome {
    /// The point a click would use.
    point: [Length; 2],
    /// Why the point moved.
    target: SnapTarget,
    /// The snapped point in viewport points.
    screen: Pos2,
}

impl DrawGizmo {
    /// A drawing tool with the 10 mm grid on and no hover.
    pub(crate) fn new() -> Self {
        Self { grid: true, hover: None, plane: None, preview: None, preview_error: None }
    }

    /// Drops the hover state, the frozen plane and the cached preview, for example when the tool
    /// ends.
    pub(crate) fn reset(&mut self) {
        self.hover = None;
        self.plane = None;
        self.preview = None;
        self.preview_error = None;
    }

    /// The last valid preview, for painting and for tests.
    pub(crate) fn preview(&self) -> Option<&Path> {
        self.preview.as_ref()
    }

    /// Why the current pointer cannot extend the draft, if it cannot.
    ///
    /// The status line shows this instead of letting a rejected point look like an accepted one.
    pub(crate) fn preview_error(&self) -> Option<&str> {
        self.preview_error.as_deref()
    }

    /// Records what the pointer at `point` would add: a new preview, or the reason there is none.
    ///
    /// A rejected point keeps the previous preview, so the chain the user already built stays on
    /// screen through an intermediate click that cannot join it. A point that simply adds nothing —
    /// the pointer sits on the chain's own end right after a click — keeps it too: clearing the
    /// cache there is what made the preview blink between clicks.
    fn refresh_preview(&mut self, draft: &DrawDraft, point: [Length; 2]) {
        match draft.preview(point) {
            Ok(Some(path)) => {
                self.preview = Some(path);
                self.preview_error = None;
            }
            Ok(None) => self.preview_error = None,
            Err(error) => self.preview_error = Some(error.to_string()),
        }
    }

    /// The plane the live draft draws on.
    pub(crate) fn plane(&self, camera: Camera) -> WorkPlane {
        self.plane.unwrap_or_else(|| WorkPlane::from_camera(camera))
    }

    /// Whether points snap to the 10 mm grid.
    pub(crate) const fn grid(&self) -> bool {
        self.grid
    }

    /// Turns the grid snap on or off.
    pub(crate) fn toggle_grid(&mut self) {
        self.grid = !self.grid;
        self.hover = None;
    }

    /// The cursor the viewport shows while a tool is live.
    pub(crate) const fn cursor(&self) -> egui::CursorIcon {
        egui::CursorIcon::Crosshair
    }

    /// Applies the pointer to the live draft.
    ///
    /// A click appends the snapped point, or closes a polyline when the point is its first one. The
    /// gizmo owns the left button while the tool is live: the transform gizmos are hidden by the
    /// caller, and `pointer_over_widget` keeps the corner orientation widget usable. Returns whether
    /// the point was accepted, which lets the caller commit a finished tool.
    ///
    /// Clicking a point that would close the chain is [`DrawClick::Close`] and the caller commits;
    /// clicking a fixed tool's last point does nothing.
    pub(crate) fn update(
        &mut self,
        response: &egui::Response,
        rect: egui::Rect,
        camera: Camera,
        editor: &mut Editor,
        pointer_over_widget: bool,
        modifiers: DrawModifiers,
    ) -> Result<bool, String> {
        let Some(draft) = editor.drawing().cloned() else {
            self.reset();
            return Ok(false);
        };
        let plane = *self.plane.get_or_insert_with(|| WorkPlane::from_camera(camera));
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let Some(pointer) = pointer else {
            self.hover = None;
            return Ok(false);
        };
        let Some(outcome) = snap_cursor(
            &draft,
            plane,
            camera,
            rect,
            pointer,
            SnapSettings { grid: self.grid, axis_lock: modifiers.axis_lock, ignore_snaps: modifiers.ignore_snaps },
        ) else {
            self.hover = None;
            return Ok(false);
        };
        self.hover = Some(outcome);
        self.refresh_preview(&draft, outcome.point);

        // A click that the orientation widget owns must not also place a point.
        if pointer_over_widget || !response.clicked_by(egui::PointerButton::Primary) {
            return Ok(false);
        }
        let accepted = match click_action(&draft, outcome.point) {
            DrawClick::Append(point) => {
                editor.drawing_point(point).map_err(|error| error.to_string())?;
                true
            }
            DrawClick::StartArc(point) => {
                editor.drawing_arc_through(point).map_err(|error| error.to_string())?;
                true
            }
            DrawClick::Close => {
                editor.close_drawing().map_err(|error| error.to_string())?;
                true
            }
            DrawClick::Ignore => false,
        };
        if accepted && let Some(draft) = editor.drawing() {
            // The click moved the chain, so the preview belongs to the new state under the same
            // pointer rather than to the state before it.
            self.refresh_preview(draft, outcome.point);
        }
        Ok(accepted)
    }

    /// Paints the placed chain, the preview, the points, the snap marker and the live size.
    pub(crate) fn paint(&self, ui: &Ui, rect: egui::Rect, camera: Camera, draft: &DrawDraft) {
        let plane = self.plane(camera);
        let painter = ui.painter().with_clip_rect(rect);

        match draft.to_path() {
            Ok(path) => paint_path(&painter, camera, rect, plane, &path, Stroke::new(2.0, DRAFT_COLOR)),
            // A chain that cannot become one path still has the segments it already stores: draw
            // them one by one so a ring rule or a missing point never makes the drawing vanish.
            Err(_) => {
                for segment in draft.segments() {
                    if let Ok(path) = Path::new(vec![*segment], false) {
                        paint_path(&painter, camera, rect, plane, &path, Stroke::new(2.0, DRAFT_COLOR));
                    }
                }
            }
        }
        if let Some(hover) = self.hover {
            if let Some(path) = self.preview() {
                paint_path(&painter, camera, rect, plane, path, Stroke::new(1.5, PREVIEW_COLOR));
            }
            for point in draft.points() {
                paint_point(&painter, camera, rect, plane, *point, DRAFT_COLOR, POINT_RADIUS);
            }
            if draft.can_close() && draft.points().first() == Some(&hover.point) {
                paint_point(&painter, camera, rect, plane, hover.point, CLOSE_COLOR, POINT_RADIUS + 2.0);
            }
            paint_marker(&painter, hover);
            if let Some(label) = size_label(draft, hover.point) {
                paint_size(&painter, hover.screen, &label);
            }
        } else {
            for point in draft.points() {
                paint_point(&painter, camera, rect, plane, *point, DRAFT_COLOR, POINT_RADIUS);
            }
        }
    }
}

/// The snapped cursor point, in document units and on screen.
fn snap_cursor(
    draft: &DrawDraft,
    plane: WorkPlane,
    camera: Camera,
    rect: egui::Rect,
    pointer: Pos2,
    settings: SnapSettings,
) -> Option<SnapOutcome> {
    let ray = ray_to(pointer, rect, camera);
    let world = plane.ray_point(ray)?;
    let depth = (world - camera.eye()).dot(camera.forward());
    let radius = camera.world_per_point(depth, f64::from(rect.height())) * f64::from(SNAP_RADIUS_POINTS);
    let raw = plane.local(world);
    // The next segment starts where the last stored segment ended, so the axis and end snap use the
    // stored joint rather than the click that began it.
    let anchor = draft.last_end().map(plane_point);
    // The last point is the segment start, so it is not offered as a place to join.
    let ends = draft
        .points()
        .split_last()
        .map_or(&[][..], |(_, ends)| ends)
        .iter()
        .map(|point| plane_point(*point))
        .collect::<Vec<_>>();
    let snapped = snap_point(SnapRequest {
        point: raw,
        anchor,
        ends: &ends,
        mode: if settings.grid { SnapMode::Grid } else { SnapMode::Free },
        axis_lock: settings.axis_lock,
        end_radius: radius,
        ignore_snaps: settings.ignore_snaps,
    });
    let (x, y) = (length_from_millimetres(snapped.point.x), length_from_millimetres(snapped.point.y));
    let point = [x?, y?];
    let world = plane.world(DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64()));
    let screen = screen_point(camera, rect, world)?;
    Some(SnapOutcome { point, target: snapped.target, screen })
}

/// What the click does, given the live draft and the snapped point.
///
/// While an arc waits for its end, every click ends it: the gesture is already chosen and a second
/// one would be ambiguous, so this test comes before the close-on-first-point check. Otherwise Tab
/// decides whether a polyline grows a straight segment or starts an arc.
fn click_action(draft: &DrawDraft, point: [Length; 2]) -> DrawClick {
    if draft.is_closed() {
        return DrawClick::Ignore;
    }
    if draft.pending_arc().is_some() {
        return DrawClick::Append(point);
    }
    if draft.tool().closes() && draft.points().first() == Some(&point) {
        return if draft.can_close() { DrawClick::Close } else { DrawClick::Ignore };
    }
    if let Some(need) = draft.tool().points()
        && draft.points().len() >= need
    {
        return DrawClick::Ignore;
    }
    if draft.last_end() == Some(point) {
        return DrawClick::Ignore;
    }
    if draft.arc_mode() && draft.tool().closes() && !draft.is_empty() {
        return DrawClick::StartArc(point);
    }
    DrawClick::Append(point)
}

/// The live size next to the cursor, or `None` while the tool has nothing to measure.
fn size_label(draft: &DrawDraft, next: [Length; 2]) -> Option<String> {
    let last = draft.last_end()?;
    let anchor = plane_point(last);
    let point = plane_point(next);
    match draft.tool() {
        DrawTool::Rectangle if draft.points().len() == 1 => {
            let width = length_from_millimetres((point.x - anchor.x).abs())?;
            let height = length_from_millimetres((point.y - anchor.y).abs())?;
            Some(format!("{width} × {height}"))
        }
        DrawTool::Circle if draft.points().len() == 1 => {
            let radius = length_from_millimetres((point - anchor).length())?;
            Some(format!("R {radius}"))
        }
        DrawTool::Arc if draft.points().len() == 2 => {
            let path = draft.preview_path(next)?;
            let radius = path.entries().first().and_then(|entry| match entry.segment() {
                ContourSegment::Arc { radius, .. } => Some(radius),
                ContourSegment::Line { .. } => None,
            })?;
            Some(format!("R {radius}"))
        }
        _ => {
            let length = length_from_millimetres((point - anchor).length())?;
            Some(length.to_string())
        }
    }
}

/// A document point in work-plane millimetres.
fn plane_point(point: [Length; 2]) -> DVec2 {
    DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64())
}

/// Paints one path as a screen-space polyline through its sampled points.
fn paint_path(
    painter: &egui::Painter,
    camera: Camera,
    rect: egui::Rect,
    plane: WorkPlane,
    path: &Path,
    stroke: Stroke,
) {
    let Ok(segments) = path.segments().map(|segment| segment.to_geom()).collect::<Result<Vec<_>, _>>() else {
        return;
    };
    let Ok(chain) = furnicraft_geom::Contour::new_with_joint_tolerance(segments, false, |first, next| {
        furnicraft_model::joint_tolerance_mm(*first, *next)
    }) else {
        return;
    };
    let Ok(samples) = chain.sample(PREVIEW_TOLERANCE_MM) else {
        return;
    };
    let mut screen = Vec::with_capacity(samples.len());
    for point in samples {
        let world = plane.world(point);
        let Some(position) = screen_point(camera, rect, world) else {
            continue;
        };
        screen.push(position);
    }
    if screen.len() >= 2 {
        painter.add(Shape::line(screen, stroke));
    }
}

/// Paints one point marker on the work plane.
fn paint_point(
    painter: &egui::Painter,
    camera: Camera,
    rect: egui::Rect,
    plane: WorkPlane,
    point: [Length; 2],
    color: Color32,
    radius: f32,
) {
    let Some(position) = screen_point(camera, rect, plane.world(plane_point(point))) else {
        return;
    };
    painter.circle_filled(position, radius, color);
    painter.circle_stroke(position, radius, Stroke::new(1.0, Color32::from_black_alpha(160)));
}

/// Paints the snap marker: a diamond whose colour says what captured the pointer.
fn paint_marker(painter: &egui::Painter, hover: SnapOutcome) {
    let color = match hover.target {
        SnapTarget::End => CLOSE_COLOR,
        SnapTarget::Grid => Color32::from_gray(210),
        SnapTarget::Axis => Color32::from_rgb(255, 220, 96),
        SnapTarget::Free => Color32::from_gray(150),
    };
    let center = hover.screen;
    let size = 6.0;
    let corners = vec![
        Pos2::new(center.x, center.y - size),
        Pos2::new(center.x + size, center.y),
        Pos2::new(center.x, center.y + size),
        Pos2::new(center.x - size, center.y),
    ];
    painter.add(Shape::closed_line(corners, Stroke::new(1.5, color)));
}

/// Paints the live size next to the cursor on a readable background.
fn paint_size(painter: &egui::Painter, pointer: Pos2, label: &str) {
    let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.0), SIZE_COLOR);
    let padding = egui::vec2(5.0, 3.0);
    let rect = egui::Rect::from_min_size(pointer + egui::vec2(14.0, 10.0), galley.size() + padding * 2.0);
    painter.rect_filled(rect, 3.0, SIZE_BACKGROUND);
    painter.galley(rect.min + padding, galley, SIZE_COLOR);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    fn draft(tool: DrawTool, points: &[(i64, i64)]) -> DrawDraft {
        let mut draft = DrawDraft::new(tool);
        for (x, y) in points {
            draft.push([millimetres(*x), millimetres(*y)]).expect("a fresh point");
        }
        draft
    }

    #[test]
    fn a_polyline_closes_through_its_first_point_only() {
        let polyline = draft(DrawTool::Polyline, &[(0, 0), (600, 0), (600, 400)]);
        let first = [millimetres(0), millimetres(0)];
        assert_eq!(click_action(&polyline, first), DrawClick::Close);
        assert_eq!(
            click_action(&polyline, [millimetres(300), millimetres(200)]),
            DrawClick::Append([millimetres(300), millimetres(200)])
        );

        let short = draft(DrawTool::Polyline, &[(0, 0), (600, 0)]);
        assert_eq!(click_action(&short, first), DrawClick::Ignore, "two points cannot close a ring");
    }

    #[test]
    fn tab_starts_an_arc_only_inside_a_running_chain() {
        let mut polyline = draft(DrawTool::Polyline, &[(0, 0), (600, 0)]);
        let through = [millimetres(650), millimetres(50)];
        assert_eq!(click_action(&polyline, through), DrawClick::Append(through));
        assert!(polyline.toggle_arc_mode());
        assert_eq!(click_action(&polyline, through), DrawClick::StartArc(through));

        // A chain needs an anchor before an arc can leave it.
        let mut fresh = DrawDraft::new(DrawTool::Polyline);
        fresh.toggle_arc_mode();
        assert_eq!(click_action(&fresh, through), DrawClick::Append(through));
        // A fixed tool keeps its own gesture and never mixes.
        let line = draft(DrawTool::Line, &[(0, 0)]);
        assert_eq!(click_action(&line, through), DrawClick::Append(through));
    }

    #[test]
    fn a_pending_arc_takes_the_click_before_the_closing_point() {
        // Once an arc waits for its end, even a click on the chain's first point ends the arc; the
        // close rule then runs on the finished segment, where it belongs.
        let mut polyline = draft(DrawTool::Polyline, &[(0, 0), (600, 0), (600, 400)]);
        polyline.toggle_arc_mode();
        polyline.push_arc_through([millimetres(300), millimetres(300)]).expect("arm the arc");
        assert_eq!(
            click_action(&polyline, [millimetres(0), millimetres(0)]),
            DrawClick::Append([millimetres(0), millimetres(0)])
        );
    }

    #[test]
    fn a_fixed_tool_ignores_a_click_after_its_last_point() {
        let line = draft(DrawTool::Line, &[(0, 0), (600, 0)]);
        assert_eq!(click_action(&line, [millimetres(600), millimetres(400)]), DrawClick::Ignore);
        assert_eq!(
            click_action(&draft(DrawTool::Circle, &[(100, 100)]), [millimetres(0), millimetres(0)]),
            DrawClick::Append([millimetres(0), millimetres(0)])
        );
    }

    #[test]
    fn a_click_on_the_previous_point_or_a_closed_chain_does_nothing() {
        let line = draft(DrawTool::Line, &[(0, 0)]);
        let same = [millimetres(0), millimetres(0)];
        assert_eq!(click_action(&line, same), DrawClick::Ignore);

        let mut closed = draft(DrawTool::Polyline, &[(0, 0), (600, 0), (600, 400)]);
        closed.close().expect("close the triangle");
        assert_eq!(click_action(&closed, [millimetres(10), millimetres(10)]), DrawClick::Ignore);
    }

    #[test]
    fn the_live_size_names_each_tools_measure() {
        let polyline = draft(DrawTool::Polyline, &[(0, 0), (600, 0)]);
        assert_eq!(size_label(&polyline, [millimetres(600), millimetres(400)]).as_deref(), Some("400 mm"));

        let line = draft(DrawTool::Line, &[(0, 0)]);
        assert_eq!(size_label(&line, [millimetres(600), millimetres(0)]).as_deref(), Some("600 mm"));

        let rectangle = draft(DrawTool::Rectangle, &[(0, 0)]);
        assert_eq!(size_label(&rectangle, [millimetres(600), millimetres(400)]).as_deref(), Some("600 mm × 400 mm"));

        let circle = draft(DrawTool::Circle, &[(100, 50)]);
        assert_eq!(size_label(&circle, [millimetres(130), millimetres(90)]).as_deref(), Some("R 50 mm"));

        let arc = draft(DrawTool::Arc, &[(0, 0), (50, 50)]);
        assert_eq!(size_label(&arc, [millimetres(100), millimetres(0)]).as_deref(), Some("R 50 mm"));

        assert!(size_label(&DrawDraft::new(DrawTool::Line), [millimetres(10), millimetres(0)]).is_none());
    }

    #[test]
    fn a_rejected_preview_keeps_the_last_valid_one() {
        let mut draft = DrawDraft::new(DrawTool::Arc);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        draft.push([millimetres(50), millimetres(50)]).expect("through");
        let mut gizmo = DrawGizmo::new();
        gizmo.refresh_preview(&draft, [millimetres(100), millimetres(0)]);
        assert!(gizmo.preview().is_some(), "three sound points preview an arc");
        assert_eq!(gizmo.preview_error(), None);

        // The third click is collinear, so the arc cannot exist; the last arc stays on screen and
        // the status line gets the reason instead of a blink.
        gizmo.refresh_preview(&draft, [millimetres(100), millimetres(100)]);
        assert!(gizmo.preview().is_some(), "the previous preview survives a rejected point");
        assert!(gizmo.preview_error().is_some(), "the reason is reported");

        gizmo.reset();
        assert!(gizmo.preview().is_none());
        assert_eq!(gizmo.preview_error(), None);
    }

    #[test]
    fn a_point_that_adds_nothing_keeps_the_preview() {
        // After a click the pointer sits on the segment's own stored end, so `preview` returns
        // `Ok(None)`; dropping the cache there made the chain blink between clicks.
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        let mut gizmo = DrawGizmo::new();
        let next = [millimetres(600), millimetres(0)];
        gizmo.refresh_preview(&draft, next);
        assert!(gizmo.preview().is_some(), "the first segment previews");
        draft.push(next).expect("the click places it");
        gizmo.refresh_preview(&draft, next);
        assert_eq!(draft.preview(next), Ok(None), "the pointer adds nothing on the stored end");
        assert!(gizmo.preview().is_some(), "the last valid preview survives the click");
        assert_eq!(gizmo.preview_error(), None);
    }

    #[test]
    fn arming_an_arc_keeps_the_chain_preview() {
        // The click that arms an arc cannot preview an arc to itself (three equal points are
        // degenerate), so the last shape stays on screen while the pointer waits for the end.
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        draft.push([millimetres(600), millimetres(0)]).expect("bottom");
        draft.push([millimetres(600), millimetres(400)]).expect("right");
        draft.toggle_arc_mode();
        let mut gizmo = DrawGizmo::new();
        let through = [millimetres(300), millimetres(300)];
        gizmo.refresh_preview(&draft, through);
        assert!(gizmo.preview().is_some(), "the next straight segment previews");
        draft.push_arc_through(through).expect("arm the arc");
        gizmo.refresh_preview(&draft, through);
        assert!(gizmo.preview().is_some(), "the pending arc never blanks the preview");
        assert!(gizmo.preview_error().is_some(), "the degenerate arc names its reason");
    }

    #[test]
    fn a_grid_click_and_a_free_click_produce_different_points() {
        let draft = DrawDraft::new(DrawTool::Polyline);
        let mut gizmo = DrawGizmo::new();
        assert!(gizmo.grid());
        gizmo.toggle_grid();
        assert!(!gizmo.grid());
        gizmo.toggle_grid();
        assert!(gizmo.grid());
        assert_eq!(gizmo.cursor(), egui::CursorIcon::Crosshair);
        assert_eq!(size_label(&draft, [millimetres(0), millimetres(0)]), None);
    }
}
