//! Sample document and evaluation of the assembly tree into a render scene.
//!
//! This is composition-root code: it owns the geometry adapter, evaluates panels through the
//! parametric layer and composes placements into world transforms. It does not mutate the
//! document; editing commands arrive in a later milestone.

use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use furnicraft_engine::Session;
use furnicraft_geom::{DMat4, DQuat, DVec3, MeshData};
use furnicraft_kernel::{SolidKernel, TessOptions};
use furnicraft_kernel_truck::TruckKernel;
use furnicraft_mesh::tessellate_panel;
use furnicraft_model::{
    Assembly, CutoutDepth, CutoutShape, FaceSide, Id, Node, PanelNominalSize, PanelRect, Placement, Project,
};
use furnicraft_param::build_panel;
use furnicraft_scene::{Camera, Drawable, SceneFrame};
use furnicraft_units::Length;

/// Colours for the sample panels, in linear RGB.
const PANEL_COLORS: [[f32; 4]; 3] = [[0.62, 0.42, 0.22, 1.0], [0.50, 0.34, 0.20, 1.0], [0.45, 0.47, 0.50, 1.0]];

/// A world-space axis-aligned bounds box built from evaluated meshes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    min: DVec3,
    max: DVec3,
    valid: bool,
}

impl Bounds {
    fn empty() -> Self {
        Self { min: DVec3::splat(f64::INFINITY), max: DVec3::splat(f64::NEG_INFINITY), valid: false }
    }

    fn include_mesh(&mut self, mesh: &MeshData, transform: DMat4) {
        for position in &mesh.positions {
            let point = transform.transform_point3(DVec3::new(
                f64::from(position[0]),
                f64::from(position[1]),
                f64::from(position[2]),
            ));
            if !point.is_finite() {
                continue;
            }
            self.min = DVec3::new(self.min.x.min(point.x), self.min.y.min(point.y), self.min.z.min(point.z));
            self.max = DVec3::new(self.max.x.max(point.x), self.max.y.max(point.y), self.max.z.max(point.z));
            self.valid = true;
        }
    }

    /// The centre of the box, or `None` when no finite vertex was included.
    pub fn center(self) -> Option<DVec3> {
        self.valid.then(|| (self.min + self.max) * 0.5)
    }

    /// The radius of the sphere around the centre that contains the box.
    pub fn radius(self) -> Option<f64> {
        self.valid.then(|| ((self.max - self.min) * 0.5).length())
    }
}

/// The evaluated frame plus the bounds used to frame the initial camera.
pub struct EvaluatedScene {
    /// Drawables and default scene settings in local-to-world space.
    pub frame: SceneFrame,
    /// Bounds of the evaluated meshes.
    pub bounds: Bounds,
}

/// Builds the sample project shown by the viewport.
///
/// Two panels demonstrate face shading and both through and blind cutouts. The sample is document
/// data created once at startup; user edits will go through engine commands.
pub fn sample_session() -> Result<Session, String> {
    let root_id = id(1)?;
    let root = Assembly::new(root_id, "FurniCraft sample", Placement::IDENTITY, Vec::new());
    let mut project = Project::new(root).map_err(describe)?;

    let standing = Placement::new(DVec3::ZERO, DQuat::from_rotation_x(FRAC_PI_2)).map_err(describe)?;
    let thickness = millimetres(18)?;
    let back = project
        .add_panel(root_id, "Back panel", panel_nominal_size(900, 600)?, None, thickness, standing)
        .map_err(describe)?;
    project.add_cutout(back, cutout(60, 70, 180, 140)?, CutoutDepth::Through, FaceSide::A).map_err(describe)?;
    project
        .add_cutout(back, cutout(330, 70, 180, 140)?, CutoutDepth::Blind { depth: millimetres(12)? }, FaceSide::A)
        .map_err(describe)?;

    let floor = Placement::new(DVec3::new(150.0, -500.0, 0.0), DQuat::IDENTITY).map_err(describe)?;
    let shelf = project
        .add_panel(root_id, "Loose shelf", panel_nominal_size(600, 400)?, None, thickness, floor)
        .map_err(describe)?;
    project.add_cutout(shelf, cutout(80, 60, 150, 100)?, CutoutDepth::Through, FaceSide::B).map_err(describe)?;

    project.validate().map_err(describe)?;
    Ok(Session::new(project))
}

/// Evaluates every panel in the project into world-space drawables.
///
/// The kernel is cleared on error so a failed evaluation cannot leak bodies into the next rebuild.
pub fn evaluate(kernel: &mut TruckKernel, project: &Project) -> Result<EvaluatedScene, String> {
    let options = TessOptions::default();
    let mut frame = SceneFrame::default();
    let mut bounds = Bounds::empty();
    let root_world = Placement::compose(Placement::IDENTITY, project.root().placement()).map_err(describe)?;
    let mut palette_index = 0;

    let result = collect_children(
        kernel,
        project.root().children(),
        root_world,
        &options,
        &mut frame,
        &mut bounds,
        &mut palette_index,
    );
    if let Err(error) = result {
        kernel.clear();
        return Err(error);
    }
    Ok(EvaluatedScene { frame, bounds })
}

