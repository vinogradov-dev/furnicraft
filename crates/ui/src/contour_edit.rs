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

use furnicraft_engine::object_cmds::{OBJECT_CONTOUR_REPLACE_ID, OBJECT_PATH_CREATE_ID};
use furnicraft_engine::panel_cmds::OUTLINE_SET_ID;
use furnicraft_engine::{ObjectContourReplace, ObjectPathCreate, PanelOutlineSet, Session};
use furnicraft_model::{
    Contour, ContourEditError, ContourSegmentId, Id, MAX_CONTOUR_COORDINATE, MAX_CONTOUR_SEGMENTS, Path, PathError,
    Placement, Project,
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

/// The tool a drawing draft is building with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawTool {
    /// Two points make one open line.
    Line,
    /// Two or more points make an open chain, or a closed ring on request.
    #[default]
    Polyline,
    /// Three points make one open arc through the middle point.
    Arc,
    /// Two opposite corners make one closed rectangle.
    Rectangle,
    /// A centre and a rim point make one closed circle.
    Circle,
}

impl DrawTool {
    /// How many points the tool needs to produce its path, or `None` for an open-ended polyline.
    ///
    /// A fixed tool stops taking points at this count; a polyline produces a path as soon as it has
    /// two points and takes as many more as the ring cap allows.
    pub const fn points(self) -> Option<usize> {
        match self {
            Self::Line | Self::Rectangle | Self::Circle => Some(2),
            Self::Arc => Some(3),
            Self::Polyline => None,
        }
    }

    /// Whether the tool can close its chain into a ring; only a polyline may.
    pub const fn closes(self) -> bool {
        matches!(self, Self::Polyline)
    }

    /// Whether the path is closed by the tool itself, without a close action.
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Rectangle | Self::Circle)
    }

    /// A short user-visible name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Arc => "Arc",
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
        }
    }
}

/// One drawing tool's chain of work-plane points.
///
/// The points are whole micrometres in the work-plane frame, so a draft becomes a document
/// [`Path`] without a float ever entering the document. A draft is view state: it runs no command
/// and bumps no revision until the drawing is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawDraft {
    tool: DrawTool,
    points: Vec<[Length; 2]>,
    closed: bool,
}

impl Default for DrawDraft {
    fn default() -> Self {
        Self::new(DrawTool::Polyline)
    }
}

impl DrawDraft {
    /// An empty, open draft for `tool`.
    pub const fn new(tool: DrawTool) -> Self {
        Self { tool, points: Vec::new(), closed: false }
    }

    /// The tool this draft belongs to.
    pub const fn tool(&self) -> DrawTool {
        self.tool
    }

    /// The points in the order they were added; a closed chain stores each point once.
    pub fn points(&self) -> &[[Length; 2]] {
        &self.points
    }

    /// Whether the user closed the chain.
    ///
    /// A rectangle or a circle produces a closed path by itself; that is a property of the tool, not
    /// of this flag, and the draft only counts as closed once the tool has all its points.
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

    /// Whether the tool has every point it needs to produce a path.
    pub fn is_complete(&self) -> bool {
        match self.tool.points() {
            Some(need) => self.points.len() == need,
            None => self.points.len() >= 2,
        }
    }

    /// Whether the chain has enough points to close into a ring.
    pub fn can_close(&self) -> bool {
        self.tool.closes() && !self.closed && self.points.len() >= 3
    }

    /// Whether the tool is done with this drawing and it should be committed.
    ///
    /// A fixed tool finishes on its last point; a polyline finishes when the user closes the chain;
    /// an open polyline is committed explicitly, because a chain is a valid drawing.
    pub fn is_finished(&self) -> bool {
        self.closed || self.tool.points().is_some_and(|need| self.points.len() >= need)
    }

