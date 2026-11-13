//! Contour segments as the document stores them.
//!
//! A contour is a closed ring of directed lines and arcs in a local XY plane. A [`Path`] is an
//! ordered chain of the same segments: open or closed, not necessarily simple, and capped at the
//! ring size it eventually has to become. The document keeps micrometres and millidegrees; this
//! module is the only place where a segment becomes floating-point millimetres, and the only place
//! where it comes back. Callers never see a `furnicraft_geom` type, so the geometry layer stays
//! free to change. The maths over a segment — a projection, a split, an arc rebuilt through new
//! ends — is `furnicraft_geom::segment`'s; this module converts once at its edge and computes no
//! trigonometry of its own.
//!
//! Validation is total: every coordinate is inside [`MAX_CONTOUR_COORDINATE`], a line has two
//! distinct endpoints, an arc has a positive radius and a non-zero sweep no larger than a full
//! turn. Values that arrive from a file go through the same constructors, so a hostile scene is an
//! error the user can read rather than a broken ring.

use std::collections::HashSet;
use std::f64::consts::TAU;

use furnicraft_geom::{Arc, DVec2, Line, Segment};
use furnicraft_units::{Degrees, Length};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ContourSegmentId;

/// One directed segment of a planar contour.
///
/// A closed ring stores its segments in travel order: the end of one segment is the start of the
/// next. Angles and lengths are document units, never floats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", try_from = "ContourSegmentWire")]
pub enum ContourSegment {
    /// A straight segment from `start` to `end`.
    Line {
        /// The start point in the contour plane.
        start: [Length; 2],
        /// The end point in the contour plane.
        end: [Length; 2],
    },
    /// A circular arc running counter-clockwise for a positive sweep and clockwise for a negative
    /// one.
    Arc {
        /// The centre of the circle.
        center: [Length; 2],
        /// The radius, positive.
        radius: Length,
        /// The angle, measured from `+X`, where the arc starts.
        start_angle: Degrees,
        /// The signed sweep; its magnitude is at most a full turn.
        sweep: Degrees,
    },
}

impl ContourSegment {
    /// Creates a straight segment.
    ///
    /// The endpoints must differ and lie inside the accepted coordinate range.
    pub fn line(start: [Length; 2], end: [Length; 2]) -> Result<Self, ContourSegmentError> {
        let segment = Self::Line { start, end };
        segment.validate()?;
        Ok(segment)
    }

    /// Creates a circular arc.
    pub fn arc(
        center: [Length; 2],
        radius: Length,
        start_angle: Degrees,
        sweep: Degrees,
    ) -> Result<Self, ContourSegmentError> {
        let segment = Self::Arc { center, radius, start_angle, sweep };
        segment.validate()?;
        Ok(segment)
    }

    /// One open arc from `start` to `end` that passes through `through`.
    ///
    /// The arc is the way round from `start` to `end` that contains the middle point, so its sweep
    /// is negative when that way is clockwise. The three points must be distinct and must not lie on
    /// one line, and the stored centre, radius and angle are whole document units, so the stored end
    /// can miss `end` by a micrometre or two. A drawing follows the stored end rather than the click
    /// (ADR 0048).
    pub fn arc_through(
        start: [Length; 2],
        through: [Length; 2],
        end: [Length; 2],
    ) -> Result<Self, ContourSegmentError> {
        if start == through || through == end || start == end {
            return Err(ContourSegmentError::DegenerateArc);
        }
        let a = point_to_geometry(start)?;
        let b = point_to_geometry(through)?;
        let c = point_to_geometry(end)?;
        // Twice the signed area of the triangle; zero means the points share one line.
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        let span = (b - a).length_squared().max((c - a).length_squared());
        if !area.is_finite() || area.abs() <= 1.0e-9 * span {
            return Err(ContourSegmentError::DegenerateArc);
        }
        let u = b - a;
        let v = c - a;
        let denominator = 2.0 * area;
        let u2 = u.length_squared();
        let v2 = v.length_squared();
        let center = a + DVec2::new((u2 * v.y - v2 * u.y) / denominator, (u.x * v2 - v.x * u2) / denominator);
        let radius = (a - center).length();
        let angle_to = |point: DVec2| (point.y - center.y).atan2(point.x - center.x);
        let start_angle = angle_to(a);
        let end_angle = angle_to(c);
        let through_angle = angle_to(b);
        let counter_clockwise = (end_angle - start_angle).rem_euclid(TAU);
        let through_turn = (through_angle - start_angle).rem_euclid(TAU);
        let sweep = if through_turn < counter_clockwise { counter_clockwise } else { counter_clockwise - TAU };
        let center = point_to_document(center)?;
        let radius = length_to_document(radius)?;
        let start_angle = degrees_to_document(start_angle)?;
        let sweep = degrees_to_document(sweep)?;
        Self::arc(center, radius, start_angle, sweep)
    }

    /// Whether this segment is a circular arc.
    pub const fn is_arc(self) -> bool {
        matches!(self, Self::Arc { .. })
    }

    /// The same curve with the opposite direction of travel.
    ///
    /// A ring is stored counter-clockwise, so a contour drawn clockwise reverses every segment.
    pub fn reversed(self) -> Result<Self, ContourSegmentError> {
        match self {
            Self::Line { start, end } => Self::line(end, start),
            Self::Arc { center, radius, start_angle, sweep } => {
                let end_angle = start_angle
                    .millidegrees()
                    .checked_add(sweep.millidegrees())
                    .map(Degrees::from_millidegrees)
                    .ok_or(ContourSegmentError::NonFinite)?;
                let reversed_sweep = sweep
                    .millidegrees()
                    .checked_neg()
                    .map(Degrees::from_millidegrees)
                    .ok_or(ContourSegmentError::NonFinite)?;
                Self::arc(center, radius, end_angle, reversed_sweep)
            }
        }
    }

    /// The start point in the contour plane.
    pub fn start(self) -> Result<[Length; 2], ContourSegmentError> {
        point_to_document(self.to_geom()?.start())
    }

    /// The end point in the contour plane.
    pub fn end(self) -> Result<[Length; 2], ContourSegmentError> {
        point_to_document(self.to_geom()?.end())
    }

    /// The point on this segment at `distance` from `far_end`, measured along the curve.
    ///
    /// `far_end` must be one of the segment's own endpoints. A line measures a straight distance,
    /// an arc measures its arc length. The distance is clamped to the segment's own extent, so a
    /// caller can pass a typed length without checking it first.
    pub fn point_at_distance_from(
        self,
        far_end: [Length; 2],
        distance: Length,
    ) -> Result<[Length; 2], ContourSegmentError> {
        match self {
            Self::Line { start, end } => {
                let start = point_to_geometry(start)?;
                let end = point_to_geometry(end)?;
                let far = point_to_geometry(far_end)?;
                let direction = end - start;
                let length = direction.length();
                if !length.is_finite() || length <= 0.0 {
                    return Err(ContourSegmentError::ZeroLength);
                }
                let unit = direction / length;
                let distance = distance.millimetres_f64().clamp(0.0, length);
                // Travel from the far end toward the other endpoint.
                let toward = if (far - start).length() <= (far - end).length() { unit } else { -unit };
                point_to_document(far + toward * distance)
            }
            Self::Arc { center, radius, start_angle, sweep } => {
                let center = point_to_geometry(center)?;
                let radius_mm = radius.millimetres_f64();
                let start_r = start_angle.radians_f64();
                let sweep_r = sweep.radians_f64();
                let far = point_to_geometry(far_end)?;
                let arc_len = sweep_r.abs() * radius_mm;
                if !arc_len.is_finite() || arc_len <= 0.0 || radius_mm <= 0.0 {
                    return Err(ContourSegmentError::InvalidSweep);
                }
                let distance = distance.millimetres_f64().clamp(0.0, arc_len);
                let far_angle = (far - center).y.atan2((far - center).x);
                let start_point = DVec2::new(start_r.cos(), start_r.sin()) * radius_mm;
                let at_start = (far - (center + start_point)).length() < 1.0e-3;
                let travel = if at_start { sweep_r.signum() } else { -sweep_r.signum() };
                let delta = travel * distance / radius_mm;
                let angle = far_angle + delta;
                point_to_document(center + DVec2::new(angle.cos(), angle.sin()) * radius_mm)
            }
        }
    }

    /// Whether the end of this segment meets the start of `next` within the drawing joint
    /// tolerance.
    ///
    /// The bound is one document unit plus the rounding slack of either arc endpoint, because an
    /// arc stores a centre, radius and angle and its endpoint is derived (ADR 0072). A caller that
    /// decides whether a drawn chain closes uses this rather than exact equality.
    pub fn end_meets(self, next: Self) -> Result<bool, ContourSegmentError> {
        let tolerance = i128::from(joint_tolerance_micrometres(self, next));
        let first = self.end()?;
        let second = next.start()?;
        let delta = [
            i128::from(first[0].micrometres()) - i128::from(second[0].micrometres()),
            i128::from(first[1].micrometres()) - i128::from(second[1].micrometres()),
        ];
        Ok(delta[0] * delta[0] + delta[1] * delta[1] <= tolerance * tolerance)
    }

    /// Converts the segment to the geometry layer, in millimetres.
    ///
    /// This is the boundary conversion: the stored integer values are exact, and the geometry
    /// layer validates its own invariants once more.
    pub fn to_geom(self) -> Result<Segment, ContourSegmentError> {
        match self {
            Self::Line { start, end } => {
                let start = point_to_geometry(start)?;
                let end = point_to_geometry(end)?;
                Ok(Segment::Line(Line::new(start, end).map_err(ContourSegmentError::from)?))
            }
            Self::Arc { center, radius, start_angle, sweep } => {
                let center = point_to_geometry(center)?;
                let radius = radius.millimetres_f64();
                Ok(Segment::Arc(
                    Arc::new(center, radius, start_angle.radians_f64(), sweep.radians_f64())
                        .map_err(ContourSegmentError::from)?,
                ))
            }
        }
    }

    /// Converts a geometry segment back to document units.
    ///
    /// Endpoints and angles are rounded to whole micrometres and millidegrees; a segment that
    /// collapses below that resolution is rejected instead of silently becoming a zero-length line.
    pub fn from_geom(segment: Segment) -> Result<Self, ContourSegmentError> {
        match segment {
            Segment::Line(line) => Self::line(point_to_document(line.start())?, point_to_document(line.end())?),
            Segment::Arc(arc) => {
                let center = point_to_document(arc.center())?;
                let radius = length_to_document(arc.radius())?;
                let start_angle = degrees_to_document(arc.start_angle())?;
                let sweep = degrees_to_document(arc.sweep_angle())?;
                Self::arc(center, radius, start_angle, sweep)
            }
        }
    }

    /// Checks every stored value against the segment invariants.
    fn validate(&self) -> Result<(), ContourSegmentError> {
        match *self {
            Self::Line { start, end } => {
                check_point(start)?;
                check_point(end)?;
                if start == end {
                    return Err(ContourSegmentError::ZeroLength);
                }
            }
            Self::Arc { center, radius, start_angle: _, sweep } => {
                check_point(center)?;
                if !radius.is_positive() || radius.micrometres() > MAX_CONTOUR_COORDINATE.micrometres() {
                    return Err(ContourSegmentError::InvalidRadius { radius, max: MAX_CONTOUR_COORDINATE });
                }
                let magnitude = sweep.millidegrees().unsigned_abs();
                if magnitude == 0 || magnitude > FULL_TURN.millidegrees() as u64 {
                    return Err(ContourSegmentError::InvalidSweep);
                }
            }
        }
        Ok(())
    }
}

/// One element of a contour ring: a segment and the stable id it carries in that ring.
///
/// The id belongs to the ring, not to the curve. The same line or arc value can sit in two rings,
/// and the edit math builds candidate curves before they have an id, so [`ContourSegment`] stays a
/// pure curve value and the id is paired with it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContourSegmentEntry {
    id: ContourSegmentId,
    segment: ContourSegment,
}

impl ContourSegmentEntry {
    /// Pairs a segment with its id.
    pub const fn new(id: ContourSegmentId, segment: ContourSegment) -> Self {
        Self { id, segment }
    }

    /// The stable id, unique inside the ring and never reused.
    pub const fn id(self) -> ContourSegmentId {
        self.id
    }

    /// The curve.
    pub const fn segment(self) -> ContourSegment {
        self.segment
    }
}

/// A full turn in millidegrees.
const FULL_TURN: Degrees = Degrees::from_millidegrees(360_000);

/// Why a contour segment is not usable.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ContourSegmentError {
    /// A coordinate lies outside the accepted range.
    #[error("contour coordinate {point} is outside \u{00b1}{max}")]
    CoordinateOutOfRange {
        /// The supplied coordinate.
        point: Length,
        /// The accepted absolute maximum.
        max: Length,
    },
    /// A line has coincident endpoints.
    #[error("a contour line needs two distinct endpoints")]
    ZeroLength,
    /// The three points of an arc are distinct but share one line, so no circle passes through them.
    #[error("an arc needs three points that do not lie on one line")]
    DegenerateArc,
    /// An arc radius is zero, negative or too large.
    #[error("arc radius {radius} must be positive and at most {max}")]
    InvalidRadius {
        /// The rejected radius.
        radius: Length,
        /// The accepted maximum.
        max: Length,
    },
    /// An arc sweep is zero or larger than a full turn.
    #[error("an arc sweep must be non-zero and at most a full turn")]
    InvalidSweep,
    /// A coordinate or angle is not finite, or is too large for document units.
    #[error("a contour value cannot be represented in document units")]
    NonFinite,
    /// The geometry layer rejected the segment despite a document-level check.
    #[error(transparent)]
    Geometry(#[from] furnicraft_geom::ContourError),
}

/// Serialized form of [`ContourSegment`]; validation happens in its `TryFrom` implementation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ContourSegmentWire {
    Line { start: [Length; 2], end: [Length; 2] },
    Arc { center: [Length; 2], radius: Length, start_angle: Degrees, sweep: Degrees },
}

impl TryFrom<ContourSegmentWire> for ContourSegment {
    type Error = ContourSegmentError;

    fn try_from(wire: ContourSegmentWire) -> Result<Self, Self::Error> {
        match wire {
            ContourSegmentWire::Line { start, end } => Self::line(start, end),
            ContourSegmentWire::Arc { center, radius, start_angle, sweep } => {
                Self::arc(center, radius, start_angle, sweep)
            }
        }
    }
}

/// Rejects a point outside the accepted coordinate range.
fn check_point(point: [Length; 2]) -> Result<(), ContourSegmentError> {
    for value in point {
        if value.micrometres().unsigned_abs() > MAX_CONTOUR_COORDINATE.micrometres() as u64 {
            return Err(ContourSegmentError::CoordinateOutOfRange { point: value, max: MAX_CONTOUR_COORDINATE });
        }
    }
    Ok(())
}

/// Converts document micrometres to geometry millimetres.
pub(crate) fn point_to_geometry(point: [Length; 2]) -> Result<DVec2, ContourSegmentError> {
    check_point(point)?;
    Ok(DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64()))
}

/// Converts geometry millimetres to document micrometres, rounded.
pub(crate) fn point_to_document(point: DVec2) -> Result<[Length; 2], ContourSegmentError> {
    Ok([length_to_document(point.x)?, length_to_document(point.y)?])
}

/// Rounds a millimetre value to whole micrometres.
pub(crate) fn length_to_document(millimetres: f64) -> Result<Length, ContourSegmentError> {
    let micrometres = millimetres * 1_000.0;
    if !micrometres.is_finite() || micrometres.round() < i64::MIN as f64 || micrometres.round() > i64::MAX as f64 {
        return Err(ContourSegmentError::NonFinite);
    }
    let length = Length::from_micrometres(micrometres.round() as i64);
    check_point([length, Length::ZERO])?;
    Ok(length)
}

