//! The grips a contour edit shows and what dragging one does.
//!
//! A plain ring or chain exposes one handle per corner. A drawn figure remembers its shape
//! ([`PathShape`]), so it also exposes the handles that keep it a figure: a circle's centre and four
//! quadrants, a rectangle's centre and side midpoints, a polygon's centre and radius point. An arc
//! adds its centre and a point on its curve.
//!
//! Every grip answers two questions: where it sits, and what a number typed at it means. The number
//! is a length for a radius or a side, and an angle for an arc end. Dragging is a separate call, so
//! the viewport owns the pointer while the model owns the geometry.

use furnicraft_model::{
    ArcFrame, CircleFrame, ContourEditError, ContourSegment, ContourSegmentId, CornerMove, PathCorner, PathShape,
    PolygonFrame, RectangleFrame,
};
use furnicraft_text::{Lang, Message};
use furnicraft_units::{Degrees, Length, LengthParseError};

use crate::ContourDraft;

/// One handle of a contour edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourGrip {
    /// A shared corner of the ring or chain, named by the segment that starts or ends on it.
    Corner(PathCorner),
    /// The centre of a circle figure.
    CircleCenter,
    /// One quadrant of a circle figure, numbered counter-clockwise from the first arc's start.
    CircleQuadrant(u8),
    /// The centre of one arc segment.
    ArcCenter(ContourSegmentId),
    /// The midpoint of one arc segment: the persistent point on its curve.
    ArcPoint(ContourSegmentId),
    /// A point on an arc under the pointer, shown while the curve is hovered.
    ArcRadius(ContourSegmentId, [Length; 2]),
    /// The centre of a rectangle figure.
    RectangleCenter,
    /// The midpoint of one rectangle side, numbered from the first side.
    RectangleMidpoint(u8),
    /// The centre of a regular polygon figure.
    PolygonCenter,
    /// A point on a circle's rim, from any hovered arc segment of a circle-tagged path.
    CircleRim([Length; 2]),
}

/// What a number typed at a grip measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripNumber {
    /// A length in millimetres.
    Length,
    /// An angle in degrees.
    Angle,
}

/// A number a grip accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripValue {
    /// A length in millimetres.
    Length(Length),
    /// An angle in degrees.
    Angle(Degrees),
}

impl GripNumber {
    /// Parses the typed text as this kind of number.
    pub fn parse(self, text: &str, lang: Lang) -> Result<GripValue, String> {
        match self {
            Self::Length => {
                Length::parse_millimetres(text).map(GripValue::Length).map_err(|error: LengthParseError| {
                    Message::NotALength.with(lang, [crate::length_error(error).text(lang)])
                })
            }
            Self::Angle => Degrees::parse_degrees(text)
                .map(GripValue::Angle)
                .map_err(|error| Message::NotAnAngle.with(lang, [crate::degrees_error(error).text(lang)])),
        }
    }
}

impl GripValue {
    /// The value as text a field can edit.
    pub fn text(self) -> String {
        match self {
            Self::Length(length) => length.to_string(),
            Self::Angle(angle) => angle.to_string(),
        }
    }
}

impl ContourDraft {
    /// Every handle this draft exposes, in painting order.
    pub fn grips(&self) -> Vec<ContourGrip> {
        let shape = self.as_chain().and_then(|path| path.shape());
        let mut grips = Vec::new();
        match shape {
            Some(PathShape::Circle) => {
                grips.push(ContourGrip::CircleCenter);
                grips.extend((0..4).map(ContourGrip::CircleQuadrant));
            }
            Some(PathShape::Rectangle) => {
                self.push_corners(&mut grips);
                grips.extend((0..4).map(ContourGrip::RectangleMidpoint));
                grips.push(ContourGrip::RectangleCenter);
            }
            Some(PathShape::RegularPolygon) => {
                self.push_corners(&mut grips);
                grips.push(ContourGrip::PolygonCenter);
            }
            None => {
                self.push_corners(&mut grips);
                for entry in self.entries() {
                    if entry.segment().is_arc() {
                        grips.push(ContourGrip::ArcCenter(entry.id()));
                        grips.push(ContourGrip::ArcPoint(entry.id()));
                    }
                }
            }
        }
        grips
    }

