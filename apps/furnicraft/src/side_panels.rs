//! The folding side panels: one expanded panel that keeps its width, one 26 pt strip when folded.
//!
//! [`side_panel`] draws a panel with a header row — the title, the count chip and the fold chevron —
//! and the body its caller hands it; folded, the panel is a narrow strip with its icon and its
//! title turned to read along it. The expanded and collapsed views are two egui panels with distinct
//! ids, so the folded width never overwrites the width the user dragged the expanded one to.
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

/// What a folding side panel's header row draws: the title, the count chip and the icon the folded
/// strip keeps.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PanelHead {
    pub(crate) title: furnicraft_text::Message,
    pub(crate) count: usize,
    pub(crate) icon: icons::Icon,
    pub(crate) default_width: f32,
    pub(crate) min_width: f32,
}

/// Draws a side panel that folds to [`theme::COLLAPSED_PANEL_WIDTH`] and keeps its width.
///
/// The expanded and collapsed views are two panels with distinct ids, switched by
/// [`egui::Panel::show_switched`], so the collapsed view's 26 pt state never overwrites the width the
/// user dragged the expanded view to. A click on the strip, a double-click on its edge or a drag
/// outward restores the panel; the chevron in the header folds it. `inset` is the band the floating
/// ribbon covers, so the panel's own header never starts under it (U51).
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
            if panel_header(ui, &head) {
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

/// The header of an unfolded side panel: the title, a count chip and the fold chevron.
///
/// Returns whether the chevron was clicked.
fn panel_header(ui: &mut egui::Ui, head: &PanelHead) -> bool {
    let mut fold = false;
    egui::Frame::new().fill(theme::surface(ui)).inner_margin(egui::Margin::symmetric(theme::STEP as i8, 0)).show(
        ui,
        |ui| {
            ui.set_height(theme::PANEL_HEADER_HEIGHT);
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(head.title.text(furnicraft_ui::language(ui.ctx()))).strong());
                count_chip(ui, head.count);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_button_at(
                        ui,
                        icons::actions::COLLAPSE,
                        12.0,
                        Message::FoldPanel.text(furnicraft_ui::language(ui.ctx())),
                        true,
                        false,
                    )
                    .clicked()
                    {
                        fold = true;
                    }
                });
            });
        },
    );
    fold
}

/// A small count chip beside a panel title.
fn count_chip(ui: &mut egui::Ui, count: usize) {
    egui::Frame::new()
        .fill(theme::control(ui))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(count.to_string()).small().color(theme::text_dim(ui)));
        });
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

    use super::*;
    use crate::test_support::drawn_text;

    #[test]
    fn a_folded_panel_keeps_its_expanded_width_and_draws_no_body() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let head = PanelHead {
            title: Message::PanelScene,
            count: 4,
            icon: icons::scene::ASSEMBLY,
            default_width: 240.0,
            min_width: 180.0,
        };
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))),
            ..Default::default()
        };
        let mut open = true;
        let width_open = std::cell::Cell::new(0.0);
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
        let width_again = std::cell::Cell::new(0.0);
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
