//! A numeric field: a number the user scrubs with a horizontal drag or types in.
//!
//! The field is one **76 × 22** box with the number right-aligned in tabular figures, plus the unit
//! as a dim caption *outside* the box. The unit never enters the editable text: a number the user
//! selects and replaces cannot drag `mm` or `°` along with it, and a pasted number is a number.
//!
//! A field owns one edit: its drag or its typing run is one undo entry (ADR 0092), the card dispatches
//! with the token `Session::edit_scope` minted, and Escape restores the value the edit started from.
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
//! offered range and written back by the field's [`CommitPolicy`]. Invalid text keeps the value the
//! field already had, marks the box red and returns the reason for the card to print.

use core::ops::RangeInclusive;
use core::time::Duration;

use egui::emath::Numeric;
use egui::text::CCursorRange;
use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Key, Margin, Response, RichText, Sense, Stroke, StrokeKind, TextEdit,
    TextStyle, Ui, Widget, pos2, vec2,
};
use furnicraft_engine::EditScope;
use furnicraft_text::Message;

/// The width every inspector field draws at, so a column of numbers lines up.
pub(crate) const FIELD_WIDTH: f32 = 76.0;

/// The height every inspector field draws at.
pub(crate) const FIELD_HEIGHT: f32 = 22.0;

/// The corner radius of the field box.
const FIELD_RADIUS: u8 = 4;

/// How long typed text waits after the last keystroke before it is committed.
pub(crate) const DEBOUNCE_SECONDS: f64 = 0.15;

/// When a field hands its number to the document.
///
/// Typed text always waits for [`DEBOUNCE_SECONDS`]: committing on every keystroke would rebuild the
/// model three times for «120». The policies differ in what a drag does — [`Scrub`] is the only place
/// the difference matters, and a field that shows a running number while the hand is on it is the
/// common case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CommitPolicy {
    /// The document follows the drag: a value the user is feeling out, like a width or an angle.
    #[default]
    Live,
    /// The field shows the dragged number but the document takes it only when the drag ends, so an
    /// expensive rebuild does not run on every frame of the gesture.
    OnRelease,
}

/// The drag in flight: the value the button went down on, how far the pointer has travelled since,
/// the place the last frame measured from, and whether Escape abandoned it.
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
    /// Escape ended the drag: the number does not follow the hand again until the button is released.
    cancelled: bool,
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
        let place = ui.input(|input| input.pointer.interact_pos()).map(|pos| pos * events_scale);
        Self { start: value, travelled, place, cancelled: false }
    }
}

/// What the user typed, kept between frames while the field holds the focus.
#[derive(Clone, Debug, Default)]
struct Typed {
    /// The text in the field.
    text: String,
    /// The value the text belongs to, so text written before another change is not applied on top.
    value: f64,
    /// Whether the text has not been parsed yet: the debounce, Enter and blur commit it.
    pending: bool,
    /// When the text last changed, in egui's clock, so the debounce can tell how long it has waited.
    changed_at: f64,
    /// Why the last committed text was refused, shown until the next accepted number.
    error: Option<Message>,
}

/// What a field did this frame.
#[derive(Debug)]
pub(crate) struct FieldOutcome {
    /// The widget's response.
    pub response: Response,
    /// The field holds a number the document should take now.
    pub commit: bool,
    /// The undo scope the commit belongs to, while the edit is in flight.
    pub scope: Option<EditScope>,
    /// Escape abandoned the edit: the caller restores the document from this scope.
    pub cancelled: Option<EditScope>,
    /// Why typed text was refused, for the card's error line.
    pub error: Option<Message>,
}

/// The undo scope of one field's edit, kept between frames while the drag or the typing runs.
#[derive(Clone, Copy, Debug, Default)]
struct EditState {
    /// The scope the first commit of the edit opened.
    scope: Option<EditScope>,
    /// Whether the edit ended last frame, so the scope closes once its commit was dispatched.
    finish_pending: bool,
    /// The scope an Escape ended, waiting for the card to restore the document.
    cancelled: Option<EditScope>,
}

/// How a field reads the text its user typed, in the field's own unit.
pub(crate) type Parse = fn(&str) -> Result<f64, Message>;

