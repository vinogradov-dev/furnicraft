//! Baked polygon meshes and their topology limits.
//!
//! A mesh is plain data: positions in micrometres and faces as index lists. Validation is total, so
//! a hostile file is a readable error and never an out-of-bounds index or an unbounded allocation.
//! The renderer and the geometry kernel consume this value; the document only stores it.

use std::collections::HashSet;

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Largest accepted number of vertices in one mesh.
pub const MAX_MESH_VERTICES: usize = 1_000_000;

/// Largest accepted number of polygon faces in one mesh.
pub const MAX_MESH_FACES: usize = 1_000_000;

/// Largest accepted number of vertices in one polygon face.
pub const MAX_FACE_VERTICES: usize = 1_024;

/// Sides around a curved primitive. A multiple of four, so the silhouette has axis points.
pub const PRIMITIVE_SEGMENTS: usize = 32;

/// Latitude bands between the two poles of a sphere.
pub const PRIMITIVE_SPHERE_BANDS: usize = 16;

/// The sine and cosine of one segment around a ring, in the XY plane.
fn ring_angle(segment: usize, segments: usize) -> (f64, f64) {
    let angle = std::f64::consts::TAU * segment as f64 / segments as f64;
    angle.sin_cos()
}

/// Rounds a float micrometre coordinate into a document length, refusing a non-finite value.
fn millimetres_to_length(value: f64) -> Result<Length, MeshError> {
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(MeshError::NonFiniteCoordinate);
    }
    Ok(Length::from_micrometres(value.round() as i64))
}

/// A rejected mesh.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MeshError {
    /// The mesh has no vertices or no faces.
    #[error("a mesh must have at least one vertex and one face")]
    Empty,
    /// The mesh has more vertices than the limit allows.
    #[error("mesh has {count} vertices, the limit is {max}")]
    TooManyVertices {
        /// The supplied vertex count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// The mesh has more faces than the limit allows.
    #[error("mesh has {count} faces, the limit is {max}")]
    TooManyFaces {
        /// The supplied face count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// A face has too few or too many vertices.
    #[error("face {face} has {count} vertices, expected between 3 and {max}")]
    FaceVertexCount {
        /// The face index.
        face: usize,
        /// The supplied vertex count.
        count: usize,
        /// The supported maximum.
        max: usize,
    },
    /// A face names the same vertex twice.
    #[error("face {face} repeats vertex {vertex}")]
    FaceRepeatedVertex {
        /// The face index.
        face: usize,
        /// The repeated vertex index.
        vertex: u32,
    },
    /// A face names a vertex that does not exist.
    #[error("face {face} refers to vertex {vertex} of {vertices}")]
    FaceIndexOutOfBounds {
        /// The face index.
        face: usize,
        /// The rejected vertex index.
        vertex: u32,
        /// The vertex count of the mesh.
        vertices: usize,
    },
    /// A vertex index is outside the mesh.
    #[error("vertex {vertex} is outside the {count} vertices of the mesh")]
    VertexIndexOutOfBounds {
        /// The rejected vertex index.
        vertex: usize,
        /// The vertex count of the mesh.
        count: usize,
    },
    /// A primitive was asked for with a size that is not positive.
    #[error("primitive size must be positive, got {edge} micrometres")]
    NonPositiveSize {
        /// The rejected size in micrometres.
        edge: i64,
    },
    /// A computed primitive coordinate is not finite or does not fit a length.
    #[error("primitive coordinate is not a representable length")]
    NonFiniteCoordinate,
}

/// Serialized form of [`Mesh`]; validation happens in its `TryFrom` implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MeshWire {
    positions: Vec<[Length; 3]>,
    faces: Vec<Vec<u32>>,
}

/// A baked polygon mesh: the geometry payload of a mesh object.
///
/// Positions are local coordinates in the object's frame. Faces are polygons, not triangles: the
/// document keeps quads and larger faces so editing a box does not degrade it, and triangulation
/// happens only when the mesh is turned into a renderable drawable. A mesh is the source of truth;
/// it has no feature history, and undo is its only record ([ADR 0037]).
///
/// Validation is bounded and total: a hostile file cannot ask for an allocation it did not pay for,
/// and every index is checked before it is used.
///
/// [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MeshWire")]
pub struct Mesh {
    positions: Vec<[Length; 3]>,
    faces: Vec<Vec<u32>>,
}

