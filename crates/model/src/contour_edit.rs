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

use std::f64::consts::TAU;

use furnicraft_geom::{Arc, ContourError as GeomError, DVec2, Segment};
use furnicraft_units::{Degrees, Length};
use thiserror::Error;

use crate::contour::{MAX_POLYGON_SIDES, MIN_POLYGON_SIDES, point_to_geometry};
use crate::{
    Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, ContourSegmentId, Path, PathError,
    PathShape,
};

/// Largest distance between an edited point and the curve it must stay on, in millimetres.
///
/// The document stores whole micrometres, so a point placed on a curve can miss it by half a
/// micrometre before it is rounded. One micrometre accepts that rounding and nothing coarser.
const SUPPORT_TOLERANCE: f64 = 1.0e-3;

/// Minimum half-extent for a rectangle side, in millimetres.
///
/// A midpoint drag that would otherwise mirror the rectangle clamps at this value, so the total
/// extent is at least 1 mm (ADR 0034).
const MIN_RECTANGLE_HALF_EXTENT_MM: f64 = 0.5;

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
    /// The path is not the figure this edit needs.
    #[error("the path is not a {shape:?} figure")]
    NotShaped {
        /// The figure the edit needs.
        shape: PathShape,
    },
    /// The named segment is not an arc, so it has no radius or end angle to set.
    #[error("contour segment {segment} is not an arc")]
    NotAnArc {
        /// The segment the edit named.
        segment: ContourSegmentId,
    },
    /// A numeric value would build a curve the document cannot hold.
    #[error("the value cannot build the figure")]
    InvalidValue,
    /// The path carries no shape tag, so it has no figure-specific grip.
    #[error("the path does not remember a figure")]
    Untagged,
}

