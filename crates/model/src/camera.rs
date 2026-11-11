//! Authored camera objects: projection, clipping range and the active camera link.
//!
//! A camera is document data, not a viewport: the user adds one, places it and chooses it as the
//! active camera. The viewport keeps its own navigation state ([ADR 0036]).
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use furnicraft_units::{Degrees, Length};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Id, PivotLocal, Placement, Properties, PropertyError, PropertyValue};

/// How an authored camera projects the scene.
///
/// A perspective camera stores its vertical field of view; an orthographic camera stores the
/// visible height in its local frame. Both are document data: switching projection is a property
/// of the camera object, not of the viewport ([ADR 0036]).
///
/// [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CameraProjection {
    /// A perspective view with a vertical field of view.
    Perspective {
        /// Vertical field of view, validated to 1°–179°.
        fov_y: Degrees,
    },
    /// An orthographic view showing a fixed height.
    Orthographic {
        /// Visible height in the camera's local frame, positive.
        height: Length,
    },
}

impl CameraProjection {
    /// The default perspective projection: 50° vertically.
    pub const DEFAULT: Self = Self::Perspective { fov_y: Degrees::from_millidegrees(50_000) };

    /// Whether the projection parameters are inside their accepted ranges.
    pub fn is_valid(&self) -> bool {
        match *self {
            Self::Perspective { fov_y } => (CameraNode::MIN_FOV.millidegrees()..=CameraNode::MAX_FOV.millidegrees())
                .contains(&fov_y.millidegrees()),
            Self::Orthographic { height } => {
                height.is_positive() && height.micrometres() <= CameraNode::MAX_ORTHOGRAPHIC_HEIGHT.micrometres()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraProjectionWire {
    kind: ProjectionKindWire,
    #[serde(default)]
    fov_y: Option<Degrees>,
    #[serde(default)]
    height: Option<Length>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectionKindWire {
    Perspective,
    Orthographic,
}

impl<'de> Deserialize<'de> for CameraProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CameraProjectionWire::deserialize(deserializer)?;
        match (wire.kind, wire.fov_y, wire.height) {
            (ProjectionKindWire::Perspective, Some(fov_y), None) => Ok(Self::Perspective { fov_y }),
            (ProjectionKindWire::Orthographic, None, Some(height)) => Ok(Self::Orthographic { height }),
            (ProjectionKindWire::Perspective, _, _) => {
                Err(de::Error::custom("a perspective camera needs exactly the field `fov_y`"))
            }
            (ProjectionKindWire::Orthographic, _, _) => {
                Err(de::Error::custom("an orthographic camera needs exactly the field `height`"))
            }
        }
    }
}

/// A camera the user authored.
///
/// The built-in viewport camera is a default, not a node. An authored camera is a scene object:
/// it has a parent, a placement, typed properties, selection and undo, and the document may name
/// one of them as its active camera ([ADR 0036]).
///
/// [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CameraNode {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default, alias = "pivot")]
    pivot_local: PivotLocal,
    #[serde(default)]
    properties: Properties,
    projection: CameraProjection,
}

impl CameraNode {
    /// Smallest accepted vertical field of view: one degree.
    pub const MIN_FOV: Degrees = Degrees::from_millidegrees(1_000);
    /// Largest accepted vertical field of view: 179 degrees.
    pub const MAX_FOV: Degrees = Degrees::from_millidegrees(179_000);
    /// Largest accepted orthographic height: one kilometre.
    pub const MAX_ORTHOGRAPHIC_HEIGHT: Length = Length::from_micrometres(1_000_000_000);

    /// Creates a camera after validating its projection.
    pub fn new(
        id: Id,
        name: impl Into<String>,
        placement: Placement,
        projection: CameraProjection,
    ) -> Result<Self, CameraError> {
        if !projection.is_valid() {
            return Err(CameraError::InvalidProjection);
        }
        Ok(Self {
            id,
            name: name.into(),
            placement,
            pivot_local: PivotLocal::ZERO,
            properties: Properties::new(),
            projection,
        })
    }

    /// The stable camera id.
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

    /// The manipulation pivot in the camera's own frame.
    pub const fn pivot_local(&self) -> PivotLocal {
        self.pivot_local
    }

    /// The projection parameters.
    pub const fn projection(&self) -> CameraProjection {
        self.projection
    }

    /// Replaces the projection after validating it.
    pub fn set_projection(&mut self, projection: CameraProjection) -> Result<(), CameraError> {
        if !projection.is_valid() {
            return Err(CameraError::InvalidProjection);
        }
        self.projection = projection;
        Ok(())
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
    pub(crate) fn set_pivot(&mut self, pivot_local: PivotLocal) {
        self.pivot_local = pivot_local;
    }

    /// Replaces the stable id when a subtree is copied.
    pub(crate) fn set_node_id(&mut self, id: Id) {
        self.id = id;
    }

    /// The typed properties of this camera.
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

/// A serialization failure of a camera node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum CameraError {
    /// The field of view or the orthographic height is outside its range.
    #[error("camera projection is outside its accepted range")]
    InvalidProjection,
}

/// Serialized form of [`CameraNode`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraNodeWire {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default, alias = "pivot")]
    pivot_local: PivotLocal,
    #[serde(default)]
    properties: Properties,
    projection: CameraProjection,
}

impl TryFrom<CameraNodeWire> for CameraNode {
    type Error = CameraError;

    fn try_from(wire: CameraNodeWire) -> Result<Self, Self::Error> {
        if !wire.projection.is_valid() {
            return Err(CameraError::InvalidProjection);
        }
        Ok(Self {
            id: wire.id,
            name: wire.name,
            placement: wire.placement,
            pivot_local: wire.pivot_local,
            properties: wire.properties,
            projection: wire.projection,
        })
    }
}

impl<'de> Deserialize<'de> for CameraNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CameraNodeWire::deserialize(deserializer)?.try_into().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    use crate::fixtures::*;
    use furnicraft_geom::{DQuat, DVec3};

    #[test]
    fn camera_and_light_nodes_round_trip_in_the_document() {
        let mut project =
            Project::new(Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new()))
                .expect("valid project");
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        let light = project
            .add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0, 0.9, 0.8], 2.0)
            .expect("add light");

        assert_eq!(project.camera(camera).expect("camera").name(), "Camera");
        assert_eq!(project.light(light).expect("light").kind(), LightKind::Sun);
        assert_eq!(project.camera(light), None, "a light is not a camera");
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Camera)), vec![camera]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Light)), vec![light]);

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"camera\":{"));
        assert!(text.contains("\"light\":{"));
        assert!(!text.contains("active_camera"), "an unset active camera is not written");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(decoded, project);
    }

    #[test]
    fn a_camera_and_a_light_move_and_hold_properties() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        let light =
            project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("add light");
        let moved = Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");

        let before = project.revision();
        let after = project.set_node_placements(&[(camera, moved), (light, moved)]).expect("a camera and a light move");
        assert_eq!(after, before.checked_next().expect("revision"), "a batch is one revision");
        assert_eq!(project.camera(camera).expect("camera").placement(), moved);
        assert_eq!(project.light(light).expect("light").placement(), moved);

        let text = PropertyValue::Text { value: "presentation".to_owned() };
        let property_revision = project.set_property(camera, "shot", text.clone()).expect("a camera holds properties");
        assert_eq!(property_revision, after.checked_next().expect("revision"));
        assert_eq!(project.properties(camera).and_then(|properties| properties.get("shot")), Some(&text));
    }

    #[test]
    fn the_active_camera_must_be_a_camera_and_is_cleared_with_it() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        let panel = Id::new(2).expect("panel");

        assert_eq!(project.set_active_camera(Some(panel)), Err(ProjectError::CameraNotFound { id: panel }));
        assert_eq!(
            project.set_active_camera(Some(Id::new(99).expect("id"))),
            Err(ProjectError::CameraNotFound { id: Id::new(99).expect("id") })
        );
        project.set_active_camera(Some(camera)).expect("activate the camera");
        assert_eq!(project.active_camera(), Some(camera));

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"active_camera\":"));
        let decoded: Project = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(decoded.active_camera(), Some(camera));

        // A placement or pivot edit keeps the camera active; deleting it falls back to the built-in.
        project.set_node_placement(camera, Placement::IDENTITY).expect("move the camera");
        assert_eq!(project.active_camera(), Some(camera));

        // Nesting is allowed; deleting the container that holds the camera clears the reference too.
        let drawer = Id::new(3).expect("id");
        let nested = project
            .add_camera(drawer, "Nested", Placement::IDENTITY, CameraProjection::DEFAULT)
            .expect("add a nested camera");
        project.set_active_camera(Some(nested)).expect("still a camera below the drawer");
        project.remove_node(drawer).expect("delete the drawer");
        assert_eq!(project.active_camera(), None);
        assert_eq!(project.camera(nested), None);
        assert!(project.camera(camera).is_some(), "a sibling camera is untouched");
    }

    #[test]
    fn a_duplicated_camera_gets_fresh_ids_and_keeps_the_active_one() {
        let mut project =
            Project::new(Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new()))
                .expect("valid project");
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        project.set_active_camera(Some(camera)).expect("active camera");

        let copy = project.duplicate_node(camera).expect("duplicate");
        assert_ne!(copy, camera);
        assert_eq!(project.camera(copy).expect("copy").name(), "Camera copy");
        assert_eq!(project.active_camera(), Some(camera), "a duplicate does not become active");
    }

    #[test]
    fn a_camera_projection_outside_its_range_is_rejected_on_build_and_load() {
        let id = Id::new(5).expect("id");
        assert_eq!(
            CameraNode::new(id, "Camera", Placement::IDENTITY, CameraProjection::Perspective { fov_y: Degrees::ZERO }),
            Err(CameraError::InvalidProjection)
        );
        assert_eq!(
            CameraNode::new(id, "Camera", Placement::IDENTITY, CameraProjection::Orthographic { height: Length::ZERO }),
            Err(CameraError::InvalidProjection)
        );

        let camera = CameraNode::new(id, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let text = serde_json::to_string(&camera).expect("serialize");
        let too_narrow = text.replace("\"fov_y\":50000", "\"fov_y\":0");
        assert_ne!(too_narrow, text);
        assert!(serde_json::from_str::<CameraNode>(&too_narrow).is_err());

        let extra_field = text.replace("\"fov_y\":50000", "\"fov_y\":50000,\"height\":1000");
        assert_ne!(extra_field, text);
        assert!(serde_json::from_str::<CameraNode>(&extra_field).is_err(), "a projection carries one parameter set");
    }

    #[test]
    fn camera_projection_and_light_params_change_in_one_revision() {
        let mut project =
            Project::new(Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new()))
                .expect("valid project");
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        let light =
            project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("add light");
        let not_a_camera = project.root().id();

        let before = project.revision();
        let orthographic = CameraProjection::Orthographic { height: Length::from_millimetres(2_000).expect("height") };
        let after = project.set_camera_projection(camera, orthographic).expect("projection");
        assert_eq!(after, before.checked_next().expect("revision"));
        assert_eq!(project.camera(camera).expect("camera").projection(), orthographic);

        // A refused value changes neither the projection nor the revision.
        project
            .set_camera_projection(camera, CameraProjection::Perspective { fov_y: Degrees::ZERO })
            .expect_err("a zero field of view is invalid");
        assert_eq!(project.camera(camera).expect("camera").projection(), orthographic);
        assert_eq!(project.revision(), after);
        assert_eq!(
            project.set_camera_projection(not_a_camera, CameraProjection::DEFAULT),
            Err(ProjectError::CameraNotFound { id: not_a_camera })
        );

        project.set_light_params(light, LightKind::Point, [0.5, 0.4, 0.3], 4.0).expect("light params");
        let light_node = project.light(light).expect("light");
        assert_eq!(light_node.kind(), LightKind::Point);
        assert_eq!(light_node.color(), [0.5, 0.4, 0.3]);
        assert_eq!(light_node.intensity(), 4.0);
        assert_eq!(
            project.set_light_params(light, LightKind::Point, [2.0; 3], 1.0),
            Err(ProjectError::Light(LightError::InvalidColor))
        );
        assert_eq!(
            project.set_light_params(camera, LightKind::Point, [1.0; 3], 1.0),
            Err(ProjectError::LightNotFound { id: camera })
        );
    }
}
