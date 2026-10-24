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
//! [`regions_from_rects`] traces the resulting closed rings with holes. Cutouts are rectangles
//! today, so this covers every v1 panel; shaped cores come next.

use std::collections::HashMap;
use std::fmt;

use crate::DVec2;

/// Coincidence tolerance for ring points, in millimetres.
const POINT_EPSILON: f64 = 1.0e-9;

/// One material component in the plane: a closed outer ring and zero or more hole rings.
///
/// Rings are validated on construction: every ring needs at least three finite points with no
/// consecutive points closer than [`POINT_EPSILON`]. Holes are not checked against the outer ring
/// here; [`regions_from_rects`] produces consistent regions from the rectangle decomposition.
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

    /// The material area: the outer ring's area minus every hole's area, in square millimetres.
    /// Ring orientation does not matter; an invalid region whose holes exceed the outer ring
    /// yields a non-positive value, which callers turn into a diagnostic, never a panic.
    pub fn area(&self) -> f64 {
        ring_area(&self.outer).abs() - self.holes.iter().map(|hole| ring_area(hole).abs()).sum::<f64>()
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

/// The material regions covered by axis-aligned rectangles, with holes.
///
/// Overlapping and touching input is normalized first, so callers may pass a raw cutout set or a
/// [`union_rects`] result. Each returned [`Region`] has a counter-clockwise outer ring and
/// clockwise hole rings, traced from the rectangle boundaries: a shared edge disappears, a boundary
/// notch becomes part of the outer ring and a corner-only contact stays two regions.
pub fn regions_from_rects(rects: &[Rect]) -> Vec<Region> {
    let rects = union_rects(rects);
    if rects.is_empty() {
        return Vec::new();
    }

    let mut xs = Vec::new();
    for rect in &rects {
        xs.push(rect.min.x);
        xs.push(rect.max.x);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup_by(|a, b| (*a - *b).abs() < POINT_EPSILON);

    let mut slab_intervals: Vec<Vec<(f64, f64)>> = Vec::new();
    for pair in xs.windows(2) {
        let (x0, x1) = (pair[0], pair[1]);
        let mut intervals: Vec<(f64, f64)> = rects
            .iter()
            .filter(|rect| rect.min.x < x1 - POINT_EPSILON && rect.max.x > x0 + POINT_EPSILON)
            .map(|rect| (rect.min.y, rect.max.y))
            .collect();
        slab_intervals.push(merge_intervals(&mut intervals));
    }

    let mut edges = Vec::new();
    let mut points = HashMap::new();

    // A vertical boundary exists wherever the material on its two sides differs.
    for (index, &x) in xs.iter().enumerate() {
        let left: &[(f64, f64)] =
            index.checked_sub(1).and_then(|previous| slab_intervals.get(previous)).map_or(&[], Vec::as_slice);
        let right: &[(f64, f64)] = slab_intervals.get(index).map_or(&[], Vec::as_slice);
        for (y0, y1) in difference_intervals(right, left) {
            push_edge(&mut edges, &mut points, DVec2::new(x, y1), DVec2::new(x, y0));
        }
        for (y0, y1) in difference_intervals(left, right) {
            push_edge(&mut edges, &mut points, DVec2::new(x, y0), DVec2::new(x, y1));
        }
    }

    for (index, intervals) in slab_intervals.iter().enumerate() {
        let (Some(&x0), Some(&x1)) = (xs.get(index), xs.get(index + 1)) else {
            continue;
        };
        for &(y0, y1) in intervals {
            push_edge(&mut edges, &mut points, DVec2::new(x0, y0), DVec2::new(x1, y0));
            push_edge(&mut edges, &mut points, DVec2::new(x1, y1), DVec2::new(x0, y1));
        }
    }

    let loops = trace_loops(&edges, &points);
    regions_from_loops(loops)
}

/// One directed boundary edge, with the material on its left.
#[derive(Clone, Copy)]
struct Edge {
    from: (i64, i64),
    to: (i64, i64),
    direction: u8,
}

/// Edge directions in counter-clockwise order.
const EAST: u8 = 0;
const NORTH: u8 = 1;
const WEST: u8 = 2;
const SOUTH: u8 = 3;

fn push_edge(edges: &mut Vec<Edge>, points: &mut HashMap<(i64, i64), DVec2>, from: DVec2, to: DVec2) {
    let Some(direction) = direction_between(from, to) else {
        return;
    };
    let from_key = point_key(from);
    let to_key = point_key(to);
    points.entry(from_key).or_insert(from);
    points.entry(to_key).or_insert(to);
    edges.push(Edge { from: from_key, to: to_key, direction });
}

fn point_key(point: DVec2) -> (i64, i64) {
    ((point.x / POINT_EPSILON).round() as i64, (point.y / POINT_EPSILON).round() as i64)
}

fn direction_between(from: DVec2, to: DVec2) -> Option<u8> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dy.abs() <= POINT_EPSILON && dx > POINT_EPSILON {
        Some(EAST)
    } else if dx.abs() <= POINT_EPSILON && dy > POINT_EPSILON {
        Some(NORTH)
    } else if dy.abs() <= POINT_EPSILON && dx < -POINT_EPSILON {
        Some(WEST)
    } else if dx.abs() <= POINT_EPSILON && dy < -POINT_EPSILON {
        Some(SOUTH)
    } else {
        None
    }
}

/// The segments of `from` that `remove` does not cover.
fn difference_intervals(from: &[(f64, f64)], remove: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    for &(y0, y1) in from {
        result.extend(subtract_intervals((y0, y1), remove.to_vec()));
    }
    result
}

/// Closes directed boundary edges into rings, keeping the material on the left.
///
/// The clockwise-next rule keeps a corner-only material contact on two loops. A walk that touches a
/// vertex twice is split into simple rings afterwards, so corner-touching holes also stay separate.
fn trace_loops(edges: &[Edge], points: &HashMap<(i64, i64), DVec2>) -> Vec<Vec<DVec2>> {
    let mut outgoing: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (index, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.from).or_default().push(index);
    }

    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let start_key = edges[start].from;
        let mut path = vec![start_key];
        let mut current = start;
        used[current] = true;
        let mut guard = 0usize;
        loop {
            let at = edges[current].to;
            path.push(at);
            if at == start_key {
                break;
            }
            guard += 1;
            if guard > edges.len() {
                break;
            }
            let Some(next) = next_edge(at, edges[current].direction, &outgoing, edges, &used) else {
                break;
            };
            used[next] = true;
            current = next;
        }
        if path.last() == Some(&start_key) && path.len() >= 4 {
            path.pop();
            let ring: Vec<DVec2> = path.iter().filter_map(|key| points.get(key).copied()).collect();
            for piece in split_self_touching(ring) {
                if let Some(piece) = simplify_ring(&piece) {
                    loops.push(piece);
                }
            }
        }
    }
    loops
}