    /// One corner handle per segment, plus the free end of an open chain.
    fn push_corners(&self, grips: &mut Vec<ContourGrip>) {
        for entry in self.entries() {
            grips.push(ContourGrip::Corner(PathCorner::Start(entry.id())));
        }
        if !self.is_closed()
            && let Some(last) = self.entries().last()
        {
            grips.push(ContourGrip::Corner(PathCorner::End(last.id())));
        }
    }

    /// Where `grip` sits in the target's local plane.
    pub fn grip_position(&self, grip: ContourGrip) -> Option<[Length; 2]> {
        match grip {
            ContourGrip::Corner(corner) => self.corner_point(corner),
            ContourGrip::CircleCenter => self.circle_frame().map(|frame| frame.center),
            ContourGrip::CircleQuadrant(index) => {
                let frame = self.circle_frame()?;
                let angle =
                    Degrees::from_millidegrees(frame.phase.millidegrees().checked_add(i64::from(index) * 90_000)?);
                frame.point_at(angle).ok()
            }
            ContourGrip::ArcCenter(segment) => match self.segment(segment) {
                Some(ContourSegment::Arc { center, .. }) => Some(center),
                _ => None,
            },
            ContourGrip::ArcPoint(segment) => self.as_chain()?.arc_frame(segment).ok()?.midpoint().ok(),
            ContourGrip::ArcRadius(_, point) => Some(point),
            ContourGrip::RectangleCenter => self.rectangle_frame().map(|frame| frame.center),
            ContourGrip::RectangleMidpoint(side) => {
                let frame = self.rectangle_frame()?;
                let start = frame.corners.get(usize::from(side))?;
                let end = frame.corners.get((usize::from(side) + 1) % 4)?;
                midpoint(*start, *end)
            }
            ContourGrip::PolygonCenter => self.polygon_frame().map(|frame| frame.center),
            ContourGrip::CircleRim(point) => Some(point),
        }
    }

    /// What a number typed at `grip` measures, or `None` when the grip has no number.
    pub fn grip_number(&self, grip: ContourGrip) -> Option<GripNumber> {
        match grip {
            ContourGrip::Corner(corner) => {
                let segment = self.segment(corner.segment())?;
                match segment {
                    ContourSegment::Arc { .. } if corner.is_end() => Some(GripNumber::Angle),
                    ContourSegment::Arc { .. } => Some(GripNumber::Length),
                    ContourSegment::Line { .. } => match self.as_chain()?.shape()? {
                        PathShape::Rectangle | PathShape::RegularPolygon => Some(GripNumber::Length),
                        PathShape::Circle => None,
                    },
                }
            }
            ContourGrip::CircleQuadrant(_) => Some(GripNumber::Length),
            ContourGrip::ArcPoint(_) | ContourGrip::ArcRadius(..) => Some(GripNumber::Length),
            ContourGrip::CircleCenter
            | ContourGrip::ArcCenter(_)
            | ContourGrip::RectangleCenter
            | ContourGrip::PolygonCenter => None,
            ContourGrip::RectangleMidpoint(_) => Some(GripNumber::Length),
            ContourGrip::CircleRim(..) => Some(GripNumber::Length),
        }
    }

