//! The scene tree.
//!
//! The tree mirrors the document hierarchy: containers contain panels and other containers. A panel
//! is an atomic leaf, so it is never shown with children and can never be ungrouped. A container is
//! an ordinary group or a furniture component; the component badge and its furniture type come from
//! the node's properties. Selection is view state and is shared with the viewport.

use egui::collapsing_header::CollapsingState;
use egui::{RichText, Ui};
use furnicraft_engine::Session;
use furnicraft_model::{Container, Id, Node, Panel, Project};

/// The selected document nodes, in click order. The last one is the primary selection.
#[derive(Debug, Default)]
pub(crate) struct Selection {
    ids: Vec<Id>,
}

impl Selection {
    /// The node whose card is shown and whose context actions are enabled.
    pub(crate) fn primary(&self) -> Option<Id> {
        self.ids.last().copied()
    }

    /// All selected node ids, oldest first.
    pub(crate) fn ids(&self) -> &[Id] {
        &self.ids
    }

    /// Whether the node is selected.
    pub(crate) fn contains(&self, id: Id) -> bool {
        self.ids.contains(&id)
    }

    /// Replaces the selection with one node.
    pub(crate) fn replace(&mut self, id: Id) {
        self.ids.clear();
        self.ids.push(id);
    }

    /// Replaces the selection with several nodes, in the given order.
    #[cfg(test)]
    pub(crate) fn replace_all(&mut self, ids: Vec<Id>) {
        self.ids = ids;
    }

    /// Empties the selection.
    pub(crate) fn clear(&mut self) {
        self.ids.clear();
    }

    /// Adds the node to the selection, or removes it when it is already selected.
    pub(crate) fn toggle(&mut self, id: Id) {
        if let Some(index) = self.ids.iter().position(|selected| *selected == id) {
            self.ids.remove(index);
        } else {
            self.ids.push(id);
        }
    }

    /// Drops ids that no longer exist, for example after undo or delete.
    pub(crate) fn retain_existing(&mut self, project: &Project) {
        self.ids.retain(|id| project.contains_node(*id));
    }

    /// Selects the first panel in depth-first order.
    pub(crate) fn select_first_panel(&mut self, project: &Project) {
        if let Some(panel) = first_panel(project.root()) {
            self.replace(panel);
        }
    }
}

/// One structural action the tree asks the editor to run.
///
/// The tree shows and restructures nodes; it never creates one. Creation is a `ToolAction` from the
/// ribbon, so there is exactly one place a new node starts. `AddChild` only arms a container for the
/// next creation, which is a view-state decision, not a second creation path (ADR 0056).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeAction {
    AddChild { parent: Id },
    Duplicate { node: Id },
    Delete { node: Id },
    Group { nodes: Vec<Id> },
    Ungroup { node: Id },
    ConvertToGroup { node: Id },
}

/// Draws the container tree, returning the actions the user requested.
///
/// The tree owns no toolbar and no heading: the surface it sits on draws its own header, and every
/// node action is reached from the row's context menu. The one multi-node action, grouping, is
/// offered there while two or more nodes are selected.
pub(crate) fn show(ui: &mut Ui, session: &Session, selection: &mut Selection) -> Vec<TreeAction> {
    let project = session.project();
    let mut actions = Vec::new();
    let group = (selection.ids().len() >= 2 && project.validate_group(selection.ids()).is_ok())
        .then(|| selection.ids().to_vec());
    let group = group.as_deref().unwrap_or(&[]);
    show_container(ui, project, project.root(), selection, &mut actions, group, 0);
    actions
}

fn container_label(project: &Project, container: &Container) -> RichText {
    let name = container.name();
    if container.id() == project.root().id() {
        return RichText::new(name).strong();
    }
    if !container.is_component() {
        return RichText::new(name);
    }
    let badge = match project.furntype(container.id()) {
        Some(furntype) if !furntype.is_unclassified() => furntype.to_string(),
        _ => "component".to_owned(),
    };
    RichText::new(format!("{name}  [{badge}]"))
}

#[allow(clippy::too_many_arguments)]
fn show_container(
    ui: &mut Ui,
    project: &Project,
    container: &Container,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
    depth: u32,
) {
    let id = container.id();
    let is_root = id == project.root().id();
    let is_component = container.is_component();
    let state_id = ui.make_persistent_id(("scene_container", id.get()));
    let state = CollapsingState::load_with_default_open(ui.ctx(), state_id, depth < 2);
    let label = container_label(project, container);
    let (_toggle, header, _) =
        state.show_header(ui, |ui| ui.selectable_label(selection.contains(id), label)).body(|ui| {
            for child in container.children() {
                show_node(ui, project, child, selection, actions, group, depth.saturating_add(1));
            }
        });
    if header.inner.clicked() {
        select(ui, selection, id);
    }
    header.inner.context_menu(|ui| {
        container_menu(ui, id, is_root, is_component, group, actions);
    });
}

