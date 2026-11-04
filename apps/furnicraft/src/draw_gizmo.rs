//! The viewport tool that draws on the work plane.
//!
//! While a drawing draft is live the gizmo owns the pointer: it maps the cursor onto the work
//! plane, snaps it to a draft end, an axis or the 10 mm grid, appends a point on a click and closes
//! a polyline through its first point. Painting shows the placed chain, the shape the next click
//! would make, the snap marker and a live size. The dummy document is never touched here; the
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
/// Joint tolerance for the preview chain, in millimetres.
const PREVIEW_JOINT_TOLERANCE_MM: f64 = 0.001;

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

/// What a click on the snapped point does to the live draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DrawClick {
    /// Append the point to the chain.
    Append([Length; 2]),
    /// Close the chain through its first point.
    Close,
    /// The click cannot change the draft.
    Ignore,
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
        Self { grid: true, hover: None, plane: None }
    }

    /// Drops the hover state and the frozen plane, for example when the tool ends.
    pub(crate) fn reset(&mut self) {
        self.hover = None;
        self.plane = None;
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
        axis_lock: bool,
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
        let Some(outcome) = snap_cursor(&draft, plane, camera, rect, pointer, self.grid, axis_lock) else {
            self.hover = None;
            return Ok(false);
        };
        self.hover = Some(outcome);

        // A click that the orientation widget owns must not also place a point.
        if pointer_over_widget || !response.clicked_by(egui::PointerButton::Primary) {
            return Ok(false);
        }
        match click_action(&draft, outcome.point) {
            DrawClick::Append(point) => {
                editor.drawing_point(point).map_err(|error| error.to_string())?;
                Ok(true)
            }
            DrawClick::Close => {
                editor.close_drawing().map_err(|error| error.to_string())?;
                Ok(true)
            }
            DrawClick::Ignore => Ok(false),
        }
    }

    /// Paints the placed chain, the preview, the points, the snap marker and the live size.
    pub(crate) fn paint(&self, ui: &Ui, rect: egui::Rect, camera: Camera, draft: &DrawDraft) {
        let plane = self.plane(camera);
        let painter = ui.painter().with_clip_rect(rect);

        if let Ok(path) = draft.to_path() {
            paint_path(&painter, camera, rect, plane, &path, Stroke::new(2.0, DRAFT_COLOR));
        }
        if let Some(hover) = self.hover {
            if let Some(path) = draft.preview_path(hover.point) {
                paint_path(&painter, camera, rect, plane, &path, Stroke::new(1.5, PREVIEW_COLOR));
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
    grid: bool,
    axis_lock: bool,
) -> Option<SnapOutcome> {
    let ray = ray_to(pointer, rect, camera);
    let world = plane.ray_point(ray)?;
    let depth = (world - camera.eye()).dot(camera.forward());
    let radius = camera.world_per_point(depth, f64::from(rect.height())) * f64::from(SNAP_RADIUS_POINTS);
    let raw = plane.local(world);
    let anchor = draft.points().last().map(|point| plane_point(*point));
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
        mode: if grid { SnapMode::Grid } else { SnapMode::Free },
        axis_lock,
        end_radius: radius,
    });
    let (x, y) = (length_from_millimetres(snapped.point.x), length_from_millimetres(snapped.point.y));
    let point = [x?, y?];
    let world = plane.world(DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64()));
    let screen = screen_point(camera, rect, world)?;
    Some(SnapOutcome { point, target: snapped.target, screen })
}

/// What the click does, given the live draft and the snapped point.
fn click_action(draft: &DrawDraft, point: [Length; 2]) -> DrawClick {
    if draft.is_closed() {
        return DrawClick::Ignore;
    }
    if draft.tool().closes() && draft.points().first() == Some(&point) {
        return if draft.can_close() { DrawClick::Close } else { DrawClick::Ignore };
    }
    if let Some(need) = draft.tool().points()
        && draft.len() >= need
    {
        return DrawClick::Ignore;
    }
    if draft.points().last() == Some(&point) {
        return DrawClick::Ignore;
    }
    DrawClick::Append(point)
}

/// The live size next to the cursor, or `None` while the tool has nothing to measure.
fn size_label(draft: &DrawDraft, next: [Length; 2]) -> Option<String> {
    let last = draft.points().last()?;
    let anchor = plane_point(*last);
    let point = plane_point(next);
    match draft.tool() {
        DrawTool::Rectangle if draft.len() == 1 => {
            let width = length_from_millimetres((point.x - anchor.x).abs())?;
            let height = length_from_millimetres((point.y - anchor.y).abs())?;
            Some(format!("{width} × {height}"))
        }
        DrawTool::Circle if draft.len() == 1 => {
            let radius = length_from_millimetres((point - anchor).length())?;
            Some(format!("R {radius}"))
        }
        DrawTool::Arc if draft.len() == 2 => {
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
    let Ok(chain) = furnicraft_geom::Contour::new_with_tolerance(segments, false, PREVIEW_JOINT_TOLERANCE_MM) else {
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
