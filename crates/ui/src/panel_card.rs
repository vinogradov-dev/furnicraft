//! The selected-panel card.
//!
//! The card renders session state and turns edits into engine commands. Text fields commit on Enter,
//! on losing focus or through an explicit button; combo boxes and checkboxes dispatch immediately.

use egui::{Color32, ComboBox, RichText, TextEdit, Ui};
use furnicraft_engine::panel_cmds::{
    BOW_CLEAR_ID, BOW_SET_ID, CORNER_CLEAR_ID, CORNER_SET_ID, EDGE_BANDING_CLEAR_ID, EDGE_BANDING_SET_ID,
    MATERIAL_SET_ID, RENAME_ID, RESIZE_ID,
};
use furnicraft_engine::{
    PanelBowClear, PanelBowSet, PanelCornerClear, PanelCornerSet, PanelDiagnostic, PanelEdgeBandingClear,
    PanelEdgeBandingSet, PanelMaterialSet, PanelRename, PanelResize, Session,
};
use furnicraft_model::{
    CornerFeature, CornerFeatureKind, EdgeBanding, EdgeId, Id, MaterialId, Panel, PanelNominalSize, RectCorner,
    RectEdge, VertexId,
};
use furnicraft_units::Length;
use serde_json::Value;

/// Persistent state of the panel card.
#[derive(Debug, Default)]
pub(crate) struct PanelCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// Editable text that survives frames until the user commits it.
#[derive(Clone, Debug)]
struct Draft {
    panel: Id,
    name: String,
    length: String,
    width: String,
}

impl Draft {
    fn from_panel(panel: &Panel) -> Self {
        let nominal = panel.nominal_size();
        Self {
            panel: panel.id(),
            name: panel.name().to_owned(),
            length: nominal.length().to_string(),
            width: nominal.width().to_string(),
        }
    }
}

/// One command the card wants to dispatch after the immutable UI borrow ends.
#[derive(Clone, Debug)]
enum Action {
    Rename { panel: Id, name: String },
    Resize { panel: Id, nominal_size: PanelNominalSize },
    SetMaterial { panel: Id, material: MaterialId },
    SetBand { panel: Id, edge: EdgeId, material: MaterialId, included_in_size: bool },
    ClearBand { panel: Id, edge: EdgeId },
    SetCorner { panel: Id, corner: VertexId, feature: CornerFeatureKind },
    ClearCorner { panel: Id, corner: VertexId },
    SetBow { panel: Id, edge: EdgeId, sagitta: Length },
    ClearBow { panel: Id, edge: EdgeId },
    Undo,
    Redo,
}

/// The three states of a corner in the outline editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CornerMode {
    None,
    Radius,
    Chamfer,
}

impl CornerMode {
    const ALL: [Self; 3] = [Self::None, Self::Radius, Self::Chamfer];

    const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Radius => "Radius",
            Self::Chamfer => "Chamfer",
        }
    }
}

impl PanelCard {
    /// Draws the card for the current selection and dispatches any accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, selected: &mut Option<Id>) {
        if let Some(panel_id) = *selected
            && session.project().panel(panel_id).is_none()
        {
            *selected = None;
            self.draft = None;
        }

        let Some(panel_id) = *selected else {
            self.draft = None;
            self.error = None;
            ui.label("Select a panel in the outliner.");
            return;
        };

        if self.draft.as_ref().is_none_or(|draft| draft.panel != panel_id) {
            self.error = None;
            self.draft = session.project().panel(panel_id).map(Draft::from_panel);
        }
        let Some(panel) = session.project().panel(panel_id) else {
            self.draft = None;
            self.error = None;
            ui.label("Select a panel in the outliner.");
            return;
        };
        let Some(draft) = self.draft.as_mut() else {
            ui.label("Select a panel in the outliner.");
            return;
        };

        let mut actions = Vec::new();
        let mut errors = Vec::new();
        ui.horizontal(|ui| {
            if ui.add_enabled(session.can_undo(), egui::Button::new("Undo")).clicked() {
                actions.push(Action::Undo);
            }
            if ui.add_enabled(session.can_redo(), egui::Button::new("Redo")).clicked() {
                actions.push(Action::Redo);
            }
            if session.is_dirty() {
                ui.label(RichText::new("Unsaved").italics());
            }
        });
        ui.separator();

