//! The document the editor opens with, and evaluation of the container tree into a render scene.
//!
//! This is composition-root code: it owns the geometry adapter, evaluates panels through the
//! parametric layer and composes placements into world transforms. It does not mutate the
//! document; editing commands arrive in a later milestone.

use std::sync::Arc;

use furnicraft_engine::Session;
use furnicraft_geom::{DMat4, DVec3, MeshData};
use furnicraft_kernel::{SolidKernel, TessOptions};
use furnicraft_kernel_truck::TruckKernel;
use furnicraft_mesh::EvaluationCache;
use furnicraft_model::{
    CameraProjection, Catalog, Container, EdgeBandingMaterial, Envelope, Id, LightKind, MaterialId, Node, NodeQuery,
    Panel, PanelMaterial, Placement, Project, StructuralKind,
};
// The demonstration fixture is the only caller of these: a test build evaluates the same document
// a maker builds, and nothing else in the shell creates cutouts, bands or a named panel size.
#[cfg(test)]
use furnicraft_geom::DQuat;
#[cfg(test)]
use furnicraft_model::{CutoutDepth, CutoutShape, EdgeBanding, EdgeId, FaceSide, PanelNominalSize, PanelRect};
use furnicraft_scene::{
    Camera, ContactShadow, DEFAULT_FOV_Y, Drawable, Lighting, MAX_POINT_LIGHTS, PointLight, Polyline, ProductEnvelope,
    ProductEnvelopeState, Projection, SceneFrame,
};
use furnicraft_ui::ViewFlags;
use furnicraft_units::Length;
#[cfg(test)]
use std::f64::consts::FRAC_PI_2;

/// A panel's surface colour, taken in tree order, in linear RGB: v1 has no per-panel material
/// colour and the three tones keep neighbouring parts readable.
const PANEL_COLORS: [[f32; 4]; 3] = [[0.62, 0.42, 0.22, 1.0], [0.50, 0.34, 0.20, 1.0], [0.45, 0.47, 0.50, 1.0]];

/// A common debug colour for every band on a panel.
const EDGE_BAND_COLOR: [f32; 4] = [0.18, 0.20, 0.24, 1.0];

/// The default surface colour of a geometry object; v1 has no per-object material.
const OBJECT_COLOR: [f32; 4] = [0.58, 0.60, 0.64, 1.0];

