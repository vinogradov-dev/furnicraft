//! The class picker: the registry tree behind one search field.
//!
//! The inspector assigns a semantic class from the registry instead of typing six digits
//! (`docs/domain/classes.md` §8). The picker lists the tree the registry already has — family,
//! type, variant — with an explicit unclassified row at the top, filters it by a person's own words
//! (label in either language, slug, alias or code) and marks the class the node carries now. A
//! retired row still renders wherever it is stored but is never offered here.
//!
//! The picker is a view of the registry, not a second way into the document: it reports the chosen
//! [`ClassCode`] and the property card dispatches the one `node.property.set` with the reserved
//! `class` key. Its query and open state are view state of the card and leave no trace in a file.

use egui::{
    Align, Button, Key, Popup, PopupCloseBehavior, RectAlign, Response, RichText, ScrollArea, Stroke, TextEdit, Ui,
    Vec2,
};
use furnicraft_model::{ClassCode, ClassRow};
use furnicraft_text::{Lang, Message};

use crate::theme;

/// The approved width of the picker, in points.
const WIDTH: f32 = 300.0;

/// The height of one class row, in points.
const ROW_HEIGHT: f32 = 22.0;

/// How far one tree level indents its row, in points.
const INDENT: f32 = 12.0;

/// The tallest the list grows before it scrolls.
const MAX_HEIGHT: f32 = 320.0;

/// The picker's view state: whether it is open and what the person typed.
///
/// It lives in the inspector card, never in the session or the document.
#[derive(Debug, Default)]
pub(crate) struct ClassPicker {
    open: bool,
    query: String,
    focus_pending: bool,
    scroll_to_current: bool,
}

impl ClassPicker {
    /// Opens the picker for the class the card is showing, with a fresh search.
    pub(crate) fn start(&mut self) {
        self.open = true;
        self.query.clear();
        self.focus_pending = true;
        self.scroll_to_current = true;
    }

    /// Closes the picker without assigning anything.
    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// Draws the open picker under `anchor` and reports the one class chosen this frame.
    ///
    /// `rows` is the registry, passed in so a test can list a retired or a synthetic row without
    /// changing the shipped table.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        anchor: &Response,
        current: ClassCode,
        rows: &[ClassRow],
    ) -> Option<ClassCode> {
        if !self.open {
            return None;
        }
        let lang = crate::language(ui.ctx());
        let mut open = self.open;
        let mut chosen = None;
        Popup::from_response(anchor)
            .id(egui::Id::unique("properties.class_picker"))
            .open_bool(&mut open)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .align(RectAlign::BOTTOM_START)
            .gap(2.0)
            .width(WIDTH)
            .frame(frame(ui))
            .show(|ui| {
                chosen = body(ui, self, current, rows, lang);
            });
        self.open = open && chosen.is_none();
        chosen
    }
}

/// One family heading and the classes of it the query leaves visible.
#[derive(Debug)]
struct FamilyRows<'a> {
    family: &'a ClassRow,
    classes: Vec<(&'a ClassRow, u8)>,
}

/// The picker's body: one search field, the unclassified row and the filtered tree.
fn body(ui: &mut Ui, picker: &mut ClassPicker, current: ClassCode, rows: &[ClassRow], lang: Lang) -> Option<ClassCode> {
    let search = ui.add(
        TextEdit::singleline(&mut picker.query)
            .hint_text(Message::ClassPickerSearch.text(lang))
            .desired_width(f32::INFINITY),
    );
    if picker.focus_pending {
        search.request_focus();
        picker.focus_pending = false;
    }
    let listed = visible_rows(rows, &picker.query, lang);
    let matches = listed.iter().map(|family| family.classes.len()).sum::<usize>();
    let mut chosen = None;

    let mut scroll = picker.scroll_to_current;
    ui.separator();
    ui.spacing_mut().item_spacing.y = 2.0;
    ScrollArea::vertical().max_height(MAX_HEIGHT).show(ui, |ui| {
        if class_row(ui, 0, Message::Unclassified.text(lang), ClassCode::UNCLASSIFIED, current, &mut scroll) {
            chosen = Some(ClassCode::UNCLASSIFIED);
        }
        if matches == 0 {
            ui.label(RichText::new(Message::ClassPickerNoMatch.text(lang)).color(theme::text_dim(ui)));
            return;
        }
        ui.separator();
        for family in &listed {
            family_caption(ui, family.family, lang);
            for (row, depth) in &family.classes {
                let Some(code) = ClassCode::from_code(row.code()) else { continue };
                let text = format!("{code}   {}", row.label().text(lang));
                if class_row(ui, *depth, &text, code, current, &mut scroll) {
                    chosen = Some(code);
                }
            }
        }
    });
    picker.scroll_to_current = false;

    // A search narrows the tree; Enter takes the first row it leaves, which is the broadest match
    // in registry order. With no query there is no obvious first choice, so Enter does nothing.
    if chosen.is_none()
        && matches > 0
        && !picker.query.trim().is_empty()
        && ui.input(|input| input.key_pressed(Key::Enter))
    {
        chosen = listed
            .iter()
            .flat_map(|family| family.classes.iter())
            .next()
            .and_then(|(row, _)| ClassCode::from_code(row.code()));
    }
    chosen
}

