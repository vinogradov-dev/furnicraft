//! The UI scale control of the settings window.
//!
//! egui computes the pixel density as the monitor's native density times its zoom factor, so a
//! change here makes every widget and font larger without shrinking the viewport's own image. It
//! also makes the icon loader rasterise a 16-point drawing at the new pixel size instead of
//! stretching the texture it uploaded at the old one (`assets/README.md`, ADR 0047).
//!
//! A scale control cannot read where the pointer is, the way a slider does: changing the scale
//! re-lays out the whole window, so the control moves under the pointer while the drag runs. The
//! field here reads only the horizontal delta of the drag (`crate::scrub`).

use egui::{Response, Ui};
use furnicraft_text::Message;

use crate::scrub::Scrub;

/// The smallest UI scale the control offers, in percent.
pub const MIN_PERCENT: u32 = 75;

/// The largest UI scale the control offers, in percent.
pub const MAX_PERCENT: u32 = 200;

/// The scale used while egui reports no meaningful zoom factor.
pub const DEFAULT_PERCENT: u32 = 100;

/// The scales the control offers as one click, in percent.
pub const PRESETS: [u32; 6] = [75, 100, 125, 150, 175, 200];

/// The egui zoom factor as a whole percentage inside [`MIN_PERCENT`]..=[`MAX_PERCENT`].
///
/// A factor outside the offered range comes back as the nearest end, so the field never shows a
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