/// A number the user scrubs with a horizontal drag or types in.
///
/// The widget owns `value` and writes every change into it, so the caller keeps a draft or a
/// document field up to date the same way it would with any other field. `decimals` decides what
/// the field shows and how far a drag rounds; integers use `0`, millimetres and degrees `3`.
pub(crate) struct Scrub<'a, N: Numeric> {
    value: &'a mut N,
    speed: f64,
    range: RangeInclusive<f64>,
    unit: &'a str,
    decimals: usize,
    policy: CommitPolicy,
    parse: Parse,
    fresh: Option<EditScope>,
    focus: bool,
}

impl<'a, N: Numeric> Scrub<'a, N> {
    /// A field over `value`, changing it by half a step per pixel of travel.
    pub(crate) fn new(value: &'a mut N) -> Self {
        Self {
            value,
            speed: 0.5,
            range: N::MIN.to_f64()..=N::MAX.to_f64(),
            unit: "",
            decimals: if N::INTEGRAL { 0 } else { 2 },
            policy: CommitPolicy::default(),
            parse: parse_number,
            fresh: None,
            focus: false,
        }
    }

    /// The scope a new edit opens, minted by the session once per frame.
    ///
    /// A field that commits on every frame of a drag must not add an undo entry per frame: the first
    /// commit opens the scope, the rest of the drag merges into it, and the next edit gets a new one.
    pub(crate) fn edit(mut self, fresh: EditScope) -> Self {
        self.fresh = Some(fresh);
        self
    }

    /// The change per pixel of horizontal travel, so the same hand movement means the same thing at
    /// any zoom and on any screen.
    pub(crate) fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// The range the value stays inside, on a drag and when typed.
    pub(crate) fn range(mut self, range: RangeInclusive<N>) -> Self {
        self.range = range.start().to_f64()..=range.end().to_f64();
        self
    }

