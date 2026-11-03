//! Planar regions for panel machining.
//!
//! Stage 3 (ADR 0009, ADR 0012) models every through cutout on a panel as part of one removed region
//! `K`, and the material that stays is `C \ K` where `C` is the core outline. This module owns that
//! 2D boolean layer. A [`Region`] is one material component: a closed outer [`Contour`] ring and zero
//! or more hole rings, in millimetres.
//!
//! A ring stays exact: a line stays a line and an arc stays an arc, so the area, the bounds and the
//! containment test never depend on a sampling tolerance. [`Region::sample`] is the only lossy step,
//! and a caller picks its tolerance once, at the polygon-only kernel boundary.
//!
//! The rectangle helpers are the exact orthogonal boolean: [`union_rects`] merges overlapping
//! cutouts, [`subtract_rects`] removes them from a rectangular core including boundary notches, and
//! [`regions_from_rects`] traces the resulting closed rings with holes. [`union_regions`],
//! [`intersect_regions`] and [`subtract_regions`] are the general boolean: they keep lines and arcs
//! exact and trace the boundary of the set, so a rectangle and a shaped ring combine the same way.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt;

use crate::{Arc, Contour, ContourError, DVec2, Line, Segment};

/// Coincidence tolerance for ring points, in millimetres.
const POINT_EPSILON: f64 = 1.0e-9;

/// Tolerance for an arc angle to count as inside the sweep, in radians.
const ANGLE_EPSILON: f64 = 1.0e-9;

/// One material component in the plane: a closed outer ring and zero or more hole rings.
///
/// Every ring is an exact [`Contour`]; a ring built from sampled points is a contour of line
/// segments, so nothing is lost either way. Rings are validated on construction: each must be
/// closed. Holes are not checked against the outer ring here; [`regions_from_rects`] produces
/// consistent regions from the rectangle decomposition.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    outer: Contour,
    holes: Vec<Contour>,
}

impl Region {
    /// Creates a region from exact closed rings.
    pub fn new(outer: Contour, holes: Vec<Contour>) -> Result<Self, RegionError> {
        validate_ring(&outer, RingKind::Outer)?;
        for (index, hole) in holes.iter().enumerate() {
            validate_ring(hole, RingKind::Hole { index })?;
        }
        Ok(Self { outer, holes })
    }

    /// Creates a region from sampled rings: every closing edge becomes an exact line segment.
    ///
    /// This is the bridge for a traced polyline such as the rectangle decomposition. A ring with
    /// fewer than three points, a non-finite point or a repeated neighbour is refused with the
    /// matching [`RegionError`].
    pub fn from_loops(outer: Vec<DVec2>, holes: Vec<Vec<DVec2>>) -> Result<Self, RegionError> {
        let outer = loop_to_contour(outer, RingKind::Outer)?;
        let holes = holes
            .into_iter()
            .enumerate()
            .map(|(index, hole)| loop_to_contour(hole, RingKind::Hole { index }))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(outer, holes)
    }

    /// The region of one axis-aligned rectangle at the origin.
    ///
    /// The outer ring runs counter-clockwise from the minimum corner. A zero, negative or
    /// non-finite width or height is [`RectError`], so a caller can report the bad dimension.
    pub fn rectangle(width: f64, height: f64) -> Result<Self, RegionError> {
        let rect = Rect::new(DVec2::ZERO, DVec2::new(width, height))?;
        Self::from_rect(rect)
    }

    /// The region of an already-validated rectangle.
    pub fn from_rect(rect: Rect) -> Result<Self, RegionError> {
        let min = rect.min();
        let max = rect.max();
        let ring = vec![min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
        Self::from_loops(ring, Vec::new())
    }

    /// The outer ring.
    pub fn outer(&self) -> &Contour {
        &self.outer
    }

    /// The hole rings.
    pub fn holes(&self) -> &[Contour] {
        &self.holes
    }

    /// The material area: the outer ring's area minus every hole's area, in square millimetres.
    /// Ring orientation does not matter; an invalid region whose holes exceed the outer ring
    /// yields a non-positive value, which callers turn into a diagnostic, never a panic.
    pub fn area(&self) -> f64 {
        contour_area(&self.outer) - self.holes.iter().map(contour_area).sum::<f64>()
    }

    /// The axis-aligned bounds of every ring, in millimetres.
    ///
    /// Returns `None` only for a region with no rings, which the constructors reject. An arc
    /// contributes its own extremes, not its chord.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut bounds: Option<(DVec2, DVec2)> = None;
        for ring in std::iter::once(&self.outer).chain(self.holes.iter()) {
            let Some((min, max)) = ring.bounds() else {
                continue;
            };
            bounds = Some(match bounds {
                Some((current_min, current_max)) => (current_min.min(min), current_max.max(max)),
                None => (min, max),
            });
        }
        bounds
    }

    /// Even-odd containment: a point is inside the material when it is inside the outer ring and in
    /// no hole. The ray test walks exact lines and arcs, so a curved edge is never approximated.
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

    /// Samples every ring into a polyline at `tolerance` millimetres.
    ///
    /// This is the only lossy step in the module. A caller that reaches the polygon-only kernel
    /// picks the tolerance once and shares it with every other profile.
    pub fn sample(&self, tolerance: f64) -> Result<RegionSamples, ContourError> {
        let outer = self.outer.sample(tolerance)?;
        let holes = self.holes.iter().map(|hole| hole.sample(tolerance)).collect::<Result<Vec<_>, _>>()?;
        Ok(RegionSamples { outer, holes })
    }
}

/// A region sampled into polylines: the outer ring and then the hole rings in ring order.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionSamples {
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
}

impl RegionSamples {
    /// The sampled outer ring; the first point is not repeated at the end.
    pub fn outer(&self) -> &[DVec2] {
        &self.outer
    }

    /// The sampled hole rings.
    pub fn holes(&self) -> &[Vec<DVec2>] {
        &self.holes
    }

    /// Consumes the samples into the outer ring and the hole rings.
    pub fn into_parts(self) -> (Vec<DVec2>, Vec<Vec<DVec2>>) {
        (self.outer, self.holes)
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
    /// A ring is an open contour, so it bounds no material.
    OpenRing {
        /// The ring that is open.
        kind: RingKind,
    },
    /// A ring cannot become a contour of line segments.
    Ring {
        /// The ring that cannot be built.
        kind: RingKind,
        /// Why the contour was refused.
        error: ContourError,
    },
    /// A rectangle the region would be built from has no positive extent.
    Rect(RectError),
    /// A boolean would examine more segment pairs than it may.
    TooComplex {
        /// The number of segments the operands hold.
        segments: usize,
        /// The largest pair count one boolean accepts.
        max: usize,
    },
    /// A boolean result ring could not be built.
    ResultRing {
        /// Why the ring was refused.
        error: ContourError,
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
            Self::OpenRing { kind } => write!(formatter, "the {kind} ring is not closed"),
            Self::Ring { kind, error } => write!(formatter, "the {kind} ring is invalid: {error}"),
            Self::Rect(error) => write!(formatter, "{error}"),
            Self::TooComplex { segments, max } => write!(
                formatter,
                "a boolean over {segments} segments needs more than the {max} edge pairs it may examine"
            ),
            Self::ResultRing { error } => write!(formatter, "a boolean result ring is invalid: {error}"),
        }
    }
}

