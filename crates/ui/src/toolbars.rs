//! Themed toolbars: the strip of panels that own the tools.
//!
//! Four panels group the tools by the theme of the assets they draw: Contour, Scene, Primitives and
//! Bodies. A panel is a collapsible section; whether it opens, and what the numeric fields remember,
//! is view state in the editor and never reaches the document. Every button is the hinted tool
//! button, including the text fallback an undrawn icon needs, and a panel that edits a number keeps
//! the text until the user commits it.

use egui::{TextEdit, Ui};
use furnicraft_engine::PrimitiveKind;
use furnicraft_model::Id;
use furnicraft_units::Length;

use crate::contour_edit::DrawTool;
use crate::icons::{self, Icon, Theme};

/// Edge length of a new cube, in millimetres; also the value the field starts with.
pub const DEFAULT_PRIMITIVE_SIZE_MM: i64 = 200;

/// Height of a new extrusion, in millimetres; also the value the field starts with.
pub const DEFAULT_EXTRUDE_HEIGHT_MM: i64 = 18;

/// One action a toolbar panel asks the editor to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    /// Starts a drawing tool.
    Draw(DrawTool),
    /// Creates a panel at the scene root.
    CreatePanel,
    /// Creates an assembly at the scene root.
    CreateAssembly,
    /// Creates a primitive at the scene root once the work plane is clicked.
    CreatePrimitive {
        /// Which primitive to build.
        kind: PrimitiveKind,
        /// Full extent of the primitive.
        size: Length,
    },
    /// Extrudes the selected closed path into a body.
    ExtrudePath {
        /// The path object to extrude.
        object: Id,
        /// Height along the path's local Z.
        height: Length,
    },
    /// Creates a camera at the scene root.
    CreateCamera,
    /// Creates a sun light at the scene root.
    CreateLight,
    /// Drops the armed container, so creation returns to the scene root.
    ClearCreationParent,
}

/// The drawing tools of the Contour panel, in button order.
pub const CONTOUR_TOOLS: [DrawTool; 5] =
    [DrawTool::Line, DrawTool::Polyline, DrawTool::Arc, DrawTool::Rectangle, DrawTool::Circle];

/// The tool that owns the next pointer events, drawn as pressed on the strip.
///
/// Only the two arming tools have a state to show: a drawing tool holds the pointer until the chain
/// is committed, and a primitive waits for its work-plane click. A one-shot creation — a panel, an
/// assembly, a camera or a light — runs and forgets, so it is never armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActivity {
    /// A drawing tool is building a chain.
    Draw(DrawTool),
    /// A primitive is waiting for its work-plane click.
    Primitive(PrimitiveKind),
}

/// The activity whose button is pressed, from what the editor holds.
///
/// The arguments are the two sources of an armed tool, not a decision: a running draft names its
/// tool and a pending primitive names its kind, and neither outranks the other because the editor
/// never holds both at once.
pub fn tool_activity(drawing: Option<DrawTool>, primitive: Option<PrimitiveKind>) -> Option<ToolActivity> {
    match (drawing, primitive) {
        (Some(tool), _) => Some(ToolActivity::Draw(tool)),
        (None, Some(kind)) => Some(ToolActivity::Primitive(kind)),
        (None, None) => None,
    }
}

/// Whether the button for one action is the armed one.
///
/// A creation that runs and forgets is never armed, so its button stays idle however long the
/// selection sits there.
fn is_armed(activity: Option<ToolActivity>, action: ToolAction) -> bool {
    match (activity, action) {
        (Some(ToolActivity::Draw(armed)), ToolAction::Draw(clicked)) => armed == clicked,
        (Some(ToolActivity::Primitive(armed)), ToolAction::CreatePrimitive { kind, .. }) => armed == kind,
        _ => false,
    }
}

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

/// The strip's label for a drawing tool: its name plus the one gesture the tool does not show.
///
/// The label is the button's tooltip and the text a placeholder asset falls back to; it is not the
/// name of the object the tool creates, which stays [`DrawTool::label`].
pub const fn strip_label(tool: DrawTool) -> &'static str {
    match tool {
        DrawTool::Polyline => "Polyline · Alt+click for an arc",
        _ => tool.label(),
    }
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

