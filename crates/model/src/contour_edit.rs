//! Editing a contour ring: move a corner, trim a segment and split a segment.
//!
//! An edit works on the stored ring and returns a new one, so the caller decides when a working copy
//! reaches the document. Every segment an edit does not reshape keeps its id, and a split mints the
//! new part from the ring counter; banding and selection keyed by [`ContourSegmentId`] therefore
//! survive an edit. A corner is named by the segment that starts there.
//!
//! An arc keeps its centre and radius: a corner on an arc is projected onto its circle and the sweep
//! is trimmed or extended, never redefined. A corner more than one micrometre off the circle is an
//! error, because redefining the circle would change the curve the user drew. The same holds for a
//! trim or a split: the point must lie on the segment's own supporting line or circle.

use furnicraft_geom::{Arc, ContourError as GeomError, DVec2, Segment};
use furnicraft_units::{Degrees, Length};
use thiserror::Error;

use crate::contour::point_to_geometry;
use crate::{Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, ContourSegmentId};

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
        edited[index] = ContourSegmentEntry::new(segment, first);
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
}
