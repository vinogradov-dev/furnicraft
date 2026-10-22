//! The editing session: document, command dispatch and undo/redo.

use furnicraft_model::Project;
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandRegistry};

/// A document plus its undo/redo history and saved-content marker.
#[derive(Debug)]
pub struct Session {
    project: Project,
    saved_project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    dirty: bool,
}

impl Session {
    /// Creates a clean session around a document.
    pub fn new(project: Project) -> Self {
        let saved_project = project.clone();
        Self { project, saved_project, undo: Vec::new(), redo: Vec::new(), dirty: false }
    }

    /// The current document. Commands are the only way to mutate it.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Whether an earlier document state can be restored.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether an undone command can be reapplied.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the document differs from the last saved content.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks the current content as saved.
    pub fn mark_saved(&mut self) {
        self.saved_project = self.project.clone();
        self.dirty = false;
    }

    /// Dispatches a command by id with JSON parameters.
    ///
    /// The command runs on a copy of the document. A successful command becomes one undo entry;
    /// a failed command leaves the current document untouched.
    pub fn dispatch(&mut self, id: &str, params: &Value) -> Result<CommandOutcome, CommandError> {
        let spec = CommandRegistry::new().get(id).ok_or_else(|| CommandError::UnknownCommand { id: id.to_owned() })?;
        let mut working = self.project.clone();
        let outcome = spec.run(&mut working, params)?;
        let previous = std::mem::replace(&mut self.project, working);
        self.undo.push(previous);
        self.redo.clear();
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(outcome)
    }

    /// Restores the content from before the last command and advances the revision.
    pub fn undo(&mut self) -> Result<(), CommandError> {
        let snapshot = self.undo.pop().ok_or(CommandError::NothingToUndo)?;
        let current = self.project.clone();
        if let Err(error) = self.project.restore_from(&snapshot) {
            self.undo.push(snapshot);
            return Err(CommandError::Model(error));
        }
        self.redo.push(current);
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(())
    }

    /// Reapplies the last undone command and advances the revision.
    pub fn redo(&mut self) -> Result<(), CommandError> {
        let snapshot = self.redo.pop().ok_or(CommandError::NothingToRedo)?;
        let current = self.project.clone();
        if let Err(error) = self.project.restore_from(&snapshot) {
            self.redo.push(snapshot);
            return Err(CommandError::Model(error));
        }
        self.undo.push(current);
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use furnicraft_model::{Id, Placement, ProjectError, Revision};
    use furnicraft_testkit::{sample_panel_size, sample_project};

    use crate::panel_cmds::PanelCreate;

    fn panel_create(session: &Session, name: &str) -> PanelCreate {
        PanelCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            size: sample_panel_size(),
            placement: Placement::IDENTITY,
        }
    }

    fn json_params(value: &PanelCreate) -> Value {
        serde_json::to_value(value).expect("params serialize")
    }