impl Mesh {
    /// Validates and stores a mesh.
    pub fn new(positions: Vec<[Length; 3]>, faces: Vec<Vec<u32>>) -> Result<Self, MeshError> {
        if positions.is_empty() || faces.is_empty() {
            return Err(MeshError::Empty);
        }
        if positions.len() > MAX_MESH_VERTICES {
            return Err(MeshError::TooManyVertices { count: positions.len(), max: MAX_MESH_VERTICES });
        }
        if faces.len() > MAX_MESH_FACES {
            return Err(MeshError::TooManyFaces { count: faces.len(), max: MAX_MESH_FACES });
        }
        for (index, face) in faces.iter().enumerate() {
            if !(3..=MAX_FACE_VERTICES).contains(&face.len()) {
                return Err(MeshError::FaceVertexCount { face: index, count: face.len(), max: MAX_FACE_VERTICES });
            }
            let mut seen = HashSet::with_capacity(face.len());
            for vertex in face {
                if !seen.insert(*vertex) {
                    return Err(MeshError::FaceRepeatedVertex { face: index, vertex: *vertex });
                }
                if *vertex as usize >= positions.len() {
                    return Err(MeshError::FaceIndexOutOfBounds {
                        face: index,
                        vertex: *vertex,
                        vertices: positions.len(),
                    });
                }
            }
        }
        Ok(Self { positions, faces })
    }

    /// The local vertex positions.
    pub fn positions(&self) -> &[[Length; 3]] {
        &self.positions
    }

    /// The polygon faces, as vertex indices.
    pub fn faces(&self) -> &[Vec<u32>] {
        &self.faces
    }

    /// Replaces one vertex position; returns an error for an unknown index.
    pub fn set_position(&mut self, index: usize, position: [Length; 3]) -> Result<(), MeshError> {
        let Some(slot) = self.positions.get_mut(index) else {
            return Err(MeshError::VertexIndexOutOfBounds { vertex: index, count: self.positions.len() });
        };
        *slot = position;
        Ok(())
    }

