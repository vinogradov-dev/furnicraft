//! Sub-object mesh editing state.
//!
//! Editing a mesh is a view state of the editor, not document data: it names the object whose
//! vertices are exposed and which of those vertices are selected. The document only changes when a
//! command moves the vertices, so leaving the mode needs no undo entry.
//!
//! A constructive object has no editable mesh until `object.collapse` bakes it ([ADR 0037]), so the
//! mode can only start on an object that carries a [`Geometry::Mesh`] payload.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use furnicraft_model::{Id, Project};

/// The object whose vertices are being edited and the selected vertex indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshEdit {
    object: Id,
    vertices: Vec<u32>,
}

impl MeshEdit {
    /// Starts an edit session on `object` with no vertex selected.
    pub fn new(object: Id) -> Self {
        Self { object, vertices: Vec::new() }
    }

    /// The object whose vertices are edited.
    pub const fn object(&self) -> Id {
        self.object
    }

    /// The selected vertex indices, oldest first.
    pub fn vertices(&self) -> &[u32] {
        &self.vertices
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

    /// Drops selected vertices that no longer exist in a mesh of `count` vertices.
    ///
    /// A delete, undo or reload can change the topology under an open editor; dropping stale
    /// indices keeps every later move inside the mesh instead of failing at dispatch time.
    pub fn retain_vertices(&mut self, count: usize) {
        self.vertices.retain(|vertex| usize::try_from(*vertex).is_ok_and(|index| index < count));
    }
}

/// Whether the node selected for editing is still a mesh object that exists.
pub(crate) fn is_editable_mesh(project: &Project, object: Id) -> bool {
    project.object(object).is_some_and(|object| object.geometry().as_mesh().is_some())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Geometry, Mesh, Placement};
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
}
