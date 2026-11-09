//! The corner orientation widget.
//!
//! Six world-axis balls are projected onto the viewport with the camera basis, so the widget reads
//! like the axes in the scene instead of a fixed diagram. Clicking one snaps the camera to look at
//! its target from that direction; clicking the direction the camera already looks from flips to
//! the opposite side, so the centre ball remains useful in a top or bottom view. A *home* button in
//! the cube's top-right corner returns the view to *Axonometric*, a flat projection button under the
//! cube switches orthographic and perspective, and an optional one-line readout under them names the
//! pitch, the yaw, the roll and the projection the viewport is drawing with.
//!
//! A screen-fixed ring around the balls turns the view about its own axis: four nodes at the top,
//! left, bottom and right set the absolute roll to 0°, 90°, 180° and 270°, and a drag anywhere on
//! the annulus turns it by hand in the step from the workspace settings. The accent handle sits at
//! the camera's current roll, so the ring reads even when the view is not a preset.
use std::f64::consts::{PI, TAU};

use eframe::egui;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2};
use furnicraft_geom::DVec3;
use furnicraft_scene::{Camera, MAX_PITCH, MIN_PITCH, Projection};
use furnicraft_ui::{icons, theme};
use furnicraft_units::Degrees;

/// Whether a direction should have a line connector to the center in the corner widget.
///
/// Only the positive axes (+X, +Y, +Z) get a connector line; negative directions draw an
/// unfilled outline instead.
fn has_connector(direction: DVec3) -> bool {
    direction.x > 0.0 || direction.y > 0.0 || direction.z > 0.0
}

/// Width and height of the cube, in points.
pub(crate) const SIZE: f32 = 124.0;

/// Distance from the viewport corner, in points.
const MARGIN: f32 = 12.0;

/// Radius of the circle the axis balls sit on, in points.
const RADIUS: f32 = 38.0;

/// Radius of an axis ball, in points.
const DOT_RADIUS: f32 = 8.5;

/// Side of the square click target around a ball, in points.
const HIT_SIZE: f32 = 22.0;

/// Radius of the roll ring, in points.
const RING_RADIUS: f32 = 52.0;

/// Stroke width of the roll ring, in points.
const RING_STROKE: f32 = 1.2;

/// Half-width of the annulus that starts a roll drag, in points.
const RING_HIT: f32 = 8.0;

/// Radius of one roll node, in points.
const NODE_RADIUS: f32 = 4.0;

/// Half-side of a node's square click target, in points.
const NODE_HIT: f32 = 9.0;

/// Radius of the accent handle that marks the current roll, in points.
const HANDLE_RADIUS: f32 = 5.0;

/// Side of the home button in the cube's top-right corner, in points.
const HOME_SIZE: f32 = 22.0;

/// Side of the projection button under the cube, in points.
const BUTTON_SIZE: f32 = 22.0;

/// Point size of the projection glyph, in points.
const BUTTON_ICON_POINT_SIZE: f32 = 20.0;

/// Gap between the cube, the projection button and the readout, in points.
const GAP: f32 = 2.0;

/// Height reserved for one line of the readout, in points.
const READOUT_HEIGHT: f32 = 14.0;

/// The quarter-turn nodes of the ring, counter-clockwise from the top.
const NODE_DEGREES: [i64; 4] = [0, 90, 180, 270];

// Gamma-space colours, matching the default grid axis colours in `furnicraft_scene`.
const X_COLOR: Color32 = Color32::from_rgb(184, 71, 71);
const Y_COLOR: Color32 = Color32::from_rgb(71, 158, 87);
const Z_COLOR: Color32 = Color32::from_rgb(77, 115, 209);

/// The six world axes, their colours and their labels, in the order the widget projects them.
const AXIS_DIRECTIONS: [(DVec3, Color32, &str); 6] = [
    (DVec3::X, X_COLOR, "X"),
    (DVec3::new(-1.0, 0.0, 0.0), X_COLOR, "-X"),
    (DVec3::Y, Y_COLOR, "Y"),
    (DVec3::new(0.0, -1.0, 0.0), Y_COLOR, "-Y"),
    (DVec3::Z, Z_COLOR, "Z"),
    (DVec3::new(0.0, 0.0, -1.0), Z_COLOR, "-Z"),
];

/// What a click or drag on the widget asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GizmoAction {
    /// Look at the target from this eye direction.
    LookFrom(DVec3),
    /// Return the view to the axonometric preset.
    Home,
    /// Switch the view between the orthographic and the perspective projection.
    ToggleProjection,
    /// Set the view's roll about its own axis to this absolute angle.
    RollTo(Degrees),
    /// Turn the view about its own axis by this angle.
    RollBy(Degrees),
}

