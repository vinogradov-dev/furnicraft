//! Themed toolbars: the strip of panels that own the tools.
//!
//! Four panels group the tools by the theme of the assets they draw: Contour, Scene, Primitives and
//! Bodies. A panel is a collapsible section; whether it opens, and what the strip remembers, is view
//! state in the editor and never reaches the document. The strip itself has no tool yet: U19 fills
//! the panels, and the buttons here are the shape every tool will use, including the text fallback
//! an undrawn icon needs.

use egui::{TextEdit, Ui};
use furnicraft_engine::PrimitiveKind;
use furnicraft_model::Id;
use furnicraft_units::Length;

use crate::contour_edit::DrawTool;
use crate::icons::{self, Icon, Theme};

/// Edge length of a new cube, in millimetres; also the value the field starts with.
pub const DEFAULT_PRIMITIVE_SIZE_MM: i64 = 200;

/// One action a toolbar panel asks the editor to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    /// Starts a drawing tool.
    Draw(DrawTool),
    /// Creates a panel under the container.
    CreatePanel {
        /// The receiving container.
        parent: Id,
    },
    /// Creates an assembly under the container.
    CreateAssembly {
        /// The receiving container.
        parent: Id,
    },
    /// Creates a primitive once the work plane is clicked.
    CreatePrimitive {
        /// The receiving container.
        parent: Id,
        /// Which primitive to build.
        kind: PrimitiveKind,
        /// Full extent of the primitive.
        size: Length,
    },
    /// Creates the default extrusion.
    CreateExtrude {
        /// The receiving container.
        parent: Id,
    },
    /// Creates a camera.
    CreateCamera {
        /// The receiving container.
        parent: Id,
    },
    /// Creates a sun light.
    CreateLight {
        /// The receiving container.
        parent: Id,
    },
}

/// The drawing tools of the Contour panel, in button order.
pub const CONTOUR_TOOLS: [DrawTool; 5] =
    [DrawTool::Line, DrawTool::Polyline, DrawTool::Arc, DrawTool::Rectangle, DrawTool::Circle];

/// The shortcut that starts a drawing tool.
///
/// The letters are free in the viewport (`F` frames, `O` toggles the projection, `G` the grid), so
/// the tooltip and the key handler can share one table.
pub const fn shortcut(tool: DrawTool) -> Option<egui::Key> {
    match tool {
        DrawTool::Line => Some(egui::Key::L),
        DrawTool::Polyline => Some(egui::Key::P),
        DrawTool::Arc => Some(egui::Key::A),
        DrawTool::Rectangle => Some(egui::Key::R),
        DrawTool::Circle => Some(egui::Key::C),
    }
}

/// The label of a tool's shortcut, for the button hint.
pub fn shortcut_label(tool: DrawTool) -> Option<&'static str> {
    shortcut(tool).map(egui::Key::name)
}

/// The icon of a drawing tool.
const fn tool_icon(tool: DrawTool) -> Icon {
    match tool {
        DrawTool::Line => icons::contour::LINE,
        DrawTool::Polyline => icons::contour::POLYLINE,
        DrawTool::Arc => icons::contour::ARC,
        DrawTool::Rectangle => icons::contour::RECTANGLE,
        DrawTool::Circle => icons::contour::CIRCLE,
    }
}

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

/// Whether each panel of the strip is open, plus the size field the Primitives panel keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: [bool; 4],
    primitive_size: String,
    error: Option<String>,
}

impl Default for Toolbars {
    fn default() -> Self {
        Self { open: [true; 4], primitive_size: DEFAULT_PRIMITIVE_SIZE_MM.to_string(), error: None }
    }
}

impl Toolbars {
    /// Every panel open and the primitive size at its default, the state a fresh editor starts in.
    pub fn new() -> Self {
        Self::default()
    }

    /// The typed edge length of a new primitive.
    pub fn primitive_size_text(&self) -> &str {
        &self.primitive_size
    }

    /// The size field, so the panel can edit the text in place.
    pub fn primitive_size_mut(&mut self) -> &mut String {
        &mut self.primitive_size
    }

    /// Replaces the typed edge length.
    pub fn set_primitive_size_text(&mut self, text: impl Into<String>) {
        self.primitive_size = text.into();
        self.error = None;
    }

    /// The typed edge length as a document length, or a readable reason it is not one.
    pub fn primitive_size(&self) -> Result<Length, String> {
        let length = Length::parse_millimetres(self.primitive_size.trim())
            .map_err(|error| format!("The cube edge is not a length: {error}"))?;
        if length.micrometres() <= 0 {
            return Err("The cube edge must be positive.".to_owned());
        }
        Ok(length)
    }

    /// The last reason the size field was refused.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Records a refused size so the panel can show it under the field.
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }
}

