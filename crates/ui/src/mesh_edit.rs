//! Sub-object mesh editing state.
//!
//! Editing a mesh is a view state of the editor, not document data: it names the object whose mesh
//! is exposed, whether a click selects a vertex or a face, and what is selected. The document only
//! changes when a command moves vertices, so leaving the mode needs no undo entry.
//!
//! A constructive object has no editable mesh until `object.collapse` bakes it ([ADR 0037]), so the
//! mode can only start on an object that carries a [`Geometry::Mesh`] payload.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use std::collections::HashSet;

use furnicraft_model::{Id, Mesh, Project};

/// Which mesh element a sub-object click selects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeshElement {
    /// Vertices, the original sub-object mode.
    #[default]
    Vertex,
    /// Whole polygon faces; a face move moves every vertex it uses once.
    Face,
}

impl MeshElement {
    /// A singular noun for status text.
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Face => "face",
        }
    }
}

/// The object whose mesh is being edited and the selected vertices or faces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshEdit {
    object: Id,
    element: MeshElement,
    vertices: Vec<u32>,
    faces: Vec<u32>,
}

impl MeshEdit {
    /// Starts an edit session on `object` in vertex mode with nothing selected.
    pub fn new(object: Id) -> Self {
        Self { object, element: MeshElement::Vertex, vertices: Vec::new(), faces: Vec::new() }
    }

    /// The object whose mesh is edited.
    pub const fn object(&self) -> Id {
        self.object
    }

    /// The element kind the viewport click selects.
    pub const fn element(&self) -> MeshElement {
        self.element
    }

    /// Switches the selection kind and drops any selection from the other kind.
    ///
    /// Keeping one mode at a time means a click, the gizmo and the status text all agree on what is
    /// selected; a user who wants the old selection back re-picks it.
    pub fn set_element(&mut self, element: MeshElement) {
        if self.element == element {
            return;
        }
        self.element = element;
        self.vertices.clear();
        self.faces.clear();
    }

    /// The selected vertex indices, oldest first.
    pub fn vertices(&self) -> &[u32] {
        &self.vertices
    }

    /// The selected face indices, oldest first.
    pub fn faces(&self) -> &[u32] {
        &self.faces
    }

    /// The unique vertex indices a move should act on.
    ///
    /// Vertex mode returns the selection directly. Face mode unions the vertices of every selected
    /// face, so a vertex shared by two faces appears once and a drag cannot move it twice.
    pub fn selected_vertices(&self, mesh: &Mesh) -> Vec<u32> {
        if self.element == MeshElement::Vertex {
            return self.vertices.clone();
        }
        let mut seen = HashSet::new();
        let mut vertices = Vec::new();
        for face in &self.faces {
            let Ok(index) = usize::try_from(*face) else {
                continue;
            };
            let Some(polygon) = mesh.faces().get(index) else {
                continue;
            };
            for vertex in polygon {
                if seen.insert(*vertex) {
                    vertices.push(*vertex);
                }
            }
        }
        vertices
    }

    /// Whether `face` is selected.
    pub fn is_face_selected(&self, face: u32) -> bool {
        self.faces.contains(&face)
    }

    /// Whether `vertex` is selected.
    pub fn is_selected(&self, vertex: u32) -> bool {
        self.vertices.contains(&vertex)
    }

    /// Makes `vertex` the only selection.
    pub fn select(&mut self, vertex: u32) {
        self.vertices.clear();
        self.vertices.push(vertex);
    }

    /// Adds `vertex` to the selection, or removes it when it is already selected.
    pub fn toggle(&mut self, vertex: u32) {
        if let Some(index) = self.vertices.iter().position(|selected| *selected == vertex) {
            self.vertices.remove(index);
        } else {
            self.vertices.push(vertex);
        }
    }

    /// Empties the vertex selection without leaving the mode.
    pub fn clear(&mut self) {
        self.vertices.clear();
    }

    /// Makes `face` the only selection.
    pub fn select_face(&mut self, face: u32) {
        self.faces.clear();
        self.faces.push(face);
    }

    /// Adds `face` to the selection, or removes it when it is already selected.
    pub fn toggle_face(&mut self, face: u32) {
        if let Some(index) = self.faces.iter().position(|selected| *selected == face) {
            self.faces.remove(index);
        } else {
            self.faces.push(face);
        }
    }

    /// Empties the face selection without leaving the mode.
    pub fn clear_faces(&mut self) {
        self.faces.clear();
    }

    /// Drops selected vertices that no longer exist in a mesh of `count` vertices.
    ///
    /// A delete, undo or reload can change the topology under an open editor; dropping stale
    /// indices keeps every later move inside the mesh instead of failing at dispatch time.
    pub fn retain_vertices(&mut self, count: usize) {
        self.vertices.retain(|vertex| usize::try_from(*vertex).is_ok_and(|index| index < count));
    }

