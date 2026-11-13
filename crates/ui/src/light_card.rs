//! The selected light card.
//!
//! A light is a scene object ([ADR 0036]): the card renames it and edits its kind, colour and
//! intensity or power. Every number is a scrub and commits as one edit — a drag writes the document
//! while the hand moves and is one undo entry, typed text lands after a short debounce — so there is
//! no Apply button. A sun intensity is a multiplier; a point power is in watts ([ADR 0039]).
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md
//! [ADR 0039]: ../docs/decisions/0039-point-lights-use-watts-and-inverse-square-falloff.md

use egui::{TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::scene_cmds::LIGHT_PARAMS_SET_ID;
use furnicraft_engine::{EditScope, LightParamsSet, NodeRename, Session};
use furnicraft_model::{Id, LightKind, LightNode};
use furnicraft_text::{Lang, Message};

use crate::command::{dispatch, json};
use crate::scrub::{CommitPolicy, Scrub};
use crate::section::section;
use crate::theme;

/// Default point-light power in watts when a sun is switched to a point light.
const DEFAULT_POINT_POWER: f32 = 1000.0;

/// A sun intensity below this reads as a multiplier, not as a wattage.
const SUN_TO_POINT_THRESHOLD: f32 = 100.0;

/// Persistent state of the light card.
#[derive(Debug, Default)]
pub(crate) struct LightCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// Values being edited and the light they were read from.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    light: Id,
    base: (LightKind, [f32; 3], f32),
    name: String,
    kind: LightKind,
    color: [f32; 3],
    intensity: f32,
}

impl Draft {
    fn new(light: Id, name: &str, kind: LightKind, color: [f32; 3], intensity: f32) -> Self {
        Self { light, base: (kind, color, intensity), name: name.to_owned(), kind, color, intensity }
    }
}

impl LightCard {
    /// Draws the light card and dispatches accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, light_id: Id) {
        let Some(light) = session.project().light(light_id) else {
            self.draft = None;
            self.error = None;
            return;
        };
        let name = light.name().to_owned();
        let base = (light.kind(), light.color(), light.intensity());
        if self.draft.as_ref().is_none_or(|draft| draft.light != light_id || draft.base != base || draft.name != name) {
            self.error = None;
            self.draft = Some(Draft::new(light_id, &name, base.0, base.1, base.2));
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        let lang = crate::language(ui.ctx());
        // One token per frame for the colour and intensity fields: the edit that takes it is one
        // undo entry.
        let fresh = session.edit_scope();
        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "light.name", Message::SectionName.text(lang), true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button(Message::CmdRename.text(lang)).clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some(Message::LightNameEmpty.text(lang).to_owned());
                    } else if trimmed != name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        let mut switched = false;
        let mut commit = None;
        let mut cancelled = Vec::new();
        let mut refused = None;
        ui.separator();
        section(ui, "light.params", Message::SectionLight.text(lang), true, |ui| {
            ui.horizontal(|ui| {
                switched |= ui.radio_value(&mut draft.kind, LightKind::Sun, Message::Sun.text(lang)).changed();
                switched |= ui.radio_value(&mut draft.kind, LightKind::Point, Message::Point.text(lang)).changed();
            });
            if switched && draft.kind == LightKind::Point {
                draft.intensity = power_for_point_switch(draft.intensity);
            }
            for (component, value) in ["R", "G", "B"].into_iter().zip(draft.color.iter_mut()) {
                ui.horizontal(|ui| {
                    ui.label(component);
                    let outcome = Scrub::new(value)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .decimals(3)
                        .policy(CommitPolicy::Live)
                        .edit(fresh)
                        .show(ui);
                    if outcome.commit {
                        commit = outcome.scope;
                    }
                    refused = refused.or(outcome.error);
                    cancelled.extend(outcome.cancelled);
                });
            }
            ui.horizontal(|ui| {
                // A sun stores an irradiance multiplier; a point light stores radiant power in watts
                // ([ADR 0039]). The two units get their own label, unit and drag speed.
                let (label, unit, speed) = if draft.kind == LightKind::Point {
                    (Message::PowerWatts.text(lang), "W", 10.0)
                } else {
                    (Message::Intensity.text(lang), "", 0.05)
                };
                ui.label(label);
                let outcome = Scrub::new(&mut draft.intensity)
                    .speed(speed)
                    .range(0.0..=LightNode::MAX_INTENSITY)
                    .decimals(2)
                    .unit(unit)
                    .policy(CommitPolicy::Live)
                    .edit(fresh)
                    .show(ui);
                if outcome.commit {
                    commit = outcome.scope;
                }
                refused = refused.or(outcome.error);
                cancelled.extend(outcome.cancelled);
            });
        });

        // Take a snapshot so the document borrow ends before a command runs.
        let fields = draft.clone();
        if let Some(error) = &self.error {
            ui.colored_label(theme::danger(ui), error);
        }

