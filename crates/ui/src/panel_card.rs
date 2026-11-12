//! The selected-panel card.
//!
//! The card renders session state and turns edits into engine commands. Text fields commit on Enter,
//! on losing focus or through an explicit button; combo boxes and checkboxes dispatch immediately.

use egui::{ComboBox, TextEdit, Ui};
use furnicraft_engine::panel_cmds::{
    BOW_CLEAR_ID, BOW_SET_ID, CORNER_BANDING_CLEAR_ID, CORNER_BANDING_SET_ID, CORNER_CLEAR_ID, CORNER_SET_ID,
    EDGE_BANDING_CLEAR_ID, EDGE_BANDING_SET_ID, MATERIAL_SET_ID, OUTLINE_CONTOUR_ID, RENAME_ID, RESIZE_ID,
    RING_BANDING_CLEAR_ID, RING_BANDING_SET_ID,
};
use furnicraft_engine::{
    EditScope, PanelBowClear, PanelBowSet, PanelCornerBandingClear, PanelCornerBandingSet, PanelCornerClear,
    PanelCornerSet, PanelDiagnostic, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMaterialSet,
    PanelOutlineToContour, PanelRename, PanelResize, PanelRingBandingClear, PanelRingBandingSet, Session,
};
use furnicraft_model::{
    ContourSegmentId, CornerBanding, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeId, Id, MachiningId,
    MaterialId, Panel, PanelNominalSize, RectCorner, RectEdge, RingBanding, SegmentSource, VertexId,
};
use furnicraft_text::Message;
use furnicraft_units::Length;
use serde_json::Value;

use crate::command::{dispatch as dispatch_command, json};
use crate::contour_edit::{ContourEntry, ContourTarget};
use crate::machining::{MachiningAction, MachiningSection, diagnostic_machining};
use crate::scrub::{self, CommitPolicy, Scrub};
use crate::section::section;
use crate::theme;

/// Persistent state of the panel card.
#[derive(Debug, Default)]
pub(crate) struct PanelCard {
    draft: Option<Draft>,
    error: Option<String>,
    machining: MachiningSection,
}

/// What the card edits, kept between frames until the user commits it.
///
/// The nominal size is in millimetres because that is what a field scrubs; the document stores a
/// [`Length`], and the card converts at the command boundary.
#[derive(Clone, Debug)]
struct Draft {
    panel: Id,
    name: String,
    length: f64,
    width: f64,
}

impl Draft {
    fn from_panel(panel: &Panel) -> Self {
        let nominal = panel.nominal_size();
        Self {
            panel: panel.id(),
            name: panel.name().to_owned(),
            length: nominal.length().millimetres_f64(),
            width: nominal.width().millimetres_f64(),
        }
    }
}

/// One command the card wants to dispatch after the immutable UI borrow ends.
#[derive(Clone, Debug)]
enum Action {
    Rename {
        panel: Id,
        name: String,
    },
    Resize {
        panel: Id,
        nominal_size: PanelNominalSize,
        scope: EditScope,
    },
    SetMaterial {
        panel: Id,
        material: MaterialId,
    },
    SetBand {
        panel: Id,
        edge: EdgeId,
        material: MaterialId,
        included_in_size: bool,
    },
    ClearBand {
        panel: Id,
        edge: EdgeId,
    },
    SetCornerBand {
        panel: Id,
        corner: VertexId,
        material: MaterialId,
        included_in_size: bool,
    },
    ClearCornerBand {
        panel: Id,
        corner: VertexId,
    },
    SetRingBand {
        panel: Id,
        segment: ContourSegmentId,
        material: MaterialId,
        included_in_size: bool,
    },
    ClearRingBand {
        panel: Id,
        segment: ContourSegmentId,
    },
    SetCorner {
        panel: Id,
        corner: VertexId,
        feature: CornerFeatureKind,
    },
    ClearCorner {
        panel: Id,
        corner: VertexId,
    },
    SetBow {
        panel: Id,
        edge: EdgeId,
        sagitta: Length,
    },
    ClearBow {
        panel: Id,
        edge: EdgeId,
    },
    ToContour {
        panel: Id,
    },
    /// Escape during a drag: put the document back and leave no undo entry.
    CancelEdit(EditScope),
}

/// What the panel card asks the editor to do with the contour edit mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelAction {
    /// Nothing was pressed.
    #[default]
    None,
    /// Enter contour editing on this panel's outline.
    StartContour,
    /// Enter contour editing on one cutout ring of this panel.
    StartCutoutContour {
        /// The panel that owns the cutout.
        panel: Id,
        /// The machining record whose ring is edited.
        machining: MachiningId,
    },
}

