//! The exact-distance fields a gesture shows over the viewport (ADR 0103).
//!
//! While a fill, a box or a move runs, the maker can state a distance instead of trusting the
//! pointer. The overlay draws one shared number field per direction of the moving part's free axis:
//! from the new face to the first surface a model probe found. The fields are the scrubbing field of
//! ADR 0092 — a bare number with `mm` outside — not a dialog, and exactly one of them is active.
//!
//! This module owns the two things the shell must not re-invent: the **measurement** (two faces of
//! the part, the surfaces that stop them, and the pin or lock on either side) and the **field
//! anatomy**. The numbers come from the caller: it hands over the coordinates the model probe
//! answered, never a second computation (ADR 0103). Nothing here writes a document.
//!
//! The keyboard is the point of the surface:
//!
//! - typing leaves the pointer behind for that direction and pins the face where the number says;
//! - Enter locks the pinned face and draws the lock glyph, so the opposite distance is measured from
//!   the locked face rather than from the original reference;
//! - Tab moves to the opposite direction, skipping one with no neighbour;
//! - Escape returns the direction to the pointer and leaves no trace;
//! - the field's menu offers «По центру» / *Centred*, which places the part in the middle of the
//!   void and yields a computed coordinate.
//!
//! A direction with no neighbour draws a disabled field and the sentence that says why. A value that
//! is negative, outside the void or would resize a part whose opposite face is locked is refused
//! with a message and writes nothing.

use core::ops::RangeInclusive;

use egui::{Align2, CornerRadius, Id, Key, Margin, Modifiers, Order, Pos2, Rect, RichText, Sense, Stroke, Ui};

use furnicraft_text::Message;

use crate::icons;
use crate::scrub::Scrub;
use crate::theme;

/// One of the two opposite directions of the free axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// Towards the void's low end: the part's low face is the reference.
    Low,
    /// Towards the void's high end: the part's high face is the reference.
    High,
}

impl Side {
    /// Every side in drawing order.
    pub const ALL: [Self; 2] = [Self::Low, Self::High];

    /// The other direction.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::High => Self::Low,
        }
    }

    /// An index into a two-entry array.
    const fn index(self) -> usize {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }
}

/// Why a distance cannot be taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceRefusal {
    /// There is no surface in that direction, so the field is disabled.
    NoNeighbour,
    /// The number is negative or leaves the part outside the void.
    OutOfVoid,
    /// The opposite face is locked, so taking this number would resize the part.
    WouldResize,
}

impl DistanceRefusal {
    /// The maker's sentence for this refusal.
    pub const fn message(self) -> Message {
        match self {
            Self::NoNeighbour => Message::DistanceNoNeighbour,
            Self::OutOfVoid => Message::DistanceOutOfVoid,
            Self::WouldResize => Message::DistanceLockedFace,
        }
    }
}

/// The two distances of one gesture, in millimetres along the free axis.
///
/// `low`/`high` are the part's two faces; `wall_low`/`wall_high` are the surfaces the model probe
/// answered, or `None` when a direction has no neighbour. The coordinates are the caller's frame —
/// the open product's own axes for a fill — and only their differences reach the fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distances {
    wall_low: Option<f64>,
    wall_high: Option<f64>,
    low: f64,
    high: f64,
    /// The face a typed number pinned, per side. A pin holds the part against the pointer until the
    /// gesture ends, even before the distance is locked.
    pins: [Option<f64>; 2],
    /// Whether Enter locked the pinned face, which the field shows with a glyph.
    locks: [bool; 2],
    /// The field that owns the keyboard, if any.
    active: Option<Side>,
}

impl Distances {
    /// The distances of a part whose faces are `low` and `high` in a void spanning `wall_low` and
    /// `wall_high`. Unusable numbers, or a part that is not between its walls, give `None`.
    pub fn new(wall_low: Option<f64>, wall_high: Option<f64>, low: f64, high: f64) -> Option<Self> {
        if !(low.is_finite() && high.is_finite()) || high <= low {
            return None;
        }
        if wall_low.is_some_and(|wall| !wall.is_finite() || wall > low)
            || wall_high.is_some_and(|wall| !wall.is_finite() || wall < high)
        {
            return None;
        }
        Some(Self { wall_low, wall_high, low, high, pins: [None; 2], locks: [false; 2], active: None })
    }