/// Default catalogue ids. The catalogue is not scene data and is never written to a file, so
/// these numbers are this catalogue's own labels.
const DEFAULT_OAK_MATERIAL: u64 = 1;
const DEFAULT_WHITE_MATERIAL: u64 = 2;
const DEFAULT_OAK_EDGE: u64 = 101;
const DEFAULT_WHITE_EDGE: u64 = 102;

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
            self.include_point(point);
        }
    }

    fn include_point(&mut self, point: DVec3) {
        if !point.is_finite() {
            return;
        }
        self.min = DVec3::new(self.min.x.min(point.x), self.min.y.min(point.y), self.min.z.min(point.z));
        self.max = DVec3::new(self.max.x.max(point.x), self.max.y.max(point.y), self.max.z.max(point.z));
        self.valid = true;
    }

    fn include_bounds(&mut self, other: Self) {
        if !other.valid {
            return;
        }
        self.include_point(other.min);
        self.include_point(other.max);
    }

    /// The minimum corner of the box, or `None` when no finite vertex was included.
    pub fn min(self) -> Option<DVec3> {
        self.valid.then_some(self.min)
    }

    /// The maximum corner of the box, or `None` when no finite vertex was included.
    pub fn max(self) -> Option<DVec3> {
        self.valid.then_some(self.max)
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

/// Builds the demonstration project the interface tests draw.
///
/// Three panels exercise face shading, through and blind cutouts, and both container kinds: an
/// assembly is a furniture component, a group is an organisational container. It is a test fixture
/// and nothing else: the editor opens [`initial_session`], which carries no object at all, because a
/// sample piece is in the way of the first real one.
#[cfg(test)]
pub fn demo_session() -> Result<Session, String> {
    let root_id = id(1)?;
    let root = Container::new(root_id, "Demo", Placement::IDENTITY, Vec::new());
    let mut project = Project::new(root).map_err(describe)?;
    let oak = material_id(DEFAULT_OAK_MATERIAL)?;
    let white = material_id(DEFAULT_WHITE_MATERIAL)?;
    let oak_edge = material_id(DEFAULT_OAK_EDGE)?;
    let white_edge = material_id(DEFAULT_WHITE_EDGE)?;

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
    Ok(Session::with_catalog(project, default_catalog()?))
}

/// The session the window opens with: an empty scene and the default catalogue.
pub fn initial_session() -> Result<Session, String> {
    empty_session()
}

/// Builds an empty scene: a root container with no children.
///
/// A new document still carries the session catalogue; the catalogue is not scene data and is
/// never written to the file, so starting from the default materials only affects what the
/// inspector can offer, not what a save contains.
pub fn empty_session() -> Result<Session, String> {
    let root = Container::new(id(1)?, "Scene", Placement::IDENTITY, Vec::new());
    let project = Project::new(root).map_err(describe)?;
    Ok(Session::with_catalog(project, default_catalog()?))
}

/// Builds the catalogue a new session starts from: two panel materials and their edges.
pub fn default_catalog() -> Result<Catalog, String> {
    let thickness = millimetres(18)?;
    let oak =
        PanelMaterial::new(material_id(DEFAULT_OAK_MATERIAL)?, "Oak veneered chipboard", "chipboard", thickness, "oak")
            .map_err(describe)?;
    let white = PanelMaterial::new(
        material_id(DEFAULT_WHITE_MATERIAL)?,
        "White melamine chipboard",
        "chipboard",
        thickness,
        "white",
    )
    .map_err(describe)?;
    let edge_thickness = millimetres(2)?;
    let edge_width = millimetres(22)?;
    let oak_edge = EdgeBandingMaterial::new(
        material_id(DEFAULT_OAK_EDGE)?,
        "Oak ABS edge",
        "abs",
        edge_thickness,
        edge_width,
        "oak",
    )
    .map_err(describe)?;
    let white_edge = EdgeBandingMaterial::new(
        material_id(DEFAULT_WHITE_EDGE)?,
        "White ABS edge",
        "abs",
        edge_thickness,
        edge_width,
        "white",
    )
    .map_err(describe)?;
    Catalog::new(vec![oak, white], vec![oak_edge, white_edge]).map_err(describe)
}

/// Evaluates a constructive object into world-space drawables and product envelopes.
///
/// The cache is the only evaluator: a node whose derivation inputs did not change is reused, so a
/// move, a rename or an unrelated edit rebuilds no mesh.
///
/// `open_product` is the session's one open product (ADR 0098), which only decides the ink of its
/// envelope and whether the frame shows while its parts are closed (ADR 0100). The shell passes
/// `Session::active_product`; it is view state, never document data.
pub fn evaluate(
    cache: &mut EvaluationCache,
    kernel: &mut TruckKernel,
    session: &Session,
    flags: &ViewFlags,
    open_product: Option<Id>,
) -> Result<EvaluatedScene, String> {
    let project = session.project();
    let root_world = Placement::compose(Placement::IDENTITY, project.root().placement()).map_err(describe)?;
    cache.start();
    let mut evaluation = Evaluation::new(cache, kernel, session, flags, open_product);
    let mut scene_bounds = Bounds::empty();
    for child in project.root().children() {
        let subtree = match evaluation.collect(std::slice::from_ref(child), root_world) {
            Ok(subtree) => subtree,
            Err(error) => {
                // A failed pass must not leave half-evaluated bodies in the kernel, nor a stale
                // entry that claims a mesh the document no longer produces.
                evaluation.kernel.clear();
                return Err(error);
            }
        };
        scene_bounds.include_bounds(subtree.all);
        // One shadow per root object, derived from its solid bounds only: a drawing on the work
        // plane is not a body and casts nothing.
        let (Some(min), Some(max)) = (subtree.solid.min(), subtree.solid.max()) else {
            continue;
        };
        if let Some(shadow) = ContactShadow::from_bounds(child.id(), min, max) {
            evaluation.frame.push_contact_shadow(shadow);
        }
    }
    let Evaluation { cache, frame, .. } = evaluation;
    cache.finish();
    let mut frame = frame;
    if let Some(lighting) = authored_lighting_with(project, &[], flags) {
        frame.lighting = lighting;
    }
    Ok(EvaluatedScene { frame, bounds: scene_bounds })
}

/// The frame lighting from the authored lights.
///
/// The first sun becomes the key light: it emits along its local `-Z`, so the direction from a
/// surface towards it is the node's world rotation applied to `+Z`, and its colour is multiplied by
/// the intensity. Every point light contributes its world position and power, up to
/// [`MAX_POINT_LIGHTS`] in tree order. A scene with no authored light keeps the built-in key light;
/// so does a scene with point lights but no sun, because the point lights only add to the key.
#[cfg(test)]
pub fn authored_lighting(project: &Project) -> Option<Lighting> {
    authored_lighting_with(project, &[], &ViewFlags::default())
}

/// The frame lighting with previewed world placements for moved light nodes.
///
/// A gizmo drag never mutates the document, so the frame it renders must take the same previewed
/// placements the drawables use. An entry in `previewed` wins over the document's world placement;
/// it lets a sun direction and a point light position follow the pointer before the command that
/// commits the drag.
pub fn authored_lighting_with(project: &Project, previewed: &[(Id, Placement)], flags: &ViewFlags) -> Option<Lighting> {
    let mut lighting = Lighting::default();
    let mut any_authored = false;
    let mut key_set = false;
    for id in project.query(&NodeQuery::any().with_kind(StructuralKind::Light)) {
        let Some(light) = project.light(id) else {
            continue;
        };
        // A hidden light contributes nothing, exactly like a hidden body: the tree says it is off.
        if flags.hides(project, id) {
            continue;
        }
        let Some(world) = previewed_world(project, id, previewed) else {
            continue;
        };
        match light.kind() {
            LightKind::Sun if !key_set => {
                let Some(direction) = (world.rotation() * DVec3::Z).try_normalize() else {
                    continue;
                };
                let intensity = light.intensity();
                let color = light.color();
                lighting.key_direction = direction;
                lighting.key_color = [color[0] * intensity, color[1] * intensity, color[2] * intensity];
                key_set = true;
                any_authored = true;
            }
            LightKind::Sun => {}
            LightKind::Point => {
                if lighting.points.len() < MAX_POINT_LIGHTS {
                    lighting.points.push(PointLight {
                        position: world.translation(),
                        color: light.color(),
                        power: light.intensity(),
                    });
                }
                any_authored = true;
            }
        }
    }
    any_authored.then_some(lighting)
}

/// A node's world placement, preferring a previewed one during a drag.
pub fn previewed_world(project: &Project, id: Id, previewed: &[(Id, Placement)]) -> Option<Placement> {
    previewed
        .iter()
        .find(|(node, _)| *node == id)
        .map(|(_, placement)| *placement)
        .or_else(|| project.world_placement(id))
}

/// Bounds of one node subtree: every drawn point, and the solid meshes that cast a contact shadow.
#[derive(Clone, Copy, Debug)]
struct SubtreeBounds {
    /// Everything the subtree draws, line chains included; used to frame the scene.
    all: Bounds,
    /// Solid meshes only; a flat drawing touches the floor but stands on nothing.
    solid: Bounds,
}

impl SubtreeBounds {
    fn empty() -> Self {
        Self { all: Bounds::empty(), solid: Bounds::empty() }
    }

    fn include(&mut self, other: Self) {
        self.all.include_bounds(other.all);
        self.solid.include_bounds(other.solid);
    }
}

/// Mutable state shared by one scene evaluation.
struct Evaluation<'a> {
    cache: &'a mut EvaluationCache,
    kernel: &'a mut TruckKernel,
    session: &'a Session,
    /// The session's hidden set: a hidden node and its whole subtree are skipped.
    flags: &'a ViewFlags,
    /// The one open product, whose envelope takes the active ink, or `None` while none is open.
    open_product: Option<Id>,
    options: TessOptions,
    frame: SceneFrame,
    palette_index: usize,
}

impl<'a> Evaluation<'a> {
    fn new(
        cache: &'a mut EvaluationCache,
        kernel: &'a mut TruckKernel,
        session: &'a Session,
        flags: &'a ViewFlags,
        open_product: Option<Id>,
    ) -> Self {
        Self {
            cache,
            kernel,
            session,
            flags,
            open_product,
            options: TessOptions::default(),
            frame: SceneFrame::default(),
            palette_index: 0,
        }
    }

    fn collect(&mut self, children: &[Node], parent: Placement) -> Result<SubtreeBounds, String> {
        let mut bounds = SubtreeBounds::empty();
        for child in children {
            // A hidden node takes its whole subtree with it, so the frame, its bounds, its
            // contact shadows and picking all forget the branch in one place.
            if self.flags.is_hidden(child.id()) {
                continue;
            }
            let child_bounds = match child {
                Node::Container(container) => {
                    let world = Placement::compose(parent, container.placement()).map_err(describe)?;
                    let subtree = self.collect(container.children(), world)?;
                    match container.envelope() {
                        Some(envelope) => self.collect_product(container.id(), envelope, world, subtree),
                        None => subtree,
                    }
                }
                Node::Panel(panel) => self.collect_panel(panel, parent)?,
                Node::Object(object) => self.collect_object(object, parent)?,
                Node::Camera(_) | Node::Light(_) => SubtreeBounds::empty(),
            };
            bounds.include(child_bounds);
        }
        Ok(bounds)
    }