        ui.label(RichText::new("Name").strong());
        ui.horizontal(|ui| {
            let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
            let clicked = ui.button("Rename").clicked();
            if response.lost_focus() || clicked {
                let name = draft.name.trim();
                if name.is_empty() {
                    errors.push("Panel name must not be empty.".to_owned());
                } else if name != panel.name() {
                    actions.push(Action::Rename { panel: panel_id, name: name.to_owned() });
                }
            }
        });

        ui.label(RichText::new("Nominal size").strong());
        ui.horizontal(|ui| {
            ui.label("Length");
            let length_response = ui.add(TextEdit::singleline(&mut draft.length).desired_width(80.0).hint_text("mm"));
            ui.label("Width");
            let width_response = ui.add(TextEdit::singleline(&mut draft.width).desired_width(80.0).hint_text("mm"));
            let clicked = ui.button("Apply size").clicked();
            if length_response.lost_focus() || width_response.lost_focus() || clicked {
                match (Length::parse_millimetres(&draft.length), Length::parse_millimetres(&draft.width)) {
                    (Ok(length), Ok(width)) => match PanelNominalSize::new(length, width) {
                        Ok(nominal_size) if nominal_size != panel.nominal_size() => {
                            actions.push(Action::Resize { panel: panel_id, nominal_size });
                        }
                        Ok(_) => {}
                        Err(error) => errors.push(error.to_string()),
                    },
                    (Err(error), _) => errors.push(format!("Length: {error}")),
                    (_, Err(error)) => errors.push(format!("Width: {error}")),
                }
            }
        });

        ui.separator();
        self.show_material(ui, session, panel_id, panel.material(), &mut actions);
        self.show_shape(ui, session, panel_id, panel, &mut actions);
        self.show_geometry(ui, session, panel_id);
        self.show_bands(ui, session, panel_id, panel, &mut actions);
        self.show_diagnostics(ui, session, panel_id);

        if let Some(error) = &self.error {
            ui.separator();
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if !errors.is_empty() {
            self.error = Some(errors.join("\n"));
            ui.ctx().request_repaint();
            return;
        }
        if !actions.is_empty() {
            self.error = None;
            for action in actions {
                if let Err(error) = run_action(session, &action) {
                    self.error = Some(error);
                    break;
                }
                if matches!(action, Action::Rename { .. } | Action::Resize { .. } | Action::Undo | Action::Redo) {
                    self.draft = None;
                }
            }
            ui.ctx().request_repaint();
        }
    }

    fn show_material(
        &self,
        ui: &mut Ui,
        session: &Session,
        panel_id: Id,
        current: Option<MaterialId>,
        actions: &mut Vec<Action>,
    ) {
        ui.label(RichText::new("Material").strong());
        if let Some(material) = current
            && session.catalog().panel_material(material).is_none()
        {
            ui.colored_label(Color32::LIGHT_RED, format!("Panel material {material} is missing from the catalogue."));
        }
        let selected_text = match current {
            Some(material) => session
                .catalog()
                .panel_material(material)
                .map_or_else(|| format!("Missing material {material}"), |material| material.name().to_owned()),
            None => "No material".to_owned(),
        };
        ComboBox::from_id_salt(("panel_material", panel_id.get())).selected_text(selected_text).show_ui(ui, |ui| {
            for material in session.catalog().panel_materials() {
                let selected = current == Some(material.id());
                if ui.selectable_label(selected, material.name()).clicked() && !selected {
                    actions.push(Action::SetMaterial { panel: panel_id, material: material.id() });
                }
            }
        });
    }

    fn show_geometry(&self, ui: &mut Ui, session: &Session, panel_id: Id) {
        ui.separator();
        ui.label(RichText::new("Resolved").strong());
        match session.panel_geometry(panel_id) {
            Ok(geometry) => {
                ui.label(format!("Thickness  {}", geometry.thickness()));
                ui.label(format!("Core  {} × {}", geometry.core_length(), geometry.core_width()));
                ui.label(format!("Overall  {} × {}", geometry.overall_length(), geometry.overall_width()));
            }
            Err(error) => {
                ui.colored_label(Color32::LIGHT_RED, format!("Geometry: {error}"));
            }
        }
    }