impl From<RectError> for RegionError {
    fn from(error: RectError) -> Self {
        Self::Rect(error)
    }
}

impl std::error::Error for RegionError {}

/// The unsigned area of one exact ring; zero for a ring that cannot be measured.
fn contour_area(ring: &Contour) -> f64 {
    ring.signed_area().unwrap_or(0.0).abs()
}

/// The signed shoelace area of a point loop; zero for a degenerate loop.
fn polygon_area(ring: &[DVec2]) -> f64 {
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

/// Even-odd containment by a ray to the right: an odd number of crossings is inside.
fn ring_contains(ring: &Contour, point: DVec2) -> bool {
    if !point.is_finite() {
        return false;
    }
    let mut inside = false;
    for segment in ring.segments() {
        if segment_crossings(*segment, point) % 2 == 1 {
            inside = !inside;
        }
    }
    inside
}

/// Crossings of one segment with the horizontal ray that runs right from `point`.
///
/// A segment counts its start and not its end, so a shared joint is counted once. A curve that
/// merely touches the ray is not a crossing.
fn segment_crossings(segment: Segment, point: DVec2) -> usize {
    match segment {
        Segment::Line(line) => line_crossings(line, point),
        Segment::Arc(arc) => arc_crossings(arc, point),
    }
}

fn line_crossings(line: Line, point: DVec2) -> usize {
    let start = line.start();
    let end = line.end();
    if (start.y > point.y) == (end.y > point.y) {
        return 0;
    }
    let parameter = (point.y - start.y) / (end.y - start.y);
    let x = start.x + (end.x - start.x) * parameter;
    usize::from(x > point.x)
}

/// Crossings of an arc with the horizontal ray, split at the arc's Y extremes.
///
/// Splitting at every `±π/2` angle makes each piece monotone in Y, so the same half-open rule as a
/// line applies and a joint between two pieces is counted once.
fn arc_crossings(arc: Arc, point: DVec2) -> usize {
    let total = arc.sweep_angle().abs();
    if !total.is_finite() || total <= ANGLE_EPSILON {
        return 0;
    }
    let mut fractions = vec![0.0_f64];
    for step in -2..=2 {
        let angle = FRAC_PI_2 + step as f64 * PI;
        let delta = if arc.sweep_angle() > 0.0 {
            turn_angle(angle - arc.start_angle())
        } else {
            turn_angle(arc.start_angle() - angle)
        };
        if delta > ANGLE_EPSILON && delta < total - ANGLE_EPSILON {
            fractions.push(delta / total);
        }
    }
    fractions.push(1.0);
    fractions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    fractions.dedup_by(|a, b| (*a - *b).abs() < ANGLE_EPSILON);

    let mut crossings = 0;
    for pair in fractions.windows(2) {
        let (Some(start), Some(end)) = (arc.point_at(pair[0]), arc.point_at(pair[1])) else {
            continue;
        };
        if (start.y > point.y) == (end.y > point.y) {
            continue;
        }
        let Some(middle) = arc.point_at((pair[0] + pair[1]) * 0.5) else {
            continue;
        };
        let side = (middle.x - arc.center().x).signum();
        if side == 0.0 {
            continue;
        }
        let ratio = ((point.y - arc.center().y) / arc.radius()).clamp(-1.0, 1.0);
        let x = arc.center().x + side * arc.radius() * (1.0 - ratio * ratio).max(0.0).sqrt();
        if x > point.x {
            crossings += 1;
        }
    }
    crossings
}

fn turn_angle(angle: f64) -> f64 {
    let mut value = angle % TAU;
    if value < 0.0 {
        value += TAU;
    }
    value
}

fn validate_ring(ring: &Contour, kind: RingKind) -> Result<(), RegionError> {
    if ring.is_closed() { Ok(()) } else { Err(RegionError::OpenRing { kind }) }
}

/// Builds an exact line contour from a point loop, closing it from the last point to the first.
fn loop_to_contour(points: Vec<DVec2>, kind: RingKind) -> Result<Contour, RegionError> {
    let mut points = points;
    if points.len() > 1
        && let (Some(first), Some(last)) = (points.first(), points.last())
        && (first.x - last.x).abs() <= POINT_EPSILON
        && (first.y - last.y).abs() <= POINT_EPSILON
    {
        points.pop();
    }
    if points.len() < 3 {
        return Err(RegionError::Degenerate { kind });
    }
    for (index, point) in points.iter().enumerate() {
        if !point.is_finite() {
            return Err(RegionError::NonFinite { kind, index });
        }
    }

    let mut segments = Vec::with_capacity(points.len());
    for (index, &start) in points.iter().enumerate() {
        let Some(&end) = points.get((index + 1) % points.len()) else {
            return Err(RegionError::Degenerate { kind });
        };
        let line = Line::new(start, end).map_err(|error| match error {
            ContourError::ZeroLength => RegionError::DuplicatePoint { kind, index },
            ContourError::InvalidInput(_) => RegionError::NonFinite { kind, index },
            error => RegionError::Ring { kind, error },
        })?;
        segments.push(Segment::Line(line));
    }
    match Contour::new(segments, true) {
        Ok(contour) => Ok(contour),
        Err(ContourError::Degenerate) => Err(RegionError::Degenerate { kind }),
        Err(error) => Err(RegionError::Ring { kind, error }),
    }
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
    let mut outers: Vec<(Contour, Vec<Vec<DVec2>>)> = Vec::new();
    let mut holes: Vec<Vec<DVec2>> = Vec::new();
    for ring in loops {
        let area = polygon_area(&ring);
        if area > POINT_EPSILON {
            if let Ok(contour) = loop_to_contour(ring, RingKind::Outer) {
                outers.push((contour, Vec::new()));
            }
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
    outers
        .into_iter()
        .filter_map(|(outer, holes)| {
            let holes = holes
                .into_iter()
                .enumerate()
                .map(|(index, hole)| loop_to_contour(hole, RingKind::Hole { index }))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            Region::new(outer, holes).ok()
        })
        .collect()
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

// ---------------------------------------------------------------------------------------------
// Exact contour booleans
// ---------------------------------------------------------------------------------------------

/// Largest number of segment pairs one boolean may examine before it refuses the input.
const MAX_BOOLEAN_PAIRS: usize = 4_000_000;

/// Distance at which two points of a boolean are the same vertex, in millimetres.
const VERTEX_TOLERANCE: f64 = 1.0e-6;

/// Distance from a boundary at which the boolean decides which side holds material, in millimetres.
///
/// It is far below the micrometre a document coordinate carries and far above the rounding of an
/// intersection, so the probe never crosses another boundary of the same input.
const SIDE_OFFSET: f64 = 1.0e-5;

/// Joint tolerance of a boolean result ring, in millimetres.
///
/// Two pieces meeting at a welded vertex can miss each other by the welding tolerance, so a result
/// ring closes on this bound instead of the strict geometry one.
const RESULT_JOINT_TOLERANCE: f64 = 1.0e-4;

/// Smallest area a traced loop must bound to become a ring, in square millimetres.
const LOOP_AREA_EPSILON: f64 = 1.0e-9;

/// The material covered by any of `regions`.
///
/// Overlapping and touching input merges into one outline; a result may still be several
/// disconnected components, one [`Region`] each. Rings stay exact: a line stays a line and an arc
/// stays an arc, so area, bounds and containment read the result without sampling.
pub fn union_regions(regions: &[Region]) -> Result<Vec<Region>, RegionError> {
    boolean_regions(regions, &[], BooleanOp::Union)
}

/// The material covered by both `first` and `second`.
///
/// The result can be several components and can keep rings of either input unchanged.
pub fn intersect_regions(first: &[Region], second: &[Region]) -> Result<Vec<Region>, RegionError> {
    boolean_regions(first, second, BooleanOp::Intersection)
}

/// The material of `from` that `remove` does not cover.
///
/// `remove` is subtracted as one union: overlapping tools need no separate merge, and a tool that
/// reaches the boundary of `from` opens a notch instead of leaving a sliver.
pub fn subtract_regions(from: &[Region], remove: &[Region]) -> Result<Vec<Region>, RegionError> {
    boolean_regions(from, remove, BooleanOp::Difference)
}

/// Which membership rule a boolean result follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BooleanOp {
    /// In either operand.
    Union,
    /// In both operands.
    Intersection,
    /// In the first operand and not in the second.
    Difference,
}

impl BooleanOp {
    /// Whether a point with these operand memberships is inside the result.
    fn holds(self, first: bool, second: bool) -> bool {
        match self {
            Self::Union => first || second,
            Self::Intersection => first && second,
            Self::Difference => first && !second,
        }
    }
}

/// The boundary of the set the operation describes.
///
/// Every edge of both operands is oriented with its own material on the left, split at every point
/// where it meets another edge, and then kept only where the result's membership differs on its two
/// sides. That side test is what makes coincident, touching and overlapping edges need no special
/// case: an edge that material covers on both sides is not a boundary of the result.
fn boolean_regions(first: &[Region], second: &[Region], op: BooleanOp) -> Result<Vec<Region>, RegionError> {
    if second.is_empty() {
        match op {
            BooleanOp::Union if first.len() <= 1 => return Ok(first.to_vec()),
            BooleanOp::Intersection => return Ok(Vec::new()),
            BooleanOp::Difference => return Ok(first.to_vec()),
            BooleanOp::Union => {}
        }
    }
    if first.is_empty() {
        return match op {
            BooleanOp::Union => Ok(second.to_vec()),
            BooleanOp::Intersection | BooleanOp::Difference => Ok(Vec::new()),
        };
    }

    let mut segments: Vec<Segment> = Vec::new();
    for region in first.iter().chain(second.iter()) {
        collect_edges(region, &mut segments)?;
    }
    if segments.is_empty() {
        return Ok(Vec::new());
    }
    let pairs = segments.len().saturating_mul(segments.len().saturating_sub(1)) / 2;
    if pairs > MAX_BOOLEAN_PAIRS {
        return Err(RegionError::TooComplex { segments: segments.len(), max: MAX_BOOLEAN_PAIRS });
    }

    let mut vertices = VertexSet::default();
    let mut splits: Vec<Vec<Split>> = vec![Vec::new(); segments.len()];
    for first_index in 0..segments.len() {
        for second_index in (first_index + 1)..segments.len() {
            let (Some(a), Some(b)) = (segments.get(first_index).copied(), segments.get(second_index).copied()) else {
                continue;
            };
            if !bounds_touch(a.bounds(), b.bounds()) {
                continue;
            }
            for point in a.intersections_with(b).into_iter().flatten() {
                register_split(&mut vertices, &mut splits, first_index, a, point);
                register_split(&mut vertices, &mut splits, second_index, b, point);
            }
            // A joint of one edge can fall inside another: that is a T-junction or a coincident
            // overlap, and both edges must be split there for the walk to stay connected.
            for point in [b.start(), b.end()] {
                register_split(&mut vertices, &mut splits, first_index, a, point);
            }
            for point in [a.start(), a.end()] {
                register_split(&mut vertices, &mut splits, second_index, b, point);
            }
        }
    }

    let mut pieces: Vec<Piece> = Vec::new();
    for (index, segment) in segments.iter().copied().enumerate() {
        let Some(list) = splits.get_mut(index) else {
            continue;
        };
        split_segment(&mut vertices, segment, list, &mut pieces);
    }

    let kept = keep_boundary(pieces, first, second, op)?;
    let loops = trace_contour_loops(&kept);
    let contours = contours_of(loops, &kept)?;
    regions_of(contours)
}

/// Appends the edges of one region, oriented with the material on the left.
fn collect_edges(region: &Region, out: &mut Vec<Segment>) -> Result<(), RegionError> {
    push_oriented(region.outer(), true, RingKind::Outer, out)?;
    for (index, hole) in region.holes().iter().enumerate() {
        push_oriented(hole, false, RingKind::Hole { index }, out)?;
    }
    Ok(())
}

/// Appends one ring in the requested direction, reversing it when it runs the other way.
fn push_oriented(
    ring: &Contour,
    counter_clockwise: bool,
    kind: RingKind,
    out: &mut Vec<Segment>,
) -> Result<(), RegionError> {
    let Some(area) = ring.signed_area() else {
        return Err(RegionError::OpenRing { kind });
    };
    let oriented = if (area > 0.0) == counter_clockwise {
        ring.clone()
    } else {
        ring.reversed().map_err(|error| RegionError::Ring { kind, error })?
    };
    out.extend(oriented.segments().iter().copied());
    Ok(())
}

/// A point where one edge is split, with its place along that edge.
#[derive(Clone, Copy, Debug)]
struct Split {
    /// The vertex the split point was welded to.
    vertex: usize,
    /// The point on the edge's supporting curve.
    point: DVec2,
    /// The parameter along the edge: `0..=1` for a line, a travel angle for an arc.
    parameter: f64,
}

/// One directed piece of an input edge, with the vertices it joins.
#[derive(Clone, Copy, Debug)]
struct Piece {
    /// The vertex the piece starts at.
    start: usize,
    /// The vertex the piece ends at.
    end: usize,
    /// The exact line or arc.
    segment: Segment,
}

/// One directed piece of the result boundary, with the material on its left.
#[derive(Clone, Copy, Debug)]
struct KeptEdge {
    /// The vertex the edge starts at.
    start: usize,
    /// The vertex the edge ends at.
    end: usize,
    /// The exact line or arc, oriented with the result material on its left.
    segment: Segment,
}

/// Points welded into shared vertices within [`VERTEX_TOLERANCE`].
#[derive(Default)]
struct VertexSet {
    /// A grid of vertex ids by rounded position.
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// The first point seen for each vertex.
    points: Vec<DVec2>,
}

impl VertexSet {
    /// Returns the id of the vertex at `point`, welding it to a neighbour when one is close enough.
    fn insert(&mut self, point: DVec2) -> usize {
        let key = vertex_key(point);
        for step_x in -1..=1 {
            for step_y in -1..=1 {
                let Some(ids) = self.cells.get(&(key.0 + step_x, key.1 + step_y)) else {
                    continue;
                };
                for &id in ids {
                    let Some(existing) = self.points.get(id) else {
                        continue;
                    };
                    if (existing.x - point.x).abs() <= VERTEX_TOLERANCE
                        && (existing.y - point.y).abs() <= VERTEX_TOLERANCE
                    {
                        return id;
                    }
                }
            }
        }
        let id = self.points.len();
        self.points.push(point);
        self.cells.entry(key).or_default().push(id);
        id
    }
}

fn vertex_key(point: DVec2) -> (i64, i64) {
    ((point.x / VERTEX_TOLERANCE).round() as i64, (point.y / VERTEX_TOLERANCE).round() as i64)
}

fn bounds_touch(first: (DVec2, DVec2), second: (DVec2, DVec2)) -> bool {
    first.0.x <= second.1.x + VERTEX_TOLERANCE
        && second.0.x <= first.1.x + VERTEX_TOLERANCE
        && first.0.y <= second.1.y + VERTEX_TOLERANCE
        && second.0.y <= first.1.y + VERTEX_TOLERANCE
}

/// Records `point` as a split of the edge at `index` when it lies strictly inside it.
fn register_split(vertices: &mut VertexSet, splits: &mut [Vec<Split>], index: usize, segment: Segment, point: DVec2) {
    let Some(split) = split_point(vertices, segment, point) else {
        return;
    };
    let Some(list) = splits.get_mut(index) else {
        return;
    };
    if list.iter().any(|existing| existing.vertex == split.vertex) {
        return;
    }
    list.push(split);
}

/// Places `point` on the supporting curve of `segment`, or `None` when it is not strictly inside.
fn split_point(vertices: &mut VertexSet, segment: Segment, point: DVec2) -> Option<Split> {
    if !point.is_finite() {
        return None;
    }
    match segment {
        Segment::Line(line) => {
            if !line.contains_point(point, VERTEX_TOLERANCE) {
                return None;
            }
            let foot = line.nearest_on_support(point)?;
            let length = line.length();
            if !length.is_finite() || length <= VERTEX_TOLERANCE {
                return None;
            }
            let parameter = ((foot - line.start()).length() / length).clamp(0.0, 1.0);
            if parameter * length <= VERTEX_TOLERANCE || (1.0 - parameter) * length <= VERTEX_TOLERANCE {
                return None;
            }
            Some(Split { vertex: vertices.insert(foot), point: foot, parameter })
        }
        Segment::Arc(arc) => {
            let foot = arc.nearest_on_support(point)?;
            if (foot - point).length() > VERTEX_TOLERANCE {
                return None;
            }
            let radial = foot - arc.center();
            let angle = radial.y.atan2(radial.x);
            let sweep = arc.sweep_angle().abs();
            let delta = if arc.sweep_angle() > 0.0 {
                turn_angle(angle - arc.start_angle())
            } else {
                turn_angle(arc.start_angle() - angle)
            };
            let margin = (VERTEX_TOLERANCE / arc.radius()).min(sweep * 0.5);
            if delta <= margin || delta >= sweep - margin {
                return None;
            }
            Some(Split { vertex: vertices.insert(foot), point: foot, parameter: delta })
        }
    }
}

/// Splits one edge at its recorded split points into directed pieces.
fn split_segment(vertices: &mut VertexSet, segment: Segment, splits: &mut Vec<Split>, out: &mut Vec<Piece>) {
    let start_point = segment.start();
    let end_point = segment.end();
    let start_vertex = vertices.insert(start_point);
    let end_vertex = vertices.insert(end_point);

    splits.retain(|split| split.vertex != start_vertex && split.vertex != end_vertex);
    splits.sort_by(|a, b| a.parameter.total_cmp(&b.parameter));
    splits.dedup_by(|a, b| a.vertex == b.vertex);

    if splits.is_empty() {
        out.push(Piece { start: start_vertex, end: end_vertex, segment });
        return;
    }

    let last_parameter = match segment {
        Segment::Line(_) => 1.0,
        Segment::Arc(arc) => arc.sweep_angle().abs(),
    };
    let mut stops: Vec<Split> = Vec::with_capacity(splits.len() + 2);
    stops.push(Split { vertex: start_vertex, point: start_point, parameter: 0.0 });
    stops.extend(splits.iter().copied());
    stops.push(Split { vertex: end_vertex, point: end_point, parameter: last_parameter });

    for pair in stops.windows(2) {
        let (Some(from), Some(to)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if from.vertex == to.vertex {
            continue;
        }
        let Some(piece) = piece_between(segment, *from, *to) else {
            continue;
        };
        out.push(Piece { start: from.vertex, end: to.vertex, segment: piece });
    }
}

/// The sub-segment of `segment` that runs from one stop to the next.
fn piece_between(segment: Segment, from: Split, to: Split) -> Option<Segment> {
    match segment {
        Segment::Line(_) => Line::new(from.point, to.point).ok().map(Segment::Line),
        Segment::Arc(arc) => {
            let span = to.parameter - from.parameter;
            if !span.is_finite() || span <= 0.0 {
                return None;
            }
            let radial = from.point - arc.center();
            let start_angle = radial.y.atan2(radial.x);
            Arc::new(arc.center(), arc.radius(), start_angle, arc.sweep_angle().signum() * span).ok().map(Segment::Arc)
        }
    }
}

/// Keeps the pieces that bound the result, oriented with the result material on the left.
fn keep_boundary(
    pieces: Vec<Piece>,
    first: &[Region],
    second: &[Region],
    op: BooleanOp,
) -> Result<Vec<KeptEdge>, RegionError> {
    let mut kept: Vec<KeptEdge> = Vec::new();
    for piece in pieces {
        let Some(left) = side_point(piece.segment, true) else {
            continue;
        };
        let Some(right) = side_point(piece.segment, false) else {
            continue;
        };
        let left_inside = op.holds(set_contains(first, left), set_contains(second, left));
        let right_inside = op.holds(set_contains(first, right), set_contains(second, right));
        if left_inside == right_inside {
            continue;
        }
        if left_inside {
            kept.push(KeptEdge { start: piece.start, end: piece.end, segment: piece.segment });
        } else {
            let segment = piece.segment.reversed().map_err(|error| RegionError::ResultRing { error })?;
            kept.push(KeptEdge { start: piece.end, end: piece.start, segment });
        }
    }
    Ok(dedupe_edges(kept))
}

/// Whether any region of the set covers `point`.
fn set_contains(regions: &[Region], point: DVec2) -> bool {
    regions.iter().any(|region| region.contains(point))
}

/// A point `SIDE_OFFSET` to the left (`left = true`) or right of the middle of `segment`.
fn side_point(segment: Segment, left: bool) -> Option<DVec2> {
    let middle = segment.point_at(0.5)?;
    let tangent = tangent_at_middle(segment)?;
    let normal = DVec2::new(-tangent.y, tangent.x);
    let offset = if left { SIDE_OFFSET } else { -SIDE_OFFSET };
    Some(middle + normal * offset)
}

fn tangent_at_middle(segment: Segment) -> Option<DVec2> {
    match segment {
        Segment::Line(line) => Some(line.direction()),
        Segment::Arc(arc) => {
            let point = arc.point_at(0.5)?;
            let radial = point - arc.center();
            let length = radial.length();
            if !length.is_finite() || length <= 0.0 {
                return None;
            }
            let radial = radial / length;
            Some(DVec2::new(-radial.y, radial.x) * arc.sweep_angle().signum())
        }
    }
}

/// Drops the second copy of an edge that two operands both contribute.
///
/// Two copies of one boundary always run the same way: a piece kept in one direction has the result
/// material on its left, so the opposite direction would have been dropped by the side test.
fn dedupe_edges(edges: Vec<KeptEdge>) -> Vec<KeptEdge> {
    let mut buckets: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    let mut unique: Vec<KeptEdge> = Vec::new();
    for edge in edges {
        let bucket = buckets.entry((edge.start, edge.end)).or_default();
        if bucket.iter().any(|&index| unique.get(index).is_some_and(|other| same_curve(other.segment, edge.segment))) {
            continue;
        }
        bucket.push(unique.len());
        unique.push(edge);
    }
    unique
}

/// Whether two edges between the same vertices are the same curve.
fn same_curve(first: Segment, second: Segment) -> bool {
    match (first, second) {
        (Segment::Line(_), Segment::Line(_)) => true,
        (Segment::Arc(a), Segment::Arc(b)) => {
            (a.center() - b.center()).length() <= VERTEX_TOLERANCE
                && (a.radius() - b.radius()).abs() <= VERTEX_TOLERANCE
                && a.sweep_angle().signum() == b.sweep_angle().signum()
        }
        _ => false,
    }
}

/// Closes the kept edges into closed walks of edge ids.
///
/// At a vertex with several ways on, the walk takes the first edge clockwise from the reversal of
/// the edge it arrived on, which hugs the material on the left and keeps a corner-only contact on
/// two loops.
fn trace_contour_loops(edges: &[KeptEdge]) -> Vec<Vec<usize>> {
    let mut outgoing: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.start).or_default().push(index);
    }

    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for start in 0..edges.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some(first) = edges.get(start) else {
            continue;
        };
        let start_vertex = first.start;
        let mut path = Vec::new();
        let mut current = start;
        let mut guard = 0usize;
        loop {
            path.push(current);
            if let Some(edge) = used.get_mut(current) {
                *edge = true;
            }
            let Some(at) = edges.get(current).map(|edge| edge.end) else {
                break;
            };
            if at == start_vertex {
                break;
            }
            guard += 1;
            if guard > edges.len() {
                break;
            }
            let Some(next) = next_contour_edge(at, edges[current].segment, &outgoing, edges, &used) else {
                break;
            };
            current = next;
        }
        if edges.get(current).map(|edge| edge.end) == Some(start_vertex) && !path.is_empty() {
            loops.extend(split_self_touching_contours(path, edges));
        }
    }
    loops
}

fn next_contour_edge(
    at: usize,
    incoming: Segment,
    outgoing: &HashMap<usize, Vec<usize>>,
    edges: &[KeptEdge],
    used: &[bool],
) -> Option<usize> {
    let candidates = outgoing.get(&at)?;
    let reverse = -incoming.tangent_at_end();
    let reverse_angle = reverse.y.atan2(reverse.x);
    let mut best: Option<(f64, usize)> = None;
    for &index in candidates {
        if used.get(index).copied().unwrap_or(true) {
            continue;
        }
        let Some(edge) = edges.get(index) else {
            continue;
        };
        let tangent = edge.segment.tangent_at_start();
        let angle = tangent.y.atan2(tangent.x);
        let turn = turn_angle(angle - reverse_angle);
        // Clockwise from the reverse means the largest counter-clockwise angle; a U-turn doubles
        // back along the incoming edge and bounds no material, so it goes last.
        let turn = if turn <= ANGLE_EPSILON { TAU } else { turn };
        if best.is_none_or(|(current, _)| turn > current) {
            best = Some((turn, index));
        }
    }
    best.map(|(_, index)| index)
}

/// Splits a closed walk that visits a vertex twice into simple rings.
fn split_self_touching_contours(path: Vec<usize>, edges: &[KeptEdge]) -> Vec<Vec<usize>> {
    let mut pending = vec![path];
    let mut simple = Vec::new();
    while let Some(path) = pending.pop() {
        let Some(first) = path.first().and_then(|&index| edges.get(index)) else {
            continue;
        };
        let mut vertices = Vec::with_capacity(path.len() + 1);
        vertices.push(first.start);
        for &index in &path {
            let Some(edge) = edges.get(index) else {
                continue;
            };
            vertices.push(edge.end);
        }

        let mut seen: HashMap<usize, usize> = HashMap::new();
        let mut repeat = None;
        for (index, vertex) in vertices.iter().enumerate().take(vertices.len().saturating_sub(1)) {
            if let Some(&earlier) = seen.get(vertex) {
                repeat = Some((earlier, index));
                break;
            }
            seen.insert(*vertex, index);
        }

        let Some((earlier, later)) = repeat else {
            simple.push(path);
            continue;
        };
        let cycle = path[earlier..later].to_vec();
        let mut rest = path[..earlier].to_vec();
        rest.extend_from_slice(&path[later..]);
        if !cycle.is_empty() {
            pending.push(cycle);
        }
        if !rest.is_empty() {
            pending.push(rest);
        }
    }
    simple
}

/// Builds one closed contour per loop, dropping loops that bound no area.
fn contours_of(loops: Vec<Vec<usize>>, edges: &[KeptEdge]) -> Result<Vec<Contour>, RegionError> {
    let mut contours = Vec::new();
    for path in loops {
        let segments: Vec<Segment> =
            path.iter().filter_map(|&index| edges.get(index)).map(|edge| edge.segment).collect();
        if segments.is_empty() {
            continue;
        }
        let segments = simplify_loop(segments);
        let contour = Contour::new_with_tolerance(segments, true, RESULT_JOINT_TOLERANCE)
            .map_err(|error| RegionError::ResultRing { error })?;
        if contour.signed_area().is_none_or(|area| area.abs() <= LOOP_AREA_EPSILON) {
            continue;
        }
        contours.push(contour);
    }
    Ok(contours)
}

/// Joins consecutive pieces that lie on one supporting line or circle.
///
/// A split point another operand's joint needed leaves a collinear vertex behind; merging keeps a
/// union, a notch or a hole as simple as the shape it describes.
fn simplify_loop(segments: Vec<Segment>) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::with_capacity(segments.len());
    for segment in segments {
        if let Some(last) = merged.last_mut()
            && let Some(joined) = join_segments(*last, segment)
        {
            *last = joined;
            continue;
        }
        merged.push(segment);
    }
    if merged.len() > 1 {
        let first = merged[0];
        let last = merged[merged.len() - 1];
        if let Some(joined) = join_segments(last, first) {
            merged.pop();
            merged[0] = joined;
        }
    }
    merged
}

