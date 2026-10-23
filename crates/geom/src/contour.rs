//! Planar line and arc contours and their offsets.
//!
//! This is the two-dimensional half of the panel-contour model. A contour is a directed chain of
//! lines and circular arcs. The offset operation moves a tangent-continuous path to the left of its
//! travel direction by a signed distance; for a counter-clockwise closed contour, positive is the
//! material side.
//!
//! Coordinates are millimetres. The module does not model a filled region, a boolean operation or
//! self-intersection; callers build a simple contour and offset paths that are known to be tangent
//! continuous. Closed contours must not self-intersect.

use std::f64::consts::{PI, TAU};
use std::fmt;

use crate::DVec2;

/// Smallest accepted length, radius or sagitta magnitude, in millimetres.
const EPSILON: f64 = 1.0e-9;
/// Maximum gap between the end of one segment and the start of the next, in millimetres.
const JOIN_TOLERANCE: f64 = 1.0e-6;
/// Default angular tolerance for a smooth joint, in radians.
pub const DEFAULT_TANGENT_TOLERANCE: f64 = 1.0e-6;
/// Upper bound on the number of segments in one contour.
const MAX_SEGMENTS: usize = 4096;
/// Upper bound on samples generated for one arc.
const MAX_ARC_STEPS: usize = 4096;
/// Upper bound on points returned by [`Contour::sample`].
const MAX_SAMPLE_POINTS: usize = 1_000_000;

/// Why a contour or one of its segments is not usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContourError {
    /// A point, angle or distance is not finite.
    InvalidInput(&'static str),
    /// A line has coincident endpoints.
    ZeroLength,
    /// A bow has a zero sagitta; use a line instead.
    ZeroSagitta,
    /// A bow sagitta is greater than half its chord, so the arc is ambiguous.
    SagittaTooDeep,
    /// An arc radius is zero or negative.
    InvalidRadius,
    /// An arc sweep is zero or larger than a full turn.
    InvalidSweep,
    /// A contour has no segments.
    Empty,
    /// A contour has more than [`MAX_SEGMENTS`] segments.
    TooManySegments,
    /// The end of a segment does not meet the start of the next one.
    Disconnected {
        /// Index of the segment whose end is checked.
        index: usize,
    },
    /// An offset would leave a tangent path at a sharp joint.
    SharpJoint {
        /// Index of the joint after this segment.
        index: usize,
    },
    /// An inward offset would consume an arc.
    CollapsedRadius,
    /// A sampling tolerance is not positive and finite.
    InvalidTolerance,
    /// Sampling would produce more than [`MAX_SAMPLE_POINTS`] points.
    TooManyPoints,
    /// A closed contour bounds no area.
    Degenerate,
}

impl fmt::Display for ContourError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid contour input: {message}"),
            Self::ZeroLength => formatter.write_str("a contour line has zero length"),
            Self::ZeroSagitta => formatter.write_str("a bow needs a non-zero sagitta"),
            Self::SagittaTooDeep => formatter.write_str("a bow sagitta must not exceed half its chord"),
            Self::InvalidRadius => formatter.write_str("an arc radius must be positive and finite"),
            Self::InvalidSweep => formatter.write_str("an arc sweep must be non-zero and at most a full turn"),
            Self::Empty => formatter.write_str("a contour needs at least one segment"),
            Self::TooManySegments => formatter.write_str("a contour has too many segments"),
            Self::Disconnected { index } => {
                write!(formatter, "segment {index} does not meet the next segment")
            }
            Self::SharpJoint { index } => {
                write!(formatter, "joint {index} is sharp; only a tangent-continuous path can be offset")
            }
            Self::CollapsedRadius => formatter.write_str("an offset would consume an arc radius"),
            Self::InvalidTolerance => formatter.write_str("a sampling tolerance must be positive and finite"),
            Self::TooManyPoints => formatter.write_str("sampling would produce too many points"),
            Self::Degenerate => formatter.write_str("a closed contour must bound a non-zero area"),
        }
    }
}

impl std::error::Error for ContourError {}

/// A directed straight segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    start: DVec2,
    end: DVec2,
}

impl Line {
    /// Creates a finite, non-degenerate line.
    pub fn new(start: DVec2, end: DVec2) -> Result<Self, ContourError> {
        if !start.is_finite() || !end.is_finite() {
            return Err(ContourError::InvalidInput("line points must be finite"));
        }
        if (end - start).length() <= EPSILON {
            return Err(ContourError::ZeroLength);
        }
        Ok(Self { start, end })
    }

