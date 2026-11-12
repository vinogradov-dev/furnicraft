//! The pivot card shared by every node kind.
//!
//! The pivot is a `PivotLocal` offset from the node's origin and is entered in millimetres. For a
//! container, object, camera or light it is authored data: it never follows a resize or a reparent,
//! and both gizmos use the same point. Every field is a scrub: a drag moves the pivot while the hand
//! moves and the whole gesture is one undo entry, typed text lands after a short debounce, and Escape
//! during a drag puts the value back. The Center button dispatches `node.pivot.center`, which puts
//! the offset at the centre of the node's local bounds. An unchanged value does not touch history.
//! A panel's pivot is derived from its geometry — the base corner — so the card shows *Pivot follows
//! origin (offset 0)* read-only. A read-only *Pivot (world)* row prints `Project::pivot_world`; the
//! card never turns the point itself.

use egui::{Color32, RichText, Ui};
use furnicraft_engine::node_cmds::{NODE_PIVOT_CENTER_ID, NODE_PIVOT_SET_ID};
use furnicraft_engine::{EditScope, NodePivotCenter, NodePivotSet, Session};
use furnicraft_model::{Id, PivotLocal, Project};
use furnicraft_text::{Lang, Message};

use crate::command::{dispatch, json};
use crate::coords;
use crate::scrub::{self, CommitPolicy, Scrub};
use crate::section::section;

/// Persistent state of the pivot card.
#[derive(Debug, Default)]
pub(crate) struct PivotCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// The pivot as the fields show it, and the stored pivot it was read from.
///
/// `base` detects a change made elsewhere, for example by undo or the viewport: the card re-reads
/// the document instead of letting a stale draft overwrite a restored pivot.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    node: Id,
    base: PivotLocal,
    x: f64,
    y: f64,
    z: f64,
}

impl Draft {
    fn new(node: Id, base: PivotLocal) -> Self {
        let [x, y, z] = base.components();
        Self { node, base, x: x.millimetres_f64(), y: y.millimetres_f64(), z: z.millimetres_f64() }
    }

    /// The component one field edits.
    fn axis(&mut self, axis: usize) -> &mut f64 {
        match axis {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => &mut self.z,
        }
    }
}

/// The three field labels, in the order the card draws them.
const AXES: [(&str, usize); 3] = [("X", 0), ("Y", 1), ("Z", 2)];

impl PivotCard {
    /// Draws the local pivot of `node` and dispatches an accepted edit.
    ///
    /// The root defines the world frame and has no editable pivot, so it never reaches here.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
        // One token per frame for the three fields: the edit that takes it owns one undo entry.
        let fresh = session.edit_scope();
        let Some(current) = session.project().pivot_local(node) else {
            self.draft = None;
            self.error = None;
            return;
        };

        let lang = crate::language(ui.ctx());
        ui.label(RichText::new(Message::Pivot.text(lang)).strong());
        if pivot_is_derived(session.project(), node) {
            self.draft = None;
            self.error = None;
            ui.label(Message::PanelPivotFollowsOrigin.text(lang));
            match coords::world_pivot(session.project(), node) {
                Some(world) => ui.label(Message::PivotWorld.with(lang, [world])),
                None => ui.label(Message::PivotWorldOutOfRange.text(lang)),
            };
            return;
        }

        if self.draft.as_ref().is_none_or(|draft| draft.node != node || draft.base != current) {
            self.error = None;
            self.draft = Some(Draft::new(node, current));
        }

        ui.label(Message::PivotInNodeFrame.text(lang));

        let Some(draft) = self.draft.as_mut() else {
            ui.colored_label(Color32::LIGHT_RED, Message::PivotNotEditable.text(lang));
            return;
        };

