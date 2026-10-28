//! The selected camera card.
//!
//! A camera is a scene object ([ADR 0036]): the card renames it, edits its projection and can make
//! it the camera the viewport renders from. Projection text is kept until Apply, so one edit is one
//! undo entry; the active-camera toggle is a single command of its own.
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use egui::{TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::scene_cmds::{CAMERA_PROJECTION_SET_ID, SCENE_ACTIVE_CAMERA_SET_ID};
use furnicraft_engine::{CameraProjectionSet, NodeRename, SceneActiveCameraSet, Session};
use furnicraft_model::{CameraProjection, Id};
use furnicraft_units::{Degrees, Length};

use crate::command::{dispatch, json};
use crate::section::section;

/// Persistent state of the camera card.
#[derive(Debug, Default)]
pub(crate) struct CameraCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// Text being edited and the projection it was read from.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    camera: Id,
    base: CameraProjection,
    name: String,
    perspective: bool,
    fov: String,
    height: String,
}

impl Draft {
    fn new(camera: Id, name: &str, projection: CameraProjection) -> Self {
        let (perspective, fov, height) = match projection {
            CameraProjection::Perspective { fov_y } => (true, fov_y.to_string(), String::new()),
            CameraProjection::Orthographic { height } => (false, String::new(), height.to_string()),
        };
        Self { camera, base: projection, name: name.to_owned(), perspective, fov, height }
    }
}

impl CameraCard {
    /// Draws the camera card and dispatches accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, camera_id: Id) {
        let Some(camera) = session.project().camera(camera_id) else {
            self.draft = None;
            self.error = None;
            return;
        };
        let name = camera.name().to_owned();
        let projection = camera.projection();
        if self
            .draft
            .as_ref()
            .is_none_or(|draft| draft.camera != camera_id || draft.base != projection || draft.name != name)
        {
            self.error = None;
            self.draft = Some(Draft::new(camera_id, &name, projection));
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "camera.name", "Name", true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button("Rename").clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some("Camera name must not be empty.".to_owned());
                    } else if trimmed != name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });

        let mut apply = false;
        ui.separator();
        section(ui, "camera.projection", "Projection", true, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut draft.perspective, true, "Perspective");
                ui.radio_value(&mut draft.perspective, false, "Orthographic");
            });
            if draft.perspective {
                ui.horizontal(|ui| {
                    ui.label("Field of view");
                    ui.add(TextEdit::singleline(&mut draft.fov).desired_width(96.0).hint_text("50°"));
                });
                ui.label(format!(
                    "Between {} and {}.",
                    CameraProjectionBounds::MIN_FOV,
                    CameraProjectionBounds::MAX_FOV
                ));
            } else {
                ui.horizontal(|ui| {
                    ui.label("Height");
                    ui.add(TextEdit::singleline(&mut draft.height).desired_width(96.0).hint_text("2000 mm"));
                });
                ui.label(format!("Positive and at most {}.", CameraProjectionBounds::MAX_HEIGHT));
            }
            if ui.button("Apply projection").clicked() {
                apply = true;
            }
        });

        // Take a snapshot so the document borrow ends before a command runs.
        let fields = draft.clone();
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }

        if let Some(name) = rename {
            match apply_rename(session, camera_id, name) {
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
            match projection_from_draft(&fields).and_then(|next| apply_projection(session, camera_id, next)) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
        } else if let Some(error) = name_error {
            self.error = Some(error);
        }

        let mut active = session.project().active_camera() == Some(camera_id);
        ui.separator();
        section(ui, "camera.active", "Render view", true, |ui| {
            if ui.checkbox(&mut active, "Render the scene from this camera").changed() {
                let request = SceneActiveCameraSet { camera: active.then_some(camera_id) };
                if let Err(error) = set_active_camera(session, request) {
                    self.error = Some(error);
                }
                ui.ctx().request_repaint();
            }
        });
    }
}

/// Ranges shown in the card; they mirror the validation in the model.
struct CameraProjectionBounds;

impl CameraProjectionBounds {
    const MIN_FOV: Degrees = Degrees::from_millidegrees(1_000);
    const MAX_FOV: Degrees = Degrees::from_millidegrees(179_000);
    const MAX_HEIGHT: Length = Length::from_micrometres(1_000_000_000);
}