/// Whether each panel of the strip is open, plus the numeric fields the panels keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: [bool; 4],
    primitive_size: String,
    extrude_height: String,
    error: Option<String>,
}

impl Default for Toolbars {
    fn default() -> Self {
        Self {
            open: [true; 4],
            primitive_size: DEFAULT_PRIMITIVE_SIZE_MM.to_string(),
            extrude_height: DEFAULT_EXTRUDE_HEIGHT_MM.to_string(),
            error: None,
        }
    }
}

impl Toolbars {
    /// Every panel open and the primitive size at its default, the state a fresh editor starts in.
    pub fn new() -> Self {
        Self::default()
    }

    /// The typed full extent of a new primitive: the cube's edge, or a curved shape's diameter.
    pub fn primitive_size_text(&self) -> &str {
        &self.primitive_size
    }

    /// The size field, so the panel can edit the text in place.
    pub fn primitive_size_mut(&mut self) -> &mut String {
        &mut self.primitive_size
    }

    /// Replaces the typed size.
    pub fn set_primitive_size_text(&mut self, text: impl Into<String>) {
        self.primitive_size = text.into();
        self.error = None;
    }

    /// The typed size as a document length, or a readable reason it is not one.
    pub fn primitive_size(&self) -> Result<Length, String> {
        let length = Length::parse_millimetres(self.primitive_size.trim())
            .map_err(|error| format!("The primitive size is not a length: {error}"))?;
        if length.micrometres() <= 0 {
            return Err("The primitive size must be positive.".to_owned());
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

    /// The typed extrusion height.
    pub fn extrude_height_text(&self) -> &str {
        &self.extrude_height
    }

    /// The height field, so the panel can edit the text in place.
    pub fn extrude_height_mut(&mut self) -> &mut String {
        &mut self.extrude_height
    }

    /// The typed extrusion height as a positive document length.
    ///
    /// The upper bound is the document's contour limit, the same one `object.extrude.create`
    /// applies, so a field the panel accepts is a field the command takes.
    pub fn extrude_height(&self) -> Result<Length, String> {
        let height = Length::parse_millimetres(self.extrude_height.trim())
            .map_err(|error| format!("The extrusion height is not a length: {error}"))?;
        if !height.is_positive() {
            return Err("The extrusion height must be positive.".to_owned());
        }
        if height.micrometres() > furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() {
            return Err(format!(
                "The extrusion height must be at most {} mm.",
                furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000
            ));
        }
        Ok(height)
    }

    /// Replaces the typed extrusion height.
    pub fn set_extrude_height_text(&mut self, text: impl Into<String>) {
        self.extrude_height = text.into();
        self.error = None;
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

/// Draws the chip that names the armed container and reports a click on its ✕.
///
/// The arm is sticky across creations, so the strip must say where the next node goes; without the
/// chip a pressed tool would create in a container the user armed much earlier.
fn creation_chip(ui: &mut Ui, name: &str) -> bool {
    let mut cleared = false;
    ui.horizontal(|ui| {
        ui.label(format!("Adding to: {name}"));
        cleared = ui.button("✕").on_hover_text("Create at the scene root again").clicked();
    });
    cleared
}

/// Draws the whole strip.
///
/// The headings are collapsible sections. A section that is open draws its tools; every button is
/// the hinted tool button, so a drawn icon is a glyph and an empty asset is a readable label. The
/// strip itself carries no parent: `creation_parent` names the container armed to receive the next
/// nodes, and the chip clears that arm. `active` is the tool that owns the next click, and its
/// button draws pressed.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut Toolbars,
    has_panel_material: bool,
    selected: Option<Id>,
    selected_path: Option<bool>,
    active: Option<ToolActivity>,
    creation_parent: Option<&str>,
    actions: &mut Vec<ToolAction>,
) {
    let context = StripContext { has_panel_material, selected, selected_path, active };
    ui.heading("Tools");
    if let Some(name) = creation_parent
        && creation_chip(ui, name)
    {
        actions.push(ToolAction::ClearCreationParent);
    }
    ui.separator();
    for panel in ToolPanel::ALL {
        let open = state.is_open(panel);
        let response = egui::CollapsingHeader::new(panel.label()).id_salt(panel.id()).open(Some(open)).show(ui, |ui| {
            show_panel(ui, panel, context, state, actions);
        });
        if response.header_response.clicked() {
            state.set_open(panel, !open);
        }
    }
}

/// What the strip reads from the scene and the pointer while it draws one panel.
///
/// The four values travel together because every panel reads the same picture: what the catalogue
/// offers, what is selected, whether that selection is a closed path, and which tool is armed.
#[derive(Clone, Copy, Debug, Default)]
struct StripContext {
    has_panel_material: bool,
    selected: Option<Id>,
    selected_path: Option<bool>,
    active: Option<ToolActivity>,
}

/// The body of one open panel.
fn show_panel(
    ui: &mut Ui,
    panel: ToolPanel,
    context: StripContext,
    state: &mut Toolbars,
    actions: &mut Vec<ToolAction>,
) {
    let StripContext { has_panel_material, selected, selected_path, active } = context;
    ui.horizontal_wrapped(|ui| match panel {
        ToolPanel::Contour => {
            for tool in CONTOUR_TOOLS {
                let action = ToolAction::Draw(tool);
                let on = is_armed(active, action);
                let label = strip_label(tool);
                if icons::tool_button(ui, tool_icon(tool), label, shortcut_label(tool), true, on).clicked() {
                    actions.push(action);
                }
            }
        }
        ToolPanel::Scene => {
            if icons::tool_button(ui, icons::scene::PANEL, "Panel", Some("N"), has_panel_material, false).clicked() {
                actions.push(ToolAction::CreatePanel);
            }
            if icons::tool_button(ui, icons::scene::ASSEMBLY, "Assembly", None, true, false).clicked() {
                actions.push(ToolAction::CreateAssembly);
            }
            if icons::tool_button(ui, icons::scene::CAMERA, "Camera", None, true, false).clicked() {
                actions.push(ToolAction::CreateCamera);
            }
            if icons::tool_button(ui, icons::scene::LIGHT, "Light", None, true, false).clicked() {
                actions.push(ToolAction::CreateLight);
            }
        }
        ToolPanel::Primitives => {
            ui.label("Size");
            ui.add(TextEdit::singleline(state.primitive_size_mut()).desired_width(72.0).hint_text("mm"));
            ui.label("mm");
            if let Some(error) = state.error() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let size = state.primitive_size();
            let enabled = size.is_ok();
            let armed_size = size.as_ref().ok().copied();
            for (kind, icon, label) in [
                (PrimitiveKind::Cube, icons::primitives::CUBE, "Cube"),
                (PrimitiveKind::Cylinder, icons::primitives::CYLINDER, "Cylinder"),
                (PrimitiveKind::Sphere, icons::primitives::SPHERE, "Sphere"),
            ] {
                let on = armed_size.is_some_and(|size| is_armed(active, ToolAction::CreatePrimitive { kind, size }));
                if icons::tool_button(ui, icon, &format!("{label}: click the work plane"), None, enabled, on).clicked()
                    && let Ok(size) = size
                {
                    actions.push(ToolAction::CreatePrimitive { kind, size });
                }
            }
        }
        ToolPanel::Bodies => {
            ui.label("Height");
            ui.add(TextEdit::singleline(state.extrude_height_mut()).desired_width(72.0).hint_text("mm"));
            ui.label("mm");
            let height = state.extrude_height();
            if let Err(error) = &height {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            // Only a closed path encloses a ring, so the button is off for anything else and the
            // hint names what is missing: a selection, a path, a closed chain, or a usable height.
            let hint = extrude_hint(selected, selected_path);
            let enabled = matches!((selected, selected_path), (Some(_), Some(true))) && height.is_ok();
            if icons::tool_button(ui, icons::bodies::EXTRUDE, hint, None, enabled, false).clicked()
                && let (Some(object), Ok(height)) = (selected, height)
            {
                actions.push(ToolAction::ExtrudePath { object, height });
            }
        }
    });
}

/// The hint of the extrude button: what the tool does, or why it is off.
///
/// `selected_path` is `None` when the selection is not a path object, so a path that is merely
/// open gets its own message instead of being called "not a path".
fn extrude_hint(selected: Option<Id>, selected_path: Option<bool>) -> &'static str {
    match (selected, selected_path) {
        (None, _) => "Select a closed path first",
        (Some(_), Some(true)) => "Extrude the selected path",
        (Some(_), Some(false)) => "The selected path is open; close it first",
        (Some(_), None) => "The selected object is not a path",
    }
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
        ctx.enable_accesskit();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, None, None, None, None, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        for panel in ToolPanel::ALL {
            assert!(texts.iter().any(|text| text == panel.label()), "{} heading is missing", panel.label());
        }
        assert!(
            labels.iter().any(|label| label.contains("Panel  (N)")),
            "a drawn icon keeps the labelled shortcut as its accessible name: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label.contains("Cube: click the work plane")),
            "the primitives panel is filled: {labels:?}"
        );
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn the_strip_names_the_armed_container_and_offers_to_clear_it() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, None, None, None, Some("Frame"), &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Adding to: Frame")), "the chip names the arm: {texts:?}");
        assert!(texts.iter().any(|text| text.contains('✕')), "the chip carries its clear button: {texts:?}");
        assert!(actions.is_empty(), "drawing alone clears nothing");
    }

    #[test]
    fn an_idle_strip_shows_no_creation_chip() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, None, None, None, None, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(!texts.iter().any(|text| text.contains("Adding to")), "nothing is armed: {texts:?}");
    }

    #[test]
    fn only_the_arming_tool_owns_the_next_click() {
        let cases = [
            (None, None, None),
            (Some(DrawTool::Line), None, Some(ToolActivity::Draw(DrawTool::Line))),
            (None, Some(PrimitiveKind::Cube), Some(ToolActivity::Primitive(PrimitiveKind::Cube))),
            (Some(DrawTool::Arc), Some(PrimitiveKind::Sphere), Some(ToolActivity::Draw(DrawTool::Arc))),
        ];
        for (drawing, primitive, expected) in cases {
            assert_eq!(tool_activity(drawing, primitive), expected, "drawing={drawing:?} primitive={primitive:?}");
        }
    }

    #[test]
    fn a_button_is_pressed_only_for_its_own_activity() {
        let size = Length::from_millimetres(DEFAULT_PRIMITIVE_SIZE_MM).expect("default size");
        let cube = ToolAction::CreatePrimitive { kind: PrimitiveKind::Cube, size };
        let cases = [
            (None, ToolAction::Draw(DrawTool::Line), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::Draw(DrawTool::Line), true),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::Draw(DrawTool::Circle), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::CreatePanel, false),
            (Some(ToolActivity::Draw(DrawTool::Line)), cube, false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), cube, true),
            (
                Some(ToolActivity::Primitive(PrimitiveKind::Cube)),
                ToolAction::CreatePrimitive { kind: PrimitiveKind::Sphere, size },
                false,
            ),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), ToolAction::CreateLight, false),
        ];
        for (activity, action, expected) in cases {
            assert_eq!(is_armed(activity, action), expected, "activity={activity:?} action={action:?}");
        }
    }

    #[test]
    fn the_strip_marks_exactly_the_armed_tool() {
        let cases = [
            (None, 0, "an idle strip presses nothing"),
            (Some(ToolActivity::Draw(DrawTool::Line)), 1, "the armed Line button is pressed"),
            (Some(ToolActivity::Primitive(PrimitiveKind::Sphere)), 1, "the armed Sphere button is pressed"),
        ];
        for (activity, expected, reason) in cases {
            let ctx = egui::Context::default();
            let mut state = Toolbars::new();
            let mut actions = Vec::new();
            let mut pressed_fill = None;
            let output = ctx.run_ui(Default::default(), |ui| {
                pressed_fill = Some(ui.visuals().selection.bg_fill);
                show(ui, &mut state, true, None, None, activity, None, &mut actions);
            });
            let pressed_fill = pressed_fill.expect("the strip ran");
            let pressed = drawn_fill(&output.shapes, pressed_fill);
            output.drop_without_applying_deltas();
            assert_eq!(pressed, expected, "{reason}");
        }
    }

    #[test]
    fn the_bodies_panel_only_offers_extrude_for_a_closed_path() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, None, None, None, None, &mut actions);
        });
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        assert!(labels.iter().any(|label| label.contains("Select a closed path first")));
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(18).expect("height")));
    }

    #[test]
    fn the_strip_names_the_arc_gesture_on_the_polyline() {
        assert_eq!(strip_label(DrawTool::Polyline), "Polyline · Alt+click for an arc");
        for tool in CONTOUR_TOOLS {
            if tool != DrawTool::Polyline {
                assert_eq!(strip_label(tool), tool.label(), "only the polyline mixes segment kinds");
            }
        }
    }

    #[test]
    fn the_extrude_hint_names_what_is_missing() {
        let node = Id::new(7).expect("id");
        assert_eq!(extrude_hint(None, None), "Select a closed path first");
        assert_eq!(extrude_hint(None, Some(true)), "Select a closed path first");
        assert_eq!(extrude_hint(Some(node), None), "The selected object is not a path");
        assert_eq!(extrude_hint(Some(node), Some(false)), "The selected path is open; close it first");
        assert_eq!(extrude_hint(Some(node), Some(true)), "Extrude the selected path");
    }

    #[test]
    fn an_open_path_leaves_the_extrude_button_off() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let node = Id::new(7).expect("id");
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, Some(node), Some(false), None, None, &mut actions);
        });
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        assert!(
            labels.iter().any(|label| label.contains("The selected path is open; close it first")),
            "the panel says why an open path cannot be extruded: {labels:?}"
        );
        assert!(actions.is_empty(), "a disabled button asks for no action");
    }

    #[test]
    fn a_closed_path_asks_for_one_extrusion_at_the_typed_height() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = Toolbars::new();
        state.set_extrude_height_text("24");
        let mut actions = Vec::new();
        let node = Id::new(7).expect("id");
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, Some(node), Some(true), None, None, &mut actions);
        });
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        assert!(labels.iter().any(|label| label.contains("Extrude the selected path")));
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(24).expect("height")));
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn a_blank_extrusion_height_is_refused_in_the_panel() {
        let mut state = Toolbars::new();
        state.set_extrude_height_text("   ");
        assert!(state.extrude_height().is_err());
        state.set_extrude_height_text("0");
        assert!(state.extrude_height().is_err());
        state.set_extrude_height_text("12");
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(12).expect("height")));
    }

    #[test]
    fn the_height_field_stops_where_the_command_does() {
        let mut state = Toolbars::new();
        // The document's contour limit is 10 m; the field must not accept a height the command
        // would refuse, or the panel would offer a tool that always fails.
        let max = furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000;
        state.set_extrude_height_text(max.to_string());
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(max).expect("height")));
        state.set_extrude_height_text((max + 1).to_string());
        assert!(state.extrude_height().is_err(), "one millimetre past the limit is refused");
    }

    #[test]
    fn the_three_primitive_buttons_are_filled() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, true, None, None, None, None, &mut actions);
        });
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        for label in ["Cube: click the work plane", "Cylinder: click the work plane", "Sphere: click the work plane"] {
            assert!(labels.iter().any(|text| text.contains(label)), "{label} is missing: {labels:?}");
        }
    }

    #[test]
    fn a_panel_without_a_catalogue_material_cannot_create_a_panel() {
        let ctx = egui::Context::default();
        let mut state = Toolbars::new();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, false, None, None, None, None, &mut actions);
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
            show(ui, &mut state, true, None, None, None, None, &mut actions);
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
            show(ui, &mut state, true, None, None, None, None, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Contour")), "a closed panel keeps its heading: {texts:?}");
        assert!(!texts.iter().any(|text| text == "Line  (L)"), "the body is not drawn: {texts:?}");
    }

    /// Counts the painted rects filled with `fill`, the selection fill of a pressed button.
    fn drawn_fill(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> usize {
        fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut usize) {
            match shape {
                egui::Shape::Rect(rect) if rect.fill == fill => *out += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, fill, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, fill, &mut out);
        }
        out
    }

    /// The accessible labels this frame published: a drawn icon carries its hint here, not as text.
    fn accessible_labels(output: &egui::FullOutput) -> Vec<String> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| update.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect())
            .unwrap_or_default()
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
