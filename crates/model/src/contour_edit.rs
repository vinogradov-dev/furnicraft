//! Editing a contour ring or a path: move a corner, trim a segment, split one and drop one.
//!
//! An edit works on the stored ring and returns a new one, so the caller decides when a working copy
//! reaches the document. Every segment an edit does not reshape keeps its id, and a split mints the
//! new part from the ring counter; banding and selection keyed by [`ContourSegmentId`] therefore
//! survive an edit. A ring addresses a corner by the segment that starts there; an open path has a
//! free start and a free end, so it addresses one with [`PathCorner`].
//!
//! An arc keeps its centre and radius: a corner on an arc is projected onto its circle and the sweep
//! is trimmed or extended, never redefined. A corner more than one micrometre off the circle is an
//! error, because redefining the circle would change the curve the user drew. The same holds for a
//! trim or a split: the point must lie on the segment's own supporting line or circle.

use furnicraft_geom::{Arc, ContourError as GeomError, DVec2, Segment};
use furnicraft_units::{Degrees, Length};
use thiserror::Error;

use crate::contour::point_to_geometry;
use crate::{
    Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, ContourSegmentId, Path, PathError,
};

/// Largest distance between an edited point and the curve it must stay on, in millimetres.
///
/// The document stores whole micrometres, so a point placed on a curve can miss it by half a
/// micrometre before it is rounded. One micrometre accepts that rounding and nothing coarser.
const SUPPORT_TOLERANCE: f64 = 1.0e-3;

