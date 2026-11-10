//! The frame a node is born in: which point of its content becomes its local zero.
//!
//! A frame is chosen once, at creation. The content is moved so the chosen anchor sits at the node's
//! own origin, the placement says where that origin is in the parent frame, the rotation is kept and
//! the pivot starts at zero. The rule is one function, [`rebase_at_anchor`]; a content kind supplies
//! only its anchor for a strategy and a shifted copy of itself ([`Anchored`]). Nothing here moves
//! anything on its own: the creation commands call the rule (ADR 0082).

use furnicraft_geom::DVec3;
use furnicraft_units::Length;
use thiserror::Error;

use crate::placement::lengths_from_millimetres;
use crate::tree::panel_bounds;
use crate::{Contour, ContourError, Node, Path, PathError, Pivot, Placement, PlacementError, RectCorner};

/// The content point a newly created node puts at its local zero.
///
/// A frame is chosen only at creation: an edit, a move or a turn never recomputes it (ADR 0082).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameAnchor {
    /// The content's own creation pivot: an open chain's first point, a closed ring's bounds centre.
    CreationPivot,
    /// A corner of the content's bounds, min-X/min-Y by default.
    ///
    /// The corner is a parameter because a panel may be anchored at any of its four bounds corners
    /// while a rectangle keeps the base corner.
    BBoxBaseCorner(RectCorner),
    /// The centre of a collection of children's union bounds, the anchor of a group.
    CenterOfBounds,
}

impl FrameAnchor {
    /// The base corner: min-X/min-Y of the content's bounds.
    pub const BASE_CORNER: Self = Self::BBoxBaseCorner(RectCorner::BottomLeft);

    /// The bounds corner this strategy asks for, or `None` for another strategy.
    pub const fn corner(self) -> Option<RectCorner> {
        match self {
            Self::BBoxBaseCorner(corner) => Some(corner),
            Self::CreationPivot | Self::CenterOfBounds => None,
        }
    }
}

/// Content inside a node, seen by the frame rule.
///
/// The trait exists so the rule is written once while a path, a contour and a container's children
/// keep their own units and their own errors. An implementation answers two questions: where the
/// strategy's anchor is in the content's own frame, and what the content looks like moved by an
/// offset of that frame.
pub trait Anchored: Sized {
    /// The anchor this strategy names, in the content's own frame.
    fn anchor(&self, strategy: FrameAnchor) -> Result<[Length; 3], FrameError>;

    /// The same content moved by `offset`, in its own frame.
    ///
    /// A flat content refuses a non-zero Z offset instead of silently ignoring it, so a caller
    /// cannot lose a third coordinate it meant to keep.
    fn shifted(&self, offset: [Length; 3]) -> Result<Self, FrameError>;
}

/// A content moved to its anchor, with the frame that now describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rebased<T> {
    /// The content, translated so its anchor sits at the local origin.
    pub content: T,
    /// The placement of the new origin in the parent frame, with the rotation it already had.
    pub placement: Placement,
    /// The pivot of the new node: always the local origin.
    pub pivot: Pivot,
}

/// Moves `content` so `strategy`'s anchor becomes the node's local zero.
///
/// The rotation is kept and the translation becomes the parent-frame position of the anchor, so
/// `placement.transform_point(anchor)` before the call equals `placement.transform_point(ZERO)`
/// after it and no world point moves. The pivot starts at zero: the local zero is what the gizmo
/// turns about, and a creation never invents a second handle (ADR 0082).
pub fn rebase_at_anchor<T: Anchored>(
    content: T,
    placement: Placement,
    strategy: FrameAnchor,
) -> Result<Rebased<T>, FrameError> {
    let anchor = content.anchor(strategy)?;
    let translation = placement.transform_point(anchor_millimetres(anchor));
    let placement = Placement::new(translation, placement.rotation())?;
    let content = content.shifted(negated(anchor)?)?;
    Ok(Rebased { content, placement, pivot: Pivot::ZERO })
}

/// A micrometre point as the millimetres geometry and placements work in.
fn anchor_millimetres(point: [Length; 3]) -> DVec3 {
    DVec3::new(point[0].millimetres_f64(), point[1].millimetres_f64(), point[2].millimetres_f64())
}

/// The opposite of an anchor or an offset; an anchor at `i64::MIN` is an error, not a wrap.
fn negated(offset: [Length; 3]) -> Result<[Length; 3], FrameError> {
    let flip =
        |value: Length| value.micrometres().checked_neg().map(Length::from_micrometres).ok_or(FrameError::OutOfRange);
    Ok([flip(offset[0])?, flip(offset[1])?, flip(offset[2])?])
}