#[allow(clippy::too_many_arguments)]
fn show_node(
    ui: &mut Ui,
    project: &Project,
    node: &Node,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
    depth: u32,
) {
    match node {
        Node::Container(container) => {
            show_container(ui, project, container, selection, actions, group, depth);
        }
        Node::Panel(panel) => show_panel(ui, panel, selection, actions, group),
        Node::Object(object) => show_object(ui, object, selection, actions, group),
        Node::Camera(camera) => show_leaf(ui, camera.id(), camera.name(), "camera", selection, actions, group),
        Node::Light(light) => show_leaf(ui, light.id(), light.name(), "light", selection, actions, group),
    }
}

/// A camera or a light row: a named leaf with a kind tag and the usual duplicate/delete actions.
fn show_leaf(
    ui: &mut Ui,
    id: Id,
    name: &str,
    kind: &str,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
) {
    let label = RichText::new(format!("{name}  [{kind}]"));
    let response = ui.selectable_label(selection.contains(id), label);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, group, actions));
}

fn show_object(
    ui: &mut Ui,
    object: &furnicraft_model::Object,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
) {
    let id = object.id();
    let kind = match object.geometry() {
        furnicraft_model::Geometry::Mesh(_) => "mesh",
        furnicraft_model::Geometry::Primitive(primitive) => primitive.shape_name(),
        furnicraft_model::Geometry::Constructive(_) => "contour",
        furnicraft_model::Geometry::Path(_) => "path",
    };
    let label = RichText::new(format!("{}  [{kind}]", object.name()));
    let response = ui.selectable_label(selection.contains(id), label);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, group, actions));
}

fn object_menu(ui: &mut Ui, id: Id, group: &[Id], actions: &mut Vec<TreeAction>) {
    group_entry(ui, group, actions);
    if ui.button("Duplicate").clicked() {
        actions.push(TreeAction::Duplicate { node: id });
        ui.close();
    }
    if ui.button("Delete").clicked() {
        actions.push(TreeAction::Delete { node: id });
        ui.close();
    }
}

fn show_panel(ui: &mut Ui, panel: &Panel, selection: &mut Selection, actions: &mut Vec<TreeAction>, group: &[Id]) {
    let id = panel.id();
    let response = ui.selectable_label(selection.contains(id), panel.name());
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| panel_menu(ui, id, group, actions));
}

/// The one multi-node entry: grouping the current selection.
///
/// The tree toolbar that used to carry it is gone, so a group is created from any row's context
/// menu while two or more nodes are selected. The menu only offers it when the selection is valid.
fn group_entry(ui: &mut Ui, group: &[Id], actions: &mut Vec<TreeAction>) {
    if group.is_empty() {
        return;
    }
    if ui.button("Group selection").clicked() {
        actions.push(TreeAction::Group { nodes: group.to_vec() });
        ui.close();
    }
    ui.separator();
}

fn select(ui: &Ui, selection: &mut Selection, id: Id) {
    let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.shift);
    if additive {
        selection.toggle(id);
    } else {
        selection.replace(id);
    }
}

fn container_menu(ui: &mut Ui, id: Id, is_root: bool, is_component: bool, group: &[Id], actions: &mut Vec<TreeAction>) {
    group_entry(ui, group, actions);
    if !is_root {
        if ui.button("Add child").on_hover_text("Create the next node inside this container").clicked() {
            actions.push(TreeAction::AddChild { parent: id });
            ui.close();
        }
        ui.separator();
        if ui.button("Duplicate").clicked() {
            actions.push(TreeAction::Duplicate { node: id });
            ui.close();
        }
        if is_component {
            if ui.button("Convert to group").clicked() {
                actions.push(TreeAction::ConvertToGroup { node: id });
                ui.close();
            }
        } else if ui.button("Ungroup").clicked() {
            actions.push(TreeAction::Ungroup { node: id });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            actions.push(TreeAction::Delete { node: id });
            ui.close();
        }
    }
}

fn panel_menu(ui: &mut Ui, id: Id, group: &[Id], actions: &mut Vec<TreeAction>) {
    group_entry(ui, group, actions);
    if ui.button("Duplicate").clicked() {
        actions.push(TreeAction::Duplicate { node: id });
        ui.close();
    }
    if ui.button("Delete").clicked() {
        actions.push(TreeAction::Delete { node: id });
        ui.close();
    }
}