    /// Reads a new measurement while keeping the pins and locks the maker set.
    ///
    /// The shell measures the ghost again after every move, so this is the per-frame sync: a locked
    /// or pinned face stays where the maker put it, and everything else follows the pointer.
    pub fn sync(&mut self, wall_low: Option<f64>, wall_high: Option<f64>, low: f64, high: f64) {
        if !(low.is_finite() && high.is_finite()) || high <= low {
            return;
        }
        self.wall_low = wall_low;
        self.wall_high = wall_high;
        // A pinned face is where the typed number put it, and the opposite face follows the
        // extent it had: the pointer may not stretch a part whose other face the maker pinned.
        let extent = self.high - self.low;
        match (self.pins[Side::Low.index()], self.pins[Side::High.index()]) {
            (Some(low), Some(high)) => {
                self.low = low;
                self.high = high;
            }
            (Some(low), None) => {
                self.low = low;
                self.high = low + extent;
            }
            (None, Some(high)) => {
                self.low = high - extent;
                self.high = high;
            }
            (None, None) => {
                self.low = low;
                self.high = high;
            }
        }
        if self.high <= self.low {
            // The face the pin holds no longer belongs to this measurement: drop the pins rather
            // than draw a field that can never act.
            self.pins = [None; 2];
            self.locks = [false; 2];
            self.low = low;
            self.high = high;
        }
    }

    /// The part's low face.
    pub const fn low(&self) -> f64 {
        self.low
    }

    /// The part's high face.
    pub const fn high(&self) -> f64 {
        self.high
    }

    /// The part's extent along the axis.
    pub fn extent(&self) -> f64 {
        self.high - self.low
    }

    /// The surface that stops a probe from this side, or `None` when there is none.
    pub const fn neighbour(&self, side: Side) -> Option<f64> {
        match side {
            Side::Low => self.wall_low,
            Side::High => self.wall_high,
        }
    }

    /// The face a field measures from: the part's own face, or the locked opposite face.
    pub fn reference(&self, side: Side) -> f64 {
        match side {
            Side::Low => {
                if self.locks[Side::High.index()] {
                    self.high
                } else {
                    self.low
                }
            }
            Side::High => {
                if self.locks[Side::Low.index()] {
                    self.low
                } else {
                    self.high
                }
            }
        }
    }

    /// The number a field shows, or `None` when the direction has no neighbour.
    pub fn value(&self, side: Side) -> Option<f64> {
        let wall = self.neighbour(side)?;
        let value = (self.reference(side) - wall).abs();
        value.is_finite().then_some(value)
    }

    /// The widest number a field may show: the void's own span.
    ///
    /// It is the span, not the free gap, because a locked opposite face makes the field show the
    /// distance from that face to the wall; taking a number is bounded by [`Self::place`], which
    /// knows the extent and refuses an out-of-void value with a sentence.
    pub fn range(&self) -> RangeInclusive<f64> {
        let span = match (self.wall_low, self.wall_high) {
            (Some(low), Some(high)) => (high - low).max(0.0),
            _ => 0.0,
        };
        // A whole millimetre is the step (ADR 0103); a fractional void still bounds the field.
        0.0..=span
    }

    /// Whether Enter locked this side.
    pub const fn locked(&self, side: Side) -> bool {
        self.locks[side.index()]
    }

    /// The field that owns the keyboard.
    pub const fn active(&self) -> Option<Side> {
        self.active
    }

    /// Makes `side` the active field.
    pub fn activate(&mut self, side: Option<Side>) {
        self.active = side;
    }

    /// Moves the keyboard to the opposite direction, skipping one without a neighbour.
    pub fn tab(&mut self) -> Option<Side> {
        let current = self.active?;
        let mut next = current.opposite();
        if self.neighbour(next).is_none() {
            next = current;
        }
        self.activate(Some(next));
        Some(next)
    }

