//! Commands that author scene objects and their constructed geometry.

use crate::tree::{find_container_mut, find_node_mut};
use crate::*;

impl Project {
    /// The contour of a constructive object that a new panel can inherit.
    ///
    /// A missing node, a mesh object or a node that is not a constructive object is an error the
    /// user can read; nothing is derived when the payload has no extrusion.
    pub fn object_contour(&self, id: Id) -> Result<&Contour, ProjectError> {
        let object = self.object(id).ok_or(ProjectError::NodeNotFound { id })?;
        let constructive = object.geometry().as_constructive().ok_or(ProjectError::NotConstructiveObject { id })?;
        constructive.contour().ok_or(ProjectError::NotConstructiveObject { id })
    }

    /// Checks whether an object's contour can become a panel outline without changing the document.
    pub fn validate_panel_contour(&self, parent: Id, source: Id) -> Result<(), ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let anchored = Panel::anchored_contour(self.object_contour(source)?)?;
        Panel::contour_size(&anchored)?;
        Ok(())
    }

    /// Adds a geometry object to an existing container and bumps the revision once.
    ///
    /// The object id is allocated from the document; callers must not invent one. The parent may
    /// be the root or any nested container.
    pub fn add_object(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        geometry: Geometry,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let object = Object::new(id, name, placement, geometry);
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Object(object));
        self.revision = next_revision;
        Ok(id)
    }

    /// The path payload of an object that carries a flat drawing.
    ///
    /// A missing node or an object with a mesh or constructive payload is an error the user can
    /// read; no drawing is derived from those payloads.
    pub fn object_path(&self, id: Id) -> Result<&Path, ProjectError> {
        let object = self.object(id).ok_or(ProjectError::NodeNotFound { id })?;
        object.geometry().as_path().ok_or(ProjectError::NotAPathObject { id })
    }

    /// Replaces a path object's drawing and bumps the revision once.
    ///
    /// A node that is not a path object is [`ProjectError::NotAPathObject`]; the id, name,
    /// placement, pivot and properties are unchanged.
    pub fn set_object_path(&mut self, id: Id, path: Path) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotAPathObject { id });
        };
        let Some(stored) = object.geometry_mut().as_path_mut() else {
            return Err(ProjectError::NotAPathObject { id });
        };
        *stored = path;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces the contour of a constructive object and bumps the revision once.
    ///
    /// A node that is not a constructive object is [`ProjectError::NotConstructiveObject`]; an
    /// invalid contour is refused before the document changes.
    pub fn set_object_contour(&mut self, id: Id, contour: Contour) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotConstructiveObject { id });
        };
        let Some(constructive) = object.geometry_mut().as_constructive_mut() else {
            return Err(ProjectError::NotConstructiveObject { id });
        };
        constructive.set_contour(contour)?;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// The parameters of a parametric primitive object.
    ///
    /// A missing node or an object with another payload is an error the user can read; no shape is
    /// invented from a mesh or a contour.
    pub fn object_primitive(&self, id: Id) -> Result<&Primitive, ProjectError> {
        let object = self.object(id).ok_or(ProjectError::NodeNotFound { id })?;
        object.geometry().as_primitive().ok_or(ProjectError::NotAPrimitiveObject { id })
    }

    /// Replaces a primitive's parameters and bumps the revision once.
    ///
    /// The id, name, placement, pivot and properties are unchanged. A node that is not a parametric
    /// primitive is [`ProjectError::NotAPrimitiveObject`]; invalid parameters are refused by the
    /// payload constructors before the document changes.
    pub fn set_object_primitive(&mut self, id: Id, primitive: Primitive) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotAPrimitiveObject { id });
        };
        let Some(stored) = object.geometry_mut().as_primitive_mut() else {
            return Err(ProjectError::NotAPrimitiveObject { id });
        };
        *stored = primitive;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a derived payload with a baked mesh and bumps the revision once.
    ///
    /// The caller evaluates the payload and passes the resulting mesh; the id, name, placement,
    /// pivot and properties are unchanged and the parameters are dropped ([ADR 0037], [ADR 0054]).
    /// A node that is neither constructive nor a primitive is refused.
    ///
    /// [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
    /// [ADR 0054]: ../docs/decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md
    pub fn collapse_object(&mut self, id: Id, mesh: Mesh) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotDerivedObject { id });
        };
        if !matches!(object.geometry(), Geometry::Constructive(_) | Geometry::Primitive(_)) {
            return Err(ProjectError::NotDerivedObject { id });
        }
        *object.geometry_mut() = Geometry::Mesh(mesh);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Moves mesh vertices to absolute local positions and bumps the revision once.
    ///
    /// The batch is validated before anything changes: an empty batch, a repeated index or an
    /// index outside the mesh rejects the whole call, so a partial move can never reach the
    /// document. A constructive or primitive object has no editable mesh and is refused; it
    /// becomes a mesh object only through `object.collapse`.
    pub fn move_mesh_vertices(&mut self, id: Id, moves: &[(u32, [Length; 3])]) -> Result<Revision, ProjectError> {
        if moves.is_empty() {
            return Err(ProjectError::NoVertexMoves);
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotAMeshObject { id });
        };
        let Some(mesh) = object.geometry_mut().as_mesh_mut() else {
            return Err(ProjectError::NotAMeshObject { id });
        };
        let count = mesh.positions().len();
        let mut seen = HashSet::with_capacity(moves.len());
        let mut resolved = Vec::with_capacity(moves.len());
        for (vertex, position) in moves {
            if !seen.insert(*vertex) {
                return Err(ProjectError::DuplicateVertex { object: id, vertex: *vertex });
            }
            let Ok(index) = usize::try_from(*vertex) else {
                return Err(MeshError::VertexIndexOutOfBounds { vertex: usize::MAX, count }.into());
            };
            if index >= count {
                return Err(MeshError::VertexIndexOutOfBounds { vertex: index, count }.into());
            }
            resolved.push((index, *position));
        }
        for (index, position) in resolved {
            mesh.set_position(index, position)?;
        }
        self.revision = next_revision;
        Ok(next_revision)
    }
}
