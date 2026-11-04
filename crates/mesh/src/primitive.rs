//! Builds the polygon mesh of a parametric primitive.
//!
//! The parameters are document data in integer micrometres; this module is the boundary where the
//! ring trigonometry runs in floating point and the result is rounded back to lengths. A cube keeps
//! six quads, a cylinder one quad per side plus two polygonal caps, and a sphere latitude bands
//! joined by triangles at the poles.

use furnicraft_model::{Cylinder, Mesh, Primitive, Sphere};
use furnicraft_units::Length;

use crate::MeshError;

/// The sine and cosine of one segment around a ring, in the XY plane.
fn ring_angle(segment: usize, segments: usize) -> (f64, f64) {
    let angle = std::f64::consts::TAU * segment as f64 / segments as f64;
    angle.sin_cos()
}

/// Rounds a float micrometre coordinate into a document length.
fn coordinate(value: f64) -> Result<Length, MeshError> {
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(MeshError::Invalid("a primitive coordinate is not a representable length".into()));
    }
    Ok(Length::from_micrometres(value.round() as i64))
}

/// Converts a vertex count to an index, refusing one that would overflow the index buffer.
fn index(value: usize) -> Result<u32, MeshError> {
    u32::try_from(value).map_err(|_| MeshError::Invalid("a primitive has too many vertices to index".into()))
}

/// Builds the document mesh of one primitive.
pub(crate) fn build_primitive_mesh(primitive: &Primitive) -> Result<Mesh, MeshError> {
    match primitive {
        Primitive::Cube(cube) => Ok(Mesh::cube(cube.edge())?),
        Primitive::Cylinder(cylinder) => cylinder_mesh(cylinder),
        Primitive::Sphere(sphere) => sphere_mesh(sphere),
    }
}

/// A capped tube: one quad per side and one polygon per cap.
fn cylinder_mesh(cylinder: &Cylinder) -> Result<Mesh, MeshError> {
    let segments = cylinder.segments() as usize;
    let radius = cylinder.radius().micrometres() as f64;
    let half_height = cylinder.height().micrometres() / 2;
    let mut positions = Vec::with_capacity(segments * 2);
    for z in [-half_height, half_height] {
        for segment in 0..segments {
            let (sin, cos) = ring_angle(segment, segments);
            positions.push([coordinate(cos * radius)?, coordinate(sin * radius)?, Length::from_micrometres(z)]);
        }
    }
    let mut faces = Vec::with_capacity(segments + 2);
    for segment in 0..segments {
        let next = (segment + 1) % segments;
        let bottom = index(segment)?;
        let bottom_next = index(next)?;
        let top = index(segments + segment)?;
        let top_next = index(segments + next)?;
        faces.push(vec![bottom, bottom_next, top_next, top]);
    }
    let bottom = (0..segments).rev().map(index).collect::<Result<Vec<_>, _>>()?;
    let top = (segments..segments * 2).map(index).collect::<Result<Vec<_>, _>>()?;
    faces.push(bottom);
    faces.push(top);
    Ok(Mesh::new(positions, faces)?)
}

/// A closed banded surface with one vertex at each pole.
fn sphere_mesh(sphere: &Sphere) -> Result<Mesh, MeshError> {
    let segments = sphere.segments() as usize;
    let bands = sphere.bands() as usize;
    let radius = sphere.radius().micrometres() as f64;
    let mut positions = Vec::with_capacity(segments * (bands - 1) + 2);
    positions.push([Length::ZERO, Length::ZERO, coordinate(-radius)?]);
    for band in 1..bands {
        let z = -radius + 2.0 * radius * band as f64 / bands as f64;
        let ring = (radius * radius - z * z).max(0.0).sqrt();
        for segment in 0..segments {
            let (sin, cos) = ring_angle(segment, segments);
            positions.push([coordinate(cos * ring)?, coordinate(sin * ring)?, coordinate(z)?]);
        }
    }
    let top = index(positions.len())?;
    positions.push([Length::ZERO, Length::ZERO, coordinate(radius)?]);
    let ring_index = |band: usize, segment: usize| -> Result<u32, MeshError> {
        index(1 + (band - 1) * segments + segment % segments)
    };
    let mut faces = Vec::new();
    for segment in 0..segments {
        let next = (segment + 1) % segments;
        faces.push(vec![0, ring_index(1, next)?, ring_index(1, segment)?]);
    }
    for band in 1..bands - 1 {
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            faces.push(vec![
                ring_index(band, segment)?,
                ring_index(band, next)?,
                ring_index(band + 1, next)?,
                ring_index(band + 1, segment)?,
            ]);
        }
    }
    for segment in 0..segments {
        let next = (segment + 1) % segments;
        faces.push(vec![top, ring_index(bands - 1, segment)?, ring_index(bands - 1, next)?]);
    }
    Ok(Mesh::new(positions, faces)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use furnicraft_model::{DEFAULT_BANDS, DEFAULT_SEGMENTS};

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    /// Newell's normal of a polygon.
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
    fn a_cylinder_is_a_capped_tube_of_the_stored_dimensions() {
        let primitive = Primitive::cylinder(millimetres(30), millimetres(100), 12).expect("cylinder");
        let mesh = build_primitive_mesh(&primitive).expect("mesh");
        assert_eq!(mesh.positions().len(), primitive.vertex_count());
        assert_eq!(mesh.faces().len(), primitive.face_count());
        assert_eq!(mesh.faces().last().map(Vec::len), Some(12), "the top cap is one polygon");
        assert_eq!(mesh.bounds_micrometres(), ([-30_000, -30_000, -50_000], [30_000, 30_000, 50_000]));
        assert_outward(&mesh);
    }

    #[test]
    fn a_sphere_is_a_closed_banded_surface_of_the_stored_radius() {
        let primitive = Primitive::sphere(millimetres(50), 16, 8).expect("sphere");
        let mesh = build_primitive_mesh(&primitive).expect("mesh");
        assert_eq!(mesh.positions().len(), primitive.vertex_count());
        assert_eq!(mesh.faces().len(), primitive.face_count());
        assert_eq!(mesh.bounds_micrometres(), ([-50_000; 3], [50_000; 3]));
        assert_outward(&mesh);
    }

    #[test]
    fn the_default_tessellation_still_builds_the_same_counts() {
        let primitive = Primitive::sphere(millimetres(50), DEFAULT_SEGMENTS, DEFAULT_BANDS).expect("sphere");
        let mesh = build_primitive_mesh(&primitive).expect("mesh");
        let segments = DEFAULT_SEGMENTS as usize;
        let bands = DEFAULT_BANDS as usize;
        assert_eq!(mesh.positions().len(), 2 + segments * (bands - 1));
        assert_eq!(mesh.faces().len(), segments * bands);
    }

    #[test]
    fn changing_the_sides_changes_the_mesh() {
        let coarse = Primitive::sphere(millimetres(50), 8, 4).expect("coarse");
        let fine = Primitive::sphere(millimetres(50), 16, 8).expect("fine");
        let coarse = build_primitive_mesh(&coarse).expect("coarse mesh");
        let fine = build_primitive_mesh(&fine).expect("fine mesh");
        assert_ne!(coarse.positions().len(), fine.positions().len());
        assert_ne!(coarse.faces().len(), fine.faces().len());
    }

    #[test]
    fn a_cube_is_the_document_cube() {
        let primitive = Primitive::cube(millimetres(100)).expect("cube");
        assert_eq!(
            build_primitive_mesh(&primitive).expect("mesh"),
            Mesh::cube(millimetres(100)).expect("document cube")
        );
    }
}
