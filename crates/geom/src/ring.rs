//! The planar ring model: one material component as exact closed rings.
//!
//! A [`Region`] is a closed outer [`Contour`] ring and zero or more hole rings, in millimetres
//! (ADR 0009, ADR 0012). Every ring stays exact: a line stays a line and an arc stays an arc, so
//! area, bounds and containment never depend on a sampling tolerance. [`Region::sample`] is the only
//! lossy step, and a caller picks its tolerance once, at the polygon-only kernel boundary.
//!
//! The contract: the constructors refuse an open, degenerate or non-finite ring and a ring that
//! repeats a neighbour, naming the ring and the point in [`RegionError`]. Coincidence is
//! [`POINT_EPSILON`] millimetres and an angle counts as inside a sweep at [`ANGLE_EPSILON`] radians;
//! the rectangle tracer in [`crate::rect`] welds at the same point bound instead of declaring its own.
//!
//! It does not combine regions: the general boolean is [`crate::boolean`], the exact orthogonal one
//! over plain rectangles is [`crate::rect`]. Holes are not checked against the outer ring here.
//!
//! Changes when: what a region is, or what an exact ring answers, changes.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt;

use crate::rect::{Rect, RectError};
use crate::{Arc, Contour, ContourError, DVec2, Line, Segment};

/// Coincidence tolerance for ring points, in millimetres.
///
/// The rectangle boolean welds its corners at the same bound, so it reads this constant from here
/// instead of declaring a second one.
pub(crate) const POINT_EPSILON: f64 = 1.0e-9;

/// Tolerance for two angles of a ring to count as the same direction, in radians.
///
/// An arc angle closer than this to a sweep bound counts as on it, and the general boolean reads it
/// for a turn and a joint sweep, so it stays beside the ring model instead of being declared twice.
pub(crate) const ANGLE_EPSILON: f64 = 1.0e-9;

/// One material component in the plane: a closed outer ring and zero or more hole rings.
///
/// Every ring is an exact [`Contour`]; a ring built from sampled points is a contour of line
/// segments, so nothing is lost either way. Rings are validated on construction: each must be
/// closed. Holes are not checked against the outer ring here; [`crate::rect::regions_from_rects`]
/// produces consistent regions from the rectangle decomposition.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    outer: Contour,
    holes: Vec<Contour>,
}

impl Region {
    /// Creates a region from exact closed rings.
    pub fn new(outer: Contour, holes: Vec<Contour>) -> Result<Self, RegionError> {
        validate_ring(&outer, RingKind::Outer)?;
        for (index, hole) in holes.iter().enumerate() {
            validate_ring(hole, RingKind::Hole { index })?;
        }
        Ok(Self { outer, holes })
    }

    /// Creates a region from sampled rings: every closing edge becomes an exact line segment.
    ///
    /// This is the bridge for a traced polyline such as the rectangle decomposition. A ring with
    /// fewer than three points, a non-finite point or a repeated neighbour is refused with the
    /// matching [`RegionError`].
    pub fn from_loops(outer: Vec<DVec2>, holes: Vec<Vec<DVec2>>) -> Result<Self, RegionError> {
        let outer = loop_to_contour(outer, RingKind::Outer)?;
        let holes = holes
            .into_iter()
            .enumerate()
            .map(|(index, hole)| loop_to_contour(hole, RingKind::Hole { index }))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(outer, holes)
    }

    /// The region of one axis-aligned rectangle at the origin.
    ///
    /// The outer ring runs counter-clockwise from the minimum corner. A zero, negative or
    /// non-finite width or height is [`RectError`], so a caller can report the bad dimension.
    pub fn rectangle(width: f64, height: f64) -> Result<Self, RegionError> {
        let rect = Rect::new(DVec2::ZERO, DVec2::new(width, height))?;
        Self::from_rect(rect)
    }