    /// The minimum and maximum local corners of the vertex cloud, in micrometres.
    pub fn bounds_micrometres(&self) -> ([i64; 3], [i64; 3]) {
        let mut min = [i64::MAX; 3];
        let mut max = [i64::MIN; 3];
        for position in &self.positions {
            for (axis, value) in position.iter().enumerate() {
                let value = value.micrometres();
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        (min, max)
    }

    /// Builds a cube of `edge` length centred on the object's local origin.
    ///
    /// The cube is the first primitive; its six quads are counter-clockwise seen from outside. An
    /// odd edge length loses at most one micrometre to integer halving, which no panel rule cares
    /// about.
    pub fn cube(edge: Length) -> Result<Self, MeshError> {
        let half = edge.micrometres() / 2;
        if half <= 0 {
            return Err(MeshError::NonPositiveSize { edge: edge.micrometres() });
        }
        let corner = |x: i64, y: i64, z: i64| {
            [Length::from_micrometres(x), Length::from_micrometres(y), Length::from_micrometres(z)]
        };
        let positions = vec![
            corner(-half, -half, -half),
            corner(half, -half, -half),
            corner(half, half, -half),
            corner(-half, half, -half),
            corner(-half, -half, half),
            corner(half, -half, half),
            corner(half, half, half),
            corner(-half, half, half),
        ];
        Self::new(
            positions,
            vec![
                vec![0, 3, 2, 1],
                vec![4, 5, 6, 7],
                vec![0, 1, 5, 4],
                vec![1, 2, 6, 5],
                vec![2, 3, 7, 6],
                vec![3, 0, 4, 7],
            ],
        )
    }

    /// Builds an upright cylinder whose diameter and height both equal `edge`.
    ///
    /// The sides are [`PRIMITIVE_SEGMENTS`] quads and the two caps are one polygon each, so a later
    /// face or vertex edit has real faces to work with instead of a triangle fan. Every ring runs
    /// counter-clockwise seen from outside, and the side quads share their vertices with the caps.
    pub fn cylinder(edge: Length) -> Result<Self, MeshError> {
        let radius = edge.micrometres() / 2;
        let half = radius;
        if radius <= 0 || half <= 0 {
            return Err(MeshError::NonPositiveSize { edge: edge.micrometres() });
        }
        let segments = PRIMITIVE_SEGMENTS;
        let mut positions = Vec::with_capacity(segments * 2);
        for z in [-half, half] {
            for segment in 0..segments {
                let (sin, cos) = ring_angle(segment, segments);
                positions.push([
                    millimetres_to_length(cos * radius as f64)?,
                    millimetres_to_length(sin * radius as f64)?,
                    Length::from_micrometres(z),
                ]);
            }
        }
        let mut faces = Vec::with_capacity(segments + 2);
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            let bottom = u32::try_from(segment).unwrap_or(0);
            let bottom_next = u32::try_from(next).unwrap_or(0);
            let top = u32::try_from(segments + segment).unwrap_or(0);
            let top_next = u32::try_from(segments + next).unwrap_or(0);
            faces.push(vec![bottom, bottom_next, top_next, top]);
        }
        let bottom = (0..segments).rev().filter_map(|index| u32::try_from(index).ok()).collect::<Vec<_>>();
        let top = (segments..segments * 2).filter_map(|index| u32::try_from(index).ok()).collect::<Vec<_>>();
        faces.push(bottom);
        faces.push(top);
        Self::new(positions, faces)
    }

    /// Builds a sphere whose diameter is `edge`, as latitude bands.
    ///
    /// The poles are single vertices and every other latitude is a ring of [`PRIMITIVE_SEGMENTS`]
    /// vertices, joined by triangles at the poles and quads in between. Latitude 0 is the equator.
    pub fn sphere(edge: Length) -> Result<Self, MeshError> {
        let radius = edge.micrometres() / 2;
        if radius <= 0 {
            return Err(MeshError::NonPositiveSize { edge: edge.micrometres() });
        }
        let segments = PRIMITIVE_SEGMENTS;
        let bands = PRIMITIVE_SPHERE_BANDS;
        let mut positions = Vec::with_capacity(segments * (bands - 1) + 2);
        positions.push([Length::ZERO, Length::ZERO, Length::from_micrometres(-radius)]);
        for band in 1..bands {
            let z = -radius as f64 + 2.0 * radius as f64 * band as f64 / bands as f64;
            let ring = ((radius as f64).powi(2) - z * z).max(0.0).sqrt();
            for segment in 0..segments {
                let (sin, cos) = ring_angle(segment, segments);
                positions.push([
                    millimetres_to_length(cos * ring)?,
                    millimetres_to_length(sin * ring)?,
                    Length::from_micrometres(z.round() as i64),
                ]);
            }
        }
        let top = u32::try_from(positions.len()).unwrap_or(0);
        positions.push([Length::ZERO, Length::ZERO, Length::from_micrometres(radius)]);
        let ring_index = |band: usize, segment: usize| -> u32 {
            let index = 1 + (band - 1) * segments + segment % segments;
            u32::try_from(index).unwrap_or(0)
        };
        let mut faces = Vec::new();
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            faces.push(vec![0, ring_index(1, next), ring_index(1, segment)]);
        }
        for band in 1..bands - 1 {
            for segment in 0..segments {
                let next = (segment + 1) % segments;
                faces.push(vec![
                    ring_index(band, segment),
                    ring_index(band, next),
                    ring_index(band + 1, next),
                    ring_index(band + 1, segment),
                ]);
            }
        }
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            faces.push(vec![top, ring_index(bands - 1, segment), ring_index(bands - 1, next)]);
        }
        Self::new(positions, faces)
    }
}

impl TryFrom<MeshWire> for Mesh {
    type Error = MeshError;