    /// Places the part so the distance on `side` is `value`, keeping its extent.
    ///
    /// This is the fill and move case: the part's size is what it is, and only its position along
    /// the axis is free. Typing a number on a side whose opposite face is locked would change the
    /// extent, so it is refused instead of silently resizing the part.
    pub fn place(&mut self, side: Side, value: f64) -> Result<(), DistanceRefusal> {
        if self.neighbour(side).is_none() {
            return Err(DistanceRefusal::NoNeighbour);
        }
        if !value.is_finite() || value < 0.0 {
            return Err(DistanceRefusal::OutOfVoid);
        }
        if self.locked(side) {
            // The face the number would move is fixed by the maker's own Enter.
            return Err(DistanceRefusal::WouldResize);
        }
        if self.locks[side.opposite().index()] {
            // The field is a span from the locked face: taking it would change the extent.
            return Err(DistanceRefusal::WouldResize);
        }
        let (low, high) = match side {
            Side::Low => {
                let wall = self.wall_low.ok_or(DistanceRefusal::NoNeighbour)?;
                let low = wall + value;
                (low, low + self.extent())
            }
            Side::High => {
                let wall = self.wall_high.ok_or(DistanceRefusal::NoNeighbour)?;
                let high = wall - value;
                (high - self.extent(), high)
            }
        };
        if low < self.wall_low.unwrap_or(f64::NEG_INFINITY) - f64::EPSILON
            || high > self.wall_high.unwrap_or(f64::INFINITY) + f64::EPSILON
        {
            return Err(DistanceRefusal::OutOfVoid);
        }
        self.low = low;
        self.high = high;
        // The part keeps its extent, so the face on the other side moves with it; its pin would
        // otherwise hold the part at the old measurement.
        self.pins[side.opposite().index()] = None;
        self.pins[side.index()] = Some(match side {
            Side::Low => low,
            Side::High => high,
        });
        Ok(())
    }

    /// Locks the face on `side` where it stands, so the opposite distance is measured from it.
    pub fn lock(&mut self, side: Side) -> Result<(), DistanceRefusal> {
        if self.neighbour(side).is_none() {
            return Err(DistanceRefusal::NoNeighbour);
        }
        self.locks[side.index()] = true;
        self.pins[side.index()] = Some(match side {
            Side::Low => self.low,
            Side::High => self.high,
        });
        Ok(())
    }

    /// Puts the part in the middle of the void and answers its low face coordinate.
    pub fn centre(&mut self) -> Result<f64, DistanceRefusal> {
        let (Some(low), Some(high)) = (self.wall_low, self.wall_high) else {
            return Err(DistanceRefusal::NoNeighbour);
        };
        let extent = self.extent();
        let free = high - low - extent;
        if free < 0.0 {
            return Err(DistanceRefusal::OutOfVoid);
        }
        self.low = low + free * 0.5;
        self.high = self.low + extent;
        // The two faces are pinned where the midpoint put them; the pointer may not pull the part
        // out of the middle until the maker types again or asks for the mouse back.
        self.pins = [Some(self.low), Some(self.high)];
        self.locks = [false; 2];
        Ok(self.low)
    }

    /// The low face coordinate a gesture must use, or `None` while the pointer drives it.
    pub fn pinned_low(&self) -> Option<f64> {
        if self.pins.iter().any(Option::is_some) || self.locks.iter().any(|locked| *locked) {
            Some(self.low)
        } else {
            None
        }
    }

    /// Abandons every pin and lock: the pointer owns the part again.
    pub fn cancel(&mut self) {
        self.pins = [None; 2];
        self.locks = [false; 2];
        self.active = None;
    }

    /// Whether the overlay owns the pointer for any direction right now.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

/// Where one field draws this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldPlacement {
    /// Which direction the field measures.
    pub side: Side,
    /// The centre of the field's plate, in screen points.
    pub position: Pos2,
}

/// What one frame of the overlay asked the shell to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceOutcome {
    /// The plates the fields drew, so a press on one is chrome and never a pick.
    pub rects: Vec<Rect>,
    /// The measurement changed: the shell re-reads [`Distances::pinned_low`] and re-plans the ghost.
    pub changed: bool,
    /// Escape returned the direction to the pointer; the shell must not also end the tool.
    pub cancel: bool,
}

/// The overlay's per-frame state that is not the measurement: the field that asked for the
/// keyboard and the sentence a refusal left behind.
#[derive(Clone, Debug, Default)]
pub struct DistanceFields {
    focus: Option<Side>,
    error: Option<Message>,
    /// Escape handed the part back to the pointer: no field takes the keyboard again until the
    /// maker clicks one or a new gesture starts.
    suspended: bool,
    /// The egui focus id of the field that held the keyboard, so a vanishing overlay can give it
    /// back instead of leaving every shortcut behind a field nobody can see.
    focused_id: Option<Id>,
}

