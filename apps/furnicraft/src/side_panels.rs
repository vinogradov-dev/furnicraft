//! The folding side panels: one expanded panel that keeps its width, one 26 pt strip when folded.
//!
//! [`side_panel`] draws a panel with a header row — the title and the fold chevron — and the body its
//! caller hands it; folded, the panel is a narrow strip with its icon and its title turned to read
//! along it. The chevron sits on the edge facing the viewport, so a panel folds from the side the
//! scene can see. The expanded and collapsed views are two egui panels with distinct ids, so the
//! folded width never overwrites the width the user dragged the expanded one to.
//!
//! Changes when: the header row, the folded strip or the fold and restore gesture changes.

use eframe::egui;
use furnicraft_text::Message;
use furnicraft_ui::{icons, theme};

/// A hairline across the panel, quieter than `ui.separator()` and on the spacing grid.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, theme::line(ui)));
}

/// Which edge a folding side panel sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelSide {
    Left,
    Right,
}

/// What a folding side panel's header row draws: the title and the icon the folded strip keeps.
///
/// A header carries no count: the tree and the card already say what they hold, and a number that
/// tracks the document would cost a scan on every rebuild.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PanelHead {
    pub(crate) title: furnicraft_text::Message,
    pub(crate) icon: icons::Icon,
    pub(crate) default_width: f32,
    pub(crate) min_width: f32,
}

/// Draws a side panel that folds to [`theme::COLLAPSED_PANEL_WIDTH`] and keeps its width.
///
/// The expanded and collapsed views are two panels with distinct ids, switched by
/// [`egui::Panel::show_switched`], so the collapsed view's 26 pt state never overwrites the width the
/// user dragged the expanded view to. A click on the strip, a double-click on its edge or a drag
/// outward restores the panel; the chevron in the header folds it, and it sits on the edge facing the
/// viewport — the right edge of a left panel, the left edge of a right one. `inset` is the band the
/// floating ribbon covers, so the panel's own header never starts under it (U51).
pub(crate) fn side_panel(
    ui: &mut egui::Ui,
    side: PanelSide,
    id: &str,
    open: &mut bool,
    head: PanelHead,
    inset: f32,
    body: impl FnOnce(&mut egui::Ui),
) {
    let mut expanded = *open;
    let mut fold_requested = false;
    let mut restore_requested = false;

    let panel = |id: String| match side {
        PanelSide::Left => egui::Panel::left(id),
        PanelSide::Right => egui::Panel::right(id),
    };
    let collapsed_fill = theme::surface(ui);
    let expanded_fill = theme::panel(ui);
    let collapsed = panel(format!("{id}_collapsed"))
        .resizable(true)
        .exact_size(theme::COLLAPSED_PANEL_WIDTH)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(collapsed_fill).inner_margin(egui::Margin::ZERO));
    let expanded_panel = panel(id.to_owned())
        .resizable(true)
        .default_size(head.default_width)
        .min_size(head.min_width)
        .frame(egui::Frame::new().fill(expanded_fill).inner_margin(egui::Margin::ZERO));

    egui::Panel::show_switched(ui, &mut expanded, collapsed, expanded_panel, |ui, unfolded| {
        ui.add_space(inset);
        if unfolded {
            if panel_header(ui, side, &head) {
                fold_requested = true;
            }
            rule(ui);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(6, theme::STEP as i8)).show(ui, |ui| {
                body(ui);
            });
        } else {
            collapsed_strip(ui, &head, &mut restore_requested);
        }
    });

    if fold_requested {
        expanded = false;
    }
    if restore_requested {
        expanded = true;
    }
    *open = expanded;
}