/// How a dragged corner follows the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerMove {
    /// The corner takes the pointer point wherever it goes, and an arc follows with its radius kept
    /// where it can and recalculated where it cannot.
    Free,
    /// The corner slides along the supporting line or circle of this segment, which resolves the
    /// ambiguity at a junction: the caller picks the segment once, at the drag start.
    Along(ContourSegmentId),
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
    /// Moves the corner named by `corner` to `point`, following `motion`.
    ///
    /// The corner is shared by two segments unless it is the free start or the free end of an open
    /// chain; then only the one segment moves. [`CornerMove::Along`] projects the point onto that
    /// segment's own supporting line or circle, so an arc keeps its centre and radius and reaches the
    /// corner by trimming or extending its sweep. [`CornerMove::Free`] takes the point itself: a line
    /// follows exactly and an arc keeps its radius when the other end and the point still fit a
    /// circle of that radius, and becomes a semicircle when they do not. A tagged figure keeps its
    /// own constraint instead of bending freely: a rectangle corner keeps the right angles, a circle
    /// quadrant keeps the circle, and a polygon vertex keeps every side. Every segment keeps its id
    /// where the figure's construction allows it.
    pub fn move_corner_at(
        &self,
        corner: PathCorner,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<Self, ContourEditError> {
        match motion {
            CornerMove::Free => match self.shape() {
                Some(PathShape::Rectangle) => self.move_rectangle_corner(corner, point),
                Some(PathShape::Circle) => {
                    let frame = self.circle_frame()?;
                    self.set_circle_radius(distance(frame.center, point)?)
                }
                Some(PathShape::RegularPolygon) => self.move_polygon_vertex(corner, point),
                None => self.move_corner_free(corner, point),
            },
            CornerMove::Along(segment) => {
                let point = self.nearest_on_support(segment, point)?;
                // Clamp on the motion segment so the corner cannot pass the opposite node.
                let point = self.clamp_to_segment(segment, point)?;
                let mut result = self.move_corner_on_support(corner, point)?;
                // An along-contour edit can turn a rectangle into a parallelogram; drop the tag
                // when the figure no longer fits the shape that was recorded.
                if let Some(shape) = result.shape()
                    && !shape.fits(&result)
                {
                    result = result.with_shape(None);
                }
                Ok(result)
            }
        }
    }

    /// Moves one corner along a segment so the distance from `far_end` equals `distance`.
    ///
    /// `far_end` must be one of the segment's two endpoints; the corner moves along the support
    /// from that end. For a line the distance is straight, for an arc it is arc length. This is
    /// the action a typed length in along-contour mode dispatches.
    pub fn move_corner_to_distance_from(
        &self,
        corner: PathCorner,
        motion_segment: ContourSegmentId,
        far_end: [Length; 2],
        distance: Length,
    ) -> Result<Self, ContourEditError> {
        let curve = self.segment(motion_segment).ok_or(ContourEditError::UnknownSegment { segment: motion_segment })?;
        let requested = match curve {
            ContourSegment::Line { start, end } => {
                let start_g = point_to_geometry(start)?;
                let end_g = point_to_geometry(end)?;
                let far = point_to_geometry(far_end)?;
                let direction = end_g - start_g;
                let length = direction.length();
                if length <= 0.0 {
                    return Err(ContourEditError::InvalidValue);
                }
                let unit = direction / length;
                // Clamp the typed length to the segment's own extent.
                let distance = distance.millimetres_f64().clamp(0.0, length);
                // Which way to travel from the far end: toward the other endpoint.
                let toward = if (far - start_g).length() <= (far - end_g).length() { unit } else { -unit };
                crate::contour::point_to_document(far + toward * distance)?
            }
            ContourSegment::Arc { center, radius, start_angle, sweep } => {
                let center_g = point_to_geometry(center)?;
                let radius_mm = radius.millimetres_f64();
                let start_r = start_angle.radians_f64();
                let sweep_r = sweep.radians_f64();
                let far = point_to_geometry(far_end)?;
                // Clamp the typed length to the arc's own length.
                let arc_len = sweep_r.abs() * radius_mm;
                let distance = distance.millimetres_f64().clamp(0.0, arc_len);
                // The angle of the far end on the circle.
                let far_angle = (far - center_g).y.atan2((far - center_g).x);
                // Travel from the far end toward the other endpoint: same sign as the sweep when
                // the far end is the start, opposite when it is the end.
                let start_point = DVec2::new(start_r.cos(), start_r.sin()) * radius_mm;
                let at_start = (far - (center_g + start_point)).length() < 1.0e-3;
                let travel = if at_start { sweep_r.signum() } else { -sweep_r.signum() };
                let delta = travel * distance / radius_mm.max(f64::MIN_POSITIVE);
                let angle = far_angle + delta;
                let offset = DVec2::new(angle.cos(), angle.sin()) * radius_mm;
                crate::contour::point_to_document(center_g + offset)?
            }
        };
        self.move_corner_on_support(corner, requested)
    }

    /// Moves one corner of a rectangle-tagged path, keeping the four right angles.
    ///
    /// The opposite corner stays where it is; the dragged corner defines the other end of the
    /// diagonal, so the centre and both extents follow. The axes keep their direction, so a
    /// rectangle never turns because a corner was dragged past its neighbour. The half-extents
    /// are clamped to [`MAX_CONTOUR_COORDINATE`], so a corner drag cannot leave the document range.
    pub fn move_rectangle_corner(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
        let frame = self.rectangle_frame()?;
        let index = self.corner_index(corner)? % 4;
        let opposite = match frame.corners.get((index + 2) % 4) {
            Some(corner) => *corner,
            None => return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle }),
        };
        let center = midpoint(opposite, point)?;
        let diagonal = point_to_geometry(point)? - point_to_geometry(opposite)?;
        let half_width = diagonal.dot(frame.first_axis).abs() * 0.5;
        let half_height = diagonal.dot(frame.second_axis).abs() * 0.5;
        if half_width <= 0.0 || half_height <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        // Clamp to the document range so a far drag cannot leave it.
        let max_half = crate::contour::MAX_CONTOUR_COORDINATE.millimetres_f64();
        let half_width = half_width.min(max_half);
        let half_height = half_height.min(max_half);
        self.rectangle_from_parts(center, frame.first_axis, frame.second_axis, half_width, half_height)
    }

    /// Moves one side midpoint of a rectangle-tagged path, keeping the opposite side.
    ///
    /// Sides `0` and `2` run along the first axis, so dragging their midpoint sets the second
    /// extent (the height); sides `1` and `3` set the first extent (the width). The opposite side
    /// and the axes stay where they are, so the figure grows and shrinks from that side. A drag
    /// that would cross the opposite side clamps at [`MIN_RECTANGLE_HALF_EXTENT_MM`] instead of
    /// mirroring.
    pub fn move_rectangle_midpoint(&self, side: usize, point: [Length; 2]) -> Result<Self, ContourEditError> {
        let frame = self.rectangle_frame()?;
        if side >= 4 {
            return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle });
        }
        let normal = if side.is_multiple_of(2) { frame.second_axis } else { frame.first_axis };
        let half_extent = if side.is_multiple_of(2) {
            frame.height.millimetres_f64() * 0.5
        } else {
            frame.width.millimetres_f64() * 0.5
        };
        let center = point_to_geometry(frame.center)?;
        let requested = point_to_geometry(point)?;
        // Side 0 (and 2) sits at `-half_extent` on the normal, side 1 (and 3) at `+half_extent`;
        // the opposite side is the one that stays.
        let opposite = if side.is_multiple_of(2) { half_extent } else { -half_extent };
        let moved = (requested - center).dot(normal);
        // Whether the dragged side started on the negative side of the normal.
        let starts_negative = side.is_multiple_of(2);
        let crossed = if starts_negative { moved > opposite } else { moved < opposite };
        let new_half = (moved - opposite).abs() * 0.5;
        let new_half = if crossed { MIN_RECTANGLE_HALF_EXTENT_MM } else { new_half.max(MIN_RECTANGLE_HALF_EXTENT_MM) };
        if !new_half.is_finite() || new_half <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let new_center = if crossed {
            // The dragged side crossed the opposite side: place the centre so the opposite side
            // stays where it is and the dragged side is `MIN_HALF` away.
            let clamp_from = if starts_negative {
                opposite - MIN_RECTANGLE_HALF_EXTENT_MM
            } else {
                opposite + MIN_RECTANGLE_HALF_EXTENT_MM
            };
            center + normal * (clamp_from)
        } else {
            center + normal * ((moved + opposite) * 0.5)
        };
        let new_center = crate::contour::point_to_document(new_center)?;
        let (half_width, half_height) = if side.is_multiple_of(2) {
            (frame.width.millimetres_f64() * 0.5, new_half)
        } else {
            (new_half, frame.height.millimetres_f64() * 0.5)
        };
        self.rectangle_from_parts(new_center, frame.first_axis, frame.second_axis, half_width, half_height)
    }

    /// Moves the centre of a tagged figure, keeping its size and its turn.
    pub fn move_shape_center(&self, center: [Length; 2]) -> Result<Self, ContourEditError> {
        let current = match self.shape() {
            Some(PathShape::Circle) => self.circle_frame()?.center,
            Some(PathShape::Rectangle) => self.rectangle_frame()?.center,
            Some(PathShape::RegularPolygon) => self.polygon_frame()?.center,
            None => return Err(ContourEditError::Untagged),
        };
        let offset = [
            center[0].checked_sub(current[0]).ok_or(ContourEditError::InvalidValue)?,
            center[1].checked_sub(current[1]).ok_or(ContourEditError::InvalidValue)?,
        ];
        self.translated_by(offset)
    }

    /// Moves the centre of one arc segment, keeping its radius and its angles.
    ///
    /// Both adjoining segments follow the arc's new endpoints, so the chain stays joined.
    pub fn move_arc_center(&self, segment: ContourSegmentId, center: [Length; 2]) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let ContourSegment::Arc { radius, start_angle, sweep, .. } = curve else {
            return Err(ContourEditError::NotAnArc { segment });
        };
        let moved = ContourSegment::arc(center, radius, start_angle, sweep)?;
        let start = moved.start()?;
        let end = moved.end()?;
        let mut edited = self.entries().to_vec();
        let Some(slot) = edited.get_mut(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *slot = ContourSegmentEntry::new(segment, moved);
        let previous = if index > 0 {
            Some(index - 1)
        } else if self.is_closed() && count > 1 {
            Some(count - 1)
        } else {
            None
        };
        let next = if index + 1 < count {
            Some(index + 1)
        } else if self.is_closed() && count > 1 {
            Some(0)
        } else {
            None
        };
        if let Some(previous) = previous {
            let entry = edited.get(previous).ok_or(ContourEditError::UnknownSegment { segment })?;
            let (id, curve) = (entry.id(), entry.segment());
            let fixed = with_corner(id, curve, start, true)?.0;
            let Some(slot) = edited.get_mut(previous) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            *slot = ContourSegmentEntry::new(id, fixed);
        }
        if let Some(next) = next {
            let entry = edited.get(next).ok_or(ContourEditError::UnknownSegment { segment })?;
            let (id, curve) = (entry.id(), entry.segment());
            let fixed = with_corner(id, curve, end, false)?.0;
            let Some(slot) = edited.get_mut(next) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            *slot = ContourSegmentEntry::new(id, fixed);
        }
        Ok(self.rebuild(edited)?)
    }

    /// The centre, radius and phase of a circle-tagged path.
    pub fn circle_frame(&self) -> Result<CircleFrame, ContourEditError> {
        if self.shape() != Some(PathShape::Circle) {
            return Err(ContourEditError::NotShaped { shape: PathShape::Circle });
        }
        let Some(first) = self.entries().first() else {
            return Err(ContourEditError::NotShaped { shape: PathShape::Circle });
        };
        let ContourSegment::Arc { center, radius, start_angle, .. } = first.segment() else {
            return Err(ContourEditError::NotShaped { shape: PathShape::Circle });
        };
        for entry in self.entries() {
            match entry.segment() {
                ContourSegment::Arc { center: arc_center, radius: arc_radius, .. }
                    if arc_center == center && arc_radius == radius => {}
                _ => return Err(ContourEditError::NotShaped { shape: PathShape::Circle }),
            }
        }
        Ok(CircleFrame { center, radius, phase: start_angle })
    }

    /// The centre, radius, angles and endpoints of one arc segment.
    pub fn arc_frame(&self, segment: ContourSegmentId) -> Result<ArcFrame, ContourEditError> {
        match self.segment(segment) {
            Some(ContourSegment::Arc { center, radius, start_angle, sweep }) => {
                Ok(ArcFrame { center, radius, start_angle, sweep })
            }
            Some(_) => Err(ContourEditError::NotAnArc { segment }),
            None => Err(ContourEditError::UnknownSegment { segment }),
        }
    }

    /// The index of the corner in ring order, where `Start` is its segment's position.
    fn corner_index(&self, corner: PathCorner) -> Result<usize, ContourEditError> {
        let (index, _) = self.position(corner.segment())?;
        Ok(if corner.is_end() { index + 1 } else { index })
    }

    /// The same path moved by `offset` in both coordinates.
    fn translated_by(&self, offset: [Length; 2]) -> Result<Self, ContourEditError> {
        let mut edited = Vec::with_capacity(self.entries().len());
        for entry in self.entries() {
            let moved = match entry.segment() {
                ContourSegment::Line { start, end } => {
                    ContourSegment::line(shift(start, offset)?, shift(end, offset)?)?
                }
                ContourSegment::Arc { center, radius, start_angle, sweep } => {
                    ContourSegment::arc(shift(center, offset)?, radius, start_angle, sweep)?
                }
            };
            edited.push(ContourSegmentEntry::new(entry.id(), moved));
        }
        Ok(self.rebuild(edited)?)
    }

    /// The four side entries of a rectangle, rebuilt around `center` with the given axes.
    fn rectangle_from_parts(
        &self,
        center: [Length; 2],
        first_axis: DVec2,
        second_axis: DVec2,
        half_width: f64,
        half_height: f64,
    ) -> Result<Self, ContourEditError> {
        let center = point_to_geometry(center)?;
        let corners = [
            center - first_axis * half_width - second_axis * half_height,
            center + first_axis * half_width - second_axis * half_height,
            center + first_axis * half_width + second_axis * half_height,
            center - first_axis * half_width + second_axis * half_height,
        ];
        let mut edited = Vec::with_capacity(4);
        for (index, entry) in self.entries().iter().enumerate().take(4) {
            let (Some(&start), Some(&end)) = (corners.get(index), corners.get((index + 1) % corners.len())) else {
                return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle });
            };
            let start = crate::contour::point_to_document(start)?;
            let end = crate::contour::point_to_document(end)?;
            edited.push(ContourSegmentEntry::new(entry.id(), ContourSegment::line(start, end)?));
        }
        Ok(self.rebuild(edited)?)
    }

    /// Rebuilds a regular polygon from its centre and a dragged vertex, keeping its ids.
    fn move_polygon_vertex(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
        self.position(corner.segment())?;
        let frame = self.polygon_frame()?;
        let sides = u32::try_from(self.entries().len()).map_err(|_| ContourEditError::InvalidValue)?;
        if !(MIN_POLYGON_SIDES..=MAX_POLYGON_SIDES).contains(&sides) {
            return Err(ContourEditError::InvalidValue);
        }
        let center = point_to_geometry(frame.center)?;
        let offset = point_to_geometry(point)? - center;
        let radius = offset.length();
        if !radius.is_finite() || radius <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let phase = offset.y.atan2(offset.x);
        let step = std::f64::consts::TAU / f64::from(sides);
        let mut corners = Vec::with_capacity(self.entries().len());
        for index in 0..self.entries().len() {
            let angle = phase + step * index as f64;
            corners.push(crate::contour::point_to_document(center + DVec2::new(angle.cos(), angle.sin()) * radius)?);
        }
        let edited = self.polygon_entries(&corners)?;
        Ok(self.rebuild(edited)?)
    }

    /// Clamps a point to lie on the segment between its two endpoints, so the corner cannot pass
    /// the opposite node when moving along the contour.
    ///
    /// For a line the point is projected onto the segment's axis and clamped to `[start, end]`.
    /// For an arc the angle is clamped to the sweep range, so the arc length stays within the
    /// segment's own extent.
    fn clamp_to_segment(&self, segment: ContourSegmentId, point: [Length; 2]) -> Result<[Length; 2], ContourEditError> {
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        match curve {
            ContourSegment::Line { start, end } => {
                let start = point_to_geometry(start)?;
                let end = point_to_geometry(end)?;
                let requested = point_to_geometry(point)?;
                let direction = end - start;
                let len2 = direction.length_squared();
                if len2 <= 0.0 {
                    return Err(ContourEditError::InvalidValue);
                }
                // Leave at least 1 µm (= 0.001 mm) so the segment stays valid in the draft.
                // The collapse to zero happens on Apply.
                let margin = 1.0e-3 / (end - start).length().max(f64::MIN_POSITIVE);
                let t = ((requested - start).dot(direction) / len2).clamp(margin, 1.0 - margin);
                Ok(crate::contour::point_to_document(start + direction * t)?)
            }
            ContourSegment::Arc { center, radius, start_angle, sweep } => {
                let center = point_to_geometry(center)?;
                let requested = point_to_geometry(point)?;
                let offset = requested - center;
                if offset.length() <= f64::EPSILON {
                    return Ok(point);
                }
                let angle = offset.y.atan2(offset.x);
                let start_r = start_angle.radians_f64();
                let sweep_r = sweep.radians_f64();
                let travel = if sweep_r >= 0.0 {
                    (angle - start_r).rem_euclid(std::f64::consts::TAU)
                } else {
                    -((-angle + start_r).rem_euclid(std::f64::consts::TAU))
                };
                // Same margin: leave 1 µm from the far end.
                let arc_len = (sweep_r.abs() * radius.millimetres_f64()).max(f64::MIN_POSITIVE);
                let margin = 1.0e-3 / arc_len;
                let low = 0.0_f64.min(sweep_r);
                let high = 0.0_f64.max(sweep_r);
                let clamped = travel.clamp(low + margin * sweep_r.abs(), high - margin * sweep_r.abs());
                let clamped_angle = start_r + clamped;
                let clamped_offset = DVec2::new(clamped_angle.cos(), clamped_angle.sin()) * radius.millimetres_f64();
                Ok(crate::contour::point_to_document(center + clamped_offset)?)
            }
        }
    }

    /// The point of `segment`'s own line or circle nearest to `point`.
    ///
    /// A junction offers two segments; the caller names one and the corner slides on it.
    fn nearest_on_support(
        &self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<[Length; 2], ContourEditError> {
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let requested = point_to_geometry(point)?;
        let nearest = curve.to_geom()?.nearest_on_support(requested).ok_or(ContourEditError::InvalidValue)?;
        Ok(crate::contour::point_to_document(nearest)?)
    }

    /// Moves the corner freely: the point is taken, and an adjoining arc keeps its radius when the
    /// new chord still fits it.
    fn move_corner_free(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
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
                let (moved, _) = free_corner(entry.segment(), point, corner.is_end())?;
                return self.replaced(index, ContourSegmentEntry::new(segment, moved));
            }
        };
        let (Some(before_entry), Some(after_entry)) = (entries.get(before_index), entries.get(after_index)) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let (mut before, reached) = free_corner(before_entry.segment(), point, true)?;
        let (mut after, reached) = free_corner(after_entry.segment(), reached, false)?;
        match (before, after) {
            (ContourSegment::Line { .. }, _) => {
                let start = before.start()?;
                before = ContourSegment::line(start, reached)?;
            }
            (_, ContourSegment::Line { .. }) => {
                let end = after.end()?;
                after = ContourSegment::line(reached, end)?;
            }
            _ => {}
        }
        let mut edited = entries.to_vec();
        let Some(before_slot) = edited.get_mut(before_index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *before_slot = ContourSegmentEntry::new(before_entry.id(), before);
        let Some(after_slot) = edited.get_mut(after_index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *after_slot = ContourSegmentEntry::new(after_entry.id(), after);
        self.rebuild(edited).map_err(|error| match error {
            PathError::NotJoined { .. } | PathError::NotClosed => ContourEditError::ArcJointMismatch,
            other => ContourEditError::Path(other),
        })
    }

    /// Moves the corner with the old constrained rule: an adjoining arc keeps its centre and radius.
    fn move_corner_on_support(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
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

    /// Sets the radius of a circle-tagged path, keeping its four arcs, their ids and their phase.
    pub fn set_circle_radius(&self, radius: Length) -> Result<Self, ContourEditError> {
        if self.shape() != Some(PathShape::Circle) {
            return Err(ContourEditError::NotShaped { shape: PathShape::Circle });
        }
        if !radius.is_positive() {
            return Err(ContourEditError::InvalidValue);
        }
        let mut edited = Vec::with_capacity(self.entries().len());
        for entry in self.entries() {
            let ContourSegment::Arc { center, start_angle, sweep, .. } = entry.segment() else {
                return Err(ContourEditError::NotShaped { shape: PathShape::Circle });
            };
            edited.push(ContourSegmentEntry::new(entry.id(), ContourSegment::arc(center, radius, start_angle, sweep)?));
        }
        Ok(self.rebuild(edited)?)
    }

    /// Sets a circle's radius from a rim point, which is what a quadrant drag gives.
    pub fn set_circle_radius_at(&self, rim: [Length; 2]) -> Result<Self, ContourEditError> {
        let frame = self.circle_frame()?;
        self.set_circle_radius(distance(frame.center, rim)?)
    }

    /// Sets a regular polygon's radius from a rim point, keeping its phase and its ids.
    pub fn set_polygon_radius_at(&self, rim: [Length; 2]) -> Result<Self, ContourEditError> {
        let frame = self.polygon_frame()?;
        self.set_polygon_radius(distance(frame.center, rim)?)
    }

    /// Sets one arc's radius from a point, which is what a drag on the curve gives.
    pub fn set_arc_radius_at(&self, segment: ContourSegmentId, rim: [Length; 2]) -> Result<Self, ContourEditError> {
        let frame = self.arc_frame(segment)?;
        self.set_arc_radius(segment, distance(frame.center, rim)?)
    }

    /// Sets the radius of a regular polygon from its centre, keeping its phase and its ids.
    pub fn set_polygon_radius(&self, radius: Length) -> Result<Self, ContourEditError> {
        let frame = self.polygon_frame()?;
        if !radius.is_positive() {
            return Err(ContourEditError::InvalidValue);
        }
        let scale = radius.millimetres_f64() / frame.radius.millimetres_f64();
        if !scale.is_finite() || scale <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let center = point_to_geometry(frame.center)?;
        let mut corners = Vec::with_capacity(frame.corners.len());
        for vertex in &frame.corners {
            let offset = point_to_geometry(*vertex)? - center;
            corners.push(crate::contour::point_to_document(center + offset * scale)?);
        }
        let edited = self.polygon_entries(&corners)?;
        Ok(self.rebuild(edited)?)
    }

    /// Replaces the side count of a regular polygon, keeping its centre, radius and phase.
    ///
    /// The sides are re-cut, so the ring comes back with fresh segment ids; a polygon is defined by
    /// its figure rather than by the ids of the sides that happened to build it.
    pub fn set_polygon_sides(&self, sides: u32) -> Result<Self, ContourEditError> {
        let frame = self.polygon_frame()?;
        let Some(&rim) = frame.corners.first() else {
            return Err(ContourEditError::NotShaped { shape: PathShape::RegularPolygon });
        };
        Ok(Path::regular_polygon(frame.center, rim, sides)?)
    }

    /// Sets the width of a rectangle-tagged path, keeping its centre, its axes and its ids.
    pub fn set_rectangle_width(&self, width: Length) -> Result<Self, ContourEditError> {
        if !width.is_positive() {
            return Err(ContourEditError::InvalidValue);
        }
        self.set_rectangle_extents(width, None)
    }

    /// Sets the height of a rectangle-tagged path, keeping its centre, its axes and its ids.
    pub fn set_rectangle_height(&self, height: Length) -> Result<Self, ContourEditError> {
        if !height.is_positive() {
            return Err(ContourEditError::InvalidValue);
        }
        self.set_rectangle_extents(height, Some(()))
    }

    /// Rebuilds the rectangle with one extent replaced.
    fn set_rectangle_extents(&self, value: Length, height: Option<()>) -> Result<Self, ContourEditError> {
        let frame = self.rectangle_frame()?;
        let (half_width, half_height) = match height {
            Some(()) => (frame.width.millimetres_f64() * 0.5, value.millimetres_f64() * 0.5),
            None => (value.millimetres_f64() * 0.5, frame.height.millimetres_f64() * 0.5),
        };
        self.rectangle_from_parts(frame.center, frame.first_axis, frame.second_axis, half_width, half_height)
    }

    /// Rebuilds one arc through a new midpoint, keeping both endpoints.
    ///
    /// The centre and radius are recalculated from the three points (start, new midpoint, end). A
    /// collinear midpoint or one that makes the endpoints swap is refused.
    pub fn set_arc_through_midpoint(
        &self,
        segment: ContourSegmentId,
        midpoint: [Length; 2],
    ) -> Result<Self, ContourEditError> {
        let (index, _) = self.position(segment)?;
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let ContourSegment::Arc { .. } = curve else {
            return Err(ContourEditError::NotAnArc { segment });
        };
        let start = curve.start()?;
        let end = curve.end()?;
        let rebuilt = ContourSegment::arc_through(start, midpoint, end).map_err(|_| ContourEditError::InvalidValue)?;
        let Some(entry) = self.entries().get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        // The endpoints are the same, so neighbours need no adjustment.
        self.replaced(index, ContourSegmentEntry::new(entry.id(), rebuilt))
    }

    /// Sets the radius of one arc, keeping both of its stored endpoints.
    ///
    /// The centre moves along the perpendicular bisector of the chord, to the side the arc already
    /// bulges, so the neighbours never move. A radius smaller than half the chord cannot reach both
    /// ends and is refused.
    pub fn set_arc_radius(&self, segment: ContourSegmentId, radius: Length) -> Result<Self, ContourEditError> {
        let (index, _) = self.position(segment)?;
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let ContourSegment::Arc { center, radius: _, start_angle, sweep } = curve else {
            return Err(ContourEditError::NotAnArc { segment });
        };
        if !radius.is_positive() {
            return Err(ContourEditError::InvalidValue);
        }
        let arc = geometry_arc(center, self.arc_radius(segment)?, start_angle, sweep)?;
        let rebuilt = arc_with_radius(arc, arc.start(), arc.end(), radius.millimetres_f64())?;
        let moved = ContourSegment::from_geom(Segment::Arc(rebuilt))?;
        let Some(entry) = self.entries().get(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        self.replaced(index, ContourSegmentEntry::new(entry.id(), moved))
    }

    /// The radius of the arc with this id.
    fn arc_radius(&self, segment: ContourSegmentId) -> Result<Length, ContourEditError> {
        match self.segment(segment) {
            Some(ContourSegment::Arc { radius, .. }) => Ok(radius),
            Some(_) => Err(ContourEditError::NotAnArc { segment }),
            None => Err(ContourEditError::UnknownSegment { segment }),
        }
    }

    /// Turns the end of one arc to an absolute angle, keeping its centre, its radius and its start.
    ///
    /// The next segment follows the new end; the start does not move, so the previous joint stays.
    /// The sweep keeps the direction it already had.
    pub fn set_arc_end_angle(&self, segment: ContourSegmentId, angle: Degrees) -> Result<Self, ContourEditError> {
        let (index, _) = self.position(segment)?;
        let curve = self.segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let ContourSegment::Arc { center, radius, start_angle, sweep } = curve else {
            return Err(ContourEditError::NotAnArc { segment });
        };
        let full = Degrees::from_millidegrees(360_000).millidegrees();
        let delta = (angle.millidegrees() - start_angle.millidegrees()).rem_euclid(full);
        let new_sweep = if sweep.millidegrees() >= 0 { delta } else { delta - full };
        if new_sweep == 0 || new_sweep.abs() > full {
            return Err(ContourEditError::InvalidValue);
        }
        let moved = ContourSegment::arc(center, radius, start_angle, Degrees::from_millidegrees(new_sweep))?;
        let reached = moved.end()?;
        let mut edited = self.entries().to_vec();
        let Some(entry) = edited.get_mut(index) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        *entry = ContourSegmentEntry::new(entry.id(), moved);
        let count = edited.len();
        if count > 1 {
            let next_index = (index + 1) % count;
            let (id, next_segment) = {
                let next = edited.get(next_index).ok_or(ContourEditError::UnknownSegment { segment })?;
                (next.id(), next.segment())
            };
            let (moved_next, _) = with_corner(id, next_segment, reached, false)?;
            let Some(slot) = edited.get_mut(next_index) else {
                return Err(ContourEditError::UnknownSegment { segment });
            };
            *slot = ContourSegmentEntry::new(id, moved_next);
        }
        Ok(self.rebuild(edited)?)
    }

    /// The centre, the corners, the circumradius and the phase of a regular polygon path.
    pub fn polygon_frame(&self) -> Result<PolygonFrame, ContourEditError> {
        if self.shape() != Some(PathShape::RegularPolygon) {
            return Err(ContourEditError::NotShaped { shape: PathShape::RegularPolygon });
        }
        let mut vertices = Vec::with_capacity(self.entries().len());
        for entry in self.entries() {
            vertices.push(entry.segment().start()?);
        }
        let Some(&first) = vertices.first() else {
            return Err(ContourEditError::NotShaped { shape: PathShape::RegularPolygon });
        };
        let count = i128::try_from(vertices.len()).map_err(|_| ContourEditError::InvalidValue)?;
        let mut sum = [0_i128; 2];
        for vertex in &vertices {
            sum[0] += i128::from(vertex[0].micrometres());
            sum[1] += i128::from(vertex[1].micrometres());
        }
        let center = [
            Length::from_micrometres(i64::try_from(sum[0] / count).map_err(|_| ContourEditError::InvalidValue)?),
            Length::from_micrometres(i64::try_from(sum[1] / count).map_err(|_| ContourEditError::InvalidValue)?),
        ];
        let dx = (i128::from(first[0].micrometres()) - i128::from(center[0].micrometres())) as f64;
        let dy = (i128::from(first[1].micrometres()) - i128::from(center[1].micrometres())) as f64;
        let radius = dx.hypot(dy);
        if !radius.is_finite() || radius <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let phase = Degrees::from_radians(dy.atan2(dx)).ok_or(ContourEditError::InvalidValue)?;
        Ok(PolygonFrame { center, corners: vertices, radius: Length::from_micrometres(radius.round() as i64), phase })
    }

    /// The side entries of a regular polygon through `corners`, keeping the ids of the sides.
    fn polygon_entries(&self, corners: &[[Length; 2]]) -> Result<Vec<ContourSegmentEntry>, ContourEditError> {
        let mut edited = Vec::with_capacity(self.entries().len());
        for (index, entry) in self.entries().iter().enumerate() {
            let (Some(&start), Some(&end)) = (corners.get(index), corners.get((index + 1) % corners.len())) else {
                return Err(ContourEditError::InvalidValue);
            };
            edited.push(ContourSegmentEntry::new(entry.id(), ContourSegment::line(start, end)?));
        }
        Ok(edited)
    }

    /// The centre, the four corners, the extents and the axes of a rectangle path.
    ///
    /// The axes are always orthonormal: the second axis is perpendicular to the first, so the four
    /// corners rebuilt from them form a rectangle with right angles even when the stored corners
    /// started as a parallelogram.
    pub fn rectangle_frame(&self) -> Result<RectangleFrame, ContourEditError> {
        if self.shape() != Some(PathShape::Rectangle) || self.entries().len() != 4 {
            return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle });
        }
        let mut document = [[Length::ZERO; 2]; 4];
        let mut geometry = [DVec2::ZERO; 4];
        for (index, entry) in self.entries().iter().enumerate().take(4) {
            let segment = entry.segment().to_geom()?;
            let (Some(slot), Some(point)) = (document.get_mut(index), geometry.get_mut(index)) else {
                return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle });
            };
            *slot = entry.segment().start()?;
            *point = segment.start();
        }
        let (Some(&first), Some(&second), Some(&third)) = (geometry.first(), geometry.get(1), geometry.get(2)) else {
            return Err(ContourEditError::NotShaped { shape: PathShape::Rectangle });
        };
        let u = second - first;
        let v = geometry.get(3).copied().unwrap_or_default() - first;
        let width = u.length();
        if !width.is_finite() || width <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let (first_axis, second_axis, height) = {
            let first_dir = (u / width).normalize();
            let v_len = v.length();
            if !v_len.is_finite() || v_len <= 0.0 {
                return Err(ContourEditError::InvalidValue);
            }
            let v_dir = v / v_len;
            // Orthonormalise: project out the first-axis component from v's direction.
            let perp = v_dir - first_dir * v_dir.dot(first_dir);
            let perp_len = perp.length();
            if perp_len > 1.0e-6 {
                let second_dir = perp / perp_len;
                // The height is the projection of v onto the perpendicular direction.
                let h = v.dot(second_dir).abs();
                if h.is_finite() && h > 0.0 { (first_dir, second_dir, h) } else { (first_dir, v_dir, v_len) }
            } else {
                // All four corners lie on one line: fall back to the raw vector.
                (first_dir, v_dir, v_len)
            }
        };
        if !height.is_finite() || height <= 0.0 {
            return Err(ContourEditError::InvalidValue);
        }
        let center = crate::contour::point_to_document((first + third) * 0.5)?;
        Ok(RectangleFrame {
            center,
            corners: document,
            width: Length::from_micrometres((width * 1_000.0).round() as i64),
            height: Length::from_micrometres((height * 1_000.0).round() as i64),
            first_axis,
            second_axis,
        })
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
        Ok((self.rebuild_topology(edited)?, inserted))
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
        Ok(self.rebuild_topology(edited)?)
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

    /// Moves the corner that starts `segment`, following `motion`.
    ///
    /// [`CornerMove::Along`] projects the point onto that segment's own line or circle and keeps the
    /// old rule. [`CornerMove::Free`] takes the point itself; an adjoining arc keeps its radius where
    /// the new chord fits it, so a ring can be reshaped the same way a chain can.
    pub fn move_corner_with(
        &self,
        segment: ContourSegmentId,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<Self, ContourEditError> {
        match motion {
            CornerMove::Along(target) => {
                let requested = point_to_geometry(point)?;
                let curve = self.segment(target).ok_or(ContourEditError::UnknownSegment { segment: target })?;
                let nearest = curve.to_geom()?.nearest_on_support(requested).ok_or(ContourEditError::InvalidValue)?;
                self.move_corner(segment, crate::contour::point_to_document(nearest)?)
            }
            CornerMove::Free => self.move_corner_free(segment, point),
        }
    }

    /// Moves the corner that starts `segment` to the point itself.
    fn move_corner_free(&self, segment: ContourSegmentId, point: [Length; 2]) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        let previous = (index + count - 1) % count;
        let entries = self.entries();
        let (Some(before), Some(current)) = (entries.get(previous), entries.get(index)) else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        let previous_id = before.id();
        let (mut before, reached) = free_corner(before.segment(), point, true)?;
        let (mut current, reached) = free_corner(current.segment(), reached, false)?;
        match (before, current) {
            (ContourSegment::Line { .. }, _) => {
                before = ContourSegment::line(before.start()?, reached)?;
            }
            (_, ContourSegment::Line { .. }) => {
                current = ContourSegment::line(reached, current.end()?)?;
            }
            _ => {}
        }
        let mut edited = entries.to_vec();
        edited[previous] = ContourSegmentEntry::new(previous_id, before);
        edited[index] = ContourSegmentEntry::new(segment, current);
        self.rebuild(edited).map_err(|error| match error {
            ContourError::NotClosed { .. } => ContourEditError::ArcJointMismatch,
            other => ContourEditError::Ring(other),
        })
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

    /// Removes `segment` from the ring and joins its two neighbours.
    ///
    /// A collapsed along-contour drag leaves a near-zero segment; removing it joins the previous
    /// segment's end to the next segment's start at one corner. A ring keeps at least three
    /// segments, so removing one from a triangle is refused.
    pub fn remove_segment(&self, segment: ContourSegmentId) -> Result<Self, ContourEditError> {
        let (index, count) = self.position(segment)?;
        if count <= 3 {
            return Err(ContourEditError::Ring(ContourError::TooFewSegments { count: count.saturating_sub(1) }));
        }
        let entries = self.entries();
        let previous = (index + count - 1) % count;
        let next = (index + 1) % count;
        // Keep the previous segment's end as the joined corner and pull the next start onto it.
        let corner = entries.get(previous).ok_or(ContourEditError::UnknownSegment { segment })?.segment().end()?;
        let next_entry = *entries.get(next).ok_or(ContourEditError::UnknownSegment { segment })?;
        let (moved_next, _) = with_corner(next_entry.id(), next_entry.segment(), corner, false)?;
        let mut edited = entries.to_vec();
        edited[next] = ContourSegmentEntry::new(next_entry.id(), moved_next);
        edited.remove(index);
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

/// What a regular polygon path looks like around its centre.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonFrame {
    /// The centre of the circumscribed circle, averaged from the stored vertices.
    pub center: [Length; 2],
    /// The corner each side starts on, in ring order.
    pub corners: Vec<[Length; 2]>,
    /// The distance from the centre to the first corner.
    pub radius: Length,
    /// The angle of the first corner around the centre, which fixes the figure's turn.
    pub phase: Degrees,
}

/// What a circle-tagged path looks like around its centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CircleFrame {
    /// The shared centre of the arcs.
    pub center: [Length; 2],
    /// The shared radius of the arcs.
    pub radius: Length,
    /// The start angle of the first quarter arc, which fixes where the quadrants sit.
    pub phase: Degrees,
}