/// Returns the first panel in depth-first order, used to open the card on a useful node.
pub(crate) fn first_panel(root: &Container) -> Option<Id> {
    first_panel_in(root.children())
}

fn first_panel_in(children: &[Node]) -> Option<Id> {
    for child in children {
        match child {
            Node::Panel(panel) => return Some(panel.id()),
            Node::Container(container) => {
                if let Some(id) = first_panel_in(container.children()) {
                    return Some(id);
                }
            }
            Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{CameraProjection, Id, LightKind, Node, Panel, PanelNominalSize, Placement};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn panel(id: u64) -> Panel {
        Panel::new(
            Id::new(id).expect("id"),
            format!("Panel {id}"),
            PanelNominalSize::new(
                furnicraft_units::Length::from_millimetres(600).expect("length"),
                furnicraft_units::Length::from_millimetres(400).expect("width"),
            )
            .expect("size"),
            None,
            furnicraft_units::Length::from_millimetres(18).expect("thickness"),
            Placement::IDENTITY,
        )
        .expect("panel")
    }

    fn container(id: u64, children: Vec<Node>) -> Container {
        Container::new(Id::new(id).expect("id"), format!("Container {id}"), Placement::IDENTITY, children)
    }

    #[test]
    fn first_panel_is_depth_first() {
        let project = sample_project();
        let expected = Id::new(2).expect("id");
        assert_eq!(first_panel(project.root()), Some(expected));
        assert_eq!(first_panel(&container(10, Vec::new())), None);
    }

    #[test]
    fn first_panel_descends_into_nested_containers() {
        let nested = container(30, vec![Node::Panel(panel(21)), Node::Panel(panel(22))]);
        let root = container(10, vec![Node::Container(nested), Node::Panel(panel(11))]);
        assert_eq!(first_panel(&root), Some(Id::new(21).expect("id")));
    }

    #[test]
    fn selection_replaces_and_toggles() {
        let first = Id::new(2).expect("id");
        let second = Id::new(3).expect("id");
        let mut selection = Selection::default();
        assert_eq!(selection.primary(), None);

        selection.replace(first);
        assert!(selection.contains(first));
        assert_eq!(selection.primary(), Some(first));

        selection.toggle(second);
        assert_eq!(selection.ids(), &[first, second]);
        assert_eq!(selection.primary(), Some(second));

        selection.toggle(first);
        assert_eq!(selection.ids(), &[second]);
        assert_eq!(selection.primary(), Some(second));

        selection.replace(first);
        assert_eq!(selection.ids(), &[first]);

        selection.clear();
        assert_eq!(selection.primary(), None);
        assert!(selection.ids().is_empty());
    }

    #[test]
    fn selection_drops_removed_nodes() {
        let project = sample_project();
        let mut selection = Selection::default();
        selection.replace(Id::new(2).expect("id"));
        selection.toggle(Id::new(999).expect("id"));
        selection.retain_existing(&project);
        assert_eq!(selection.ids(), &[Id::new(2).expect("id")]);
        selection.replace(Id::new(3).expect("id"));
        selection.retain_existing(&project);
        assert!(selection.contains(Id::new(3).expect("id")));
    }

    /// The tree is a view and a set of structural edits only.
    ///
    /// Creation is not a `TreeAction`: the toolbar strip owns every tool, so the tree cannot grow a
    /// second creation path by adding a button back. Arming a container creates nothing, so
    /// `AddChild` is allowed and is checked here alongside the rest.
    #[test]
    fn the_tree_has_no_creation_action() {
        let kinds = [
            TreeAction::AddChild { parent: Id::new(3).expect("id") },
            TreeAction::Duplicate { node: Id::new(2).expect("id") },
            TreeAction::Delete { node: Id::new(2).expect("id") },
            TreeAction::Group { nodes: Vec::new() },
            TreeAction::Ungroup { node: Id::new(2).expect("id") },
            TreeAction::ConvertToGroup { node: Id::new(2).expect("id") },
        ];
        for action in kinds {
            let debug = format!("{action:?}");
            assert!(!debug.starts_with("Create"), "the tree must not create, got {debug}");
        }
    }

    #[test]
    fn selection_keeps_authored_cameras_and_lights() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let light = project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("light");

        let mut selection = Selection::default();
        selection.replace_all(vec![camera, light]);
        selection.retain_existing(&project);
        assert_eq!(
            selection.ids(),
            &[camera, light],
            "a camera and a light are scene nodes, so the next frame keeps them selected"
        );
    }
}
