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

use furnicraft_geom::{DVec3, MeshData, triangulate_face};
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
/// Every face is ear-clipped in its own plane, so a concave cap keeps its triangles inside the
/// contour; a face that ear clipping rejects — a self-crossing one, which dragging a vertex can
/// make — falls back to a fan so the edited mesh still draws. Each face gets one normal, so the
/// object renders faceted without smoothing. Positions are converted from micrometres to local
/// millimetres here, at the render boundary, and a degenerate face is skipped rather than producing
/// a NaN normal.
pub fn object_mesh(mesh: &Mesh) -> Result<MeshData, MeshError> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for face in mesh.faces() {
        let points = face_points(mesh, face)?;
        let Some(normal) = face_normal(&points) else {
            continue;
        };
        let normal = [normal.x as f32, normal.y as f32, normal.z as f32];
        let triangles = triangulate_face(&points).unwrap_or_else(|_| fan_triangles(points.len()));
        for triangle in triangles {
            let [Some(first), Some(second), Some(third)] = triangle.map(|index| points.get(index as usize).copied())
            else {
                return Err(MeshError::Invalid("a triangle refers to a missing vertex".into()));
            };
            if positions.len() > (u32::MAX as usize).saturating_sub(3) {
                return Err(MeshError::Invalid("the object has too many vertices to index".into()));
            }
            let Ok(base) = u32::try_from(positions.len()) else {
                return Err(MeshError::Invalid("the object has too many vertices to index".into()));
            };
            for point in [first, second, third] {
                positions.push([point.x as f32, point.y as f32, point.z as f32]);
                normals.push(normal);
            }
            indices.extend([base, base + 1, base + 2]);
        }
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

/// The face's vertices in local millimetres, in face order.
fn face_points(mesh: &Mesh, face: &[u32]) -> Result<Vec<DVec3>, MeshError> {
    let mut points = Vec::with_capacity(face.len());
    for index in face {
        let Some(position) = mesh.positions().get(*index as usize) else {
            return Err(MeshError::Invalid(format!("face refers to missing vertex {index}")));
        };
        let point = DVec3::new(
            position[0].micrometres() as f64 / 1000.0,
            position[1].micrometres() as f64 / 1000.0,
            position[2].micrometres() as f64 / 1000.0,
        );
        if !point.is_finite() {
            return Err(MeshError::Invalid("a vertex is not finite".into()));
        }
        points.push(point);
    }
    Ok(points)
}

/// The fan fallback: triangles from the first vertex, valid only while the face is convex.
fn fan_triangles(len: usize) -> Vec<[u32; 3]> {
    let mut triangles = Vec::with_capacity(len.saturating_sub(2));
    for index in 1..len.saturating_sub(1) {
        let (Ok(current), Ok(next)) = (u32::try_from(index), u32::try_from(index + 1)) else {
            break;
        };
        triangles.push([0, current, next]);
    }
    triangles
}

/// Newell's normal of a polygon, or `None` when it is degenerate.
fn face_normal(points: &[DVec3]) -> Option<DVec3> {
    let mut normal = DVec3::ZERO;
    for (index, current) in points.iter().enumerate() {
        let next = points.get((index + 1) % points.len())?;
        normal.x += (current.y - next.y) * (current.z + next.z);
        normal.y += (current.z - next.z) * (current.x + next.x);
        normal.z += (current.x - next.x) * (current.y + next.y);
    }
    let length = normal.length();
    (length.is_finite() && length > f64::EPSILON).then(|| normal / length)
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

    /// A 100 x 100 contour with a 20 x 60 notch cut into its top edge, in millimetres.
    fn notched_contour() -> furnicraft_model::Contour {
        let points = [[0, 0], [100, 0], [100, 100], [60, 100], [60, 40], [40, 40], [40, 100], [0, 100]]
            .map(|[x, y]| [millimetres(x), millimetres(y)]);
        furnicraft_model::Contour::from_polygon(points.to_vec()).expect("a simple contour")
    }

    fn inside_notched_contour(point: [f32; 2]) -> bool {
        let ring = notched_contour();
        let points = ring.sampled_points(1.0).expect("sampled contour");
        let mut inside = false;
        for (index, current) in points.iter().enumerate() {
            let Some(next) = points.get((index + 1) % points.len()) else {
                continue;
            };
            let (ax, ay) = (current[0].millimetres_f64() as f32, current[1].millimetres_f64() as f32);
            let (bx, by) = (next[0].millimetres_f64() as f32, next[1].millimetres_f64() as f32);
            if (ay > point[1]) != (by > point[1]) {
                let at = (bx - ax) * (point[1] - ay) / (by - ay) + ax;
                if point[0] < at {
                    inside = !inside;
                }
            }
        }
        inside
    }

    #[test]
    fn a_concave_extrusion_cap_keeps_its_triangles_inside_the_contour() {
        let constructive =
            furnicraft_model::Constructive::extrude(notched_contour(), millimetres(18)).expect("a constructive object");
        let mesh = furnicraft_param::build_constructive_mesh(&constructive).expect("document mesh");
        let data = object_mesh(&mesh).expect("render mesh");
        let mut caps = 0;
        for triangle in data.indices.as_chunks::<3>().0 {
            let corners: Vec<[f32; 3]> =
                triangle.iter().filter_map(|index| data.positions.get(*index as usize).copied()).collect();
            let [Some(a), Some(b), Some(c)] = [corners.first(), corners.get(1), corners.get(2)] else {
                continue;
            };
            if a[2] != b[2] || b[2] != c[2] {
                continue;
            }
            caps += 1;
            let centroid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            assert!(
                inside_notched_contour(centroid),
                "a cap triangle at {centroid:?} left the contour: {triangle:?} of {corners:?}"
            );
        }
        assert_eq!(caps, 12, "each cap of an eight-point ring has six triangles");
    }

    #[test]
    fn truck_kernel_tessellates_a_panel_in_local_coordinates() {
        let mut kernel = TruckKernel::new();
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        let panel = Panel::new(Id::new(1).expect("id"), "Panel", size, None, millimetres(18), Placement::IDENTITY)
            .expect("valid panel");
        let geometry = panel.geometry_with_fallbacks().expect("resolved geometry");
        let body = build_panel(&mut kernel, &panel, &geometry).expect("panel body");
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
