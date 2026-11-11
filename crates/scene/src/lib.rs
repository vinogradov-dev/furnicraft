//! Renderer-agnostic scene description: what a frame contains.
//!
//! Drawables, world transforms, colours, the grid, contact shadows and lights are plain data, so a
//! scene can be built and tested without a GPU. The camera and its projection live in [`camera`],
//! the rays that pick with it in [`ray`]; both are re-exported here. The renderer owns the GPU
//! resources and interprets this description; the document never stores a camera, a grid or a
//! drawable.
//!
//! Physical lengths are millimetres and angles radians, as in [`camera`]; a colour is gamma space
//! unless it says linear.
//!
//! The surface is over B6's 25 public items on purpose: they are the fields and constructors of the
//! frame's value types (`Drawable`, `Polyline`, `ShadingStyle`, `Grid`, `ContactShadow`,
//! `ContactShadows`, `Lighting`, `PointLight`) beside `SceneFrame`'s own builders, and a frame is
//! assembled from all of them at once — splitting them would put one description in eight modules a
//! reader has to join back.
//!
//! Changes when: what a frame carries, or how a frame describes a drawable, a shadow or a light,
//! changes.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::sync::Arc;

use furnicraft_geom::{DMat4, DQuat, DVec3, MeshData};
use furnicraft_model::Id;

use crate::ray::{ray_hit_drawable, ray_hit_polyline};

pub mod camera;
pub mod ray;

pub use camera::{
    Camera, CameraError, DEFAULT_FOV_Y, MAX_DISTANCE_MM, MAX_PITCH, MIN_DISTANCE_MM, MIN_PITCH, POLE_TOLERANCE,
    Projection, canonical_up,
};
pub use ray::{LINE_PICK_RADIUS_POINTS, Ray, ray_segment, ray_triangle};

/// Gamma-space colour of a selected line chain.
///
/// A tint mixed into the path colour moves too little to be seen, and the line pipeline cannot
/// widen a chain, so the selected chain takes this explicit colour and the shell repeats it as a
/// thick screen-space outline. Cyan stays visible over a brown panel, a light one and the yellow
/// unselected path.
pub const SELECTION_LINE_COLOR: [f32; 4] = [0.15, 0.85, 1.0, 1.0];

/// How the renderer shades the drawables of a frame.
///
/// The value is frame data, not document data: a viewport chooses it and the settings file remembers
/// the default, but a `.fcs` never carries it (ADR 0029, ADR 0070). The renderer turns each style
/// into pipelines; [`ShadingStyle`] only promises which passes are on, so a frame can be tested
/// without a GPU.
///
/// [ADR 0029]: ../docs/decisions/0029-view-state-belongs-to-the-session.md
/// [ADR 0070]: ../docs/decisions/0070-display-styles-are-frame-data.md
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadingStyle {
    /// Shaded faces with the deduplicated mesh edges drawn over them.
    #[default]
    ShadedWithEdges,
    /// Shaded faces with no edge pass.
    Shaded,
    /// The edge pass alone, with no faces.
    Wireframe,
    /// Edges drawn through the faces, so hidden geometry stays visible.
    ShadedWithHiddenEdges,
    /// Blended faces with no depth write, drawn back to front by object centre.
    Translucent,
}

impl ShadingStyle {
    /// Every style, in the order the viewport cluster draws its buttons.
    pub const ALL: [Self; 5] =
        [Self::ShadedWithEdges, Self::Shaded, Self::Wireframe, Self::ShadedWithHiddenEdges, Self::Translucent];

    /// Whether the style draws filled faces.
    pub const fn draws_faces(self) -> bool {
        !matches!(self, Self::Wireframe)
    }

    /// Whether the style draws the mesh edge pass.
    pub const fn draws_edges(self) -> bool {
        !matches!(self, Self::Shaded | Self::Translucent)
    }

    /// Whether the edge pass ignores the depth test, drawing edges through the faces.
    pub const fn edges_through_faces(self) -> bool {
        matches!(self, Self::ShadedWithHiddenEdges)
    }

    /// Whether the faces are blended and write no depth.
    pub const fn is_translucent(self) -> bool {
        matches!(self, Self::Translucent)
    }
}

/// A floor grid on the world XY plane.
///
/// The renderer derives line geometry from these values; the document knows nothing about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Whether the grid is drawn at all. A hidden grid costs no vertices.
    pub visible: bool,
    /// Distance between adjacent minor lines, in millimetres.
    pub spacing_mm: f64,
    /// Half-size of the square grid around the world origin, in millimetres.
    pub extent_mm: f64,
    /// Every `major_every`-th line is drawn with `major_color`.
    pub major_every: u32,
    /// Colour of ordinary lines, in gamma space.
    pub minor_color: [f32; 4],
    /// Colour of every major line, in gamma space.
    pub major_color: [f32; 4],
    /// Colour of the world X axis, in gamma space.
    pub axis_x_color: [f32; 4],
    /// Colour of the world Y axis, in gamma space.
    pub axis_y_color: [f32; 4],
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: true,
            spacing_mm: 100.0,
            extent_mm: 5000.0,
            major_every: 5,
            minor_color: [0.28, 0.30, 0.34, 1.0],
            major_color: [0.42, 0.45, 0.50, 1.0],
            axis_x_color: [0.72, 0.28, 0.28, 1.0],
            axis_y_color: [0.28, 0.62, 0.34, 1.0],
        }
    }
}

/// How close a bound must come to the floor to count as resting on it, in millimetres.
///
/// A panel placed at `z = 0` lands exactly on the plane, but a file edited elsewhere can leave a
/// sub-millimetre gap; the tolerance keeps that cabinet on the floor without giving a shelf mounted
/// ten millimetres up a shadow it should not have.
pub const FLOOR_TOLERANCE_MM: f64 = 0.5;

/// Fraction of an object's footprint half-extent the shadow ellipse covers.
///
/// Below one so the shadow stays inside the object's outline instead of spilling past it; the
/// falloff reaches zero at the ellipse rim, inside the footprint edge.
pub const SHADOW_FOOTPRINT_SCALE: f64 = 0.92;

/// Height of the shadow plane above the floor, in millimetres.
///
/// The grid is lifted to `0.02`, so a shadow at `0.01` sits under the lines and never z-fights with
/// the floor.
const SHADOW_GROUND_Z_MM: f64 = 0.01;

/// A soft contact shadow on the world XY plane under one object that rests on the floor.
///
/// The shadow is an axis-aligned ellipse derived from the object's world bounds. The renderer draws
/// it unlit with one transparent triangle pass and a radial falloff — no shadow map, no extra pass
/// and no texture — so a cabinet reads as standing on something instead of floating in the grid. It
/// is frame data, never document data (ADR 0071).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadow {
    /// The node whose footprint this shadow follows.
    pub id: Id,
    /// Centre of the ellipse on the ground plane, in world millimetres. `z` is the shadow plane.
    pub center: DVec3,
    /// Half-axes of the ellipse along world X and Y, in millimetres.
    pub radii: [f64; 2],
}

impl ContactShadow {
    /// Derives the shadow under a world-space bound, or `None` when the bound does not rest on the
    /// floor.
    ///
    /// A non-finite bound, a footprint with no area, or a lowest point further than
    /// [`FLOOR_TOLERANCE_MM`] from the floor casts nothing. The ellipse covers
    /// [`SHADOW_FOOTPRINT_SCALE`] of the footprint, so it always stays inside the object's outline.
    pub fn from_bounds(id: Id, min: DVec3, max: DVec3) -> Option<Self> {
        if !min.is_finite() || !max.is_finite() || min.z.abs() > FLOOR_TOLERANCE_MM {
            return None;
        }
        let radii = [(max.x - min.x) * 0.5 * SHADOW_FOOTPRINT_SCALE, (max.y - min.y) * 0.5 * SHADOW_FOOTPRINT_SCALE];
        if !radii[0].is_finite() || !radii[1].is_finite() || radii[0] <= 0.0 || radii[1] <= 0.0 {
            return None;
        }
        Some(Self { id, center: DVec3::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, SHADOW_GROUND_Z_MM), radii })
    }
}

