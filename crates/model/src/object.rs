//! Free-form geometry objects: a baked mesh, a path or a contour with a feature history.
//!
//! An object is a leaf in the container tree, placed like a panel but carrying no manufacturing
//! rules. A constructive payload stores the features it was built from; the mesh is derived and
//! replaces the history only when the user collapses it. A path payload is a flat drawing that has
//! no solid yet.

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::contour::{Contour, ContourError, MAX_CONTOUR_COORDINATE, Path};
use crate::mesh::Mesh;
use crate::{Id, Pivot, Placement, Properties, PropertyError, PropertyValue};

/// Largest number of features in a constructive object.
pub const MAX_OBJECT_FEATURES: usize = 32;

/// Serialized form of [`Extrude`]; validation happens in its `TryFrom` implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtrudeWire {
    contour: Contour,
    height: Length,
}

/// The first feature: a contour extruded along `+Z` from `z = 0` to `z = height`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ExtrudeWire")]
pub struct Extrude {
    contour: Contour,
    height: Length,
}

impl Extrude {
    /// Validates the height and stores the feature.
    pub fn new(contour: Contour, height: Length) -> Result<Self, ConstructiveError> {
        if !height.is_positive() || height.micrometres() > MAX_CONTOUR_COORDINATE.micrometres() {
            return Err(ConstructiveError::InvalidHeight { height, max: MAX_CONTOUR_COORDINATE });
        }
        Ok(Self { contour, height })
    }

    /// The contour in the object's local plane.
    pub const fn contour(&self) -> &Contour {
        &self.contour
    }

    /// The extrusion height, positive.
    pub const fn height(&self) -> Length {
        self.height
    }

    /// Replaces the contour, keeping the height.
    pub fn set_contour(&mut self, contour: Contour) {
        self.contour = contour;
    }
}

impl TryFrom<ExtrudeWire> for Extrude {
    type Error = ConstructiveError;

    fn try_from(wire: ExtrudeWire) -> Result<Self, Self::Error> {
        Self::new(wire.contour, wire.height)
    }
}

/// One step of a constructive object's feature history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Feature {
    /// Extrusion of a closed contour.
    Extrude(Extrude),
}

/// Serialized form of [`Constructive`]; validation happens in its `TryFrom` implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstructiveWire {
    features: Vec<Feature>,
}

/// A constructive geometry payload: an ordered feature history whose mesh is derived.
///
/// The contour and feature parameters are the source of truth; the mesh is never stored in the
/// document and is not editable until [`Project::collapse_object`] bakes it ([ADR 0037]).
///
/// [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ConstructiveWire")]
pub struct Constructive {
    features: Vec<Feature>,
}

impl Constructive {
    /// Validates the feature list and stores it.
    pub fn new(features: Vec<Feature>) -> Result<Self, ConstructiveError> {
        if features.is_empty() || features.len() > MAX_OBJECT_FEATURES {
            return Err(ConstructiveError::FeatureCount { count: features.len(), max: MAX_OBJECT_FEATURES });
        }
        if !matches!(features.first(), Some(Feature::Extrude(_))) {
            return Err(ConstructiveError::MissingExtrude);
        }
        Ok(Self { features })
    }

    /// Creates an object with a single extrusion.
    pub fn extrude(contour: Contour, height: Length) -> Result<Self, ConstructiveError> {
        Self::new(vec![Feature::Extrude(Extrude::new(contour, height)?)])
    }

    /// The ordered features, at least one.
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// The first extrusion, which every constructive object has.
    pub fn extrude_feature(&self) -> Option<&Extrude> {
        match self.features.first() {
            Some(Feature::Extrude(extrude)) => Some(extrude),
            None => None,
        }
    }

    /// The current contour of the first extrusion.
    pub fn contour(&self) -> Option<&Contour> {
        self.extrude_feature().map(Extrude::contour)
    }

    /// The height of the first extrusion.
    pub fn height(&self) -> Option<Length> {
        self.extrude_feature().map(Extrude::height)
    }