/// One clickable row: indented by its tree level and marked when it is the current class.
fn class_row(ui: &mut Ui, depth: u8, text: &str, code: ClassCode, current: ClassCode, scroll: &mut bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_space(f32::from(depth) * INDENT);
        let width = ui.available_width();
        let response = ui.add(
            Button::new(RichText::new(text))
                .selected(code == current)
                .truncate()
                .min_size(Vec2::new(width, ROW_HEIGHT)),
        );
        if *scroll && code == current {
            response.scroll_to_me(Some(Align::Center));
            *scroll = false;
        }
        let response = if code.is_unclassified() { response } else { response.on_hover_text(code.to_string()) };
        response.clicked()
    })
    .inner
}

/// A family heading: not assignable, so it reads as a caption instead of a row.
fn family_caption(ui: &mut Ui, family: &ClassRow, lang: Lang) {
    let code = ClassCode::from_code(family.code()).map(|code| code.to_string()).unwrap_or_default();
    ui.add_space(4.0);
    ui.label(RichText::new(format!("{code}   {}", family.label().text(lang))).strong().color(theme::text_dim(ui)));
}

/// The families and classes a query leaves visible, in registry order.
///
/// A family or a type that matches brings its children with it, so a person who types `Хранение`
/// or `storage` still sees the cabinet rows; a class is listed only when it is assignable at all.
/// A retired row is never listed, although it still renders where a document already stores it.
fn visible_rows<'a>(rows: &'a [ClassRow], query: &str, lang: Lang) -> Vec<FamilyRows<'a>> {
    let mut families = Vec::new();
    for family in rows.iter().filter(|row| row.is_family()) {
        let Some(family_code) = ClassCode::from_code(family.code()) else { continue };
        let family_matches = matches(family, query, lang);
        let mut classes = Vec::new();
        for type_row in rows.iter().filter(|row| row.is_assignable() && row.parent() == Some(family_code)) {
            let Some(type_code) = ClassCode::from_code(type_row.code()) else { continue };
            let type_matches = family_matches || matches(type_row, query, lang);
            let variants = rows.iter().filter(|row| {
                row.is_assignable() && row.parent() == Some(type_code) && (type_matches || matches(row, query, lang))
            });
            let variants = variants.collect::<Vec<_>>();
            if type_matches || !variants.is_empty() {
                classes.push((type_row, 1));
                classes.extend(variants.into_iter().map(|variant| (variant, 2)));
            }
        }
        if !classes.is_empty() {
            families.push(FamilyRows { family, classes });
        }
    }
    families
}

/// Whether `row` answers `query` by its label in either language, its slug, an alias or its code.
fn matches(row: &ClassRow, query: &str, lang: Lang) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let other = match lang {
        Lang::En => Lang::Ru,
        Lang::Ru => Lang::En,
    };
    row.label().text(lang).to_lowercase().contains(&query)
        || row.label().text(other).to_lowercase().contains(&query)
        || row.slug().to_lowercase().contains(&query)
        || row.aliases().iter().any(|alias| alias.to_lowercase().contains(&query))
        || ClassCode::from_code(row.code()).is_some_and(|code| code.to_string().contains(&query))
}

