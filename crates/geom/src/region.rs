//! Planar regions for panel machining.
//!
//! Stage 3 (ADR 0009, ADR 0012) models every through cutout on a panel as part of one removed region
//! `K`, and the material that stays is `C \ K` where `C` is the core outline. This module owns that
//! 2D boolean layer. A [`Region`] is one material component: a closed outer ring and zero or more
//! hole rings, in millimetres.
//!
//! The value type, its area and an even-odd containment test are in place, together with the exact
//! orthogonal boolean for axis-aligned rectangles: [`union_rects`] merges overlapping cutouts,
//! [`subtract_rects`] removes them from a rectangular core including boundary notches, and
//! [`connected_count`] checks the one-component invariant. Cutouts are rectangles today, so this
//! slice covers every v1 panel; shaped cores and traced rings with holes come next.

use std::fmt;

use crate::DVec2;

/// Coincidence tolerance for ring points, in millimetres.
const POINT_EPSILON: f64 = 1.0e-9;

/// One material component in the plane: a closed outer ring and zero or more hole rings.
///
/// Rings are validated on construction: every ring needs at least three finite points with no
/// consecutive points closer than [`POINT_EPSILON`]. The rectangle boolean does not build a
/// [`Region`] yet, so a hole is not checked against its outer ring here.
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

/// Why a [`Rect`] cannot be constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectError {
    /// The width or the height is not positive.
    Degenerate,
    /// A corner is not finite.
    NonFinite,
}

impl fmt::Display for RectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Degenerate => formatter.write_str("the rectangle needs a positive width and height"),
            Self::NonFinite => formatter.write_str("a rectangle corner is not finite"),
        }
    }
}

impl std::error::Error for RectError {}

/// An axis-aligned rectangle in millimetres, with a minimum and a maximum corner.
///
/// This is the shape of every v1 cutout and the intermediate form of the orthogonal boolean:
/// [`union_rects`] and [`subtract_rects`] decompose a region into disjoint rectangles. Open at the
/// maximum edge, so a rectangle never overlaps the next one that starts on that edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    min: DVec2,
    max: DVec2,
}

impl Rect {
    /// Creates a rectangle with finite corners and positive width and height.
    pub fn new(min: DVec2, max: DVec2) -> Result<Self, RectError> {
        if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
            return Err(RectError::NonFinite);
        }
        if max.x - min.x <= POINT_EPSILON || max.y - min.y <= POINT_EPSILON {
            return Err(RectError::Degenerate);
        }
        Ok(Self { min, max })
    }

    /// The minimum corner.
    pub const fn min(self) -> DVec2 {
        self.min
    }

    /// The maximum corner.
    pub const fn max(self) -> DVec2 {
        self.max
    }

    /// The width along X.
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    /// The height along Y.
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    /// The area in square millimetres.
    pub fn area(self) -> f64 {
        self.width() * self.height()
    }

    /// Whether two rectangles share a positive-length vertical or horizontal edge.
    fn touches(self, other: Self) -> bool {
        let overlap_x = self.min.x < other.max.x - POINT_EPSILON && other.min.x < self.max.x - POINT_EPSILON;
        let overlap_y = self.min.y < other.max.y - POINT_EPSILON && other.min.y < self.max.y - POINT_EPSILON;
        let side_touch =
            (self.max.x - other.min.x).abs() < POINT_EPSILON || (other.max.x - self.min.x).abs() < POINT_EPSILON;
        let cap_touch =
            (self.max.y - other.min.y).abs() < POINT_EPSILON || (other.max.y - self.min.y).abs() < POINT_EPSILON;
        (side_touch && overlap_y) || (cap_touch && overlap_x)
    }
}

/// The total area of a disjoint rectangle decomposition.
pub fn total_area(rects: &[Rect]) -> f64 {
    rects.iter().map(|rect| rect.area()).sum()
}

/// The union of axis-aligned rectangles, as a disjoint set of rectangles.
///
/// The result is exact for axis-aligned input: overlapping rectangles merge, and the output
/// rectangles do not overlap. Horizontal neighbours with the same Y range are coalesced so a plain
/// stack of rectangles comes back as itself.
pub fn union_rects(rects: &[Rect]) -> Vec<Rect> {
    let mut parts = Vec::new();
    for slab in slabs(rects.iter().map(|rect| (rect.min.x, rect.max.x))) {
        let mut intervals: Vec<(f64, f64)> = rects
            .iter()
            .filter(|rect| rect.min.x < slab.1 - POINT_EPSILON && rect.max.x > slab.0 + POINT_EPSILON)
            .map(|rect| (rect.min.y, rect.max.y))
            .collect();
        for (y0, y1) in merge_intervals(&mut intervals) {
            if let Ok(rect) = Rect::new(DVec2::new(slab.0, y0), DVec2::new(slab.1, y1)) {
                parts.push(rect);
            }
        }
    }
    coalesce_horizontal(parts)
}