    /// The start point.
    pub const fn start(self) -> DVec2 {
        self.start
    }

    /// The end point.
    pub const fn end(self) -> DVec2 {
        self.end
    }

    /// The unit direction of travel.
    pub fn direction(self) -> DVec2 {
        (self.end - self.start).normalize()
    }

    /// The unit normal to the left of travel.
    pub fn left_normal(self) -> DVec2 {
        let direction = self.direction();
        DVec2::new(-direction.y, direction.x)
    }

    /// The line length.
    pub fn length(self) -> f64 {
        (self.end - self.start).length()
    }

    /// A point at parameter `t` in `0..=1`.
    pub fn point_at(self, t: f64) -> Option<DVec2> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some(self.start + (self.end - self.start) * t)
    }

    /// The unit tangent at the start.
    pub fn tangent_at_start(self) -> DVec2 {
        self.direction()
    }

    /// The unit tangent at the end.
    pub fn tangent_at_end(self) -> DVec2 {
        self.direction()
    }

    /// Moves the line to the left by `distance` millimetres.
    pub fn offset(self, distance: f64) -> Result<Self, ContourError> {
        if !distance.is_finite() {
            return Err(ContourError::InvalidInput("offset distance must be finite"));
        }
        let shift = self.left_normal() * distance;
        Self::new(self.start + shift, self.end + shift)
    }
}

/// A directed circular arc.
///
/// Angles are radians. A positive sweep runs counter-clockwise; a negative sweep runs clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    center: DVec2,
    radius: f64,
    start_angle: f64,
    sweep_angle: f64,
}

impl Arc {
    /// Creates an arc from a center, radius, start angle and signed sweep.
    pub fn new(center: DVec2, radius: f64, start_angle: f64, sweep_angle: f64) -> Result<Self, ContourError> {
        if !center.is_finite() || !start_angle.is_finite() {
            return Err(ContourError::InvalidInput("arc center and angles must be finite"));
        }
        if !radius.is_finite() || radius <= EPSILON {
            return Err(ContourError::InvalidRadius);
        }
        if !sweep_angle.is_finite() || sweep_angle.abs() <= EPSILON || sweep_angle.abs() > TAU + EPSILON {
            return Err(ContourError::InvalidSweep);
        }
        Ok(Self { center, radius, start_angle, sweep_angle })
    }

    /// Creates a minor circular arc through two endpoints with a signed sagitta.
    ///
    /// A positive sagitta bows to the left of the directed chord from `start` to `end`; a negative
    /// sagitta bows to the right. The magnitude must not exceed half the chord. This is the bow
    /// primitive used by a panel edge.
    pub fn from_endpoints_sagitta(start: DVec2, end: DVec2, sagitta: f64) -> Result<Self, ContourError> {
        if !start.is_finite() || !end.is_finite() || !sagitta.is_finite() {
            return Err(ContourError::InvalidInput("bow points and sagitta must be finite"));
        }
        let chord = end - start;
        let chord_length = chord.length();
        if chord_length <= EPSILON {
            return Err(ContourError::ZeroLength);
        }
        if sagitta.abs() <= EPSILON {
            return Err(ContourError::ZeroSagitta);
        }
        if sagitta.abs() > chord_length * 0.5 + EPSILON {
            return Err(ContourError::SagittaTooDeep);
        }

        let left = DVec2::new(-chord.y, chord.x) / chord_length;
        let middle = (start + end) * 0.5;
        let center_distance = chord_length * chord_length / (8.0 * sagitta) - sagitta * 0.5;
        let center = middle - left * center_distance;
        let radius = (start - center).length();
        if !center.is_finite() || !radius.is_finite() || radius <= EPSILON {
            return Err(ContourError::InvalidRadius);
        }

        let magnitude = 4.0 * (2.0 * sagitta.abs() / chord_length).atan();
        let sweep_angle = -sagitta.signum() * magnitude;
        let start_angle = (start.y - center.y).atan2(start.x - center.x);
        Self::new(center, radius, start_angle, sweep_angle)
    }

    /// The center of the circle.
    pub const fn center(self) -> DVec2 {
        self.center
    }

    /// The radius of the circle.
    pub const fn radius(self) -> f64 {
        self.radius
    }

