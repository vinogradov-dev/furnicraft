//! The bar a previewing operation shows over the viewport's bottom centre.
//!
//! One row: the command's name, the shared 76 pt field with its unit *outside* the box, and the
//! ✓ / ✕ pair. The bar writes nothing: it reports the button pressed this frame and leaves the
//! preview to the caller, and its field has no undo scope, because a preview is session state and
//! not an edit. The check is idempotent because the operation behind it is: once it has applied,
//! the bar is gone and a second click has nothing to hit.
//!
//! Enter applies and Escape cancels, exactly like the two buttons, so a pull can be finished
//! without leaving the keyboard. The field is read first, so a height typed into it is committed by
//! the field before the check runs.
//!
//! The bar is a `Middle`-layer area at the viewport's bottom centre, where the view cluster also
//! sits; the shell lifts the cluster by [`BAR_HEIGHT`] while a bar is up so the two never overlap.

use core::ops::RangeInclusive;

use egui::{Align2, CornerRadius, Id, Margin, Order, Rect, Stroke, Ui};

use crate::icons::{actions, icon_button};
use crate::scrub::Scrub;
use crate::theme;
use crate::{Message, language};

/// The height of the bar's frame, so the shell can lift the view cluster clear of it.
pub const BAR_HEIGHT: f32 = 34.0;

/// What the bar's buttons asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandBarAction {
    /// ✓ or Enter: the caller applies the preview, once.
    Apply,
    /// ✕ or Escape: the caller drops the preview and restores the pre-command state.
    Cancel,
}

/// What one bar draws.
pub struct CommandBar<'a> {
    /// The command's name, as the registry labels it.
    pub label: Message,
    /// The value the field edits, in the bar's unit.
    pub value: &'a mut f64,
    /// The unit, drawn dim outside the box: it is never part of the editable text.
    pub unit: &'a str,
    /// The range the field stays inside.
    pub range: RangeInclusive<f64>,
    /// The digits the field shows.
    pub decimals: usize,
    /// Whether the field takes the keyboard focus this frame, so a fresh bar can be typed into
    /// without a click. The caller clears it after the frame.
    pub focus: bool,
}

/// What one frame of the bar did: the button it reported and the rectangle it drew in.
///
/// The rectangle is the shell's, so a press on the bar is chrome and never a pick: the bar floats
/// over the scene, and the pointer that lands on its field must not also reach the viewport.
#[derive(Clone, Copy, Debug)]
pub struct CommandBarOutcome {
    /// The button pressed this frame, if any.
    pub action: Option<CommandBarAction>,
    /// The rectangle the bar occupied.
    pub rect: Rect,
}

/// Draws the bar over `viewport` and reports the button pressed this frame.
pub fn show(ui: &mut Ui, viewport: Rect, bar: CommandBar<'_>, opacity: u32) -> CommandBarOutcome {
    let CommandBar { label, value, unit, range, decimals, focus } = bar;
    let lang = language(ui.ctx());
    let fill = theme::at_opacity(theme::floating(ui), opacity.clamp(80, 100));
    let mut action = None;
    let anchor = egui::pos2(viewport.center().x, viewport.bottom() - theme::FLOATING_OFFSET);
    let rect = egui::Area::new(Id::unique("command_bar"))
        .order(Order::Middle)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(anchor)
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(fill)
                .stroke(Stroke::new(1.0, theme::line(ui)))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::symmetric(8, 6));
            frame.show(ui, |ui| {
                // The bar floats over the viewport, so it may never grow wider than the surface it
                // covers: a narrow window folds the row instead of clipping it.
                ui.set_max_width((viewport.width() - 2.0 * theme::FLOATING_OFFSET).max(0.0));
                ui.horizontal(|ui| {
                    ui.label(label.text(lang));
                    let outcome =
                        Scrub::new(value).speed(0.5).range(range).decimals(decimals).unit(unit).focus(focus).show(ui);
                    if let Some(error) = outcome.error {
                        ui.colored_label(theme::danger(ui), error.text(lang));
                    }
                    ui.add_space(4.0);
                    if icon_button(ui, actions::CONFIRM, Message::ApplyEnter.text(lang)).clicked() {
                        action = Some(CommandBarAction::Apply);
                    }
                    if icon_button(ui, actions::CANCEL, Message::CancelEsc.text(lang)).clicked() {
                        action = Some(CommandBarAction::Cancel);
                    }
                });
            });
        })
        .response
        .rect;
    // The keys are read after the field drew, so Enter commits the typed height before the check
    // takes it. A cancel outranks an apply when both arrive in one frame.
    if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
        action = Some(CommandBarAction::Apply);
    }
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(CommandBarAction::Cancel);
    }
    CommandBarOutcome { action, rect }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use egui::{Pos2, Vec2};

    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    /// Draws one bar frame with `events` and returns what it asked for.
    fn run(events: Vec<egui::Event>, value: &mut f64, focus: bool) -> Option<CommandBarAction> {
        let ctx = egui::Context::default();
        crate::set_language(&ctx, crate::Lang::En);
        let mut action = None;
        let input = egui::RawInput { screen_rect: Some(viewport()), events, ..Default::default() };
        let output = ctx.run_ui(input, |ui| {
            action = show(
                ui,
                viewport(),
                CommandBar {
                    label: Message::CmdSetExtrusionHeight,
                    value,
                    unit: "mm",
                    range: 1.0..=10_000.0,
                    decimals: 3,
                    focus,
                },
                92,
            )
            .action;
        });
        output.drop_without_applying_deltas();
        action
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn a_bar_at_rest_asks_for_nothing() {
        let mut value = 120.0;
        assert_eq!(run(Vec::new(), &mut value, false), None, "drawing the bar applies nothing");
        assert_eq!(value, 120.0, "the field writes only what the user typed or dragged");
    }

    #[test]
    fn enter_applies_and_escape_cancels() {
        let mut value = 120.0;
        assert_eq!(run(vec![key(egui::Key::Enter)], &mut value, false), Some(CommandBarAction::Apply));
        assert_eq!(run(vec![key(egui::Key::Escape)], &mut value, false), Some(CommandBarAction::Cancel));
    }

    #[test]
    fn the_unit_stays_outside_the_editable_text() {
        // The field's buffer is the number alone: a click in the box selects the digits, never `mm`.
        // An `Area` measures itself on its first frame, so the text it draws is read on the second.
        let mut value = 120.0;
        let ctx = egui::Context::default();
        crate::set_language(&ctx, crate::Lang::En);
        let mut texts = Vec::new();
        let mut rect = Rect::NOTHING;
        for _ in 0..2 {
            let output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport()), ..Default::default() }, |ui| {
                let outcome = show(
                    ui,
                    viewport(),
                    CommandBar {
                        label: Message::CmdSetExtrusionHeight,
                        value: &mut value,
                        unit: "mm",
                        range: 1.0..=10_000.0,
                        decimals: 3,
                        focus: false,
                    },
                    92,
                );
                rect = outcome.rect;
            });
            texts = texts_of(&output);
            output.drop_without_applying_deltas();
        }
        assert!(texts.iter().any(|text| text == "120"), "the number is drawn in the box: {texts:?}");
        assert!(texts.iter().any(|text| text == "mm"), "the unit is drawn beside it: {texts:?}");
        assert!(!texts.iter().any(|text| text.contains("120 mm")), "the unit is never part of the number");
        assert!(rect.width() > 0.0 && viewport().contains_rect(rect), "the bar stays inside the view it covers");
    }

    /// Every text the frame painted.
    fn texts_of(output: &egui::FullOutput) -> Vec<String> {
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, texts);
                    }
                }
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }
}