/// The material left in `core` after removing every rectangle in `cuts`, as disjoint rectangles.
///
/// Rectangles outside the core contribute nothing. A cut that crosses the core boundary opens a
/// notch: the material decomposition simply stops at the core's edge.
pub fn subtract_rects(core: Rect, cuts: &[Rect]) -> Vec<Rect> {
    let mut boundaries = vec![core.min.x, core.max.x];
    for cut in cuts {
        boundaries.push(cut.min.x);
        boundaries.push(cut.max.x);
    }
    let mut parts = Vec::new();
    for slab in slabs(boundaries.into_iter().map(|x| (x, x))) {
        let start = slab.0.max(core.min.x);
        let end = slab.1.min(core.max.x);
        if end - start <= POINT_EPSILON {
            continue;
        }
        let mut holes: Vec<(f64, f64)> = cuts
            .iter()
            .filter(|cut| cut.min.x < end - POINT_EPSILON && cut.max.x > start + POINT_EPSILON)
            .map(|cut| (cut.min.y.max(core.min.y), cut.max.y.min(core.max.y)))
            .filter(|(y0, y1)| y1 - y0 > POINT_EPSILON)
            .collect();
        let material = subtract_intervals((core.min.y, core.max.y), merge_intervals(&mut holes));
        for (y0, y1) in material {
            if let Ok(rect) = Rect::new(DVec2::new(start, y0), DVec2::new(end, y1)) {
                parts.push(rect);
            }
        }
    }
    coalesce_horizontal(parts)
}

/// The number of connected material pieces in a disjoint rectangle decomposition.
///
/// Two rectangles connect only through a shared edge of positive length; touching at a single
/// corner is a zero-width bridge and does not connect material.
pub fn connected_count(rects: &[Rect]) -> usize {
    let mut group: Vec<Option<usize>> = vec![None; rects.len()];
    for index in 0..rects.len() {
        if group[index].is_some() {
            continue;
        }
        let id = index;
        group[index] = Some(id);
        let mut stack = vec![index];
        while let Some(current) = stack.pop() {
            for (other, candidate) in rects.iter().enumerate() {
                if group[other].is_none() && rects[current].touches(*candidate) {
                    group[other] = Some(id);
                    stack.push(other);
                }
            }
        }
    }
    group.iter().flatten().copied().collect::<std::collections::BTreeSet<_>>().len()
}

/// Distinct, sorted vertical slabs between every `x0` and `x1` in the input.
fn slabs(ranges: impl Iterator<Item = (f64, f64)>) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = Vec::new();
    for (x0, x1) in ranges {
        xs.push(x0);
        xs.push(x1);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup_by(|a, b| (*a - *b).abs() < POINT_EPSILON);
    xs.windows(2).filter(|pair| pair[1] - pair[0] > POINT_EPSILON).map(|pair| (pair[0], pair[1])).collect()
}

/// Sort and merge overlapping or touching intervals.
fn merge_intervals(intervals: &mut [(f64, f64)]) -> Vec<(f64, f64)> {
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (y0, y1) in intervals.iter().copied() {
        match merged.last_mut() {
            Some(last) if y0 <= last.1 + POINT_EPSILON => last.1 = last.1.max(y1),
            _ => merged.push((y0, y1)),
        }
    }
    merged
}

/// The `base` interval with the sorted `holes` intervals removed.
fn subtract_intervals(base: (f64, f64), holes: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    let mut cursor = base.0;
    for (hole0, hole1) in holes {
        if hole0 > cursor {
            result.push((cursor, hole0.min(base.1)));
        }
        cursor = cursor.max(hole1);
        if cursor >= base.1 {
            break;
        }
    }
    if cursor < base.1 {
        result.push((cursor, base.1));
    }
    result.retain(|(y0, y1)| y1 - y0 > POINT_EPSILON);
    result
}

