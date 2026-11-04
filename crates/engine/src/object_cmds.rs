//! Commands that create free-form geometry objects.
//!
//! A primitive is a baked mesh from the start: it has no feature history, and undo is its only
//! record ([ADR 0037]). A constructive object starts from a contour extruded by one feature; its
//! mesh is derived, and `object.collapse` bakes it ([ADR 0037]). Every command allocates the
//! object id, builds the payload in the object's local frame and places it under a container in
//! one revision.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use furnicraft_model::{Constructive, Contour, Geometry, Id, Mesh, Path, Placement, Project, ProjectError};
use furnicraft_param::{build_constructive_mesh, validate_extrusion_contour};
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

/// Largest accepted primitive size: ten metres, the same bound the panel rules use.
pub const MAX_PRIMITIVE_SIZE: Length = Length::from_micrometres(10_000_000);

/// The primitive kinds the command can build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind {
    /// A cube centred on the object's local origin.
    Cube,
}

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
    /// Edge length of the cube, positive and at most [`MAX_PRIMITIVE_SIZE`].
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

fn valid_size(size: Length) -> bool {
    size.micrometres() > 0 && size.micrometres() <= MAX_PRIMITIVE_SIZE.micrometres()
}

fn object_primitive_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectPrimitiveCreate>(params, OBJECT_PRIMITIVE_CREATE_ID) else {
        return false;
    };
    !params.name.trim().is_empty() && valid_size(params.size) && session.project().is_container(params.parent)
}

fn object_primitive_create_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPrimitiveCreate>(params, OBJECT_PRIMITIVE_CREATE_ID)?;
    validate_name(&params.name, OBJECT_PRIMITIVE_CREATE_ID)?;
    if !valid_size(params.size) {
        return Err(CommandError::InvalidParams {
            command: OBJECT_PRIMITIVE_CREATE_ID,
            message: format!("size must be positive and at most {} micrometres", MAX_PRIMITIVE_SIZE.micrometres()),
        });
    }
    let mesh = match params.primitive {
        PrimitiveKind::Cube => Mesh::cube(params.size).map_err(ProjectError::from)?,
    };
    let id = project.add_object(params.parent, params.name, params.placement, Geometry::Mesh(mesh))?;
    Ok(CommandOutcome::created(id))
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
    /// Closed flat contour in the object's local XY plane.
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
    let constructive = Constructive::extrude(params.contour, params.height).map_err(ProjectError::from)?;
    let id = project.add_object(params.parent, params.name, params.placement, Geometry::Constructive(constructive))?;
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
    /// The constructive object whose contour changes.
    pub object: Id,
    /// The new closed flat contour.
    pub contour: Contour,
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
        && validate_extrusion_contour(&params.contour).is_ok()
}

fn object_contour_replace_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectContourReplace>(params, OBJECT_CONTOUR_REPLACE_ID)?;
    // Validate the new ring before the document changes: a contour the cap cannot hold would
    // leave a constructive object that evaluates to an error and renders nothing.
    evaluate_contour(OBJECT_CONTOUR_REPLACE_ID, &params.contour)?;
    project.set_object_contour(params.object, params.contour)?;
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
    /// Open or closed chain of line and arc segments, in the object's local XY plane.
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
    let id = project.add_object(params.parent, params.name, params.placement, Geometry::Path(params.path))?;
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
    /// The constructive object to bake.
    pub object: Id,
}

/// The `object.collapse` command.
pub(crate) const OBJECT_COLLAPSE: CommandSpec =
    CommandSpec::new(OBJECT_COLLAPSE_ID, OBJECT_COLLAPSE_LABEL, object_collapse_enabled, object_collapse_run);

fn object_collapse_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ObjectCollapse>(params, OBJECT_COLLAPSE_ID) else {
        return false;
    };
    session.project().object(params.object).is_some_and(|object| object.geometry().as_constructive().is_some())
}

