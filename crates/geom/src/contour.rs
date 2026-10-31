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

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt;

use crate::DVec2;

/// Smallest accepted length, radius or sagitta magnitude, in millimetres.
const EPSILON: f64 = 1.0e-9;
/// Maximum gap between the end of one segment and the start of the next, in millimetres.
const JOIN_TOLERANCE: f64 = 1.0e-6;
/// Default angular tolerance for a smooth joint, in radians.
pub const DEFAULT_TANGENT_TOLERANCE: f64 = 1.0e-6;
/// Largest sweep a caller may add when it extends an arc to a point, in radians.
///
/// The cap keeps an extension from wrapping around the supporting circle and finding a point on
/// the opposite side.
pub const MAX_ARC_EXTENSION: f64 = FRAC_PI_2;
/// Tolerance for treating a point as lying on a supporting line or circle, in millimetres.
const SUPPORT_TOLERANCE: f64 = 1.0e-6;
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
    /// A split point does not lie strictly inside the segment it would split.
    SplitOutside,
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
            Self::SplitOutside => formatter.write_str("a split point must lie strictly inside the segment"),
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

    /// The axis-aligned bounds of the line.
    pub fn bounds(self) -> (DVec2, DVec2) {
        let min = DVec2::new(self.start.x.min(self.end.x), self.start.y.min(self.end.y));
        let max = DVec2::new(self.start.x.max(self.end.x), self.start.y.max(self.end.y));
        (min, max)
    }

    /// A point at parameter `t` in `0..=1`.
    pub fn point_at(self, t: f64) -> Option<DVec2> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some(self.start + (self.end - self.start) * t)
    }

    /// The parameter of the projection of `point` onto the supporting line.
    ///
    /// The result is not restricted to `0..=1`; a caller that means the segment checks the span.
    fn parameter_of(self, point: DVec2) -> Option<f64> {
        if !point.is_finite() {
            return None;
        }
        let direction = self.end - self.start;
        let length_squared = direction.length_squared();
        if !length_squared.is_finite() || length_squared <= EPSILON * EPSILON {
            return None;
        }
        let parameter = (point - self.start).dot(direction) / length_squared;
        parameter.is_finite().then_some(parameter)
    }

    /// The point of the supporting line nearest to `point`.
    pub fn nearest_on_support(self, point: DVec2) -> Option<DVec2> {
        let parameter = self.parameter_of(point)?;
        let nearest = self.start + (self.end - self.start) * parameter;
        nearest.is_finite().then_some(nearest)
    }

    /// Whether `point` lies on the segment within `tolerance` millimetres.
    pub fn contains_point(self, point: DVec2, tolerance: f64) -> bool {
        if !point.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
            return false;
        }
        let Some(parameter) = self.parameter_of(point) else {
            return false;
        };
        if !(0.0..=1.0).contains(&parameter) {
            return false;
        }
        let foot = self.start + (self.end - self.start) * parameter;
        (point - foot).length() <= tolerance
    }

    /// Splits the line at a point strictly inside it.
    ///
    /// The first part keeps the start and the second part keeps the end, so the two parts share
    /// `point` exactly. A point on an endpoint or off the segment is rejected.
    pub fn split_at(self, point: DVec2) -> Result<(Self, Self), ContourError> {
        let Some(parameter) = self.parameter_of(point) else {
            return Err(ContourError::InvalidInput("split point must be finite"));
        };
        if !self.contains_point(point, SUPPORT_TOLERANCE) {
            return Err(ContourError::SplitOutside);
        }
        let length = self.length();
        if parameter * length <= EPSILON || (1.0 - parameter) * length <= EPSILON {
            return Err(ContourError::SplitOutside);
        }
        Ok((Self::new(self.start, point)?, Self::new(point, self.end)?))
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

    /// The point where the two infinite lines meet, or `None` when they are parallel.
    ///
    /// The result is not checked against either segment: a call site that joins offset segments
    /// decides whether the mitre is acceptable.
    pub fn intersection(self, other: Self) -> Option<DVec2> {
        let direction = self.end - self.start;
        let other_direction = other.end - other.start;
        let denominator = direction.x * other_direction.y - direction.y * other_direction.x;
        if denominator.abs() <= EPSILON {
            return None;
        }
        let offset = other.start - self.start;
        let parameter = (offset.x * other_direction.y - offset.y * other_direction.x) / denominator;
        let point = self.start + direction * parameter;
        point.is_finite().then_some(point)
    }

    /// The points where the supporting line meets an arc's supporting circle.
    ///
    /// The result is not restricted to either span: a call site that cuts a band against a
    /// neighbouring face decides which candidate is acceptable. A tangent line returns one point
    /// and a line that misses the circle returns none.
    pub fn intersections_with_arc(self, arc: Arc) -> [Option<DVec2>; 2] {
        let direction = self.end - self.start;
        let length_squared = direction.length_squared();
        if !length_squared.is_finite() || length_squared <= EPSILON * EPSILON {
            return [None, None];
        }
        let to_center = arc.center() - self.start;
        let foot = self.start + direction * (to_center.dot(direction) / length_squared);
        let distance = (arc.center() - foot).length();
        if !distance.is_finite() || distance > arc.radius() + SUPPORT_TOLERANCE {
            return [None, None];
        }
        let half = (arc.radius() * arc.radius() - distance * distance).max(0.0).sqrt();
        if half <= SUPPORT_TOLERANCE {
            return [foot.is_finite().then_some(foot), None];
        }
        let unit = direction / length_squared.sqrt();
        [Some(foot - unit * half), Some(foot + unit * half)]
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

    /// The axis-aligned bounds of the arc.
    ///
    /// The bounds include the endpoints and every axis extreme crossed by the sweep.
    pub fn bounds(self) -> (DVec2, DVec2) {
        let start = self.start();
        let end = self.end();
        let mut min = DVec2::new(start.x.min(end.x), start.y.min(end.y));
        let mut max = DVec2::new(start.x.max(end.x), start.y.max(end.y));
        for angle in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2] {
            if self.contains_angle(angle) {
                let point = self.point_at_angle(angle);
                min = DVec2::new(min.x.min(point.x), min.y.min(point.y));
                max = DVec2::new(max.x.max(point.x), max.y.max(point.y));
            }
        }
        (min, max)
    }

    /// Whether the sweep from the start angle reaches `angle` in its direction of travel.
    fn contains_angle(self, angle: f64) -> bool {
        let sweep = self.sweep_angle;
        if sweep.abs() >= TAU - EPSILON {
            return true;
        }
        let delta = if sweep > 0.0 {
            positive_angle(angle - self.start_angle)
        } else {
            positive_angle(self.start_angle - angle)
        };
        delta <= sweep.abs() + EPSILON
    }

    /// A point at parameter `t` in `0..=1`.
    pub fn point_at(self, t: f64) -> Option<DVec2> {
        if !t.is_finite() || !(0.0..=1.0).contains(&t) {
            return None;
        }
        Some(self.point_at_angle(self.start_angle + self.sweep_angle * t))
    }

    /// The point of the supporting circle nearest to `point`.
    ///
    /// Returns `None` for a non-finite point and for the centre, where every point of the circle is
    /// equally distant.
    pub fn nearest_on_support(self, point: DVec2) -> Option<DVec2> {
        if !point.is_finite() {
            return None;
        }
        let radial = point - self.center;
        let length = radial.length();
        if !length.is_finite() || length <= EPSILON {
            return None;
        }
        Some(self.center + radial / length * self.radius)
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

    /// The points where the two supporting circles meet.
    ///
    /// Tangent circles return one point. Separated, nested and concentric circles return none.
    pub fn intersections_with_arc(self, other: Self) -> [Option<DVec2>; 2] {
        let between = other.center - self.center;
        let distance = between.length();
        if !distance.is_finite() || distance <= SUPPORT_TOLERANCE {
            return [None, None];
        }
        if distance > self.radius + other.radius + SUPPORT_TOLERANCE
            || distance < (self.radius - other.radius).abs() - SUPPORT_TOLERANCE
        {
            return [None, None];
        }
        let along = (self.radius * self.radius - other.radius * other.radius + distance * distance) / (2.0 * distance);
        let base = self.center + between / distance * along;
        let height_squared = self.radius * self.radius - along * along;
        if !base.is_finite() || !height_squared.is_finite() {
            return [None, None];
        }
        let height = height_squared.max(0.0).sqrt();
        if height <= SUPPORT_TOLERANCE {
            return [Some(base), None];
        }
        let normal = DVec2::new(-between.y, between.x) / distance;
        [Some(base + normal * height), Some(base - normal * height)]
    }

    /// Whether a point lies on the supporting circle and inside the sweep.
    ///
    /// `tolerance` is a linear distance in millimetres; it is converted to angular slack at the
    /// arc's radius so a point at an endpoint is accepted.
    pub fn contains_point(self, point: DVec2, tolerance: f64) -> bool {
        if !point.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
            return false;
        }
        let radial = point - self.center;
        if (radial.length() - self.radius).abs() > tolerance {
            return false;
        }
        let angle = radial.y.atan2(radial.x);
        let delta = if self.sweep_angle > 0.0 {
            positive_angle(angle - self.start_angle)
        } else {
            positive_angle(self.start_angle - angle)
        };
        delta <= self.sweep_angle.abs() + tolerance / self.radius + 1.0e-12
    }

    /// The angular distance from the start angle to `point` in the direction of travel.
    ///
    /// The result is in `0..TAU` and names a point on the supporting circle, not necessarily on
    /// the sweep.
    fn travel_delta(self, point: DVec2) -> Result<f64, ContourError> {
        if !point.is_finite() {
            return Err(ContourError::InvalidInput("arc point must be finite"));
        }
        let radial = point - self.center;
        if (radial.length() - self.radius).abs() > SUPPORT_TOLERANCE {
            return Err(ContourError::InvalidInput("point must lie on the arc's circle"));
        }
        let angle = radial.y.atan2(radial.x);
        Ok(if self.sweep_angle > 0.0 {
            positive_angle(angle - self.start_angle)
        } else {
            positive_angle(self.start_angle - angle)
        })
    }

    /// Moves the arc's start to a point on its supporting circle, keeping the end fixed.
    ///
    /// A point inside the sweep trims the arc, a point up to [`MAX_ARC_EXTENSION`] behind the start
    /// extends it, and a point past the end is rejected.
    pub fn with_start_point(self, point: DVec2) -> Result<Self, ContourError> {
        let ahead = self.travel_delta(point)?;
        if ahead <= EPSILON || ahead >= TAU - EPSILON {
            return Ok(self);
        }
        let sign = self.sweep_angle.signum();
        if ahead <= self.sweep_angle.abs() + 1.0e-12 {
            let sweep = self.sweep_angle.abs() - ahead;
            if sweep <= EPSILON {
                return Err(ContourError::InvalidSweep);
            }
            return Self::new(self.center, self.radius, self.start_angle + sign * ahead, sign * sweep);
        }
        let behind = TAU - ahead;
        if behind > MAX_ARC_EXTENSION {
            return Err(ContourError::InvalidSweep);
        }
        let sweep = self.sweep_angle.abs() + behind;
        if sweep >= TAU - EPSILON {
            return Err(ContourError::InvalidSweep);
        }
        Self::new(self.center, self.radius, self.start_angle - sign * behind, sign * sweep)
    }

    /// Moves the arc's end to a point on its supporting circle, keeping the start fixed.
    ///
    /// A point inside the sweep trims the arc, a point up to [`MAX_ARC_EXTENSION`] beyond the end
    /// extends it, and a point behind the start is rejected.
    pub fn with_end_point(self, point: DVec2) -> Result<Self, ContourError> {
        let ahead = self.travel_delta(point)?;
        if ahead <= EPSILON || ahead >= TAU - EPSILON {
            return Err(ContourError::InvalidSweep);
        }
        let extension = ahead - self.sweep_angle.abs();
        if extension > MAX_ARC_EXTENSION {
            return Err(ContourError::InvalidSweep);
        }
        Self::new(self.center, self.radius, self.start_angle, self.sweep_angle.signum() * ahead)
    }

    /// Splits the arc at a point strictly inside its sweep.
    ///
    /// Both parts keep the centre, the radius and the direction of travel, so the joint they share
    /// is exactly the split point. A point on an endpoint or off the circle is rejected.
    pub fn split_at(self, point: DVec2) -> Result<(Self, Self), ContourError> {
        let ahead = self.travel_delta(point)?;
        if ahead <= EPSILON || ahead >= self.sweep_angle.abs() - EPSILON {
            return Err(ContourError::SplitOutside);
        }
        let sign = self.sweep_angle.signum();
        let first = Self::new(self.center, self.radius, self.start_angle, sign * ahead)?;
        let second = Self::new(
            self.center,
            self.radius,
            self.start_angle + sign * ahead,
            sign * (self.sweep_angle.abs() - ahead),
        )?;
        Ok((first, second))
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

    /// Whether `point` lies on the segment within `tolerance` millimetres.
    pub fn contains_point(self, point: DVec2, tolerance: f64) -> bool {
        match self {
            Self::Line(line) => line.contains_point(point, tolerance),
            Self::Arc(arc) => arc.contains_point(point, tolerance),
        }
    }

    /// The point of the segment's supporting line or circle nearest to `point`.
    pub fn nearest_on_support(self, point: DVec2) -> Option<DVec2> {
        match self {
            Self::Line(line) => line.nearest_on_support(point),
            Self::Arc(arc) => arc.nearest_on_support(point),
        }
    }

    /// Splits the segment at a point strictly inside it.
    ///
    /// Both parts keep the segment's supporting curve and its direction of travel, so the joint
    /// they share is exactly the split point.
    pub fn split_at(self, point: DVec2) -> Result<(Self, Self), ContourError> {
        match self {
            Self::Line(line) => {
                let (first, second) = line.split_at(point)?;
                Ok((Self::Line(first), Self::Line(second)))
            }
            Self::Arc(arc) => {
                let (first, second) = arc.split_at(point)?;
                Ok((Self::Arc(first), Self::Arc(second)))
            }
        }
    }

    /// The axis-aligned bounds of the segment.
    pub fn bounds(self) -> (DVec2, DVec2) {
        match self {
            Self::Line(line) => line.bounds(),
            Self::Arc(arc) => arc.bounds(),
        }
    }

    /// Moves the segment to the left by `distance` millimetres.
    pub fn offset(self, distance: f64) -> Result<Self, ContourError> {
        match self {
            Self::Line(line) => Ok(Self::Line(line.offset(distance)?)),
            Self::Arc(arc) => Ok(Self::Arc(arc.offset(distance)?)),
        }
    }

    /// The points where two segments' supporting curves meet.
    ///
    /// A line pair returns its single infinite-line intersection, a line and a circle up to two
    /// points, and two circles up to two points. The result is not restricted to either span.
    pub fn intersections_with(self, other: Self) -> [Option<DVec2>; 2] {
        match (self, other) {
            (Self::Line(first), Self::Line(second)) => [first.intersection(second), None],
            (Self::Line(line), Self::Arc(arc)) | (Self::Arc(arc), Self::Line(line)) => line.intersections_with_arc(arc),
            (Self::Arc(first), Self::Arc(second)) => first.intersections_with_arc(second),
        }
    }

    /// The sub-segment from `start` to `end`, in the segment's direction of travel.
    ///
    /// Both points must lie on the segment's supporting curve. For an arc this may trim or extend
    /// the sweep within [`MAX_ARC_EXTENSION`].
    pub fn trimmed(self, start: DVec2, end: DVec2) -> Result<Self, ContourError> {
        match self {
            Self::Line(_) => Ok(Self::Line(Line::new(start, end)?)),
            Self::Arc(arc) => Ok(Self::Arc(arc.with_start_point(start)?.with_end_point(end)?)),
        }
    }

    /// The segment with its start (`at_end = false`) or end fixed at `point`.
    ///
    /// The other endpoint is kept; an arc may be trimmed or extended within
    /// [`MAX_ARC_EXTENSION`].
    pub fn trimmed_to(self, point: DVec2, at_end: bool) -> Result<Self, ContourError> {
        match self {
            Self::Line(line) => {
                if at_end {
                    Ok(Self::Line(Line::new(line.start(), point)?))
                } else {
                    Ok(Self::Line(Line::new(point, line.end())?))
                }
            }
            Self::Arc(arc) => {
                Ok(Self::Arc(if at_end { arc.with_end_point(point)? } else { arc.with_start_point(point)? }))
            }
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

    /// The axis-aligned bounds of the whole contour.
    ///
    /// Returns `None` only for an empty contour, which constructors reject.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut min = DVec2::ZERO;
        let mut max = DVec2::ZERO;
        let mut first = true;
        for segment in &self.segments {
            let (segment_min, segment_max) = segment.bounds();
            if first {
                min = segment_min;
                max = segment_max;
                first = false;
            } else {
                min = DVec2::new(min.x.min(segment_min.x), min.y.min(segment_min.y));
                max = DVec2::new(max.x.max(segment_max.x), max.y.max(segment_max.y));
            }
        }
        (!first).then_some((min, max))
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

fn positive_angle(angle: f64) -> f64 {
    let mut value = angle % TAU;
    if value < 0.0 {
        value += TAU;
    }
    value
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
    use std::f64::consts::FRAC_PI_4;

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
    fn line_intersection_meets_the_supporting_lines() {
        let bottom = Line::new(point(0.0, 2.0), point(80.0, 2.0)).expect("bottom");
        let chamfer = Line::new(point(80.0, 0.0), point(100.0, 5.0)).expect("chamfer");
        let meeting = bottom.intersection(chamfer).expect("the lines meet");
        assert!((meeting - point(88.0, 2.0)).length() < 1e-12, "met at {meeting:?}");

        let parallel = Line::new(point(0.0, 4.0), point(10.0, 4.0)).expect("parallel");
        assert!(bottom.intersection(parallel).is_none());
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

    #[test]
    fn contour_bounds_follow_arc_extremes() {
        let contour = rounded_rectangle(100.0, 60.0, 10.0);
        let (min, max) = contour.bounds().expect("bounds");
        assert!((min - point(0.0, 0.0)).length() < 1e-12, "min {min:?}");
        assert!((max - point(100.0, 60.0)).length() < 1e-12, "max {max:?}");

        let arc = Arc::new(DVec2::ZERO, 10.0, 0.0, PI * 0.5).expect("quarter arc");
        let (min, max) = arc.bounds();
        assert!((min - point(0.0, 0.0)).length() < 1e-12, "arc min {min:?}");
        assert!((max - point(10.0, 10.0)).length() < 1e-12, "arc max {max:?}");
    }

    #[test]
    fn bow_bounds_follow_the_bulge() {
        let outward = Arc::from_endpoints_sagitta(point(0.0, 0.0), point(100.0, 0.0), -10.0).expect("bow");
        let (min, max) = outward.bounds();
        assert!((min.y + 10.0).abs() < 1e-9, "outward min {min:?}");
        assert!((max.y - 0.0).abs() < 1e-9, "outward max {max:?}");
    }

    #[test]
    fn line_meets_a_circle_in_two_points_or_one() {
        let circle = Arc::new(DVec2::ZERO, 5.0, 0.0, TAU).expect("circle");
        let crossing = Line::new(point(-10.0, 0.0), point(10.0, 0.0)).expect("crossing");
        let mut hits: Vec<_> = crossing.intersections_with_arc(circle).into_iter().flatten().collect();
        hits.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(hits.len(), 2);
        assert!((hits[0] - point(-5.0, 0.0)).length() < 1e-9, "hits {hits:?}");
        assert!((hits[1] - point(5.0, 0.0)).length() < 1e-9, "hits {hits:?}");

        let tangent = Line::new(point(-10.0, 5.0), point(10.0, 5.0)).expect("tangent");
        let hits: Vec<_> = tangent.intersections_with_arc(circle).into_iter().flatten().collect();
        assert_eq!(hits.len(), 1);
        assert!((hits[0] - point(0.0, 5.0)).length() < 1e-9);

        let missing = Line::new(point(-10.0, 6.0), point(10.0, 6.0)).expect("missing");
        assert!(missing.intersections_with_arc(circle).iter().all(Option::is_none));
    }

    #[test]
    fn circles_meet_at_two_points_or_one() {
        let first = Arc::new(DVec2::ZERO, 5.0, 0.0, TAU).expect("first");
        let second = Arc::new(point(8.0, 0.0), 5.0, 0.0, TAU).expect("second");
        let mut hits: Vec<_> = first.intersections_with_arc(second).into_iter().flatten().collect();
        hits.sort_by(|a, b| a.y.total_cmp(&b.y));
        assert_eq!(hits.len(), 2);
        assert!((hits[0] - point(4.0, -3.0)).length() < 1e-9, "hits {hits:?}");
        assert!((hits[1] - point(4.0, 3.0)).length() < 1e-9, "hits {hits:?}");

        let tangent = Arc::new(point(10.0, 0.0), 5.0, 0.0, TAU).expect("tangent");
        let hits: Vec<_> = first.intersections_with_arc(tangent).into_iter().flatten().collect();
        assert_eq!(hits.len(), 1);
        assert!((hits[0] - point(5.0, 0.0)).length() < 1e-9);

        let apart = Arc::new(point(20.0, 0.0), 5.0, 0.0, TAU).expect("apart");
        assert!(first.intersections_with_arc(apart).iter().all(Option::is_none));

        let nested = Arc::new(DVec2::ZERO, 2.0, 0.0, TAU).expect("nested");
        assert!(first.intersections_with_arc(nested).iter().all(Option::is_none));

        let concentric = Arc::new(DVec2::ZERO, 7.0, 0.0, TAU).expect("concentric");
        assert!(first.intersections_with_arc(concentric).iter().all(Option::is_none));
    }

    #[test]
    fn arc_contains_point_checks_the_circle_and_the_sweep() {
        let quarter = Arc::new(DVec2::ZERO, 10.0, 0.0, FRAC_PI_2).expect("quarter");
        let middle = quarter.point_at(0.5).expect("midpoint");
        assert!(quarter.contains_point(middle, 1e-6));
        assert!(quarter.contains_point(quarter.end(), 1e-6));
        assert!(!quarter.contains_point(point(-10.0, 0.0), 1e-6));
        assert!(!quarter.contains_point(point(11.0, 0.0), 1e-6));
        assert!(!quarter.contains_point(DVec2::new(f64::NAN, 0.0), 1e-6));
    }

    #[test]
    fn arc_trims_and_extends_to_points_on_its_circle() {
        let quarter = Arc::new(DVec2::ZERO, 10.0, 0.0, FRAC_PI_2).expect("quarter");
        let on_circle = |angle: f64| point(10.0 * angle.cos(), 10.0 * angle.sin());

        let trimmed = quarter.with_start_point(on_circle(PI / 6.0)).expect("trim the start");
        assert!((trimmed.start_angle() - PI / 6.0).abs() < 1e-12);
        assert!((trimmed.sweep_angle() - PI / 3.0).abs() < 1e-12);
        assert!((trimmed.start() - on_circle(PI / 6.0)).length() < 1e-9);

        let extended = quarter.with_end_point(on_circle(2.0 * PI / 3.0)).expect("extend the end");
        assert!((extended.sweep_angle() - 2.0 * PI / 3.0).abs() < 1e-12);
        assert!((extended.end() - on_circle(2.0 * PI / 3.0)).length() < 1e-9);

        let extended = quarter.with_start_point(on_circle(-PI / 6.0)).expect("extend the start");
        assert!((extended.start() - on_circle(-PI / 6.0)).length() < 1e-9);
        assert!((extended.sweep_angle() - 2.0 * PI / 3.0).abs() < 1e-12);

        assert!(matches!(quarter.with_end_point(on_circle(5.0 * PI / 3.0)), Err(ContourError::InvalidSweep)));
        assert!(matches!(quarter.with_end_point(point(10.0, 1.0)), Err(ContourError::InvalidInput(_))));

        let clockwise = Arc::new(DVec2::ZERO, 10.0, 0.0, -FRAC_PI_2).expect("clockwise quarter");
        let extended = clockwise.with_end_point(on_circle(-3.0 * PI / 4.0)).expect("extend a clockwise arc");
        assert!((extended.sweep_angle() + 3.0 * PI / 4.0).abs() < 1e-12);
    }

    #[test]
    fn segment_intersections_dispatch_and_trim() {
        let line = Segment::Line(Line::new(point(0.0, -10.0), point(0.0, 10.0)).expect("line"));
        let arc = Segment::Arc(Arc::new(DVec2::ZERO, 5.0, 0.0, TAU).expect("circle"));
        let hits: Vec<_> = line.intersections_with(arc).into_iter().flatten().collect();
        assert_eq!(hits.len(), 2);

        let trimmed = arc.trimmed(point(5.0, 0.0), point(0.0, 5.0)).expect("trim the circle");
        assert!((trimmed.length() - 5.0 * FRAC_PI_2).abs() < 1e-9);
        assert!(line.trimmed(point(0.0, -10.0), point(0.0, -10.0)).is_err());
    }

    #[test]
    fn line_nearest_point_and_containment_follow_the_span() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("line");
        let nearest = line.nearest_on_support(point(4.0, 3.0)).expect("a foot");
        assert!((nearest - point(4.0, 0.0)).length() < 1e-12);
        assert!(line.contains_point(point(4.0, 0.0), 1e-9));
        assert!(line.contains_point(point(4.0, 0.5), 1.0));
        assert!(!line.contains_point(point(4.0, 1.5), 1.0));
        assert!(!line.contains_point(point(11.0, 0.0), 1e-9));
        assert!(!line.contains_point(DVec2::new(f64::NAN, 0.0), 1e-9));
        assert!(line.nearest_on_support(DVec2::new(f64::INFINITY, 0.0)).is_none());
    }

    #[test]
    fn line_splits_only_inside_its_span() {
        let line = Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("line");
        let (first, second) = line.split_at(point(4.0, 0.0)).expect("split");
        assert!((first.length() - 4.0).abs() < 1e-12);
        assert!((second.length() - 6.0).abs() < 1e-12);
        assert_eq!(first.end(), second.start());

        assert!(matches!(line.split_at(point(0.0, 0.0)), Err(ContourError::SplitOutside)));
        assert!(matches!(line.split_at(point(10.0, 0.0)), Err(ContourError::SplitOutside)));
        assert!(matches!(line.split_at(point(4.0, 1.0)), Err(ContourError::SplitOutside)));
    }

    #[test]
    fn arc_nearest_point_and_split_keep_the_circle() {
        let quarter = Arc::new(DVec2::ZERO, 10.0, 0.0, FRAC_PI_2).expect("quarter");
        let nearest = quarter.nearest_on_support(point(20.0, 0.0)).expect("a circle point");
        assert!((nearest - point(10.0, 0.0)).length() < 1e-12);
        assert!(quarter.nearest_on_support(DVec2::ZERO).is_none());

        let middle = quarter.point_at(0.5).expect("midpoint");
        let (first, second) = quarter.split_at(middle).expect("split");
        assert!((first.sweep_angle() - FRAC_PI_4).abs() < 1e-12);
        assert!((second.sweep_angle() - FRAC_PI_4).abs() < 1e-12);
        assert!((first.end() - second.start()).length() < 1e-12);
        assert!((first.radius() - 10.0).abs() < 1e-12);

        let clockwise = Arc::new(DVec2::ZERO, 10.0, 0.0, -FRAC_PI_2).expect("clockwise quarter");
        let middle = clockwise.point_at(0.5).expect("midpoint");
        let (first, second) = clockwise.split_at(middle).expect("split a clockwise arc");
        assert!((first.sweep_angle() + FRAC_PI_4).abs() < 1e-12);
        assert!((second.sweep_angle() + FRAC_PI_4).abs() < 1e-12);
        assert!((first.end() - second.start()).length() < 1e-12);

        assert!(matches!(quarter.split_at(quarter.start()), Err(ContourError::SplitOutside)));
        assert!(matches!(quarter.split_at(point(10.0, 1.0)), Err(ContourError::InvalidInput(_))));
    }

    #[test]
    fn segment_support_and_split_dispatch_by_kind() {
        let line = Segment::Line(Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("line"));
        let line_nearest = line.nearest_on_support(point(5.0, 2.0)).expect("a foot");
        assert!((line_nearest - point(5.0, 0.0)).length() < 1e-12);
        assert!(line.contains_point(point(5.0, 0.0), 1e-9));

        let arc = Segment::Arc(Arc::new(point(0.0, 0.0), 5.0, 0.0, TAU).expect("circle"));
        assert!(arc.contains_point(point(0.0, 5.0), 1e-9));
        let (first, second) = arc.split_at(point(0.0, -5.0)).expect("split the circle");
        assert!((first.length() - 1.5 * PI * 5.0).abs() < 1e-9);
        assert!((second.length() - 0.5 * PI * 5.0).abs() < 1e-9);
    }
}