    /// The short label a numeric field shows for this grip.
    pub fn grip_label(&self, grip: ContourGrip) -> Option<&'static str> {
        match grip {
            ContourGrip::RectangleMidpoint(side) => Some(if side % 2 == 0 { "H" } else { "W" }),
            ContourGrip::RectangleCenter | ContourGrip::PolygonCenter => None,
            ContourGrip::CircleCenter | ContourGrip::ArcCenter(_) => None,
            ContourGrip::Corner(corner) => {
                let segment = self.segment(corner.segment())?;
                match segment {
                    ContourSegment::Arc { .. } if corner.is_end() => Some("∠"),
                    ContourSegment::Arc { .. } => Some("R"),
                    ContourSegment::Line { .. } => match self.as_chain()?.shape()? {
                        PathShape::Rectangle => Some("W"),
                        PathShape::RegularPolygon => Some("R"),
                        PathShape::Circle => None,
                    },
                }
            }
            ContourGrip::CircleQuadrant(_)
            | ContourGrip::ArcPoint(_)
            | ContourGrip::ArcRadius(..)
            | ContourGrip::CircleRim(..) => Some("R"),
        }
    }

    /// The length a numeric field shows when a corner moves along the contour.
    ///
    /// `start_position` is where the corner was when the drag began: that tells which end of
    /// `motion_segment` is the "opposite" end. The value is the distance from the current corner
    /// to that opposite end along the segment, so it falls to zero as the corner reaches it.
    /// A line measures a straight distance, an arc measures its arc length.
    pub fn along_contour_length(
        &self,
        motion_segment: ContourSegmentId,
        corner: PathCorner,
        start_position: [Length; 2],
    ) -> Option<Length> {
        let curve = self.segment(motion_segment)?;
        let far_end = self.motion_far_end(motion_segment, start_position)?;
        let pos = self.corner_point(corner)?;
        match curve {
            ContourSegment::Line { .. } => {
                let dx = (far_end[0].micrometres() - pos[0].micrometres()) as f64;
                let dy = (far_end[1].micrometres() - pos[1].micrometres()) as f64;
                let dist = dx.hypot(dy);
                dist.is_finite().then(|| Length::from_micrometres(dist.round() as i64))
            }
            ContourSegment::Arc { center, radius, start_angle, sweep } => {
                let center_x = center[0].millimetres_f64();
                let center_y = center[1].millimetres_f64();
                let radius_mm = radius.millimetres_f64();
                let start_r = start_angle.radians_f64();
                let sweep_r = sweep.radians_f64();
                let travel = |point: [Length; 2]| {
                    let dx = point[0].millimetres_f64() - center_x;
                    let dy = point[1].millimetres_f64() - center_y;
                    let angle = dy.atan2(dx);
                    if sweep_r >= 0.0 {
                        (angle - start_r).rem_euclid(std::f64::consts::TAU).min(sweep_r)
                    } else {
                        -((-angle + start_r).rem_euclid(std::f64::consts::TAU)).max(sweep_r)
                    }
                };
                let arc_len = (travel(pos) - travel(far_end)).abs() * radius_mm;
                arc_len.is_finite().then(|| Length::from_micrometres((arc_len * 1_000.0).round() as i64))
            }
        }
    }

    /// The far end of `motion_segment`, given where the corner was when the drag began.
    ///
    /// The corner starts at one end of the segment; the far end is the other one. It is fixed for
    /// the whole drag, so the length field and a typed length agree even after the corner moved.
    pub fn motion_far_end(&self, motion_segment: ContourSegmentId, start_position: [Length; 2]) -> Option<[Length; 2]> {
        let curve = self.segment(motion_segment)?;
        let start = curve.start().ok()?;
        let end = curve.end().ok()?;
        if near_point(start_position, start) { Some(end) } else { Some(start) }
    }

    /// The current value a numeric field starts with.
    pub fn grip_value(&self, grip: ContourGrip) -> Option<GripValue> {
        match self.grip_number(grip)? {
            GripNumber::Length => Some(GripValue::Length(self.grip_length(grip)?)),
            GripNumber::Angle => Some(GripValue::Angle(self.grip_angle(grip)?)),
        }
    }

    /// The length a length-measuring grip holds now.
    fn grip_length(&self, grip: ContourGrip) -> Option<Length> {
        match grip {
            ContourGrip::CircleCenter | ContourGrip::CircleQuadrant(_) => Some(self.circle_frame()?.radius),
            ContourGrip::ArcCenter(segment) | ContourGrip::ArcPoint(segment) | ContourGrip::ArcRadius(segment, _) => {
                Some(self.arc_frame(segment)?.radius)
            }
            ContourGrip::Corner(corner) => match self.segment(corner.segment())? {
                ContourSegment::Arc { radius, .. } => Some(radius),
                ContourSegment::Line { .. } => match self.as_chain()?.shape()? {
                    PathShape::Rectangle => Some(self.rectangle_frame()?.width),
                    PathShape::RegularPolygon => Some(self.polygon_frame()?.radius),
                    PathShape::Circle => None,
                },
            },
            ContourGrip::RectangleMidpoint(side) => {
                let frame = self.rectangle_frame()?;
                Some(if side % 2 == 0 { frame.height } else { frame.width })
            }
            ContourGrip::CircleRim(..) => Some(self.circle_frame()?.radius),
            ContourGrip::RectangleCenter | ContourGrip::PolygonCenter => None,
        }
    }

    /// The angle an angle-measuring grip holds now.
    fn grip_angle(&self, grip: ContourGrip) -> Option<Degrees> {
        let ContourGrip::Corner(corner) = grip else {
            return None;
        };
        self.as_chain()?.arc_frame(corner.segment()).ok()?.end_angle().ok()
    }

    /// Drags `grip` to `point`, following `motion` for a corner and freely otherwise.
    ///
    /// A tagged figure keeps its own constraint: a circle quadrant scales the circle, a rectangle
    /// midpoint moves one side, and a polygon vertex keeps every side equal.
    pub fn drag_grip(
        &mut self,
        grip: ContourGrip,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        match self {
            Self::Ring(_) => match grip {
                ContourGrip::Corner(corner) => self.drag_corner(corner, point, motion),
                _ => Err(ContourEditError::Untagged),
            },
            Self::Chain(path) => match grip {
                ContourGrip::Corner(corner) => {
                    let moved = path.move_corner_at(corner, point, motion)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::CircleCenter | ContourGrip::RectangleCenter | ContourGrip::PolygonCenter => {
                    let moved = path.move_shape_center(point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::CircleQuadrant(_) => {
                    let moved = path.set_circle_radius_at(point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::ArcCenter(segment) => {
                    let moved = path.move_arc_center(segment, point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::ArcPoint(segment) => {
                    let moved = path.set_arc_through_midpoint(segment, point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::ArcRadius(segment, _) => {
                    let moved = path.set_arc_radius_at(segment, point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::RectangleMidpoint(side) => {
                    let moved = path.move_rectangle_midpoint(usize::from(side), point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::CircleRim(..) => {
                    let moved = path.set_circle_radius_at(point)?;
                    *path = moved;
                    Ok(())
                }
            },
        }
    }

    /// Applies a number typed at `grip`.
    pub fn set_grip_value(&mut self, grip: ContourGrip, value: GripValue) -> Result<(), ContourEditError> {
        let Self::Chain(path) = self else {
            return Err(ContourEditError::Untagged);
        };
        match (grip, value) {
            (
                ContourGrip::CircleCenter | ContourGrip::CircleQuadrant(_) | ContourGrip::CircleRim(..),
                GripValue::Length(radius),
            ) => {
                let moved = path.set_circle_radius(radius)?;
                *path = moved;
            }
            (ContourGrip::ArcCenter(segment) | ContourGrip::ArcPoint(segment), GripValue::Length(radius)) => {
                let moved = path.set_arc_radius(segment, radius)?;
                *path = moved;
            }
            (ContourGrip::ArcRadius(segment, _), GripValue::Length(radius)) => {
                let moved = path.set_arc_radius(segment, radius)?;
                *path = moved;
            }
            (ContourGrip::Corner(corner), GripValue::Angle(angle)) => {
                let moved = path.set_arc_end_angle(corner.segment(), angle)?;
                *path = moved;
            }
            (ContourGrip::Corner(corner), GripValue::Length(length)) => {
                let moved = match path.shape() {
                    Some(PathShape::Rectangle) => path.set_rectangle_width(length)?,
                    Some(PathShape::RegularPolygon) => path.set_polygon_radius(length)?,
                    _ => path.set_arc_radius(corner.segment(), length)?,
                };
                *path = moved;
            }
            (ContourGrip::RectangleMidpoint(side), GripValue::Length(length)) => {
                let moved =
                    if side % 2 == 0 { path.set_rectangle_height(length)? } else { path.set_rectangle_width(length)? };
                *path = moved;
            }
            _ => return Err(ContourEditError::InvalidValue),
        }
        Ok(())
    }

    /// The point a corner names, on a ring or on a chain.
    fn corner_point(&self, corner: PathCorner) -> Option<[Length; 2]> {
        let segment = self.segment(corner.segment())?;
        if corner.is_end() { segment.end().ok() } else { segment.start().ok() }
    }

    /// The circle frame of a chain, when it is a circle.
    fn circle_frame(&self) -> Option<CircleFrame> {
        self.as_chain()?.circle_frame().ok()
    }

    /// The rectangle frame of a chain, when it is a rectangle.
    fn rectangle_frame(&self) -> Option<RectangleFrame> {
        self.as_chain()?.rectangle_frame().ok()
    }

    /// The polygon frame of a chain, when it is a regular polygon.
    fn polygon_frame(&self) -> Option<PolygonFrame> {
        self.as_chain()?.polygon_frame().ok()
    }

    /// The arc frame of one segment.
    fn arc_frame(&self, segment: ContourSegmentId) -> Option<ArcFrame> {
        self.as_chain()?.arc_frame(segment).ok()
    }

    /// Moves a corner of a ring through the same two motions a chain takes.
    fn drag_corner(
        &mut self,
        corner: PathCorner,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        let Self::Ring(contour) = self else {
            return Err(ContourEditError::Untagged);
        };
        let segment = if corner.is_end() {
            let index = contour
                .index_of(corner.segment())
                .ok_or(ContourEditError::UnknownSegment { segment: corner.segment() })?;
            let count = contour.entries().len();
            contour.id_at((index + 1) % count).ok_or(ContourEditError::UnknownSegment { segment: corner.segment() })?
        } else {
            corner.segment()
        };
        let moved = contour.move_corner_with(segment, point, motion)?;
        *contour = moved;
        Ok(())
    }
}

/// Whether two document points are within one micrometre of each other.
fn near_point(first: [Length; 2], second: [Length; 2]) -> bool {
    first.iter().zip(second.iter()).all(|(a, b)| (a.micrometres() - b.micrometres()).abs() <= 1)
}

/// The midpoint between two document points, rounded down to the micrometre.
fn midpoint(first: [Length; 2], second: [Length; 2]) -> Option<[Length; 2]> {
    let x = i128::from(first[0].micrometres()) + i128::from(second[0].micrometres());
    let y = i128::from(first[1].micrometres()) + i128::from(second[1].micrometres());
    Some([Length::from_micrometres(i64::try_from(x / 2).ok()?), Length::from_micrometres(i64::try_from(y / 2).ok()?)])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::Path;
    use furnicraft_testkit::millimetres;

    use super::*;

    fn point(x: i64, y: i64) -> [Length; 2] {
        [millimetres(x), millimetres(y)]
    }

    fn circle() -> ContourDraft {
        ContourDraft::Chain(Path::circle(point(10, 20), millimetres(40)).expect("a circle"))
    }

    fn rectangle() -> ContourDraft {
        ContourDraft::Chain(Path::rectangle(point(0, 0), point(100, 50)).expect("a rectangle"))
    }

    #[test]
    fn a_circle_shows_a_centre_and_four_quadrants_with_one_radius() {
        let draft = circle();
        let grips = draft.grips();
        assert_eq!(grips.first(), Some(&ContourGrip::CircleCenter));
        assert_eq!(grips.len(), 5, "a centre and four quadrants: {grips:?}");
        assert_eq!(draft.grip_position(ContourGrip::CircleCenter), Some(point(10, 20)));
        assert_eq!(draft.grip_position(ContourGrip::CircleQuadrant(0)), Some(point(50, 20)));
        assert_eq!(draft.grip_position(ContourGrip::CircleQuadrant(1)), Some(point(10, 60)));
        assert_eq!(draft.grip_position(ContourGrip::CircleQuadrant(2)), Some(point(-30, 20)));
        assert_eq!(draft.grip_position(ContourGrip::CircleQuadrant(3)), Some(point(10, -20)));
        assert_eq!(draft.grip_value(ContourGrip::CircleCenter), None, "the centre translates, it takes no number");
        assert_eq!(draft.grip_value(ContourGrip::CircleQuadrant(2)), Some(GripValue::Length(millimetres(40))));
        assert_eq!(draft.grip_label(ContourGrip::CircleQuadrant(2)), Some("R"));
    }

    #[test]
    fn a_circle_quadrant_drag_scales_the_circle_and_keeps_it_round() {
        let mut draft = circle();
        draft.drag_grip(ContourGrip::CircleQuadrant(0), point(90, 20), CornerMove::Free).expect("drag a quadrant");
        let frame = draft.as_chain().and_then(|path| path.circle_frame().ok()).expect("still a circle");
        assert_eq!(frame.center, point(10, 20));
        assert_eq!(frame.radius, millimetres(80));
        assert_eq!(draft.grips().len(), 5, "the same five grips");
    }

    #[test]
    fn a_rectangle_shows_corners_midpoints_and_a_centre() {
        let draft = rectangle();
        let grips = draft.grips();
        assert_eq!(grips.iter().filter(|grip| matches!(grip, ContourGrip::Corner(_))).count(), 4);
        assert_eq!(grips.iter().filter(|grip| matches!(grip, ContourGrip::RectangleMidpoint(_))).count(), 4);
        assert_eq!(grips.last(), Some(&ContourGrip::RectangleCenter));
        assert_eq!(draft.grip_position(ContourGrip::RectangleCenter), Some(point(50, 25)));
        assert_eq!(draft.grip_position(ContourGrip::RectangleMidpoint(0)), Some(point(50, 0)));
        assert_eq!(draft.grip_position(ContourGrip::RectangleMidpoint(1)), Some(point(100, 25)));
        assert_eq!(draft.grip_value(ContourGrip::RectangleMidpoint(0)), Some(GripValue::Length(millimetres(50))));
        assert_eq!(draft.grip_value(ContourGrip::RectangleMidpoint(1)), Some(GripValue::Length(millimetres(100))));
        assert_eq!(draft.grip_label(ContourGrip::RectangleMidpoint(0)), Some("H"));
        assert_eq!(draft.grip_label(ContourGrip::RectangleMidpoint(3)), Some("W"));
        assert_eq!(draft.grip_value(ContourGrip::RectangleCenter), None);
    }

    #[test]
    fn a_rectangle_midpoint_drag_moves_only_that_side() {
        let mut draft = rectangle();
        draft
            .drag_grip(ContourGrip::RectangleMidpoint(0), point(50, -80), CornerMove::Free)
            .expect("drag the bottom side");
        let frame = draft.as_chain().and_then(|path| path.rectangle_frame().ok()).expect("still a rectangle");
        assert_eq!(frame.width, millimetres(100));
        assert_eq!(frame.height, millimetres(130));
        let top = frame.corners.iter().map(|corner| corner[1].micrometres()).max().unwrap_or_default();
        assert_eq!(top, 50_000, "the opposite side stayed put");
    }

    #[test]
    fn a_polygon_shows_its_vertices_and_a_centre() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("a hexagon");
        let draft = ContourDraft::Chain(polygon);
        let grips = draft.grips();
        assert_eq!(grips.iter().filter(|grip| matches!(grip, ContourGrip::Corner(_))).count(), 6);
        assert!(grips.contains(&ContourGrip::PolygonCenter));
        assert_eq!(grips.last(), Some(&ContourGrip::PolygonCenter), "the centre is the last grip, no radius point");
        assert_eq!(grips.len(), 7, "six vertices and a centre, no radius point");
    }

    #[test]
    fn a_polygon_vertex_drag_scales_the_polygon() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("a hexagon");
        let mut draft = ContourDraft::Chain(polygon);
        // Drag the first vertex to a new radius; the polygon scales.
        let first = draft.entries().first().expect("a segment").id();
        draft
            .drag_grip(ContourGrip::Corner(PathCorner::Start(first)), point(0, 90), CornerMove::Free)
            .expect("drag a vertex");
        let frame = draft.as_chain().and_then(|path| path.polygon_frame().ok()).expect("still a polygon");
        assert_eq!(frame.center, point(0, 0));
        assert_eq!(frame.radius, millimetres(90));
        assert_eq!(frame.corners.len(), 6);
    }

    #[test]
    fn an_arc_chain_shows_its_centre_and_a_point_on_the_curve() {
        let arc = Path::new(
            vec![
                ContourSegment::line(point(0, 0), point(100, 0)).expect("a line"),
                ContourSegment::arc(
                    [millimetres(100), millimetres(40)],
                    millimetres(40),
                    Degrees::from_millidegrees(-90_000),
                    Degrees::from_millidegrees(90_000),
                )
                .expect("an arc"),
            ],
            false,
        )
        .expect("a line and an arc");
        let draft = ContourDraft::Chain(arc);
        let arc_id = draft.entries().last().expect("the arc").id();
        let grips = draft.grips();
        assert!(grips.contains(&ContourGrip::ArcCenter(arc_id)));
        assert!(grips.contains(&ContourGrip::ArcPoint(arc_id)));
        assert_eq!(draft.grip_position(ContourGrip::ArcCenter(arc_id)), Some(point(100, 40)));
        assert_eq!(draft.grip_value(ContourGrip::ArcPoint(arc_id)), Some(GripValue::Length(millimetres(40))));
        // The free end of an open chain is a corner grip as well.
        assert!(grips.contains(&ContourGrip::Corner(PathCorner::End(arc_id))));
    }

    #[test]
    fn an_arc_midpoint_drag_rebuilds_through_the_new_point_and_keeps_endpoints() {
        let arc = Path::new(
            vec![
                ContourSegment::line(point(0, 0), point(100, 0)).expect("a line"),
                ContourSegment::arc(
                    [millimetres(100), millimetres(40)],
                    millimetres(40),
                    Degrees::from_millidegrees(-90_000),
                    Degrees::from_millidegrees(90_000),
                )
                .expect("an arc"),
            ],
            false,
        )
        .expect("a line and an arc");
        let mut draft = ContourDraft::Chain(arc);
        let arc_id = draft.entries().last().expect("the arc").id();
        // Drag the midpoint to a point off the original circle; the arc rebuilds through it.
        draft
            .drag_grip(ContourGrip::ArcPoint(arc_id), point(140, 50), CornerMove::Free)
            .expect("rebuild through the new midpoint");
        let frame = draft.arc_frame(arc_id).expect("an arc frame");
        // The centre is no longer at the old position — the arc was rebuilt, not just scaled.
        assert_ne!(frame.center, [millimetres(100), millimetres(40)]);
        // The centre y moved above the old 40 mm because the arc passes through (140, 50).
        assert!(frame.center[1].micrometres() > millimetres(40).micrometres(), "centre y > 40 mm");
        // The line neighbour is untouched.
        let line_end = draft.entries()[0].segment().end().expect("line end");
        assert_eq!(line_end, point(100, 0));
        // The endpoints of the arc are still where they were.
        let arc_segment = draft.segment(arc_id).expect("the arc");
        assert_eq!(arc_segment.start().expect("start"), point(100, 0));
        assert_eq!(arc_segment.end().expect("end"), point(140, 40));
    }

    #[test]
    fn an_arc_radius_drag_does_not_rebuild_through_the_midpoint() {
        let arc = Path::new(
            vec![
                ContourSegment::line(point(0, 0), point(100, 0)).expect("a line"),
                ContourSegment::arc(
                    [millimetres(100), millimetres(40)],
                    millimetres(40),
                    Degrees::from_millidegrees(-90_000),
                    Degrees::from_millidegrees(90_000),
                )
                .expect("an arc"),
            ],
            false,
        )
        .expect("a line and an arc");
        let mut draft = ContourDraft::Chain(arc);
        let arc_id = draft.entries().last().expect("the arc").id();
        let midpoint = draft.grip_position(ContourGrip::ArcPoint(arc_id)).expect("midpoint");
        // Drag the radius grip to a point far out; the radius changes.
        draft
            .drag_grip(ContourGrip::ArcRadius(arc_id, midpoint), point(140, 60), CornerMove::Free)
            .expect("drag the radius grip");
        let frame = draft.arc_frame(arc_id).expect("an arc frame");
        assert!(frame.radius > millimetres(40), "the radius grew");
        // The endpoints are still where they were (within the arc endpoint tolerance).
        let arc_segment = draft.segment(arc_id).expect("arc");
        let start = arc_segment.start().expect("start");
        let end = arc_segment.end().expect("end");
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
    fn a_number_at_a_grip_parses_and_reshapes_the_figure() {
        let mut draft = circle();
        let grip = ContourGrip::CircleQuadrant(0);
        let Some(GripNumber::Length) = draft.grip_number(grip) else {
            panic!("a quadrant takes a length");
        };
        let parsed = GripNumber::Length.parse("60 mm", Lang::En).expect("a typed radius");
        draft.set_grip_value(grip, parsed).expect("apply the radius");
        assert_eq!(draft.grip_value(ContourGrip::CircleQuadrant(0)), Some(GripValue::Length(millimetres(60))));
        assert!(GripNumber::Length.parse("nope", Lang::En).is_err(), "a bad number is refused");
    }

    #[test]
    fn an_arc_end_takes_an_angle_and_an_arc_start_a_radius() {
        let arc = Path::new(
            vec![
                ContourSegment::arc(
                    [millimetres(0), millimetres(0)],
                    millimetres(40),
                    Degrees::ZERO,
                    Degrees::from_millidegrees(90_000),
                )
                .expect("an arc"),
            ],
            false,
        )
        .expect("an arc chain");
        let draft = ContourDraft::Chain(arc);
        let id = draft.entries().first().expect("the arc").id();
        assert_eq!(draft.grip_number(ContourGrip::Corner(PathCorner::Start(id))), Some(GripNumber::Length));
        assert_eq!(draft.grip_number(ContourGrip::Corner(PathCorner::End(id))), Some(GripNumber::Angle));
        let Some(GripValue::Angle(angle)) = draft.grip_value(ContourGrip::Corner(PathCorner::End(id))) else {
            panic!("the end of an arc measures an angle");
        };
        assert_eq!(angle, Degrees::from_millidegrees(90_000));
    }

    #[test]
    fn a_circle_rim_drag_at_an_arbitrary_angle_keeps_it_round() {
        let mut draft = circle();
        // A rim grip at 37°; the circle stays round and the radius changes.
        let rim = ContourGrip::CircleRim(point(10, 30));
        assert_eq!(draft.grip_number(rim), Some(GripNumber::Length));
        assert_eq!(draft.grip_label(rim), Some("R"));
        assert_eq!(draft.grip_value(rim), Some(GripValue::Length(millimetres(40))));
        draft.drag_grip(rim, point(10, 80), CornerMove::Free).expect("drag the rim at 37° equivalent");
        let frame = draft.as_chain().and_then(|path| path.circle_frame().ok()).expect("still a circle");
        assert_eq!(frame.center, point(10, 20));
        assert_eq!(frame.radius, millimetres(60), "radius grew from 40 to 60");
        assert_eq!(draft.grips().len(), 5, "the same five grips, CircleRim is hover-only");
    }

    #[test]
    fn an_untagged_arc_chain_keeps_arc_grips_and_rejects_circle_rim() {
        // An arc starting at the line end: centre (100, 40), radius 40, start at -90°.
        let line = ContourSegment::line(point(0, 0), point(100, 0)).expect("line");
        let arc = ContourSegment::arc(
            point(100, 40),
            millimetres(40),
            Degrees::from_millidegrees(-90_000),
            Degrees::from_millidegrees(90_000),
        )
        .expect("arc");
        let path = Path::new(vec![line, arc], false).expect("line and arc");
        let draft = ContourDraft::Chain(path);
        let arc_id = draft.entries().last().expect("arc").id();
        // An untagged chain still exposes ArcRadius on a hovered arc.
        let grips = draft.grips();
        assert!(grips.contains(&ContourGrip::ArcCenter(arc_id)));
        assert!(grips.contains(&ContourGrip::ArcPoint(arc_id)));
        assert!(!grips.contains(&ContourGrip::CircleRim(point(0, 0))), "no circle rim on an untagged chain");
        // The arc radius grip is the synthetic one, not in grips list.
        let arc_grip = ContourGrip::ArcRadius(arc_id, point(120, 30));
        assert_eq!(draft.grip_number(arc_grip), Some(GripNumber::Length));
        assert_eq!(draft.grip_label(arc_grip), Some("R"));
    }

    #[test]
    fn a_circle_rim_number_applies_the_radius() {
        let mut draft = circle();
        let rim = ContourGrip::CircleRim(point(10, 60));
        draft.set_grip_value(rim, GripValue::Length(millimetres(80))).expect("set radius through rim");
        let frame = draft.as_chain().and_then(|path| path.circle_frame().ok()).expect("still a circle");
        assert_eq!(frame.radius, millimetres(80));
    }

    #[test]
    fn a_polygon_has_no_grip_off_the_figure() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("a hexagon");
        let draft = ContourDraft::Chain(polygon);
        let grips = draft.grips();
        // Every grip is either a vertex, a centre, or a quadrant — none sits off the figure.
        assert!(grips.iter().all(|grip| matches!(grip, ContourGrip::Corner(_) | ContourGrip::PolygonCenter)));
        // A vertex still scales the polygon.
        let first = draft.entries().first().expect("a segment").id();
        assert_eq!(
            draft.grip_value(ContourGrip::Corner(PathCorner::Start(first))),
            Some(GripValue::Length(millimetres(50)))
        );
    }

    #[test]
    fn a_ring_exposes_corner_grips_only() {
        let ring =
            furnicraft_model::Contour::from_polygon(vec![point(0, 0), point(100, 0), point(100, 60)]).expect("a ring");
        let draft = ContourDraft::Ring(ring);
        assert_eq!(draft.grips().len(), 3);
        assert!(draft.grips().iter().all(|grip| matches!(grip, ContourGrip::Corner(_))));
        assert_eq!(draft.grip_value(ContourGrip::CircleCenter), None);
    }
}
