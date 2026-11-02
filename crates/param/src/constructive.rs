//! The analytic mesh backend for one extrusion.
//!
//! A single linear extrusion is exact as polygons: two caps and one quad per profile edge. The
//! viewport and `object.collapse` therefore need no geometry kernel, and the collapsed mesh is
//! exactly the mesh the user saw. A plan with a boolean steps outside that exact case and is
//! refused here; such a plan must go through the kernel backend instead.
//!
//! The document keeps micrometres, so this module converts the plan's millimetre profile back to
//! whole micrometres at the last moment. The round trip is exact for a contour that was sampled
//! from document units, which is why collapse does not move a vertex.

use furnicraft_geom::{DVec2, DVec3, Frame};
use furnicraft_kernel::{BooleanOp, KernelError, Profile};
use furnicraft_model::{Constructive, Mesh};
use furnicraft_units::Length;

use crate::solid::{self, Extrude, PlanBackend, PlanBuilder, SolidPlan};
use crate::{ParamError, sampled_cap};

/// Micrometres in one millimetre, the boundary factor of the geometry layer.
const MICROMETRES_PER_MILLIMETRE: f64 = 1_000.0;

/// Builds the polygon mesh of a constructive object with one extrusion.
///
/// The work is routed through the same [`SolidPlan`] the kernel backend consumes; only the backend
/// differs. Coordinates stay in micrometres in the object's local frame, from `z = 0` to the
/// feature height.
pub fn build_constructive_mesh(constructive: &Constructive) -> Result<Mesh, ParamError> {
    let plan = constructive_plan(constructive)?;
    solid::evaluate_with(&plan, &mut MeshBackend).map_err(ParamError::from)
}

/// Compiles the one supported feature into a plan.
///
/// A history with anything but a single extrusion has no analytic answer yet; it is refused before
/// a backend sees it, so the error names the feature history rather than a backend limitation.
fn constructive_plan(constructive: &Constructive) -> Result<SolidPlan, ParamError> {
    if constructive.features().len() != 1 {
        return Err(ParamError::UnsupportedConstructive);
    }
    let Some(extrude) = constructive.extrude_feature() else {
        return Err(ParamError::UnsupportedConstructive);
    };
    let points = sampled_cap(extrude.contour())?;
    let mut outer = Vec::with_capacity(points.len());
    for point in &points {
        outer.push(DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64()));
    }
    let profile = Profile::new(outer, Vec::new())?;
    let plan = PlanBuilder::new().extrude(profile, Frame::WORLD, extrude.height().millimetres_f64())?.build()?;
    Ok(plan)
}

/// Turns one extrusion into the document mesh.
struct MeshBackend;

impl PlanBackend for MeshBackend {
    type Body = Mesh;

    fn extrude(&mut self, extrude: &Extrude) -> Result<Mesh, KernelError> {
        mesh_from_extrusion(extrude)
    }

    fn boolean(&mut self, _op: BooleanOp, _target: Mesh, _tool: Mesh) -> Result<Mesh, KernelError> {
        Err(KernelError::Unsupported("the analytic backend evaluates one extrusion"))
    }
}

/// Two cap faces and one quad per profile edge.
fn mesh_from_extrusion(extrude: &Extrude) -> Result<Mesh, KernelError> {
    let frame = extrude.frame();
    if frame.x != DVec3::X || frame.y != DVec3::Y || frame.z != DVec3::Z {
        return Err(KernelError::Unsupported("the analytic backend needs a world-aligned frame"));
    }
    let profile = extrude.profile();
    if !profile.holes.is_empty() {
        return Err(KernelError::Unsupported("the analytic backend needs a profile without holes"));
    }

    let bottom = micrometres(frame.origin.z)?;
    let top = micrometres(frame.origin.z + extrude.height())?;
    let mut points = Vec::with_capacity(profile.outer.len());
    for point in &profile.outer {
        let world = frame.point(point.x, point.y);
        points.push([micrometres(world.x)?, micrometres(world.y)?]);
    }
    let count = u32::try_from(points.len())
        .map_err(|_| KernelError::InvalidInput("the profile has more points than an index can address".into()))?;
    if count < 3 {
        return Err(KernelError::InvalidInput("an extrusion needs at least three profile points".into()));
    }

    let mut positions = Vec::with_capacity(points.len().saturating_mul(2));
    for [x, y] in &points {
        positions.push([Length::from_micrometres(*x), Length::from_micrometres(*y), Length::from_micrometres(bottom)]);
    }
    for [x, y] in &points {
        positions.push([Length::from_micrometres(*x), Length::from_micrometres(*y), Length::from_micrometres(top)]);
    }
    let mut faces = Vec::with_capacity(points.len().saturating_add(2));
    // The bottom cap is the contour reversed so its normal points down; the top keeps the
    // counter-clockwise order. Each side quad is counter-clockwise seen from outside.
    faces.push((0..count).rev().collect());
    faces.push((count..count.saturating_mul(2)).collect());
    for index in 0..count {
        let next = (index + 1) % count;
        faces.push(vec![index, next, next.saturating_add(count), index.saturating_add(count)]);
    }
    Mesh::new(positions, faces).map_err(|error| KernelError::InvalidInput(error.to_string()))
}

