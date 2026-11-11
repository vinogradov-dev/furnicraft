//! The placement card shared by panels and assemblies.
//!
//! Coordinates are local to the parent container and entered in millimetres; a read-only *World
//! position* row shows what the parent chain composes to, read from `Project::world_placement`.
//! Rotation is shown as intrinsic XYZ Euler angles in degrees; the document keeps a unit
//! quaternion, so the angles are only a view of it. The card keeps the typed text until the edit
//! ends: Enter in any field or the Apply button parses all six fields and dispatches one
//! `node.placement.set`, so an edit is one undo entry. The half that did not change is kept
//! bit-for-bit. *Zero translation* dispatches `node.placement.world_zero`, which clears the
//! parent-frame translation and keeps the rotation.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::{NODE_PLACEMENT_SET_ID, NODE_PLACEMENT_WORLD_ZERO_ID};
use furnicraft_engine::{NodePlacementSet, NodePlacementWorldZero, Session};
use furnicraft_model::{EulerAngles, Id, Placement};
use furnicraft_text::{Lang, Message};
use furnicraft_units::{Degrees, Length};

use crate::command::{dispatch, json};
use crate::coords;
use crate::section::section;

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
    rx: String,
    ry: String,
    rz: String,
}

impl Draft {
    fn new(node: Id, base: Placement) -> Option<Self> {
        let [x, y, z] = base.translation_lengths()?;
        let rotation = base.rotation_euler();
        Some(Self {
            node,
            base,
            x: x.to_string(),
            y: y.to_string(),
            z: z.to_string(),
            rx: rotation.x.to_string(),
            ry: rotation.y.to_string(),
            rz: rotation.z.to_string(),
        })
    }
}

impl PlacementCard {
    /// Draws the local placement of `node` and dispatches an accepted edit.
    ///
    /// The root container defines the world frame and has no placement, so it never reaches here.
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

        ui.label(RichText::new(Message::Placement.text(crate::language(ui.ctx()))).strong());
        let parent_name = session
            .project()
            .parent_of(node)
            .and_then(|parent| session.project().container(parent))
            .map(|container| container.name().to_owned());
        let lang = crate::language(ui.ctx());
        let translation_title = match parent_name {
            Some(name) => Message::TranslationLocalTo.with(lang, [name]),
            None => Message::TranslationLocalToParent.text(lang).to_owned(),
        };

        let Some(draft) = self.draft.as_mut() else {
            ui.colored_label(Color32::LIGHT_RED, Message::TranslationOutOfRange.text(crate::language(ui.ctx())));
            return;
        };

        let world = coords::world_origin(session.project(), node);
        let can_zero = draft.base.translation_lengths().is_some_and(|translation| translation != [Length::ZERO; 3]);
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let mut commit = false;
        let mut zero = false;
        let lang = crate::language(ui.ctx());
        section(ui, "placement.transform", Message::SectionTransform.text(lang), true, |ui| {
            section(ui, "placement.transform.translation", translation_title, true, |ui| {
                for (label, text) in [("X", &mut draft.x), ("Y", &mut draft.y), ("Z", &mut draft.z)] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let response = ui.add(TextEdit::singleline(text).desired_width(96.0).hint_text("mm"));
                        ui.label("mm");
                        commit |= response.lost_focus() && enter;
                    });
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_zero, egui::Button::new(Message::CmdZeroTranslation.text(lang))).clicked() {
                        zero = true;
                    }
                });
                match &world {
                    Some(world) => ui.label(Message::WorldPosition.with(lang, [world])),
                    None => ui.label(Message::WorldPositionOutOfRange.text(lang)),
                };
            });
            section(
                ui,
                "placement.transform.rotation",
                Message::RotationWithOrder.with(lang, [EulerAngles::ORDER_LABEL]),
                true,
                |ui| {
                    for (label, text) in [("X", &mut draft.rx), ("Y", &mut draft.ry), ("Z", &mut draft.rz)] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let response = ui.add(TextEdit::singleline(text).desired_width(96.0).hint_text("°"));
                            ui.label("°");
                            commit |= response.lost_focus() && enter;
                        });
                    }
                },
            );
            if ui.button(Message::Apply.text(lang)).clicked() {
                commit = true;
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }

        if zero {
            match apply_world_zero(session, node, lang) {
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
        let fields = Fields {
            x: draft.x.clone(),
            y: draft.y.clone(),
            z: draft.z.clone(),
            rx: draft.rx.clone(),
            ry: draft.ry.clone(),
            rz: draft.rz.clone(),
        };
        match apply_placement(session, node, &fields, lang) {
            Ok(()) => {
                self.error = None;
                self.draft = None;
            }
            Err(error) => self.error = Some(error),
        }
        ui.ctx().request_repaint();
    }
}

/// Text of all six placement fields as typed by the user.
#[derive(Clone, Debug, PartialEq)]
struct Fields {
    x: String,
    y: String,
    z: String,
    rx: String,
    ry: String,
    rz: String,
}