impl DistanceFields {
    /// The sentence the last refused number left, if any.
    pub const fn error(&self) -> Option<Message> {
        self.error
    }

    /// Forgets the fields, their error, their focus and the suspended state, for example when the
    /// tool disarms or a part was created.
    pub fn reset(&mut self) {
        self.focus = None;
        self.error = None;
        self.suspended = false;
        self.focused_id = None;
    }

    /// Hides the overlay and gives the keyboard focus back to the window.
    ///
    /// A field that vanishes while focused would leave `text_edit_focused` true, and every shortcut
    /// behind it would be dead until a click somewhere else; the gesture ends by handing the focus
    /// to nobody.
    pub fn dismiss(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.focused_id.take() {
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
        self.focus = None;
        self.error = None;
        self.suspended = false;
    }

    /// Draws the fields over the viewport and reads the keyboard for the active one.
    ///
    /// `placements` lists the directions that have an on-screen position; a side with no placement
    /// draws nothing. The values themselves come from `state`, so this never measures anything.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut Distances,
        placements: &[FieldPlacement],
        opacity: u32,
    ) -> DistanceOutcome {
        let lang = crate::language(ui.ctx());
        let mut outcome = DistanceOutcome::default();
        // Tab belongs to the gesture, not to egui's focus order: consume it before a text edit
        // sees it, so the field's own Tab cannot move the keyboard somewhere else.
        let tab = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Tab));
        if tab
            && state.active().is_some()
            && let Some(next) = state.tab()
        {
            self.focus = Some(next);
            self.error = None;
        }
        // A fresh gesture gives the field nearest the pointer the keyboard, so a number can be
        // typed without aiming at a plate first. Escape suspends that until the maker asks again.
        if state.active().is_none()
            && !self.suspended
            && let Some(side) = nearest(placements, ui)
        {
            state.activate(Some(side));
            self.focus = Some(side);
        }
        let escape = ui.input(|input| input.key_pressed(Key::Escape));
        let enter = ui.input(|input| input.key_pressed(Key::Enter));
        let mut request = self.focus.take();
        if request.is_none() && ui.memory(|memory| memory.focused()).is_none() {
            request = state.active();
        }
        let mut focused: Option<Side> = None;
        for placement in placements {
            let side = placement.side;
            let area = egui::Area::new(Id::unique(("furnicraft.distance", side.index())))
                .order(Order::Middle)
                .fixed_pos(placement.position)
                .pivot(Align2::CENTER_CENTER)
                .show(ui.ctx(), |ui| {
                    let fill = theme::at_opacity(theme::floating(ui), opacity.clamp(80, 100));
                    let frame = egui::Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(1.0, theme::line(ui)))
                        .corner_radius(CornerRadius::same(theme::RADIUS))
                        .inner_margin(Margin::symmetric(6, 3));
                    frame.show(ui, |ui| match state.neighbour(side) {
                        None => {
                            ui.horizontal(|ui| {
                                ui.add_space(2.0);
                                ui.label(RichText::new("—").monospace().color(theme::text_dim(ui)));
                                ui.label(RichText::new("mm").small().color(theme::text_dim(ui)));
                            });
                            ui.label(
                                RichText::new(Message::DistanceNoNeighbour.text(lang))
                                    .small()
                                    .color(theme::text_dim(ui)),
                            );
                        }
                        Some(_) => {
                            ui.horizontal(|ui| {
                                if state.locked(side) {
                                    let (rect, lock) = ui.allocate_exact_size(
                                        egui::vec2(icons::ICON_POINT_SIZE, icons::ICON_POINT_SIZE),
                                        Sense::hover(),
                                    );
                                    icons::draw_icon(
                                        ui,
                                        icons::actions::LOCK,
                                        rect,
                                        icons::ICON_POINT_SIZE,
                                        theme::text_dim(ui),
                                    );
                                    if let Some(value) = state.value(side) {
                                        lock.on_hover_text(Message::DistanceLocked.with(lang, [format!("{value:.0}")]));
                                    }
                                }
                                let mut value = state.value(side).unwrap_or(0.0);
                                let field = Scrub::new(&mut value)
                                    .speed(0.5)
                                    .range(state.range())
                                    .decimals(0)
                                    .unit("mm")
                                    .focus(request == Some(side))
                                    .show(ui);
                                if field.response.has_focus() {
                                    focused = Some(side);
                                    self.focused_id = Some(field.response.id);
                                }
                                if field.commit {
                                    match state.place(side, value) {
                                        Ok(()) => {
                                            self.error = None;
                                            outcome.changed = true;
                                        }
                                        Err(refusal) => self.error = Some(refusal.message()),
                                    }
                                }
                                if let Some(message) = field.error {
                                    self.error = Some(message);
                                }
                                field.response.context_menu(|ui| {
                                    if ui.button(Message::DistanceCentred.text(lang)).clicked() {
                                        match state.centre() {
                                            Ok(_) => {
                                                self.error = None;
                                                outcome.changed = true;
                                            }
                                            Err(refusal) => self.error = Some(refusal.message()),
                                        }
                                        ui.close();
                                    }
                                });
                            });
                        }
                    });
                });
            outcome.rects.push(area.response.rect);
        }
        if let Some(side) = focused {
            state.activate(Some(side));
            self.suspended = false;
        }
        if escape && state.is_active() {
            state.cancel();
            self.focus = None;
            self.error = None;
            self.suspended = true;
            outcome.cancel = true;
            outcome.changed = true;
        } else if enter && let Some(side) = state.active() {
            match state.lock(side) {
                Ok(()) => {
                    self.error = None;
                    outcome.changed = true;
                }
                Err(refusal) => self.error = Some(refusal.message()),
            }
        }
        outcome
    }
}

