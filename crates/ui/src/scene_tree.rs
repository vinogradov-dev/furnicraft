//! The scene tree.
//!
//! The tree mirrors the document hierarchy: containers contain panels and other containers. A panel
//! is an atomic leaf, so it is never shown with children and can never be ungrouped. A container is
//! an ordinary group or a furniture component; the component badge and its furniture type come from
//! the node's properties. Selection is view state and is shared with the viewport.

use egui::collapsing_header::CollapsingState;
use egui::{RichText, Ui};
use furnicraft_engine::Session;
use furnicraft_model::{Container, FurnType, Id, Node, Panel, Project};

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

    /// Empties the selection.
    pub(crate) fn clear(&mut self) {
        self.ids.clear();
    }

    /// Replaces the selection with a query result, or clears it when nothing matched.
    pub(crate) fn replace_all(&mut self, ids: Vec<Id>) {
        self.ids = ids;
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

/// One action the tree asks the editor to dispatch after drawing ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeAction {
    CreatePanel { parent: Id },
    CreateAssembly { parent: Id },
    CreateObject { parent: Id },
    Duplicate { node: Id },
    Delete { node: Id },
    Group { nodes: Vec<Id> },
    Ungroup { node: Id },
    ConvertToGroup { node: Id },
    SelectUnderType { furntype: FurnType },
    Undo,
    Redo,
}

/// Draws the toolbar and the container tree, returning the actions the user requested.
pub(crate) fn show(
    ui: &mut Ui,
    session: &Session,
    selection: &mut Selection,
    type_filter: &mut String,
) -> Vec<TreeAction> {
    let project = session.project();
    let mut actions = Vec::new();
    show_toolbar(ui, session, selection, type_filter, &mut actions);
    ui.separator();
    let has_panel_material = !session.catalog().panel_materials().is_empty();
    show_container(ui, project, project.root(), selection, &mut actions, 0, has_panel_material);
    actions
}

fn show_toolbar(
    ui: &mut Ui,
    session: &Session,
    selection: &Selection,
    type_filter: &mut String,
    actions: &mut Vec<TreeAction>,
) {
    let project = session.project();
    let parent = add_parent(project, selection);
    let has_panel_material = !session.catalog().panel_materials().is_empty();
    let primary = selection.primary();
    let primary_node = primary.and_then(|id| project.node(id));
    let can_ungroup = primary.is_some_and(|id| {
        id != project.root().id() && project.container(id).is_some_and(|container| !container.is_component())
    });
    let can_convert = primary.is_some_and(|id| project.is_component(id));
    let can_group = selection.ids().len() >= 2 && project.validate_group(selection.ids()).is_ok();
    let can_add_panel = has_panel_material && project.is_container(parent);

    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(can_add_panel, egui::Button::new("+ Panel")).clicked() {
            actions.push(TreeAction::CreatePanel { parent });
        }
        if ui.button("+ Assembly").clicked() {
            actions.push(TreeAction::CreateAssembly { parent });
        }
        if ui.button("+ Cube").clicked() {
            actions.push(TreeAction::CreateObject { parent });
        }
        ui.separator();
        if ui.add_enabled(can_group, egui::Button::new("Group")).clicked() {
            actions.push(TreeAction::Group { nodes: selection.ids().to_vec() });
        }
        if ui.add_enabled(can_ungroup, egui::Button::new("Ungroup")).clicked()
            && let Some(node) = primary
        {
            actions.push(TreeAction::Ungroup { node });
        }
        if ui.add_enabled(can_convert, egui::Button::new("Convert to group")).clicked()
            && let Some(node) = primary
        {
            actions.push(TreeAction::ConvertToGroup { node });
        }
        if ui.add_enabled(primary_node.is_some(), egui::Button::new("Duplicate")).clicked()
            && let Some(node) = primary
        {
            actions.push(TreeAction::Duplicate { node });
        }
        if ui.add_enabled(primary_node.is_some(), egui::Button::new("Delete")).clicked()
            && let Some(node) = primary
        {
            actions.push(TreeAction::Delete { node });
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(session.can_undo(), egui::Button::new("Undo")).clicked() {
            actions.push(TreeAction::Undo);
        }
        if ui.add_enabled(session.can_redo(), egui::Button::new("Redo")).clicked() {
            actions.push(TreeAction::Redo);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Type");
        ui.add(egui::TextEdit::singleline(type_filter).hint_text("12.03").desired_width(90.0));
        let parsed = type_filter.trim().parse::<FurnType>().ok();
        if ui
            .add_enabled(parsed.is_some(), egui::Button::new("Select subtree"))
            .on_hover_text("Select every node of this furniture type and its sub-types")
            .clicked()
            && let Some(furntype) = parsed
        {
            actions.push(TreeAction::SelectUnderType { furntype });
        }
        if ui.add_enabled(!type_filter.is_empty(), egui::Button::new("Clear")).clicked() {
            type_filter.clear();
        }
    });
}

