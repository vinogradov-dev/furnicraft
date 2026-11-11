//! The mesh edit mode: the vertex and face selection on one baked mesh.
//!
//! One mode, one object: entering needs exactly one selected node that carries a mesh, and the
//! mode never outlives that node or that selection. Vertical and face indices are picked by the
//! viewport, so they are not validated here; an index the mesh no longer has is dropped by
//! `retain_mesh_edit`.
//!
//! Refuses nothing loudly: `start_mesh_edit` and the two selectors return `false` when the mode is
//! not available, so the caller can ignore the gesture.
//!
//! Changes when: sub-object editing gains, loses or renames an operation.

use furnicraft_model::Project;

use super::Editor;
use crate::mesh_edit::{MeshEdit, MeshElement, is_editable_mesh};

impl Editor {
    /// The mesh edit session, when the editor is in sub-object mode.
    pub fn mesh_edit(&self) -> Option<&MeshEdit> {
        self.mesh_edit.as_ref()
    }

    /// Enters vertex editing on the primary selection.
    ///
    /// The mode needs exactly one selected node and that node must carry a baked mesh; a panel,
    /// a container or a constructive object refuses it. Returns whether the mode started.
    pub fn start_mesh_edit(&mut self, project: &Project) -> bool {
        if self.selection.ids().len() != 1 {
            return false;
        }
        let Some(object) = self.selection.primary() else {
            return false;
        };
        if !is_editable_mesh(project, object) {
            return false;
        }
        self.contour_edit = None;
        self.drawing = None;
        self.mesh_edit = Some(MeshEdit::new(object));
        true
    }

    /// Leaves vertex editing and drops the vertex selection.
    pub fn end_mesh_edit(&mut self) {
        self.mesh_edit = None;
    }

    /// Selects a vertex, replacing the selection or adding to it.
    ///
    /// Returns `false` when the editor is not in mesh mode. The caller only passes indices that
    /// were picked from the edited mesh, so the index is not validated here.
    pub fn select_mesh_vertex(&mut self, vertex: u32, additive: bool) -> bool {
        let Some(edit) = self.mesh_edit.as_mut() else {
            return false;
        };
        if additive {
            edit.toggle(vertex);
        } else {
            edit.select(vertex);
        }
        true
    }

    /// Clears the vertex selection while staying in mesh mode.
    pub fn clear_mesh_vertices(&mut self) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.clear();
        }
    }

    /// Selects a face, replacing the selection or adding to it.
    ///
    /// Returns `false` when the editor is not in mesh mode. The caller only passes indices that
    /// were picked from the edited mesh, so the index is not validated here.
    pub fn select_mesh_face(&mut self, face: u32, additive: bool) -> bool {
        let Some(edit) = self.mesh_edit.as_mut() else {
            return false;
        };
        if additive {
            edit.toggle_face(face);
        } else {
            edit.select_face(face);
        }
        true
    }

    /// Clears the face selection while staying in mesh mode.
    pub fn clear_mesh_faces(&mut self) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.clear_faces();
        }
    }

    /// Switches between vertex and face selection.
    pub fn set_mesh_element(&mut self, element: MeshElement) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.set_element(element);
        }
    }

    /// Leaves mesh mode when its object vanished or the selection moved elsewhere, and drops
    /// vertex and face indices that the current mesh no longer has.
    pub fn retain_mesh_edit(&mut self, project: &Project) {
        let Some(object) = self.mesh_edit.as_ref().map(MeshEdit::object) else {
            return;
        };
        if self.selection.ids() != [object] || !is_editable_mesh(project, object) {
            self.mesh_edit = None;
            return;
        }
        let (vertices, faces) = project
            .object(object)
            .and_then(|object| object.geometry().as_mesh())
            .map_or((0, 0), |mesh| (mesh.positions().len(), mesh.faces().len()));
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.retain_vertices(vertices);
            edit.retain_faces(faces);
        }
    }
}