    /// The unit, drawn dim outside the box: it is never part of the editable text.
    pub(crate) fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    /// The digits shown after the decimal point; `0` draws a whole number.
    pub(crate) fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals.min(15);
        self
    }

    /// When the document takes the number.
    pub(crate) fn policy(mut self, policy: CommitPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// How the typed text becomes a number; the default reads a plain decimal.
    pub(crate) fn parse(mut self, parse: Parse) -> Self {
        self.parse = parse;
        self
    }

    /// Takes the keyboard focus this frame, so a field that just appeared can be typed into
    /// without a click. The caller owns the flag and clears it, because a field that asked for the
    /// focus every frame could never lose it.
    pub(crate) fn focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    /// Draws the field and reports whether this frame's number belongs in the document.
    pub(crate) fn show(self, ui: &mut Ui) -> FieldOutcome {
        let Self { value, speed, range, unit, decimals, policy, parse, fresh, focus } = self;
        // The space is reserved without a widget: an allocation with `Sense::hover` would tell egui
        // this id cannot take the focus, and egui would drop the focus it had just given it.
        let (id, rect) = ui.allocate_space(vec2(FIELD_WIDTH, FIELD_HEIGHT));
        ui.memory_mut(|memory| memory.interested_in_focus(id, ui.layer_id()));
        if focus {
            ui.memory_mut(|memory| memory.request_focus(id));
        }

        // A value the range does not cover — a hand-edited file, a preset this build no longer
        // offers — becomes the nearest end, so the field never shows what it could not set.
        let raw = value.to_f64();
        let settled = clamped(raw, &range).unwrap_or(raw);
        if settled != raw {
            *value = N::from_f64(settled);
        }
        let old = value.to_f64();
        let shown = number_text(old, decimals);

        // The scale the window backend converted this pass's pointer positions with. A zoom takes
        // effect at the start of a pass, so that is the scale the pass before this one ended at.
        let scale_now = ui.ctx().pixels_per_point();
        let events_scale = ui.data(|data| data.get_temp::<f32>(id.with("events_scale"))).unwrap_or(scale_now);
        ui.data_mut(|data| data.insert_temp(id.with("events_scale"), scale_now));

        // The edit's undo scope lives beside the field: a scope that ended last frame closes here,
        // after the commit it belongs to was dispatched.
        let mut edit = ui.data(|data| data.get_temp::<EditState>(id.with("edit"))).unwrap_or_default();
        if edit.finish_pending {
            edit.scope = None;
            edit.finish_pending = false;
        }

        let focused = ui.memory(|memory| memory.has_focus(id));
        let mut commit = false;
        let mut cancel = false;
        let mut report = None;

        // The focus can leave a field without this widget seeing the frame that took it — a Tab
        // handled through the keyboard's focus order does exactly that. Text typed until then is
        // committed here, once, instead of lingering in the buffer.
        if !focused && let Some(mut state) = ui.data(|data| data.get_temp::<Typed>(id)).filter(|state| state.pending) {
            state.pending = false;
            match parse(&state.text).and_then(|parsed| clamped(parsed, &range).ok_or(Message::NumberInvalid)) {
                Ok(parsed) => {
                    state.value = parsed;
                    state.text = number_text(parsed, decimals);
                    state.error = None;
                    commit = parsed != old;
                    if commit {
                        *value = N::from_f64(parsed);
                    }
                }
                Err(message) => {
                    state.error = Some(message);
                    state.text = shown.clone();
                    report = Some(message);
                }
            }
            ui.data_mut(|data| data.insert_temp(id, state));
        }

        paint_fill(ui, rect, focused);

        let mut response = if focused {
            let typed = std::cell::RefCell::new(ui.data(|data| data.get_temp::<Typed>(id)).unwrap_or_else(|| Typed {
                text: shown.clone(),
                value: old,
                ..Typed::default()
            }));
            // A number that moved underneath a finished edit — undo, a gizmo, the document itself —
            // replaces the text; text the user is still editing is left alone.
            {
                let mut state = typed.borrow_mut();
                if !state.pending && state.value != old {
                    state.text = shown.clone();
                    state.value = old;
                }
            }

            let response = ui.put(
                rect,
                TextEdit::singleline(&mut typed.borrow_mut().text)
                    .id(id)
                    .align(Align2::RIGHT_CENTER)
                    .margin(Margin::symmetric(5, 2))
                    .frame(egui::Frame::NONE),
            );
            if response.gained_focus() {
                let text = typed.borrow().text.clone();
                select_all(ui, id, &text);
            }

            let now = ui.input(|input| input.time);
            if response.changed() {
                let mut state = typed.borrow_mut();
                state.pending = true;
                state.changed_at = now;
                state.error = None;
            }

            let escaped = ui.input(|input| input.key_pressed(Key::Escape));
            let enter = ui.input(|input| input.key_pressed(Key::Enter));
            let lost = response.lost_focus();
            let due = typed.borrow().pending && now - typed.borrow().changed_at >= DEBOUNCE_SECONDS;
            if escaped {
                // Escape abandons the typed text: the number the edit started from comes back.
                let mut state = typed.borrow_mut();
                state.text = shown.clone();
                state.pending = false;
                state.error = None;
                cancel = true;
            } else if enter || lost || due {
                let text = typed.borrow().text.clone();
                match parse(&text).and_then(|parsed| clamped(parsed, &range).ok_or(Message::NumberInvalid)) {
                    Ok(parsed) => {
                        let mut state = typed.borrow_mut();
                        state.pending = false;
                        state.error = None;
                        state.value = parsed;
                        state.text = number_text(parsed, decimals);
                        commit = parsed != old;
                        if commit {
                            *value = N::from_f64(parsed);
                        }
                    }
                    Err(message) => {
                        let mut state = typed.borrow_mut();
                        state.pending = !lost;
                        state.error = Some(message);
                        report = Some(message);
                        if lost {
                            state.text = shown.clone();
                            state.value = old;
                        }
                    }
                }
            }
            if due || typed.borrow().pending {
                let remaining = DEBOUNCE_SECONDS - (now - typed.borrow().changed_at);
                ui.ctx().request_repaint_after(Duration::from_secs_f64(remaining.clamp(0.0, DEBOUNCE_SECONDS)));
            }
            let error = typed.borrow().error;
            report = report.or(error);
            let state = typed.borrow().clone();
            ui.data_mut(|data| data.insert_temp(id, state));
            paint_border(ui, rect, true, error.is_some());
            response
        } else {
            let response = ui.interact(rect, id, Sense::click_and_drag());
            let visuals = ui.style().interact(&response);
            ui.painter().text(
                pos2(rect.right() - 5.0, rect.center().y),
                Align2::RIGHT_CENTER,
                &shown,
                monospace(ui),
                visuals.text_color(),
            );

            if response.clicked() {
                ui.data_mut(|data| data.insert_temp(id, Typed { text: shown.clone(), value: old, ..Typed::default() }));
                ui.memory_mut(|memory| memory.request_focus(id));
                select_all(ui, id, &shown);
            }
            // egui ends a drag on Escape, so the drag's last frame is also a frame that may have
            // ended it: the cancel is read here, before the release hands a number over.
            if response.dragged() || response.drag_stopped() {
                let mut drag = if response.drag_started() {
                    ui.data_mut(|data| data.remove::<Typed>(id));
                    Drag::from_press(ui, old, events_scale)
                } else {
                    let mut drag =
                        ui.data(|data| data.get_temp::<Drag>(id)).unwrap_or(Drag { start: old, ..Drag::default() });
                    drag.travelled += travel(ui, &mut drag, events_scale);
                    drag
                };
                // Escape during the drag puts the number back where the hand found it and stops it
                // following until the button is released.
                if ui.input(|input| input.key_pressed(Key::Escape)) && !drag.cancelled {
                    drag.cancelled = true;
                    cancel = true;
                }
                let dragged = clamped(rounded(drag.start + drag.travelled * speed, decimals), &range);
                if response.dragged() && !drag.cancelled {
                    ui.set_cursor_icon(CursorIcon::None);
                    if let Some(candidate) = dragged.filter(|candidate| *candidate != old) {
                        *value = N::from_f64(candidate);
                        commit = policy == CommitPolicy::Live;
                    }
                }
                if response.drag_stopped() {
                    // The release hands over the number the drag ended on. A live field has already
                    // sent it; an on-release field sends it exactly here.
                    if !drag.cancelled && dragged.is_some_and(|candidate| candidate != drag.start) {
                        if let Some(candidate) = dragged {
                            *value = N::from_f64(candidate);
                        }
                        commit = policy == CommitPolicy::OnRelease || dragged != Some(old);
                    }
                    ui.data_mut(|data| data.remove::<Drag>(id));
                } else {
                    ui.data_mut(|data| data.insert_temp(id, drag));
                }
            }
            let error = ui.data(|data| data.get_temp::<Typed>(id)).and_then(|typed| typed.error);
            report = report.or(error);
            paint_border(ui, rect, false, error.is_some());
            response
        };

        if !unit.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(unit).small().color(ui.visuals().weak_text_color()));
        }

        if edit.scope.is_none()
            && fresh.is_some()
            && (response.drag_started() || response.gained_focus() || (commit && !cancel))
        {
            edit.scope = fresh;
        }
        if cancel {
            edit.cancelled = edit.scope.take();
        }
        if response.drag_stopped() || response.lost_focus() {
            edit.finish_pending = true;
        }
        let scope = edit.scope;
        let cancelled = edit.cancelled.take();
        ui.data_mut(|data| data.insert_temp(id.with("edit"), edit));

        if value.to_f64() != old {
            response.mark_changed();
        }
        response.widget_info(|| egui::WidgetInfo::drag_value(ui.is_enabled(), value.to_f64()));
        FieldOutcome { response, commit, scope, cancelled, error: report }
    }
}

