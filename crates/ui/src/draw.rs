//! One drawing tool's work in progress: the chain it builds and the path it commits.
//!
//! A [`DrawDraft`] holds work-plane clicks, undoes the last action, previews the next segment and
//! closes the chain. It is view state: it runs no command and bumps no revision until the drawing
//! is committed as one `object.path.create`. Everything is whole micrometres, so a draft becomes a
//! document [`Path`] without a float entering the document (ADR 0048).
//!
//! A fixed tool stops at its own point count; a polyline takes points until the ring cap, and Tab
//! switches its next segment between a line and an arc (ADR 0076). A path may touch or cross
//! itself — the ring rules run only when it becomes a contour — and the closing joint is judged
//! with the drawing tolerance, so a rounded arc end that lands beside the first point still closes
//! the ring (ADR 0072). A pending arc is the closing segment once the chain has its two straight
//! runs (ADR 0077), and *Close* closes and commits in one operation (ADR 0052).
//!
//! It refuses a repeat of the previous point, a coordinate outside the document range, a point past
//! the ring cap and a close that cannot form a path; each is a [`DrawDraftError`] and leaves the
//! draft editable.
//!
//! It does not choose the work plane, create the node or hold the editor's tool selection: the
//! caller passes the plane's placement to [`DrawDraft::commit`].
//!
//! Changes when: a drawing tool, its point grammar or the closing rule changes.

use furnicraft_engine::object_cmds::OBJECT_PATH_CREATE_ID;
use furnicraft_engine::{ObjectPathCreate, Session};
use furnicraft_model::{
    ContourSegment, ContourSegmentError, Id, MAX_CONTOUR_COORDINATE, MAX_CONTOUR_SEGMENTS, MAX_POLYGON_SIDES,
    MIN_POLYGON_SIDES, Path, PathError, Placement,
};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;
use thiserror::Error;

use crate::command::{dispatch, json};

/// The tool a drawing draft is building with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawTool {
    /// Two points make one open line.
    Line,
    /// Two or more points make an open chain, or a closed ring on request. Tab switches each
    /// segment between a straight line and an arc.
    #[default]
    Polyline,
    /// Three points make one open arc through the middle point.
    Arc,
    /// A centre, a start and an end point make one open arc of that radius and sweep.
    ArcFromCenter,
    /// Two opposite corners make one closed rectangle.
    Rectangle,
    /// A centre and a corner make one closed rectangle of twice that offset.
    RectangleFromCenter,
    /// A centre, a side direction and a corner make one closed rectangle at any angle.
    RectangleFromCenterDirected,
    /// A centre and a rim point make one closed circle.
    Circle,
    /// Three points on the rim make one closed circle.
    CircleThreePoints,
    /// A centre and a rim point make one closed regular polygon; the side count comes from the
    /// ribbon.
    RegularPolygon,
}

impl DrawTool {
    /// How many points the tool needs to produce its path, or `None` for an open-ended polyline.
    ///
    /// A fixed tool stops taking points at this count; a polyline produces a path as soon as it has
    /// two points and takes as many more as the ring cap allows.
    pub const fn points(self) -> Option<usize> {
        match self {
            Self::Line | Self::Rectangle | Self::RectangleFromCenter | Self::Circle | Self::RegularPolygon => Some(2),
            Self::Arc | Self::ArcFromCenter | Self::CircleThreePoints | Self::RectangleFromCenterDirected => Some(3),
            Self::Polyline => None,
        }
    }

    /// Whether the tool can close its chain into a ring; only a polyline may.
    pub const fn closes(self) -> bool {
        matches!(self, Self::Polyline)
    }

