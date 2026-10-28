//! Triangle meshes derived from the document model.
//!
//! Turns a panel body into indexed geometry with normals. Coordinates stay in the body's local
//! frame; a placement is applied later by the scene. This crate owns no GPU state.
//!
//! The revision-keyed evaluation cache arrives with the document skeleton; the first slice only
//! validates the kernel's tessellation before exposing it.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use furnicraft_geom::MeshData;
use furnicraft_kernel::{BodyId, KernelError, SolidKernel, TessOptions};
use furnicraft_model::Mesh;
use thiserror::Error;

/// An error raised while building a render mesh.
#[derive(Debug, Error)]
pub enum MeshError {
    /// The geometry kernel rejected or failed tessellation.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// The adapter returned a mesh that is not safe to render.
    #[error("invalid panel mesh: {0}")]
    Invalid(String),
}

/// Tessellates a panel body and checks the mesh before it reaches the renderer.
///
/// The returned positions and normals are in the body's local millimetres. The caller must keep
/// the body alive in the kernel for the duration of the call; the mesh itself owns no kernel
/// handle.
pub fn tessellate_panel(kernel: &dyn SolidKernel, body: BodyId, options: &TessOptions) -> Result<MeshData, MeshError> {
    let mesh = kernel.tessellate(body, options)?;
    validate_mesh(&mesh)?;
    Ok(mesh)
}

/// Triangulates a document mesh into flat-shaded renderable triangles.
///
/// Polygons are fan-triangulated and each face gets one normal, so the object renders faceted
/// without smoothing. Positions are converted from micrometres to local millimetres here, at the
/// render boundary, and a degenerate face is skipped rather than producing a NaN normal.
pub fn object_mesh(mesh: &Mesh) -> Result<MeshData, MeshError> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for face in mesh.faces() {
        let mut points = Vec::with_capacity(face.len());
        for index in face {
            let Some(position) = mesh.positions().get(*index as usize) else {
                return Err(MeshError::Invalid(format!("face refers to missing vertex {index}")));
            };
            let point = [
                position[0].micrometres() as f64 / 1000.0,
                position[1].micrometres() as f64 / 1000.0,
                position[2].micrometres() as f64 / 1000.0,
            ];
            if point.iter().any(|value| !value.is_finite()) {
                return Err(MeshError::Invalid("a vertex is not finite".into()));
            }
            points.push(point);
        }
        let Some(normal) = face_normal(&points) else {
            continue;
        };
        let normal = [normal[0] as f32, normal[1] as f32, normal[2] as f32];
        let base = u32::try_from(positions.len())
            .map_err(|_| MeshError::Invalid("the object has too many vertices to index".into()))?;
        for index in 1..points.len().saturating_sub(1) {
            for corner in [0, index, index + 1] {
                let Some(point) = points.get(corner) else {
                    continue;
                };
                positions.push([point[0] as f32, point[1] as f32, point[2] as f32]);
                normals.push(normal);
            }
        }
        let added = u32::try_from(points.len().saturating_sub(2).saturating_mul(3))
            .map_err(|_| MeshError::Invalid("the face has too many vertices".into()))?;
        indices.extend(base..base.saturating_add(added));
    }
    let data = MeshData { positions, normals, indices };
    if data.is_empty() {
        // Every face was degenerate, so there is nothing to draw. An object must not fail the
        // whole frame because one of its faces has zero area.
        return Ok(data);
    }
    validate_mesh(&data)?;
    Ok(data)
}