impl<N: Numeric> Widget for Scrub<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Paints the box behind the number: dark while the field has the focus, the control colour else.
fn paint_fill(ui: &Ui, rect: egui::Rect, focused: bool) {
    let fill = if focused { ui.visuals().extreme_bg_color } else { ui.visuals().widgets.inactive.weak_bg_fill };
    ui.painter().rect_filled(rect, FIELD_RADIUS, fill);
}

/// Paints the box's outline, red while the typed text was refused.
fn paint_border(ui: &Ui, rect: egui::Rect, focused: bool, invalid: bool) {
    let stroke = if invalid {
        Stroke::new(1.0, Color32::LIGHT_RED)
    } else if focused {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.inactive.bg_stroke
    };
    ui.painter().rect_stroke(rect, FIELD_RADIUS, stroke, StrokeKind::Inside);
}

/// The font of every number in a field: monospaced digits are the tabular figures of the theme, so a
/// column of values never shifts a character when one of them gains a digit.
fn monospace(ui: &Ui) -> FontId {
    TextStyle::Monospace.resolve(ui.style())
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

/// The number as the field shows it.
///
/// Trailing zeros are trimmed, so a millimetre field shows `12.5` and not `12.500`; the digits after
/// the point stay fixed while the value moves, which is what keeps the column still.
fn number_text(value: f64, decimals: usize) -> String {
    if decimals == 0 {
        return format!("{}", value.round() as i64);
    }
    let text = format!("{value:.decimals$}");
    let trimmed = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { text.as_str() };
    if trimmed == "-0" { "0".to_owned() } else { trimmed.to_owned() }
}

/// Reads a typed number, taking a comma for the decimal point.
fn parse_number(text: &str) -> Result<f64, Message> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|value| value.is_finite()).ok_or(Message::NumberInvalid)
}

