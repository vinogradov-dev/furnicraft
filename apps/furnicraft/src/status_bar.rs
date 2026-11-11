//! The status bar: the armed tool, the gesture it expects, the counts it affects and the last error.
//!
//! One thin row at the bottom of the window. It reads a [`StatusInfo`] snapshot of the session, the
//! editor and the view camera, draws one row and returns what the row asked for in a [`StatusOut`];
//! the shell applies that after the row has drawn, so the bar never changes the document while it
//! draws. It offers no view control: those float over the viewport in `crate::view_cluster`, so the
//! scene starts at the model instead of at a wall of labels.
//!
//! Changes when: what the bar reports or the buttons it offers change — not when a tool gains a
//! gesture, which belongs to the tool's own module.

use eframe::egui;
use furnicraft_ui::{DrawTool, MeshElement, theme};
use furnicraft_units::Length;

use crate::document::Document;
use crate::view_cluster::NavigationMode;
use crate::{FurniCraftApp, TransformMode};

/// The status text and controls, read once per frame so both sides of the bar draw from data.
struct StatusInfo {
    error: Option<String>,
    /// Why the workspace defaults were used instead of the saved settings, if they were.
    notice: Option<String>,
    mode: TransformMode,
    contour: Option<(usize, bool, bool)>,
    drawing: Option<(DrawTool, usize, bool, bool)>,
    /// Why the pointer cannot extend the live draft, if it cannot.
    preview_error: Option<String>,
    /// The height a live pull arrow shows, if a fresh extrusion is waiting for it.
    extrude: Option<Length>,
    primitive: Option<(furnicraft_engine::PrimitiveKind, Length)>,
    grid: bool,
    mesh: Option<(MeshElement, usize)>,
    /// The navigation mode armed in the floating cluster, if any.
    navigation: Option<NavigationMode>,
    /// The container armed to receive the next creation, if any.
    creation_parent: Option<String>,
    /// The selected node whose lock is refusing edits, if one is selected.
    locked: Option<String>,
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
    FinishExtrude,
    CancelExtrude,
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
    if let Some(error) = &info.error {
        ui.colored_label(theme::danger(ui), error);
        theme::divider(ui);
    } else if let Some(notice) = &info.notice {
        // A workspace that fell back to its defaults is a fact, not a failure: it uses the quiet
        // text colour and keeps the rest of the bar usable.
        hint(ui, notice);
        theme::divider(ui);
    }
    // A locked selection refuses the gizmo and the destructive commands, so the bar says which node
    // is locked: the missing handles read as a refusal and the way out is named (ADR 0065).
    if let Some(name) = &info.locked {
        ui.label(format!("Locked · {name}")).on_hover_text("Unlock it in the scene tree to move or delete it");
        theme::divider(ui);
    }
    // The arm is sticky across creations, so the bar names the container the next node lands in;
    // without the chip a pressed tool would create in a container armed much earlier (ADR 0056).
    if let Some(name) = &info.creation_parent {
        ui.label(format!("Adding to: {name}"));
        if ui.button("✕").on_hover_text("Create at the scene root again").clicked() {
            return (Some(StatusAction::ClearCreationParent), None);
        }
        theme::divider(ui);
    }
    if let Some((segments, dirty, along)) = info.contour {
        ui.label(format!("Contour edit · {segments} segments{}", if dirty { " · modified" } else { "" }));
        let motion = ui.selectable_label(along, if along { "Along contour" } else { "Free move" }).clicked();
        let apply = ui.button("Apply").clicked();
        let cancel = ui.button("Cancel").clicked();
        hint(
            ui,
            if along {
                "Drag corners along their segment  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel"
            } else {
                "Drag corners freely  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel"
            },
        );
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
        ui.label(format!(
            "Draw · {}{}{}",
            tool.label(),
            if points == 0 { String::new() } else { format!(" · {points} points") },
            if tool.closes() { if arc_mode { " · arc" } else { " · line" } } else { "" }
        ));
        let grid = ui.selectable_label(info.grid, "Grid 10 mm  (G)").clicked();
        // The ribbon's *Close* and this one read the same rule, so a closure one offers is never
        // greyed out by the other (ADR 0077).
        let close = ui
            .add_enabled(furnicraft_ui::toolbars::draft_can_close(tool, points, closed), egui::Button::new("Close"))
            .clicked();
        let finish = ui.add_enabled(points >= 2, egui::Button::new("Finish  (Enter)")).clicked();
        let cancel = ui.button("Cancel  (Esc)").clicked();
        hint(
            ui,
            "LMB add point  ·  Tab line/arc  ·  Shift axis lock  ·  Alt ignores snaps  ·  Enter/RMB finish  ·  Esc cancel",
        );
        if let Some(reason) = &info.preview_error {
            hint(ui, reason);
        }
        let action = if grid {
            Some(StatusAction::ToggleGrid)
        } else if close {
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
    if let Some(height) = info.extrude {
        ui.label(format!("Extrude · {height}"));
        let finish = ui.button("Finish  (Enter)").clicked();
        let drop = ui.button("Drop  (Esc)").clicked();
        hint(ui, "Drag the arrow or type a height  ·  Enter finish  ·  Esc drop the pull");
        let action = if finish {
            Some(StatusAction::FinishExtrude)
        } else if drop {
            Some(StatusAction::CancelExtrude)
        } else {
            None
        };
        return (action, None);
    }
    if let Some((kind, size)) = info.primitive {
        let name = match kind {
            furnicraft_engine::PrimitiveKind::Cube => "Cube",
            furnicraft_engine::PrimitiveKind::Cylinder => "Cylinder",
            furnicraft_engine::PrimitiveKind::Sphere => "Sphere",
        };
        ui.label(format!("Primitive · {name} {size}"));
        let cancel = ui.button("Cancel  (Esc)").clicked();
        hint(ui, "LMB place on the work plane  ·  Esc cancel");
        return (cancel.then_some(StatusAction::CancelPrimitive), None);
    }
    if let Some(mode) = info.navigation {
        ui.label(format!("Navigation · {}", mode.name()));
        let done = ui.button("Done  (Esc)").clicked();
        hint(ui, mode.gesture());
        return (done.then_some(StatusAction::EndNavigation), None);
    }
    ui.label(egui::RichText::new("Gizmo").color(theme::text_dim(ui)));
    let mut mode = info.mode;
    let moved = ui.selectable_value(&mut mode, TransformMode::Move, "Move").clicked()
        | ui.selectable_value(&mut mode, TransformMode::Rotate, "Rotate").clicked();
    theme::divider(ui);
    if let Some((element, count)) = info.mesh {
        ui.label(format!("Mesh edit · {count} {} selected", element.noun()));
        ui.label(
            egui::RichText::new("LMB select  ·  Shift/Ctrl add  ·  Esc done  ·  MMB orbit").color(theme::text_dim(ui)),
        );
    } else {
        hint(ui, "LMB select  ·  double-click enter  ·  Esc leave  ·  MMB orbit  ·  Shift+MMB pan  ·  Ctrl+MMB zoom");
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
        let info = self.tabs.active().status_info(notice.as_deref(), self.shell.mode, self.shell.cluster.navigation);
        let mut out = StatusOut::default();
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            // One thin row, left to right: the bar is chrome, not a second sidebar.
            ui.horizontal_centered(|ui| {
                (out.state, out.mode) = status_state_ui(ui, &info);
            });
        });
        if let Some(mode) = out.mode {
            self.shell.mode = mode;
        }
        match out.state {
            // The window's two: the rest belong to the scene the bar reports on.
            Some(StatusAction::EndNavigation) => self.shell.cluster.navigation = None,
            Some(action) => self.tabs.active_mut().apply_status(action),
            None => {}
        }
    }
}