    fn show_shape(&self, ui: &mut Ui, _session: &Session, panel_id: Id, panel: &Panel, actions: &mut Vec<Action>) {
        ui.separator();
        ui.label(RichText::new("Outline").strong());
        let locked = !panel.edge_banding().is_empty() || !panel.machining().is_empty();
        if locked {
            ui.colored_label(Color32::LIGHT_RED, "Clear edge banding and machining before shaping the outline.");
        }
        ui.add_enabled_ui(!locked, |ui| {
            for corner in RectCorner::ALL {
                let corner_id = corner.id();
                let current = panel.corner_feature(corner_id);
                let mode = current.map_or(CornerMode::None, |feature| match feature.feature() {
                    CornerFeatureKind::Radius { .. } => CornerMode::Radius,
                    CornerFeatureKind::Chamfer { .. } => CornerMode::Chamfer,
                });
                ui.horizontal(|ui| {
                    ui.label(format!("Corner {}", corner.label()));
                    let mut selected = mode;
                    ComboBox::from_id_salt(("corner_mode", panel_id.get(), corner_id.get()))
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for option in CornerMode::ALL {
                                if ui.selectable_label(selected == option, option.label()).clicked()
                                    && selected != option
                                {
                                    selected = option;
                                    if option == CornerMode::None {
                                        if current.is_some() {
                                            actions.push(Action::ClearCorner { panel: panel_id, corner: corner_id });
                                        }
                                    } else if let Some(feature) = corner_feature_for_mode(option, current) {
                                        actions.push(Action::SetCorner { panel: panel_id, corner: corner_id, feature });
                                    }
                                }
                            }
                        });
                    match current.map(CornerFeature::feature) {
                        Some(CornerFeatureKind::Radius { radius }) => {
                            let mut millimetres = radius.millimetres_f64();
                            let response = ui.add(
                                egui::DragValue::new(&mut millimetres).speed(0.5).range(0.1..=1_000.0).suffix(" mm"),
                            );
                            if (response.drag_stopped() || response.lost_focus())
                                && let Some(value) = positive_length(millimetres)
                                && value != radius
                            {
                                actions.push(Action::SetCorner {
                                    panel: panel_id,
                                    corner: corner_id,
                                    feature: CornerFeatureKind::Radius { radius: value },
                                });
                            }
                        }
                        Some(CornerFeatureKind::Chamfer { incoming, outgoing }) => {
                            let mut incoming_mm = incoming.millimetres_f64();
                            let mut outgoing_mm = outgoing.millimetres_f64();
                            ui.label("in");
                            let incoming_response = ui.add(
                                egui::DragValue::new(&mut incoming_mm).speed(0.5).range(0.1..=1_000.0).suffix(" mm"),
                            );
                            ui.label("out");
                            let outgoing_response = ui.add(
                                egui::DragValue::new(&mut outgoing_mm).speed(0.5).range(0.1..=1_000.0).suffix(" mm"),
                            );
                            let committed = incoming_response.drag_stopped()
                                || outgoing_response.drag_stopped()
                                || incoming_response.lost_focus()
                                || outgoing_response.lost_focus();
                            if committed
                                && let (Some(incoming_value), Some(outgoing_value)) =
                                    (positive_length(incoming_mm), positive_length(outgoing_mm))
                                && (incoming_value != incoming || outgoing_value != outgoing)
                            {
                                actions.push(Action::SetCorner {
                                    panel: panel_id,
                                    corner: corner_id,
                                    feature: CornerFeatureKind::Chamfer {
                                        incoming: incoming_value,
                                        outgoing: outgoing_value,
                                    },
                                });
                            }
                        }
                        None => {}
                    }
                });
            }

            ui.separator();
            ui.label(RichText::new("Bowed edges").strong());
            for edge in RectEdge::ALL {
                let edge_id = edge.id();
                let current = panel.edge_bow(edge_id);
                ui.horizontal(|ui| {
                    ui.label(edge_label(edge_id.get()));
                    let mut millimetres = current.map_or(0.0, |bow| bow.sagitta().millimetres_f64());
                    let response = ui
                        .add(egui::DragValue::new(&mut millimetres).speed(0.5).range(-1_000.0..=1_000.0).suffix(" mm"));
                    if (response.drag_stopped() || response.lost_focus())
                        && let Some(sagitta) = signed_length(millimetres)
                    {
                        if sagitta.is_zero() {
                            if current.is_some() {
                                actions.push(Action::ClearBow { panel: panel_id, edge: edge_id });
                            }
                        } else if current.is_none_or(|bow| bow.sagitta() != sagitta) {
                            actions.push(Action::SetBow { panel: panel_id, edge: edge_id, sagitta });
                        }
                    }
                    if current.is_some() && ui.button("Clear").clicked() {
                        actions.push(Action::ClearBow { panel: panel_id, edge: edge_id });
                    }
                });
            }
        });
    }

    fn show_bands(&self, ui: &mut Ui, session: &Session, panel_id: Id, panel: &Panel, actions: &mut Vec<Action>) {
        ui.separator();
        ui.label(RichText::new("Edge banding").strong());
        let shaped = panel.has_contour_features();
        if shaped {
            ui.colored_label(
                Color32::LIGHT_RED,
                "New banding on shaped panels is not available yet; existing bands can be cleared.",
            );
        }
        let bands = panel.edge_banding();
        for edge in 1..=4 {
            let Some(edge_id) = EdgeId::new(edge) else {
                continue;
            };
            let band = bands.iter().find(|band| band.edge() == edge_id);
            ui.horizontal(|ui| {
                ui.label(edge_label(edge));
                let selected_text = band.map_or_else(
                    || "None".to_owned(),
                    |band| {
                        session.catalog().edge_material(band.material()).map_or_else(
                            || format!("Missing material {}", band.material()),
                            |material| material.name().to_owned(),
                        )
                    },
                );
                ui.add_enabled_ui(!shaped, |ui| {
                    ComboBox::from_id_salt(("edge_material", panel_id.get(), edge))
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(band.is_none(), "None").clicked() && band.is_some() {
                                actions.push(Action::ClearBand { panel: panel_id, edge: edge_id });
                            }
                            for material in session.catalog().edge_materials() {
                                let selected = band.is_some_and(|band| band.material() == material.id());
                                if ui.selectable_label(selected, material.name()).clicked() && !selected {
                                    actions.push(Action::SetBand {
                                        panel: panel_id,
                                        edge: edge_id,
                                        material: material.id(),
                                        included_in_size: band.is_some_and(EdgeBanding::included_in_size),
                                    });
                                }
                            }
                        });
                });
                if let Some(band) = band {
                    let mut included = band.included_in_size();
                    let material_known = session.catalog().edge_material(band.material()).is_some();
                    if ui
                        .add_enabled(!shaped && material_known, egui::Checkbox::new(&mut included, "Included"))
                        .changed()
                    {
                        actions.push(Action::SetBand {
                            panel: panel_id,
                            edge: edge_id,
                            material: band.material(),
                            included_in_size: included,
                        });
                    }
                    if ui.button("Clear").clicked() {
                        actions.push(Action::ClearBand { panel: panel_id, edge: edge_id });
                    }
                }
            });
        }
    }

    fn show_diagnostics(&self, ui: &mut Ui, session: &Session, panel_id: Id) {
        ui.separator();
        ui.label(RichText::new("Diagnostics").strong());
        let diagnostics = session.panel_diagnostics(panel_id);
        if diagnostics.is_empty() {
            ui.label("No issues.");
            return;
        }
        for diagnostic in diagnostics {
            ui.colored_label(Color32::LIGHT_RED, diagnostic_message(&diagnostic));
        }
    }
}