fn add_parent(project: &Project, selection: &Selection) -> Id {
    let Some(primary) = selection.primary() else {
        return project.root().id();
    };
    if project.is_container(primary) { primary } else { project.parent_of(primary).unwrap_or(project.root().id()) }
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
    depth: u32,
    has_panel_material: bool,
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
                show_node(ui, project, child, selection, actions, depth.saturating_add(1), has_panel_material);
            }
        });
    if header.inner.clicked() {
        select(ui, selection, id);
    }
    header.inner.context_menu(|ui| {
        container_menu(ui, id, is_root, is_component, has_panel_material, actions);
    });
}

fn show_node(
    ui: &mut Ui,
    project: &Project,
    node: &Node,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    depth: u32,
    has_panel_material: bool,
) {
    match node {
        Node::Container(container) => {
            show_container(ui, project, container, selection, actions, depth, has_panel_material);
        }
        Node::Panel(panel) => show_panel(ui, panel, selection, actions),
        Node::Object(object) => show_object(ui, object, selection, actions),
        Node::Camera(camera) => show_leaf(ui, camera.id(), camera.name(), "camera", selection, actions),
        Node::Light(light) => show_leaf(ui, light.id(), light.name(), "light", selection, actions),
    }
}

/// A camera or a light row: a named leaf with a kind tag and the usual duplicate/delete actions.
fn show_leaf(ui: &mut Ui, id: Id, name: &str, kind: &str, selection: &mut Selection, actions: &mut Vec<TreeAction>) {
    let label = RichText::new(format!("{name}  [{kind}]"));
    let response = ui.selectable_label(selection.contains(id), label);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, actions));
}

fn show_object(
    ui: &mut Ui,
    object: &furnicraft_model::Object,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
) {
    let id = object.id();
    let kind = match object.geometry() {
        furnicraft_model::Geometry::Mesh(_) => "mesh",
    };
    let label = RichText::new(format!("{}  [{kind}]", object.name()));
    let response = ui.selectable_label(selection.contains(id), label);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, actions));
}

fn object_menu(ui: &mut Ui, id: Id, actions: &mut Vec<TreeAction>) {
    if ui.button("Duplicate").clicked() {
        actions.push(TreeAction::Duplicate { node: id });
        ui.close();
    }
    if ui.button("Delete").clicked() {
        actions.push(TreeAction::Delete { node: id });
        ui.close();
    }
}

fn show_panel(ui: &mut Ui, panel: &Panel, selection: &mut Selection, actions: &mut Vec<TreeAction>) {
    let id = panel.id();
    let response = ui.selectable_label(selection.contains(id), panel.name());
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| panel_menu(ui, id, actions));
}

fn select(ui: &Ui, selection: &mut Selection, id: Id) {
    let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.shift);
    if additive {
        selection.toggle(id);
    } else {
        selection.replace(id);
    }
}

fn container_menu(
    ui: &mut Ui,
    id: Id,
    is_root: bool,
    is_component: bool,
    can_add_panel: bool,
    actions: &mut Vec<TreeAction>,
) {
    if ui.add_enabled(can_add_panel, egui::Button::new("Add panel")).clicked() {
        actions.push(TreeAction::CreatePanel { parent: id });
        ui.close();
    }
    if ui.button("Add assembly").clicked() {
        actions.push(TreeAction::CreateAssembly { parent: id });
        ui.close();
    }
    if ui.button("Add cube").clicked() {
        actions.push(TreeAction::CreateObject { parent: id });
        ui.close();
    }
    if !is_root {
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

fn panel_menu(ui: &mut Ui, id: Id, actions: &mut Vec<TreeAction>) {
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

    use furnicraft_model::{Id, Node, Panel, PanelNominalSize, Placement};
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
    fn selection_replaces_all_matches() {
        let first = Id::new(2).expect("id");
        let second = Id::new(3).expect("id");
        let mut selection = Selection::default();
        selection.replace(Id::new(4).expect("id"));

        selection.replace_all(vec![first, second]);
        assert_eq!(selection.ids(), &[first, second]);
        assert_eq!(selection.primary(), Some(second));

        selection.replace_all(Vec::new());
        assert!(selection.ids().is_empty());
        assert_eq!(selection.primary(), None);
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

    #[test]
    fn adding_uses_the_selected_container_or_the_panels_parent() {
        let project = sample_project();
        let mut selection = Selection::default();
        assert_eq!(add_parent(&project, &selection), project.root().id());

        let drawer = Id::new(3).expect("id");
        selection.replace(drawer);
        assert_eq!(add_parent(&project, &selection), drawer);

        let panel = Id::new(4).expect("id");
        selection.replace(panel);
        assert_eq!(add_parent(&project, &selection), drawer);

        let root_panel = Id::new(2).expect("id");
        selection.replace(root_panel);
        assert_eq!(add_parent(&project, &selection), project.root().id());
    }
}
