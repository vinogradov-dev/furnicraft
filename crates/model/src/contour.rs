//! Contour segments as the document stores them.
//!
//! A contour is a closed ring of directed lines and arcs in a local XY plane. The document keeps
//! micrometres and millidegrees; this module is the only place where a segment becomes
//! floating-point millimetres, and the only place where it comes back. Callers never see a
//! `furnicraft_geom` type, so the geometry layer stays free to change.
//!
//! Validation is total: every coordinate is inside [`MAX_CONTOUR_COORDINATE`], a line has two
//! distinct endpoints, an arc has a positive radius and a non-zero sweep no larger than a full
//! turn. Values that arrive from a file go through the same constructors, so a hostile scene is an
//! error the user can read rather than a broken ring.

use furnicraft_geom::{Arc, DVec2, Line, Segment};
use furnicraft_units::{Degrees, Length};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::MAX_CONTOUR_COORDINATE;

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

    /// The start point in the contour plane.
    pub fn start(self) -> Result<[Length; 2], ContourSegmentError> {
        point_to_document(self.to_geom()?.start())
    }

    /// The end point in the contour plane.
    pub fn end(self) -> Result<[Length; 2], ContourSegmentError> {
        point_to_document(self.to_geom()?.end())
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
fn point_to_geometry(point: [Length; 2]) -> Result<DVec2, ContourSegmentError> {
    check_point(point)?;
    Ok(DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64()))
}

/// Converts geometry millimetres to document micrometres.
fn point_to_document(point: DVec2) -> Result<[Length; 2], ContourSegmentError> {
    Ok([length_to_document(point.x)?, length_to_document(point.y)?])
}

/// Rounds a millimetre value to whole micrometres.
fn length_to_document(millimetres: f64) -> Result<Length, ContourSegmentError> {
    let micrometres = millimetres * 1_000.0;
    if !micrometres.is_finite() || micrometres.round() < i64::MIN as f64 || micrometres.round() > i64::MAX as f64 {
        return Err(ContourSegmentError::NonFinite);
    }
    let length = Length::from_micrometres(micrometres.round() as i64);
    check_point([length, Length::ZERO])?;
    Ok(length)
}

/// Rounds a radian value to whole millidegrees.
fn degrees_to_document(radians: f64) -> Result<Degrees, ContourSegmentError> {
    Degrees::from_radians(radians).ok_or(ContourSegmentError::NonFinite)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits")
    }

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
}
