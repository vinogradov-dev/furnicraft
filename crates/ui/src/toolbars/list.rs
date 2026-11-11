//! The variant list under a ribbon cell: every input method of one tool, one row each (U67).
//!
//! A tool with more than one way to be built draws a caret in the corner of its cell, and a click
//! opens this list instead of running anything; a tool with a single method runs on the click and
//! draws no caret. The list is the approved F1 anatomy: 298 points wide, a 34-point header with the
//! tool's glyph, its name and the key that runs the pinned method, 30-point rows carrying the glyph,
//! the method's name, its own key and a pin, optional 9.5-point section captions, and no footer.
//!
//! It is an [`egui::Popup`] anchored under the cell, so the window edge decides its side: below the
//! cell when there is room, above it at a short height, and right-aligned when the default side
//! would cross the window's right edge. The popup owns Escape and the outside click; a tab change
//! closes the list because the cell that owns it is no longer drawn ([`super::Toolbars`] drops the
//! state), and running a row closes it here. Pinning deliberately keeps it open: the accent moves to
//! the new row, which is the whole feedback the pin has.
//!
//! The open tool and the highlighted row are view state in the ribbon, never document data: opening
//! the list writes nothing, and picking a row only reports what the editor should run.

use egui::{Align, Layout, Popup, PopupCloseBehavior, RectAlign, Response, Sense, Stroke, Ui, UiBuilder, Vec2};
use furnicraft_text::{Lang, Message};

use super::ToolAction;
use super::spec::{Method, ToolSpec};
use crate::icons::{self, Icon};
use crate::theme;

/// The approved width of the list, in points.
pub const WIDTH: f32 = 298.0;

/// The header of the list: the tool's glyph, its name and its key.
pub const HEADER_HEIGHT: f32 = 34.0;

/// One method row.
pub const ROW_HEIGHT: f32 = 30.0;

/// The caption above a section of methods.
pub const CAPTION_HEIGHT: f32 = 16.0;

/// The glyph of one row, in points.
const ROW_ICON: f32 = 17.0;

/// The glyph of the header, in points.
const HEADER_ICON: f32 = 18.0;

/// The left and right air inside a row, in points.
const ROW_INSET: f32 = 10.0;

/// The accent bar that marks the pinned row, in points.
const PIN_BAR: f32 = 2.0;

/// The list one ribbon cell opened: which tool, and the row the keyboard has reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VariantList {
    /// The id of the tool whose methods the list draws.
    pub tool: &'static str,
    /// The row Enter runs; the arrow keys move it and it wraps at both ends.
    pub highlighted: usize,
}

impl VariantList {
    /// A list for `tool` with the first row highlighted.
    pub const fn new(tool: &'static ToolSpec) -> Self {
        Self { tool: tool.id, highlighted: 0 }
    }

    /// Moves the highlight `step` rows, wrapping at both ends.
    pub fn step(&mut self, step: isize, rows: usize) {
        if rows == 0 {
            self.highlighted = 0;
            return;
        }
        let rows = rows as isize;
        self.highlighted = (self.highlighted as isize + step).rem_euclid(rows) as usize;
    }
}

/// What one row of the list asked the ribbon to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListChoice {
    /// Run the method of that row and close the list.
    Run(usize),
    /// Pin the method of that row; the list stays open under the new accent.
    Pin(usize),
}

/// One drawn row: the method, the sentence it explains itself with, and what it would run.
pub struct Row {
    /// The method the map declares.
    pub method: Method,
    /// The hint of the row: the method's name and key, or why it cannot run.
    pub hint: String,
    /// Whether the method can run here.
    pub enabled: bool,
    /// The action a click asks the editor to run, unless the method cannot run.
    pub action: Option<ToolAction>,
}

/// Draws `tool`'s open list under `anchor` and reports what the pointer or the keyboard chose.
///
/// `slot` is the ribbon's view state: the function clears it when the list closes (Escape, an
/// outside click, a run row) so the caller's state never claims an invisible list. `pinned` is the
/// index of the pinned row, the only row that carries the accent.
pub fn show(
    ui: &mut Ui,
    slot: &mut Option<VariantList>,
    tool: &'static ToolSpec,
    rows: &[Row],
    anchor: &Response,
    pinned: usize,
    lang: Lang,
) -> Option<ListChoice> {
    let mut list = (*slot)?;
    let mut open = true;
    let mut chosen = None;
    Popup::from_response(anchor)
        .id(egui::Id::unique(("ribbon.variants", tool.id)))
        .open_bool(&mut open)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_START)
        .gap(2.0)
        .width(WIDTH)
        .frame(frame(ui))
        .show(|ui| {
            chosen = body(ui, &mut list, tool, rows, pinned, lang);
        });
    let still_open = open && !matches!(chosen, Some(ListChoice::Run(_)));
    *slot = still_open.then_some(list);
    chosen
}

/// The frame the list draws: the darkest chrome fill, one hairline and the corner of a floating card.
fn frame(ui: &Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::band(ui))
        .stroke(Stroke::new(1.0, theme::line(ui)))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::ZERO)
}

/// The header, the rows, the captions and the keyboard of one open list.
fn body(
    ui: &mut Ui,
    list: &mut VariantList,
    tool: &'static ToolSpec,
    rows: &[Row],
    pinned: usize,
    lang: Lang,
) -> Option<ListChoice> {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    header(ui, tool, lang);
    let mut chosen = None;
    let mut section = None;
    for (index, row) in rows.iter().enumerate() {
        if row.method.section != section {
            section = row.method.section;
            if let Some(text) = section {
                caption(ui, text, lang);
            }
        }
        chosen = method_row(ui, index, row, pinned, index == list.highlighted, lang).or(chosen);
    }
    keyboard(ui, list, rows).or(chosen)
}