    /// Replaces the contour of the first extrusion, keeping every other feature.
    pub fn set_contour(&mut self, contour: Contour) -> Result<(), ConstructiveError> {
        let Some(Feature::Extrude(extrude)) = self.features.first_mut() else {
            return Err(ConstructiveError::MissingExtrude);
        };
        extrude.set_contour(contour);
        Ok(())
    }
}

impl TryFrom<ConstructiveWire> for Constructive {
    type Error = ConstructiveError;

    fn try_from(wire: ConstructiveWire) -> Result<Self, Self::Error> {
        Self::new(wire.features)
    }
}

/// A validation failure of a constructive object.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ConstructiveError {
    /// The contour is not a valid closed polyline.
    #[error(transparent)]
    Contour(#[from] ContourError),
    /// The extrusion height is not positive or is too large.
    #[error("an extrusion height must be positive and at most {max}, got {height}")]
    InvalidHeight {
        /// The rejected height.
        height: Length,
        /// The accepted maximum.
        max: Length,
    },
    /// The feature list is empty or too long.
    #[error("a constructive object needs between 1 and {max} features, got {count}")]
    FeatureCount {
        /// The supplied feature count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// The first feature is not an extrusion.
    #[error("the first feature of a constructive object must be an extrusion")]
    MissingExtrude,
}

/// The geometry payload of an object node.
///
/// Exactly one payload is stored. A `Mesh` is edited directly; a [`Constructive`] payload is
/// rebuilt from its features and is not editable as a mesh until the object is collapsed
/// ([ADR 0037]). A [`Path`] is a flat chain of drawn segments: it has no solid yet, and it becomes
/// one only when it is closed and extruded.
///
/// [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Geometry {
    /// A baked mesh, edited directly.
    Mesh(Mesh),
    /// A contour and its feature history; the mesh is derived.
    Constructive(Constructive),
    /// A flat, open or closed chain of drawn segments.
    Path(Path),
}

impl Geometry {
    /// The mesh, when the payload is a baked mesh.
    pub const fn as_mesh(&self) -> Option<&Mesh> {
        match self {
            Self::Mesh(mesh) => Some(mesh),
            Self::Constructive(_) | Self::Path(_) => None,
        }
    }

    /// The mesh for mutation, when the payload is a baked mesh.
    pub const fn as_mesh_mut(&mut self) -> Option<&mut Mesh> {
        match self {
            Self::Mesh(mesh) => Some(mesh),
            Self::Constructive(_) | Self::Path(_) => None,
        }
    }

    /// The feature history, when the payload is constructive.
    pub const fn as_constructive(&self) -> Option<&Constructive> {
        match self {
            Self::Constructive(constructive) => Some(constructive),
            Self::Mesh(_) | Self::Path(_) => None,
        }
    }

    /// The feature history for mutation, when the payload is constructive.
    pub const fn as_constructive_mut(&mut self) -> Option<&mut Constructive> {
        match self {
            Self::Constructive(constructive) => Some(constructive),
            Self::Mesh(_) | Self::Path(_) => None,
        }
    }

    /// The flat drawing, when the payload is a path.
    pub const fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Mesh(_) | Self::Constructive(_) => None,
        }
    }

    /// The flat drawing for mutation, when the payload is a path.
    pub const fn as_path_mut(&mut self) -> Option<&mut Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Mesh(_) | Self::Constructive(_) => None,
        }
    }
}

/// A free-form geometry object: a named, placed leaf with a geometry payload.
///
/// A panel is not an object: panel rules are manufacturing rules. An object is a leaf in v1, so
/// several parts are grouped by wrapping them in a container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default)]
    pivot: Pivot,
    #[serde(default)]
    properties: Properties,
    geometry: Geometry,
}

impl Object {
    /// Creates an object with a zero pivot.
    pub fn new(id: Id, name: impl Into<String>, placement: Placement, geometry: Geometry) -> Self {
        Self { id, name: name.into(), placement, pivot: Pivot::ZERO, properties: Properties::new(), geometry }
    }

