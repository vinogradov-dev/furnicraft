//! The selected-panel card.
//!
//! The card renders session state and turns edits into engine commands. Text fields commit on Enter,
//! on losing focus or through an explicit button; combo boxes and checkboxes dispatch immediately.

use egui::{Color32, ComboBox, RichText, TextEdit, Ui};
use furnicraft_engine::panel_cmds::{
    EDGE_BANDING_CLEAR_ID, EDGE_BANDING_SET_ID, MATERIAL_SET_ID, RENAME_ID, RESIZE_ID,
};
use furnicraft_engine::{
    PanelDiagnostic, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMaterialSet, PanelRename, PanelResize, Session,
};
use furnicraft_model::{EdgeBanding, EdgeId, Id, MaterialId, Panel, PanelNominalSize};
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
    Undo,
    Redo,
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
        self.show_geometry(ui, session, panel_id);
        self.show_bands(ui, session, panel_id, panel.edge_banding(), &mut actions);
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

    fn show_bands(
        &self,
        ui: &mut Ui,
        session: &Session,
        panel_id: Id,
        bands: &[EdgeBanding],
        actions: &mut Vec<Action>,
    ) {
        ui.separator();
        ui.label(RichText::new("Edge banding").strong());
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
                ComboBox::from_id_salt(("edge_material", panel_id.get(), edge)).selected_text(selected_text).show_ui(
                    ui,
                    |ui| {
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
                    },
                );
                if let Some(band) = band {
                    let mut included = band.included_in_size();
                    let material_known = session.catalog().edge_material(band.material()).is_some();
                    if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, "Included")).changed() {
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
}
