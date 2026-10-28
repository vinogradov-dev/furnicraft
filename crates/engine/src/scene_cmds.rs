//! Commands for authored cameras and lights.
//!
//! A camera or a light is a scene object like any other node: it has a parent, a placement, typed
//! properties, selection and undo ([ADR 0036]). The built-in camera and key light are defaults,
//! not nodes, so these commands create or edit authored objects only.
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use furnicraft_model::{CameraProjection, Catalog, Id, LightKind, LightNode, Placement, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::node_cmds::{parse, validate_name};
use crate::session::Session;

/// Stable id of the camera creation command.
pub const CAMERA_CREATE_ID: &str = "camera.create";

/// User-facing label of the camera creation command.
pub const CAMERA_CREATE_LABEL: &str = "Create camera";

/// Parameters for `camera.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraCreate {
    /// Id of the container that receives the new camera.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Placement relative to the parent container.
    pub placement: Placement,
    /// Projection parameters, validated on the way in.
    pub projection: CameraProjection,
}

/// The `camera.create` command.
pub(crate) const CAMERA_CREATE: CommandSpec =
    CommandSpec::new(CAMERA_CREATE_ID, CAMERA_CREATE_LABEL, camera_create_enabled, camera_create_run);

fn camera_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<CameraCreate>(params, CAMERA_CREATE_ID) else {
        return false;
    };
    session.project().is_container(params.parent) && !params.name.trim().is_empty() && params.projection.is_valid()
}

fn camera_create_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<CameraCreate>(params, CAMERA_CREATE_ID)?;
    validate_name(&params.name, CAMERA_CREATE_ID)?;
    let id = project.add_camera(params.parent, params.name, params.placement, params.projection)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the light creation command.
pub const LIGHT_CREATE_ID: &str = "light.create";

/// User-facing label of the light creation command.
pub const LIGHT_CREATE_LABEL: &str = "Create light";

/// Parameters for `light.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightCreate {
    /// Id of the container that receives the new light.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Placement relative to the parent container.
    pub placement: Placement,
    /// Directional sun or point light.
    pub kind: LightKind,
    /// Linear RGB colour, each component inside `0.0..=1.0`.
    pub color: [f32; 3],
    /// Non-negative intensity multiplier.
    pub intensity: f32,
}

/// The `light.create` command.
pub(crate) const LIGHT_CREATE: CommandSpec =
    CommandSpec::new(LIGHT_CREATE_ID, LIGHT_CREATE_LABEL, light_create_enabled, light_create_run);

fn light_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<LightCreate>(params, LIGHT_CREATE_ID) else {
        return false;
    };
    session.project().is_container(params.parent)
        && !params.name.trim().is_empty()
        && valid_color(params.color)
        && valid_intensity(params.intensity)
}

fn light_create_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<LightCreate>(params, LIGHT_CREATE_ID)?;
    validate_name(&params.name, LIGHT_CREATE_ID)?;
    if !valid_color(params.color) {
        return Err(CommandError::InvalidParams {
            command: LIGHT_CREATE_ID,
            message: "colour must be finite and inside 0.0..=1.0".to_owned(),
        });
    }
    if !valid_intensity(params.intensity) {
        return Err(CommandError::InvalidParams {
            command: LIGHT_CREATE_ID,
            message: "intensity must be finite and non-negative".to_owned(),
        });
    }
    let id =
        project.add_light(params.parent, params.name, params.placement, params.kind, params.color, params.intensity)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the active-camera command.
pub const SCENE_ACTIVE_CAMERA_SET_ID: &str = "scene.active_camera.set";

/// User-facing label of the active-camera command.
pub const SCENE_ACTIVE_CAMERA_SET_LABEL: &str = "Set active camera";

/// Parameters for `scene.active_camera.set`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneActiveCameraSet {
    /// The camera to render from, or `None` for the built-in camera.
    pub camera: Option<Id>,
}

