//! The pivot card shared by panels and containers.
//!
//! The pivot is stored in the node's own frame and is entered in millimetres. For a container it is
//! authored data: it never follows a resize or a reparent, and both gizmos use the same point.
//! Enter in a field or the Apply button dispatches one `node.pivot.set`; the Center button
//! dispatches `node.pivot.center`, which puts the pivot at the centre of the node's local bounds.
//! Each accepted edit is one undo entry, and an unchanged value does not touch history. A panel's
//! pivot is derived from its geometry — the base corner — so the card shows it read-only. A
//! read-only *Pivot (world)* row prints `Project::pivot_world`; the card never turns the point
//! itself.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::{NODE_PIVOT_CENTER_ID, NODE_PIVOT_SET_ID};
use furnicraft_engine::{NodePivotCenter, NodePivotSet, Session};
use furnicraft_model::{Id, PivotLocal, Project};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::coords;
use crate::section::section;

/// Persistent state of the pivot card.
#[derive(Debug, Default)]
pub(crate) struct PivotCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// Text being edited and the stored pivot it was read from.
///
/// `base` detects a change made elsewhere, for example by undo or the viewport: the card re-reads the
/// document instead of letting stale text overwrite a restored pivot.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    node: Id,
    base: PivotLocal,
    x: String,
    y: String,
    z: String,
}

impl Draft {
    fn new(node: Id, base: PivotLocal) -> Self {
        let [x, y, z] = base.components();
        Self { node, base, x: x.to_string(), y: y.to_string(), z: z.to_string() }
    }
}

impl PivotCard {
    /// Draws the local pivot of `node` and dispatches an accepted edit.
    ///
    /// The root defines the world frame and has no editable pivot, so it never reaches here.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
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
            ui.label(Message::PivotDerived.text(lang));
            section(ui, "pivot.local", Message::SectionLocalPivot.text(lang), true, |ui| {
                ui.label(Message::ZeroMillimetres.text(lang));
                ui.label(Message::PivotBaseCorner.text(lang));
            });
            match coords::world_origin(session.project(), node) {
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

        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let mut commit = false;
        let mut center = false;
        section(ui, "pivot.local", Message::SectionLocalPivot.text(lang), true, |ui| {
            for (label, text) in [("X", &mut draft.x), ("Y", &mut draft.y), ("Z", &mut draft.z)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    let response = ui.add(TextEdit::singleline(text).desired_width(96.0).hint_text("mm"));
                    ui.label("mm");
                    commit |= response.lost_focus() && enter;
                });
            }
            ui.horizontal(|ui| {
                if ui.button(Message::Apply.text(lang)).clicked() {
                    commit = true;
                }
                if ui.button(Message::Center.text(lang)).clicked() {
                    center = true;
                }
            });
        });
        match coords::world_pivot(session.project(), node) {
            Some(world) => ui.label(Message::PivotWorld.with(lang, [world])),
            None => ui.label(Message::PivotWorldOutOfRange.text(lang)),
        };
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
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

        if !commit {
            return;
        }
        let fields = Fields { x: draft.x.clone(), y: draft.y.clone(), z: draft.z.clone() };
        match apply_pivot(session, node, &fields, lang) {
            Ok(()) => {
                self.error = None;
                self.draft = None;
            }
            Err(error) => self.error = Some(error),
        }
        ui.ctx().request_repaint();
    }
}

/// Text of the three pivot fields as typed by the user.
#[derive(Clone, Debug, PartialEq)]
struct Fields {
    x: String,
    y: String,
    z: String,
}

/// Parses the three fields and moves the pivot, leaving the document untouched when nothing changed.
fn apply_pivot(session: &mut Session, node: Id, fields: &Fields, lang: Lang) -> Result<(), String> {
    let current = session.project().pivot_local(node).ok_or_else(|| Message::NodeNotInScene.with(lang, [node]))?;
    let pivot = PivotLocal::new(
        parse_length("X", &fields.x, lang)?,
        parse_length("Y", &fields.y, lang)?,
        parse_length("Z", &fields.z, lang)?,
    );
    if pivot == current {
        return Ok(());
    }
    let params = json(&NodePivotSet { node, pivot })?;
    dispatch(session, NODE_PIVOT_SET_ID, &params).map(|_| ())
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

fn parse_length(label: &str, text: &str, lang: Lang) -> Result<Length, String> {
    Length::parse_millimetres(text)
        .map_err(|error| Message::FieldWithError.with(lang, [label, crate::length_error(error).text(lang)]))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::Session;
    use furnicraft_model::PivotLocal;
    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_text::Lang;

    use super::*;

    fn fields(x: &str, y: &str, z: &str) -> Fields {
        Fields { x: x.to_owned(), y: y.to_owned(), z: z.to_owned() }
    }

    #[test]
    fn applying_fields_moves_the_pivot_as_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");

        apply_pivot(&mut session, drawer, &fields("1.5", "-2.5", "3"), Lang::En).expect("apply pivot");
        assert_eq!(
            session.project().pivot_local(drawer),
            Some(PivotLocal::new(
                Length::from_micrometres(1_500),
                Length::from_micrometres(-2_500),
                Length::from_micrometres(3_000),
            ))
        );

        session.undo().expect("undo the pivot");
        assert!(!session.can_undo(), "a pivot edit must be exactly one undo entry");
    }

    #[test]
    fn an_unchanged_pivot_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply_pivot(&mut session, panel, &fields("0", "0", "0"), Lang::En).expect("apply the stored pivot");
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
    fn invalid_text_names_the_axis_and_keeps_the_document() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        let error = apply_pivot(&mut session, panel, &fields("wide", "0", "0"), Lang::En).expect_err("invalid X");
        assert!(error.starts_with("X: "), "error was {error}");
        assert!(!session.can_undo());

        let missing = Id::new(999).expect("id");
        let error = apply_pivot(&mut session, missing, &fields("0", "0", "0"), Lang::En).expect_err("missing node");
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
        assert_eq!(draft.x, "350 mm");
        assert_eq!(draft.y, "250 mm");
        assert_eq!(draft.z, "9 mm");
        assert_eq!(draft.base, pivot);
    }
}