/// The popup's frame: the darkest chrome fill, one hairline and the corner of a floating card.
fn frame(ui: &Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::band(ui))
        .stroke(Stroke::new(1.0, theme::line(ui)))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(6))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Extent, Level, Measure, Mount, Nesting, OperationSet, Supply, TraitSet};
    use furnicraft_text::Message;

    use super::*;

    fn listed<'a>(rows: &'a [ClassRow], query: &str, lang: Lang) -> Vec<&'a ClassRow> {
        visible_rows(rows, query, lang)
            .into_iter()
            .flat_map(|family| family.classes.into_iter().map(|(row, _)| row))
            .collect()
    }

    fn shelf() -> ClassCode {
        "30.11".parse().expect("the horizontal panel code")
    }

    #[test]
    fn a_label_a_slug_and_an_alias_all_find_the_same_row() {
        for query in ["полка", "полочка", "shelf", "horizontal-panel", "горизонтальная панель"]
        {
            let rows = listed(furnicraft_model::registry(), query, Lang::Ru);
            assert!(
                rows.iter().any(|row| row.code() == shelf().value()),
                "`{query}` does not find the horizontal panel: {:?}",
                rows.iter().map(|row| row.slug()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn a_family_name_brings_its_classes_and_a_type_brings_its_variants() {
        let storage = listed(furnicraft_model::registry(), "Хранение", Lang::Ru);
        assert!(storage.iter().any(|row| row.slug() == "base-cabinet"));
        assert!(storage.iter().any(|row| row.slug() == "horizontal-panel"));
        assert!(!storage.iter().any(|row| row.slug() == "chair"));

        let cup_hinge = listed(furnicraft_model::registry(), "cup hinge", Lang::En);
        assert!(cup_hinge.iter().any(|row| row.slug() == "cup-hinge"));
        assert!(cup_hinge.iter().any(|row| row.slug() == "cup-hinge-overlay"));
    }

    #[test]
    fn a_retired_class_is_hidden_from_the_tree() {
        let rows = [
            ClassRow::family(200_000, "lying", Message::ClassFamilyLying),
            ClassRow::class(
                200_100,
                "bed",
                Message::ClassBed,
                TraitSet::new(
                    Supply::MadeInShop,
                    Measure::Count,
                    Nesting::Never,
                    Extent::Free,
                    Level::Product,
                    None,
                    Some(Mount::Floor),
                    None,
                    OperationSet::of(&[]),
                ),
                &[],
            )
            .with_aliases(&["bed"])
            .retired(),
        ];
        assert!(listed(&rows, "bed", Lang::En).is_empty(), "a retired row is never offered");
        assert_eq!(rows[1].label(), Message::ClassBed, "but it still renders under its label");
    }

    #[test]
    fn the_picker_draws_the_filtered_tree_and_marks_the_current_class() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        crate::set_language(&ctx, Lang::Ru);
        let mut picker = ClassPicker::default();
        picker.start();
        picker.query = "полка".to_owned();

        let screen = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..2 {
            let output = ctx.run_ui(screen.clone(), |ui| {
                let anchor = ui.add(Button::new("Class"));
                let chosen = picker.show(ui, &anchor, ClassCode::UNCLASSIFIED, furnicraft_model::registry());
                assert!(chosen.is_none(), "drawing the picker assigns nothing");
            });
            texts = drawn_text(&output.shapes);
            output.drop_without_applying_deltas();
        }

        assert!(texts.iter().any(|text| text.contains("Горизонтальная панель")), "the row is drawn: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("30.11")), "the code is visible: {texts:?}");
        assert!(texts.iter().any(|text| text == "без классификации"), "the unclassified row is explicit: {texts:?}");
        assert!(!texts.iter().any(|text| text.contains("Стул")), "the search hides the rest: {texts:?}");
        assert!(picker.open, "drawing keeps the picker open");
    }

    #[test]
    fn escape_closes_the_picker_without_choosing() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        crate::set_language(&ctx, Lang::En);
        let mut picker = ClassPicker::default();
        picker.start();

        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                events: vec![escape.clone()],
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let anchor = ui.add(Button::new("Class"));
                let chosen = picker.show(ui, &anchor, ClassCode::UNCLASSIFIED, furnicraft_model::registry());
                assert!(chosen.is_none());
            });
            output.drop_without_applying_deltas();
        }
        assert!(!picker.open, "Escape closes the picker");
    }

    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        use egui::epaint::Shape;
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
}
