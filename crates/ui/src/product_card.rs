//! The card that creates a product frame: three sides and a type.
//!
//! A maker starts a piece by saying how big its reserved space is and what the piece is
//! (ADR 0100). The card is the numeric path to `product.create`: it draws the three sides in whole
//! millimetres, offers the registry's product classes behind one search field, and returns a
//! [`ProductDraft`]. It never dispatches a command and never touches the document — the editor
//! turns the draft into `product.create` and then opens the new frame as a session view operation
//! (ADR 0098).
//!
//! The drawn gesture (a base rectangle and a pulled height) reaches the same command when the box
//! tool lands (ADR 0101); this card is not a second creation path, only the typed one.
//!
//! Changes when: the product frame gains a field, the class list changes how it is searched, or the
//! numeric defaults move.

use egui::{Button, Key, Modal, RichText, ScrollArea, TextEdit, Vec2};
use furnicraft_model::{ClassCode, Envelope};
use furnicraft_text::Message;
use furnicraft_units::Length;

use crate::theme;

/// The width the card draws at, in points.
const WIDTH: f32 = 380.0;

/// The height one class row takes, in points.
const ROW_HEIGHT: f32 = 22.0;

/// How far one class level indents its row, in points.
const INDENT: f32 = 12.0;

/// The tallest the class list grows before it scrolls, in points.
const MAX_LIST_HEIGHT: f32 = 220.0;

/// The width of a new frame's base, in millimetres, until the maker types another.
pub const DEFAULT_WIDTH_MM: f64 = 600.0;

/// The depth of a new frame's base, in millimetres.
pub const DEFAULT_DEPTH_MM: f64 = 560.0;

/// The height of a new frame, in millimetres.
pub const DEFAULT_HEIGHT_MM: f64 = 720.0;

/// The largest side the field offers: the envelope's own ceiling, in whole millimetres (ADR 0097).
fn max_side_mm() -> f64 {
    Envelope::MAX_SIDE.micrometres() as f64 / 1_000.0
}

/// What the maker asked for, ready for `product.create`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductDraft {
    /// The authored width, depth and height of the reserved box.
    pub envelope: Envelope,
    /// The product class; the general-product placeholder until another is chosen.
    pub class: ClassCode,
}

/// The open card and what the maker has typed into it.
///
/// The three numbers and the query are drafts: they leave no trace in the document, and a cancelled
/// card keeps them for the next opening.
#[derive(Debug)]
pub struct ProductCard {
    open: bool,
    width: f64,
    depth: f64,
    height: f64,
    class: ClassCode,
    query: String,
}

impl Default for ProductCard {
    fn default() -> Self {
        Self {
            open: false,
            width: DEFAULT_WIDTH_MM,
            depth: DEFAULT_DEPTH_MM,
            height: DEFAULT_HEIGHT_MM,
            class: ClassCode::GENERAL_PRODUCT,
            query: String::new(),
        }
    }
}

impl ProductCard {
    /// Creates a closed card with the default sides and the placeholder class.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the card with a fresh search; the sides and the class stay as they were left.
    pub fn start(&mut self) {
        self.open = true;
        self.query.clear();
    }

    /// Whether the card is up, so the window can keep the keys for it.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Closes the card without creating anything.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The draft the fields describe, or `None` when a side is not a valid envelope.
    pub fn draft(&self) -> Option<ProductDraft> {
        let side = |millimetres: f64| Length::from_millimetres(millimetres.round() as i64);
        let envelope = Envelope::new(side(self.width)?, side(self.depth)?, side(self.height)?).ok()?;
        Some(ProductDraft { envelope, class: self.class })
    }

