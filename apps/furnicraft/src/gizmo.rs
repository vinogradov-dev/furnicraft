//! The corner orientation widget.
//!
//! Six world-axis balls are projected onto the viewport with the camera basis, so the widget reads
//! like the axes in the scene instead of a fixed diagram. Clicking one snaps the camera to look at
//! its target from that direction; clicking the direction the camera already looks from flips to
//! the opposite side, so the centre ball remains useful in a top or bottom view. A *home* button
//! under the cube returns the view to *Axonometric*, and an optional one-line readout under it names
//! the pitch, the yaw, the roll and the projection the viewport is drawing with.
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
pub(crate) const SIZE: f32 = 104.0;

/// Distance from the viewport corner, in points.
const MARGIN: f32 = 12.0;

/// Radius of the circle the axis balls sit on, in points.
const RADIUS: f32 = 38.0;

/// Radius of an axis ball, in points.
const DOT_RADIUS: f32 = 8.5;

/// Side of the square click target around a ball, in points.
const HIT_SIZE: f32 = 22.0;

/// Side of the home button under the cube, in points.
const HOME_SIZE: f32 = 22.0;

/// Gap between the cube and the home button, in points.
const GAP: f32 = 2.0;

/// Height reserved for one line of the readout, in points.
const READOUT_HEIGHT: f32 = 14.0;

// Gamma-space colours, matching the default grid axis colours in `furnicraft_scene`.
const X_COLOR: Color32 = Color32::from_rgb(184, 71, 71);
const Y_COLOR: Color32 = Color32::from_rgb(71, 158, 87);
const Z_COLOR: Color32 = Color32::from_rgb(77, 115, 209);

/// What a click on the widget asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GizmoAction {
    /// Look at the target from this eye direction.
    LookFrom(DVec3),
    /// Return the view to the axonometric preset.
    Home,
}

/// The widget rectangle in the top-right corner of `viewport`, if the viewport is large enough.
///
/// The rectangle covers the cube and the home button, because both take clicks; the readout is a
/// passive label painted below it and is deliberately outside, so it neither blocks a click on the
/// scene nor widens the area that stops the transform gizmo.
pub(crate) fn widget_rect(viewport: Rect) -> Option<Rect> {
    let width = SIZE + GAP + HOME_SIZE;
    let height = SIZE;
    if viewport.width() < width + 2.0 * MARGIN || viewport.height() < height + 2.0 * MARGIN {
        return None;
    }
    let min = egui::pos2(viewport.right() - SIZE - MARGIN, viewport.top() + MARGIN);
    Some(Rect::from_min_size(min, Vec2::new(SIZE, height)))
}

/// The home button's rectangle inside the widget.
fn home_rect(widget: Rect) -> Rect {
    Rect::from_center_size(
        egui::pos2(widget.right() - HOME_SIZE / 2.0, widget.top() + HOME_SIZE / 2.0),
        Vec2::splat(HOME_SIZE),
    )
}

/// Draws the widget and returns the one action a click asked for.
///
/// `readout` turns the line under the home button on; the numbers are read from the same camera the
/// viewport draws with, so the label cannot disagree with the image.
pub(crate) fn show(ui: &Ui, viewport: Rect, camera: Camera, readout: bool) -> Option<GizmoAction> {
    let rect = widget_rect(viewport)?;
    let cube = Rect::from_min_size(rect.min, Vec2::splat(SIZE));
    let painter = ui.painter().with_clip_rect(rect);
    let mut action = None;

    let center = cube.center();
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();

    let pointer_over_widget = ui.input(|input| input.pointer.hover_pos().is_some_and(|pointer| rect.contains(pointer)));
    if pointer_over_widget {
        painter.circle_stroke(
            center,
            SIZE * 0.48,
            Stroke::new(1.5, Color32::from_rgba_unmultiplied(220, 224, 232, 140)),
        );
    }

    let directions = [
        (DVec3::X, X_COLOR, "X"),
        (-DVec3::X, X_COLOR, "-X"),
        (DVec3::Y, Y_COLOR, "Y"),
        (-DVec3::Y, Y_COLOR, "-Y"),
        (DVec3::Z, Z_COLOR, "Z"),
        (-DVec3::Z, Z_COLOR, "-Z"),
    ];
    let mut entries: Vec<(f32, Vec2, DVec3, Color32, &'static str)> = directions
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

    if home_button(ui, home_rect(rect)) {
        action = Some(GizmoAction::Home);
    }

    if readout {
        paint_readout(ui, rect, camera);
    }
    action
}

/// The home button under the cube; returns whether it was clicked.
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
/// -51.7° rather than 308.3°; the roll is the tilt of the camera's right vector above the horizon,
/// which an orbit camera always keeps at zero. The projection is named the way the toggle names it,
/// and every number is formatted through `furnicraft-units` so the readout has one source of truth.
pub(crate) fn readout(camera: Camera) -> String {
    let pitch = Degrees::from_radians(camera.pitch()).unwrap_or(Degrees::ZERO);
    let yaw = signed_degrees(camera.yaw());
    let roll = Degrees::from_radians(camera.right().z.clamp(-1.0, 1.0).asin()).unwrap_or(Degrees::ZERO);
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
        assert!(rect.contains_rect(home_rect(rect)), "the home button is part of the clickable widget");

        let small = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(SIZE));
        assert!(widget_rect(small).is_none());
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
                let action = show(ui, viewport, Camera::default(), readout);
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
}
