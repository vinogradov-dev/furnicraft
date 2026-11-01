//! Contour editing state: a working copy of a panel outline or an object contour.
//!
//! The mode is view state. Entering it reads the stored ring into a draft; moving a corner or
//! splitting a side only changes the draft, so the document, its revision and the undo history stay
//! untouched until the user applies. Applying dispatches exactly one command
//! (`panel.outline.set` or `object.contour.replace`), so the whole edit is one undo entry. Leaving
//! without applying drops the draft and changes nothing.

use furnicraft_engine::object_cmds::OBJECT_CONTOUR_REPLACE_ID;
use furnicraft_engine::panel_cmds::OUTLINE_SET_ID;
use furnicraft_engine::{ObjectContourReplace, PanelOutlineSet, Session};
use furnicraft_model::{Contour, ContourEditError, ContourSegmentId, Id, Project};
use furnicraft_units::Length;

use crate::command::{dispatch, json};

/// The node whose contour the editor is changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourTarget {
    /// The outline ring of a contour panel.
    Panel(Id),
    /// The contour of a constructive object.
    Object(Id),
}

impl ContourTarget {
    /// The node that owns the edited contour.
    pub const fn id(self) -> Id {
        match self {
            Self::Panel(id) | Self::Object(id) => id,
        }
    }

    /// The contour this target stores, or `None` when the node is gone or has none.
    pub fn contour(self, project: &Project) -> Option<&Contour> {
        match self {
            Self::Panel(id) => project.panel(id)?.outline_contour(),
            Self::Object(id) => project.object_contour(id).ok(),
        }
    }
}

/// One live contour edit: the target, the ring it started from and the working copy.
///
/// The base ring lets the viewport show what changed and lets [`ContourEdit::reset`] throw the
/// draft away without leaving the mode. A corner or a split only ever edits the draft.
#[derive(Clone, Debug)]
pub struct ContourEdit {
    target: ContourTarget,
    base: Contour,
    draft: Contour,
}

impl ContourEdit {
    /// Starts editing `target`, or returns `None` when it has no contour.
    pub fn start(project: &Project, target: ContourTarget) -> Option<Self> {
        let base = target.contour(project)?.clone();
        Some(Self { target, base: base.clone(), draft: base })
    }

    /// Whether the target still has a contour to edit.
    pub fn is_editable(project: &Project, target: ContourTarget) -> bool {
        target.contour(project).is_some()
    }

    /// The node being edited.
    pub const fn target(&self) -> ContourTarget {
        self.target
    }

    /// The ring the edit started from.
    pub const fn base(&self) -> &Contour {
        &self.base
    }

    /// The working copy the viewport shows.
    pub const fn draft(&self) -> &Contour {
        &self.draft
    }

    /// Whether the draft differs from the stored ring.
    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    /// Throws the draft away and starts again from the stored ring.
    pub fn reset(&mut self) {
        self.draft = self.base.clone();
    }

    /// Moves the corner at the start of `segment` to `point` in the target's local plane.
    pub fn move_corner(&mut self, segment: ContourSegmentId, point: [Length; 2]) -> Result<(), ContourEditError> {
        let moved = self.draft.move_corner(segment, point)?;
        self.draft = moved;
        Ok(())
    }