impl PolygonFrame {
    /// One point on the circumcircle midway between the first two corners.
    ///
    /// The vertex handles already cover the corners, so the radius grip sits on the arc between
    /// them: dragging it scales the figure without turning it.
    pub fn radius_point(&self) -> Result<[Length; 2], ContourEditError> {
        let count = u32::try_from(self.corners.len()).map_err(|_| ContourEditError::InvalidValue)?;
        if count < MIN_POLYGON_SIDES {
            return Err(ContourEditError::NotShaped { shape: PathShape::RegularPolygon });
        }
        let step = std::f64::consts::TAU / f64::from(count);
        let angle = self.phase.radians_f64() + step * 0.5;
        let center = point_to_geometry(self.center)?;
        let offset = DVec2::new(angle.cos(), angle.sin()) * self.radius.millimetres_f64();
        Ok(crate::contour::point_to_document(center + offset)?)
    }
}

impl CircleFrame {
    /// The point of the circle at `angle`, rounded to whole micrometres.
    pub fn point_at(self, angle: Degrees) -> Result<[Length; 2], ContourEditError> {
        let center = point_to_geometry(self.center)?;
        let radians = angle.radians_f64();
        let offset = DVec2::new(radians.cos(), radians.sin()) * self.radius.millimetres_f64();
        Ok(crate::contour::point_to_document(center + offset)?)
    }
}

