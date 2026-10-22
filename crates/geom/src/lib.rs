//! Geometry foundation shared by every FurniCraft crate.
//!
//! Vectors and matrices are `glam` types re-exported here; FurniCraft-specific shapes
//! (frames, bounds, meshes) live in this crate. Physical values are millimetres at this
//! boundary; the document keeps `furnicraft_units::Length` (micrometres).
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub use glam::{DMat4, DQuat, DVec2, DVec3, DVec4};

/// A right-handed local frame: `x` and `y` span the profile plane, `z` is the extrusion axis.
///
/// Coordinates are millimetres. The frame is always orthonormal with `z = x × y`; constructors
/// reject degenerate input instead of producing a skewed frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Frame origin in world space.
    pub origin: DVec3,
    /// Unit vector of the local X axis.
    pub x: DVec3,
    /// Unit vector of the local Y axis.
    pub y: DVec3,
    /// Unit vector of the local Z axis (`x × y`).
    pub z: DVec3,
}

impl Frame {
    /// The world frame: origin at zero, axes aligned with world X/Y/Z.
    pub const WORLD: Frame = Frame { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, z: DVec3::Z };

    /// Builds a frame from an origin and two direction vectors.
    ///
    /// The directions are normalised and orthogonalised; `y` is projected onto the plane
    /// perpendicular to `x`. Returns `None` for a non-finite input or a zero-length direction.
    pub fn from_origin_axes(origin: DVec3, x: DVec3, y: DVec3) -> Option<Frame> {
        if !origin.is_finite() || !x.is_finite() || !y.is_finite() {
            return None;
        }
        let x = x.try_normalize()?;
        let y = (y - x * x.dot(y)).try_normalize()?;
        let z = x.cross(y);
        Some(Frame { origin, x, y, z })
    }

    /// Maps a point in the local profile plane to world space.
    pub fn point(&self, u: f64, v: f64) -> DVec3 {
        self.origin + self.x * u + self.y * v
    }

    /// Returns the frame translated along its local Z axis.
    pub fn offset(&self, distance: f64) -> Frame {
        Frame { origin: self.origin + self.z * distance, ..*self }
    }

    /// Checks that the axes are finite, unit length and mutually perpendicular.
    pub fn is_orthonormal(&self) -> bool {
        let unit = |v: DVec3| v.is_finite() && (v.length() - 1.0).abs() <= 1e-9;
        unit(self.x)
            && unit(self.y)
            && unit(self.z)
            && self.x.dot(self.y).abs() <= 1e-9
            && self.x.cross(self.y).dot(self.z) >= 1.0 - 1e-9
    }
}

/// An indexed triangle mesh in local millimetres, ready for the renderer.
///
/// Positions and normals are `f32` because this is the rendering boundary; the geometry kernel
/// keeps `f64`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Vertex positions, three floats per vertex.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals, three floats per vertex; empty when the mesh is flat-shaded per face.
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices, three per triangle.
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// True when the mesh has no triangles.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_frame_is_orthonormal() {
        assert!(Frame::WORLD.is_orthonormal());
    }

    #[test]
    fn frame_orthogonalises_axes() {
        let frame =
            Frame::from_origin_axes(DVec3::new(1.0, 2.0, 3.0), DVec3::new(2.0, 0.0, 0.0), DVec3::new(1.0, 3.0, 0.0));
        let frame = frame.expect("valid frame");
        assert!(frame.is_orthonormal());
        assert!((frame.x - DVec3::X).length() < 1e-12);
        assert!((frame.y - DVec3::Y).length() < 1e-12);
        assert!((frame.z - DVec3::Z).length() < 1e-12);
    }

    #[test]
    fn degenerate_frame_is_rejected() {
        assert!(Frame::from_origin_axes(DVec3::ZERO, DVec3::ZERO, DVec3::Y).is_none());
        assert!(Frame::from_origin_axes(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X, DVec3::Y).is_none());
    }

    #[test]
    fn mesh_counts() {
        let mesh = MeshData { positions: vec![[0.0; 3]; 3], normals: Vec::new(), indices: vec![0, 1, 2] };
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert!(!mesh.is_empty());
    }
}
