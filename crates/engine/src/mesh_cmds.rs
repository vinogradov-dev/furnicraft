//! Commands that edit a baked mesh.
//!
//! A mesh object has no feature history, so editing its vertices directly is the only way to shape
//! it ([ADR 0037]). The command takes absolute local positions rather than a delta: the shell
//! computes a target once per drag and sends it on release, so a repeated or replayed command
//! produces the same result.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md

use furnicraft_model::{Id, Project};
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::class_rules::{Dimension, NodeRules};
use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;
use furnicraft_text::Message;

/// Stable id of the mesh vertex move command.
pub const MESH_VERTICES_MOVE_ID: &str = "mesh.vertices.move";

/// User-facing label of the mesh vertex move command.
pub const MESH_VERTICES_MOVE_LABEL: Message = Message::CmdMoveVertices;

/// Parameters for `mesh.vertices.move`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshVerticesMove {
    /// The mesh object whose vertices move.
    pub object: Id,
    /// One entry per moved vertex; every index must be unique and inside the mesh.
    pub moves: Vec<MeshVertexMove>,
}

/// One vertex and the absolute local position it moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshVertexMove {
    /// Index into the object's vertex array for the revision the caller read.
    pub vertex: u32,
    /// The new position in the object's local frame.
    pub position: [Length; 3],
}

/// The `mesh.vertices.move` command.
pub(crate) const MESH_VERTICES_MOVE: CommandSpec = CommandSpec::new(
    MESH_VERTICES_MOVE_ID,
    MESH_VERTICES_MOVE_LABEL,
    mesh_vertices_move_enabled,
    mesh_vertices_move_run,
);

fn mesh_vertices_move_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<MeshVerticesMove>(params) else {
        return false;
    };
    !params.moves.is_empty()
        && session.project().object(params.object).is_some_and(|object| object.geometry().as_mesh().is_some())
        && class_allows_vertex_moves(session.project(), &params).is_ok()
}

fn mesh_vertices_move_run(
    project: &mut Project,
    _catalog: &furnicraft_model::Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<MeshVerticesMove>(params)?;
    if params.moves.is_empty() {
        return Err(CommandError::InvalidParams {
            command: MESH_VERTICES_MOVE_ID,
            message: "at least one vertex move is required".to_owned(),
        });
    }
    let moves: Vec<(u32, [Length; 3])> = params.moves.iter().map(|entry| (entry.vertex, entry.position)).collect();
    class_allows_vertex_moves(project, &params)?;
    project.move_mesh_vertices(params.object, &moves)?;
    Ok(CommandOutcome::changed())
}

/// The class gate of a vertex edit: a fixed dimension may not move, and a length-only class may
/// not move the width or the thickness.
///
/// A mesh object is resized by moving its vertices, so it reads the same `extent` a panel does. A
/// move that leaves a dimension where it was is not an edit on that axis and is not refused.
fn class_allows_vertex_moves(project: &Project, params: &MeshVerticesMove) -> Result<(), CommandError> {
    let Some(object) = project.object(params.object) else {
        return Ok(());
    };
    let Some(mesh) = object.geometry().as_mesh() else {
        return Ok(());
    };
    let rules = NodeRules::of(project, params.object);
    for entry in &params.moves {
        let Some(current) = usize::try_from(entry.vertex).ok().and_then(|index| mesh.positions().get(index)) else {
            continue;
        };
        for (axis, dimension) in Dimension::ALL.into_iter().enumerate() {
            let Some(before) = current.get(axis) else {
                continue;
            };
            let Some(after) = entry.position.get(axis) else {
                continue;
            };
            if after != before {
                rules.allow_resize(dimension).map_err(|refusal| CommandError::class(MESH_VERTICES_MOVE_ID, refusal))?;
            }
        }
    }
    Ok(())
}

fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T, CommandError> {
    serde_json::from_value(params.clone())
        .map_err(|error| CommandError::InvalidParams { command: MESH_VERTICES_MOVE_ID, message: error.to_string() })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Geometry, Mesh, Placement};
    use furnicraft_testkit::{millimetres, sample_catalog, sample_project};
    use furnicraft_units::Length;

    use super::*;

    /// A session with one cube at its root, and the cube's id.
    fn cube_session() -> (Session, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(millimetres(100)).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("object");
        (Session::with_catalog(project, sample_catalog()), id)
    }

    fn move_params(object: Id, moves: &[(u32, [Length; 3])]) -> Value {
        serde_json::to_value(MeshVerticesMove {
            object,
            moves: moves
                .iter()
                .map(|(vertex, position)| MeshVertexMove { vertex: *vertex, position: *position })
                .collect(),
        })
        .expect("params serialize")
    }

    fn position(session: &Session, object: Id, vertex: usize) -> [Length; 3] {
        session.project().object(object).expect("object").geometry().as_mesh().expect("mesh").positions()[vertex]
    }

    #[test]
    fn dispatch_moves_a_vertex_in_one_revision() {
        let (mut session, object) = cube_session();
        let before = position(&session, object, 0);
        let untouched = position(&session, object, 1);
        let target = [millimetres(10), millimetres(20), millimetres(30)];
        assert_ne!(before, target);

        let revision = session.project().revision();
        let params = move_params(object, &[(0, target)]);
        let outcome = session.dispatch(MESH_VERTICES_MOVE_ID, &params).expect("move vertex");

        assert_eq!(outcome.created_id(), None);
        assert_eq!(session.project().revision().get(), revision.get() + 1);
        assert_eq!(position(&session, object, 0), target);
        assert_eq!(position(&session, object, 1), untouched, "only the named vertex moves");

        session.undo().expect("undo");
        assert_eq!(position(&session, object, 0), before, "undo restores the vertex");
        session.redo().expect("redo");
        assert_eq!(position(&session, object, 0), target, "redo applies the move again");
    }

    #[test]
    fn a_batch_moves_every_vertex_once() {
        let (mut session, object) = cube_session();
        let untouched = position(&session, object, 1);
        let target = [millimetres(1), millimetres(2), millimetres(3)];
        let params = move_params(object, &[(0, target), (4, target)]);
        session.dispatch(MESH_VERTICES_MOVE_ID, &params).expect("move two vertices");

        assert_eq!(position(&session, object, 0), target);
        assert_eq!(position(&session, object, 4), target);
        assert_eq!(position(&session, object, 1), untouched);
    }

    #[test]
    fn an_out_of_bounds_index_rejects_the_whole_batch() {
        let (mut session, object) = cube_session();
        let revision = session.project().revision();
        let target = [millimetres(1), millimetres(2), millimetres(3)];
        let params = move_params(object, &[(0, target), (8, target)]);

        let error = session.dispatch(MESH_VERTICES_MOVE_ID, &params).expect_err("stale index");

        assert!(error.to_string().contains("vertex 8"), "error was {error}");
        assert_eq!(session.project().revision(), revision);
        assert!(!session.can_undo(), "a failed command does not enter the history");
        assert_ne!(position(&session, object, 0), target);
    }

    #[test]
    fn an_empty_batch_and_a_non_mesh_node_are_rejected() {
        let (mut session, object) = cube_session();
        let revision = session.project().revision();
        let empty = move_params(object, &[]);
        assert!(session.dispatch(MESH_VERTICES_MOVE_ID, &empty).is_err());

        let panel_params = move_params(Id::new(2).expect("id"), &[(0, [Length::ZERO; 3])]);
        assert!(session.dispatch(MESH_VERTICES_MOVE_ID, &panel_params).is_err());

        assert_eq!(session.project().revision(), revision);
        assert!(!session.can_undo());
    }

    #[test]
    fn enabled_rejects_unknown_objects_and_empty_batches() {
        let (session, object) = cube_session();
        let registry = crate::CommandRegistry::new();
        let spec = registry.get(MESH_VERTICES_MOVE_ID).expect("registered");

        assert!(spec.enabled(&session, &move_params(object, &[(0, [Length::ZERO; 3])])));
        assert!(!spec.enabled(&session, &move_params(object, &[])));
        assert!(!spec.enabled(&session, &move_params(Id::new(99).expect("id"), &[(0, [Length::ZERO; 3])])));
        assert!(!spec.enabled(&session, &move_params(Id::new(2).expect("id"), &[(0, [Length::ZERO; 3])])));
    }

    #[test]
    fn params_round_trip_through_json() {
        let params = MeshVerticesMove {
            object: Id::new(7).expect("id"),
            moves: vec![MeshVertexMove { vertex: 3, position: [millimetres(-5), millimetres(0), millimetres(12)] }],
        };
        let value = serde_json::to_value(&params).expect("serialize");
        let decoded: MeshVerticesMove = serde_json::from_value(value).expect("deserialize");
        assert_eq!(decoded, params);
        assert_eq!(
            serde_json::to_value(&params).expect("serialize")["moves"][0]["position"][0],
            serde_json::json!(-5000)
        );
    }

    #[test]
    fn a_move_after_undo_keeps_the_revision_monotonic() {
        let (mut session, object) = cube_session();
        let target = [millimetres(1), millimetres(2), millimetres(3)];
        let params = move_params(object, &[(0, target)]);
        session.dispatch(MESH_VERTICES_MOVE_ID, &params).expect("first move");
        let after_first = session.project().revision();
        session.undo().expect("undo");
        assert!(session.project().revision() > after_first, "undo invalidates caches");
        let undone = session.project().revision();
        session.dispatch(MESH_VERTICES_MOVE_ID, &params).expect("second move");
        assert!(session.project().revision() > undone);
    }
    #[test]
    fn a_length_only_class_refuses_a_vertex_move_across_the_width() {
        use furnicraft_model::{ClassCode, PropertyValue, SYSTEM_CLASS};

        let (mut session, object) = cube_session();
        let class: ClassCode = "70.01".parse().expect("the worktop code");
        let assignment = serde_json::json!({
            "node": object,
            "key": SYSTEM_CLASS,
            "value": PropertyValue::class(class),
        });
        session.dispatch("node.property.set", &assignment).expect("class the cube");

        let start = position(&session, object, 0);
        let along_length = [Length::from_micrometres(start[0].micrometres() + 10_000), start[1], start[2]];
        let along_width = [start[0], Length::from_micrometres(start[1].micrometres() + 10_000), start[2]];

        let allowed = session
            .dispatch(MESH_VERTICES_MOVE_ID, &move_params(object, &[(0, along_length)]))
            .expect("a worktop changes its length");
        assert_eq!(allowed.created_id(), None);
        let revision = session.project().revision();

        let refused = session
            .dispatch(MESH_VERTICES_MOVE_ID, &move_params(object, &[(0, along_width)]))
            .expect_err("a worktop keeps its width");
        assert!(matches!(
            refused,
            CommandError::ClassRefused { command: MESH_VERTICES_MOVE_ID, refusal: crate::ClassRefusal::Dimension(_) }
        ));
        assert_eq!(session.project().revision(), revision, "the refused move changes nothing");
    }
}
