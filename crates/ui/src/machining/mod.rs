//! The machining section of the panel card.
//!
//! The section lists a panel's operations in application order. Every row has the same chrome — a
//! fold toggle, the kind and its anchor, reorder and remove buttons — and a body the operation kind
//! contributes. The add menu is built from [`MACHINING_DESCRIPTORS`], so a kind added to the table
//! appears in the card without touching this module.
//!
//! Diagnostics that name a [`MachiningId`] are shown with their operation, both on the row and in
//! its body, so a folded row cannot hide the problem. Which rows are expanded is view state of the
//! card, kept in a set of open ids here; it is never written to the document.

mod cutout;

use std::collections::HashSet;

use egui::{Color32, ComboBox, RichText, Ui};
use furnicraft_engine::panel_cmds::{MACHINING_ADD_ID, MACHINING_MOVE_ID, MACHINING_REMOVE_ID, MACHINING_UPDATE_ID};
use furnicraft_engine::{
    PanelDiagnostic, PanelMachiningAdd, PanelMachiningMove, PanelMachiningRemove, PanelMachiningUpdate, Session,
};
use furnicraft_model::{
    EdgeId, EdgeRef, FaceSide, Id, MACHINING_DESCRIPTORS, Machining, MachiningAnchor, MachiningCapabilities,
    MachiningCategory, MachiningId, MachiningKind, MachiningParams, Panel,
};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::contour_edit::{ContourEntry, ContourTarget};
use crate::panel_card::{diagnostic_message, edge_label};

/// One machining edit the card wants to dispatch after the immutable UI borrow ends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MachiningAction {
    /// Append a new operation of a kind, anchored as the caller chose.
    Add { panel: Id, anchor: MachiningAnchor, params: MachiningParams },
    /// Replace one record in place, keeping its id and position.
    Update { panel: Id, machining: MachiningId, anchor: MachiningAnchor, params: MachiningParams },
    /// Remove one record.
    Remove { panel: Id, machining: MachiningId },
    /// Move one record to a final list position.
    Move { panel: Id, machining: MachiningId, index: usize },
}

/// Persistent view state of the machining section.
///
/// The set holds the operations whose body is open. It is card state: the document never stores it,
/// and it is pruned to the records the panel actually has before every draw.
#[derive(Debug, Default)]
pub(crate) struct MachiningSection {
    open: HashSet<MachiningId>,
}