impl Toolbars {
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
/// The headings are collapsible sections. A section that is open draws its tools; every button is
/// the hinted tool button, so a drawn icon is a glyph and an empty asset is a readable label.
pub fn show(ui: &mut Ui, state: &mut Toolbars, parent: Id, has_panel_material: bool, actions: &mut Vec<ToolAction>) {
    ui.heading("Tools");
    ui.separator();
    for panel in ToolPanel::ALL {
        let open = state.is_open(panel);
        let response = egui::CollapsingHeader::new(panel.label()).id_salt(panel.id()).open(Some(open)).show(ui, |ui| {
            show_panel(ui, panel, parent, has_panel_material, state, actions);
        });
        if response.header_response.clicked() {
            state.set_open(panel, !open);
        }
    }
}

/// The body of one open panel.
fn show_panel(
    ui: &mut Ui,
    panel: ToolPanel,
    parent: Id,
    has_panel_material: bool,
    state: &mut Toolbars,
    actions: &mut Vec<ToolAction>,
) {
    ui.horizontal_wrapped(|ui| match panel {
        ToolPanel::Contour => {
            for tool in CONTOUR_TOOLS {
                if icons::tool_button(ui, tool_icon(tool), tool.label(), shortcut_label(tool), true).clicked() {
                    actions.push(ToolAction::Draw(tool));
                }
            }
        }
        ToolPanel::Scene => {
            if icons::tool_button(ui, icons::scene::PANEL, "Panel", Some("N"), has_panel_material).clicked() {
                actions.push(ToolAction::CreatePanel { parent });
            }
            if icons::tool_button(ui, icons::scene::ASSEMBLY, "Assembly", None, true).clicked() {
                actions.push(ToolAction::CreateAssembly { parent });
            }
            if icons::tool_button(ui, icons::scene::CAMERA, "Camera", None, true).clicked() {
                actions.push(ToolAction::CreateCamera { parent });
            }
            if icons::tool_button(ui, icons::scene::LIGHT, "Light", None, true).clicked() {
                actions.push(ToolAction::CreateLight { parent });
            }
        }
        ToolPanel::Primitives => {
            ui.label("Edge");
            ui.add(TextEdit::singleline(state.primitive_size_mut()).desired_width(72.0).hint_text("mm"));
            ui.label("mm");
            if let Some(error) = state.error() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let size = state.primitive_size();
            let enabled = size.is_ok();
            for (kind, icon, label) in [
                (PrimitiveKind::Cube, icons::primitives::CUBE, "Cube"),
                (PrimitiveKind::Cylinder, icons::primitives::CYLINDER, "Cylinder"),
                (PrimitiveKind::Sphere, icons::primitives::SPHERE, "Sphere"),
            ] {
                if icons::tool_button(ui, icon, &format!("{label}: click the work plane"), None, enabled).clicked()
                    && let Ok(size) = size
                {
                    actions.push(ToolAction::CreatePrimitive { parent, kind, size });
                }
            }
        }
        ToolPanel::Bodies => {
            if icons::tool_button(ui, icons::bodies::EXTRUDE, "Extrude", None, true).clicked() {
                actions.push(ToolAction::CreateExtrude { parent });
            }
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
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, Id::new(1).expect("root"), true, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        for panel in ToolPanel::ALL {
            assert!(texts.iter().any(|text| text == panel.label()), "{} heading is missing", panel.label());
        }
        assert!(
            texts.iter().any(|text| text == "Panel  (N)"),
            "an empty icon falls back to the labelled shortcut: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "Cube: click the work plane"),
            "the primitives panel is filled: {texts:?}"
        );
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn the_three_primitive_buttons_are_filled() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, Id::new(1).expect("root"), true, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        for label in ["Cube: click the work plane", "Cylinder: click the work plane", "Sphere: click the work plane"] {
            assert!(texts.iter().any(|text| text == label), "{label} is missing: {texts:?}");
        }
    }

    #[test]
    fn a_panel_without_a_catalogue_material_cannot_create_a_panel() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, Id::new(1).expect("root"), false, &mut actions);
        });
        output.drop_without_applying_deltas();
        assert!(actions.is_empty(), "the Panel tool is disabled without a material");
    }

    #[test]
    fn every_tool_has_a_distinct_shortcut() {
        let mut seen = std::collections::HashSet::new();
        for tool in CONTOUR_TOOLS {
            let key = shortcut(tool).expect("every drawing tool has a key");
            assert!(seen.insert(key.name()), "{} reuses a shortcut", key.name());
            assert_eq!(shortcut_label(tool), Some(key.name()));
        }
        assert_eq!(shortcut(DrawTool::Line).map(egui::Key::name), Some("L"));
        assert_eq!(shortcut(DrawTool::Rectangle).map(egui::Key::name), Some("R"));
    }

    #[test]
    fn a_closed_panel_draws_no_body() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        state.set_open(ToolPanel::Scene, false);
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, Id::new(1).expect("root"), true, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text == "Scene"), "a closed panel keeps its heading");
        assert!(!texts.iter().any(|text| text == "Panel"), "a closed panel draws no body: {texts:?}");
    }

    #[test]
    fn a_closed_contour_panel_draws_only_its_heading() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        state.set_open(ToolPanel::Contour, false);
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, Id::new(1).expect("root"), true, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Contour")), "a closed panel keeps its heading: {texts:?}");
        assert!(!texts.iter().any(|text| text == "Line  (L)"), "the body is not drawn: {texts:?}");
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