    /// Records one product's authored box as a scene item and returns the subtree bounds with it.
    ///
    /// The three states of ADR 0100 are decided here and nowhere else: the open product is the one
    /// the session names, a closed product whose subtree drew something keeps its lines hidden, and
    /// a closed product with nothing in it still shows its box. The envelope joins the drawn bounds
    /// so *Frame all* sees an empty piece; it never joins the solid bounds, because a reserved box is
    /// not a body and casts no contact shadow.
    fn collect_product(
        &mut self,
        id: Id,
        envelope: Envelope,
        world: Placement,
        mut bounds: SubtreeBounds,
    ) -> SubtreeBounds {
        let state = if self.open_product == Some(id) {
            ProductEnvelopeState::Open
        } else if bounds.all.valid {
            ProductEnvelopeState::ClosedWithParts
        } else {
            ProductEnvelopeState::ClosedEmpty
        };
        let size =
            DVec3::new(envelope.x().millimetres_f64(), envelope.y().millimetres_f64(), envelope.z().millimetres_f64());
        let transform = DMat4::from_rotation_translation(world.rotation(), world.translation());
        for bits in 0..8 {
            bounds.all.include_point(transform.transform_point3(DVec3::new(
                if bits & 1 == 0 { 0.0 } else { size.x },
                if bits & 2 == 0 { 0.0 } else { size.y },
                if bits & 4 == 0 { 0.0 } else { size.z },
            )));
        }
        self.frame.push_product_envelope(ProductEnvelope { id, size, world_transform: transform, state });
        bounds
    }

    fn collect_panel(&mut self, panel: &Panel, parent: Placement) -> Result<SubtreeBounds, String> {
        let world = Placement::compose(parent, panel.placement()).map_err(describe)?;
        let session = self.session;
        let geometry = session.panel_geometry(panel.id()).map_err(describe)?;
        let revision = session.project().revision();
        let evaluated = self
            .cache
            .panel(revision, panel, &geometry, &|spec| session.band_spec_profile(spec), self.kernel, &self.options)
            .map_err(describe)?;
        let transform = DMat4::from_rotation_translation(world.rotation(), world.translation());
        let color = PANEL_COLORS[self.palette_index % PANEL_COLORS.len()];
        self.palette_index = self.palette_index.saturating_add(1);
        let mut bounds = SubtreeBounds::empty();
        if let Some(body) = evaluated.body() {
            bounds.all.include_mesh(body, transform);
            bounds.solid.include_mesh(body, transform);
            let drawable = Drawable::new(panel.id(), Arc::clone(body), transform, color);
            self.frame.push(with_cached_edges(drawable, evaluated.body_edges(), evaluated.body_all_edges()));
        }
        for (index, mesh) in evaluated.bands().iter().enumerate() {
            bounds.all.include_mesh(mesh, transform);
            bounds.solid.include_mesh(mesh, transform);
            let drawable = Drawable::new(panel.id(), Arc::clone(mesh), transform, EDGE_BAND_COLOR);
            self.frame.push(with_cached_edges(
                drawable,
                evaluated.band_edges().get(index),
                evaluated.band_all_edges().get(index),
            ));
        }
        Ok(bounds)
    }

    /// Turns a geometry object into one flat-shaded drawable or one line chain in world space.
    ///
    /// A mesh payload is drawn as stored. A constructive or primitive payload is evaluated through
    /// the cache's one evaluator — the same function `object.collapse` bakes — so the collapsed
    /// object keeps the shape the viewport showed.
    fn collect_object(
        &mut self,
        object: &furnicraft_model::Object,
        parent: Placement,
    ) -> Result<SubtreeBounds, String> {
        let world = Placement::compose(parent, object.placement()).map_err(describe)?;
        let revision = self.session.project().revision();
        // A previewing operation hands the viewport the geometry the node would have, so the frame
        // draws the result the command bar is about to commit instead of the stored one.
        let geometry =
            self.session.drawn_geometry(object.id()).ok_or_else(|| format!("{}: no geometry", object.name()))?;
        let evaluated = self
            .cache
            .object_with(revision, object.id(), geometry)
            .map_err(|error| format!("{}: {error}", object.name()))?;
        let transform = DMat4::from_rotation_translation(world.rotation(), world.translation());
        let mut bounds = SubtreeBounds::empty();
        // A path has no solid: it is drawn as an unlit chain of line segments, and its points are
        // sampled from the same analytic segments a later extrusion uses. The chain leaves its
        // colour to the frame, so a theme switch repaints it without a rebuild.
        if let Some(points) = evaluated.polyline() {
            for point in points.iter() {
                bounds.all.include_point(transform.transform_point3(*point));
            }
            self.frame.push_polyline(Polyline::in_ink(object.id(), Arc::clone(points), transform));
            return Ok(bounds);
        }
        if let Some(mesh) = evaluated.body() {
            bounds.all.include_mesh(mesh, transform);
            bounds.solid.include_mesh(mesh, transform);
            let drawable = Drawable::new(object.id(), Arc::clone(mesh), transform, OBJECT_COLOR);
            self.frame.push(with_cached_edges(drawable, evaluated.body_edges(), evaluated.body_all_edges()));
            return Ok(bounds);
        }
        Err(format!("{}: the object has no drawable geometry", object.name()))
    }
}

/// Attaches the cached form edges and the full triangulation edges to a drawable.
///
/// The two lists come from the same cached mesh, so this copies two `Arc`s and never searches the
/// mesh. The renderer picks one from the frame's style; the other is shared with the cache.
fn with_cached_edges(drawable: Drawable, edges: Option<&Arc<[u32]>>, all_edges: Option<&Arc<[u32]>>) -> Drawable {
    let drawable = match edges {
        Some(edges) => drawable.with_edges(Arc::clone(edges)),
        None => drawable,
    };
    match all_edges {
        Some(all_edges) => drawable.with_all_edges(Arc::clone(all_edges)),
        None => drawable,
    }
}

/// Places the initial camera so the whole scene fits in the viewport.
pub fn home_camera(bounds: Bounds) -> Result<Camera, String> {
    let Some(center) = bounds.center() else {
        return Ok(Camera::default());
    };
    let radius = bounds.radius().unwrap_or(0.0).max(50.0);
    let half_fov = DEFAULT_FOV_Y * 0.5;
    let distance = radius / half_fov.tan() * 1.25;
    Camera::new(center, distance).map_err(describe)
}

/// Distance from an authored camera's eye to its orbit target, used as a pivot placeholder.
const AUTHORED_CAMERA_DISTANCE_MM: f64 = 1_000.0;

