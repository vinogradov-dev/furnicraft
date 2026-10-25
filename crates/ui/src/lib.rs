//! The egui shell: panels, widgets and input wiring.
//!
//! Thin by design: actions are dispatched to the engine as commands, and the UI never mutates the
//! document directly.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod assembly_card;
mod command;
mod panel_card;
mod placement_card;
mod scene_tree;

use egui::{Color32, Ui};
use furnicraft_engine::node_cmds::{
    ASSEMBLY_CREATE_ID, NODE_DELETE_ID, NODE_DUPLICATE_ID, NODE_GROUP_ID, NODE_UNGROUP_ID,
};
use furnicraft_engine::panel_cmds::CREATE_ID as PANEL_CREATE_ID;
use furnicraft_engine::{AssemblyCreate, NodeDelete, NodeDuplicate, NodeGroup, NodeUngroup, PanelCreate, Session};
use furnicraft_model::{Id, PanelNominalSize, Placement};
use furnicraft_units::Length;

use crate::assembly_card::AssemblyCard;
use crate::command::{dispatch, json};
use crate::panel_card::PanelCard;
use crate::placement_card::PlacementCard;
use crate::scene_tree::{Selection, TreeAction};

/// Default length of a newly created panel, in millimetres.
const NEW_PANEL_LENGTH_MM: i64 = 600;
/// Default width of a newly created panel, in millimetres.
const NEW_PANEL_WIDTH_MM: i64 = 400;

/// Selection and draft state owned by the editor panels.
#[derive(Debug, Default)]
pub struct Editor {
    selection: Selection,
    card: PanelCard,
    assembly_card: AssemblyCard,
    placement_card: PlacementCard,
    error: Option<String>,
    initialized: bool,
}

impl Editor {
    /// Creates an editor with no node selected; the scene tree selects the first panel on first show.
    pub fn new() -> Self {
        Self::default()
    }

    /// The primary selected node, if any.
    pub fn selected_node(&self) -> Option<Id> {
        self.selection.primary()
    }

    /// Replaces the selection with one node, or clears it for `None`.
    ///
    /// The viewport calls this with the result of picking. A deliberate selection also counts as
    /// the first show, so the tree does not replace it with the first panel afterwards.
    pub fn select_node(&mut self, node: Option<Id>) {
        self.initialized = true;
        match node {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Draws the scene tree and dispatches the tree actions.
    pub fn show_scene_tree(&mut self, ui: &mut Ui, session: &mut Session) {
        self.selection.retain_existing(session.project());
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
        let Some(primary) = self.selection.primary() else {
            ui.label("Select a node in the scene tree.");
            return;
        };
        if session.project().parent_of(primary).is_some() {
            self.placement_card.show(ui, session, primary);
        }
        if session.project().panel(primary).is_some() {
            self.card.show(ui, session, Some(primary));
        } else if session.project().assembly(primary).is_some() {
            self.assembly_card.show(ui, session, primary);
        } else {
            ui.label("Select a node in the scene tree.");
        }
    }

    fn run_action(&mut self, session: &mut Session, action: TreeAction) -> Result<(), String> {
        match action {
            TreeAction::CreatePanel { parent } => {
                let material = session
                    .catalog()
                    .panel_materials()
                    .first()
                    .ok_or_else(|| "Add a panel material to the catalogue first.".to_owned())?
                    .id();
                let nominal_size = default_panel_size()?;
                let params = json(&PanelCreate {
                    parent,
                    name: "Panel".to_owned(),
                    nominal_size,
                    material,
                    placement: Placement::IDENTITY,
                })?;
                let outcome = dispatch(session, PANEL_CREATE_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
            }
            TreeAction::CreateAssembly { parent } => {
                let params =
                    json(&AssemblyCreate { parent, name: "Assembly".to_owned(), placement: Placement::IDENTITY })?;
                let outcome = dispatch(session, ASSEMBLY_CREATE_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
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
            TreeAction::Undo => {
                session.undo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
            }
            TreeAction::Redo => {
                session.redo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
            }
        }
        Ok(())
    }

    fn after_node_removed(&mut self, session: &Session, parent: Option<Id>) {
        self.selection.retain_existing(session.project());
        if self.selection.primary().is_none()
            && let Some(parent) = parent
            && session.project().contains_node(parent)
        {
            self.selection.replace(parent);
        }
    }
}

fn default_panel_size() -> Result<PanelNominalSize, String> {
    let length = Length::from_millimetres(NEW_PANEL_LENGTH_MM)
        .ok_or_else(|| "default panel length is out of range".to_owned())?;
    let width =
        Length::from_millimetres(NEW_PANEL_WIDTH_MM).ok_or_else(|| "default panel width is out of range".to_owned())?;
    PanelNominalSize::new(length, width).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn picking_replaces_the_tree_selection_and_an_empty_click_clears_it() {
        let mut editor = Editor::new();
        assert_eq!(editor.selected_node(), None);
        let node = Id::new(2).expect("id");
        editor.select_node(Some(node));
        assert_eq!(editor.selected_node(), Some(node));
        editor.select_node(None);
        assert_eq!(editor.selected_node(), None);
    }
}