/// The field nearest the pointer, or the first one when the pointer is elsewhere.
fn nearest(placements: &[FieldPlacement], ui: &Ui) -> Option<Side> {
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let mut best: Option<(f32, Side)> = None;
    for placement in placements {
        let distance = pointer.map_or(0.0, |pointer| placement.position.distance(pointer));
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, placement.side));
        }
    }
    best.map(|(_, side)| side)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn distances() -> Distances {
        // A 600 mm void, an 18 mm part sitting 100 mm from the low wall.
        Distances::new(Some(0.0), Some(600.0), 100.0, 118.0).expect("a usable measurement")
    }

    #[test]
    fn the_fields_show_the_two_gaps() {
        let state = distances();
        assert_eq!(state.value(Side::Low), Some(100.0));
        assert_eq!(state.value(Side::High), Some(482.0));
        assert_eq!(state.range(), 0.0..=600.0);
    }

    #[test]
    fn a_locked_face_is_the_reference_of_the_opposite_distance() {
        let mut state = distances();
        state.activate(Some(Side::Low));
        state.lock(Side::Low).expect("the low face has a neighbour");
        assert!(state.locked(Side::Low));
        // The high field now measures from the locked face to the high wall: 600 - 100, not the gap.
        assert_eq!(state.value(Side::High), Some(500.0));
        assert_eq!(state.reference(Side::High), 100.0);
        assert_eq!(state.value(Side::Low), Some(100.0));
    }

    #[test]
    fn typing_a_distance_moves_the_part_and_keeps_its_extent() {
        let mut state = distances();
        state.place(Side::Low, 250.0).expect("the part fits");
        assert_eq!(state.low(), 250.0);
        assert_eq!(state.high(), 268.0);
        assert_eq!(state.value(Side::High), Some(332.0));
        assert_eq!(state.pinned_low(), Some(250.0));
    }

    #[test]
    fn the_centred_value_is_the_midpoint_coordinate() {
        let mut state = distances();
        let low = state.centre().expect("both walls are known");
        assert_eq!(low, (600.0 - 18.0) * 0.5);
        assert_eq!(state.value(Side::Low), Some(state.value(Side::High).expect("the high wall")));
        assert_eq!(state.pinned_low(), Some(low), "the middle holds while the pointer moves");
    }

    #[test]
    fn a_missing_neighbour_disables_the_field() {
        let mut state = Distances::new(None, Some(600.0), 100.0, 118.0).expect("one wall is enough");
        assert_eq!(state.neighbour(Side::Low), None);
        assert_eq!(state.value(Side::Low), None);
        assert_eq!(state.place(Side::Low, 10.0), Err(DistanceRefusal::NoNeighbour));
        assert_eq!(DistanceRefusal::NoNeighbour.message(), Message::DistanceNoNeighbour);
    }

    #[test]
    fn an_impossible_value_is_refused_and_writes_nothing() {
        let mut state = distances();
        for value in [-1.0, 583.0, f64::NAN] {
            let before = state;
            assert!(state.place(Side::Low, value).is_err(), "{value} must be refused");
            assert_eq!(state, before, "a refusal changes nothing");
        }
    }

    #[test]
    fn a_locked_opposite_face_refuses_a_resizing_number() {
        let mut state = distances();
        state.lock(Side::High).expect("the high face has a neighbour");
        assert_eq!(state.place(Side::Low, 250.0), Err(DistanceRefusal::WouldResize));
        // The displayed number is the span from the locked face, not a gap: taking it would also
        // change the extent, so a fixed-size part refuses every number on that side.
        assert_eq!(
            state.place(Side::Low, state.value(Side::Low).expect("the low wall")),
            Err(DistanceRefusal::WouldResize)
        );
        assert_eq!(state.low(), 100.0);
        assert_eq!(state.high(), 118.0);
    }

    #[test]
    fn tab_moves_to_the_opposite_direction_and_skips_a_disabled_one() {
        let mut state = distances();
        state.activate(Some(Side::Low));
        assert_eq!(state.tab(), Some(Side::High));
        state.activate(Some(Side::High));
        assert_eq!(state.tab(), Some(Side::Low));
        let mut one_sided = Distances::new(Some(0.0), None, 100.0, 118.0).expect("one wall");
        one_sided.activate(Some(Side::Low));
        assert_eq!(one_sided.tab(), Some(Side::Low), "a direction without a neighbour is skipped");
    }

    #[test]
    fn escape_returns_the_direction_to_the_pointer() {
        let mut state = distances();
        state.activate(Some(Side::Low));
        state.place(Side::Low, 250.0).expect("the part fits");
        state.lock(Side::Low).expect("the face has a neighbour");
        assert_eq!(state.pinned_low(), Some(250.0));
        state.cancel();
        assert_eq!(state.active(), None);
        assert_eq!(state.pinned_low(), None);
        assert!(!state.locked(Side::Low));
        assert_eq!(state.low(), 250.0, "the part stays where the last number put it");
    }

    #[test]
    fn a_sync_keeps_the_pinned_face_and_follows_the_rest() {
        let mut state = distances();
        state.place(Side::Low, 250.0).expect("the part fits");
        state.sync(Some(0.0), Some(600.0), 400.0, 418.0);
        assert_eq!(state.low(), 250.0, "the pinned face does not follow the pointer");
        assert_eq!(state.high(), 268.0);
    }

    #[test]
    fn the_lock_glyph_and_the_focus_ring_are_drawn_offscreen() {
        let context = egui::Context::default();
        crate::set_language(&context, crate::Lang::En);
        let mut state = distances();
        state.activate(Some(Side::Low));
        state.lock(Side::Low).expect("the face has a neighbour");
        let placements = [
            FieldPlacement { side: Side::Low, position: Pos2::new(200.0, 300.0) },
            FieldPlacement { side: Side::High, position: Pos2::new(500.0, 300.0) },
        ];
        let mut fields = DistanceFields::default();
        let mut last = None;
        let mut locked = false;
        for _ in 0..2 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    last = Some(fields.show(ui, &mut state, &placements, 92));
                },
            );
            if let Some(outcome) = &last {
                locked = contains_image(&output.shapes, outcome.rects.first().copied());
            }
            output.drop_without_applying_deltas();
        }
        let outcome = last.expect("two frames drew the fields");
        assert_eq!(outcome.rects.len(), 2, "one plate per direction");
        assert_eq!(state.active(), Some(Side::Low), "exactly one field owns the keyboard");
        assert!(locked, "the locked field draws the lock glyph");
    }

    /// Whether any image sits inside `rect`, which is how the lock glyph reaches the screen.
    fn contains_image(shapes: &[egui::epaint::ClippedShape], rect: Option<egui::Rect>) -> bool {
        let Some(rect) = rect else {
            return false;
        };
        fn walk(shape: &egui::Shape, rect: egui::Rect) -> bool {
            match shape {
                egui::Shape::Mesh(mesh) => mesh.vertices.iter().all(|vertex| rect.contains(vertex.pos)),
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| walk(shape, rect)),
                _ => false,
            }
        }
        shapes.iter().any(|clipped| walk(&clipped.shape, rect))
    }
}
