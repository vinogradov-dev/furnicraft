//! A numeric field that scrubs on a horizontal drag.
//!
//! The field changes its value while the primary button is held and the pointer moves left or right,
//! and it measures that movement in the pointer's own travel, never in where it is on screen: a
//! widget that grows or shrinks under the pointer, and a window that rescales under it, cannot move
//! the number. That is the difference from egui's slider, which reads the pointer's place over the
//! widget. The UI scale is the worst case, because changing it re-lays out every widget while the
//! drag runs, so the same physical pointer position is a different number of points every pass —
//! a place-based control reads that rescaling as travel and runs away.
//!
//! The pointer is hidden for the duration of the drag — the number, not the pointer, is what the
//! user is steering. A click enters the value as text; the typed number is parsed, clamped to the
//! offered range and written back on Enter or when the field loses focus.

use core::ops::RangeInclusive;

use egui::emath::Numeric;
use egui::text::CCursorRange;
use egui::{Button, CursorIcon, Id, Key, Response, RichText, Sense, TextEdit, Ui, Widget};

/// The drag in flight: the value the button went down on, how far the pointer has travelled since,
/// and the place the last frame measured from.
#[derive(Clone, Copy, Debug, Default)]
struct Drag {
    /// The value the field held when the button went down.
    start: f64,
    /// The pointer's travel since then: physical pixels, or the platform's own motion unit when it
    /// reports raw motion.
    travelled: f64,
    /// Where the pointer was last frame, in physical pixels. Only the platforms that report no raw
    /// motion need it.
    place: Option<egui::Pos2>,
}

impl Drag {
    /// The drag as its first frame sees it.
    ///
    /// egui calls a press a drag only once the pointer has moved a few points, and the trip from the
    /// button down belongs to the drag even though it arrived as movement: the first frame starts
    /// where the press was, not at zero. Nothing has rescaled the window between the press and this
    /// frame, so the trip is one scale's worth of points times that scale.
    fn from_press(ui: &Ui, value: f64, events_scale: f32) -> Self {
        let travelled = ui
            .input(|input| input.pointer.total_drag_delta())
            .map_or(0.0, |delta| f64::from(delta.x) * f64::from(events_scale));
        // The place the next frame measures from is where the pointer is now: the trip from the
        // press is already in `travelled`.
        let place = ui.input(|input| input.pointer.interact_pos()).map(|pos| pos * events_scale);
        Self { start: value, travelled, place }
    }
}

/// What the user typed, kept between frames while the field holds the focus.
#[derive(Clone, Debug, Default)]
struct Typed {
    /// The text in the field.
    text: String,
    /// The value the text belongs to, so text written before another change is not applied on top.
    value: f64,
}

/// A number the user scrubs with a horizontal drag or types in.
///
/// The widget owns `value` and writes every change into it, so the caller keeps a draft or a
/// document field up to date the same way it would with any other field. `decimals` decides what
/// the field shows and how far a drag rounds; integers use `0`.
pub struct Scrub<'a, N: Numeric> {
    value: &'a mut N,
    speed: f64,
    range: RangeInclusive<f64>,
    suffix: &'a str,
    decimals: usize,
}

impl<'a, N: Numeric> Scrub<'a, N> {
    /// A field over `value`, changing it by half a step per pixel of travel.
    pub fn new(value: &'a mut N) -> Self {
        Self {
            value,
            speed: 0.5,
            range: N::MIN.to_f64()..=N::MAX.to_f64(),
            suffix: "",
            decimals: if N::INTEGRAL { 0 } else { 2 },
        }
    }

    /// The change per pixel of horizontal travel, so the same hand movement means the same thing at
    /// any zoom and on any screen.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// The range the value stays inside, on a drag and when typed.
    pub fn range(mut self, range: RangeInclusive<N>) -> Self {
        self.range = range.start().to_f64()..=range.end().to_f64();
        self
    }