/// Parses the six fields and places `node`, leaving the document untouched when nothing changed.
///
/// Only the half that changed is rebuilt: a translation edit keeps the exact stored quaternion and a
/// rotation edit keeps the exact stored translation. An Euler triple that wraps to the displayed one,
/// or one that describes the same rotation in another way, is not a change.
fn apply_placement(session: &mut Session, node: Id, fields: &Fields, lang: Lang) -> Result<(), String> {
    let current = session
        .project()
        .node(node)
        .map(|node| node.placement())
        .ok_or_else(|| Message::NodeNotInScene.with(lang, [node]))?;
    let translation =
        [parse_length("X", &fields.x, lang)?, parse_length("Y", &fields.y, lang)?, parse_length("Z", &fields.z, lang)?];
    let rotation = EulerAngles {
        x: parse_degrees(&Message::RotateAxis.with(lang, ["X"]), &fields.rx, lang)?,
        y: parse_degrees(&Message::RotateAxis.with(lang, ["Y"]), &fields.ry, lang)?,
        z: parse_degrees(&Message::RotateAxis.with(lang, ["Z"]), &fields.rz, lang)?,
    };

    let mut placement = current;
    if current.translation_lengths() != Some(translation) {
        placement = placement
            .with_translation(translation[0], translation[1], translation[2])
            .map_err(|error| error.to_string())?;
    }
    if rotation.wrapped() != current.rotation_euler() {
        placement = placement.with_rotation_euler(rotation).map_err(|error| error.to_string())?;
    }
    if placement.same_transform(current) {
        return Ok(());
    }
    let params = json(&NodePlacementSet { node, placement })?;
    dispatch(session, NODE_PLACEMENT_SET_ID, &params).map(|_| ())
}

/// Clears the parent-frame translation through `node.placement.world_zero`, keeping the rotation.
fn apply_world_zero(session: &mut Session, node: Id, lang: Lang) -> Result<(), String> {
    if session.project().node(node).is_none() {
        return Err(Message::NodeNotInScene.with(lang, [node]));
    }
    let params = json(&NodePlacementWorldZero { node })?;
    dispatch(session, NODE_PLACEMENT_WORLD_ZERO_ID, &params).map(|_| ())
}

fn parse_length(label: &str, text: &str, lang: Lang) -> Result<Length, String> {
    Length::parse_millimetres(text)
        .map_err(|error| Message::FieldWithError.with(lang, [label, crate::length_error(error).text(lang)]))
}