    /// The region of an already-validated rectangle.
    pub fn from_rect(rect: Rect) -> Result<Self, RegionError> {
        let min = rect.min();
        let max = rect.max();
        let ring = vec![min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
        Self::from_loops(ring, Vec::new())
    }

    /// The outer ring.
    pub fn outer(&self) -> &Contour {
        &self.outer
    }

    /// The hole rings.
    pub fn holes(&self) -> &[Contour] {
        &self.holes
    }

    /// The material area: the outer ring's area minus every hole's area, in square millimetres.
    /// Ring orientation does not matter; an invalid region whose holes exceed the outer ring
    /// yields a non-positive value, which callers turn into a diagnostic, never a panic.
    pub fn area(&self) -> f64 {
        contour_area(&self.outer) - self.holes.iter().map(contour_area).sum::<f64>()
    }

    /// The axis-aligned bounds of every ring, in millimetres.
    ///
    /// Returns `None` only for a region with no rings, which the constructors reject. An arc
    /// contributes its own extremes, not its chord.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut bounds: Option<(DVec2, DVec2)> = None;
        for ring in std::iter::once(&self.outer).chain(self.holes.iter()) {
            let Some((min, max)) = ring.bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                Some((current_min, current_max)) => (current_min.min(min), current_max.max(max)),
                None => (min, max),
            });
        }
        bounds
    }

    /// Even-odd containment: a point is inside the material when it is inside the outer ring and in
    /// no hole. The ray test walks exact lines and arcs, so a curved edge is never approximated.
    pub fn contains(&self, point: DVec2) -> bool {
        if !ring_contains(&self.outer, point) {
            return false;
        }
        for hole in &self.holes {
            if ring_contains(hole, point) {
                return false;
            }
        }
        true
    }

    /// Samples every ring into a polyline at `tolerance` millimetres.
    ///
    /// This is the only lossy step in the module. A caller that reaches the polygon-only kernel
    /// picks the tolerance once and shares it with every other profile.
    pub fn sample(&self, tolerance: f64) -> Result<RegionSamples, ContourError> {
        let outer = self.outer.sample(tolerance)?;
        let holes = self.holes.iter().map(|hole| hole.sample(tolerance)).collect::<Result<Vec<_>, _>>()?;
        Ok(RegionSamples { outer, holes })
    }
}

/// A region sampled into polylines: the outer ring and then the hole rings in ring order.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionSamples {
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
}

impl RegionSamples {
    /// The sampled outer ring; the first point is not repeated at the end.
    pub fn outer(&self) -> &[DVec2] {
        &self.outer
    }

    /// The sampled hole rings.
    pub fn holes(&self) -> &[Vec<DVec2>] {
        &self.holes
    }

    /// Consumes the samples into the outer ring and the hole rings.
    pub fn into_parts(self) -> (Vec<DVec2>, Vec<Vec<DVec2>>) {
        (self.outer, self.holes)
    }
}

/// Which ring a [`RegionError`] refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingKind {
    /// The outer ring.
    Outer,
    /// The hole at `index`.
    Hole {
        /// Index of the hole.
        index: usize,
    },
}

impl fmt::Display for RingKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Outer => formatter.write_str("outer"),
            Self::Hole { index } => write!(formatter, "hole {index}"),
        }
    }
}

/// Why a [`Region`] cannot be constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionError {
    /// A ring has fewer than three points.
    Degenerate {
        /// The ring that is degenerate.
        kind: RingKind,
    },
    /// A ring contains a point that is not finite.
    NonFinite {
        /// The ring that contains the point.
        kind: RingKind,
        /// Index of the offending point.
        index: usize,
    },
    /// Two consecutive points of a ring are closer than [`POINT_EPSILON`].
    DuplicatePoint {
        /// The ring that contains the points.
        kind: RingKind,
        /// Index of the first of the two points.
        index: usize,
    },
    /// A ring is an open contour, so it bounds no material.
    OpenRing {
        /// The ring that is open.
        kind: RingKind,
    },
    /// A ring cannot become a contour of line segments.
    Ring {
        /// The ring that cannot be built.
        kind: RingKind,
        /// Why the contour was refused.
        error: ContourError,
    },
    /// A rectangle the region would be built from has no positive extent.
    Rect(RectError),
    /// A boolean would examine more segment pairs than it may.
    TooComplex {
        /// The number of segments the operands hold.
        segments: usize,
        /// The largest pair count one boolean accepts.
        max: usize,
    },
    /// A boolean result ring could not be built.
    ResultRing {
        /// Why the ring was refused.
        error: ContourError,
    },
}