impl Document {
    /// Everything the bar reads from this document.
    ///
    /// The mode and the armed navigation gesture are the window's, so they are handed in instead of
    /// read here; everything else belongs to the scene the tab holds.
    fn status_info(&self, notice: Option<&str>, mode: TransformMode, navigation: Option<NavigationMode>) -> StatusInfo {
        StatusInfo {
            error: self.last_error.clone(),
            notice: notice.map(str::to_owned),
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
            extrude: self.extrude_gizmo.height(),
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
            creation_parent: self.editor.creation_parent_name(self.session.project()).map(str::to_owned),
            locked: self
                .editor
                .locked_selection(self.session.project())
                .and_then(|id| self.session.project().node(id))
                .map(|node| node.name().to_owned()),
        }
    }

    /// Runs the one document-side action the bar asked for.
    ///
    /// The armed navigation gesture is the window's, so the shell clears that one itself; every arm
    /// here acts on the scene this tab holds.
    fn apply_status(&mut self, action: StatusAction) {
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
                if let Err(error) = self.close_draft() {
                    self.last_error = Some(error);
                }
            }
            StatusAction::FinishDraft => {
                if let Err(error) = self.commit_drawing() {
                    self.last_error = Some(error);
                }
            }
            StatusAction::CancelDraft => self.cancel_drawing(),
            StatusAction::FinishExtrude => {
                if let (Some(object), Some(height)) = (self.extrude_gizmo.object(), self.extrude_gizmo.height())
                    && let Err(error) = self.finish_extrude(object, height)
                {
                    self.last_error = Some(error);
                }
            }
            StatusAction::CancelExtrude => self.extrude_gizmo.cancel(),
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

    fn idle_status() -> StatusInfo {
        StatusInfo {
            error: None,
            notice: None,
            mode: TransformMode::Move,
            contour: None,
            drawing: None,
            preview_error: None,
            extrude: None,
            primitive: None,
            grid: true,
            mesh: None,
            navigation: None,
            creation_parent: None,
            locked: None,
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
        // The gizmo and the destructive commands refuse a locked node, so the bar says which one is
        // locked and what unlocks it (ADR 0065).
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { locked: Some("Drawer".to_owned()), ..idle_status() };
        let output = ctx.run_ui(Default::default(), |ui| {
            status_state_ui(ui, &info);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Locked · Drawer")), "the locked node names itself: {texts:?}");
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
}