    /// The stable object id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The placement relative to the parent frame.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The manipulation pivot in the object's own frame.
    pub const fn pivot(&self) -> Pivot {
        self.pivot
    }

    /// The geometry payload.
    pub const fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// The geometry payload for mutation.
    pub const fn geometry_mut(&mut self) -> &mut Geometry {
        &mut self.geometry
    }

    /// Replaces the user-visible name; the caller has already rejected a blank one.
    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Replaces the placement relative to the parent container.
    pub(crate) fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    /// Replaces the stored manipulation pivot.
    pub(crate) fn set_pivot(&mut self, pivot: Pivot) {
        self.pivot = pivot;
    }

    /// Replaces the stable id when a subtree is copied.
    pub(crate) fn set_node_id(&mut self, id: Id) {
        self.id = id;
    }

    /// The typed properties of this object.
    pub const fn properties(&self) -> &Properties {
        &self.properties
    }

    /// Stores one validated property.
    pub fn set_property(&mut self, key: &str, value: PropertyValue) -> Result<(), PropertyError> {
        self.properties.insert(key, value).map(|_| ())
    }

    /// Removes one property; returns whether it was present.
    pub fn clear_property(&mut self, key: &str) -> bool {
        self.properties.remove(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::*;

    use crate::tree::object_bounds_mm;
    use furnicraft_geom::{DQuat, DVec3};

    #[test]
    fn object_round_trips_in_the_current_schema() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![sample_object(5).into()]);
        let project = Project::new(root).expect("valid project");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let object = project.object(Id::new(5).expect("id")).expect("object");
        assert_eq!(object.geometry().as_mesh().expect("mesh").faces().len(), 6);

        let text = serde_json::to_string(&project).expect("serialize project");
        assert!(text.contains(&format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION)));
        assert!(text.contains("\"object\":{"));
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn object_node_commands_rename_place_duplicate_and_delete() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![sample_object(5).into()]);
        let mut project = Project::new(root).expect("valid project");
        let id = Id::new(5).expect("id");

        project.rename_node(id, "Bracket").expect("rename");
        assert_eq!(project.object(id).expect("object").name(), "Bracket");

        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("placement");
        project.set_node_placement(id, placement).expect("place");
        assert_eq!(project.object(id).expect("object").placement(), placement);

        let copy = project.duplicate_node(id).expect("duplicate");
        assert_ne!(copy, id);
        let copy_object = project.object(copy).expect("copied object");
        assert_eq!(copy_object.name(), "Bracket copy");
        assert_eq!(copy_object.geometry(), project.object(id).expect("object").geometry());

        project.remove_node(id).expect("delete");
        assert!(project.object(id).is_none());
        assert!(project.object(copy).is_some());
    }

    #[test]
    fn mesh_vertices_move_as_one_revision() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![sample_object(5).into()]);
        let mut project = Project::new(root).expect("valid project");
        let id = Id::new(5).expect("id");
        let before = project.object(id).expect("object").geometry().as_mesh().expect("mesh").positions()[0];
        let target = [millimetres(10), millimetres(20), millimetres(30)];

        let revision = project.move_mesh_vertices(id, &[(0, target)]).expect("move the vertex");

        assert_eq!(revision.get(), 1);
        assert_eq!(project.revision().get(), 1);
        let mesh = project.object(id).expect("object").geometry().as_mesh().expect("mesh");
        assert_eq!(mesh.positions()[0], target);
        assert_ne!(before, target);
        assert_eq!(mesh.positions().len(), 8, "a move changes no topology");
        assert_eq!(mesh.faces().len(), 6);
    }

    #[test]
    fn a_bad_vertex_batch_is_rejected_without_mutating() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![sample_object(5).into()]);
        let mut project = Project::new(root).expect("valid project");
        let id = Id::new(5).expect("id");
        let before = project.clone();
        let target = [millimetres(1), millimetres(2), millimetres(3)];

        assert_eq!(project.move_mesh_vertices(id, &[]), Err(ProjectError::NoVertexMoves));
        assert_eq!(
            project.move_mesh_vertices(id, &[(0, target), (0, target)]),
            Err(ProjectError::DuplicateVertex { object: id, vertex: 0 })
        );
        assert_eq!(
            project.move_mesh_vertices(id, &[(0, target), (8, target)]),
            Err(ProjectError::InvalidGeometry(MeshError::VertexIndexOutOfBounds { vertex: 8, count: 8 }))
        );
        assert_eq!(project.revision(), before.revision(), "a rejected batch does not bump the revision");
        assert!(project.same_content(&before));
    }

    #[test]
    fn a_non_mesh_node_has_no_editable_vertices() {
        let root = Container::new(
            Id::new(1).expect("id"),
            "Root",
            Placement::IDENTITY,
            vec![sample_panel(Id::new(2).expect("id"), "Shelf").into()],
        );
        let mut project = Project::new(root).expect("valid project");
        let target = [millimetres(1), millimetres(2), millimetres(3)];

        assert_eq!(
            project.move_mesh_vertices(Id::new(2).expect("id"), &[(0, target)]),
            Err(ProjectError::NotAMeshObject { id: Id::new(2).expect("id") })
        );
        assert_eq!(
            project.move_mesh_vertices(Id::new(99).expect("id"), &[(0, target)]),
            Err(ProjectError::NodeNotFound { id: Id::new(99).expect("id") })
        );
        assert_eq!(project.revision(), Revision::ZERO);
    }

    #[test]
    fn object_duplicate_id_is_rejected() {
        let root = Container::new(
            Id::new(1).expect("id"),
            "Root",
            Placement::IDENTITY,
            vec![sample_object(5).into(), sample_object(5).into()],
        );
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(Id::new(5).expect("id"))));
    }

    #[test]
    fn query_selects_objects_by_structural_kind() {
        let root = Container::new(
            Id::new(1).expect("id"),
            "Root",
            Placement::IDENTITY,
            vec![sample_object(5).into(), sample_panel(Id::new(2).expect("id"), "Shelf").into()],
        );
        let project = Project::new(root).expect("valid project");
        let found = project.query(&NodeQuery::any().with_kind(StructuralKind::Object));
        assert_eq!(found, vec![Id::new(5).expect("id")]);
    }

    #[test]
    fn a_constructive_object_keeps_its_contour_and_collapses_to_a_mesh() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let root = project.root().id();
        let constructive = Constructive::extrude(rectangle_contour(100, 50), millimetres(18)).expect("constructive");
        let id = project
            .add_object(root, "Countertop", Placement::IDENTITY, Geometry::Constructive(constructive.clone()))
            .expect("object");

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"kind\":\"constructive\""), "the payload is tagged");
        assert_eq!(serde_json::from_str::<Project>(&text).expect("load"), project, "the history round-trips");
        assert!(project.object(id).expect("object").geometry().as_mesh().is_none(), "the mesh is derived");

        let wider = rectangle_contour(120, 50);
        let revision = project.set_object_contour(id, wider.clone()).expect("replace the contour");
        assert_eq!(revision.get(), 2, "creation was revision one");
        let object = project.object(id).expect("object");
        assert_eq!(object.name(), "Countertop");
        assert_eq!(object.placement(), Placement::IDENTITY);
        assert_eq!(
            object.geometry().as_constructive().expect("constructive").contour(),
            Some(&wider),
            "the id, name and placement survive a contour edit"
        );
        assert_eq!(
            project.move_mesh_vertices(id, &[(0, [millimetres(1), millimetres(1), millimetres(1)])]),
            Err(ProjectError::NotAMeshObject { id })
        );

        let mesh = Mesh::cube(millimetres(10)).expect("mesh");
        let revision = project.collapse_object(id, mesh.clone()).expect("collapse");
        assert_eq!(revision.get(), 3);
        let object = project.object(id).expect("object");
        assert_eq!(object.geometry().as_mesh(), Some(&mesh));
        assert!(object.geometry().as_constructive().is_none(), "collapse drops the history");
        assert_eq!(project.collapse_object(id, mesh), Err(ProjectError::NotConstructiveObject { id }));
    }

    #[test]
    fn a_constructive_object_bounds_use_the_contour_and_height() {
        let geometry = Geometry::Constructive(
            Constructive::extrude(rectangle_contour(100, 50), millimetres(18)).expect("constructive"),
        );
        let (min, max) = object_bounds_mm(&geometry).expect("bounds");
        assert!((min - DVec3::ZERO).length() < 1.0e-9);
        assert!((max - DVec3::new(100.0, 50.0, 18.0)).length() < 1.0e-9);
    }

    fn sample_path() -> Path {
        Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
            ],
            false,
        )
        .expect("open path")
    }

    #[test]
    fn a_path_object_round_trips_and_replaces_its_drawing() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let root = project.root().id();
        let path = sample_path();
        let id = project
            .add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path.clone()))
            .expect("a path object is added like any other object");

        assert_eq!(project.object_path(id), Ok(&path), "the drawing is queried by node id");
        assert!(project.object(id).expect("object").geometry().as_mesh().is_none(), "a path has no mesh");
        assert!(project.object(id).expect("object").geometry().as_constructive().is_none());

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"kind\":\"path\""), "the payload is tagged: {text}");
        let decoded: Project = serde_json::from_str(&text).expect("load");
        assert_eq!(decoded, project, "the path survives a save and load");
        assert_eq!(decoded.object_path(id), Ok(&path));

        let longer = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(0), millimetres(50)])
                    .expect("line"),
            ],
            false,
        )
        .expect("longer path");
        let revision = project.set_object_path(id, longer.clone()).expect("replace the drawing");
        assert_eq!(revision.get(), 2, "creation was revision one");
        assert_eq!(project.object_path(id), Ok(&longer));
        assert_eq!(project.object(id).expect("object").name(), "Sketch", "the node identity is unchanged");
    }

    #[test]
    fn a_path_command_refuses_other_payloads_and_missing_nodes() {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![sample_object(5).into()]);
        let mut project = Project::new(root).expect("valid project");
        let mesh_id = Id::new(5).expect("id");

        assert_eq!(project.object_path(mesh_id), Err(ProjectError::NotAPathObject { id: mesh_id }));
        assert_eq!(project.set_object_path(mesh_id, sample_path()), Err(ProjectError::NotAPathObject { id: mesh_id }));
        assert_eq!(
            project.object_path(Id::new(99).expect("id")),
            Err(ProjectError::NodeNotFound { id: Id::new(99).expect("id") })
        );
        assert_eq!(project.revision(), Revision::ZERO, "a refused command does not bump the revision");
    }

    #[test]
    fn a_path_object_bounds_use_the_chain_extents() {
        let geometry = Geometry::Path(sample_path());
        let (min, max) = object_bounds_mm(&geometry).expect("bounds");
        assert!((min - DVec3::ZERO).length() < 1.0e-9);
        assert!((max - DVec3::new(100.0, 50.0, 0.0)).length() < 1.0e-9, "a path has no height");
    }

    #[test]
    fn a_bad_extrusion_height_is_rejected_on_build_and_load() {
        let contour = rectangle_contour(10, 10);
        assert_eq!(
            Constructive::extrude(contour.clone(), Length::ZERO),
            Err(ConstructiveError::InvalidHeight { height: Length::ZERO, max: MAX_CONTOUR_COORDINATE })
        );
        let constructive = Constructive::extrude(contour, millimetres(10)).expect("constructive");
        let text = serde_json::to_string(&constructive).expect("serialize");
        let too_high = text.replace("\"height\":10000", "\"height\":0");
        assert_ne!(text, too_high, "the fixture must contain the height in micrometres");
        assert!(serde_json::from_str::<Constructive>(&too_high).is_err());
    }
}
