//! Triangulation of one planar face.
//!
//! A document face can be concave: the cap of an extrusion follows the contour, and a contour with
//! an inward notch has a reflex vertex. Fanning the face from its first vertex is only valid while
//! every vertex is convex, so this module cuts ears instead and keeps every triangle inside the
//! face.
//!
//! Coordinates are millimetres. The face is projected onto the plane where its Newell normal is the
//! dominant axis: exact for a planar face, a best effort for a slightly skewed one. A face that
//! crosses itself has no inside, so it is reported as [`TriangulateError::NotSimple`] rather than
//! covered with overlapping triangles.

use std::fmt;

use crate::DVec2;
use crate::DVec3;

/// Upper bound on the points of one face; a larger face is bad data, not a render job.
pub const MAX_FACE_POINTS: usize = 4096;

/// Two projected points closer than this are the same point, in millimetres.
const POINT_EPSILON: f64 = 1.0e-9;
/// A triangle corner whose doubled area is below this is collinear, in square millimetres.
const AREA_EPSILON: f64 = 1.0e-12;
/// A point closer than this to an ear edge still blocks that ear, in square millimetres.
const INSIDE_EPSILON: f64 = 1.0e-9;

/// Why a face cannot be triangulated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriangulateError {
    /// The face has fewer than three points.
    TooFewPoints {
        /// How many points the face has.
        count: usize,
    },
    /// The face has more points than [`MAX_FACE_POINTS`].
    TooManyPoints {
        /// How many points the face has.
        count: usize,
    },
    /// A point is not finite.
    NonFinite {
        /// Index of the offending point.
        index: usize,
    },
    /// The points are collinear, repeated or otherwise cover no area.
    Degenerate,
    /// The face crosses itself, so it has no inside.
    NotSimple,
}

impl fmt::Display for TriangulateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewPoints { count } => write!(formatter, "a face needs three points, it has {count}"),
            Self::TooManyPoints { count } => {
                write!(formatter, "a face has {count} points, the limit is {MAX_FACE_POINTS}")
            }
            Self::NonFinite { index } => write!(formatter, "point {index} of the face is not finite"),
            Self::Degenerate => formatter.write_str("the face covers no area"),
            Self::NotSimple => formatter.write_str("the face crosses itself"),
        }
    }
}

impl std::error::Error for TriangulateError {}

/// Triangulates one planar face into triangles of vertex indices.
///
/// The triangles follow the winding of the input: a counter-clockwise face yields triangles whose
/// normals point along its Newell normal, a clockwise one the other way. A face with `n` points
/// yields exactly `n - 2` triangles; repeated and collinear points are dropped from the work ring,
/// so the count can be lower when the face carries such points.
pub fn triangulate_face(points: &[DVec3]) -> Result<Vec<[u32; 3]>, TriangulateError> {
    if points.len() < 3 {
        return Err(TriangulateError::TooFewPoints { count: points.len() });
    }
    if points.len() > MAX_FACE_POINTS {
        return Err(TriangulateError::TooManyPoints { count: points.len() });
    }
    if let Some(index) = points.iter().position(|point| !point.is_finite()) {
        return Err(TriangulateError::NonFinite { index });
    }
    let Some(normal) = newell_normal(points) else {
        return Err(TriangulateError::Degenerate);
    };
    let (u, v) = projection_axes(normal);
    let projected: Vec<DVec2> = points.iter().map(|point| project(*point, u, v)).collect();
    let area = ring_area(&projected);
    if area.abs() <= AREA_EPSILON {
        return Err(TriangulateError::Degenerate);
    }
    let orientation = area.signum();
    let mut ring: Vec<usize> = (0..points.len()).collect();
    ear_clip(&projected, &mut ring, orientation)
}

/// Newell's normal of a polygon, or `None` when it covers no area.
fn newell_normal(points: &[DVec3]) -> Option<DVec3> {
    let mut normal = DVec3::ZERO;
    for (index, current) in points.iter().enumerate() {
        let next = points.get((index + 1) % points.len())?;
        normal.x += (current.y - next.y) * (current.z + next.z);
        normal.y += (current.z - next.z) * (current.x + next.x);
        normal.z += (current.x - next.x) * (current.y + next.y);
    }
    let length = normal.length();
    (length.is_finite() && length > POINT_EPSILON).then(|| normal / length)
}

/// The two axes to keep, chosen so the projected area keeps the sign of the normal's dominant axis.
fn projection_axes(normal: DVec3) -> (usize, usize) {
    let axes = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
    let dominant = axes.iter().enumerate().fold(0, |best, (index, value)| {
        if *value > axes.get(best).copied().unwrap_or(f64::NEG_INFINITY) { index } else { best }
    });
    match dominant {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    }
}

fn project(point: DVec3, u: usize, v: usize) -> DVec2 {
    let coordinates = [point.x, point.y, point.z];
    DVec2::new(coordinates.get(u).copied().unwrap_or(0.0), coordinates.get(v).copied().unwrap_or(0.0))
}