/// The authored camera the scene renders from, or `None` for the built-in camera.
///
/// A camera node looks down its local `-Z` with `+Y` up. The projection is copied from the
/// document; near and far stay at the scene defaults because the model does not store them yet.
/// An authored camera pins an explicit basis, so viewport navigation leaves it alone.
pub fn authored_camera(project: &Project) -> Option<Camera> {
    let id = project.active_camera()?;
    let world = project.world_placement(id)?;
    let projection = match project.camera(id)?.projection() {
        CameraProjection::Perspective { fov_y } => Projection::Perspective { fov_y: fov_y.radians_f64() },
        CameraProjection::Orthographic { height } => Projection::Orthographic { height: height.millimetres_f64() },
    };
    let eye = world.translation();
    let rotation = world.rotation();
    let right = rotation * DVec3::X;
    let up = rotation * DVec3::Y;
    let forward = rotation * -DVec3::Z;
    let target = eye + forward * AUTHORED_CAMERA_DISTANCE_MM;
    Camera::new(target, AUTHORED_CAMERA_DISTANCE_MM)
        .ok()
        .map(|camera| camera.looked_from(eye, right, up, forward).with_projection(projection))
}

/// The camera a viewport draws with: the active authored camera, or the navigable view fallback.
pub fn scene_camera(project: &Project, view: Camera) -> Camera {
    authored_camera(project).unwrap_or(view)
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

#[cfg(test)]
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

#[cfg(test)]
fn panel_nominal_size(length: i64, width: i64) -> Result<PanelNominalSize, String> {
    PanelNominalSize::new(millimetres(length)?, millimetres(width)?).map_err(describe)
}

#[cfg(test)]
fn rectangle(x: i64, y: i64, width: i64, height: i64) -> Result<PanelRect, String> {
    PanelRect::new(millimetres(x)?, millimetres(y)?, millimetres(width)?, millimetres(height)?).map_err(describe)
}

#[cfg(test)]
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
    use furnicraft_model::{ClassCode, ContourSegment, Geometry, Path, Revision};
    use furnicraft_testkit::sample_project;

    fn panels_in<'a>(children: &'a [Node], panels: &mut Vec<&'a Panel>) {
        for child in children {
            match child {
                Node::Panel(panel) => panels.push(panel),
                Node::Container(container) => panels_in(container.children(), panels),
                Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
            }
        }
    }

    #[test]
    fn an_authored_camera_maps_its_placement_and_projection() {
        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        let rotation = DQuat::from_rotation_z(FRAC_PI_2);
        let placement = Placement::new(DVec3::new(100.0, 200.0, 300.0), rotation).expect("placement");
        let fov = furnicraft_units::Degrees::from_millidegrees(60_000);
        let camera_id = project
            .add_camera(root, "Presentation", placement, CameraProjection::Perspective { fov_y: fov })
            .expect("camera");
        assert!(authored_camera(&project).is_none(), "the built-in camera is used while none is active");
        let view = Camera::default();
        assert_eq!(scene_camera(&project, view), view);

        project.set_active_camera(Some(camera_id)).expect("active camera");
        let camera = authored_camera(&project).expect("the authored camera");
        assert!((camera.eye() - DVec3::new(100.0, 200.0, 300.0)).length() < 1.0e-9);
        assert!(camera.forward().dot(rotation * -DVec3::Z) > 1.0 - 1.0e-9);
        assert!(camera.right().dot(rotation * DVec3::X) > 1.0 - 1.0e-9);
        assert!(camera.up().dot(rotation * DVec3::Y) > 1.0 - 1.0e-9);
        assert_eq!(camera.projection(), Projection::Perspective { fov_y: fov.radians_f64() });
        assert!(camera.project_ndc(camera.eye() + camera.forward() * 10.0, 1.0).is_some());

        // An orthographic camera keeps its visible height.
        project
            .set_camera_projection(
                camera_id,
                CameraProjection::Orthographic { height: Length::from_millimetres(500).expect("height") },
            )
            .expect("orthographic");
        let camera = authored_camera(&project).expect("camera");
        assert_eq!(
            camera.projection(),
            Projection::Orthographic { height: Length::from_millimetres(500).expect("height").millimetres_f64() }
        );
    }

    #[test]
    fn an_authored_sun_lights_the_frame_through_its_world_rotation() {
        // A scene without a light keeps the built-in key light; a point light adds to that key.
        let mut project = sample_project();
        assert_eq!(authored_lighting(&project), None);
        let root = project.root().id();
        project
            .add_light(root, "Fill", Placement::IDENTITY, LightKind::Point, [0.2, 0.3, 0.4], 1000.0)
            .expect("point light");
        let lighting = authored_lighting(&project).expect("a point light lights the frame");
        assert_eq!(lighting.key_color, Lighting::default().key_color, "a point-only scene keeps the built-in key");
        assert_eq!(lighting.points.len(), 1);
        assert_eq!(lighting.points[0].power, 1000.0);
        assert_eq!(lighting.points[0].color, [0.2, 0.3, 0.4]);

        let spin = DQuat::from_rotation_y(FRAC_PI_2);
        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), spin).expect("placement");
        project.add_light(root, "Sun", placement, LightKind::Sun, [1.0, 0.5, 0.25], 2.0).expect("sun");
        let lighting = authored_lighting(&project).expect("the sun lights the frame");
        assert!(
            lighting.key_direction.dot(DVec3::X) > 1.0 - 1.0e-9,
            "a sun emits along local -Z, so the direction towards it is +Z turned into +X: {lighting:?}"
        );
        assert_eq!(lighting.key_color, [2.0, 1.0, 0.5]);
        assert_eq!(lighting.ambient, Lighting::default().ambient);

        // The direction follows the whole parent chain, not only the light's own placement.
        let group = Container::new(
            id(2).expect("id"),
            "Group",
            Placement::new(DVec3::ZERO, DQuat::from_rotation_z(FRAC_PI_2)).expect("placement"),
            Vec::new(),
        );
        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, vec![Node::Container(group)]);
        let mut nested = Project::new(root).expect("project");
        nested
            .add_light(id(2).expect("id"), "Sun", placement, LightKind::Sun, [1.0, 1.0, 1.0], 1.0)
            .expect("nested sun");
        let lighting = authored_lighting(&nested).expect("the nested sun lights the frame");
        assert!(
            lighting.key_direction.dot(DVec3::Y) > 1.0 - 1.0e-9,
            "the parent turn and the light turn compose: {lighting:?}"
        );
    }

    #[test]
    fn point_lights_use_their_world_position_and_a_frame_shades_at_most_eight() {
        let mut project = sample_project();
        let root = project.root().id();
        for index in 1..=10 {
            let placement =
                Placement::new(DVec3::new(index as f64 * 100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
            project
                .add_light(root, format!("Point {index}"), placement, LightKind::Point, [1.0; 3], 1000.0)
                .expect("point light");
        }

        let lighting = authored_lighting(&project).expect("the point lights light the frame");
        assert_eq!(lighting.points.len(), MAX_POINT_LIGHTS, "the fragment loop has a fixed budget");
        for (index, point) in lighting.points.iter().enumerate() {
            assert_eq!(point.position, DVec3::new((index + 1) as f64 * 100.0, 0.0, 0.0));
            assert_eq!(point.power, 1000.0);
        }
    }

    #[test]
    fn a_point_light_adds_its_parent_placement() {
        let group = Container::new(
            id(2).expect("id"),
            "Group",
            Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement"),
            Vec::new(),
        );
        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, vec![Node::Container(group)]);
        let mut project = Project::new(root).expect("project");
        project
            .add_light(
                id(2).expect("id"),
                "Fill",
                Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement"),
                LightKind::Point,
                [1.0; 3],
                1000.0,
            )
            .expect("nested point light");

        let lighting = authored_lighting(&project).expect("the point light lights the frame");
        assert_eq!(lighting.points[0].position, DVec3::new(110.0, 0.0, 0.0));
    }

    #[test]
    fn a_previewed_placement_lights_the_frame_before_the_command_lands() {
        let mut project = sample_project();
        let root = project.root().id();
        let sun = project
            .add_light(
                root,
                "Sun",
                Placement::new(DVec3::ZERO, DQuat::IDENTITY).expect("placement"),
                LightKind::Sun,
                [1.0; 3],
                1.0,
            )
            .expect("sun");
        let point = project
            .add_light(
                root,
                "Fill",
                Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement"),
                LightKind::Point,
                [1.0; 3],
                1000.0,
            )
            .expect("point light");

        let base = authored_lighting_with(&project, &[], &ViewFlags::default()).expect("authored lighting");
        assert_eq!(base.points[0].position, DVec3::new(100.0, 0.0, 0.0));

        // A quarter turn about X aims the sun's emission along -Y, so the light comes from +Y.
        let turned = Placement::new(DVec3::ZERO, DQuat::from_rotation_x(-FRAC_PI_2)).expect("placement");
        let moved = Placement::new(DVec3::new(300.0, 40.0, 0.0), DQuat::IDENTITY).expect("placement");
        let previewed = authored_lighting_with(&project, &[(sun, turned), (point, moved)], &ViewFlags::default())
            .expect("previewed lighting");
        assert!(previewed.key_direction.dot(DVec3::Y) > 1.0 - 1.0e-9, "the previewed sun lights from +Y");
        assert_eq!(previewed.points[0].position, DVec3::new(300.0, 40.0, 0.0));
        assert_eq!(previewed.points[0].power, 1000.0, "a preview does not change the power");
    }

    #[test]
    fn the_demo_document_has_three_panels_and_three_cutouts() {
        let session = demo_session().expect("the demo session");
        let project = session.project();
        project.validate().expect("a valid demo document");

        let mut panels = Vec::new();
        panels_in(project.root().children(), &mut panels);
        let cutouts: usize = panels
            .iter()
            .map(|panel| panel.machining().iter().filter(|machining| machining.cutout().is_some()).count())
            .sum();
        let bands: usize = panels.iter().map(|panel| panel.edge_banding().len()).sum();
        assert_eq!(panels.len(), 3);
        assert_eq!(cutouts, 3);
        assert_eq!(bands, 3);
    }

    #[test]
    fn the_demo_document_has_one_group_and_one_component() {
        let session = demo_session().expect("the demo session");
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
    fn the_demo_materials_resolve_from_the_catalogue() {
        let session = demo_session().expect("the demo session");
        let catalog = session.catalog();
        let mut panel_materials = Vec::new();
        let mut panels = Vec::new();
        panels_in(session.project().root().children(), &mut panels);
        for panel in &panels {
            let material = panel.material().expect("a demo panel has a material");
            let resolved = catalog.panel_material(material).expect("a demo panel material resolves");
            assert!(!resolved.name().is_empty());
            panel_materials.push(material);
            for band in panel.edge_banding() {
                let resolved = catalog.edge_material(band.material()).expect("a demo edge material resolves");
                assert!(!resolved.name().is_empty());
            }
        }
        assert_eq!(panel_materials.len(), 3);
        assert_ne!(panel_materials[0], panel_materials[1], "the two panels demonstrate different materials");
    }

    #[test]
    fn an_empty_session_has_a_root_and_nothing_to_draw() {
        let session = empty_session().expect("empty session");
        session.project().validate().expect("valid empty document");
        assert!(session.project().root().children().is_empty());
        assert!(!session.is_dirty());

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        assert!(evaluated.frame.drawables().is_empty());
        assert!(evaluated.frame.shadows().is_empty(), "nothing on the floor casts a shadow");
        assert_eq!(evaluated.bounds.center(), None);
        assert!(home_camera(evaluated.bounds).is_ok(), "an empty scene still has a usable camera");
    }

    #[test]
    fn the_editor_opens_a_document_with_nothing_in_it() {
        // The launcher calls this function; a demonstration scene there would put three panels, a
        // group and a drawer in front of the maker's first piece.
        let session = initial_session().expect("the initial session");
        assert!(session.project().root().children().is_empty(), "a new document has no object");
        assert!(!session.is_dirty(), "opening a document is not an edit");
        assert!(
            !session.catalog().panel_materials().is_empty(),
            "the default materials stay: a panel needs one to be created"
        );
    }

    #[test]
    fn a_larger_scene_fits_farther_out() {
        let bounds = |half_extent: f64| {
            let mut bounds = Bounds::empty();
            bounds.include_point(DVec3::splat(-half_extent));
            bounds.include_point(DVec3::splat(half_extent));
            bounds
        };
        let small = home_camera(bounds(100.0)).expect("small fit");
        let large = home_camera(bounds(1_000.0)).expect("large fit");
        assert!(
            large.distance() > small.distance(),
            "a larger scene frames from farther away: {} vs {}",
            large.distance(),
            small.distance()
        );
        assert_eq!(small.target(), DVec3::ZERO, "the fit looks at the scene centre");
        assert_eq!(large.target(), DVec3::ZERO);
    }

    #[test]
    fn a_drawing_on_the_floor_casts_no_contact_shadow() {
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)])
                    .expect("line"),
            ],
            false,
        )
        .expect("path");

        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("object");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        assert_eq!(evaluated.frame.polylines().len(), 1);
        assert!(evaluated.frame.shadows().is_empty(), "a flat chain is not a body and stands on nothing");
    }

    #[test]
    fn the_demo_scene_rests_on_one_contact_shadow() {
        let session = demo_session().expect("the demo session");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");

        assert_eq!(evaluated.frame.shadows().len(), 1, "only the floor-standing group casts one");
        let shadow = evaluated.frame.shadows().first().expect("shadow");
        assert!(shadow.radii[0] > 0.0 && shadow.radii[1] > 0.0);
        assert_eq!(shadow.center.z, 0.01, "the ellipse lies on the shadow plane");
        let floor = evaluated.bounds.min().expect("bounds");
        assert!(floor.z.abs() < 1.0e-9, "the demo scene stands on the floor");
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
    fn a_constructive_object_is_evaluated_into_a_drawable() {
        use furnicraft_model::{Constructive, Contour};

        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let contour = Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(600), millimetres(0)],
            [millimetres(600), millimetres(400)],
            [millimetres(0), millimetres(400)],
        ])
        .expect("contour");
        let constructive = Constructive::extrude(contour.into(), millimetres(18)).expect("constructive");

        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        let object =
            project.add_object(root, "Top", Placement::IDENTITY, Geometry::Constructive(constructive)).expect("object");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        assert_eq!(evaluated.frame.drawables().len(), 1, "the derived mesh is one drawable");
        let drawable = &evaluated.frame.drawables()[0];
        assert_eq!(drawable.id, object, "the drawable carries the object id, so picking and selection work");
        assert!(!drawable.mesh.positions.is_empty(), "the extrusion is tessellated from its contour");
        let center = evaluated.bounds.center().expect("the object contributes bounds");
        assert!(center.y > 0.0 && center.z > 0.0, "bounds follow the contour and the extrusion height");
    }

    #[test]
    fn a_primitive_is_drawn_from_its_parameters_and_picks_by_id() {
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let primitive = furnicraft_model::Primitive::sphere(millimetres(40), 16, 8).expect("sphere");
        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        let object =
            project.add_object(root, "Ball", Placement::IDENTITY, Geometry::Primitive(primitive)).expect("object");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        assert_eq!(evaluated.frame.drawables().len(), 1, "a primitive is one drawable");
        let drawable = &evaluated.frame.drawables()[0];
        assert_eq!(drawable.id, object, "the drawable carries the object id");
        assert!(!drawable.mesh.positions.is_empty(), "the mesh is derived from the parameters");
        let center = evaluated.bounds.center().expect("the object contributes bounds");
        assert!(center.x.abs() < 1.0e-3 && center.z.abs() < 1.0e-3, "a sphere is centred on its origin");
    }

    #[test]
    fn an_open_path_is_drawn_as_a_polyline() {
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)])
                    .expect("line"),
            ],
            false,
        )
        .expect("path");

        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("placement");
        let object = project.add_object(root, "Sketch", placement, Geometry::Path(path)).expect("object");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        assert!(evaluated.frame.drawables().is_empty(), "a path has no solid");
        assert_eq!(evaluated.frame.polylines().len(), 1);
        let line = &evaluated.frame.polylines()[0];
        assert_eq!(line.id, object);
        assert_eq!(line.points.len(), 3, "two segments share one joint");
        assert_eq!(line.points[0], DVec3::ZERO);
        assert_eq!(line.points[1], DVec3::new(600.0, 0.0, 0.0));
        assert_eq!(line.points[2], DVec3::new(600.0, 400.0, 0.0));
        let origin = line.world_transform.transform_point3(line.points[0]);
        assert_eq!(origin, DVec3::new(10.0, 20.0, 30.0), "the object placement moves the chain");
        let center = evaluated.bounds.center().expect("the path contributes bounds");
        assert!(center.x > 10.0 && center.y > 20.0);
    }

    #[test]
    fn a_closed_path_draws_its_closing_segment() {
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let corner = |x: i64, y: i64| [millimetres(x), millimetres(y)];
        let path = Path::new(
            vec![
                ContourSegment::line(corner(0, 0), corner(600, 0)).expect("line"),
                ContourSegment::line(corner(600, 0), corner(0, 400)).expect("line"),
                ContourSegment::line(corner(0, 400), corner(0, 0)).expect("line"),
            ],
            true,
        )
        .expect("closed path");

        let root = Container::new(id(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root = project.root().id();
        project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("object");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));

        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");
        let line = &evaluated.frame.polylines()[0];
        assert_eq!(line.points.len(), 4, "the closing segment keeps its end point");
        assert_eq!(line.points[3], line.points[0], "the chain closes on its first point");
    }

    #[test]
    fn the_demo_scene_evaluates_panels_and_banding() {
        let session = demo_session().expect("the demo session");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("evaluated scene");

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

    /// The mesh handle of the first drawable carrying `id`.
    fn mesh_of(scene: &EvaluatedScene, id: Id) -> &Arc<MeshData> {
        scene
            .frame
            .drawables()
            .iter()
            .find(|drawable| drawable.id == id)
            .map(|drawable| &drawable.mesh)
            .expect("drawable")
    }

    fn panel_id(session: &Session, name: &str) -> Id {
        let mut panels = Vec::new();
        panels_in(session.project().root().children(), &mut panels);
        panels.iter().find(|panel| panel.name() == name).map(|panel| panel.id()).expect("panel by name")
    }

    #[test]
    fn a_rebuild_at_a_new_revision_reuses_every_unchanged_mesh() {
        let mut session = demo_session().expect("the demo session");
        let renamed = panel_id(&session, "Loose shelf");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let first = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("first scene");

        // A rename bumps the document revision but none of the derived geometry depends on it.
        let params = serde_json::json!({ "node": renamed, "name": "Shelf" });
        session.dispatch("node.rename", &params).expect("rename");

        let second = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("second scene");
        assert!(session.project().revision() > Revision::ZERO);
        assert_eq!(first.frame.drawables().len(), second.frame.drawables().len());
        for (before, after) in first.frame.drawables().iter().zip(second.frame.drawables()) {
            assert_eq!(before.id, after.id);
            assert!(Arc::ptr_eq(&before.mesh, &after.mesh), "an unchanged node keeps its mesh across a rebuild");
        }
        assert_eq!(kernel.body_count(), 0, "a hit touches no kernel body");
    }

    /// The highest vertex of the mesh drawn for `id`, in the node's own millimetres.
    fn top_of(scene: &EvaluatedScene, id: Id) -> f32 {
        mesh_of(scene, id).positions.iter().map(|position| position[2]).fold(f32::MIN, f32::max)
    }

    #[test]
    fn a_preview_draws_the_body_the_command_would_store() {
        // A previewing operation shows a node the document does not hold yet. The frame must follow
        // the overlay — the taller body, not the stored one — while the document keeps its height,
        // its revision and its history (M71).
        let mut project = sample_project();
        let root = project.root().id();
        let path = Path::rectangle(
            [millimetres(20).expect("x"), millimetres(10).expect("y")],
            [millimetres(120).expect("width"), millimetres(60).expect("height")],
        )
        .expect("a rectangle");
        let body = furnicraft_model::Constructive::extrude(path, millimetres(100).expect("height")).expect("a body");
        let object = project
            .add_object(root, "Body", Placement::IDENTITY, Geometry::Constructive(body.clone()))
            .expect("object");
        let mut session = Session::new(project);
        let revision = session.project().revision();
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let stored = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("stored scene");
        assert!((top_of(&stored, object) - 100.0).abs() < 0.5, "the document draws its own height");

        let mut previewed = body;
        previewed.set_height(millimetres(250).expect("height")).expect("a preview height");
        session.set_preview_geometry(object, Geometry::Constructive(previewed));
        let preview = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("preview scene");

        assert_eq!(session.preview().len(), 1, "one node is previewed");
        assert_eq!(session.project().revision(), revision, "a preview is not an edit");
        assert!(!session.can_undo(), "a preview leaves no undo entry");
        assert!((top_of(&preview, object) - 250.0).abs() < 0.5, "the frame shows the preview height");

        session.clear_preview();
        let restored =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("restored scene");
        assert!((top_of(&restored, object) - 100.0).abs() < 0.5, "the cross restores the pre-command body");
    }

    #[test]
    fn moving_a_panel_keeps_its_mesh_and_only_moves_its_transform() {
        let mut session = demo_session().expect("the demo session");
        let id = panel_id(&session, "Loose shelf");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let before = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("before move");

        let placement = Placement::new(DVec3::new(400.0, -500.0, 0.0), DQuat::IDENTITY).expect("placement");
        let params = serde_json::json!({ "node": id, "placement": placement });
        session.dispatch("node.placement.set", &params).expect("move");

        let after = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("after move");
        assert!(
            Arc::ptr_eq(mesh_of(&before, id), mesh_of(&after, id)),
            "a placement is not a derivation input, so the mesh is reused"
        );
        let transform = after.frame.drawables().iter().find(|drawable| drawable.id == id).map(|d| d.world_transform);
        assert!(transform.is_some_and(|transform| transform.w_axis.x > 0.0));
    }

    #[test]
    fn a_resize_rebuilds_only_the_resized_panel() {
        let mut session = demo_session().expect("the demo session");
        let resized = panel_id(&session, "Loose shelf");
        let untouched = panel_id(&session, "Back panel");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let before = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("before resize");

        let size = furnicraft_model::PanelNominalSize::new(
            millimetres(650).expect("length"),
            millimetres(420).expect("width"),
        )
        .expect("size");
        let params = serde_json::json!({ "panel": resized, "nominal_size": size });
        session.dispatch("panel.resize", &params).expect("resize");

        let after = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("after resize");
        assert!(!Arc::ptr_eq(mesh_of(&before, resized), mesh_of(&after, resized)), "the resized panel rebuilds");
        assert!(
            Arc::ptr_eq(mesh_of(&before, untouched), mesh_of(&after, untouched)),
            "an edit elsewhere leaves a panel's mesh alone"
        );
    }

    #[test]
    fn deleting_a_node_drops_its_cache_entry() {
        let mut session = demo_session().expect("the demo session");
        let id = panel_id(&session, "Loose shelf");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("scene");
        assert!(cache.cached(id).is_some());

        let params = serde_json::json!({ "node": id });
        session.dispatch("node.delete", &params).expect("delete");
        let after =
            evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("scene after delete");
        assert!(cache.cached(id).is_none(), "a deleted node releases its mesh");
        assert!(after.frame.drawables().iter().all(|drawable| drawable.id != id));
    }

    /// The frame forgets a hidden branch in one place: drawables, bounds, contact shadows and the
    /// light rig all read the same set, and the mesh cache keeps the entries for when it is shown
    /// again.
    #[test]
    fn a_hidden_container_takes_its_whole_subtree_out_of_the_frame() {
        let session = Session::with_catalog(sample_project(), default_catalog().expect("catalogue"));
        let shelf = Id::new(2).expect("shelf id");
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();

        let visible = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("the demo scene");
        assert!(visible.frame.drawables().iter().any(|drawable| drawable.id == front));

        let mut flags = ViewFlags::default();
        flags.set_hidden(drawer, true);
        let hidden = evaluate(&mut cache, &mut kernel, &session, &flags, None).expect("the scene without the drawer");
        let drawn: Vec<Id> = hidden.frame.drawables().iter().map(|drawable| drawable.id).collect();
        assert!(!drawn.contains(&front), "a child of a hidden container is not drawn");
        assert!(drawn.contains(&shelf), "a sibling branch stays drawn");
        assert!(hidden.frame.shadows().iter().all(|shadow| shadow.id != drawer), "and casts no shadow");
        assert!(
            cache.cached(front).is_none(),
            "a hidden branch releases its mesh like any node the pass did not touch; showing it rebuilds it"
        );
    }

    #[test]
    fn a_hidden_light_does_not_light_the_frame() {
        let mut project = sample_project();
        let root = project.root().id();
        let group = project.add_component(root, "Lamp", Placement::IDENTITY).expect("container");
        let sun = project.add_light(group, "Sun", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("sun");
        assert!(authored_lighting(&project).is_some(), "an authored sun is the key light");

        let mut flags = ViewFlags::default();
        flags.set_hidden(sun, true);
        assert_eq!(authored_lighting_with(&project, &[], &flags), None, "a hidden sun is switched off");

        let mut flags = ViewFlags::default();
        flags.set_hidden(group, true);
        assert_eq!(
            authored_lighting_with(&project, &[], &flags),
            None,
            "a light under a hidden container is switched off with it"
        );
    }

    /// A hidden node is gone from the frame, so the ray never finds it: picking reads the same
    /// scene the viewport draws and needs no second hidden set.
    #[test]
    fn a_hidden_node_cannot_be_picked() {
        let size =
            PanelNominalSize::new(millimetres(700).expect("length"), millimetres(500).expect("width")).expect("size");
        let hidden_panel = Panel::new(
            id(2).expect("id"),
            "Hidden",
            size,
            None,
            millimetres(18).expect("thickness"),
            Placement::IDENTITY,
        )
        .expect("panel");
        let aside = Placement::new(DVec3::new(3000.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let shown_panel =
            Panel::new(id(3).expect("id"), "Shown", size, None, millimetres(18).expect("thickness"), aside)
                .expect("panel");
        let root = Container::new(
            id(1).expect("id"),
            "Root",
            Placement::IDENTITY,
            vec![hidden_panel.into(), shown_panel.into()],
        );
        let project = Project::new(root).expect("project");
        let session = Session::with_catalog(project, default_catalog().expect("catalogue"));
        let camera = furnicraft_scene::Camera::new(DVec3::ZERO, 1500.0).expect("camera");
        let viewport = eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(800.0, 600.0));
        let pointer = viewport.center();
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();

        let mut visible = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("the scene");
        visible.frame.camera = camera;
        assert_eq!(
            crate::mesh_pick::pick_scene(viewport, &visible.frame, pointer),
            Some(id(2).expect("id")),
            "the camera looks at the panel at the origin"
        );

        let mut flags = ViewFlags::default();
        flags.set_hidden(id(2).expect("id"), true);
        let mut hidden =
            evaluate(&mut cache, &mut kernel, &session, &flags, None).expect("the scene without the panel");
        hidden.frame.camera = camera;
        assert_eq!(
            crate::mesh_pick::pick_scene(viewport, &hidden.frame, pointer),
            None,
            "a hidden panel is not in the frame, so the ray finds nothing"
        );
    }

    /// A root with one product of `1 x 1 x 1 m` at `(100, 200, 0)` and, when asked, one flat panel
    /// inside it. The product is not the root, because a product is always a node of the scene.
    fn product_scene(with_panel: bool) -> (Session, Id) {
        let root = Container::new(id(1).expect("id"), "Scene", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root_id = project.root().id();
        let envelope = Envelope::new(
            millimetres(1000).expect("length"),
            millimetres(1000).expect("length"),
            millimetres(1000).expect("length"),
        )
        .expect("a valid envelope");
        let placement = Placement::new(DVec3::new(100.0, 200.0, 0.0), DQuat::IDENTITY).expect("placement");
        let product =
            project.add_product(root_id, "Cabinet", ClassCode::GENERAL_PRODUCT, envelope, placement).expect("product");
        if with_panel {
            project
                .add_panel(
                    product,
                    "Shelf",
                    panel_nominal_size(600, 400).expect("size"),
                    None,
                    millimetres(18).expect("thickness"),
                    Placement::IDENTITY,
                )
                .expect("panel");
        }
        project.validate().expect("a valid product scene");
        (Session::new(project), product)
    }

    fn ray_down(x: f64, y: f64) -> furnicraft_scene::Ray {
        furnicraft_scene::Ray::new(DVec3::new(x, y, 2000.0), -DVec3::Z).expect("ray")
    }

    #[test]
    fn a_product_envelope_is_drawn_in_the_three_states_of_the_decision() {
        let (empty, product) = product_scene(false);
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let closed = evaluate(&mut cache, &mut kernel, &empty, &ViewFlags::default(), None).expect("scene");
        let envelopes = closed.frame.product_envelopes();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].id, product);
        assert_eq!(envelopes[0].state, ProductEnvelopeState::ClosedEmpty);
        assert_eq!(envelopes[0].size, DVec3::splat(1000.0));
        assert_eq!(
            DVec3::new(
                envelopes[0].world_transform.w_axis.x,
                envelopes[0].world_transform.w_axis.y,
                envelopes[0].world_transform.w_axis.z,
            ),
            DVec3::new(100.0, 200.0, 0.0)
        );
        assert!(
            closed.bounds.radius().is_some_and(|radius| radius > 0.0),
            "an empty piece is visible, so it is framed"
        );

        let open = evaluate(&mut cache, &mut kernel, &empty, &ViewFlags::default(), Some(product)).expect("scene");
        assert_eq!(open.frame.product_envelopes()[0].state, ProductEnvelopeState::Open);

        let (filled, product) = product_scene(true);
        let with_parts = evaluate(&mut cache, &mut kernel, &filled, &ViewFlags::default(), None).expect("scene");
        assert_eq!(with_parts.frame.product_envelopes()[0].state, ProductEnvelopeState::ClosedWithParts);
        let opened = evaluate(&mut cache, &mut kernel, &filled, &ViewFlags::default(), Some(product)).expect("scene");
        assert_eq!(opened.frame.product_envelopes()[0].state, ProductEnvelopeState::Open, "opening reveals the box");
        assert!(
            with_parts.frame.drawables().iter().any(|drawable| drawable.id != product),
            "the panel is drawn inside the closed piece"
        );
        assert!(closed.frame.drawables().iter().all(|drawable| drawable.id != product), "the envelope is never a body");
    }

    #[test]
    fn a_product_envelope_is_picked_inside_its_empty_space() {
        let (session, product) = product_scene(false);
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let scene = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("scene");

        assert_eq!(scene.frame.pick(ray_down(600.0, 700.0), 600.0), Some(product), "a click in the void");
        assert_eq!(scene.frame.pick(ray_down(150.0, 250.0), 600.0), Some(product), "a click on the box edge");
        assert_eq!(scene.frame.pick(ray_down(50.0, 50.0), 600.0), None, "a click outside the piece");

        let mut flags = ViewFlags::default();
        flags.set_hidden(product, true);
        let hidden = evaluate(&mut cache, &mut kernel, &session, &flags, None).expect("scene without the piece");
        assert_eq!(hidden.frame.pick(ray_down(600.0, 700.0), 600.0), None, "a hidden piece is not picked");
    }

    #[test]
    fn a_panel_inside_the_frame_wins_the_click() {
        let (session, product) = product_scene(true);
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let scene = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("scene");
        let panel = scene
            .frame
            .drawables()
            .iter()
            .find(|drawable| drawable.id != product)
            .map(|drawable| drawable.id)
            .expect("the panel is drawn");

        assert_eq!(scene.frame.pick(ray_down(150.0, 250.0), 600.0), Some(panel), "a click on a part picks the part");
        assert_eq!(scene.frame.pick(ray_down(950.0, 950.0), 600.0), Some(product), "the void still picks the frame");
        assert_ne!(panel, product);
    }

    #[test]
    fn the_innermost_product_wins_when_boxes_nest() {
        let root = Container::new(id(1).expect("id"), "Scene", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("project");
        let root_id = project.root().id();
        let box_envelope = |side: i64| {
            Envelope::new(
                millimetres(side).expect("length"),
                millimetres(side).expect("length"),
                millimetres(side).expect("length"),
            )
            .expect("a valid envelope")
        };
        let outer = project
            .add_product(root_id, "Outer", ClassCode::GENERAL_PRODUCT, box_envelope(1000), Placement::IDENTITY)
            .expect("outer product");
        let inner_placement = Placement::new(DVec3::new(400.0, 400.0, 0.0), DQuat::IDENTITY).expect("placement");
        let inner = project
            .add_product(root_id, "Inner", ClassCode::GENERAL_PRODUCT, box_envelope(200), inner_placement)
            .expect("inner product");
        project.validate().expect("valid nested products");
        let session = Session::new(project);
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let scene = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default(), None).expect("scene");

        assert_eq!(scene.frame.product_envelopes().len(), 2, "both pieces carry a box");
        assert_eq!(scene.frame.pick(ray_down(500.0, 500.0), 600.0), Some(inner), "the tightest box is the piece meant");
        assert_eq!(
            scene.frame.pick(ray_down(100.0, 100.0), 600.0),
            Some(outer),
            "outside the inner piece is the outer"
        );
        assert_ne!(inner, outer);
    }
}