/// Why a contour edit cannot be applied.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum ContourEditError {
    /// The ring has no segment with this id.
    #[error("contour segment {segment} is not part of the ring")]
    UnknownSegment {
        /// The id the edit named.
        segment: ContourSegmentId,
    },
    /// A point meant to trim or split a segment does not lie on it.
    #[error("the point does not lie on contour segment {segment}")]
    PointOffSegment {
        /// The segment the point was meant to trim or split.
        segment: ContourSegmentId,
    },
    /// A corner does not lie on the circle of an adjoining arc.
    #[error("the corner does not lie on the circle of arc segment {segment}")]
    PointOffCircle {
        /// The arc whose circle the corner misses.
        segment: ContourSegmentId,
    },
    /// A trim would turn the segment around instead of shortening or extending it.
    #[error("trimming contour segment {segment} would reverse it")]
    ReversedSegment {
        /// The segment that would reverse.
        segment: ContourSegmentId,
    },
    /// Two adjoining arcs cannot reach one shared corner.
    #[error("a corner between two arcs cannot reach the requested point")]
    ArcJointMismatch,
    /// A segment is not a valid curve.
    #[error(transparent)]
    Segment(#[from] ContourSegmentError),
    /// The edited ring would not be a valid contour.
    #[error(transparent)]
    Ring(#[from] ContourError),
    /// The geometry layer rejected the trim or the split.
    #[error(transparent)]
    Geometry(#[from] GeomError),
    /// The edited path would not be a valid chain.
    #[error(transparent)]
    Path(#[from] PathError),
    /// A path with one segment has no neighbour to join after a removal.
    #[error("the only segment of a path cannot be removed")]
    OnlySegment,
}

/// One corner of a path, named by a segment it belongs to.
///
/// A ring wraps, so a corner is named by the segment that starts on it. A path may be open, so its
/// first start and its last end have only one segment: [`PathCorner::Start`] is the point a segment
/// begins on, shared with the previous segment's end, and [`PathCorner::End`] is the point it
/// finishes on, shared with the next segment's start. On a closed chain `Start` of the first segment
/// and `End` of the last name the same corner, which the edit resolves to one pair of neighbours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathCorner {
    /// The start point of the named segment.
    Start(ContourSegmentId),
    /// The end point of the named segment.
    End(ContourSegmentId),
}

impl PathCorner {
    /// The segment the corner is stored on.
    pub const fn segment(self) -> ContourSegmentId {
        match self {
            Self::Start(segment) | Self::End(segment) => segment,
        }
    }

    /// Whether the corner is the segment's end rather than its start.
    pub const fn is_end(self) -> bool {
        matches!(self, Self::End(_))
    }
}

impl Path {
    /// Moves the corner named by `corner` to `point`.
    ///
    /// The corner is shared by two segments unless it is the free start or the free end of an open
    /// chain; then only the one segment moves. A line takes the point exactly, an adjoining arc keeps
    /// its centre and radius and reaches the corner by trimming or extending its sweep, and every
    /// other segment keeps its id.
    pub fn move_corner_at(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
        let segment = corner.segment();
        let (index, count) = self.position(segment)?;
        let entries = self.entries();
        let Some(entry) = entries.get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let (before_index, after_index) = match self.corner_neighbour(index, count, corner.is_end()) {
            Some(neighbour) if corner.is_end() => (index, neighbour),
            Some(neighbour) => (neighbour, index),
            None => {
                let (moved, _) = with_corner(segment, entry.segment(), point, corner.is_end())?;
                return self.replaced(index, ContourSegmentEntry::new(segment, moved));
            }
        };
        let (Some(before_entry), Some(after_entry)) = (entries.get(before_index), entries.get(after_index)) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let (before, after) =
            join_corner(before_entry.id(), before_entry.segment(), after_entry.id(), after_entry.segment(), point)?;
        let mut edited = entries.to_vec();
        let Some(before_slot) = edited.get_mut(before_index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *before_slot = ContourSegmentEntry::new(before_entry.id(), before);
        let Some(after_slot) = edited.get_mut(after_index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *after_slot = ContourSegmentEntry::new(after_entry.id(), after);
        Ok(self.rebuild(edited)?)
    }

    /// Splits `segment` at a point strictly inside it, keeping the path open or closed.
    ///
    /// The part before the point keeps the segment's id and the part after receives a fresh id, the
    /// same rule as [`Contour::split_segment`].
    pub fn split_segment_at(
        &self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<(Self, ContourSegmentId), ContourEditError> {
        let (index, _) = self.position(segment)?;
        let entries = self.entries();
        let Some(entry) = entries.get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let requested = point_to_geometry(point)?;
        let Some(nearest) = entry.segment().to_geom()?.nearest_on_support(requested) else {
            return Err(ContourEditError::PointOffSegment { segment });
        };
        if (nearest - requested).length() > SUPPORT_TOLERANCE {
            return Err(ContourEditError::PointOffSegment { segment });
        }
        let (first, second) = split_document(entry.segment(), nearest)?;
        let inserted = self.next_segment_id();
        let mut edited = entries.to_vec();
        let Some(slot) = edited.get_mut(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *slot = ContourSegmentEntry::new(segment, first);
        if index >= edited.len() {
            return Err(ContourEditError::UnknownSegment { segment });
        }
        edited.insert(index + 1, ContourSegmentEntry::new(inserted, second));
        Ok((self.rebuild(edited)?, inserted))
    }

    /// Removes `segment` and joins the two neighbours that faced it.
    ///
    /// The next segment's start moves to the previous segment's end, so everything before the
    /// removed segment stays where it is and an arc among the neighbours keeps its circle. Removing
    /// the first or the last segment of an open chain only shortens the chain; a closed chain is
    /// re-closed around the joined corner, and an open chain of one segment has nothing to remove.
    pub fn remove_segment(&self, segment: ContourSegmentId) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        if count <= 1 {
            return Err(ContourEditError::OnlySegment);
        }
        let entries = self.entries();
        let last = count - 1;
        let join = if index == 0 {
            (self.is_closed() && count > 2).then_some((last, 1))
        } else if index == last {
            (self.is_closed() && count > 2).then_some((index - 1, 0))
        } else {
            Some((index - 1, index + 1))
        };
        let mut edited = entries.to_vec();
        if let Some((before_index, after_index)) = join {
            let (Some(before_entry), Some(after_entry)) = (entries.get(before_index), entries.get(after_index)) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            let corner = before_entry.segment().end()?;
            let (before, after) = join_corner(
                before_entry.id(),
                before_entry.segment(),
                after_entry.id(),
                after_entry.segment(),
                corner,
            )?;
            let Some(before_slot) = edited.get_mut(before_index) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            *before_slot = ContourSegmentEntry::new(before_entry.id(), before);
            let Some(after_slot) = edited.get_mut(after_index) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            *after_slot = ContourSegmentEntry::new(after_entry.id(), after);
        }
        if index >= edited.len() {
            return Err(ContourEditError::UnknownSegment { segment });
        }
        edited.remove(index);
        Ok(self.rebuild(edited)?)
    }

    /// Closes an open chain into a ring, adding one line from the last end to the first start.
    ///
    /// The gap is bridged with a straight line and the chain keeps every id; a chain whose ends
    /// already meet is only flagged closed. Closing means the user wants a ring, so the result must
    /// pass the ring rules: a bridge that crosses the chain, a chain that crosses itself and a chain
    /// that encloses no area are refused, and a closed path is returned unchanged.
    pub fn close(&self) -> Result<Self, ContourEditError> {
        let entries = self.entries();
        if self.is_closed() {
            return Ok(self.rebuild(entries.to_vec())?);
        }
        let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
            return Err(PathError::TooFewSegments { count: 0 }.into());
        };
        let start = first.segment().start()?;
        let end = last.segment().end()?;
        let mut closed = entries.to_vec();
        if end != start {
            closed.push(ContourSegmentEntry::new(self.next_segment_id(), ContourSegment::line(end, start)?));
        }
        let path = self.rebuild_as(closed, true)?;
        // A chain that cannot become a ring is a ring-rule refusal, reported like every other one.
        path.to_contour().map_err(|error| match error {
            PathError::Contour(inner) => ContourEditError::Ring(inner),
            other => ContourEditError::Path(other),
        })?;
        Ok(path)
    }

    /// The other segment that shares the corner, or `None` at the free end of an open chain.
    fn corner_neighbour(&self, index: usize, count: usize, at_end: bool) -> Option<usize> {
        if at_end {
            if index + 1 < count {
                Some(index + 1)
            } else if self.is_closed() {
                Some(0)
            } else {
                None
            }
        } else if index > 0 {
            Some(index - 1)
        } else if self.is_closed() {
            Some(count - 1)
        } else {
            None
        }
    }

    /// The same path with one entry replaced.
    fn replaced(&self, index: usize, entry: ContourSegmentEntry) -> Result<Self, ContourEditError> {
        let mut edited = self.entries().to_vec();
        let Some(slot) = edited.get_mut(index) else {
            return Err(ContourEditError::UnknownSegment { segment: entry.id() });
        };
        *slot = entry;
        Ok(self.rebuild(edited)?)
    }

    /// The position of a segment and the number of segments in the chain.
    fn position(&self, segment: ContourSegmentId) -> Result<(usize, usize), ContourEditError> {
        let index = self.index_of(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        Ok((index, self.entries().len()))
    }
}

impl Contour {
    /// Moves the corner at the start of `segment` to `point`.
    ///
    /// The previous segment's end and this segment's start both move to the same corner. A line
    /// takes the point exactly; an adjoining arc keeps its centre and radius and reaches the corner
    /// by trimming or extending its sweep. Every other segment keeps its id.
    pub fn move_corner(&self, segment: ContourSegmentId, point: [Length; 2]) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        let previous = (index + count - 1) % count;
        let entries = self.entries();
        let (Some(before), Some(current)) = (entries.get(previous), entries.get(index)) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let previous_id = before.id();
        let (before, current) = join_corner(previous_id, before.segment(), segment, current.segment(), point)?;
        let mut edited = entries.to_vec();
        edited[previous] = ContourSegmentEntry::new(previous_id, before);
        edited[index] = ContourSegmentEntry::new(segment, current);
        Ok(self.rebuild(edited)?)
    }

    /// Trims or extends one end of `segment` along its own supporting curve to `point`.
    ///
    /// The corner the segment shares with its neighbour moves with it; the neighbour is trimmed or
    /// extended to the same corner. `at_end` addresses the segment's end instead of its start. A
    /// line trims to any point on its supporting line as long as it does not pass its other end; an
    /// arc trims or extends along its circle within [`furnicraft_geom::MAX_ARC_EXTENSION`]. The
    /// segment keeps its id.
    pub fn trim_segment(
        &self,
        segment: ContourSegmentId,
        at_end: bool,
        point: [Length; 2],
    ) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        let entries = self.entries();
        let Some(entry) = entries.get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let geometry = entry.segment().to_geom()?;
        let requested = point_to_geometry(point)?;
        let Some(nearest) = geometry.nearest_on_support(requested) else {
            return Err(ContourEditError::PointOffSegment { segment });
        };
        if (nearest - requested).length() > SUPPORT_TOLERANCE {
            return Err(ContourEditError::PointOffSegment { segment });
        }
        if let Segment::Line(line) = geometry {
            let direction = line.end() - line.start();
            let travel = if at_end { requested - line.start() } else { line.end() - requested };
            if travel.dot(direction) <= 0.0 {
                return Err(ContourEditError::ReversedSegment { segment });
            }
        }
        let (trimmed, corner) = with_corner(segment, entry.segment(), point, at_end)?;
        let neighbour_index = if at_end { (index + 1) % count } else { (index + count - 1) % count };
        let Some(neighbour) = entries.get(neighbour_index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let (before, after) = if at_end {
            join_corner(segment, trimmed, neighbour.id(), neighbour.segment(), corner)?
        } else {
            join_corner(neighbour.id(), neighbour.segment(), segment, trimmed, corner)?
        };
        let mut edited = entries.to_vec();
        if at_end {
            edited[index] = ContourSegmentEntry::new(segment, before);
            edited[neighbour_index] = ContourSegmentEntry::new(neighbour.id(), after);
        } else {
            edited[neighbour_index] = ContourSegmentEntry::new(neighbour.id(), before);
            edited[index] = ContourSegmentEntry::new(segment, after);
        }
        Ok(self.rebuild(edited)?)
    }

    /// Splits `segment` at a point strictly inside it.
    ///
    /// The part before the point keeps the segment's id and the part after it receives a fresh id
    /// from the ring counter, so a call site can select the new corner. Both parts keep the
    /// supporting curve of the original segment.
    pub fn split_segment(
        &self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<(Self, ContourSegmentId), ContourEditError> {
        let (index, _) = self.position(segment)?;
        let entries = self.entries();
        let Some(entry) = entries.get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let geometry = entry.segment().to_geom()?;
        let requested = point_to_geometry(point)?;
        let Some(nearest) = geometry.nearest_on_support(requested) else {
            return Err(ContourEditError::PointOffSegment { segment });
        };
        if (nearest - requested).length() > SUPPORT_TOLERANCE {
            return Err(ContourEditError::PointOffSegment { segment });
        }
        let (first, second) = split_document(entry.segment(), nearest)?;
        let inserted = self.next_segment_id();
        let mut edited = entries.to_vec();
        let Some(slot) = edited.get_mut(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *slot = ContourSegmentEntry::new(segment, first);
        if index >= edited.len() {
            return Err(ContourEditError::UnknownSegment { segment });
        }
        edited.insert(index + 1, ContourSegmentEntry::new(inserted, second));
        Ok((self.rebuild(edited)?, inserted))
    }

    /// The position of a segment and the number of segments in the ring.
    fn position(&self, segment: ContourSegmentId) -> Result<(usize, usize), ContourEditError> {
        let index = self.index_of(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        Ok((index, self.entries().len()))
    }
}

/// The same segment with one endpoint at `corner`, and the corner it actually reaches.
///
/// A line takes the corner exactly. An arc keeps its centre and radius and reaches the corner by
/// trimming or extending its sweep; a corner more than [`SUPPORT_TOLERANCE`] off the circle is an
/// error.
fn with_corner(
    id: ContourSegmentId,
    segment: ContourSegment,
    corner: [Length; 2],
    at_end: bool,
) -> Result<(ContourSegment, [Length; 2]), ContourEditError> {
    match segment {
        ContourSegment::Line { start, end } => {
            let moved = if at_end { ContourSegment::line(start, corner)? } else { ContourSegment::line(corner, end)? };
            Ok((moved, corner))
        }
        ContourSegment::Arc { center, radius, start_angle, sweep } => {
            let arc = geometry_arc(center, radius, start_angle, sweep)?;
            let requested = point_to_geometry(corner)?;
            let Some(nearest) = arc.nearest_on_support(requested) else {
                return Err(ContourEditError::PointOffCircle { segment: id });
            };
            if (nearest - requested).length() > SUPPORT_TOLERANCE {
                return Err(ContourEditError::PointOffCircle { segment: id });
            }
            let moved = if at_end { arc.with_end_point(nearest)? } else { arc.with_start_point(nearest)? };
            let moved = ContourSegment::from_geom(Segment::Arc(moved))?;
            let reached = if at_end { moved.end()? } else { moved.start()? };
            Ok((moved, reached))
        }
    }
}

/// Moves the shared corner of two segments, reconciling a rounded arc endpoint.
///
/// Both segments reach the requested point. Only an arc moves a corner off the exact request, by
/// rounding its angle to millidegrees, so a line on the other side is rebuilt at the arc's reached
/// corner. Two arcs must already agree; otherwise the corner is not representable in the document.
fn join_corner(
    before_id: ContourSegmentId,
    before: ContourSegment,
    after_id: ContourSegmentId,
    after: ContourSegment,
    corner: [Length; 2],
) -> Result<(ContourSegment, ContourSegment), ContourEditError> {
    let (mut before, before_corner) = with_corner(before_id, before, corner, true)?;
    let (mut after, after_corner) = with_corner(after_id, after, corner, false)?;
    if before_corner == after_corner {
        return Ok((before, after));
    }
    match (before, after) {
        (ContourSegment::Line { .. }, ContourSegment::Arc { .. }) => {
            before = with_corner(before_id, before, after_corner, true)?.0;
        }
        (ContourSegment::Arc { .. }, ContourSegment::Line { .. }) => {
            after = with_corner(after_id, after, before_corner, false)?.0;
        }
        _ => return Err(ContourEditError::ArcJointMismatch),
    }
    Ok((before, after))
}

/// Splits a document segment at a point on its supporting curve.
///
/// A line split converts both parts back to document units. An arc split is built from the stored
/// millidegrees so the two parts share the exact end angle of the first part: converting each part
/// on its own could round the shared corner to two different angles and open the ring.
fn split_document(segment: ContourSegment, point: DVec2) -> Result<(ContourSegment, ContourSegment), ContourEditError> {
    match segment {
        ContourSegment::Line { .. } => {
            let (first, second) = segment.to_geom()?.split_at(point)?;
            Ok((ContourSegment::from_geom(first)?, ContourSegment::from_geom(second)?))
        }
        ContourSegment::Arc { center, radius, start_angle, sweep } => {
            let arc = geometry_arc(center, radius, start_angle, sweep)?;
            let (first, _) = arc.split_at(point)?;
            let first_sweep =
                Degrees::from_radians(first.sweep_angle()).ok_or(ContourSegmentError::NonFinite)?.millidegrees();
            let split_angle =
                start_angle.millidegrees().checked_add(first_sweep).ok_or(ContourSegmentError::NonFinite)?;
            let end_angle =
                start_angle.millidegrees().checked_add(sweep.millidegrees()).ok_or(ContourSegmentError::NonFinite)?;
            let second_sweep = end_angle.checked_sub(split_angle).ok_or(ContourSegmentError::NonFinite)?;
            let first = ContourSegment::arc(center, radius, start_angle, Degrees::from_millidegrees(first_sweep))?;
            let second = ContourSegment::arc(
                center,
                radius,
                Degrees::from_millidegrees(split_angle),
                Degrees::from_millidegrees(second_sweep),
            )?;
            Ok((first, second))
        }
    }
}

/// The geometry arc a stored arc describes.
fn geometry_arc(
    center: [Length; 2],
    radius: Length,
    start_angle: Degrees,
    sweep: Degrees,
) -> Result<Arc, ContourEditError> {
    Ok(Arc::new(point_to_geometry(center)?, radius.millimetres_f64(), start_angle.radians_f64(), sweep.radians_f64())?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unreachable)]

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

    fn micrometres(value: i64) -> Length {
        Length::from_micrometres(value)
    }

    fn point(x: i64, y: i64) -> [Length; 2] {
        [millimetres(x), millimetres(y)]
    }

    fn fine_point(x: i64, y: i64) -> [Length; 2] {
        [micrometres(x), micrometres(y)]
    }

    fn id(value: u32) -> ContourSegmentId {
        ContourSegmentId::new(value).expect("a non-zero id")
    }

    fn ids(contour: &Contour) -> Vec<u32> {
        contour.entries().iter().map(|entry| entry.id().get()).collect()
    }

    fn rectangle() -> Contour {
        Contour::from_polygon(vec![point(0, 0), point(100, 0), point(100, 60), point(0, 60)]).expect("rectangle")
    }

    /// A rectangle whose bottom-right corner is a quarter arc of radius 10.
    fn rounded() -> Contour {
        Contour::new(vec![
            ContourSegment::line(point(0, 0), point(90, 0)).expect("bottom"),
            ContourSegment::arc(
                [millimetres(90), millimetres(10)],
                millimetres(10),
                Degrees::from_millidegrees(-90_000),
                Degrees::from_millidegrees(90_000),
            )
            .expect("corner arc"),
            ContourSegment::line(point(100, 10), point(100, 60)).expect("right"),
            ContourSegment::line(point(100, 60), point(0, 60)).expect("top"),
            ContourSegment::line(point(0, 60), point(0, 0)).expect("left"),
        ])
        .expect("rounded rectangle")
    }

    fn arc_fields(segment: ContourSegment) -> ([Length; 2], Length, i64, i64) {
        let ContourSegment::Arc { center, radius, start_angle, sweep } = segment else {
            panic!("the segment must be an arc");
        };
        (center, radius, start_angle.millidegrees(), sweep.millidegrees())
    }

    #[test]
    fn moving_a_corner_reshapes_both_neighbours_and_keeps_ids() {
        let contour = rectangle();
        let moved = contour.move_corner(id(2), point(110, -10)).expect("move the corner");
        assert_eq!(ids(&moved), vec![1, 2, 3, 4]);
        assert_eq!(moved.next_segment_id(), contour.next_segment_id());
        assert_eq!(moved.segment(id(1)).expect("bottom").end().expect("end"), point(110, -10));
        assert_eq!(moved.segment(id(2)).expect("right").start().expect("start"), point(110, -10));
        assert_eq!(moved.segment(id(3)).expect("top").start().expect("start"), point(100, 60));
    }

    #[test]
    fn moving_the_first_corner_wraps_to_the_last_segment() {
        let contour = rectangle();
        let moved = contour.move_corner(id(1), point(-10, -10)).expect("move the zero corner");
        assert_eq!(moved.segment(id(4)).expect("left").end().expect("end"), point(-10, -10));
        assert_eq!(moved.segment(id(1)).expect("bottom").start().expect("start"), point(-10, -10));
    }

    #[test]
    fn moving_a_corner_on_an_arc_keeps_the_circle() {
        let contour = rounded();
        // The point is on the arc's circle at -30 degrees, rounded to whole micrometres.
        let moved = contour.move_corner(id(2), fine_point(98_660, 5_000)).expect("move the arc's start");
        let (center, radius, start_angle, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(center, [millimetres(90), millimetres(10)]);
        assert_eq!(radius, millimetres(10));
        assert!((start_angle + 30_000).abs() <= 1, "start angle {start_angle}");
        assert!((sweep - 30_000).abs() <= 1, "sweep {sweep}");
        assert_eq!(
            moved.segment(id(1)).expect("bottom").end().expect("end"),
            moved.segment(id(2)).expect("arc").start().expect("start"),
            "the line follows the arc's reached corner"
        );
    }

    #[test]
    fn a_corner_off_an_arc_circle_is_rejected() {
        let contour = rounded();
        assert_eq!(contour.move_corner(id(2), point(90, 1)), Err(ContourEditError::PointOffCircle { segment: id(2) }));
    }

    #[test]
    fn an_unknown_segment_is_rejected() {
        assert_eq!(
            rectangle().move_corner(id(9), point(1, 1)),
            Err(ContourEditError::UnknownSegment { segment: id(9) })
        );
        assert!(matches!(
            rounded().trim_segment(id(9), true, point(1, 1)),
            Err(ContourEditError::UnknownSegment { segment }) if segment == id(9)
        ));
        assert!(matches!(
            rounded().split_segment(id(9), point(1, 1)),
            Err(ContourEditError::UnknownSegment { segment }) if segment == id(9)
        ));
    }

    #[test]
    fn a_move_that_crosses_the_ring_is_rejected() {
        let contour = rectangle();
        assert!(matches!(
            contour.move_corner(id(1), point(50, 100)),
            Err(ContourEditError::Ring(ContourError::SelfIntersecting { .. }))
        ));
    }

    #[test]
    fn trimming_a_line_moves_the_neighbour_and_keeps_ids() {
        let contour = rectangle();
        let trimmed = contour.trim_segment(id(2), true, point(100, 40)).expect("trim the right edge");
        assert_eq!(ids(&trimmed), vec![1, 2, 3, 4]);
        assert_eq!(trimmed.next_segment_id(), contour.next_segment_id());
        assert_eq!(trimmed.segment(id(2)).expect("right").start().expect("start"), point(100, 0));
        assert_eq!(trimmed.segment(id(2)).expect("right").end().expect("end"), point(100, 40));
        assert_eq!(trimmed.segment(id(3)).expect("top").start().expect("start"), point(100, 40));
    }

    #[test]
    fn trimming_at_the_start_moves_the_previous_segment() {
        let contour = rectangle();
        let trimmed = contour.trim_segment(id(2), false, point(100, 20)).expect("trim the start");
        assert_eq!(trimmed.segment(id(2)).expect("right").start().expect("start"), point(100, 20));
        assert_eq!(trimmed.segment(id(1)).expect("bottom").end().expect("end"), point(100, 20));
    }

    #[test]
    fn trimming_can_extend_a_segment_along_its_own_line() {
        let contour = rectangle();
        let extended = contour.trim_segment(id(2), true, point(100, 70)).expect("extend the right edge");
        assert_eq!(extended.segment(id(2)).expect("right").start().expect("start"), point(100, 0));
        assert_eq!(extended.segment(id(2)).expect("right").end().expect("end"), point(100, 70));
        assert_eq!(extended.segment(id(3)).expect("top").start().expect("start"), point(100, 70));
        assert_eq!(ids(&extended), vec![1, 2, 3, 4]);
    }

    #[test]
    fn moving_the_corner_after_an_arc_extends_the_arc() {
        let contour = rounded();
        let moved = contour.move_corner(id(3), fine_point(98_660, 15_000)).expect("extend the arc to 30 degrees");
        let (center, radius, _, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(center, [millimetres(90), millimetres(10)]);
        assert_eq!(radius, millimetres(10));
        assert!((sweep - 120_000).abs() <= 1, "sweep {sweep}");
        assert_eq!(
            moved.segment(id(3)).expect("right").start().expect("start"),
            moved.segment(id(2)).expect("arc").end().expect("end")
        );
    }

    #[test]
    fn a_trim_off_the_support_or_past_the_other_end_is_rejected() {
        let contour = rectangle();
        assert_eq!(
            contour.trim_segment(id(2), true, point(90, 30)),
            Err(ContourEditError::PointOffSegment { segment: id(2) })
        );
        assert_eq!(
            contour.trim_segment(id(2), true, point(100, -5)),
            Err(ContourEditError::ReversedSegment { segment: id(2) })
        );
    }

    #[test]
    fn trimming_an_arc_keeps_its_circle_and_moves_the_neighbour() {
        let contour = rounded();
        let trimmed = contour.trim_segment(id(2), true, fine_point(98_660, 5_000)).expect("trim the arc");
        let arc = trimmed.segment(id(2)).expect("arc");
        let (center, radius, _, sweep) = arc_fields(arc);
        assert_eq!(center, [millimetres(90), millimetres(10)]);
        assert_eq!(radius, millimetres(10));
        assert!((sweep - 60_000).abs() <= 1, "sweep {sweep}");
        assert_eq!(trimmed.segment(id(3)).expect("right").start().expect("start"), arc.end().expect("end"));
    }

    #[test]
    fn splitting_a_line_keeps_the_first_id_and_mints_a_new_one() {
        let contour = rectangle();
        let (split, inserted) = contour.split_segment(id(1), point(40, 0)).expect("split the bottom edge");
        assert_eq!(inserted, id(5));
        assert_eq!(split.next_segment_id(), id(6));
        assert_eq!(ids(&split), vec![1, 5, 2, 3, 4]);
        assert_eq!(split.segment(id(1)).expect("first").end().expect("end"), point(40, 0));
        assert_eq!(split.segment(inserted).expect("second").start().expect("start"), point(40, 0));
        assert_eq!(
            split.signed_area2().expect("area"),
            contour.signed_area2().expect("area"),
            "splitting a line does not change the shape"
        );
    }

    #[test]
    fn splitting_an_arc_keeps_the_circle_and_the_shared_joint() {
        let contour = rounded();
        let (split, inserted) = contour.split_segment(id(2), fine_point(98_660, 5_000)).expect("split the arc");
        assert_eq!(inserted, id(6));
        assert_eq!(split.next_segment_id(), id(7));
        assert_eq!(ids(&split), vec![1, 2, 6, 3, 4, 5]);
        let first = split.segment(id(2)).expect("first part");
        let second = split.segment(inserted).expect("second part");
        assert_eq!(first.end().expect("end"), second.start().expect("start"), "the two parts share one corner");
        assert_eq!(arc_fields(first).0, arc_fields(second).0);
        assert_eq!(arc_fields(first).1, arc_fields(second).1);
        assert_eq!(split.bounds_micrometres(), contour.bounds_micrometres());
        assert_eq!(split.entries().len(), contour.entries().len() + 1);
    }

    #[test]
    fn a_split_at_an_endpoint_or_off_the_segment_is_rejected() {
        let contour = rounded();
        assert!(matches!(
            contour.split_segment(id(1), point(0, 0)),
            Err(ContourEditError::Geometry(GeomError::SplitOutside))
        ));
        assert!(matches!(
            contour.split_segment(id(2), point(90, 0)),
            Err(ContourEditError::Geometry(GeomError::SplitOutside))
        ));
        assert_eq!(
            contour.split_segment(id(1), point(40, 5)),
            Err(ContourEditError::PointOffSegment { segment: id(1) })
        );
        assert!(matches!(
            contour.split_segment(id(1), point(150, 0)),
            Err(ContourEditError::Geometry(GeomError::SplitOutside))
        ));
    }

    fn open_path(points: Vec<[Length; 2]>) -> Path {
        Path::polyline(points, false).expect("an open path")
    }

    fn path_ids(path: &Path) -> Vec<u32> {
        path.entries().iter().map(|entry| entry.id().get()).collect()
    }

    /// A line from `(0, 0)` to `(100, 0)` joined to a quarter arc of radius 40, closing at 90°.
    fn line_and_arc() -> Path {
        Path::new(
            vec![
                ContourSegment::line(point(0, 0), point(100, 0)).expect("line"),
                ContourSegment::arc(
                    [millimetres(100), millimetres(40)],
                    millimetres(40),
                    Degrees::from_millidegrees(-90_000),
                    Degrees::from_millidegrees(90_000),
                )
                .expect("arc"),
            ],
            false,
        )
        .expect("line and arc")
    }

    #[test]
    fn an_open_chain_moves_its_first_and_its_last_corner() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60)]);

        let moved = path.move_corner_at(PathCorner::Start(id(1)), point(-20, -10)).expect("move the free start");
        assert_eq!(moved.segment(id(1)).expect("first").start().expect("start"), point(-20, -10));
        assert_eq!(
            moved.segment(id(2)).expect("second").start().expect("start"),
            point(100, 0),
            "the next segment keeps its start"
        );
        assert_eq!(path_ids(&moved), vec![1, 2]);

        let moved = path.move_corner_at(PathCorner::End(id(2)), point(120, 70)).expect("move the free end");
        assert_eq!(moved.segment(id(2)).expect("second").end().expect("end"), point(120, 70));
        assert_eq!(
            moved.segment(id(1)).expect("first").end().expect("end"),
            point(100, 0),
            "the previous segment keeps its end"
        );

        let moved = path.move_corner_at(PathCorner::End(id(1)), point(110, -10)).expect("move the shared corner");
        assert_eq!(moved.segment(id(1)).expect("first").end().expect("end"), point(110, -10));
        assert_eq!(moved.segment(id(2)).expect("second").start().expect("start"), point(110, -10));
    }

    #[test]
    fn an_open_arc_keeps_its_circle_when_its_shared_corner_moves() {
        let path = line_and_arc();
        // 45° below the axis, on the arc's circle, rounded to whole micrometres.
        let moved = path.move_corner_at(PathCorner::End(id(1)), fine_point(128_284, 11_716)).expect("move the corner");
        let (center, radius, start_angle, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(center, [millimetres(100), millimetres(40)]);
        assert_eq!(radius, millimetres(40));
        assert!((start_angle + 45_000).abs() <= 1, "start angle {start_angle}");
        assert!((sweep - 45_000).abs() <= 1, "sweep {sweep}");
        assert_eq!(
            moved.segment(id(1)).expect("line").end().expect("end"),
            moved.segment(id(2)).expect("arc").start().expect("start"),
            "the line follows the arc's reached corner"
        );
    }

    #[test]
    fn a_corner_off_the_arc_circle_is_refused_on_a_path() {
        let path = line_and_arc();
        assert_eq!(
            path.move_corner_at(PathCorner::End(id(1)), point(100, 5)),
            Err(ContourEditError::PointOffCircle { segment: id(2) })
        );
    }

    #[test]
    fn the_wrap_corner_of_a_closed_path_is_one_corner() {
        let ring = Path::rectangle(point(0, 0), point(100, 60)).expect("rectangle");
        let by_start = ring.move_corner_at(PathCorner::Start(id(1)), point(-10, -10)).expect("by start");
        let by_end = ring.move_corner_at(PathCorner::End(id(4)), point(-10, -10)).expect("by end");
        assert_eq!(path_ids(&by_start), path_ids(&by_end));
        assert_eq!(by_start.segment(id(1)).expect("bottom").start().expect("start"), point(-10, -10));
        assert_eq!(
            by_start.segment(id(4)).expect("left").end().expect("end"),
            by_end.segment(id(4)).expect("left").end().expect("end")
        );
    }

    #[test]
    fn splitting_an_open_path_keeps_the_first_id_and_mints_a_new_one() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60)]);
        let (split, inserted) = path.split_segment_at(id(1), point(40, 0)).expect("split the first side");
        assert_eq!(inserted, id(3));
        assert_eq!(path_ids(&split), vec![1, 3, 2]);
        assert_eq!(split.next_segment_id(), id(4));
        assert!(!split.is_closed(), "a split keeps the chain open");
        assert_eq!(split.segment(id(1)).expect("first part").end().expect("end"), point(40, 0));
        assert_eq!(split.segment(inserted).expect("second part").start().expect("start"), point(40, 0));
    }

    #[test]
    fn removing_a_segment_joins_the_neighbours() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60), point(0, 60)]);

        let removed = path.remove_segment(id(2)).expect("remove the right side");
        assert_eq!(path_ids(&removed), vec![1, 3]);
        assert_eq!(
            removed.segment(id(1)).expect("bottom").end().expect("end"),
            point(100, 0),
            "the previous segment stays where it was"
        );
        assert_eq!(
            removed.segment(id(3)).expect("top").start().expect("start"),
            point(100, 0),
            "the next segment reaches the joined corner"
        );
        assert!(!removed.is_closed());

        let first = path.remove_segment(id(1)).expect("remove the free start");
        assert_eq!(path_ids(&first), vec![2, 3]);
        assert_eq!(first.segment(id(2)).expect("right").start().expect("start"), point(100, 0));

        let last = path.remove_segment(id(3)).expect("remove the free end");
        assert_eq!(path_ids(&last), vec![1, 2]);
    }

    #[test]
    fn removing_a_segment_from_a_closed_path_recloses_it() {
        let ring = Path::rectangle(point(0, 0), point(100, 60)).expect("rectangle");
        let removed = ring.remove_segment(id(1)).expect("remove the bottom");
        assert!(removed.is_closed());
        assert_eq!(path_ids(&removed), vec![2, 3, 4]);
        assert_eq!(
            removed.segment(id(2)).expect("right").start().expect("start"),
            point(0, 0),
            "the last segment's end and the next start meet"
        );
        assert!(removed.to_contour().is_ok(), "the ring stays a ring");
    }

    #[test]
    fn a_path_of_one_segment_has_nothing_to_remove() {
        let line =
            Path::new(vec![ContourSegment::line(point(0, 0), point(100, 0)).expect("line")], false).expect("one line");
        assert_eq!(line.remove_segment(id(1)), Err(ContourEditError::OnlySegment));
    }

    #[test]
    fn closing_a_chain_adds_the_bridge_and_keeps_every_id() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60)]);
        let closed = path.close().expect("a triangle closes");
        assert!(closed.is_closed());
        assert_eq!(path_ids(&closed), vec![1, 2, 3]);
        assert_eq!(closed.next_segment_id(), id(4));
        assert_eq!(closed.entries().len(), 3, "two lines and one bridge");
        assert_eq!(closed.to_contour().expect("a ring").entries().len(), 3);

        // The last line already ends where the chain began, so no bridge is needed.
        let returning = open_path(vec![point(0, 0), point(100, 0), point(100, 60), point(0, 0)]);
        let closed = returning.close().expect("a triangle closes");
        assert_eq!(closed.entries().len(), 3, "the ends already meet");
        assert_eq!(closed.next_segment_id(), returning.next_segment_id());
    }

    #[test]
    fn closing_an_already_closed_path_changes_nothing() {
        let ring = Path::rectangle(point(0, 0), point(100, 60)).expect("rectangle");
        let closed = ring.close().expect("closing a ring is harmless");
        assert!(closed.is_closed());
        assert_eq!(path_ids(&closed), path_ids(&ring));
        assert_eq!(closed.next_segment_id(), ring.next_segment_id());
    }

    #[test]
    fn closing_refuses_a_bridge_that_crosses_the_chain() {
        // The free end sits in the pocket of the spiral, so the bridge crosses the y = 20 side.
        let path = open_path(vec![
            point(0, 0),
            point(100, 0),
            point(100, 100),
            point(0, 100),
            point(0, 20),
            point(80, 20),
            point(80, 80),
            point(20, 80),
            point(20, 50),
        ]);
        let result = path.close();
        assert!(matches!(result, Err(ContourEditError::Ring(ContourError::SelfIntersecting { .. }))), "got {result:?}");
    }
}
