//! The variant list under a ribbon cell: every input method of one tool, one row each (U67).
//!
//! A tool with more than one way to be built draws a caret face in its cell, and a click on it opens
//! this list; the main face of the cell runs the pinned method and draws its glyph. The list is the
//! approved F1 anatomy: 298 points wide, a 34-point header with the tool's glyph, its name and the
//! key that runs the pinned method, 30-point rows carrying the glyph, the method's name, its own key
//! and a pin, optional 9.5-point section captions, and no footer.
//!
//! A row's columns are fixed for the whole list — glyph, name, key, pin, at least 6 points apart — so
//! a row without a key and a row with a long one keep the same name column, and the pin never sits on
//! the key (U74). The pin is not drawn at rest: it appears on the row under the pointer and on the row
//! the keyboard reached, while its column stays reserved, so nothing shifts when it appears. The
//! pinned row is marked by its accent bar and fill instead, which is what stays when the pin hides.
//!
//! It is an [`egui::Popup`] anchored under the cell, so the window edge decides its side: below the
//! cell when there is room, above it at a short height, and right-aligned when the default side
//! would cross the window's right edge. The popup owns Escape and the outside click; a tab change
//! closes the list because the cell that owns it is no longer drawn ([`super::Toolbars`] drops the
//! state), and running a row closes it here. Pinning deliberately keeps it open: the accent moves to
//! the new row, which is the whole feedback the pin has.
//!
//! The popup owns the pointer across its whole rectangle while it is drawn — the header, the rows,
//! the key column and the pin — and reports that rectangle as [`Shown::rect`] so the shell can treat
//! it as chrome over the scene. The cursor is claimed here, once for the whole popup, instead of by
//! each row: a row's own response covers only the part of it a control was drawn over, so the key
//! and the pin column fell through to whatever the viewport wrote for the frame (X37).
//!
//! The open tool and the highlighted row are view state in the ribbon, never document data: opening
//! the list writes nothing, and picking a row only reports what the editor should run.

use egui::{
    Align, Color32, Layout, Popup, PopupCloseBehavior, RectAlign, Response, Sense, Stroke, Ui, UiBuilder, Vec2,
};
use furnicraft_text::{Lang, Message};

use super::ToolAction;
use super::spec::{Method, ToolSpec};
use crate::icons::{self, Icon, IconTone};
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
pub(super) const ROW_INSET: f32 = 10.0;

/// The accent bar that marks the pinned row, in points.
const PIN_BAR: f32 = 2.0;

/// The air between two columns of a row, in points.
///
/// It is the smallest gap that keeps the pin off the key in either language, and the same gap holds
/// between the name and the key, so the row reads as four aligned columns (U74).
const COLUMN_GAP: f32 = 6.0;

/// The air between the glyph and the name of a row, in points.
const LABEL_GAP: f32 = 8.0;

/// The width of the pin's column, in points: the pin is a 20 x 12 control, and the column keeps that
/// width on every row whether the pin is drawn or not.
pub(super) const PIN_COLUMN: f32 = 20.0;

/// The point size of the pin, in points.
const PIN_POINT_SIZE: f32 = 12.0;

/// The point size of a row's key label, in points.
const KEY_POINT_SIZE: f32 = 10.5;

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

/// What one open list did on the frame it drew: the row it chose, and the rectangle it covered.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    /// The row the pointer or the keyboard chose, if any.
    pub choice: Option<ListChoice>,
    /// The rectangle the popup covered, while it is drawn.
    ///
    /// It is `None` on the frame the list is closed, and the shell keeps it only for the frame it
    /// was measured on: the popup is a surface over the scene, so a press or a tool cursor inside
    /// it belongs to the list and not to the viewport (X37).
    pub rect: Option<egui::Rect>,
}