    #[test]
    fn dispatch_panel_create_changes_the_document_once() {
        let mut session = Session::new(sample_project());
        let params = json_params(&panel_create(&session, "Back"));

        let outcome = session.dispatch("panel.create", &params).expect("create panel");

        assert_eq!(outcome.created_id(), Some(Id::new(5).expect("id")));
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().root().children().len(), 3);
        assert!(session.can_undo());
        assert!(!session.can_redo());
        assert!(session.is_dirty());
    }

    #[test]
    fn undo_and_redo_restore_the_document_and_ids() {
        let mut session = Session::new(sample_project());
        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2);
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().next_id().get(), 6);
        assert!(!session.can_undo());
        assert!(session.can_redo());
        assert!(!session.is_dirty());

        session.redo().expect("redo");
        assert_eq!(session.project().revision().get(), 3);
        assert_eq!(session.project().root().children().len(), 3);
        assert_eq!(session.project().next_id().get(), 6);
        assert!(session.can_undo());
        assert!(!session.can_redo());
        assert!(session.is_dirty());
    }

    #[test]
    fn mark_saved_tracks_content() {
        let mut session = Session::new(sample_project());
        assert!(!session.is_dirty());

        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");
        assert!(session.is_dirty());

        session.mark_saved();
        assert!(!session.is_dirty());

        session.undo().expect("undo");
        assert!(session.is_dirty());
    }

    #[test]
    fn new_command_clears_redo() {
        let mut session = Session::new(sample_project());
        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");
        session.undo().expect("undo");
        assert!(session.can_redo());

        let params = json_params(&panel_create(&session, "Side"));
        session.dispatch("panel.create", &params).expect("create another panel");
        assert!(!session.can_redo());
    }

    #[test]
    fn empty_history_reports_readable_errors() {
        let mut session = Session::new(sample_project());
        assert_eq!(session.undo(), Err(CommandError::NothingToUndo));
        assert_eq!(session.redo(), Err(CommandError::NothingToRedo));
    }

    #[test]
    fn unknown_command_does_not_change_the_document() {
        let mut session = Session::new(sample_project());
        let error = session.dispatch("missing.command", &Value::Null).expect_err("unknown command");
        assert!(matches!(error, CommandError::UnknownCommand { ref id } if id == "missing.command"));
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.is_dirty());
    }

    #[test]
    fn invalid_params_do_not_change_the_document() {
        let mut session = Session::new(sample_project());
        let error = session.dispatch("panel.create", &Value::Null).expect_err("invalid params");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.create", .. }));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn out_of_range_panel_size_is_rejected_before_mutation() {
        let mut session = Session::new(sample_project());
        let mut params = json_params(&panel_create(&session, "Back"));
        let size = params
            .as_object_mut()
            .and_then(|object| object.get_mut("size"))
            .and_then(Value::as_object_mut)
            .expect("size is an object");
        size.insert("length".to_owned(), Value::from(0));

        let error = session.dispatch("panel.create", &params).expect_err("invalid size");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.create", .. }));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn empty_panel_name_is_rejected_before_mutation() {
        let mut session = Session::new(sample_project());
        let params = json_params(&panel_create(&session, "   "));
        let error = session.dispatch("panel.create", &params).expect_err("empty name");
        assert!(matches!(
            error,
            CommandError::InvalidParams { command: "panel.create", ref message } if message.contains("name")
        ));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn missing_parent_is_reported_without_mutation() {
        let mut session = Session::new(sample_project());
        let mut request = panel_create(&session, "Orphan");
        let missing = Id::new(999).expect("id");
        request.parent = missing;
        let params = json_params(&request);

        let error = session.dispatch("panel.create", &params).expect_err("missing parent");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::AssemblyNotFound { id }) if id == missing
        ));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn enabled_rejects_wrong_state_and_params() {
        let session = Session::new(sample_project());
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.create").expect("panel.create is registered");

        let valid = panel_create(&session, "Back");
        assert!(spec.enabled(&session, &json_params(&valid)));

        let mut missing = panel_create(&session, "Orphan");
        missing.parent = Id::new(999).expect("id");
        assert!(!spec.enabled(&session, &json_params(&missing)));

        let empty = panel_create(&session, " ");
        assert!(!spec.enabled(&session, &json_params(&empty)));
        assert!(!spec.enabled(&session, &Value::Null));
    }

    #[test]
    fn panel_create_params_round_trip_and_reject_unknown_fields() {
        let session = Session::new(sample_project());
        let request = panel_create(&session, "Back");
        let value = serde_json::to_value(&request).expect("serialize params");
        let decoded: PanelCreate = serde_json::from_value(value.clone()).expect("deserialize params");
        assert_eq!(decoded, request);

        let mut object = value.as_object().cloned().expect("params are an object");
        object.insert("extra".to_owned(), Value::Null);
        assert!(serde_json::from_value::<PanelCreate>(Value::Object(object)).is_err());
    }
}
