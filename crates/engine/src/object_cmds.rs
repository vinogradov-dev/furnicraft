//! Commands that create free-form geometry objects.
//!
//! A primitive stores the parameters that built it, so a sphere is edited by its radius rather than
//! by rewriting its vertices; the mesh is derived and `object.collapse` bakes it ([ADR 0054]). A
//! constructive object starts from a contour extruded by one feature; its mesh is derived too, and
//! `object.collapse` bakes it ([ADR 0037]). Every command allocates the object id, builds the
//! payload in the object's local frame and places it under a container in one revision.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
//! [ADR 0054]: ../docs/decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md

use furnicraft_mesh::{evaluate_constructive, evaluate_primitive};
use furnicraft_model::{
    Constructive, Contour, FrameAnchor, Geometry, Id, Path, Placement, Primitive, Project, ProjectError,
    rebase_at_anchor,
};
use furnicraft_param::validate_extrusion_contour;
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::node_cmds::{parse, validate_name};
use crate::session::Session;

/// Stable id of the primitive creation command.
pub const OBJECT_PRIMITIVE_CREATE_ID: &str = "object.primitive.create";

/// User-facing label of the primitive creation command.
pub const OBJECT_PRIMITIVE_CREATE_LABEL: &str = "Create primitive";

/// Stable id of the primitive parameter replacement command.
pub const OBJECT_PRIMITIVE_REPLACE_ID: &str = "object.primitive.replace";

/// User-facing label of the primitive parameter replacement command.
pub const OBJECT_PRIMITIVE_REPLACE_LABEL: &str = "Replace primitive parameters";

/// The primitive shapes the command can build, re-exported from the document payload.
pub use furnicraft_model::{MAX_PRIMITIVE_SIZE, PrimitiveKind};

/// Parameters for `object.primitive.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPrimitiveCreate {
    /// Id of the container that receives the new object.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Which primitive to build.
    pub primitive: PrimitiveKind,
    /// Full extent of the shape: the cube's edge, or the diameter of a cylinder or sphere.
    pub size: Length,
    /// Placement relative to the parent container.
    pub placement: Placement,
}

/// The `object.primitive.create` command.
pub(crate) const OBJECT_PRIMITIVE_CREATE: CommandSpec = CommandSpec::new(
    OBJECT_PRIMITIVE_CREATE_ID,
    OBJECT_PRIMITIVE_CREATE_LABEL,
    object_primitive_create_enabled,
    object_primitive_create_run,
);

/// The primitive the creation size builds, or the reason it cannot be built.
fn creation_primitive(kind: PrimitiveKind, size: Length) -> Result<Primitive, String> {
    Primitive::from_kind(kind, size).map_err(|error| error.to_string())
}

fn object_primitive_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPrimitiveCreate>(params, OBJECT_PRIMITIVE_CREATE_ID) else {
        return false;
    };
    !params.name.trim().is_empty()
        && session.project().is_container(params.parent)
        && creation_primitive(params.primitive, params.size).is_ok()
}

fn object_primitive_create_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPrimitiveCreate>(params, OBJECT_PRIMITIVE_CREATE_ID)?;
    validate_name(&params.name, OBJECT_PRIMITIVE_CREATE_ID)?;
    let primitive = creation_primitive(params.primitive, params.size)
        .map_err(|message| CommandError::InvalidParams { command: OBJECT_PRIMITIVE_CREATE_ID, message })?;
    let id = project.add_object(params.parent, params.name, params.placement, Geometry::Primitive(primitive))?;
    Ok(CommandOutcome::created(id))
}

/// Parameters for `object.primitive.replace`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPrimitiveReplace {
    /// The parametric primitive whose parameters change.
    pub object: Id,
    /// The validated parameters to store; the shape kind is part of the payload.
    pub primitive: Primitive,
}

/// The `object.primitive.replace` command.
pub(crate) const OBJECT_PRIMITIVE_REPLACE: CommandSpec = CommandSpec::new(
    OBJECT_PRIMITIVE_REPLACE_ID,
    OBJECT_PRIMITIVE_REPLACE_LABEL,
    object_primitive_replace_enabled,
    object_primitive_replace_run,
);

fn object_primitive_replace_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPrimitiveReplace>(params, OBJECT_PRIMITIVE_REPLACE_ID) else {
        return false;
    };
    session.project().object_primitive(params.object).is_ok()
}

fn object_primitive_replace_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPrimitiveReplace>(params, OBJECT_PRIMITIVE_REPLACE_ID)?;
    project.set_object_primitive(params.object, params.primitive)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the extrusion creation command.
pub const OBJECT_EXTRUDE_CREATE_ID: &str = "object.extrude.create";

/// User-facing label of the extrusion creation command.
pub const OBJECT_EXTRUDE_CREATE_LABEL: &str = "Create extrusion";

/// Parameters for `object.extrude.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectExtrudeCreate {
    /// Id of the container that receives the new object.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Closed flat contour in the frame of `placement`; the command moves it so its creation pivot
    /// becomes the object's local zero.
    pub contour: Contour,
    /// Positive extrusion height along `+Z`.
    pub height: Length,
    /// Placement relative to the parent container.
    pub placement: Placement,
}

/// The `object.extrude.create` command.
pub(crate) const OBJECT_EXTRUDE_CREATE: CommandSpec = CommandSpec::new(
    OBJECT_EXTRUDE_CREATE_ID,
    OBJECT_EXTRUDE_CREATE_LABEL,
    object_extrude_create_enabled,
    object_extrude_create_run,
);

fn valid_extrusion_height(height: Length) -> bool {
    height.is_positive() && height.micrometres() <= furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres()
}

fn object_extrude_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectExtrudeCreate>(params, OBJECT_EXTRUDE_CREATE_ID) else {
        return false;
    };
    !params.name.trim().is_empty()
        && valid_extrusion_height(params.height)
        && session.project().is_container(params.parent)
        && validate_extrusion_contour(&params.contour).is_ok()
}

