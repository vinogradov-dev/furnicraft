//! The placement card shared by panels and assemblies.
//!
//! Coordinates are local to the parent assembly and entered in millimetres. The card keeps the
//! typed text until the edit ends: Enter in any field or the Apply button parses X, Y and Z and
//! dispatches one `node.placement.set`, so a move is one undo entry. Rotation is preserved until
//! the rotate gizmo lands.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID;
use furnicraft_engine::{NodePlacementSet, Session};
use furnicraft_model::{Id, Placement};
use furnicraft_units::Length;

use crate::command::{dispatch, json};

/// Persistent state of the placement card.
#[derive(Debug, Default)]
pub(crate) struct PlacementCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// Text being edited and the document placement it was read from.
///
/// `base` detects a change made elsewhere, for example by undo: the card re-reads the document
/// instead of letting stale text overwrite a restored placement.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    node: Id,
    base: Placement,
    x: String,
    y: String,
    z: String,
}

impl Draft {
    fn new(node: Id, base: Placement) -> Option<Self> {
        let [x, y, z] = base.translation_lengths()?;
        Some(Self { node, base, x: x.to_string(), y: y.to_string(), z: z.to_string() })
    }
}

impl PlacementCard {
    /// Draws the local placement of `node` and dispatches an accepted edit.
    ///
    /// The root assembly defines the world frame and has no placement, so it never reaches here.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
        let Some(current) = session.project().node(node).map(|node| node.placement()) else {
            self.draft = None;
            self.error = None;
            return;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.node != node || draft.base != current) {
            self.error = None;
            self.draft = Draft::new(node, current);
        }

        ui.label(RichText::new("Placement").strong());
        if let Some(parent) = session.project().parent_of(node) {
            let name = session.project().assembly(parent).map_or("parent assembly", |assembly| assembly.name());
            ui.label(format!("Local to {name}"));
        }

        let Some(draft) = self.draft.as_mut() else {
            ui.colored_label(Color32::LIGHT_RED, "The stored translation is outside the editable millimetre range.");
            return;
        };

        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let mut commit = false;
        for (label, text) in [("X", &mut draft.x), ("Y", &mut draft.y), ("Z", &mut draft.z)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let response = ui.add(TextEdit::singleline(text).desired_width(96.0).hint_text("mm"));
                ui.label("mm");
                commit |= response.lost_focus() && enter;
            });
        }
        if ui.button("Apply").clicked() {
            commit = true;
        }

        if !commit {
            return;
        }
        let (x, y, z) = (draft.x.clone(), draft.y.clone(), draft.z.clone());
        match apply_placement(session, node, &x, &y, &z) {
            Ok(()) => {
                self.error = None;
                self.draft = None;
            }
            Err(error) => self.error = Some(error),
        }
        ui.ctx().request_repaint();
    }
}

/// Parses the three fields and moves `node`, leaving the document untouched when nothing changed.
fn apply_placement(session: &mut Session, node: Id, x: &str, y: &str, z: &str) -> Result<(), String> {
    let current = session
        .project()
        .node(node)
        .map(|node| node.placement())
        .ok_or_else(|| format!("node {node} is not in the scene"))?;
    let placement = current
        .with_translation(parse_axis("X", x)?, parse_axis("Y", y)?, parse_axis("Z", z)?)
        .map_err(|error| error.to_string())?;
    if placement == current {
        return Ok(());
    }
    let params = json(&NodePlacementSet { node, placement })?;
    dispatch(session, NODE_PLACEMENT_SET_ID, &params).map(|_| ())
}

fn parse_axis(label: &str, text: &str) -> Result<Length, String> {
    Length::parse_millimetres(text).map_err(|error| format!("{label}: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    #[test]
    fn applying_fields_moves_the_node_as_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        apply_placement(&mut session, panel, "10", "-2.5", "3").expect("apply placement");
        let placement = session.project().panel(panel).expect("panel").placement();
        assert_eq!(
            placement.translation_lengths(),
            Some(
                [Length::from_micrometres(10_000), Length::from_micrometres(-2_500), Length::from_micrometres(3_000),]
            )
        );
        assert_eq!(placement.rotation(), Placement::IDENTITY.rotation());

        session.undo().expect("undo the move");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert!(!session.can_undo(), "a move must be exactly one undo entry");
    }

    #[test]
    fn an_unchanged_placement_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply_placement(&mut session, panel, "0", "0 mm", "+0.0").expect("apply identity");
        assert!(!session.can_undo());
    }

    #[test]
    fn invalid_text_names_the_axis_and_keeps_the_document() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        let error = apply_placement(&mut session, panel, "wide", "0", "0").expect_err("invalid X");
        assert!(error.starts_with("X: "), "error was {error}");
        assert!(!session.can_undo());

        let error = apply_placement(&mut session, panel, "0", "1.2345", "0").expect_err("too precise");
        assert!(error.starts_with("Y: "), "error was {error}");

        let missing = Id::new(999).expect("id");
        let error = apply_placement(&mut session, missing, "0", "0", "0").expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
    }

    #[test]
    fn the_draft_prefills_from_the_document_and_reads_it_back_exactly() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply_placement(&mut session, panel, "12.5", "-7", "0.125").expect("apply placement");
        let placement = session.project().panel(panel).expect("panel").placement();

        let draft = Draft::new(panel, placement).expect("representable translation");
        assert_eq!(draft.x, "12.5 mm");
        assert_eq!(draft.y, "-7 mm");
        assert_eq!(draft.z, "0.125 mm");
        let rebuilt = placement
            .with_translation(
                parse_axis("X", &draft.x).expect("x"),
                parse_axis("Y", &draft.y).expect("y"),
                parse_axis("Z", &draft.z).expect("z"),
            )
            .expect("rebuild");
        assert_eq!(rebuilt, placement);
        assert_eq!(draft.base, placement);
    }

    #[test]
    fn a_hostile_translation_has_no_draft() {
        let huge: Placement = serde_json::from_value(serde_json::json!({
            "translation": [1e300, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("finite placement");
        let node = Id::new(2).expect("id");
        assert!(Draft::new(node, huge).is_none());
    }
}
