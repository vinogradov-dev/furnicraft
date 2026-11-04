//! Themed toolbars: the strip of panels that own the tools.
//!
//! Four panels group the tools by the theme of the assets they draw: Contour, Scene, Primitives and
//! Bodies. A panel is a collapsible section; whether it opens, and what the strip remembers, is view
//! state in the editor and never reaches the document. The strip itself has no tool yet: U19 fills
//! the panels, and the buttons here are the shape every tool will use, including the text fallback
//! an undrawn icon needs.

use egui::Ui;

use crate::icons::{self, Theme};

/// One themed panel of the toolbar strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPanel {
    /// Drawing on the work plane.
    Contour,
    /// Creating panels, assemblies, cameras and lights.
    Scene,
    /// Solid primitives.
    Primitives,
    /// Bodies built from a drawing.
    Bodies,
}

impl ToolPanel {
    /// Every panel, in strip order.
    pub const ALL: [Self; 4] = [Self::Contour, Self::Scene, Self::Primitives, Self::Bodies];

    /// The heading the strip shows for this panel.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Contour => "Contour",
            Self::Scene => "Scene",
            Self::Primitives => "Primitives",
            Self::Bodies => "Bodies",
        }
    }

    /// The icon theme this panel draws from.
    pub const fn theme(self) -> Theme {
        match self {
            Self::Contour => Theme::Contour,
            Self::Scene => Theme::Scene,
            Self::Primitives => Theme::Primitives,
            Self::Bodies => Theme::Bodies,
        }
    }

    /// The stable id `CollapsingState` stores this panel's open state under.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Contour => "toolbar_contour",
            Self::Scene => "toolbar_scene",
            Self::Primitives => "toolbar_primitives",
            Self::Bodies => "toolbar_bodies",
        }
    }
}

/// Whether each panel of the strip is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: [bool; 4],
}

impl Default for Toolbars {
    fn default() -> Self {
        Self { open: [true; 4] }
    }
}

impl Toolbars {
    /// Every panel open, the state a fresh editor starts in.
    pub const fn new() -> Self {
        Self { open: [true; 4] }
    }

    /// Whether the panel is open.
    pub fn is_open(&self, panel: ToolPanel) -> bool {
        panel_slot(panel).is_some_and(|slot| self.open.get(slot).copied().unwrap_or(false))
    }

    /// Opens or closes one panel and returns its new state.
    pub fn set_open(&mut self, panel: ToolPanel, open: bool) -> bool {
        let Some(slot) = panel_slot(panel) else {
            return false;
        };
        if let Some(value) = self.open.get_mut(slot) {
            *value = open;
        }
        self.is_open(panel)
    }

    /// Flips one panel and returns its new state.
    pub fn toggle(&mut self, panel: ToolPanel) -> bool {
        self.set_open(panel, !self.is_open(panel))
    }
}

/// The index of a panel in the fixed strip order.
fn panel_slot(panel: ToolPanel) -> Option<usize> {
    ToolPanel::ALL.iter().position(|candidate| *candidate == panel)
}

/// Draws the whole strip.
///
/// The headings are collapsible sections. A section that is open draws its tools; the panels are
/// empty until U19, so each one says so instead of pretending to offer a control.
pub fn show(ui: &mut Ui, state: &mut Toolbars) {
    ui.heading("Tools");
    ui.separator();
    for panel in ToolPanel::ALL {
        let open = state.is_open(panel);
        let response = egui::CollapsingHeader::new(panel.label()).id_salt(panel.id()).open(Some(open)).show(ui, |ui| {
            show_panel(ui, panel);
        });
        if response.header_response.clicked() {
            state.set_open(panel, !open);
        }
    }
}

/// The body of one open panel.
fn show_panel(ui: &mut Ui, panel: ToolPanel) {
    ui.horizontal_wrapped(|ui| {
        let icons = panel.theme().icons();
        if icons.is_empty() {
            ui.label(format!("The {} panel is empty.", panel.label()));
            return;
        }
        // The theme's first icon is the panel's own glyph; drawing it here keeps the strip readable
        // while the tool buttons themselves arrive in U19.
        let icon = icons.first().copied().filter(|icon| icon.is_drawn());
        if let Some(icon) = icon {
            icons::icon_button(ui, icon, panel.label());
        } else {
            ui.label(format!("{} tools", panel.label()));
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn every_panel_starts_open_and_keeps_its_own_state() {
        let mut state = Toolbars::new();
        for panel in ToolPanel::ALL {
            assert!(state.is_open(panel), "{} starts open", panel.label());
            assert!(!panel.id().is_empty());
        }

        assert!(!state.toggle(ToolPanel::Contour));
        assert!(!state.is_open(ToolPanel::Contour));
        assert!(state.is_open(ToolPanel::Scene), "a toggle touches one panel");
        assert!(state.toggle(ToolPanel::Contour));
        assert!(state.is_open(ToolPanel::Contour));

        assert!(!state.set_open(ToolPanel::Bodies, false));
        assert!(!state.is_open(ToolPanel::Bodies));
    }

    #[test]
    fn every_panel_names_its_theme_and_label() {
        let cases = [
            (ToolPanel::Contour, Theme::Contour, "Contour", "toolbar_contour"),
            (ToolPanel::Scene, Theme::Scene, "Scene", "toolbar_scene"),
            (ToolPanel::Primitives, Theme::Primitives, "Primitives", "toolbar_primitives"),
            (ToolPanel::Bodies, Theme::Bodies, "Bodies", "toolbar_bodies"),
        ];
        for (panel, theme, label, id) in cases {
            assert_eq!(panel.theme(), theme);
            assert_eq!(panel.label(), label);
            assert_eq!(panel.id(), id);
        }
    }

    #[test]
    fn the_strip_starts_with_a_contour_toolbar() {
        assert_eq!(ToolPanel::ALL[0], ToolPanel::Contour);
        assert_eq!(ToolPanel::ALL.len(), 4);
    }

    #[test]
    fn the_strip_draws_every_heading_and_a_readable_body() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        for panel in ToolPanel::ALL {
            assert!(texts.iter().any(|text| text == panel.label()), "{} heading is missing", panel.label());
        }
        assert!(
            texts.iter().any(|text| text == "Scene tools"),
            "an empty icon theme leaves a readable body: {texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Bodies tools"), "every undrawn theme says so: {texts:?}");
    }

    #[test]
    fn a_closed_panel_draws_no_body() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        state.set_open(ToolPanel::Scene, false);
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text == "Scene"), "a closed panel keeps its heading");
        assert!(!texts.iter().any(|text| text == "Scene tools"), "a closed panel draws no body: {texts:?}");
    }

    #[test]
    fn a_closed_contour_panel_draws_only_its_heading() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        state.set_open(ToolPanel::Contour, false);
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Contour")), "a closed panel keeps its heading: {texts:?}");
        assert!(!texts.iter().any(|text| text == "Contour tools"), "the body is not drawn: {texts:?}");
    }

    /// Collects the text of every text shape, including nested ones.
    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
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
}