impl MachiningSection {
    /// Draws the section and collects the edits the user made.
    ///
    /// `contour_target` names the live contour edit, so this section can show a cutout ring as
    /// editing, busy or ready; `single_selection` mirrors the editor rule that one selected panel
    /// owns the mode. Returns the record whose *Edit contour* was pressed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        panel_id: Id,
        panel: &Panel,
        thickness: Length,
        diagnostics: &[PanelDiagnostic],
        contour_target: Option<ContourTarget>,
        single_selection: bool,
        actions: &mut Vec<MachiningAction>,
    ) -> Option<MachiningId> {
        self.retain_existing(panel);

        if panel.machining().is_empty() {
            ui.label(Message::NoMachining.text(crate::language(ui.ctx())));
        }
        let count = panel.machining().len();
        let mut contour_request = None;
        for (index, record) in panel.machining().iter().enumerate() {
            if self.show_record(
                ui,
                panel_id,
                record,
                index,
                count,
                thickness,
                diagnostics,
                contour_target,
                single_selection,
                actions,
            ) {
                contour_request = Some(record.id());
            }
        }

        ui.add_space(4.0);
        self.show_add_menu(ui, panel_id, panel, actions);
        contour_request
    }

    /// Dispatches one edit and returns the record an add created.
    pub(crate) fn run_action(
        &mut self,
        session: &mut Session,
        action: &MachiningAction,
    ) -> Result<Option<MachiningId>, String> {
        match action {
            MachiningAction::Add { panel, anchor, params } => {
                let request = PanelMachiningAdd { panel: *panel, anchor: *anchor, params: params.clone() };
                let outcome = dispatch(session, MACHINING_ADD_ID, &json(&request)?)?;
                let created = outcome.created_machining_id();
                if let Some(id) = created {
                    self.open.insert(id);
                }
                Ok(created)
            }
            MachiningAction::Update { panel, machining, anchor, params } => {
                let request = PanelMachiningUpdate {
                    panel: *panel,
                    machining: *machining,
                    anchor: *anchor,
                    params: params.clone(),
                };
                dispatch(session, MACHINING_UPDATE_ID, &json(&request)?)?;
                Ok(None)
            }
            MachiningAction::Remove { panel, machining } => {
                let request = PanelMachiningRemove { panel: *panel, machining: *machining };
                dispatch(session, MACHINING_REMOVE_ID, &json(&request)?)?;
                self.open.remove(machining);
                Ok(None)
            }
            MachiningAction::Move { panel, machining, index } => {
                let request = PanelMachiningMove { panel: *panel, machining: *machining, index: *index };
                dispatch(session, MACHINING_MOVE_ID, &json(&request)?)?;
                Ok(None)
            }
        }
    }

    /// Drops open flags for records the panel no longer has.
    fn retain_existing(&mut self, panel: &Panel) {
        self.open.retain(|id| panel.machining_by_id(*id).is_some());
    }

    /// Draws one operation row: chrome, then the body its kind contributes.
    ///
    /// Returns whether the user pressed *Edit contour* on this record.
    #[allow(clippy::too_many_arguments)]
    fn show_record(
        &mut self,
        ui: &mut Ui,
        panel_id: Id,
        record: &Machining,
        index: usize,
        count: usize,
        thickness: Length,
        diagnostics: &[PanelDiagnostic],
        contour_target: Option<ContourTarget>,
        single_selection: bool,
        actions: &mut Vec<MachiningAction>,
    ) -> bool {
        let id = record.id();
        let messages = bound_messages(diagnostics, id, crate::language(ui.ctx()));
        let mut open = self.open.contains(&id);

        ui.horizontal(|ui| {
            if ui
                .small_button(if open { "▾" } else { "▸" })
                .on_hover_text(Message::ShowHideOperation.text(crate::language(ui.ctx())))
                .clicked()
            {
                open = !open;
            }
            ui.label(RichText::new(operation_title(record, crate::language(ui.ctx()))).strong());
            if !messages.is_empty() {
                ui.colored_label(Color32::YELLOW, "⚠").on_hover_text(messages.join("\n"));
            }
            if ui
                .add_enabled(index > 0, egui::Button::new("↑").small())
                .on_hover_text(Message::MoveEarlier.text(crate::language(ui.ctx())))
                .clicked()
            {
                actions.push(MachiningAction::Move { panel: panel_id, machining: id, index: index - 1 });
            }
            if ui
                .add_enabled(index + 1 < count, egui::Button::new("↓").small())
                .on_hover_text(Message::MoveLater.text(crate::language(ui.ctx())))
                .clicked()
            {
                actions.push(MachiningAction::Move { panel: panel_id, machining: id, index: index + 1 });
            }
            if ui.small_button("−").on_hover_text(Message::RemoveOperation.text(crate::language(ui.ctx()))).clicked()
            {
                actions.push(MachiningAction::Remove { panel: panel_id, machining: id });
            }
        });

        if open {
            let contour_request =
                Self::show_body(ui, panel_id, record, thickness, &messages, contour_target, single_selection, actions);
            self.open.insert(id);
            contour_request
        } else {
            self.open.remove(&id);
            false
        }
    }

    /// Draws the expanded body: the shared anchor control, the kind section and its diagnostics.
    ///
    /// Returns whether the user pressed *Edit contour*.
    #[allow(clippy::too_many_arguments)]
    fn show_body(
        ui: &mut Ui,
        panel_id: Id,
        record: &Machining,
        thickness: Length,
        messages: &[String],
        contour_target: Option<ContourTarget>,
        single_selection: bool,
        actions: &mut Vec<MachiningAction>,
    ) -> bool {
        let capabilities = record.params().descriptor().capabilities();
        let mut anchor = record.anchor();
        let mut params = record.params().clone();
        let mut changed = false;
        let mut contour_request = false;

        ui.indent(("machining_body", record.id().get()), |ui| {
            if let Some(edited) = anchor_editor(ui, panel_id, record.id(), anchor, capabilities) {
                anchor = edited;
                changed = true;
            }
            let entry = ContourEntry::of(
                contour_target,
                ContourTarget::Cutout { panel: panel_id, machining: record.id() },
                single_selection,
            );
            let body = kind_body(ui, panel_id, record, thickness, entry);
            if let Some(edited) = body.params {
                params = edited;
                changed = true;
            }
            contour_request = body.edit_contour;
            for message in messages {
                ui.colored_label(Color32::LIGHT_RED, message);
            }
        });

        if changed {
            actions.push(MachiningAction::Update { panel: panel_id, machining: record.id(), anchor, params });
        }
        contour_request
    }

    /// Draws the add menu from the descriptor table, grouped by category.
    fn show_add_menu(&mut self, ui: &mut Ui, panel_id: Id, panel: &Panel, actions: &mut Vec<MachiningAction>) {
        ui.horizontal(|ui| {
            ComboBox::from_id_salt(("machining_add", panel_id.get()))
                .selected_text(Message::AddOperation.text(crate::language(ui.ctx())))
                .show_ui(ui, |ui| {
                    for (category, kinds) in add_menu_groups() {
                        ui.label(
                            RichText::new(machining_category_label(category).text(crate::language(ui.ctx()))).weak(),
                        );
                        for kind in kinds {
                            if ui
                                .selectable_label(false, machining_kind_label(kind).text(crate::language(ui.ctx())))
                                .clicked()
                                && let Some(anchor) = default_anchor(kind, panel)
                            {
                                actions.push(MachiningAction::Add {
                                    panel: panel_id,
                                    anchor,
                                    params: kind.descriptor().default_params(),
                                });
                            }
                        }
                        ui.separator();
                    }
                });
        });
    }
}