/// The header: the tool's glyph, its name, and the key that runs the pinned method.
fn header(ui: &mut Ui, tool: &'static ToolSpec, lang: Lang) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, HEADER_HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let inner = rect.shrink2(Vec2::new(ROW_INSET, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)), |ui| {
        if let Some(shortcut) = tool.shortcut {
            ui.label(small(shortcut, theme::text_dim(ui)));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            glyph(ui, tool.icon, HEADER_ICON, theme::text(ui));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tool.label.text(lang)).size(12.0).color(theme::text(ui)));
        });
    });
    let bottom = egui::pos2(rect.left(), rect.bottom() - 0.5);
    ui.painter().hline(rect.x_range(), bottom.y, Stroke::new(1.0, theme::line(ui)));
}

/// One method row: the glyph, the name, the method's own key and the pin.
fn method_row(
    ui: &mut Ui,
    index: usize,
    row: &Row,
    pinned: usize,
    highlighted: bool,
    lang: Lang,
) -> Option<ListChoice> {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(WIDTH, ROW_HEIGHT), Sense::click());
    let pinned_here = index == pinned;
    let color = if row.enabled { theme::text(ui) } else { theme::text_dim(ui) };
    if ui.is_rect_visible(rect) {
        if pinned_here {
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, theme::control(ui));
            let bar = egui::Rect::from_min_size(rect.left_top(), Vec2::new(PIN_BAR, rect.height()));
            ui.painter().rect_filled(bar, egui::CornerRadius::ZERO, theme::accent(ui));
        } else if response.hovered() || highlighted {
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, theme::control_hover(ui));
        }
    }
    let mut pin = None;
    let inner = rect.shrink2(Vec2::new(ROW_INSET, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)), |ui| {
        if row.enabled {
            let hint = if pinned_here { Message::MethodPinned.text(lang) } else { Message::PinMethod.text(lang) };
            pin = Some(icons::icon_button_sized(ui, icons::actions::PIN, 20.0, 12.0, hint, true, pinned_here));
        }
        if let Some(shortcut) = row.method.shortcut {
            ui.label(small(shortcut, theme::text_dim(ui)));
            ui.add_space(6.0);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            glyph(ui, row.method.icon, ROW_ICON, color);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(row.method.label.text(lang)).size(11.5).color(color));
        });
    });
    if !row.enabled {
        return None;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.hint));
    if pin.is_some_and(|pin| pin.clicked()) {
        return Some(ListChoice::Pin(index));
    }
    response.clicked().then_some(ListChoice::Run(index))
}

/// A section caption in the list's small capitals, above the first row of its section.
fn caption(ui: &mut Ui, caption: Message, lang: Lang) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, CAPTION_HEIGHT), Sense::hover());
    let text = caption.text(lang).to_uppercase();
    let galley =
        ui.fonts_mut(|fonts| fonts.layout_no_wrap(text, egui::FontId::proportional(9.5), egui::Color32::PLACEHOLDER));
    let pos = egui::pos2(rect.left() + ROW_INSET, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, theme::text_dim(ui));
}

/// The arrow keys and Enter, read only while no text field owns the keyboard.
///
/// A ribbon field can hold the focus while the list is open, so the list never steals a key the user
/// is typing into; Escape is the popup's own and is not read here.
fn keyboard(ui: &mut Ui, list: &mut VariantList, rows: &[Row]) -> Option<ListChoice> {
    if ui.memory(|memory| memory.focused().is_some()) {
        return None;
    }
    let rows_len = rows.len();
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
        list.step(1, rows_len);
    }
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
        list.step(-1, rows_len);
    }
    let run = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let row = rows.get(list.highlighted)?;
    (run && row.enabled).then_some(ListChoice::Run(list.highlighted))
}

/// Paints a method's glyph and returns the space it took.
///
/// A method that has no artwork of its own keeps the same gap, so the names of a tool's methods stay
/// on one left edge instead of shifting by whether an icon was drawn.
fn glyph(ui: &mut Ui, icon: Option<Icon>, point_size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(point_size), Sense::hover());
    if let Some(icon) = icon.filter(|icon| icon.is_drawn()) {
        icons::draw_icon(ui, icon, rect, point_size, color);
    }
}

/// A key label in the list's small type.
fn small(text: &str, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(10.5).color(color)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn list() -> VariantList {
        VariantList { tool: "sketch.create.circle", highlighted: 0 }
    }

    #[test]
    fn the_arrow_keys_wrap_at_both_ends() {
        let mut list = list();
        list.step(1, 3);
        assert_eq!(list.highlighted, 1);
        list.step(1, 3);
        list.step(1, 3);
        assert_eq!(list.highlighted, 0, "the last row wraps to the first");
        list.step(-1, 3);
        assert_eq!(list.highlighted, 2, "the first row wraps to the last");
    }

    #[test]
    fn a_list_with_no_rows_keeps_the_highlight_on_the_first() {
        let mut list = VariantList { tool: "sketch.create.circle", highlighted: 2 };
        list.step(1, 0);
        assert_eq!(list.highlighted, 0);
    }

    #[test]
    fn one_row_is_the_fixed_anatomy_of_the_approved_mock() {
        assert_eq!(WIDTH, 298.0);
        assert_eq!(ROW_HEIGHT, 30.0);
        assert_eq!(HEADER_HEIGHT, 34.0);
        assert_eq!(CAPTION_HEIGHT, 16.0);
    }
}