    /// What follows the number, a unit or a sign (`" mm"`).
    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    /// The digits shown after the decimal point; `0` draws a whole number.
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals.min(15);
        self
    }
}

impl<N: Numeric> Widget for Scrub<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self { value, speed, range, suffix, decimals } = self;
        let id = ui.next_auto_id();
        ui.memory_mut(|memory| memory.interested_in_focus(id, ui.layer_id()));

        // A value the range does not cover — a hand-edited file, a preset this build no longer
        // offers — becomes the nearest end, so the field never shows what it could not set.
        let raw = value.to_f64();
        let settled = clamped(raw, &range).unwrap_or(raw);
        if settled != raw {
            *value = N::from_f64(settled);
        }
        // The focus went elsewhere in an earlier frame: the text typed until then lands now.
        commit_typed(ui, id, value, &range);
        let old = value.to_f64();

        let text = number_text(old, decimals);
        let size = ui.spacing().interact_size;
        // The scale the window backend converted this pass's pointer positions with. A zoom takes
        // effect at the start of a pass, so that is the scale the pass before this one ended at.
        let scale_now = ui.ctx().pixels_per_point();
        let events_scale = ui.data(|data| data.get_temp::<f32>(id.with("events_scale"))).unwrap_or(scale_now);
        ui.data_mut(|data| data.insert_temp(id.with("events_scale"), scale_now));

        let mut response = if ui.memory(|memory| memory.has_focus(id)) {
            let mut typed =
                ui.data(|data| data.get_temp::<Typed>(id)).unwrap_or_else(|| Typed { text: text.clone(), value: old });
            let mut response = ui.add(TextEdit::singleline(&mut typed.text).id(id).min_size(size).desired_width(0.0));
            if response.gained_focus() {
                select_all(ui, id, &typed.text);
            }
            let escaped = ui.input(|input| input.key_pressed(Key::Escape));
            if response.lost_focus() && !escaped {
                write(value, parse(&typed.text).and_then(|parsed| clamped(parsed, &range)), old);
                ui.data_mut(|data| data.remove::<Typed>(id));
            } else if escaped {
                ui.data_mut(|data| data.remove::<Typed>(id));
            } else {
                ui.data_mut(|data| data.insert_temp(id, Typed { text: typed.text, value: value.to_f64() }));
            }
            // A keystroke changes the text, not the number: the number changes when a parsed value
            // is committed, and the comparison below reports that.
            response.flags.remove(egui::response::Flags::CHANGED);
            response
        } else {
            let response = ui
                .add(
                    Button::new(RichText::new(format!("{text}{suffix}")).monospace())
                        .sense(Sense::click_and_drag())
                        .min_size(size),
                )
                .on_hover_cursor(CursorIcon::ResizeHorizontal);
            if response.dragged() {
                ui.set_cursor_icon(CursorIcon::None);
                // The number follows the hand from the value the button went down on, so neither a
                // field that grows under the pointer nor the window's own rescaling can move it.
                let drag = if response.drag_started() {
                    Drag::from_press(ui, old, events_scale)
                } else {
                    let mut drag =
                        ui.data(|data| data.get_temp::<Drag>(id)).unwrap_or(Drag { start: old, ..Drag::default() });
                    drag.travelled += travel(ui, &mut drag, events_scale);
                    drag
                };
                ui.data_mut(|data| data.insert_temp(id, drag));
                let dragged = drag.start + drag.travelled * speed;
                write(value, clamped(rounded(dragged, decimals), &range), old);
            }
            if response.drag_stopped() {
                ui.data_mut(|data| data.remove::<Drag>(id));
            }
            if response.clicked() {
                ui.data_mut(|data| data.insert_temp(id, Typed { text: text.clone(), value: old }));
                ui.memory_mut(|memory| memory.request_focus(id));
                // The whole number is selected before the field draws its text edit, so the first
                // keystroke replaces it instead of extending it.
                select_all(ui, id, &text);
            }
            response
        };
        if value.to_f64() != old {
            response.mark_changed();
        }
        response.widget_info(|| egui::WidgetInfo::drag_value(ui.is_enabled(), value.to_f64()));
        response
    }
}

