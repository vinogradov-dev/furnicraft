//! `truck` adapter for the FurniCraft geometry kernel port.
//!
//! This is the only crate allowed to depend on the third-party geometry engine. The rest of the
//! workspace sees only `furnicraft-kernel` types and traits. Every call into `truck` is wrapped
//! in a panic guard: a kernel bug must become a typed error, never a crashed application.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use furnicraft_geom::{DVec2, DVec3, Frame, MeshData};
use furnicraft_kernel::{
    BodyId, BooleanOp, KernelError, Measure, Profile, Result, SolidKernel, TessOptions, signed_area,
};
use truck_meshalgo::prelude::{MeshedShape, PolygonMesh, RobustMeshableShape, StandardVertex};
use truck_modeling as tm;
use truck_modeling::builder;

/// Maximum coordinate we accept from a caller, in millimetres (ten kilometres).
const MAX_COORDINATE: f64 = 1.0e7;
/// Boolean tolerance in millimetres. Large enough to absorb double noise on a 700 mm panel,
/// small enough that a 1 µm feature is still meaningful.
const BOOLEAN_TOLERANCE: f64 = 1.0e-3;
/// Truck's tessellator panics at or below its own tolerance; stay well above it.
const MIN_TESSELLATION_TOLERANCE: f64 = 1.0e-4;

/// A solid geometry kernel backed by `truck`.
///
/// Bodies are owned by the kernel and addressed by [`BodyId`]. A body is consumed when it takes
/// part in a boolean operation; callers must use the returned handle from then on.
#[derive(Default)]
pub struct TruckKernel {
    /// Slot storage. Slots are never reused, so a stale handle cannot address a newer body.
    bodies: Vec<Option<tm::Solid>>,
}

impl TruckKernel {
    /// Creates an empty kernel.
    pub fn new() -> TruckKernel {
        TruckKernel::default()
    }

    /// Number of live bodies, for tests and diagnostics.
    pub fn body_count(&self) -> usize {
        self.bodies.iter().filter(|slot| slot.is_some()).count()
    }

    fn store(&mut self, solid: tm::Solid) -> Result<BodyId> {
        let index = u32::try_from(self.bodies.len()).map_err(|_| KernelError::Failed("body table overflow".into()))?;
        self.bodies.push(Some(solid));
        Ok(BodyId::from_raw(index))
    }

    fn body(&self, id: BodyId) -> Result<&tm::Solid> {
        self.bodies
            .get(id.index() as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| KernelError::InvalidInput(format!("unknown or consumed body {}", id.index())))
    }

    fn take_body(&mut self, id: BodyId) -> Result<tm::Solid> {
        self.bodies
            .get_mut(id.index() as usize)
            .and_then(Option::take)
            .ok_or_else(|| KernelError::InvalidInput(format!("unknown or consumed body {}", id.index())))
    }

    fn polygon(&self, id: BodyId, tolerance: f64) -> Result<PolygonMesh> {
        if !tolerance.is_finite() || tolerance < MIN_TESSELLATION_TOLERANCE {
            return Err(KernelError::InvalidInput(format!(
                "tessellation deflection must be at least {MIN_TESSELLATION_TOLERANCE} mm"
            )));
        }
        let solid = self.body(id)?;
        guard("tessellate", || Ok(solid.robust_triangulation(tolerance).to_polygon()))
    }
}

impl SolidKernel for TruckKernel {
    fn extrude(&mut self, profile: &Profile, frame: &Frame, height: f64) -> Result<BodyId> {
        profile.validate()?;
        if !frame.is_orthonormal() {
            return Err(KernelError::InvalidInput("frame axes are not orthonormal".into()));
        }
        if !height.is_finite() || height <= 0.0 || height > MAX_COORDINATE {
            return Err(KernelError::InvalidInput(format!("invalid extrusion height: {height}")));
        }

        let outer = wire_from_loop(frame, &profile.outer, true)?;
        let mut wires = Vec::with_capacity(1 + profile.holes.len());
        wires.push(outer);
        for hole in &profile.holes {
            wires.push(wire_from_loop(frame, hole, false)?);
        }

        let face = guard("attach profile plane", || {
            builder::try_attach_plane(&wires)
                .map_err(|error| KernelError::Failed(format!("cannot attach profile plane: {error}")))
        })?;
        let sweep = tm::Vector3::new(frame.z.x, frame.z.y, frame.z.z) * height;
        let solid = guard("extrude", || Ok(builder::tsweep(&face, sweep)))?;
        guard("check solid", || ensure_closed(&solid))?;
        self.store(solid)
    }