fn object_extrude_create_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectExtrudeCreate>(params, OBJECT_EXTRUDE_CREATE_ID)?;
    validate_name(&params.name, OBJECT_EXTRUDE_CREATE_ID)?;
    if !valid_extrusion_height(params.height) {
        return Err(CommandError::InvalidParams {
            command: OBJECT_EXTRUDE_CREATE_ID,
            message: format!(
                "height must be positive and at most {} micrometres",
                furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres()
            ),
        });
    }
    evaluate_contour(OBJECT_EXTRUDE_CREATE_ID, &params.contour)?;
    let rebased = rebase_at_anchor(params.contour, params.placement, FrameAnchor::CreationPivot).map_err(|error| {
        CommandError::InvalidParams { command: OBJECT_EXTRUDE_CREATE_ID, message: error.to_string() }
    })?;
    let constructive = Constructive::extrude(rebased.content.into(), params.height).map_err(ProjectError::from)?;
    let id = project.add_object_with_pivot(
        params.parent,
        params.name,
        rebased.placement,
        Geometry::Constructive(constructive),
        rebased.pivot,
    )?;
    Ok(CommandOutcome::created(id))
}

/// Refuses a contour the render mesh cannot hold, before anything is stored.
///
/// A contour is valid as a ring but still too detailed for the one face its extrusion cap becomes;
/// storing it would leave an object that evaluates to an error and draws nothing.
fn evaluate_contour(command: &'static str, contour: &Contour) -> Result<(), CommandError> {
    validate_extrusion_contour(contour)
        .map_err(|error| CommandError::Evaluation { command, message: error.to_string() })
}

/// Stable id of the contour replacement command.
pub const OBJECT_CONTOUR_REPLACE_ID: &str = "object.contour.replace";

/// User-facing label of the contour replacement command.
pub const OBJECT_CONTOUR_REPLACE_LABEL: &str = "Replace contour";

/// Parameters for `object.contour.replace`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectContourReplace {
    /// The constructive object whose path changes.
    pub object: Id,
    /// The new closed path the body owns.
    pub path: Path,
}

/// The `object.contour.replace` command.
pub(crate) const OBJECT_CONTOUR_REPLACE: CommandSpec = CommandSpec::new(
    OBJECT_CONTOUR_REPLACE_ID,
    OBJECT_CONTOUR_REPLACE_LABEL,
    object_contour_replace_enabled,
    object_contour_replace_run,
);

fn object_contour_replace_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectContourReplace>(params, OBJECT_CONTOUR_REPLACE_ID) else {
        return false;
    };
    session.project().object(params.object).is_some_and(|object| object.geometry().as_constructive().is_some())
        && extrusion_path_ready(&params.path).is_ok()
}

fn object_contour_replace_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectContourReplace>(params, OBJECT_CONTOUR_REPLACE_ID)?;
    // Validate the new ring before the document changes: a path that is not a ring or whose cap
    // the mesh cannot hold would leave a body that evaluates to an error and renders nothing.
    let contour = extrusion_path_ready(&params.path)?;
    evaluate_contour(OBJECT_CONTOUR_REPLACE_ID, &contour)?;
    project.set_object_body_path(params.object, params.path)?;
    Ok(CommandOutcome::changed())
}

/// The ring an extrudable path encloses, or the reason the command cannot store it.
///
/// An open chain has no ring, so the refusal names the path rather than a ring rule.
fn extrusion_path_ready(path: &Path) -> Result<Contour, CommandError> {
    path.to_contour().map_err(|error| CommandError::InvalidParams {
        command: OBJECT_CONTOUR_REPLACE_ID,
        message: format!("the body path must be a closed ring: {error}"),
    })
}

/// Stable id of the in-place path extrusion command.
pub const OBJECT_PATH_EXTRUDE_ID: &str = "object.path.extrude";

/// User-facing label of the path extrusion command.
pub const OBJECT_PATH_EXTRUDE_LABEL: &str = "Extrude path";

/// Parameters for `object.path.extrude`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPathExtrude {
    /// The path node that becomes the body.
    pub object: Id,
    /// Positive extrusion height along `+Z`.
    pub height: Length,
}

/// The `object.path.extrude` command.
pub(crate) const OBJECT_PATH_EXTRUDE: CommandSpec = CommandSpec::new(
    OBJECT_PATH_EXTRUDE_ID,
    OBJECT_PATH_EXTRUDE_LABEL,
    object_path_extrude_enabled,
    object_path_extrude_run,
);

fn object_path_extrude_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPathExtrude>(params, OBJECT_PATH_EXTRUDE_ID) else {
        return false;
    };
    valid_extrusion_height(params.height)
        && session
            .project()
            .object_path(params.object)
            .is_ok_and(|path| path.to_contour().is_ok_and(|contour| validate_extrusion_contour(&contour).is_ok()))
}

fn object_path_extrude_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPathExtrude>(params, OBJECT_PATH_EXTRUDE_ID)?;
    if !valid_extrusion_height(params.height) {
        return Err(CommandError::InvalidParams {
            command: OBJECT_PATH_EXTRUDE_ID,
            message: format!(
                "height must be positive and at most {} micrometres",
                furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres()
            ),
        });
    }
    let path = project.object_path(params.object)?;
    let contour = path.to_contour().map_err(|error| CommandError::InvalidParams {
        command: OBJECT_PATH_EXTRUDE_ID,
        message: format!("the path must be a closed ring: {error}"),
    })?;
    evaluate_contour(OBJECT_PATH_EXTRUDE_ID, &contour)?;
    project.extrude_object_path(params.object, params.height)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the extrusion height command.
pub const OBJECT_EXTRUDE_HEIGHT_SET_ID: &str = "object.extrude.height.set";

/// User-facing label of the extrusion height command.
pub const OBJECT_EXTRUDE_HEIGHT_SET_LABEL: &str = "Set extrusion height";

/// Parameters for `object.extrude.height.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectExtrudeHeightSet {
    /// The constructive object whose extrusion height changes.
    pub object: Id,
    /// Positive extrusion height along `+Z`.
    pub height: Length,
}

/// The `object.extrude.height.set` command.
pub(crate) const OBJECT_EXTRUDE_HEIGHT_SET: CommandSpec = CommandSpec::new(
    OBJECT_EXTRUDE_HEIGHT_SET_ID,
    OBJECT_EXTRUDE_HEIGHT_SET_LABEL,
    object_extrude_height_set_enabled,
    object_extrude_height_set_run,
);

