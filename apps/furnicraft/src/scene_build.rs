//! Sample document and evaluation of the container tree into a render scene.
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
    Catalog, Container, CutoutDepth, CutoutShape, EdgeBanding, EdgeBandingMaterial, EdgeId, FaceSide, Id, MaterialId,
    Node, Panel, PanelMaterial, PanelNominalSize, PanelRect, Placement, Project,
};
use furnicraft_param::build_panel_parts_with;
use furnicraft_scene::{Camera, Drawable, SceneFrame};
use furnicraft_units::Length;

/// Colours for the sample panels, in linear RGB.
const PANEL_COLORS: [[f32; 4]; 3] = [[0.62, 0.42, 0.22, 1.0], [0.50, 0.34, 0.20, 1.0], [0.45, 0.47, 0.50, 1.0]];

/// A common debug colour for every band on a panel.
const EDGE_BAND_COLOR: [f32; 4] = [0.18, 0.20, 0.24, 1.0];

/// Sample catalogue ids. They are stable only for this demonstration document.
const SAMPLE_OAK_MATERIAL: u64 = 1;
const SAMPLE_WHITE_MATERIAL: u64 = 2;
const SAMPLE_OAK_EDGE: u64 = 101;
const SAMPLE_WHITE_EDGE: u64 = 102;

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
/// Three panels demonstrate face shading, through and blind cutouts, and both container kinds: an
/// assembly is a furniture component, a group is an organisational container. The sample is
/// document data created once at startup; user edits will go through engine commands.
pub fn sample_session() -> Result<Session, String> {
    let root_id = id(1)?;
    let root = Container::new(root_id, "FurniCraft sample", Placement::IDENTITY, Vec::new());
    let mut project = Project::new(root).map_err(describe)?;
    let oak = material_id(SAMPLE_OAK_MATERIAL)?;
    let white = material_id(SAMPLE_WHITE_MATERIAL)?;
    let oak_edge = material_id(SAMPLE_OAK_EDGE)?;
    let white_edge = material_id(SAMPLE_WHITE_EDGE)?;

    let standing = Placement::new(DVec3::ZERO, DQuat::from_rotation_x(FRAC_PI_2)).map_err(describe)?;
    let thickness = millimetres(18)?;
    let back = project
        .add_panel(root_id, "Back panel", panel_nominal_size(900, 600)?, Some(oak), thickness, standing)
        .map_err(describe)?;
    project.add_cutout(back, cutout(60, 70, 180, 140)?, CutoutDepth::Through, FaceSide::A).map_err(describe)?;
    project
        .add_cutout(back, cutout(330, 70, 180, 140)?, CutoutDepth::Blind { depth: millimetres(12)? }, FaceSide::A)
        .map_err(describe)?;
    add_band(&mut project, back, 1, true, oak_edge)?;
    add_band(&mut project, back, 2, false, oak_edge)?;

    let floor = Placement::new(DVec3::new(150.0, -500.0, 0.0), DQuat::IDENTITY).map_err(describe)?;
    let shelf = project
        .add_panel(root_id, "Loose shelf", panel_nominal_size(600, 400)?, Some(white), thickness, floor)
        .map_err(describe)?;
    project.add_cutout(shelf, cutout(80, 60, 150, 100)?, CutoutDepth::Through, FaceSide::B).map_err(describe)?;
    add_band(&mut project, shelf, 4, true, white_edge)?;

    project.group_nodes(&[back, shelf], "Loose parts").map_err(describe)?;

    let drawer = project.add_component(root_id, "Drawer", Placement::IDENTITY).map_err(describe)?;
    let front_offset = Placement::new(DVec3::new(0.0, 120.0, 40.0), DQuat::IDENTITY).map_err(describe)?;
    project
        .add_panel(drawer, "Drawer front", panel_nominal_size(500, 180)?, Some(white), thickness, front_offset)
        .map_err(describe)?;

    project.validate().map_err(describe)?;
    Ok(Session::with_catalog(project, sample_catalog()?))
}