/// What a rectangle-tagged path looks like around its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectangleFrame {
    /// The centre of the rectangle: the midpoint of two opposite corners.
    pub center: [Length; 2],
    /// The four corners in ring order, the first one where the first side starts.
    pub corners: [[Length; 2]; 4],
    /// The side length from `corners[0]` to `corners[1]`, on the first axis.
    pub width: Length,
    /// The side length from `corners[1]` to `corners[2]`, on the second axis.
    pub height: Length,
    /// The unit axis of the first side.
    first_axis: DVec2,
    /// The unit axis of the second side.
    second_axis: DVec2,
}

/// What one arc segment looks like around its centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcFrame {
    /// The centre of the arc.
    pub center: [Length; 2],
    /// The radius of the arc.
    pub radius: Length,
    /// The angle the arc starts at.
    pub start_angle: Degrees,
    /// The signed sweep from the start to the end.
    pub sweep: Degrees,
}

impl ArcFrame {
    /// The angle the arc ends at.
    pub fn end_angle(self) -> Result<Degrees, ContourEditError> {
        let millidegrees = self
            .start_angle
            .millidegrees()
            .checked_add(self.sweep.millidegrees())
            .ok_or(ContourEditError::InvalidValue)?;
        Ok(Degrees::from_millidegrees(millidegrees))
    }

