//! What a tagged figure looks like around its centre, and every drag or typed value over it.
//!
//! A figure stores no dimension, only a [`PathShape`] tag (ADR 0073); a frame reads the shape back
//! out of the stored ring — a rectangle's orthonormal axes and four corners, a regular polygon's
//! circumcircle and vertices, the shared circle of a circle's four quarter arcs, one arc's centre,
//! radius and sweep. Every setter writes the ring back with the same segment ids wherever the
//! construction allows it, so banding and selection keyed by [`ContourSegmentId`] survive a drag.
//!
//! Units are the document's: whole micrometres and millidegrees in, whole micrometres and
//! millidegrees out; the millimetre trigonometry happens inside `furnicraft_geom`, and a point on a
//! circle may miss it by half a micrometre before it is rounded. A frame refuses a path whose tag
//! is not the figure asked for ([`ContourEditError::NotShaped`]), a value that cannot build the
//! figure ([`ContourEditError::InvalidValue`]) and an arc edit on a segment that is not an arc
//! ([`ContourEditError::NotAnArc`]).
//!
//! The corner machinery is `contour_edit`'s and the segment maths is `furnicraft_geom`'s: this
//! module calls `with_corner`, `geometry_arc` and `distance` from the former and computes no
//! trigonometry of its own (ADR 0040, D1).
//!
//! Changes when: the frame, the grips or a numeric edit of a tagged figure changes — not when a
//! corner of an untagged ring moves.

use furnicraft_geom::{DVec2, Segment, point_on_circle};
use furnicraft_units::{Degrees, Length};

use crate::contour::{MAX_POLYGON_SIDES, MIN_POLYGON_SIDES, point_to_geometry};
use crate::contour_edit::{ContourEditError, SUPPORT_TOLERANCE, distance, geometry_arc, with_corner};
use crate::{ContourSegment, ContourSegmentEntry, ContourSegmentId, Path, PathCorner, PathShape};

/// Minimum half-extent for a rectangle side, in millimetres.
///
/// A midpoint drag that would otherwise mirror the rectangle clamps at this value, so the total
/// extent is at least 1 mm (ADR 0034).
const MIN_RECTANGLE_HALF_EXTENT_MM: f64 = 0.5;

impl Path {
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
    pub(crate) fn move_polygon_vertex(&self, corner: PathCorner, point: [Length; 2]) -> Result<Self, ContourEditError> {
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
            corners.push(crate::contour::point_to_document(point_on_circle(center, radius, angle))?);
        }
        let edited = self.polygon_entries(&corners)?;
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
        let rebuilt = arc
            .with_radius_through(arc.start(), arc.end(), radius.millimetres_f64(), SUPPORT_TOLERANCE)
            .map_err(|_| ContourEditError::InvalidValue)?;
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
        Ok(crate::contour::point_to_document(point_on_circle(center, self.radius.millimetres_f64(), angle))?)
    }
}

impl CircleFrame {
    /// The point of the circle at `angle`, rounded to whole micrometres.
    pub fn point_at(self, angle: Degrees) -> Result<[Length; 2], ContourEditError> {
        let center = point_to_geometry(self.center)?;
        let point = point_on_circle(center, self.radius.millimetres_f64(), angle.radians_f64());
        Ok(crate::contour::point_to_document(point)?)
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
    /// The unit axis of the first side, perpendicular to the second.
    pub(crate) first_axis: DVec2,
    /// The unit axis of the second side.
    pub(crate) second_axis: DVec2,
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
        let point = point_on_circle(center, self.radius.millimetres_f64(), angle.radians_f64());
        Ok(crate::contour::point_to_document(point)?)
    }

    /// The point midway along the sweep.
    pub fn midpoint(self) -> Result<[Length; 2], ContourEditError> {
        let half = self.sweep.millidegrees() / 2;
        let millidegrees = self.start_angle.millidegrees().checked_add(half).ok_or(ContourEditError::InvalidValue)?;
        self.point_at(Degrees::from_millidegrees(millidegrees))
    }
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