    /// The start angle in radians.
    pub const fn start_angle(self) -> f64 {
        self.start_angle
    }

    /// The signed sweep in radians.
    pub const fn sweep_angle(self) -> f64 {
        self.sweep_angle
    }

    /// A point on the supporting circle at an angle.
    fn point_at_angle(self, angle: f64) -> DVec2 {
        self.center + DVec2::new(angle.cos(), angle.sin()) * self.radius
    }

    /// The start point.
    pub fn start(self) -> DVec2 {
        self.point_at_angle(self.start_angle)
    }

    /// The end point.
    pub fn end(self) -> DVec2 {
        self.point_at_angle(self.start_angle + self.sweep_angle)
    }

    /// The arc length.
    pub fn length(self) -> f64 {
        self.radius * self.sweep_angle.abs()
    }

    /// A point at parameter `t` in `0..=1`.
    pub fn point_at(self, t: f64) -> Option<DVec2> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some(self.point_at_angle(self.start_angle + self.sweep_angle * t))
    }

    /// The unit tangent at the start.
    pub fn tangent_at_start(self) -> DVec2 {
        self.tangent_at_angle(self.start_angle)
    }

    /// The unit tangent at the end.
    pub fn tangent_at_end(self) -> DVec2 {
        self.tangent_at_angle(self.start_angle + self.sweep_angle)
    }

    fn tangent_at_angle(self, angle: f64) -> DVec2 {
        let radial = DVec2::new(angle.cos(), angle.sin());
        let left = DVec2::new(-radial.y, radial.x);
        left * self.sweep_angle.signum()
    }

    /// Moves the arc to the left by `distance` millimetres.
    ///
    /// On a counter-clockwise arc the left side is toward the center, so the radius shrinks; on a
    /// clockwise arc it grows.
    pub fn offset(self, distance: f64) -> Result<Self, ContourError> {
        if !distance.is_finite() {
            return Err(ContourError::InvalidInput("offset distance must be finite"));
        }
        let radius = self.radius - self.sweep_angle.signum() * distance;
        if !radius.is_finite() || radius <= EPSILON {
            return Err(ContourError::CollapsedRadius);
        }
        Self::new(self.center, radius, self.start_angle, self.sweep_angle)
    }
}

/// One directed segment of a contour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// A straight segment.
    Line(Line),
    /// A circular arc.
    Arc(Arc),
}

impl Segment {
    /// The start point.
    pub fn start(self) -> DVec2 {
        match self {
            Self::Line(line) => line.start(),
            Self::Arc(arc) => arc.start(),
        }
    }

    /// The end point.
    pub fn end(self) -> DVec2 {
        match self {
            Self::Line(line) => line.end(),
            Self::Arc(arc) => arc.end(),
        }
    }

    /// The length.
    pub fn length(self) -> f64 {
        match self {
            Self::Line(line) => line.length(),
            Self::Arc(arc) => arc.length(),
        }
    }

    /// The unit tangent at the start.
    pub fn tangent_at_start(self) -> DVec2 {
        match self {
            Self::Line(line) => line.tangent_at_start(),
            Self::Arc(arc) => arc.tangent_at_start(),
        }
    }

    /// The unit tangent at the end.
    pub fn tangent_at_end(self) -> DVec2 {
        match self {
            Self::Line(line) => line.tangent_at_end(),
            Self::Arc(arc) => arc.tangent_at_end(),
        }
    }

    /// A point at parameter `t` in `0..=1`.
    pub fn point_at(self, t: f64) -> Option<DVec2> {
        match self {
            Self::Line(line) => line.point_at(t),
            Self::Arc(arc) => arc.point_at(t),
        }
    }

    /// Moves the segment to the left by `distance` millimetres.
    pub fn offset(self, distance: f64) -> Result<Self, ContourError> {
        match self {
            Self::Line(line) => Ok(Self::Line(line.offset(distance)?)),
            Self::Arc(arc) => Ok(Self::Arc(arc.offset(distance)?)),
        }
    }
}

/// A directed chain of lines and arcs.
///
/// A closed contour has a non-zero signed area and its last segment ends at the first segment's
/// start. An open contour is a path; its offset is still available when every internal joint is
/// tangent continuous. Self-intersection is not checked here.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    segments: Vec<Segment>,
    closed: bool,
}