/// Reads the draft text into a projection, refusing text that is not a number or is outside range.
fn projection_from_draft(draft: &Draft) -> Result<CameraProjection, String> {
    if draft.perspective {
        let fov_y = Degrees::parse_degrees(&draft.fov).map_err(|error| format!("Field of view: {error}."))?;
        let projection = CameraProjection::Perspective { fov_y };
        if !projection.is_valid() {
            return Err(format!(
                "Field of view must be between {} and {}.",
                CameraProjectionBounds::MIN_FOV,
                CameraProjectionBounds::MAX_FOV
            ));
        }
        Ok(projection)
    } else {
        let height =
            Length::parse_millimetres(&draft.height).map_err(|error| format!("Orthographic height: {error}."))?;
        let projection = CameraProjection::Orthographic { height };
        if !projection.is_valid() {
            return Err(format!(
                "Orthographic height must be positive and at most {}.",
                CameraProjectionBounds::MAX_HEIGHT
            ));
        }
        Ok(projection)
    }
}

fn apply_rename(session: &mut Session, camera: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node: camera, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

fn apply_projection(session: &mut Session, camera: Id, projection: CameraProjection) -> Result<(), String> {
    let params = json(&CameraProjectionSet { camera, projection })?;
    dispatch(session, CAMERA_PROJECTION_SET_ID, &params).map(|_| ())
}

fn set_active_camera(session: &mut Session, request: SceneActiveCameraSet) -> Result<(), String> {
    let params = json(&request)?;
    dispatch(session, SCENE_ACTIVE_CAMERA_SET_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::CameraCreate;
    use furnicraft_engine::scene_cmds::CAMERA_CREATE_ID;
    use furnicraft_model::Placement;
    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn camera_session() -> (Session, Id) {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let params = json(&CameraCreate {
            parent: root,
            name: "Camera".to_owned(),
            placement: Placement::IDENTITY,
            projection: CameraProjection::DEFAULT,
        })
        .expect("params");
        let camera =
            dispatch(&mut session, CAMERA_CREATE_ID, &params).expect("add camera").created_id().expect("a camera id");
        (session, camera)
    }

    #[test]
    fn the_draft_shows_and_parses_the_current_projection() {
        let (_, camera) = camera_session();
        let draft = Draft::new(camera, "Camera", CameraProjection::DEFAULT);
        assert!(draft.perspective);
        assert_eq!(draft.fov, "50°");
        assert_eq!(projection_from_draft(&draft), Ok(CameraProjection::DEFAULT));

        let draft = Draft {
            camera,
            base: CameraProjection::DEFAULT,
            name: "Camera".to_owned(),
            perspective: false,
            fov: String::new(),
            height: "2000 mm".to_owned(),
        };
        assert_eq!(
            projection_from_draft(&draft),
            Ok(CameraProjection::Orthographic { height: Length::from_millimetres(2_000).expect("height") })
        );
    }

    #[test]
    fn a_text_or_range_failure_is_reported_instead_of_dispatching() {
        let (session, camera) = camera_session();
        let before = session.project().revision();
        let bad = Draft {
            camera,
            base: CameraProjection::DEFAULT,
            name: "Camera".to_owned(),
            perspective: true,
            fov: "wide".to_owned(),
            height: String::new(),
        };
        assert!(projection_from_draft(&bad).is_err());

        let outside = Draft { fov: "0°".to_owned(), ..bad.clone() };
        assert!(projection_from_draft(&outside).is_err());
        let zero_height = Draft { perspective: false, height: "0 mm".to_owned(), ..bad };
        assert!(projection_from_draft(&zero_height).is_err());
        assert_eq!(session.project().revision(), before);
    }

    #[test]
    fn the_active_camera_toggle_dispatches_one_command() {
        let (mut session, camera) = camera_session();
        set_active_camera(&mut session, SceneActiveCameraSet { camera: Some(camera) }).expect("activate");
        assert_eq!(session.project().active_camera(), Some(camera));
        set_active_camera(&mut session, SceneActiveCameraSet { camera: None }).expect("clear");
        assert_eq!(session.project().active_camera(), None);
        assert!(session.can_undo());
    }
}
