//! The UI scale control in the viewport strip.
//!
//! egui computes the pixel density as the monitor's native density times its zoom factor, so a
//! change here makes every widget and font larger without shrinking the viewport's own image. It
//! also makes the icon loader rasterise a 16-point drawing at the new pixel size instead of
//! stretching the texture it uploaded at the old one (`assets/README.md`, ADR 0047).

use egui::{Response, Slider, Ui};

/// The smallest UI scale the control offers, in percent.
pub const MIN_PERCENT: u32 = 75;

/// The largest UI scale the control offers, in percent.
pub const MAX_PERCENT: u32 = 200;

/// The scale used while egui reports no meaningful zoom factor.
pub const DEFAULT_PERCENT: u32 = 100;

/// The egui zoom factor as a whole percentage inside [`MIN_PERCENT`]..=[`MAX_PERCENT`].
///
/// A factor outside the offered range comes back as the nearest end, so the slider never shows a
/// value it could not have set. A non-finite factor is not a scale at all and falls back to
/// [`DEFAULT_PERCENT`].
pub fn percent(zoom_factor: f32) -> u32 {
    if !zoom_factor.is_finite() {
        return DEFAULT_PERCENT;
    }
    let percent = (zoom_factor * 100.0).round();
    if percent < MIN_PERCENT as f32 {
        MIN_PERCENT
    } else if percent > MAX_PERCENT as f32 {
        MAX_PERCENT
    } else {
        // The finite value is inside the integer range checked above.
        percent as u32
    }
}

/// The egui zoom factor for a whole percentage, clamped to the offered range.
pub fn factor(percent: u32) -> f32 {
    percent.clamp(MIN_PERCENT, MAX_PERCENT) as f32 / 100.0
}

/// Draws the scale slider and applies a change to `ui.ctx().set_zoom_factor`.
///
/// The current value is read from the context every frame, so a zoom made with egui's keyboard
/// shortcuts appears in the slider too. Reading the context also keeps the control stateless: it
/// owns no field and survives a panel rebuild without restoring a stale value.
pub fn show(ui: &mut Ui) -> Response {
    let mut percent = percent(ui.ctx().zoom_factor());
    let response =
        ui.add(Slider::new(&mut percent, MIN_PERCENT..=MAX_PERCENT).step_by(5.0).suffix(" %").text("UI scale"));
    if response.changed() {
        ui.ctx().set_zoom_factor(factor(percent));
    }
    response.on_hover_text("Icons are rasterised at the new pixel size instead of stretched")
}

#[cfg(test)]
mod tests {
    use egui::epaint::{ClippedShape, Shape};

    use super::*;

    /// Collects the text of every text shape, including nested ones.
    fn drawn_text(shapes: &[ClippedShape]) -> Vec<String> {
        fn walk(shape: &Shape, out: &mut Vec<String>) {
            match shape {
                Shape::Text(text) => out.push(text.galley.text().to_owned()),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn percent_rounds_and_clamps_the_zoom_factor() {
        let cases = [
            (1.0, 100),
            (0.75, MIN_PERCENT),
            (1.25, 125),
            (2.0, MAX_PERCENT),
            (0.1, MIN_PERCENT),
            (4.0, MAX_PERCENT),
            (f32::NAN, DEFAULT_PERCENT),
            (f32::INFINITY, DEFAULT_PERCENT),
            (f32::NEG_INFINITY, DEFAULT_PERCENT),
        ];
        for (zoom_factor, expected) in cases {
            assert_eq!(percent(zoom_factor), expected, "zoom factor {zoom_factor}");
        }
    }

    #[test]
    fn factor_is_the_percentage_as_a_zoom_factor() {
        assert_eq!(factor(75), 0.75);
        assert_eq!(factor(100), 1.0);
        assert_eq!(factor(125), 1.25);
        assert_eq!(factor(200), 2.0);
    }

    #[test]
    fn factor_clamps_to_the_offered_range() {
        assert_eq!(factor(0), 0.75);
        assert_eq!(factor(10), 0.75);
        assert_eq!(factor(500), 2.0);
    }

    #[test]
    fn the_control_shows_the_current_scale() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.zoom_factor = 1.25);
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui);
        });

        let text = drawn_text(&output.shapes).join(" ");
        output.drop_without_applying_deltas();
        assert!(text.contains("UI scale"), "{text}");
        assert!(text.contains("125"), "{text}");
    }
}