/// Newell's normal of a polygon, or `None` when it is degenerate.
fn face_normal(points: &[[f64; 3]]) -> Option<[f64; 3]> {
    let mut normal = [0.0f64; 3];
    for (index, current) in points.iter().enumerate() {
        let next = points.get((index + 1) % points.len())?;
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    Some([normal[0] / length, normal[1] / length, normal[2] / length])
}

fn validate_mesh(mesh: &MeshData) -> Result<(), MeshError> {
    if mesh.positions.is_empty() {
        return Err(MeshError::Invalid("the mesh has no vertices".into()));
    }
    if mesh.indices.is_empty() {
        return Err(MeshError::Invalid("the mesh has no triangles".into()));
    }
    if !mesh.indices.len().is_multiple_of(3) {
        return Err(MeshError::Invalid("the index buffer is not a whole number of triangles".into()));
    }
    if !mesh.normals.is_empty() && mesh.normals.len() != mesh.positions.len() {
        return Err(MeshError::Invalid("the normal buffer does not match the vertex buffer".into()));
    }
    if mesh.positions.iter().flatten().any(|value| !value.is_finite()) {
        return Err(MeshError::Invalid("the mesh has a non-finite vertex position".into()));
    }
    if mesh.normals.iter().flatten().any(|value| !value.is_finite()) {
        return Err(MeshError::Invalid("the mesh has a non-finite normal".into()));
    }

    let vertex_count = u32::try_from(mesh.positions.len())
        .map_err(|_| MeshError::Invalid("the mesh has more vertices than an index can address".into()))?;
    if mesh.indices.iter().any(|index| *index >= vertex_count) {
        return Err(MeshError::Invalid("the mesh has an index outside the vertex buffer".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_kernel::{BooleanOp, Measure, Profile};
    use furnicraft_kernel_truck::TruckKernel;
    use furnicraft_model::{Id, Panel, PanelNominalSize, Placement};
    use furnicraft_param::build_panel;
    use furnicraft_units::Length;

    use super::*;

    struct StaticKernel {
        mesh: Option<MeshData>,
    }

    impl SolidKernel for StaticKernel {
        fn extrude(
            &mut self,
            _profile: &Profile,
            _frame: &furnicraft_geom::Frame,
            _height: f64,
        ) -> furnicraft_kernel::Result<BodyId> {
            Err(KernelError::Unsupported("static kernel does not implement extrusion"))
        }

        fn boolean(&mut self, _op: BooleanOp, _target: BodyId, _tool: BodyId) -> furnicraft_kernel::Result<BodyId> {
            Err(KernelError::Unsupported("static kernel does not implement booleans"))
        }

        fn tessellate(&self, _body: BodyId, _options: &TessOptions) -> furnicraft_kernel::Result<MeshData> {
            self.mesh.clone().ok_or(KernelError::Unsupported("static kernel has no mesh"))
        }

        fn measure(&self, _body: BodyId) -> furnicraft_kernel::Result<Measure> {
            Err(KernelError::Unsupported("static kernel does not implement measurement"))
        }

        fn clear(&mut self) {
            self.mesh = None;
        }
    }

    fn triangle() -> MeshData {
        MeshData {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
        }
    }

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    #[test]
    fn tessellate_panel_returns_a_valid_mesh() {
        let expected = triangle();
        let kernel = StaticKernel { mesh: Some(expected.clone()) };
        let mesh = tessellate_panel(&kernel, BodyId::from_raw(1), &TessOptions::default()).expect("valid mesh");
        assert_eq!(mesh, expected);
    }

    #[test]
    fn tessellate_panel_propagates_kernel_errors() {
        let kernel = StaticKernel { mesh: None };
        let error = tessellate_panel(&kernel, BodyId::from_raw(1), &TessOptions::default()).expect_err("kernel error");
        assert!(matches!(error, MeshError::Kernel(KernelError::Unsupported(_))));
    }

    #[test]
    fn invalid_meshes_are_rejected() {
        let mut mesh = triangle();
        mesh.positions.clear();
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.indices.clear();
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.indices.pop();
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.normals.pop();
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.positions[0][0] = f32::NAN;
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.indices[2] = 3;
        assert!(matches!(validate_mesh(&mesh), Err(MeshError::Invalid(_))));

        let mut mesh = triangle();
        mesh.normals.clear();
        assert!(validate_mesh(&mesh).is_ok());
    }

    #[test]
    fn object_mesh_triangulates_a_cube_into_twelve_flat_triangles() {
        let cube = furnicraft_model::Mesh::cube(millimetres(100)).expect("cube");
        let mesh = object_mesh(&cube).expect("render mesh");
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.vertex_count(), 36, "flat shading duplicates vertices per face");
        for normal in &mesh.normals {
            let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            assert!((length - 1.0).abs() < 1.0e-5, "normal was {normal:?}");
            let axes = normal.iter().filter(|value| value.abs() > 0.5).count();
            assert_eq!(axes, 1, "a cube normal points along one axis, was {normal:?}");
        }
        let half = 50.0f32;
        for position in &mesh.positions {
            assert_eq!(position[0].abs(), half);
            assert_eq!(position[1].abs(), half);
            assert_eq!(position[2].abs(), half);
        }
    }

    #[test]
    fn object_mesh_skips_degenerate_faces_without_failing() {
        let collinear = furnicraft_model::Mesh::new(
            vec![
                [millimetres(0), millimetres(0), millimetres(0)],
                [millimetres(10), millimetres(0), millimetres(0)],
                [millimetres(20), millimetres(0), millimetres(0)],
                [millimetres(0), millimetres(10), millimetres(0)],
            ],
            vec![vec![0, 1, 2], vec![0, 1, 3]],
        )
        .expect("the model accepts a geometrically degenerate face");
        let mesh = object_mesh(&collinear).expect("render mesh");
        assert_eq!(mesh.triangle_count(), 1, "only the triangle with area survives");
        assert!(mesh.positions.iter().flatten().all(|value| value.is_finite()));
    }

    #[test]
    fn object_mesh_with_only_degenerate_faces_is_empty() {
        let collinear = furnicraft_model::Mesh::new(
            vec![
                [millimetres(0), millimetres(0), millimetres(0)],
                [millimetres(10), millimetres(0), millimetres(0)],
                [millimetres(20), millimetres(0), millimetres(0)],
            ],
            vec![vec![0, 1, 2]],
        )
        .expect("valid mesh");
        let mesh = object_mesh(&collinear).expect("render mesh");
        assert!(mesh.is_empty());
    }

    #[test]
    fn truck_kernel_tessellates_a_panel_in_local_coordinates() {
        let mut kernel = TruckKernel::new();
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        let panel = Panel::new(Id::new(1).expect("id"), "Panel", size, None, millimetres(18), Placement::IDENTITY)
            .expect("valid panel");
        let geometry = panel.geometry_with_fallbacks().expect("resolved geometry");
        let body = build_panel(&mut kernel, &panel, geometry).expect("panel body");
        let mesh = tessellate_panel(&kernel, body, &TessOptions::default()).expect("panel mesh");

        assert!(mesh.triangle_count() >= 12, "triangles: {}", mesh.triangle_count());
        assert_eq!(mesh.positions.len(), mesh.normals.len());
        assert_eq!(mesh.indices.len(), mesh.triangle_count() * 3);
        for position in &mesh.positions {
            assert!((-1.0e-3_f32..=700.0_f32 + 1.0e-3_f32).contains(&position[0]));
            assert!((-1.0e-3_f32..=500.0_f32 + 1.0e-3_f32).contains(&position[1]));
            assert!((-1.0e-3_f32..=18.0_f32 + 1.0e-3_f32).contains(&position[2]));
        }
    }
}
