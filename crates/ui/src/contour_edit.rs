//! Contour and drawing state: a working copy of a panel outline or an object contour, and the
//! chain a drawing tool is building.
//!
//! Both are view state. Entering a contour edit reads the stored ring into a draft; moving a corner
//! or splitting a side only changes the draft, so the document, its revision and the undo history
//! stay untouched until the user applies. Applying dispatches exactly one command
//! (`panel.outline.set` or `object.contour.replace`), so the whole edit is one undo entry. Leaving
//! without applying drops the draft and changes nothing.
//!
//! A [`DrawDraft`] is the same pattern without a base ring: the tool appends plane points, undoes
//! the last one, closes the chain or cancels it, and no command runs until the drawing is committed.

use furnicraft_engine::object_cmds::OBJECT_CONTOUR_REPLACE_ID;
use furnicraft_engine::panel_cmds::OUTLINE_SET_ID;
use furnicraft_engine::{ObjectContourReplace, PanelOutlineSet, Session};
use furnicraft_model::{
    Contour, ContourEditError, ContourSegment, ContourSegmentId, Id, MAX_CONTOUR_COORDINATE, MAX_CONTOUR_SEGMENTS,
    Path, PathError, Project,
};
use furnicraft_units::Length;
use thiserror::Error;

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

/// A chain of work-plane points a drawing tool is building.
///
/// The points are whole micrometres in the work-plane frame, so a draft becomes a document
/// [`Path`] without a float ever entering the document. A draft is view state: it runs no command
/// and bumps no revision until the drawing is committed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawDraft {
    points: Vec<[Length; 2]>,
    closed: bool,
}

impl DrawDraft {
    /// An empty, open draft.
    pub const fn new() -> Self {
        Self { points: Vec::new(), closed: false }
    }

    /// The points in the order they were added; a closed chain stores each point once.
    pub fn points(&self) -> &[[Length; 2]] {
        &self.points
    }

    /// Whether the chain is closed back to its first point.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Whether the draft holds no point yet.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The number of points, not segments.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether the chain has enough points to close into a ring.
    pub fn can_close(&self) -> bool {
        !self.closed && self.points.len() >= 3
    }

    /// Appends a point, refusing a repeat of the previous point, an out-of-range point and an
    /// overflow of the ring cap.
    ///
    /// A path may touch or cross itself — the ring rules run only when it becomes a contour — but
    /// two equal neighbours would make a zero-length segment the document cannot store.
    pub fn push(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if self.points.len() >= MAX_CONTOUR_SEGMENTS {
            return Err(DrawDraftError::Full);
        }
        if !point_in_range(point) {
            return Err(DrawDraftError::PointOutOfRange);
        }
        if self.points.last() == Some(&point) {
            return Err(DrawDraftError::DuplicatePoint);
        }
        self.points.push(point);
        Ok(())
    }

    /// Removes the last action: a close first, then the last point.
    ///
    /// Returns `false` for an empty, open draft.
    pub fn undo(&mut self) -> bool {
        if self.closed {
            self.closed = false;
            return true;
        }
        self.points.pop().is_some()
    }

    /// Closes the chain back to its first point.
    ///
    /// Three points are the fewest a ring can have, so a shorter chain is refused. The closed
    /// chain is validated before the state changes, so a draft that cannot become a path — for
    /// example because its last point repeats the first — stays open and editable. Whether the ring
    /// is simple and non-degenerate is a ring rule that runs when the path becomes a contour.
    pub fn close(&mut self) -> Result<(), DrawDraftError> {
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if self.points.len() < 3 {
            return Err(DrawDraftError::TooFewPoints { need: 3, got: self.points.len() });
        }
        self.path_of(true)?;
        self.closed = true;
        Ok(())
    }

    /// The path these points describe, open or closed.
    pub fn to_path(&self) -> Result<Path, DrawDraftError> {
        self.path_of(self.closed)
    }