fn run_action(session: &mut Session, action: &Action) -> Result<(), String> {
    match action {
        Action::Rename { panel, name } => {
            let params = json(&PanelRename { panel: *panel, name: name.clone() })?;
            dispatch(session, RENAME_ID, &params)
        }
        Action::Resize { panel, nominal_size } => {
            let params = json(&PanelResize { panel: *panel, nominal_size: *nominal_size })?;
            dispatch(session, RESIZE_ID, &params)
        }
        Action::SetMaterial { panel, material } => {
            let params = json(&PanelMaterialSet { panel: *panel, material: *material })?;
            dispatch(session, MATERIAL_SET_ID, &params)
        }
        Action::SetBand { panel, edge, material, included_in_size } => {
            let params = json(&PanelEdgeBandingSet {
                panel: *panel,
                edge: *edge,
                material: *material,
                included_in_size: *included_in_size,
            })?;
            dispatch(session, EDGE_BANDING_SET_ID, &params)
        }
        Action::ClearBand { panel, edge } => {
            let params = json(&PanelEdgeBandingClear { panel: *panel, edge: *edge })?;
            dispatch(session, EDGE_BANDING_CLEAR_ID, &params)
        }
        Action::SetCorner { panel, corner, feature } => {
            let params = json(&PanelCornerSet { panel: *panel, corner: *corner, feature: *feature })?;
            dispatch(session, CORNER_SET_ID, &params)
        }
        Action::ClearCorner { panel, corner } => {
            let params = json(&PanelCornerClear { panel: *panel, corner: *corner })?;
            dispatch(session, CORNER_CLEAR_ID, &params)
        }
        Action::SetBow { panel, edge, sagitta } => {
            let params = json(&PanelBowSet { panel: *panel, edge: *edge, sagitta: *sagitta })?;
            dispatch(session, BOW_SET_ID, &params)
        }
        Action::ClearBow { panel, edge } => {
            let params = json(&PanelBowClear { panel: *panel, edge: *edge })?;
            dispatch(session, BOW_CLEAR_ID, &params)
        }
        Action::Undo => session.undo().map_err(|error| error.to_string()),
        Action::Redo => session.redo().map_err(|error| error.to_string()),
    }
}

