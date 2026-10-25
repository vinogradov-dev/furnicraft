//! The selected-container card.
//!
//! An assembly is a meaningful furniture component; a group is an organisational container with no
//! furniture type. Both own children and are renamed through `node.rename`. Grouping and ungrouping
//! live in the scene tree so the same action covers several selected nodes.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::{NodeRename, Session};
use furnicraft_model::{Id, Project};

use crate::command::{dispatch, json};

/// Persistent state of the container card.
#[derive(Debug, Default)]
pub(crate) struct ContainerCard {
    draft: Option<Draft>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct Draft {
    container: Id,
    name: String,
}

/// The two kinds of container the document stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Assembly,
    Group,
}

impl ContainerKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Assembly => "Assembly",
            Self::Group => "Group",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Assembly => {
                "A furniture or industrial component. It keeps its own identity and is dismantled by a dedicated \
                 tool, not by Ungroup."
            }
            Self::Group => {
                "An organisational container with no furniture type. Ungroup flattens it into its parent without \
                 moving any child in world space."
            }
        }
    }
}

fn container_summary(project: &Project, id: Id) -> Option<(ContainerKind, &str, usize)> {
    if let Some(assembly) = project.assembly(id) {
        return Some((ContainerKind::Assembly, assembly.name(), assembly.children().len()));
    }
    let group = project.group(id)?;
    Some((ContainerKind::Group, group.name(), group.children().len()))
}

impl ContainerCard {
    /// Draws the card for the selected container and dispatches accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, container_id: Id) {
        let Some((kind, name, children)) = container_summary(session.project(), container_id) else {
            self.draft = None;
            self.error = None;
            ui.label("Select a node in the scene tree.");
            return;
        };
        let name = name.to_owned();
        if self.draft.as_ref().is_none_or(|draft| draft.container != container_id) {
            self.error = None;
            self.draft = Some(Draft { container: container_id, name: name.clone() });
        }
        let is_root = container_id == session.project().root().id();
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        ui.separator();
        ui.label(RichText::new("Name").strong());
        let mut rename = None;
        ui.horizontal(|ui| {
            let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
            let clicked = ui.button("Rename").clicked();
            if response.lost_focus() || clicked {
                let trimmed = draft.name.trim();
                if trimmed.is_empty() {
                    self.error = Some(format!("{} name must not be empty.", kind.label()));
                } else if trimmed != name {
                    rename = Some(trimmed.to_owned());
                }
            }
        });

        ui.separator();
        ui.label(RichText::new(kind.label()).strong());
        ui.label(if is_root {
            "Root group; it defines the world frame.".to_owned()
        } else {
            kind.description().to_owned()
        });
        ui.label(format!("Children  {children}"));

        if let Some(new_name) = rename {
            match rename_container(session, container_id, new_name) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
        }

        if let Some(error) = &self.error {
            ui.separator();
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }
}

fn rename_container(session: &mut Session, node: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    #[test]
    fn renaming_an_assembly_dispatches_through_the_engine() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let assembly = Id::new(3).expect("assembly id");
        rename_container(&mut session, assembly, "Drawer box".to_owned()).expect("rename");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer box");
        session.undo().expect("undo");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer");
    }

    #[test]
    fn renaming_the_root_group_dispatches_through_the_engine() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        rename_container(&mut session, root, "Workshop".to_owned()).expect("rename root");
        assert_eq!(session.project().root().name(), "Workshop");
        assert_eq!(session.project().group(root).expect("group").name(), "Workshop");
    }
}
