//! The selected-container card.
//!
//! A container is either an ordinary group or a furniture component. The difference is data, not a
//! separate node kind: a component carries a non-zero `furntype`, a `prototype.id` or
//! `component = true`. Both own children and are renamed through `node.rename`. Grouping and
//! ungrouping live in the scene tree so the same action covers several selected nodes.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::{NodeRename, Session};
use furnicraft_model::{Id, Project};

use crate::command::{dispatch, json};
use crate::section::section;

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

/// A container's role for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Component,
    Group,
}

impl ContainerKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Component => "Component",
            Self::Group => "Group",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Component => {
                "A furniture or industrial component. It keeps its own identity and must be converted to a group \
                 before Ungroup will flatten it."
            }
            Self::Group => {
                "An organisational container with no furniture type. Ungroup flattens it into its parent without \
                 moving any child in world space."
            }
        }
    }
}

fn container_summary(project: &Project, id: Id) -> Option<(ContainerKind, &str, usize)> {
    let container = project.container(id)?;
    let kind = if project.is_component(id) { ContainerKind::Component } else { ContainerKind::Group };
    Some((kind, container.name(), container.children().len()))
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

        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "container.name", "Name", true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button("Rename").clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some(format!("{} name must not be empty.", kind.label()));
                    } else if trimmed != name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        ui.separator();
        section(ui, "container.summary", "Container", true, |ui| {
            ui.label(RichText::new(kind.label()).strong());
            ui.label(if is_root {
                "Root; it defines the world frame and is never a furniture component.".to_owned()
            } else {
                kind.description().to_owned()
            });
            ui.label(format!("Children  {children}"));
        });

        if let Some(error) = name_error {
            self.error = Some(error);
        }
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
    fn renaming_a_component_dispatches_through_the_engine() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let container = Id::new(3).expect("container id");
        rename_container(&mut session, container, "Drawer box".to_owned()).expect("rename");
        assert_eq!(session.project().container(container).expect("container").name(), "Drawer box");
        assert_eq!(container_summary(session.project(), container).expect("summary").0, ContainerKind::Component);
        session.undo().expect("undo");
        assert_eq!(session.project().container(container).expect("container").name(), "Drawer");
    }

    #[test]
    fn an_unmarked_container_shows_as_a_group() {
        let session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        assert_eq!(container_summary(session.project(), root).expect("summary").0, ContainerKind::Group);
    }

    #[test]
    fn renaming_the_root_container_dispatches_through_the_engine() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        rename_container(&mut session, root, "Workshop".to_owned()).expect("rename root");
        assert_eq!(session.project().root().name(), "Workshop");
        assert_eq!(session.project().container(root).expect("container").name(), "Workshop");
    }
}
