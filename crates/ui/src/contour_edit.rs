//! Contour and drawing state: a working copy of a panel outline, an object contour or a path
//! chain, and the chain a drawing tool is building.
//!
//! Both are view state. Entering a contour edit reads the stored ring or chain into a draft; moving
//! a corner or splitting a side only changes the draft, so the document, its revision and the undo
//! history stay untouched until the user applies. Applying dispatches exactly one command
//! (`panel.outline.set`, `object.contour.replace` or `object.path.replace`), so the whole edit is
//! one undo entry. Leaving without applying drops the draft and changes nothing.
//!
//! A [`DrawDraft`] is the same pattern without a base ring: the tool appends plane points, undoes
//! the last one, closes the chain or cancels it, and no command runs until the drawing is committed.

use furnicraft_engine::object_cmds::{OBJECT_CONTOUR_REPLACE_ID, OBJECT_PATH_CREATE_ID, OBJECT_PATH_REPLACE_ID};
use furnicraft_engine::panel_cmds::{MACHINING_UPDATE_ID, OUTLINE_SET_ID};
use furnicraft_engine::{
    ObjectContourReplace, ObjectPathCreate, ObjectPathReplace, PanelMachiningUpdate, PanelOutlineSet, Session,
};
use furnicraft_model::{
    Contour, ContourEditError, ContourSegment, ContourSegmentEntry, ContourSegmentError, ContourSegmentId, Cutout,
    CutoutShape, Id, MAX_CONTOUR_COORDINATE, MAX_CONTOUR_SEGMENTS, MachiningId, MachiningParams, Path, PathCorner,
    PathError, Placement, Project,
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
    /// The open or closed chain of a path object.
    Path(Id),
    /// The ring of a contour cutout on a panel.
    ///
    /// The panel is also the node whose placement frames the ring, because machining coordinates are
    /// nominal panel-local.
    Cutout {
        /// The panel that owns the cutout.
        panel: Id,
        /// The stable machining record the ring belongs to.
        machining: MachiningId,
    },
}

impl ContourTarget {
    /// The node that owns the edited contour.
    pub const fn id(self) -> Id {
        match self {
            Self::Panel(id) | Self::Object(id) | Self::Path(id) => id,
            Self::Cutout { panel, .. } => panel,
        }
    }

    /// The ring of a contour cutout, when this target names one.
    fn cutout_ring(self, project: &Project) -> Option<&Contour> {
        let Self::Cutout { panel, machining } = self else {
            return None;
        };
        let record = project.panel(panel)?.machining_by_id(machining)?;
        match record.cutout()?.shape() {
            CutoutShape::Contour(contour) => Some(contour),
            CutoutShape::Rectangle(_) => None,
        }
    }

    /// The draft this target stores, or `None` when the node is gone or has none.
    pub fn draft(self, project: &Project) -> Option<ContourDraft> {
        match self {
            Self::Panel(id) => project.panel(id)?.outline_contour().cloned().map(ContourDraft::Ring),
            Self::Object(id) => project.object_contour(id).ok().cloned().map(ContourDraft::Ring),
            Self::Path(id) => project.object_path(id).ok().cloned().map(ContourDraft::Chain),
            Self::Cutout { .. } => self.cutout_ring(project).cloned().map(ContourDraft::Ring),
        }
    }

    /// Whether the target still stores a draft this mode can edit.
    pub fn is_editable(self, project: &Project) -> bool {
        match self {
            Self::Panel(id) => project.panel(id).and_then(|panel| panel.outline_contour()).is_some(),
            Self::Object(id) => project.object_contour(id).is_ok(),
            Self::Path(id) => project.object_path(id).is_ok(),
            Self::Cutout { .. } => self.cutout_ring(project).is_some(),
        }
    }
}

/// What a card entry into the contour edit mode can do.
///
/// The editor owns one live edit at a time, so a card section must know whether it is the live
/// target, whether another target owns the viewport, or whether the selection rule blocks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContourEntry {
    /// This target is the live edit.
    Editing,
    /// Another contour target owns the viewport.
    Busy,
    /// The selection is not exactly the target's node.
    SelectSingle,
    /// The entry button can start the edit.
    Ready,
}

impl ContourEntry {
    /// The entry state for `mine`, given the live target and the selection rule.
    pub(crate) fn of(target: Option<ContourTarget>, mine: ContourTarget, single_selection: bool) -> Self {
        match target {
            Some(current) if current == mine => Self::Editing,
            Some(_) => Self::Busy,
            None if !single_selection => Self::SelectSingle,
            None => Self::Ready,
        }
    }
}

/// The working copy of a contour edit: a closed ring or a path chain.
///
/// A panel outline and a constructive contour are rings; a path is a chain that may be open. Both
/// store [`ContourSegmentEntry`] values under the same stable ids, so an edit addresses a segment
/// either way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContourDraft {
    /// A closed ring: a panel outline or a constructive contour.
    Ring(Contour),
    /// An open or closed chain: a path object.
    Chain(Path),
}

impl ContourDraft {
    /// The chain elements with their ids, in travel order.
    pub fn entries(&self) -> &[ContourSegmentEntry] {
        match self {
            Self::Ring(contour) => contour.entries(),
            Self::Chain(path) => path.entries(),
        }
    }

