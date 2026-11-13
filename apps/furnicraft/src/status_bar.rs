//! The status bar: the armed tool, the selection's context and the window's display state.
//!
//! One thin row at the bottom of the window, in three zones: the command and step on the left, the
//! resolved selection in the middle — kind, name, size, material, banding and the first warning —
//! and the snap, projection, scale and idle/busy state on the right. It reads a [`StatusInfo`]
//! snapshot of the session, the editor and the view camera, draws one row and returns what the row
//! asked for in a [`StatusOut`]; the shell applies that after the row has drawn, so the bar never
//! changes the document while it draws. It offers no view control: those float over the viewport in
//! `crate::view_cluster`, so the scene starts at the model instead of at a wall of labels.
//!
//! Changes when: what the bar reports or the buttons it offers change — not when a tool gains a
//! gesture, which belongs to the tool's own module.

use eframe::egui;
use furnicraft_model::{Catalog, Geometry, Id, Node, Project};
use furnicraft_scene::Projection;
use furnicraft_text::{Lang, Message};
use furnicraft_ui::{DrawTool, MeshElement, theme};
use furnicraft_units::Length;

use crate::document::Document;
use crate::view_cluster::NavigationMode;
use crate::{FurniCraftApp, TransformMode};

/// What separates the parts of the selection context.
const SUMMARY_SEPARATOR: &str = " · ";

/// The status text and controls, read once per frame so both sides of the bar draw from data.
struct StatusInfo {
    error: Option<String>,
    /// Why the workspace defaults were used instead of the saved settings, if they were.
    notice: Option<crate::settings_store::Notice>,
    mode: TransformMode,
    contour: Option<(usize, bool, bool)>,
    drawing: Option<(DrawTool, usize, bool, bool)>,
    /// Why the pointer cannot extend the live draft, if it cannot.
    preview_error: Option<String>,
    primitive: Option<(furnicraft_engine::PrimitiveKind, Length)>,
    grid: bool,
    mesh: Option<(MeshElement, usize)>,
    /// The navigation mode armed in the floating cluster, if any.
    navigation: Option<NavigationMode>,
    /// The container armed to receive the next creation, if any.
    creation_parent: Option<String>,
    /// The selection context of the centre zone.
    summary: StatusSummary,
    /// The projection the active view is drawing with.
    projection: Projection,
    /// Whether this frame evaluates the scene, which the right zone reports.
    rebuilding: bool,
}

/// The selection context, built once per frame from the selection the editor already resolved.
///
/// Every field is optional so a new parameter is one more field and one more chip, never a second
/// row. Derived values are read from the selected node alone: the summary never walks the tree and
/// never resolves a panel's geometry, which is the card's job.
#[derive(Clone, Debug, Default, PartialEq)]
struct StatusSummary {
    /// How many nodes the selection holds.
    count: usize,
    /// The selected node's name; absent for a multi-selection or an empty one.
    name: Option<String>,
    /// The node kind, such as *panel* or *component*.
    kind: Option<String>,
    /// A panel's nominal length, width and thickness.
    size: Option<String>,
    /// A panel's material name.
    material: Option<String>,
    /// The edge-banding thickness, when the panel carries a band.
    banding: Option<String>,
    /// The first problem the selection would report.
    warning: Option<StatusWarning>,
}

/// One warning the selection carries, drawn as a chip in the status bar.
#[derive(Clone, Debug, PartialEq)]
struct StatusWarning {
    /// The sentence the chip shows.
    text: String,
    /// What the user can do about it, shown on hover.
    hint: Option<String>,
}