/// The header of an unfolded side panel: the title and the fold chevron.
///
/// The chevron sits on the edge the panel folds from — its right on a left panel, its left on a right
/// panel — so the control always faces the viewport the panel gives back. Returns whether it was
/// clicked.
fn panel_header(ui: &mut egui::Ui, side: PanelSide, head: &PanelHead) -> bool {
    let mut fold = false;
    egui::Frame::new().fill(theme::surface(ui)).inner_margin(egui::Margin::symmetric(theme::STEP as i8, 0)).show(
        ui,
        |ui| {
            ui.set_height(theme::PANEL_HEADER_HEIGHT);
            ui.horizontal_centered(|ui| {
                if side == PanelSide::Right {
                    fold = fold_chevron(ui);
                }
                ui.label(egui::RichText::new(head.title.text(furnicraft_ui::language(ui.ctx()))).strong());
                if side == PanelSide::Left {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        fold = fold_chevron(ui);
                    });
                }
            });
        },
    );
    fold
}

/// The chevron that folds an unfolded panel, carrying the hint that names the gesture.
fn fold_chevron(ui: &mut egui::Ui) -> bool {
    icons::icon_button_at(
        ui,
        icons::actions::COLLAPSE,
        12.0,
        Message::FoldPanel.text(furnicraft_ui::language(ui.ctx())),
        true,
        false,
    )
    .clicked()
}