    /// The segment with this id.
    pub fn segment(&self, id: ContourSegmentId) -> Option<ContourSegment> {
        match self {
            Self::Ring(contour) => contour.segment(id),
            Self::Chain(path) => path.segment(id),
        }
    }

    /// The position of a segment id in travel order.
    pub fn index_of(&self, id: ContourSegmentId) -> Option<usize> {
        match self {
            Self::Ring(contour) => contour.index_of(id),
            Self::Chain(path) => path.index_of(id),
        }
    }

    /// The id of the segment at this position in travel order.
    pub fn id_at(&self, index: usize) -> Option<ContourSegmentId> {
        match self {
            Self::Ring(contour) => contour.id_at(index),
            Self::Chain(path) => path.id_at(index),
        }
    }

    /// The number of segments in the draft.
    pub fn segment_count(&self) -> usize {
        self.entries().len()
    }

    /// Whether the draft is a closed chain.
    pub const fn is_closed(&self) -> bool {
        match self {
            Self::Ring(_) => true,
            Self::Chain(path) => path.is_closed(),
        }
    }

    /// The ring, when the draft is one.
    pub const fn as_ring(&self) -> Option<&Contour> {
        match self {
            Self::Ring(contour) => Some(contour),
            Self::Chain(_) => None,
        }
    }

    /// The chain, when the draft is one.
    pub const fn as_chain(&self) -> Option<&Path> {
        match self {
            Self::Ring(_) => None,
            Self::Chain(path) => Some(path),
        }
    }
}

/// One live contour edit: the target, the draft it started from and the working copy.
///
/// The base draft lets the viewport show what changed and lets [`ContourEdit::reset`] throw the
/// working copy away without leaving the mode. A corner or a split only ever edits the working copy.
#[derive(Clone, Debug)]
pub struct ContourEdit {
    target: ContourTarget,
    base: ContourDraft,
    draft: ContourDraft,
}

impl ContourEdit {
    /// Starts editing `target`, or returns `None` when it has no ring or chain.
    pub fn start(project: &Project, target: ContourTarget) -> Option<Self> {
        let base = target.draft(project)?;
        Some(Self { target, base: base.clone(), draft: base })
    }

    /// Whether the target still has a ring or chain to edit.
    pub fn is_editable(project: &Project, target: ContourTarget) -> bool {
        target.is_editable(project)
    }

    /// The node being edited.
    pub const fn target(&self) -> ContourTarget {
        self.target
    }

    /// The draft the edit started from.
    pub const fn base(&self) -> &ContourDraft {
        &self.base
    }

    /// The working copy the viewport shows.
    pub const fn draft(&self) -> &ContourDraft {
        &self.draft
    }

    /// Whether the working copy differs from the stored draft.
    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    /// Throws the working copy away and starts again from the stored draft.
    pub fn reset(&mut self) {
        self.draft = self.base.clone();
    }

    /// Moves the corner at the start of `segment` to `point` in the target's local plane.
    pub fn move_corner(&mut self, segment: ContourSegmentId, point: [Length; 2]) -> Result<(), ContourEditError> {
        self.move_corner_at(PathCorner::Start(segment), point)
    }

    /// Moves the corner named by `corner` to `point` in the target's local plane.
    ///
    /// A ring addresses a corner by the segment that starts on it, so an end names the next
    /// segment's start; a chain uses the corner the path edit names.
    pub fn move_corner_at(&mut self, corner: PathCorner, point: [Length; 2]) -> Result<(), ContourEditError> {
        match &mut self.draft {
            ContourDraft::Ring(contour) => {
                let segment = ring_corner_segment(contour, corner)?;
                let moved = contour.move_corner(segment, point)?;
                *contour = moved;
            }
            ContourDraft::Chain(path) => {
                let moved = path.move_corner_at(corner, point)?;
                *path = moved;
            }
        }
        Ok(())
    }