/// Draws the scale field and its presets, and applies a change to `ui.ctx().set_zoom_factor`.
///
/// `percent` is the value the workspace stores: it is read from the window's zoom every frame, so a
/// zoom made with egui's keyboard shortcuts appears in the field too, and it is written back with
/// the percentage this control applied, so the stored scale is never a step behind the window.
/// Reading the window also keeps the control stateless: it owns no field and survives a panel
/// rebuild without restoring a stale value.
pub fn show(ui: &mut Ui, percent: &mut u32) -> Response {
    let lang = crate::language(ui.ctx());
    let stored = *percent;
    let live = self::percent(ui.ctx().zoom_factor());
    let mut chosen = live;
    let mut response = ui
        .horizontal(|ui| {
            ui.label(Message::UiScale.text(lang));
            ui.add(Scrub::new(&mut chosen).speed(0.5).range(MIN_PERCENT..=MAX_PERCENT).suffix(" %"))
        })
        .inner;
    ui.horizontal(|ui| {
        for preset in PRESETS {
            if ui.selectable_label(chosen == preset, format!("{preset} %")).clicked() {
                chosen = preset;
            }
        }
    });
    if chosen != live {
        ui.ctx().set_zoom_factor(factor(chosen));
    }
    *percent = chosen;
    if chosen != stored {
        response.mark_changed();
    }
    response.on_hover_text(Message::UiScaleHint.text(lang))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use egui::epaint::{ClippedShape, Shape};
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2, pos2, vec2};

    use super::*;

    const SCREEN: Vec2 = vec2(800.0, 600.0);

    fn input(events: Vec<Event>) -> RawInput {
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), events, ..Default::default() }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Collects the text of every text shape and the rectangle it was drawn in, nested ones too.
    fn drawn_text(shapes: &[ClippedShape]) -> Vec<(String, Rect)> {
        fn walk(shape: &Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                Shape::Text(text) => {
                    out.push((text.galley.text().to_owned(), Rect::from_min_size(text.pos, text.galley.size())));
                }
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

    /// Draws the control for one frame, returning the shapes it drew and the field's rectangle.
    fn frame(ctx: &egui::Context, events: Vec<Event>) -> (Vec<ClippedShape>, Rect) {
        let mut percent = DEFAULT_PERCENT;
        let mut field = Rect::ZERO;
        let output = ctx.run_ui(input(events), |ui| {
            field = show(ui, &mut percent).rect;
        });
        let shapes = output.shapes.clone();
        output.drop_without_applying_deltas();
        (shapes, field)
    }

    /// The rectangle the text `label` was drawn in on a quiet frame.
    fn label_rect(ctx: &egui::Context, label: &str) -> Rect {
        let (shapes, _) = frame(ctx, vec![]);
        drawn_text(&shapes)
            .into_iter()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| rect)
            .expect("the label is drawn")
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
    fn the_control_shows_the_current_scale_and_every_preset() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.zoom_factor = 1.25);
        let (shapes, _) = frame(&ctx, vec![]);

        let text: Vec<String> = drawn_text(&shapes).into_iter().map(|(text, _)| text).collect();
        let text = text.join(" ");
        assert!(text.contains("UI scale"), "{text}");
        assert!(text.contains("125 %"), "{text}");
        for preset in PRESETS {
            assert!(text.contains(&format!("{preset} %")), "the {preset} % preset is missing: {text}");
        }
    }

    #[test]
    fn a_preset_button_sets_its_exact_percent() {
        let ctx = egui::Context::default();
        let centre = label_rect(&ctx, "150 %").center();
        // Hover, press, release, and one more frame: a zoom factor becomes active at the start of
        // the pass after the one that set it.
        for events in
            [vec![Event::PointerMoved(centre)], vec![button(centre, true)], vec![button(centre, false)], vec![]]
        {
            frame(&ctx, events);
        }
        assert_eq!(ctx.zoom_factor(), 1.5, "a preset applies its own percentage");
        assert_eq!(percent(ctx.zoom_factor()), 150);
    }

    #[test]
    fn a_drag_on_the_field_changes_the_scale_at_half_a_percent_per_point() {
        let ctx = egui::Context::default();
        let (_, field) = frame(&ctx, vec![]);
        let from = pos2(field.left() + 10.0, field.center().y);
        let to = pos2(from.x + 40.0, from.y);
        for events in [
            vec![Event::PointerMoved(from)],
            vec![button(from, true)],
            vec![Event::PointerMoved(to)],
            vec![button(to, false)],
            vec![],
        ] {
            frame(&ctx, events);
        }
        assert_eq!(ctx.zoom_factor(), 1.2, "forty points of drag is twenty percent");
    }

    #[test]
    fn the_control_reports_the_percentage_it_applied() {
        let ctx = egui::Context::default();
        let mut percent = DEFAULT_PERCENT;
        let centre = label_rect(&ctx, "175 %").center();
        let mut changed = false;
        for events in
            [vec![Event::PointerMoved(centre)], vec![button(centre, true)], vec![button(centre, false)], vec![]]
        {
            let output = ctx.run_ui(input(events), |ui| {
                changed |= show(ui, &mut percent).changed();
            });
            output.drop_without_applying_deltas();
        }
        assert!(changed, "the control says the stored scale changed");
        assert_eq!(percent, 175, "and names the scale it applied, not the one the window still shows");
    }

    /// The window in points, so a dense monitor holds the same layout in twice the pixels.
    const WINDOW_POINTS: Vec2 = vec2(1280.0, 840.0);

    /// One frame the way the window hands it over: the integration reports the monitor's native
    /// density and converts every physical position to points with the scale in force, so a zoom
    /// change moves the point coordinates under a pointer that has not moved.
    fn physical_input(_ctx: &egui::Context, native: f32, events: Vec<Event>) -> RawInput {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { native_pixels_per_point: Some(native), ..Default::default() },
        );
        RawInput {
            viewports,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW_POINTS)),
            events,
            ..Default::default()
        }
    }

    /// Drags the control to the right the way the window does, `steps` times by `motion` physical
    /// pixels, and returns the percentage it showed on every frame.
    ///
    /// `native` is the monitor's density, so the same hand travel is `native` times as many pixels;
    /// `raw` says whether the platform reports the pointer's own motion (`Event::MouseMoved`) or
    /// only its place.
    fn window_drag(native: f32, motion: Vec2, raw: bool) -> Vec<u32> {
        // The control sits where the Settings window puts it, well away from the corner of the
        // window: that distance is what a position-driven drag multiplies by the scale change.
        let row = Rect::from_min_size(pos2(700.0, 450.0), vec2(500.0, 160.0));
        let ctx = egui::Context::default();
        let mut percent = DEFAULT_PERCENT;
        let mut seen = Vec::new();
        let mut draw = |events: Vec<Event>| -> Rect {
            let mut field = Rect::ZERO;
            let output = ctx.run_ui(physical_input(&ctx, native, events), |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
                    field = show(ui, &mut percent).rect;
                });
            });
            output.drop_without_applying_deltas();
            seen.push(percent);
            field
        };
        // Every position is converted with the scale in force at that moment, as the window
        // backend does it, so a zoom change moves the point coordinates.
        let place = |pointer: Pos2| pointer / ctx.pixels_per_point();
        let field = draw(vec![]);
        let mut pointer = field.center() * ctx.pixels_per_point();
        draw(vec![Event::PointerMoved(place(pointer))]);
        let button = |pointer: Pos2, pressed: bool| Event::PointerButton {
            pos: place(pointer),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        draw(vec![button(pointer, true)]);
        for _ in 0..10 {
            pointer += motion;
            let mut events = vec![Event::PointerMoved(place(pointer))];
            if raw {
                events.push(Event::MouseMoved(motion));
            }
            draw(events);
        }
        // The hand stops, and the window may still be settling under the new scale.
        for _ in 0..3 {
            draw(vec![Event::PointerMoved(place(pointer))]);
        }
        seen
    }

    /// Whether the control only ever climbed.
    fn climbed(seen: &[u32]) -> bool {
        seen.windows(2).all(|pair| pair[1] >= pair[0])
    }

    #[test]
    fn a_drag_in_the_window_climbs_and_stops_with_the_hand() {
        // Two pixels a frame for ten frames, at half a percent a pixel: ten percent.
        let seen = window_drag(1.0, vec2(2.0, 0.0), true);
        assert_eq!(seen.last().copied(), Some(110), "{seen:?}");
        assert!(climbed(&seen), "the scale never falls back while the hand moves right: {seen:?}");
    }

    #[test]
    fn a_platform_that_reports_only_the_pointer_place_climbs_the_same_way() {
        let seen = window_drag(1.0, vec2(2.0, 0.0), false);
        assert_eq!(seen.last().copied(), Some(110), "{seen:?}");
        assert!(climbed(&seen), "a window that rescales under the pointer cannot move the number: {seen:?}");
    }

    #[test]
    fn a_platform_without_raw_motion_measures_the_pixels_the_screen_has() {
        // The same physical travel on a 2x monitor; the two points of hand travel are twice the
        // pixels, and the place-based path counts pixels.
        let seen = window_drag(2.0, vec2(2.0, 0.0), false);
        assert_eq!(seen.last().copied(), Some(110), "{seen:?}");
        assert!(climbed(&seen), "{seen:?}");
    }

    #[test]
    fn a_typed_scale_outside_the_offered_range_is_clamped() {
        let ctx = egui::Context::default();
        let (_, field) = frame(&ctx, vec![]);
        let centre = field.center();
        let enter = Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        for events in [
            vec![Event::PointerMoved(centre)],
            vec![button(centre, true)],
            vec![button(centre, false)],
            // The field starts editing a frame later, with its number selected.
            vec![],
            vec![Event::Text("900".to_owned())],
            vec![enter],
            vec![],
            vec![],
        ] {
            frame(&ctx, events);
        }
        assert_eq!(percent(ctx.zoom_factor()), MAX_PERCENT, "a typed value past the top stops at the top");
        assert_eq!(ctx.zoom_factor(), factor(MAX_PERCENT));
    }
}
