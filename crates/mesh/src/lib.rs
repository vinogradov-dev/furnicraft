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