    fn boolean(&mut self, op: BooleanOp, target: BodyId, tool: BodyId) -> Result<BodyId> {
        if target == tool {
            return Err(KernelError::InvalidInput("a boolean needs two different bodies".into()));
        }
        let target_solid = self.take_body(target)?;
        let mut tool_solid = self.take_body(tool)?;

        let result = guard("boolean", || {
            let result = match op {
                BooleanOp::Union => truck_shapeops::or(&target_solid, &tool_solid, BOOLEAN_TOLERANCE),
                BooleanOp::Intersection => truck_shapeops::and(&target_solid, &tool_solid, BOOLEAN_TOLERANCE),
                BooleanOp::Difference => {
                    tool_solid.not();
                    truck_shapeops::and(&target_solid, &tool_solid, BOOLEAN_TOLERANCE)
                }
            };
            result.ok_or_else(|| KernelError::Failed(format!("{op:?} produced no solid")))
        })?;

        guard("check solid", || ensure_closed(&result))?;
        self.store(result)
    }

    fn tessellate(&self, body: BodyId, options: &TessOptions) -> Result<MeshData> {
        let polygon = self.polygon(body, options.deflection)?;
        polygon_to_mesh(&polygon)
    }

    fn measure(&self, body: BodyId) -> Result<Measure> {
        let polygon = self.polygon(body, 0.05)?;
        let volume = polygon_volume(&polygon)?.abs();
        Ok(Measure { volume })
    }

    fn clear(&mut self) {
        self.bodies.clear();
    }
}

/// Runs a `truck` operation and turns a panic into a typed error.
fn guard<T>(operation: &'static str, run: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(result) => result,
        Err(payload) => {
            let message = if let Some(text) = payload.downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = payload.downcast_ref::<String>() {
                text.clone()
            } else {
                "unknown panic payload".to_owned()
            };
            Err(KernelError::Internal(format!("{operation}: {message}")))
        }
    }
}

fn ensure_closed(solid: &tm::Solid) -> Result<()> {
    if solid.boundaries().iter().all(|shell| shell.shell_condition() == tm::ShellCondition::Closed) {
        Ok(())
    } else {
        Err(KernelError::Failed("the result is not a closed solid".into()))
    }
}

/// Builds a closed planar wire from a profile loop. Outer loops must be counter-clockwise and
/// holes clockwise, so the plane normal points along the frame's Z axis.
fn wire_from_loop(frame: &Frame, points: &[DVec2], counter_clockwise: bool) -> Result<tm::Wire> {
    if points.len() < 3 {
        return Err(KernelError::InvalidInput("a profile loop needs at least three points".into()));
    }
    if points.iter().any(|point| !point.is_finite()) {
        return Err(KernelError::InvalidInput("a profile loop has a non-finite point".into()));
    }
    let area = signed_area(points);
    if area.abs() <= 1e-9 {
        return Err(KernelError::InvalidInput("a profile loop has zero area".into()));
    }

    let mut ordered = points.to_vec();
    if (counter_clockwise && area < 0.0) || (!counter_clockwise && area > 0.0) {
        ordered.reverse();
    }

    let mut vertices = Vec::with_capacity(ordered.len());
    for point in &ordered {
        let world = frame.point(point.x, point.y);
        if !world.is_finite() || world.abs().max_element() > MAX_COORDINATE {
            return Err(KernelError::InvalidInput("a profile point maps outside the supported world range".into()));
        }
        vertices.push(builder::vertex(tm::Point3::new(world.x, world.y, world.z)));
    }

    let mut edges = Vec::with_capacity(vertices.len());
    for index in 0..vertices.len() {
        let next = (index + 1) % vertices.len();
        edges.push(builder::line(&vertices[index], &vertices[next]));
    }
    Ok(edges.into_iter().collect())
}

