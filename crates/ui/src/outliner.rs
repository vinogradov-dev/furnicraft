//! The document outliner.
//!
//! The outliner is a read-only view of the assembly tree plus panel selection. Selection is view
//! state: it never changes the document and never dispatches a command.

use egui::{CollapsingHeader, RichText, Ui};
use furnicraft_model::{Assembly, Id, Node};

/// Shows the assembly tree and updates the selected panel.
pub(crate) fn show(ui: &mut Ui, root: &Assembly, selected: &mut Option<Id>) {
    ui.label(RichText::new(root.name()).strong());
    for child in root.children() {
        show_node(ui, child, selected, 1);
    }
}

fn show_node(ui: &mut Ui, node: &Node, selected: &mut Option<Id>, depth: u32) {
    match node {
        Node::Assembly(assembly) => {
            CollapsingHeader::new(assembly.name()).id_salt(assembly.id()).default_open(depth <= 2).show(ui, |ui| {
                for child in assembly.children() {
                    show_node(ui, child, selected, depth.saturating_add(1));
                }
            });
        }
        Node::Panel(panel) => {
            let is_selected = *selected == Some(panel.id());
            if ui.selectable_label(is_selected, panel.name()).clicked() {
                *selected = Some(panel.id());
            }
        }
    }
}

/// Returns the first panel in depth-first order, used to open the card on a useful panel.
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
}