    /// Appends a point, refusing a repeat of the previous point, an out-of-range point, a point
    /// beyond a fixed tool's count and an overflow of the ring cap.
    ///
    /// A path may touch or cross itself — the ring rules run only when it becomes a contour — but
    /// two equal neighbours would make a zero-length segment the document cannot store.
    pub fn push(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if let Some(need) = self.tool.points()
            && self.points.len() >= need
        {
            return Err(DrawDraftError::Complete { tool: self.tool });
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
    /// Only a polyline can close; a rectangle and a circle are closed by their constructors and a
    /// line and an arc stay open. Three points are the fewest a ring can have, so a shorter chain is
    /// refused. The closed chain is validated before the state changes, so a draft that cannot
    /// become a path — for example because its last point repeats the first — stays open and
    /// editable. Whether the ring is simple and non-degenerate is a ring rule that runs when the
    /// path becomes a contour.
    pub fn close(&mut self) -> Result<(), DrawDraftError> {
        if !self.tool.closes() {
            return Err(DrawDraftError::NotClosable { tool: self.tool });
        }
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if self.points.len() < 3 {
            return Err(DrawDraftError::TooFewPoints { need: 3, got: self.points.len() });
        }
        self.path_with(None, true)?;
        self.closed = true;
        Ok(())
    }

    /// The path these points describe, if the tool has all the points it needs.
    pub fn to_path(&self) -> Result<Path, DrawDraftError> {
        self.path_with(None, self.closed)
    }

    /// The path the draft would have if `next` were added, or `None` when the draft already holds
    /// every point the tool takes.
    ///
    /// This is the preview: a rectangle shows its fourth side and a circle its whole rim while the
    /// pointer still moves. A polyline previews the next point as one more segment without closing.
    pub fn preview_path(&self, next: [Length; 2]) -> Option<Path> {
        if self.closed || !point_in_range(next) {
            return None;
        }
        if let Some(need) = self.tool.points()
            && self.points.len() >= need
        {
            return None;
        }
        if self.points.len() >= MAX_CONTOUR_SEGMENTS {
            return None;
        }
        if self.points.last() == Some(&next) {
            return None;
        }
        self.path_with(Some(next), false).ok()
    }

    /// The path with an optional extra point, built by the draft's tool.
    fn path_with(&self, next: Option<[Length; 2]>, closed: bool) -> Result<Path, DrawDraftError> {
        let mut points = Vec::with_capacity(self.points.len() + 1);
        points.extend_from_slice(&self.points);
        if let Some(next) = next {
            points.push(next);
        }
        let need = self.tool.points().unwrap_or(2);
        if points.len() < need {
            return Err(DrawDraftError::TooFewPoints { need, got: points.len() });
        }
        match self.tool {
            DrawTool::Line => Path::polyline(points, false).map_err(DrawDraftError::from),
            DrawTool::Polyline => Path::polyline(points, closed).map_err(DrawDraftError::from),
            DrawTool::Arc => {
                let (Some(&start), Some(&through), Some(&end)) = (points.first(), points.get(1), points.get(2)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 3, got: points.len() });
                };
                Path::arc_through(start, through, end).map_err(DrawDraftError::from)
            }
            DrawTool::Rectangle => {
                let (Some(&first), Some(&second)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::rectangle(first, second).map_err(DrawDraftError::from)
            }
            DrawTool::Circle => {
                let (Some(&center), Some(&rim)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::circle_through(center, rim).map_err(DrawDraftError::from)
            }
        }
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
    /// A fixed tool already has every point it needs.
    #[error("the {} tool is complete", tool.label())]
    Complete {
        /// The tool that is complete.
        tool: DrawTool,
    },
    /// Only a polyline can be closed by the user.
    #[error("the {} tool cannot be closed", tool.label())]
    NotClosable {
        /// The tool that refused the close.
        tool: DrawTool,
    },
    /// The points do not form a path.
    #[error(transparent)]
    Path(#[from] PathError),
}

/// A path committed to the document, with the tool that drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawnPath {
    /// The created node.
    pub object: Id,
    /// The tool that produced the path.
    pub tool: DrawTool,
}

impl DrawDraft {
    /// Commits the drawing as one `object.path.create`.
    ///
    /// The path is placed on the work plane: `placement` maps local X and Y onto the plane's `u` and
    /// `v`, so the stored coordinates are the visible drawing. A fixed tool commits its finished
    /// path, a polyline commits what it has whether it was closed or not. On success the caller
    /// drops the draft; a rejected path leaves it intact so the user can undo or cancel.
    pub fn commit(&self, session: &mut Session, parent: Id, placement: Placement) -> Result<Id, String> {
        let path = self.to_path().map_err(|error| error.to_string())?;
        let name = format!("{} drawing", self.tool.label());
        let params = json(&ObjectPathCreate { parent, name, path, placement })?;
        let outcome = dispatch(session, OBJECT_PATH_CREATE_ID, &params)?;
        outcome.created_id().ok_or_else(|| "the path command created no node".to_owned())
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

    #[test]
    fn a_drawing_draft_builds_an_open_path_from_its_points() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("point");
        assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 1 }));
        assert_eq!(draft.close(), Err(DrawDraftError::TooFewPoints { need: 3, got: 1 }));
        assert!(!draft.can_close());
    }