    /// The point of the arc at `angle`, rounded to whole micrometres.
    pub fn point_at(self, angle: Degrees) -> Result<[Length; 2], ContourEditError> {
        let center = point_to_geometry(self.center)?;
        let radians = angle.radians_f64();
        let offset = DVec2::new(radians.cos(), radians.sin()) * self.radius.millimetres_f64();
        Ok(crate::contour::point_to_document(center + offset)?)
    }

    /// The point midway along the sweep.
    pub fn midpoint(self) -> Result<[Length; 2], ContourEditError> {
        let half = self.sweep.millidegrees() / 2;
        let millidegrees = self.start_angle.millidegrees().checked_add(half).ok_or(ContourEditError::InvalidValue)?;
        self.point_at(Degrees::from_millidegrees(millidegrees))
    }
}

/// The same segment moved freely to `corner`, and the corner it actually reaches.
///
/// A line takes the corner exactly. An arc keeps its radius when the other end and the corner still
/// fit a circle of that radius, and falls back to a semicircle when the chord outgrows it; the arc
/// keeps the side it bulged to, so a drag does not flip the curve.
fn free_corner(
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
            let other = if at_end { arc.start() } else { arc.end() };
            let target = point_to_geometry(corner)?;
            let rebuilt = if at_end { arc_rebuilt(arc, other, target)? } else { arc_rebuilt(arc, target, other)? };
            let moved = ContourSegment::from_geom(Segment::Arc(rebuilt))?;
            let reached = if at_end { moved.end()? } else { moved.start()? };
            Ok((moved, reached))
        }
    }
}

/// An arc from `start` to `end` that keeps the radius and the bulge of `arc` where it can.
///
/// When the chord fits two circles of that radius the centre that stays on the same side as the old
/// one is used, so the curve keeps its direction. When the chord is longer than the diameter the
/// radius is recalculated as half the chord, which is the one semicircle that still reaches both
/// ends.
fn arc_rebuilt(arc: Arc, start: DVec2, end: DVec2) -> Result<Arc, ContourEditError> {
    let chord = (end - start).length();
    if !chord.is_finite() || chord < SUPPORT_TOLERANCE {
        return Err(ContourEditError::InvalidValue);
    }
    if chord <= 2.0 * arc.radius() {
        arc_with_radius(arc, start, end, arc.radius())
    } else {
        arc_with_radius(arc, start, end, chord * 0.5)
    }
}

/// An arc from `start` to `end` with a given radius, on the side `arc` already bulges to.
fn arc_with_radius(arc: Arc, start: DVec2, end: DVec2, radius: f64) -> Result<Arc, ContourEditError> {
    let chord = (end - start).length();
    if !chord.is_finite() || chord < SUPPORT_TOLERANCE || !radius.is_finite() || radius <= 0.0 {
        return Err(ContourEditError::InvalidValue);
    }
    if radius < chord * 0.5 - SUPPORT_TOLERANCE {
        return Err(ContourEditError::InvalidValue);
    }
    let midpoint = (start + end) * 0.5;
    let direction = (end - start) / chord;
    let normal = DVec2::new(-direction.y, direction.x);
    let bulge = (arc_midpoint(arc) - start).perp_dot(direction);
    let half = (radius * radius - (chord * 0.5) * (chord * 0.5)).max(0.0).sqrt();
    let toward = (arc.center() - midpoint).dot(normal);
    let center = if toward >= 0.0 { midpoint + normal * half } else { midpoint - normal * half };
    arc_between(center, radius, start, end, bulge >= 0.0)
}

/// An arc on `center` from `start` to `end` that bulges to the left of travel when `left` is set.
fn arc_between(center: DVec2, radius: f64, start: DVec2, end: DVec2, left: bool) -> Result<Arc, ContourEditError> {
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let end_angle = (end.y - center.y).atan2(end.x - center.x);
    let counter_clockwise = (end_angle - start_angle).rem_euclid(TAU);
    if !counter_clockwise.is_finite() || counter_clockwise <= 0.0 {
        return Err(ContourEditError::InvalidValue);
    }
    let forward = Arc::new(center, radius, start_angle, counter_clockwise).map_err(ContourEditError::Geometry)?;
    let bulge = (arc_midpoint(forward) - start).perp_dot(end - start);
    if (bulge >= 0.0) == left {
        return Ok(forward);
    }
    Arc::new(center, radius, start_angle, counter_clockwise - TAU).map_err(ContourEditError::Geometry)
}