/// The `scene.active_camera.set` command.
pub(crate) const SCENE_ACTIVE_CAMERA_SET: CommandSpec = CommandSpec::new(
    SCENE_ACTIVE_CAMERA_SET_ID,
    SCENE_ACTIVE_CAMERA_SET_LABEL,
    scene_active_camera_set_enabled,
    scene_active_camera_set_run,
);

fn scene_active_camera_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<SceneActiveCameraSet>(params, SCENE_ACTIVE_CAMERA_SET_ID) else {
        return false;
    };
    match params.camera {
        Some(id) => session.project().camera(id).is_some(),
        None => true,
    }
}

fn scene_active_camera_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<SceneActiveCameraSet>(params, SCENE_ACTIVE_CAMERA_SET_ID)?;
    project.set_active_camera(params.camera)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the camera projection command.
pub const CAMERA_PROJECTION_SET_ID: &str = "camera.projection.set";

/// User-facing label of the camera projection command.
pub const CAMERA_PROJECTION_SET_LABEL: &str = "Set camera projection";

/// Parameters for `camera.projection.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraProjectionSet {
    /// The camera whose projection changes.
    pub camera: Id,
    /// The new projection, validated on the way in.
    pub projection: CameraProjection,
}

/// The `camera.projection.set` command.
pub(crate) const CAMERA_PROJECTION_SET: CommandSpec = CommandSpec::new(
    CAMERA_PROJECTION_SET_ID,
    CAMERA_PROJECTION_SET_LABEL,
    camera_projection_set_enabled,
    camera_projection_set_run,
);

fn camera_projection_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<CameraProjectionSet>(params, CAMERA_PROJECTION_SET_ID) else {
        return false;
    };
    session.project().camera(params.camera).is_some() && params.projection.is_valid()
}

fn camera_projection_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<CameraProjectionSet>(params, CAMERA_PROJECTION_SET_ID)?;
    project.set_camera_projection(params.camera, params.projection)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the light parameter command.
pub const LIGHT_PARAMS_SET_ID: &str = "light.params.set";

/// User-facing label of the light parameter command.
pub const LIGHT_PARAMS_SET_LABEL: &str = "Set light parameters";

/// Parameters for `light.params.set`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightParamsSet {
    /// The light whose parameters change.
    pub light: Id,
    /// Directional sun or point light.
    pub kind: LightKind,
    /// Linear RGB colour, each component inside `0.0..=1.0`.
    pub color: [f32; 3],
    /// Non-negative intensity multiplier.
    pub intensity: f32,
}

/// The `light.params.set` command.
pub(crate) const LIGHT_PARAMS_SET: CommandSpec =
    CommandSpec::new(LIGHT_PARAMS_SET_ID, LIGHT_PARAMS_SET_LABEL, light_params_set_enabled, light_params_set_run);

fn light_params_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<LightParamsSet>(params, LIGHT_PARAMS_SET_ID) else {
        return false;
    };
    session.project().light(params.light).is_some() && valid_color(params.color) && valid_intensity(params.intensity)
}

fn light_params_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<LightParamsSet>(params, LIGHT_PARAMS_SET_ID)?;
    if !valid_color(params.color) {
        return Err(CommandError::InvalidParams {
            command: LIGHT_PARAMS_SET_ID,
            message: "colour must be finite and inside 0.0..=1.0".to_owned(),
        });
    }
    if !valid_intensity(params.intensity) {
        return Err(CommandError::InvalidParams {
            command: LIGHT_PARAMS_SET_ID,
            message: "intensity must be finite and non-negative".to_owned(),
        });
    }
    project.set_light_params(params.light, params.kind, params.color, params.intensity)?;
    Ok(CommandOutcome::changed())
}