impl Contour {
    /// Creates a contour after checking that it has at least one segment and that consecutive
    /// endpoints meet.
    pub fn new(segments: Vec<Segment>, closed: bool) -> Result<Self, ContourError> {
        if segments.is_empty() {
            return Err(ContourError::Empty);
        }
        if segments.len() > MAX_SEGMENTS {
            return Err(ContourError::TooManySegments);
        }
        for (index, segment) in segments.iter().enumerate() {
            let next_index = index + 1;
            let next = if next_index < segments.len() {
                segments.get(next_index)
            } else if closed {
                segments.first()
            } else {
                None
            };
            if let Some(next) = next
                && (segment.end() - next.start()).length() > JOIN_TOLERANCE
            {
                return Err(ContourError::Disconnected { index });
            }
        }
        let contour = Self { segments, closed };
        if closed {
            match contour.signed_area() {
                Some(area) if area.is_finite() && area.abs() > EPSILON => {}
                _ => return Err(ContourError::Degenerate),
            }
        }
        Ok(contour)
    }

    /// The segments in travel order.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether the contour is closed.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// The start point of the first segment.
    pub fn start(&self) -> Option<DVec2> {
        self.segments.first().map(|segment| segment.start())
    }

    /// The end point of the last segment.
    pub fn end(&self) -> Option<DVec2> {
        self.segments.last().map(|segment| segment.end())
    }

    /// The total path length.
    pub fn length(&self) -> f64 {
        self.segments.iter().map(|segment| segment.length()).sum()
    }

    /// The signed area for a closed contour, positive for counter-clockwise.
    pub fn signed_area(&self) -> Option<f64> {
        if !self.closed {
            return None;
        }
        let mut area = 0.0;
        for segment in &self.segments {
            area += segment_signed_area(*segment);
        }
        Some(area)
    }

    /// Whether the contour is counter-clockwise; `None` for an open contour.
    pub fn is_counter_clockwise(&self) -> Option<bool> {
        self.signed_area().map(|area| area > 0.0)
    }

    /// Checks the joint after `index` for tangent continuity.
    ///
    /// Returns `None` when the index is out of range or names the open end of a path.
    pub fn joint_is_smooth(&self, index: usize, tolerance: f64) -> Option<bool> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return None;
        }
        let current = self.segments.get(index)?;
        let next = if index + 1 < self.segments.len() {
            self.segments.get(index + 1)
        } else if self.closed {
            self.segments.first()
        } else {
            None
        }?;
        Some(tangent_angle(current.tangent_at_end(), next.tangent_at_start()) <= tolerance)
    }

    /// Whether every internal joint is tangent continuous.
    pub fn is_tangent_continuous(&self, tolerance: f64) -> bool {
        (0..self.segments.len()).all(|index| self.joint_is_smooth(index, tolerance) != Some(false))
    }

    /// Moves every segment to the left by `distance` millimetres.
    ///
    /// The path must be tangent continuous at every internal joint; a sharp joint is rejected. A
    /// positive distance on a counter-clockwise contour moves toward the material side.
    pub fn offset(&self, distance: f64) -> Result<Self, ContourError> {
        if !distance.is_finite() {
            return Err(ContourError::InvalidInput("offset distance must be finite"));
        }
        if distance == 0.0 {
            return Ok(self.clone());
        }
        for index in 0..self.segments.len() {
            if self.joint_is_smooth(index, DEFAULT_TANGENT_TOLERANCE) == Some(false) {
                return Err(ContourError::SharpJoint { index });
            }
        }
        let segments = self.segments.iter().map(|segment| segment.offset(distance)).collect::<Result<Vec<_>, _>>()?;
        Self::new(segments, self.closed)
    }

    /// Samples the contour as a polyline.
    ///
    /// The returned points are suitable for a kernel profile. For a closed contour the first point
    /// is not repeated at the end. Arc sampling is bounded by `tolerance` and by internal caps.
    pub fn sample(&self, tolerance: f64) -> Result<Vec<DVec2>, ContourError> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(ContourError::InvalidTolerance);
        }
        let mut points = Vec::new();
        for segment in &self.segments {
            points.push(segment.start());
            if let Segment::Arc(arc) = segment {
                let steps = arc_steps(*arc, tolerance)?;
                for step in 1..steps {
                    let Some(point) = arc.point_at(step as f64 / steps as f64) else {
                        return Err(ContourError::InvalidInput("arc sample parameter is not finite"));
                    };
                    points.push(point);
                }
            }
            if points.len() > MAX_SAMPLE_POINTS {
                return Err(ContourError::TooManyPoints);
            }
        }
        if !self.closed
            && let Some(end) = self.end()
        {
            points.push(end);
        }
        if self.closed && points.len() < 3 {
            return Err(ContourError::Degenerate);
        }
        Ok(points)
    }
}