/// Reads millimetres, refusing more precision than a micrometre stores.
pub(crate) fn parse_millimetres(text: &str) -> Result<f64, Message> {
    furnicraft_units::Length::parse_millimetres(text)
        .map(furnicraft_units::Length::millimetres_f64)
        .map_err(crate::length_error)
}

/// A number of millimetres as a length, rounded to the micrometre the document stores.
///
/// A field works in `f64` because that is what a drag produces; the value becomes a [`Length`] here,
/// once, at the boundary. A value that cannot be a length — infinite, or past a kilometre — is `None`
/// instead of a silently clamped length.
pub(crate) fn millimetres(value: f64) -> Option<furnicraft_units::Length> {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return None;
    }
    let micrometres = (value * 1_000.0).round();
    if !micrometres.is_finite() || micrometres < i64::MIN as f64 || micrometres > i64::MAX as f64 {
        return None;
    }
    Some(furnicraft_units::Length::from_micrometres(micrometres as i64))
}

/// A number of degrees as an angle, rounded to the millidegree the document stores.
pub(crate) fn degrees(value: f64) -> Option<furnicraft_units::Degrees> {
    if !value.is_finite() {
        return None;
    }
    let millidegrees = (value * 1_000.0).round();
    if !millidegrees.is_finite() || millidegrees < i64::MIN as f64 || millidegrees > i64::MAX as f64 {
        return None;
    }
    Some(furnicraft_units::Degrees::from_millidegrees(millidegrees as i64))
}