/// Splits a boundary walk that visits a vertex twice into simple rings.
fn split_self_touching(ring: Vec<DVec2>) -> Vec<Vec<DVec2>> {
    let mut pending = vec![ring];
    let mut simple = Vec::new();
    while let Some(ring) = pending.pop() {
        let mut seen: HashMap<(i64, i64), usize> = HashMap::new();
        let mut repeat = None;
        for (index, point) in ring.iter().enumerate() {
            if let Some(&first) = seen.get(&point_key(*point)) {
                repeat = Some((first, index));
                break;
            }
            seen.insert(point_key(*point), index);
        }
        let Some((first, second)) = repeat else {
            simple.push(ring);
            continue;
        };
        let cycle = ring[first..second].to_vec();
        let mut rest = ring[..first].to_vec();
        rest.extend_from_slice(&ring[second..]);
        if cycle.len() >= 3 {
            pending.push(cycle);
        }
        if rest.len() >= 3 {
            pending.push(rest);
        }
    }
    simple
}

fn next_edge(
    at: (i64, i64),
    incoming: u8,
    outgoing: &HashMap<(i64, i64), Vec<usize>>,
    edges: &[Edge],
    used: &[bool],
) -> Option<usize> {
    let reverse = (incoming + 2) % 4;
    let candidates = outgoing.get(&at)?;
    for step in 1..=3 {
        let wanted = (reverse + 4 - step) % 4;
        if let Some(&edge) = candidates.iter().find(|&&index| !used[index] && edges[index].direction == wanted) {
            return Some(edge);
        }
    }
    None
}