/// Rounds a radian value to whole millidegrees.
pub(crate) fn degrees_to_document(radians: f64) -> Result<Degrees, ContourSegmentError> {
    Degrees::from_radians(radians).ok_or(ContourSegmentError::NonFinite)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_line_round_trips_through_serde() {
        let segment = ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(40)])
            .expect("a valid line");
        let json = serde_json::to_string(&segment).expect("serialize");
        let back: ContourSegment = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, segment);
    }

    #[test]
    fn an_arc_round_trips_through_serde() {
        let segment = ContourSegment::arc(
            [millimetres(100), millimetres(0)],
            millimetres(100),
            Degrees::from_millidegrees(0),
            Degrees::from_millidegrees(-90_000),
        )
        .expect("a valid arc");
        let json = serde_json::to_string(&segment).expect("serialize");
        let back: ContourSegment = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, segment);
    }

    #[test]
    fn a_zero_length_line_is_rejected() {
        let point = [millimetres(10), millimetres(10)];
        assert_eq!(ContourSegment::line(point, point), Err(ContourSegmentError::ZeroLength));
    }

    #[test]
    fn a_bad_arc_radius_or_sweep_is_rejected() {
        let center = [Length::ZERO; 2];
        assert!(matches!(
            ContourSegment::arc(center, Length::ZERO, Degrees::ZERO, Degrees::from_millidegrees(90_000)),
            Err(ContourSegmentError::InvalidRadius { .. })
        ));
        assert_eq!(
            ContourSegment::arc(center, millimetres(10), Degrees::ZERO, Degrees::ZERO),
            Err(ContourSegmentError::InvalidSweep)
        );
        assert_eq!(
            ContourSegment::arc(center, millimetres(10), Degrees::ZERO, Degrees::from_millidegrees(360_001)),
            Err(ContourSegmentError::InvalidSweep)
        );
        assert!(
            ContourSegment::arc(center, millimetres(10), Degrees::ZERO, FULL_TURN).is_ok(),
            "a full turn is accepted"
        );
    }

    #[test]
    fn a_coordinate_outside_the_limit_is_rejected() {
        let far = Length::from_micrometres(MAX_CONTOUR_COORDINATE.micrometres() + 1);
        assert!(matches!(
            ContourSegment::line([Length::ZERO; 2], [far, Length::ZERO]),
            Err(ContourSegmentError::CoordinateOutOfRange { .. })
        ));
    }

    #[test]
    fn an_invalid_segment_is_rejected_on_load() {
        let json = r#"{"kind":"arc","center":[0,0],"radius":0,"start_angle":0,"sweep":90000}"#;
        assert!(serde_json::from_str::<ContourSegment>(json).is_err(), "a zero radius cannot load");
        let json = r#"{"kind":"line","start":[0,0],"end":[0,0]}"#;
        assert!(serde_json::from_str::<ContourSegment>(json).is_err(), "a zero-length line cannot load");
    }

    #[test]
    fn an_arc_endpoint_follows_its_center_and_angle() {
        let segment = ContourSegment::arc(
            [millimetres(100), millimetres(0)],
            millimetres(100),
            Degrees::from_millidegrees(0),
            Degrees::from_millidegrees(90_000),
        )
        .expect("a quarter arc");
        assert_eq!(segment.start().expect("start"), [millimetres(200), millimetres(0)]);
        assert_eq!(segment.end().expect("end"), [millimetres(100), millimetres(100)]);
    }

    #[test]
    fn geometry_conversion_round_trips() {
        let line =
            ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(120), millimetres(45)]).expect("line");
        assert_eq!(ContourSegment::from_geom(line.to_geom().expect("to geometry")).expect("back"), line);

        let arc = ContourSegment::arc(
            [millimetres(50), millimetres(-20)],
            millimetres(75),
            Degrees::from_millidegrees(30_000),
            Degrees::from_millidegrees(-120_000),
        )
        .expect("arc");
        assert_eq!(ContourSegment::from_geom(arc.to_geom().expect("to geometry")).expect("back"), arc);
    }

    use crate::fixtures::*;

    #[test]
    fn a_contour_rejects_every_shape_error_at_construction_and_load() {
        assert_eq!(
            Contour::from_polygon(vec![[millimetres(0), millimetres(0)], [millimetres(1), millimetres(0)]]),
            Err(ContourError::TooFewSegments { count: 2 })
        );
        assert_eq!(
            Contour::from_polygon(vec![
                [millimetres(0), millimetres(0)],
                [millimetres(10), millimetres(0)],
                [millimetres(10), millimetres(0)],
                [millimetres(0), millimetres(10)]
            ]),
            Err(ContourError::Segment(ContourSegmentError::ZeroLength))
        );
        assert_eq!(
            Contour::from_polygon(vec![
                [millimetres(0), millimetres(0)],
                [millimetres(10), millimetres(0)],
                [millimetres(20), millimetres(0)]
            ]),
            Err(ContourError::ZeroArea)
        );
        assert!(matches!(
            Contour::from_polygon(vec![
                [millimetres(0), millimetres(0)],
                [millimetres(30), millimetres(0)],
                [millimetres(20), millimetres(20)],
                [millimetres(10), millimetres(-10)]
            ]),
            Err(ContourError::SelfIntersecting { .. })
        ));
        let far = Length::from_micrometres(MAX_CONTOUR_COORDINATE.micrometres() + 1);
        assert_eq!(
            Contour::from_polygon(vec![
                [far, millimetres(0)],
                [millimetres(10), millimetres(0)],
                [millimetres(10), millimetres(10)]
            ]),
            Err(ContourError::Segment(ContourSegmentError::CoordinateOutOfRange {
                point: far,
                max: MAX_CONTOUR_COORDINATE
            }))
        );
        let too_many = (0..=MAX_CONTOUR_SEGMENTS)
            .map(|index| [millimetres(index as i64), millimetres((index % 2) as i64)])
            .collect::<Vec<_>>();
        assert!(matches!(Contour::from_polygon(too_many), Err(ContourError::TooManySegments { .. })));

        let too_short = r#"{"segments":[{"kind":"line","start":[0,0],"end":[10000,0]},{"kind":"line","start":[10000,0],"end":[0,0]}]}"#;
        assert!(serde_json::from_str::<Contour>(too_short).is_err(), "load runs the same validation");
        let open = r#"{"segments":[
            {"kind":"line","start":[0,0],"end":[10000,0]},
            {"kind":"line","start":[10000,0],"end":[10000,10000]},
            {"kind":"line","start":[10000,10000],"end":[0,10000]}
        ]}"#;
        assert!(serde_json::from_str::<Contour>(open).is_err(), "an open ring cannot load");
    }

    #[test]
    fn a_contour_segment_must_meet_the_next_one() {
        let segments = vec![
            ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]).expect("line"),
            ContourSegment::line([millimetres(100), millimetres(1)], [millimetres(100), millimetres(50)])
                .expect("line"),
            ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(0), millimetres(50)]).expect("line"),
            ContourSegment::line([millimetres(0), millimetres(50)], [millimetres(0), millimetres(0)]).expect("line"),
        ];
        assert_eq!(Contour::new(segments), Err(ContourError::NotClosed { index: 0 }));
    }

    #[test]
    fn a_ring_joins_its_segments_with_the_drawing_tolerance() {
        let point = |x: i64, y: i64| [Length::from_micrometres(x), Length::from_micrometres(y)];
        let square = |last: [Length; 2]| {
            vec![
                ContourSegment::line(point(0, 0), point(100_000, 0)).expect("line"),
                ContourSegment::line(point(100_000, 0), point(100_000, 50_000)).expect("line"),
                ContourSegment::line(point(100_000, 50_000), point(0, 50_000)).expect("line"),
                ContourSegment::line(point(0, 50_000), last).expect("the closing line"),
            ]
        };
        assert!(Contour::new(square(point(1, 0))).is_ok(), "one micrometre is still a joint");
        assert_eq!(Contour::new(square(point(2, 0))), Err(ContourError::NotClosed { index: 3 }));
    }

    /// An arc stores its centre, radius and angles, so its start point is derived: after rounding it
    /// can miss the drawn point by a micrometre. A ring the user drew must still assemble (ADR 0072).
    #[test]
    fn a_ring_joins_an_arc_to_the_point_it_was_drawn_from() {
        let point = |x: i64, y: i64| [Length::from_micrometres(x), Length::from_micrometres(y)];
        let drawn = point(100_000, 0);
        let arc = ContourSegment::arc_through(drawn, point(-60_000, 20_000), point(-90_000, -71_000)).expect("arc");
        assert_ne!(arc.start().expect("start"), drawn, "the stored start is derived and rounded");
        let end = arc.end().expect("end");
        let ring = Contour::new(vec![
            ContourSegment::line(point(0, 0), drawn).expect("line"),
            arc,
            ContourSegment::line(end, point(0, 0)).expect("line"),
        ])
        .expect("the one-micrometre rounding is a joint");
        ring.to_geometry().expect("the geometry layer uses the same tolerance");
    }

    /// A 600 mm arc can miss the drawn joint by about `radius × 8.73e-6` ≈ 5 µm after its centre,
    /// radius and angle are rounded; a fixed one-micrometre bound refused the ring (ADR 0072).
    #[test]
    fn a_ring_takes_the_rounding_slack_of_a_large_arc() {
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(600),
            Degrees::ZERO,
            Degrees::from_millidegrees(60_000),
        )
        .expect("a 60 degree arc of 600 mm");
        let end = arc.end().expect("a derived end");
        let ring = |gap: i64| {
            let start = [Length::from_micrometres(end[0].micrometres() + gap), end[1]];
            Contour::new(vec![
                arc,
                ContourSegment::line(start, [Length::ZERO, Length::ZERO]).expect("the chord"),
                ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(600), Length::ZERO]).expect("the base"),
            ])
        };
        let rounded = ring(5).expect("five micrometres are inside the slack of a 600 mm arc");
        rounded.to_geometry().expect("the geometry boundary accepts the same joint");
        assert_eq!(ring(7), Err(ContourError::NotClosed { index: 0 }), "past the slack it is a gap");
    }

    /// A small arc gets a small slack: 100 mm rounds to less than one micrometre, so the one-unit
    /// base is the whole bound and a 20 µm gap is still a gap.
    #[test]
    fn a_small_arc_still_refuses_a_wide_gap() {
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(100),
            Degrees::ZERO,
            Degrees::from_millidegrees(60_000),
        )
        .expect("a 60 degree arc of 100 mm");
        let end = arc.end().expect("a derived end");
        let gap = [Length::from_micrometres(end[0].micrometres() + 20), end[1]];
        let ring = Contour::new(vec![
            arc,
            ContourSegment::line(gap, [Length::ZERO, Length::ZERO]).expect("the chord"),
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("the base"),
        ]);
        assert_eq!(ring, Err(ContourError::NotClosed { index: 0 }));
    }

    #[test]
    fn a_clockwise_ring_is_stored_counter_clockwise_exactly() {
        let clockwise = Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(0), millimetres(50)],
            [millimetres(100), millimetres(50)],
            [millimetres(100), millimetres(0)],
        ])
        .expect("a clockwise rectangle is accepted");
        assert_eq!(
            clockwise.segments().next(),
            Some(ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"))
        );
        assert_eq!(clockwise.bounds_micrometres(), ([0, 0], [100_000, 50_000]));
    }

    #[test]
    fn a_fresh_ring_numbers_its_segments_from_one() {
        let contour = plate_contour();
        let ids = contour.entries().iter().map(|entry| entry.id().get()).collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 2, 3, 4]);
        assert_eq!(contour.next_segment_id().get(), 5, "the counter follows the last id");

        assert_eq!(contour.index_of(segment_id(2)), Some(1));
        assert_eq!(contour.id_at(1), Some(segment_id(2)));
        assert_eq!(contour.segment(segment_id(2)), contour.entries().get(1).map(|entry| entry.segment()));
        assert_eq!(contour.segment(segment_id(9)), None, "an id outside the ring has no segment");
        assert_eq!(contour.id_at(9), None, "a position outside the ring has no id");
    }

    #[test]
    fn segment_ids_survive_a_serde_round_trip() {
        let contour = plate_contour();
        let json = serde_json::to_string(&contour).expect("serialize");
        assert!(json.contains("\"next_segment_id\":5"), "the counter is written: {json}");
        assert!(json.contains("\"id\":1"), "every segment carries its id: {json}");
        let back: Contour = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, contour);
    }

    #[test]
    fn a_ring_with_broken_ids_is_a_readable_error() {
        let no_counter = r#"{"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":3,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":4,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let error = serde_json::from_str::<Contour>(no_counter).expect_err("ids without a counter are rejected");
        assert!(error.to_string().contains("next_segment_id"), "error was {error}");

        let repeated = r#"{"next_segment_id":5,"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":1,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":2,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":3,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let error = serde_json::from_str::<Contour>(repeated).expect_err("a repeated id is rejected");
        assert!(error.to_string().contains("appears twice"), "error was {error}");

        let stale_counter = r#"{"next_segment_id":4,"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":3,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":4,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let error =
            serde_json::from_str::<Contour>(stale_counter).expect_err("a counter that repeats an id is rejected");
        assert!(error.to_string().contains("greater than the largest stored id"), "error was {error}");

        let zero = r#"{"next_segment_id":5,"segments":[
            {"id":0,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":3,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":4,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let error = serde_json::from_str::<Contour>(zero).expect_err("id zero is rejected");
        assert!(error.to_string().contains("must be non-zero"), "error was {error}");
    }

    #[test]
    fn a_rebuilt_ring_keeps_the_ids_it_was_given_and_raises_the_counter() {
        let contour = plate_contour();
        let bottom = ContourSegmentId::new(1).expect("id 1");
        let left_half =
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(50), Length::ZERO]).expect("line");
        let right_half =
            ContourSegment::line([millimetres(50), Length::ZERO], [millimetres(100), Length::ZERO]).expect("line");

        // Split the bottom segment in two: the first half keeps id 1, the second takes the next id.
        let mut entries = Vec::with_capacity(contour.entries().len() + 1);
        entries.push(ContourSegmentEntry::new(bottom, left_half));
        entries.push(ContourSegmentEntry::new(contour.next_segment_id(), right_half));
        entries.extend(contour.entries().iter().skip(1).copied());

        let edited = contour.rebuild(entries).expect("the split ring is valid");
        assert_eq!(edited.entries().len(), 5);
        assert_eq!(edited.next_segment_id().get(), 6, "the minted id is spent");
        assert_eq!(edited.segment(bottom), Some(left_half), "the split segment keeps its id");
        assert_eq!(edited.segment(segment_id(5)), Some(right_half), "the new half carries the minted id");
        for id in 2..=4 {
            assert_eq!(
                edited.segment(segment_id(id)),
                contour.segment(segment_id(id)),
                "an untouched segment keeps its curve"
            );
        }
    }

    #[test]
    fn a_ring_rejects_a_repeated_id_at_construction() {
        let contour = plate_contour();
        let mut entries = contour.entries().to_vec();
        if let Some(entry) = entries.get_mut(2) {
            *entry = ContourSegmentEntry::new(segment_id(1), entry.segment());
        }
        assert_eq!(Contour::from_entries(entries), Err(ContourError::DuplicateSegmentId { id: segment_id(1) }));
    }

    #[test]
    fn reversing_a_ring_keeps_each_id_with_its_segment() {
        let clockwise = Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(0), millimetres(50)],
            [millimetres(100), millimetres(50)],
            [millimetres(100), millimetres(0)],
        ])
        .expect("a clockwise rectangle");
        // Id 1 was the left edge going up. The constructor turns the ring around, so the same id now
        // names that edge going down, at the end of the stored travel order.
        assert_eq!(
            clockwise.segment(segment_id(1)),
            Some(ContourSegment::line([Length::ZERO, millimetres(50)], [Length::ZERO, Length::ZERO]).expect("line"))
        );
        assert_eq!(clockwise.index_of(segment_id(1)), Some(3));
        assert_eq!(clockwise.next_segment_id().get(), 5, "reversing does not change the counter");
    }

    #[test]
    fn a_contour_with_one_corner_arc_round_trips_and_bounds_the_arc() {
        // A 100 x 50 plate whose top-right corner is a 20 mm quarter round.
        let arc = ContourSegment::arc(
            [millimetres(80), millimetres(30)],
            millimetres(20),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let contour = Contour::new(vec![
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"),
            ContourSegment::line([millimetres(100), Length::ZERO], arc.start().expect("start")).expect("line"),
            arc,
            ContourSegment::line(arc.end().expect("end"), [Length::ZERO, millimetres(50)]).expect("line"),
            ContourSegment::line([Length::ZERO, millimetres(50)], [Length::ZERO, Length::ZERO]).expect("line"),
        ])
        .expect("a rounded plate");
        assert_eq!(contour.segments().len(), 5);
        assert_eq!(contour.bounds_micrometres(), ([0, 0], [100_000, 50_000]));
        // The corner itself is gone: the round cuts it, so the area is the plate minus the corner.
        let sampled = contour.sampled_points(50.0).expect("sampled");
        assert!(sampled.len() > 5, "the arc is sampled into the polyline");

        let text = serde_json::to_string(&contour).expect("serialize");
        assert!(text.contains("\"kind\":\"arc\""), "the arc is saved as a segment");
        assert_eq!(serde_json::from_str::<Contour>(&text).expect("load"), contour);
    }

    #[test]
    fn the_whole_ring_converts_to_geometry_with_its_arc() {
        let arc = ContourSegment::arc(
            [millimetres(80), millimetres(30)],
            millimetres(20),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let contour = Contour::new(vec![
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"),
            ContourSegment::line([millimetres(100), Length::ZERO], arc.start().expect("start")).expect("line"),
            arc,
            ContourSegment::line(arc.end().expect("end"), [Length::ZERO, millimetres(50)]).expect("line"),
            ContourSegment::line([Length::ZERO, millimetres(50)], [Length::ZERO, Length::ZERO]).expect("line"),
        ])
        .expect("a rounded plate");

        let geometry = contour.to_geometry().expect("geometry ring");
        assert!(geometry.is_closed());
        assert!(geometry.is_counter_clockwise().expect("closed ring"));
        assert_eq!(geometry.segments().len(), 5);
        assert_eq!(geometry.segments().iter().filter(|segment| matches!(segment, Segment::Arc(_))).count(), 1);
        let rounded_corner = 20.0 * 20.0 - std::f64::consts::PI * 20.0 * 20.0 / 4.0;
        assert!(
            geometry.signed_area().is_some_and(|area| (area - (100.0 * 50.0 - rounded_corner)).abs() < 1.0e-9),
            "area was {:?}",
            geometry.signed_area()
        );
    }

    #[test]
    fn a_rounded_plate_encloses_the_plate_area_minus_its_corner() {
        // Unit square 100 x 100 with a quarter circle of radius 100 at the origin corner is a disc
        // quarter, so its area is pi r^2 / 4. The sagging arc is direct: start at (100, 0), sweep
        // 90 degrees counter-clockwise to (0, 100), then two lines back along the axes.
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(100),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let contour = Contour::new(vec![
            arc,
            ContourSegment::line([Length::ZERO, millimetres(100)], [Length::ZERO, Length::ZERO]).expect("line"),
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"),
        ])
        .expect("a quarter disc");
        let area = std::f64::consts::PI * 100.0 * 100.0 / 4.0;
        let sampled = contour.sampled_points(1.0).expect("sampled");
        // Shoelace on the sampled polygon stays within the chord error of the exact area.
        let mut twice = 0.0;
        for index in 0..sampled.len() {
            let current = sampled.get(index).expect("point");
            let next = sampled.get((index + 1) % sampled.len()).expect("point");
            twice += current[0].millimetres_f64() * next[1].millimetres_f64()
                - next[0].millimetres_f64() * current[1].millimetres_f64();
        }
        assert!((twice.abs() / 2.0 - area).abs() < 1.0, "got {} for {area}", twice.abs() / 2.0);
    }

    #[test]
    fn an_arc_that_crosses_a_line_is_rejected() {
        // A three-quarter arc from (100, 0) through (0, 100) and (-100, 0) to (0, -100), then a
        // vertical line back up past the arc and a line to the arc's start. Both lines cut the arc,
        // so the ring is not simple even though every joint meets.
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(100),
            Degrees::ZERO,
            Degrees::from_millidegrees(270_000),
        )
        .expect("arc");
        let contour = Contour::new(vec![
            arc,
            ContourSegment::line([Length::ZERO, millimetres(-100)], [Length::ZERO, millimetres(200)]).expect("line"),
            ContourSegment::line([Length::ZERO, millimetres(200)], [millimetres(100), Length::ZERO]).expect("line"),
        ]);
        assert_eq!(contour, Err(ContourError::SelfIntersecting { first: 0, second: 1 }));
    }

    #[test]
    fn a_line_ring_that_crosses_itself_is_still_rejected() {
        // A pentagon with a spike: the two edges of the spike cross the far edge, so the ring is not
        // simple even though every joint meets.
        let contour = Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(100), millimetres(0)],
            [millimetres(100), millimetres(100)],
            [millimetres(50), millimetres(-50)],
            [millimetres(0), millimetres(100)],
        ]);
        assert!(matches!(contour, Err(ContourError::SelfIntersecting { .. })), "got {contour:?}");
    }

    #[test]
    fn a_single_line_is_an_open_path() {
        let segment =
            ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]).expect("a line");
        let path = Path::new(vec![segment], false).expect("a one-segment path");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 1);
        assert_eq!(path.segments().len(), 1);
        assert_eq!(path.id_at(0), Some(segment_id(1)));
        assert_eq!(path.index_of(segment_id(1)), Some(0));
        assert_eq!(path.segment(segment_id(1)), Some(segment));
        assert_eq!(path.next_segment_id().get(), 2);
    }

    #[test]
    fn a_closed_path_is_anchored_at_the_centre_of_its_bounds() {
        let rectangle = Path::rectangle([millimetres(100), millimetres(50)], [millimetres(0), millimetres(0)])
            .expect("a rectangle");
        assert_eq!(rectangle.creation_origin(), Ok([millimetres(50), millimetres(25), Length::ZERO]));

        let circle = Path::circle([millimetres(100), millimetres(50)], millimetres(40)).expect("a circle");
        assert_eq!(circle.creation_origin(), Ok([millimetres(100), millimetres(50), Length::ZERO]));
    }

    #[test]
    fn an_open_path_is_anchored_at_the_start_of_its_first_stored_segment() {
        let polyline = Path::polyline(
            vec![
                [millimetres(600), millimetres(400)],
                [millimetres(600), millimetres(0)],
                [millimetres(0), millimetres(0)],
            ],
            false,
        )
        .expect("an open chain");
        assert_eq!(polyline.creation_origin(), Ok([millimetres(600), millimetres(400), Length::ZERO]));

        let arc = ContourSegment::arc(
            [millimetres(100), Length::ZERO],
            millimetres(100),
            Degrees::from_millidegrees(30_000),
            Degrees::from_millidegrees(60_000),
        )
        .expect("an arc");
        let path = Path::new(vec![arc], false).expect("a one-arc path");
        let start = arc.start().expect("the stored start");
        assert_eq!(path.creation_origin(), Ok([start[0], start[1], Length::ZERO]));
    }

    #[test]
    fn a_ring_is_anchored_at_the_centre_of_its_bounds() {
        let contour = plate_contour();
        assert_eq!(contour.creation_origin(), [millimetres(50), millimetres(25), Length::ZERO]);
    }

    #[test]
    fn a_rectangle_is_four_lines_whatever_order_its_corners_arrive_in() {
        let path = Path::rectangle([millimetres(100), millimetres(50)], [millimetres(0), millimetres(0)])
            .expect("a rectangle");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4, "one line per side");
        let starts = path.segments().map(|segment| segment.start().expect("start")).collect::<Vec<_>>();
        assert_eq!(
            starts,
            vec![
                [millimetres(0), millimetres(0)],
                [millimetres(100), millimetres(0)],
                [millimetres(100), millimetres(50)],
                [millimetres(0), millimetres(50)],
            ],
            "the path runs counter-clockwise from the minimum corner"
        );
        assert_eq!(path.bounds_micrometres(), ([0, 0], [100_000, 50_000]));
    }

    #[test]
    fn a_rectangle_without_width_or_height_is_refused() {
        assert_eq!(
            Path::rectangle([millimetres(10), millimetres(0)], [millimetres(10), millimetres(20)]),
            Err(PathError::DegenerateRectangle)
        );
        assert_eq!(
            Path::rectangle([millimetres(0), millimetres(5)], [millimetres(20), millimetres(5)]),
            Err(PathError::DegenerateRectangle)
        );
        assert_eq!(
            Path::rectangle([millimetres(10), millimetres(10)], [millimetres(10), millimetres(10)]),
            Err(PathError::DegenerateRectangle)
        );
    }

    #[test]
    fn a_circle_is_four_quarter_arcs_with_exact_joints() {
        let center = [millimetres(100), millimetres(50)];
        let path = Path::circle(center, millimetres(40)).expect("a circle");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4);
        for entry in path.entries() {
            let ContourSegment::Arc { radius, sweep, .. } = entry.segment() else {
                panic!("a circle is made of arcs");
            };
            assert_eq!(radius, millimetres(40));
            assert_eq!(sweep, Degrees::from_millidegrees(90_000));
        }
        let starts = path.segments().map(|segment| segment.start().expect("start")).collect::<Vec<_>>();
        assert_eq!(
            starts,
            vec![
                [millimetres(140), millimetres(50)],
                [millimetres(100), millimetres(90)],
                [millimetres(60), millimetres(50)],
                [millimetres(100), millimetres(10)],
            ],
            "the joints sit on the axes, so the radius is exact"
        );
        assert_eq!(path.bounds_micrometres(), ([60_000, 10_000], [140_000, 90_000]));
    }

    #[test]
    fn a_circle_needs_a_positive_radius() {
        assert!(matches!(
            Path::circle([Length::ZERO; 2], Length::ZERO),
            Err(PathError::Segment(ContourSegmentError::InvalidRadius { .. }))
        ));
        let too_large = Length::from_micrometres(MAX_CONTOUR_COORDINATE.micrometres() + 1);
        assert!(matches!(
            Path::circle([Length::ZERO; 2], too_large),
            Err(PathError::Segment(ContourSegmentError::InvalidRadius { .. }))
        ));
    }

    #[test]
    fn an_arc_through_three_points_passes_through_the_middle_one() {
        let path = Path::arc_through(
            [millimetres(0), millimetres(0)],
            [millimetres(50), millimetres(50)],
            [millimetres(100), millimetres(0)],
        )
        .expect("a half circle over the two outer points");
        assert!(!path.is_closed());
        let entry = *path.entries().first().expect("one arc");
        let ContourSegment::Arc { center, radius, sweep, .. } = entry.segment() else {
            panic!("an arc through three points is one arc");
        };
        assert_eq!(center, [millimetres(50), millimetres(0)]);
        assert_eq!(radius, millimetres(50));
        assert!(sweep.millidegrees() < 0, "the way through the middle point is clockwise");
        assert_eq!(entry.segment().start(), Ok([millimetres(0), millimetres(0)]));
        assert_eq!(entry.segment().end(), Ok([millimetres(100), millimetres(0)]));
        let middle = entry.segment().to_geom().expect("geometry").point_at(0.5).expect("midpoint");
        assert!((middle.x - 50.0).abs() < 0.01 && (middle.y - 50.0).abs() < 0.01, "got {middle:?}");
    }

    #[test]
    fn an_arc_through_three_points_turns_the_other_way_when_the_middle_point_asks() {
        let path = Path::arc_through(
            [millimetres(0), millimetres(0)],
            [millimetres(50), millimetres(-50)],
            [millimetres(100), millimetres(0)],
        )
        .expect("a half circle under the two outer points");
        let entry = *path.entries().first().expect("one arc");
        let ContourSegment::Arc { sweep, .. } = entry.segment() else {
            panic!("an arc");
        };
        assert!(sweep.millidegrees() > 0, "the way through the middle point is counter-clockwise");
    }

    #[test]
    fn an_arc_through_three_points_rejects_degenerate_input() {
        let a = [millimetres(0), millimetres(0)];
        let b = [millimetres(50), millimetres(50)];
        let c = [millimetres(100), millimetres(0)];
        assert_eq!(Path::arc_through(a, a, c), Err(PathError::DegenerateArc), "coincident start and middle");
        assert_eq!(Path::arc_through(a, b, b), Err(PathError::DegenerateArc), "coincident middle and end");
        assert_eq!(Path::arc_through(a, b, a), Err(PathError::DegenerateArc), "coincident start and end");
        assert_eq!(
            Path::arc_through(a, b, [millimetres(100), millimetres(100)]),
            Err(PathError::DegenerateArc),
            "three points on one line have no circle"
        );
    }

    #[test]
    fn a_path_bounds_include_the_arc_extremes_it_crosses() {
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(100),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let path = Path::new(vec![arc], false).expect("a one-arc path");
        assert_eq!(path.bounds_micrometres(), ([0, 0], [100_000, 100_000]));
    }

    #[test]
    fn a_single_full_turn_arc_is_a_closed_path_and_round_trips() {
        let arc = ContourSegment::arc([millimetres(100), millimetres(0)], millimetres(100), Degrees::ZERO, FULL_TURN)
            .expect("a full turn");
        let path = Path::new(vec![arc], true).expect("a circle is a closed one-segment path");
        assert!(path.is_closed());
        assert_eq!(path.segment(segment_id(1)), Some(arc), "the arc stays an arc");

        let json = serde_json::to_string(&path).expect("serialize");
        assert!(json.contains("\"closed\":true"), "the flag is written: {json}");
        assert!(json.contains("\"id\":1"), "the segment id is written: {json}");
        let back: Path = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, path);
    }

    #[test]
    fn a_polyline_builds_one_line_per_pair_and_closes_on_request() {
        let points = vec![
            [millimetres(0), millimetres(0)],
            [millimetres(600), millimetres(0)],
            [millimetres(600), millimetres(400)],
        ];
        let open = Path::polyline(points.clone(), false).expect("an open chain");
        assert!(!open.is_closed());
        assert_eq!(open.entries().len(), 2, "three points make two segments");
        assert_eq!(open.entries().last().and_then(|entry| entry.segment().end().ok()), Some(points[2]));

        let closed = Path::polyline(points.clone(), true).expect("a closed triangle");
        assert!(closed.is_closed());
        assert_eq!(closed.entries().len(), 3, "closing adds one line");
        assert_eq!(closed.entries().last().and_then(|entry| entry.segment().end().ok()), Some(points[0]));
    }

    #[test]
    fn a_polyline_needs_two_points_and_distinct_neighbours() {
        assert_eq!(Path::polyline(Vec::new(), false), Err(PathError::TooFewSegments { count: 0 }));
        assert_eq!(
            Path::polyline(vec![[millimetres(10), millimetres(10)]], false),
            Err(PathError::TooFewSegments { count: 0 })
        );
        assert_eq!(
            Path::polyline(vec![[millimetres(10), millimetres(10)]], true),
            Err(PathError::TooFewSegments { count: 0 }),
            "one point cannot even make a line"
        );
        let repeated = vec![[millimetres(10), millimetres(10)], [millimetres(10), millimetres(10)]];
        assert!(matches!(Path::polyline(repeated, false), Err(PathError::Segment(ContourSegmentError::ZeroLength))));
    }

    #[test]
    fn a_circle_through_a_rim_point_uses_its_distance_as_the_radius() {
        let path = Path::circle_through([millimetres(100), millimetres(50)], [millimetres(130), millimetres(90)])
            .expect("a 3-4-5 rim point");
        assert!(path.is_closed());
        let ContourSegment::Arc { radius, .. } = path.entries()[0].segment() else {
            panic!("a circle is made of arcs");
        };
        assert_eq!(radius, millimetres(50), "30 and 40 make a radius of 50");
        assert_eq!(path.bounds_micrometres(), ([50_000, 0], [150_000, 100_000]));
    }

    #[test]
    fn a_circle_through_a_point_rounds_the_radius_to_whole_micrometres() {
        let rim = [Length::from_micrometres(3), Length::from_micrometres(4)];
        let path = Path::circle_through([Length::ZERO; 2], rim).expect("a 3-4-5 circle");
        let ContourSegment::Arc { radius, .. } = path.entries()[0].segment() else {
            panic!("a circle is made of arcs");
        };
        assert_eq!(radius, Length::from_micrometres(5));
    }

    #[test]
    fn a_circle_through_its_own_centre_or_a_huge_rim_is_refused() {
        let center = [millimetres(100), millimetres(50)];
        assert!(matches!(
            Path::circle_through(center, center),
            Err(PathError::Segment(ContourSegmentError::InvalidRadius { .. }))
        ));
        let far = [Length::from_micrometres(MAX_CONTOUR_COORDINATE.micrometres()), Length::ZERO];
        assert!(matches!(
            Path::circle_through([Length::from_micrometres(-MAX_CONTOUR_COORDINATE.micrometres()), Length::ZERO], far),
            Err(PathError::Segment(ContourSegmentError::InvalidRadius { .. }))
        ));
    }

    #[test]
    fn a_drawn_figure_remembers_its_shape() {
        let rectangle =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(50)]).expect("rectangle");
        assert_eq!(rectangle.shape(), Some(PathShape::Rectangle));
        let circle = Path::circle([millimetres(20), millimetres(10)], millimetres(40)).expect("circle");
        assert_eq!(circle.shape(), Some(PathShape::Circle));
        let polygon = Path::regular_polygon([millimetres(0), millimetres(0)], [millimetres(50), millimetres(0)], 6)
            .expect("hexagon");
        assert_eq!(polygon.shape(), Some(PathShape::RegularPolygon));
        assert_eq!(polygon.segments().count(), 6);
        let arc = Path::arc_from_center(
            [millimetres(0), millimetres(0)],
            [millimetres(50), millimetres(0)],
            [millimetres(0), millimetres(50)],
        )
        .expect("a quarter arc");
        assert_eq!(arc.shape(), None);
        assert!(!arc.is_closed());
        let ContourSegment::Arc { radius, sweep, .. } = arc.entries()[0].segment() else {
            panic!("an arc from its centre is an arc");
        };
        assert_eq!(radius, millimetres(50));
        assert_eq!(sweep, Degrees::from_millidegrees(90_000));
        let polyline = Path::polyline(vec![[millimetres(0), millimetres(0)], [millimetres(10), millimetres(0)]], false)
            .expect("line");
        assert_eq!(polyline.shape(), None);
    }

    #[test]
    fn a_translated_path_moves_every_point_and_keeps_its_ids_shape_and_angles() {
        let rectangle =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(50)]).expect("rectangle");
        let offset = [Length::from_micrometres(600_000), Length::from_micrometres(-400_000)];

        let moved = rectangle.translated(offset).expect("a moved rectangle");

        assert_eq!(moved.shape(), Some(PathShape::Rectangle), "a moved rectangle is still a rectangle");
        assert_eq!(moved.next_segment_id(), rectangle.next_segment_id(), "the id counter travels");
        assert_eq!(moved.bounds_micrometres(), ([600_000, -400_000], [700_000, -350_000]));
        for (before, after) in rectangle.entries().iter().zip(moved.entries()) {
            assert_eq!(before.id(), after.id());
            let start = before.segment().start().expect("a start");
            assert_eq!(after.segment().start().expect("a start"), translate_point(start, offset).expect("moved"));
            let end = before.segment().end().expect("an end");
            assert_eq!(after.segment().end().expect("an end"), translate_point(end, offset).expect("moved"));
        }

        let arc = Path::arc_from_center(
            [millimetres(0), millimetres(0)],
            [millimetres(50), millimetres(0)],
            [millimetres(0), millimetres(50)],
        )
        .expect("a quarter arc");
        let moved_arc = arc.translated(offset).expect("a moved arc");
        let ContourSegment::Arc { center, radius, start_angle, sweep } = moved_arc.entries()[0].segment() else {
            panic!("an arc stays an arc");
        };
        assert_eq!(center, offset, "an arc moves by its centre");
        assert_eq!(radius, millimetres(50));
        assert_eq!(start_angle, Degrees::from_millidegrees(0));
        assert_eq!(sweep, Degrees::from_millidegrees(90_000));
    }

    #[test]
    fn a_translation_that_leaves_the_coordinate_range_is_an_error() {
        let arc = Path::arc_from_center(
            [millimetres(0), millimetres(0)],
            [millimetres(50), millimetres(0)],
            [millimetres(0), millimetres(50)],
        )
        .expect("a quarter arc");
        assert_eq!(
            arc.translated([Length::from_micrometres(i64::MAX), Length::ZERO]),
            Err(PathError::Segment(ContourSegmentError::CoordinateOutOfRange {
                point: Length::from_micrometres(i64::MAX),
                max: MAX_CONTOUR_COORDINATE,
            }))
        );
    }

    #[test]
    fn a_circle_through_three_points_is_the_circumcircle() {
        let path = Path::circle_three_points(
            [millimetres(0), millimetres(0)],
            [millimetres(100), millimetres(0)],
            [millimetres(50), millimetres(50)],
        )
        .expect("a right triangle lies on its hypotenuse circle");
        assert_eq!(path.shape(), Some(PathShape::Circle));
        assert_eq!(path.bounds_micrometres(), ([0, -50_000], [100_000, 50_000]), "centre (50, 0), radius 50");
        assert_eq!(
            Path::circle_three_points(
                [millimetres(0), millimetres(0)],
                [millimetres(50), millimetres(50)],
                [millimetres(100), millimetres(100)],
            ),
            Err(PathError::DegenerateCircle)
        );
    }

    #[test]
    fn a_rectangle_from_its_centre_mirrors_the_corner() {
        let path = Path::rectangle_from_center([millimetres(0), millimetres(0)], [millimetres(100), millimetres(50)])
            .expect("rectangle");
        assert_eq!(path.shape(), Some(PathShape::Rectangle));
        assert_eq!(path.bounds_micrometres(), ([-100_000, -50_000], [100_000, 50_000]));
        assert_eq!(
            Path::rectangle_from_center([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]),
            Err(PathError::DegenerateRectangle)
        );
        let turned = Path::rectangle_from_center_directed(
            [millimetres(0), millimetres(0)],
            [millimetres(10), millimetres(10)],
            [millimetres(100), millimetres(0)],
        )
        .expect("a square turned by 45 degrees");
        assert_eq!(turned.shape(), Some(PathShape::Rectangle));
        assert_eq!(turned.segments().count(), 4);
        let (min, max) = turned.bounds_micrometres();
        assert_eq!(min, [-100_000, -100_000]);
        assert_eq!(max, [100_000, 100_000]);
    }

    #[test]
    fn a_regular_polygon_refuses_a_side_count_outside_its_range() {
        let center = [millimetres(0), millimetres(0)];
        let rim = [millimetres(50), millimetres(0)];
        assert!(matches!(Path::regular_polygon(center, rim, 2), Err(PathError::InvalidSides { .. })));
        assert!(matches!(
            Path::regular_polygon(center, rim, MAX_POLYGON_SIDES + 1),
            Err(PathError::InvalidSides { .. })
        ));
        assert_eq!(Path::regular_polygon(center, center, 5), Err(PathError::DegeneratePolygon));
        let widest = Path::regular_polygon(center, rim, MAX_POLYGON_SIDES).expect("the largest polygon");
        assert_eq!(widest.segments().count(), MAX_POLYGON_SIDES as usize);
    }

    #[test]
    fn a_stored_shape_tag_must_fit_the_segments() {
        let rectangle =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(100), millimetres(50)]).expect("rectangle");
        let wire = serde_json::to_value(&rectangle).expect("serialize");
        let loaded: Path = serde_json::from_value(wire.clone()).expect("the tag fits");
        assert_eq!(loaded.shape(), Some(PathShape::Rectangle));

        let mut wrong_tag = wire;
        wrong_tag["shape"] = serde_json::json!("circle");
        let reason = serde_json::from_value::<Path>(wrong_tag)
            .map_err(|error| error.to_string())
            .expect_err("a rectangle tagged as a circle");
        assert!(reason.contains("tagged as a"), "the loader names the mismatch, got {reason}");

        let unknown = r#"{"segments":[{"id":1,"kind":"line","start":[0,0],"end":[10000,0]},
            {"id":2,"kind":"line","start":[10000,0],"end":[10000,10000]},
            {"id":3,"kind":"line","start":[10000,10000],"end":[0,0]}],"next_segment_id":4,"closed":true}"#;
        let untagged: Path = serde_json::from_str(unknown).expect("a path without a constraint tag loads");
        assert_eq!(untagged.shape(), None, "a drawn chain carries no constraint set");
        let bad = unknown.replace("\"closed\":true", "\"closed\":true,\"shape\":\"blob\"");
        assert!(serde_json::from_str::<Path>(&bad).is_err(), "an unknown shape is an error");
    }

    #[test]
    fn an_open_polyline_joins_its_segments_and_keeps_its_ids() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(0), millimetres(50)])
                    .expect("line"),
            ],
            false,
        )
        .expect("an open path");
        assert!(!path.is_closed());
        let ids = path.entries().iter().map(|entry| entry.id().get()).collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 2, 3], "ids follow the drawn order");
        assert_eq!(path.next_segment_id().get(), 4);
    }

    #[test]
    fn a_joint_may_miss_by_one_micrometre_and_no_more() {
        let first =
            ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]).expect("line");
        let one_micrometre = Length::from_micrometres(100_001);
        let touching =
            ContourSegment::line([one_micrometre, millimetres(0)], [millimetres(100), millimetres(50)]).expect("line");
        assert!(Path::new(vec![first, touching], false).is_ok(), "a one-micrometre gap is accepted");

        let two_micrometres = Length::from_micrometres(100_002);
        let broken =
            ContourSegment::line([two_micrometres, millimetres(0)], [millimetres(100), millimetres(50)]).expect("line");
        assert_eq!(Path::new(vec![first, broken], false), Err(PathError::NotJoined { index: 0 }));
    }

    #[test]
    fn a_closed_path_must_join_its_last_end_to_its_first_start() {
        let square = || {
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(0), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(0), millimetres(50)], [millimetres(0), millimetres(0)])
                    .expect("line"),
            ]
        };
        let path = Path::new(square(), true).expect("a closed square");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4);

        let mut open = square();
        open[3] = ContourSegment::line([millimetres(0), millimetres(50)], [Length::from_micrometres(2), Length::ZERO])
            .expect("line");
        assert_eq!(Path::new(open, true), Err(PathError::NotClosed), "two micrometres do not close");
    }

    #[test]
    fn a_path_rejects_bad_ids_and_a_broken_stored_chain() {
        assert_eq!(Path::new(Vec::new(), false), Err(PathError::TooFewSegments { count: 0 }));

        let contour = plate_contour();
        let mut repeated = contour.entries().to_vec();
        if let Some(entry) = repeated.get_mut(1) {
            *entry = ContourSegmentEntry::new(segment_id(1), entry.segment());
        }
        assert_eq!(Path::from_entries(repeated, true), Err(PathError::DuplicateSegmentId { id: segment_id(1) }));

        let stale_counter = r#"{"next_segment_id":4,"closed":false,"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":3,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":4,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let error = serde_json::from_str::<Path>(stale_counter).expect_err("a counter that repeats an id is rejected");
        assert!(error.to_string().contains("greater than the largest stored id"), "error was {error}");

        let broken_joint = r#"{"next_segment_id":3,"closed":false,"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,2000],"end":[100000,50000]}
        ]}"#;
        let error = serde_json::from_str::<Path>(broken_joint).expect_err("a broken stored joint is rejected");
        assert!(error.to_string().contains("does not meet"), "error was {error}");

        let no_ids = r#"{"next_segment_id":2,"closed":false,"segments":[
            {"kind":"line","start":[0,0],"end":[100000,0]}
        ]}"#;
        let error = serde_json::from_str::<Path>(no_ids).expect_err("a path needs ids");
        assert!(error.to_string().contains("missing field `id`"), "error was {error}");
    }

    #[test]
    fn a_rebuilt_path_keeps_its_closed_flag_and_counter() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
            ],
            false,
        )
        .expect("a path");
        let first = path.id_at(0).expect("first id");
        let extension = ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
            .expect("line");
        let extended = path
            .rebuild(vec![
                ContourSegmentEntry::new(first, path.segment(first).expect("segment")),
                ContourSegmentEntry::new(path.next_segment_id(), extension),
            ])
            .expect("a rebuilt path");
        assert!(!extended.is_closed(), "the open flag is kept");
        assert_eq!(extended.segment(first), path.segment(first), "an untouched id keeps its curve");
        assert_eq!(extended.next_segment_id().get(), 3, "the minted id is spent");
    }

    #[test]
    fn a_closed_path_becomes_a_ring_with_its_ids() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(0), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(0), millimetres(50)], [millimetres(0), millimetres(0)])
                    .expect("line"),
            ],
            true,
        )
        .expect("a closed square");
        let contour = path.to_contour().expect("a simple closed path is a ring");
        assert_eq!(contour.entries().len(), 4);
        assert_eq!(
            contour.entries().iter().map(|entry| entry.id()).collect::<Vec<_>>(),
            path.entries().iter().map(|entry| entry.id()).collect::<Vec<_>>(),
            "the drawn ids travel into the ring"
        );
        assert_eq!(contour.bounds_micrometres(), ([0, 0], [100_000, 50_000]));
    }

    #[test]
    fn a_clockwise_path_becomes_a_counter_clockwise_ring() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(0), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(0), millimetres(50)], [millimetres(100), millimetres(50)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(50)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(0), millimetres(0)])
                    .expect("line"),
            ],
            true,
        )
        .expect("a clockwise square");
        let contour = path.to_contour().expect("a clockwise ring is turned around");
        assert_eq!(
            contour.segments().next(),
            Some(ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line")),
            "the ring is stored counter-clockwise"
        );
        assert_eq!(contour.id_at(0), Some(segment_id(4)), "the reversed segment keeps its id");
    }

    #[test]
    fn a_closed_path_keeps_its_id_counter() {
        let json = r#"{"next_segment_id":9,"closed":true,"segments":[
            {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
            {"id":2,"kind":"line","start":[100000,0],"end":[100000,50000]},
            {"id":3,"kind":"line","start":[100000,50000],"end":[0,50000]},
            {"id":4,"kind":"line","start":[0,50000],"end":[0,0]}
        ]}"#;
        let path: Path = serde_json::from_str(json).expect("a stored closed path");
        let contour = path.to_contour().expect("a stored simple ring");
        assert_eq!(contour.next_segment_id().get(), 9, "the path counter survives the conversion");
    }

    #[test]
    fn an_open_path_cannot_become_a_ring() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
            ],
            false,
        )
        .expect("an open path");
        assert_eq!(path.to_contour(), Err(PathError::Open));
        let error = path.to_contour().expect_err("an open chain owns no ring");
        assert!(error.to_string().contains("open path"), "the reason is readable: {error}");
    }

    #[test]
    fn a_crossing_path_cannot_become_a_ring() {
        let corners = [
            [millimetres(0), millimetres(0)],
            [millimetres(30), millimetres(0)],
            [millimetres(20), millimetres(20)],
            [millimetres(10), millimetres(-10)],
        ];
        let segments = (0..corners.len())
            .map(|index| ContourSegment::line(corners[index], corners[(index + 1) % corners.len()]).expect("line"))
            .collect::<Vec<_>>();
        let path = Path::new(segments, true).expect("a closed self-crossing chain");
        assert!(matches!(path.to_contour(), Err(PathError::Contour(ContourError::SelfIntersecting { .. }))));
    }

    #[test]
    fn a_flat_closed_path_cannot_become_a_ring() {
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(10), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(10), millimetres(0)], [millimetres(20), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(20), millimetres(0)], [millimetres(0), millimetres(0)])
                    .expect("line"),
            ],
            true,
        )
        .expect("a closed collinear chain");
        assert_eq!(path.to_contour(), Err(PathError::Contour(ContourError::ZeroArea)));
    }
}

