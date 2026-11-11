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
use furnicraft_text::Message;

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

/// What the container card asks the editor to do beyond the edits it dispatches itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ContainerAction {
    /// Nothing was pressed.
    #[default]
    None,
    /// Arm this container so the next created node lands inside it.
    AddChild,
}

/// A container's role for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Component,
    Group,
}

impl ContainerKind {
    const fn label(self) -> Message {
        match self {
            Self::Component => Message::ContainerRoleComponent,
            Self::Group => Message::ContainerRoleGroup,
        }
    }

    const fn description(self) -> Message {
        match self {
            Self::Component => Message::ContainerComponentDescription,
            Self::Group => Message::ContainerGroupDescription,
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
    ///
    /// Returns the creation request separately: arming a container is editor view state, so the
    /// card asks the editor to hold it instead of touching the document.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, container_id: Id) -> ContainerAction {
        let Some((kind, name, children)) = container_summary(session.project(), container_id) else {
            self.draft = None;
            self.error = None;
            ui.label(Message::SelectNodeInTree.text(crate::language(ui.ctx())));
            return ContainerAction::None;
        };
        let name = name.to_owned();
        if self.draft.as_ref().is_none_or(|draft| draft.container != container_id) {
            self.error = None;
            self.draft = Some(Draft { container: container_id, name: name.clone() });
        }
        let is_root = container_id == session.project().root().id();
        let Some(draft) = self.draft.as_mut() else {
            return ContainerAction::None;
        };

        let mut action = ContainerAction::None;
        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "container.name", Message::SectionName.text(crate::language(ui.ctx())), true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button(Message::CmdRename.text(crate::language(ui.ctx()))).clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some(
                            Message::NameMustNotBeEmpty
                                .with(crate::language(ui.ctx()), [kind.label().text(crate::language(ui.ctx()))]),
                        );
                    } else if trimmed != name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        ui.separator();
        let lang = crate::language(ui.ctx());
        section(ui, "container.summary", Message::SectionContainer.text(lang), true, |ui| {
            ui.label(RichText::new(kind.label().text(lang)).strong());
            ui.label(if is_root {
                Message::RootDescription.text(lang).to_owned()
            } else {
                kind.description().text(lang).to_owned()
            });
            ui.label(Message::ChildrenCount.with(lang, [children]));
        });

        if !is_root {
            ui.separator();
            let lang = crate::language(ui.ctx());
            section(ui, "container.creation", Message::SectionCreateInside.text(lang), true, |ui| {
                if ui.button(Message::TreeAddChild.text(lang)).on_hover_text(Message::AddChildHint.text(lang)).clicked()
                {
                    action = ContainerAction::AddChild;
                }
            });
        }

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
        action
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