fn simplify_ring(points: &[DVec2]) -> Option<Vec<DVec2>> {
    let mut ring: Vec<DVec2> = Vec::new();
    for &point in points {
        let duplicate =
            ring.last().is_some_and(|last| (*last - point).length_squared() <= POINT_EPSILON * POINT_EPSILON);
        if !duplicate {
            ring.push(point);
        }
    }
    while ring.len() > 1 && (ring[0] - ring[ring.len() - 1]).length_squared() <= POINT_EPSILON * POINT_EPSILON {
        ring.pop();
    }
    if ring.len() < 3 {
        return None;
    }
    loop {
        let mut removed = false;
        for index in 0..ring.len() {
            let previous = ring[(index + ring.len() - 1) % ring.len()];
            let current = ring[index];
            let next = ring[(index + 1) % ring.len()];
            let straight_vertical = (previous.x - current.x).abs() < POINT_EPSILON
                && (current.x - next.x).abs() < POINT_EPSILON
                && (previous.y - current.y) * (current.y - next.y) > 0.0;
            let straight_horizontal = (previous.y - current.y).abs() < POINT_EPSILON
                && (current.y - next.y).abs() < POINT_EPSILON
                && (previous.x - current.x) * (current.x - next.x) > 0.0;
            if straight_vertical || straight_horizontal {
                ring.remove(index);
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
        if ring.len() < 3 {
            return None;
        }
    }
    Some(ring)
}

fn regions_from_loops(loops: Vec<Vec<DVec2>>) -> Vec<Region> {
    let mut outers: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = Vec::new();
    let mut holes: Vec<Vec<DVec2>> = Vec::new();
    for ring in loops {
        let area = ring_area(&ring);
        if area > POINT_EPSILON {
            outers.push((ring, Vec::new()));
        } else if area < -POINT_EPSILON {
            holes.push(ring);
        }
    }
    for hole in holes {
        let Some(&point) = hole.first() else {
            continue;
        };
        let Some((_, region_holes)) = outers.iter_mut().find(|(outer, _)| ring_contains(outer, point)) else {
            continue;
        };
        region_holes.push(hole);
    }
    outers.into_iter().filter_map(|(outer, holes)| Region::new(outer, holes).ok()).collect()
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

    #[test]
    fn a_single_rectangle_traces_one_ring() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].holes().is_empty());
        assert_eq!(regions[0].outer().len(), 4);
        assert!((regions[0].area() - 100.0).abs() < 1.0e-9);
        assert!(ring_area(regions[0].outer()) > 0.0);
        assert!(regions[0].contains(DVec2::new(5.0, 5.0)));
    }

    #[test]
    fn touching_rectangles_trace_one_outline() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 0.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().len(), 4);
        assert!((regions[0].area() - 200.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_partial_shared_edge_traces_a_step() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 5.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().len(), 8);
        assert!((regions[0].area() - 200.0).abs() < 1.0e-9);
    }

    #[test]
    fn an_interior_cutout_traces_a_hole() {
        let material = subtract_rects(bounds(0.0, 0.0, 100.0, 50.0), &[bounds(20.0, 10.0, 20.0, 10.0)]);
        let regions = regions_from_rects(&material);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
        assert!((regions[0].area() - 4800.0).abs() < 1.0e-9);
        assert!(!regions[0].contains(DVec2::new(30.0, 15.0)));
        assert!(regions[0].contains(DVec2::new(10.0, 10.0)));
        assert!(ring_area(&regions[0].holes()[0]) < 0.0);
    }

    #[test]
    fn two_interior_cutouts_trace_two_holes() {
        let material = subtract_rects(
            bounds(0.0, 0.0, 100.0, 50.0),
            &[bounds(10.0, 10.0, 20.0, 20.0), bounds(50.0, 10.0, 20.0, 20.0)],
        );
        let regions = regions_from_rects(&material);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 2);
        assert!((regions[0].area() - 4200.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_boundary_notch_is_part_of_the_outer_ring() {
        let material = subtract_rects(bounds(0.0, 0.0, 100.0, 50.0), &[bounds(30.0, 40.0, 40.0, 20.0)]);
        let regions = regions_from_rects(&material);
        assert_eq!(regions.len(), 1);
        assert!(regions[0].holes().is_empty());
        assert_eq!(regions[0].outer().len(), 8);
        assert!((regions[0].area() - 4600.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_split_core_traces_two_regions() {
        let material = subtract_rects(bounds(0.0, 0.0, 100.0, 50.0), &[bounds(0.0, 20.0, 100.0, 10.0)]);
        let regions = regions_from_rects(&material);
        assert_eq!(regions.len(), 2);
        let total: f64 = regions.iter().map(Region::area).sum();
        assert!((total - 4000.0).abs() < 1.0e-9);
    }

    #[test]
    fn corner_only_contact_traces_two_regions() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 10.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 2);
        let total: f64 = regions.iter().map(Region::area).sum();
        assert!((total - 200.0).abs() < 1.0e-9);
    }

    #[test]
    fn overlapping_rectangles_are_normalized_before_tracing() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(5.0, 5.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().len(), 8);
        assert!((regions[0].area() - 175.0).abs() < 1.0e-9);
    }

    #[test]
    fn an_empty_rectangle_set_traces_no_regions() {
        assert!(regions_from_rects(&[]).is_empty());
    }

    #[test]
    fn a_staircase_traces_one_ring() {
        let regions = regions_from_rects(&[
            bounds(0.0, 0.0, 10.0, 10.0),
            bounds(10.0, 0.0, 10.0, 20.0),
            bounds(20.0, 0.0, 10.0, 30.0),
        ]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().len(), 8);
        assert!((regions[0].area() - 600.0).abs() < 1.0e-9);
    }

    #[test]
    fn corner_touching_cutouts_trace_two_holes() {
        let material = subtract_rects(
            bounds(0.0, 0.0, 100.0, 50.0),
            &[bounds(10.0, 10.0, 10.0, 10.0), bounds(20.0, 20.0, 10.0, 10.0)],
        );
        let regions = regions_from_rects(&material);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 2);
        assert!((regions[0].area() - 4800.0).abs() < 1.0e-9);
    }

    #[test]
    fn area_ignores_ring_orientation() {
        let clockwise_outer =
            vec![DVec2::new(0.0, 0.0), DVec2::new(0.0, 10.0), DVec2::new(10.0, 10.0), DVec2::new(10.0, 0.0)];
        let region = Region::new(clockwise_outer, vec![rect(2.0, 2.0, 6.0, 6.0)]).expect("valid region");
        assert!((region.area() - 64.0).abs() < 1.0e-9);
    }
}
