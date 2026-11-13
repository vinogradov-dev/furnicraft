//! A furniture surface as an axis-aligned world box, in millimetres.
//!
//! Two pure queries measure the document against the same surfaces: the furniture move rule stops a
//! drag at one (`move_query`), and the niche probe reports the first one along a ray (`niche`). They
//! share this value so they cannot disagree about where a surface is (ADR 0102, ADR 0103).
//!
//! A turned body has no axis-aligned sides any more, so its eight corners are transformed and the
//! tightest box around them is used. The box is a measurement device, never stored in the document.

use furnicraft_geom::DVec3;

use super::move_query::MoveAxis;
use crate::{Id, Placement};

/// What owns a furniture surface.
///
/// A product's authored envelope is a shell; a panel, an object or any other container is a body. The
/// probe adds [`SurfaceKind::Envelope`] for the face of the piece the ray started inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    /// A virtual face of the product that holds the measured point.
    Envelope,
    /// A panel face.
    Panel,
    /// A geometry body: an object, a hardware model, a primitive.
    Body,
    /// Another product's envelope: its shell, not the space inside it.
    Shell,
}

/// One furniture surface as an axis-aligned world box, in millimetres.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SurfaceBox {
    pub(crate) id: Id,
    pub(crate) min: DVec3,
    pub(crate) max: DVec3,
    pub(crate) kind: SurfaceKind,
}

/// One axis of a [`SurfaceBox`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span {
    pub(crate) min: f64,
    pub(crate) max: f64,
}

impl SurfaceBox {
    /// Whether the surface is a product shell rather than a body.
    pub(crate) fn is_shell(self) -> bool {
        self.kind == SurfaceKind::Shell
    }

    /// The box's extent on `axis`.
    pub(crate) fn span(self, axis: MoveAxis) -> Span {
        match axis {
            MoveAxis::X => Span { min: self.min.x, max: self.max.x },
            MoveAxis::Y => Span { min: self.min.y, max: self.max.y },
            MoveAxis::Z => Span { min: self.min.z, max: self.max.z },
        }
    }

    /// Whether `point` lies inside the box or on its faces.
    pub(crate) fn contains(self, point: DVec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }
}

/// A local box through a node's world placement, as the world box around its eight corners.
///
/// A corner that is not finite is no box.
pub(crate) fn surface_box(id: Id, min: DVec3, max: DVec3, world: Placement, kind: SurfaceKind) -> Option<SurfaceBox> {
    let mut box_ = SurfaceBox { id, min: DVec3::splat(f64::INFINITY), max: DVec3::splat(f64::NEG_INFINITY), kind };
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                let corner = world.transform_point(DVec3::new(x, y, z));
                if !corner.is_finite() {
                    return None;
                }
                box_.min = box_.min.min(corner);
                box_.max = box_.max.max(corner);
            }
        }
    }
    Some(box_)
}

/// The visible faces of a surface a probe can meet along a unit direction, or `None`.
///
/// The slab test returns the near face for a point outside the box and the far face for a point
/// inside it; a ray that starts on a face and points inward meets the opposite face, while one that
/// starts on a face and points outward meets it at zero. That is what makes a point on the envelope
/// answer with a zero extent on the axis it leaves.
pub(crate) fn face_ahead(point: DVec3, direction: DVec3, min: DVec3, max: DVec3) -> Option<f64> {
    const EPSILON: f64 = 1.0e-9;
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    for axis in 0..3 {
        let origin = point[axis];
        let step = direction[axis];
        let low = min[axis];
        let high = max[axis];
        if step.abs() <= EPSILON {
            if origin < low - EPSILON || origin > high + EPSILON {
                return None;
            }
            continue;
        }
        let first = (low - origin) / step;
        let second = (high - origin) / step;
        enter = enter.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    if exit < enter || exit < 0.0 {
        return None;
    }
    if enter > EPSILON { Some(enter) } else { Some(exit) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn unit_box() -> (DVec3, DVec3) {
        (DVec3::ZERO, DVec3::new(10.0, 10.0, 10.0))
    }

    #[test]
    fn a_face_ahead_enters_from_outside_and_leaves_from_inside() {
        let (min, max) = unit_box();
        let outside = DVec3::new(-5.0, 5.0, 5.0);
        assert_eq!(face_ahead(outside, DVec3::X, min, max), Some(5.0), "the near face");
        let inside = DVec3::new(5.0, 5.0, 5.0);
        assert_eq!(face_ahead(inside, DVec3::X, min, max), Some(5.0), "the far face");
        let leaving = DVec3::new(0.0, 5.0, 5.0);
        assert_eq!(face_ahead(leaving, -DVec3::X, min, max), Some(0.0), "a face the ray already sits on");
        assert_eq!(face_ahead(leaving, DVec3::X, min, max), Some(10.0), "and the opposite one inward");
    }

    #[test]
    fn a_ray_that_misses_the_box_meets_no_face() {
        let (min, max) = unit_box();
        assert_eq!(face_ahead(DVec3::new(-5.0, 50.0, 5.0), DVec3::X, min, max), None);
        assert_eq!(face_ahead(DVec3::new(5.0, 5.0, 5.0), -DVec3::X, min, max), Some(5.0), "the far face backwards");
        assert_eq!(face_ahead(DVec3::new(50.0, 5.0, 5.0), DVec3::X, min, max), None, "a box behind the ray");
    }

    #[test]
    fn a_turned_body_is_measured_by_the_box_around_its_corners() {
        let turn = Placement::IDENTITY
            .with_rotation_euler(crate::EulerAngles {
                z: furnicraft_units::Degrees::from_millidegrees(45_000),
                ..crate::EulerAngles::default()
            })
            .expect("a turn");
        let box_ =
            surface_box(Id::new(1).expect("id"), DVec3::ZERO, DVec3::new(10.0, 2.0, 1.0), turn, SurfaceKind::Panel)
                .expect("a box");
        assert!((box_.min.x + std::f64::consts::SQRT_2).abs() < 1e-9, "was {box_:?}");
        assert!((box_.max.x - 5.0 * std::f64::consts::SQRT_2).abs() < 1e-9, "was {box_:?}");
        assert!((box_.max.y - 6.0 * std::f64::consts::SQRT_2).abs() < 1e-9, "was {box_:?}");
        assert!(!box_.is_shell());
    }
}