/// Largest number of segments in a constructive contour.
pub const MAX_CONTOUR_SEGMENTS: usize = 256;

/// Largest accepted contour coordinate and extrusion height: ten metres.
pub const MAX_CONTOUR_COORDINATE: Length = Length::from_micrometres(10_000_000);

/// Serialized form of [`Contour`]; validation happens in its `TryFrom` implementation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContourWire {
    segments: Vec<ContourSegmentEntryWire>,
    next_segment_id: ContourSegmentId,
}

/// Serialized form of one ring element: the curve and its stable id.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ContourSegmentEntryWire {
    Line { id: ContourSegmentId, start: [Length; 2], end: [Length; 2] },
    Arc { id: ContourSegmentId, center: [Length; 2], radius: Length, start_angle: Degrees, sweep: Degrees },
}

impl ContourSegmentEntryWire {
    /// The stable id the file carries.
    const fn id(self) -> ContourSegmentId {
        match self {
            Self::Line { id, .. } | Self::Arc { id, .. } => id,
        }
    }

    /// The curve, validated by the segment constructor.
    fn segment(self) -> Result<ContourSegment, ContourSegmentError> {
        match self {
            Self::Line { start, end, .. } => ContourSegment::line(start, end),
            Self::Arc { center, radius, start_angle, sweep, .. } => {
                ContourSegment::arc(center, radius, start_angle, sweep)
            }
        }
    }
}