impl StatusSummary {
    /// Builds the context of `selection`, whose primary is the last id.
    ///
    /// The editor hands in the selection it already resolved, so this reads the selected nodes only:
    /// no tree walk, no geometry solve and no second look at the document. The warning is the
    /// cheapest one to know — the editor's lock flag and a catalogue lookup; an outline that does not
    /// resolve needs the panel's geometry, which the inspector already computes.
    fn of(project: &Project, catalog: &Catalog, selection: &[Id], locked: Option<Id>, lang: Lang) -> Self {
        if selection.is_empty() {
            return Self::default();
        }
        let primary = selection.last().copied();
        let mut summary = Self { count: selection.len(), ..Self::default() };
        let Some(node) = primary.and_then(|id| project.node(id)) else {
            return summary;
        };
        // The warning is the primary's in either case; name, size and material describe one node,
        // so a multi-selection carries the count and stops there.
        summary.warning = warning_of(catalog, node, locked, lang);
        if summary.count > 1 {
            return summary;
        }
        summary.name = Some(node.name().to_owned());
        match node {
            Node::Panel(panel) => {
                summary.kind = Some(Message::ToolPanel.text(lang).to_owned());
                let nominal = panel.nominal_size();
                summary.size = Some(
                    Message::StatusSize.with(lang, [nominal.length(), nominal.width(), panel.last_known_thickness()]),
                );
                summary.material = panel
                    .material()
                    .and_then(|id| catalog.panel_material(id))
                    .map(|material| material.name().to_owned());
                summary.banding = panel.edge_banding().first().map(|band| {
                    let thickness = catalog
                        .edge_material(band.material())
                        .map_or_else(|| band.last_known_thickness(), |material| material.thickness());
                    Message::StatusBanding.with(lang, [thickness])
                });
            }
            Node::Container(container) => {
                summary.kind = Some(
                    if container.is_component() {
                        Message::ContainerRoleComponent
                    } else {
                        Message::ContainerRoleGroup
                    }
                    .text(lang)
                    .to_owned(),
                );
            }
            Node::Object(object) => {
                summary.kind = Some(
                    match object.geometry() {
                        Geometry::Mesh(_) => Message::PrimitiveBadgeMesh,
                        Geometry::Constructive(_) => Message::PrimitiveBadgeConstructive,
                        Geometry::Primitive(_) => Message::PrimitiveBadgePrimitive,
                        Geometry::Path(_) => Message::PrimitiveBadgePath,
                    }
                    .text(lang)
                    .to_owned(),
                );
            }
            Node::Camera(_) => summary.kind = Some(Message::ToolCamera.text(lang).to_owned()),
            Node::Light(_) => summary.kind = Some(Message::ToolLight.text(lang).to_owned()),
        }
        summary
    }

    /// Whether the zone has anything to draw.
    fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The line the centre zone draws, or `None` when nothing is selected.
    fn line(&self, lang: Lang) -> Option<String> {
        // A multi-selection has no single name, size or material to report: the count is the one
        // fact that describes it without pretending it is one node.
        if self.count > 1 {
            return Some(Message::StatusSelectionCount.with(lang, [self.count]));
        }
        let parts: Vec<&str> = [&self.name, &self.kind, &self.size, &self.material, &self.banding]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join(SUMMARY_SEPARATOR))
    }
}

/// The first problem of one selected node: a lock, then a missing panel material.
///
/// Every check is a flag or a catalogue lookup, so the frame path stays free of derived geometry.
/// A missing edge material or an unresolved outline is a panel diagnostic: it needs the resolved
/// panel, which the inspector computes for its own section and the bar does not repeat per frame.
fn warning_of(catalog: &Catalog, node: &Node, locked: Option<Id>, lang: Lang) -> Option<StatusWarning> {
    if locked == Some(node.id()) {
        return Some(StatusWarning {
            text: Message::LockedStatus.with(lang, [node.name()]),
            hint: Some(Message::UnlockHint.text(lang).to_owned()),
        });
    }
    let Node::Panel(panel) = node else {
        return None;
    };
    match panel.material() {
        None => Some(StatusWarning { text: Message::DiagNoMaterial.text(lang).to_owned(), hint: None }),
        Some(material) if catalog.panel_material(material).is_none() => {
            Some(StatusWarning { text: Message::DiagPanelMaterialMissing.with(lang, [material]), hint: None })
        }
        Some(_) => None,
    }
}

/// What the bar asked for, after the row has drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatusOut {
    state: Option<StatusAction>,
    mode: Option<TransformMode>,
}

/// What the status text asked the shell to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusAction {
    ApplyContour,
    CancelContour,
    ToggleContourAlong,
    ToggleGrid,
    CloseDraft,
    FinishDraft,
    CancelDraft,
    CancelPrimitive,
    ClearCreationParent,
    EndNavigation,
}

/// A quiet hint that truncates instead of running under the right-hand view controls.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::text_dim(ui))).truncate());
}