/// Signed shoelace area of the projected ring, in square millimetres.
fn ring_area(ring: &[DVec2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut doubled = 0.0;
    for index in 0..ring.len() {
        let Some(current) = ring.get(index) else {
            continue;
        };
        let Some(next) = ring.get((index + 1) % ring.len()) else {
            continue;
        };
        doubled += current.perp_dot(*next);
    }
    doubled * 0.5
}

/// Twice the signed area of the corner `previous`, `current`, `next`.
fn corner_area(previous: DVec2, current: DVec2, next: DVec2) -> f64 {
    (current - previous).perp_dot(next - current)
}

/// Cuts ears off the ring until three points are left.
fn ear_clip(projected: &[DVec2], ring: &mut Vec<usize>, orientation: f64) -> Result<Vec<[u32; 3]>, TriangulateError> {
    let mut triangles = Vec::with_capacity(ring.len().saturating_sub(2));
    let mut guard = 0usize;
    let limit = ring.len().saturating_mul(ring.len()).saturating_add(1);
    while ring.len() > 3 {
        guard += 1;
        if guard > limit {
            return Err(TriangulateError::NotSimple);
        }
        match find_ear(projected, ring, orientation) {
            Some(position) => {
                let Some((previous, current, next)) = neighbours(ring, position) else {
                    return Err(TriangulateError::NotSimple);
                };
                triangles.push([to_index(previous)?, to_index(current)?, to_index(next)?]);
                ring.remove(position);
            }
            None => {
                let Some(position) = find_collinear(projected, ring) else {
                    return Err(TriangulateError::NotSimple);
                };
                ring.remove(position);
            }
        }
    }
    let (Some(first), Some(second), Some(third)) = (ring.first(), ring.get(1), ring.get(2)) else {
        return Err(TriangulateError::NotSimple);
    };
    triangles.push([to_index(*first)?, to_index(*second)?, to_index(*third)?]);
    Ok(triangles)
}

/// The ring indices around `position`, wrapping at both ends.
fn neighbours(ring: &[usize], position: usize) -> Option<(usize, usize, usize)> {
    let length = ring.len();
    if position >= length || length < 3 {
        return None;
    }
    let previous = *ring.get((position + length - 1) % length)?;
    let current = *ring.get(position)?;
    let next = *ring.get((position + 1) % length)?;
    Some((previous, current, next))
}

/// The first ring position whose corner is convex and holds no other ring point.
fn find_ear(projected: &[DVec2], ring: &[usize], orientation: f64) -> Option<usize> {
    for position in 0..ring.len() {
        let (previous, current, next) = neighbours(ring, position)?;
        let (Some(a), Some(b), Some(c)) = (projected.get(previous), projected.get(current), projected.get(next)) else {
            continue;
        };
        if corner_area(*a, *b, *c) * orientation <= AREA_EPSILON {
            continue;
        }
        let mut blocked = false;
        for index in ring {
            if *index == previous || *index == current || *index == next {
                continue;
            }
            let Some(point) = projected.get(*index) else {
                continue;
            };
            if point_inside(*a, *b, *c, *point, orientation) {
                blocked = true;
                break;
            }
        }
        if !blocked {
            return Some(position);
        }
    }
    None
}

/// Whether the point is strictly inside the triangle, with the triangle's own winding.
fn point_inside(a: DVec2, b: DVec2, c: DVec2, point: DVec2, orientation: f64) -> bool {
    let ab = (b - a).perp_dot(point - a) * orientation;
    let bc = (c - b).perp_dot(point - b) * orientation;
    let ca = (a - c).perp_dot(point - c) * orientation;
    ab > INSIDE_EPSILON && bc > INSIDE_EPSILON && ca > INSIDE_EPSILON
}

/// A ring position whose corner covers no area, so removing it changes nothing.
fn find_collinear(projected: &[DVec2], ring: &[usize]) -> Option<usize> {
    for position in 0..ring.len() {
        let (previous, current, next) = neighbours(ring, position)?;
        let (Some(a), Some(b), Some(c)) = (projected.get(previous), projected.get(current), projected.get(next)) else {
            continue;
        };
        if corner_area(*a, *b, *c).abs() <= AREA_EPSILON {
            return Some(position);
        }
    }
    None
}

fn to_index(index: usize) -> Result<u32, TriangulateError> {
    u32::try_from(index).map_err(|_| TriangulateError::TooManyPoints { count: index })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A 100 x 100 square with a 20 x 60 notch cut into its top edge, counter-clockwise.
    fn notched_square() -> Vec<DVec3> {
        [
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (60.0, 100.0),
            (60.0, 40.0),
            (40.0, 40.0),
            (40.0, 100.0),
            (0.0, 100.0),
        ]
        .into_iter()
        .map(|(x, y)| DVec3::new(x, y, 0.0))
        .collect()
    }

    fn triangle_area(a: DVec3, b: DVec3, c: DVec3) -> f64 {
        (b - a).cross(c - a).length() * 0.5
    }

    #[test]
    fn a_concave_face_triangulates_inside_itself() {
        let ring = notched_square();
        let triangles = triangulate_face(&ring).expect("a simple face");
        assert_eq!(triangles.len(), ring.len() - 2, "a simple n-gon has n - 2 triangles");
        let mut area = 0.0;
        for triangle in &triangles {
            let [a, b, c] = triangle.map(|index| ring.get(index as usize).copied().expect("a known vertex"));
            let patch = triangle_area(a, b, c);
            assert!(patch > 0.0, "every triangle is counter-clockwise like the face");
            area += patch;
        }
        let expected = 100.0 * 100.0 - 20.0 * 60.0;
        assert!((area - expected).abs() < 1.0e-6, "the triangles cover the face, area was {area}");
        for triangle in &triangles {
            let [a, b, c] = triangle.map(|index| ring.get(index as usize).copied().expect("a known vertex"));
            let centroid = (a + b + c) / 3.0;
            assert!(
                point_in_ring(&ring, DVec2::new(centroid.x, centroid.y)),
                "the centroid {centroid:?} of triangle {triangle:?} left the notch"
            );
        }
    }

    #[test]
    fn a_clockwise_face_keeps_its_winding() {
        let mut ring = notched_square();
        ring.reverse();
        let triangles = triangulate_face(&ring).expect("a simple face");
        assert_eq!(triangles.len(), ring.len() - 2);
        let mut area = 0.0;
        for triangle in &triangles {
            let [a, b, c] = triangle.map(|index| ring.get(index as usize).copied().expect("a known vertex"));
            area += (b - a).cross(c - a).z * 0.5;
        }
        let expected = -(100.0 * 100.0 - 20.0 * 60.0);
        assert!((area - expected).abs() < 1.0e-6, "clockwise input stays clockwise, area was {area}");
    }

    #[test]
    fn a_tilted_face_is_projected_on_its_dominant_plane() {
        let flat = notched_square();
        let rotation = crate::DQuat::from_rotation_x(0.7) * crate::DQuat::from_rotation_y(0.4);
        let tilted = flat.iter().map(|point| rotation * *point).collect::<Vec<_>>();
        let triangles = triangulate_face(&tilted).expect("a tilted planar face");
        assert_eq!(triangles.len(), flat.len() - 2);
        let mut area = 0.0;
        for triangle in &triangles {
            let [a, b, c] = triangle.map(|index| tilted.get(index as usize).copied().expect("a known vertex"));
            area += triangle_area(a, b, c);
        }
        let expected = 100.0 * 100.0 - 20.0 * 60.0;
        assert!((area - expected).abs() < 1.0e-6, "a rigid turn keeps the area, it was {area}");
    }

    #[test]
    fn a_convex_face_still_triangulates() {
        let square = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let triangles = triangulate_face(&square).expect("a square");
        assert_eq!(triangles.len(), 2);
    }

    #[test]
    fn a_repeated_or_collinear_point_does_not_break_the_face() {
        let mut ring = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let triangles = triangulate_face(&ring).expect("a square with a stray point");
        assert!((covered_area(&ring, &triangles) - 100.0).abs() < 1.0e-6, "the stray point changes no area");
        ring.push(DVec3::new(0.0, 10.0, 0.0));
        let triangles = triangulate_face(&ring).expect("a square with a repeated point");
        assert!((covered_area(&ring, &triangles) - 100.0).abs() < 1.0e-6, "a repeated point adds no area");
        assert!(triangles.iter().all(|triangle| {
            let [a, b, c] = triangle.map(|index| ring.get(index as usize).copied().expect("a known vertex"));
            triangle_area(a, b, c).is_finite()
        }));
    }

    /// Total area of the triangles, each corner taken from the face it belongs to.
    fn covered_area(ring: &[DVec3], triangles: &[[u32; 3]]) -> f64 {
        triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| ring.get(index as usize).copied().expect("a known vertex"));
                triangle_area(a, b, c)
            })
            .sum()
    }

    #[test]
    fn a_face_without_area_or_with_too_few_points_is_rejected() {
        let collinear = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), DVec3::new(20.0, 0.0, 0.0)];
        assert_eq!(triangulate_face(&collinear), Err(TriangulateError::Degenerate));
        assert_eq!(triangulate_face(&collinear[..2]), Err(TriangulateError::TooFewPoints { count: 2 }));
        let mut non_finite = notched_square();
        non_finite[1] = DVec3::new(f64::NAN, 0.0, 0.0);
        assert_eq!(triangulate_face(&non_finite), Err(TriangulateError::NonFinite { index: 1 }));
    }

    /// Even-odd ray casting against the XY ring, in the test only.
    fn point_in_ring(ring: &[DVec3], point: DVec2) -> bool {
        let mut inside = false;
        for (index, current) in ring.iter().enumerate() {
            let Some(next) = ring.get((index + 1) % ring.len()) else {
                continue;
            };
            if (current.y > point.y) != (next.y > point.y) {
                let at = (next.x - current.x) * (point.y - current.y) / (next.y - current.y) + current.x;
                if point.x < at {
                    inside = !inside;
                }
            }
        }
        inside
    }
}