    /// Splits `segment` at `point` and returns the id of the new part.
    pub fn split_segment(
        &mut self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<ContourSegmentId, ContourEditError> {
        let (split, inserted) = self.draft.split_segment(segment, point)?;
        self.draft = split;
        Ok(inserted)
    }

    /// Applies the draft as one command.
    ///
    /// The caller owns the mode and drops it only on success; a rejected edit leaves the draft in
    /// place so the user can fix it or cancel.
    pub fn apply(&self, session: &mut Session) -> Result<(), String> {
        match self.target {
            ContourTarget::Panel(panel) => {
                let params = json(&PanelOutlineSet { panel, contour: self.draft.clone() })?;
                dispatch(session, OUTLINE_SET_ID, &params).map(|_| ())
            }
            ContourTarget::Object(object) => {
                let params = json(&ObjectContourReplace { object, contour: self.draft.clone() })?;
                dispatch(session, OBJECT_CONTOUR_REPLACE_ID, &params).map(|_| ())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::NodeDelete;
    use furnicraft_engine::node_cmds::NODE_DELETE_ID;
    use furnicraft_model::{Constructive, Container, Geometry, Panel, Placement, Project};
    use furnicraft_testkit::{sample_catalog, sample_panel_thickness, sample_project};

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    fn rectangle(width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(width), millimetres(0)],
            [millimetres(width), millimetres(height)],
            [millimetres(0), millimetres(height)],
        ])
        .expect("rectangle")
    }

    /// A project whose root holds one contour panel.
    fn panel_project() -> (Project, Id) {
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Shelf",
            rectangle(700, 500),
            None,
            sample_panel_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        (Project::new(root).expect("project"), Id::new(2).expect("id"))
    }

    /// A project whose root holds one constructive object with a rectangular contour.
    fn object_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let constructive =
            Constructive::extrude(rectangle(700, 500), sample_panel_thickness()).expect("constructive object");
        let id = project
            .add_object(root, "Counter", Placement::IDENTITY, Geometry::Constructive(constructive))
            .expect("object");
        (project, id)
    }

    fn session(project: Project) -> Session {
        Session::with_catalog(project, sample_catalog())
    }

    #[test]
    fn a_panel_edit_works_on_a_copy_and_applies_in_one_command() {
        let (project, panel) = panel_project();
        let mut session = session(project);
        let target = ContourTarget::Panel(panel);
        let mut edit = ContourEdit::start(session.project(), target).expect("a contour panel is editable");
        assert_eq!(edit.target(), target);
        assert!(!edit.is_dirty());

        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(800), millimetres(0)]).expect("move the corner");
        assert!(edit.is_dirty());
        assert_eq!(edit.draft().bounds_micrometres().1[0], millimetres(800).micrometres());
        assert_eq!(
            session.project().panel(panel).expect("panel").outline_contour(),
            Some(edit.base()),
            "the document still has the stored ring"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the outline");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), Some(edit.draft()));
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the ring");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), Some(edit.base()));
    }

    #[test]
    fn reset_and_split_edit_only_the_draft() {
        let (project, panel) = panel_project();
        let mut edit = ContourEdit::start(&project, ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        let inserted = edit.split_segment(segment, [millimetres(350), millimetres(0)]).expect("split");
        assert_eq!(edit.draft().segments().len(), 5);
        assert!(edit.draft().index_of(inserted).is_some());
        assert!(edit.is_dirty());

        edit.reset();
        assert!(!edit.is_dirty());
        assert_eq!(edit.draft(), edit.base());
        assert_eq!(edit.draft().segments().len(), 4);
    }

    #[test]
    fn an_object_edit_replaces_the_object_contour() {
        let (project, object) = object_project();
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Object(object)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        edit.move_corner(segment, [millimetres(-100), millimetres(0)]).expect("move the corner");
        edit.apply(&mut session).expect("apply the contour");
        assert_eq!(
            session.project().object_contour(object).expect("contour"),
            edit.draft(),
            "an object contour is stored exactly as edited"
        );
    }

    #[test]
    fn a_rectangle_panel_has_no_contour_to_edit() {
        let project = sample_project();
        let rectangle_panel = Id::new(2).expect("id");
        assert!(ContourEdit::start(&project, ContourTarget::Panel(rectangle_panel)).is_none());
        assert!(!ContourEdit::is_editable(&project, ContourTarget::Panel(rectangle_panel)));
        assert!(ContourEdit::start(&project, ContourTarget::Object(Id::new(99).expect("id"))).is_none());
    }

    #[test]
    fn a_rejected_apply_leaves_the_document_alone() {
        let (project, panel) = panel_project();
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(800), millimetres(0)]).expect("move the corner");

        let params = serde_json::to_value(NodeDelete { node: panel }).expect("params");
        session.dispatch(NODE_DELETE_ID, &params).expect("delete the panel under the edit");
        let revision = session.project().revision();
        assert!(edit.apply(&mut session).is_err(), "the panel is gone");
        assert_eq!(session.project().revision(), revision, "a failed apply changes nothing");
        assert!(edit.is_dirty(), "the draft stays so the user can cancel or retry");
    }
}