/// Converts a millimetre coordinate to whole document micrometres.
fn micrometres(value_mm: f64) -> Result<i64, KernelError> {
    let value = (value_mm * MICROMETRES_PER_MILLIMETRE).round();
    if !value.is_finite() || !(i64::MIN as f64..=i64::MAX as f64).contains(&value) {
        return Err(KernelError::InvalidInput("a point lies outside the document range".into()));
    }
    Ok(value as i64)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_kernel::SolidKernel;
    use furnicraft_kernel_truck::TruckKernel;
    use furnicraft_model::{Contour, Feature, Id, Panel, PanelNominalSize, Placement};
    use furnicraft_units::Length;

    use super::*;
    use crate::build_panel;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn rectangle(size: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [millimetres(size), Length::ZERO],
            [millimetres(size), millimetres(height)],
            [Length::ZERO, millimetres(height)],
        ])
        .expect("rectangle")
    }

    /// Signed volume of a polygon mesh through the divergence theorem.
    fn signed_volume(mesh: &Mesh) -> f64 {
        let point = |index: u32| {
            let position = mesh.positions()[index as usize];
            DVec3::new(position[0].millimetres_f64(), position[1].millimetres_f64(), position[2].millimetres_f64())
        };
        let mut total = 0.0;
        for face in mesh.faces() {
            for corner in 1..face.len().saturating_sub(1) {
                let a = point(face[0]);
                let b = point(face[corner]);
                let c = point(face[corner + 1]);
                total += a.dot(b.cross(c)) / 6.0;
            }
        }
        total
    }

    #[test]
    fn a_single_extrusion_has_two_caps_and_one_quad_per_edge() {
        let constructive = Constructive::extrude(rectangle(100, 50), millimetres(18)).expect("constructive");
        let mesh = build_constructive_mesh(&constructive).expect("mesh");
        assert_eq!(mesh.positions().len(), 8);
        assert_eq!(mesh.faces().len(), 6);
        assert!(mesh.faces().iter().all(|face| face.len() == 4));
        assert!((signed_volume(&mesh) - 100.0 * 50.0 * 18.0).abs() < 1.0e-6);
    }

    #[test]
    fn the_analytic_object_matches_the_kernel_panel_volume() {
        let constructive = Constructive::extrude(rectangle(700, 500), millimetres(18)).expect("constructive");
        let mesh = build_constructive_mesh(&constructive).expect("mesh");
        let analytic = signed_volume(&mesh);

        let mut kernel = TruckKernel::new();
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("panel size");
        let panel = Panel::new(Id::new(1).expect("id"), "Panel", size, None, millimetres(18), Placement::IDENTITY)
            .expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        let body = build_panel(&mut kernel, &panel, geometry).expect("kernel body");
        let kernel_volume = kernel.measure(body).expect("measure").volume;
        assert!((analytic - kernel_volume).abs() < 1.0, "analytic {analytic} vs kernel {kernel_volume}");
    }

    #[test]
    fn a_boolean_plan_is_refused_by_the_analytic_backend() {
        let tool = PlanBuilder::new()
            .extrude(Profile::rectangle(10.0, 10.0).expect("tool"), Frame::WORLD, 18.0)
            .expect("tool")
            .build()
            .expect("tool plan");
        let plan = PlanBuilder::new()
            .extrude(Profile::rectangle(100.0, 50.0).expect("base"), Frame::WORLD, 18.0)
            .expect("base")
            .boolean(BooleanOp::Difference, tool)
            .build()
            .expect("boolean plan");
        let error = solid::evaluate_with(&plan, &mut MeshBackend).expect_err("the analytic backend refuses booleans");
        assert!(matches!(error, KernelError::Unsupported(_)), "{error}");
    }

    #[test]
    fn a_history_with_more_than_one_feature_is_not_evaluated_silently() {
        let extrude = furnicraft_model::Extrude::new(rectangle(10, 10), millimetres(5)).expect("extrude");
        let constructive = Constructive::new(vec![Feature::Extrude(extrude.clone()), Feature::Extrude(extrude)])
            .expect("two features are valid data");
        assert!(matches!(build_constructive_mesh(&constructive), Err(ParamError::UnsupportedConstructive)));
    }

    #[test]
    fn a_contour_the_cap_cannot_hold_is_refused_before_the_mesh_is_built() {
        use furnicraft_model::ContourSegment;
        use furnicraft_units::Degrees;

        let start = Degrees::from_millidegrees(0);
        let sweep = Degrees::from_millidegrees(350_000);
        let outer =
            ContourSegment::arc([Length::ZERO, Length::ZERO], millimetres(5_000), start, sweep).expect("outer arc");
        let inner = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(4_000),
            sweep,
            Degrees::from_millidegrees(-350_000),
        )
        .expect("inner arc");
        let contour = Contour::new(vec![
            outer,
            ContourSegment::line(outer.end().expect("outer end"), inner.start().expect("inner start"))
                .expect("inward line"),
            inner,
            ContourSegment::line(inner.end().expect("inner end"), outer.start().expect("outer start"))
                .expect("outward line"),
        ])
        .expect("a simple band");
        let constructive = Constructive::extrude(contour, millimetres(18)).expect("constructive");
        assert!(matches!(build_constructive_mesh(&constructive), Err(ParamError::ContourTooDetailed { .. })));
    }

    #[test]
    fn a_rotated_frame_is_refused_by_the_analytic_backend() {
        let profile = Profile::rectangle(10.0, 10.0).expect("profile");
        let frame = Frame { x: DVec3::Y, y: -DVec3::X, ..Frame::WORLD };
        let extrude = Extrude::new(profile, frame, 5.0).expect("extrusion");
        let error = mesh_from_extrusion(&extrude).expect_err("a rotated frame has no analytic mesh");
        assert!(matches!(error, KernelError::Unsupported(_)), "{error}");
    }
}