/// Merge horizontally adjacent slabs that share an identical Y range.
fn coalesce_horizontal(parts: Vec<Rect>) -> Vec<Rect> {
    let mut sorted = parts;
    sorted.sort_by(|a, b| {
        a.min
            .y
            .partial_cmp(&b.min.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.min.x.partial_cmp(&b.min.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut merged: Vec<Rect> = Vec::new();
    for rect in sorted {
        if let Some(last) = merged.last_mut()
            && (last.max.x - rect.min.x).abs() < POINT_EPSILON
            && (last.min.y - rect.min.y).abs() < POINT_EPSILON
            && (last.max.y - rect.max.y).abs() < POINT_EPSILON
        {
            last.max.x = rect.max.x;
            continue;
        }
        merged.push(rect);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<DVec2> {
        vec![DVec2::new(x, y), DVec2::new(x + width, y), DVec2::new(x + width, y + height), DVec2::new(x, y + height)]
    }

    fn rect_with_hole() -> Region {
        Region::new(rect(0.0, 0.0, 100.0, 50.0), vec![rect(10.0, 10.0, 20.0, 10.0)]).expect("a rect with a hole")
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
        assert!(matches!(Region::new(vec![], vec![]), Err(RegionError::Degenerate { kind: RingKind::Outer })));
        assert!(matches!(
            Region::new(rect(0.0, 0.0, 100.0, 50.0), vec![vec![]]),
            Err(RegionError::Degenerate { kind: RingKind::Hole { index: 0 } })
        ));
    }

    #[test]
    fn duplicate_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(0.0, 10.0)];
        assert!(matches!(
            Region::new(ring, vec![]),
            Err(RegionError::DuplicatePoint { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn non_finite_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(f64::NAN, 5.0), DVec2::new(3.0, 0.0)];
        assert!(matches!(Region::new(ring, vec![]), Err(RegionError::NonFinite { kind: RingKind::Outer, index: 1 })));
    }

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect::new(DVec2::new(x, y), DVec2::new(x + width, y + height)).expect("valid rectangle")
    }

    fn assert_disjoint(rects: &[Rect]) {
        for (index, a) in rects.iter().enumerate() {
            for b in &rects[index + 1..] {
                let overlap_x = a.min().x.max(b.min().x) < a.max().x.min(b.max().x) - POINT_EPSILON;
                let overlap_y = a.min().y.max(b.min().y) < a.max().y.min(b.max().y) - POINT_EPSILON;
                assert!(!(overlap_x && overlap_y), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn degenerate_and_non_finite_rectangles_are_rejected() {
        assert!(matches!(Rect::new(DVec2::new(0.0, 0.0), DVec2::new(0.0, 10.0)), Err(RectError::Degenerate)));
        assert!(matches!(Rect::new(DVec2::new(0.0, 0.0), DVec2::new(-5.0, 10.0)), Err(RectError::Degenerate)));
        assert!(matches!(Rect::new(DVec2::new(f64::NAN, 0.0), DVec2::new(10.0, 10.0)), Err(RectError::NonFinite)));
    }

    #[test]
    fn union_of_disjoint_rectangles_keeps_both() {
        let parts = union_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(20.0, 0.0, 10.0, 10.0)]);
        assert_eq!(parts.len(), 2);
        assert!((total_area(&parts) - 200.0).abs() < 1.0e-9);
        assert_disjoint(&parts);
    }

    #[test]
    fn union_merges_overlapping_rectangles() {
        let parts = union_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(5.0, 5.0, 10.0, 10.0)]);
        assert!((total_area(&parts) - 175.0).abs() < 1.0e-9);
        assert_disjoint(&parts);
    }

    #[test]
    fn union_coalesces_touching_rectangles() {
        let side_by_side = union_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 0.0, 10.0, 10.0)]);
        assert_eq!(side_by_side.len(), 1);
        assert!((total_area(&side_by_side) - 200.0).abs() < 1.0e-9);
        let stacked = union_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(0.0, 10.0, 10.0, 10.0)]);
        assert_eq!(stacked.len(), 1);
        assert!((total_area(&stacked) - 200.0).abs() < 1.0e-9);
    }

    #[test]
    fn subtraction_around_an_interior_cutout_keeps_one_component() {
        let core = bounds(0.0, 0.0, 100.0, 50.0);
        let parts = subtract_rects(core, &[bounds(20.0, 10.0, 20.0, 10.0)]);
        assert!((total_area(&parts) - 4800.0).abs() < 1.0e-9);
        assert_eq!(connected_count(&parts), 1);
        assert_disjoint(&parts);
    }

    #[test]
    fn subtraction_across_the_boundary_opens_a_notch() {
        let core = bounds(0.0, 0.0, 100.0, 50.0);
        let parts = subtract_rects(core, &[bounds(30.0, 40.0, 40.0, 20.0)]);
        assert!((total_area(&parts) - 4600.0).abs() < 1.0e-9);
        assert_eq!(connected_count(&parts), 1);
    }

    #[test]
    fn subtraction_that_splits_the_core_reports_two_components() {
        let core = bounds(0.0, 0.0, 100.0, 50.0);
        let parts = subtract_rects(core, &[bounds(-10.0, 20.0, 120.0, 10.0)]);
        assert!((total_area(&parts) - 4000.0).abs() < 1.0e-9);
        assert_eq!(connected_count(&parts), 2);
    }

    #[test]
    fn subtraction_outside_the_core_changes_nothing() {
        let core = bounds(0.0, 0.0, 100.0, 50.0);
        let parts = subtract_rects(core, &[bounds(200.0, 200.0, 10.0, 10.0)]);
        assert_eq!(parts, vec![core]);
    }

    #[test]
    fn subtraction_covering_the_core_leaves_nothing() {
        let parts = subtract_rects(bounds(0.0, 0.0, 100.0, 50.0), &[bounds(-10.0, -10.0, 200.0, 200.0)]);
        assert!(parts.is_empty());
        assert_eq!(connected_count(&parts), 0);
    }

    #[test]
    fn material_connected_only_at_a_corner_is_not_one_component() {
        let parts = vec![bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 10.0, 10.0, 10.0)];
        assert_eq!(connected_count(&parts), 2);
    }

    #[test]
    fn material_sharing_an_edge_is_one_component() {
        let parts = vec![bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 0.0, 10.0, 10.0)];
        assert_eq!(connected_count(&parts), 1);
    }
}