/// The contact-shadow switch and strength, beside the grid in a frame.
///
/// A change is read at draw time like [`ShadingStyle`], so turning the pass off costs no upload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadows {
    /// Whether the pass draws at all.
    pub visible: bool,
    /// Peak opacity at the centre, from 0 to 1.
    pub strength: f32,
    /// Gamma-space RGB of the shadow, like [`Grid`] colours, because the pass is unlit.
    pub color: [f32; 3],
}

impl ContactShadows {
    /// Whether the pass draws anything: the switch is on and the strength is above zero.
    pub fn draws(&self) -> bool {
        self.visible && self.strength.is_finite() && self.strength > 0.0
    }
}

impl Default for ContactShadows {
    fn default() -> Self {
        Self { visible: true, strength: 0.4, color: [0.0, 0.0, 0.0] }
    }
}

/// The largest number of point lights one frame can shade.
///
/// A frame has one directional key light and this many point lights, which keeps the uniform small
/// and the fragment loop bounded. Further point lights are stored in the document but do not light
/// the viewport.
pub const MAX_POINT_LIGHTS: usize = 8;

/// A point light in world space.
///
/// The power is in watts, following Blender's point-light unit: a light of 1000 W at one metre
/// shades a white surface at about half intensity under the viewport's fixed exposure. The
/// renderer applies inverse-square falloff and no hard range cutoff ([ADR 0039]).
///
/// [ADR 0039]: ../docs/decisions/0039-point-lights-use-watts-and-inverse-square-falloff.md
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// World-space origin.
    pub position: DVec3,
    /// Linear RGB colour.
    pub color: [f32; 3],
    /// Radiant power in watts.
    pub power: f32,
}

/// The key light, point lights and ambient term used to shade solid drawables.
#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Unit direction from a surface towards the key light, in world space.
    pub key_direction: DVec3,
    /// Linear RGB intensity of the key light.
    pub key_color: [f32; 3],
    /// Fraction of the surface colour visible without direct light.
    pub ambient: f32,
    /// Point lights in tree order, at most [`MAX_POINT_LIGHTS`] of them when built from a scene.
    pub points: Vec<PointLight>,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            key_direction: DVec3::new(-0.35, 0.55, 0.75).try_normalize().unwrap_or(DVec3::Z),
            key_color: [1.0, 0.97, 0.92],
            ambient: 0.32,
            points: Vec::new(),
        }
    }
}

/// One shaded mesh in world space.
///
/// The mesh is shared so rebuilding a scene does not copy vertex data. `world_transform` is a
/// rigid transform composed from the assembly tree; the renderer applies it before uploading.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawable {
    /// The document object this mesh belongs to.
    pub id: Id,
    /// Local-space triangles.
    pub mesh: Arc<MeshData>,
    /// Boundary and crease edges of `mesh`, two vertex indices per edge.
    ///
    /// The list is derived beside the mesh and cached with it (ADR 0053), so the edge pass costs no
    /// search per frame. `None` means the drawable has no edge pass.
    pub edges: Option<Arc<[u32]>>,
    /// Every triangle edge of `mesh`, triangulation diagonals included.
    ///
    /// Only the optional *all surface edges* style reads it; the default edge pass uses [`Self::edges`].
    pub all_edges: Option<Arc<[u32]>>,
    /// Rigid local-to-world transform.
    pub world_transform: DMat4,
    /// Linear RGB colour and opacity.
    pub color: [f32; 4],
}

impl Drawable {
    /// Creates a drawable from a document id, a mesh, a rigid world transform and a colour.
    ///
    /// The drawable starts with no edge lists; [`Drawable::with_edges`] and
    /// [`Drawable::with_all_edges`] add them.
    pub fn new(id: Id, mesh: Arc<MeshData>, world_transform: DMat4, color: [f32; 4]) -> Self {
        Self { id, mesh, edges: None, all_edges: None, world_transform, color }
    }

    /// Attaches the boundary and crease edge list derived from `mesh`.
    pub fn with_edges(mut self, edges: Arc<[u32]>) -> Self {
        self.edges = Some(edges);
        self
    }

    /// Attaches the full triangulation edge list derived from `mesh`.
    pub fn with_all_edges(mut self, all_edges: Arc<[u32]>) -> Self {
        self.all_edges = Some(all_edges);
        self
    }
}

/// One unlit polyline in world space.
///
/// A path is drawn as a chain of line segments, not a solid: it has no faces and no shading. The
/// points are local-space and `world_transform` is the rigid transform of the node that owns the
/// path, exactly like [`Drawable`], so a drag preview moves the matrix instead of copying the
/// points. The colour is gamma-space, like [`Grid`] colours, because the line pipeline is unlit.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    /// The document object this chain belongs to.
    pub id: Id,
    /// Local-space points in travel order; at least two.
    pub points: Arc<[DVec3]>,
    /// Rigid local-to-world transform.
    pub world_transform: DMat4,
    /// Gamma-space RGB and opacity.
    pub color: [f32; 4],
}

impl Polyline {
    /// Creates a polyline from a document id, local points, a rigid world transform and a colour.
    pub fn new(id: Id, points: Arc<[DVec3]>, world_transform: DMat4, color: [f32; 4]) -> Self {
        Self { id, points, world_transform, color }
    }
}

/// Everything needed to draw and light one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    /// The active orbit camera.
    pub camera: Camera,
    /// How the drawables are shaded.
    pub shading: ShadingStyle,
    /// Whether the edge pass draws every triangle edge instead of only the form edges.
    ///
    /// A diagnostic view state beside [`SceneFrame::shading`], never document data.
    pub all_edges: bool,
    /// The floor grid.
    pub grid: Grid,
    /// The contact-shadow switch and strength.
    pub contact_shadows: ContactShadows,
    /// Soft shadows under the objects that rest on the floor.
    pub shadows: Vec<ContactShadow>,
    /// The key light.
    pub lighting: Lighting,
    /// The clear colour, in gamma space.
    pub background: [f32; 4],
    /// Shaded meshes in world space.
    pub drawables: Vec<Drawable>,
    /// Unlit line chains in world space.
    pub polylines: Vec<Polyline>,
}

impl Default for SceneFrame {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            shading: ShadingStyle::default(),
            all_edges: false,
            grid: Grid::default(),
            contact_shadows: ContactShadows::default(),
            shadows: Vec::new(),
            lighting: Lighting::default(),
            background: [0.10, 0.11, 0.13, 1.0],
            drawables: Vec::new(),
            polylines: Vec::new(),
        }
    }
}

impl SceneFrame {
    /// Adds a drawable to the frame.
    pub fn push(&mut self, drawable: Drawable) {
        self.drawables.push(drawable);
    }

    /// Adds an unlit line chain to the frame.
    pub fn push_polyline(&mut self, polyline: Polyline) {
        self.polylines.push(polyline);
    }

    /// Adds a contact shadow to the frame.
    pub fn push_contact_shadow(&mut self, shadow: ContactShadow) {
        self.shadows.push(shadow);
    }

    /// The frame's drawables.
    pub fn drawables(&self) -> &[Drawable] {
        &self.drawables
    }

    /// The frame's contact shadows.
    pub fn shadows(&self) -> &[ContactShadow] {
        &self.shadows
    }

    /// The frame's unlit line chains.
    pub fn polylines(&self) -> &[Polyline] {
        &self.polylines
    }