    /// Drops selected faces that no longer exist in a mesh of `count` faces.
    pub fn retain_faces(&mut self, count: usize) {
        self.faces.retain(|face| usize::try_from(*face).is_ok_and(|index| index < count));
    }
}

/// Whether the node selected for editing is still a mesh object that exists.
pub(crate) fn is_editable_mesh(project: &Project, object: Id) -> bool {
    project.object(object).is_some_and(|object| object.geometry().as_mesh().is_some())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Geometry, Placement};
    use furnicraft_testkit::sample_project;
    use furnicraft_units::Length;

    use super::*;

    fn cube_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("object");
        (project, id)
    }

    #[test]
    fn selecting_a_vertex_replaces_or_toggles_the_selection() {
        let object = Id::new(5).expect("id");
        let mut edit = MeshEdit::new(object);
        assert!(edit.vertices().is_empty());

        edit.select(3);
        assert_eq!(edit.vertices(), [3]);
        assert!(edit.is_selected(3));
        assert!(!edit.is_selected(4));

        edit.toggle(4);
        assert_eq!(edit.vertices(), [3, 4], "an additive click appends");
        edit.toggle(3);
        assert_eq!(edit.vertices(), [4], "a second additive click removes");

        edit.clear();
        assert!(edit.vertices().is_empty());
        assert_eq!(edit.object(), object);
    }

    #[test]
    fn retaining_vertices_drops_stale_indices() {
        let mut edit = MeshEdit::new(Id::new(5).expect("id"));
        edit.toggle(0);
        edit.toggle(7);
        edit.toggle(8);
        edit.retain_vertices(8);
        assert_eq!(edit.vertices(), [0, 7], "an index past the mesh is dropped");

        edit.retain_vertices(0);
        assert!(edit.vertices().is_empty());
    }

    #[test]
    fn only_an_existing_mesh_object_is_editable() {
        let (project, cube) = cube_project();
        assert!(is_editable_mesh(&project, cube));
        assert!(!is_editable_mesh(&project, Id::new(2).expect("panel")), "a panel has no mesh");
        assert!(!is_editable_mesh(&project, Id::new(99).expect("missing")), "an unknown node is not editable");
    }

    #[test]
    fn switching_element_drops_the_other_selection() {
        let object = Id::new(5).expect("id");
        let mut edit = MeshEdit::new(object);
        edit.select(3);
        edit.set_element(MeshElement::Face);
        assert_eq!(edit.element(), MeshElement::Face);
        assert!(edit.vertices().is_empty(), "a vertex selection does not survive a mode switch");

        edit.select_face(2);
        edit.set_element(MeshElement::Vertex);
        assert_eq!(edit.element(), MeshElement::Vertex);
        assert!(edit.faces().is_empty(), "a face selection does not survive a mode switch");

        edit.set_element(MeshElement::Vertex);
        assert_eq!(edit.element(), MeshElement::Vertex, "re-selecting the same mode keeps the state");
    }

    #[test]
    fn a_face_selection_unions_its_vertices_once() {
        let (project, cube) = cube_project();
        let mesh = project.object(cube).and_then(|object| object.geometry().as_mesh()).expect("mesh");
        let mut edit = MeshEdit::new(cube);
        edit.set_element(MeshElement::Face);
        assert!(edit.selected_vertices(mesh).is_empty());

        // Faces 0 [0,3,2,1] and 2 [0,1,5,4] share vertices 0 and 1.
        edit.select_face(0);
        edit.toggle_face(2);
        assert_eq!(edit.faces(), [0, 2]);
        assert!(edit.is_face_selected(2));
        assert_eq!(edit.selected_vertices(mesh), [0, 3, 2, 1, 5, 4], "a shared vertex is listed once");

        edit.toggle_face(0);
        assert_eq!(edit.selected_vertices(mesh), [0, 1, 5, 4], "removing a face drops only its own vertices");

        edit.clear_faces();
        assert!(edit.selected_vertices(mesh).is_empty());

        let mut vertices = MeshEdit::new(cube);
        vertices.select(6);
        assert_eq!(vertices.selected_vertices(mesh), [6], "vertex mode returns the stored selection");
    }

    #[test]
    fn retaining_faces_drops_stale_indices() {
        let mut edit = MeshEdit::new(Id::new(5).expect("id"));
        edit.set_element(MeshElement::Face);
        edit.select_face(1);
        edit.toggle_face(5);
        edit.toggle_face(6);
        edit.retain_faces(6);
        assert_eq!(edit.faces(), [1, 5], "an index past the mesh is dropped");

        edit.retain_faces(0);
        assert!(edit.faces().is_empty());
    }
}