/// The live roll drag: the pointer angle at the previous frame and what was already applied.
///
/// The drag accumulates the unwrapped visual angle and applies the difference between the quantised
/// total and what the camera already received, so a slow drag never loses or repeats a step and a
/// drag across the ±180° seam does not jump.
#[derive(Clone, Copy, Debug)]
struct RingDrag {
    /// The pointer's visual angle at the previous frame, in radians.
    last_angle: f64,
    /// The total visual turn since the press, unwrapped, in degrees.
    accumulated_degrees: f64,
    /// The total turn already handed to the camera, in degrees.
    applied_degrees: f64,
    /// The step this drag started with; a settings change applies to the next drag.
    step_degrees: u32,
}

/// The widget's interaction state that outlives one frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct Gizmo {
    /// The roll drag in progress, if any.
    drag: Option<RingDrag>,
}

impl Gizmo {
    /// Whether a roll drag currently owns the pointer.
    ///
    /// The shell reads this before it paints, so a drag that started on the ring never also orbits
    /// the viewport underneath it.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Draws the widget and returns the one action a click or a drag step asked for.
    ///
    /// `readout` turns the line under the projection button on; the numbers are read from the same
    /// camera the viewport draws with, so the label cannot disagree with the image.
    /// `step_degrees` is the hand-turn step from the workspace settings.
    pub(crate) fn show(
        &mut self,
        ui: &Ui,
        viewport: Rect,
        camera: Camera,
        readout: bool,
        step_degrees: u32,
    ) -> Option<GizmoAction> {
        let rect = widget_rect(viewport)?;
        let cube = Rect::from_min_size(rect.min, Vec2::splat(SIZE));
        let center = cube.center();
        let painter = ui.painter().with_clip_rect(rect);

        // The ring is registered first, so the axis balls, the nodes, Home and the projection button
        // all win the hit where they overlap it.
        let mut action = self.ring(ui, &painter, rect, center, camera, step_degrees);

        let forward = camera.forward();
        let right = camera.right();
        let up = camera.up();

        let mut entries: Vec<(f32, Vec2, DVec3, Color32, &'static str)> = AXIS_DIRECTIONS
            .iter()
            .map(|(direction, color, label)| {
                let toward = (-direction.dot(forward)) as f32;
                let offset = Vec2::new(direction.dot(right) as f32, -(direction.dot(up) as f32)) * RADIUS;
                (toward, offset, *direction, *color, *label)
            })
            .collect();
        entries.sort_by(|first, second| first.0.total_cmp(&second.0));

        for (toward, offset, direction, color, label) in entries {
            let front = (0.5 * (toward + 1.0)).clamp(0.0, 1.0);
            let tone = 0.45 + 0.55 * front;
            let position = center + offset;
            let ball = color.gamma_multiply(tone);
            let positive = has_connector(direction);
            if positive {
                painter.line_segment([center, position], Stroke::new(1.5, ball.gamma_multiply(0.8)));
                painter.circle_filled(position, DOT_RADIUS, ball);
            } else {
                painter.circle_stroke(position, DOT_RADIUS, Stroke::new(1.0, ball));
            }

            let hit = Rect::from_center_size(position, Vec2::splat(HIT_SIZE));
            let response = ui.interact(hit, ui.scope_id().with(("orientation_axis", label)), Sense::click());
            let hovered = response.hovered();
            if positive || hovered {
                let text_color = if hovered { Color32::WHITE } else { Color32::from_gray(235).gamma_multiply(tone) };
                painter.text(position, Align2::CENTER_CENTER, label, FontId::proportional(8.5), text_color);
            }
            if response.clicked() {
                action = Some(GizmoAction::LookFrom(snap_direction(direction, camera)));
            }
        }

        // The accent handle marks the roll the view is actually drawing with.
        let handle = ring_point(center, camera.roll().to_degrees());
        painter.circle_filled(handle, HANDLE_RADIUS, theme::accent(ui));
        painter.circle_stroke(handle, HANDLE_RADIUS, Stroke::new(1.0, Color32::from_gray(24)));

        if home_button(ui, home_rect(rect)) {
            action = Some(GizmoAction::Home);
        }

        // The projection button is the third entry point to the one toggle path: the icon draws the
        // mode the view is in, and the hint names the switch. A click belongs to the view this widget
        // belongs to, like the axis balls.
        let (icon, hint) = crate::view_cluster::projection_button(camera.projection());
        if icons::flat_icon_button_at(
            ui,
            projection_rect(rect),
            ui.scope_id().with("orientation_projection"),
            icon,
            BUTTON_ICON_POINT_SIZE,
            hint,
        )
        .clicked()
        {
            action = Some(GizmoAction::ToggleProjection);
        }

        if readout {
            paint_readout(ui, rect, camera);
        }
        action
    }

    /// Registers the ring, draws its stroke, nodes and handle, and returns a roll action for this
    /// frame.
    ///
    /// Only a press whose origin lies on the annulus starts a drag; a press inside the cube or on
    /// the button row is left to the widgets above. The nodes are registered after the drag, so a
    /// click on a node sets the absolute roll instead of starting a hand turn.
    fn ring(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        rect: Rect,
        center: egui::Pos2,
        camera: Camera,
        step_degrees: u32,
    ) -> Option<GizmoAction> {
        let _ = ui.interact(rect, ui.scope_id().with("orientation_roll_ring"), Sense::click_and_drag());
        let (pointer, pressed_origin, primary_down) = ui
            .input(|input| (input.pointer.interact_pos(), input.pointer.press_origin(), input.pointer.primary_down()));

        if ui.input(|input| input.pointer.primary_pressed())
            && let Some(origin) = pressed_origin
            && ring_hit(origin, center)
            && node_at(origin, center).is_none()
            && !axis_ball_at(origin, center, camera)
        {
            self.drag = Some(RingDrag {
                last_angle: visual_angle(center, origin),
                accumulated_degrees: 0.0,
                applied_degrees: 0.0,
                step_degrees: step_degrees.clamp(1, 15),
            });
        }
        if !primary_down {
            self.drag = None;
        }

        let mut action = None;
        if primary_down
            && let Some(drag) = &mut self.drag
            && let Some(pointer) = pointer
        {
            let angle = visual_angle(center, pointer);
            let delta = wrap_delta(angle - drag.last_angle).to_degrees();
            drag.last_angle = angle;
            if delta.is_finite() {
                drag.accumulated_degrees += delta;
                let quantized = quantize(drag.accumulated_degrees, drag.step_degrees);
                let step = quantized - drag.applied_degrees;
                if step != 0.0 {
                    drag.applied_degrees = quantized;
                    action = Some(GizmoAction::RollBy(degrees(step)));
                }
            }
        }

        if self.drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if pointer.is_some_and(|pointer| ring_hit(pointer, center)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        let ring_color = theme::text_dim(ui).gamma_multiply(0.55);
        painter.circle_stroke(center, RING_RADIUS, Stroke::new(RING_STROKE, ring_color));
        // The top node's tick makes "up" readable without a label.
        painter.line_segment(
            [egui::pos2(center.x, center.y - RING_RADIUS - 7.0), egui::pos2(center.x, center.y - RING_RADIUS - 3.0)],
            Stroke::new(RING_STROKE, ring_color),
        );

        for degree in NODE_DEGREES {
            let position = ring_point(center, degree as f64);
            let hit = Rect::from_center_size(position, Vec2::splat(2.0 * NODE_HIT));
            let response = ui.interact(hit, ui.scope_id().with(("orientation_roll_node", degree)), Sense::click());
            let color = if response.hovered() { theme::accent(ui) } else { theme::text_dim(ui) };
            painter.circle_filled(position, NODE_RADIUS, color);
            if response.clicked() {
                action = Some(GizmoAction::RollTo(Degrees::from_millidegrees(degree * 1_000)));
            }
            let _ = response.on_hover_text(format!("Roll to {degree}°"));
        }

        action
    }
}

/// The widget rectangle in the top-right corner of `viewport`, if the viewport is large enough.
///
/// The rectangle covers the cube, the home button in its corner and the projection button under it,
/// because all three take clicks; the readout is a passive label painted below it and is
/// deliberately outside, so it neither blocks a click on the scene nor widens the area that stops
/// the transform gizmo.
pub(crate) fn widget_rect(viewport: Rect) -> Option<Rect> {
    let height = SIZE + GAP + BUTTON_SIZE;
    if viewport.width() < SIZE + 2.0 * MARGIN || viewport.height() < height + 2.0 * MARGIN {
        return None;
    }
    let min = egui::pos2(viewport.right() - SIZE - MARGIN, viewport.top() + MARGIN);
    Some(Rect::from_min_size(min, Vec2::new(SIZE, height)))
}

/// The home button's rectangle in the cube's top-right corner.
fn home_rect(widget: Rect) -> Rect {
    Rect::from_center_size(
        egui::pos2(widget.right() - HOME_SIZE / 2.0, widget.top() + HOME_SIZE / 2.0),
        Vec2::splat(HOME_SIZE),
    )
}

/// The projection button's rectangle, centred under the cube.
fn projection_rect(widget: Rect) -> Rect {
    Rect::from_center_size(
        egui::pos2(widget.center().x, widget.top() + SIZE + GAP + BUTTON_SIZE / 2.0),
        Vec2::splat(BUTTON_SIZE),
    )
}

/// The visual angle of `pointer` about `center`, counter-clockwise from the top, in radians.
fn visual_angle(center: egui::Pos2, pointer: egui::Pos2) -> f64 {
    let dx = f64::from(pointer.x - center.x);
    let dy = f64::from(pointer.y - center.y);
    (-dx).atan2(-dy).rem_euclid(TAU)
}

/// The point on the ring at `degrees`, counter-clockwise from the top.
fn ring_point(center: egui::Pos2, degrees: f64) -> egui::Pos2 {
    let radians = degrees.to_radians();
    egui::pos2(center.x - RING_RADIUS * radians.sin() as f32, center.y - RING_RADIUS * radians.cos() as f32)
}

/// Whether `pointer` lies in the annulus that starts a hand turn.
fn ring_hit(pointer: egui::Pos2, center: egui::Pos2) -> bool {
    let distance = (pointer - center).length();
    (RING_RADIUS - RING_HIT..=RING_RADIUS + RING_HIT).contains(&distance)
}

/// The node the pointer is on, if any; its square target wins the press over a hand turn.
fn node_at(pointer: egui::Pos2, center: egui::Pos2) -> Option<i64> {
    NODE_DEGREES.into_iter().find(|degree| {
        let position = ring_point(center, *degree as f64);
        Rect::from_center_size(position, Vec2::splat(2.0 * NODE_HIT)).contains(pointer)
    })
}

/// Whether the pointer is inside one of the six axis-ball squares.
///
/// A ball keeps its own 22 pt target where it touches the annulus, so a press there snaps the view
/// instead of starting a roll.
fn axis_ball_at(pointer: egui::Pos2, center: egui::Pos2, camera: Camera) -> bool {
    AXIS_DIRECTIONS.iter().any(|(direction, _, _)| {
        let offset = Vec2::new(direction.dot(camera.right()) as f32, -(direction.dot(camera.up()) as f32)) * RADIUS;
        Rect::from_center_size(center + offset, Vec2::splat(HIT_SIZE)).contains(pointer)
    })
}

/// Wraps a pointer delta into `(-π, π]` so a turn across the seam does not jump.
fn wrap_delta(delta: f64) -> f64 {
    if !delta.is_finite() {
        return 0.0;
    }
    let wrapped = (delta + PI).rem_euclid(TAU) - PI;
    if wrapped <= -PI { wrapped + TAU } else { wrapped }
}

/// Rounds `total_degrees` to the nearest whole step, as the camera receives it.
fn quantize(total_degrees: f64, step_degrees: u32) -> f64 {
    if !total_degrees.is_finite() {
        return 0.0;
    }
    let step = f64::from(step_degrees.clamp(1, 15));
    (total_degrees / step).round() * step
}

/// Converts a float degree count to the integer angle the camera takes, rounding to a millidegree.
fn degrees(value: f64) -> Degrees {
    Degrees::from_radians(value.to_radians()).unwrap_or(Degrees::ZERO)
}

/// The home button in the cube's top-right corner; returns whether it was clicked.
fn home_button(ui: &Ui, rect: Rect) -> bool {
    let response = ui.interact(rect, ui.scope_id().with("orientation_home"), Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::RADIUS), visuals.weak_bg_fill);
    let glyph = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    icons::draw_icon(ui, icons::view::HOME, glyph, 14.0, visuals.fg_stroke.color);
    response.on_hover_text("Home  ·  back to the axonometric view").clicked()
}

/// Paints the one-line readout under the widget.
///
/// It is drawn outside [`widget_rect`], so it never blocks a click; the line is right-aligned to the
/// widget's right edge because the widget sits in the viewport's corner, so the text grows into the
/// scene instead of running under a side panel.
fn paint_readout(ui: &Ui, widget: Rect, camera: Camera) {
    let text = readout(camera);
    let color = theme::text_dim(ui);
    let galley = ui.painter().layout_no_wrap(text, FontId::proportional(9.5), color);
    let size = galley.size();
    let center_y = widget.bottom() + GAP + READOUT_HEIGHT / 2.0;
    let position = egui::pos2(widget.right() - size.x, center_y - size.y / 2.0);
    let clip = Rect::from_min_max(
        egui::pos2(position.x, center_y - READOUT_HEIGHT / 2.0),
        egui::pos2(widget.right(), center_y + READOUT_HEIGHT / 2.0),
    );
    ui.painter().with_clip_rect(clip).galley(position, galley, color);
}

/// The pitch, yaw, roll and projection line.
///
/// Pitch and yaw are the camera's own orbit angles, wrapped to a signed degree so the line reads as
/// -51.7° rather than 308.3°; the roll is the camera's field about its own view axis, presented the
/// same way, so a 270° roll reads as -90.0°. The projection is named the way the toggle names it,
/// and every number is formatted through `furnicraft-units` so the readout has one source of truth.
pub(crate) fn readout(camera: Camera) -> String {
    let pitch = Degrees::from_radians(camera.pitch()).unwrap_or(Degrees::ZERO);
    let yaw = signed_degrees(camera.yaw());
    let roll = signed_degrees(camera.roll());
    let projection = match camera.projection() {
        Projection::Orthographic { .. } => "Ortho",
        Projection::Perspective { .. } => "Persp",
    };
    format!(
        "Pitch {} · Yaw {} · Roll {} · {projection}",
        pitch.format_fixed(1),
        yaw.format_fixed(1),
        roll.format_fixed(1)
    )
}

/// Wraps a yaw in radians into `(-π, π]` and returns it as degrees.
fn signed_degrees(yaw: f64) -> Degrees {
    let half_turn = std::f64::consts::PI;
    let wrapped = (yaw.rem_euclid(2.0 * half_turn) + half_turn).rem_euclid(2.0 * half_turn) - half_turn;
    Degrees::from_radians(wrapped).unwrap_or(Degrees::ZERO)
}

/// The direction to look from when `direction` is clicked.
///
/// A click on the side the camera already looks from flips to the opposite side. A top or bottom
/// view sits exactly at the pole, where its axis ball is the eye direction; the pole check keeps
/// the flip when the pitch is a hair short of the limit.
fn snap_direction(direction: DVec3, camera: Camera) -> DVec3 {
    let eye = (camera.eye() - camera.target()).try_normalize().unwrap_or(DVec3::Z);
    let at_top = camera.pitch() >= MAX_PITCH - 1.0e-9;
    let at_bottom = camera.pitch() <= MIN_PITCH + 1.0e-9;
    let aligned = direction.dot(eye) > 0.999 || (direction.z > 0.5 && at_top) || (direction.z < -0.5 && at_bottom);
    if aligned { -direction } else { direction }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn the_widget_sits_in_the_corner_and_hides_in_a_small_viewport() {
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let rect = widget_rect(viewport).expect("the viewport is large enough");
        assert!((rect.right() - (viewport.right() - MARGIN)).abs() < 1.0e-6);
        assert!((rect.top() - (viewport.top() + MARGIN)).abs() < 1.0e-6);
        assert!((rect.width() - SIZE).abs() < 1.0e-6);
        assert!((rect.height() - (SIZE + GAP + BUTTON_SIZE)).abs() < 1.0e-6);
        assert!(rect.contains_rect(home_rect(rect)), "the home button is part of the clickable widget");
        assert!(rect.contains_rect(projection_rect(rect)), "and so is the projection button");

        let small = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(SIZE));
        assert!(widget_rect(small).is_none());
    }