    /// The document id of the nearest drawable or line chain hit by `ray`.
    ///
    /// A drawable is hit when one of its triangles crosses the ray. A line chain has no thickness,
    /// so it is hit when the ray passes within [`LINE_PICK_RADIUS_POINTS`] points of a segment at
    /// that segment's depth; `viewport_height` is the viewport's height in points, and a
    /// non-positive or non-finite one accepts only an exact crossing. A path wins a tie with a
    /// solid it lies on, so a contour drawn on a face stays selectable.
    ///
    /// Malformed vertices, indices and world transforms are skipped. Picking returns a document
    /// id, never a mesh index or a pointer: the caller can select it or send it to a command.
    pub fn pick(&self, ray: Ray, viewport_height: f64) -> Option<Id> {
        let mut nearest: Option<(f64, Id)> = None;
        for drawable in &self.drawables {
            let Some(distance) = ray_hit_drawable(ray, drawable) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, drawable.id));
            }
        }
        for polyline in &self.polylines {
            let Some(distance) = ray_hit_polyline(ray, polyline, self.camera, viewport_height) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| distance <= best) {
                nearest = Some((distance, polyline.id));
            }
        }
        nearest.map(|(_, id)| id)
    }

    /// A copy of the frame with every selected drawable and line chain highlighted for picking
    /// feedback.
    ///
    /// The base frame keeps its material colours, so clearing the selection restores them without
    /// rebuilding meshes. Passing the panels below a selected container highlights the container as
    /// one object. A line chain takes [`SELECTION_LINE_COLOR`] explicitly: a tint is invisible on a
    /// one-pixel line, and the shell widens it as an overlay.
    pub fn with_selection_highlight(&self, selected: &[Id]) -> Self {
        let mut frame = self.clone();
        if selected.is_empty() {
            return frame;
        }
        for drawable in &mut frame.drawables {
            if selected.contains(&drawable.id) {
                tint_selected(&mut drawable.color);
            }
        }
        for polyline in &mut frame.polylines {
            if selected.contains(&polyline.id) {
                polyline.color = SELECTION_LINE_COLOR;
            }
        }
        frame
    }

    /// A copy of the frame with the given drawables and line chains shifted by a world-space
    /// translation.
    ///
    /// This is a drag preview: it moves the listed primitives without touching the document, so the
    /// renderer can follow the pointer before one command commits the result. A non-finite delta
    /// leaves the frame unchanged.
    pub fn with_world_translation(&self, ids: &[Id], delta: DVec3) -> Self {
        let mut frame = self.clone();
        if ids.is_empty() || !delta.is_finite() {
            return frame;
        }
        let translation = DMat4::from_translation(delta);
        for drawable in &mut frame.drawables {
            if ids.contains(&drawable.id) {
                drawable.world_transform = translation * drawable.world_transform;
            }
        }
        for polyline in &mut frame.polylines {
            if ids.contains(&polyline.id) {
                polyline.world_transform = translation * polyline.world_transform;
            }
        }
        for shadow in &mut frame.shadows {
            if ids.contains(&shadow.id) {
                // A shadow stays on the ground plane: only its footprint slides with the object.
                shadow.center.x += delta.x;
                shadow.center.y += delta.y;
            }
        }
        frame
    }

    /// A copy of the frame with the given drawables and line chains rotated about a world-space
    /// pivot.
    ///
    /// This is a drag preview: it turns the listed primitives without touching the document, so the
    /// renderer can follow the pointer before one command commits the result. The rotation must be a
    /// finite unit quaternion and the pivot finite; anything else leaves the frame unchanged.
    pub fn with_world_rotation(&self, ids: &[Id], pivot: DVec3, rotation: DQuat) -> Self {
        let mut frame = self.clone();
        if ids.is_empty() || !pivot.is_finite() || !rotation.is_finite() || (rotation.length() - 1.0).abs() > 1.0e-9 {
            return frame;
        }
        let about_pivot = DMat4::from_translation(pivot) * DMat4::from_quat(rotation) * DMat4::from_translation(-pivot);
        for drawable in &mut frame.drawables {
            if ids.contains(&drawable.id) {
                drawable.world_transform = about_pivot * drawable.world_transform;
            }
        }
        for polyline in &mut frame.polylines {
            if ids.contains(&polyline.id) {
                polyline.world_transform = about_pivot * polyline.world_transform;
            }
        }
        for shadow in &mut frame.shadows {
            if ids.contains(&shadow.id) {
                // The ellipse stays world-axis aligned, so only its centre follows the turn; the
                // footprint is a soft blob and does not need a second orientation.
                let turned = about_pivot.transform_point3(shadow.center);
                shadow.center.x = turned.x;
                shadow.center.y = turned.y;
            }
        }
        frame
    }

    /// A copy of the frame with one drawable's local mesh replaced.
    ///
    /// This is a drag preview for sub-object editing: the shell builds a mesh with the moved
    /// vertices and swaps it in for one frame, so the document and the renderer cache stay untouched
    /// until the command commits. An unknown id leaves the frame unchanged.
    pub fn with_drawable_mesh(&self, id: Id, mesh: Arc<MeshData>) -> Self {
        let mut frame = self.clone();
        for drawable in &mut frame.drawables {
            if drawable.id == id {
                drawable.mesh = mesh.clone();
            }
        }
        frame
    }
}

/// Linear RGB colour mixed into the selected drawable.
const SELECTION_TINT: [f64; 3] = [1.0, 0.55, 0.15];

/// Fraction of [`SELECTION_TINT`] mixed into the base colour.
const SELECTION_MIX: f64 = 0.35;

