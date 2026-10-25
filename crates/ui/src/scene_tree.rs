//! The scene tree.
//!
//! The tree mirrors the document hierarchy: an assembly contains panels and nested assemblies. A
//! panel is an atomic leaf, so it is never shown with children and can never be ungrouped; an
//! assembly is an editable group that can be wrapped and flattened. Selection is view state and is
//! shared with the viewport.

use egui::collapsing_header::CollapsingState;
use egui::{RichText, Ui};
use furnicraft_engine::Session;
use furnicraft_model::{Assembly, Id, Node, Panel, Project};

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
    Duplicate { node: Id },
    Delete { node: Id },
    Group { nodes: Vec<Id> },
    Ungroup { node: Id },
    Undo,
    Redo,
}

/// Draws the toolbar and the assembly tree, returning the actions the user requested.
pub(crate) fn show(ui: &mut Ui, session: &Session, selection: &mut Selection) -> Vec<TreeAction> {
    let project = session.project();
    let mut actions = Vec::new();
    show_toolbar(ui, session, selection, &mut actions);
    ui.separator();
    let has_panel_material = !session.catalog().panel_materials().is_empty();
    show_assembly(ui, project, project.root(), selection, &mut actions, 0, has_panel_material);
    actions
}

fn show_toolbar(ui: &mut Ui, session: &Session, selection: &Selection, actions: &mut Vec<TreeAction>) {
    let project = session.project();
    let parent = add_parent(project, selection);
    let has_panel_material = !session.catalog().panel_materials().is_empty();
    let primary = selection.primary();
    let primary_node = primary.and_then(|id| project.node(id));
    let can_ungroup = primary.is_some_and(|id| project.assembly(id).is_some() && id != project.root().id());
    let can_group = selection.ids().len() >= 2 && project.validate_group(selection.ids()).is_ok();
    let can_add_panel = has_panel_material && project.assembly(parent).is_some();

    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(can_add_panel, egui::Button::new("+ Panel")).clicked() {
            actions.push(TreeAction::CreatePanel { parent });
        }
        if ui.button("+ Assembly").clicked() {
            actions.push(TreeAction::CreateAssembly { parent });
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
}

fn add_parent(project: &Project, selection: &Selection) -> Id {
    let Some(primary) = selection.primary() else {
        return project.root().id();
    };
    if project.assembly(primary).is_some() {
        primary
    } else {
        project.parent_of(primary).unwrap_or(project.root().id())
    }
}

fn show_assembly(
    ui: &mut Ui,
    project: &Project,
    assembly: &Assembly,
    selection: &mut Selection,
    actions: &mut Vec<TreeAction>,
    depth: u32,
    has_panel_material: bool,
) {
    let id = assembly.id();
    let is_root = id == project.root().id();
    let state_id = ui.make_persistent_id(("scene_assembly", id.get()));
    let state = CollapsingState::load_with_default_open(ui.ctx(), state_id, depth < 2);
    let label = if is_root { RichText::new(assembly.name()).strong() } else { RichText::new(assembly.name()) };
    let (_toggle, header, _) =
        state.show_header(ui, |ui| ui.selectable_label(selection.contains(id), label)).body(|ui| {
            for child in assembly.children() {
                show_node(ui, project, child, selection, actions, depth.saturating_add(1), has_panel_material);
            }
        });
    if header.inner.clicked() {
        select(ui, selection, id);
    }
    header.inner.context_menu(|ui| assembly_menu(ui, id, is_root, has_panel_material, actions));
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
        Node::Assembly(assembly) => {
            show_assembly(ui, project, assembly, selection, actions, depth, has_panel_material);
        }
        Node::Panel(panel) => show_panel(ui, panel, selection, actions),
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

fn assembly_menu(ui: &mut Ui, id: Id, is_root: bool, can_add_panel: bool, actions: &mut Vec<TreeAction>) {
    if ui.add_enabled(can_add_panel, egui::Button::new("Add panel")).clicked() {
        actions.push(TreeAction::CreatePanel { parent: id });
        ui.close();
    }
    if ui.button("Add assembly").clicked() {
        actions.push(TreeAction::CreateAssembly { parent: id });
        ui.close();
    }
    if !is_root {
        ui.separator();
        if ui.button("Duplicate").clicked() {
            actions.push(TreeAction::Duplicate { node: id });
            ui.close();
        }
        if ui.button("Ungroup").clicked() {
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
pub(crate) fn first_panel(root: &Assembly) -> Option<Id> {
    for child in root.children() {
        match child {
            Node::Panel(panel) => return Some(panel.id()),
            Node::Assembly(assembly) => {
                if let Some(id) = first_panel(assembly) {
                    return Some(id);
                }
            }
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

    fn assembly(id: u64, children: Vec<Node>) -> Assembly {
        Assembly::new(Id::new(id).expect("id"), format!("Assembly {id}"), Placement::IDENTITY, children)
    }

    #[test]
    fn first_panel_is_depth_first() {
        let project = sample_project();
        let expected = Id::new(2).expect("id");
        assert_eq!(first_panel(project.root()), Some(expected));
        assert_eq!(first_panel(&assembly(10, Vec::new())), None);
    }

    #[test]
    fn first_panel_descends_into_nested_assemblies() {
        let nested = assembly(20, vec![Node::Panel(panel(21)), Node::Panel(panel(22))]);
        let root = assembly(10, vec![Node::Assembly(nested), Node::Panel(panel(11))]);
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

    #[test]
    fn adding_uses_the_selected_assembly_or_the_panels_parent() {
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