fn collect_children(
    kernel: &mut TruckKernel,
    children: &[Node],
    parent: Placement,
    options: &TessOptions,
    frame: &mut SceneFrame,
    bounds: &mut Bounds,
    palette_index: &mut usize,
) -> Result<(), String> {
    for child in children {
        match child {
            Node::Assembly(assembly) => {
                let world = Placement::compose(parent, assembly.placement()).map_err(describe)?;
                collect_children(kernel, assembly.children(), world, options, frame, bounds, palette_index)?;
            }
            Node::Panel(panel) => {
                let world = Placement::compose(parent, panel.placement()).map_err(describe)?;
                let geometry = panel.geometry_with_fallbacks().map_err(describe)?;
                let body = build_panel(kernel, panel, geometry).map_err(describe)?;
                let mesh = tessellate_panel(kernel, body, options).map_err(describe)?;
                kernel.clear();
                let transform = DMat4::from_rotation_translation(world.rotation(), world.translation());
                bounds.include_mesh(&mesh, transform);
                let color = PANEL_COLORS[*palette_index % PANEL_COLORS.len()];
                *palette_index = palette_index.saturating_add(1);
                frame.push(Drawable::new(panel.id(), Arc::new(mesh), transform, color));
            }
        }
    }
    Ok(())
}

/// Places the initial camera so the whole sample fits in the viewport.
pub fn home_camera(bounds: Bounds) -> Result<Camera, String> {
    let Some(center) = bounds.center() else {
        return Ok(Camera::default());
    };
    let radius = bounds.radius().unwrap_or(0.0).max(50.0);
    let half_fov = Camera::default().fov_y() * 0.5;
    let distance = radius / half_fov.tan() * 1.25;
    Camera::new(center, distance).map_err(describe)
}

fn id(value: u64) -> Result<Id, String> {
    Id::new(value).ok_or_else(|| "ids start at one".to_owned())
}

fn millimetres(value: i64) -> Result<Length, String> {
    Length::from_millimetres(value).ok_or_else(|| format!("{value} mm does not fit a length"))
}

fn panel_nominal_size(length: i64, width: i64) -> Result<PanelNominalSize, String> {
    PanelNominalSize::new(millimetres(length)?, millimetres(width)?).map_err(describe)
}

fn rectangle(x: i64, y: i64, width: i64, height: i64) -> Result<PanelRect, String> {
    PanelRect::new(millimetres(x)?, millimetres(y)?, millimetres(width)?, millimetres(height)?).map_err(describe)
}

fn cutout(x: i64, y: i64, width: i64, height: i64) -> Result<CutoutShape, String> {
    Ok(CutoutShape::Rectangle(rectangle(x, y, width, height)?))
}

fn describe(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use furnicraft_model::Machining;

    #[test]
    fn sample_document_has_two_panels_and_three_cutouts() {
        let session = sample_session().expect("sample session");
        let project = session.project();
        project.validate().expect("valid sample document");

        let mut panels = 0;
        let mut cutouts = 0;
        for child in project.root().children() {
            if let Node::Panel(panel) = child {
                panels += 1;
                cutouts +=
                    panel.machining().iter().filter(|machining| matches!(machining, Machining::Cutout(_))).count();
            }
        }
        assert_eq!(panels, 2);
        assert_eq!(cutouts, 3);
    }

    #[test]
    fn bounds_report_their_centre_and_radius() {
        let mut bounds = Bounds::empty();
        let mesh = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [100.0, 200.0, 40.0]],
            normals: Vec::new(),
            indices: Vec::new(),
        };
        bounds.include_mesh(&mesh, DMat4::IDENTITY);
        assert_eq!(bounds.center(), Some(DVec3::new(50.0, 100.0, 20.0)));
        assert!(bounds.radius().is_some_and(|radius| radius > 0.0));
    }

    #[test]
    fn sample_scene_evaluates_into_two_drawables() {
        let session = sample_session().expect("sample session");
        let mut kernel = TruckKernel::new();
        let evaluated = evaluate(&mut kernel, session.project()).expect("evaluated scene");

        assert_eq!(evaluated.frame.drawables().len(), 2);
        assert!(evaluated.frame.drawables().iter().all(|drawable| !drawable.mesh.positions.is_empty()));
        assert!(evaluated.bounds.center().is_some());
        assert!(home_camera(evaluated.bounds).is_ok());
        assert_eq!(kernel.body_count(), 0, "evaluation releases every kernel body");
    }
}