/// Draws the shared *Edit contour* entry state and returns whether the button was pressed.
fn contour_entry(ui: &mut Ui, entry: ContourEntry, hover: Message) -> bool {
    let lang = crate::language(ui.ctx());
    match entry {
        ContourEntry::Editing => {
            ui.label(Message::PanelEditingInViewport.text(lang));
            false
        }
        ContourEntry::Busy => {
            ui.label(Message::PanelFinishContour.text(lang));
            false
        }
        ContourEntry::SelectSingle => {
            ui.label(Message::PanelSelectSingle.text(lang));
            false
        }
        ContourEntry::Ready => {
            ui.button(Message::PanelEditContour.text(lang)).on_hover_text(hover.text(lang)).clicked()
        }
    }
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

    const fn label(self) -> Message {
        match self {
            Self::None => Message::CornerNone,
            Self::Radius => Message::CornerRadius,
            Self::Chamfer => Message::CornerChamfer,
        }
    }
}

impl PanelCard {
    /// Draws the card for the current panel and dispatches any accepted edits.
    ///
    /// Returns what the user asked to do with contour editing; the editor owns the mode and the
    /// working copy. `contour_target` names the live edit, so the section it belongs to shows the
    /// state instead of the entry button and the outline does not claim a cutout edit.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        selected: Option<Id>,
        contour_target: Option<ContourTarget>,
        single_selection: bool,
    ) -> PanelAction {
        let lang = crate::language(ui.ctx());
        // One token per frame for the numeric fields: a field that begins an edit takes it, and the
        // whole drag or typing run lands in one undo entry (ADR 0092).
        let fresh = session.edit_scope();
        let Some(panel_id) = selected else {
            self.draft = None;
            self.error = None;
            ui.label(Message::SelectNodeInTree.text(lang));
            return PanelAction::None;
        };

        if self.draft.as_ref().is_none_or(|draft| draft.panel != panel_id) {
            self.error = None;
            self.draft = session.project().panel(panel_id).map(Draft::from_panel);
        }
        let Some(panel) = session.project().panel(panel_id) else {
            self.draft = None;
            self.error = None;
            ui.label(Message::SelectNodeInTree.text(lang));
            return PanelAction::None;
        };
        let Some(draft) = self.draft.as_mut() else {
            ui.label(Message::SelectNodeInTree.text(lang));
            return PanelAction::None;
        };

        let mut actions = Vec::new();
        let mut machining_actions: Vec<MachiningAction> = Vec::new();
        let mut errors = Vec::new();
        let mut contour_request = PanelAction::None;
        let panel_entry = ContourEntry::of(contour_target, ContourTarget::Panel(panel_id), single_selection);
        let diagnostics = session.panel_diagnostics(panel_id);
        let thickness = session
            .panel_geometry(panel_id)
            .map_or_else(|_| panel.last_known_thickness(), |geometry| geometry.thickness());
        ui.separator();
        section(ui, "panel.name", Message::SectionName.text(lang), true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button(Message::CmdRename.text(lang)).clicked();
                if response.lost_focus() || clicked {
                    let name = draft.name.trim();
                    if name.is_empty() {
                        errors.push(Message::PanelNameEmpty.text(lang).to_owned());
                    } else if name != panel.name() {
                        actions.push(Action::Rename { panel: panel_id, name: name.to_owned() });
                    }
                }
            });
        });

        section(ui, "panel.dimensions", Message::SectionDimensions.text(lang), true, |ui| {
            section(ui, "panel.dimensions.nominal_size", Message::SectionNominalSize.text(lang), true, |ui| {
                if panel.is_contour() {
                    ui.label(Message::PanelContourSetsSize.text(lang));
                } else {
                    let mut commit = None;
                    for (label, value) in
                        [(Message::Length.text(lang), &mut draft.length), (Message::Width.text(lang), &mut draft.width)]
                    {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let outcome = Scrub::new(value)
                                .speed(0.5)
                                .range(1.0..=10_000.0)
                                .decimals(3)
                                .unit("mm")
                                .parse(scrub::parse_millimetres)
                                .policy(CommitPolicy::Live)
                                .edit(fresh)
                                .show(ui);
                            if let Some(message) = outcome.error {
                                errors.push(message.text(lang).to_owned());
                            }
                            if let Some(scope) = outcome.cancelled {
                                actions.push(Action::CancelEdit(scope));
                            }
                            if outcome.commit {
                                commit = outcome.scope;
                            }
                        });
                    }
                    if let Some(scope) = commit {
                        match (scrub::millimetres(draft.length), scrub::millimetres(draft.width)) {
                            (Some(length), Some(width)) => match PanelNominalSize::new(length, width) {
                                Ok(nominal_size) if nominal_size != panel.nominal_size() => {
                                    actions.push(Action::Resize { panel: panel_id, nominal_size, scope });
                                }
                                Ok(_) => {}
                                Err(error) => errors.push(error.to_string()),
                            },
                            _ => errors.push(Message::LengthErrorOutOfRange.text(lang).to_owned()),
                        }
                    }
                }
            });

            section(ui, "panel.dimensions.resolved", Message::SectionResolved.text(lang), true, |ui| {
                show_resolved(ui, session, panel_id);
            });
        });

        ui.separator();
        section(ui, "panel.material", Message::SectionMaterial.text(lang), true, |ui| {
            self.show_material(ui, session, panel_id, panel.material(), &mut actions);
        });
        ui.separator();
        section(ui, "panel.shape", Message::SectionShape.text(lang), true, |ui| {
            contour_request = self.show_shape(ui, panel_id, panel, panel_entry, &mut actions);
        });
        ui.separator();
        section(ui, "panel.banding", Message::SectionBanding.text(lang), true, |ui| {
            self.show_bands(ui, session, panel_id, panel, &mut actions);
        });
        ui.separator();
        section(ui, "panel.machining", Message::SectionMachining.text(lang), true, |ui| {
            if let Some(machining) = self.machining.show(
                ui,
                panel_id,
                panel,
                thickness,
                &diagnostics,
                contour_target,
                single_selection,
                &mut machining_actions,
            ) {
                contour_request = PanelAction::StartCutoutContour { panel: panel_id, machining };
            }
        });
        ui.separator();
        section(ui, "panel.diagnostics", Message::SectionDiagnostics.text(lang), true, |ui| {
            self.show_diagnostics(ui, &diagnostics);
        });

        if let Some(error) = &self.error {
            ui.separator();
            ui.colored_label(theme::danger(ui), error);
        }
        if !errors.is_empty() {
            self.error = Some(errors.join("\n"));
            ui.ctx().request_repaint();
            return contour_request;
        }
        if !actions.is_empty() {
            self.error = None;
            for action in actions {
                if let Err(error) = run_action(session, &action) {
                    self.error = Some(error);
                    break;
                }
                if matches!(action, Action::ToContour { .. }) {
                    contour_request = PanelAction::StartContour;
                }
                if matches!(action, Action::Rename { .. } | Action::Resize { .. }) {
                    self.draft = None;
                }
            }
            ui.ctx().request_repaint();
        }
        if !machining_actions.is_empty() {
            if self.error.is_none() {
                for action in &machining_actions {
                    if let Err(error) = self.machining.run_action(session, action) {
                        self.error = Some(error);
                        break;
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        contour_request
    }

    fn show_material(
        &self,
        ui: &mut Ui,
        session: &Session,
        panel_id: Id,
        current: Option<MaterialId>,
        actions: &mut Vec<Action>,
    ) {
        let lang = crate::language(ui.ctx());
        if let Some(material) = current
            && session.catalog().panel_material(material).is_none()
        {
            ui.colored_label(theme::danger(ui), Message::PanelMaterialMissingCatalogue.with(lang, [material]));
        }
        let selected_text = match current {
            Some(material) => session
                .catalog()
                .panel_material(material)
                .map_or_else(|| Message::MissingMaterial.with(lang, [material]), |material| material.name().to_owned()),
            None => Message::NoMaterial.text(lang).to_owned(),
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

    /// Draws the outline section and reports a contour-edit request.
    ///
    /// A contour panel has no rectangle corners or bows: its ring is the outline, so the card shows
    /// the ring summary and the entry point into the viewport edit mode instead.
    fn show_shape(
        &self,
        ui: &mut Ui,
        panel_id: Id,
        panel: &Panel,
        entry: ContourEntry,
        actions: &mut Vec<Action>,
    ) -> PanelAction {
        let lang = crate::language(ui.ctx());
        let mut request = PanelAction::None;
        section(ui, "panel.shape.outline", Message::SectionOutline.text(lang), true, |ui| {
            if panel.is_contour() {
                if let Some(contour) = panel.outline_contour() {
                    let segments = contour.segments().len();
                    let arcs = contour.segments().filter(|segment| segment.is_arc()).count();
                    let counted = Message::counted(
                        segments,
                        Message::PanelContourRingOne,
                        Message::PanelContourRingFew,
                        Message::PanelContourRingMany,
                    );
                    ui.label(counted.text(lang));
                    if arcs > 0 {
                        ui.label(Message::ArcsCount.with(lang, [arcs]));
                    }
                }
                if contour_entry(ui, entry, Message::ContourReshapeHint) {
                    request = PanelAction::StartContour;
                }
            } else {
                self.show_outline(ui, panel_id, panel, actions);
                ui.add_space(4.0);
                if contour_entry(ui, entry, Message::BakeOutlineHint) {
                    actions.push(Action::ToContour { panel: panel_id });
                }
            }
        });
        if !panel.is_contour() {
            section(ui, "panel.shape.bowed_edges", Message::SectionBowedEdges.text(lang), true, |ui| {
                self.show_bowed_edges(ui, panel_id, panel, actions);
            });
        }
        request
    }

    fn show_outline(&self, ui: &mut Ui, panel_id: Id, panel: &Panel, actions: &mut Vec<Action>) {
        let lang = crate::language(ui.ctx());
        for corner in RectCorner::ALL {
            let corner_id = corner.id();
            let current = panel.corner_feature(corner_id);
            let mode = current.map_or(CornerMode::None, |feature| match feature.feature() {
                CornerFeatureKind::Radius { .. } => CornerMode::Radius,
                CornerFeatureKind::Chamfer { .. } => CornerMode::Chamfer,
            });
            ui.horizontal(|ui| {
                ui.label(Message::CornerWith.with(lang, [corner.label()]));
                let mut selected = mode;
                ComboBox::from_id_salt(("corner_mode", panel_id.get(), corner_id.get()))
                    .selected_text(mode.label().text(lang))
                    .show_ui(ui, |ui| {
                        for option in CornerMode::ALL {
                            if ui.selectable_label(selected == option, option.label().text(lang)).clicked()
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
                        ui.label(Message::Inward.text(lang));
                        let (incoming_committed, _) = millimetre_field(ui, incoming, 0.1..=1_000.0);
                        ui.label(Message::Outward.text(lang));
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
    }

    fn show_bowed_edges(&self, ui: &mut Ui, panel_id: Id, panel: &Panel, actions: &mut Vec<Action>) {
        let lang = crate::language(ui.ctx());
        for edge in RectEdge::ALL {
            let edge_id = edge.id();
            let current = panel.edge_bow(edge_id);
            ui.horizontal(|ui| {
                ui.label(edge_label(edge_id.get()).text(lang));
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
                if current.is_some() && ui.button(Message::Clear.text(lang)).clicked() {
                    actions.push(Action::ClearBow { panel: panel_id, edge: edge_id });
                }
            });
        }
    }

    fn show_bands(&self, ui: &mut Ui, session: &Session, panel_id: Id, panel: &Panel, actions: &mut Vec<Action>) {
        let lang = crate::language(ui.ctx());
        let listed = listed_band_sources(panel);
        if listed.is_empty() {
            ui.label(Message::NoBanding.text(lang));
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
                SegmentSource::Ring(segment) => {
                    if let Some(band) = panel.ring_banding_at(segment) {
                        show_ring_band_row(ui, session, panel, panel_id, segment, &band, actions);
                    }
                }
            }
        }

        ui.add_space(4.0);
        let available = available_band_segments(panel);
        if available.is_empty() {
            if listed.is_empty() {
                ui.label(Message::AddChamferOrRadius.text(lang));
            } else {
                ui.label(Message::AllSegmentsBanded.text(lang));
            }
            return;
        }
        let Some(material) = session.catalog().edge_materials().first() else {
            ui.label(Message::AddEdgeMaterial.text(lang));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(Message::AddBanding.text(lang));
            ComboBox::from_id_salt(("add_banding", panel_id.get()))
                .selected_text(Message::ChooseSegment.text(lang))
                .show_ui(ui, |ui| {
                    for source in &available {
                        if ui.selectable_label(false, segment_label(panel, *source, lang)).clicked() {
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
                                SegmentSource::Ring(segment) => actions.push(Action::SetRingBand {
                                    panel: panel_id,
                                    segment: *segment,
                                    material: material.id(),
                                    included_in_size: false,
                                }),
                            }
                        }
                    }
                });
        });
    }

    fn show_diagnostics(&self, ui: &mut Ui, diagnostics: &[PanelDiagnostic]) {
        let lang = crate::language(ui.ctx());
        let mut listed = 0;
        for diagnostic in diagnostics {
            if diagnostic_machining(diagnostic).is_some() {
                continue;
            }
            ui.colored_label(theme::danger(ui), diagnostic_message(diagnostic, lang));
            listed += 1;
        }
        let bound = diagnostics.len().saturating_sub(listed);
        if bound > 0 {
            let counted = Message::counted(
                bound,
                Message::MachiningIssueOne,
                Message::MachiningIssueFew,
                Message::MachiningIssueMany,
            );
            ui.label(counted.text(lang));
        }
        if listed == 0 && bound == 0 {
            ui.label(Message::NoIssues.text(lang));
        }
    }
}

fn show_resolved(ui: &mut Ui, session: &Session, panel_id: Id) {
    let lang = crate::language(ui.ctx());
    match session.panel_geometry(panel_id) {
        Ok(geometry) => {
            ui.label(Message::ThicknessRow.with(lang, [geometry.thickness()]));
            ui.label(Message::CoreRow.with(lang, [geometry.core_length(), geometry.core_width()]));
            ui.label(Message::OverallRow.with(lang, [geometry.overall_length(), geometry.overall_width()]));
        }
        Err(error) => {
            ui.colored_label(theme::danger(ui), Message::GeometryError.with(lang, [error]));
        }
    }
}

fn run_action(session: &mut Session, action: &Action) -> Result<(), String> {
    match action {
        Action::Rename { panel, name } => {
            let params = json(&PanelRename { panel: *panel, name: name.clone() })?;
            dispatch(session, RENAME_ID, &params)
        }
        Action::Resize { panel, nominal_size, scope } => {
            let params = json(&PanelResize { panel: *panel, nominal_size: *nominal_size })?;
            dispatch_in(session, *scope, RESIZE_ID, &params)
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
        Action::SetRingBand { panel, segment, material, included_in_size } => {
            let params = json(&PanelRingBandingSet {
                panel: *panel,
                segment: *segment,
                material: *material,
                included_in_size: *included_in_size,
            })?;
            dispatch(session, RING_BANDING_SET_ID, &params)
        }
        Action::ClearRingBand { panel, segment } => {
            let params = json(&PanelRingBandingClear { panel: *panel, segment: *segment })?;
            dispatch(session, RING_BANDING_CLEAR_ID, &params)
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
        Action::ToContour { panel } => {
            let params = json(&PanelOutlineToContour { panel: *panel })?;
            dispatch(session, OUTLINE_CONTOUR_ID, &params)
        }
        Action::CancelEdit(scope) => {
            session.cancel_edit(*scope);
            Ok(())
        }
    }
}

fn dispatch(session: &mut Session, id: &str, params: &Value) -> Result<(), String> {
    dispatch_command(session, id, params).map(|_| ())
}

/// Dispatches one step of an edit, so a field that commits every frame of a drag stays one undo
/// entry.
fn dispatch_in(session: &mut Session, scope: EditScope, id: &str, params: &Value) -> Result<(), String> {
    session.dispatch_in(scope, id, params).map(|_| ()).map_err(|error| error.to_string())
}

pub(crate) fn edge_label(edge: u32) -> Message {
    match edge {
        1 => Message::EdgeBottom,
        2 => Message::EdgeRight,
        3 => Message::EdgeTop,
        4 => Message::EdgeLeft,
        _ => Message::EdgeGeneric,
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
/// The field shows the number alone and keeps the text the user types until the edit ends: Enter,
/// losing focus or the end of a drag. The value is read back from the document every frame, so a
/// cutout or a chamfer does not rebuild while the user is still typing; a drag shows its running
/// number and hands it over once, at the release.
pub(crate) fn millimetre_field(
    ui: &mut Ui,
    value: Length,
    range: std::ops::RangeInclusive<f64>,
) -> (Option<f64>, egui::Response) {
    let mut millimetres = value.millimetres_f64();
    let outcome = Scrub::new(&mut millimetres)
        .speed(0.5)
        .range(range)
        .decimals(3)
        .unit("mm")
        .parse(scrub::parse_millimetres)
        .policy(CommitPolicy::OnRelease)
        .show(ui);
    (outcome.commit.then_some(millimetres), outcome.response)
}

pub(crate) fn positive_length(millimetres: f64) -> Option<Length> {
    let value = scrub::millimetres(millimetres)?;
    value.is_positive().then_some(value)
}

pub(crate) fn signed_length(millimetres: f64) -> Option<Length> {
    scrub::millimetres(millimetres)
}

/// Formats one derived panel issue for the card.
pub(crate) fn diagnostic_message(diagnostic: &PanelDiagnostic, lang: crate::Lang) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    match diagnostic {
        PanelDiagnostic::NoMaterial => Cow::Borrowed(Message::DiagNoMaterial.text(lang)),
        PanelDiagnostic::MissingMaterial { material } => {
            Cow::Owned(Message::DiagPanelMaterialMissing.with(lang, [material]))
        }
        PanelDiagnostic::MissingBandingMaterial { edge, material } => {
            Cow::Owned(Message::DiagEdgeMaterialMissing.with(lang, [edge.to_string(), material.to_string()]))
        }
        PanelDiagnostic::MissingCornerBandingMaterial { corner, material } => {
            Cow::Owned(Message::DiagCornerMaterialMissing.with(lang, [corner.to_string(), material.to_string()]))
        }
        PanelDiagnostic::MissingRingBandingMaterial { segment, material } => {
            Cow::Owned(Message::DiagSegmentMaterialMissing.with(lang, [segment.to_string(), material.to_string()]))
        }
        PanelDiagnostic::NonPositiveCore => Cow::Borrowed(Message::DiagNonPositiveCore.text(lang)),
        PanelDiagnostic::InvalidOutline => Cow::Borrowed(Message::DiagInvalidOutline.text(lang)),
        PanelDiagnostic::MachiningOutsideCore { machining } => {
            Cow::Owned(Message::DiagMachiningOutsideCore.with(lang, [machining]))
        }
        PanelDiagnostic::CoreRemoved => Cow::Borrowed(Message::DiagCoreRemoved.text(lang)),
        PanelDiagnostic::CoreDisconnected => Cow::Borrowed(Message::DiagCoreDisconnected.text(lang)),
        PanelDiagnostic::BlindDepthNotLessThanThickness { machining, depth, thickness } => Cow::Owned(
            Message::DiagBlindDepth.with(lang, [machining.to_string(), depth.to_string(), thickness.to_string()]),
        ),
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
    for band in panel.ring_banding() {
        let source = SegmentSource::Ring(band.segment());
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
        SegmentSource::Ring(segment) => {
            panel.outline_contour().is_some_and(|contour| contour.index_of(segment).is_some())
        }
    }
}

fn available_band_segments(panel: &Panel) -> Vec<SegmentSource> {
    if panel.is_contour() {
        let Some(contour) = panel.outline_contour() else {
            return Vec::new();
        };
        return contour
            .entries()
            .iter()
            .filter(|entry| panel.ring_banding_at(entry.id()).is_none())
            .map(|entry| SegmentSource::Ring(entry.id()))
            .collect();
    }
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

fn segment_label(panel: &Panel, source: SegmentSource, lang: crate::Lang) -> String {
    match source {
        SegmentSource::Edge(edge) => edge_label(edge.get()).text(lang).to_owned(),
        SegmentSource::Corner(corner) => corner_segment_label(panel, corner, lang),
        SegmentSource::Ring(segment) => {
            let arc = panel
                .outline_contour()
                .and_then(|contour| contour.segment(segment))
                .is_some_and(|segment| segment.is_arc());
            if arc {
                Message::SegmentWithArc.with(lang, [segment.get()])
            } else {
                Message::SegmentWith.with(lang, [segment.get()])
            }
        }
    }
}

fn corner_segment_label(panel: &Panel, corner: VertexId, lang: crate::Lang) -> String {
    let name = RectCorner::ALL
        .iter()
        .find(|candidate| candidate.id() == corner)
        .map_or(Message::CornerGeneric.text(lang), |candidate| candidate.label());
    let shape = panel.corner_feature(corner).map_or("", |feature| match feature.feature() {
        CornerFeatureKind::Radius { .. } => Message::CornerRadiusSuffix.text(lang),
        CornerFeatureKind::Chamfer { .. } => Message::CornerChamferSuffix.text(lang),
    });
    Message::CornerWithShape.with(lang, [name, shape])
}

fn band_key(source: SegmentSource) -> u64 {
    match source {
        SegmentSource::Edge(edge) => u64::from(edge.get()),
        SegmentSource::Corner(corner) => 1_000 + u64::from(corner.get()),
        SegmentSource::Ring(segment) => 2_000 + u64::from(segment.get()),
    }
}

fn material_name(session: &Session, material: MaterialId, lang: crate::Lang) -> String {
    session
        .catalog()
        .edge_material(material)
        .map_or_else(|| Message::MissingMaterial.with(lang, [material]), |material| material.name().to_owned())
}

fn show_edge_band_row(
    ui: &mut Ui,
    session: &Session,
    panel_id: Id,
    edge: EdgeId,
    band: &EdgeBanding,
    actions: &mut Vec<Action>,
) {
    let lang = crate::language(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(edge_label(edge.get()).text(lang));
        ComboBox::from_id_salt(("band_material", panel_id.get(), band_key(SegmentSource::Edge(edge))))
            .selected_text(material_name(session, band.material(), lang))
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
        if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, Message::Included.text(lang))).changed() {
            actions.push(Action::SetBand {
                panel: panel_id,
                edge,
                material: band.material(),
                included_in_size: included,
            });
        }
        if ui.small_button("−").on_hover_text(Message::RemoveBanding.text(lang)).clicked() {
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
    let lang = crate::language(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(corner_segment_label(panel, corner, lang));
        ComboBox::from_id_salt(("band_material", panel_id.get(), band_key(SegmentSource::Corner(corner))))
            .selected_text(material_name(session, band.material(), lang))
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
        if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, Message::Included.text(lang))).changed() {
            actions.push(Action::SetCornerBand {
                panel: panel_id,
                corner,
                material: band.material(),
                included_in_size: included,
            });
        }
        if ui.small_button("−").on_hover_text(Message::RemoveBanding.text(lang)).clicked() {
            actions.push(Action::ClearCornerBand { panel: panel_id, corner });
        }
    });
}

fn show_ring_band_row(
    ui: &mut Ui,
    session: &Session,
    panel: &Panel,
    panel_id: Id,
    segment: ContourSegmentId,
    band: &RingBanding,
    actions: &mut Vec<Action>,
) {
    let lang = crate::language(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(segment_label(panel, SegmentSource::Ring(segment), lang));
        ComboBox::from_id_salt(("band_material", panel_id.get(), band_key(SegmentSource::Ring(segment))))
            .selected_text(material_name(session, band.material(), lang))
            .show_ui(ui, |ui| {
                for material in session.catalog().edge_materials() {
                    let chosen = band.material() == material.id();
                    if ui.selectable_label(chosen, material.name()).clicked() && !chosen {
                        actions.push(Action::SetRingBand {
                            panel: panel_id,
                            segment,
                            material: material.id(),
                            included_in_size: band.included_in_size(),
                        });
                    }
                }
            });
        let mut included = band.included_in_size();
        let material_known = session.catalog().edge_material(band.material()).is_some();
        if ui.add_enabled(material_known, egui::Checkbox::new(&mut included, Message::Included.text(lang))).changed() {
            actions.push(Action::SetRingBand {
                panel: panel_id,
                segment,
                material: band.material(),
                included_in_size: included,
            });
        }
        if ui.small_button("−").on_hover_text(Message::RemoveBanding.text(lang)).clicked() {
            actions.push(Action::ClearRingBand { panel: panel_id, segment });
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use furnicraft_model::{Contour, MachiningId, Placement};

    fn material(value: u64) -> MaterialId {
        MaterialId::new(value).expect("material id")
    }

    /// A session whose root holds a 700 x 500 contour panel.
    fn contour_session() -> (Session, Id) {
        let mut project = furnicraft_testkit::sample_project();
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [mm(700), Length::ZERO],
            [mm(700), mm(500)],
            [Length::ZERO, mm(500)],
        ])
        .expect("ring");
        let panel = project
            .add_panel_contour(project.root().id(), "Contour", &ring, None, mm(18), Placement::IDENTITY)
            .expect("contour panel");
        (Session::with_catalog(project, furnicraft_testkit::sample_catalog()), panel)
    }

    fn ring_segments(panel: &Panel) -> Vec<SegmentSource> {
        panel.outline_contour().expect("ring").entries().iter().map(|entry| SegmentSource::Ring(entry.id())).collect()
    }

    fn diagnostic_message_en(diagnostic: &PanelDiagnostic) -> String {
        diagnostic_message(diagnostic, crate::Lang::En).into_owned()
    }

    #[test]
    fn diagnostics_have_readable_messages() {
        assert_eq!(diagnostic_message_en(&PanelDiagnostic::NoMaterial), "No panel material is assigned.");
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::MissingMaterial { material: material(7) }),
            "Panel material 7 is missing from the catalogue."
        );
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::MissingBandingMaterial {
                edge: EdgeId::new(2).expect("edge"),
                material: material(101),
            }),
            "Edge 2 uses missing edge material 101."
        );
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::MissingCornerBandingMaterial {
                corner: RectCorner::BottomRight.id(),
                material: material(101),
            }),
            "Corner 2 uses missing edge material 101."
        );
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::MissingRingBandingMaterial {
                segment: ContourSegmentId::new(3).expect("segment"),
                material: material(101),
            }),
            "Segment 3 uses missing edge material 101."
        );
        assert_eq!(diagnostic_message_en(&PanelDiagnostic::NonPositiveCore), "The panel core is not positive.");
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::InvalidOutline),
            "The panel outline or its edge banding does not form a valid contour."
        );
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::MachiningOutsideCore {
                machining: MachiningId::new(9).expect("id")
            }),
            "Machining 9 lies outside the core."
        );
        assert_eq!(diagnostic_message_en(&PanelDiagnostic::CoreRemoved), "The cutouts remove the whole panel core.");
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::CoreDisconnected),
            "The cutouts split the panel core into disconnected pieces."
        );
        assert_eq!(
            diagnostic_message_en(&PanelDiagnostic::BlindDepthNotLessThanThickness {
                machining: MachiningId::new(9).expect("id"),
                depth: Length::from_millimetres(20).expect("depth"),
                thickness: Length::from_millimetres(18).expect("thickness"),
            }),
            "Machining 9 blind depth 20 mm is not less than thickness 18 mm."
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
    fn to_contour_action_bakes_a_rectangle_and_refuses_a_ring() {
        let mut session =
            Session::with_catalog(furnicraft_testkit::sample_project(), furnicraft_testkit::sample_catalog());
        let panel_id = Id::new(2).expect("panel id");

        run_action(&mut session, &Action::ToContour { panel: panel_id }).expect("bake the outline");
        assert!(session.project().panel(panel_id).expect("panel").is_contour());

        let stored = session.project().revision();
        let error = run_action(&mut session, &Action::ToContour { panel: panel_id }).expect_err("already a ring");
        assert!(error.contains("already a contour"), "error was {error}");
        assert_eq!(session.project().revision(), stored);

        session.undo().expect("undo");
        assert!(!session.project().panel(panel_id).expect("panel").is_contour());
    }

    #[test]
    fn edge_labels_match_the_documented_numbering() {
        assert_eq!(
            [edge_label(1), edge_label(2), edge_label(3), edge_label(4)],
            [Message::EdgeBottom, Message::EdgeRight, Message::EdgeTop, Message::EdgeLeft]
        );
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

        session.undo().expect("undo");
        assert!(session.project().panel(panel_id).expect("panel").edge_banding().is_empty());
    }

    #[test]
    fn contour_card_offers_banding_per_ring_segment() {
        let (mut session, panel_id) = contour_session();
        let panel = session.project().panel(panel_id).expect("panel");
        let segments = ring_segments(panel);
        assert_eq!(segments.len(), 4);
        assert_eq!(available_band_segments(panel), segments);
        assert!(listed_band_sources(panel).is_empty());

        let segment = panel.outline_contour().expect("ring").entries().first().expect("segment").id();
        run_action(
            &mut session,
            &Action::SetRingBand {
                panel: panel_id,
                segment,
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: true,
            },
        )
        .expect("band the ring segment");

        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(listed_band_sources(panel), vec![SegmentSource::Ring(segment)]);
        assert_eq!(available_band_segments(panel).len(), 3);
        assert!(!available_band_segments(panel).contains(&SegmentSource::Ring(segment)));
        assert_eq!(panel.ring_banding_at(segment).map(|band| band.included_in_size()), Some(true));

        session.undo().expect("undo");
        assert!(session.project().panel(panel_id).expect("panel").ring_banding_at(segment).is_none());
    }

    #[test]
    fn ring_band_clear_dispatches_through_the_engine() {
        let (mut session, panel_id) = contour_session();
        let segment = session
            .project()
            .panel(panel_id)
            .expect("panel")
            .outline_contour()
            .expect("ring")
            .entries()
            .first()
            .expect("segment")
            .id();
        run_action(
            &mut session,
            &Action::SetRingBand {
                panel: panel_id,
                segment,
                material: furnicraft_testkit::sample_edge_material_id(),
                included_in_size: false,
            },
        )
        .expect("band the ring segment");
        run_action(&mut session, &Action::ClearRingBand { panel: panel_id, segment }).expect("clear the ring band");
        assert!(session.project().panel(panel_id).expect("panel").ring_banding_at(segment).is_none());
        assert!(
            available_band_segments(session.project().panel(panel_id).expect("panel"))
                .contains(&SegmentSource::Ring(segment))
        );
    }

    #[test]
    fn contour_shape_section_offers_no_corners_or_bows() {
        let (session, panel_id) = contour_session();
        let mut actions = Vec::new();
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0))),
                ..Default::default()
            },
            |ui| {
                let panel = session.project().panel(panel_id).expect("panel");
                let card = PanelCard::default();
                let _ = card.show_shape(ui, panel_id, panel, ContourEntry::Ready, &mut actions);
            },
        );
        output.textures_delta.clear();
        assert!(actions.is_empty(), "a contour panel must not offer corner or bow edits: {actions:?}");
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
                    let card = PanelCard::default();
                    card.show_outline(ui, panel_id, panel, &mut actions);
                    card.show_bowed_edges(ui, panel_id, panel, &mut actions);
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