    /// Builds the path with an explicit closing flag and validates it.
    fn path_of(&self, closed: bool) -> Result<Path, DrawDraftError> {
        if self.points.len() < 2 {
            return Err(DrawDraftError::TooFewPoints { need: 2, got: self.points.len() });
        }
        let mut segments = Vec::with_capacity(self.points.len());
        for pair in self.points.windows(2) {
            let (Some(&start), Some(&end)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            segments.push(ContourSegment::line(start, end).map_err(PathError::from)?);
        }
        if closed && let (Some(&first), Some(&last)) = (self.points.first(), self.points.last()) {
            segments.push(ContourSegment::line(last, first).map_err(PathError::from)?);
        }
        Ok(Path::new(segments, closed)?)
    }
}

/// Whether a point lies inside the coordinate range the document accepts.
fn point_in_range(point: [Length; 2]) -> bool {
    let max = MAX_CONTOUR_COORDINATE.micrometres();
    point.iter().all(|coordinate| {
        let value = coordinate.micrometres();
        (-max..=max).contains(&value)
    })
}

/// Why a drawing draft refused an edit.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DrawDraftError {
    /// No drawing is in progress.
    #[error("no drawing is in progress")]
    NotStarted,
    /// A point lies outside the coordinate range the document accepts.
    #[error("the point is outside the drawing range")]
    PointOutOfRange,
    /// The point repeats the previous one, so the segment would have no length.
    #[error("a drawing point must differ from the previous one")]
    DuplicatePoint,
    /// The draft already holds the largest chain one contour ring can take.
    #[error("a drawing holds at most {MAX_CONTOUR_SEGMENTS} points")]
    Full,
    /// The draft needs more points for this action.
    #[error("the drawing needs at least {need} points, it has {got}")]
    TooFewPoints {
        /// The smallest accepted count.
        need: usize,
        /// The current count.
        got: usize,
    },
    /// The chain is already closed.
    #[error("the drawing is already closed")]
    AlreadyClosed,
    /// The points do not form a path.
    #[error(transparent)]
    Path(#[from] PathError),
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

    #[test]
    fn a_drawing_draft_builds_an_open_path_from_its_points() {
        let mut draft = DrawDraft::new();
        assert!(draft.is_empty());
        assert!(!draft.can_close());
        assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 0 }));

        let points = [
            [millimetres(0), millimetres(0)],
            [millimetres(600), millimetres(0)],
            [millimetres(600), millimetres(400)],
        ];
        for point in points {
            draft.push(point).expect("a fresh point");
        }
        assert_eq!(draft.len(), 3);
        assert!(!draft.is_closed());
        assert!(draft.can_close(), "three points enclose an area");

        let path = draft.to_path().expect("an open path");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 2);
        let mut ends = path.segments().map(|segment| segment.start().expect("start"));
        assert_eq!(ends.next(), Some(points[0]));
        assert_eq!(path.entries().last().and_then(|entry| entry.segment().end().ok()), Some(points[2]));
    }

    #[test]
    fn closing_a_drawing_draft_adds_the_last_segment_once() {
        let mut draft = DrawDraft::new();
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
        {
            draft.push(point).expect("point");
        }
        draft.close().expect("a triangle closes");
        assert!(draft.is_closed());
        assert!(!draft.can_close());
        assert_eq!(draft.len(), 3, "a closed chain stores each point once");

        let path = draft.to_path().expect("a closed path");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 3);
        let last = path.entries().last().expect("the closing segment");
        assert_eq!(last.segment().end().ok(), Some([millimetres(0), millimetres(0)]));

        assert_eq!(draft.close(), Err(DrawDraftError::AlreadyClosed));
        assert_eq!(draft.push([millimetres(1), millimetres(1)]), Err(DrawDraftError::AlreadyClosed));
    }

    #[test]
    fn a_drawing_draft_needs_three_points_to_close() {
        let mut draft = DrawDraft::new();
        draft.push([millimetres(0), millimetres(0)]).expect("point");
        assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 1 }));
        assert_eq!(draft.close(), Err(DrawDraftError::TooFewPoints { need: 3, got: 1 }));
        assert!(!draft.can_close());
    }

    #[test]
    fn undoing_a_drawing_draft_reopens_before_it_drops_a_point() {
        let mut draft = DrawDraft::new();
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
        {
            draft.push(point).expect("point");
        }
        draft.close().expect("close");

        assert!(draft.undo(), "the first undo opens the chain");
        assert!(!draft.is_closed());
        assert_eq!(draft.len(), 3, "opening keeps every point");
        assert!(draft.undo(), "the next undo drops the last point");
        assert_eq!(draft.len(), 2);
        assert!(draft.undo());
        assert!(draft.undo());
        assert!(draft.is_empty());
        assert!(!draft.undo(), "an empty open draft has nothing to undo");
    }

    #[test]
    fn a_drawing_draft_refuses_a_repeat_and_a_point_out_of_range() {
        let mut draft = DrawDraft::new();
        draft.push([millimetres(0), millimetres(0)]).expect("point");
        draft.push([millimetres(600), millimetres(0)]).expect("point");
        assert_eq!(draft.push([millimetres(600), millimetres(0)]), Err(DrawDraftError::DuplicatePoint));
        draft.push([millimetres(600), millimetres(400)]).expect("a chain may touch an earlier point");
        draft.push([millimetres(0), millimetres(0)]).expect("a path need not be simple");

        let too_far = MAX_CONTOUR_COORDINATE.checked_add(Length::from_micrometres(1)).expect("length");
        assert_eq!(
            draft.push([too_far, Length::ZERO]),
            Err(DrawDraftError::PointOutOfRange),
            "a point outside the document range never enters a draft"
        );
    }

    #[test]
    fn a_drawing_draft_stops_at_the_ring_cap() {
        let mut draft = DrawDraft::new();
        for index in 0..MAX_CONTOUR_SEGMENTS {
            let x = Length::from_micrometres(i64::try_from(index).expect("index") * 1_000);
            draft.push([x, Length::ZERO]).expect("a point below the cap");
        }
        assert_eq!(draft.len(), MAX_CONTOUR_SEGMENTS);
        let next = Length::from_micrometres(i64::try_from(MAX_CONTOUR_SEGMENTS).expect("index") * 1_000);
        assert_eq!(draft.push([next, Length::ZERO]), Err(DrawDraftError::Full));
    }

    #[test]
    fn a_drawing_draft_refuses_a_close_that_cannot_form_a_path() {
        let mut draft = DrawDraft::new();
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(0), millimetres(0)]]
        {
            draft.push(point).expect("a path need not be simple");
        }
        assert!(draft.close().is_err(), "closing would make a zero-length segment");
        assert!(!draft.is_closed(), "a refused close leaves the draft open and editable");
        assert_eq!(draft.len(), 3);
    }

    #[test]
    fn a_drawing_draft_shows_its_reason_to_a_user() {
        assert_eq!(DrawDraftError::NotStarted.to_string(), "no drawing is in progress");
        assert_eq!(
            DrawDraftError::TooFewPoints { need: 3, got: 1 }.to_string(),
            "the drawing needs at least 3 points, it has 1"
        );
        assert!(!DrawDraftError::Full.to_string().is_empty());
        assert!(!DrawDraftError::DuplicatePoint.to_string().is_empty());
        assert!(!DrawDraftError::PointOutOfRange.to_string().is_empty());
        assert!(!DrawDraftError::AlreadyClosed.to_string().is_empty());
    }
}