/// The folded panel: a 26 pt strip with the panel's icon and its title turned to read along the
/// strip. A click anywhere on the strip restores the panel.
fn collapsed_strip(ui: &mut egui::Ui, head: &PanelHead, restore: &mut bool) {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *restore = true;
    }
    let fill = if response.hovered() { theme::control_hover(ui) } else { theme::surface(ui) };
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, fill);

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + theme::PANEL_HEADER_HEIGHT / 2.0),
        egui::Vec2::splat(14.0),
    );
    icons::draw_icon(ui, head.icon, icon_rect, 14.0, theme::text_dim(ui));

    // The title is turned so it reads bottom to top. `with_angle` turns about the galley's top-left
    // corner, so the corner is offset by the rotated half-size to centre the turned text.
    let dim = theme::text_dim(ui);
    let galley = ui.painter().layout_no_wrap(
        head.title.text(furnicraft_ui::language(ui.ctx())).to_owned(),
        egui::FontId::proportional(12.0),
        dim,
    );
    let size = galley.size();
    let center = egui::pos2(rect.center().x, rect.center().y + theme::PANEL_HEADER_HEIGHT / 2.0);
    let pos = egui::pos2(center.x - size.y / 2.0, center.y + size.x / 2.0);
    ui.painter().add(egui::epaint::TextShape::new(pos, galley, dim).with_angle(-std::f32::consts::FRAC_PI_2));
    response.on_hover_text(
        Message::OpenPanel
            .with(furnicraft_ui::language(ui.ctx()), [head.title.text(furnicraft_ui::language(ui.ctx()))]),
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::cell::Cell;

    use super::*;
    use crate::test_support::drawn_text;

    /// The window every test draws into.
    const SCREEN: egui::Vec2 = egui::vec2(1000.0, 600.0);

    /// A header whose title, icon and widths stand for a real panel.
    fn head() -> PanelHead {
        PanelHead { title: Message::PanelScene, icon: icons::scene::ASSEMBLY, default_width: 240.0, min_width: 180.0 }
    }

    /// A context on the real theme with animation off, so one frame draws its final state.
    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        ctx
    }

    fn input() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)), ..Default::default() }
    }

    /// The bounding rect of every rasterised glyph a frame painted.
    fn drawn_glyphs(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Mesh(mesh) => out.push(mesh.calc_bounds()),
                egui::Shape::Vec(shapes) => {
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

    /// The header band of the one panel a frame drew, found by the surface fill it paints.
    fn header_band(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> egui::Rect {
        let mut bands = Vec::new();
        for clipped in shapes {
            if let egui::Shape::Rect(rect) = &clipped.shape
                && rect.fill == fill
                && (rect.rect.height() - theme::PANEL_HEADER_HEIGHT).abs() < 0.5
            {
                bands.push(rect.rect);
            }
        }
        assert_eq!(bands.len(), 1, "one header band: {bands:?}");
        bands[0]
    }

    /// Draws one unfolded panel of `side` and answers what it drew.
    fn draw_header(ctx: &egui::Context, side: PanelSide) -> (Vec<String>, Vec<egui::Rect>, egui::Rect) {
        let mut open = true;
        let surface = Cell::new(egui::Color32::TRANSPARENT);
        let output = ctx.run_ui(input(), |ui| {
            side_panel(ui, side, "header_test", &mut open, head(), 0.0, |ui| {
                surface.set(theme::surface(ui));
                ui.label("BODY-CONTENT");
            });
        });
        let text = drawn_text(&output.shapes);
        let glyphs = drawn_glyphs(&output.shapes);
        let band = header_band(&output.shapes, surface.get());
        output.drop_without_applying_deltas();
        assert!(open, "the panel starts unfolded");
        (text, glyphs, band)
    }

    #[test]
    fn a_panel_header_draws_its_title_and_no_count() {
        let ctx = context();
        for side in [PanelSide::Left, PanelSide::Right] {
            let (text, _, _) = draw_header(&ctx, side);
            assert!(text.iter().any(|line| line.contains("Scene")), "{side:?} names itself: {text:?}");
            assert!(
                !text.iter().any(|line| line.chars().any(|c| c.is_ascii_digit())),
                "{side:?} draws no count: {text:?}"
            );
        }
    }

    #[test]
    fn a_panel_chevron_sits_on_the_edge_facing_the_viewport() {
        let ctx = context();
        for (side, faces_right) in [(PanelSide::Left, true), (PanelSide::Right, false)] {
            let (_, glyphs, band) = draw_header(&ctx, side);
            assert_eq!(glyphs.len(), 1, "an unfolded header paints one glyph: {glyphs:?}");
            let glyph = glyphs[0];
            let edge = 2.0 * theme::PANEL_HEADER_HEIGHT;
            if faces_right {
                assert!(
                    glyph.center().x > band.center().x && glyph.max.x > band.max.x - edge,
                    "a left panel folds from its right edge: {glyph:?} in {band:?}"
                );
            } else {
                assert!(
                    glyph.center().x < band.center().x && glyph.min.x < band.min.x + edge,
                    "a right panel folds from its left edge: {glyph:?} in {band:?}"
                );
            }
            assert!(
                glyph.center().y > band.top() && glyph.center().y < band.bottom(),
                "the chevron rides the header row: {glyph:?} in {band:?}"
            );
        }
    }

    #[test]
    fn a_folded_panel_keeps_its_expanded_width_and_draws_no_body() {
        let ctx = context();
        let head = head();
        let mut open = true;
        let width_open = Cell::new(0.0);
        let output = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, 0.0, |ui| {
                width_open.set(ui.max_rect().width());
                ui.label("BODY-CONTENT");
            });
        });
        output.drop_without_applying_deltas();
        assert!(open, "the panel starts unfolded");
        assert!(width_open.get() > theme::COLLAPSED_PANEL_WIDTH, "an unfolded panel has its content width");

        open = false;
        let folded = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, 0.0, |ui| {
                ui.label("BODY-CONTENT");
            });
        });
        let folded_text = drawn_text(&folded.shapes);
        folded.drop_without_applying_deltas();
        assert!(!open, "the panel stays folded");
        assert!(
            !folded_text.iter().any(|text| text.contains("BODY-CONTENT")),
            "a folded panel draws no body: {folded_text:?}"
        );
        assert!(folded_text.iter().any(|text| text.contains("Scene")), "the strip keeps the turned title");

        open = true;
        let width_again = Cell::new(0.0);
        let output = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, 0.0, |ui| {
                width_again.set(ui.max_rect().width());
                ui.label("BODY-CONTENT");
            });
        });
        output.drop_without_applying_deltas();
        assert!(open, "the panel is unfolded again");
        assert!(
            (width_again.get() - width_open.get()).abs() < 1.0,
            "the expanded width survives the fold: {} vs {}",
            width_again.get(),
            width_open.get()
        );
    }
}