/// Builds the demonstration catalogue handed to the sample session.
pub fn sample_catalog() -> Result<Catalog, String> {
    let thickness = millimetres(18)?;
    let oak =
        PanelMaterial::new(material_id(SAMPLE_OAK_MATERIAL)?, "Oak veneered chipboard", "chipboard", thickness, "oak")
            .map_err(describe)?;
    let white = PanelMaterial::new(
        material_id(SAMPLE_WHITE_MATERIAL)?,
        "White melamine chipboard",
        "chipboard",
        thickness,
        "white",
    )
    .map_err(describe)?;
    let edge_thickness = millimetres(2)?;
    let edge_width = millimetres(22)?;
    let oak_edge = EdgeBandingMaterial::new(
        material_id(SAMPLE_OAK_EDGE)?,
        "Oak ABS edge",
        "abs",
        edge_thickness,
        edge_width,
        "oak",
    )
    .map_err(describe)?;
    let white_edge = EdgeBandingMaterial::new(
        material_id(SAMPLE_WHITE_EDGE)?,
        "White ABS edge",
        "abs",
        edge_thickness,
        edge_width,
        "white",
    )
    .map_err(describe)?;
    Catalog::new(vec![oak, white], vec![oak_edge, white_edge]).map_err(describe)
}

/// Evaluates every panel in the session into world-space drawables.
///
/// Catalogue values decide panel and banding geometry; the kernel is cleared on error so a failed
/// evaluation cannot leak bodies into the next rebuild.
pub fn evaluate(kernel: &mut TruckKernel, session: &Session) -> Result<EvaluatedScene, String> {
    let project = session.project();
    let root_world = Placement::compose(Placement::IDENTITY, project.root().placement()).map_err(describe)?;
    let mut evaluation = Evaluation::new(kernel, session);
    if let Err(error) = evaluation.collect(project.root().children(), root_world) {
        evaluation.kernel.clear();
        return Err(error);
    }
    Ok(EvaluatedScene { frame: evaluation.frame, bounds: evaluation.bounds })
}

/// Mutable state shared by one scene evaluation.
struct Evaluation<'a> {
    kernel: &'a mut TruckKernel,
    session: &'a Session,
    options: TessOptions,
    frame: SceneFrame,
    bounds: Bounds,
    palette_index: usize,
}

impl<'a> Evaluation<'a> {
    fn new(kernel: &'a mut TruckKernel, session: &'a Session) -> Self {
        Self {
            kernel,
            session,
            options: TessOptions::default(),
            frame: SceneFrame::default(),
            bounds: Bounds::empty(),
            palette_index: 0,
        }
    }

    fn collect(&mut self, children: &[Node], parent: Placement) -> Result<(), String> {
        for child in children {
            match child {
                Node::Container(container) => {
                    let world = Placement::compose(parent, container.placement()).map_err(describe)?;
                    self.collect(container.children(), world)?;
                }
                Node::Panel(panel) => self.collect_panel(panel, parent)?,
                // A geometry object has no drawable yet; the mesh path lands with the object
                // commands. It still contributes the document ids the tree and the inspector show.
                Node::Object(_) => {}
            }
        }
        Ok(())
    }

    fn collect_panel(&mut self, panel: &Panel, parent: Placement) -> Result<(), String> {
        let world = Placement::compose(parent, panel.placement()).map_err(describe)?;
        let session = self.session;
        let geometry = session.panel_geometry(panel.id()).map_err(describe)?;
        let parts = build_panel_parts_with(&mut *self.kernel, panel, geometry, |spec| session.band_spec_profile(spec))
            .map_err(describe)?;
        let body_mesh = tessellate_panel(&*self.kernel, parts.body(), &self.options).map_err(describe)?;
        let mut band_meshes = Vec::with_capacity(parts.bands().len());
        for part in parts.bands() {
            band_meshes.push(tessellate_panel(&*self.kernel, part.body(), &self.options).map_err(describe)?);
        }
        self.kernel.clear();
        let transform = DMat4::from_rotation_translation(world.rotation(), world.translation());
        self.bounds.include_mesh(&body_mesh, transform);
        let color = PANEL_COLORS[self.palette_index % PANEL_COLORS.len()];
        self.palette_index = self.palette_index.saturating_add(1);
        self.frame.push(Drawable::new(panel.id(), Arc::new(body_mesh), transform, color));
        for mesh in band_meshes {
            self.bounds.include_mesh(&mesh, transform);
            self.frame.push(Drawable::new(panel.id(), Arc::new(mesh), transform, EDGE_BAND_COLOR));
        }
        Ok(())
    }
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

fn material_id(value: u64) -> Result<MaterialId, String> {
    MaterialId::new(value).ok_or_else(|| "material ids start at one".to_owned())
}

fn millimetres(value: i64) -> Result<Length, String> {
    Length::from_millimetres(value).ok_or_else(|| format!("{value} mm does not fit a length"))
}

fn add_band(
    project: &mut Project,
    panel: Id,
    edge: u32,
    included_in_size: bool,
    material: MaterialId,
) -> Result<(), String> {
    let edge = EdgeId::new(edge).ok_or_else(|| "edge ids start at one".to_owned())?;
    let band =
        EdgeBanding::new(edge, material, included_in_size, millimetres(2)?, millimetres(22)?).map_err(describe)?;
    project.set_edge_banding(panel, band).map_err(describe)?;
    Ok(())
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

    fn panels_in<'a>(children: &'a [Node], panels: &mut Vec<&'a Panel>) {
        for child in children {
            match child {
                Node::Panel(panel) => panels.push(panel),
                Node::Container(container) => panels_in(container.children(), panels),
                Node::Object(_) => {}
            }
        }
    }

