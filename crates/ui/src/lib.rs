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

mod command;
mod container_card;
mod object_card;
mod panel_card;
mod pivot_card;
mod placement_card;
mod properties_card;
mod scene_tree;
mod section;

use std::collections::HashSet;

use egui::{Color32, Ui};
use furnicraft_engine::node_cmds::{
    ASSEMBLY_CREATE_ID, NODE_COMPONENT_CONVERT_TO_GROUP_ID, NODE_DELETE_ID, NODE_DUPLICATE_ID, NODE_GROUP_ID,
    NODE_UNGROUP_ID,
};
use furnicraft_engine::object_cmds::OBJECT_PRIMITIVE_CREATE_ID;
use furnicraft_engine::panel_cmds::CREATE_ID as PANEL_CREATE_ID;
use furnicraft_engine::{
    AssemblyCreate, NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup, NodeUngroup,
    ObjectPrimitiveCreate, PanelCreate, PrimitiveKind, Session,
};
use furnicraft_model::{Id, NodeQuery, PanelNominalSize, Placement, Project};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::container_card::ContainerCard;
use crate::object_card::ObjectCard;
use crate::panel_card::PanelCard;
use crate::pivot_card::PivotCard;
use crate::placement_card::PlacementCard;
use crate::properties_card::PropertiesCard;
use crate::scene_tree::{Selection, TreeAction};

/// Default length of a newly created panel, in millimetres.
const NEW_PANEL_LENGTH_MM: i64 = 600;
/// Default width of a newly created panel, in millimetres.
const NEW_PANEL_WIDTH_MM: i64 = 400;

/// Edge length of the default cube primitive, in millimetres.
const NEW_OBJECT_SIZE_MM: i64 = 200;

/// Selection and draft state owned by the editor panels.
#[derive(Debug, Default)]
pub struct Editor {
    selection: Selection,
    open: HashSet<Id>,
    card: PanelCard,
    container_card: ContainerCard,
    object_card: ObjectCard,
    placement_card: PlacementCard,
    pivot_card: PivotCard,
    properties_card: PropertiesCard,
    type_filter: String,
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

    /// Every selected node in selection order, oldest first; the last one is the primary.
    ///
    /// Multi-selection is view state, so a gizmo drag moves all of these nodes as one document
    /// change while the inspector keeps showing the primary.
    pub fn selected_nodes(&self) -> &[Id] {
        self.selection.ids()
    }

