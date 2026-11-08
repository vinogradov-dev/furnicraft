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
    /// A point on a polygon's circumcircle between its first two corners.
    PolygonRadius,
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
    pub fn parse(self, text: &str) -> Result<GripValue, String> {
        match self {
            Self::Length => Length::parse_millimetres(text)
                .map(GripValue::Length)
                .map_err(|error: LengthParseError| format!("Not a length: {error}")),
            Self::Angle => {
                Degrees::parse_degrees(text).map(GripValue::Angle).map_err(|error| format!("Not an angle: {error}"))
            }
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
                grips.push(ContourGrip::PolygonRadius);
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
            ContourGrip::PolygonRadius => self.polygon_frame()?.radius_point().ok(),
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
            ContourGrip::PolygonRadius => Some(GripNumber::Length),
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
            | ContourGrip::PolygonRadius => Some("R"),
        }
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
            ContourGrip::PolygonRadius => Some(self.polygon_frame()?.radius),
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
                ContourGrip::ArcPoint(segment) | ContourGrip::ArcRadius(segment, _) => {
                    let moved = path.set_arc_radius_at(segment, point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::RectangleMidpoint(side) => {
                    let moved = path.move_rectangle_midpoint(usize::from(side), point)?;
                    *path = moved;
                    Ok(())
                }
                ContourGrip::PolygonRadius => {
                    let moved = path.set_polygon_radius_at(point)?;
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
            (ContourGrip::CircleCenter | ContourGrip::CircleQuadrant(_), GripValue::Length(radius)) => {
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
            (ContourGrip::PolygonRadius, GripValue::Length(radius)) => {
                let moved = path.set_polygon_radius(radius)?;
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

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("a whole millimetre fits a length")
    }

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
    fn a_polygon_shows_its_vertices_a_centre_and_a_radius_point() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("a hexagon");
        let draft = ContourDraft::Chain(polygon);
        let grips = draft.grips();
        assert_eq!(grips.iter().filter(|grip| matches!(grip, ContourGrip::Corner(_))).count(), 6);
        assert!(grips.contains(&ContourGrip::PolygonCenter));
        assert_eq!(grips.last(), Some(&ContourGrip::PolygonRadius));
        assert_eq!(draft.grip_value(ContourGrip::PolygonRadius), Some(GripValue::Length(millimetres(50))));
        let rim = draft.grip_position(ContourGrip::PolygonRadius).expect("a radius point");
        let distance = ((rim[0].micrometres() as f64).powi(2) + (rim[1].micrometres() as f64).powi(2)).sqrt();
        assert!((distance - 50_000.0).abs() <= 1.0, "on the circumcircle: {rim:?}");
    }

    #[test]
    fn a_polygon_radius_drag_scales_without_turning() {
        let polygon = Path::regular_polygon(point(0, 0), point(50, 0), 6).expect("a hexagon");
        let mut draft = ContourDraft::Chain(polygon);
        draft.drag_grip(ContourGrip::PolygonRadius, point(0, 90), CornerMove::Free).expect("drag the radius point");
        let frame = draft.as_chain().and_then(|path| path.polygon_frame().ok()).expect("still a polygon");
        assert_eq!(frame.center, point(0, 0));
        assert_eq!(frame.radius, millimetres(90));
        assert_eq!(frame.phase, Degrees::ZERO, "the first corner still points east");
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
    fn a_number_at_a_grip_parses_and_reshapes_the_figure() {
        let mut draft = circle();
        let grip = ContourGrip::CircleQuadrant(0);
        let Some(GripNumber::Length) = draft.grip_number(grip) else {
            panic!("a quadrant takes a length");
        };
        let parsed = GripNumber::Length.parse("60 mm").expect("a typed radius");
        draft.set_grip_value(grip, parsed).expect("apply the radius");
        assert_eq!(draft.grip_value(ContourGrip::CircleQuadrant(0)), Some(GripValue::Length(millimetres(60))));
        assert!(GripNumber::Length.parse("nope").is_err(), "a bad number is refused");
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
    fn a_ring_exposes_corner_grips_only() {
        let ring =
            furnicraft_model::Contour::from_polygon(vec![point(0, 0), point(100, 0), point(100, 60)]).expect("a ring");
        let draft = ContourDraft::Ring(ring);
        assert_eq!(draft.grips().len(), 3);
        assert!(draft.grips().iter().all(|grip| matches!(grip, ContourGrip::Corner(_))));
        assert_eq!(draft.grip_value(ContourGrip::CircleCenter), None);
    }
}