/// The named corner of a flat pair of extents, on the drawing plane.
fn flat_corner(corner: RectCorner, min: [i64; 2], max: [i64; 2]) -> [Length; 3] {
    let (x, y) = flat_corner_micrometres(corner, min, max);
    [Length::from_micrometres(x), Length::from_micrometres(y), Length::ZERO]
}

/// The centre of a flat pair of extents, summed in `i128` so it cannot overflow.
fn flat_center(min: [i64; 2], max: [i64; 2]) -> [Length; 3] {
    fn middle(low: i64, high: i64) -> Length {
        let average = (i128::from(low) + i128::from(high)) / 2;
        Length::from_micrometres(i64::try_from(average).unwrap_or(low))
    }
    [middle(min[0], max[0]), middle(min[1], max[1]), Length::ZERO]
}

/// The two micrometre coordinates of a bounds corner.
fn flat_corner_micrometres(corner: RectCorner, min: [i64; 2], max: [i64; 2]) -> (i64, i64) {
    match corner {
        RectCorner::BottomLeft => (min[0], min[1]),
        RectCorner::BottomRight => (max[0], min[1]),
        RectCorner::TopRight => (max[0], max[1]),
        RectCorner::TopLeft => (min[0], max[1]),
    }
}

/// The named corner of a millimetre box, on its base plane (`min.z`).
fn box_corner(corner: RectCorner, min: DVec3, max: DVec3) -> DVec3 {
    let (x, y) = match corner {
        RectCorner::BottomLeft => (min.x, min.y),
        RectCorner::BottomRight => (max.x, min.y),
        RectCorner::TopRight => (max.x, max.y),
        RectCorner::TopLeft => (min.x, max.y),
    };
    DVec3::new(x, y, min.z)
}

impl Anchored for Path {
    fn anchor(&self, strategy: FrameAnchor) -> Result<[Length; 3], FrameError> {
        match strategy {
            FrameAnchor::CreationPivot => Ok(self.creation_pivot()?.components()),
            FrameAnchor::BBoxBaseCorner(corner) => {
                let (min, max) = self.bounds_micrometres();
                Ok(flat_corner(corner, min, max))
            }
            FrameAnchor::CenterOfBounds => {
                let (min, max) = self.bounds_micrometres();
                Ok(flat_center(min, max))
            }
        }
    }

    fn shifted(&self, offset: [Length; 3]) -> Result<Self, FrameError> {
        let [x, y, z] = offset;
        if !z.is_zero() {
            return Err(FrameError::OffPlane { micrometres: z.micrometres() });
        }
        Ok(self.translated([x, y])?)
    }
}

impl Anchored for Contour {
    fn anchor(&self, strategy: FrameAnchor) -> Result<[Length; 3], FrameError> {
        match strategy {
            FrameAnchor::CreationPivot | FrameAnchor::CenterOfBounds => {
                let (min, max) = self.bounds_micrometres();
                Ok(flat_center(min, max))
            }
            FrameAnchor::BBoxBaseCorner(corner) => {
                let (min, max) = self.bounds_micrometres();
                Ok(flat_corner(corner, min, max))
            }
        }
    }

    fn shifted(&self, offset: [Length; 3]) -> Result<Self, FrameError> {
        let [x, y, z] = offset;
        if !z.is_zero() {
            return Err(FrameError::OffPlane { micrometres: z.micrometres() });
        }
        Ok(self.translated([x, y])?)
    }
}

impl Anchored for Vec<Node> {
    fn anchor(&self, strategy: FrameAnchor) -> Result<[Length; 3], FrameError> {
        let Some((min, max)) = panel_bounds(self, Placement::IDENTITY, 0) else {
            return Err(FrameError::NoBounds);
        };
        let anchor = match strategy {
            FrameAnchor::CreationPivot => return Err(FrameError::NoCreationPivot),
            FrameAnchor::BBoxBaseCorner(corner) => box_corner(corner, min, max),
            FrameAnchor::CenterOfBounds => (min + max) * 0.5,
        };
        lengths_from_millimetres(anchor).ok_or(FrameError::OutOfRange)
    }

    fn shifted(&self, offset: [Length; 3]) -> Result<Self, FrameError> {
        let delta = anchor_millimetres(offset);
        let mut children = self.clone();
        for child in &mut children {
            let placement = child.placement();
            child.set_placement(Placement::new(placement.translation() + delta, placement.rotation())?);
        }
        Ok(children)
    }
}

