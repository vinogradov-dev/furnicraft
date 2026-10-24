//! The selected-panel card.
//!
//! The card renders session state and turns edits into engine commands. Text fields commit on Enter,
//! on losing focus or through an explicit button; combo boxes and checkboxes dispatch immediately.

use egui::{Color32, ComboBox, RichText, TextEdit, Ui};
use furnicraft_engine::panel_cmds::{
    BOW_CLEAR_ID, BOW_SET_ID, CORNER_BANDING_CLEAR_ID, CORNER_BANDING_SET_ID, CORNER_CLEAR_ID, CORNER_SET_ID,
    EDGE_BANDING_CLEAR_ID, EDGE_BANDING_SET_ID, MATERIAL_SET_ID, RENAME_ID, RESIZE_ID,
};
use furnicraft_engine::{
    PanelBowClear, PanelBowSet, PanelCornerBandingClear, PanelCornerBandingSet, PanelCornerClear, PanelCornerSet,
    PanelDiagnostic, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMaterialSet, PanelRename, PanelResize, Session,
};
use furnicraft_model::{
    CornerBanding, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeId, Id, MaterialId, Panel, PanelNominalSize,
    RectCorner, RectEdge, SegmentSource, VertexId,
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
    SetCornerBand { panel: Id, corner: VertexId, material: MaterialId, included_in_size: bool },
    ClearCornerBand { panel: Id, corner: VertexId },
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
        ui.scope(|ui| {
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
                            let (committed, _) = millimetre_field(ui, radius, 0.1..=1_000.0);
                            let value = committed.and_then(positive_length);
                            if let Some(value) = value
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
                            ui.label("in");
                            let (incoming_committed, _) = millimetre_field(ui, incoming, 0.1..=1_000.0);
                            ui.label("out");
                            let (outgoing_committed, _) = millimetre_field(ui, outgoing, 0.1..=1_000.0);
                            let incoming_value = incoming_committed.and_then(positive_length);
                            let outgoing_value = outgoing_committed.and_then(positive_length);
                            if (incoming_value.is_some() || outgoing_value.is_some())
                                && (incoming_value.unwrap_or(incoming) != incoming
                                    || outgoing_value.unwrap_or(outgoing) != outgoing)
                            {
                                actions.push(Action::SetCorner {
                                    panel: panel_id,
                                    corner: corner_id,
                                    feature: CornerFeatureKind::Chamfer {
                                        incoming: incoming_value.unwrap_or(incoming),
                                        outgoing: outgoing_value.unwrap_or(outgoing),
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
                    let current_sagitta = current.map_or(Length::ZERO, |bow| bow.sagitta());
                    let (committed, _) = millimetre_field(ui, current_sagitta, -1_000.0..=1_000.0);
                    if let Some(sagitta) = committed.and_then(signed_length) {
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
        ui.label(RichText::new("Banding").strong());

        let listed = listed_band_sources(panel);
        if listed.is_empty() {
            ui.label("No banding.");
        }
        for source in &listed {
            match *source {
                SegmentSource::Edge(edge) => {
                    if let Some(band) = panel.edge_banding().iter().find(|band| band.edge() == edge) {
                        show_edge_band_row(ui, session, panel_id, edge, band, actions);
                    }
                }
                SegmentSource::Corner(corner) => {
                    if let Some(band) = panel.corner_banding_at(corner) {
                        show_corner_band_row(ui, session, panel, panel_id, corner, &band, actions);
                    }
                }
            }
        }

        ui.add_space(4.0);
        let available = available_band_segments(panel);
        if available.is_empty() {
            if listed.is_empty() {
                ui.label("Add a chamfer or radius to band a segment.");
            } else {
                ui.label("All existing segments are banded.");
            }
            return;
        }
        let Some(material) = session.catalog().edge_materials().first() else {
            ui.label("Add an edge material to the catalogue first.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Add banding");
            ComboBox::from_id_salt(("add_banding", panel_id.get())).selected_text("Choose a segment").show_ui(
                ui,
                |ui| {
                    for source in &available {
                        if ui.selectable_label(false, segment_label(panel, *source)).clicked() {
                            match source {
                                SegmentSource::Edge(edge) => actions.push(Action::SetBand {
                                    panel: panel_id,
                                    edge: *edge,
                                    material: material.id(),
                                    included_in_size: false,
                                }),
                                SegmentSource::Corner(corner) => actions.push(Action::SetCornerBand {
                                    panel: panel_id,
                                    corner: *corner,
                                    material: material.id(),
                                    included_in_size: false,
                                }),
                            }
                        }
                    }
                },
            );
        });
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
        Action::SetCornerBand { panel, corner, material, included_in_size } => {
            let params = json(&PanelCornerBandingSet {
                panel: *panel,
                corner: *corner,
                material: *material,
                included_in_size: *included_in_size,
            })?;
            dispatch(session, CORNER_BANDING_SET_ID, &params)
        }
        Action::ClearCornerBand { panel, corner } => {
            let params = json(&PanelCornerBandingClear { panel: *panel, corner: *corner })?;
            dispatch(session, CORNER_BANDING_CLEAR_ID, &params)
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

/// A millimetre value that lives in the document.
///
/// The widget keeps the text the user types until the edit ends: Enter, losing focus or the end of
/// a drag. The value is read back from the document on every frame, so updating it on each
/// keystroke would replace the typed text with the document value while the user is still typing.
fn millimetre_field(ui: &mut Ui, value: Length, range: std::ops::RangeInclusive<f64>) -> (Option<f64>, egui::Response) {
    let mut millimetres = value.millimetres_f64();
    let response = ui
        .add(egui::DragValue::new(&mut millimetres).speed(0.5).range(range).suffix(" mm").update_while_editing(false));
    let committed = (response.drag_stopped() || response.lost_focus()).then_some(millimetres);
    (committed, response)
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
        PanelDiagnostic::MissingCornerBandingMaterial { corner, material } => {
            format!("Corner {corner} uses missing edge material {material}.")
        }
        PanelDiagnostic::NonPositiveCore => "The panel core is not positive.".to_owned(),
        PanelDiagnostic::MachiningOutsideCore { cutout } => {
            format!("Cutout {cutout} lies outside the core.")
        }
        PanelDiagnostic::CoreRemoved => "The cutouts remove the whole panel core.".to_owned(),
        PanelDiagnostic::CoreDisconnected => "The cutouts split the panel core into disconnected pieces.".to_owned(),
        PanelDiagnostic::BlindDepthNotLessThanThickness { cutout, depth, thickness } => {
            format!("Cutout {cutout} blind depth {depth} is not less than thickness {thickness}.")
        }
    }
}

fn listed_band_sources(panel: &Panel) -> Vec<SegmentSource> {
    let mut sources: Vec<SegmentSource> =
        panel.band_order().iter().copied().filter(|source| segment_exists(panel, *source)).collect();
    for band in panel.edge_banding() {
        let source = SegmentSource::Edge(band.edge());
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    for band in panel.corner_banding() {
        let source = SegmentSource::Corner(band.corner());
        if segment_exists(panel, source) && !sources.contains(&source) {
            sources.push(source);
        }
    }
    sources
}

fn segment_exists(panel: &Panel, source: SegmentSource) -> bool {
    match source {
        SegmentSource::Edge(edge) => matches!(edge.get(), 1..=4),
        SegmentSource::Corner(corner) => panel.corner_feature(corner).is_some(),
    }
}

fn available_band_segments(panel: &Panel) -> Vec<SegmentSource> {
    let mut available = Vec::new();
    for edge in RectEdge::ALL {
        let edge = edge.id();
        if !panel.edge_banding().iter().any(|band| band.edge() == edge) {
            available.push(SegmentSource::Edge(edge));
        }
    }
    for corner in RectCorner::ALL {
        let corner = corner.id();
        if panel.corner_feature(corner).is_some() && panel.corner_banding_at(corner).is_none() {
            available.push(SegmentSource::Corner(corner));
        }
    }
    available
}

fn segment_label(panel: &Panel, source: SegmentSource) -> String {
    match source {
        SegmentSource::Edge(edge) => edge_label(edge.get()).to_owned(),
        SegmentSource::Corner(corner) => corner_segment_label(panel, corner),
    }
}

fn corner_segment_label(panel: &Panel, corner: VertexId) -> String {
    let name = RectCorner::ALL
        .iter()
        .find(|candidate| candidate.id() == corner)
        .map_or("Corner", |candidate| candidate.label());
    let shape = panel.corner_feature(corner).map_or("", |feature| match feature.feature() {
        CornerFeatureKind::Radius { .. } => " (radius)",
        CornerFeatureKind::Chamfer { .. } => " (chamfer)",
    });
    format!("Corner {name}{shape}")
}

fn band_key(source: SegmentSource) -> u64 {
    match source {
        SegmentSource::Edge(edge) => u64::from(edge.get()),
        SegmentSource::Corner(corner) => 1_000 + u64::from(corner.get()),
    }
}

fn material_name(session: &Session, material: MaterialId) -> String {
    session
        .catalog()
        .edge_material(material)
        .map_or_else(|| format!("Missing material {material}"), |material| material.name().to_owned())
}

fn show_edge_band_row(
    ui: &mut Ui,
    session: &Session,
    panel_id: Id,
    edge: EdgeId,
    band: &EdgeBanding,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(edge_label(edge.get()));
        ComboBox::from_id_salt(("band_material", panel_id.get(), band_key(SegmentSource::Edge(edge))))
            .selected_text(material_name(session, band.material()))
            .show_ui(ui, |ui| {
                for material in session.catalog().edge_materials() {
                    let chosen = band.material() == material.id();
                    if ui.selectable_label(chosen, material.name()).clicked() && !chosen {
                        actions.push(Action::SetBand {
                            panel: panel_id,
                            edge,
                            material: material.id(),
                            included_in_size: band.included_in_size(),
                        });
                    }
                }
            });
        let mut included = band.included_in_size();
        let material_known = session.catalog().edge_material(band.material()).is_some();
        if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, "Included")).changed() {
            actions.push(Action::SetBand {
                panel: panel_id,
                edge,
                material: band.material(),
                included_in_size: included,
            });
        }
        if ui.small_button("−").on_hover_text("Remove banding").clicked() {
            actions.push(Action::ClearBand { panel: panel_id, edge });
        }
    });
}

fn show_corner_band_row(
    ui: &mut Ui,
    session: &Session,
    panel: &Panel,
    panel_id: Id,
    corner: VertexId,
    band: &CornerBanding,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(corner_segment_label(panel, corner));
        ComboBox::from_id_salt(("band_material", panel_id.get(), band_key(SegmentSource::Corner(corner))))
            .selected_text(material_name(session, band.material()))
            .show_ui(ui, |ui| {
                for material in session.catalog().edge_materials() {
                    let chosen = band.material() == material.id();
                    if ui.selectable_label(chosen, material.name()).clicked() && !chosen {
                        actions.push(Action::SetCornerBand {
                            panel: panel_id,
                            corner,
                            material: material.id(),
                            included_in_size: band.included_in_size(),
                        });
                    }
                }
            });
        let mut included = band.included_in_size();
        let material_known = session.catalog().edge_material(band.material()).is_some();
        if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, "Included")).changed() {
            actions.push(Action::SetCornerBand {
                panel: panel_id,
                corner,
                material: band.material(),
                included_in_size: included,
            });
        }
        if ui.small_button("−").on_hover_text("Remove banding").clicked() {
            actions.push(Action::ClearCornerBand { panel: panel_id, corner });
        }
    });
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
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::MissingCornerBandingMaterial {
                corner: RectCorner::BottomRight.id(),
                material: material(101),
            }),
            "Corner 2 uses missing edge material 101."
        );
        assert_eq!(diagnostic_message(&PanelDiagnostic::NonPositiveCore), "The panel core is not positive.");
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::MachiningOutsideCore { cutout: Id::new(9).expect("id") }),
            "Cutout 9 lies outside the core."
        );
        assert_eq!(diagnostic_message(&PanelDiagnostic::CoreRemoved), "The cutouts remove the whole panel core.");
        assert_eq!(
            diagnostic_message(&PanelDiagnostic::CoreDisconnected),
            "The cutouts split the panel core into disconnected pieces."
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
    fn band_picker_lists_existing_unbanded_segments() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(
            available_band_segments(panel),
            vec![
                SegmentSource::Edge(RectEdge::Bottom.id()),
                SegmentSource::Edge(RectEdge::Right.id()),
                SegmentSource::Edge(RectEdge::Top.id()),
                SegmentSource::Edge(RectEdge::Left.id()),
            ]
        );
        assert!(listed_band_sources(panel).is_empty());

        run_action(
            &mut session,
            &Action::SetBand {
                panel: panel_id,
                edge: RectEdge::Bottom.id(),
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: false,
            },
        )
        .expect("band bottom");
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(listed_band_sources(panel), vec![SegmentSource::Edge(RectEdge::Bottom.id())]);
        assert_eq!(available_band_segments(panel).len(), 3);

        let corner = RectCorner::BottomRight.id();
        run_action(
            &mut session,
            &Action::SetCorner {
                panel: panel_id,
                corner,
                feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(20).expect("radius") },
            },
        )
        .expect("shape corner");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(available_band_segments(panel).contains(&SegmentSource::Corner(corner)));
        assert!(!available_band_segments(panel).contains(&SegmentSource::Corner(RectCorner::BottomLeft.id())));

        run_action(
            &mut session,
            &Action::SetCornerBand {
                panel: panel_id,
                corner,
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: false,
            },
        )
        .expect("band corner");
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(listed_band_sources(panel).len(), 2);
        assert!(!available_band_segments(panel).contains(&SegmentSource::Corner(corner)));

        run_action(&mut session, &Action::ClearCornerBand { panel: panel_id, corner }).expect("clear corner band");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(available_band_segments(panel).contains(&SegmentSource::Corner(corner)));
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
    fn corner_band_actions_dispatch_through_the_engine() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        let corner = RectCorner::BottomRight.id();

        run_action(
            &mut session,
            &Action::SetCorner {
                panel: panel_id,
                corner,
                feature: CornerFeatureKind::Chamfer {
                    incoming: Length::from_millimetres(12).expect("incoming"),
                    outgoing: Length::from_millimetres(8).expect("outgoing"),
                },
            },
        )
        .expect("set chamfer");
        run_action(
            &mut session,
            &Action::SetCornerBand {
                panel: panel_id,
                corner,
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: true,
            },
        )
        .expect("set corner band");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        run_action(&mut session, &Action::ClearCornerBand { panel: panel_id, corner }).expect("clear corner band");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_none());
    }

    #[test]
    fn band_actions_accept_a_shaped_panel() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        let corner = RectCorner::BottomLeft.id();
        run_action(
            &mut session,
            &Action::SetCorner {
                panel: panel_id,
                corner,
                feature: CornerFeatureKind::Chamfer {
                    incoming: Length::from_millimetres(10).expect("incoming"),
                    outgoing: Length::from_millimetres(15).expect("outgoing"),
                },
            },
        )
        .expect("set chamfer");
        run_action(
            &mut session,
            &Action::SetBand {
                panel: panel_id,
                edge: RectEdge::Bottom.id(),
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: true,
            },
        )
        .expect("band shaped panel");
        assert!(
            session
                .project()
                .panel(panel_id)
                .expect("panel")
                .edge_banding()
                .iter()
                .any(|band| band.edge() == RectEdge::Bottom.id())
        );
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

    fn shape_frame(
        context: &mut egui::Context,
        session: &Session,
        panel_id: Id,
        events: Vec<egui::Event>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0))),
                events,
                ..Default::default()
            },
            |ui| {
                if let Some(panel) = session.project().panel(panel_id) {
                    PanelCard::default().show_shape(ui, session, panel_id, panel, &mut actions);
                }
            },
        );
        output.textures_delta.clear();
        actions
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn mm(millimetres: i64) -> Length {
        Length::from_millimetres(millimetres).expect("millimetres")
    }

    fn first_corner() -> VertexId {
        RectCorner::ALL.first().copied().expect("first corner").id()
    }

    /// Shapes every corner the same way, types `text` into one of the first corner's number
    /// fields and ends the edit with `ending`.
    ///
    /// Focus moves with Tab: the first press takes the corner mode combo, the following presses
    /// walk the number fields to its right. The typed text must survive until the edit ends, so
    /// typing is asserted to dispatch nothing on its own.
    fn typed_corner_field(
        feature: CornerFeatureKind,
        tabs: usize,
        text: &str,
        ending: Vec<egui::Event>,
    ) -> Vec<Action> {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        for corner in RectCorner::ALL {
            run_action(&mut session, &Action::SetCorner { panel: panel_id, corner: corner.id(), feature })
                .expect("shape corner");
        }

        let mut context = egui::Context::default();
        shape_frame(&mut context, &session, panel_id, Vec::new());
        for _ in 0..tabs {
            shape_frame(&mut context, &session, panel_id, vec![key_event(egui::Key::Tab)]);
        }
        shape_frame(&mut context, &session, panel_id, Vec::new());
        let typed = shape_frame(&mut context, &session, panel_id, vec![egui::Event::Text(text.to_owned())]);
        assert!(typed.is_empty(), "typing must not dispatch before the edit ends");
        let mut actions = shape_frame(&mut context, &session, panel_id, ending);
        // A focus change is processed at the end of the frame, so the commit can land in the next.
        actions.extend(shape_frame(&mut context, &session, panel_id, Vec::new()));
        actions
    }

    fn assert_corner_feature(actions: &[Action], expected: CornerFeatureKind) {
        let corner = first_corner();
        assert!(
            actions.iter().any(|action| matches!(
                action,
                Action::SetCorner { corner: edited, feature, .. } if *edited == corner && *feature == expected
            )),
            "expected {expected:?} on corner {corner:?}, got {actions:?}"
        );
    }

    #[test]
    fn a_typed_radius_commits_on_enter() {
        let actions = typed_corner_field(
            CornerFeatureKind::Radius { radius: mm(20) },
            2,
            "12",
            vec![key_event(egui::Key::Enter)],
        );
        assert_corner_feature(&actions, CornerFeatureKind::Radius { radius: mm(12) });
    }

    #[test]
    fn a_typed_radius_commits_when_the_field_is_left() {
        let actions =
            typed_corner_field(CornerFeatureKind::Radius { radius: mm(20) }, 2, "12", vec![key_event(egui::Key::Tab)]);
        assert_corner_feature(&actions, CornerFeatureKind::Radius { radius: mm(12) });
    }

    #[test]
    fn a_typed_bowed_edge_commits_on_enter() {
        let session = Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");
        let mut context = egui::Context::default();
        shape_frame(&mut context, &session, panel_id, Vec::new());
        // The sample panel has no corner features, so four Tabs walk the corner menus and the
        // fifth reaches the first bowed edge.
        for _ in 0..5 {
            shape_frame(&mut context, &session, panel_id, vec![key_event(egui::Key::Tab)]);
        }
        shape_frame(&mut context, &session, panel_id, Vec::new());
        let typed = shape_frame(&mut context, &session, panel_id, vec![egui::Event::Text("7".to_owned())]);
        assert!(typed.is_empty(), "typing must not dispatch before the edit ends");
        let mut actions = shape_frame(&mut context, &session, panel_id, vec![key_event(egui::Key::Enter)]);
        actions.extend(shape_frame(&mut context, &session, panel_id, Vec::new()));

        let edge = RectEdge::Bottom.id();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::SetBow { edge: edited, sagitta, .. } if *edited == edge && *sagitta == mm(7))),
            "typing 7 must commit a 7 mm bow on the bottom edge, got {actions:?}"
        );
    }

    #[test]
    fn a_typed_chamfer_side_commits_and_keeps_the_other_side() {
        let actions = typed_corner_field(
            CornerFeatureKind::Chamfer { incoming: mm(10), outgoing: mm(12) },
            3,
            "14",
            vec![key_event(egui::Key::Enter)],
        );
        assert_corner_feature(&actions, CornerFeatureKind::Chamfer { incoming: mm(10), outgoing: mm(14) });
    }
}