    fn try_from(wire: MeshWire) -> Result<Self, Self::Error> {
        Self::new(wire.positions, wire.faces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn mesh_rejects_bad_topology() {
        let p = millimetres;
        let square = vec![[p(0), p(0), p(0)], [p(1), p(0), p(0)], [p(1), p(1), p(0)]];
        assert_eq!(Mesh::new(Vec::new(), vec![vec![0, 1, 2]]), Err(MeshError::Empty));
        assert_eq!(Mesh::new(square.clone(), Vec::new()), Err(MeshError::Empty));
        assert_eq!(
            Mesh::new(square.clone(), vec![vec![0, 1]]),
            Err(MeshError::FaceVertexCount { face: 0, count: 2, max: MAX_FACE_VERTICES })
        );
        assert_eq!(
            Mesh::new(square.clone(), vec![vec![0, 1, 1]]),
            Err(MeshError::FaceRepeatedVertex { face: 0, vertex: 1 })
        );
        assert_eq!(
            Mesh::new(square, vec![vec![0, 1, 3]]),
            Err(MeshError::FaceIndexOutOfBounds { face: 0, vertex: 3, vertices: 3 })
        );
    }

    #[test]
    fn mesh_caps_the_vertices_of_one_face() {
        let p = millimetres;
        let positions: Vec<[Length; 3]> = (0..=MAX_FACE_VERTICES).map(|index| [p(index as i64), p(0), p(0)]).collect();
        let face: Vec<u32> = (0..=MAX_FACE_VERTICES as u32).collect();
        assert_eq!(
            Mesh::new(positions, vec![face]),
            Err(MeshError::FaceVertexCount { face: 0, count: MAX_FACE_VERTICES + 1, max: MAX_FACE_VERTICES })
        );
    }

    /// The outward normal of a polygon, from its vertex order (Newell's method).
    fn normal(mesh: &Mesh, face: &[u32]) -> [f64; 3] {
        let point = |index: u32| {
            let position = mesh.positions()[index as usize];
            [position[0].millimetres_f64(), position[1].millimetres_f64(), position[2].millimetres_f64()]
        };
        let mut normal = [0.0; 3];
        for (index, vertex) in face.iter().enumerate() {
            let current = point(*vertex);
            let next = point(face[(index + 1) % face.len()]);
            normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
            normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
            normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
        }
        normal
    }

    /// Every face of a convex primitive centred on the origin points away from it.
    fn assert_outward(mesh: &Mesh) {
        for (index, face) in mesh.faces().iter().enumerate() {
            let mut centroid = [0.0; 3];
            for vertex in face {
                let position = mesh.positions()[*vertex as usize];
                for axis in 0..3 {
                    centroid[axis] += position[axis].millimetres_f64() / face.len() as f64;
                }
            }
            let normal = normal(mesh, face);
            let dot = (0..3).map(|axis| normal[axis] * centroid[axis]).sum::<f64>();
            assert!(dot > 0.0, "face {index} at {centroid:?} has normal {normal:?} pointing inward");
        }
    }

    #[test]
    fn a_cylinder_is_a_capped_tube_of_the_full_edge_length() {
        let edge = millimetres(100);
        let mesh = Mesh::cylinder(edge).expect("a cylinder");
        assert_eq!(mesh.positions().len(), PRIMITIVE_SEGMENTS * 2);
        assert_eq!(mesh.faces().len(), PRIMITIVE_SEGMENTS + 2, "one quad per side and two caps");
        assert_eq!(mesh.faces().last().map(Vec::len), Some(PRIMITIVE_SEGMENTS), "the top cap is one polygon");
        assert_eq!(mesh.bounds_micrometres(), ([-50_000, -50_000, -50_000], [50_000, 50_000, 50_000]));
        assert_outward(&mesh);

        let max_radius = mesh
            .positions()
            .iter()
            .map(|point| (point[0].millimetres_f64().powi(2) + point[1].millimetres_f64().powi(2)).sqrt())
            .fold(0.0_f64, f64::max);
        assert!((max_radius - 50.0).abs() < 0.001, "the diameter is the edge length");
    }

    #[test]
    fn a_sphere_is_a_closed_banded_surface_of_the_full_edge_length() {
        let edge = millimetres(100);
        let mesh = Mesh::sphere(edge).expect("a sphere");
        assert_eq!(mesh.positions().len(), 2 + PRIMITIVE_SEGMENTS * (PRIMITIVE_SPHERE_BANDS - 1));
        let triangles = PRIMITIVE_SEGMENTS * 2;
        let quads = PRIMITIVE_SEGMENTS * (PRIMITIVE_SPHERE_BANDS - 2);
        assert_eq!(mesh.faces().len(), triangles + quads, "a ring of triangles at each pole and quads between");
        assert_eq!(mesh.bounds_micrometres(), ([-50_000, -50_000, -50_000], [50_000, 50_000, 50_000]));
        assert_outward(&mesh);

        let max_radius = mesh
            .positions()
            .iter()
            .map(|point| {
                (point[0].millimetres_f64().powi(2)
                    + point[1].millimetres_f64().powi(2)
                    + point[2].millimetres_f64().powi(2))
                .sqrt()
            })
            .fold(0.0_f64, f64::max);
        assert!((max_radius - 50.0).abs() < 0.001, "the diameter is the edge length");
    }

    #[test]
    fn a_curved_primitive_needs_a_positive_edge() {
        for edge in [Length::ZERO, Length::from_micrometres(-100)] {
            assert_eq!(Mesh::cylinder(edge), Err(MeshError::NonPositiveSize { edge: edge.micrometres() }));
            assert_eq!(Mesh::sphere(edge), Err(MeshError::NonPositiveSize { edge: edge.micrometres() }));
        }
    }
}