fn dispatch(session: &mut Session, id: &str, params: &Value) -> Result<(), String> {
    session.dispatch(id, params).map(|_| ()).map_err(|error| error.to_string())
}

fn json<T: serde::Serialize>(params: &T) -> Result<Value, String> {
    serde_json::to_value(params).map_err(|error| format!("could not encode command parameters: {error}"))
}

fn edge_label(edge: u32) -> &'static str {
    match edge {
        1 => "Bottom",
        2 => "Right",
        3 => "Top",
        4 => "Left",
        _ => "Edge",
    }
}

fn corner_feature_for_mode(mode: CornerMode, current: Option<CornerFeature>) -> Option<CornerFeatureKind> {
    match mode {
        CornerMode::None => None,
        CornerMode::Radius => {
            let radius = match current.map(CornerFeature::feature) {
                Some(CornerFeatureKind::Radius { radius }) => radius,
                _ => default_feature_length(),
            };
            Some(CornerFeatureKind::Radius { radius })
        }
        CornerMode::Chamfer => {
            let (incoming, outgoing) = match current.map(CornerFeature::feature) {
                Some(CornerFeatureKind::Chamfer { incoming, outgoing }) => (incoming, outgoing),
                _ => (default_feature_length(), default_feature_length()),
            };
            Some(CornerFeatureKind::Chamfer { incoming, outgoing })
        }
    }
}

fn default_feature_length() -> Length {
    Length::from_micrometres(10_000)
}

fn positive_length(millimetres: f64) -> Option<Length> {
    let value = millimetres_to_length(millimetres)?;
    value.is_positive().then_some(value)
}

fn signed_length(millimetres: f64) -> Option<Length> {
    millimetres_to_length(millimetres)
}

fn millimetres_to_length(millimetres: f64) -> Option<Length> {
    if !millimetres.is_finite() || millimetres.abs() > 1_000_000.0 {
        return None;
    }
    let micrometres = (millimetres * 1000.0).round();
    if !micrometres.is_finite() {
        return None;
    }
    Some(Length::from_micrometres(micrometres as i64))
}