    /// Replaces the selection with one node, or clears it for `None`.
    ///
    /// A deliberate selection also counts as the first show, so the tree does not replace it with
    /// the first panel afterwards.
    pub fn select_node(&mut self, node: Option<Id>) {
        self.initialized = true;
        match node {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Selects the result of a viewport click on `hit`.
    ///
    /// The viewport returns the panel under the pointer; a closed container above it is selected
    /// instead, so a click picks the cabinet rather than one of its boards. A click outside the
    /// scene clears the selection.
    pub fn select_viewport_hit(&mut self, project: &Project, hit: Option<Id>) {
        self.initialized = true;
        match hit.and_then(|hit| resolve_viewport_pick(project, &self.open, hit)) {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Enters one level below the picked node: opens it and selects the child that owns `hit`.
    ///
    /// A double-click on a closed container reveals its contents; double-clicking a panel has no
    /// effect because a panel has no children.
    pub fn enter_viewport_level(&mut self, project: &Project, hit: Option<Id>) {
        let Some(hit) = hit else {
            return;
        };
        self.initialized = true;
        let Some(selected) = resolve_viewport_pick(project, &self.open, hit) else {
            return;
        };
        self.selection.replace(selected);
        if !project.is_container(selected) {
            return;
        }
        self.open.insert(selected);
        if let Some(child) = child_containing(project, selected, hit) {
            self.selection.replace(child);
        }
    }

    /// Closes one level and returns the selection to the container that owns it.
    ///
    /// The current node closes when it is an open container; otherwise the nearest open ancestor
    /// closes. With no open ancestor this does nothing.
    pub fn exit_level(&mut self, project: &Project) {
        let Some(selected) = self.selection.primary() else {
            return;
        };
        if project.is_container(selected) && self.open.remove(&selected) {
            if let Some(parent) = project.parent_of(selected)
                && parent != project.root().id()
            {
                self.selection.replace(parent);
            }
            return;
        }
        let mut node = selected;
        while let Some(parent) = project.parent_of(node) {
            if parent == project.root().id() {
                break;
            }
            if self.open.remove(&parent) {
                self.selection.replace(parent);
                return;
            }
            node = parent;
        }
    }

    /// Drops open flags for nodes that no longer exist or no longer own children.
    fn retain_open(&mut self, project: &Project) {
        self.open.retain(|id| project.is_container(*id));
    }

    /// Draws the scene tree and dispatches the tree actions.
    pub fn show_scene_tree(&mut self, ui: &mut Ui, session: &mut Session) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        if !self.initialized {
            self.selection.select_first_panel(session.project());
            self.initialized = true;
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
            ui.separator();
        }
        let actions = scene_tree::show(ui, session, &mut self.selection, &mut self.type_filter);
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
            self.pivot_card.show(ui, session, primary);
        }
        if session.project().panel(primary).is_some() {
            self.card.show(ui, session, Some(primary));
        } else if session.project().object(primary).is_some() {
            self.object_card.show(ui, session, primary);
        } else if session.project().is_container(primary) {
            self.container_card.show(ui, session, primary);
        } else {
            ui.label("Select a node in the scene tree.");
        }
        self.properties_card.show(ui, session, primary);
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
            TreeAction::CreateObject { parent } => {
                let size = Length::from_millimetres(NEW_OBJECT_SIZE_MM)
                    .ok_or_else(|| "The default cube size does not fit a length.".to_owned())?;
                let params = json(&ObjectPrimitiveCreate {
                    parent,
                    name: "Cube".to_owned(),
                    primitive: PrimitiveKind::Cube,
                    size,
                    placement: Placement::IDENTITY,
                })?;
                let outcome = dispatch(session, OBJECT_PRIMITIVE_CREATE_ID, &params)?;
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
            TreeAction::ConvertToGroup { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeComponentConvertToGroup { node })?;
                dispatch(session, NODE_COMPONENT_CONVERT_TO_GROUP_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::SelectUnderType { furntype } => {
                let ids = session.project().query(&NodeQuery::any().with_furntype_under(furntype));
                if ids.is_empty() {
                    return Err(format!("No node has furniture type {furntype} or a sub-type."));
                }
                self.selection.replace_all(ids);
            }
            TreeAction::Undo => {
                session.undo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
                self.retain_open(session.project());
            }
            TreeAction::Redo => {
                session.redo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
                self.retain_open(session.project());
            }
        }
        Ok(())
    }

    fn after_node_removed(&mut self, session: &Session, parent: Option<Id>) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        if self.selection.primary().is_none()
            && let Some(parent) = parent
            && session.project().contains_node(parent)
        {
            self.selection.replace(parent);
        }
    }
}

/// The node a viewport click should select: the outermost closed container above the hit panel.
///
/// Walking up stops at the nearest open container, so an open level exposes its children. The root
/// is not a selectable node; a top-level panel or container is selected directly.
fn resolve_viewport_pick(project: &Project, open: &HashSet<Id>, hit: Id) -> Option<Id> {
    if !project.contains_node(hit) {
        return None;
    }
    if project.is_container(hit) {
        return Some(hit);
    }
    let root = project.root().id();
    let mut node = hit;
    while let Some(parent) = project.parent_of(node) {
        if parent == root || open.contains(&parent) {
            break;
        }
        node = parent;
    }
    Some(node)
}

/// The direct child of `container` whose subtree contains `node`.
fn child_containing(project: &Project, container: Id, node: Id) -> Option<Id> {
    if node == container {
        return None;
    }
    let mut current = node;
    for _ in 0..Project::MAX_ASSEMBLY_DEPTH {
        let parent = project.parent_of(current)?;
        if parent == container {
            return Some(current);
        }
        current = parent;
    }
    None
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

    use furnicraft_engine::NodePropertySet;
    use furnicraft_engine::node_cmds::NODE_PROPERTY_SET_ID;
    use furnicraft_model::{FurnType, PropertyValue, SYSTEM_FURNTYPE};
    use furnicraft_testkit::{sample_catalog, sample_project};

    #[test]
    fn picking_replaces_the_tree_selection_and_an_empty_click_clears_it() {
        let mut editor = Editor::new();
        assert_eq!(editor.selected_node(), None);
        assert!(editor.selected_nodes().is_empty());
        let node = Id::new(2).expect("id");
        editor.select_node(Some(node));
        assert_eq!(editor.selected_node(), Some(node));
        assert_eq!(editor.selected_nodes(), [node], "the primary is the only selected node");
        editor.select_node(None);
        assert_eq!(editor.selected_node(), None);
        assert!(editor.selected_nodes().is_empty());
    }

    #[test]
    fn selecting_a_furniture_type_subtree_replaces_the_selection() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        set_furntype(&mut session, drawer, "12.03.02");
        set_furntype(&mut session, front, "12.03.01");

        let mut editor = Editor::new();
        editor
            .run_action(&mut session, TreeAction::SelectUnderType { furntype: "12.03".parse().expect("type") })
            .expect("select subtree");
        assert_eq!(editor.selected_nodes(), [drawer, front]);
        assert_eq!(editor.selected_node(), Some(front), "the last match is primary");

        assert!(
            editor
                .run_action(&mut session, TreeAction::SelectUnderType { furntype: "13".parse().expect("type") })
                .is_err()
        );
        assert_eq!(editor.selected_nodes(), [drawer, front], "a miss leaves the selection alone");

        editor.selection.clear();
        editor
            .run_action(&mut session, TreeAction::SelectUnderType { furntype: "12".parse().expect("type") })
            .expect("select group");
        assert_eq!(editor.selected_nodes(), [drawer, front]);
    }

    fn set_furntype(session: &mut Session, node: Id, furntype: &str) {
        let furntype: FurnType = furntype.parse().expect("valid furntype");
        let params =
            json(&NodePropertySet { node, key: SYSTEM_FURNTYPE.to_owned(), value: PropertyValue::furn_type(furntype) })
                .expect("params");
        dispatch(session, NODE_PROPERTY_SET_ID, &params).expect("set furntype");
    }

    fn grouped_project() -> (Project, Id, Id, Id) {
        let mut project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let drawer = Id::new(3).expect("drawer id");
        let group = project.group_nodes(&[shelf, drawer], "Group").expect("group siblings");
        (project, group, drawer, Id::new(4).expect("front id"))
    }

    #[test]
    fn a_viewport_click_selects_the_outermost_closed_container() {
        let (project, group, _drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.select_viewport_hit(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(group));
        assert!(!editor.open.contains(&group));

        let mut project = project;
        let extra = project
            .add_panel(
                project.root().id(),
                "Top",
                default_panel_size().expect("size"),
                None,
                Length::from_millimetres(18).expect("thickness"),
                Placement::IDENTITY,
            )
            .expect("add top panel");
        editor.select_viewport_hit(&project, Some(extra));
        assert_eq!(editor.selected_node(), Some(extra));

        editor.select_viewport_hit(&project, None);
        assert_eq!(editor.selected_node(), None);

        editor.select_viewport_hit(&project, Some(Id::new(999).expect("id")));
        assert_eq!(editor.selected_node(), None);
    }

    #[test]
    fn double_click_enters_one_level_and_reveals_the_child_under_the_pointer() {
        let (project, group, drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(drawer));
        assert!(editor.open.contains(&group));

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front));
        assert!(editor.open.contains(&drawer));

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front), "a panel has no level to enter");
    }

    #[test]
    fn escape_closes_one_level_and_returns_to_the_parent_container() {
        let (project, group, drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), None, "nothing is open yet");

        editor.enter_viewport_level(&project, Some(front));
        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(drawer));
        assert!(!editor.open.contains(&drawer));
        assert!(editor.open.contains(&group));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(group));
        assert!(!editor.open.contains(&group));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(group), "the top level stays selected");
    }

    #[test]
    fn creating_a_cube_dispatches_the_primitive_command_and_selects_it() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let mut editor = Editor::new();
        let root = session.project().root().id();

        editor.run_action(&mut session, TreeAction::CreateObject { parent: root }).expect("create cube");

        let id = editor.selected_node().expect("the new object is selected");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Cube");
        assert_eq!(object.geometry().as_mesh().expect("mesh").faces().len(), 6);
        assert_eq!(session.project().revision().get(), 1, "one command is one revision");

        editor.run_action(&mut session, TreeAction::Undo).expect("undo");
        assert_eq!(editor.selected_node(), None, "undo drops the selection of a vanished object");
        assert!(session.project().object(id).is_none());
    }
}