fn object_collapse_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectCollapse>(params, OBJECT_COLLAPSE_ID)?;
    let constructive = project
        .object(params.object)
        .and_then(|object| object.geometry().as_constructive())
        .ok_or(ProjectError::NotConstructiveObject { id: params.object })?;
    let mesh = build_constructive_mesh(constructive)
        .map_err(|error| CommandError::Evaluation { command: OBJECT_COLLAPSE_ID, message: error.to_string() })?;
    project.collapse_object(params.object, mesh)?;
    Ok(CommandOutcome::changed())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{ContourSegment, Revision};
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
    fn dispatch_creates_one_mesh_object_in_one_revision() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let params = cube_params(&session, Length::from_millimetres(100).expect("length"));

        let outcome = session.dispatch(OBJECT_PRIMITIVE_CREATE_ID, &params).expect("create cube");

        let id = outcome.created_id().expect("the command returns the new object id");
        assert_eq!(session.project().revision().get(), 1);
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Cube");
        let mesh = object.geometry().as_mesh().expect("mesh payload");
        assert_eq!(mesh.positions().len(), 8);
        assert_eq!(mesh.faces().len(), 6);
        assert_eq!(mesh.bounds_micrometres(), ([-50_000; 3], [50_000; 3]));
        assert!(session.can_undo());

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2, "undo advances the revision to invalidate caches");
        assert!(session.project().object(id).is_none());
    }

    #[test]
    fn a_size_outside_the_limit_is_rejected_without_mutating() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        for size in [Length::ZERO, MAX_PRIMITIVE_SIZE.checked_add(Length::from_micrometres(1)).expect("sum")] {
            let params = cube_params(&session, size);
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
    /// The ring is valid and simple, but at the mesh sampling tolerance it holds more than
    /// [`furnicraft_model::MAX_FACE_VERTICES`] points, so one extrusion cap cannot store it.
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
    fn a_contour_the_cap_cannot_hold_is_refused_without_mutating() {
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
        assert!(!create.enabled(&session, &value), "a ring the cap cannot hold is not creatable");
        let error = session.dispatch(OBJECT_EXTRUDE_CREATE_ID, &value).expect_err("the cap cannot hold it");
        assert!(error.to_string().contains("extrusion cap"), "the error names the limit: {error}");
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.can_undo(), "a refused create leaves no undo entry");

        let id = session
            .dispatch(OBJECT_EXTRUDE_CREATE_ID, &extrude_params(&session, "Top", height))
            .expect("create extrusion")
            .created_id()
            .expect("a new object id");
        let original = session
            .project()
            .object(id)
            .and_then(|object| object.geometry().as_constructive())
            .and_then(Constructive::contour)
            .cloned()
            .expect("the stored ring");
        let before = session.project().revision();
        let replace = serde_json::to_value(ObjectContourReplace { object: id, contour: band_contour() })
            .expect("params serialize");
        let spec = registry.get(OBJECT_CONTOUR_REPLACE_ID).expect("registered");
        assert!(!spec.enabled(&session, &replace), "the replacement ring is not buildable either");
        assert!(session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &replace).is_err());
        assert_eq!(session.project().revision(), before);
        assert_eq!(
            session
                .project()
                .object(id)
                .and_then(|object| object.geometry().as_constructive())
                .and_then(Constructive::contour),
            Some(&original),
            "the stored ring is untouched"
        );
        session.undo().expect("the refused command left no entry");
        assert!(session.project().object(id).is_none(), "undo removes the creation, nothing was recorded between");
        assert!(!session.can_undo());
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
                .and_then(Constructive::contour)
                .cloned()
        };
        let original = contour(&session).expect("the stored ring");
        let wider = rectangle(120, 50);
        let value = serde_json::to_value(ObjectContourReplace { object: id, contour: wider.clone() })
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
        let value = serde_json::to_value(ObjectContourReplace { object: id, contour: wider.clone() }).expect("params");
        session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).expect("replace the contour");
        assert_eq!(session.project().revision().get(), 2);
        let object = session.project().object(id).expect("object");
        assert_eq!(
            object.geometry().as_constructive().expect("constructive").contour(),
            Some(&wider),
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
        let value =
            serde_json::to_value(ObjectContourReplace { object: cube, contour: rectangle(10, 10) }).expect("params");
        assert!(session.dispatch(OBJECT_CONTOUR_REPLACE_ID, &value).is_err());
        let value = serde_json::to_value(ObjectCollapse { object: cube }).expect("params");
        assert!(session.dispatch(OBJECT_COLLAPSE_ID, &value).is_err());

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