/// Why a frame cannot be chosen for a content.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum FrameError {
    /// The content has no bounds, so no strategy has a point to anchor on.
    #[error("the content has no bounds to anchor a frame on")]
    NoBounds,
    /// A collection of children has no creation pivot of its own.
    #[error("a container's children have no creation pivot; use the bounds centre or a bounds corner")]
    NoCreationPivot,
    /// The content is flat and cannot move along Z.
    #[error("flat content cannot move along Z by {micrometres} µm")]
    OffPlane {
        /// The refused Z offset.
        micrometres: i64,
    },
    /// An anchor, an offset or the resulting translation is outside the representable range.
    #[error("the anchor is outside the representable range")]
    OutOfRange,
    /// The moved path is not a valid chain.
    #[error(transparent)]
    Path(#[from] PathError),
    /// The moved ring is not a valid contour.
    #[error(transparent)]
    Contour(#[from] ContourError),
    /// The resulting placement is not a finite rigid frame.
    #[error(transparent)]
    Placement(#[from] PlacementError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{box_mesh, millimetres, sample_panel};
    use crate::{ContourSegment, ContourSegmentId, Geometry, Id, Object};
    use furnicraft_geom::DQuat;
    use furnicraft_units::Degrees;

    /// The world points of a path's segment ends, in millimetres.
    fn path_world_points(path: &Path, placement: Placement) -> Vec<DVec3> {
        let mut points = Vec::new();
        for entry in path.entries() {
            let segment = entry.segment();
            for point in [segment.start(), segment.end()].into_iter().flatten() {
                points.push(placement.transform_point(DVec3::new(
                    point[0].millimetres_f64(),
                    point[1].millimetres_f64(),
                    0.0,
                )));
            }
        }
        points
    }

    /// The id of the ring's arc.
    fn arc_of(ring: &Contour) -> ContourSegmentId {
        ring.entries()
            .iter()
            .find(|entry| entry.segment().is_arc())
            .map(|entry| entry.id())
            .expect("the ring has an arc")
    }

    fn arc_fields(ring: &Contour, id: ContourSegmentId) -> ([Length; 2], Length, Degrees, Degrees) {
        let Some(ContourSegment::Arc { center, radius, start_angle, sweep }) = ring.segment(id) else {
            panic!("the segment with id {id} is an arc");
        };
        (center, radius, start_angle, sweep)
    }

    #[test]
    fn an_open_chain_is_born_at_its_first_point() {
        let line = Path::polyline(
            vec![
                [millimetres(600), millimetres(400)],
                [millimetres(700), millimetres(400)],
                [millimetres(700), millimetres(500)],
            ],
            false,
        )
        .expect("a polyline");
        let before = path_world_points(&line, Placement::IDENTITY);

        let rebased = rebase_at_anchor(line, Placement::IDENTITY, FrameAnchor::CreationPivot).expect("rebase a line");

        assert_eq!(rebased.placement.translation(), DVec3::new(600.0, 400.0, 0.0));
        assert_eq!(rebased.placement.rotation(), DQuat::IDENTITY);
        assert!(rebased.pivot.is_zero(), "a creation leaves the pivot at the local zero");
        assert_eq!(rebased.content.bounds_micrometres().0, [0, 0], "the first point is the local zero");
        let after = path_world_points(&rebased.content, rebased.placement);
        assert_eq!(before.len(), after.len());
        for (before, after) in before.iter().zip(&after) {
            assert!((*before - *after).length() < 1e-3, "{before:?} moved to {after:?}");
        }
    }

    #[test]
    fn a_closed_ring_is_born_at_its_bounds_centre() {
        let rectangle = Path::rectangle([millimetres(600), millimetres(400)], [millimetres(700), millimetres(500)])
            .expect("a rectangle");
        let before = path_world_points(&rectangle, Placement::IDENTITY);

        let rebased =
            rebase_at_anchor(rectangle, Placement::IDENTITY, FrameAnchor::CreationPivot).expect("rebase a ring");

        assert_eq!(rebased.placement.translation(), DVec3::new(650.0, 450.0, 0.0));
        assert!(rebased.pivot.is_zero());
        let (min, max) = rebased.content.bounds_micrometres();
        assert_eq!((min, max), ([-50_000, -50_000], [50_000, 50_000]), "the ring straddles its own zero");
        let after = path_world_points(&rebased.content, rebased.placement);
        for (before, after) in before.iter().zip(&after) {
            assert!((*before - *after).length() < 1e-3, "{before:?} moved to {after:?}");
        }
    }

    #[test]
    fn a_ring_anchored_on_a_turned_parent_keeps_its_world_shape() {
        let rectangle = Path::rectangle([millimetres(600), millimetres(400)], [millimetres(700), millimetres(500)])
            .expect("a rectangle");
        let parent = Placement::new(DVec3::new(-20.0, 75.0, 3.0), DQuat::from_rotation_z(0.75)).expect("parent");
        let before = path_world_points(&rectangle, parent);

        let rebased = rebase_at_anchor(rectangle, parent, FrameAnchor::BASE_CORNER).expect("rebase in place");

        assert_eq!(rebased.placement.rotation(), parent.rotation(), "the rotation is kept");
        assert_eq!(rebased.placement.translation(), parent.transform_point(DVec3::new(600.0, 400.0, 0.0)));
        assert_eq!(rebased.content.bounds_micrometres().0, [0, 0]);
        let after = path_world_points(&rebased.content, rebased.placement);
        for (before, after) in before.iter().zip(&after) {
            assert!((*before - *after).length() < 1e-3, "{before:?} moved to {after:?}");
        }
    }

    #[test]
    fn a_rebased_contour_keeps_its_ids_centres_and_angles() {
        let ring = Contour::new(vec![
            ContourSegment::line([millimetres(300), millimetres(200)], [millimetres(400), millimetres(200)])
                .expect("a line"),
            ContourSegment::arc(
                [millimetres(300), millimetres(200)],
                millimetres(100),
                Degrees::from_millidegrees(0),
                Degrees::from_millidegrees(90_000),
            )
            .expect("a quarter arc"),
            ContourSegment::line([millimetres(300), millimetres(300)], [millimetres(300), millimetres(200)])
                .expect("a line"),
        ])
        .expect("a closed ring");
        let arc = arc_of(&ring);
        let (_, radius, start_angle, sweep) = arc_fields(&ring, arc);

        let rebased = rebase_at_anchor(ring, Placement::IDENTITY, FrameAnchor::BASE_CORNER).expect("rebase a ring");

        assert_eq!(rebased.placement.translation(), DVec3::new(300.0, 200.0, 0.0));
        assert!(rebased.pivot.is_zero());
        assert_eq!(arc_of(&rebased.content), arc, "the segment ids travel with their segments");
        let (moved_center, moved_radius, moved_angle, moved_sweep) = arc_fields(&rebased.content, arc);
        assert_eq!(moved_center, [Length::ZERO, Length::ZERO], "the centre moved with the ring");
        assert_eq!((moved_radius, moved_angle, moved_sweep), (radius, start_angle, sweep));
    }

    #[test]
    fn the_base_corner_is_min_x_min_y_and_a_named_corner_is_honoured() {
        let path = Path::rectangle([millimetres(20), millimetres(10)], [millimetres(120), millimetres(60)])
            .expect("a rectangle");

        assert_eq!(
            path.anchor(FrameAnchor::BASE_CORNER).expect("the base corner"),
            [millimetres(20), millimetres(10), Length::ZERO]
        );
        assert_eq!(
            path.anchor(FrameAnchor::BBoxBaseCorner(RectCorner::TopRight)).expect("the top right corner"),
            [millimetres(120), millimetres(60), Length::ZERO]
        );
        assert_eq!(FrameAnchor::BASE_CORNER.corner(), Some(RectCorner::BottomLeft));
        assert_eq!(FrameAnchor::CreationPivot.corner(), None);
    }

    #[test]
    fn the_centre_of_children_is_the_centre_of_their_union() {
        let panel: Node = sample_panel(Id::new(2).expect("id"), "Side").into();
        let cube: Node = Object::new(
            Id::new(3).expect("id"),
            "Cube",
            Placement::new(DVec3::new(1000.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement"),
            Geometry::Mesh(box_mesh()),
        )
        .into();
        let children = vec![panel, cube];

        let anchor = children.anchor(FrameAnchor::CenterOfBounds).expect("the union centre");

        assert_eq!(anchor, [millimetres(505), millimetres(250), Length::from_micrometres(9_000)]);
        let rebased = rebase_at_anchor(children, Placement::IDENTITY, FrameAnchor::CenterOfBounds).expect("rebase");
        assert_eq!(rebased.placement.translation(), DVec3::new(505.0, 250.0, 9.0));
        assert!(rebased.pivot.is_zero());
        let (min, max) = panel_bounds(&rebased.content, Placement::IDENTITY, 0).expect("bounds");
        assert!((min + DVec3::new(505.0, 250.0, 9.0)).length() < 1e-3, "min was {min:?}");
        assert!((max - DVec3::new(505.0, 250.0, 9.0)).length() < 1e-3, "max was {max:?}");
    }

    #[test]
    fn a_collection_without_bounds_or_creation_pivot_is_refused() {
        let empty: Vec<Node> = Vec::new();
        assert_eq!(empty.anchor(FrameAnchor::CenterOfBounds), Err(FrameError::NoBounds));

        let children = vec![sample_panel(Id::new(2).expect("id"), "Side").into()];
        assert_eq!(children.anchor(FrameAnchor::CreationPivot), Err(FrameError::NoCreationPivot));
    }

    #[test]
    fn flat_content_refuses_a_z_offset_instead_of_dropping_it() {
        let line = Path::polyline(vec![[millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)]], false)
            .expect("a line");
        assert_eq!(
            line.shifted([Length::ZERO, Length::ZERO, millimetres(1)]),
            Err(FrameError::OffPlane { micrometres: 1_000 })
        );
    }
}
