//! A closed ring or open path built from straight lines and circular arcs, and its offset.
//!
//! This is the two-dimensional half of the panel-contour model: the chain that joins segments, the
//! checks on its joints, its area and bounds, and the offset that moves a tangent-continuous path to
//! the left of its travel direction by a signed distance; for a counter-clockwise closed contour,
//! positive is the material side.
//!
//! Coordinates are millimetres and angles are radians. Consecutive endpoints must meet within
//! [`JOIN_TOLERANCE`], or within a bound the caller derives per joint from document units (ADR
//! 0072); a closed ring must bound a non-zero signed area.
//!
//! The module does not model a filled region, a boolean operation or self-intersection; callers
//! build a simple contour and offset paths that are known to be tangent continuous, and a closed
//! contour must not cross itself. The geometry of one segment — parameterisation, projection, split
//! — lives in `segment.rs`.
//!
//! Changes when: what a ring is, or how a chain is validated, reversed, offset or sampled
//! changes.

use std::f64::consts::PI;
use std::fmt;

use crate::DVec2;
use crate::segment::{Arc, EPSILON, Segment};

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
    /// endpoints meet within [`JOIN_TOLERANCE`].
    pub fn new(segments: Vec<Segment>, closed: bool) -> Result<Self, ContourError> {
        Self::new_with_tolerance(segments, closed, JOIN_TOLERANCE)
    }

    /// Creates a contour whose joints may miss by at most `joint_tolerance` millimetres.
    ///
    /// The document stores whole micrometres, so a ring converted from document units can close
    /// only to within one micrometre: an arc endpoint rounded into document units and the next
    /// segment's rounded start differ by less than that. Such a caller passes its own bound rather
    /// than the strict geometry one.
    pub fn new_with_tolerance(
        segments: Vec<Segment>,
        closed: bool,
        joint_tolerance: f64,
    ) -> Result<Self, ContourError> {
        if !joint_tolerance.is_finite() || joint_tolerance < 0.0 {
            return Err(ContourError::InvalidInput("a joint tolerance must be finite and non-negative"));
        }
        Self::new_with_joint_tolerance(segments, closed, move |_, _| joint_tolerance)
    }

    /// Creates a contour whose every joint is judged by its own tolerance in millimetres.
    ///
    /// The bound depends on the two segments at a joint: a line is stored exactly, while an arc
    /// endpoint is derived from a rounded centre, radius and angle, so the joint may miss by more
    /// the larger the arc is. A caller converting document segments passes a rule rather than one
    /// number, so a wide arc does not loosen a line-to-line joint on the same ring (ADR 0072). A
    /// non-finite or negative bound is refused like the constant one.
    pub fn new_with_joint_tolerance<F>(
        segments: Vec<Segment>,
        closed: bool,
        joint_tolerance: F,
    ) -> Result<Self, ContourError>
    where
        F: Fn(&Segment, &Segment) -> f64,
    {
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
            if let Some(next) = next {
                let tolerance = joint_tolerance(segment, next);
                if !tolerance.is_finite() || tolerance < 0.0 {
                    return Err(ContourError::InvalidInput("a joint tolerance must be finite and non-negative"));
                }
                if (segment.end() - next.start()).length() > tolerance {
                    return Err(ContourError::Disconnected { index });
                }
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

    /// The same path with the opposite direction of travel.
    ///
    /// Every segment is reversed and the order is flipped, so the path keeps its geometry and its
    /// joints; a closed contour that ran counter-clockwise runs clockwise. The result is returned
    /// without revalidation: it is the same curve as the validated input.
    pub fn reversed(&self) -> Result<Self, ContourError> {
        let segments = self.segments.iter().rev().map(|segment| segment.reversed()).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { segments, closed: self.closed })
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
    use crate::segment::Line;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

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
    fn a_document_scale_gap_is_accepted_only_with_its_own_tolerance() {
        let segments = vec![
            Segment::Line(Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("first")),
            Segment::Line(Line::new(point(10.000_5, 0.0), point(10.0, 10.0)).expect("second")),
        ];
        assert!(matches!(Contour::new(segments.clone(), false), Err(ContourError::Disconnected { .. })));
        assert!(Contour::new_with_tolerance(segments, false, 1.0e-3).is_ok());
        assert!(matches!(
            Contour::new_with_tolerance(
                vec![
                    Segment::Line(Line::new(point(0.0, 0.0), point(10.0, 0.0)).expect("first")),
                    Segment::Line(Line::new(point(11.0, 0.0), point(11.0, 10.0)).expect("second")),
                ],
                false,
                1.0e-3
            ),
            Err(ContourError::Disconnected { index: 0 })
        ));
    }

    #[test]
    fn a_joint_rule_judges_each_pair_on_its_own() {
        // The document hands the geometry layer one bound per pair: an arc endpoint is derived, a
        // line endpoint is stored exactly, and a wide arc must not loosen a line-to-line joint.
        let arc = Segment::Arc(Arc::new(DVec2::ZERO, 600.0, 0.0, 1.0).expect("arc"));
        let end = arc.end();
        let line = Segment::Line(Line::new(end + DVec2::new(0.005, 0.0), end + DVec2::new(0.005, 10.0)).expect("line"));
        let rule = |first: &Segment, _next: &Segment| {
            if matches!(first, Segment::Arc(_)) { 6.0e-3 } else { 1.0e-3 }
        };
        assert!(Contour::new_with_joint_tolerance(vec![arc, line], false, rule).is_ok());
        assert!(matches!(
            Contour::new_with_tolerance(vec![arc, line], false, 1.0e-3),
            Err(ContourError::Disconnected { index: 0 })
        ));
        assert!(matches!(
            Contour::new_with_joint_tolerance(vec![arc, line], false, |_, _| f64::NAN),
            Err(ContourError::InvalidInput(_))
        ));
    }

    #[test]
    fn closed_contour_samples_without_a_duplicate_end() {
        let contour = rounded_rectangle(100.0, 60.0, 10.0);
        let points = contour.sample(0.05).expect("samples");
        assert!(points.len() > 8);
        assert!((points.first().copied().expect("first") - points.last().copied().expect("last")).length() > 1e-9);
    }

    #[test]
    fn reversing_a_contour_flips_its_direction_and_keeps_its_geometry() {
        let contour = rounded_rectangle(100.0, 60.0, 10.0);
        let reversed = contour.reversed().expect("reversed");

        assert_eq!(reversed.is_counter_clockwise(), Some(false));
        let area = contour.signed_area().expect("area");
        let reversed_area = reversed.signed_area().expect("area");
        assert!((area + reversed_area).abs() < 1e-9, "areas {area} and {reversed_area}");
        assert_eq!(reversed.bounds(), contour.bounds());
        assert_eq!(reversed.segments().len(), contour.segments().len());
        assert!(reversed.segments().iter().any(|segment| matches!(segment, Segment::Arc(_))));

        let again = reversed.reversed().expect("reversed twice");
        assert_eq!(again.segments(), contour.segments());
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