fn tangent_angle(first: DVec2, second: DVec2) -> f64 {
    first.perp_dot(second).atan2(first.dot(second)).abs()
}

fn segment_signed_area(segment: Segment) -> f64 {
    match segment {
        Segment::Line(line) => line.start().perp_dot(line.end()) * 0.5,
        Segment::Arc(arc) => {
            let start = arc.start_angle();
            let end = start + arc.sweep_angle();
            let center = arc.center();
            let radius = arc.radius();
            let sweep = arc.sweep_angle();
            (radius * (center.x * (end.sin() - start.sin()) - center.y * (end.cos() - start.cos()))
                + radius * radius * sweep)
                * 0.5
        }
    }
}

fn arc_steps(arc: Arc, tolerance: f64) -> Result<usize, ContourError> {
    let ratio = 1.0 - tolerance / arc.radius();
    let step = if ratio <= -1.0 { PI * 0.5 } else { 2.0 * ratio.acos() };
    let step = step.clamp(1.0e-6, PI * 0.5);
    let count = (arc.sweep_angle().abs() / step).ceil();
    if !count.is_finite() || count > MAX_ARC_STEPS as f64 {
        return Err(ContourError::TooManyPoints);
    }
    Ok((count as usize).max(1))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn point(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn rounded_rectangle(width: f64, height: f64, radius: f64) -> Contour {
        let segments = vec![
            Segment::Line(Line::new(point(radius, 0.0), point(width - radius, 0.0)).expect("bottom line")),
            Segment::Arc(Arc::new(point(width - radius, radius), radius, -PI * 0.5, PI * 0.5).expect("bottom-right")),
            Segment::Line(Line::new(point(width, radius), point(width, height - radius)).expect("right line")),
            Segment::Arc(Arc::new(point(width - radius, height - radius), radius, 0.0, PI * 0.5).expect("top-right")),
            Segment::Line(Line::new(point(width - radius, height), point(radius, height)).expect("top line")),
            Segment::Arc(Arc::new(point(radius, height - radius), radius, PI * 0.5, PI * 0.5).expect("top-left")),
            Segment::Line(Line::new(point(0.0, height - radius), point(0.0, radius)).expect("left line")),
            Segment::Arc(Arc::new(point(radius, radius), radius, PI, PI * 0.5).expect("bottom-left")),
        ];
        Contour::new(segments, true).expect("rounded rectangle")
    }

    #[test]
    fn line_offset_moves_to_the_left() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("line");
        let offset = line.offset(2.0).expect("offset");
        assert!((offset.start() - point(0.0, 2.0)).length() < 1e-12);
        assert!((offset.end() - point(10.0, 2.0)).length() < 1e-12);
        assert!((offset.length() - 10.0).abs() < 1e-12);
    }

    #[test]
    fn arc_offset_follows_the_direction_of_travel() {
        let ccw = Arc::new(DVec2::ZERO, 10.0, 0.0, PI * 0.5).expect("ccw arc");
        let offset = ccw.offset(2.0).expect("offset");
        assert!((offset.radius() - 8.0).abs() < 1e-12);
        assert!((offset.start() - point(8.0, 0.0)).length() < 1e-12);
        assert!((offset.end() - point(0.0, 8.0)).length() < 1e-12);

        let cw = Arc::new(DVec2::ZERO, 10.0, PI * 0.5, -PI * 0.5).expect("cw arc");
        let offset = cw.offset(2.0).expect("offset");
        assert!((offset.radius() - 12.0).abs() < 1e-12);
        assert!((offset.start() - point(0.0, 12.0)).length() < 1e-12);
        assert!((offset.end() - point(12.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn rounded_rectangle_is_tangent_and_offsets_inward() {
        let contour = rounded_rectangle(100.0, 60.0, 10.0);
        assert!(contour.is_tangent_continuous(DEFAULT_TANGENT_TOLERANCE));
        assert!(contour.is_counter_clockwise() == Some(true));
        let area = contour.signed_area().expect("closed area");
        let expected = 100.0 * 60.0 - (4.0 - PI) * 10.0 * 10.0;
        assert!((area - expected).abs() < 1e-9, "area {area}, expected {expected}");

        let offset = contour.offset(2.0).expect("inner offset");
        let inner_area = offset.signed_area().expect("closed area");
        let expected_inner = 96.0 * 56.0 - (4.0 - PI) * 8.0 * 8.0;
        assert!((inner_area - expected_inner).abs() < 1e-9, "inner area {inner_area}, expected {expected_inner}");
        let inner_arc = match offset.segments().first() {
            Some(Segment::Line(_)) => offset.segments().get(1),
            _ => None,
        };
        assert!(matches!(inner_arc, Some(Segment::Arc(arc)) if (arc.radius() - 8.0).abs() < 1e-12));
    }

    #[test]
    fn sharp_joint_is_rejected_by_offset() {
        let contour = Contour::new(
            vec![
                Segment::Line(Line::new(point(0.0, 0.0), point(100.0, 0.0)).expect("bottom")),
                Segment::Line(Line::new(point(100.0, 0.0), point(100.0, 60.0)).expect("right")),
                Segment::Line(Line::new(point(100.0, 60.0), point(0.0, 60.0)).expect("top")),
                Segment::Line(Line::new(point(0.0, 60.0), point(0.0, 0.0)).expect("left")),
            ],
            true,
        )
        .expect("rectangle");
        assert!(matches!(contour.offset(2.0), Err(ContourError::SharpJoint { index: 0 })));
    }

    #[test]
    fn sagitta_bow_uses_the_left_side_of_the_chord() {
        let positive = Arc::from_endpoints_sagitta(point(0.0, 0.0), point(100.0, 0.0), 10.0).expect("bow");
        let midpoint = positive.point_at(0.5).expect("arc midpoint");
        assert!((midpoint.y - 10.0).abs() < 1e-6, "midpoint {midpoint:?}");
        assert!(positive.sweep_angle() < 0.0);

        let negative = Arc::from_endpoints_sagitta(point(0.0, 0.0), point(100.0, 0.0), -10.0).expect("bow");
        let midpoint = negative.point_at(0.5).expect("arc midpoint");
        assert!((midpoint.y + 10.0).abs() < 1e-6, "midpoint {midpoint:?}");
        assert!(negative.sweep_angle() > 0.0);
    }

    #[test]
    fn sagitta_rejects_zero_and_too_deep_bows() {
        assert!(matches!(
            Arc::from_endpoints_sagitta(point(0.0, 0.0), point(100.0, 0.0), 0.0),
            Err(ContourError::ZeroSagitta)
        ));
        assert!(matches!(
            Arc::from_endpoints_sagitta(point(0.0, 0.0), point(100.0, 0.0), 51.0),
            Err(ContourError::SagittaTooDeep)
        ));
    }

    #[test]
    fn open_line_path_can_be_offset_and_sampled() {
        let contour =
            Contour::new(vec![Segment::Line(Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("line"))], false)
                .expect("open path");
        let offset = contour.offset(2.0).expect("offset");
        assert_eq!(offset.start(), Some(point(0.0, 2.0)));
        assert_eq!(offset.end(), Some(point(10.0, 2.0)));
        let points = offset.sample(0.05).expect("samples");
        assert_eq!(points, vec![point(0.0, 2.0), point(10.0, 2.0)]);
    }

    #[test]
    fn collapsed_arc_is_rejected() {
        let arc = Arc::new(DVec2::ZERO, 1.0, 0.0, PI).expect("arc");
        assert!(matches!(arc.offset(2.0), Err(ContourError::CollapsedRadius)));
    }

    #[test]
    fn disconnected_contour_is_rejected() {
        let segments = vec![
            Segment::Line(Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("first")),
            Segment::Line(Line::new(point(11.0, 0.0), point(11.0, 10.0)).expect("second")),
        ];
        assert!(matches!(Contour::new(segments, false), Err(ContourError::Disconnected { index: 0 })));
    }

    #[test]
    fn closed_contour_samples_without_a_duplicate_end() {
        let contour = rounded_rectangle(100.0, 60.0, 10.0);
        let points = contour.sample(0.05).expect("samples");
        assert!(points.len() > 8);
        assert!((points.first().copied().expect("first") - points.last().copied().expect("last")).length() > 1e-9);
    }
}