/// One drawn row: the method, the sentence it explains itself with, and what it would run.
pub struct Row {
    /// The method the map declares.
    pub method: Method,
    /// The hint of the row: the method's name and key, or why it cannot run.
    pub hint: String,
    /// Whether the method can run here.
    pub enabled: bool,
    /// Whether the method owns the next click of the viewport, which the cell draws as pressed.
    pub armed: bool,
    /// The action a click asks the editor to run, unless the method cannot run.
    pub action: Option<ToolAction>,
}

/// The four columns of a row, in the order the eye reads them: the glyph, the name, the key and the
/// pin.
///
/// The widths are computed once for a whole list, so the name and the key of every row start and end
/// on the same edges and a row whose pin is hidden looks exactly like one whose pin is drawn (U74).
#[derive(Clone, Copy, Debug)]
pub(super) struct Columns {
    pub(super) glyph: egui::Rect,
    pub(super) label: egui::Rect,
    pub(super) shortcut: egui::Rect,
    pub(super) pin: egui::Rect,
}

/// Lays one row out from its right edge, so the pin and the key can never meet.
///
/// `shortcut` is the widest key of the whole list, measured once; a row with no key of its own keeps
/// the same name column as the widest one, which is what makes the columns look fixed.
pub(super) fn columns(rect: egui::Rect, shortcut_width: f32) -> Columns {
    let row = rect.shrink2(Vec2::new(ROW_INSET, 0.0));
    let pin_width = PIN_COLUMN.min(row.width());
    let shortcut_width = shortcut_width.min((row.width() - pin_width - COLUMN_GAP).max(0.0));
    let pin = egui::Rect::from_center_size(
        egui::pos2(row.right() - pin_width / 2.0, row.center().y),
        Vec2::new(pin_width, PIN_POINT_SIZE),
    );
    let shortcut_right = pin.left() - COLUMN_GAP;
    let shortcut = egui::Rect::from_min_max(
        egui::pos2(shortcut_right - shortcut_width, row.top()),
        egui::pos2(shortcut_right, row.bottom()),
    );
    let glyph =
        egui::Rect::from_min_size(egui::pos2(row.left(), row.center().y - ROW_ICON / 2.0), Vec2::splat(ROW_ICON));
    let label_left = glyph.right() + LABEL_GAP;
    let label = egui::Rect::from_min_max(
        egui::pos2(label_left, row.top()),
        egui::pos2((shortcut.left() - COLUMN_GAP).max(label_left), row.bottom()),
    );
    Columns { glyph, label, shortcut, pin }
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
) -> Shown {
    let Some(mut list) = *slot else {
        return Shown::default();
    };
    let mut open = true;
    let mut chosen = None;
    let drawn = Popup::from_response(anchor)
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
    let rect = if still_open { drawn.map(|drawn| drawn.response.rect) } else { None };
    claim_cursor(ui, rect);
    Shown { choice: chosen, rect }
}