/// Reads degrees, refusing more precision than a millidegree stores.
pub(crate) fn parse_degrees(text: &str) -> Result<f64, Message> {
    furnicraft_units::Degrees::parse_degrees(text)
        .map(furnicraft_units::Degrees::degrees_f64)
        .map_err(crate::degrees_error)
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

    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2, pos2, vec2};

    use super::*;

    const SCREEN: Vec2 = vec2(800.0, 600.0);

    fn input(events: Vec<Event>) -> RawInput {
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), events, ..Default::default() }
    }

    fn input_at(time: f64, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(time),
            events,
            ..Default::default()
        }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
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

    /// Draws a field pinned to `rect` and returns what it reported this frame.
    fn draw_field(
        ui: &mut egui::Ui,
        session: &mut furnicraft_engine::Session,
        rect: Rect,
        value: &mut u32,
        policy: CommitPolicy,
    ) -> FieldOutcome {
        let fresh = session.edit_scope();
        let mut outcome = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            outcome =
                Some(Scrub::new(value).speed(1.0).range(0..=1000).decimals(0).policy(policy).edit(fresh).show(ui));
        });
        outcome.expect("the field draws itself")
    }

    fn edit_session() -> furnicraft_engine::Session {
        furnicraft_engine::Session::new(furnicraft_testkit::sample_project())
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
            ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let outcome = Scrub::new(value).speed(10_000.0).range(0..=1_000_000).show(ui);
                    rects.push(outcome.response.rect);
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
        assert_eq!(grown.width(), FIELD_WIDTH, "the box keeps its width while the number grows inside it");
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

    /// Clicks the field at `rect` and returns the context the frames ran on.
    fn click_field(ctx: &egui::Context, rect: Rect, frames: Vec<Vec<Event>>, value: &mut u32) {
        for events in frames {
            let output = ctx.run_ui(input(events), |ui| {
                ui.put(rect, Scrub::new(value).range(75..=200).decimals(0));
            });
            output.drop_without_applying_deltas();
        }
    }

    /// Clicks the field at `rect`, types `text` and presses Enter; returns the value it holds.
    fn type_into_field(rect: Rect, text: &str, start: u32) -> u32 {
        let ctx = egui::Context::default();
        let mut value = start;
        let centre = rect.center();
        click_field(
            &ctx,
            rect,
            vec![
                moved(centre),
                vec![button(centre, true)],
                vec![button(centre, false)],
                // The field starts editing a frame later, and selects its number as it gains focus.
                vec![],
                vec![Event::Text(text.to_owned())],
                vec![key(Key::Enter)],
                vec![],
            ],
            &mut value,
        );
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
    fn invalid_text_keeps_the_value_and_reports_the_error() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let mut session = edit_session();
        let mut report = None;
        let centre = rect.center();
        for events in [
            moved(centre),
            vec![button(centre, true)],
            vec![button(centre, false)],
            vec![],
            vec![Event::Text("wide".to_owned())],
            vec![key(Key::Enter)],
        ] {
            let output = ctx.run_ui(input(events), |ui| {
                let outcome = draw_field(ui, &mut session, rect, &mut value, CommitPolicy::Live);
                report = outcome.error;
            });
            output.drop_without_applying_deltas();
        }
        assert_eq!(value, 100, "a refused number leaves the value where it was");
        assert_eq!(report, Some(Message::NumberInvalid), "and says why");
    }

    #[test]
    fn typing_commits_once_after_the_debounce_and_not_per_character() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let mut session = edit_session();
        let mut commits = 0;
        let centre = rect.center();
        let mut frames = vec![
            (0.0, moved(centre)),
            (0.0, vec![button(centre, true)]),
            (0.0, vec![button(centre, false)]),
            (0.0, vec![]),
        ];
        for (index, character) in "120".chars().enumerate() {
            frames.push((0.05 * index as f64, vec![Event::Text(character.to_string())]));
        }
        // A frame inside the debounce window, then one past it.
        frames.push((0.10, vec![]));
        frames.push((0.30, vec![]));
        for (time, events) in frames {
            let output = ctx.run_ui(input_at(time, events), |ui| {
                let outcome = draw_field(ui, &mut session, rect, &mut value, CommitPolicy::Live);
                commits += usize::from(outcome.commit);
            });
            output.drop_without_applying_deltas();
        }
        assert_eq!(value, 120);
        assert_eq!(commits, 1, "«120» is one commit, not three");
    }

    #[test]
    fn enter_and_blur_both_commit() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        assert_eq!(type_into_field(rect, "150", 100), 150);

        // Blur: click the field, type, then click somewhere else.
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let centre = rect.center();
        let elsewhere = pos2(600.0, 500.0);
        for events in vec![
            moved(centre),
            vec![button(centre, true)],
            vec![button(centre, false)],
            vec![],
            vec![Event::Text("170".to_owned())],
            moved(elsewhere),
            vec![button(elsewhere, true)],
            vec![button(elsewhere, false)],
            vec![],
        ] {
            let output = ctx.run_ui(input(events), |ui| {
                ui.put(rect, Scrub::new(&mut value).range(75..=200).decimals(0));
            });
            output.drop_without_applying_deltas();
        }
        assert_eq!(value, 170, "losing the focus commits what was typed");
    }

    #[test]
    fn escape_during_a_drag_reports_a_cancel_and_stops_the_number() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let mut session = edit_session();
        let mut cancel = false;
        let from = pos2(310.0, 290.0);
        let frames = vec![
            moved(from),
            vec![button(from, true)],
            moved(pos2(330.0, 290.0)),
            vec![key(Key::Escape)],
            moved(pos2(400.0, 290.0)),
            vec![button(pos2(400.0, 290.0), false)],
            vec![],
        ];
        for events in frames {
            let output = ctx.run_ui(input(events), |ui| {
                let outcome = draw_field(ui, &mut session, rect, &mut value, CommitPolicy::Live);
                cancel |= outcome.cancelled.is_some();
            });
            output.drop_without_applying_deltas();
        }
        assert!(cancel, "Escape must tell the card to restore the document");
        assert_eq!(value, 120, "the number stops where it was when Escape was pressed");
    }

    #[test]
    fn a_drag_hands_over_every_frame_or_only_at_release() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let from = pos2(310.0, 290.0);
        let commits = |policy: CommitPolicy| {
            let ctx = egui::Context::default();
            let mut value = 100u32;
            let mut session = edit_session();
            let mut commits = 0;
            for events in gesture(from, &[pos2(330.0, 290.0), pos2(360.0, 290.0)]) {
                let output = ctx.run_ui(input(events), |ui| {
                    let outcome = draw_field(ui, &mut session, rect, &mut value, policy);
                    commits += usize::from(outcome.commit);
                });
                output.drop_without_applying_deltas();
            }
            (value, commits)
        };
        assert_eq!(commits(CommitPolicy::Live), (150, 2), "a live field follows the hand");
        assert_eq!(commits(CommitPolicy::OnRelease), (150, 1), "an on-release field commits once, at the end");
    }

    #[test]
    fn the_unit_is_drawn_outside_the_box_and_never_enters_the_editable_text() {
        let rect = Rect::from_min_size(pos2(300.0, 280.0), vec2(100.0, 20.0));
        let ctx = egui::Context::default();
        let mut value = 120.5f64;
        let centre = rect.center();
        let mut buffer = None;
        for events in [moved(centre), vec![button(centre, true)], vec![button(centre, false)], vec![]] {
            let output = ctx.run_ui(input(events), |ui| {
                let mut outcome = None;
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    outcome = Some(Scrub::new(&mut value).unit("mm").decimals(3).show(ui));
                });
                buffer = outcome
                    .and_then(|outcome| ui.data(|data| data.get_temp::<Typed>(outcome.response.id)))
                    .map(|typed| typed.text);
            });
            output.drop_without_applying_deltas();
        }
        let text = buffer.unwrap_or_default();
        assert_eq!(text, "120.5", "the editable text is the number alone");
        assert!(!text.contains("mm"), "a unit inside the editable string is the bug this field exists to fix");
    }

    #[test]
    fn the_box_is_seventy_six_by_twenty_two() {
        let ctx = egui::Context::default();
        let mut value = 100u32;
        let mut size = None;
        let output = ctx.run_ui(input(vec![]), |ui| {
            let outcome = Scrub::new(&mut value).show(ui);
            size = Some(outcome.response.rect.size());
        });
        output.drop_without_applying_deltas();
        assert_eq!(size, Some(vec2(FIELD_WIDTH, FIELD_HEIGHT)));
        assert_eq!((FIELD_WIDTH, FIELD_HEIGHT), (76.0, 22.0));
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
        assert_eq!(number_text(12.0, 2), "12");
        assert_eq!(number_text(12.5, 3), "12.5");
        assert_eq!(number_text(-0.0001, 2), "0");
    }

    #[test]
    fn a_hostile_range_or_value_never_panics() {
        assert_eq!(clamped(0.5, &(f64::NAN..=1.0)), None, "a range with no inside has no value");
        assert_eq!(clamped(f64::NAN, &(0.0..=1.0)), None, "a NaN is not a number to show");
        assert_eq!(clamped(1.5, &(2.0..=1.0)), Some(1.5), "a backwards range is read as its ends");
        assert_eq!(rounded(f64::INFINITY, 3), f64::INFINITY);
        assert_eq!(parse_number("not a number"), Err(Message::NumberInvalid));
        assert_eq!(parse_number("1,5"), Ok(1.5));
        assert_eq!(parse_number(" 42 "), Ok(42.0));
    }

    #[test]
    fn a_length_text_is_read_without_its_unit_and_refuses_extra_precision() {
        assert_eq!(parse_millimetres("12.5"), Ok(12.5));
        assert_eq!(parse_millimetres("12.5 mm"), Ok(12.5));
        assert_eq!(parse_millimetres("1.2345"), Err(Message::LengthErrorTooPrecise));
        assert_eq!(parse_degrees("30.5"), Ok(30.5));
        assert_eq!(parse_degrees("30.5°"), Ok(30.5));
        assert_eq!(parse_degrees("1.2345"), Err(Message::DegreesErrorTooPrecise));
    }
}
