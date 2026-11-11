//! The panels that draw the scene tree and the inspector card of the primary selection.
//!
//! The scene tree dispatches document commands; the inspector picks the card that fits the selected
//! node and hands each card the mode it may start, so the panel, the object, the camera and the
//! light card never own an edit mode themselves. Errors are shown in the tree and cleared by the
//! next action.
//!
//! Refuses nothing directly: a refused command is shown as its sentence, and the editable modes
//! (`start_mesh_edit`, `start_contour_edit`, `arm_creation`) refuse a node that cannot take them.
//!
//! Changes when: a card joins the inspector or a tree action changes.

use egui::{Color32, Ui};

use furnicraft_engine::node_cmds::{
    NODE_COMPONENT_CONVERT_TO_GROUP_ID, NODE_DELETE_ID, NODE_DUPLICATE_ID, NODE_GROUP_ID, NODE_UNGROUP_ID,
};
use furnicraft_engine::{NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup, NodeUngroup, Session};
use furnicraft_model::Id;

use super::Editor;
use crate::command::{dispatch, json};
use crate::container_card::ContainerAction;
use crate::contour_edit::{ContourEdit, ContourTarget};
use crate::object_card::ObjectAction;
use crate::panel_card::PanelAction;
use crate::scene_tree::{self, TreeAction};

impl Editor {
    /// Draws the scene tree and dispatches the tree actions.
    pub fn show_scene_tree(&mut self, ui: &mut Ui, session: &mut Session) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        self.retain_creation_parent(session.project());
        if !self.initialized {
            self.selection.select_first_panel(session.project());
            self.initialized = true;
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
            ui.separator();
        }
        let actions = scene_tree::show(ui, session, &mut self.selection);
        if actions.is_empty() {
            return;
        }
        self.error = None;
        for action in actions {
            if let Err(error) = self.run_action(session, action) {
                self.error = Some(error);
                break;
            }
        }
        ui.ctx().request_repaint();
    }

    /// Draws the inspector card for the primary selection.
    pub fn show_card(&mut self, ui: &mut Ui, session: &mut Session) {
        self.retain_mesh_edit(session.project());
        self.retain_contour_edit(session.project());
        let Some(primary) = self.selection.primary() else {
            ui.label("Select a node in the scene tree.");
            return;
        };
        if session.project().parent_of(primary).is_some() {
            self.placement_card.show(ui, session, primary);
            self.pivot_card.show(ui, session, primary);
        }
        if session.project().panel(primary).is_some() {
            let contour_target = self.contour_edit.as_ref().map(ContourEdit::target);
            let single_selection = self.selection.ids().len() == 1;
            let action = self.card.show(ui, session, Some(primary), contour_target, single_selection);
            match action {
                PanelAction::None => {}
                PanelAction::StartContour => {
                    self.start_contour_edit(session.project(), ContourTarget::Panel(primary));
                }
                PanelAction::StartCutoutContour { panel, machining } => {
                    self.start_contour_edit(session.project(), ContourTarget::Cutout { panel, machining });
                }
            }
        } else if session.project().object(primary).is_some() {
            let single_selection = self.selection.ids().len() == 1;
            let contour_editing = self.contour_edit.as_ref().is_some_and(|edit| edit.target().id() == primary);
            let action =
                self.object_card.show(ui, session, primary, self.mesh_edit.as_ref(), contour_editing, single_selection);
            match action {
                ObjectAction::None => {}
                ObjectAction::Toggle => {
                    if self.mesh_edit.is_some() {
                        self.end_mesh_edit();
                    } else {
                        self.start_mesh_edit(session.project());
                    }
                }
                ObjectAction::Element(element) => self.set_mesh_element(element),
                ObjectAction::StartContour => {
                    let target = if session.project().object_path(primary).is_ok() {
                        ContourTarget::Path(primary)
                    } else {
                        ContourTarget::Object(primary)
                    };
                    self.start_contour_edit(session.project(), target);
                }
            }
        } else if session.project().camera(primary).is_some() {
            self.camera_card.show(ui, session, primary);
        } else if session.project().light(primary).is_some() {
            self.light_card.show(ui, session, primary);
        } else if session.project().is_container(primary) {
            if self.container_card.show(ui, session, primary) == ContainerAction::AddChild {
                self.arm_creation(session.project(), primary);
            }
        } else {
            ui.label("Select a node in the scene tree.");
        }
        self.properties_card.show(ui, session, primary);
    }

    pub(super) fn run_action(&mut self, session: &mut Session, action: TreeAction) -> Result<(), String> {
        match action {
            TreeAction::AddChild { parent } => {
                self.arm_creation(session.project(), parent);
            }
            TreeAction::Duplicate { node } => {
                let params = json(&NodeDuplicate { node })?;
                let outcome = dispatch(session, NODE_DUPLICATE_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
            }
            TreeAction::Delete { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeDelete { node })?;
                dispatch(session, NODE_DELETE_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::Group { nodes } => {
                let params = json(&NodeGroup { nodes, name: "Group".to_owned() })?;
                let outcome = dispatch(session, NODE_GROUP_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
            }
            TreeAction::Ungroup { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeUngroup { node })?;
                dispatch(session, NODE_UNGROUP_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::ConvertToGroup { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeComponentConvertToGroup { node })?;
                dispatch(session, NODE_COMPONENT_CONVERT_TO_GROUP_ID, &params)?;
                self.after_node_removed(session, parent);
            }
        }
        Ok(())
    }

    fn after_node_removed(&mut self, session: &Session, parent: Option<Id>) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        self.retain_creation_parent(session.project());
        if self.selection.primary().is_none()
            && let Some(parent) = parent
            && session.project().contains_node(parent)
        {
            self.selection.replace(parent);
        }
    }
}
