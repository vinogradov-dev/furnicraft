//! Read-only world values the inspector shows.
//!
//! Every value is a project query — `world_placement` or `pivot_world` — formatted for the card.
//! The cards never compose placements or turn a point themselves; they print what the model
//! already derived.

use furnicraft_model::{Id, PivotLocal, Project};
use furnicraft_units::Length;

/// Formats three exact lengths as one inspector value.
pub(crate) fn point(point: [Length; 3]) -> String {
    format!("{}  {}  {}", point[0], point[1], point[2])
}

/// The world-space origin in millimetres.
///
/// Returns `None` when the node is missing or the composed origin is outside the editable range,
/// so a hostile document shows no invented number.
pub(crate) fn world_origin(project: &Project, node: Id) -> Option<String> {
    let origin = PivotLocal::from_local(project.origin_world(node)?)?;
    Some(point(origin.components()))
}

/// The node's manipulation pivot in world space, in millimetres.
///
/// This is the point both gizmos turn about. Returns `None` when the node is missing or the value
/// is outside the editable range.
pub(crate) fn world_pivot(project: &Project, node: Id) -> Option<String> {
    let pivot = PivotLocal::from_local(project.pivot_world(node)?)?;
    Some(point(pivot.components()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID;
    use furnicraft_engine::{NodePlacementSet, Session};
    use furnicraft_model::{Id, PivotLocal, Placement};
    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn placement(translation: [f64; 3], rotation: [f64; 4]) -> Placement {
        serde_json::from_value(serde_json::json!({ "translation": translation, "rotation": rotation }))
            .expect("placement")
    }

    fn set(session: &mut Session, node: Id, placement: Placement) {
        let params = serde_json::to_value(NodePlacementSet { node, placement }).expect("params");
        session.dispatch(NODE_PLACEMENT_SET_ID, &params).expect("set placement");
    }

    /// The sample drawer is turned 90° about Z at x = 100 mm and holds the front at x = 10 mm.
    fn turned_session() -> (Session, Id) {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let half = std::f64::consts::FRAC_PI_4;
        set(&mut session, drawer, placement([100.0, 0.0, 0.0], [0.0, 0.0, half.sin(), half.cos()]));
        set(&mut session, front, placement([10.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]));
        (session, front)
    }

    #[test]
    fn the_world_origin_is_the_composed_parent_frame() {
        let (session, front) = turned_session();
        assert_eq!(world_origin(session.project(), front).as_deref(), Some("100 mm  10 mm  0 mm"));
        assert_eq!(world_pivot(session.project(), front).as_deref(), Some("100 mm  10 mm  0 mm"));
    }

    #[test]
    fn the_world_pivot_follows_a_local_pivot_move() {
        let (mut session, _front) = turned_session();
        let drawer = Id::new(3).expect("drawer id");
        let pivot = PivotLocal::new(
            Length::from_micrometres(1_500),
            Length::from_micrometres(-2_500),
            Length::from_micrometres(3_000),
        );
        let params = serde_json::to_value(furnicraft_engine::NodePivotSet { node: drawer, pivot }).expect("params");
        session.dispatch(furnicraft_engine::node_cmds::NODE_PIVOT_SET_ID, &params).expect("set pivot");

        assert_eq!(world_pivot(session.project(), drawer).as_deref(), Some("102.5 mm  1.5 mm  3 mm"));
    }

    #[test]
    fn a_turn_about_the_pivot_keeps_the_world_pivot_and_moves_the_world_origin() {
        let (mut session, _front) = turned_session();
        let drawer = Id::new(3).expect("drawer id");
        let pivot = PivotLocal::new(
            Length::from_micrometres(1_500),
            Length::from_micrometres(-2_500),
            Length::from_micrometres(3_000),
        );
        let params = serde_json::to_value(furnicraft_engine::NodePivotSet { node: drawer, pivot }).expect("params");
        session.dispatch(furnicraft_engine::node_cmds::NODE_PIVOT_SET_ID, &params).expect("set pivot");

        // A further 90° turn about the pivot, expressed in the parent frame: the same placement the
        // rotate gizmo would dispatch. The drawer already turns 90°, so the composed turn is 180°.
        set(&mut session, drawer, placement([104.0, -1.0, 0.0], [0.0, 0.0, 1.0, 0.0]));

        assert_eq!(world_pivot(session.project(), drawer).as_deref(), Some("102.5 mm  1.5 mm  3 mm"));
        assert_eq!(world_origin(session.project(), drawer).as_deref(), Some("104 mm  -1 mm  0 mm"));
    }

    #[test]
    fn a_missing_node_has_no_world_row() {
        let (session, _) = turned_session();
        let missing = Id::new(999).expect("id");
        assert_eq!(world_origin(session.project(), missing), None);
        assert_eq!(world_pivot(session.project(), missing), None);
    }
}