/// The armed tool, the gesture it expects and the counts it would affect.
fn status_state_ui(ui: &mut egui::Ui, info: &StatusInfo) -> (Option<StatusAction>, Option<TransformMode>) {
    let lang = furnicraft_ui::language(ui.ctx());
    if let Some(error) = &info.error {
        ui.colored_label(theme::danger(ui), error);
        theme::divider(ui);
    } else if let Some(notice) = &info.notice {
        // A workspace that fell back to its defaults is a fact, not a failure: it uses the quiet
        // text colour and keeps the rest of the bar usable.
        hint(ui, &notice.text(lang));
        theme::divider(ui);
    }
    // A locked selection refuses the gizmo and the destructive commands; the centre zone carries
    // the chip that names it (ADR 0065).
    // The arm is sticky across creations, so the bar names the container the next node lands in;
    // without the chip a pressed tool would create in a container armed much earlier (ADR 0056).
    if let Some(name) = &info.creation_parent {
        ui.label(Message::AddingTo.with(lang, [name]));
        if ui.button("✕").on_hover_text(Message::CreateAtRoot.text(lang)).clicked() {
            return (Some(StatusAction::ClearCreationParent), None);
        }
        theme::divider(ui);
    }
    if let Some((segments, dirty, along)) = info.contour {
        let mut summary = Message::counted(
            segments,
            Message::ContourEditStatusOne,
            Message::ContourEditStatusFew,
            Message::ContourEditStatusMany,
        )
        .text(lang);
        if dirty {
            summary.push_str(Message::ContourModifiedSuffix.text(lang));
        }
        ui.label(summary);
        let motion = ui
            .selectable_label(along, if along { Message::AlongContour } else { Message::FreeMove }.text(lang))
            .clicked();
        let apply = ui.button(Message::Apply.text(lang)).clicked();
        let cancel = ui.button(Message::Cancel.text(lang)).clicked();
        hint(ui, if along { Message::ContourAlongHint } else { Message::ContourFreeHint }.text(lang));
        let action = if motion {
            Some(StatusAction::ToggleContourAlong)
        } else if apply {
            Some(StatusAction::ApplyContour)
        } else if cancel {
            Some(StatusAction::CancelContour)
        } else {
            None
        };
        return (action, None);
    }
    if let Some((tool, points, closed, arc_mode)) = info.drawing {
        let points_suffix = if points == 0 {
            String::new()
        } else {
            Message::counted(points, Message::PointsSuffixOne, Message::PointsSuffixFew, Message::PointsSuffixMany)
                .text(lang)
        };
        let kind_suffix = if tool.closes() {
            if arc_mode { Message::DrawArcSuffix.text(lang) } else { Message::DrawLineSuffix.text(lang) }
        } else {
            ""
        };
        ui.label(Message::DrawStatus.with(lang, [tool.label().text(lang), &points_suffix, kind_suffix]));
        // The ribbon's *Close* and this one read the same rule, so a closure one offers is never
        // greyed out by the other (ADR 0077).
        let close = ui
            .add_enabled(
                furnicraft_ui::toolbars::draft_can_close(tool, points, closed),
                egui::Button::new(Message::Close.text(lang)),
            )
            .clicked();
        let finish = ui.add_enabled(points >= 2, egui::Button::new(Message::FinishEnter.text(lang))).clicked();
        let cancel = ui.button(Message::CancelEsc.text(lang)).clicked();
        hint(ui, Message::DrawHint.text(lang));
        if let Some(reason) = &info.preview_error {
            hint(ui, reason);
        }
        let action = if close {
            Some(StatusAction::CloseDraft)
        } else if finish {
            Some(StatusAction::FinishDraft)
        } else if cancel {
            Some(StatusAction::CancelDraft)
        } else {
            None
        };
        return (action, None);
    }
    // A live pull arrow is drawn and finished by the viewport's own command bar: the bar owns the
    // height field, the check and the cross, so the status bar names no second Apply (M71).
    if let Some((kind, size)) = info.primitive {
        let name = match kind {
            furnicraft_engine::PrimitiveKind::Cuboid => Message::ToolBox,
            furnicraft_engine::PrimitiveKind::Cylinder => Message::ToolCylinder,
            furnicraft_engine::PrimitiveKind::Sphere => Message::ToolSphere,
        };
        ui.label(Message::PrimitiveStatus.with(lang, [name.text(lang), &size.to_string()]));
        let cancel = ui.button(Message::CancelEsc.text(lang)).clicked();
        hint(ui, Message::PrimitivePlaceHint.text(lang));
        return (cancel.then_some(StatusAction::CancelPrimitive), None);
    }
    if let Some(mode) = info.navigation {
        ui.label(Message::NavigationStatus.with(lang, [mode.name().text(lang)]));
        let done = ui.button(Message::DoneEsc.text(lang)).clicked();
        hint(ui, mode.gesture().text(lang));
        return (done.then_some(StatusAction::EndNavigation), None);
    }
    ui.label(egui::RichText::new(Message::Gizmo.text(lang)).color(theme::text_dim(ui)));
    let mut mode = info.mode;
    let moved = ui.selectable_value(&mut mode, TransformMode::Move, Message::Move.text(lang)).clicked()
        | ui.selectable_value(&mut mode, TransformMode::Rotate, Message::Rotate.text(lang)).clicked();
    theme::divider(ui);
    if let Some((element, count)) = info.mesh {
        let counted = match element {
            MeshElement::Vertex => Message::counted(
                count,
                Message::MeshEditStatusOne,
                Message::MeshEditStatusFew,
                Message::MeshEditStatusMany,
            ),
            MeshElement::Face => Message::counted(
                count,
                Message::MeshFaceStatusOne,
                Message::MeshFaceStatusFew,
                Message::MeshFaceStatusMany,
            ),
        };
        ui.label(counted.text(lang));
        ui.label(egui::RichText::new(Message::MeshEditHint.text(lang)).color(theme::text_dim(ui)));
    } else {
        hint(ui, Message::ViewportHint.text(lang));
    }
    (None, moved.then_some(mode))
}