    /// Splits `segment` at `point` and returns the id of the new part.
    pub fn split_segment(
        &mut self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<ContourSegmentId, ContourEditError> {
        match &mut self.draft {
            ContourDraft::Ring(contour) => {
                let (split, inserted) = contour.split_segment(segment, point)?;
                *contour = split;
                Ok(inserted)
            }
            ContourDraft::Chain(path) => {
                let (split, inserted) = path.split_segment_at(segment, point)?;
                *path = split;
                Ok(inserted)
            }
        }
    }

    /// Applies the working copy as one command.
    ///
    /// The caller owns the mode and drops it only on success; a rejected edit leaves the draft in
    /// place so the user can fix it or cancel.
    pub fn apply(&self, session: &mut Session) -> Result<(), String> {
        match (self.target, &self.draft) {
            (ContourTarget::Panel(panel), ContourDraft::Ring(contour)) => {
                let params = json(&PanelOutlineSet { panel, contour: contour.clone() })?;
                dispatch(session, OUTLINE_SET_ID, &params).map(|_| ())
            }
            (ContourTarget::Object(object), ContourDraft::Ring(contour)) => {
                let params = json(&ObjectContourReplace { object, contour: contour.clone() })?;
                dispatch(session, OBJECT_CONTOUR_REPLACE_ID, &params).map(|_| ())
            }
            (ContourTarget::Path(object), ContourDraft::Chain(path)) => {
                let params = json(&ObjectPathReplace { object, path: path.clone() })?;
                dispatch(session, OBJECT_PATH_REPLACE_ID, &params).map(|_| ())
            }
            (ContourTarget::Cutout { panel, machining }, ContourDraft::Ring(contour)) => {
                let record = session
                    .project()
                    .panel(panel)
                    .and_then(|panel| panel.machining_by_id(machining))
                    .ok_or_else(|| format!("machining {machining} is gone"))?;
                let cutout = record.cutout().ok_or_else(|| format!("machining {machining} is not a cutout"))?;
                let params =
                    MachiningParams::Cutout(Cutout::new(CutoutShape::Contour(contour.clone()), cutout.depth()));
                let request = PanelMachiningUpdate { panel, machining, anchor: record.anchor(), params };
                let params = json(&request)?;
                dispatch(session, MACHINING_UPDATE_ID, &params).map(|_| ())
            }
            _ => Err("the contour draft does not match its target".to_owned()),
        }
    }
}

/// The segment whose start names the same corner as `corner` on a closed ring.
fn ring_corner_segment(contour: &Contour, corner: PathCorner) -> Result<ContourSegmentId, ContourEditError> {
    let segment = corner.segment();
    if !corner.is_end() {
        return Ok(segment);
    }
    let index = contour.index_of(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
    let count = contour.entries().len();
    if count == 0 {
        return Err(ContourEditError::UnknownSegment { segment });
    }
    contour.id_at((index + 1) % count).ok_or(ContourEditError::UnknownSegment { segment })
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

/// One drawing tool's chain of work-plane segments.
///
/// A polyline holds the segments it has built rather than the points it was given, because an arc
/// ends where the stored arc ends — its rounded centre and angle can miss the clicked point — and
/// the next segment must start there. A fixed tool keeps its clicked points and builds its one curve
/// on demand. Everything is whole micrometres, so a draft becomes a document [`Path`] without a
/// float entering the document. A draft is view state: it runs no command and bumps no revision
/// until the drawing is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawDraft {
    tool: DrawTool,
    /// The chain a polyline has built, in travel order.
    segments: Vec<ContourSegment>,
    /// The clicks, in order. A fixed tool builds its curve from these; a polyline uses them for the
    /// first anchor and to know the count a fixed tool needs.
    points: Vec<[Length; 2]>,
    /// An arc through a clicked point, waiting for the point that ends it.
    pending_arc: Option<[Length; 2]>,
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
        Self { tool, segments: Vec::new(), points: Vec::new(), pending_arc: None, closed: false }
    }

    /// The tool this draft belongs to.
    pub const fn tool(&self) -> DrawTool {
        self.tool
    }

    /// The chain built so far, in travel order.
    pub fn segments(&self) -> &[ContourSegment] {
        &self.segments
    }

    /// The clicks, in the order they happened.
    ///
    /// A polyline starts each new segment at the stored end of the previous one, so a click is where
    /// the user meant the segment to go, not necessarily where a rounded arc actually put its end.
    pub fn points(&self) -> &[[Length; 2]] {
        &self.points
    }

    /// The stored end of the last segment: where the next one starts.
    pub fn last_end(&self) -> Option<[Length; 2]> {
        self.segments.last().and_then(|segment| segment.end().ok()).or_else(|| self.points.last().copied())
    }

    /// The point a pending arc passes through, or `None` when no arc waits for its end.
    pub const fn pending_arc(&self) -> Option<[Length; 2]> {
        self.pending_arc
    }

    /// Whether the user closed the chain.
    ///
    /// A rectangle or a circle produces a closed path by itself; that is a property of the tool, not
    /// of this flag, and the draft only counts as closed once the tool has all its points.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Whether the draft holds no segment and no click yet.
    ///
    /// A polyline keeps the click that anchors its first segment, so that one click alone is still
    /// an empty draft.
    pub fn is_empty(&self) -> bool {
        if self.tool.closes() { self.segments.is_empty() && self.points.len() <= 1 } else { self.points.is_empty() }
    }

    /// The number of segments built so far.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether the tool has every point it needs to produce a path.
    pub fn is_complete(&self) -> bool {
        if self.pending_arc.is_some() {
            return false;
        }
        match self.tool.points() {
            Some(need) => self.points.len() == need,
            None => !self.segments.is_empty(),
        }
    }

    /// Whether the chain has enough segments to close into a ring.
    pub fn can_close(&self) -> bool {
        self.tool.closes() && !self.closed && self.pending_arc.is_none() && self.segments.len() >= 2
    }

    /// Whether the tool is done with this drawing and it should be committed.
    ///
    /// A fixed tool finishes on its last point; a polyline finishes when the user closes the chain;
    /// an open polyline is committed explicitly, because a chain is a valid drawing. A pending arc is
    /// never finished: it still waits for the click that ends it.
    pub fn is_finished(&self) -> bool {
        self.pending_arc.is_none() && (self.closed || self.tool.points().is_some_and(|need| self.points.len() >= need))
    }

    /// Appends a click, which becomes a straight segment on a polyline and one point on a fixed tool.
    ///
    /// A path may touch or cross itself — the ring rules run only when it becomes a contour — but two
    /// equal neighbours would make a zero-length segment the document cannot store.
    pub fn push(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        if let Some(through) = self.pending_arc {
            return self.finish_arc(through, point);
        }
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if let Some(need) = self.tool.points()
            && self.points.len() >= need
        {
            return Err(DrawDraftError::Complete { tool: self.tool });
        }
        if self.segments.len() >= MAX_CONTOUR_SEGMENTS || self.points.len() >= MAX_CONTOUR_SEGMENTS {
            return Err(DrawDraftError::Full);
        }
        if !point_in_range(point) {
            return Err(DrawDraftError::PointOutOfRange);
        }
        if !self.tool.closes() {
            if self.points.last() == Some(&point) {
                return Err(DrawDraftError::DuplicatePoint);
            }
            self.points.push(point);
            return Ok(());
        }
        let Some(&previous) = self.points.last() else {
            self.points.push(point);
            return Ok(());
        };
        if previous == point && self.segments.is_empty() {
            return Err(DrawDraftError::DuplicatePoint);
        }
        let start = self.last_end().ok_or(DrawDraftError::TooFewPoints { need: 2, got: 0 })?;
        if start == point {
            return Err(DrawDraftError::DuplicatePoint);
        }
        self.segments.push(ContourSegment::line(start, point)?);
        self.points.push(point);
        Ok(())
    }

    /// Starts an arc at the chain's last end through `through`, waiting for the point that ends it.
    ///
    /// The arc is completed by the next [`DrawDraft::push`], and the following segment starts at its
    /// stored end rather than at the clicked one. The first click of a drawing only sets the start,
    /// exactly as it does for a straight segment.
    pub fn push_arc_through(&mut self, through: [Length; 2]) -> Result<(), DrawDraftError> {
        if self.pending_arc.is_some() {
            return Err(DrawDraftError::ArcInProgress);
        }
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if !self.tool.closes() && self.tool != DrawTool::Arc {
            return Err(DrawDraftError::NotClosable { tool: self.tool });
        }
        if self.segments.len() >= MAX_CONTOUR_SEGMENTS || self.points.len() >= MAX_CONTOUR_SEGMENTS {
            return Err(DrawDraftError::Full);
        }
        if !point_in_range(through) {
            return Err(DrawDraftError::PointOutOfRange);
        }
        if self.points.is_empty() || self.tool == DrawTool::Arc {
            // The Arc tool keeps three points and builds its one arc on demand, like every other
            // fixed tool; only a polyline grows a chain.
            if self.last_end() == Some(through) {
                return Err(DrawDraftError::DuplicatePoint);
            }
            self.points.push(through);
            return Ok(());
        }
        if self.last_end() == Some(through) {
            return Err(DrawDraftError::DuplicatePoint);
        }
        self.pending_arc = Some(through);
        Ok(())
    }

    /// Removes the last action: a pending arc, then a close, then the last segment.
    ///
    /// Returns `false` for an empty, open draft.
    pub fn undo(&mut self) -> bool {
        if self.pending_arc.take().is_some() {
            return true;
        }
        if self.closed {
            self.closed = false;
            return true;
        }
        if self.tool.closes() && self.segments.pop().is_some() {
            self.points.pop();
            if self.segments.is_empty() {
                // The anchor alone is not a drawing, so undoing the first segment clears it too.
                self.points.clear();
            }
            return true;
        }
        self.points.pop().is_some()
    }

    /// Closes the chain back to its first point.
    ///
    /// Only a polyline can close; a rectangle and a circle are closed by their constructors and a
    /// line and an arc stay open. A ring needs three segments, so a shorter chain is refused. The
    /// closed chain is validated before the state changes, so a draft that cannot become a path —
    /// for example because its last point repeats the first — stays open and editable. Whether the
    /// ring is simple and non-degenerate is a ring rule that runs when the path becomes a contour.
    pub fn close(&mut self) -> Result<(), DrawDraftError> {
        if self.pending_arc.is_some() {
            return Err(DrawDraftError::ArcInProgress);
        }
        if !self.tool.closes() {
            return Err(DrawDraftError::NotClosable { tool: self.tool });
        }
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        let closing = self.closing_segment()?;
        let count = self.segments.len().saturating_add(usize::from(closing.is_some()));
        if count < 3 {
            return Err(DrawDraftError::TooFewPoints { need: 3, got: count });
        }
        let mut segments = self.segments.clone();
        if let Some(segment) = closing {
            segments.push(segment);
        }
        Path::new(segments, true).map_err(DrawDraftError::from)?;
        self.closed = true;
        Ok(())
    }

    /// The path these segments describe, if the tool has all of them.
    pub fn to_path(&self) -> Result<Path, DrawDraftError> {
        self.path_with(None, self.closed)
    }

    /// The path the draft would have if `next` were appended as a straight segment, or `None` when
    /// the draft already holds everything the tool takes.
    ///
    /// This is the preview: a rectangle shows its fourth side and a circle its whole rim while the
    /// pointer still moves, and a pending arc previews the arc it will become.
    pub fn preview_path(&self, next: [Length; 2]) -> Option<Path> {
        if self.closed || !point_in_range(next) {
            return None;
        }
        if self.pending_arc.is_none()
            && let Some(need) = self.tool.points()
            && self.points.len() >= need
        {
            return None;
        }
        if self.segments.len() >= MAX_CONTOUR_SEGMENTS {
            return None;
        }
        if self.last_end() == Some(next) {
            return None;
        }
        self.path_with(Some(next), false).ok()
    }

    /// The one segment that runs from the last stored end back to the first stored start.
    ///
    /// A closed chain adds it only when the ends differ, so a chain that already returned to its
    /// start does not store a zero-length segment.
    fn closing_segment(&self) -> Result<Option<ContourSegment>, DrawDraftError> {
        let (Some(first), Some(last)) = (self.segments.first(), self.segments.last()) else {
            return Ok(None);
        };
        let start = first.start()?;
        let end = last.end()?;
        if start == end {
            return Ok(None);
        }
        Ok(Some(ContourSegment::line(end, start)?))
    }

    /// The path with an optional extra click, built by the draft's tool.
    ///
    /// Every segment after an arc starts at the stored end of the previous one, so a chain the
    /// viewport drew is a chain the ring assembler accepts.
    fn path_with(&self, next: Option<[Length; 2]>, closed: bool) -> Result<Path, DrawDraftError> {
        if self.tool.closes() {
            let mut segments = self.segments.clone();
            if let Some(segment) = self.next_segment(next)? {
                segments.push(segment);
            }
            if segments.is_empty() {
                return Err(DrawDraftError::TooFewPoints { need: 2, got: 0 });
            }
            if closed {
                let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
                    return Err(DrawDraftError::TooFewPoints { need: 3, got: segments.len() });
                };
                let start = first.start()?;
                let end = last.end()?;
                if start != end {
                    segments.push(ContourSegment::line(end, start)?);
                }
            }
            return Path::new(segments, closed).map_err(DrawDraftError::from);
        }
        let mut points = self.points.clone();
        if let Some(next) = next {
            points.push(next);
        }
        let need = self.tool.points().unwrap_or(2);
        if points.len() < need {
            return Err(DrawDraftError::TooFewPoints { need, got: points.len() });
        }
        match self.tool {
            DrawTool::Line => Path::polyline(points, false).map_err(DrawDraftError::from),
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
            DrawTool::Polyline => Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() }),
        }
    }

    /// Completes the pending arc through `through` at `point` and clears the waiting state.
    fn finish_arc(&mut self, through: [Length; 2], point: [Length; 2]) -> Result<(), DrawDraftError> {
        if !point_in_range(point) {
            return Err(DrawDraftError::PointOutOfRange);
        }
        if self.segments.len() >= MAX_CONTOUR_SEGMENTS || self.points.len() >= MAX_CONTOUR_SEGMENTS {
            return Err(DrawDraftError::Full);
        }
        let Some(&anchor) = self.points.last() else {
            return Err(DrawDraftError::TooFewPoints { need: 3, got: 0 });
        };
        let start = self.segments.last().and_then(|segment| segment.end().ok()).unwrap_or(anchor);
        if start == point || through == point {
            return Err(DrawDraftError::DuplicatePoint);
        }
        let arc = ContourSegment::arc_through(start, through, point).map_err(|error| match error {
            ContourSegmentError::DegenerateArc => DrawDraftError::DuplicatePoint,
            other => DrawDraftError::Segment(other),
        })?;
        self.pending_arc = None;
        self.segments.push(arc);
        self.points.push(point);
        Ok(())
    }

    /// The segment a click would add to a polyline, or the arc a pending one would complete.
    fn next_segment(&self, next: Option<[Length; 2]>) -> Result<Option<ContourSegment>, DrawDraftError> {
        let Some(next) = next else {
            return Ok(None);
        };
        if let Some(through) = self.pending_arc {
            let Some(&anchor) = self.points.last() else {
                return Err(DrawDraftError::TooFewPoints { need: 3, got: 0 });
            };
            let start = self.segments.last().and_then(|segment| segment.end().ok()).unwrap_or(anchor);
            return Ok(Some(ContourSegment::arc_through(start, through, next)?));
        }
        let Some(start) = self.last_end() else {
            return Ok(None);
        };
        if start == next {
            return Err(DrawDraftError::DuplicatePoint);
        }
        Ok(Some(ContourSegment::line(start, next)?))
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
    /// The chain already has an arc waiting for the click that ends it.
    #[error("the arc is still waiting for its end point")]
    ArcInProgress,
    /// A segment itself is invalid, for example two coincident ends.
    #[error(transparent)]
    Segment(#[from] ContourSegmentError),
    /// The segments do not form a path.
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
    use furnicraft_engine::PanelMachiningAdd;
    use furnicraft_engine::node_cmds::NODE_DELETE_ID;
    use furnicraft_engine::panel_cmds::MACHINING_ADD_ID;
    use furnicraft_model::{
        Constructive, Container, CutoutDepth, FaceSide, Geometry, MachiningAnchor, Panel, Placement, Project,
    };
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

    /// A rectangle inside the sample panel, with its minimum corner at `(x, y)`.
    fn offset_rectangle(x: i64, y: i64, width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(x), millimetres(y)],
            [millimetres(x + width), millimetres(y)],
            [millimetres(x + width), millimetres(y + height)],
            [millimetres(x), millimetres(y + height)],
        ])
        .expect("offset rectangle")
    }

    /// A session whose sample panel owns one blind contour cutout, anchored on face B.
    fn cutout_session() -> (Session, Id, MachiningId) {
        let mut session = session(sample_project());
        let panel = Id::new(2).expect("panel id");
        let request = PanelMachiningAdd {
            panel,
            anchor: MachiningAnchor::face(FaceSide::B),
            params: MachiningParams::Cutout(Cutout::new(
                CutoutShape::Contour(offset_rectangle(100, 100, 200, 100)),
                CutoutDepth::Blind { depth: millimetres(6) },
            )),
        };
        let params = json(&request).expect("params");
        let outcome = session.dispatch(MACHINING_ADD_ID, &params).expect("add the contour cutout");
        let machining = outcome.created_machining_id().expect("the command created a record");
        (session, panel, machining)
    }

    /// A project whose root holds one path object.
    fn path_project(path: Path) -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let id = project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("path object");
        (project, id)
    }

    fn open_path() -> Path {
        Path::polyline(
            vec![
                [millimetres(0), millimetres(0)],
                [millimetres(600), millimetres(0)],
                [millimetres(600), millimetres(400)],
            ],
            false,
        )
        .expect("open path")
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
        assert_eq!(edit.draft().as_ring().expect("ring").bounds_micrometres().1[0], millimetres(800).micrometres());
        assert_eq!(
            session.project().panel(panel).expect("panel").outline_contour(),
            edit.base().as_ring(),
            "the document still has the stored ring"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the outline");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), edit.draft().as_ring());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the ring");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), edit.base().as_ring());
    }

    #[test]
    fn reset_and_split_edit_only_the_draft() {
        let (project, panel) = panel_project();
        let mut edit = ContourEdit::start(&project, ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        let inserted = edit.split_segment(segment, [millimetres(350), millimetres(0)]).expect("split");
        assert_eq!(edit.draft().segment_count(), 5);
        assert!(edit.draft().index_of(inserted).is_some());
        assert!(edit.is_dirty());

        edit.reset();
        assert!(!edit.is_dirty());
        assert_eq!(edit.draft(), edit.base());
        assert_eq!(edit.draft().segment_count(), 4);
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
            edit.draft().as_ring().expect("ring"),
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
    fn a_contour_entry_knows_the_live_edit_and_the_selection_rule() {
        let panel = Id::new(2).expect("id");
        let machining = MachiningId::new(10).expect("id");
        let mine = ContourTarget::Cutout { panel, machining };
        assert_eq!(ContourEntry::of(None, mine, true), ContourEntry::Ready);
        assert_eq!(ContourEntry::of(None, mine, false), ContourEntry::SelectSingle);
        assert_eq!(ContourEntry::of(Some(mine), mine, false), ContourEntry::Editing);
        assert_eq!(
            ContourEntry::of(Some(ContourTarget::Panel(panel)), mine, true),
            ContourEntry::Busy,
            "another target owns the viewport"
        );
    }

    #[test]
    fn a_cutout_ring_edit_is_one_revision_and_one_undo() {
        let (mut session, panel, machining) = cutout_session();
        let target = ContourTarget::Cutout { panel, machining };
        assert_eq!(target.id(), panel, "the panel frames the ring");
        assert!(ContourEdit::is_editable(session.project(), target));
        let stored = offset_rectangle(100, 100, 200, 100);
        let mut edit = ContourEdit::start(session.project(), target).expect("a contour cutout is editable");
        assert_eq!(edit.base().as_ring(), Some(&stored));
        assert!(!edit.is_dirty());

        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(340), millimetres(100)]).expect("move the corner");
        assert!(edit.is_dirty());
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(stored.clone())),
            "the document still has the stored ring"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the cutout ring");
        let record = session
            .project()
            .panel(panel)
            .and_then(|panel| panel.machining_by_id(machining))
            .expect("the record stays");
        let cutout = record.cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &CutoutShape::Contour(edit.draft().as_ring().expect("ring").clone()));
        assert_eq!(record.anchor().as_face(), Some(FaceSide::B), "the anchor is unchanged");
        assert_eq!(cutout.depth(), CutoutDepth::Blind { depth: millimetres(6) }, "the depth is unchanged");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));

        session.undo().expect("one undo restores the ring");
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(stored)),
        );
    }

    #[test]
    fn a_cutout_ring_that_leaves_the_core_is_refused() {
        let (mut session, panel, machining) = cutout_session();
        let target = ContourTarget::Cutout { panel, machining };
        let mut edit = ContourEdit::start(session.project(), target).expect("editable");
        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(900), millimetres(100)]).expect("move the corner out");

        let revision = session.project().revision();
        assert!(edit.apply(&mut session).is_err(), "the ring no longer fits the core");
        assert_eq!(session.project().revision(), revision, "a refused update changes nothing");
        assert!(edit.is_dirty(), "the draft stays so the user can fix it");
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(offset_rectangle(100, 100, 200, 100))),
            "the stored ring is untouched"
        );
    }

    #[test]
    fn a_rectangle_cutout_has_no_ring_to_edit() {
        let mut session = session(sample_project());
        let panel = Id::new(2).expect("panel id");
        let request = PanelMachiningAdd {
            panel,
            anchor: MachiningAnchor::face(FaceSide::A),
            params: MachiningParams::Cutout(Cutout::new(
                CutoutShape::Rectangle(furnicraft_testkit::sample_cutout_rect()),
                CutoutDepth::Through,
            )),
        };
        let params = json(&request).expect("params");
        let machining = session
            .dispatch(MACHINING_ADD_ID, &params)
            .expect("add the rectangle cutout")
            .created_machining_id()
            .expect("record");
        let target = ContourTarget::Cutout { panel, machining };
        assert!(ContourEdit::start(session.project(), target).is_none());
        assert!(!ContourEdit::is_editable(session.project(), target));
    }

    #[test]
    fn an_open_path_edit_is_one_revision_and_one_undo() {
        let path = open_path();
        let (project, object) = path_project(path.clone());
        let mut session = session(project);
        let target = ContourTarget::Path(object);
        assert_eq!(target.id(), object);
        assert!(ContourEdit::is_editable(session.project(), target));
        let mut edit = ContourEdit::start(session.project(), target).expect("a path is editable");
        assert_eq!(edit.base(), &ContourDraft::Chain(path.clone()));
        assert!(!edit.is_dirty());

        let last = edit.draft().entries().last().map(|entry| entry.id()).expect("last segment");
        edit.move_corner_at(PathCorner::End(last), [millimetres(700), millimetres(500)]).expect("move the free end");
        assert!(edit.is_dirty());
        assert_eq!(
            session.project().object_path(object).expect("path"),
            &path,
            "the document still has the stored chain"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the path");
        let stored = session.project().object_path(object).expect("path");
        assert_eq!(Some(stored), edit.draft().as_chain(), "a path is stored exactly as edited");
        assert!(!stored.is_closed());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the chain");
        assert_eq!(session.project().object_path(object).expect("path"), &path);
        assert!(!session.can_undo(), "the edit was the only undo entry");
    }

    #[test]
    fn a_closed_path_stays_closed_through_an_edit() {
        let path =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(700), millimetres(500)]).expect("rectangle");
        let (project, object) = path_project(path);
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Path(object)).expect("editable");
        assert!(edit.draft().is_closed());

        let first = edit.draft().id_at(0).expect("first segment");
        edit.move_corner(first, [millimetres(-50), millimetres(-20)]).expect("move a corner");
        edit.apply(&mut session).expect("apply the path");

        let stored = session.project().object_path(object).expect("path");
        assert!(stored.is_closed(), "a replacement keeps the closed flag");
        assert_eq!(Some(stored), edit.draft().as_chain());
    }

    #[test]
    fn splitting_a_path_side_keeps_it_open_and_one_undo() {
        let path = open_path();
        let (project, object) = path_project(path);
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Path(object)).expect("editable");
        let first = edit.draft().id_at(0).expect("first segment");

        let inserted = edit.split_segment(first, [millimetres(300), millimetres(0)]).expect("split the bottom");
        assert_eq!(edit.draft().segment_count(), 3);
        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the split");

        let stored = session.project().object_path(object).expect("path");
        assert!(!stored.is_closed());
        assert_eq!(stored.entries().len(), 3);
        assert!(stored.index_of(inserted).is_some());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores two segments");
        assert_eq!(session.project().object_path(object).expect("path").entries().len(), 2);
    }

    #[test]
    fn a_constructive_target_does_not_edit_a_path() {
        let (project, object) = path_project(open_path());
        assert!(ContourEdit::start(&project, ContourTarget::Object(object)).is_none());
        assert!(!ContourEdit::is_editable(&project, ContourTarget::Object(object)));
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
    fn a_drawing_draft_builds_an_open_path_from_its_clicks() {
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
        assert_eq!(draft.len(), 2, "the first click starts the chain, the next two make segments");
        assert!(!draft.is_closed());
        assert!(draft.can_close(), "two segments enclose an area");

        let path = draft.to_path().expect("an open path");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 2);
        let mut ends = path.segments().map(|segment| segment.start().expect("start"));
        assert_eq!(ends.next(), Some(points[0]));
        assert_eq!(path.entries().last().and_then(|entry| entry.segment().end().ok()), Some(points[2]));
    }

    #[test]
    fn an_arc_mixed_with_lines_closes_and_reaches_a_contour() {
        // A rounded corner: a straight run, an arc through a control point, then the rest of the box.
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        draft.push([millimetres(600), millimetres(0)]).expect("straight run");
        draft.push_arc_through([millimetres(700), millimetres(100)]).expect("arc through");
        draft.push([millimetres(600), millimetres(400)]).expect("arc end");
        draft.push([millimetres(0), millimetres(400)]).expect("top run");
        draft.close().expect("the chain closes");

        let path = draft.to_path().expect("a mixed path");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4, "three clicks then the closing segment");
        assert_eq!(
            path.entries().iter().filter(|entry| entry.segment().is_arc()).count(),
            1,
            "the arc survives as one segment"
        );
        let ring = path.to_contour().expect("a mixed chain becomes a ring");
        assert_eq!(ring.entries().len(), 4);
    }

    #[test]
    fn the_next_segment_starts_at_the_stored_arc_end() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        draft.push_arc_through([millimetres(50), millimetres(50)]).expect("arc through");
        draft.push([millimetres(100), millimetres(0)]).expect("the far end");
        let arc_end = draft.segments().last().and_then(|segment| segment.end().ok()).expect("a stored end");
        // The stored end is rounded to whole document units, so it can differ from the click.
        assert!((arc_end[0].micrometres() - millimetres(100).micrometres()).abs() <= 1);
        draft.push([millimetres(100), millimetres(400)]).expect("the next straight segment");
        let line = draft.segments().last().expect("a line");
        assert_eq!(line.start().ok(), Some(arc_end), "the line starts on the stored arc end");
    }

    #[test]
    fn a_one_micrometre_gap_still_becomes_a_ring() {
        // A rounded arc end can miss the clicked point by one micrometre. `Path` accepts that joint
        // and, since ADR 0072, so does the ring assembler, so `to_contour` — the only way to an
        // extrusion — takes the chain the user drew instead of refusing it.
        let stored = [millimetres(600), millimetres(400)];
        let gap = [stored[0], Length::from_micrometres(stored[1].micrometres() + 1)];
        let chain = |end: [Length; 2], next_start: [Length; 2]| {
            Path::new(
                vec![
                    ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)])
                        .expect("bottom"),
                    ContourSegment::line([millimetres(600), millimetres(0)], end).expect("right"),
                    ContourSegment::line(next_start, [millimetres(0), millimetres(400)]).expect("top"),
                    ContourSegment::line([millimetres(0), millimetres(400)], [millimetres(0), millimetres(0)])
                        .expect("left"),
                ],
                true,
            )
        };
        let drawn = chain(gap, stored).expect("a path tolerates one micrometre");
        assert!(drawn.to_contour().is_ok(), "the ring takes the same tolerance as the path");
        assert!(chain(stored, stored).expect("an exact chain").to_contour().is_ok(), "the exact joint closes");
    }

    #[test]
    fn undo_cancels_a_pending_arc_before_the_last_segment() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("start");
        draft.push([millimetres(600), millimetres(0)]).expect("a line");
        draft.push_arc_through([millimetres(650), millimetres(50)]).expect("a pending arc");
        assert!(draft.pending_arc().is_some());
        assert!(!draft.is_finished(), "an arc still waits for its end");
        assert_eq!(draft.close(), Err(DrawDraftError::ArcInProgress), "an unfinished arc cannot close");

        // The next click ends the arc instead of starting a straight segment.
        draft.push([millimetres(700), millimetres(0)]).expect("the arc end");
        assert!(draft.pending_arc().is_none());
        assert_eq!(draft.len(), 2);
        assert!(draft.segments().last().is_some_and(|segment| segment.is_arc()));

        assert!(draft.undo(), "the last segment goes");
        assert_eq!(draft.len(), 1, "the line before the arc survives");
        assert!(draft.undo());
        assert!(draft.is_empty());
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
        assert_eq!(draft.len(), 2, "the closing segment is built on demand, not stored as a click");

        let path = draft.to_path().expect("a closed path");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 3);
        let last = path.entries().last().expect("the closing segment");
        assert_eq!(last.segment().end().ok(), Some([millimetres(0), millimetres(0)]));

        assert_eq!(draft.close(), Err(DrawDraftError::AlreadyClosed));
        assert_eq!(draft.push([millimetres(1), millimetres(1)]), Err(DrawDraftError::AlreadyClosed));
    }

    #[test]
    fn a_drawing_draft_needs_three_segments_to_close() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        draft.push([millimetres(0), millimetres(0)]).expect("point");
        assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 0 }));
        draft.push([millimetres(600), millimetres(0)]).expect("point");
        assert_eq!(draft.close(), Err(DrawDraftError::TooFewPoints { need: 3, got: 2 }));
        assert!(!draft.can_close());
    }

    #[test]
    fn undoing_a_drawing_draft_reopens_before_it_drops_a_segment() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
        {
            draft.push(point).expect("point");
        }
        draft.close().expect("close");

        assert!(draft.undo(), "the first undo opens the chain");
        assert!(!draft.is_closed());
        assert_eq!(draft.len(), 2, "opening keeps every segment");
        assert!(draft.undo(), "the next undo drops the last segment");
        assert_eq!(draft.len(), 1);
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
        // The first click anchors the chain, so one more click than the segment cap is tried.
        for index in 0..=MAX_CONTOUR_SEGMENTS {
            let x = Length::from_micrometres(i64::try_from(index).expect("index") * 1_000);
            let result = draft.push([x, Length::ZERO]);
            if index == MAX_CONTOUR_SEGMENTS {
                assert_eq!(result, Err(DrawDraftError::Full), "a click past the cap is refused");
            } else {
                result.expect("a point below the cap");
            }
        }
        assert_eq!(draft.len(), MAX_CONTOUR_SEGMENTS - 1);
        assert_eq!(
            draft.push([Length::from_micrometres(MAX_CONTOUR_SEGMENTS as i64 * 1_000), Length::ZERO]),
            Err(DrawDraftError::Full),
            "the anchor click fills the count, so the next one is past the cap"
        );
    }

    #[test]
    fn a_drawing_draft_refuses_a_close_that_cannot_form_a_path() {
        let mut draft = DrawDraft::new(DrawTool::Polyline);
        for point in
            [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(0), millimetres(0)]]
        {
            draft.push(point).expect("a path need not be simple");
        }
        assert!(draft.close().is_err(), "two segments cannot become a ring");
        assert!(!draft.is_closed(), "a refused close leaves the draft open and editable");
        assert_eq!(draft.len(), 2);
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