/// The heading of one operation row: the kind and the anchor it acts on.
fn operation_title(record: &Machining, lang: Lang) -> String {
    format!("{} · {}", machining_kind_label(record.params().kind()).text(lang), anchor_label(record.anchor(), lang))
}

/// The interface's name for a machining kind, so the model's English label never reaches the card.
fn machining_kind_label(kind: MachiningKind) -> Message {
    match kind {
        MachiningKind::Cutout => Message::MachiningCutout,
    }
}

/// The interface's name for an add-menu category.
fn machining_category_label(category: MachiningCategory) -> Message {
    match category {
        MachiningCategory::Cut => Message::MachiningCategoryCut,
    }
}

/// A short user-facing name for an anchor.
fn anchor_label(anchor: MachiningAnchor, lang: Lang) -> String {
    match anchor {
        MachiningAnchor::Face { side } => Message::FaceWith.with(lang, [side.label()]),
        MachiningAnchor::Edge { edge } => match edge {
            EdgeRef::Rect { edge } => Message::EdgeWith.with(lang, [edge_label(edge.get()).text(lang)]),
            EdgeRef::Ring { segment } => Message::SegmentWith.with(lang, [segment.get()]),
        },
    }
}

/// The machining record a diagnostic is about, when it names one.
///
/// A panel-wide problem such as a split core is not bound to a record and stays in the Diagnostics
/// section; an operation that does not fit is shown with that operation instead.
pub(crate) fn diagnostic_machining(diagnostic: &PanelDiagnostic) -> Option<MachiningId> {
    match diagnostic {
        PanelDiagnostic::MachiningOutsideCore { machining }
        | PanelDiagnostic::BlindDepthNotLessThanThickness { machining, .. } => Some(*machining),
        _ => None,
    }
}

/// The diagnostics bound to one record, formatted for the card.
fn bound_messages(diagnostics: &[PanelDiagnostic], id: MachiningId, lang: Lang) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_machining(diagnostic) == Some(id))
        .map(|diagnostic| diagnostic_message(diagnostic, lang).into_owned())
        .collect()
}

/// The add-menu groups in table order: a category with the kinds that belong to it.
fn add_menu_groups() -> Vec<(MachiningCategory, Vec<MachiningKind>)> {
    let mut groups: Vec<(MachiningCategory, Vec<MachiningKind>)> = Vec::new();
    for descriptor in MACHINING_DESCRIPTORS {
        let category = descriptor.category();
        match groups.iter_mut().find(|(existing, _)| *existing == category) {
            Some((_, kinds)) => kinds.push(descriptor.kind()),
            None => groups.push((category, vec![descriptor.kind()])),
        }
    }
    groups
}