    #[test]
    fn sample_document_has_three_panels_and_three_cutouts() {
        let session = sample_session().expect("sample session");
        let project = session.project();
        project.validate().expect("valid sample document");

        let mut panels = Vec::new();
        panels_in(project.root().children(), &mut panels);
        let cutouts: usize = panels
            .iter()
            .map(|panel| panel.machining().iter().filter(|machining| matches!(machining, Machining::Cutout(_))).count())
            .sum();
        let bands: usize = panels.iter().map(|panel| panel.edge_banding().len()).sum();
        assert_eq!(panels.len(), 3);
        assert_eq!(cutouts, 3);
        assert_eq!(bands, 3);
    }

    #[test]
    fn sample_document_has_one_group_and_one_component() {
        let session = sample_session().expect("sample session");
        let project = session.project();
        let root = project.root();
        assert_eq!(root.children().len(), 2);
        let mut groups = 0;
        let mut components = 0;
        for child in root.children() {
            if let Node::Container(container) = child {
                if container.is_component() {
                    components += 1;
                } else {
                    groups += 1;
                }
            }
        }
        assert_eq!(groups, 1);
        assert_eq!(components, 1);
    }

    #[test]
    fn sample_materials_resolve_from_the_catalogue() {
        let session = sample_session().expect("sample session");
        let catalog = session.catalog();
        let mut panel_materials = Vec::new();
        let mut panels = Vec::new();
        panels_in(session.project().root().children(), &mut panels);
        for panel in &panels {
            let material = panel.material().expect("sample panel has a material");
            let resolved = catalog.panel_material(material).expect("sample panel material resolves");
            assert!(!resolved.name().is_empty());
            panel_materials.push(material);
            for band in panel.edge_banding() {
                let resolved = catalog.edge_material(band.material()).expect("sample edge material resolves");
                assert!(!resolved.name().is_empty());
            }
        }
        assert_eq!(panel_materials.len(), 3);
        assert_ne!(panel_materials[0], panel_materials[1], "the two panels demonstrate different materials");
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
    fn sample_scene_evaluates_panels_and_banding() {
        let session = sample_session().expect("sample session");
        let mut kernel = TruckKernel::new();
        let evaluated = evaluate(&mut kernel, &session).expect("evaluated scene");

        assert_eq!(evaluated.frame.drawables().len(), 6);
        let mut groups = std::collections::HashMap::new();
        for drawable in evaluated.frame.drawables() {
            assert!(!drawable.mesh.positions.is_empty());
            *groups.entry(drawable.id).or_insert(0) += 1;
        }
        assert_eq!(groups.len(), 3, "three panels");
        let mut counts: Vec<_> = groups.values().copied().collect();
        counts.sort_unstable();
        assert_eq!(counts, vec![1, 2, 3], "body plus bands per panel");
        assert!(evaluated.bounds.center().is_some());
        assert!(home_camera(evaluated.bounds).is_ok());
        assert_eq!(kernel.body_count(), 0, "evaluation releases every kernel body");
    }
}
