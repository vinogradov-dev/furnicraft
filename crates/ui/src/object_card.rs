//! The selected geometry object card.
//!
//! An object is a leaf with one geometry payload. Today the only payload is a baked mesh, edited
//! directly with no feature history: the card names the object and reports what it carries, and
//! mesh editing arrives with its own tool. A constructive object will show its contour and
//! features here instead ([ADR 0037]).
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::{NodeRename, Session};
use furnicraft_model::{Geometry, Id, Project};

use crate::command::{dispatch, json};
use crate::section::section;

/// Persistent state of the object card.
#[derive(Debug, Default)]
pub(crate) struct ObjectCard {
    draft: Option<Draft>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct Draft {
    object: Id,
    name: String,
}

/// What the card reports about one object, owned so no document borrow outlives the frame.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectSummary {
    name: String,
    kind: &'static str,
    vertices: usize,
    faces: usize,
}

fn object_summary(project: &Project, id: Id) -> Option<ObjectSummary> {
    let object = project.object(id)?;
    match object.geometry() {
        Geometry::Mesh(mesh) => Some(ObjectSummary {
            name: object.name().to_owned(),
            kind: "Mesh",
            vertices: mesh.positions().len(),
            faces: mesh.faces().len(),
        }),
    }
}

impl ObjectCard {
    /// Draws the card for the selected object and dispatches accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, object_id: Id) {
        let Some(summary) = object_summary(session.project(), object_id) else {
            self.draft = None;
            self.error = None;
            ui.label("Select a node in the scene tree.");
            return;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.object != object_id) {
            self.error = None;
            self.draft = Some(Draft { object: object_id, name: summary.name.clone() });
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "object.name", "Name", true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button("Rename").clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some("Object name must not be empty.".to_owned());
                    } else if trimmed != summary.name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        ui.separator();
        section(ui, "object.geometry", "Geometry", true, |ui| {
            ui.label(RichText::new(summary.kind).strong());
            ui.label(format!("Vertices  {}", summary.vertices));
            ui.label(format!("Faces  {}", summary.faces));
            ui.label("Editable as a mesh; undo is its only history.".to_owned());
        });

        if let Some(error) = name_error {
            self.error = Some(error);
        }
        if let Some(new_name) = rename {
            match rename_object(session, object_id, new_name) {
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

fn rename_object(session: &mut Session, node: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Geometry, Mesh, Placement};
    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_units::Length;

    use super::*;

    fn object_project() -> (furnicraft_model::Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("add object");
        (project, id)
    }

    #[test]
    fn summary_reports_a_mesh_object() {
        let (project, id) = object_project();
        let summary = object_summary(&project, id).expect("summary");
        assert_eq!(summary.kind, "Mesh");
        assert_eq!(summary.vertices, 8);
        assert_eq!(summary.faces, 6);
        assert_eq!(summary.name, "Cube");
    }

    #[test]
    fn an_unknown_id_has_no_summary() {
        let (project, _) = object_project();
        assert_eq!(object_summary(&project, Id::new(99).expect("id")), None);
    }

    #[test]
    fn renaming_an_object_dispatches_through_the_engine() {
        let (project, id) = object_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        rename_object(&mut session, id, "Bracket".to_owned()).expect("rename");
        assert_eq!(session.project().object(id).expect("object").name(), "Bracket");
        session.undo().expect("undo");
        assert_eq!(session.project().object(id).expect("object").name(), "Cube");
    }
}