fn polygon_volume(mesh: &PolygonMesh) -> Result<f64> {
    let positions = mesh.positions();
    let mut volume = 0.0;
    for face in mesh.face_iter() {
        let points = face_points(positions, face)?;
        if points.len() < 3 {
            return Err(KernelError::Failed("a tessellated face has fewer than three points".into()));
        }
        let first = points[0];
        for index in 1..points.len() - 1 {
            volume += first.dot(points[index].cross(points[index + 1])) / 6.0;
        }
    }
    if volume.is_finite() { Ok(volume) } else { Err(KernelError::Failed("the measured volume is not finite".into())) }
}

fn polygon_to_mesh(mesh: &PolygonMesh) -> Result<MeshData> {
    let positions = mesh.positions();
    let mut data = MeshData::default();
    for face in mesh.face_iter() {
        let points = face_points(positions, face)?;
        if points.len() < 3 {
            continue;
        }
        let first = points[0];
        for index in 1..points.len() - 1 {
            let second = points[index];
            let third = points[index + 1];
            let normal = (second - first).cross(third - first);
            if normal.length_squared() <= 1e-20 {
                continue;
            }
            let normal = normal.normalize();
            if data.positions.len() > (u32::MAX as usize) - 3 {
                return Err(KernelError::Failed("the tessellated mesh is too large".into()));
            }
            let base = u32::try_from(data.positions.len())
                .map_err(|_| KernelError::Failed("the tessellated mesh is too large".into()))?;
            for point in [first, second, third] {
                data.positions.push([to_f32(point.x)?, to_f32(point.y)?, to_f32(point.z)?]);
                data.normals.push([normal.x as f32, normal.y as f32, normal.z as f32]);
            }
            data.indices.extend([base, base + 1, base + 2]);
        }
    }
    if data.is_empty() {
        return Err(KernelError::Failed("tessellation produced no triangles".into()));
    }
    Ok(data)
}

fn face_points(positions: &[tm::Point3], face: &[StandardVertex]) -> Result<Vec<DVec3>> {
    let mut points = Vec::with_capacity(face.len());
    for vertex in face {
        let point = positions
            .get(vertex.pos)
            .ok_or_else(|| KernelError::Failed("a tessellated face refers to a missing vertex".into()))?;
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return Err(KernelError::Failed("a tessellated vertex is not finite".into()));
        }
        points.push(DVec3::new(point.x, point.y, point.z));
    }
    Ok(points)
}