    /// Draws the open card and reports the draft the maker confirmed.
    ///
    /// Escape closes the card without a draft; Enter (while no field owns the keyboard) and the
    /// *Create* button confirm it. The card stays open when the draft cannot be built, so a bad
    /// number is fixed in place instead of dropping the work.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ProductDraft> {
        if !self.open {
            return None;
        }
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.open = false;
            return None;
        }
        let lang = crate::language(ctx);
        let mut chosen = None;
        Modal::new(egui::Id::unique("product-frame-card")).show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.heading(Message::ProductCardTitle.text(lang));
            ui.label(RichText::new(Message::ProductCardHint.text(lang)).color(theme::text_dim(ui)));
            ui.separator();

            // Three sides in whole millimetres: the field rounds a drag and a typed value alike to
            // the step the document stores, so a frame is never half a millimetre out.
            for (label, value) in [
                (Message::Width, &mut self.width),
                (Message::Depth, &mut self.depth),
                (Message::Height, &mut self.height),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label.text(lang));
                    crate::scrub::Scrub::new(value)
                        .speed(0.5)
                        .range(1.0..=max_side_mm())
                        .decimals(0)
                        .unit("mm")
                        .show(ui);
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(Message::PropertyKindClass.text(lang));
                // The chosen class is named beside the list, so the placeholder it defaults to is
                // visible even while the list is scrolled away from its row.
                let current = self.class.label().map(|label| label.text(lang)).unwrap_or_default();
                ui.label(RichText::new(current).strong());
            });
            ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text(Message::ClassPickerSearch.text(lang))
                    .desired_width(f32::INFINITY),
            );
            let rows = crate::class_picker::product_rows(&self.query, lang);
            ui.spacing_mut().item_spacing.y = 2.0;
            ScrollArea::vertical().max_height(MAX_LIST_HEIGHT).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new(Message::ClassPickerNoMatch.text(lang)).color(theme::text_dim(ui)));
                    return;
                }
                for (code, text, depth) in rows {
                    ui.horizontal(|ui| {
                        ui.add_space(f32::from(depth.saturating_sub(1)) * INDENT);
                        let row = ui.add(
                            Button::new(text)
                                .selected(code == self.class)
                                .truncate()
                                .min_size(Vec2::new(WIDTH - INDENT, ROW_HEIGHT)),
                        );
                        if row.clicked() {
                            self.class = code;
                        }
                    });
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                let cancel = ui.button(Message::Cancel.text(lang));
                if cancel.clicked() {
                    self.open = false;
                }
                let create = ui.add_enabled(self.draft().is_some(), Button::new(Message::GroupCreate.text(lang)));
                let entered = ui.ctx().input(|input| input.key_pressed(Key::Enter)) && !ui.ctx().text_edit_focused();
                if (create.clicked() || entered) && self.draft().is_some() {
                    chosen = self.draft();
                }
            });
        });
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_text::Lang;

    use super::*;

    fn screen() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        }
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

    /// Draws the card for a few frames and returns every text the last one painted.
    fn present(card: &mut ProductCard, lang: Lang) -> (Vec<String>, Option<ProductDraft>) {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        crate::set_language(&ctx, lang);
        let mut draft = None;
        let mut texts = Vec::new();
        for _ in 0..2 {
            let output = ctx.run_ui(screen(), |_ui| {
                draft = card.show(&ctx);
            });
            texts = drawn_text(&output.shapes);
            output.drop_without_applying_deltas();
        }
        (texts, draft)
    }

    #[test]
    fn the_card_asks_for_three_sides_and_lists_the_product_classes() {
        let mut card = ProductCard::new();
        card.start();
        let (texts, draft) = present(&mut card, Lang::En);

        assert!(draft.is_none(), "drawing the card creates nothing");
        assert!(texts.iter().any(|text| text == "New product frame"), "the card names itself: {texts:?}");
        for label in ["Width", "Depth", "Height"] {
            assert!(texts.iter().any(|text| text == label), "the card asks for {label}: {texts:?}");
        }
        assert!(
            texts.iter().any(|text| text.contains("General product")),
            "the placeholder class is listed: {texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Class"), "the list is captioned: {texts:?}");
        assert!(texts.iter().any(|text| text == "Create"), "the card can be confirmed: {texts:?}");
    }

    #[test]
    fn a_search_narrows_the_class_list() {
        let mut card = ProductCard::new();
        card.start();
        card.query = "no such product".to_owned();
        let (texts, _) = present(&mut card, Lang::En);
        assert!(texts.iter().any(|text| text == "No class matches."), "a query with no answer says so: {texts:?}");
        assert!(!texts.iter().any(|text| text.starts_with("80.01")), "and hides the rows: {texts:?}");

        card.query = "general".to_owned();
        let (texts, _) = present(&mut card, Lang::En);
        assert!(
            texts.iter().any(|text| text.starts_with("80.01") && text.contains("General product")),
            "a matching query brings the row back: {texts:?}"
        );
    }

    #[test]
    fn the_draft_carries_the_three_whole_millimetre_sides_and_the_placeholder_class() {
        let mut card = ProductCard::new();
        card.start();
        card.width = 599.6;
        card.height = 2000.0;
        let (_, draft) = present(&mut card, Lang::En);
        assert!(draft.is_none(), "presenting alone creates nothing");

        let draft = card.draft().expect("the defaults are a valid envelope");
        assert_eq!(draft.envelope.x(), Length::from_millimetres(600).expect("a side"));
        assert_eq!(draft.envelope.y(), Length::from_millimetres(560).expect("a side"));
        assert_eq!(draft.envelope.z(), Length::from_millimetres(2000).expect("a side"));
        assert_eq!(draft.class, ClassCode::GENERAL_PRODUCT);
    }

    #[test]
    fn escape_closes_the_card_and_enter_creates() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        crate::set_language(&ctx, Lang::En);
        let mut card = ProductCard::new();
        card.start();

        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let enter = ctx.run_ui(egui::RawInput { events: vec![key(egui::Key::Enter)], ..screen() }, |_ui| {
            let draft = card.show(&ctx);
            assert!(draft.is_some(), "Enter confirms the card");
        });
        enter.drop_without_applying_deltas();
        assert!(!card.is_open(), "a confirmed card closes");

        card.start();
        let escape = ctx.run_ui(egui::RawInput { events: vec![key(egui::Key::Escape)], ..screen() }, |_ui| {
            assert!(card.show(&ctx).is_none(), "Escape creates nothing");
        });
        escape.drop_without_applying_deltas();
        assert!(!card.is_open(), "and the card closes");
    }
}