        if let Some(name) = rename {
            match apply_rename(session, light_id, name) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return;
        }
        for scope in cancelled {
            session.cancel_edit(scope);
        }
        let commit = if switched { Some(None) } else { commit.map(Some) };
        if let Some(scope) = commit {
            match params_from_draft(&fields, lang).and_then(|request| apply_params(session, request, scope)) {
                Ok(()) => {
                    self.error = None;
                    if scope.is_some() {
                        // The drag continues from the document value next frame.
                        self.draft = None;
                    }
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
        } else if let Some(message) = refused {
            self.error = Some(message.text(lang).to_owned());
        } else if let Some(error) = name_error {
            self.error = Some(error);
        }
    }
}

/// A sun intensity is a small multiplier; a point light needs a lamp-sized power in watts.
fn power_for_point_switch(intensity: f32) -> f32 {
    if intensity < SUN_TO_POINT_THRESHOLD { DEFAULT_POINT_POWER } else { intensity }
}

/// Builds the command request, refusing a value that is not finite or is outside its range.
fn params_from_draft(draft: &Draft, lang: Lang) -> Result<LightParamsSet, String> {
    let color = draft.color;
    if !color.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)) {
        return Err(Message::ColourComponents.text(lang).to_owned());
    }
    if !draft.intensity.is_finite() || !(0.0..=LightNode::MAX_INTENSITY).contains(&draft.intensity) {
        let unit = if draft.kind == LightKind::Point { Message::Power } else { Message::Intensity };
        return Err(
            Message::IntensityRange.with(lang, [unit.text(lang), LightNode::MAX_INTENSITY.to_string().as_str()])
        );
    }
    Ok(LightParamsSet { light: draft.light, kind: draft.kind, color, intensity: draft.intensity })
}

fn apply_rename(session: &mut Session, light: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node: light, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

/// Applies the parameters; with a scope, the whole drag is one undo entry.
fn apply_params(session: &mut Session, request: LightParamsSet, scope: Option<EditScope>) -> Result<(), String> {
    let params = json(&request)?;
    match scope {
        Some(scope) => {
            session.dispatch_in(scope, LIGHT_PARAMS_SET_ID, &params).map(|_| ()).map_err(|error| error.to_string())
        }
        None => dispatch(session, LIGHT_PARAMS_SET_ID, &params).map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::LightCreate;
    use furnicraft_engine::scene_cmds::LIGHT_CREATE_ID;
    use furnicraft_model::Placement;
    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn light_session() -> (Session, Id) {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let params = json(&LightCreate {
            parent: root,
            name: "Key".to_owned(),
            placement: Placement::IDENTITY,
            kind: LightKind::Sun,
            color: [1.0, 0.9, 0.8],
            intensity: 2.0,
        })
        .expect("params");
        let light =
            dispatch(&mut session, LIGHT_CREATE_ID, &params).expect("add light").created_id().expect("a light id");
        (session, light)
    }

    #[test]
    fn the_draft_reads_the_document_and_builds_one_request() {
        let (_, light) = light_session();
        let draft = Draft::new(light, "Key", LightKind::Sun, [1.0, 0.9, 0.8], 2.0);
        assert_eq!(
            params_from_draft(&draft, Lang::En),
            Ok(LightParamsSet { light, kind: LightKind::Sun, color: [1.0, 0.9, 0.8], intensity: 2.0 })
        );
    }

    #[test]
    fn an_out_of_range_value_is_refused_before_dispatch() {
        let (session, light) = light_session();
        let before = session.project().revision();
        let bad_color = Draft {
            light,
            base: (LightKind::Sun, [1.0; 3], 1.0),
            name: "Key".to_owned(),
            kind: LightKind::Point,
            color: [1.5, 0.0, 0.0],
            intensity: 1.0,
        };
        assert!(params_from_draft(&bad_color, Lang::En).is_err());
        let bad_intensity = Draft { color: [1.0; 3], intensity: f32::NAN, ..bad_color };
        assert!(params_from_draft(&bad_intensity, Lang::En).is_err());
        assert_eq!(session.project().revision(), before);
    }

    #[test]
    fn switching_a_sun_to_a_point_light_uses_a_lamp_sized_power() {
        assert_eq!(power_for_point_switch(1.0), DEFAULT_POINT_POWER, "a sun multiplier becomes a lamp");
        assert_eq!(power_for_point_switch(2.5), DEFAULT_POINT_POWER);
        assert_eq!(power_for_point_switch(250.0), 250.0, "a value already in watts is kept");
        assert_eq!(power_for_point_switch(1000.0), 1000.0);
    }

    #[test]
    fn applying_changes_one_revision_and_can_be_undone() {
        let (mut session, light) = light_session();
        let request = LightParamsSet { light, kind: LightKind::Point, color: [0.2, 0.3, 0.4], intensity: 4.0 };
        apply_params(&mut session, request, None).expect("apply");
        let node = session.project().light(light).expect("light");
        assert_eq!(node.kind(), LightKind::Point);
        assert_eq!(node.color(), [0.2, 0.3, 0.4]);
        assert_eq!(node.intensity(), 4.0);
        session.undo().expect("undo");
        assert_eq!(session.project().light(light).expect("light").kind(), LightKind::Sun);
    }
}
