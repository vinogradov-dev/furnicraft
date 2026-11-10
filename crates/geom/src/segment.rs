//! One directed piece of a planar contour: a straight [`Line`], a circular [`Arc`] and the
//! [`Segment`] that unifies them.
//!
//! Coordinates are millimetres and angles are radians. Every tolerance is the caller's argument;
//! the module owns no precision policy beyond the degenerate-length threshold [`EPSILON`] and the
//! [`SUPPORT_TOLERANCE`] that decides whether a point is treated as lying on a supporting line or
//! circle.
//!
//! It refuses non-finite input, a zero-length line, a zero radius, a sagitta at or beyond half its
//! chord and a split point outside the segment; it returns [`ContourError`] rather than clamping or
//! guessing. It does not model a chain: joining segments, offsetting a ring and sampling it are
//! `contour.rs`'s job.
//!
//! Changes when: the geometry of a single segment changes — the arc parameterisation, the
//! point-to-support projection or the split rule.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::{ContourError, DVec2};

/// Smallest accepted length, radius or sagitta magnitude, in millimetres.
pub(crate) const EPSILON: f64 = 1.0e-9;
/// Largest sweep a caller may add when it extends an arc to a point, in radians.
///
/// The cap keeps an extension from wrapping around the supporting circle and finding a point on
/// the opposite side.
pub const MAX_ARC_EXTENSION: f64 = FRAC_PI_2;
/// Tolerance for treating a point as lying on a supporting line or circle, in millimetres.
const SUPPORT_TOLERANCE: f64 = 1.0e-6;

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

    /// The same curve with the opposite direction of travel.
    ///
    /// A line swaps its endpoints; an arc starts where it used to end and sweeps the other way, so
    /// the supporting circle, its radius and its extent do not change.
    pub fn reversed(self) -> Result<Self, ContourError> {
        match self {
            Self::Line(line) => Ok(Self::Line(Line::new(line.end(), line.start())?)),
            Self::Arc(arc) => {
                Ok(Self::Arc(Arc::new(arc.center, arc.radius, arc.start_angle + arc.sweep_angle, -arc.sweep_angle)?))
            }
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

fn positive_angle(angle: f64) -> f64 {
    let mut value = angle % TAU;
    if value < 0.0 {
        value += TAU;
    }
    value
}