    /// Whether the path is closed by the tool itself, without a close action.
    pub const fn is_closed(self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::RectangleFromCenter
                | Self::RectangleFromCenterDirected
                | Self::Circle
                | Self::CircleThreePoints
                | Self::RegularPolygon
        )
    }

    /// A short user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::Line => Message::ToolLine,
            Self::Polyline => Message::ToolPolyline,
            Self::Arc => Message::ToolArc,
            Self::ArcFromCenter => Message::ToolArcFromCentre,
            Self::Rectangle => Message::ToolRectangle,
            Self::RectangleFromCenter => Message::ToolRectangleFromCentre,
            Self::RectangleFromCenterDirected => Message::ToolRectangleFromCentreDirected,
            Self::Circle => Message::ToolCircle,
            Self::CircleThreePoints => Message::ToolCircleThreePoints,
            Self::RegularPolygon => Message::ToolRegularPolygon,
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
    /// Whether a polyline's next segment is an arc; Tab flips it.
    arc_mode: bool,
    /// The side count a regular polygon uses, from the ribbon.
    sides: u32,
    closed: bool,
}

/// The side count a regular polygon starts with before the ribbon names another.
pub const DEFAULT_POLYGON_SIDES: u32 = 6;

impl Default for DrawDraft {
    fn default() -> Self {
        Self::new(DrawTool::Polyline)
    }
}

impl DrawDraft {
    /// An empty, open draft for `tool`.
    pub const fn new(tool: DrawTool) -> Self {
        Self::with_sides(tool, DEFAULT_POLYGON_SIDES)
    }

    /// An empty, open draft for `tool`, with the side count a regular polygon uses.
    ///
    /// The count is clamped to the model's accepted range, so a bad ribbon value can never build a
    /// polygon the document refuses.
    pub const fn with_sides(tool: DrawTool, sides: u32) -> Self {
        let sides = if sides < MIN_POLYGON_SIDES {
            MIN_POLYGON_SIDES
        } else if sides > MAX_POLYGON_SIDES {
            MAX_POLYGON_SIDES
        } else {
            sides
        };
        Self {
            tool,
            segments: Vec::new(),
            points: Vec::new(),
            pending_arc: None,
            arc_mode: false,
            sides,
            closed: false,
        }
    }

    /// The tool this draft belongs to.
    pub const fn tool(&self) -> DrawTool {
        self.tool
    }

    /// Whether the next polyline segment is an arc.
    pub const fn arc_mode(&self) -> bool {
        self.arc_mode
    }

    /// Flips a polyline between straight and arc input; other tools have no second mode.
    ///
    /// Returns the mode after the call, so the status line can name it.
    pub fn toggle_arc_mode(&mut self) -> bool {
        if self.tool == DrawTool::Polyline {
            self.arc_mode = !self.arc_mode;
        }
        self.arc_mode
    }