/// The anchor a new operation starts on, chosen from its capabilities.
///
/// A kind that accepts a face starts on face A; one that only follows an edge starts on the first
/// outline edge. A kind that accepts neither is not offered by the table.
fn default_anchor(kind: MachiningKind, panel: &Panel) -> Option<MachiningAnchor> {
    let capabilities = kind.descriptor().capabilities();
    if capabilities.face() {
        return Some(MachiningAnchor::face(FaceSide::A));
    }
    if !capabilities.edge() {
        return None;
    }
    if let Some(contour) = panel.outline_contour() {
        let segment = contour.entries().first()?.id();
        return Some(MachiningAnchor::edge(EdgeRef::Ring { segment }));
    }
    let edge = EdgeId::new(1)?;
    Some(MachiningAnchor::edge(EdgeRef::Rect { edge }))
}

/// Draws the anchor control the kind's capabilities allow.
///
/// Only a face anchor is editable today; the branch for an edge anchor names it until the first
/// edge operation (D9) brings its own picker.
fn anchor_editor(
    ui: &mut Ui,
    panel_id: Id,
    machining: MachiningId,
    current: MachiningAnchor,
    capabilities: MachiningCapabilities,
) -> Option<MachiningAnchor> {
    let MachiningAnchor::Face { side } = current else {
        ui.label(
            Message::AnchorWith.with(crate::language(ui.ctx()), [anchor_label(current, crate::language(ui.ctx()))]),
        );
        return None;
    };
    if !capabilities.face() {
        return None;
    }
    let mut chosen = side;
    ui.horizontal(|ui| {
        ui.label(Message::Entry.text(crate::language(ui.ctx())));
        ComboBox::from_id_salt(("machining_face", panel_id.get(), machining.get()))
            .selected_text(Message::FaceWith.with(crate::language(ui.ctx()), [side.label()]))
            .show_ui(ui, |ui| {
                for option in [FaceSide::A, FaceSide::B] {
                    if ui
                        .selectable_label(
                            side == option,
                            Message::FaceWith.with(crate::language(ui.ctx()), [option.label()]),
                        )
                        .clicked()
                        && side != option
                    {
                        chosen = option;
                    }
                }
            });
    });
    (chosen != side).then_some(MachiningAnchor::face(chosen))
}

/// What one kind's body contributed to the card this frame.
#[derive(Debug, Default)]
struct BodyEdit {
    /// Replacement parameters when a field committed a change.
    params: Option<MachiningParams>,
    /// Whether the user pressed *Edit contour*.
    edit_contour: bool,
}