/// Mixes the selection tint into an RGB colour, leaving the alpha channel untouched.
fn tint_selected(color: &mut [f32; 4]) {
    for (channel, tint) in color.iter_mut().zip(SELECTION_TINT) {
        let base = f64::from(*channel);
        *channel = (base * (1.0 - SELECTION_MIX) + tint * SELECTION_MIX) as f32;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::f64::consts::{FRAC_PI_2, TAU};

    use furnicraft_geom::DVec4;
    use furnicraft_model::Id;

    fn assert_close(actual: DVec3, expected: DVec3) {
        assert!((actual - expected).length() < 1.0e-9, "actual {actual:?}, expected {expected:?}");
    }

    #[test]
    fn a_display_style_is_frame_data_with_a_default() {
        assert_eq!(SceneFrame::default().shading, ShadingStyle::ShadedWithEdges);
        assert_eq!(ShadingStyle::ALL.len(), 5, "the cluster offers every style the renderer interprets");
        assert!(ShadingStyle::Wireframe.draws_edges() && !ShadingStyle::Wireframe.draws_faces());
        assert!(!ShadingStyle::Translucent.draws_edges() && ShadingStyle::Translucent.is_translucent());
        assert!(!ShadingStyle::Shaded.draws_edges() && ShadingStyle::Shaded.draws_faces());
        assert!(ShadingStyle::ShadedWithHiddenEdges.edges_through_faces());
        assert!(!ShadingStyle::ShadedWithEdges.edges_through_faces());
    }

    #[test]
    fn a_contact_shadow_rests_on_the_floor_inside_the_footprint() {
        let id = Id::new(7).expect("id");
        let shadow = ContactShadow::from_bounds(id, DVec3::new(-100.0, -50.0, 0.0), DVec3::new(100.0, 50.0, 600.0))
            .expect("a bound on the floor casts a shadow");
        assert_eq!(shadow.id, id);
        assert!((shadow.center - DVec3::new(0.0, 0.0, SHADOW_GROUND_Z_MM)).length() < 1.0e-9);
        assert!((shadow.radii[0] - 92.0).abs() < 1.0e-9, "the ellipse is narrower than the footprint");
        assert!((shadow.radii[1] - 46.0).abs() < 1.0e-9);
        assert!(shadow.radii[0] < 100.0 && shadow.radii[1] < 50.0, "the ellipse stays inside the footprint");
        assert!(shadow.center.z < 0.02, "the shadow sits under the grid lines");
    }

    #[test]
    fn a_bound_without_floor_contact_casts_no_shadow() {
        let id = Id::new(8).expect("id");
        let floating = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 10.0), DVec3::new(600.0, 400.0, 700.0));
        assert!(floating.is_none(), "a shelf ten millimetres up floats, it does not rest");
        let within = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.4), DVec3::new(600.0, 400.0, 18.0));
        assert!(within.is_some(), "a sub-millimetre gap still reads as the floor");
        let flat = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 0.0));
        assert!(flat.is_none(), "a footprint with no area casts nothing");
        let hostile = ContactShadow::from_bounds(id, DVec3::new(f64::NAN, 0.0, 0.0), DVec3::new(1.0, 1.0, 1.0));
        assert!(hostile.is_none(), "a non-finite bound casts nothing");
    }

    #[test]
    fn the_contact_shadow_switch_removes_the_pass() {
        let mut frame = SceneFrame::default();
        assert!(frame.contact_shadows.draws(), "a frame draws the shadows by default");
        assert!(frame.shadows().is_empty());
        let id = Id::new(9).expect("id");
        let shadow = ContactShadow::from_bounds(id, DVec3::new(0.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 500.0));
        frame.push_contact_shadow(shadow.expect("shadow"));
        assert_eq!(frame.shadows().len(), 1);

        frame.contact_shadows.visible = false;
        assert!(!frame.contact_shadows.draws(), "the switch removes the pass from the frame");
        frame.contact_shadows.visible = true;
        frame.contact_shadows.strength = 0.0;
        assert!(!frame.contact_shadows.draws(), "a zero strength draws nothing either");
    }

    #[test]
    fn default_camera_orbits_at_a_finite_distance() {
        let camera = Camera::default();
        assert!(camera.eye().is_finite());
        assert!(((camera.eye() - camera.target()).length() - camera.distance()).abs() < 1.0e-9);
        assert!((camera.forward().length() - 1.0).abs() < 1.0e-12);
        assert!((camera.right().dot(camera.up())).abs() < 1.0e-12);
    }

    #[test]
    fn camera_rejects_hostile_constructor_input() {
        assert!(matches!(Camera::new(DVec3::new(f64::NAN, 0.0, 0.0), 1000.0), Err(CameraError::NonFiniteTarget)));
        assert!(matches!(Camera::new(DVec3::ZERO, f64::INFINITY), Err(CameraError::InvalidDistance { .. })));
        assert!(matches!(Camera::new(DVec3::ZERO, MIN_DISTANCE_MM * 0.5), Err(CameraError::InvalidDistance { .. })));
    }

    #[test]
    fn camera_looks_at_the_target() {
        let camera = Camera::new(DVec3::new(10.0, 20.0, 30.0), 1000.0).expect("valid camera");
        let eye = camera.eye();
        let view = camera.view_matrix();
        assert_close(view.transform_point3(eye), DVec3::ZERO);
        assert_close(view.transform_point3(camera.target()), DVec3::new(0.0, 0.0, -1000.0));
    }

    #[test]
    fn orbit_wraps_yaw_and_clamps_pitch() {
        let mut camera = Camera::default();
        camera.orbit(TAU + 0.25, 100.0);
        assert!((0.0..TAU).contains(&camera.yaw()));
        assert_eq!(camera.pitch(), MAX_PITCH);
        camera.orbit(0.0, -10_000.0);
        assert_eq!(camera.pitch(), MIN_PITCH);
        let yaw = camera.yaw();
        camera.orbit(f64::NAN, 0.1);
        assert_eq!(camera.yaw(), yaw);
    }

    #[test]
    fn zoom_clamps_and_ignores_invalid_factors() {
        let mut camera = Camera::default();
        camera.zoom(0.5);
        assert_eq!(camera.distance(), 600.0);
        camera.zoom(1.0e12);
        assert_eq!(camera.distance(), MAX_DISTANCE_MM);
        camera.zoom(1.0e-12);
        assert_eq!(camera.distance(), MIN_DISTANCE_MM);
        let distance = camera.distance();
        camera.zoom(f64::NAN);
        camera.zoom(0.0);
        assert_eq!(camera.distance(), distance);
    }

    #[test]
    fn pan_moves_the_target_with_the_pointer() {
        let mut camera = Camera::default();
        let before = camera.target();
        camera.pan_pixels(100.0, 0.0, 800.0);
        let moved = camera.target() - before;
        assert!(moved.length() > 0.0);
        assert!(moved.dot(camera.right()) < 0.0, "dragging right moves the target left");
        camera.pan_pixels(f64::INFINITY, 0.0, 800.0);
        assert!(camera.target().is_finite());
    }

    #[test]
    fn look_from_snaps_to_axis_views_and_keeps_the_target() {
        let default = Camera::default();
        let mut camera = Camera::default();
        camera.look_from(DVec3::X);
        let offset = camera.eye() - camera.target();
        assert!((offset.try_normalize().expect("eye offset") - DVec3::X).length() < 1.0e-9);
        assert!(camera.forward().dot(-DVec3::X) > 1.0 - 1.0e-9);
        assert_eq!(camera.target(), default.target());
        assert_eq!(camera.distance(), default.distance());

        camera.look_from(-DVec3::Y);
        let offset = camera.eye() - camera.target();
        assert!((offset.try_normalize().expect("eye offset") - -DVec3::Y).length() < 1.0e-9);

        camera.look_from(DVec3::Z);
        assert_eq!(camera.pitch(), MAX_PITCH);
        let yaw = camera.yaw();
        camera.look_from(DVec3::ZERO);
        assert_eq!(camera.yaw(), yaw);
        assert_eq!(camera.pitch(), MAX_PITCH);
        camera.look_from(DVec3::new(f64::NAN, 0.0, 0.0));
        assert_eq!(camera.yaw(), yaw);
        assert_eq!(camera.pitch(), MAX_PITCH);
    }

    #[test]
    fn a_pole_view_looks_straight_down_and_projects_its_target() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        assert_eq!(camera.pitch(), FRAC_PI_2);
        assert!((camera.forward() - -DVec3::Z).length() < 1.0e-12, "forward was {:?}", camera.forward());
        let up = camera.up();
        assert!(up.is_finite());
        assert!((up.length() - 1.0).abs() < 1.0e-12, "up was {up:?}");
        assert!(up.dot(camera.forward()).abs() < 1.0e-12, "up is perpendicular to forward");

        let aspect = 16.0 / 9.0;
        let centre = camera.ray_ndc(0.0, 0.0, aspect);
        assert!((centre.direction() - -DVec3::Z).length() < 1.0e-12, "the centre ray looks straight down");
        let ndc = camera.project_ndc(camera.target(), aspect).expect("the target is in front");
        assert!(ndc.length() < 1.0e-12, "the target projects to the viewport centre, got {ndc:?}");

        camera.look_from(-DVec3::Z);
        assert_eq!(camera.pitch(), MIN_PITCH);
        assert!((camera.forward() - DVec3::Z).length() < 1.0e-12, "forward was {:?}", camera.forward());
        assert!((camera.up().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn a_pole_view_takes_the_canonical_up() {
        // Start from a yaw the pole must not keep, so a camera that still inherits it fails here.
        let mut camera = Camera::default();
        camera.orbit(0.9, 0.0);
        let target = camera.target();
        let distance = camera.distance();

        camera.look_from_oriented(DVec3::Z, canonical_up(DVec3::Z));
        assert_eq!(camera.pitch(), MAX_PITCH);
        assert!(camera.up().dot(DVec3::Y) > 1.0 - 1.0e-9, "the top view is +Y up, got {:?}", camera.up());
        assert!(camera.right().dot(DVec3::X) > 1.0 - 1.0e-9, "and +X right, got {:?}", camera.right());
        assert_eq!(camera.target(), target, "the target does not move");
        assert_eq!(camera.distance(), distance, "nor the distance");

        camera.look_from_oriented(-DVec3::Z, canonical_up(-DVec3::Z));
        assert_eq!(camera.pitch(), MIN_PITCH);
        assert!(camera.up().dot(DVec3::Y) > 1.0 - 1.0e-9, "the bottom view is +Y up, got {:?}", camera.up());
        assert!(camera.right().dot(-DVec3::X) > 1.0 - 1.0e-9, "and -X right, got {:?}", camera.right());
        assert_eq!(camera.target(), target);
    }

    #[test]
    fn a_non_pole_direction_ignores_the_requested_up() {
        let mut camera = Camera::default();
        camera.look_from_oriented(DVec3::X, DVec3::Y);
        assert!(camera.up().dot(DVec3::Z) > 1.0 - 1.0e-9, "a side view keeps +Z up, got {:?}", camera.up());
        assert!(camera.right().dot(DVec3::Y) > 1.0 - 1.0e-9, "and turns with its yaw, got {:?}", camera.right());
    }

    #[test]
    fn a_pole_without_a_usable_up_keeps_the_yaw() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::X);
        let yaw = camera.yaw();

        // An up parallel to the view direction names no roll, so the pole keeps the yaw it had.
        camera.look_from_oriented(DVec3::Z, DVec3::Z);
        assert_eq!(camera.pitch(), MAX_PITCH);
        assert_eq!(camera.yaw(), yaw);

        // A zero direction is not a view at all and must not move the camera.
        camera.look_from_oriented(DVec3::ZERO, DVec3::Y);
        assert_eq!(camera.pitch(), MAX_PITCH);
        assert_eq!(camera.yaw(), yaw);
    }

    #[test]
    fn the_canonical_up_is_y_at_a_pole_and_z_elsewhere() {
        assert_eq!(canonical_up(DVec3::Z), DVec3::Y);
        assert_eq!(canonical_up(-DVec3::Z), DVec3::Y);
        assert_eq!(canonical_up(DVec3::X), DVec3::Z);
        assert_eq!(canonical_up(-DVec3::Y), DVec3::Z);
        assert_eq!(canonical_up(DVec3::new(0.3, 0.7, 0.5)), DVec3::Z);
    }

    #[test]
    fn orbiting_off_a_pole_moves_the_view() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        let before = camera.forward();
        camera.orbit(0.0, -0.1);
        assert!((camera.pitch() - (FRAC_PI_2 - 0.1)).abs() < 1.0e-12);
        assert!((camera.forward() - before).length() > 0.05, "an orbit leaves the pole");
        assert!((camera.up().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn a_roll_turns_the_basis_about_the_view_axis() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::new(0.6, 0.8, 0.3));
        let (eye, target, forward) = (camera.eye(), camera.target(), camera.forward());
        let (right, up) = (camera.right(), camera.up());

        camera.roll_to(FRAC_PI_2);
        assert!((camera.eye() - eye).length() < 1.0e-9, "the eye does not move");
        assert!((camera.target() - target).length() < 1.0e-9, "nor the target");
        assert!((camera.forward() - forward).length() < 1.0e-9, "nor the view direction");
        assert_close(camera.up(), right);
        assert_close(camera.right(), -up);
        assert!((camera.up() - camera.right().cross(camera.forward())).length() < 1.0e-9, "up stays right × forward");
        assert!((camera.right().length() - 1.0).abs() < 1.0e-9);
        assert!((camera.up().length() - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_roll_moves_a_world_point_from_screen_right_to_screen_up() {
        let mut camera = Camera::default();
        let aspect = 16.0 / 9.0;
        let point = camera.target() + camera.right() * 300.0;
        let before = camera.project_ndc(point, aspect).expect("the point is in front");
        assert!(before.x > 0.3 && before.y.abs() < 0.01, "the point starts on screen right: {before:?}");

        camera.roll_to(FRAC_PI_2);
        let after = camera.project_ndc(point, aspect).expect("the point is still in front");
        assert!(after.y > 0.3 && after.x.abs() < 0.01, "the image turned counter-clockwise: {after:?}");
    }

    #[test]
    fn a_ray_follows_the_rolled_basis() {
        let mut camera = Camera::default();
        camera.roll_to(FRAC_PI_2);
        let right_ray = camera.ray_ndc(1.0, 0.0, 1.0);
        assert!(right_ray.direction().dot(camera.right()) > 0.3, "the right edge follows the rolled right vector");
        assert!(right_ray.direction().dot(camera.up()).abs() < 1.0e-9, "and has no up component");
        let up_ray = camera.ray_ndc(0.0, 1.0, 1.0);
        assert!(up_ray.direction().dot(camera.up()) > 0.3, "the top edge follows the rolled up vector");
        assert!(up_ray.direction().dot(camera.right()).abs() < 1.0e-9, "and has no right component");
    }

    #[test]
    fn the_view_matrix_stays_orthonormal_under_a_roll() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::new(0.4, 0.7, 0.5));
        camera.roll_to(0.9);
        let columns = camera.view_matrix().to_cols_array();
        let right = DVec3::new(columns[0], columns[4], columns[8]);
        let up = DVec3::new(columns[1], columns[5], columns[9]);
        let back = DVec3::new(columns[2], columns[6], columns[10]);
        for pair in [(right, up), (right, back), (up, back)] {
            assert!(pair.0.dot(pair.1).abs() < 1.0e-9, "the basis is orthogonal");
            assert!((pair.0.length() - 1.0).abs() < 1.0e-9);
        }
        assert!((right.cross(up).dot(back) - 1.0).abs() < 1.0e-9, "the view basis is right-handed");
    }

    #[test]
    fn a_rolled_pole_view_stays_straight_down_and_rolls() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        let (eye, target, forward) = (camera.eye(), camera.target(), camera.forward());
        camera.roll_to(0.7);
        assert!((camera.eye() - eye).length() < 1.0e-9);
        assert!((camera.target() - target).length() < 1.0e-9);
        assert!((camera.forward() - forward).length() < 1.0e-9, "the pole still looks straight down");
        assert!((camera.up().length() - 1.0).abs() < 1.0e-9);
        assert!(camera.up().dot(camera.forward()).abs() < 1.0e-9);
        assert!((camera.up() - DVec3::Y).length() > 0.1, "the roll leaves the canonical up");
    }

    #[test]
    fn look_from_resets_the_roll_and_navigation_keeps_it() {
        let mut camera = Camera::default();
        camera.roll_to(0.8);
        camera.orbit(0.0, 0.0);
        assert!((camera.roll() - 0.8).abs() < 1.0e-12, "orbit keeps the roll");
        camera.pan_pixels(10.0, 0.0, 800.0);
        assert!((camera.roll() - 0.8).abs() < 1.0e-12, "pan keeps the roll");
        camera.zoom(1.1);
        assert!((camera.roll() - 0.8).abs() < 1.0e-12, "zoom keeps the roll");
        camera.toggle_projection();
        assert!((camera.roll() - 0.8).abs() < 1.0e-12, "the projection toggle keeps the roll");

        camera.look_from(DVec3::new(0.3, 0.4, 0.5));
        assert_eq!(camera.roll(), 0.0, "a standard direction resets the roll");
        camera.roll_to(0.9);
        camera.look_from_oriented(DVec3::Z, DVec3::Y);
        assert_eq!(camera.roll(), 0.0, "the oriented pole snap resets the roll too");
    }

    #[test]
    fn a_roll_wraps_and_ignores_hostile_input() {
        let mut camera = Camera::default();
        camera.roll_to(-FRAC_PI_2);
        assert!((camera.roll() - (TAU - FRAC_PI_2)).abs() < 1.0e-12, "a negative roll wraps into 0..TAU");
        camera.roll_by(TAU);
        assert!((camera.roll() - (TAU - FRAC_PI_2)).abs() < 1.0e-12, "a full turn changes nothing");
        camera.roll_by(0.1);
        assert!((camera.roll() - (TAU - FRAC_PI_2 + 0.1)).abs() < 1.0e-12);

        let roll = camera.roll();
        camera.roll_to(f64::NAN);
        camera.roll_to(f64::INFINITY);
        camera.roll_by(f64::NEG_INFINITY);
        assert_eq!(camera.roll(), roll, "non-finite input changes nothing");
    }

    #[test]
    fn a_pinned_camera_ignores_the_roll() {
        let mut rolled = Camera::default().looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, DVec3::Y, -DVec3::Z);
        let pinned = rolled;
        rolled.roll_to(FRAC_PI_2);
        rolled.roll_by(0.4);
        assert_eq!(rolled, pinned, "a pinned basis ignores the roll and keeps every field");
        assert_close(rolled.right(), DVec3::X);
        assert_close(rolled.up(), DVec3::Y);
    }

    #[test]
    fn panning_follows_the_rolled_axes() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::new(0.5, 0.5, 0.4));
        let unrolled_right = camera.right();
        camera.roll_to(FRAC_PI_2);
        let before = camera.target();
        camera.pan_pixels(100.0, 0.0, 800.0);
        let moved = camera.target() - before;
        assert!(moved.dot(camera.right()) < 0.0, "a rightward drag moves along the rolled right");
        assert!(moved.dot(unrolled_right).abs() < 1.0e-9, "and not along the unrolled right");
    }

    #[test]
    fn the_view_matrix_keeps_an_authored_up_vector() {
        let up = DVec3::new(0.0, 1.0, 0.005).try_normalize().expect("a finite up vector");
        let camera = Camera::default().looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, up, -DVec3::Z);
        assert_close(camera.up(), up);
        let columns = camera.view_matrix().to_cols_array();
        assert_close(DVec3::new(columns[1], columns[5], columns[9]), up);
    }

    #[test]
    fn projection_is_finite_at_both_depth_planes() {
        let camera = Camera::default();
        let projection = camera.projection_matrix(16.0 / 9.0);
        assert!(projection.is_finite());

        let near = projection * DVec4::new(0.0, 0.0, -camera.near, 1.0);
        let far = projection * DVec4::new(0.0, 0.0, -camera.far, 1.0);
        assert!((near.z / near.w - 0.0).abs() < 1.0e-9);
        assert!((far.z / far.w - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn toggling_the_projection_keeps_the_visible_height() {
        let mut camera = Camera::default();
        let perspective = camera;
        camera.toggle_projection();
        let Projection::Orthographic { height } = camera.projection() else {
            panic!("a perspective view becomes orthographic");
        };
        let expected = 2.0 * perspective.distance() * (DEFAULT_FOV_Y * 0.5).tan();
        assert!((height - expected).abs() < 1.0e-9, "the orthographic height is the frame at the target");
        assert_eq!(camera.target(), perspective.target());
        assert_eq!(camera.yaw(), perspective.yaw());
        assert_eq!(camera.pitch(), perspective.pitch());
        assert_eq!(camera.distance(), perspective.distance());

        camera.toggle_projection();
        assert!(matches!(camera.projection(), Projection::Perspective { .. }));
        assert!((camera.distance() - perspective.distance()).abs() < 1.0e-9, "the frame comes back");
        assert_eq!(camera.target(), perspective.target());
        assert_eq!(camera.yaw(), perspective.yaw());
        assert_eq!(camera.pitch(), perspective.pitch());
    }

    #[test]
    fn set_orthographic_keeps_the_visible_height_and_the_framing() {
        let mut camera = Camera::default();
        camera.orbit(0.3, 0.1);
        camera.zoom(1.4);
        let perspective = camera;
        camera.set_orthographic();
        let Projection::Orthographic { height } = camera.projection() else {
            panic!("a perspective view becomes orthographic");
        };
        let expected = 2.0 * perspective.distance() * (DEFAULT_FOV_Y * 0.5).tan();
        assert!((height - expected).abs() < 1.0e-9, "the height is the frame at the target");
        assert_eq!(camera.target(), perspective.target(), "the target stays");
        assert_eq!(camera.distance(), perspective.distance(), "the distance stays");
        assert_eq!(camera.yaw(), perspective.yaw());
        assert_eq!(camera.pitch(), perspective.pitch());

        // A second call is a no-op: a standard view that is already orthographic keeps its height.
        camera.zoom(0.5);
        let Projection::Orthographic { height: zoomed } = camera.projection() else {
            panic!("zooming keeps the projection");
        };
        camera.set_orthographic();
        let Projection::Orthographic { height: kept } = camera.projection() else {
            panic!("the view is still orthographic");
        };
        assert!((kept - zoomed).abs() < 1.0e-9, "an orthographic view keeps its visible height");

        // An authored camera is pinned to its document projection.
        let authored = Camera::default()
            .looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, DVec3::Y, -DVec3::Z)
            .with_projection(Projection::Perspective { fov_y: DEFAULT_FOV_Y });
        let mut pinned = authored;
        pinned.set_orthographic();
        assert_eq!(pinned, authored, "a pinned camera is not view state the viewport may change");
    }

    #[test]
    fn zooming_an_orthographic_view_scales_the_visible_height() {
        let mut camera = Camera::default();
        camera.toggle_projection();
        let Projection::Orthographic { height } = camera.projection() else {
            panic!("the view is orthographic");
        };
        camera.zoom(0.5);
        let Projection::Orthographic { height: zoomed } = camera.projection() else {
            panic!("zooming keeps the projection");
        };
        assert!((zoomed - height * 0.5).abs() < 1.0e-9, "zooming in halves the visible height");

        camera.toggle_projection();
        camera.toggle_projection();
        let Projection::Orthographic { height: returned } = camera.projection() else {
            panic!("the round trip returns an orthographic view");
        };
        assert!((returned - zoomed).abs() < 1.0e-9, "a toggle pair keeps the zoomed frame");
    }

    #[test]
    fn an_authored_camera_keeps_its_projection() {
        let authored = Camera::default()
            .looked_from(DVec3::new(0.0, 0.0, 100.0), DVec3::X, DVec3::Y, -DVec3::Z)
            .with_projection(Projection::Orthographic { height: 500.0 });
        let mut camera = authored;
        camera.toggle_projection();
        camera.zoom(4.0);
        assert_eq!(camera, authored, "a pinned camera is not view state the viewport may change");
    }

    #[test]
    fn an_orthographic_camera_projects_without_perspective() {
        let camera = Camera::default().with_projection(Projection::Orthographic { height: 100.0 });
        let aspect = 2.0;
        let eye = camera.eye();
        let point = eye + camera.forward() * 500.0 + camera.right() * 100.0;
        let ndc = camera.project_ndc(point, aspect).expect("a point in front of the camera");
        assert!((ndc.x - 1.0).abs() < 1.0e-9, "ndc was {ndc:?}");
        assert!(ndc.y.abs() < 1.0e-9);

        let centre = camera.ray_ndc(0.0, 0.0, aspect);
        let right = camera.ray_ndc(1.0, 0.0, aspect);
        assert_close(centre.direction(), camera.forward());
        assert_close(right.direction(), camera.forward());
        assert!((right.origin() - centre.origin()).dot(camera.right()) > 99.0, "rays are parallel and offset");

        assert!(camera.project_ndc(eye - camera.forward() * 10.0, aspect).is_none(), "behind is rejected");

        let projection = camera.projection_matrix(aspect);
        let near = projection * DVec4::new(0.0, 0.0, -camera.near, 1.0);
        let far = projection * DVec4::new(0.0, 0.0, -camera.far, 1.0);
        assert!((near.z / near.w - 0.0).abs() < 1.0e-9);
        assert!((far.z / far.w - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_fixed_view_keeps_its_basis_and_ignores_navigation() {
        let mut camera = Camera::default().looked_from(DVec3::new(1.0, 2.0, 3.0), DVec3::X, DVec3::Y, -DVec3::Z);
        assert_close(camera.eye(), DVec3::new(1.0, 2.0, 3.0));
        assert_close(camera.forward(), -DVec3::Z);
        assert_close(camera.right(), DVec3::X);
        assert_close(camera.up(), DVec3::Y);

        let before = camera;
        camera.orbit(1.0, 1.0);
        camera.zoom(0.5);
        camera.pan_pixels(10.0, 10.0, 800.0);
        camera.look_from(DVec3::X);
        assert_eq!(camera, before, "an authored view is not navigated by the viewport");

        let orbit = Camera::default();
        assert_eq!(orbit.looked_from(DVec3::NAN, DVec3::X, DVec3::Y, DVec3::Z), orbit);
        assert_eq!(orbit.looked_from(DVec3::ZERO, DVec3::X, DVec3::X, DVec3::Z), orbit);
    }

    #[test]
    fn scene_accepts_drawables() {
        let mut frame = SceneFrame::default();
        let mesh = Arc::new(MeshData::default());
        let id = Id::new(7).expect("id");
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.drawables().len(), 1);
        assert_eq!(frame.drawables()[0].id, id);
    }

    fn line_chain(id: Id) -> Polyline {
        Polyline::new(id, Arc::from(vec![DVec3::ZERO, DVec3::X]), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0])
    }

    #[test]
    fn scene_accepts_polylines() {
        let mut frame = SceneFrame::default();
        let id = Id::new(7).expect("id");
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)]);
        frame.push_polyline(Polyline::new(id, points.clone(), DMat4::IDENTITY, [1.0; 4]));

        assert_eq!(frame.polylines().len(), 1);
        assert_eq!(frame.polylines()[0].id, id);
        assert_eq!(frame.polylines()[0].points, points);
        assert!(frame.drawables().is_empty(), "a line chain is not a solid drawable");
    }

    #[test]
    fn a_world_translation_preview_moves_only_the_listed_polylines() {
        let moved = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push_polyline(line_chain(moved));
        frame.push_polyline(line_chain(other));

        let translated = frame.with_world_translation(&[moved], DVec3::new(1.0, 2.0, 3.0));
        let position = translated.polylines()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 2.0, 3.0)).length() < 1.0e-12);
        assert_eq!(translated.polylines()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.polylines()[0].world_transform, DMat4::IDENTITY, "the base frame keeps its transform");
    }

    #[test]
    fn a_world_rotation_preview_turns_only_the_listed_polylines() {
        let turned = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        let offset = DMat4::from_translation(DVec3::new(2.0, 0.0, 0.0));
        frame.push_polyline(Polyline::new(turned, Arc::from(vec![DVec3::ZERO, DVec3::X]), offset, [1.0; 4]));
        frame.push_polyline(line_chain(other));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let rotated = frame.with_world_rotation(&[turned], DVec3::new(1.0, 0.0, 0.0), quarter_turn);
        let position = rotated.polylines()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 1.0, 0.0)).length() < 1.0e-12, "position was {position:?}");
        assert_eq!(rotated.polylines()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.polylines()[0].world_transform, offset);
    }

    #[test]
    fn selection_highlight_gives_every_selected_polyline_the_explicit_colour() {
        let selected = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push_polyline(line_chain(selected));
        frame.push_polyline(line_chain(other));

        let highlighted = frame.with_selection_highlight(&[selected]);
        assert_eq!(highlighted.polylines()[0].color, SELECTION_LINE_COLOR);
        assert_ne!(SELECTION_LINE_COLOR, frame.polylines()[0].color, "picking the colour must be visible");
        assert_eq!(highlighted.polylines()[1].color, frame.polylines()[1].color);
        assert_eq!(frame.polylines()[0].color, [0.5, 0.5, 0.5, 1.0], "the base frame keeps its colour");
    }

    fn quad_mesh() -> Arc<MeshData> {
        Arc::new(MeshData {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
        })
    }

    #[test]
    fn the_centre_ray_looks_where_the_camera_does() {
        let camera = Camera::default();
        let ray = camera.ray_ndc(0.0, 0.0, 16.0 / 9.0);
        assert_close(ray.origin(), camera.eye());
        assert_close(ray.direction(), camera.forward());

        let invalid = camera.ray_ndc(f64::NAN, f64::INFINITY, -1.0);
        assert_close(invalid.direction(), camera.forward());
        assert!(invalid.origin().is_finite());
    }

    #[test]
    fn projection_and_ray_agree_inside_and_outside_the_viewport() {
        let camera = Camera::default();
        let aspect = 16.0 / 9.0;
        let point = camera.target() + camera.right() * 100.0 + camera.up() * 50.0;
        let ndc = camera.project_ndc(point, aspect).expect("point in front of the camera");
        assert!(ndc.x > 0.0 && ndc.y > 0.0, "ndc was {ndc:?}");
        let ray = camera.ray_ndc(ndc.x, ndc.y, aspect);
        let to_point = (point - ray.origin()).try_normalize().expect("direction");
        assert!(ray.direction().dot(to_point) > 1.0 - 1.0e-9);

        let centre = camera.project_ndc(camera.target(), aspect).expect("target");
        assert!(centre.length() < 1.0e-9, "centre was {centre:?}");

        let behind = camera.eye() + (camera.eye() - camera.target());
        assert!(camera.project_ndc(behind, aspect).is_none());
        assert!(camera.project_ndc(DVec3::new(f64::NAN, 0.0, 0.0), aspect).is_none());

        let inside = camera.ray_ndc(1.0, 0.0, aspect);
        let outside = camera.ray_ndc(2.0, 0.0, aspect);
        assert!(outside.direction().dot(camera.right()) > inside.direction().dot(camera.right()));
        assert!(camera.ray_ndc(1.0e300, 0.0, aspect).direction().is_finite());
    }

    #[test]
    fn ray_closest_point_on_an_axis_line_is_a_drag_parameter() {
        let ray = Ray::new(DVec3::new(5.0, 10.0, 0.0), -DVec3::Y).expect("ray");
        let parameter = ray.closest_parameter_on_line(DVec3::ZERO, DVec3::X).expect("line");
        assert!((parameter - 5.0).abs() < 1.0e-12, "parameter was {parameter}");

        let through_origin = Ray::new(DVec3::new(0.0, 1.0, 0.0), -DVec3::Y).expect("ray");
        assert!(through_origin.closest_parameter_on_line(DVec3::ZERO, DVec3::X).expect("line").abs() < 1.0e-12);

        let parallel = Ray::new(DVec3::ZERO, DVec3::X).expect("ray");
        assert!(parallel.closest_parameter_on_line(DVec3::ZERO, DVec3::X).is_none());
        assert!(ray.closest_parameter_on_line(DVec3::ZERO, DVec3::ZERO).is_none());
        assert!(ray.closest_parameter_on_line(DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X).is_none());
    }

    #[test]
    fn a_world_translation_preview_moves_only_the_listed_drawables() {
        let moved = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(moved, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let translated = frame.with_world_translation(&[moved], DVec3::new(1.0, 2.0, 3.0));
        let position = translated.drawables()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 2.0, 3.0)).length() < 1.0e-12);
        assert_eq!(translated.drawables()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.drawables()[0].world_transform, DMat4::IDENTITY);

        assert_eq!(frame.with_world_translation(&[], DVec3::X), frame);
        assert_eq!(frame.with_world_translation(&[moved], DVec3::new(f64::NAN, 0.0, 0.0)), frame);
    }

    #[test]
    fn a_world_rotation_preview_turns_only_the_listed_drawables_about_the_pivot() {
        let turned = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        let offset = DMat4::from_translation(DVec3::new(2.0, 0.0, 0.0));
        frame.push(Drawable::new(turned, quad_mesh(), offset, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let rotated = frame.with_world_rotation(&[turned], DVec3::new(1.0, 0.0, 0.0), quarter_turn);
        let position = rotated.drawables()[0].world_transform.transform_point3(DVec3::ZERO);
        assert!((position - DVec3::new(1.0, 1.0, 0.0)).length() < 1.0e-12, "position was {position:?}");
        assert_eq!(rotated.drawables()[1].world_transform, DMat4::IDENTITY);
        assert_eq!(frame.drawables()[0].world_transform, offset);
    }

    #[test]
    fn a_world_rotation_preview_rejects_empty_ids_and_bad_input() {
        let turned = Id::new(2).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(turned, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let quarter_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        assert_eq!(frame.with_world_rotation(&[], DVec3::ZERO, quarter_turn), frame);
        assert_eq!(frame.with_world_rotation(&[turned], DVec3::new(f64::NAN, 0.0, 0.0), quarter_turn), frame);
        assert_eq!(frame.with_world_rotation(&[turned], DVec3::ZERO, DQuat::from_xyzw(2.0, 0.0, 0.0, 0.0)), frame);
    }

    #[test]
    fn a_mesh_preview_replaces_only_the_named_drawable() {
        let edited = Id::new(2).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(edited, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));

        let mut replacement = (*quad_mesh()).clone();
        replacement.positions[0] = [9.0, 9.0, 9.0];
        let replacement = Arc::new(replacement);
        let preview = frame.with_drawable_mesh(edited, replacement.clone());

        assert_eq!(preview.drawables()[0].mesh, replacement);
        assert_eq!(preview.drawables()[1].mesh, quad_mesh());
        assert_eq!(frame.drawables()[0].mesh, quad_mesh(), "the base frame keeps its mesh");
        assert_eq!(frame.with_drawable_mesh(Id::new(99).expect("id"), replacement), frame);
    }

    #[test]
    fn picking_returns_the_nearest_document_id() {
        let near = Id::new(2).expect("id");
        let far = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(near, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push(Drawable::new(far, quad_mesh(), DMat4::from_translation(DVec3::new(0.0, 0.0, 1.0)), [1.0; 4]));

        let from_above = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_above, 600.0), Some(far));
        let from_below = Ray::new(DVec3::new(0.5, 0.5, -10.0), DVec3::Z).expect("ray");
        assert_eq!(frame.pick(from_below, 600.0), Some(near));

        let miss = Ray::new(DVec3::new(2.0, 2.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(miss, 600.0), None);
    }

    #[test]
    fn picking_follows_the_world_transform() {
        let id = Id::new(4).expect("id");
        let mut frame = SceneFrame::default();
        let transform = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        frame.push(Drawable::new(id, quad_mesh(), transform, [1.0; 4]));

        let hit = Ray::new(DVec3::new(10.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(hit, 600.0), Some(id));
        let original_position = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(original_position, 600.0), None);
    }

    #[test]
    fn malformed_meshes_are_skipped_instead_of_panicking() {
        let id = Id::new(5).expect("id");
        let mesh =
            Arc::new(MeshData { positions: vec![[0.0; 3]], normals: Vec::new(), indices: vec![0, 1, 2, 0, 0, 0] });
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(id, mesh, DMat4::IDENTITY, [1.0; 4]));
        let ray = Ray::new(DVec3::new(0.0, 0.0, 1.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(ray, 600.0), None);
    }

    #[test]
    fn world_per_point_follows_the_projection_and_the_depth() {
        let camera = Camera::new(DVec3::ZERO, 1000.0).expect("camera");
        let expected = 2.0 * 1000.0 * (DEFAULT_FOV_Y * 0.5).tan() / 1000.0;
        assert!((camera.world_per_point(1000.0, 1000.0) - expected).abs() < 1.0e-12);
        assert!(camera.world_per_point(2000.0, 1000.0) > camera.world_per_point(1000.0, 1000.0));

        let orthographic = camera.with_projection(Projection::Orthographic { height: 500.0 });
        assert_eq!(orthographic.world_per_point(10.0, 1000.0), 0.5);
        assert_eq!(orthographic.world_per_point(900.0, 1000.0), 0.5);

        assert_eq!(camera.world_per_point(f64::NAN, 1000.0), 0.0);
        assert_eq!(camera.world_per_point(10.0, 0.0), 0.0);
    }

    #[test]
    fn ray_segment_measures_the_closest_approach() {
        let ray = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        let (along, separation) = ray_segment(ray, DVec3::ZERO, DVec3::X).expect("crossing");
        assert!((along - 10.0).abs() < 1.0e-9);
        assert!(separation < 1.0e-9);

        let offset = DVec3::new(0.0, 0.25, 0.0);
        let (_, separation) = ray_segment(ray, offset, offset + DVec3::X).expect("near miss");
        assert!((separation - 0.25).abs() < 1.0e-9);

        let past_the_end = Ray::new(DVec3::new(2.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        let (_, separation) = ray_segment(past_the_end, DVec3::ZERO, DVec3::X).expect("clamped to the end");
        assert!((separation - 1.0).abs() < 1.0e-9);

        let (along, separation) = ray_segment(ray, DVec3::ZERO, DVec3::ZERO).expect("a point");
        assert!((along - 10.0).abs() < 1.0e-9);
        assert!((separation - 0.5).abs() < 1.0e-9, "the zero-length segment is its own start point");

        let behind = Ray::new(DVec3::new(0.5, 0.0, -10.0), -DVec3::Z).expect("ray");
        assert_eq!(ray_segment(behind, DVec3::ZERO, DVec3::X), None);
        assert_eq!(ray_segment(ray, DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X), None);
    }

    #[test]
    fn picking_a_path_returns_its_document_id_near_the_line() {
        let id = Id::new(6).expect("id");
        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        frame.push_polyline(Polyline::new(id, points, DMat4::IDENTITY, [1.0; 4]));

        // At ten millimetres deep and a 600-point viewport, six points are about 0.083 mm.
        let near = Ray::new(DVec3::new(0.5, 0.04, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(near, 600.0), Some(id));
        let far = Ray::new(DVec3::new(0.5, 0.4, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(far, 600.0), None);

        let exact = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(exact, 0.0), Some(id), "an exact crossing needs no tolerance");
        assert_eq!(frame.pick(near, 0.0), None, "a broken viewport height means no radius");

        let miss = Ray::new(DVec3::new(5.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(miss, 600.0), None, "past the end of the chain");
    }

    #[test]
    fn picking_follows_the_polyline_world_transform() {
        let id = Id::new(7).expect("id");
        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        let transform = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        frame.push_polyline(Polyline::new(id, points, transform, [1.0; 4]));

        let hit = Ray::new(DVec3::new(10.5, 0.01, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(hit, 600.0), Some(id));
        let original_position = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        assert_eq!(frame.pick(original_position, 600.0), None);
    }

    #[test]
    fn the_nearest_of_a_solid_and_a_path_wins_and_a_path_wins_a_tie() {
        let solid = Id::new(8).expect("id");
        let path = Id::new(9).expect("id");
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::new(0.0, 0.5, 0.0), DVec3::new(1.0, 0.5, 0.0)]);
        let above = Ray::new(DVec3::new(0.5, 0.5, 10.0), -DVec3::Z).expect("ray");

        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(solid, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        frame.push_polyline(Polyline::new(path, points.clone(), DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(above, 600.0), Some(path), "a path on a face stays selectable");

        frame = SceneFrame::default();
        frame.push(Drawable::new(solid, quad_mesh(), DMat4::IDENTITY, [1.0; 4]));
        let below = DMat4::from_translation(DVec3::new(0.0, 0.0, -1.0));
        frame.push_polyline(Polyline::new(path, points, below, [1.0; 4]));
        assert_eq!(frame.pick(above, 600.0), Some(solid), "an occluded path loses to the solid");
    }

    #[test]
    fn malformed_polylines_are_skipped_instead_of_panicking() {
        let id = Id::new(10).expect("id");
        let ray = Ray::new(DVec3::new(0.5, 0.0, 10.0), -DVec3::Z).expect("ray");
        let mut frame = SceneFrame::default();
        let broken: Arc<[DVec3]> = Arc::from(vec![DVec3::new(f64::NAN, 0.0, 0.0), DVec3::X]);
        frame.push_polyline(Polyline::new(id, broken, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None);

        let mut frame = SceneFrame::default();
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        let broken = DMat4::from_translation(DVec3::new(f64::INFINITY, 0.0, 0.0));
        frame.push_polyline(Polyline::new(id, points, broken, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None);

        let mut frame = SceneFrame::default();
        let single: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO]);
        frame.push_polyline(Polyline::new(id, single, DMat4::IDENTITY, [1.0; 4]));
        assert_eq!(frame.pick(ray, 600.0), None, "one point has no segment");
    }

    #[test]
    fn selection_highlight_tints_every_selected_drawable() {
        let selected = Id::new(2).expect("id");
        let also_selected = Id::new(4).expect("id");
        let other = Id::new(3).expect("id");
        let mut frame = SceneFrame::default();
        frame.push(Drawable::new(selected, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));
        frame.push(Drawable::new(other, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));
        frame.push(Drawable::new(also_selected, quad_mesh(), DMat4::IDENTITY, [0.5, 0.5, 0.5, 1.0]));

        let highlighted = frame.with_selection_highlight(&[selected, also_selected]);
        assert_ne!(highlighted.drawables()[0].color, frame.drawables()[0].color);
        assert_ne!(highlighted.drawables()[2].color, frame.drawables()[2].color);
        assert_eq!(highlighted.drawables()[1].color, frame.drawables()[1].color);
        assert_eq!(frame.drawables()[0].color, [0.5, 0.5, 0.5, 1.0]);

        assert_eq!(frame.with_selection_highlight(&[]), frame);
        let missing = Id::new(99).expect("id");
        assert_eq!(frame.with_selection_highlight(&[missing]), frame);
    }
}