    /// The side count of the regular polygon this draft would build.
    pub const fn sides(&self) -> u32 {
        self.sides
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

    /// Whether the chain can close into a ring right now.
    ///
    /// Two segments are enough: a straight close adds the third, and a pending arc *is* the closing
    /// segment, so a chain of two lines with an arc waiting can finish the ring. This is the same
    /// predicate the ribbon's *Close*, the status bar's *Close* and the first-node highlight read,
    /// so a closure the document accepts is never greyed out by the chrome.
    pub fn can_close(&self) -> bool {
        self.tool.closes() && !self.closed && self.segments.len() >= 2
    }

    /// Whether the tool is done with this drawing and it should be committed.
    ///
    /// A fixed tool finishes on its last point; a polyline finishes when the user closes the chain;
    /// an open polyline is committed explicitly, because a chain is a valid drawing. A pending arc
    /// never finishes a chain by itself — it still waits for the click that ends it — but a chain
    /// the user already closed through that arc is finished, because the closing segment is derived
    /// from the pending midpoint (ADR 0077).
    pub fn is_finished(&self) -> bool {
        self.closed || (self.pending_arc.is_none() && self.tool.points().is_some_and(|need| self.points.len() >= need))
    }

    /// Appends a click, which becomes a straight segment on a polyline and one point on a fixed tool.
    ///
    /// A path may touch or cross itself — the ring rules run only when it becomes a contour — but two
    /// equal neighbours would make a zero-length segment the document cannot store.
    pub fn push(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        if self.closed {
            return Err(DrawDraftError::AlreadyClosed);
        }
        if let Some(through) = self.pending_arc {
            return self.finish_arc(through, point);
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

    /// Removes the last action: a close, then a pending arc, then the last segment.
    ///
    /// A closed draft is opened first, because a close that used a pending arc keeps that arc
    /// waiting: taking the arc before reopening would drop the closing segment and leave the chain
    /// looking straight. Returns `false` for an empty, open draft.
    pub fn undo(&mut self) -> bool {
        if self.closed {
            self.closed = false;
            return true;
        }
        if self.pending_arc.take().is_some() {
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
    /// line and an arc stay open. A ring needs three segments, so a shorter chain is refused. A
    /// pending arc is the closing segment: it runs from the last stored end through its waiting
    /// midpoint to the first start, so a chain the user has already curved is finished by the arc
    /// they were drawing instead of demanding one more click (ADR 0077). The closed chain is
    /// validated before the state changes, so a draft that cannot become a path — for example
    /// because its last point repeats the first or its closing arc is degenerate — stays open and
    /// editable. The arc keeps waiting on the closed draft so `to_path` derives the same closing
    /// segment and undo can reopen the chain with the arc still there; a click on a closed draft is
    /// ignored, so the waiting arc cannot append a second one. Whether the ring is simple and
    /// non-degenerate is a ring rule that runs when the path becomes a contour.
    pub fn close(&mut self) -> Result<(), DrawDraftError> {
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

    /// The path the draft would have if `next` were appended, or `None` when the draft already
    /// holds everything the tool takes.
    ///
    /// This is the preview: a rectangle shows its fourth side and a circle its whole rim while the
    /// pointer still moves, and a pending arc previews the arc it will become.
    pub fn preview_path(&self, next: [Length; 2]) -> Option<Path> {
        self.preview(next).ok().flatten()
    }

    /// The preview, with the reason a click at `next` cannot extend the draft.
    ///
    /// The caller keeps the last valid preview when this is an error, so a chain never blinks out
    /// at an intermediate click; the reason goes to the status line instead.
    pub fn preview(&self, next: [Length; 2]) -> Result<Option<Path>, DrawDraftError> {
        if self.closed || !point_in_range(next) {
            return Ok(None);
        }
        if self.pending_arc.is_none()
            && let Some(need) = self.tool.points()
            && self.points.len() >= need
        {
            return Ok(None);
        }
        if self.segments.len() >= MAX_CONTOUR_SEGMENTS {
            return Ok(None);
        }
        if self.last_end() == Some(next) {
            return Ok(None);
        }
        self.path_with(Some(next), false).map(Some)
    }

    /// The one segment that runs from the last stored end back to the first stored start.
    ///
    /// A closed chain adds it only when the two ends do not already meet within the drawing joint
    /// tolerance, so a rounded arc end that lands on the first point closes the ring without a
    /// sliver segment in front of it (ADR 0072). A pending arc is that closing segment: it runs
    /// from the stored end through the midpoint the user already placed to the first start, so the
    /// chain is finished by the arc the user was drawing rather than by a straight tie (ADR 0077).
    fn closing_segment(&self) -> Result<Option<ContourSegment>, DrawDraftError> {
        let (Some(first), Some(last)) = (self.segments.first(), self.segments.last()) else {
            return Ok(None);
        };
        closing_segment(*first, *last, self.pending_arc)
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
                // A last end that already meets the first start within the drawing tolerance is the
                // closing joint itself; the same rule and the same pending arc as `close` build the
                // closing segment, so the validated ring and the committed path are one chain.
                if let Some(segment) = closing_segment(*first, *last, self.pending_arc)? {
                    segments.push(segment);
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
            DrawTool::ArcFromCenter => {
                let (Some(&center), Some(&start), Some(&end)) = (points.first(), points.get(1), points.get(2)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 3, got: points.len() });
                };
                Path::arc_from_center(center, start, end).map_err(DrawDraftError::from)
            }
            DrawTool::Rectangle => {
                let (Some(&first), Some(&second)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::rectangle(first, second).map_err(DrawDraftError::from)
            }
            DrawTool::RectangleFromCenter => {
                let (Some(&center), Some(&corner)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::rectangle_from_center(center, corner).map_err(DrawDraftError::from)
            }
            DrawTool::RectangleFromCenterDirected => {
                let (Some(&center), Some(&direction), Some(&corner)) = (points.first(), points.get(1), points.get(2))
                else {
                    return Err(DrawDraftError::TooFewPoints { need: 3, got: points.len() });
                };
                Path::rectangle_from_center_directed(center, direction, corner).map_err(DrawDraftError::from)
            }
            DrawTool::Circle => {
                let (Some(&center), Some(&rim)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::circle_through(center, rim).map_err(DrawDraftError::from)
            }
            DrawTool::CircleThreePoints => {
                let (Some(&first), Some(&second), Some(&third)) = (points.first(), points.get(1), points.get(2)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 3, got: points.len() });
                };
                Path::circle_three_points(first, second, third).map_err(DrawDraftError::from)
            }
            DrawTool::RegularPolygon => {
                let (Some(&center), Some(&rim)) = (points.first(), points.get(1)) else {
                    return Err(DrawDraftError::TooFewPoints { need: 2, got: points.len() });
                };
                Path::regular_polygon(center, rim, self.sides).map_err(DrawDraftError::from)
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
        // An arc that ends on the chain's first start closes the ring, which is the one way a
        // closing click in arc mode can make a ring. The stored arc end is derived, so the joint is
        // judged with the drawing tolerance rather than exact equality; `close` validates the whole
        // ring before the draft becomes closed. A ring needs three segments; a shorter chain stays
        // open so the next click can still extend it.
        let meets_first = match self.segments.first() {
            Some(first) => arc.end_meets(*first)?,
            None => false,
        };
        if self.tool.closes() && meets_first && self.segments.len() >= 3 {
            self.close()?;
        }
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

/// The segment that closes a chain from the end of `last` back to the start of `first`.
///
/// The two ends meeting within the drawing joint tolerance is the closing joint itself, so no
/// segment is added: a rounded arc end that lands on the first point closes the ring without a
/// sliver in front of it (ADR 0072). A pending arc becomes that closing segment — it runs from the
/// stored end through `through` to the first start — so a chain can finish on the arc the user was
/// drawing instead of a straight tie (ADR 0077). A straight close runs the other way round, from
/// the stored end to the first start. Both `close` and `to_path` call this one rule, so the ring
/// that was validated and the ring that is committed are the same chain.
fn closing_segment(
    first: ContourSegment,
    last: ContourSegment,
    through: Option<[Length; 2]>,
) -> Result<Option<ContourSegment>, DrawDraftError> {
    if last.end_meets(first)? {
        return Ok(None);
    }
    let start = last.end()?;
    let end = first.start()?;
    Ok(Some(match through {
        Some(through) => ContourSegment::arc_through(start, through, end)?,
        None => ContourSegment::line(start, end)?,
    }))
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
    /// `placement` is the work plane's frame relative to the target container. The command moves the
    /// chain so its creation pivot becomes the object's local zero, and stores that pivot position
    /// as the placement; the coordinates of the drawn figure are not the stored ones. A fixed tool
    /// commits its finished path, a polyline commits what it has whether it was closed or not. On
    /// success the caller drops the draft; a rejected path leaves it intact so the user can undo or
    /// cancel.
    pub fn commit(&self, session: &mut Session, parent: Id, placement: Placement, lang: Lang) -> Result<Id, String> {
        let path = self.to_path().map_err(|error| error.to_string())?;
        let name = Message::DrawingName.with(lang, [self.tool.label().text(lang)]);
        let params = json(&ObjectPathCreate { parent, name, path, placement })?;
        let outcome = dispatch(session, OBJECT_PATH_CREATE_ID, &params)?;
        outcome.created_id().ok_or_else(|| "the path command created no node".to_owned())
    }
}

#[cfg(test)]
#[path = "draw_tests.rs"]
mod tests;