/// The single segment covering two consecutive pieces, or `None` when they bend.
fn join_segments(first: Segment, second: Segment) -> Option<Segment> {
    match (first, second) {
        (Segment::Line(a), Segment::Line(b)) => {
            let direction = a.direction();
            let next = b.direction();
            if direction.perp_dot(next).abs() > ANGLE_EPSILON || direction.dot(next) <= 0.0 {
                return None;
            }
            Line::new(a.start(), b.end()).ok().map(Segment::Line)
        }
        (Segment::Arc(a), Segment::Arc(b)) => {
            if (a.center() - b.center()).length() > VERTEX_TOLERANCE
                || (a.radius() - b.radius()).abs() > VERTEX_TOLERANCE
                || a.sweep_angle().signum() != b.sweep_angle().signum()
            {
                return None;
            }
            let sweep = a.sweep_angle().abs() + b.sweep_angle().abs();
            if sweep > TAU + ANGLE_EPSILON {
                return None;
            }
            Arc::new(a.center(), a.radius(), a.start_angle(), a.sweep_angle().signum() * sweep).ok().map(Segment::Arc)
        }
        _ => None,
    }
}

/// Groups the result rings into regions, pairing each hole with the smallest ring that holds it.
fn regions_of(contours: Vec<Contour>) -> Result<Vec<Region>, RegionError> {
    let mut outers: Vec<(Contour, Vec<Contour>)> = Vec::new();
    let mut holes: Vec<Contour> = Vec::new();
    for contour in contours {
        match contour.signed_area() {
            Some(area) if area > 0.0 => outers.push((contour, Vec::new())),
            _ => holes.push(contour),
        }
    }

    for hole in holes {
        let Some(segment) = hole.segments().first().copied() else {
            continue;
        };
        let Some(point) = side_point(segment, true) else {
            continue;
        };
        let mut best: Option<(f64, usize)> = None;
        for (index, (outer, _)) in outers.iter().enumerate() {
            if !ring_contains(outer, point) {
                continue;
            }
            let area = contour_area(outer);
            if best.is_none_or(|(current, _)| area < current) {
                best = Some((area, index));
            }
        }
        let Some((_, index)) = best else {
            continue;
        };
        if let Some((_, holes)) = outers.get_mut(index) {
            holes.push(hole);
        }
    }

    let mut regions =
        outers.into_iter().map(|(outer, holes)| Region::new(outer, holes)).collect::<Result<Vec<_>, _>>()?;
    regions.sort_by(compare_regions);
    Ok(regions)
}

