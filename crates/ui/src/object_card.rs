//! The selected geometry object card.
//!
//! An object is a leaf with one geometry payload. Today the only payload is a baked mesh, edited
//! directly with no feature history: the card names the object, reports what it carries and starts
//! the sub-object vertex editor. A constructive object will show its contour and features here
//! instead ([ADR 0037]).
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::{NodeRename, Session};
use furnicraft_model::{Geometry, Id, Project};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::mesh_edit::MeshEdit;
use crate::section::section;

/// Most selected vertices the card lists with their positions; the count covers the rest.
const MAX_LISTED_VERTICES: usize = 12;

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
    ///
    /// Returns whether the user asked to start or finish vertex editing; the editor owns the mode
    /// and validates that the object is a mesh before it starts.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        object_id: Id,
        mesh_edit: Option<&MeshEdit>,
        single_selection: bool,
    ) -> bool {
        let Some(summary) = object_summary(session.project(), object_id) else {
            self.draft = None;
            self.error = None;
            ui.label("Select a node in the scene tree.");
            return false;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.object != object_id) {
            self.error = None;
            self.draft = Some(Draft { object: object_id, name: summary.name.clone() });
        }
        let Some(draft) = self.draft.as_mut() else {
            return false;
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

        ui.separator();
        let mut toggle = false;
        section(ui, "object.mesh_edit", "Mesh editing", true, |ui| {
            if single_selection {
                let editing = mesh_edit.is_some_and(|edit| edit.object() == object_id);
                let label = if editing { "Done" } else { "Edit mesh" };
                toggle = ui.button(label).clicked();
            } else {
                ui.label("Select only this object to edit its vertices.");
            }
            if let Some(edit) = mesh_edit.filter(|edit| edit.object() == object_id) {
                ui.label(format!("Selected vertices  {}", edit.vertices().len()));
                let positions = selected_positions(session.project(), object_id, edit.vertices());
                for (vertex, position) in positions.iter().take(MAX_LISTED_VERTICES) {
                    ui.label(format!("v{vertex}  {}", format_position(*position)));
                }
                if positions.len() > MAX_LISTED_VERTICES {
                    ui.label(format!("… and {} more", positions.len() - MAX_LISTED_VERTICES));
                }
                ui.label("Click a vertex in the viewport; Shift or Ctrl adds it to the selection.");
            }
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
        toggle
    }
}

/// The selected vertices that still exist, with their local positions.
fn selected_positions(project: &Project, object: Id, vertices: &[u32]) -> Vec<(u32, [Length; 3])> {
    let Some(mesh) = project.object(object).and_then(|object| object.geometry().as_mesh()) else {
        return Vec::new();
    };
    vertices
        .iter()
        .filter_map(|vertex| {
            let index = usize::try_from(*vertex).ok()?;
            mesh.positions().get(index).map(|position| (*vertex, *position))
        })
        .collect()
}

fn format_position(position: [Length; 3]) -> String {
    format!("{}  {}  {}", position[0], position[1], position[2])
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
    fn selected_positions_skip_indices_the_mesh_no_longer_has() {
        let (project, id) = object_project();
        let positions = selected_positions(&project, id, &[0, 3, 8]);
        assert_eq!(positions.len(), 2, "vertex 8 is outside a cube");
        assert_eq!(positions[0].0, 0);
        assert_eq!(positions[1].0, 3);
        let cube = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        assert_eq!(positions[0].1, cube.positions()[0]);
        assert_eq!(positions[1].1, cube.positions()[3]);
        assert!(format_position(positions[0].1).ends_with(" mm"));
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