impl From<ContourSegmentEntry> for ContourSegmentEntryWire {
    fn from(entry: ContourSegmentEntry) -> Self {
        let id = entry.id();
        match entry.segment() {
            ContourSegment::Line { start, end } => Self::Line { id, start, end },
            ContourSegment::Arc { center, radius, start_angle, sweep } => {
                Self::Arc { id, center, radius, start_angle, sweep }
            }
        }
    }
}

/// A closed flat contour in the object's local XY plane.
///
/// The ring is a list of directed lines and arcs, each carrying a stable [`ContourSegmentId`]. A
/// valid contour has at least three segments, is closed, every segment lies inside
/// [`MAX_CONTOUR_COORDINATE`], it encloses a non-zero area and it does not cross itself. The
/// constructor stores it counter-clockwise, so face winding does not depend on how the user drew it.
///
/// Each segment is converted to the geometry layer, where a line or arc is validated once, and the
/// joints, the signed area and the self-intersection test then run on that exact curve. Document
/// units stay integer: every point that leaves the module is rounded to a whole micrometre.
///
/// A joint is judged with the same rule the drawing path uses: a base of
/// [`PATH_JOINT_TOLERANCE_MICROMETRES`] plus the rounding slack of either arc endpoint. An arc
/// stores its centre, radius and angles, so its endpoints are derived and can miss the point the
/// user drew by more than a micrometre on a large radius; requiring exact equality would refuse a
/// ring the user legitimately drew (ADR 0072).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ContourWire", into = "ContourWire")]
pub struct Contour {
    entries: Vec<ContourSegmentEntry>,
    next_segment_id: ContourSegmentId,
}