/// Claims the frame's cursor for the whole popup while the pointer is inside it.
///
/// One rectangle read from the popup's own response, not one per row: the header, the name, the key
/// and the pin are one surface, and a pointer that changes shape as it crosses a column of it reads
/// as a control that only half exists (X37). The list is chrome, so its cursor is the arrow.
fn claim_cursor(ui: &Ui, rect: Option<egui::Rect>) {
    let inside = rect.is_some_and(|rect| ui.ctx().pointer_hover_pos().is_some_and(|pointer| rect.contains(pointer)));
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }
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
    let shortcut_width = shortcut_column(ui, rows);
    let mut chosen = None;
    let mut section = None;
    for (index, row) in rows.iter().enumerate() {
        if row.method.section != section {
            section = row.method.section;
            if let Some(text) = section {
                caption(ui, text, lang);
            }
        }
        chosen = method_row(ui, index, row, pinned, index == list.highlighted, shortcut_width, lang).or(chosen);
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

/// The width of the widest key label in the list, measured once so every row's key column is the same.
pub(super) fn shortcut_column(ui: &Ui, rows: &[Row]) -> f32 {
    rows.iter().filter_map(|row| row.method.shortcut).fold(0.0, |widest, key| {
        let width =
            ui.fonts_mut(|fonts| fonts.layout_no_wrap(key.to_owned(), key_font(), Color32::PLACEHOLDER).size().x);
        widest.max(width)
    })
}

/// One method row: the glyph, the name, the method's own key and the pin.
///
/// The pin is a control that appears under the pointer and on the row the keyboard reached; its
/// column is reserved on every row, and it is only interactive while it is drawn, so an invisible pin
/// can never swallow the click that runs the method (U74).
fn method_row(
    ui: &mut Ui,
    index: usize,
    row: &Row,
    pinned: usize,
    highlighted: bool,
    shortcut_width: f32,
    lang: Lang,
) -> Option<ListChoice> {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(WIDTH, ROW_HEIGHT), Sense::click());
    let pinned_here = index == pinned;
    // The row is hot under the pointer or on the keyboard's row. It is read from the rect rather than
    // from the row's own response because the pin is a second control inside the row: the row stays
    // hot, and the pin stays where the pointer found it, as long as the pointer is over the row (U74).
    let hot = highlighted || ui.rect_contains_pointer(rect);
    let show_pin = row.enabled && hot;
    let columns = columns(rect, shortcut_width);
    let color = if row.enabled { theme::text(ui) } else { theme::text_dim(ui) };
    if ui.is_rect_visible(rect) {
        if pinned_here {
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, theme::control(ui));
            let bar = egui::Rect::from_min_size(rect.left_top(), Vec2::new(PIN_BAR, rect.height()));
            ui.painter().rect_filled(bar, egui::CornerRadius::ZERO, theme::accent(ui));
        } else if hot {
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, theme::control_hover(ui));
        }
        paint_row(ui, row, &columns, color, show_pin, pinned_here, lang);
    }
    let mut pin = None;
    if row.enabled {
        let hint = if pinned_here { Message::MethodPinned.text(lang) } else { Message::PinMethod.text(lang) };
        let sense = if show_pin { Sense::click() } else { Sense::hover() };
        let button = ui.interact(columns.pin, response.id.with("pin"), sense);
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, show_pin, hint));
        pin = Some(button);
    }
    if !row.enabled {
        return None;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.hint));
    if pin.is_some_and(|pin| pin.clicked()) {
        return Some(ListChoice::Pin(index));
    }
    response.clicked().then_some(ListChoice::Run(index))
}

/// Paints the four columns of one row: the glyph, the name cut with an ellipsis, the key and the pin.
fn paint_row(ui: &Ui, row: &Row, columns: &Columns, color: Color32, show_pin: bool, pinned_here: bool, lang: Lang) {
    if let Some(icon) = row.method.icon.filter(|icon| icon.is_drawn()) {
        icons::paint_icon(ui, icon, columns.glyph, ROW_ICON, color);
    }
    let galley = label_galley(ui, row.method.label.text(lang), columns.label.width(), color);
    let pos = egui::pos2(columns.label.left(), columns.label.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
    if let Some(shortcut) = row.method.shortcut {
        ui.painter().text(
            columns.shortcut.right_center(),
            egui::Align2::RIGHT_CENTER,
            shortcut,
            key_font(),
            theme::text_dim(ui),
        );
    }
    if show_pin {
        let (ink, tone) =
            if pinned_here { (theme::accent(ui), IconTone::Selected) } else { (theme::text(ui), IconTone::Hover) };
        icons::paint_icon_toned(ui, icons::actions::PIN, columns.pin, PIN_POINT_SIZE, ink, tone);
    }
}

/// The name of a method laid out in its own column, cut with an ellipsis instead of reaching the key.
fn label_galley(ui: &Ui, text: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat { font_id: egui::FontId::proportional(11.5), color, ..Default::default() };
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
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
/// The inspector can hold the focus while the list is open, so the list never steals a key the user
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
    egui::RichText::new(text).size(KEY_POINT_SIZE).color(color)
}

/// The font of a key label.
fn key_font() -> egui::FontId {
    egui::FontId::proportional(KEY_POINT_SIZE)
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