/// The point halfway along an arc.
fn arc_midpoint(arc: Arc) -> DVec2 {
    let angle = arc.start_angle() + arc.sweep_angle() * 0.5;
    arc.center() + DVec2::new(angle.cos(), angle.sin()) * arc.radius()
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

/// The distance between two document points, rounded to whole micrometres.
fn distance(first: [Length; 2], second: [Length; 2]) -> Result<Length, ContourEditError> {
    let offset = point_to_geometry(second)? - point_to_geometry(first)?;
    let length = offset.length();
    if !length.is_finite() {
        return Err(ContourEditError::InvalidValue);
    }
    Ok(Length::from_micrometres((length * 1_000.0).round() as i64))
}

/// The midpoint between two document points.
fn midpoint(first: [Length; 2], second: [Length; 2]) -> Result<[Length; 2], ContourEditError> {
    let sum = point_to_geometry(first)? + point_to_geometry(second)?;
    Ok(crate::contour::point_to_document(sum * 0.5)?)
}

/// A document point moved by an integer offset.
fn shift(point: [Length; 2], offset: [Length; 2]) -> Result<[Length; 2], ContourEditError> {
    let x = point[0].checked_add(offset[0]).ok_or(ContourEditError::InvalidValue)?;
    let y = point[1].checked_add(offset[1]).ok_or(ContourEditError::InvalidValue)?;
    Ok([x, y])
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

        let moved = path
            .move_corner_at(PathCorner::Start(id(1)), point(-20, -10), CornerMove::Free)
            .expect("move the free start");
        assert_eq!(moved.segment(id(1)).expect("first").start().expect("start"), point(-20, -10));
        assert_eq!(
            moved.segment(id(2)).expect("second").start().expect("start"),
            point(100, 0),
            "the next segment keeps its start"
        );
        assert_eq!(path_ids(&moved), vec![1, 2]);

        let moved =
            path.move_corner_at(PathCorner::End(id(2)), point(120, 70), CornerMove::Free).expect("move the free end");
        assert_eq!(moved.segment(id(2)).expect("second").end().expect("end"), point(120, 70));
        assert_eq!(
            moved.segment(id(1)).expect("first").end().expect("end"),
            point(100, 0),
            "the previous segment keeps its end"
        );

        let moved = path
            .move_corner_at(PathCorner::End(id(1)), point(110, -10), CornerMove::Free)
            .expect("move the shared corner");
        assert_eq!(moved.segment(id(1)).expect("first").end().expect("end"), point(110, -10));
        assert_eq!(moved.segment(id(2)).expect("second").start().expect("start"), point(110, -10));
    }

    #[test]
    fn an_open_arc_keeps_its_circle_when_its_shared_corner_moves() {
        let path = line_and_arc();
        // 45° below the axis, on the arc's circle, rounded to whole micrometres.
        let moved = path
            .move_corner_at(PathCorner::End(id(1)), fine_point(128_284, 11_716), CornerMove::Along(id(2)))
            .expect("move the corner");
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
    fn an_along_contour_corner_slides_on_the_arc_circle() {
        let path = line_and_arc();
        // The pointer is off the circle; the corner slides to the point of the circle nearest to it
        // instead of jumping off the arc.
        let moved = path
            .move_corner_at(PathCorner::End(id(1)), point(130, 10), CornerMove::Along(id(2)))
            .expect("the corner slides on the arc");
        let (center, radius, _, _) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(center, [millimetres(100), millimetres(40)]);
        assert_eq!(radius, millimetres(40));
        assert_eq!(
            moved.segment(id(1)).expect("line").end().expect("end"),
            moved.segment(id(2)).expect("arc").start().expect("start")
        );
    }

    #[test]
    fn the_wrap_corner_of_a_closed_path_is_one_corner() {
        let ring = Path::rectangle(point(0, 0), point(100, 60)).expect("rectangle");
        let by_start =
            ring.move_corner_at(PathCorner::Start(id(1)), point(-10, -10), CornerMove::Free).expect("by start");
        let by_end = ring.move_corner_at(PathCorner::End(id(4)), point(-10, -10), CornerMove::Free).expect("by end");
        assert_eq!(path_ids(&by_start), path_ids(&by_end));
        assert_eq!(by_start.segment(id(1)).expect("bottom").start().expect("start"), point(-10, -10));
        assert_eq!(
            by_start.segment(id(4)).expect("left").end().expect("end"),
            by_end.segment(id(4)).expect("left").end().expect("end")
        );
    }

    #[test]
    fn reshaping_keeps_the_shape_tag_and_a_split_drops_it() {
        let rectangle = Path::rectangle(point(0, 0), point(100, 60)).expect("rectangle");
        let moved =
            rectangle.move_corner_at(PathCorner::End(id(1)), point(120, -10), CornerMove::Free).expect("move a corner");
        assert_eq!(moved.shape(), Some(crate::PathShape::Rectangle), "a moved corner is the same figure");
        let (split, _) = rectangle.split_segment_at(id(1), point(40, 0)).expect("split the bottom side");
        assert_eq!(split.shape(), None, "a split figure is no longer the rectangle that was drawn");
        let (restored, _) = split.split_segment_at(id(5), point(60, 0)).expect("split the new side");
        assert_eq!(restored.shape(), None, "the tag never comes back on its own");
    }

    #[test]
    fn a_free_arc_end_keeps_its_radius_while_the_chord_fits() {
        let path = line_and_arc();
        let moved =
            path.move_corner_at(PathCorner::End(id(2)), point(150, 60), CornerMove::Free).expect("a free arc end");
        let (center, radius, _, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(radius, millimetres(40), "the chord still fits a circle of the old radius");
        assert!(center[1] > millimetres(30), "the arc keeps bulging to the same side, centre {center:?}");
        assert_eq!(moved.segment(id(2)).expect("arc").end().expect("end"), point(150, 60));
        assert_eq!(moved.segment(id(1)).expect("line").end().expect("end"), point(100, 0), "the other end stays");
        assert!(sweep > 90_000, "the arc got longer, sweep {sweep}");
    }

    #[test]
    fn a_free_arc_end_recalculates_the_radius_when_the_chord_outgrows_it() {
        let path = line_and_arc();
        let moved = path
            .move_corner_at(PathCorner::End(id(2)), point(200, 0), CornerMove::Free)
            .expect("a semicircle still reaches both ends");
        let (_, radius, _, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(radius, millimetres(50), "the chord is a diameter");
        assert_eq!(sweep.abs(), 180_000, "a diameter makes a semicircle");
        assert_eq!(moved.segment(id(2)).expect("arc").end().expect("end"), point(200, 0));
    }

    #[test]
    fn a_free_arc_end_keeps_the_other_end_in_place() {
        let path = line_and_arc();
        let moved =
            path.move_corner_at(PathCorner::End(id(2)), point(180, 40), CornerMove::Free).expect("a free arc end");
        // The start of the arc (shared with the line) must not move.
        assert_eq!(moved.segment(id(2)).expect("arc").start().expect("start"), point(100, 0));
        assert_eq!(moved.segment(id(1)).expect("line").end().expect("end"), point(100, 0));
        // The free end reached the pointer.
        assert_eq!(moved.segment(id(2)).expect("arc").end().expect("end"), point(180, 40));
    }

    #[test]
    fn an_arc_midpoint_rebuilds_the_arc_through_both_endpoints() {
        let path = line_and_arc();
        let moved = path.set_arc_through_midpoint(id(2), point(140, 60)).expect("rebuild through a new midpoint");
        let (center, radius, _, _) = arc_fields(moved.segment(id(2)).expect("arc"));
        // The endpoints stayed where they were.
        assert_eq!(moved.segment(id(2)).expect("arc").start().expect("start"), point(100, 0));
        assert_eq!(moved.segment(id(2)).expect("arc").end().expect("end"), point(140, 40));
        // The centre and radius changed to pass through (140, 60).
        let start = DVec2::new(100.0, 0.0);
        let through = DVec2::new(140.0, 60.0);
        let end = DVec2::new(140.0, 40.0);
        let center_geom = DVec2::new(center[0].millimetres_f64(), center[1].millimetres_f64());
        let r = radius.millimetres_f64();
        assert!(((start - center_geom).length() - r).abs() < 1.0e-3, "start is on the circle");
        assert!(((through - center_geom).length() - r).abs() < 1.0e-3, "through is on the circle");
        assert!(((end - center_geom).length() - r).abs() < 1.0e-3, "end is on the circle");
        assert!(moved.segment(id(1)).expect("line").end().expect("end") == point(100, 0), "neighbour stays");
    }

    #[test]
    fn a_collinear_arc_midpoint_is_refused() {
        let path = line_and_arc();
        // A midpoint on the straight line between the two endpoints is collinear.
        assert_eq!(path.set_arc_through_midpoint(id(2), point(120, 20)), Err(ContourEditError::InvalidValue));
        // A midpoint at an endpoint is degenerate.
        assert_eq!(path.set_arc_through_midpoint(id(2), point(140, 40)), Err(ContourEditError::InvalidValue));
        assert_eq!(
            path.set_arc_through_midpoint(id(1), point(0, 0)),
            Err(ContourEditError::NotAnArc { segment: id(1) }),
            "a line is not an arc"
        );
    }

    #[test]
    fn the_mid_node_stays_finite_after_a_midpoint_rebuild() {
        let path = line_and_arc();
        let moved = path.set_arc_through_midpoint(id(2), point(105, 10)).expect("a small displacement");
        let (_, radius, _, _) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert!(radius.millimetres_f64().is_finite() && radius.millimetres_f64() > 0.0);
        // The endpoints are where they were (within the arc endpoint tolerance).
        let start = moved.segment(id(2)).expect("arc").start().expect("start");
        let end = moved.segment(id(2)).expect("arc").end().expect("end");
        assert!(
            (start[0].micrometres() - 100_000).abs() <= 1 && start[1].micrometres().abs() <= 1,
            "start at (100, 0): {start:?}"
        );
        assert!(
            (end[0].micrometres() - 140_000).abs() <= 1 && (end[1].micrometres() - 40_000).abs() <= 1,
            "end at (140, 40): {end:?}"
        );
    }

    #[test]
    fn a_free_arc_end_can_shorten_the_arc() {
        let path = line_and_arc();
        let moved = path
            .move_corner_at(PathCorner::End(id(2)), point(101, 1), CornerMove::Free)
            .expect("a short arc is still an arc");
        let (_, radius, _, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(radius, millimetres(40));
        assert!(sweep.abs() < 10_000, "the arc shrank towards its start, sweep {sweep}");
    }

    #[test]
    fn a_circle_radius_keeps_the_four_arcs_and_their_ids() {
        let circle = Path::circle(point(10, 20), millimetres(40)).expect("circle");
        let widened = circle.set_circle_radius(millimetres(60)).expect("a wider circle");
        assert_eq!(path_ids(&widened), path_ids(&circle));
        for entry in widened.entries() {
            let ContourSegment::Arc { center, radius, .. } = entry.segment() else {
                panic!("a circle is four arcs");
            };
            assert_eq!(center, point(10, 20));
            assert_eq!(radius, millimetres(60));
        }
        assert_eq!(widened.shape(), Some(crate::PathShape::Circle));
    }

    #[test]
    fn a_polygon_radius_scales_about_its_centre_and_keeps_its_ids() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("hexagon");
        let wider = polygon.set_polygon_radius(millimetres(100)).expect("a wider hexagon");
        assert_eq!(path_ids(&wider), path_ids(&polygon));
        assert_eq!(wider.shape(), Some(crate::PathShape::RegularPolygon));
        for entry in wider.entries() {
            let start = entry.segment().start().expect("start");
            let dx = f64::from((start[0].micrometres()) as i32);
            let dy = f64::from((start[1].micrometres()) as i32);
            assert!((dx.hypot(dy) - 100_000.0).abs() <= 2.0, "every vertex sits on the new circle");
        }
    }

    #[test]
    fn a_polygon_side_count_rebuilds_the_figure_at_the_same_radius() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("hexagon");
        let octagon = polygon.set_polygon_sides(8).expect("an octagon");
        assert_eq!(octagon.entries().len(), 8);
        assert_eq!(octagon.shape(), Some(crate::PathShape::RegularPolygon));
        let first = octagon.entries()[0].segment().start().expect("start");
        assert_eq!(first, point(50, 0), "the first vertex keeps the rim point");
    }

    #[test]
    fn a_rectangle_width_and_height_keep_the_four_sides_and_their_ids() {
        let rectangle = Path::rectangle(point(0, 0), point(100, 50)).expect("rectangle");
        let wider = rectangle.set_rectangle_width(millimetres(200)).expect("a wider rectangle");
        assert_eq!(wider.bounds_micrometres(), ([-50_000, 0], [150_000, 50_000]));
        assert_eq!(path_ids(&wider), path_ids(&rectangle));
        let taller = wider.set_rectangle_height(millimetres(100)).expect("a taller rectangle");
        assert_eq!(taller.bounds_micrometres(), ([-50_000, -25_000], [150_000, 75_000]));
        assert_eq!(taller.shape(), Some(crate::PathShape::Rectangle));
    }

    #[test]
    fn a_rectangle_corner_keeps_the_right_angles_and_pins_the_opposite_corner() {
        let rectangle = Path::rectangle(point(0, 0), point(100, 50)).expect("rectangle");
        let moved = rectangle
            .move_corner_at(PathCorner::Start(id(3)), point(120, 80), CornerMove::Free)
            .expect("move a corner freely");
        assert_eq!(moved.shape(), Some(crate::PathShape::Rectangle));
        assert_eq!(moved.bounds_micrometres(), ([0, 0], [120_000, 80_000]));
        assert_eq!(path_ids(&moved), path_ids(&rectangle), "the sides keep their ids");
        let frame = moved.rectangle_frame().expect("a rectangle frame");
        assert_eq!(frame.width, millimetres(120));
        assert_eq!(frame.height, millimetres(80));
        assert_eq!(frame.center, point(60, 40));
    }

    #[test]
    fn a_rectangle_midpoint_changes_only_its_own_extent() {
        let rectangle = Path::rectangle(point(0, 0), point(100, 50)).expect("rectangle");
        // Side 0 runs along the width, so its midpoint sets the height about the centre.
        let taller = rectangle.move_rectangle_midpoint(0, point(50, -80)).expect("a taller rectangle");
        assert_eq!(taller.bounds_micrometres(), ([0, -80_000], [100_000, 50_000]), "the top side stayed");
        // Side 1 runs along the height, so its midpoint sets the width.
        let wider = taller.move_rectangle_midpoint(1, point(150, -15)).expect("a wider rectangle");
        assert_eq!(wider.bounds_micrometres(), ([0, -80_000], [150_000, 50_000]));
        assert_eq!(wider.shape(), Some(crate::PathShape::Rectangle));
        assert!(rectangle.move_rectangle_midpoint(4, point(1, 1)).is_err(), "a fifth side is refused");
    }

    #[test]
    fn a_circle_corner_keeps_the_figure_round() {
        let circle = Path::circle(point(10, 20), millimetres(40)).expect("circle");
        let moved =
            circle.move_corner_at(PathCorner::Start(id(1)), point(90, 20), CornerMove::Free).expect("drag a quadrant");
        assert_eq!(moved.shape(), Some(crate::PathShape::Circle));
        let frame = moved.circle_frame().expect("a circle frame");
        assert_eq!(frame.center, point(10, 20), "the centre did not move");
        assert_eq!(frame.radius, millimetres(80));
        for entry in moved.entries() {
            let ContourSegment::Arc { center, radius, .. } = entry.segment() else {
                panic!("a circle is four arcs");
            };
            assert_eq!(center, point(10, 20));
            assert_eq!(radius, millimetres(80), "every quarter followed");
        }
    }

    #[test]
    fn a_polygon_corner_keeps_every_side_equal() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("hexagon");
        let moved =
            polygon.move_corner_at(PathCorner::Start(id(1)), point(0, 80), CornerMove::Free).expect("drag a vertex");
        let frame = moved.polygon_frame().expect("a polygon frame");
        assert_eq!(frame.center, point(0, 0));
        assert_eq!(frame.radius, millimetres(80));
        assert_eq!(frame.phase, Degrees::from_millidegrees(90_000));
        assert_eq!(moved.entries().len(), 6);
        let sides: Vec<f64> = moved
            .entries()
            .iter()
            .map(|entry| {
                let start = entry.segment().start().expect("start");
                let end = entry.segment().end().expect("end");
                let dx = (end[0].micrometres() - start[0].micrometres()) as f64;
                let dy = (end[1].micrometres() - start[1].micrometres()) as f64;
                dx.hypot(dy)
            })
            .collect();
        let first = sides.first().copied().unwrap_or_default();
        assert!(sides.iter().all(|side| (side - first).abs() <= 2.0), "every side stays equal: {sides:?}");
    }

    #[test]
    fn a_shape_centre_moves_the_whole_figure() {
        let circle = Path::circle(point(10, 20), millimetres(40)).expect("circle");
        let moved = circle.move_shape_center(point(110, 120)).expect("move the circle");
        let frame = moved.circle_frame().expect("a circle frame");
        assert_eq!(frame.center, point(110, 120));
        assert_eq!(frame.radius, millimetres(40));
        assert_eq!(path_ids(&moved), path_ids(&circle));

        let rectangle = Path::rectangle(point(0, 0), point(100, 50)).expect("rectangle");
        let moved = rectangle.move_shape_center(point(10, 10)).expect("move the rectangle");
        assert_eq!(moved.bounds_micrometres(), ([-40_000, -15_000], [60_000, 35_000]));

        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("hexagon");
        let moved = polygon.move_shape_center(point(-20, 30)).expect("move the hexagon");
        assert_eq!(moved.polygon_frame().expect("a polygon frame").center, point(-20, 30));

        let untagged = open_path(vec![point(0, 0), point(100, 0)]);
        assert_eq!(untagged.move_shape_center(point(1, 1)), Err(ContourEditError::Untagged));
    }

    #[test]
    fn an_arc_centre_moves_the_curve_and_drags_its_neighbours() {
        let path = line_and_arc();
        let moved = path.move_arc_center(id(2), point(120, 40)).expect("move the arc centre");
        let (center, radius, _, sweep) = arc_fields(moved.segment(id(2)).expect("arc"));
        assert_eq!(center, point(120, 40));
        assert_eq!(radius, millimetres(40));
        assert_eq!(sweep, 90_000, "the angle is kept");
        assert_eq!(moved.segment(id(1)).expect("line").end().expect("end"), point(120, 0));
        assert_eq!(moved.segment(id(2)).expect("arc").end().expect("end"), point(160, 40));
        assert_eq!(path.move_arc_center(id(1), point(0, 0)), Err(ContourEditError::NotAnArc { segment: id(1) }));
    }

    #[test]
    fn a_circle_frame_places_its_quadrants_and_an_arc_its_midpoint() {
        let circle = Path::circle(point(10, 20), millimetres(40)).expect("circle");
        let frame = circle.circle_frame().expect("a circle frame");
        assert_eq!(frame.point_at(Degrees::ZERO).expect("east"), point(50, 20));
        assert_eq!(frame.point_at(Degrees::from_millidegrees(90_000)).expect("north"), point(10, 60));

        let arc = line_and_arc();
        let frame = arc.arc_frame(id(2)).expect("an arc frame");
        assert_eq!(frame.end_angle().expect("end"), Degrees::from_millidegrees(0));
        let midpoint = frame.midpoint().expect("midpoint");
        assert!((midpoint[0].micrometres() - 128_284).abs() <= 1, "{midpoint:?}");
        assert!((midpoint[1].micrometres() - 11_716).abs() <= 1, "{midpoint:?}");
        assert!(arc.arc_frame(id(1)).is_err(), "a line is not an arc");
    }

    #[test]
    fn an_arc_radius_keeps_both_ends_and_the_neighbours() {
        let path = line_and_arc();
        let wider = path.set_arc_radius(id(2), millimetres(60)).expect("a wider arc");
        let arc = wider.segment(id(2)).expect("arc");
        assert_eq!(arc_fields(arc).1, millimetres(60));
        let near = |first: [Length; 2], second: [Length; 2]| {
            first.iter().zip(second.iter()).all(|(a, b)| (a.micrometres() - b.micrometres()).abs() <= 1)
        };
        assert!(near(arc.start().expect("start"), point(100, 0)), "the start is pinned");
        assert!(near(arc.end().expect("end"), point(140, 40)), "the end is pinned");
        assert_eq!(wider.segment(id(1)).expect("line").end().expect("end"), point(100, 0));
        assert_eq!(path.set_arc_radius(id(1), millimetres(10)), Err(ContourEditError::NotAnArc { segment: id(1) }));
    }

    #[test]
    fn an_arc_end_angle_turns_the_end_and_drags_the_next_segment() {
        let path = line_and_arc();
        // The arc's start is at -90 degrees; +180 degrees is the far side of its circle.
        let turned = path.set_arc_end_angle(id(2), Degrees::from_millidegrees(90_000)).expect("a half turn");
        let arc = turned.segment(id(2)).expect("arc");
        let (center, radius, _, sweep) = arc_fields(arc);
        assert_eq!(center, [millimetres(100), millimetres(40)]);
        assert_eq!(radius, millimetres(40));
        assert_eq!(sweep, 180_000);
        assert_eq!(arc.end().expect("end"), point(100, 80));
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

    #[test]
    fn a_rotated_rectangle_keeps_right_angles_through_a_midpoint_drag() {
        // A rectangle at 45 degrees: corners at (0,0), (70,70), (0,140), (-70,70).
        let corners = vec![
            ContourSegment::line(point(0, 0), point(70, 70)).expect("side 0"),
            ContourSegment::line(point(70, 70), point(0, 140)).expect("side 1"),
            ContourSegment::line(point(0, 140), point(-70, 70)).expect("side 2"),
            ContourSegment::line(point(-70, 70), point(0, 0)).expect("side 3"),
        ];
        let rect = Path::new(corners, true).expect("a diamond rectangle").with_shape(Some(crate::PathShape::Rectangle));
        // Drag the midpoint of side 0 (the bottom edge).
        let dragged = rect.move_rectangle_midpoint(0, point(0, -40)).expect("drag the midpoint");
        let frame = dragged.rectangle_frame().expect("still a rectangle");
        // The axes must be perpendicular.
        let dot = frame.first_axis.dot(frame.second_axis).abs();
        assert!(dot < 1.0e-6, "axes should be perpendicular: dot={dot}");
        // The shape tag is still Rectangle.
        assert_eq!(dragged.shape(), Some(crate::PathShape::Rectangle));
    }

    #[test]
    fn a_midpoint_drag_never_yields_zero() {
        let rect = Path::rectangle(point(0, 0), point(100, 50)).expect("a rectangle");
        // Drag the bottom midpoint to the opposite side — it should clamp, not mirror or collapse.
        let clamped = rect.move_rectangle_midpoint(0, point(50, 60)).expect("clamped at min extent");
        let frame = clamped.rectangle_frame().expect("still a rectangle");
        // Total extent must be at least 1 mm (0.5 mm half).
        assert!(
            frame.height.millimetres_f64() >= 1.0,
            "height should be at least 1 mm, got {}",
            frame.height.millimetres_f64()
        );
        let bounds = clamped.bounds_micrometres();
        // Height = 1 mm: y = [49 mm, 50 mm]. Width unchanged: x = [0, 100 mm].
        assert_eq!(bounds, ([0, 49_000], [100_000, 50_000]), "bounds should show 1 mm height: {bounds:?}");
    }

    #[test]
    fn a_corner_drag_cannot_leave_the_range() {
        let rect = Path::rectangle(point(0, 0), point(100, 50)).expect("a rectangle");
        // Drag a corner to the maximum coordinate; the rectangle should stay inside the range.
        let limit = crate::contour::MAX_CONTOUR_COORDINATE;
        let clamped = rect.move_rectangle_corner(PathCorner::Start(id(3)), [limit, limit]).expect("clamped corner");
        let frame = clamped.rectangle_frame().expect("still a rectangle");
        // Every corner must be inside the document range.
        for corner in &frame.corners {
            for value in corner {
                assert!(
                    value.micrometres().unsigned_abs() <= limit.micrometres() as u64,
                    "corner {corner:?} is outside the range"
                );
            }
        }
    }

    #[test]
    fn an_along_contour_edit_drops_the_shape_tag() {
        let rect = Path::rectangle(point(0, 0), point(100, 50)).expect("a rectangle");
        // Slide the bottom-left corner along the bottom edge (segment id 1) past the bottom-right
        // corner, turning the rectangle into a non-rectangular shape.
        let moved = rect
            .move_corner_at(PathCorner::Start(id(1)), point(150, 0), CornerMove::Along(id(1)))
            .expect("slide along the bottom");
        // The tag must be dropped because the figure is no longer a rectangle.
        assert_eq!(moved.shape(), None, "the rectangle tag was dropped");
    }

    #[test]
    fn a_non_right_angled_tagged_rectangle_is_a_load_error() {
        // Four lines that do not form right angles: the "rectangle" tag is a lie.
        let json = r#"{
            "entries":[
                {"id":1,"kind":"line","start":[0,0],"end":[100000,0]},
                {"id":2,"kind":"line","start":[100000,0],"end":[110000,60000]},
                {"id":3,"kind":"line","start":[110000,60000],"end":[0,60000]},
                {"id":4,"kind":"line","start":[0,60000],"end":[0,0]}
            ],
            "next_segment_id":5,
            "closed":true,
            "shape":"rectangle"
        }"#;
        let result = serde_json::from_str::<Path>(json);
        assert!(result.is_err(), "a non-right-angled rectangle must be rejected on load: {result:?}");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("rectangle") || error.contains("shape"), "error mentions the shape: {error}");
    }

    #[test]
    fn an_along_contour_drag_cannot_pass_the_opposite_node() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60)]);
        // Drag the shared corner far past the far end of the first segment.
        let moved = path
            .move_corner_at(PathCorner::End(id(1)), point(250, 0), CornerMove::Along(id(1)))
            .expect("clamped at the opposite node");
        let end = moved.segment(id(1)).expect("first").end().expect("end");
        // The corner must stay inside the segment, short of the far end by the 1 µm margin.
        assert!(end[0].micrometres() <= 100_000, "the corner did not pass the far end: {end:?}");
        assert!(end[0].micrometres() >= 99_000, "the corner reached the far end: {end:?}");
    }

    #[test]
    fn a_typed_length_moves_the_corner_from_the_opposite_end() {
        let path = open_path(vec![point(0, 0), point(100, 0), point(100, 60)]);
        // The corner at the end of the first segment; the far end is the start (0, 0).
        let moved = path
            .move_corner_to_distance_from(PathCorner::End(id(1)), id(1), point(0, 0), millimetres(30))
            .expect("move 30 mm from the far start");
        let end = moved.segment(id(1)).expect("first").end().expect("end");
        assert_eq!(end, point(30, 0), "the corner sits 30 mm from the far start");
    }

    #[test]
    fn a_typed_arc_length_moves_the_corner_along_the_arc() {
        let path = line_and_arc();
        // The arc has radius 40 and a 90° sweep; half its arc length is 40·π/4 ≈ 31.416 mm.
        let arc_id = id(2);
        let half = Length::from_micrometres(31_416);
        let moved = path
            .move_corner_to_distance_from(PathCorner::End(arc_id), arc_id, point(100, 0), half)
            .expect("half the arc length");
        let end = moved.segment(arc_id).expect("arc").end().expect("end");
        // Half of a 90° arc from the start is 45°; the end is at (100 + 40·cos(-45°), 40 + 40·sin(-45°)).
        let expected_x = 100.0 + 40.0 * std::f64::consts::FRAC_1_SQRT_2;
        let expected_y = 40.0 - 40.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((end[0].millimetres_f64() - expected_x).abs() < 0.5, "end x {end:?}");
        assert!((end[1].millimetres_f64() - expected_y).abs() < 0.5, "end y {end:?}");
    }
}