/// How far the pointer travelled this frame.
///
/// A platform that reports raw motion — `winit` forwards its device motion as `Event::MouseMoved` —
/// gives movement that has nothing to do with where the pointer is, so nothing on screen can disturb
/// it, and it is taken as the platform reports it: the unit of that motion is the platform's, but it
/// is proportional to the hand and free of every screen coordinate.
///
/// Without raw motion the only evidence is the pointer's place, which is read as the difference
/// between two places, each in the pixels of the scale it was measured at, so a frame that rescales
/// the window contributes nothing. `events_scale` is the scale the window backend converted this
/// pass's positions with: `egui` applies a zoom change at the start of a pass, so the events of a
/// pass carry the scale the previous pass ended at.
fn travel(ui: &Ui, drag: &mut Drag, events_scale: f32) -> f64 {
    if let Some(motion) = ui.input(|input| input.pointer.motion()) {
        return f64::from(motion.x);
    }
    // Nothing but the pointer's place is left: measure it in pixels, and only as a difference.
    let Some(position) = ui.input(|input| input.pointer.interact_pos()) else {
        return 0.0;
    };
    let place = position * events_scale;
    let step = drag.place.map_or(0.0, |last| f64::from(place.x - last.x));
    drag.place = Some(place);
    step
}

/// Applies the text typed before the field lost the focus, if the value is still the one it was
/// typed on.
///
/// A typed text that outlives its edit frame means the focus left the field — a click into another
/// widget — before the field could read it back, so the number lands here instead.
fn commit_typed<N: Numeric>(ui: &Ui, id: Id, value: &mut N, range: &RangeInclusive<f64>) {
    let Some(typed) = ui.data_mut(|data| data.remove_temp::<Typed>(id)) else {
        return;
    };
    let current = value.to_f64();
    if typed.value != current {
        return;
    }
    write(value, parse(&typed.text).and_then(|parsed| clamped(parsed, range)), current);
}

/// Writes a parsed or dragged number into the caller's value, leaving it alone when the field could
/// not read a number and when the number did not move.
fn write<N: Numeric>(value: &mut N, candidate: Option<f64>, old: f64) {
    let Some(candidate) = candidate.filter(|candidate| *candidate != old) else {
        return;
    };
    *value = N::from_f64(candidate);
}

/// The number as the field shows it.
fn number_text(value: f64, decimals: usize) -> String {
    if decimals == 0 { format!("{}", value.round() as i64) } else { format!("{value:.decimals$}") }
}