fn compare_regions(first: &Region, second: &Region) -> std::cmp::Ordering {
    let key = |region: &Region| region.bounds().map_or((0.0, 0.0, 0.0, 0.0), |(min, max)| (min.x, min.y, max.x, max.y));
    let (a_min_x, a_min_y, a_max_x, a_max_y) = key(first);
    let (b_min_x, b_min_y, b_max_x, b_max_y) = key(second);
    a_min_x
        .total_cmp(&b_min_x)
        .then(a_min_y.total_cmp(&b_min_y))
        .then(a_max_x.total_cmp(&b_max_x))
        .then(a_max_y.total_cmp(&b_max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<DVec2> {
        vec![DVec2::new(x, y), DVec2::new(x + width, y), DVec2::new(x + width, y + height), DVec2::new(x, y + height)]
    }

    fn rounded_corner() -> f64 {
        let radius = 20.0;
        radius * radius - PI * radius * radius / 4.0
    }

    fn rounded_plate() -> Region {
        let radius = 20.0;
        let segments = vec![
            Segment::Line(Line::new(DVec2::ZERO, DVec2::new(100.0, 0.0)).expect("bottom")),
            Segment::Line(Line::new(DVec2::new(100.0, 0.0), DVec2::new(100.0, 50.0 - radius)).expect("right")),
            Segment::Arc(Arc::new(DVec2::new(100.0 - radius, 50.0 - radius), radius, 0.0, FRAC_PI_2).expect("corner")),
            Segment::Line(Line::new(DVec2::new(100.0 - radius, 50.0), DVec2::new(0.0, 50.0)).expect("top")),
            Segment::Line(Line::new(DVec2::new(0.0, 50.0), DVec2::ZERO).expect("left")),
        ];
        Region::new(Contour::new(segments, true).expect("rounded plate"), vec![]).expect("region")
    }

    fn rect_with_hole() -> Region {
        Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![rect(10.0, 10.0, 20.0, 10.0)]).expect("a rect with a hole")
    }

    #[test]
    fn a_rectangle_has_its_area() {
        let region = Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![]).expect("valid rect");
        assert!((region.area() - 5000.0).abs() < 1.0e-9);
    }

    #[test]
    fn the_rectangle_constructor_runs_counter_clockwise_from_the_origin() {
        let region = Region::rectangle(100.0, 50.0).expect("valid rectangle");
        assert_eq!(region.outer().segments().len(), 4);
        assert_eq!(region.outer().start(), Some(DVec2::ZERO));
        assert!(region.outer().is_counter_clockwise() == Some(true));
        assert_eq!(region.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 50.0))));
        assert!((region.area() - 5000.0).abs() < 1.0e-9);
        assert!(region.contains(DVec2::new(50.0, 25.0)));
        assert!(!region.contains(DVec2::new(150.0, 25.0)));
    }

    #[test]
    fn a_rectangle_sampled_through_the_region_keeps_its_four_corners() {
        let region = Region::rectangle(100.0, 50.0).expect("valid rectangle");
        let samples = region.sample(0.05).expect("samples");
        assert_eq!(samples.outer(), &rect(0.0, 0.0, 100.0, 50.0)[..]);
        assert!(samples.holes().is_empty());
        let (outer, holes) = samples.into_parts();
        assert_eq!(outer.len(), 4);
        assert!(holes.is_empty());
    }

    #[test]
    fn a_rectangle_without_positive_extent_is_rejected() {
        assert!(matches!(Region::rectangle(0.0, 10.0), Err(RegionError::Rect(RectError::Degenerate))));
        assert!(matches!(Region::rectangle(10.0, -1.0), Err(RegionError::Rect(RectError::Degenerate))));
        assert!(matches!(Region::rectangle(f64::NAN, 10.0), Err(RegionError::Rect(RectError::NonFinite))));
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
        assert!(matches!(Region::from_loops(vec![], vec![]), Err(RegionError::Degenerate { kind: RingKind::Outer })));
        assert!(matches!(
            Region::from_loops(rect(0.0, 0.0, 100.0, 50.0), vec![vec![]]),
            Err(RegionError::Degenerate { kind: RingKind::Hole { index: 0 } })
        ));
    }

    #[test]
    fn duplicate_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(0.0, 10.0)];
        assert!(matches!(
            Region::from_loops(ring, vec![]),
            Err(RegionError::DuplicatePoint { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn non_finite_ring_points_are_rejected() {
        let ring = vec![DVec2::new(0.0, 0.0), DVec2::new(f64::NAN, 5.0), DVec2::new(3.0, 0.0)];
        assert!(matches!(
            Region::from_loops(ring, vec![]),
            Err(RegionError::NonFinite { kind: RingKind::Outer, index: 1 })
        ));
    }

    #[test]
    fn an_open_ring_is_rejected() {
        let open = Contour::new(
            vec![
                Segment::Line(Line::new(DVec2::ZERO, DVec2::new(10.0, 0.0)).expect("bottom")),
                Segment::Line(Line::new(DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)).expect("right")),
            ],
            false,
        )
        .expect("open path");
        assert!(matches!(Region::new(open, vec![]), Err(RegionError::OpenRing { kind: RingKind::Outer })));
    }

    #[test]
    fn an_exact_ring_answers_area_bounds_and_containment_without_sampling() {
        let region = rounded_plate();
        assert!((region.area() - (5000.0 - rounded_corner())).abs() < 1.0e-9);
        assert_eq!(region.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 50.0))));
        assert!(region.contains(DVec2::new(50.0, 25.0)));
        assert!(!region.contains(DVec2::new(95.0, 45.0)), "the rounded corner is cut away");
        let samples = region.sample(0.05).expect("samples");
        assert!(samples.outer().len() > region.outer().segments().len());
    }

    #[test]
    fn an_arc_ring_contains_points_by_its_own_curve() {
        let ring = Contour::new(
            vec![
                Segment::Line(Line::new(DVec2::ZERO, DVec2::new(20.0, 0.0)).expect("bottom")),
                Segment::Arc(Arc::new(DVec2::new(20.0, 10.0), 10.0, -FRAC_PI_2, PI).expect("right half")),
                Segment::Line(Line::new(DVec2::new(20.0, 20.0), DVec2::ZERO).expect("left")),
            ],
            true,
        )
        .expect("arc ring");
        let region = Region::new(ring, vec![]).expect("region");
        assert!(region.contains(DVec2::new(28.0, 10.0)), "inside the bulge");
        assert!(region.contains(DVec2::new(22.0, 10.0)), "inside the bulge");
        assert!(!region.contains(DVec2::new(32.0, 10.0)), "right of the bulge");
        assert!(region.contains(DVec2::new(10.0, 5.0)), "inside the triangle");
        assert!(!region.contains(DVec2::new(5.0, 10.0)), "above the diagonal");
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
        assert_eq!(regions[0].outer().segments().len(), 4);
        assert!((regions[0].area() - 100.0).abs() < 1.0e-9);
        assert!(regions[0].outer().signed_area().is_some_and(|area| area > 0.0));
        assert!(regions[0].contains(DVec2::new(5.0, 5.0)));
    }

    #[test]
    fn touching_rectangles_trace_one_outline() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 0.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().segments().len(), 4);
        assert!((regions[0].area() - 200.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_partial_shared_edge_traces_a_step() {
        let regions = regions_from_rects(&[bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 5.0, 10.0, 10.0)]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].outer().segments().len(), 8);
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
        assert!(regions[0].holes()[0].signed_area().is_some_and(|area| area < 0.0));
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
        assert_eq!(regions[0].outer().segments().len(), 8);
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
        assert_eq!(regions[0].outer().segments().len(), 8);
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
        assert_eq!(regions[0].outer().segments().len(), 8);
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
        let region = Region::from_loops(clockwise_outer, vec![rect(2.0, 2.0, 6.0, 6.0)]).expect("valid region");
        assert!((region.area() - 64.0).abs() < 1.0e-9);
    }

    // -- exact contour booleans ------------------------------------------------------------------

    fn rect_region(x: f64, y: f64, width: f64, height: f64) -> Region {
        Region::from_loops(rect(x, y, width, height), Vec::new()).expect("a rectangle region")
    }

    fn circle_ring(center: DVec2, radius: f64) -> Contour {
        Contour::new(vec![Segment::Arc(Arc::new(center, radius, 0.0, TAU).expect("a circle arc"))], true)
            .expect("a circle ring")
    }

    fn circle_region(center: DVec2, radius: f64) -> Region {
        Region::new(circle_ring(center, radius), Vec::new()).expect("a circle region")
    }

    fn total(regions: &[Region]) -> f64 {
        regions.iter().map(Region::area).sum()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1.0e-6, "area {actual}, expected {expected}");
    }

    #[test]
    fn union_merges_overlapping_regions() {
        let regions =
            union_regions(&[rect_region(0.0, 0.0, 10.0, 10.0), rect_region(5.0, 5.0, 10.0, 10.0)]).expect("union");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 175.0);
        assert_eq!(regions[0].outer().segments().len(), 8);
        assert!(regions[0].holes().is_empty());
    }

    #[test]
    fn union_drops_a_shared_edge() {
        let regions =
            union_regions(&[rect_region(0.0, 0.0, 10.0, 10.0), rect_region(10.0, 0.0, 10.0, 10.0)]).expect("union");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 200.0);
        assert_eq!(regions[0].outer().segments().len(), 4);
    }

    #[test]
    fn union_keeps_disjoint_components() {
        let regions =
            union_regions(&[rect_region(0.0, 0.0, 10.0, 10.0), rect_region(20.0, 0.0, 10.0, 10.0)]).expect("union");
        assert_eq!(regions.len(), 2);
        assert_close(total(&regions), 200.0);
    }

    #[test]
    fn union_keeps_a_corner_only_contact_on_two_components() {
        let regions =
            union_regions(&[rect_region(0.0, 0.0, 10.0, 10.0), rect_region(10.0, 10.0, 10.0, 10.0)]).expect("union");
        assert_eq!(regions.len(), 2);
        assert_close(total(&regions), 200.0);
    }

    #[test]
    fn union_of_identical_regions_returns_one_outline() {
        let region = rect_region(0.0, 0.0, 10.0, 10.0);
        let regions = union_regions(&[region.clone(), region]).expect("union");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 100.0);
        assert_eq!(regions[0].outer().segments().len(), 4);
    }

    #[test]
    fn intersection_keeps_the_shared_area() {
        let regions = intersect_regions(&[rect_region(0.0, 0.0, 10.0, 10.0)], &[rect_region(5.0, 5.0, 10.0, 10.0)])
            .expect("intersection");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 25.0);
        assert_eq!(regions[0].outer().segments().len(), 4);
    }

    #[test]
    fn intersection_of_touching_regions_is_empty() {
        let regions = intersect_regions(&[rect_region(0.0, 0.0, 10.0, 10.0)], &[rect_region(10.0, 0.0, 10.0, 10.0)])
            .expect("intersection");
        assert!(regions.is_empty());
    }

    #[test]
    fn intersection_of_identical_regions_returns_one_outline() {
        let region = rect_region(0.0, 0.0, 10.0, 10.0);
        let regions =
            intersect_regions(std::slice::from_ref(&region), std::slice::from_ref(&region)).expect("intersection");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 100.0);
    }

    #[test]
    fn subtraction_removes_an_interior_cutout_as_a_hole() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[rect_region(20.0, 10.0, 20.0, 10.0)])
            .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 4800.0);
        assert_eq!(regions[0].holes().len(), 1);
        assert!(regions[0].holes()[0].signed_area().is_some_and(|area| area < 0.0));
        assert!(!regions[0].contains(DVec2::new(30.0, 15.0)));
        assert!(regions[0].contains(DVec2::new(10.0, 10.0)));
    }

    #[test]
    fn subtraction_of_a_region_across_the_boundary_opens_a_notch() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[rect_region(30.0, 40.0, 40.0, 20.0)])
            .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 4600.0);
        assert!(regions[0].holes().is_empty());
        assert_eq!(regions[0].outer().segments().len(), 8);
    }

    #[test]
    fn subtraction_that_splits_the_core_returns_two_components() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[rect_region(-10.0, 20.0, 120.0, 10.0)])
            .expect("subtraction");
        assert_eq!(regions.len(), 2);
        assert_close(total(&regions), 4000.0);
    }

    #[test]
    fn subtraction_of_identical_regions_is_empty() {
        let region = rect_region(0.0, 0.0, 10.0, 10.0);
        let regions =
            subtract_regions(std::slice::from_ref(&region), std::slice::from_ref(&region)).expect("subtraction");
        assert!(regions.is_empty());
    }

    #[test]
    fn subtraction_of_a_flush_cutout_keeps_the_remaining_rectangle() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 10.0, 10.0)], &[rect_region(5.0, 0.0, 10.0, 10.0)])
            .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 50.0);
        assert_eq!(regions[0].outer().segments().len(), 4);
        assert_eq!(regions[0].bounds(), Some((DVec2::ZERO, DVec2::new(5.0, 10.0))));
    }

    #[test]
    fn subtraction_of_a_disjoint_cutout_changes_nothing() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 10.0, 10.0)], &[rect_region(50.0, 50.0, 5.0, 5.0)])
            .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 100.0);
        assert_eq!(regions[0].outer().segments().len(), 4);
    }

    #[test]
    fn subtraction_of_a_cutout_that_covers_the_core_leaves_nothing() {
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 10.0, 10.0)], &[rect_region(-5.0, -5.0, 20.0, 20.0)])
            .expect("subtraction");
        assert!(regions.is_empty());
    }

    #[test]
    fn overlapping_cutouts_are_subtracted_as_one_union() {
        let regions = subtract_regions(
            &[rect_region(0.0, 0.0, 100.0, 50.0)],
            &[rect_region(10.0, 10.0, 20.0, 20.0), rect_region(20.0, 20.0, 20.0, 20.0)],
        )
        .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 5000.0 - 700.0);
        assert_eq!(regions[0].holes().len(), 1);
    }

    #[test]
    fn a_circular_cutout_subtracts_exactly() {
        let regions =
            subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[circle_region(DVec2::new(50.0, 25.0), 10.0)])
                .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 5000.0 - PI * 100.0);
        assert_eq!(regions[0].holes().len(), 1);
        let hole = &regions[0].holes()[0];
        assert!(hole.segments().iter().any(|segment| matches!(segment, Segment::Arc(_))));
        assert!(!regions[0].contains(DVec2::new(50.0, 25.0)));
        assert!(regions[0].contains(DVec2::new(20.0, 25.0)));
    }

    #[test]
    fn a_circular_cutout_across_the_edge_opens_a_curved_notch() {
        let regions =
            subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[circle_region(DVec2::new(50.0, 0.0), 10.0)])
                .expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 5000.0 - PI * 50.0);
        assert!(regions[0].holes().is_empty());
        assert!(regions[0].outer().segments().iter().any(|segment| matches!(segment, Segment::Arc(_))));
        assert!(!regions[0].contains(DVec2::new(50.0, 5.0)));
    }

    #[test]
    fn union_of_a_rectangle_and_a_circle_keeps_the_arc() {
        let regions = union_regions(&[rect_region(0.0, 0.0, 10.0, 10.0), circle_region(DVec2::new(10.0, 5.0), 5.0)])
            .expect("union");
        assert_eq!(regions.len(), 1);
        assert_close(total(&regions), 100.0 + PI * 25.0 / 2.0);
        assert!(regions[0].outer().segments().iter().any(|segment| matches!(segment, Segment::Arc(_))));
    }

    #[test]
    fn union_of_two_overlapping_circles_keeps_exact_arcs() {
        let regions = union_regions(&[circle_region(DVec2::ZERO, 10.0), circle_region(DVec2::new(10.0, 0.0), 10.0)])
            .expect("union");
        assert_eq!(regions.len(), 1);
        let lens = 200.0 * (0.5_f64).acos() - 5.0 * (400.0_f64 - 100.0).sqrt();
        assert_close(total(&regions), 200.0 * PI - lens);
        assert!(regions[0].outer().segments().iter().any(|segment| matches!(segment, Segment::Arc(_))));
        assert!(regions[0].contains(DVec2::new(-5.0, 0.0)));
        assert!(regions[0].contains(DVec2::new(15.0, 0.0)));
        assert!(!regions[0].contains(DVec2::new(5.0, 9.0)));
    }

    #[test]
    fn a_hole_in_the_removed_region_leaves_an_island() {
        let annulus = Region::new(
            circle_ring(DVec2::new(50.0, 50.0), 20.0),
            vec![circle_ring(DVec2::new(50.0, 50.0), 10.0).reversed().expect("a reversed hole")],
        )
        .expect("an annulus");
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 100.0)], &[annulus]).expect("subtraction");
        assert_eq!(regions.len(), 2);
        assert_close(total(&regions), 10_000.0 - 300.0 * PI);
        let holed = regions.iter().filter(|region| region.holes().len() == 1).count();
        assert_eq!(holed, 1);
        assert!(regions.iter().any(|region| region.contains(DVec2::new(50.0, 50.0))));
    }

    #[test]
    fn an_empty_operand_leaves_the_other_side_alone() {
        let region = rect_region(0.0, 0.0, 10.0, 10.0);
        assert_eq!(union_regions(std::slice::from_ref(&region)).expect("union"), vec![region.clone()]);
        assert_eq!(subtract_regions(std::slice::from_ref(&region), &[]).expect("subtraction"), vec![region.clone()]);
        assert!(intersect_regions(&[region], &[]).expect("intersection").is_empty());
        assert!(union_regions(&[]).expect("union").is_empty());
    }

    #[test]
    fn a_hole_touching_the_outer_ring_at_one_point_stays_a_hole() {
        let notch = Region::from_loops(
            vec![DVec2::new(50.0, 50.0), DVec2::new(60.0, 40.0), DVec2::new(40.0, 40.0)],
            Vec::new(),
        )
        .expect("a triangle");
        let regions = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 50.0)], &[notch]).expect("subtraction");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 1);
        assert_close(total(&regions), 4900.0);
        assert!(!regions[0].contains(DVec2::new(50.0, 45.0)));
        assert!(regions[0].contains(DVec2::new(20.0, 20.0)));
    }

    #[test]
    fn a_bar_across_a_ring_hole_splits_it_into_two_holes() {
        let annulus = Region::new(
            circle_ring(DVec2::new(50.0, 50.0), 20.0),
            vec![circle_ring(DVec2::new(50.0, 50.0), 10.0).reversed().expect("a reversed hole")],
        )
        .expect("an annulus");
        let mut parts = subtract_regions(&[rect_region(0.0, 0.0, 100.0, 100.0)], &[annulus]).expect("ring cut");
        parts.push(rect_region(25.0, 49.0, 50.0, 2.0));
        let regions = union_regions(&parts).expect("union with a bar");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].holes().len(), 2);
        // The bar keeps the strip of the ring it crosses; the exact strip has curved ends.
        let strip = |t: f64| {
            t / 2.0 * (400.0 - t * t).sqrt() + 200.0 * (t / 20.0).asin()
                - t / 2.0 * (100.0 - t * t).sqrt()
                - 50.0 * (t / 10.0).asin()
        };
        let kept = 2.0 * (strip(1.0) - strip(-1.0));
        assert_close(total(&regions), 10_000.0 - 300.0 * PI + kept);
        assert!(regions[0].contains(DVec2::new(50.0, 50.0)));
        assert!(!regions[0].contains(DVec2::new(50.0, 40.0)));
    }

    #[test]
    fn a_boolean_result_is_ordered_by_position() {
        let regions =
            union_regions(&[rect_region(50.0, 0.0, 10.0, 10.0), rect_region(0.0, 0.0, 10.0, 10.0)]).expect("union");
        assert_eq!(regions.len(), 2);
        assert!(regions[0].bounds().is_some_and(|(min, _)| min.x < 1.0));
        assert!(regions[1].bounds().is_some_and(|(min, _)| min.x > 49.0));
    }
}
