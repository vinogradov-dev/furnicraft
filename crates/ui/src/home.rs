//! The Home surface: what the house button opens.
//!
//! Home is a start view, not a document surface. It offers a new scene, the file dialog, the recent
//! files the workspace remembers, and the empty template; it reads the recent list and returns one
//! [`HomeAction`], which the shell performs through the same File menu path as the header. Escape
//! and a click on the backdrop dismiss it.

use std::path::{Path, PathBuf};

use egui::{Align, Context, Id, Layout, Modal, RichText, Ui};

use furnicraft_text::Message;

use crate::theme;

/// What the Home surface asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HomeAction {
    /// Start a new, empty scene.
    New,
    /// Open the file dialog.
    Open,
    /// Start from the empty template.
    EmptyTemplate,
    /// Open one of the recent files.
    OpenRecent(PathBuf),
    /// Dismiss the surface.
    Close,
}

/// Draws the Home surface and returns the one action it asked for, if any.
///
/// The recent list is borrowed from the workspace settings, so Home never owns or edits it; the
/// shell records a path when a scene is actually opened or saved.
pub fn show(ctx: &Context, recent: &[PathBuf]) -> Option<HomeAction> {
    let mut action = None;
    let response =
        Modal::new(Id::unique("home_surface")).backdrop_color(egui::Color32::from_black_alpha(160)).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.heading(theme::PRODUCT_NAME);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(Message::HomeClose.text(crate::language(ctx))).clicked() {
                        action = Some(HomeAction::Close);
                    }
                });
            });
            ui.label(RichText::new(Message::HomeStartScene.text(crate::language(ui.ctx()))).color(theme::text_dim(ui)));
            ui.add_space(theme::STEP * 2.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(Message::HomeNewScene.text(crate::language(ui.ctx()))).size(14.0),
                        )
                        .min_size(egui::vec2(150.0, 32.0)),
                    )
                    .clicked()
                {
                    action = Some(HomeAction::New);
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(Message::HomeOpenScene.text(crate::language(ui.ctx()))).size(14.0),
                        )
                        .min_size(egui::vec2(150.0, 32.0)),
                    )
                    .clicked()
                {
                    action = Some(HomeAction::Open);
                }
            });
            ui.add_space(theme::STEP * 2.0);
            template_card(ui, &mut action);
            ui.add_space(theme::STEP * 2.0);
            ui.label(RichText::new(Message::HomeRecentFiles.text(crate::language(ui.ctx()))).strong());
            if recent.is_empty() {
                ui.label(
                    RichText::new(Message::HomeNoScene.text(crate::language(ui.ctx()))).color(theme::text_dim(ui)),
                );
            } else {
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for path in recent {
                        recent_row(ui, path, &mut action);
                    }
                });
            }
        });
    if response.should_close() {
        action = Some(HomeAction::Close);
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        action = Some(HomeAction::Close);
    }
    action
}

/// The empty-template card: the one template the build ships with.
fn template_card(ui: &mut Ui, action: &mut Option<HomeAction>) {
    egui::Frame::new()
        .fill(theme::panel(ui))
        .stroke(egui::Stroke::new(1.0, theme::line(ui)))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(Message::HomeEmptyTemplate.text(crate::language(ui.ctx()))).strong());
                    ui.label(
                        RichText::new(Message::HomeEmptyTemplateHint.text(crate::language(ui.ctx())))
                            .small()
                            .color(theme::text_dim(ui)),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(Message::HomeUseTemplate.text(crate::language(ui.ctx()))).clicked() {
                        *action = Some(HomeAction::EmptyTemplate);
                    }
                });
            });
        });
}

/// One recent file: its name and its directory in one selectable row.
fn recent_row(ui: &mut Ui, path: &Path, action: &mut Option<HomeAction>) {
    let name =
        path.file_name().and_then(|name| name.to_str()).unwrap_or(Message::HomeUnnamed.text(crate::language(ui.ctx())));
    let directory = path.parent().map(|parent| parent.display().to_string()).unwrap_or_default();
    let label = egui::RichText::new(format!("{name}\n{directory}"));
    let response = ui.selectable_label(false, label);
    if response.clicked() {
        *action = Some(HomeAction::OpenRecent(path.to_path_buf()));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use egui::epaint::Shape;

    use super::*;

    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
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

    /// Runs the surface twice and returns the text it drew.
    ///
    /// A modal is an `egui::Area`: its first pass measures the content and only the second one
    /// paints, so a single pass would report an empty window even when the surface is correct.
    fn drawn_home(recent: &[PathBuf]) -> Vec<String> {
        let ctx = Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let first = ctx.run_ui(input.clone(), |ui| {
            show(ui.ctx(), recent);
        });
        first.drop_without_applying_deltas();
        let output = ctx.run_ui(input, |ui| {
            show(ui.ctx(), recent);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        texts
    }

    #[test]
    fn home_names_the_start_actions_and_the_recent_file() {
        let texts = drawn_home(&[PathBuf::from("/tmp/cabinet.fcs")]);
        for expected in ["New scene", "Open scene…", "Empty template", "cabinet.fcs"] {
            assert!(texts.iter().any(|text| text.contains(expected)), "{expected} is missing: {texts:?}");
        }
    }

    #[test]
    fn home_says_so_when_nothing_was_opened_yet() {
        let texts = drawn_home(&[]);
        assert!(texts.iter().any(|text| text.contains("No scene opened yet")), "{texts:?}");
    }
}