/// Reads a typed number, taking a comma for the decimal point.
fn parse(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Rounds a dragged value the same way the field shows it.
fn rounded(value: f64, decimals: usize) -> f64 {
    let factor = 10f64.powi(decimals.min(15) as i32);
    let scaled = value * factor;
    if scaled.is_finite() { scaled.round() / factor } else { value }
}

/// Keeps `value` inside `range`. A NaN has no place in a range and is refused, as is a range with
/// no inside, so no hostile number reaches a panicking clamp.
fn clamped(value: f64, range: &RangeInclusive<f64>) -> Option<f64> {
    let (start, end) = (*range.start(), *range.end());
    if value.is_nan() || start.is_nan() || end.is_nan() {
        return None;
    }
    Some(value.clamp(start.min(end), start.max(end)))
}

/// Selects the whole number when the field starts editing, so typing replaces it.
fn select_all(ui: &Ui, id: Id, text: &str) {
    let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::select_all_str(text)));
    state.store(ui.ctx(), id);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, UiBuilder, Vec2, pos2, vec2};

    use super::*;

    const SCREEN: Vec2 = vec2(800.0, 600.0);

    fn input(events: Vec<Event>) -> RawInput {
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), events, ..Default::default() }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn moved(pos: Pos2) -> Vec<Event> {
        vec![Event::PointerMoved(pos)]
    }

    /// A press-and-drag gesture: hover, press, move through `path`, release at its end.
    fn gesture(from: Pos2, path: &[Pos2]) -> Vec<Vec<Event>> {
        let mut frames = vec![moved(from), vec![button(from, true)]];
        frames.extend(path.iter().map(|pos| moved(*pos)));
        let end = path.last().copied().unwrap_or(from);
        frames.push(vec![button(end, false)]);
        frames
    }

    /// Runs `frames` over a field pinned to `rect`, returning the value it holds and the rect it
    /// drew in every frame.
    fn run_fields(frames: Vec<Vec<Event>>, rect: Rect, start: f64, speed: f64, range: f64) -> (f64, Vec<Rect>) {
        let ctx = egui::Context::default();
        let mut value = start;
        let mut rects = Vec::new();
        for events in frames {
            let output = ctx.run_ui(input(events), |ui| {
                let response = ui.put(rect, Scrub::new(&mut value).speed(speed).range(0.0..=range).decimals(0));
                rects.push(response.rect);
            });
            output.drop_without_applying_deltas();
        }
        (value, rects)
    }

    #[test]
    fn a_drag_reads_the_horizontal_delta_and_not_the_pointer_position() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        // The same forty points of travel, started from two different places on the field.
        let (left, _) = run_fields(gesture(pos2(310.0, 290.0), &[pos2(350.0, 290.0)]), rect, 100.0, 1.0, 1000.0);
        let (right, _) = run_fields(gesture(pos2(340.0, 290.0), &[pos2(380.0, 290.0)]), rect, 100.0, 1.0, 1000.0);
        assert_eq!(left, 140.0);
        assert_eq!(right, 140.0, "where the drag starts cannot change what it adds");
    }

    #[test]
    fn a_vertical_drag_leaves_the_value_alone() {
        let rect = Rect::from_min_size(pos2(300.0, 200.0), vec2(100.0, 20.0));
        let (value, _) = run_fields(gesture(pos2(350.0, 290.0), &[pos2(350.0, 240.0)]), rect, 100.0, 1.0, 1000.0);
        assert_eq!(value, 100.0, "only the horizontal delta is read");
    }

    /// Draws a right-aligned field in `row` and returns the cursor the frame asked for.
    fn run_row(ctx: &egui::Context, value: &mut u32, rects: &mut Vec<Rect>, events: Vec<Event>) -> CursorIcon {
        let row = Rect::from_min_size(pos2(200.0, 280.0), vec2(300.0, 40.0));
        let output = ctx.run_ui(input(events), |ui| {
            ui.scope_builder(UiBuilder::new().max_rect(row), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.add(Scrub::new(value).speed(10_000.0).range(0..=1_000_000));
                    rects.push(response.rect);
                });
            });
        });
        let icon = output.platform_output.cursor_icon;
        output.drop_without_applying_deltas();
        icon
    }

    #[test]
    fn a_field_that_grows_under_the_pointer_does_not_move_the_number() {
        // Right-aligned in its row, so the field grows leftwards under the pointer as the number
        // gains digits: the case a position-driven control turns into a chase.
        let ctx = egui::Context::default();
        let mut value = 9u32;
        let mut rects = Vec::new();
        let mut icons = Vec::new();
        icons.push(run_row(&ctx, &mut value, &mut rects, vec![]));
        let start = *rects.last().expect("the field drew itself");
        let from = pos2(start.right() - 8.0, start.center().y);
        let mut frames = gesture(from, &[pos2(from.x + 20.0, from.y), pos2(from.x + 40.0, from.y)]);
        let release = frames.pop().unwrap_or_default();
        for events in frames {
            icons.push(run_row(&ctx, &mut value, &mut rects, events));
        }
        let grown = *rects.last().expect("the field drew itself");
        run_row(&ctx, &mut value, &mut rects, release);
        assert_eq!(value, 400_009, "the number is the delta from the press, whatever the field's width");
        assert!(grown.width() > start.width(), "the field grew under the pointer while the drag ran");
        assert_eq!(icons.last(), Some(&CursorIcon::None), "the pointer is hidden for the whole drag");
    }

    #[test]
    fn the_pointer_hides_while_the_drag_lasts_and_comes_back_after_it() {
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let mut icons = Vec::new();
        for events in gesture(pos2(310.0, 290.0), &[pos2(330.0, 290.0)]) {
            let output = ctx.run_ui(input(events), |ui| {
                ui.put(rect, Scrub::new(&mut value));
            });
            icons.push(output.platform_output.cursor_icon);
            output.drop_without_applying_deltas();
        }
        let output = ctx.run_ui(input(vec![]), |ui| {
            ui.put(rect, Scrub::new(&mut value));
        });
        icons.push(output.platform_output.cursor_icon);
        output.drop_without_applying_deltas();
        // The gesture is hover, press, move, release, then one frame after it.
        assert_ne!(icons.first(), Some(&CursorIcon::None), "the pointer shows while the field is only hovered");
        assert_eq!(icons.get(2), Some(&CursorIcon::None), "the pointer hides while the drag lasts");
        assert_ne!(icons.get(3), Some(&CursorIcon::None), "and comes back once the button is released");
    }

    /// Clicks the field at `rect`, types `text` and presses Enter; returns the value it holds.
    fn type_into_field(rect: Rect, text: &str, start: u32) -> u32 {
        let ctx = egui::Context::default();
        let mut value = start;
        let centre = rect.center();
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frames = vec![
            moved(centre),
            vec![button(centre, true)],
            vec![button(centre, false)],
            // The field starts editing a frame later, and selects its number as it gains focus.
            vec![],
            vec![Event::Text(text.to_owned())],
            vec![enter],
            vec![],
        ];
        for events in frames {
            let output = ctx.run_ui(input(events), |ui| {
                ui.put(rect, Scrub::new(&mut value).range(75..=200));
            });
            output.drop_without_applying_deltas();
        }
        value
    }

    #[test]
    fn a_typed_value_outside_the_range_is_clamped() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        assert_eq!(type_into_field(rect, "900", 100), 200, "a value past the top comes back as the top");
        assert_eq!(type_into_field(rect, "20", 100), 75, "a value below the bottom comes back as the bottom");
    }

    #[test]
    fn a_typed_value_replaces_the_number_instead_of_extending_it() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        assert_eq!(type_into_field(rect, "120", 100), 120);
    }

    #[test]
    fn a_value_the_field_could_not_set_shows_the_nearest_end() {
        let ctx = egui::Context::default();
        let mut value = 900u32;
        let output = ctx.run_ui(input(vec![]), |ui| {
            ui.add(Scrub::new(&mut value).range(75..=200));
        });
        output.drop_without_applying_deltas();
        assert_eq!(value, 200);
    }

    #[test]
    fn a_number_is_shown_with_the_decimals_it_is_given() {
        assert_eq!(number_text(100.0, 0), "100");
        assert_eq!(number_text(0.256, 3), "0.256");
        assert_eq!(number_text(12.0, 2), "12.00");
    }

    #[test]
    fn a_hostile_range_or_value_never_panics() {
        assert_eq!(clamped(0.5, &(f64::NAN..=1.0)), None, "a range with no inside has no value");
        assert_eq!(clamped(f64::NAN, &(0.0..=1.0)), None, "a NaN is not a number to show");
        assert_eq!(clamped(1.5, &(2.0..=1.0)), Some(1.5), "a backwards range is read as its ends");
        assert_eq!(rounded(f64::INFINITY, 3), f64::INFINITY);
        assert_eq!(parse("not a number"), None);
        assert_eq!(parse("1,5"), Some(1.5));
        assert_eq!(parse(" 42 "), Some(42.0));
    }
}