impl fmt::Display for RegionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Degenerate { kind } => write!(formatter, "the {kind} ring needs at least three points"),
            Self::NonFinite { kind, index } => {
                write!(formatter, "point {index} of the {kind} ring is not finite")
            }
            Self::DuplicatePoint { kind, index } => {
                write!(formatter, "point {index} of the {kind} ring repeats its neighbour")
            }
            Self::OpenRing { kind } => write!(formatter, "the {kind} ring is not closed"),
            Self::Ring { kind, error } => write!(formatter, "the {kind} ring is invalid: {error}"),
            Self::Rect(error) => write!(formatter, "{error}"),
            Self::TooComplex { segments, max } => write!(
                formatter,
                "a boolean over {segments} segments needs more than the {max} edge pairs it may examine"
            ),
            Self::ResultRing { error } => write!(formatter, "a boolean result ring is invalid: {error}"),
        }
    }
}

impl From<RectError> for RegionError {
    fn from(error: RectError) -> Self {
        Self::Rect(error)
    }
}

impl std::error::Error for RegionError {}

/// The unsigned area of one exact ring; zero for a ring that cannot be measured.
///
/// Shared with the general boolean, which pairs a result hole with the smallest ring that holds it.
pub(crate) fn contour_area(ring: &Contour) -> f64 {
    ring.signed_area().unwrap_or(0.0).abs()
}

/// Even-odd containment by a ray to the right: an odd number of crossings is inside.
///
/// Shared with the rectangle tracer, which assigns a traced hole to the outer ring that holds it.
pub(crate) fn ring_contains(ring: &Contour, point: DVec2) -> bool {
    if !point.is_finite() {
        return false;
    }
    let mut inside = false;
    for segment in ring.segments() {
        if segment_crossings(*segment, point) % 2 == 1 {
            inside = !inside;
        }
    }
    inside
}

/// Crossings of one segment with the horizontal ray that runs right from `point`.
///
/// A segment counts its start and not its end, so a shared joint is counted once. A curve that
/// merely touches the ray is not a crossing.
fn segment_crossings(segment: Segment, point: DVec2) -> usize {
    match segment {
        Segment::Line(line) => line_crossings(line, point),
        Segment::Arc(arc) => arc_crossings(arc, point),
    }
}

fn line_crossings(line: Line, point: DVec2) -> usize {
    let start = line.start();
    let end = line.end();
    if (start.y > point.y) == (end.y > point.y) {
        return 0;
    }
    let parameter = (point.y - start.y) / (end.y - start.y);
    let x = start.x + (end.x - start.x) * parameter;
    usize::from(x > point.x)
}

/// Crossings of an arc with the horizontal ray, split at the arc's Y extremes.
///
/// Splitting at every `±π/2` angle makes each piece monotone in Y, so the same half-open rule as a
/// line applies and a joint between two pieces is counted once.
fn arc_crossings(arc: Arc, point: DVec2) -> usize {
    let total = arc.sweep_angle().abs();
    if !total.is_finite() || total <= ANGLE_EPSILON {
        return 0;
    }
    let mut fractions = vec![0.0_f64];
    for step in -2..=2 {
        let angle = FRAC_PI_2 + step as f64 * PI;
        let delta = if arc.sweep_angle() > 0.0 {
            turn_angle(angle - arc.start_angle())
        } else {
            turn_angle(arc.start_angle() - angle)
        };
        if delta > ANGLE_EPSILON && delta < total - ANGLE_EPSILON {
            fractions.push(delta / total);
        }
    }
    fractions.push(1.0);
    fractions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    fractions.dedup_by(|a, b| (*a - *b).abs() < ANGLE_EPSILON);

    let mut crossings = 0;
    for pair in fractions.windows(2) {
        let (Some(start), Some(end)) = (arc.point_at(pair[0]), arc.point_at(pair[1])) else {
            continue;
        };
        if (start.y > point.y) == (end.y > point.y) {
            continue;
        }
        let Some(middle) = arc.point_at((pair[0] + pair[1]) * 0.5) else {
            continue;
        };
        let side = (middle.x - arc.center().x).signum();
        if side == 0.0 {
            continue;
        }
        let ratio = ((point.y - arc.center().y) / arc.radius()).clamp(-1.0, 1.0);
        let x = arc.center().x + side * arc.radius() * (1.0 - ratio * ratio).max(0.0).sqrt();
        if x > point.x {
            crossings += 1;
        }
    }
    crossings
}

/// The same angle in `0..TAU`; shared with the general boolean, which measures a turn by it.
pub(crate) fn turn_angle(angle: f64) -> f64 {
    let mut value = angle % TAU;
    if value < 0.0 {
        value += TAU;
    }
    value
}

fn validate_ring(ring: &Contour, kind: RingKind) -> Result<(), RegionError> {
    if ring.is_closed() { Ok(()) } else { Err(RegionError::OpenRing { kind }) }
}

