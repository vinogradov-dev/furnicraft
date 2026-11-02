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
}