/// Largest magnitude of the signed area sum, in square micrometres, before it is rejected.
///
/// Ten metres square is `2 * 10^14`; the bound only has to catch a non-finite or absurd sum.
const MAX_CONTOUR_AREA2: f64 = 1.0e18;
/// Chord tolerance of the polyline used by the self-intersection test, in micrometres.
///
/// The samples lie on the curve itself, so the tolerance only decides how finely an arc is cut; a
/// tenth of a millimetre finds a crossing far finer than any furniture part cares about.
pub(crate) const SELF_INTERSECTION_TOLERANCE_MICROMETRES: f64 = 100.0;

impl Contour {
    /// Validates a ring of segments, gives every segment a fresh id and stores it counter-clockwise.
    ///
    /// Fresh ids run `1..=n` in the order the caller supplied the segments, and the next id is
    /// `n + 1`. A clockwise ring is turned around after numbering, so its ids are not in stored
    /// travel order; they are labels, and [`Contour::index_of`] is what maps one to a position. A
    /// ring rebuilt from existing entries keeps its ids through [`Contour::rebuild`] instead.
    pub fn new(segments: Vec<ContourSegment>) -> Result<Self, ContourError> {
        let count = segments.len();
        if count < 3 {
            return Err(ContourError::TooFewSegments { count });
        }
        if count > MAX_CONTOUR_SEGMENTS {
            return Err(ContourError::TooManySegments { count, max: MAX_CONTOUR_SEGMENTS });
        }
        let mut entries = Vec::with_capacity(count);
        let mut next = 1_u32;
        for segment in segments {
            let id = ContourSegmentId::new(next).ok_or(ContourError::SegmentIdExhausted)?;
            next = next.checked_add(1).ok_or(ContourError::SegmentIdExhausted)?;
            entries.push(ContourSegmentEntry::new(id, segment));
        }
        let next_segment_id = ContourSegmentId::new(next).ok_or(ContourError::SegmentIdExhausted)?;
        Self::assemble(entries, next_segment_id)
    }

    /// Validates a ring of entries and stores it counter-clockwise.
    ///
    /// The ids must be unique. The counter is derived as one above the largest id, so a ring built
    /// from scratch never hands out an id twice; [`Contour::rebuild`] is the variant that keeps a
    /// counter across an edit.
    pub fn from_entries(entries: Vec<ContourSegmentEntry>) -> Result<Self, ContourError> {
        let next_segment_id = next_id_after_entries(&entries).ok_or(ContourError::SegmentIdExhausted)?;
        Self::assemble(entries, next_segment_id)
    }

    /// The same ring rebuilt from new entries, keeping this contour's id counter.
    ///
    /// This is the edit path: a caller changes one segment, keeps the id of every segment it did not
    /// touch, mints a new id from [`Contour::next_segment_id`] for every segment it adds, and the
    /// returned contour hands out ids above both the old counter and the largest stored id.
    pub fn rebuild(&self, entries: Vec<ContourSegmentEntry>) -> Result<Self, ContourError> {
        let after_entries = next_id_after_entries(&entries).ok_or(ContourError::SegmentIdExhausted)?;
        let next = self.next_segment_id.get().max(after_entries.get());
        let next_segment_id = ContourSegmentId::new(next).ok_or(ContourError::SegmentIdExhausted)?;
        Self::assemble(entries, next_segment_id)
    }