fn object_extrude_height_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectExtrudeHeightSet>(params, OBJECT_EXTRUDE_HEIGHT_SET_ID) else {
        return false;
    };
    valid_extrusion_height(params.height)
        && session
            .project()
            .object(params.object)
            .is_some_and(|object| object.geometry().as_constructive().is_some_and(|body| body.height().is_some()))
}

fn object_extrude_height_set_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectExtrudeHeightSet>(params, OBJECT_EXTRUDE_HEIGHT_SET_ID)?;
    if !valid_extrusion_height(params.height) {
        return Err(CommandError::InvalidParams {
            command: OBJECT_EXTRUDE_HEIGHT_SET_ID,
            message: format!(
                "height must be positive and at most {} micrometres",
                furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres()
            ),
        });
    }
    project.set_object_extrusion_height(params.object, params.height)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the path creation command.
pub const OBJECT_PATH_CREATE_ID: &str = "object.path.create";

/// User-facing label of the path creation command.
pub const OBJECT_PATH_CREATE_LABEL: &str = "Create path";

/// Parameters for `object.path.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPathCreate {
    /// Id of the container that receives the new object.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Open or closed chain of line and arc segments, in the frame of `placement`; the command moves
    /// it so its creation pivot becomes the object's local zero.
    pub path: Path,
    /// Placement relative to the parent container.
    pub placement: Placement,
}

/// The `object.path.create` command.
pub(crate) const OBJECT_PATH_CREATE: CommandSpec = CommandSpec::new(
    OBJECT_PATH_CREATE_ID,
    OBJECT_PATH_CREATE_LABEL,
    object_path_create_enabled,
    object_path_create_run,
);

fn object_path_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPathCreate>(params, OBJECT_PATH_CREATE_ID) else {
        return false;
    };
    !params.name.trim().is_empty() && session.project().is_container(params.parent)
}

fn object_path_create_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPathCreate>(params, OBJECT_PATH_CREATE_ID)?;
    validate_name(&params.name, OBJECT_PATH_CREATE_ID)?;
    let rebased = rebase_at_anchor(params.path, params.placement, FrameAnchor::CreationPivot)
        .map_err(|error| CommandError::InvalidParams { command: OBJECT_PATH_CREATE_ID, message: error.to_string() })?;
    let id = project.add_object_with_pivot(
        params.parent,
        params.name,
        rebased.placement,
        Geometry::Path(rebased.content),
        rebased.pivot,
    )?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the path replacement command.
pub const OBJECT_PATH_REPLACE_ID: &str = "object.path.replace";

/// User-facing label of the path replacement command.
pub const OBJECT_PATH_REPLACE_LABEL: &str = "Replace path";

/// Parameters for `object.path.replace`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPathReplace {
    /// The path object whose drawing changes.
    pub object: Id,
    /// The new open or closed chain.
    pub path: Path,
}

/// The `object.path.replace` command.
pub(crate) const OBJECT_PATH_REPLACE: CommandSpec = CommandSpec::new(
    OBJECT_PATH_REPLACE_ID,
    OBJECT_PATH_REPLACE_LABEL,
    object_path_replace_enabled,
    object_path_replace_run,
);

fn object_path_replace_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPathReplace>(params, OBJECT_PATH_REPLACE_ID) else {
        return false;
    };
    session.project().object_path(params.object).is_ok()
}

fn object_path_replace_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPathReplace>(params, OBJECT_PATH_REPLACE_ID)?;
    project.set_object_path(params.object, params.path)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the collapse command.
pub const OBJECT_COLLAPSE_ID: &str = "object.collapse";

/// User-facing label of the collapse command.
pub const OBJECT_COLLAPSE_LABEL: &str = "Collapse to mesh";

/// Parameters for `object.collapse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectCollapse {
    /// The constructive or primitive object to bake.
    pub object: Id,
}

/// The `object.collapse` command.
pub(crate) const OBJECT_COLLAPSE: CommandSpec =
    CommandSpec::new(OBJECT_COLLAPSE_ID, OBJECT_COLLAPSE_LABEL, object_collapse_enabled, object_collapse_run);

/// Whether a geometry payload is derived from parameters and can be baked to a mesh.
fn is_derived(geometry: &Geometry) -> bool {
    matches!(geometry, Geometry::Constructive(_) | Geometry::Primitive(_))
}

fn object_collapse_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectCollapse>(params, OBJECT_COLLAPSE_ID) else {
        return false;
    };
    session.project().object(params.object).is_some_and(|object| is_derived(object.geometry()))
}

