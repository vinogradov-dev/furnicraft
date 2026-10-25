//! The corner orientation widget.
//!
//! Six world-axis balls are projected onto the viewport with the camera basis, so the widget reads
//! like the axes in the scene instead of a fixed diagram. Clicking one snaps the camera to look at
//! its target from that direction; clicking the direction the camera already looks from flips to
//! the opposite side, so the centre ball remains useful in a top or bottom view.

use eframe::egui;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use furnicraft_geom::DVec3;
use furnicraft_scene::{Camera, MAX_PITCH, MIN_PITCH};

/// Width and height of the widget, in points.
pub(crate) const SIZE: f32 = 104.0;

/// Distance from the viewport corner, in points.
const MARGIN: f32 = 12.0;

/// Radius of the circle the axis balls sit on, in points.
const RADIUS: f32 = 38.0;

/// Radius of an axis ball, in points.
const DOT_RADIUS: f32 = 8.5;

/// Side of the square click target around a ball, in points.
const HIT_SIZE: f32 = 22.0;

// Gamma-space colours, matching the default grid axis colours in `furnicraft_scene`.
const X_COLOR: Color32 = Color32::from_rgb(184, 71, 71);
const Y_COLOR: Color32 = Color32::from_rgb(71, 158, 87);
const Z_COLOR: Color32 = Color32::from_rgb(77, 115, 209);

/// The widget rectangle in the top-right corner of `viewport`, if the viewport is large enough.
pub(crate) fn widget_rect(viewport: Rect) -> Option<Rect> {
    if viewport.width() < SIZE + 2.0 * MARGIN || viewport.height() < SIZE + 2.0 * MARGIN {
        return None;
    }
    let min = egui::pos2(viewport.right() - SIZE - MARGIN, viewport.top() + MARGIN);
    Some(Rect::from_min_size(min, Vec2::splat(SIZE)))
}

/// Draws the widget and returns the eye direction to snap to when a ball is clicked.
pub(crate) fn show(ui: &Ui, viewport: Rect, camera: Camera) -> Option<DVec3> {
    let rect = widget_rect(viewport)?;
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 10.0, Color32::from_rgba_unmultiplied(18, 20, 24, 170));
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0, Color32::from_gray(64)), StrokeKind::Inside);

    let center = rect.center();
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();

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

    let mut snapped = None;
    for (toward, offset, direction, color, label) in entries {
        let front = (0.5 * (toward + 1.0)).clamp(0.0, 1.0);
        let tone = 0.45 + 0.55 * front;
        let position = center + offset;
        let ball = color.gamma_multiply(tone);
        painter.line_segment([center, position], Stroke::new(1.5, ball.gamma_multiply(0.8)));
        painter.circle_filled(position, DOT_RADIUS, ball);

        let hit = Rect::from_center_size(position, Vec2::splat(HIT_SIZE));
        let response = ui.interact(hit, ui.scope_id().with(("orientation_axis", label)), Sense::click());
        let text_color = if response.hovered() { Color32::WHITE } else { Color32::from_gray(235).gamma_multiply(tone) };
        painter.text(position, Align2::CENTER_CENTER, label, FontId::proportional(8.5), text_color);
        if response.clicked() {
            snapped = Some(snap_direction(direction, camera));
        }
    }
    snapped
}

/// The direction to look from when `direction` is clicked.
///
/// A click on the side the camera already looks from flips to the opposite side. Top and bottom
/// views stop at the pitch limit, five degrees short of the pole, so their axis balls never align
/// exactly with the eye; the clamped pitch is treated as aligned.
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

        let small = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(SIZE));
        assert!(widget_rect(small).is_none());
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
    fn clicking_the_pole_flips_even_though_the_pitch_stops_short_of_it() {
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
}