    /// Builds a line-only ring from polygon corner points, in travel order.
    pub fn from_polygon(points: Vec<[Length; 2]>) -> Result<Self, ContourError> {
        if points.len() < 3 {
            return Err(ContourError::TooFewSegments { count: points.len() });
        }
        if points.len() > MAX_CONTOUR_SEGMENTS {
            return Err(ContourError::TooManySegments { count: points.len(), max: MAX_CONTOUR_SEGMENTS });
        }
        let mut segments = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let (Some(start), Some(end)) = (points.get(index), points.get((index + 1) % points.len())) else {
                continue;
            };
            segments.push(ContourSegment::line(*start, *end)?);
        }
        Self::new(segments)
    }

    /// Validates a ring of entries and stores it with the given next id.
    ///
    /// The counter must be larger than every stored id. Validation order matters: the count, the ids
    /// and the counter are checked before the ring is sampled or its area is summed.
    fn assemble(entries: Vec<ContourSegmentEntry>, next_segment_id: ContourSegmentId) -> Result<Self, ContourError> {
        let count = entries.len();
        if count < 3 {
            return Err(ContourError::TooFewSegments { count });
        }
        if count > MAX_CONTOUR_SEGMENTS {
            return Err(ContourError::TooManySegments { count, max: MAX_CONTOUR_SEGMENTS });
        }
        let mut seen = HashSet::with_capacity(count);
        let mut max_id = 0_u32;
        for entry in &entries {
            if !seen.insert(entry.id()) {
                return Err(ContourError::DuplicateSegmentId { id: entry.id() });
            }
            max_id = max_id.max(entry.id().get());
        }
        if next_segment_id.get() <= max_id {
            return Err(ContourError::NextSegmentIdNotAfterMax { next: next_segment_id, max: max_id });
        }
        for index in 0..count {
            let (Some(entry), Some(next)) = (entries.get(index), entries.get((index + 1) % count)) else {
                continue;
            };
            if !joints_meet(entry.segment(), next.segment())? {
                return Err(ContourError::NotClosed { index });
            }
        }
        let mut contour = Self { entries, next_segment_id };
        let area2 = contour.signed_area2()?;
        if area2 == 0.0 {
            return Err(ContourError::ZeroArea);
        }
        // The crossing test runs before the ring is turned counter-clockwise, so the reported
        // indices name the segments the caller supplied.
        if let Some((first, second)) = contour.first_self_intersection()? {
            return Err(ContourError::SelfIntersecting { first, second });
        }
        if area2 < 0.0 {
            contour = contour.reversed()?;
        }
        Ok(contour)
    }

    /// The ring elements with their ids, in travel order.
    pub fn entries(&self) -> &[ContourSegmentEntry] {
        &self.entries
    }

    /// The segments in travel order.
    ///
    /// A segment is a copy of the stored curve; ask [`Contour::entries`] when the id matters.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = ContourSegment> + '_ {
        self.entries.iter().map(|entry| entry.segment())
    }

    /// Converts the whole ring to the geometry layer, in millimetres.
    ///
    /// Every segment keeps its curve, so an arc stays an arc until the geometry boundary samples
    /// it. The ids stay with the document ring; the geometry layer only needs the shape.
    pub fn to_geometry(&self) -> Result<furnicraft_geom::Contour, ContourError> {
        let segments = self
            .entries
            .iter()
            .map(|entry| entry.segment().to_geom())
            .collect::<Result<Vec<_>, ContourSegmentError>>()?;
        Ok(furnicraft_geom::Contour::new_with_joint_tolerance(segments, true, |first, next| {
            joint_tolerance_mm(*first, *next)
        })?)
    }

    /// The segment with this id.
    pub fn segment(&self, id: ContourSegmentId) -> Option<ContourSegment> {
        self.entries.iter().find(|entry| entry.id() == id).map(|entry| entry.segment())
    }

    /// The position of a segment id in travel order.
    pub fn index_of(&self, id: ContourSegmentId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id() == id)
    }

    /// The id of the segment at this position in travel order.
    pub fn id_at(&self, index: usize) -> Option<ContourSegmentId> {
        self.entries.get(index).map(|entry| entry.id())
    }

    /// The next id an edit may hand out; ids are never reused.
    pub const fn next_segment_id(&self) -> ContourSegmentId {
        self.next_segment_id
    }

    /// The same ring moved by a whole-micrometre offset in its own plane.
    ///
    /// Ids travel with their segments, so a panel ring anchored at a new zero corner keeps every
    /// band record meaningful. The offset is applied with [`i64::checked_add`] so a hostile value
    /// is an error rather than a wrapped coordinate.
    pub fn translated(&self, offset: [Length; 2]) -> Result<Self, ContourError> {
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let segment = match entry.segment() {
                ContourSegment::Line { start, end } => {
                    ContourSegment::line(translate_point(start, offset)?, translate_point(end, offset)?)?
                }
                ContourSegment::Arc { center, radius, start_angle, sweep } => {
                    ContourSegment::arc(translate_point(center, offset)?, radius, start_angle, sweep)?
                }
            };
            entries.push(ContourSegmentEntry::new(entry.id(), segment));
        }
        Self::assemble(entries, self.next_segment_id)
    }

    /// Whether every segment is a straight line, so the ring is a plain polygon.
    ///
    /// A line-only ring can be written back as corner points without losing anything; a ring with
    /// an arc cannot, because its corners do not describe the curve.
    pub fn is_line_only(&self) -> bool {
        self.entries.iter().all(|entry| !entry.segment().is_arc())
    }

    /// Samples the contour into a closed polyline for a polygon-only kernel profile.
    ///
    /// The points are rounded to whole micrometres, so the polyline stays in document units. An arc
    /// contributes as many interior points as `tolerance_micrometres` needs: a chord deviates from
    /// its arc by less than the tolerance. The first point is not repeated at the end.
    pub fn sampled_points(&self, tolerance_micrometres: f64) -> Result<Vec<[Length; 2]>, ContourError> {
        if !tolerance_micrometres.is_finite() || tolerance_micrometres <= 0.0 {
            return Err(ContourError::InvalidTolerance);
        }
        let mut points: Vec<[Length; 2]> = Vec::with_capacity(self.entries.len().saturating_mul(2));
        for entry in &self.entries {
            let segment = entry.segment();
            points.push(segment.start()?);
            let geometry = segment.to_geom()?;
            let mut steps = 1_usize;
            if let ContourSegment::Arc { radius, sweep, .. } = segment {
                steps = arc_steps(radius, sweep, tolerance_micrometres)?;
            }
            for step in 1..steps {
                let fraction = step as f64 / steps as f64;
                let point = geometry.point_at(fraction).ok_or(ContourError::InvalidTolerance)?;
                let document = point_to_document(point)?;
                if points.last() != Some(&document) {
                    points.push(document);
                }
            }
            if points.len() > MAX_SAMPLED_CONTOUR_POINTS {
                return Err(ContourError::SamplingTooDetailed { max: MAX_SAMPLED_CONTOUR_POINTS });
            }
        }
        if points.len() < 3 {
            return Err(ContourError::TooFewSegments { count: points.len() });
        }
        Ok(points)
    }

    /// The two axis-aligned extents of the contour, in micrometres.
    ///
    /// An arc contributes the axis extremes it crosses, not only its endpoints.
    pub fn bounds_micrometres(&self) -> ([i64; 2], [i64; 2]) {
        entries_bounds_micrometres(&self.entries)
    }

    /// The origin a newly created extrusion takes: the centre of the ring's local bounds.
    ///
    /// The ring is flat, so the origin lies on the base plane (`z = 0`). This runs once at creation;
    /// a later contour replacement never moves the stored origin (ADR 0088).
    pub fn creation_origin(&self) -> [Length; 3] {
        let (min, max) = self.bounds_micrometres();
        bounds_center_origin(min, max)
    }

    /// Twice the signed area of the ring, in square micrometres; positive for counter-clockwise.
    ///
    /// A line-only ring is summed exactly in integers, so a sliver far from the origin neither
    /// loses its sign to rounding nor reads as zero. A ring with an arc sums its exact arc
    /// integrals in floating point, which is where the geometry boundary already is.
    pub(crate) fn signed_area2(&self) -> Result<f64, ContourError> {
        if self.is_line_only() {
            let mut total = 0_i128;
            for entry in &self.entries {
                let ContourSegment::Line { start, end } = entry.segment() else {
                    continue;
                };
                let start = [i128::from(start[0].micrometres()), i128::from(start[1].micrometres())];
                let end = [i128::from(end[0].micrometres()), i128::from(end[1].micrometres())];
                total += start[0] * end[1] - end[0] * start[1];
            }
            let area2 = total as f64;
            if !area2.is_finite() {
                return Err(ContourError::AreaOutOfRange);
            }
            return Ok(area2);
        }
        let mut total = 0.0;
        for entry in &self.entries {
            total += match entry.segment().to_geom()? {
                furnicraft_geom::Segment::Line(line) => line.start().perp_dot(line.end()),
                furnicraft_geom::Segment::Arc(arc) => arc_area2(arc),
            };
        }
        let area2 = total * 1_000_000.0;
        if !area2.is_finite() || area2.abs() > MAX_CONTOUR_AREA2 {
            return Err(ContourError::AreaOutOfRange);
        }
        Ok(area2)
    }

    /// The ring with the opposite direction of travel.
    ///
    /// Every id travels with its segment, so a ring turned counter-clockwise by the constructor
    /// keeps the ids the caller supplied.
    fn reversed(self) -> Result<Self, ContourError> {
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in self.entries.into_iter().rev() {
            entries.push(ContourSegmentEntry::new(entry.id(), entry.segment().reversed()?));
        }
        Ok(Self { entries, next_segment_id: self.next_segment_id })
    }

    /// The first pair of non-adjacent segments that touch, as segment indices.
    ///
    /// Every segment is sampled into chords on the curve itself, so a crossing inside two arcs, or
    /// between an arc and a line, is found as soon as the samples are close enough. Two adjacent
    /// segments share their joint by construction; that one chord pair is skipped, while any other
    /// touch between two neighbours still counts. Segment bounds skip pairs that cannot meet, and a
    /// chord-pair budget bounds the work a hostile ring can ask for.
    fn first_self_intersection(&self) -> Result<Option<(usize, usize)>, ContourError> {
        let mut polylines: Vec<(SegBounds, Vec<[i64; 2]>)> = Vec::with_capacity(self.entries.len());
        let mut total = 0_usize;
        for entry in &self.entries {
            let points = sampled_segment(entry.segment(), SELF_INTERSECTION_TOLERANCE_MICROMETRES)?;
            total = total.saturating_add(points.len());
            if total > MAX_SELF_INTERSECTION_CHORDS {
                return Err(ContourError::SamplingTooDetailed { max: MAX_SELF_INTERSECTION_CHORDS });
            }
            polylines.push((SegBounds::of(&points), points));
        }
        let count = polylines.len();
        let mut budget = MAX_SELF_INTERSECTION_PAIRS;
        for first in 0..count {
            let Some((first_bounds, first_points)) = polylines.get(first) else {
                continue;
            };
            for second in (first + 1)..count {
                let Some((second_bounds, second_points)) = polylines.get(second) else {
                    continue;
                };
                if !first_bounds.overlaps(*second_bounds) {
                    continue;
                }
                let first_chords = first_points.len().saturating_sub(1);
                let second_chords = second_points.len().saturating_sub(1);
                let needed = first_chords.saturating_mul(second_chords);
                if needed > budget {
                    return Err(ContourError::SamplingTooDetailed { max: MAX_SELF_INTERSECTION_PAIRS });
                }
                budget -= needed;
                let shared = shared_joint(count, first, second, first_chords, second_chords);
                for first_chord in 0..first_chords {
                    for second_chord in 0..second_chords {
                        if shared == Some((first_chord, second_chord)) {
                            continue;
                        }
                        let (Some(a), Some(b)) = (first_points.get(first_chord), first_points.get(first_chord + 1))
                        else {
                            continue;
                        };
                        let (Some(c), Some(d)) = (second_points.get(second_chord), second_points.get(second_chord + 1))
                        else {
                            continue;
                        };
                        if segments_touch(*a, *b, *c, *d) {
                            return Ok(Some((first, second)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

/// The two axis-aligned extents of a chain of segments, in micrometres.
///
/// An arc contributes the axis extremes it crosses, not only its endpoints. Both a ring and an
/// open path use the same rule, so their drawing extents never disagree.
fn entries_bounds_micrometres(entries: &[ContourSegmentEntry]) -> ([i64; 2], [i64; 2]) {
    let mut min = [i64::MAX; 2];
    let mut max = [i64::MIN; 2];
    let mut include = |point: [Length; 2]| {
        for (axis, value) in point.iter().enumerate() {
            let value = value.micrometres();
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    };
    for entry in entries {
        let segment = entry.segment();
        if let (Ok(start), Ok(end)) = (segment.start(), segment.end()) {
            include(start);
            include(end);
        }
        if let ContourSegment::Arc { start_angle, sweep, .. } = segment
            && let Ok(geometry) = segment.to_geom()
        {
            for turn in 0..4_i64 {
                let quarter = Degrees::from_millidegrees(turn.saturating_mul(90_000));
                if !arc_reaches(start_angle, sweep, quarter) {
                    continue;
                }
                if let Some(point) = geometry.point_at(arc_fraction(start_angle, sweep, quarter))
                    && let Ok(point) = point_to_document(point)
                {
                    include(point);
                }
            }
        }
    }
    if min[0] > max[0] {
        return ([0; 2], [0; 2]);
    }
    (min, max)
}

/// The centre of a pair of extents, in micrometres, as an origin on the drawing plane.
///
/// The midpoint is summed in `i128`, so two extents near the ends of the coordinate range cannot
/// overflow; the result always lies between them and therefore fits back into `i64`.
fn bounds_center_origin(min: [i64; 2], max: [i64; 2]) -> [Length; 3] {
    fn middle(low: i64, high: i64) -> Length {
        let average = (i128::from(low) + i128::from(high)) / 2;
        Length::from_micrometres(i64::try_from(average).unwrap_or(low))
    }
    [middle(min[0], max[0]), middle(min[1], max[1]), Length::ZERO]
}

/// A path is one chain of at most this many segments, the ring size it has to fit.
const MAX_PATH_SEGMENTS: usize = MAX_CONTOUR_SEGMENTS;

/// Smallest side count of a regular polygon the drawing tools accept.
pub const MIN_POLYGON_SIDES: u32 = 3;

/// Largest side count of a regular polygon the drawing tools accept.
///
/// A polygon is a ring of lines, so it can never exceed the ring segment cap; the cap is what the
/// limit expresses as a user-visible number.
pub const MAX_POLYGON_SIDES: u32 = 128;

/// Base joint tolerance between two consecutive path segments, in micrometres.
///
/// A drawn arc endpoint is exact only in floating point, so after rounding to whole micrometres it
/// can differ from the next stored endpoint by one unit; an arc adds the radius-dependent rounding
/// slack of [`arc_slack_micrometres`] on top. This is the same rule the document ring uses when it
/// assembles and when it converts to geometry (ADR 0072).
const PATH_JOINT_TOLERANCE_MICROMETRES: i64 = 1;

/// An ordered chain of directed lines and arcs in a local XY plane.
///
/// Unlike [`Contour`], a path may be open and need not be simple: it is what the user draws and
/// edits before the shape becomes a ring. It has at least one segment and at most
/// [`MAX_CONTOUR_SEGMENTS`], because a path is what a contour ring is made from. Every segment
/// carries a stable [`ContourSegmentId`]; consecutive segments meet within the drawing joint
/// tolerance — a micrometre plus the rounding slack of either arc endpoint (ADR 0072) — and a
/// closed path also joins its last end to its first start. One path is one chain: it has no
/// sub-paths.
///
/// A path is stored as drawn — direction and orientation are kept — and it is not validated for
/// self-intersection or enclosed area. Those are ring rules, and they run when the path is turned
/// into a contour.
///
/// A closed path may also remember the figure it was drawn as ([`PathShape`]). The tag stores no
/// dimensions: the centre, the radius and the side count are read from the segments, and the tag
/// only decides which constraint set the edit grips apply (ADR 0073). A structural edit clears it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PathWire", into = "PathWire")]
pub struct Path {
    entries: Vec<ContourSegmentEntry>,
    next_segment_id: ContourSegmentId,
    closed: bool,
    shape: Option<PathShape>,
}

/// The regular figure a closed path was drawn as.
///
/// The tag is edit intent, not geometry: every dimension is derived from the segments, so a path
/// cannot contradict itself by storing a radius that its arcs do not have. A tag that does not fit
/// the stored segments is a load error (`PathError::ShapeMismatch`) rather than a silent downgrade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathShape {
    /// Four axis-aligned lines, the figure [`Path::rectangle`] builds.
    Rectangle,
    /// A full turn of arcs that share one centre and one radius.
    Circle,
    /// Three or more equal lines closing a regular polygon.
    RegularPolygon,
}

impl PathShape {
    /// Whether the segments of `path` are the figure this tag names.
    ///
    /// The check is structural and cheap: it runs once on load, so a hand-edited file cannot claim a
    /// rectangle that is a pentagon. It does not re-derive every angle; the edit operations keep
    /// the figure and this guard only has to reject a tag that belongs to different geometry.
    pub fn fits(self, path: &Path) -> bool {
        if !path.closed {
            return false;
        }
        match self {
            Self::Rectangle => {
                if path.entries.len() != 4 || path.entries.iter().any(|entry| entry.segment().is_arc()) {
                    return false;
                }
                // Verify right angles: the two axis directions must be perpendicular within a
                // micrometre of the diagonal length. Compute from the first and the fourth corner
                // against the first corner.
                let entries = path.entries();
                let Some(&first_entry) = entries.first() else {
                    return false;
                };
                let Ok(first) = first_entry.segment().to_geom() else {
                    return false;
                };
                let Ok(second) = entries.get(1).map_or(Err(()), |e| e.segment().to_geom().map_err(|_| ())) else {
                    return false;
                };
                let Ok(fourth) = entries.get(3).map_or(Err(()), |e| e.segment().to_geom().map_err(|_| ())) else {
                    return false;
                };
                let u = second.start() - first.start();
                let v = fourth.start() - first.start();
                let dot = u.dot(v).abs();
                // The dot product should be zero for a rectangle. Allow one micrometre of the
                // diagonal length, which is `(u.length() + v.length()) * 0.001 * 0.5` ≈ half a
                // part in a thousand — generous for rounding but tight enough for a rectangle.
                let bound = (u.length() + v.length()) * 0.5e-3;
                dot <= bound
            }
            Self::Circle => {
                let mut center = None;
                let mut radius = None;
                let mut sweep = 0_i64;
                for entry in &path.entries {
                    let ContourSegment::Arc { center: arc_center, radius: arc_radius, sweep: arc_sweep, .. } =
                        entry.segment()
                    else {
                        return false;
                    };
                    if center.is_some_and(|stored| stored != arc_center)
                        || radius.is_some_and(|stored| stored != arc_radius)
                    {
                        return false;
                    }
                    center = Some(arc_center);
                    radius = Some(arc_radius);
                    let Some(total) = sweep.checked_add(arc_sweep.millidegrees().abs()) else {
                        return false;
                    };
                    sweep = total;
                }
                sweep == FULL_TURN.millidegrees()
            }
            Self::RegularPolygon => {
                let count = path.entries.len();
                if !(3..=MAX_CONTOUR_SEGMENTS).contains(&count) {
                    return false;
                }
                let mut first = None;
                for entry in &path.entries {
                    let Ok(segment) = entry.segment().to_geom() else {
                        return false;
                    };
                    if matches!(segment, Segment::Arc(_)) {
                        return false;
                    }
                    let length = segment.length();
                    match first {
                        None => first = Some(length),
                        Some(expected) if (length - expected).abs() > LENGTH_TOLERANCE_MM => return false,
                        Some(_) => {}
                    }
                }
                true
            }
        }
    }
}

/// How far two side lengths of a regular polygon may differ, in millimetres.
///
/// The sides are built from rounded vertices, so they differ by the rounding of one document unit.
const LENGTH_TOLERANCE_MM: f64 = 1.0e-3;

impl Path {
    /// Validates a chain of segments, gives every segment a fresh id and stores it open or closed.
    ///
    /// Fresh ids run `1..=n` in travel order and the next id is `n + 1`; a closed path keeps its
    /// drawn direction. A closed chain must also close its last joint.
    pub fn new(segments: Vec<ContourSegment>, closed: bool) -> Result<Self, PathError> {
        let count = segments.len();
        if count == 0 {
            return Err(PathError::TooFewSegments { count });
        }
        if count > MAX_PATH_SEGMENTS {
            return Err(PathError::TooManySegments { count, max: MAX_PATH_SEGMENTS });
        }
        let mut entries = Vec::with_capacity(count);
        let mut next = 1_u32;
        for segment in segments {
            let id = ContourSegmentId::new(next).ok_or(PathError::SegmentIdExhausted)?;
            next = next.checked_add(1).ok_or(PathError::SegmentIdExhausted)?;
            entries.push(ContourSegmentEntry::new(id, segment));
        }
        let next_segment_id = ContourSegmentId::new(next).ok_or(PathError::SegmentIdExhausted)?;
        Self::assemble(entries, next_segment_id, closed)
    }

    /// Validates a chain of entries with their ids and stores it open or closed.
    ///
    /// The ids must be unique and the counter is derived as one above the largest id, so a path
    /// built from scratch never hands out an id twice; [`Path::rebuild`] keeps a counter across an
    /// edit.
    pub fn from_entries(entries: Vec<ContourSegmentEntry>, closed: bool) -> Result<Self, PathError> {
        let next_segment_id = next_id_after_entries(&entries).ok_or(PathError::SegmentIdExhausted)?;
        Self::assemble(entries, next_segment_id, closed)
    }

    /// The same path rebuilt from new entries, keeping this path's counter and open/closed flag.
    ///
    /// This is the edit path: a caller changes one segment, keeps the id of every segment it did not
    /// touch, and mints a new id from [`Path::next_segment_id`] for every segment it adds. The shape
    /// tag survives, because reshaping a figure keeps it the same figure; a caller that changes the
    /// structure instead calls [`Path::rebuild_topology`].
    pub fn rebuild(&self, entries: Vec<ContourSegmentEntry>) -> Result<Self, PathError> {
        self.rebuild_of(entries, self.closed, self.shape)
    }

    /// The same path rebuilt from new entries with a new open or closed flag.
    ///
    /// [`Path::rebuild`] keeps the flag; closing a chain is the one edit that flips it, and it still
    /// has to keep the counter that [`Path::rebuild`] keeps. Closing adds a segment, so the shape tag
    /// is dropped: a figure that was drawn open is not the regular shape it may resemble.
    pub(crate) fn rebuild_as(&self, entries: Vec<ContourSegmentEntry>, closed: bool) -> Result<Self, PathError> {
        self.rebuild_of(entries, closed, None)
    }

    /// The same path rebuilt from new entries after a change of structure.
    ///
    /// Splitting or removing a segment can turn a rectangle into a pentagon, so the shape tag is
    /// dropped rather than left to describe a figure that no longer exists.
    pub(crate) fn rebuild_topology(&self, entries: Vec<ContourSegmentEntry>) -> Result<Self, PathError> {
        self.rebuild_of(entries, self.closed, None)
    }

    /// Rebuilds the entries, the flag and the tag, keeping the id counter.
    fn rebuild_of(
        &self,
        entries: Vec<ContourSegmentEntry>,
        closed: bool,
        shape: Option<PathShape>,
    ) -> Result<Self, PathError> {
        let after_entries = next_id_after_entries(&entries).ok_or(PathError::SegmentIdExhausted)?;
        let next = self.next_segment_id.get().max(after_entries.get());
        let next_segment_id = ContourSegmentId::new(next).ok_or(PathError::SegmentIdExhausted)?;
        let path = Self::assemble(entries, next_segment_id, closed)?;
        Ok(path.with_shape(shape))
    }

    /// The figure this path was drawn as, when the grips should keep its own constraints.
    pub const fn shape(&self) -> Option<PathShape> {
        self.shape
    }

    /// The same path carrying a shape tag.
    pub fn with_shape(mut self, shape: Option<PathShape>) -> Self {
        self.shape = shape;
        self
    }

    /// A closed rectangle whose sides follow the axes, from two opposite corners.
    ///
    /// The corners may be given in any order; the path runs counter-clockwise from the corner with
    /// the smaller `x` and `y`. Two corners that share an `x` or a `y` enclose no rectangle and are
    /// refused.
    pub fn rectangle(first: [Length; 2], second: [Length; 2]) -> Result<Self, PathError> {
        if first[0] == second[0] || first[1] == second[1] {
            return Err(PathError::DegenerateRectangle);
        }
        let x = [first[0].min(second[0]), first[0].max(second[0])];
        let y = [first[1].min(second[1]), first[1].max(second[1])];
        let corners = [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]];
        let mut segments = Vec::with_capacity(4);
        for index in 0..4 {
            let (Some(&start), Some(&end)) = (corners.get(index), corners.get((index + 1) % 4)) else {
                return Err(PathError::DegenerateRectangle);
            };
            segments.push(ContourSegment::line(start, end)?);
        }
        Self::new(segments, true).map(|path| path.with_shape(Some(PathShape::Rectangle)))
    }

    /// A closed circle as four quarter arcs.
    ///
    /// Every joint sits on an axis, so a stored endpoint is the centre plus or minus the radius with
    /// no rounding to absorb. A radius that is not positive is refused by the arc constructor.
    pub fn circle(center: [Length; 2], radius: Length) -> Result<Self, PathError> {
        let mut segments = Vec::with_capacity(4);
        for quarter in 0..4_i64 {
            let start = Degrees::from_millidegrees(quarter.saturating_mul(90_000));
            segments.push(ContourSegment::arc(center, radius, start, Degrees::from_millidegrees(90_000))?);
        }
        Self::new(segments, true).map(|path| path.with_shape(Some(PathShape::Circle)))
    }

    /// A closed circle through a point away from its centre.
    ///
    /// A pointer gives a point rather than a radius, so the distance from `center` to `rim` is
    /// rounded to whole micrometres before the four quarter arcs are built. A rim point on the
    /// centre or a radius beyond the document range is refused.
    pub fn circle_through(center: [Length; 2], rim: [Length; 2]) -> Result<Self, PathError> {
        check_point(center)?;
        check_point(rim)?;
        let dx = (i128::from(rim[0].micrometres()) - i128::from(center[0].micrometres())) as f64;
        let dy = (i128::from(rim[1].micrometres()) - i128::from(center[1].micrometres())) as f64;
        let radius = dx.hypot(dy);
        if !radius.is_finite() || radius > i64::MAX as f64 {
            return Err(PathError::Segment(ContourSegmentError::NonFinite));
        }
        let radius = Length::from_micrometres(radius.round() as i64);
        Self::circle(center, radius)
    }

    /// The closed circle that passes through three points.
    ///
    /// The centre is the intersection of the perpendicular bisectors, so the three points must be
    /// distinct and not collinear. Centre and radius are rounded to whole micrometres before the
    /// four quarter arcs are built, so the stored circle can miss a click by a micrometre.
    pub fn circle_three_points(first: [Length; 2], second: [Length; 2], third: [Length; 2]) -> Result<Self, PathError> {
        check_point(first)?;
        check_point(second)?;
        check_point(third)?;
        let a = point_to_geometry(first)?;
        let b = point_to_geometry(second)?;
        let c = point_to_geometry(third)?;
        let determinant = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        let span = (b - a).length_squared().max((c - a).length_squared());
        if !determinant.is_finite() || determinant.abs() <= 1.0e-9 * span {
            return Err(PathError::DegenerateCircle);
        }
        let a2 = a.length_squared();
        let b2 = b.length_squared();
        let c2 = c.length_squared();
        let center = DVec2::new(
            (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / determinant,
            (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / determinant,
        );
        let radius = (a - center).length();
        if !radius.is_finite() || radius <= 0.0 {
            return Err(PathError::DegenerateCircle);
        }
        let center = point_to_document(center)?;
        let radius = length_to_document(radius)?;
        Self::circle(center, radius)
    }

    /// One open arc from a centre, a start point on the circle and a direction for its end.
    ///
    /// The radius is the distance from the centre to `start`; the end point only supplies the
    /// angle, so a click that misses the circle still ends the arc where the user pointed. The arc
    /// runs counter-clockwise, like a dial, and a zero or full turn is refused.
    pub fn arc_from_center(center: [Length; 2], start: [Length; 2], end: [Length; 2]) -> Result<Self, PathError> {
        check_point(center)?;
        check_point(start)?;
        check_point(end)?;
        let c = point_to_geometry(center)?;
        let s = point_to_geometry(start)?;
        let e = point_to_geometry(end)?;
        let radius = (s - c).length();
        if !radius.is_finite() || radius <= 0.0 {
            return Err(PathError::DegenerateArc);
        }
        let start_angle = (s.y - c.y).atan2(s.x - c.x);
        let end_angle = (e.y - c.y).atan2(e.x - c.x);
        let sweep = (end_angle - start_angle).rem_euclid(TAU);
        if !sweep.is_finite() || sweep <= 0.0 {
            return Err(PathError::DegenerateArc);
        }
        let segment = ContourSegment::arc(
            center,
            length_to_document(radius)?,
            degrees_to_document(start_angle)?,
            degrees_to_document(sweep)?,
        )
        .map_err(|error| match error {
            ContourSegmentError::DegenerateArc => PathError::DegenerateArc,
            other => PathError::Segment(other),
        })?;
        Self::new(vec![segment], false)
    }

    /// A closed rectangle centred on `center`, with `corner` as one of its corners.
    ///
    /// The opposite corner mirrors `corner` through the centre, so the rectangle has the axes of the
    /// document and twice the offset as its sides. A corner on either axis through the centre
    /// encloses no area and is refused.
    pub fn rectangle_from_center(center: [Length; 2], corner: [Length; 2]) -> Result<Self, PathError> {
        check_point(center)?;
        check_point(corner)?;
        let dx = i128::from(corner[0].micrometres()) - i128::from(center[0].micrometres());
        let dy = i128::from(corner[1].micrometres()) - i128::from(center[1].micrometres());
        if dx == 0 || dy == 0 {
            return Err(PathError::DegenerateRectangle);
        }
        let low = [mirrored(center[0], dx)?, mirrored(center[1], dy)?];
        let high = [mirrored(center[0], -dx)?, mirrored(center[1], -dy)?];
        Self::rectangle(low, high)
    }

    /// A closed rectangle centred on `center`, its sides turned by a direction point, through `corner`.
    ///
    /// One axis of the rectangle follows `center → direction` and the other is square to it; `corner`
    /// is the far corner, so its offset from the centre gives the half extents along both axes. The
    /// direction must differ from the centre and the corner must be off both axes.
    pub fn rectangle_from_center_directed(
        center: [Length; 2],
        direction: [Length; 2],
        corner: [Length; 2],
    ) -> Result<Self, PathError> {
        check_point(center)?;
        check_point(direction)?;
        check_point(corner)?;
        let c = point_to_geometry(center)?;
        let axis = point_to_geometry(direction)? - c;
        let reach = axis.length();
        if !reach.is_finite() || reach <= 0.0 {
            return Err(PathError::DegenerateRectangle);
        }
        let u = axis / reach;
        let v = DVec2::new(-u.y, u.x);
        let offset = point_to_geometry(corner)? - c;
        let half_x = offset.dot(u);
        let half_y = offset.dot(v);
        if !half_x.is_finite()
            || !half_y.is_finite()
            || half_x.abs() < LENGTH_TOLERANCE_MM
            || half_y.abs() < LENGTH_TOLERANCE_MM
        {
            return Err(PathError::DegenerateRectangle);
        }
        let points = [
            c - u * half_x - v * half_y,
            c + u * half_x - v * half_y,
            c + u * half_x + v * half_y,
            c - u * half_x + v * half_y,
        ];
        let mut corners = Vec::with_capacity(4);
        for point in points {
            corners.push(point_to_document(point)?);
        }
        let mut segments = Vec::with_capacity(4);
        for index in 0..4 {
            let (Some(&start), Some(&end)) = (corners.get(index), corners.get((index + 1) % 4)) else {
                return Err(PathError::DegenerateRectangle);
            };
            if start == end {
                return Err(PathError::DegenerateRectangle);
            }
            segments.push(ContourSegment::line(start, end)?);
        }
        Self::new(segments, true).map(|path| path.with_shape(Some(PathShape::Rectangle)))
    }

    /// A closed regular polygon from its centre, a rim point and a side count.
    ///
    /// The rim point gives both the radius and the phase: the first vertex sits on it and the rest
    /// follow counter-clockwise at `360 / sides` degrees. Between three and [`MAX_POLYGON_SIDES`]
    /// sides are accepted; the vertices are rounded, so the sides can differ by one micrometre.
    pub fn regular_polygon(center: [Length; 2], rim: [Length; 2], sides: u32) -> Result<Self, PathError> {
        check_point(center)?;
        check_point(rim)?;
        if !(MIN_POLYGON_SIDES..=MAX_POLYGON_SIDES).contains(&sides) {
            return Err(PathError::InvalidSides { sides, min: MIN_POLYGON_SIDES, max: MAX_POLYGON_SIDES });
        }
        let c = point_to_geometry(center)?;
        let offset = point_to_geometry(rim)? - c;
        let radius = offset.length();
        if !radius.is_finite() || radius <= 0.0 {
            return Err(PathError::DegeneratePolygon);
        }
        let first = offset.y.atan2(offset.x);
        let step = TAU / f64::from(sides);
        let mut corners = Vec::with_capacity(sides as usize);
        for index in 0..sides {
            let angle = first + step * f64::from(index);
            corners.push(point_to_document(c + DVec2::new(angle.cos(), angle.sin()) * radius)?);
        }
        let mut segments = Vec::with_capacity(corners.len());
        for index in 0..corners.len() {
            let (Some(&start), Some(&end)) = (corners.get(index), corners.get((index + 1) % corners.len())) else {
                return Err(PathError::DegeneratePolygon);
            };
            if start == end {
                return Err(PathError::DegeneratePolygon);
            }
            segments.push(ContourSegment::line(start, end)?);
        }
        Self::new(segments, true).map(|path| path.with_shape(Some(PathShape::RegularPolygon)))
    }

    /// One open or closed chain through `points`, one line per pair.
    ///
    /// Adjacent equal points are refused because they would make a zero-length segment, and a
    /// closed chain gets one more line from the last point back to the first. The chain is not
    /// checked for self-intersection or enclosed area: those are ring rules.
    pub fn polyline(points: Vec<[Length; 2]>, closed: bool) -> Result<Self, PathError> {
        if points.len() < 2 {
            return Err(PathError::TooFewSegments { count: 0 });
        }
        let segment_count = if closed { points.len() } else { points.len() - 1 };
        if segment_count > MAX_PATH_SEGMENTS {
            return Err(PathError::TooManySegments { count: segment_count, max: MAX_PATH_SEGMENTS });
        }
        let mut segments = Vec::with_capacity(segment_count);
        for pair in points.windows(2) {
            let (Some(&start), Some(&end)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            segments.push(ContourSegment::line(start, end)?);
        }
        if closed && let (Some(&first), Some(&last)) = (points.first(), points.last()) {
            segments.push(ContourSegment::line(last, first)?);
        }
        Self::new(segments, closed)
    }

    /// One open arc from `start` to `end` that passes through `through`.
    ///
    /// The arc is the way round from `start` to `end` that contains the middle point, so its sweep
    /// is negative when that way is clockwise. The three points must be distinct and must not lie
    /// on one line; a stored centre, radius and angle are whole document units, so a stored endpoint
    /// can miss its input point by a micrometre or two.
    pub fn arc_through(start: [Length; 2], through: [Length; 2], end: [Length; 2]) -> Result<Self, PathError> {
        let segment = ContourSegment::arc_through(start, through, end).map_err(|error| match error {
            ContourSegmentError::DegenerateArc => PathError::DegenerateArc,
            other => PathError::Segment(other),
        })?;
        Self::new(vec![segment], false)
    }

    /// Validates a chain of entries, its counter and its joints, then stores it.
    fn assemble(
        entries: Vec<ContourSegmentEntry>,
        next_segment_id: ContourSegmentId,
        closed: bool,
    ) -> Result<Self, PathError> {
        let count = entries.len();
        if count == 0 {
            return Err(PathError::TooFewSegments { count });
        }
        if count > MAX_PATH_SEGMENTS {
            return Err(PathError::TooManySegments { count, max: MAX_PATH_SEGMENTS });
        }
        let mut seen = HashSet::with_capacity(count);
        let mut max_id = 0_u32;
        for entry in &entries {
            if !seen.insert(entry.id()) {
                return Err(PathError::DuplicateSegmentId { id: entry.id() });
            }
            max_id = max_id.max(entry.id().get());
        }
        if next_segment_id.get() <= max_id {
            return Err(PathError::NextSegmentIdNotAfterMax { next: next_segment_id, max: max_id });
        }
        for index in 0..count {
            let (Some(entry), Some(next)) = (entries.get(index), entries.get(index + 1)) else {
                continue;
            };
            if !joints_meet(entry.segment(), next.segment())? {
                return Err(PathError::NotJoined { index });
            }
        }
        if closed {
            let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
                return Err(PathError::TooFewSegments { count });
            };
            if !joints_meet(last.segment(), first.segment())? {
                return Err(PathError::NotClosed);
            }
        }
        Ok(Self { entries, next_segment_id, closed, shape: None })
    }

    /// The chain elements with their ids, in travel order.
    pub fn entries(&self) -> &[ContourSegmentEntry] {
        &self.entries
    }

    /// The segments in travel order.
    ///
    /// A segment is a copy of the stored curve; ask [`Path::entries`] when the id matters.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = ContourSegment> + '_ {
        self.entries.iter().map(|entry| entry.segment())
    }

    /// Whether the last segment is joined to the first one.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// The two axis-aligned extents of the chain, in micrometres.
    ///
    /// An arc contributes the axis extremes it crosses, not only its endpoints. A path encloses no
    /// area by itself, so these are the extents of the drawing in the local XY plane.
    pub fn bounds_micrometres(&self) -> ([i64; 2], [i64; 2]) {
        entries_bounds_micrometres(&self.entries)
    }

    /// The same chain moved by a whole-micrometre offset in its own plane.
    ///
    /// This is the flat half of re-anchoring a created drawing: ids, the open/closed flag and the
    /// shape tag travel with the segments, because a moved rectangle is still a rectangle, and both
    /// ends of a line move while an arc moves by its centre with its radius and angles unchanged. It
    /// mirrors [`Contour::translated`], so one creation offset re-anchors either kind. The offset is
    /// applied with [`i64::checked_add`], so a hostile value is an error rather than a wrapped
    /// coordinate.
    pub fn translated(&self, offset: [Length; 2]) -> Result<Self, PathError> {
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let segment = match entry.segment() {
                ContourSegment::Line { start, end } => {
                    ContourSegment::line(translate_point(start, offset)?, translate_point(end, offset)?)?
                }
                ContourSegment::Arc { center, radius, start_angle, sweep } => {
                    ContourSegment::arc(translate_point(center, offset)?, radius, start_angle, sweep)?
                }
            };
            entries.push(ContourSegmentEntry::new(entry.id(), segment));
        }
        self.rebuild(entries)
    }

    /// The origin a newly created drawing takes: the centre of a closed chain, or its first point.
    ///
    /// A closed chain is anchored at the middle of its bounds. An open chain is anchored at the
    /// start of its first stored segment, so a line or an arc starts where the user drew it; the
    /// stored start is the rounded curve start, never the raw pointer point. The chain is flat,
    /// so the origin lies on `z = 0`. This runs once at creation; a later path replacement never
    /// moves the stored origin (ADR 0088).
    pub fn creation_origin(&self) -> Result<[Length; 3], PathError> {
        if self.is_closed() {
            let (min, max) = self.bounds_micrometres();
            return Ok(bounds_center_origin(min, max));
        }
        let first = self.entries.first().ok_or(PathError::TooFewSegments { count: 0 })?;
        let start = first.segment().start()?;
        Ok([start[0], start[1], Length::ZERO])
    }

    /// The segment with this id.
    pub fn segment(&self, id: ContourSegmentId) -> Option<ContourSegment> {
        self.entries.iter().find(|entry| entry.id() == id).map(|entry| entry.segment())
    }

    /// The position of a segment id in travel order.
    pub fn index_of(&self, id: ContourSegmentId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id() == id)
    }

    /// The id of the segment at this position in travel order.
    pub fn id_at(&self, index: usize) -> Option<ContourSegmentId> {
        self.entries.get(index).map(|entry| entry.id())
    }

    /// The next id an edit may hand out; ids are never reused.
    pub const fn next_segment_id(&self) -> ContourSegmentId {
        self.next_segment_id
    }

    /// The ring this path encloses, when it is a ring the document accepts.
    ///
    /// The segments and their ids travel into the ring, so anchors the user drew and any
    /// per-segment banding stay meaningful; the path's id counter is kept as well. The ring is
    /// stored counter-clockwise, so a path drawn clockwise comes back reversed with its ids
    /// intact. An open chain is refused before the ring rules run, and a chain that crosses itself
    /// or encloses no area is refused with the reason: those are ring rules, and a panel or an
    /// extrusion only takes a ring.
    pub fn to_contour(&self) -> Result<Contour, PathError> {
        if !self.closed {
            return Err(PathError::Open);
        }
        Contour::assemble(self.entries.clone(), self.next_segment_id).map_err(PathError::Contour)
    }
}

impl From<Contour> for Path {
    /// The same ring as a closed path, with its segment ids and its id counter.
    ///
    /// A ring is a path that already satisfies every path rule, so this conversion loses nothing
    /// but the contour's ring-only guarantees: it carries no shape tag, because a ring is not a
    /// figure the drawing tools recognised.
    fn from(contour: Contour) -> Self {
        Self { entries: contour.entries, next_segment_id: contour.next_segment_id, closed: true, shape: None }
    }
}

/// Whether the end of `current` meets the start of `next` within the joint tolerance.
fn joints_meet(current: ContourSegment, next: ContourSegment) -> Result<bool, ContourSegmentError> {
    current.end_meets(next)
}

/// The joint tolerance between two consecutive stored segments, in micrometres.
///
/// The one document unit of [`PATH_JOINT_TOLERANCE_MICROMETRES`] covers the rounding of the two
/// stored points; an arc adds [`arc_slack_micrometres`] because its endpoint is derived from a
/// rounded centre, radius and angle (ADR 0072).
fn joint_tolerance_micrometres(first: ContourSegment, next: ContourSegment) -> i64 {
    let base = PATH_JOINT_TOLERANCE_MICROMETRES;
    let slack = |segment: ContourSegment| match segment {
        ContourSegment::Line { .. } => 0,
        ContourSegment::Arc { radius, .. } => arc_slack_micrometres(i128::from(radius.micrometres())),
    };
    base.saturating_add(slack(first)).saturating_add(slack(next))
}

/// The rounding slack of an arc endpoint of this radius, in micrometres.
///
/// An arc stores whole millidegrees, so its derived endpoint can move by
/// `radius × 8.73e-6` — half a millidegree in radians — plus the radius and centre rounding the
/// base bound already covers. `873 / 100_000_000` is that factor as an exact decimal. The result
/// is rounded down, and the one-unit base then covers the remainder, so the document never accepts
/// a joint the geometry boundary would refuse.
fn arc_slack_micrometres(radius_micrometres: i128) -> i64 {
    let product = radius_micrometres.saturating_mul(873);
    let slack = product / 100_000_000;
    i64::try_from(slack).unwrap_or(i64::MAX)
}

/// The joint tolerance between two geometry segments converted from stored ones, in millimetres.
///
/// This is the same rule [`Contour::assemble`] and [`Path::assemble`] use, for a caller that hands
/// a chain to [`furnicraft_geom::Contour::new_with_joint_tolerance`]: one document unit plus the
/// rounding slack of either arc endpoint. A line contributes nothing, because its endpoints are
/// stored exactly (ADR 0072).
pub fn joint_tolerance_mm(first: Segment, second: Segment) -> f64 {
    let slack = |segment: Segment| match segment {
        Segment::Line(_) => 0,
        Segment::Arc(arc) => {
            let radius = arc.radius() * 1_000.0;
            if radius.is_finite() && radius > 0.0 {
                arc_slack_micrometres(i128::from(radius.round() as i64))
            } else {
                0
            }
        }
    };
    let micrometres = 1_i64.saturating_add(slack(first)).saturating_add(slack(second));
    micrometres as f64 * 1.0e-3
}

/// `center - offset` in whole micrometres, refusing a value that does not fit.
fn mirrored(center: Length, offset: i128) -> Result<Length, PathError> {
    let value = i128::from(center.micrometres()) - offset;
    let value = i64::try_from(value).map_err(|_| PathError::Segment(ContourSegmentError::NonFinite))?;
    Ok(Length::from_micrometres(value))
}

/// Why a path is not usable.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PathError {
    /// A segment is not a valid line or arc.
    #[error(transparent)]
    Segment(#[from] ContourSegmentError),
    /// A path has no segments.
    #[error("a path needs at least one segment, got {count}")]
    TooFewSegments {
        /// The supplied segment count.
        count: usize,
    },
    /// A path has more segments than one contour ring can hold.
    #[error("a path has {count} segments, the limit is {max}")]
    TooManySegments {
        /// The supplied segment count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// The end of a segment does not meet the start of the next one.
    #[error("path segment {index} does not meet the next segment")]
    NotJoined {
        /// Index of the segment whose end is checked.
        index: usize,
    },
    /// The last segment does not meet the first one, so the path cannot be closed.
    #[error("the last path segment does not meet the first one")]
    NotClosed,
    /// Two path elements carry the same id.
    #[error("path segment id {id} appears twice in one path")]
    DuplicateSegmentId {
        /// The repeated id.
        id: ContourSegmentId,
    },
    /// The next-segment counter would hand out an id that the path already holds.
    #[error("the next path segment id {next} must be greater than the largest stored id {max}")]
    NextSegmentIdNotAfterMax {
        /// The stored counter.
        next: ContourSegmentId,
        /// The largest id in the path.
        max: u32,
    },
    /// The id space of one path is used up.
    #[error("the path segment id space is exhausted")]
    SegmentIdExhausted,
    /// A stored path carries a segment without an id.
    #[error("a path must carry an id for every segment and a next segment id")]
    AmbiguousSegmentIds,
    /// Two rectangle corners share an axis, so the shape encloses no area.
    #[error("a rectangle needs two corners that differ on both axes")]
    DegenerateRectangle,
    /// The three points of an arc coincide or lie on one line.
    #[error("an arc through three points needs three distinct, non-collinear points")]
    DegenerateArc,
    /// The three points of a circle coincide or lie on one line.
    #[error("a circle through three points needs three distinct, non-collinear points")]
    DegenerateCircle,
    /// The centre and the rim point of a polygon coincide, or a vertex collapses.
    #[error("a regular polygon needs a rim point away from its centre")]
    DegeneratePolygon,
    /// A polygon asks for a side count the document cannot hold.
    #[error("a regular polygon needs between {min} and {max} sides, got {sides}")]
    InvalidSides {
        /// The requested side count.
        sides: u32,
        /// The smallest accepted count.
        min: u32,
        /// The largest accepted count.
        max: u32,
    },
    /// The shape tag of a stored path does not describe its segments.
    #[error("the path is tagged as a {shape:?} but its segments are not that figure")]
    ShapeMismatch {
        /// The tag the file carried.
        shape: PathShape,
    },
    /// The path is open, so it does not enclose a ring.
    #[error("an open path encloses no ring; close it before it becomes a contour")]
    Open,
    /// The closed chain is not a valid ring.
    #[error(transparent)]
    Contour(#[from] ContourError),
}

/// Serialized form of [`Path`]; validation happens in its `TryFrom` implementation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathWire {
    segments: Vec<ContourSegmentEntryWire>,
    next_segment_id: ContourSegmentId,
    closed: bool,
    #[serde(default)]
    shape: Option<PathShape>,
}

impl TryFrom<PathWire> for Path {
    type Error = PathError;

    fn try_from(wire: PathWire) -> Result<Self, Self::Error> {
        let mut entries = Vec::with_capacity(wire.segments.len());
        for segment in wire.segments {
            entries.push(ContourSegmentEntry::new(segment.id(), segment.segment()?));
        }
        let path = Self::assemble(entries, wire.next_segment_id, wire.closed)?;
        if let Some(shape) = wire.shape
            && !shape.fits(&path)
        {
            return Err(PathError::ShapeMismatch { shape });
        }
        Ok(path.with_shape(wire.shape))
    }
}

impl From<Path> for PathWire {
    fn from(path: Path) -> Self {
        Self {
            segments: path.entries.iter().map(|entry| ContourSegmentEntryWire::from(*entry)).collect(),
            next_segment_id: path.next_segment_id,
            closed: path.closed,
            shape: path.shape,
        }
    }
}

impl TryFrom<ContourWire> for Contour {
    type Error = ContourError;

    fn try_from(wire: ContourWire) -> Result<Self, Self::Error> {
        let mut entries = Vec::with_capacity(wire.segments.len());
        for segment in wire.segments {
            entries.push(ContourSegmentEntry::new(segment.id(), segment.segment()?));
        }
        Self::assemble(entries, wire.next_segment_id)
    }
}

impl From<Contour> for ContourWire {
    fn from(contour: Contour) -> Self {
        Self {
            segments: contour.entries.iter().map(|entry| ContourSegmentEntryWire::from(*entry)).collect(),
            next_segment_id: contour.next_segment_id,
        }
    }
}

/// The id just above the largest stored id, so a fresh ring never reuses one.
fn next_id_after_entries(entries: &[ContourSegmentEntry]) -> Option<ContourSegmentId> {
    let max = entries.iter().map(|entry| entry.id().get()).max().unwrap_or(0);
    let next = max.checked_add(1)?;
    ContourSegmentId::new(next)
}

/// Moves one contour point by a whole-micrometre offset.
///
/// Range checking is left to the segment constructor the moved point enters; this only refuses a
/// sum that does not fit an `i64`.
fn translate_point(point: [Length; 2], offset: [Length; 2]) -> Result<[Length; 2], ContourError> {
    let move_axis = |value: Length, delta: Length| {
        value
            .micrometres()
            .checked_add(delta.micrometres())
            .map(Length::from_micrometres)
            .ok_or(ContourSegmentError::NonFinite)
    };
    Ok([move_axis(point[0], offset[0])?, move_axis(point[1], offset[1])?])
}

/// Twice the signed area of one arc, in square millimetres; it follows from the sector integral.
fn arc_area2(arc: furnicraft_geom::Arc) -> f64 {
    let start = arc.start_angle();
    let end = start + arc.sweep_angle();
    let center = arc.center();
    let radius = arc.radius();
    radius * (center.x * (end.sin() - start.sin()) - center.y * (end.cos() - start.cos()))
        + radius * radius * arc.sweep_angle()
}

/// One segment as chords on its curve, both endpoints included.
///
/// The interior samples are rounded together with the endpoints, so every chord pair still shares
/// the exact joint of two adjacent segments.
fn sampled_segment(segment: ContourSegment, tolerance_micrometres: f64) -> Result<Vec<[i64; 2]>, ContourError> {
    let geometry = segment.to_geom()?;
    let steps = match segment {
        ContourSegment::Arc { radius, sweep, .. } => arc_steps(radius, sweep, tolerance_micrometres)?,
        ContourSegment::Line { .. } => 1,
    };
    let mut points = Vec::with_capacity(steps.saturating_add(1));
    for step in 0..=steps {
        let fraction = step as f64 / steps as f64;
        let point = geometry.point_at(fraction).ok_or(ContourError::InvalidTolerance)?;
        let document = point_to_document(point)?;
        let micrometres = [document[0].micrometres(), document[1].micrometres()];
        if points.last() != Some(&micrometres) {
            points.push(micrometres);
        }
    }
    Ok(points)
}

/// The chord pair that is the shared joint of two adjacent segments, if they are adjacent.
///
/// The pair is `(last chord of the first segment, first chord of the second)`; for the closing
/// joint it is `(first chord of segment 0, last chord of the last segment)`.
fn shared_joint(
    count: usize,
    first: usize,
    second: usize,
    first_chords: usize,
    second_chords: usize,
) -> Option<(usize, usize)> {
    if second == first.saturating_add(1) {
        Some((first_chords.checked_sub(1)?, 0))
    } else if first == 0 && second.saturating_add(1) == count {
        Some((0, second_chords.checked_sub(1)?))
    } else {
        None
    }
}

/// The micrometre bounds of one sampled segment, used to skip pairs that cannot meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SegBounds {
    min: [i64; 2],
    max: [i64; 2],
}

impl SegBounds {
    /// The bounds around a non-empty sample list.
    pub(crate) fn of(points: &[[i64; 2]]) -> Self {
        let mut bounds = Self { min: [i64::MAX; 2], max: [i64::MIN; 2] };
        for point in points {
            for (axis, value) in point.iter().enumerate() {
                bounds.min[axis] = bounds.min[axis].min(*value);
                bounds.max[axis] = bounds.max[axis].max(*value);
            }
        }
        bounds
    }

    /// Whether two closed bounds share at least one point.
    pub(crate) fn overlaps(self, other: Self) -> bool {
        self.min[0] <= other.max[0]
            && other.min[0] <= self.max[0]
            && self.min[1] <= other.max[1]
            && other.min[1] <= self.max[1]
    }
}

/// Whether a sweep from `start` reaches `angle` in its direction of travel.
fn arc_reaches(start: Degrees, sweep: Degrees, angle: Degrees) -> bool {
    let full = 360_000_i64;
    let magnitude = i64::try_from(sweep.millidegrees().unsigned_abs()).unwrap_or(full).min(full);
    if magnitude >= full {
        return true;
    }
    let mut delta = (angle.millidegrees().wrapping_sub(start.millidegrees()) % full + full) % full;
    if sweep.millidegrees() < 0 {
        delta = full - delta;
    }
    delta <= magnitude
}

/// The parameter of `angle` along an arc from `start` with `sweep`, clamped into `0..=1`.
fn arc_fraction(start: Degrees, sweep: Degrees, angle: Degrees) -> f64 {
    let full = 360_000_f64;
    let mut delta = (angle.millidegrees() as f64 - start.millidegrees() as f64).rem_euclid(full);
    if sweep.millidegrees() < 0 {
        delta = full - delta;
    }
    let magnitude = sweep.millidegrees().unsigned_abs() as f64;
    if magnitude <= 0.0 {
        return 0.0;
    }
    (delta / magnitude).clamp(0.0, 1.0)
}

/// Interior chord count of one arc so that every chord stays within `tolerance` of the curve.
fn arc_steps(radius: Length, sweep: Degrees, tolerance_micrometres: f64) -> Result<usize, ContourError> {
    let arc_radius = radius.micrometres() as f64;
    if arc_radius <= 0.0 {
        return Err(ContourError::InvalidTolerance);
    }
    let tolerance = tolerance_micrometres.clamp(1.0, arc_radius);
    let step = (2.0 * (1.0 - tolerance / arc_radius).acos()).clamp(1.0e-6, std::f64::consts::FRAC_PI_2);
    let sweep = (sweep.millidegrees().unsigned_abs() as f64 / 1_000.0).to_radians();
    let count = (sweep / step).ceil();
    if !count.is_finite() || count > MAX_CONTOUR_SAMPLES_PER_ARC as f64 {
        return Err(ContourError::SamplingTooDetailed { max: MAX_CONTOUR_SAMPLES_PER_ARC });
    }
    Ok((count as usize).max(1))
}

/// Upper bound on the interior points sampled from one arc.
const MAX_CONTOUR_SAMPLES_PER_ARC: usize = 4_096;
/// Upper bound on the points sampled from one contour.
const MAX_SAMPLED_CONTOUR_POINTS: usize = 65_536;
/// Upper bound on the chords the self-intersection test may build for one contour.
const MAX_SELF_INTERSECTION_CHORDS: usize = 65_536;
/// Upper bound on the chord pairs the self-intersection test may compare for one contour.
pub(crate) const MAX_SELF_INTERSECTION_PAIRS: usize = 16_000_000;

fn orientation(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    (i128::from(b[0]) - i128::from(a[0])) * (i128::from(c[1]) - i128::from(a[1]))
        - (i128::from(b[1]) - i128::from(a[1])) * (i128::from(c[0]) - i128::from(a[0]))
}

fn on_segment(a: [i64; 2], b: [i64; 2], point: [i64; 2]) -> bool {
    point[0] >= a[0].min(b[0]) && point[0] <= a[0].max(b[0]) && point[1] >= a[1].min(b[1]) && point[1] <= a[1].max(b[1])
}

/// Whether two closed chords share any point, including a collinear overlap.
pub(crate) fn segments_touch(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    let abc = orientation(a, b, c);
    let abd = orientation(a, b, d);
    let cda = orientation(c, d, a);
    let cdb = orientation(c, d, b);
    if ((abc > 0 && abd < 0) || (abc < 0 && abd > 0)) && ((cda > 0 && cdb < 0) || (cda < 0 && cdb > 0)) {
        return true;
    }
    (abc == 0 && on_segment(a, b, c))
        || (abd == 0 && on_segment(a, b, d))
        || (cda == 0 && on_segment(c, d, a))
        || (cdb == 0 && on_segment(c, d, b))
}

/// A validation failure of a constructive contour.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ContourError {
    /// A segment is not a valid line or arc.
    #[error(transparent)]
    Segment(#[from] ContourSegmentError),
    /// The geometry layer rejected the whole ring despite valid segments.
    #[error(transparent)]
    Geometry(#[from] furnicraft_geom::ContourError),
    /// The contour has fewer than three segments.
    #[error("a contour needs at least three segments, got {count}")]
    TooFewSegments {
        /// The supplied segment count.
        count: usize,
    },
    /// The contour has more segments than the limit allows.
    #[error("a contour has {count} segments, the limit is {max}")]
    TooManySegments {
        /// The supplied segment count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// The end of a segment does not meet the start of the next one, beyond the joint tolerance.
    #[error("contour segment {index} does not meet the next segment")]
    NotClosed {
        /// Index of the segment whose end is checked.
        index: usize,
    },
    /// The contour encloses no area.
    #[error("the contour has zero area")]
    ZeroArea,
    /// The signed area of the contour is too large to represent.
    #[error("the contour encloses too much area")]
    AreaOutOfRange,
    /// Sampling the contour would need more points than the limit allows.
    #[error("sampling the contour would need more than {max} points")]
    SamplingTooDetailed {
        /// The supported maximum.
        max: usize,
    },
    /// Two non-adjacent edges touch.
    #[error("contour edges {first} and {second} cross")]
    SelfIntersecting {
        /// The first edge index.
        first: usize,
        /// The second edge index.
        second: usize,
    },
    /// The sampling tolerance is not positive and finite.
    #[error("a sampling tolerance must be positive and finite")]
    InvalidTolerance,
    /// Two ring elements carry the same id.
    #[error("contour segment id {id} appears twice in one ring")]
    DuplicateSegmentId {
        /// The repeated id.
        id: ContourSegmentId,
    },
    /// The next-segment counter would hand out an id that the ring already holds.
    #[error("the next contour segment id {next} must be greater than the largest stored id {max}")]
    NextSegmentIdNotAfterMax {
        /// The stored counter.
        next: ContourSegmentId,
        /// The largest id in the ring.
        max: u32,
    },
    /// The id space of one contour is used up.
    #[error("the contour segment id space is exhausted")]
    SegmentIdExhausted,
    /// A stored ring mixes ids and segments without them, or carries ids without their counter.
    #[error("a contour ring must carry an id for every segment and a next segment id, or neither")]
    AmbiguousSegmentIds,
    /// A stored contour names both a segment ring and a legacy point list.
    #[error("a contour cannot carry both a segment ring and a point list")]
    AmbiguousWire,
    /// A stored contour names neither a segment ring nor a legacy point list.
    #[error("a contour needs a segment ring")]
    MissingRing,
}