impl FurniCraftApp {
    /// The thin bar at the bottom of the window: the armed tool, the gesture it expects, the counts
    /// it would affect and the last error.
    ///
    /// View controls no longer live here: they float over the viewport in
    /// [`crate::view_cluster::ViewCluster`], so the scene starts at the model instead of at a wall of
    /// labels. The bar is chrome and carries no document state; everything it shows is read from the
    /// active tab, and the two things that belong to the window — the transform mode and the armed
    /// navigation gesture — are applied here rather than in the document.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let notice = self.settings_notice.clone();
        let lang = furnicraft_ui::language(ui.ctx());
        let info = self.tabs.active().status_info(
            notice.as_ref(),
            self.shell.mode,
            self.shell.cluster.navigation,
            self.shell.rebuilding,
            lang,
        );
        let mut out = StatusOut::default();
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            status_row_ui(ui, &info, &mut out);
        });
        if let Some(mode) = out.mode {
            self.shell.mode = mode;
        }
        match out.state {
            // The window's two: the rest belong to the scene the bar reports on.
            Some(StatusAction::EndNavigation) => self.shell.cluster.navigation = None,
            Some(action) => self.tabs.active_mut().apply_status(action, lang),
            None => {}
        }
    }
}

/// The one row in three zones: the command on the left, the selection in the middle and the
/// window's display state on the right.
///
/// The command keeps its natural width and the centre elides into what the right zone leaves, so an
/// active tool never loses a button to a long node name; the right zone keeps a fixed reserve for
/// its readouts.
fn status_row_ui(ui: &mut egui::Ui, info: &StatusInfo, out: &mut StatusOut) {
    let lang = furnicraft_ui::language(ui.ctx());
    ui.horizontal_centered(|ui| {
        let row = ui.available_width();
        let right = (row * 0.30).clamp(120.0, 260.0).min(row);
        let height = theme::STATUS_HEIGHT;
        ui.allocate_ui_with_layout(
            egui::vec2((row - right).max(0.0), height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                (out.state, out.mode) = status_state_ui(ui, info);
                if !info.summary.is_empty() {
                    theme::divider(ui);
                    status_center_ui(ui, &info.summary, lang);
                }
            },
        );
        ui.allocate_ui_with_layout(egui::vec2(right, height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(action) = status_right_ui(ui, info) {
                out.state = Some(action);
            }
        });
    });
}