    #[test]
    fn the_projection_button_sits_centred_below_the_cube() {
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let widget = widget_rect(viewport).expect("large enough");
        let cube = Rect::from_min_size(widget.min, Vec2::splat(SIZE));
        let button = projection_rect(widget);
        assert!((button.center().x - cube.center().x).abs() < 1.0e-6, "the button is centred under the cube");
        assert!(button.top() >= cube.bottom(), "the button starts below the cube: {button:?} vs {cube:?}");
        assert!(button.bottom() <= widget.bottom() + 1.0e-6, "the widget's height ends with the button");
        assert!((button.width() - BUTTON_SIZE).abs() < 1.0e-6);
        assert!(!cube.intersects(button), "the button never covers an axis ball's square");
    }

    #[test]
    fn clicking_the_projection_button_asks_for_the_toggle() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let pointer = projection_rect(widget_rect(viewport).expect("large enough")).center();

        let press = |pressed| egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        // A hover frame registers the widget, then a press and a release make the click.
        assert!(run_pointer_frame(&ctx, viewport, 0, vec![egui::Event::PointerMoved(pointer)]).is_none());
        assert!(run_pointer_frame(&ctx, viewport, 1, vec![press(true)]).is_none());
        assert_eq!(run_pointer_frame(&ctx, viewport, 2, vec![press(false)]), Some(GizmoAction::ToggleProjection));
    }

    #[test]
    fn a_click_away_from_the_projection_button_asks_for_nothing() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let pointer = viewport.center();
        let press = |pressed| egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = run_pointer_frame(&ctx, viewport, 0, vec![egui::Event::PointerMoved(pointer)]);
        let _ = run_pointer_frame(&ctx, viewport, 1, vec![press(true)]);
        assert!(run_pointer_frame(&ctx, viewport, 2, vec![press(false)]).is_none());
    }

    /// Runs one widget frame with `events`, returning the action a click asked for.
    fn run_pointer_frame(
        ctx: &egui::Context,
        viewport: Rect,
        frame: u32,
        events: Vec<egui::Event>,
    ) -> Option<GizmoAction> {
        let input = egui::RawInput {
            screen_rect: Some(viewport),
            time: Some(f64::from(frame) * 0.1),
            predicted_dt: 0.1,
            events,
            ..Default::default()
        };
        let mut action = None;
        let output = ctx.run_ui(input, |ui| {
            action = Gizmo::default().show(ui, viewport, Camera::default(), false, 1);
        });
        output.drop_without_applying_deltas();
        action
    }

    #[test]
    fn the_readout_names_the_angles_and_the_projection() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::new(0.6, 0.8, 0.5));
        assert!(matches!(camera.projection(), Projection::Perspective { .. }));
        let text = readout(camera);
        assert!(text.starts_with("Pitch "), "the line starts with the pitch: {text}");
        assert!(text.contains(" · Yaw "), "and continues with the yaw: {text}");
        assert!(text.contains(" · Roll "), "and the roll: {text}");
        assert!(text.ends_with("Persp"), "a perspective view names the mode: {text}");

        // Pitch, yaw and roll each carry one decimal place, exactly.
        let numbers: Vec<&str> = text.split(" · ").collect();
        assert_eq!(numbers.len(), 4, "four fields: {text}");
        for field in &numbers[..3] {
            let value = field.split_once(' ').map(|(_, value)| value).unwrap_or_default();
            assert!(value.ends_with('°'), "a field ends in a degree sign: {field}");
            assert_eq!(
                value.split('.').nth(1).map(|fraction| fraction.chars().count()),
                Some(2),
                "one decimal place and the degree sign: {field}"
            );
        }

        camera.toggle_projection();
        assert!(readout(camera).ends_with("Ortho"), "an orthographic view names the mode");

        camera.roll_to(std::f64::consts::FRAC_PI_2);
        assert!(readout(camera).contains("Roll 90.0°"), "the readout shows the camera's roll field");
    }

    #[test]
    fn the_readout_hides_when_its_switch_is_off() {
        // The label is painted only on request: the widget's own rectangle never covers it either
        // way, so a hidden readout leaves the scene exactly as it was.
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let widget = widget_rect(viewport).expect("large enough");
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let count = |readout: bool| {
            let mut texts = 0;
            let output = ctx.run_ui(Default::default(), |ui| {
                let action = Gizmo::default().show(ui, viewport, Camera::default(), readout, 1);
                assert!(action.is_none(), "drawing alone asks for nothing");
            });
            fn walk(shape: &egui::Shape, out: &mut usize) {
                match shape {
                    egui::Shape::Text(_) => *out += 1,
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            walk(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut texts);
            }
            output.drop_without_applying_deltas();
            texts
        };
        let without = count(false);
        let with = count(true);
        assert!(widget.width() <= SIZE + 0.5);
        assert_eq!(with, without + 1, "the switch adds exactly the one readout line: {with} vs {without}");
    }

    #[test]
    fn a_yaw_is_presented_as_a_signed_angle() {
        assert_eq!(signed_degrees(0.0), Degrees::ZERO);
        assert_eq!(signed_degrees(std::f64::consts::PI).format_fixed(1), "-180.0°", "a half turn wraps to -180°");
        assert_eq!(signed_degrees(-std::f64::consts::FRAC_PI_2).format_fixed(1), "-90.0°");
        // 308° is -52° once wrapped, which is what the user reads.
        let wrapped = signed_degrees(308.0_f64.to_radians());
        assert!((wrapped.degrees_f64() + 52.0).abs() < 1.0e-6, "wrapped to {wrapped}");
    }

    #[test]
    fn clicking_the_current_view_flips_to_the_opposite_side() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::X);
        assert_eq!(snap_direction(DVec3::X, camera), -DVec3::X);
        assert_eq!(snap_direction(-DVec3::X, camera), -DVec3::X);
        assert_eq!(snap_direction(DVec3::Y, camera), DVec3::Y);
        assert_eq!(snap_direction(DVec3::Z, camera), DVec3::Z);
    }

    #[test]
    fn clicking_a_pole_flips_to_the_opposite_side() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        assert_eq!(snap_direction(DVec3::Z, camera), -DVec3::Z);
        assert_eq!(snap_direction(-DVec3::Z, camera), -DVec3::Z);

        camera.look_from(-DVec3::Z);
        assert_eq!(snap_direction(-DVec3::Z, camera), DVec3::Z);
        assert_eq!(snap_direction(DVec3::Z, camera), DVec3::Z);

        // A side view is not the pole, even though the camera pitch is at its limit only at the pole.
        camera.look_from(DVec3::X);
        assert_eq!(snap_direction(DVec3::X, camera), -DVec3::X);
        assert_ne!(snap_direction(DVec3::Y, camera), -DVec3::Y);
    }

    #[test]
    fn has_connector_tests() {
        assert!(has_connector(DVec3::X));
        assert!(has_connector(DVec3::Y));
        assert!(has_connector(DVec3::Z));
        assert!(!has_connector(-DVec3::X));
        assert!(!has_connector(-DVec3::Y));
        assert!(!has_connector(-DVec3::Z));
        assert!(!has_connector(DVec3::ZERO));
    }

    /// The cube's centre inside `viewport`.
    fn widget_center(viewport: Rect) -> egui::Pos2 {
        let widget = widget_rect(viewport).expect("the viewport is large enough");
        Rect::from_min_size(widget.min, Vec2::splat(SIZE)).center()
    }

    /// One widget frame with `events`, keeping the interaction state in `gizmo`.
    fn widget_frame(
        ctx: &egui::Context,
        viewport: Rect,
        gizmo: &mut Gizmo,
        camera: Camera,
        step_degrees: u32,
        frame: u32,
        events: Vec<egui::Event>,
    ) -> Option<GizmoAction> {
        let input = egui::RawInput {
            screen_rect: Some(viewport),
            time: Some(f64::from(frame) * 0.1),
            predicted_dt: 0.1,
            events,
            ..Default::default()
        };
        let mut action = None;
        let output = ctx.run_ui(input, |ui| {
            action = gizmo.show(ui, viewport, camera, false, step_degrees);
        });
        output.drop_without_applying_deltas();
        action
    }

    fn press_event(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Drives a press at `from` degrees and a move to `to` degrees, releasing afterwards.
    fn hand_turn(
        ctx: &egui::Context,
        viewport: Rect,
        gizmo: &mut Gizmo,
        step_degrees: u32,
        from: f64,
        to: f64,
    ) -> Option<GizmoAction> {
        let center = widget_center(viewport);
        let start = ring_point(center, from);
        let end = ring_point(center, to);
        let camera = Camera::default();
        let opened =
            widget_frame(ctx, viewport, gizmo, camera, step_degrees, 0, vec![egui::Event::PointerMoved(start)]);
        assert!(opened.is_none(), "hovering asks for nothing");
        let pressed = widget_frame(ctx, viewport, gizmo, camera, step_degrees, 1, vec![press_event(start, true)]);
        assert!(pressed.is_none(), "the press alone asks for nothing");
        let action = widget_frame(ctx, viewport, gizmo, camera, step_degrees, 2, vec![egui::Event::PointerMoved(end)]);
        let _ = widget_frame(ctx, viewport, gizmo, camera, step_degrees, 3, vec![press_event(end, false)]);
        action
    }

    #[test]
    fn the_ring_nodes_are_quarter_turns_from_the_top() {
        let center = egui::pos2(0.0, 0.0);
        let expected = [
            egui::pos2(0.0, -RING_RADIUS),
            egui::pos2(-RING_RADIUS, 0.0),
            egui::pos2(0.0, RING_RADIUS),
            egui::pos2(RING_RADIUS, 0.0),
        ];
        for (degree, expected) in NODE_DEGREES.iter().zip(expected) {
            let position = ring_point(center, *degree as f64);
            assert!((position - expected).length() < 1.0e-4, "{degree}° at {position:?}, expected {expected:?}");
            let angle = visual_angle(center, position).to_degrees();
            assert!(
                (angle - *degree as f64).rem_euclid(360.0) < 1.0e-9,
                "the point at {degree}° reads back as {angle}°"
            );
        }
    }

    #[test]
    fn clicking_a_ring_node_sets_the_absolute_roll() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let center = widget_center(viewport);
        for (index, degree) in NODE_DEGREES.into_iter().enumerate() {
            let pointer = ring_point(center, degree as f64);
            let offset = index as u32 * 3;
            let step = |frame, events| run_pointer_frame(&ctx, viewport, offset + frame, events);
            let _ = step(0, vec![egui::Event::PointerMoved(pointer)]);
            let _ = step(1, vec![press_event(pointer, true)]);
            let action = step(2, vec![press_event(pointer, false)]);
            assert_eq!(
                action,
                Some(GizmoAction::RollTo(Degrees::from_millidegrees(degree * 1_000))),
                "the {degree}° node sets the absolute roll"
            );
        }
    }

    #[test]
    fn a_hand_turn_quantises_to_the_step() {
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let one = {
            let ctx = egui::Context::default();
            furnicraft_ui::theme::install(&ctx);
            let mut gizmo = Gizmo::default();
            let action = hand_turn(&ctx, viewport, &mut gizmo, 1, 10.0, 47.0);
            assert_eq!(action, Some(GizmoAction::RollBy(Degrees::from_millidegrees(37_000))));
            assert!(!gizmo.is_dragging(), "the release ends the drag");
            true
        };
        assert!(one);

        let five = {
            let ctx = egui::Context::default();
            furnicraft_ui::theme::install(&ctx);
            let mut gizmo = Gizmo::default();
            let action = hand_turn(&ctx, viewport, &mut gizmo, 5, 10.0, 47.0);
            assert_eq!(action, Some(GizmoAction::RollBy(Degrees::from_millidegrees(35_000))), "37° rounds to 35°");
            true
        };
        assert!(five);
    }

    #[test]
    fn a_sub_step_turn_applies_nothing_and_the_remainder_carries() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let center = widget_center(viewport);
        let camera = Camera::default();
        let mut gizmo = Gizmo::default();
        let start = ring_point(center, 10.0);
        let first = ring_point(center, 12.0);
        let second = ring_point(center, 16.0);

        let _ = widget_frame(&ctx, viewport, &mut gizmo, camera, 5, 0, vec![egui::Event::PointerMoved(start)]);
        let _ = widget_frame(&ctx, viewport, &mut gizmo, camera, 5, 1, vec![press_event(start, true)]);
        let small = widget_frame(&ctx, viewport, &mut gizmo, camera, 5, 2, vec![egui::Event::PointerMoved(first)]);
        assert!(small.is_none(), "2° is below the 5° step");
        assert!(gizmo.is_dragging(), "the drag still owns the pointer");
        let carried = widget_frame(&ctx, viewport, &mut gizmo, camera, 5, 3, vec![egui::Event::PointerMoved(second)]);
        assert_eq!(
            carried,
            Some(GizmoAction::RollBy(Degrees::from_millidegrees(5_000))),
            "6° of accumulated turn applies exactly one step"
        );
    }

    #[test]
    fn a_turn_across_the_seam_does_not_jump() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut gizmo = Gizmo::default();
        let action = hand_turn(&ctx, viewport, &mut gizmo, 1, 170.0, 190.0);
        assert_eq!(
            action,
            Some(GizmoAction::RollBy(Degrees::from_millidegrees(20_000))),
            "170° to 190° is +20°, not -340°"
        );
    }

    #[test]
    fn a_drag_that_starts_on_a_node_does_not_turn_the_ring() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let center = widget_center(viewport);
        let camera = Camera::default();
        let node = ring_point(center, 0.0);
        let elsewhere = ring_point(center, 40.0);
        let mut gizmo = Gizmo::default();
        let _ = widget_frame(&ctx, viewport, &mut gizmo, camera, 1, 0, vec![egui::Event::PointerMoved(node)]);
        let _ = widget_frame(&ctx, viewport, &mut gizmo, camera, 1, 1, vec![press_event(node, true)]);
        let action = widget_frame(&ctx, viewport, &mut gizmo, camera, 1, 2, vec![egui::Event::PointerMoved(elsewhere)]);
        assert!(action.is_none(), "the node owns the press, so the ring never starts a drag");
        assert!(!gizmo.is_dragging());
    }

    #[test]
    fn the_ring_hit_is_the_annulus_and_skips_the_axis_balls() {
        let center = egui::pos2(100.0, 100.0);
        assert!(ring_hit(center + Vec2::new(0.0, -RING_RADIUS), center), "the top of the annulus starts a turn");
        assert!(ring_hit(center + Vec2::new(RING_RADIUS, 0.0), center), "and so does the right");
        assert!(!ring_hit(center, center), "the middle of the cube is not the ring");
        assert!(!ring_hit(center + Vec2::new(0.0, -RADIUS), center), "an axis ball keeps its own hit");
        assert!(!ring_hit(center + Vec2::new(0.0, -RING_RADIUS - RING_HIT - 1.0), center), "outside the annulus");
    }

    #[test]
    fn the_handle_sits_at_the_camera_roll() {
        let center = egui::pos2(0.0, 0.0);
        let mut camera = Camera::default();
        camera.roll_to(std::f64::consts::FRAC_PI_2);
        let handle = ring_point(center, camera.roll().to_degrees());
        let expected = ring_point(center, 90.0);
        assert!((handle - expected).length() < 1.0e-4, "a +90° roll puts the handle on the left node");
    }

    #[test]
    fn a_wrap_and_a_quantise_survive_hostile_input() {
        assert!((wrap_delta(std::f64::consts::PI) - std::f64::consts::PI).abs() < 1.0e-12);
        assert!((wrap_delta(-std::f64::consts::PI) - std::f64::consts::PI).abs() < 1.0e-12);
        assert!(wrap_delta(f64::NAN).is_finite());
        assert!(wrap_delta(f64::INFINITY).is_finite());
        assert_eq!(quantize(37.0, 1), 37.0);
        assert_eq!(quantize(37.0, 5), 35.0);
        assert_eq!(quantize(-3.0, 5), -5.0);
        assert_eq!(quantize(f64::NAN, 5), 0.0);
        assert_eq!(quantize(10.0, 0), 10.0, "a zero step is read as one degree");
    }
}