        let mut center = false;
        let mut edits: Vec<(usize, f64, EditScope)> = Vec::new();
        let mut cancelled: Vec<EditScope> = Vec::new();
        let mut refused: Option<Message> = None;
        section(ui, "pivot.local", Message::PivotLocalOffset.text(lang), true, |ui| {
            for (label, axis) in AXES {
                ui.horizontal(|ui| {
                    ui.label(label);
                    let outcome = Scrub::new(draft.axis(axis))
                        .speed(0.5)
                        .range(-1_000_000.0..=1_000_000.0)
                        .decimals(3)
                        .unit("mm")
                        .parse(scrub::parse_millimetres)
                        .policy(CommitPolicy::Live)
                        .edit(fresh)
                        .show(ui);
                    if outcome.commit
                        && let Some(scope) = outcome.scope
                    {
                        edits.push((axis, *draft.axis(axis), scope));
                    }
                    refused = refused.or(outcome.error);
                    cancelled.extend(outcome.cancelled);
                });
            }
            ui.horizontal(|ui| {
                if ui.button(Message::Center.text(lang)).clicked() {
                    center = true;
                }
            });
        });
        match coords::world_pivot(session.project(), node) {
            Some(world) => ui.label(Message::PivotWorld.with(lang, [world])),
            None => ui.label(Message::PivotWorldOutOfRange.text(lang)),
        };
        if let Some(message) = refused {
            ui.colored_label(Color32::LIGHT_RED, message.text(lang));
        }

        for scope in cancelled {
            session.cancel_edit(scope);
        }

        if center {
            match apply_center(session, node, lang) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return;
        }

        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if edits.is_empty() {
            return;
        }
        for (axis, value, scope) in edits {
            match apply_axis(session, node, axis, value, scope, lang) {
                Ok(()) => self.error = None,
                Err(error) => self.error = Some(error),
            }
        }
        self.draft = None;
        ui.ctx().request_repaint();
    }
}

/// Writes one component of the pivot and leaves the other two as the document holds them.
fn apply_axis(
    session: &mut Session,
    node: Id,
    axis: usize,
    value: f64,
    scope: EditScope,
    lang: Lang,
) -> Result<(), String> {
    let current = session.project().pivot_local(node).ok_or_else(|| Message::NodeNotInScene.with(lang, [node]))?;
    let label = AXES.get(axis).map_or("?", |(label, _)| *label);
    let Some(length) = scrub::millimetres(value) else {
        return Err(Message::FieldWithError.with(lang, [label, Message::LengthErrorOutOfRange.text(lang)]));
    };
    let mut components = current.components();
    if let Some(slot) = components.get_mut(axis) {
        *slot = length;
    }
    let pivot = PivotLocal::new(components[0], components[1], components[2]);
    if pivot == current {
        return Ok(());
    }
    let params = json(&NodePivotSet { node, pivot })?;
    session.dispatch_in(scope, NODE_PIVOT_SET_ID, &params).map(|_| ()).map_err(|error| error.to_string())
}

/// Centres the pivot on the node's local bounds as one command.
fn apply_center(session: &mut Session, node: Id, lang: Lang) -> Result<(), String> {
    let current = session.project().pivot_local(node).ok_or_else(|| Message::NodeNotInScene.with(lang, [node]))?;
    let center = session.project().bounds_center_local(node).unwrap_or_default();
    let pivot = PivotLocal::from_local(center).unwrap_or_default();
    if pivot == current {
        return Ok(());
    }
    let params = json(&NodePivotCenter { node })?;
    dispatch(session, NODE_PIVOT_CENTER_ID, &params).map(|_| ())
}