/// Formats one derived panel issue for the card.
pub(crate) fn diagnostic_message(diagnostic: &PanelDiagnostic) -> String {
    match diagnostic {
        PanelDiagnostic::NoMaterial => "No panel material is assigned.".to_owned(),
        PanelDiagnostic::MissingMaterial { material } => {
            format!("Panel material {material} is missing from the catalogue.")
        }
        PanelDiagnostic::MissingBandingMaterial { edge, material } => {
            format!("Edge {edge} uses missing edge material {material}.")
        }
        PanelDiagnostic::NonPositiveCore => "The panel core is not positive.".to_owned(),
        PanelDiagnostic::MachiningOutsideCore { cutout } => {
            format!("Cutout {cutout} does not fit inside the core.")
        }
        PanelDiagnostic::BlindDepthNotLessThanThickness { cutout, depth, thickness } => {
            format!("Cutout {cutout} blind depth {depth} is not less than thickness {thickness}.")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn material(value: u64) -> MaterialId {
        MaterialId::new(value).expect("material id")
    }

    #[test]
    fn diagnostics_have_readable_messages() {
        assert_eq!(diagnostic_message(&PanelDiagnostic::NoMaterial), "No panel material is assigned.");
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::MissingMaterial { material: material(7) }),
            "Panel material 7 is missing from the catalogue."
        );
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::MissingBandingMaterial {
                edge: EdgeId::new(2).expect("edge"),
                material: material(101),
            }),
            "Edge 2 uses missing edge material 101."
        );
        assert_eq!(diagnostic_message(&PanelDiagnostic::NonPositiveCore), "The panel core is not positive.");
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::MachiningOutsideCore { cutout: Id::new(9).expect("id") }),
            "Cutout 9 does not fit inside the core."
        );
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::BlindDepthNotLessThanThickness {
                cutout: Id::new(9).expect("id"),
                depth: Length::from_millimetres(20).expect("depth"),
                thickness: Length::from_millimetres(18).expect("thickness"),
            }),
            "Cutout 9 blind depth 20 mm is not less than thickness 18 mm."
        );
    }

    #[test]
    fn edge_labels_match_the_documented_numbering() {
        assert_eq!([edge_label(1), edge_label(2), edge_label(3), edge_label(4)], ["Bottom", "Right", "Top", "Left"]);
    }

    #[test]
    fn card_actions_dispatch_through_the_engine() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");

        run_action(&mut session, &Action::Rename { panel: panel_id, name: "Renamed".to_owned() }).expect("rename");
        assert_eq!(session.project().panel(panel_id).expect("panel").name(), "Renamed");

        let material = furnicraft_testkit::sample_material_id();
        run_action(&mut session, &Action::SetMaterial { panel: panel_id, material }).expect("set material");
        assert_eq!(session.project().panel(panel_id).expect("panel").material(), Some(material));

        let edge = EdgeId::new(1).expect("edge");
        run_action(
            &mut session,
            &Action::SetBand {
                panel: panel_id,
                edge,
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: true,
            },
        )
        .expect("set band");
        assert!(
            session.project().panel(panel_id).expect("panel").edge_banding().iter().any(|band| band.edge() == edge)
        );

        run_action(&mut session, &Action::Undo).expect("undo");
        assert!(session.project().panel(panel_id).expect("panel").edge_banding().is_empty());
    }

    #[test]
    fn corner_and_bow_actions_dispatch_through_the_engine() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        let corner = RectCorner::TopRight.id();
        let edge = RectEdge::Top.id();

        run_action(
            &mut session,
            &Action::SetCorner {
                panel: panel_id,
                corner,
                feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(25).expect("radius") },
            },
        )
        .expect("set corner");
        assert!(session.project().panel(panel_id).expect("panel").corner_feature(corner).is_some());

        run_action(
            &mut session,
            &Action::SetBow { panel: panel_id, edge, sagitta: Length::from_millimetres(12).expect("sagitta") },
        )
        .expect("set bow");
        assert!(session.project().panel(panel_id).expect("panel").edge_bow(edge).is_some());

        run_action(&mut session, &Action::ClearBow { panel: panel_id, edge }).expect("clear bow");
        assert!(session.project().panel(panel_id).expect("panel").edge_bow(edge).is_none());

        run_action(&mut session, &Action::ClearCorner { panel: panel_id, corner }).expect("clear corner");
        assert!(session.project().panel(panel_id).expect("panel").corner_feature(corner).is_none());
    }

    #[test]
    fn corner_modes_start_from_existing_values_and_sane_defaults() {
        let default = default_feature_length();
        assert_eq!(
            corner_feature_for_mode(CornerMode::Radius, None),
            Some(CornerFeatureKind::Radius { radius: default })
        );
        assert_eq!(
            corner_feature_for_mode(CornerMode::Chamfer, None),
            Some(CornerFeatureKind::Chamfer { incoming: default, outgoing: default })
        );
        assert_eq!(corner_feature_for_mode(CornerMode::None, None), None);

        let existing = CornerFeature::new(
            RectCorner::BottomLeft.id(),
            CornerFeatureKind::Chamfer {
                incoming: Length::from_millimetres(8).expect("incoming"),
                outgoing: Length::from_millimetres(14).expect("outgoing"),
            },
        )
        .expect("feature");
        assert_eq!(corner_feature_for_mode(CornerMode::Chamfer, Some(existing)), Some(existing.feature()));
        assert_eq!(positive_length(0.0), None);
        assert_eq!(positive_length(-5.0), None);
        assert!(signed_length(-5.0).is_some());
    }
}