/// Builds an exact line contour from a point loop, closing it from the last point to the first.
///
/// Shared with the rectangle tracer, which turns each closed boundary walk into a ring.
pub(crate) fn loop_to_contour(points: Vec<DVec2>, kind: RingKind) -> Result<Contour, RegionError> {
    let mut points = points;
    if points.len() > 1
        && let (Some(first), Some(last)) = (points.first(), points.last())
        && (first.x - last.x).abs() <= POINT_EPSILON
        && (first.y - last.y).abs() <= POINT_EPSILON
    {
        points.pop();
    }
    if points.len() < 3 {
        return Err(RegionError::Degenerate { kind });
    }
    for (index, point) in points.iter().enumerate() {
        if !point.is_finite() {
            return Err(RegionError::NonFinite { kind, index });
        }
    }

    let mut segments = Vec::with_capacity(points.len());
    for (index, &start) in points.iter().enumerate() {
        let Some(&end) = points.get((index + 1) % points.len()) else {
            return Err(RegionError::Degenerate { kind });
        };
        let line = Line::new(start, end).map_err(|error| match error {
            ContourError::ZeroLength => RegionError::DuplicatePoint { kind, index },
            ContourError::InvalidInput(_) => RegionError::NonFinite { kind, index },
            error => RegionError::Ring { kind, error },
        })?;
        segments.push(Segment::Line(line));
    }
    match Contour::new(segments, true) {
        Ok(contour) => Ok(contour),
        Err(ContourError::Degenerate) => Err(RegionError::Degenerate { kind }),
        Err(error) => Err(RegionError::Ring { kind, error }),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<DVec2> {
        vec![DVec2::new(x, y), DVec2::new(x + width, y), DVec2::new(x + width, y + height), DVec2::new(x, y + height)]
    }

    fn rounded_corner() -> f64 {
        let radius = 20.0;
        radius * radius - PI * radius * radius / 4.0
    }

    fn rounded_plate() -> Region {
        let radius = 20.0;
        let segments = vec![
            Segment::Line(Line::new(DVec2::ZERO, DVec2::new(100.0, 0.0)).expect("bottom")),
            Segment::Line(Line::new(DVec2::new(100.0, 0.0), DVec2::new(100.0, 50.0 - radius)).expect("right")),
            Segment::Arc(Arc::new(DVec2::new(100.0 - radius, 50.0 - radius), radius, 0.0, FRAC_PI_2).expect("corner")),
            Segment::Line(Line::new(DVec2::new(100.0 - radius, 50.0), DVec2::new(0.0, 50.0)).expect("top")),
            Segment::Line(Line::new(DVec2::new(0.0, 50.0), DVec2::ZERO).expect("left")),
        ];
        Region::new(Contour::new(segments, true).expect("rounded plate"), vec![]).expect("region")
    }

    fn rect_with_hole() -> Region {
        Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![rect(10.0, 10.0, 20.0, 10.0)]).expect("a rect with a hole")
    }

    #[test]
    fn a_rectangle_has_its_area() {
        let region = Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![]).expect("valid rect");
        assert!((region.area() - 5000.0).abs() < 1.0e-9);
    }

    #[test]
    fn the_rectangle_constructor_runs_counter_clockwise_from_the_origin() {
        let region = Region::rectangle(100.0, 50.0).expect("valid rectangle");
        assert_eq!(region.outer().segments().len(), 4);
        assert_eq!(region.outer().start(), Some(DVec2::ZERO));
        assert!(region.outer().is_counter_clockwise() == Some(true));
        assert_eq!(region.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 50.0))));
        assert!((region.area() - 5000.0).abs() < 1.0e-9);
        assert!(region.contains(DVec2::new(50.0, 25.0)));
        assert!(!region.contains(DVec2::new(150.0, 25.0)));
    }

    #[test]
    fn a_rectangle_sampled_through_the_region_keeps_its_four_corners() {
        let region = Region::rectangle(100.0, 50.0).expect("valid rectangle");
        let samples = region.sample(0.05).expect("samples");
        assert_eq!(samples.outer(), &rect(0.0, 0.0, 100.0, 50.0)[..]);
        assert!(samples.holes().is_empty());
        let (outer, holes) = samples.into_parts();
        assert_eq!(outer.len(), 4);
        assert!(holes.is_empty());
    }

    #[test]
    fn a_rectangle_without_positive_extent_is_rejected() {
        assert!(matches!(Region::rectangle(0.0, 10.0), Err(RegionError::Rect(RectError::Degenerate))));
        assert!(matches!(Region::rectangle(10.0, -1.0), Err(RegionError::Rect(RectError::Degenerate))));
        assert!(matches!(Region::rectangle(f64::NAN, 10.0), Err(RegionError::Rect(RectError::NonFinite))));
    }

    #[test]
    fn a_hole_subtracts_its_area() {
        let region = rect_with_hole();
        let outer_area = 100.0 * 50.0;
        let hole_area = 20.0 * 10.0;
        assert!((region.area() - (outer_area - hole_area)).abs() < 1.0e-9);
    }

    #[test]
    fn points_inside_the_material_are_contained() {
        let region = rect_with_hole();
        assert!(region.contains(DVec2::new(5.0, 5.0)));
        assert!(region.contains(DVec2::new(90.0, 40.0)));
    }

    #[test]
    fn a_point_in_a_hole_is_not_contained() {
        let region = rect_with_hole();
        assert!(!region.contains(DVec2::new(15.0, 15.0)));
    }

    #[test]
    fn degenerate_rings_are_rejected() {
        assert!(matches!(Region::from_loops(vec![], vec![]), Err(RegionError::Degenerate { kind: RingKind::Outer })));
        assert!(matches!(
            Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![vec![]]),
            Err(RegionError::Degenerate { kind: RingKind::Hole { index: 0 } })
        ));
    }

    #[test]
    fn duplicate_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(0.0, 10.0)];
        assert!(matches!(
            Region::from_loops(ring, vec![]),
            Err(RegionError::DuplicatePoint { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn non_finite_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(f64::NAN, 5.0), DVec2::new(3.0, 0.0)];
        assert!(matches!(
            Region::from_loops(ring, vec![]),
            Err(RegionError::NonFinite { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn an_open_ring_is_rejected() {
        let open = Contour::new(
            vec![
                Segment::Line(Line::new(DVec2::ZERO, DVec2::new(10.0, 0.0)).expect("bottom")),
                Segment::Line(Line::new(DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)).expect("right")),
            ],
            false,
        )
        .expect("open path");
        assert!(matches!(Region::new(open, vec![]), Err(RegionError::OpenRing { kind: RingKind::Outer })));
    }

    #[test]
    fn an_exact_ring_answers_area_bounds_and_containment_without_sampling() {
        let region = rounded_plate();
        assert!((region.area() - (5000.0 - rounded_corner())).abs() < 1.0e-9);
        assert_eq!(region.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 50.0))));
        assert!(region.contains(DVec2::new(50.0, 25.0)));
        assert!(!region.contains(DVec2::new(95.0, 45.0)), "the rounded corner is cut away");
        let samples = region.sample(0.05).expect("samples");
        assert!(samples.outer().len() > region.outer().segments().len());
    }

    #[test]
    fn an_arc_ring_contains_points_by_its_own_curve() {
        let ring = Contour::new(
            vec![
                Segment::Line(Line::new(DVec2::ZERO, DVec2::new(20.0, 0.0)).expect("bottom")),
                Segment::Arc(Arc::new(DVec2::new(20.0, 10.0), 10.0, -FRAC_PI_2, PI).expect("right half")),
                Segment::Line(Line::new(DVec2::new(20.0, 20.0), DVec2::ZERO).expect("left")),
            ],
            true,
        )
        .expect("arc ring");
        let region = Region::new(ring, vec![]).expect("region");
        assert!(region.contains(DVec2::new(28.0, 10.0)), "inside the bulge");
        assert!(region.contains(DVec2::new(22.0, 10.0)), "inside the bulge");
        assert!(!region.contains(DVec2::new(32.0, 10.0)), "right of the bulge");
        assert!(region.contains(DVec2::new(10.0, 5.0)), "inside the triangle");
        assert!(!region.contains(DVec2::new(5.0, 10.0)), "above the diagonal");
    }

    #[test]
    fn area_ignores_ring_orientation() {
        let clockwise_outer =
            vec![DVec2::new(0.0, 0.0), DVec2::new(0.0, 10.0), DVec2::new(10.0, 10.0), DVec2::new(10.0, 0.0)];
        let region = Region::from_loops(clockwise_outer, vec![rect(2.0, 2.0, 6.0, 6.0)]).expect("valid region");
        assert!((region.area() - 64.0).abs() < 1.0e-9);
    }
}
