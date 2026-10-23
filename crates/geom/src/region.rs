//! Planar regions for panel machining.
//!
//! Stage 3 (ADR 0009, ADR 0012) models every through cutout on a panel as part of one removed region
//! `K`, and the material that stays is `C \ K` where `C` is the core outline. This module owns that
//! 2D boolean layer. A [`Region`] is one material component: a closed outer ring and zero or more
//! hole rings, in millimetres.
//!
//! This slice carries the validated value type, its area and an even-odd containment test. The
//! boolean operations (`union`, `difference`) and the connected-core invariant follow next; `area`
//! and `contains` are the primitives they and the diagnostic path build on.

use std::fmt;

use crate::DVec2;

/// Coincidence tolerance for ring points, in millimetres.
const POINT_EPSILON: f64 = 1.0e-9;

/// One material component in the plane: a closed outer ring and zero or more hole rings.
///
/// Rings are validated on construction: every ring needs at least three finite points with no
/// consecutive points closer than [`POINT_EPSILON`]. The boolean layer is not wired in yet, so this
/// slice does not re-orient rings and does not check that holes stay inside the outer ring.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
}

impl Region {
    /// Creates a region after validating every ring.
    pub fn new(outer: Vec<DVec2>, holes: Vec<Vec<DVec2>>) -> Result<Self, RegionError> {
        validate_ring(&outer, RingKind::Outer)?;
        for (index, hole) in holes.iter().enumerate() {
            validate_ring(hole, RingKind::Hole { index })?;
        }
        Ok(Self { outer, holes })
    }

    /// The outer ring.
    pub fn outer(&self) -> &[DVec2] {
        &self.outer
    }

    /// The hole rings.
    pub fn holes(&self) -> &[Vec<DVec2>] {
        &self.holes
    }

    /// The material area: the outer ring's shoelace area minus every hole's area, in square
    /// millimetres. The sign follows the ring orientations; callers turn a non-positive result
    /// into a diagnostic, never a panic.
    pub fn area(&self) -> f64 {
        ring_area(&self.outer) - self.holes.iter().map(|hole| ring_area(hole)).sum::<f64>()
    }

    /// Even-odd containment: a point is inside the material when it is inside the outer ring and in
    /// no hole.
    pub fn contains(&self, point: DVec2) -> bool {
        if !ring_contains(&self.outer, point) {
            return false;
        }
        for hole in &self.holes {
            if ring_contains(hole, point) {
                return false;
            }
        }
        true
    }
}

/// Which ring a [`RegionError`] refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingKind {
    /// The outer ring.
    Outer,
    /// The hole at `index`.
    Hole {
        /// Index of the hole.
        index: usize,
    },
}

impl fmt::Display for RingKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Outer => formatter.write_str("outer"),
            Self::Hole { index } => write!(formatter, "hole {index}"),
        }
    }
}

/// Why a [`Region`] cannot be constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionError {
    /// A ring has fewer than three points.
    Degenerate {
        /// The ring that is degenerate.
        kind: RingKind,
    },
    /// A ring contains a point that is not finite.
    NonFinite {
        /// The ring that contains the point.
        kind: RingKind,
        /// Index of the offending point.
        index: usize,
    },
    /// Two consecutive points of a ring are closer than [`POINT_EPSILON`].
    DuplicatePoint {
        /// The ring that contains the points.
        kind: RingKind,
        /// Index of the first of the two points.
        index: usize,
    },
}

impl fmt::Display for RegionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Degenerate { kind } => write!(formatter, "the {kind} ring needs at least three points"),
            Self::NonFinite { kind, index } => {
                write!(formatter, "point {index} of the {kind} ring is not finite")
            }
            Self::DuplicatePoint { kind, index } => {
                write!(formatter, "point {index} of the {kind} ring repeats its neighbour")
            }
        }
    }
}

impl std::error::Error for RegionError {}

/// The signed shoelace area of one ring; zero for a degenerate ring.
fn ring_area(ring: &[DVec2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut doubled = 0.0;
    for index in 0..ring.len() {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        doubled += a.x * b.y - b.x * a.y;
    }
    doubled * 0.5
}

/// Even-odd ray casting: a horizontal ray to the right crosses the ring an odd number of times when
/// the point is inside.
fn ring_contains(ring: &[DVec2], point: DVec2) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..ring.len() {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        let crosses = (a.y > point.y) != (b.y > point.y);
        if !crosses {
            continue;
        }
        let at_point = (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x;
        if point.x < at_point {
            inside = !inside;
        }
    }
    inside
}

fn validate_ring(ring: &[DVec2], kind: RingKind) -> Result<(), RegionError> {
    if ring.len() < 3 {
        return Err(RegionError::Degenerate { kind });
    }
    for (index, point) in ring.iter().enumerate() {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(RegionError::NonFinite { kind, index });
        }
        let next = ring[(index + 1) % ring.len()];
        if (point.x - next.x).abs() < POINT_EPSILON && (point.y - next.y).abs() < POINT_EPSILON {
            return Err(RegionError::DuplicatePoint { kind, index });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + width, y),
            DVec2::new(x + width, y + height),
            DVec2::new(x, y + height),
        ]
    }

    fn rect_with_hole() -> Region {
        Region::new(rect(0.0, 0.0, 100.0, 50.0), vec![rect(10.0, 10.0, 20.0, 10.0)])
            .expect("a rect with a hole")
    }

    #[test]
    fn a_rectangle_has_its_area() {
        let region = Region::new(rect(0.0, 0.0, 100.0, 50.0), vec![]).expect("valid rect");
        assert!((region.area() - 5000.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_hole_subtracts_its_area() {
        let region = rect_with_hole();
        let outer_area = 100.0 * 50.0;
        let hole_area = 20.0 * 10.0;
        assert!((region.area() - (outer_area - hole_area)).abs() < 1.0e-9);
    }

    #[test]
    fn points_inside_the_material_are_contained() {
        let region = rect_with_hole();
        assert!(region.contains(DVec2::new(5.0, 5.0)));
        assert!(region.contains(DVec2::new(90.0, 40.0)));
    }

    #[test]
    fn a_point_in_a_hole_is_not_contained() {
        let region = rect_with_hole();
        assert!(!region.contains(DVec2::new(15.0, 15.0)));
    }

    #[test]
    fn degenerate_rings_are_rejected() {
        assert!(matches!(
            Region::new(vec![], vec![]),
            Err(RegionError::Degenerate { kind: RingKind::Outer })
        ));
        assert!(matches!(
            Region::new(rect(0.0, 0.0, 100.0, 50.0), vec![vec![]]),
            Err(RegionError::Degenerate { kind: RingKind::Hole { index: 0 } })
        ));
    }

    #[test]
    fn duplicate_ring_points_are_rejected() {
        let ring = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(0.0, 10.0),
        ];
        assert!(matches!(
            Region::new(ring, vec![]),
            Err(RegionError::DuplicatePoint { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn non_finite_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(f64::NAN, 5.0), DVec2::new(3.0, 0.0)];
        assert!(matches!(
            Region::new(ring, vec![]),
            Err(RegionError::NonFinite { kind: RingKind::Outer, index: 1 })
        ));
    }
}