fn to_f32(value: f64) -> Result<f32> {
    if value.is_finite() && value.abs() <= f32::MAX as f64 {
        Ok(value as f32)
    } else {
        Err(KernelError::Failed(format!("coordinate {value} does not fit a render mesh")))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn frame_for_x_extrusion() -> Frame {
        Frame::from_origin_axes(DVec3::new(-10.0, 0.0, 0.0), DVec3::Y, DVec3::Z).expect("a valid X extrusion frame")
    }

    #[test]
    fn extruded_panel_has_the_expected_volume_and_mesh() {
        let mut kernel = TruckKernel::new();
        let profile = Profile::rectangle(700.0, 500.0).expect("panel profile");
        let panel = kernel.extrude(&profile, &Frame::WORLD, 18.0).expect("panel");

        let measure = kernel.measure(panel).expect("measure");
        assert!((measure.volume - 6_300_000.0).abs() < 1.0, "volume was {}", measure.volume);

        let mesh = kernel.tessellate(panel, &TessOptions::default()).expect("mesh");
        assert!(mesh.triangle_count() >= 12, "triangles: {}", mesh.triangle_count());
        assert_eq!(mesh.positions.len(), mesh.normals.len());
        assert_eq!(mesh.indices.len(), mesh.triangle_count() * 3);
    }

    #[test]
    fn panel_with_a_rectangular_hole_keeps_the_hole_open() {
        let mut kernel = TruckKernel::new();
        let outer = Profile::rectangle(100.0, 100.0).expect("outer profile");
        let hole = vec![DVec2::new(40.0, 40.0), DVec2::new(60.0, 40.0), DVec2::new(60.0, 60.0), DVec2::new(40.0, 60.0)];
        let profile = Profile::new(outer.outer, vec![hole]).expect("profile with a hole");
        let body = kernel.extrude(&profile, &Frame::WORLD, 18.0).expect("panel");
        let measure = kernel.measure(body).expect("measure");
        let expected = 100.0 * 100.0 * 18.0 - 20.0 * 20.0 * 18.0;
        assert!((measure.volume - expected).abs() < 1.0, "volume was {}, expected {expected}", measure.volume);
    }

    #[test]
    fn panel_groove_and_chamfer_match_analytic_volume() {
        let mut kernel = TruckKernel::new();
        let panel_profile = Profile::rectangle(700.0, 500.0).expect("panel profile");
        let mut body = kernel.extrude(&panel_profile, &Frame::WORLD, 18.0).expect("panel");

        // A dovetail groove along X: 12 mm wide at the top, 8 mm at the bottom, 6 mm deep.
        // The cutter is extended above the panel so its top face is not coplanar with the
        // panel's top face. Inside the panel the section is exactly the trapezoid.
        let groove = Profile::new(
            vec![
                DVec2::new(243.666_666_666_7, 19.0),
                DVec2::new(256.333_333_333_3, 19.0),
                DVec2::new(254.0, 12.0),
                DVec2::new(246.0, 12.0),
            ],
            Vec::new(),
        )
        .expect("groove profile");
        let groove_body = kernel.extrude(&groove, &frame_for_x_extrusion(), 720.0).expect("groove tool");
        body = kernel.boolean(BooleanOp::Difference, body, groove_body).expect("subtract groove");

        // A 3 mm chamfer at the top front edge. The cutter extends 1 mm outside the panel
        // so no cutter face is coplanar with a panel face; the intersection is a 3x3 prism.
        let chamfer =
            Profile::new(vec![DVec2::new(-1.0, 19.0), DVec2::new(4.0, 19.0), DVec2::new(-1.0, 14.0)], Vec::new())
                .expect("chamfer profile");
        let chamfer_body = kernel.extrude(&chamfer, &frame_for_x_extrusion(), 720.0).expect("chamfer tool");
        body = kernel.boolean(BooleanOp::Difference, body, chamfer_body).expect("subtract chamfer");

        let measure = kernel.measure(body).expect("measure");
        let expected = 6_300_000.0 - 42_000.0 - 3_150.0;
        assert!((measure.volume - expected).abs() < 10.0, "volume was {}, expected {expected}", measure.volume);

        let mesh = kernel.tessellate(body, &TessOptions::new(0.01).expect("tessellation")).expect("mesh");
        assert!(mesh.triangle_count() > 12, "triangles: {}", mesh.triangle_count());
        assert!(mesh.positions.iter().flatten().all(|value| value.is_finite()));
    }

    #[test]
    fn invalid_input_is_reported_not_panicked() {
        let mut kernel = TruckKernel::new();
        let profile = Profile::rectangle(10.0, 10.0).expect("profile");
        assert!(kernel.extrude(&profile, &Frame::WORLD, 0.0).is_err());
        assert!(kernel.extrude(&profile, &Frame::WORLD, f64::NAN).is_err());

        let body = kernel.extrude(&profile, &Frame::WORLD, 1.0).expect("body");
        assert!(kernel.boolean(BooleanOp::Difference, body, body).is_err());
        assert!(kernel.tessellate(body, &TessOptions { deflection: 0.0 }).is_err());
    }

    #[test]
    fn clear_drops_bodies() {
        let mut kernel = TruckKernel::new();
        let profile = Profile::rectangle(10.0, 10.0).expect("profile");
        let body = kernel.extrude(&profile, &Frame::WORLD, 1.0).expect("body");
        assert_eq!(kernel.body_count(), 1);
        kernel.clear();
        assert_eq!(kernel.body_count(), 0);
        assert!(kernel.measure(body).is_err());
    }
}