fn parse_degrees(label: &str, text: &str, lang: Lang) -> Result<Degrees, String> {
    Degrees::parse_degrees(text)
        .map_err(|error| Message::FieldWithError.with(lang, [label, crate::degrees_error(error).text(lang)]))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn fields(x: &str, y: &str, z: &str, rx: &str, ry: &str, rz: &str) -> Fields {
        Fields {
            x: x.to_owned(),
            y: y.to_owned(),
            z: z.to_owned(),
            rx: rx.to_owned(),
            ry: ry.to_owned(),
            rz: rz.to_owned(),
        }
    }

    fn identity_fields() -> Fields {
        fields("0", "0", "0", "0", "0", "0")
    }

    fn fields_from_draft(draft: &Draft) -> Fields {
        Fields {
            x: draft.x.clone(),
            y: draft.y.clone(),
            z: draft.z.clone(),
            rx: draft.rx.clone(),
            ry: draft.ry.clone(),
            rz: draft.rz.clone(),
        }
    }

    fn set_placement(session: &mut Session, node: Id, placement: Placement) {
        let params = json(&NodePlacementSet { node, placement }).expect("command parameters");
        dispatch(session, NODE_PLACEMENT_SET_ID, &params).expect("set placement");
    }

    #[test]
    fn applying_fields_moves_the_node_as_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        apply_placement(&mut session, panel, &fields("10", "-2.5", "3", "0", "0", "0"), Lang::En)
            .expect("apply placement");
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
    fn applying_rotation_is_one_undo_step_and_keeps_the_translation() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        apply_placement(&mut session, panel, &fields("0", "0", "0", "90", "0", "45"), Lang::En)
            .expect("apply rotation");
        let placement = session.project().panel(panel).expect("panel").placement();
        assert_eq!(placement.translation(), Placement::IDENTITY.translation());
        let angles = placement.rotation_euler();
        assert_eq!(angles.x, Degrees::from_millidegrees(90_000));
        assert_eq!(angles.y, Degrees::ZERO);
        assert_eq!(angles.z, Degrees::from_millidegrees(45_000));

        session.undo().expect("undo the rotation");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert!(!session.can_undo(), "a rotation edit must be exactly one undo entry");
    }

    #[test]
    fn an_unchanged_placement_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply_placement(&mut session, panel, &identity_fields(), Lang::En).expect("apply identity");
        assert!(!session.can_undo());
    }

    #[test]
    fn a_full_turn_or_a_wrapped_angle_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        apply_placement(&mut session, panel, &fields("0", "0", "0", "360", "-720", "0"), Lang::En)
            .expect("apply full turn");
        assert!(!session.can_undo(), "a full turn is not a change");

        apply_placement(&mut session, panel, &fields("0", "0", "0", "0", "0", "0"), Lang::En).expect("apply identity");
        assert!(!session.can_undo());
    }

    #[test]
    fn invalid_text_names_the_axis_and_keeps_the_document() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        let error = apply_placement(&mut session, panel, &fields("wide", "0", "0", "0", "0", "0"), Lang::En)
            .expect_err("invalid X");
        assert!(error.starts_with("X: "), "error was {error}");
        assert!(!session.can_undo());

        let error = apply_placement(&mut session, panel, &fields("0", "1.2345", "0", "0", "0", "0"), Lang::En)
            .expect_err("too precise");
        assert!(error.starts_with("Y: "), "error was {error}");

        let error = apply_placement(&mut session, panel, &fields("0", "0", "0", "spin", "0", "0"), Lang::En)
            .expect_err("invalid rotate X");
        assert!(error.starts_with("Rotate X: "), "error was {error}");
        assert!(!session.can_undo());

        let missing = Id::new(999).expect("id");
        let error = apply_placement(&mut session, missing, &identity_fields(), Lang::En).expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
    }

    #[test]
    fn the_draft_prefills_from_the_document_and_reads_it_back_exactly() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply_placement(&mut session, panel, &fields("12.5", "-7", "0.125", "30.5", "-15", "90"), Lang::En)
            .expect("apply placement");
        let placement = session.project().panel(panel).expect("panel").placement();

        let draft = Draft::new(panel, placement).expect("representable translation");
        assert_eq!(draft.x, "12.5 mm");
        assert_eq!(draft.y, "-7 mm");
        assert_eq!(draft.z, "0.125 mm");
        assert_eq!(draft.rx, "30.5°");
        assert_eq!(draft.ry, "-15°");
        assert_eq!(draft.rz, "90°");

        let rebuilt = placement
            .with_translation(
                parse_length("X", &draft.x, Lang::En).expect("x"),
                parse_length("Y", &draft.y, Lang::En).expect("y"),
                parse_length("Z", &draft.z, Lang::En).expect("z"),
            )
            .expect("rebuild translation")
            .with_rotation_euler(EulerAngles {
                x: parse_degrees("Rotate X", &draft.rx, Lang::En).expect("rx"),
                y: parse_degrees("Rotate Y", &draft.ry, Lang::En).expect("ry"),
                z: parse_degrees("Rotate Z", &draft.rz, Lang::En).expect("rz"),
            })
            .expect("rebuild rotation");
        assert_eq!(rebuilt, placement);
        assert_eq!(draft.base, placement);
    }

    #[test]
    fn a_rotation_edit_keeps_the_exact_translation() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        let placement: Placement = serde_json::from_value(serde_json::json!({
            "translation": [1.234567, -0.0004, 5.5],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("placement");
        set_placement(&mut session, panel, placement);

        let mut draft = Draft::new(panel, placement).expect("draft");
        draft.rx = "45".to_owned();
        apply_placement(&mut session, panel, &fields_from_draft(&draft), Lang::En).expect("rotate");

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.translation(), placement.translation());
        assert_eq!(after.rotation_euler().x, Degrees::from_millidegrees(45_000));
    }

    #[test]
    fn a_translation_edit_keeps_the_exact_quaternion() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        let angle: f64 = 0.123_456;
        let placement: Placement = serde_json::from_value(serde_json::json!({
            "translation": [0.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, (angle / 2.0).sin(), (angle / 2.0).cos()]
        }))
        .expect("placement");
        set_placement(&mut session, panel, placement);

        let mut draft = Draft::new(panel, placement).expect("draft");
        draft.x = "10".to_owned();
        apply_placement(&mut session, panel, &fields_from_draft(&draft), Lang::En).expect("move");

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.rotation(), placement.rotation());
        assert_eq!(after.translation_lengths(), Some([Length::from_micrometres(10_000), Length::ZERO, Length::ZERO]));
    }

    #[test]
    fn zeroing_clears_the_translation_and_keeps_the_rotation_as_one_undo_step() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let half = std::f64::consts::FRAC_PI_4;
        let start: Placement = serde_json::from_value(serde_json::json!({
            "translation": [10.0, -2.5, 3.0],
            "rotation": [0.0, 0.0, half.sin(), half.cos()]
        }))
        .expect("turned placement");
        project.set_node_placement(panel, start).expect("place the panel");
        let mut session = Session::with_catalog(project, sample_catalog());

        apply_world_zero(&mut session, panel, Lang::En).expect("zero the translation");

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.translation_lengths(), Some([Length::ZERO; 3]));
        assert_eq!(after.rotation(), start.rotation());

        session.undo().expect("undo world zero");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), start);
        assert!(!session.can_undo(), "world zero must be exactly one undo entry");

        let error = apply_world_zero(&mut session, Id::new(999).expect("id"), Lang::En).expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
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
