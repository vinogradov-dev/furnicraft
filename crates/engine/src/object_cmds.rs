//! Commands that create free-form geometry objects.
//!
//! A primitive is a baked mesh from the start: it has no feature history, and undo is its only
//! record ([ADR 0037]). The command allocates the object id, builds the mesh in the object's local
//! frame and places it under a container in one revision.

use furnicraft_model::{Geometry, Id, Mesh, Placement, Project, ProjectError};
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::node_cmds::validate_name;
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
    let Ok(params) = parse::<ObjectPrimitiveCreate>(params) else {
        return false;
    };
    !params.name.trim().is_empty() && valid_size(params.size) && session.project().is_container(params.parent)
}

fn object_primitive_create_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ObjectPrimitiveCreate>(params)?;
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

fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T, CommandError> {
    serde_json::from_value(params.clone()).map_err(|error| CommandError::InvalidParams {
        command: OBJECT_PRIMITIVE_CREATE_ID,
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::Revision;
    use furnicraft_testkit::{sample_catalog, sample_project};

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
}