/// Whether the pivot comes from the node's geometry instead of the user.
///
/// A panel is anchored at the base corner of its outline (ADR 0083), so its pivot is read-only.
fn pivot_is_derived(project: &Project, node: Id) -> bool {
    project.panel(node).is_some()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::Session;
    use furnicraft_model::PivotLocal;
    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_units::Length;

    use super::*;

    /// Nudges one pivot axis inside one edit, as a drag does.
    fn drag(session: &mut Session, node: Id, axis: usize, values: &[f64]) {
        let scope = session.edit_scope();
        for value in values {
            apply_axis(session, node, axis, *value, scope, Lang::En).expect("axis edit");
        }
        session.end_edit();
    }

    #[test]
    fn an_axis_edit_moves_the_pivot_as_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");

        drag(&mut session, drawer, 0, &[1.5]);
        drag(&mut session, drawer, 1, &[-2.5]);
        drag(&mut session, drawer, 2, &[3.0]);
        assert_eq!(
            session.project().pivot_local(drawer),
            Some(PivotLocal::new(
                Length::from_micrometres(1_500),
                Length::from_micrometres(-2_500),
                Length::from_micrometres(3_000),
            ))
        );

        session.undo().expect("undo Z");
        session.undo().expect("undo Y");
        session.undo().expect("undo X");
        assert!(!session.can_undo(), "each axis edit must be exactly one undo entry");
    }

    #[test]
    fn a_drag_that_writes_every_frame_is_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let before = session.project().pivot_local(drawer);

        drag(&mut session, drawer, 0, &[1.0, 2.0, 3.0, 4.0]);
        session.undo().expect("undo the drag");
        assert_eq!(session.project().pivot_local(drawer), before);
        assert!(!session.can_undo(), "a drag is one undo entry, whatever number of frames it took");
    }

    #[test]
    fn an_unchanged_pivot_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let stored = session.project().pivot_local(drawer).expect("pivot");
        let x = stored.components()[0].millimetres_f64();
        drag(&mut session, drawer, 0, &[x]);
        assert!(!session.can_undo());
    }

    #[test]
    fn centring_uses_the_local_bounds_and_is_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        session
            .dispatch(
                NODE_PIVOT_SET_ID,
                &json(&NodePivotSet { node: drawer, pivot: PivotLocal::ZERO }).expect("params"),
            )
            .expect("clear pivot");

        apply_center(&mut session, drawer, Lang::En).expect("center pivot");
        assert_eq!(
            session.project().pivot_local(drawer),
            Some(PivotLocal::new(
                Length::from_micrometres(350_000),
                Length::from_micrometres(250_000),
                Length::from_micrometres(9_000),
            ))
        );
        assert_eq!(session.project().revision().get(), 2);
        session.undo().expect("undo centring");
        assert_eq!(session.project().pivot_local(drawer), Some(PivotLocal::ZERO));
    }

    #[test]
    fn centring_at_the_stored_pivot_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        apply_center(&mut session, drawer, Lang::En).expect("center pivot");
        let revision = session.project().revision();
        apply_center(&mut session, drawer, Lang::En).expect("center the centred pivot");
        assert_eq!(session.project().revision(), revision, "a second centring changes nothing");
    }

    #[test]
    fn a_hostile_value_is_refused_and_names_the_axis() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let scope = session.edit_scope();

        let error = apply_axis(&mut session, drawer, 0, f64::INFINITY, scope, Lang::En).expect_err("infinite X");
        assert!(error.starts_with("X: "), "error was {error}");
        assert!(!session.can_undo());

        let missing = Id::new(999).expect("id");
        let error = apply_axis(&mut session, missing, 0, 0.0, scope, Lang::En).expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
    }

    #[test]
    fn a_panel_pivot_is_derived_and_a_container_pivot_is_authored() {
        let project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("drawer id");
        assert!(pivot_is_derived(&project, panel), "a panel's pivot comes from its geometry");
        assert!(!pivot_is_derived(&project, drawer), "a container's pivot is authored");
        assert!(!pivot_is_derived(&project, project.root().id()));
        assert!(!pivot_is_derived(&project, Id::new(999).expect("id")));
    }

    #[test]
    fn the_draft_prefills_from_the_document() {
        let drawer = Id::new(3).expect("drawer id");
        let pivot = PivotLocal::new(
            Length::from_micrometres(350_000),
            Length::from_micrometres(250_000),
            Length::from_micrometres(9_000),
        );
        let draft = Draft::new(drawer, pivot);
        assert_eq!((draft.x, draft.y, draft.z), (350.0, 250.0, 9.0));
        assert_eq!(draft.base, pivot);
    }
}