fn valid_color(color: [f32; 3]) -> bool {
    color.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

fn valid_intensity(intensity: f32) -> bool {
    intensity.is_finite() && (0.0..=LightNode::MAX_INTENSITY).contains(&intensity)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::Revision;
    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn camera_params(session: &Session, name: &str) -> Value {
        serde_json::to_value(CameraCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            placement: Placement::IDENTITY,
            projection: CameraProjection::DEFAULT,
        })
        .expect("params serialize")
    }

    fn light_params(session: &Session, name: &str) -> Value {
        serde_json::to_value(LightCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            placement: Placement::IDENTITY,
            kind: LightKind::Sun,
            color: [1.0, 0.9, 0.8],
            intensity: 2.0,
        })
        .expect("params serialize")
    }

    #[test]
    fn camera_and_light_creation_take_one_revision_each() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let camera = session
            .dispatch(CAMERA_CREATE_ID, &camera_params(&session, "Camera"))
            .expect("create camera")
            .created_id()
            .expect("a camera id");
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().camera(camera).expect("camera").name(), "Camera");

        let light = session
            .dispatch(LIGHT_CREATE_ID, &light_params(&session, "Key"))
            .expect("create light")
            .created_id()
            .expect("a light id");
        assert_eq!(session.project().revision().get(), 2);
        assert_eq!(session.project().light(light).expect("light").intensity(), 2.0);
        assert!(session.can_undo());
    }

    #[test]
    fn creation_rejects_a_bad_parent_name_or_value_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let missing = Id::new(99).expect("id");

        let mut bad_parent = CameraCreate {
            parent: missing,
            name: "Camera".to_owned(),
            placement: Placement::IDENTITY,
            projection: CameraProjection::DEFAULT,
        };
        let value = serde_json::to_value(&bad_parent).expect("params");
        assert!(session.dispatch(CAMERA_CREATE_ID, &value).is_err());

        bad_parent.parent = session.project().root().id();
        bad_parent.name = "  ".to_owned();
        let value = serde_json::to_value(&bad_parent).expect("params");
        assert!(session.dispatch(CAMERA_CREATE_ID, &value).is_err());

        bad_parent.name = "Camera".to_owned();
        bad_parent.projection = CameraProjection::Perspective { fov_y: furnicraft_units::Degrees::ZERO };
        let value = serde_json::to_value(&bad_parent).expect("params");
        assert!(session.dispatch(CAMERA_CREATE_ID, &value).is_err());

        let bad_light = LightCreate {
            parent: session.project().root().id(),
            name: "Key".to_owned(),
            placement: Placement::IDENTITY,
            kind: LightKind::Point,
            color: [2.0, 0.0, 0.0],
            intensity: 1.0,
        };
        let value = serde_json::to_value(&bad_light).expect("params");
        assert!(session.dispatch(LIGHT_CREATE_ID, &value).is_err());

        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.can_undo());
    }

    #[test]
    fn the_active_camera_only_accepts_a_camera_and_clears_with_none() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let camera = session
            .dispatch(CAMERA_CREATE_ID, &camera_params(&session, "Camera"))
            .expect("create camera")
            .created_id()
            .expect("a camera id");
        let root = session.project().root().id();

        let value = serde_json::to_value(SceneActiveCameraSet { camera: Some(root) }).expect("params");
        assert!(session.dispatch(SCENE_ACTIVE_CAMERA_SET_ID, &value).is_err());

        let value = serde_json::to_value(SceneActiveCameraSet { camera: Some(camera) }).expect("params");
        session.dispatch(SCENE_ACTIVE_CAMERA_SET_ID, &value).expect("activate");
        assert_eq!(session.project().active_camera(), Some(camera));

        let value = serde_json::to_value(SceneActiveCameraSet { camera: None }).expect("params");
        session.dispatch(SCENE_ACTIVE_CAMERA_SET_ID, &value).expect("clear");
        assert_eq!(session.project().active_camera(), None);
        assert!(session.can_undo(), "each change is one undo entry");
    }

    #[test]
    fn projection_and_light_parameter_commands_write_one_revision() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let camera = session
            .dispatch(CAMERA_CREATE_ID, &camera_params(&session, "Camera"))
            .expect("create camera")
            .created_id()
            .expect("a camera id");
        let light = session
            .dispatch(LIGHT_CREATE_ID, &light_params(&session, "Key"))
            .expect("create light")
            .created_id()
            .expect("a light id");
        let before = session.project().revision();

        let orthographic = CameraProjection::Orthographic {
            height: furnicraft_units::Length::from_millimetres(2_000).expect("height"),
        };
        let value = serde_json::to_value(CameraProjectionSet { camera, projection: orthographic }).expect("params");
        session.dispatch(CAMERA_PROJECTION_SET_ID, &value).expect("projection");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        assert_eq!(session.project().camera(camera).expect("camera").projection(), orthographic);

        let value = serde_json::to_value(LightParamsSet {
            light,
            kind: LightKind::Point,
            color: [0.2, 0.3, 0.4],
            intensity: 3.0,
        })
        .expect("params");
        session.dispatch(LIGHT_PARAMS_SET_ID, &value).expect("light params");
        assert_eq!(session.project().light(light).expect("light").kind(), LightKind::Point);

        let refusal = serde_json::to_value(LightParamsSet {
            light: camera,
            kind: LightKind::Point,
            color: [1.0; 3],
            intensity: 1.0,
        })
        .expect("params");
        assert!(session.dispatch(LIGHT_PARAMS_SET_ID, &refusal).is_err());
        let revision = session.project().revision();
        assert_eq!(revision, before.checked_next().and_then(|r| r.checked_next()).expect("revisions"));
    }

    #[test]
    fn a_camera_and_a_light_rename_move_and_hold_properties() {
        use crate::node_cmds::{NODE_PLACEMENT_SET_MANY_ID, NODE_PROPERTY_SET_ID, NODE_RENAME_ID};
        use crate::{NodePlacement, NodePlacementSetMany, NodePropertySet, NodeRename};
        use furnicraft_model::PropertyValue;

        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let camera = session
            .dispatch(CAMERA_CREATE_ID, &camera_params(&session, "Camera"))
            .expect("create camera")
            .created_id()
            .expect("a camera id");
        let light = session
            .dispatch(LIGHT_CREATE_ID, &light_params(&session, "Key"))
            .expect("create light")
            .created_id()
            .expect("a light id");

        let value = serde_json::to_value(NodeRename { node: camera, name: "Presentation".to_owned() }).expect("params");
        session.dispatch(NODE_RENAME_ID, &value).expect("rename the camera");
        assert_eq!(session.project().camera(camera).expect("camera").name(), "Presentation");

        let value = serde_json::to_value(NodeRename { node: light, name: "Fill".to_owned() }).expect("params");
        session.dispatch(NODE_RENAME_ID, &value).expect("rename the light");
        assert_eq!(session.project().light(light).expect("light").name(), "Fill");

        let value = serde_json::to_value(NodePropertySet {
            node: light,
            key: "shot".to_owned(),
            value: PropertyValue::Bool { value: true },
        })
        .expect("params");
        session.dispatch(NODE_PROPERTY_SET_ID, &value).expect("a light holds properties");

        let moved: Placement = serde_json::from_value(serde_json::json!({
            "translation": [100.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0, 1.0],
        }))
        .expect("placement");
        let before = session.project().revision();
        let value = serde_json::to_value(NodePlacementSetMany {
            placements: vec![
                NodePlacement { node: camera, placement: moved },
                NodePlacement { node: light, placement: moved },
            ],
        })
        .expect("params");
        session.dispatch(NODE_PLACEMENT_SET_MANY_ID, &value).expect("move the camera and the light");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        assert_eq!(session.project().camera(camera).expect("camera").placement(), moved);
        assert_eq!(session.project().light(light).expect("light").placement(), moved);

        session.undo().expect("undo the batch");
        assert_eq!(session.project().camera(camera).expect("camera").placement(), Placement::IDENTITY);
    }
}