    #[test]
    fn undoing_a_drawing_draft_reopens_before_it_drops_a_point() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
        let mut draft = DrawDraft::new(DrawTool::Polyline);
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
    fn the_line_tool_makes_one_open_segment() {
        let mut draft = DrawDraft::new(DrawTool::Line);
        assert_eq!(draft.tool(), DrawTool::Line);
        assert!(!draft.is_complete());
        draft.push([millimetres(0), millimetres(0)]).expect("first point");
        assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 1 }));
        draft.push([millimetres(600), millimetres(0)]).expect("second point");
        assert!(draft.is_complete());
        assert_eq!(
            draft.push([millimetres(600), millimetres(400)]),
            Err(DrawDraftError::Complete { tool: DrawTool::Line }),
            "a fixed tool stops at its point count"
        );

        let path = draft.to_path().expect("a line");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 1);
        assert_eq!(draft.close(), Err(DrawDraftError::NotClosable { tool: DrawTool::Line }), "a line is always open");
    }

    #[test]
    fn the_arc_tool_makes_one_open_arc_through_its_middle_point() {
        let mut draft = DrawDraft::new(DrawTool::Arc);
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(50), millimetres(50)], [millimetres(100), millimetres(0)]]
        {
            draft.push(point).expect("an arc point");
        }
        assert!(draft.is_complete());
        assert_eq!(
            draft.push([millimetres(0), millimetres(100)]),
            Err(DrawDraftError::Complete { tool: DrawTool::Arc })
        );
        let path = draft.to_path().expect("an arc");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 1);
        assert!(path.entries()[0].segment().is_arc());
    }

    #[test]
    fn the_rectangle_tool_closes_two_corners() {
        let mut draft = DrawDraft::new(DrawTool::Rectangle);
        draft.push([millimetres(600), millimetres(400)]).expect("first corner");
        draft.push([millimetres(0), millimetres(0)]).expect("opposite corner");
        assert!(draft.tool().is_closed(), "the tool produces a closed path");
        assert!(!draft.is_closed(), "the user did not close anything");
        assert!(!draft.can_close(), "there is nothing left to close");
        let path = draft.to_path().expect("a rectangle");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4);
        assert_eq!(path.bounds_micrometres(), ([0, 0], [600_000, 400_000]));
    }

    #[test]
    fn the_circle_tool_closes_a_centre_and_a_rim_point() {
        let mut draft = DrawDraft::new(DrawTool::Circle);
        draft.push([millimetres(100), millimetres(50)]).expect("centre");
        draft.push([millimetres(130), millimetres(90)]).expect("rim");
        assert!(draft.is_complete());
        let path = draft.to_path().expect("a circle");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4);
        assert!(path.entries().iter().all(|entry| entry.segment().is_arc()));
        assert_eq!(path.bounds_micrometres(), ([50_000, 0], [150_000, 100_000]));
    }

    #[test]
    fn the_polyline_tool_takes_points_until_it_is_closed() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        assert!(!draft.is_complete(), "a polyline needs two points");
        draft.push([millimetres(0), millimetres(0)]).expect("first");
        assert!(!draft.is_complete());
        draft.push([millimetres(600), millimetres(0)]).expect("second");
        assert!(draft.is_complete());
        draft.push([millimetres(600), millimetres(400)]).expect("a polyline keeps taking points");
        assert!(draft.can_close());
        draft.close().expect("close the triangle");
        let path = draft.to_path().expect("a closed polyline");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 3);
    }

    #[test]
    fn a_draft_finishes_when_its_tool_is_done() {
        let mut line = DrawDraft::new(DrawTool::Line);
        line.push([millimetres(0), millimetres(0)]).expect("first");
        assert!(!line.is_finished());
        line.push([millimetres(600), millimetres(0)]).expect("second");
        assert!(line.is_finished(), "a complete line commits on its last click");

        let mut polyline = DrawDraft::new(DrawTool::Polyline);
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
        {
            polyline.push(point).expect("point");
        }
        assert!(!polyline.is_finished(), "an open chain waits for close or commit");
        polyline.close().expect("close");
        assert!(polyline.is_finished());

        let mut circle = DrawDraft::new(DrawTool::Circle);
        circle.push([millimetres(100), millimetres(50)]).expect("centre");
        assert!(!circle.is_finished());
        circle.push([millimetres(130), millimetres(90)]).expect("rim");
        assert!(circle.is_finished());
    }

    #[test]
    fn every_tool_states_its_point_count_and_name() {
        let cases = [
            (DrawTool::Line, Some(2), false, false, "Line"),
            (DrawTool::Polyline, None, true, true, "Polyline"),
            (DrawTool::Arc, Some(3), false, false, "Arc"),
            (DrawTool::Rectangle, Some(2), false, false, "Rectangle"),
            (DrawTool::Circle, Some(2), false, false, "Circle"),
        ];
        for (tool, need, closes, open_ended, label) in cases {
            assert_eq!(tool.points(), need, "{}", tool.label());
            assert_eq!(tool.closes(), closes, "{}", tool.label());
            assert_eq!(tool.points().is_none(), open_ended, "{}", tool.label());
            assert_eq!(tool.label(), label);
            assert!(!DrawDraft::new(tool).is_complete(), "a fresh {} draft is empty", tool.label());
        }
    }

    #[test]
    fn a_drawing_draft_shows_its_reason_to_a_user() {
        assert_eq!(DrawDraftError::NotStarted.to_string(), "no drawing is in progress");
        assert_eq!(
            DrawDraftError::TooFewPoints { need: 3, got: 1 }.to_string(),
            "the drawing needs at least 3 points, it has 1"
        );
        assert_eq!(DrawDraftError::Complete { tool: DrawTool::Arc }.to_string(), "the Arc tool is complete");
        assert_eq!(DrawDraftError::NotClosable { tool: DrawTool::Line }.to_string(), "the Line tool cannot be closed");
        assert!(!DrawDraftError::Full.to_string().is_empty());
        assert!(!DrawDraftError::DuplicatePoint.to_string().is_empty());
        assert!(!DrawDraftError::PointOutOfRange.to_string().is_empty());
        assert!(!DrawDraftError::AlreadyClosed.to_string().is_empty());
    }
}