/// Draws the body one operation kind contributes.
fn kind_body(ui: &mut Ui, panel_id: Id, record: &Machining, thickness: Length, entry: ContourEntry) -> BodyEdit {
    match record.params() {
        MachiningParams::Cutout(cutout) => {
            let edited = cutout::show(ui, panel_id, record.id(), cutout, thickness, entry);
            BodyEdit { params: edited.params.map(MachiningParams::Cutout), edit_contour: edited.edit_contour }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use furnicraft_model::{Cutout, CutoutDepth, CutoutShape, PanelRect};
    use furnicraft_testkit::{sample_catalog, sample_cutout_rect, sample_project};

    fn mm(millimetres: i64) -> Length {
        Length::from_millimetres(millimetres).expect("millimetres")
    }

    fn panel_id() -> Id {
        Id::new(2).expect("panel id")
    }

    fn session() -> Session {
        Session::with_catalog(sample_project(), sample_catalog())
    }

    fn cutout(rect: PanelRect, depth: CutoutDepth) -> MachiningParams {
        MachiningParams::Cutout(Cutout::new(CutoutShape::Rectangle(rect), depth))
    }

    fn add(section: &mut MachiningSection, session: &mut Session, params: MachiningParams) -> MachiningId {
        section
            .run_action(
                session,
                &MachiningAction::Add { panel: panel_id(), anchor: MachiningAnchor::face(FaceSide::A), params },
            )
            .expect("add machining")
            .expect("an add reports its record")
    }

    /// Draws one frame of the section on the sample panel.
    fn frame(session: &Session, section: &mut MachiningSection) -> Vec<MachiningAction> {
        let panel = session.project().panel(panel_id()).expect("panel");
        let diagnostics = session.panel_diagnostics(panel_id());
        let thickness = session
            .panel_geometry(panel_id())
            .map_or_else(|_| panel.last_known_thickness(), |geometry| geometry.thickness());
        let mut actions = Vec::new();
        let mut request = None;
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0))),
                ..Default::default()
            },
            |ui| request = section.show(ui, panel_id(), panel, thickness, &diagnostics, None, true, &mut actions),
        );
        output.textures_delta.clear();
        assert!(request.is_none(), "no frame presses Edit contour");
        actions
    }

    #[test]
    fn the_add_menu_lists_every_kind_once_in_table_order() {
        let groups = add_menu_groups();
        let listed: Vec<MachiningKind> = groups.iter().flat_map(|(_, kinds)| kinds.iter().copied()).collect();
        assert_eq!(listed.as_slice(), MachiningKind::ALL);
        let categories: HashSet<MachiningCategory> =
            MachiningKind::ALL.iter().map(|kind| kind.descriptor().category()).collect();
        assert_eq!(groups.len(), categories.len());
        for (category, kinds) in groups {
            assert!(!kinds.is_empty(), "category {} is empty", category.label());
            assert!(!category.label().is_empty());
        }
    }

    #[test]
    fn a_new_operation_starts_on_an_anchor_its_kind_accepts() {
        let session = session();
        let panel = session.project().panel(panel_id()).expect("panel");
        for kind in MachiningKind::ALL {
            let anchor = default_anchor(*kind, panel).expect("every kind accepts an anchor");
            assert!(kind.descriptor().capabilities().accepts_anchor(anchor), "{kind:?} must accept {anchor:?}");
        }
    }

    #[test]
    fn an_edge_only_kind_anchors_on_the_first_ring_segment_of_a_contour_panel() {
        use furnicraft_model::{Contour, Placement};
        let mut project = sample_project();
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [mm(700), Length::ZERO],
            [mm(700), mm(500)],
            [Length::ZERO, mm(500)],
        ])
        .expect("ring");
        let id = project
            .add_panel_contour(project.root().id(), "Contour", &ring, None, mm(18), Placement::IDENTITY)
            .expect("contour panel");
        let panel = project.panel(id).expect("panel");
        let edge_only = MachiningKind::ALL.iter().copied().find(|kind| {
            let capabilities = kind.descriptor().capabilities();
            !capabilities.face() && capabilities.edge()
        });
        // No edge-only kind exists yet; the branch is exercised once D9 adds one.
        let Some(kind) = edge_only else {
            return;
        };
        let anchor = default_anchor(kind, panel).expect("edge anchor");
        let EdgeRef::Ring { segment } = anchor.as_edge().expect("edge anchor") else {
            panic!("a contour panel anchors an edge operation on a ring segment");
        };
        assert_eq!(Some(segment), ring.entries().first().map(|entry| entry.id()));
    }

    #[test]
    fn an_added_operation_opens_and_dispatches_through_the_engine() {
        let mut session = session();
        let mut section = MachiningSection::default();
        let before = session.project().revision();
        let created = add(&mut section, &mut session, cutout(sample_cutout_rect(), CutoutDepth::Through));
        assert!(section.open.contains(&created));
        assert_eq!(session.project().panel(panel_id()).expect("panel").machining().len(), 1);
        assert!(session.project().revision() > before);
    }

    #[test]
    fn row_chrome_updates_moves_and_removes_one_record() {
        let mut session = session();
        let mut section = MachiningSection::default();
        let first = add(&mut section, &mut session, cutout(sample_cutout_rect(), CutoutDepth::Through));
        let second = add(
            &mut section,
            &mut session,
            cutout(
                PanelRect::new(mm(300), mm(300), mm(100), mm(50)).expect("rect"),
                CutoutDepth::Blind { depth: mm(8) },
            ),
        );
        assert_ne!(first, second);

        section
            .run_action(
                &mut session,
                &MachiningAction::Update {
                    panel: panel_id(),
                    machining: first,
                    anchor: MachiningAnchor::face(FaceSide::B),
                    params: cutout(sample_cutout_rect(), CutoutDepth::Blind { depth: mm(6) }),
                },
            )
            .expect("update");
        let panel = session.project().panel(panel_id()).expect("panel");
        let record = panel.machining_by_id(first).expect("record");
        assert_eq!(record.anchor().as_face(), Some(FaceSide::B));
        assert_eq!(record.cutout().map(Cutout::depth), Some(CutoutDepth::Blind { depth: mm(6) }));

        section
            .run_action(&mut session, &MachiningAction::Move { panel: panel_id(), machining: first, index: 1 })
            .expect("move");
        let panel = session.project().panel(panel_id()).expect("panel");
        assert_eq!(panel.machining().last().map(Machining::id), Some(first));

        section
            .run_action(&mut session, &MachiningAction::Remove { panel: panel_id(), machining: first })
            .expect("remove");
        let panel = session.project().panel(panel_id()).expect("panel");
        assert!(panel.machining_by_id(first).is_none());
        assert!(!section.open.contains(&first));

        session.undo().expect("undo");
        assert!(session.project().panel(panel_id()).expect("panel").machining_by_id(first).is_some());
    }

    #[test]
    fn a_rejected_edit_leaves_the_record_alone_and_reports_the_error() {
        let mut session = session();
        let mut section = MachiningSection::default();
        let created = add(&mut section, &mut session, cutout(sample_cutout_rect(), CutoutDepth::Through));
        let error = section
            .run_action(
                &mut session,
                &MachiningAction::Update {
                    panel: panel_id(),
                    machining: created,
                    anchor: MachiningAnchor::face(FaceSide::A),
                    params: cutout(
                        PanelRect::new(mm(5_000), mm(5_000), mm(100), mm(50)).expect("rect"),
                        CutoutDepth::Through,
                    ),
                },
            )
            .expect_err("a cutout outside the panel is refused");
        assert!(!error.is_empty());
        let panel = session.project().panel(panel_id()).expect("panel");
        assert_eq!(
            panel.machining_by_id(created).and_then(Machining::cutout).map(Cutout::shape),
            Some(&CutoutShape::Rectangle(sample_cutout_rect()))
        );
    }

    #[test]
    fn open_rows_are_pruned_to_the_records_the_panel_has() {
        let mut session = session();
        let mut section = MachiningSection::default();
        let created = add(&mut section, &mut session, cutout(sample_cutout_rect(), CutoutDepth::Through));
        section.open.insert(MachiningId::new(999).expect("stale id"));
        let panel = session.project().panel(panel_id()).expect("panel");
        section.retain_existing(panel);
        assert_eq!(section.open, HashSet::from([created]));
    }

    #[test]
    fn diagnostics_bound_to_a_record_stay_with_it() {
        let id = MachiningId::new(9).expect("id");
        assert_eq!(diagnostic_machining(&PanelDiagnostic::MachiningOutsideCore { machining: id }), Some(id));
        assert_eq!(
            diagnostic_machining(&PanelDiagnostic::BlindDepthNotLessThanThickness {
                machining: id,
                depth: mm(20),
                thickness: mm(18),
            }),
            Some(id)
        );
        assert_eq!(diagnostic_machining(&PanelDiagnostic::CoreDisconnected), None);
        assert_eq!(diagnostic_machining(&PanelDiagnostic::NoMaterial), None);
    }

    #[test]
    fn the_title_names_the_kind_and_its_anchor() {
        let record = Machining::new(
            MachiningId::new(5).expect("id"),
            MachiningAnchor::face(FaceSide::B),
            cutout(sample_cutout_rect(), CutoutDepth::Through),
        );
        assert_eq!(operation_title(&record, Lang::En), "Cutout · Face B");
        let edge = Machining::new(
            MachiningId::new(6).expect("id"),
            MachiningAnchor::edge(EdgeRef::Rect { edge: EdgeId::new(1).expect("edge") }),
            cutout(sample_cutout_rect(), CutoutDepth::Through),
        );
        assert_eq!(operation_title(&edge, Lang::En), "Cutout · Edge Bottom");
    }

    #[test]
    fn an_empty_panel_draws_its_add_menu_without_dispatching() {
        let session = session();
        let mut section = MachiningSection::default();
        assert!(frame(&session, &mut section).is_empty());
    }

    #[test]
    fn an_open_row_draws_from_the_document_without_dispatching() {
        let mut session = session();
        let seeded = session
            .dispatch(
                MACHINING_ADD_ID,
                &json(&PanelMachiningAdd {
                    panel: panel_id(),
                    anchor: MachiningAnchor::face(FaceSide::A),
                    params: cutout(sample_cutout_rect(), CutoutDepth::Through),
                })
                .expect("params"),
            )
            .expect("seed the panel");
        let created = seeded.created_machining_id().expect("created");
        let mut section = MachiningSection::default();
        section.open.insert(created);
        assert!(frame(&session, &mut section).is_empty());
        assert!(section.open.contains(&created), "an open row stays open while it exists");
    }
}