/// The centre zone: the selection's context, elided to the width it is given.
///
/// One label rather than a row of chips, so the whole line truncates with one ellipsis and one
/// tooltip: a `Label::truncate` keeps the full line in its hover text, which is where the parts that
/// did not fit stay readable.
fn status_center_ui(ui: &mut egui::Ui, summary: &StatusSummary, lang: Lang) {
    if let Some(warning) = &summary.warning {
        warning_chip(ui, warning);
        ui.add_space(theme::STEP);
    }
    let Some(line) = summary.line(lang) else {
        return;
    };
    ui.add(egui::Label::new(egui::RichText::new(&line)).truncate()).on_hover_text(&line);
}

/// The right zone: snap, projection, scale and whether the scene is rebuilding.
///
/// It is drawn right to left, so the state sits at the window edge and the snap toggle next to the
/// selection context. Only the snap is a control; the rest are readouts of the window's view state.
fn status_right_ui(ui: &mut egui::Ui, info: &StatusInfo) -> Option<StatusAction> {
    let lang = furnicraft_ui::language(ui.ctx());
    let (state, colour) = if info.rebuilding {
        (Message::StatusRebuilding, theme::warn(ui))
    } else {
        (Message::StatusReady, theme::text_dim(ui))
    };
    ui.label(egui::RichText::new(state.text(lang)).color(colour));
    theme::divider(ui);
    let scale = furnicraft_ui::ui_scale::percent(ui.ctx().zoom_factor());
    ui.label(egui::RichText::new(Message::StatusScale.with(lang, [scale])).color(theme::text_dim(ui)))
        .on_hover_text(Message::ScaleHint.text(lang));
    theme::divider(ui);
    let projection = match info.projection {
        Projection::Perspective { .. } => Message::Perspective,
        Projection::Orthographic { .. } => Message::Orthographic,
    };
    ui.label(egui::RichText::new(projection.text(lang)).color(theme::text_dim(ui)))
        .on_hover_text(Message::ShortcutProjection.text(lang));
    theme::divider(ui);
    let snap = ui
        .selectable_label(info.grid, Message::SnapTenMillimetres.text(lang))
        .on_hover_text(Message::ShortcutGrid.text(lang));
    snap.clicked().then_some(StatusAction::ToggleGrid)
}

/// A warning as a tinted pill, so a refusal reads as a state and not as another hint.
fn warning_chip(ui: &mut egui::Ui, warning: &StatusWarning) -> egui::Response {
    let colour = theme::warn(ui);
    let frame = egui::Frame::new()
        .fill(colour.gamma_multiply(0.18))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(5, 1));
    let response = frame.show(ui, |ui| ui.label(egui::RichText::new(&warning.text).color(colour).small())).response;
    match &warning.hint {
        Some(hint) => response.on_hover_text(hint),
        None => response,
    }
}

impl Document {
    /// Everything the bar reads from this document.
    ///
    /// The mode, the armed navigation gesture and the busy state are the window's, so they are
    /// handed in instead of read here; everything else belongs to the scene the tab holds.
    fn status_info(
        &self,
        notice: Option<&crate::settings_store::Notice>,
        mode: TransformMode,
        navigation: Option<NavigationMode>,
        rebuilding: bool,
        lang: Lang,
    ) -> StatusInfo {
        let project = self.session.project();
        let selection = self.editor.selected_nodes();
        let locked = self.editor.locked_selection(project);
        StatusInfo {
            error: self.last_error.clone(),
            notice: notice.cloned(),
            mode,
            contour: self
                .editor
                .contour_edit()
                .map(|edit| (edit.draft().segment_count(), edit.is_dirty(), self.contour_gizmo.along())),
            drawing: self
                .editor
                .drawing()
                .map(|draft| (draft.tool(), draft.len(), draft.is_closed(), draft.arc_mode())),
            preview_error: self.draw_gizmo.preview_error().map(str::to_owned),
            primitive: self.editor.pending_primitive(),
            grid: self.draw_gizmo.grid(),
            mesh: self.editor.mesh_edit().map(|edit| {
                let count = match edit.element() {
                    MeshElement::Vertex => edit.vertices().len(),
                    MeshElement::Face => edit.faces().len(),
                };
                (edit.element(), count)
            }),
            navigation,
            creation_parent: self.editor.creation_parent_name(project).map(str::to_owned),
            summary: StatusSummary::of(project, self.session.catalog(), selection, locked, lang),
            projection: self.projection(),
            rebuilding,
        }
    }

