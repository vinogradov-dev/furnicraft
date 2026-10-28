//! The selected light card.
//!
//! A light is a scene object ([ADR 0036]): the card renames it and edits its kind, colour and
//! intensity. Values stay in the card until Apply, so one edit is one undo entry.
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use egui::{DragValue, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::scene_cmds::LIGHT_PARAMS_SET_ID;
use furnicraft_engine::{LightParamsSet, NodeRename, Session};
use furnicraft_model::{Id, LightKind, LightNode};

use crate::command::{dispatch, json};
use crate::section::section;

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

        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "light.name", "Name", true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button("Rename").clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some("Light name must not be empty.".to_owned());
                    } else if trimmed != name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        let mut apply = false;
        ui.separator();
        section(ui, "light.params", "Light", true, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut draft.kind, LightKind::Sun, "Sun");
                ui.radio_value(&mut draft.kind, LightKind::Point, "Point");
            });
            ui.horizontal(|ui| {
                ui.label("Colour");
                for (component, value) in ["R", "G", "B"].into_iter().zip(draft.color.iter_mut()) {
                    ui.label(component);
                    ui.add(DragValue::new(value).speed(0.01).range(0.0..=1.0).fixed_decimals(3));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Intensity");
                ui.add(
                    DragValue::new(&mut draft.intensity)
                        .speed(0.05)
                        .range(0.0..=LightNode::MAX_INTENSITY)
                        .fixed_decimals(2),
                );
            });
            if ui.button("Apply light").clicked() {
                apply = true;
            }
        });

        // Take a snapshot so the document borrow ends before a command runs.
        let fields = draft.clone();
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
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
        if apply {
            match params_from_draft(&fields).and_then(|request| apply_params(session, request)) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
        } else if let Some(error) = name_error {
            self.error = Some(error);
        }
    }
}

/// Builds the command request, refusing a value that is not finite or is outside its range.
fn params_from_draft(draft: &Draft) -> Result<LightParamsSet, String> {
    let color = draft.color;
    if !color.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)) {
        return Err("Colour components must be between 0 and 1.".to_owned());
    }
    if !draft.intensity.is_finite() || !(0.0..=LightNode::MAX_INTENSITY).contains(&draft.intensity) {
        return Err(format!("Intensity must be between 0 and {}.", LightNode::MAX_INTENSITY));
    }
    Ok(LightParamsSet { light: draft.light, kind: draft.kind, color, intensity: draft.intensity })
}

fn apply_rename(session: &mut Session, light: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node: light, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

fn apply_params(session: &mut Session, request: LightParamsSet) -> Result<(), String> {
    let params = json(&request)?;
    dispatch(session, LIGHT_PARAMS_SET_ID, &params).map(|_| ())
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
            params_from_draft(&draft),
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
        assert!(params_from_draft(&bad_color).is_err());
        let bad_intensity = Draft { color: [1.0; 3], intensity: f32::NAN, ..bad_color };
        assert!(params_from_draft(&bad_intensity).is_err());
        assert_eq!(session.project().revision(), before);
    }

    #[test]
    fn applying_changes_one_revision_and_can_be_undone() {
        let (mut session, light) = light_session();
        let request = LightParamsSet { light, kind: LightKind::Point, color: [0.2, 0.3, 0.4], intensity: 4.0 };
        apply_params(&mut session, request).expect("apply");
        let node = session.project().light(light).expect("light");
        assert_eq!(node.kind(), LightKind::Point);
        assert_eq!(node.color(), [0.2, 0.3, 0.4]);
        assert_eq!(node.intensity(), 4.0);
        session.undo().expect("undo");
        assert_eq!(session.project().light(light).expect("light").kind(), LightKind::Sun);
    }
}