fn object_collapse_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectCollapse>(params, OBJECT_COLLAPSE_ID)?;
    let geometry = project
        .object(params.object)
        .map(|object| object.geometry())
        .ok_or(ProjectError::NodeNotFound { id: params.object })?;
    let evaluation = |message: String| CommandError::Evaluation { command: OBJECT_COLLAPSE_ID, message };
    let mesh = match geometry {
        Geometry::Constructive(constructive) => {
            evaluate_constructive(constructive).map_err(|error| evaluation(error.to_string()))?
        }
        Geometry::Primitive(primitive) => {
            evaluate_primitive(primitive).map_err(|error| evaluation(error.to_string()))?
        }
        Geometry::Mesh(_) | Geometry::Path(_) => {
            return Err(ProjectError::NotDerivedObject { id: params.object }.into());
        }
    };
    project.collapse_object(params.object, mesh)?;
    Ok(CommandOutcome::changed())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{ContourSegment, DEFAULT_BANDS, DEFAULT_SEGMENTS, EulerAngles, Pivot, Revision};
    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_units::Degrees;

    use crate::CommandRegistry;

    use super::*;

    fn cube_params(session: &Session, size: Length) -> Value {
        serde_json::to_value(ObjectPrimitiveCreate {
            parent: session.project().root().id(),
            name: "Cube".to_owned(),
            primitive: PrimitiveKind::Cube,
            size,
            placement: Placement::IDENTITY,
        })
        .expect("params serialize")
    }

    #[test]
    fn dispatch_stores_the_recipe_in_one_revision() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let params = cube_params(&session, Length::from_millimetres(100).expect("length"));

        let outcome = session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &params).expect("create cube");

        let id = outcome.created_id().expect("the command returns the new object id");
        assert_eq!(session.project().revision().get(), 1);
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Cube");
        let primitive = object.geometry().as_primitive().expect("a parametric payload");
        assert_eq!(primitive, &Primitive::cube(Length::from_millimetres(100).expect("length")).expect("cube"));
        assert!(object.geometry().as_mesh().is_none(), "the mesh is derived, not stored");
        assert!(session.can_undo());

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2, "undo advances the revision to invalidate caches");
        assert!(session.project().object(id).is_none());
    }

    #[test]
    fn every_primitive_kind_stores_its_parameters() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let size = Length::from_millimetres(100).expect("length");
        let cases = [
            (PrimitiveKind::Cube, 8, 6),
            (PrimitiveKind::Cylinder, DEFAULT_SEGMENTS as usize * 2, DEFAULT_SEGMENTS as usize + 2),
            (
                PrimitiveKind::Sphere,
                2 + DEFAULT_SEGMENTS as usize * (DEFAULT_BANDS as usize - 1),
                DEFAULT_SEGMENTS as usize * DEFAULT_BANDS as usize,
            ),
        ];
        for (primitive, vertices, faces) in cases {
            let params = serde_json::to_value(ObjectPrimitiveCreate {
                parent: session.project().root().id(),
                name: "Primitive".to_owned(),
                primitive,
                size,
                placement: Placement::IDENTITY,
            })
            .expect("params serialize");
            let outcome = session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &params).expect("create the primitive");
            let id = outcome.created_id().expect("an id");
            let object = session.project().object(id).expect("object");
            let stored = object.geometry().as_primitive().expect("a parametric payload");
            assert_eq!(stored.kind(), primitive, "{primitive:?} keeps its shape");
            assert_eq!(stored.mesh_counts(), (vertices, faces), "{primitive:?} counts");
            assert_eq!(stored.bounds_micrometres(), ([-50_000; 3], [50_000; 3]), "{primitive:?} bounds");
        }
        assert_eq!(session.project().revision().get(), 3, "one command per primitive");
    }

    #[test]
    fn a_size_outside_the_limit_is_rejected_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let too_large = MAX_PRIMITIVE_SIZE.checked_add(Length::from_micrometres(1)).expect("sum");
        for size in [Length::ZERO, Length::from_micrometres(1), too_large] {
            let params = cube_params(&session, size);
            let registry = CommandRegistry::new();
            let spec = registry.get(OBJECT_PRIMITIVE_CREATE_ID).expect("registered");
            assert!(!spec.enabled(&session, &params), "size {size} is not creatable");
            assert!(session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &params).is_err(), "size {size} must be rejected");
        }
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.can_undo());
    }

    #[test]
    fn a_blank_name_is_rejected_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let value = serde_json::to_value(ObjectPrimitiveCreate {
            parent: session.project().root().id(),
            name: "  ".to_owned(),
            primitive: PrimitiveKind::Cube,
            size: Length::from_millimetres(100).expect("length"),
            placement: Placement::IDENTITY,
        })
        .expect("params serialize");
        assert!(session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &value).is_err());
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn an_unknown_parent_is_rejected_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let value = serde_json::to_value(ObjectPrimitiveCreate {
            parent: Id::new(99).expect("id"),
            name: "Cube".to_owned(),
            primitive: PrimitiveKind::Cube,
            size: Length::from_millimetres(100).expect("length"),
            placement: Placement::IDENTITY,
        })
        .expect("params serialize");
        assert!(session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &value).is_err());
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn replacing_the_parameters_is_one_revision_and_one_undo() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(OBJECT_PRIMITIVE_CREATE_ID, &cube_params(&session, Length::from_millimetres(100).expect("size")))
            .expect("create")
            .created_id()
            .expect("id");
        let original = Primitive::cube(Length::from_millimetres(100).expect("size")).expect("cube");
        assert_eq!(session.project().object_primitive(id), Ok(&original));

        let smoother = Primitive::sphere(Length::from_millimetres(60).expect("radius"), 24, 12).expect("sphere");
        let value = serde_json::to_value(ObjectPrimitiveReplace { object: id, primitive: smoother.clone() })
            .expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_PRIMITIVE_REPLACE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));
        let before = session.project().revision();
        session.dispatch(OBJECT_PRIMITIVE_REPLACE_ID, &value).expect("replace the parameters");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"), "one revision per edit");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Cube", "only the parameters change");
        assert_eq!(object.geometry().as_primitive(), Some(&smoother));

        session.undo().expect("one undo restores the parameters");
        assert_eq!(session.project().object_primitive(id), Ok(&original));
        assert!(session.can_undo(), "the creation is still one entry below");
    }

    #[test]
    fn a_primitive_collapses_to_exactly_the_mesh_the_evaluator_produces() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let size = Length::from_millimetres(80).expect("size");
        let value = serde_json::to_value(ObjectPrimitiveCreate {
            parent: session.project().root().id(),
            name: "Ball".to_owned(),
            primitive: PrimitiveKind::Sphere,
            size,
            placement: Placement::IDENTITY,
        })
        .expect("params serialize");
        let id = session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &value).expect("create").created_id().expect("id");
        let primitive = session.project().object_primitive(id).expect("the parameters").clone();
        let evaluated = furnicraft_mesh::evaluate_primitive(&primitive).expect("evaluated mesh");

        let value = serde_json::to_value(ObjectCollapse { object: id }).expect("params");
        let spec = CommandRegistry::new().get(OBJECT_COLLAPSE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));
        session.dispatch(OBJECT_COLLAPSE_ID, &value).expect("collapse");
        let baked = session.project().object(id).expect("object").geometry().as_mesh().expect("a baked mesh").clone();
        assert_eq!(baked, evaluated, "the viewport and the collapse share one evaluator");
        assert_eq!(session.project().object(id).expect("object").name(), "Ball", "collapse keeps every other field");
        assert!(session.project().object_primitive(id).is_err(), "a baked mesh has no parameters to edit");

        let replacement = Primitive::sphere(Length::from_millimetres(10).expect("radius"), 8, 4).expect("sphere");
        let value =
            serde_json::to_value(ObjectPrimitiveReplace { object: id, primitive: replacement }).expect("params");
        let spec = CommandRegistry::new().get(OBJECT_PRIMITIVE_REPLACE_ID).expect("registered");
        assert!(!spec.enabled(&session, &value), "a baked mesh has no parameters");
        assert!(session.dispatch(OBJECT_PRIMITIVE_REPLACE_ID, &value).is_err());
    }

    #[test]
    fn a_primitive_replace_on_another_payload_is_refused_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let before = session.project().revision();
        let value = serde_json::to_value(ObjectPrimitiveReplace {
            object: id,
            primitive: Primitive::cube(Length::from_millimetres(20).expect("size")).expect("cube"),
        })
        .expect("params");
        let spec = CommandRegistry::new().get(OBJECT_PRIMITIVE_REPLACE_ID).expect("registered");
        assert!(!spec.enabled(&session, &value));
        assert!(session.dispatch(OBJECT_PRIMITIVE_REPLACE_ID, &value).is_err());
        assert_eq!(session.project().revision(), before);
        assert!(session.project().object(id).expect("object").geometry().as_constructive().is_some());
    }

    fn rectangle(width: i64, height: i64) -> Contour {
        let millimetres = |value| Length::from_millimetres(value).expect("length");
        Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(width), millimetres(0)],
            [millimetres(width), millimetres(height)],
            [millimetres(0), millimetres(height)],
        ])
        .expect("rectangle")
    }
    fn extrude_params(session: &Session, name: &str, height: Length) -> Value {
        serde_json::to_value(ObjectExtrudeCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            contour: rectangle(100, 50),
            height,
            placement: Placement::IDENTITY,
        })
        .expect("params serialize")
    }

    /// A "C" band: an outer and an inner arc joined by two radial lines.
    ///
    /// The ring is valid and simple and its arcs are metres long, which is the shape that used to
    /// sample past one extrusion cap (X2).
    fn band_contour() -> Contour {
        let centre = [Length::ZERO, Length::ZERO];
        let start = Degrees::from_millidegrees(0);
        let sweep = Degrees::from_millidegrees(350_000);
        let outer = ContourSegment::arc(centre, Length::from_millimetres(5_000).expect("radius"), start, sweep)
            .expect("outer arc");
        let inner = ContourSegment::arc(
            centre,
            Length::from_millimetres(4_000).expect("radius"),
            sweep,
            Degrees::from_millidegrees(-350_000),
        )
        .expect("inner arc");
        let inward = ContourSegment::line(outer.end().expect("outer end"), inner.start().expect("inner start"))
            .expect("inward line");
        let outward = ContourSegment::line(inner.end().expect("inner end"), outer.start().expect("outer start"))
            .expect("outward line");
        Contour::new(vec![outer, inward, inner, outward]).expect("a simple band")
    }

    #[test]
    fn a_large_legal_ring_is_created_replaced_and_drawn() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let registry = CommandRegistry::new();
        let height = Length::from_millimetres(18).expect("height");
        let value = serde_json::to_value(ObjectExtrudeCreate {
            parent: session.project().root().id(),
            name: "Band".to_owned(),
            contour: band_contour(),
            height,
            placement: Placement::IDENTITY,
        })
        .expect("params serialize");
        let create = registry.get(OBJECT_EXTRUDE_CREATE_ID).expect("registered");
        assert!(create.enabled(&session, &value), "a legal ring is creatable whatever its radius");
        let id = session.dispatch(OBJECT_EXTRUDE_CREATE_ID, &value).expect("create the band").created_id().expect("id");

        let replace = serde_json::to_value(ObjectContourReplace { object: id, path: band_contour().into() })
            .expect("params serialize");
        let spec = registry.get(OBJECT_CONTOUR_REPLACE_ID).expect("registered");
        assert!(spec.enabled(&session, &replace), "the replacement ring is buildable too");
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &replace).expect("replace the contour");

        // The object draws: the viewport evaluator builds it and the cap is one face inside the
        // document limit.
        let object = session.project().object(id).expect("the band exists");
        let constructive = object.geometry().as_constructive().expect("a constructive object");
        let mesh = furnicraft_param::build_constructive_mesh(constructive).expect("the band draws");
        let cap = mesh.faces().first().expect("a cap");
        assert!(cap.len() > 3 && cap.len() <= furnicraft_model::MAX_FACE_VERTICES, "cap holds {}", cap.len());
    }

    #[test]
    fn a_contour_replacement_is_one_revision_and_one_undo() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("a new object id");
        let contour = |session: &Session| {
            session
                .project()
                .object(id)
                .and_then(|object| object.geometry().as_constructive())
                .and_then(|body| body.contour().ok())
        };
        let original = contour(&session).expect("the stored ring");
        let wider = rectangle(120, 50);
        let value = serde_json::to_value(ObjectContourReplace { object: id, path: wider.clone().into() })
            .expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_CONTOUR_REPLACE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));
        let before = session.project().revision();
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).expect("replace the contour");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        assert_eq!(contour(&session), Some(wider), "the new ring is stored");
        session.undo().expect("one undo restores the old ring");
        assert_eq!(contour(&session), Some(original));
        session.undo().expect("the replacement was one entry");
        assert!(session.project().object(id).is_none());
        assert!(!session.can_undo());
    }

    #[test]
    fn an_extrusion_is_created_rebuilt_and_collapsed_one_revision_at_a_time() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let height = Length::from_millimetres(18).expect("height");
        let id = session
            .dispatch(OBJECT_EXTRUDE_CREATE_ID, &extrude_params(&session, "Top", height))
            .expect("create extrusion")
            .created_id()
            .expect("a new object id");
        assert_eq!(session.project().revision().get(), 1);
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Top");
        let constructive = object.geometry().as_constructive().expect("a constructive payload");
        assert_eq!(constructive.height(), Some(height));
        assert!(object.geometry().as_mesh().is_none(), "a constructive object has no mesh in the document");

        let wider = rectangle(120, 50);
        let value =
            serde_json::to_value(ObjectContourReplace { object: id, path: wider.clone().into() }).expect("params");
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).expect("replace the contour");
        assert_eq!(session.project().revision().get(), 2);
        let object = session.project().object(id).expect("object");
        assert_eq!(
            object.geometry().as_constructive().expect("constructive").contour(),
            Ok(wider.clone()),
            "the name, placement and height survive a contour edit"
        );

        let value = serde_json::to_value(ObjectCollapse { object: id }).expect("params");
        session.dispatch(OBJECT_COLLAPSE_ID, &value).expect("collapse");
        assert_eq!(session.project().revision().get(), 3);
        let object = session.project().object(id).expect("object");
        let mesh = object.geometry().as_mesh().expect("the collapsed payload is a mesh");
        assert_eq!(mesh.positions().len(), 8);
        assert_eq!(mesh.faces().len(), 6);
        assert!(object.geometry().as_constructive().is_none());
        assert_eq!(object.name(), "Top", "collapse keeps every other field");

        assert!(session.dispatch(OBJECT_COLLAPSE_ID, &value).is_err(), "a mesh object cannot collapse again");
        session.undo().expect("undo the collapse");
        assert!(
            session.project().object(id).expect("object").geometry().as_constructive().is_some(),
            "undo restores the history"
        );
    }

    #[test]
    fn a_collapse_bakes_exactly_the_mesh_the_evaluator_produces() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let height = Length::from_millimetres(18).expect("height");
        let id = session
            .dispatch(OBJECT_EXTRUDE_CREATE_ID, &extrude_params(&session, "Top", height))
            .expect("create extrusion")
            .created_id()
            .expect("a new object id");
        let constructive = session
            .project()
            .object(id)
            .expect("object")
            .geometry()
            .as_constructive()
            .expect("a constructive payload")
            .clone();
        let evaluated = furnicraft_mesh::evaluate_constructive(&constructive).expect("evaluated mesh");

        let value = serde_json::to_value(ObjectCollapse { object: id }).expect("params");
        session.dispatch(OBJECT_COLLAPSE_ID, &value).expect("collapse");
        let baked = session.project().object(id).expect("object").geometry().as_mesh().expect("a baked mesh").clone();
        assert_eq!(baked, evaluated, "the viewport and the collapse share one evaluator");
    }

    #[test]
    fn a_bad_height_or_a_mesh_object_is_refused_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let value = extrude_params(&session, "Top", Length::ZERO);
        assert!(session.dispatch(OBJECT_EXTRUDE_CREATE_ID, &value).is_err(), "a zero height is not a solid");
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.can_undo());

        let cube = session
            .dispatch(OBJECT_PRIMITIVE_CREATE_ID, &cube_params(&session, Length::from_millimetres(10).expect("size")))
            .expect("a cube to compare kinds")
            .created_id()
            .expect("id");
        let value = serde_json::to_value(ObjectContourReplace { object: cube, path: rectangle(10, 10).into() })
            .expect("params");
        assert!(session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).is_err());
        let value = serde_json::to_value(ObjectCollapse { object: cube }).expect("params");
        session.dispatch(OBJECT_COLLAPSE_ID, &value).expect("a primitive collapses to its derived mesh");
        assert!(session.dispatch(OBJECT_COLLAPSE_ID, &value).is_err(), "a baked mesh cannot collapse again");

        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(5).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let value = serde_json::to_value(crate::MeshVerticesMove {
            object: id,
            moves: vec![crate::MeshVertexMove { vertex: 0, position: [Length::ZERO, Length::ZERO, Length::ZERO] }],
        })
        .expect("params");
        assert!(
            session.dispatch(crate::MESH_VERTICES_MOVE_ID, &value).is_err(),
            "a constructive object has no editable mesh before collapse"
        );
    }

    /// An open two-segment drawing, so replacing it is visible in the stored value.
    fn open_path(end: i64) -> Path {
        let millimetres = |value| Length::from_millimetres(value).expect("length");
        Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(end), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(end), millimetres(0)], [millimetres(end), millimetres(40)])
                    .expect("line"),
            ],
            false,
        )
        .expect("open path")
    }

    fn path_create_params(session: &Session, name: &str, path: Path) -> Value {
        serde_json::to_value(ObjectPathCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            path,
            placement: Placement::IDENTITY,
        })
        .expect("params serialize")
    }

    #[test]
    fn a_path_is_created_in_one_revision_and_undone_in_one_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let path = open_path(100);
        let value = path_create_params(&session, "Sketch", path.clone());
        let spec = CommandRegistry::new().get(OBJECT_PATH_CREATE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));

        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &value)
            .expect("create the drawing")
            .created_id()
            .expect("a new object id");
        assert_eq!(session.project().revision().get(), 1);
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Sketch");
        assert_eq!(session.project().object_path(id), Ok(&path));
        assert!(object.geometry().as_mesh().is_none(), "a path is not a solid");

        session.undo().expect("undo the creation");
        assert!(session.project().object(id).is_none());
        assert!(!session.can_undo());
    }

    #[test]
    fn a_path_replacement_is_one_revision_and_one_undo() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let original = open_path(100);
        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Sketch", original.clone()))
            .expect("create the drawing")
            .created_id()
            .expect("id");

        let replacement = open_path(120);
        let value = serde_json::to_value(ObjectPathReplace { object: id, path: replacement.clone() })
            .expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_PATH_REPLACE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));

        let before = session.project().revision();
        session.dispatch(OBJECT_PATH_REPLACE_ID, &value).expect("replace the drawing");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        assert_eq!(session.project().object_path(id), Ok(&replacement));
        assert_eq!(session.project().object(id).expect("object").name(), "Sketch", "only the drawing changes");

        session.undo().expect("one undo restores the first drawing");
        assert_eq!(session.project().object_path(id), Ok(&original));
        assert!(session.can_undo(), "the creation is still one entry below the replacement");
    }

    #[test]
    fn extruding_a_path_turns_the_node_into_the_body_in_one_revision() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let millimetres = |value| Length::from_millimetres(value).expect("length");
        let closed =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(50)]).expect("rectangle");
        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Plate", closed.clone()))
            .expect("create the drawing")
            .created_id()
            .expect("id");
        let stored = session.project().object_path(id).expect("the drawn path").clone();
        let frame = session.project().object(id).map(|object| (object.placement(), object.pivot())).expect("object");
        let children = session.project().root().children().len();

        let value =
            serde_json::to_value(ObjectPathExtrude { object: id, height: millimetres(18) }).expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_PATH_EXTRUDE_ID).expect("registered");
        assert!(spec.enabled(&session, &value));
        let before = session.project().revision();
        session.dispatch(OBJECT_PATH_EXTRUDE_ID, &value).expect("extrude in place");

        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        let object = session.project().object(id).expect("the node stays");
        assert_eq!(object.name(), "Plate", "the node keeps its name");
        let body = object.geometry().as_constructive().expect("the drawing is the body now");
        assert_eq!(body.path(), &stored, "the body owns the path that was drawn");
        assert_eq!(body.height(), Some(millimetres(18)));
        assert_eq!((object.placement(), object.pivot()), frame, "extruding in place leaves the frame alone");
        assert_eq!(session.project().root().children().len(), children, "no sibling body appears");
        assert!(session.project().object_path(id).is_err(), "the drawing is gone");
        assert!(!spec.enabled(&session, &value), "a body is not extruded a second time");

        session.undo().expect("one undo restores the drawing");
        assert_eq!(session.project().object_path(id), Ok(&stored));
    }

    #[test]
    fn an_open_path_cannot_be_extruded() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Sketch", open_path(100)))
            .expect("create the drawing")
            .created_id()
            .expect("id");
        let value = serde_json::to_value(ObjectPathExtrude {
            object: id,
            height: Length::from_millimetres(18).expect("height"),
        })
        .expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_PATH_EXTRUDE_ID).expect("registered");
        assert!(!spec.enabled(&session, &value), "an open chain encloses no ring");
        let before = session.project().revision();
        assert!(session.dispatch(OBJECT_PATH_EXTRUDE_ID, &value).is_err());
        assert_eq!(session.project().revision(), before);
        assert!(session.project().object_path(id).is_ok(), "the drawing is untouched");
    }

    #[test]
    fn the_extrusion_height_command_keeps_the_path() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let path = session
            .project()
            .object(id)
            .and_then(|object| object.geometry().as_constructive())
            .map(|body| body.path().clone());
        let path = path.expect("the body owns a path");

        let value = serde_json::to_value(ObjectExtrudeHeightSet {
            object: id,
            height: Length::from_millimetres(30).expect("height"),
        })
        .expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_EXTRUDE_HEIGHT_SET_ID).expect("registered");
        assert!(spec.enabled(&session, &value));
        let before = session.project().revision();
        session.dispatch(OBJECT_EXTRUDE_HEIGHT_SET_ID, &value).expect("raise the body");

        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        let body = session.project().object(id).and_then(|object| object.geometry().as_constructive()).expect("a body");
        assert_eq!(body.height(), Some(Length::from_millimetres(30).expect("height")));
        assert_eq!(body.path(), &path, "the height change keeps the path");

        let bad = serde_json::to_value(ObjectExtrudeHeightSet { object: id, height: Length::ZERO }).expect("params");
        assert!(!spec.enabled(&session, &bad), "a height must be positive");
    }

    #[test]
    fn a_body_path_replacement_keeps_every_other_field() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let wider = Path::rectangle(
            [Length::ZERO, Length::ZERO],
            [Length::from_millimetres(120).expect("length"), Length::from_millimetres(50).expect("length")],
        )
        .expect("rectangle");
        let value = serde_json::to_value(ObjectContourReplace { object: id, path: wider.clone() }).expect("params");
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).expect("replace the path");

        let body = session.project().object(id).and_then(|object| object.geometry().as_constructive()).expect("a body");
        assert_eq!(body.path(), &wider, "the body stores the new path");
        assert_eq!(body.height(), Some(Length::from_millimetres(18).expect("height")), "the height survives");
        assert_eq!(
            body.contour().expect("ring").bounds_micrometres(),
            ([0, 0], [120_000, 50_000]),
            "the body draws the new ring"
        );
    }

    #[test]
    fn a_created_drawing_is_born_at_its_own_zero() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let millimetres = |value| Length::from_millimetres(value).expect("length");

        let closed = Path::rectangle([millimetres(600), millimetres(400)], [millimetres(700), millimetres(500)])
            .expect("rectangle");
        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Frame", closed))
            .expect("create the drawing")
            .created_id()
            .expect("id");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.placement().translation().x, 650.0, "the ring's centre is the translation");
        assert_eq!(object.placement().translation().y, 450.0);
        assert_eq!(session.project().pivot(id), Some(Pivot::ZERO), "the creation pivot is the local zero");
        assert_eq!(
            session.project().object_path(id).expect("path").bounds_micrometres(),
            ([-50_000, -50_000], [50_000, 50_000]),
            "a closed ring straddles its own zero"
        );

        let open =
            Path::polyline(vec![[millimetres(600), millimetres(400)], [millimetres(700), millimetres(450)]], false)
                .expect("open chain");
        let id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Line", open))
            .expect("create the line")
            .created_id()
            .expect("id");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.placement().translation().x, 600.0, "the first point is the translation");
        assert_eq!(object.placement().translation().y, 400.0);
        assert_eq!(session.project().pivot(id), Some(Pivot::ZERO));
        assert_eq!(
            session.project().object_path(id).expect("path").bounds_micrometres().0,
            [0, 0],
            "an open chain's first point is the local zero"
        );
    }

    #[test]
    fn an_extruded_body_is_born_at_its_rings_centre() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.placement().translation().x, 50.0, "the ring's centre is the translation");
        assert_eq!(object.placement().translation().y, 25.0);
        assert_eq!(session.project().pivot(id), Some(Pivot::ZERO), "the creation pivot is the local zero");
        let body = object.geometry().as_constructive().expect("a body");
        assert_eq!(
            body.contour().expect("ring").bounds_micrometres(),
            ([-50_000, -25_000], [50_000, 25_000]),
            "the body's ring straddles the local zero"
        );
    }

    #[test]
    fn a_drawing_in_a_moved_container_lands_where_it_was_clicked() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let millimetres = |value| Length::from_millimetres(value).expect("length");
        // A container turned about Z and moved, so a parent-frame placement is not a world one.
        let parent_world = Placement::IDENTITY
            .with_translation(millimetres(-320), millimetres(150), millimetres(700))
            .expect("translation")
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(40_000), ..EulerAngles::default() })
            .expect("rotation");
        let parent = {
            let params = serde_json::to_value(crate::AssemblyCreate {
                parent: root,
                name: "Frame".to_owned(),
                placement: parent_world,
            })
            .expect("params serialize");
            session
                .dispatch(crate::node_cmds::ASSEMBLY_CREATE_ID, &params)
                .expect("create the container")
                .created_id()
                .expect("the container id")
        };
        // The shell hands over the work plane's world frame; the command must receive the parent-frame one.
        let world = Placement::IDENTITY
            .with_translation(millimetres(200), millimetres(100), millimetres(900))
            .expect("work plane");
        let local = Placement::compose(parent_world.inverse().expect("inverse"), world).expect("parent frame");
        let line =
            Path::polyline(vec![[millimetres(600), millimetres(400)], [millimetres(700), millimetres(450)]], false)
                .expect("open chain");
        let value =
            serde_json::to_value(ObjectPathCreate { parent, name: "Line".to_owned(), path: line, placement: local })
                .expect("params serialize");

        let revision = session.project().revision();
        let id = session.dispatch(OBJECT_PATH_CREATE_ID, &value).expect("create the drawing").created_id().expect("id");

        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        let object = session.project().object(id).expect("object");
        assert!(!object.placement().same_transform(world), "the stored placement is relative to the parent");
        let world_placement = session.project().world_placement(id).expect("world placement");
        let clicked = Placement::IDENTITY
            .with_translation(millimetres(800), millimetres(500), millimetres(900))
            .expect("the clicked point in world space");
        assert!(
            (world_placement.translation() - clicked.translation()).length() < 1e-9,
            "the first clicked point is still on the pointer: {:?}",
            world_placement.translation()
        );
        assert!(world_placement.rotation().dot(Placement::IDENTITY.rotation()).abs() >= 1.0 - 1e-12);
        assert_eq!(session.project().pivot(id), Some(Pivot::ZERO));
        assert_eq!(session.project().object_path(id).expect("path").bounds_micrometres().0, [0, 0]);

        session.undo().expect("one undo removes the drawing");
        assert!(session.project().object(id).is_none());
        assert!(session.project().container(parent).is_some(), "the container is its own undo entry");
    }

    #[test]
    fn a_geometry_replacement_leaves_the_creation_pivot_alone() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let millimetres = |value| Length::from_millimetres(value).expect("length");

        let closed =
            Path::rectangle([millimetres(100), millimetres(50)], [millimetres(0), millimetres(0)]).expect("rectangle");
        let path_id = session
            .dispatch(OBJECT_PATH_CREATE_ID, &path_create_params(&session, "Frame", closed))
            .expect("create the drawing")
            .created_id()
            .expect("id");
        let path_pivot = session.project().pivot(path_id).expect("pivot");
        let replacement = Path::polyline(
            vec![[millimetres(1_000), millimetres(1_000)], [millimetres(2_000), millimetres(1_000)]],
            false,
        )
        .expect("open chain");
        let value =
            serde_json::to_value(ObjectPathReplace { object: path_id, path: replacement }).expect("params serialize");
        session.dispatch(OBJECT_PATH_REPLACE_ID, &value).expect("replace the drawing");
        assert_eq!(session.project().pivot(path_id), Some(path_pivot), "a path edit never moves the pivot");

        let body_id = session
            .dispatch(
                OBJECT_EXTRUDE_CREATE_ID,
                &extrude_params(&session, "Top", Length::from_millimetres(18).expect("height")),
            )
            .expect("create extrusion")
            .created_id()
            .expect("id");
        let body_pivot = session.project().pivot(body_id).expect("pivot");
        let value = serde_json::to_value(ObjectContourReplace { object: body_id, path: rectangle(200, 100).into() })
            .expect("params serialize");
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).expect("replace the contour");
        assert_eq!(session.project().pivot(body_id), Some(body_pivot), "a contour edit never moves the pivot");
    }

    #[test]
    fn a_blank_name_or_an_unknown_parent_is_refused_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let registry = CommandRegistry::new();
        let spec = registry.get(OBJECT_PATH_CREATE_ID).expect("registered");

        let blank = path_create_params(&session, "  ", open_path(100));
        assert!(!spec.enabled(&session, &blank));
        assert!(session.dispatch(OBJECT_PATH_CREATE_ID, &blank).is_err());

        let unknown = serde_json::to_value(ObjectPathCreate {
            parent: Id::new(99).expect("id"),
            name: "Sketch".to_owned(),
            path: open_path(100),
            placement: Placement::IDENTITY,
        })
        .expect("params");
        assert!(!spec.enabled(&session, &unknown));
        assert!(session.dispatch(OBJECT_PATH_CREATE_ID, &unknown).is_err());

        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.can_undo(), "a refused create leaves no undo entry");
    }

    #[test]
    fn a_path_replace_on_a_mesh_object_is_refused_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let cube = session
            .dispatch(OBJECT_PRIMITIVE_CREATE_ID, &cube_params(&session, Length::from_millimetres(10).expect("size")))
            .expect("a cube")
            .created_id()
            .expect("id");
        let value =
            serde_json::to_value(ObjectPathReplace { object: cube, path: open_path(100) }).expect("params serialize");
        let spec = CommandRegistry::new().get(OBJECT_PATH_REPLACE_ID).expect("registered");
        assert!(!spec.enabled(&session, &value));
        assert!(session.dispatch(OBJECT_PATH_REPLACE_ID, &value).is_err());

        assert_eq!(session.project().revision().get(), 1, "the cube's own revision is the only one");
        assert!(session.project().object_path(cube).is_err(), "a mesh object stores no drawing");
    }
}