    /// Runs the one document-side action the bar asked for.
    ///
    /// The armed navigation gesture is the window's, so the shell clears that one itself; every arm
    /// here acts on the scene this tab holds.
    fn apply_status(&mut self, action: StatusAction, lang: furnicraft_text::Lang) {
        match action {
            StatusAction::ApplyContour => {
                if let Err(error) = self.apply_contour_edit() {
                    self.last_error = Some(error);
                }
            }
            StatusAction::CancelContour => self.cancel_contour_edit(),
            StatusAction::ToggleContourAlong => self.contour_gizmo.toggle_along(),
            StatusAction::ToggleGrid => self.draw_gizmo.toggle_grid(),
            StatusAction::CloseDraft => {
                if let Err(error) = self.close_draft(lang) {
                    self.last_error = Some(error);
                }
            }
            StatusAction::FinishDraft => {
                if let Err(error) = self.commit_drawing(lang) {
                    self.last_error = Some(error);
                }
            }
            StatusAction::CancelDraft => self.cancel_drawing(),
            StatusAction::CancelPrimitive => self.editor.cancel_primitive(),
            StatusAction::ClearCreationParent => self.editor.clear_creation_parent(),
            StatusAction::EndNavigation => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::test_support::drawn_text;
    use furnicraft_engine::panel_cmds::EDGE_BANDING_SET_ID;
    use furnicraft_engine::{PanelEdgeBandingSet, Session};
    use furnicraft_model::{Container, PanelNominalSize, Placement, RectEdge};
    use furnicraft_testkit::{
        millimetres, sample_catalog, sample_edge_material_id, sample_material_id, sample_project,
    };

    fn idle_status() -> StatusInfo {
        StatusInfo {
            error: None,
            notice: None,
            mode: TransformMode::Move,
            contour: None,
            drawing: None,
            preview_error: None,
            primitive: None,
            grid: true,
            mesh: None,
            navigation: None,
            creation_parent: None,
            summary: StatusSummary::default(),
            projection: Projection::Perspective { fov_y: 0.9 },
            rebuilding: false,
        }
    }

    #[test]
    fn the_status_bar_draws_one_thin_row() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = idle_status();
        let mut out = StatusOut::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            ui.horizontal_centered(|ui| {
                (out.state, out.mode) = status_state_ui(ui, &info);
            });
        });
        output.drop_without_applying_deltas();
        assert_eq!(out.state, None, "drawing the idle bar asks for nothing");
        assert_eq!(out.mode, None);
    }

    #[test]
    fn the_status_bar_names_the_armed_drawing_tool() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { drawing: Some((DrawTool::Rectangle, 3, false, false)), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Draw")), "the armed tool is named: {texts:?}");
        assert!(texts.iter().any(|text| text.contains('3')), "the draft's point count is shown: {texts:?}");
        assert_eq!(out.state, None, "drawing alone runs no tool");
    }

    #[test]
    fn the_status_bar_offers_close_from_two_segments_and_through_a_pending_arc() {
        // The status bar is the second *Close* button. It enables from two segments — the closing
        // one makes three — and a pending arc is drawn as a segment the draft already has, so the
        // count it reports is the one the command needs (ADR 0077). The rule is the ribbon's, so a
        // closure one surface offers is never greyed out by the other.
        assert!(furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 2, false));
        assert!(furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 3, false));
        assert!(!furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 1, false));
        assert!(!furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 2, true));

        // The bar still draws the button, so the pair never vanishes between clicks.
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { drawing: Some((DrawTool::Polyline, 2, false, true)), ..idle_status() };
        let output = ctx.run_ui(Default::default(), |ui| {
            status_state_ui(ui, &info);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Close")), "the bar names its own Close: {texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("arc")),
            "the bar names the pending arc's segment kind: {texts:?}"
        );
    }

    #[test]
    fn the_status_bar_names_the_locked_node() {
        // The gizmo and the destructive commands refuse a locked node, so the centre zone carries a
        // warning that names it and a hint that says what unlocks it (ADR 0065).
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let summary = StatusSummary {
            count: 1,
            name: Some("Drawer".to_owned()),
            kind: Some("Panel".to_owned()),
            warning: Some(StatusWarning {
                text: Message::LockedStatus.with(Lang::En, ["Drawer"]),
                hint: Some(Message::UnlockHint.text(Lang::En).to_owned()),
            }),
            ..StatusSummary::default()
        };
        let output = ctx.run_ui(Default::default(), |ui| status_center_ui(ui, &summary, Lang::En));
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Locked · Drawer")), "the locked node names itself: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Drawer · Panel")), "and keeps its context: {texts:?}");
    }

    #[test]
    fn the_status_bar_names_the_armed_container_and_offers_to_clear_it() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { creation_parent: Some("Carcass".to_owned()), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(
            texts.iter().any(|text| text.contains("Adding to: Carcass")),
            "the armed container names itself: {texts:?}"
        );
        assert!(texts.iter().any(|text| text.contains('✕')), "the bar offers to clear the arm: {texts:?}");
    }

    #[test]
    fn the_status_bar_names_the_armed_navigation_gesture() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { navigation: Some(NavigationMode::Orbit), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Navigation · Orbit")), "the armed mode is named: {texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("Drag in the viewport to orbit")),
            "the bar names the gesture the mode expects: {texts:?}"
        );
        assert_eq!(out.state, None, "an idle armed mode runs nothing until its button is clicked");
    }

    /// One *Front* panel with a material and an edge band next to a plain *Shelf*.
    fn banded_session() -> Session {
        let size = PanelNominalSize::new(millimetres(600), millimetres(720)).expect("a 600 × 720 panel");
        let front = furnicraft_model::Panel::new(
            Id::new(2).expect("id"),
            "Front",
            size,
            Some(sample_material_id()),
            millimetres(18),
            Placement::IDENTITY,
        )
        .expect("the front panel is valid");
        let shelf = furnicraft_model::Panel::new(
            Id::new(4).expect("id"),
            "Shelf",
            size,
            None,
            millimetres(18),
            Placement::IDENTITY,
        )
        .expect("the shelf panel is valid");
        let root =
            Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![front.into(), shelf.into()]);
        let mut session = Session::with_catalog(Project::new(root).expect("the project is valid"), sample_catalog());
        let params = serde_json::to_value(PanelEdgeBandingSet {
            panel: Id::new(2).expect("id"),
            edge: RectEdge::Bottom.id(),
            material: sample_edge_material_id(),
            included_in_size: true,
        })
        .expect("the band parameters encode");
        session.dispatch(EDGE_BANDING_SET_ID, &params).expect("the front edge is banded");
        session
    }

    #[test]
    fn the_summary_reads_only_the_resolved_selection() {
        let session = banded_session();
        let front = Id::new(2).expect("id");
        let summary = StatusSummary::of(session.project(), session.catalog(), &[front], None, Lang::En);
        let line = summary.line(Lang::En).expect("a selected panel has context");
        assert_eq!(summary.count, 1);
        assert_eq!(summary.name.as_deref(), Some("Front"));
        assert_eq!(summary.kind.as_deref(), Some("Panel"));
        assert_eq!(summary.size.as_deref(), Some("600 mm × 720 mm × 18 mm"));
        assert_eq!(summary.material.as_deref(), Some("Chipboard"));
        assert_eq!(summary.banding.as_deref(), Some("edge 2 mm"));
        assert!(
            line.starts_with("Front · Panel · 600 mm × 720 mm × 18 mm · Chipboard · edge 2 mm"),
            "the centre zone names the node, its kind, size, material and band: {line}"
        );
        assert!(!line.contains("Shelf"), "an unselected sibling never reaches the bar: {line}");
        assert!(summary.warning.is_none(), "nothing is wrong with the selected panel");
    }

    #[test]
    fn a_multi_selection_reports_its_count_and_nothing_more() {
        let session = banded_session();
        let selection = [Id::new(2).expect("id"), Id::new(4).expect("id")];
        let summary = StatusSummary::of(session.project(), session.catalog(), &selection, None, Lang::En);
        assert_eq!(summary.count, 2);
        assert_eq!(summary.line(Lang::En).as_deref(), Some("Selected: 2"));
        assert!(summary.name.is_none(), "no single name describes two nodes");
    }

    #[test]
    fn an_empty_selection_has_no_context() {
        let session = banded_session();
        let summary = StatusSummary::of(session.project(), session.catalog(), &[], None, Lang::En);
        assert_eq!(summary, StatusSummary::default());
        assert_eq!(summary.line(Lang::En), None);
    }

    #[test]
    fn the_summary_warns_about_a_lock_before_a_missing_material() {
        let session = banded_session();
        let front = Id::new(2).expect("id");
        let locked = StatusSummary::of(session.project(), session.catalog(), &[front], Some(front), Lang::En);
        let warning = locked.warning.expect("a locked node warns");
        assert_eq!(warning.text, "Locked · Front");
        assert!(warning.hint.is_some(), "the chip says what unlocks it");

        let shelf = Id::new(4).expect("id");
        let plain = StatusSummary::of(session.project(), session.catalog(), &[shelf], None, Lang::En);
        assert_eq!(plain.warning.expect("a panel without a material warns").text, "No panel material is assigned.");

        // The sample project's panels carry no catalogue material either, and an unlocked node with
        // no material is still only one warning, never a list.
        let project = sample_project();
        let summary = StatusSummary::of(&project, &sample_catalog(), &[Id::new(2).expect("id")], None, Lang::En);
        assert!(summary.warning.is_some());
    }

    /// The text, elision flag and width of every drawn galley, for the elision test.
    fn drawn_galleys(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, bool, f32)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, bool, f32)>) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push((text.galley.text().to_owned(), text.galley.elided, text.galley.rect.width()));
                }
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

    #[test]
    fn a_long_context_elides_and_keeps_the_full_line_for_its_tooltip() {
        // `Label::truncate` draws an ellipsis and keeps the whole string in the galley, which is the
        // tooltip's text; the middle zone is the only zone that shrinks for it.
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let summary = StatusSummary {
            count: 1,
            name: Some("A very long cabinet name that cannot fit the middle zone at all".to_owned()),
            kind: Some("Panel".to_owned()),
            size: Some("600 mm × 720 mm × 18 mm".to_owned()),
            material: Some("Chipboard".to_owned()),
            banding: Some("edge 2 mm".to_owned()),
            warning: None,
        };
        let line = summary.line(Lang::En).expect("a line");
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 40.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(120.0, theme::STATUS_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| status_center_ui(ui, &summary, Lang::En),
            );
        });
        let galleys = drawn_galleys(&output.shapes);
        output.drop_without_applying_deltas();
        let (text, elided, width) = galleys
            .iter()
            .find(|(text, _, _)| text == &line)
            .cloned()
            .unwrap_or_else(|| panic!("the centre zone drew the full line: {galleys:?}"));
        assert!(elided, "a line wider than its zone elides: {text}");
        assert!(width <= 121.0, "the elided line stays inside the zone: {width}");
    }

    #[test]
    fn the_right_zone_names_the_snap_step_and_the_busy_state() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        furnicraft_ui::set_language(&ctx, Lang::Ru);
        let busy = StatusInfo { rebuilding: true, ..idle_status() };
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                status_right_ui(ui, &busy);
            });
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Привязка 10 мм")), "the snap names its step: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Перестроение…")), "a rebuilding frame says so: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Перспективная")), "the projection is named: {texts:?}");

        let idle = StatusInfo { rebuilding: false, ..idle_status() };
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                status_right_ui(ui, &idle);
            });
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Готово")), "an idle frame reports ready: {texts:?}");
    }

    #[test]
    fn the_bar_draws_its_three_zones_in_a_narrow_window() {
        // The 1024 px window the owner checks by eye: the command, the selection and the display
        // state all draw, with no zone clipped away by another.
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let summary = StatusSummary {
            count: 1,
            name: Some("Front".to_owned()),
            kind: Some("Panel".to_owned()),
            ..StatusSummary::default()
        };
        let info = StatusInfo { summary, ..idle_status() };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 200.0))),
            ..Default::default()
        };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(input, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            status_row_ui(ui, &info, &mut out);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Front · Panel")), "the centre draws: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Snap 10 mm")), "the right draws: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Gizmo")), "the left draws: {texts:?}");
    }
}
