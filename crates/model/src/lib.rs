//! The FurniCraft document: the container tree, panels, machining, banding, materials and the scene
//! objects the user authors.
//!
//! Pure serializable data plus its invariants. No geometry kernel, no GPU, no UI, no file I/O. The
//! crate root is a facade: each module owns one family of values or rules and its public items are
//! re-exported here, so callers keep writing `furnicraft_model::Type`.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::HashSet;

use furnicraft_geom::DVec3;
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod camera;
mod catalog;
mod container;
mod contour;
mod contour_edit;
#[cfg(test)]
mod fixtures;
mod furntype;
mod ids;
mod light;
mod mesh;
mod node;
mod object;
mod panel;
mod panel_features;
mod placement;
mod properties;
mod query;
mod tree;

pub use camera::{CameraError, CameraNode, CameraProjection};
pub use catalog::{Catalog, CatalogError, EdgeBandingMaterial, MaterialId, PanelMaterial};
pub use container::Container;
pub use contour::{
    Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, MAX_CONTOUR_COORDINATE,
    MAX_CONTOUR_SEGMENTS,
};
use contour::{MAX_SELF_INTERSECTION_PAIRS, SELF_INTERSECTION_TOLERANCE_MICROMETRES, SegBounds, segments_touch};
pub use contour_edit::ContourEditError;
pub use furntype::{FURNTYPE_MAX_CODE, FurnType, FurnTypeError};
pub use ids::{ContourSegmentId, EdgeId, Id, Revision, VertexId};
pub use light::{LightError, LightKind, LightNode};
use light::{valid_light_color, valid_light_intensity};
pub use mesh::{MAX_FACE_VERTICES, MAX_MESH_FACES, MAX_MESH_VERTICES, Mesh, MeshError};
pub use node::Node;
pub use object::{Constructive, ConstructiveError, Extrude, Feature, Geometry, MAX_OBJECT_FEATURES, Object};
pub use panel::{
    BandingSpec, ContourPath, CoreViolation, CornerBanding, Cutout, CutoutDepth, CutoutShape, EdgeBanding, EntryFace,
    FaceSide, Machining, Panel, PanelContourError, PanelDimensionError, PanelError, PanelGeometry, PanelGeometryError,
    PanelNominalSize, PanelOutline, PanelRect, PanelRectError, RectCorner, RectEdge, RingBanding, SegmentSource,
};
use panel::{core_violation_of, rects_overlap, validate_ring_banding};
pub use panel_features::{CornerFeature, CornerFeatureKind, EdgeBow};
pub use placement::{EulerAngles, Pivot, Placement, PlacementError};
use properties::validate_property_value;
pub use properties::{
    Properties, PropertyChange, PropertyError, PropertyValue, SYSTEM_COMPONENT, SYSTEM_FURNTYPE, SYSTEM_PROTOTYPE_ID,
    SYSTEM_PROTOTYPE_PARAMETERS, SYSTEM_PROTOTYPE_VERSION,
};
pub use query::{NodeQuery, PropertyPredicate, StructuralKind};
use tree::{
    assign_new_ids, count_nodes, default_container_pivot, find_container, find_container_mut, find_node, find_node_mut,
    find_panel_mut, find_panel_with_depth, find_parent, node_height, object_bounds_mm, panel_bounds, remove_child,
};

/// The root of a saved document: its schema version, revision, id allocation and node tree.
///
/// The material catalogue, dimensions and settings arrive with later M1 steps. Loading validates
/// the schema version and the tree invariants; an unsupported version or a duplicate id is a
/// readable error, never a reinterpretation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProjectWire")]
pub struct Project {
    schema_version: u32,
    revision: Revision,
    next_id: Id,
    root: Container,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_camera: Option<Id>,
}

impl Project {
    /// The schema version written by this build.
    ///
    /// Version 3 moved panel-owned machining from core-local to nominal panel-local coordinates.
    /// Version 5 split assembly components from organisational groups. Version 6 stores the
    /// manipulation pivot on every node. Version 7 replaces the two container kinds with one
    /// `Container` that carries typed properties: an old assembly becomes a component container
    /// and an old group an ordinary one. Version 8 adds the geometry object node with a baked mesh;
    /// older files simply contain none. Version 9 adds authored camera and light nodes and the
    /// document's active camera; older files simply contain none. Version 10 adds the constructive
    /// geometry payload, a contour with a feature history whose mesh is derived; a version 9 object
    /// still loads as a baked mesh. Version 11 stores the contour as directed line and arc segments;
    /// a version 10 point list loads as a ring of lines, so the shape is unchanged. Version 12 gives
    /// every contour segment a stable id and the ring a next-segment counter; a version 11 ring loads
    /// with ids `1..n`.
    pub const CURRENT_SCHEMA_VERSION: u32 = 14;

    /// Maximum nesting depth of containers, including the root at depth one.
    pub const MAX_ASSEMBLY_DEPTH: u32 = 64;

    /// Creates a project from a root container and validates its tree.
    ///
    /// The root defines the world frame, so its pivot is forced to zero even if the supplied
    /// container was built with children.
    pub fn new(root: Container) -> Result<Self, ProjectError> {
        let max_id = scan_tree(&root)?;
        let next_id = next_id_after(max_id)?;
        let mut root = root;
        root.set_pivot(Pivot::ZERO);
        Ok(Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            revision: Revision::ZERO,
            next_id,
            root,
            active_camera: None,
        })
    }

    /// The schema version stored in the document.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// The current document revision.
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// The next id that [`Project::add_panel`] or [`Project::add_component`] will allocate.
    pub const fn next_id(&self) -> Id {
        self.next_id
    }

    /// The root container.
    ///
    /// The root defines the world frame and is never a furniture component. It cannot be deleted,
    /// moved, grouped, ungrouped or dissolved.
    pub const fn root(&self) -> &Container {
        &self.root
    }

    /// Finds a container anywhere in the tree, including the root.
    pub fn container(&self, id: Id) -> Option<&Container> {
        find_container(&self.root, id, 1).map(|(_, container)| container)
    }

    /// Whether the id names a container or a panel.
    pub fn is_container(&self, id: Id) -> bool {
        self.container(id).is_some()
    }

    /// Whether the id names a container marked as a furniture component.
    pub fn is_component(&self, id: Id) -> bool {
        id != self.root.id() && self.container(id).is_some_and(Container::is_component)
    }

    /// Finds a panel anywhere in the tree.
    pub fn panel(&self, id: Id) -> Option<&Panel> {
        find_panel_with_depth(&self.root, id, 1).map(|(_, panel)| panel)
    }

    /// Finds a geometry object anywhere in the tree.
    pub fn object(&self, id: Id) -> Option<&Object> {
        self.node(id)?.as_object()
    }

    /// Finds an authored camera anywhere in the tree.
    pub fn camera(&self, id: Id) -> Option<&CameraNode> {
        self.node(id)?.as_camera()
    }

    /// Finds an authored light anywhere in the tree.
    pub fn light(&self, id: Id) -> Option<&LightNode> {
        self.node(id)?.as_light()
    }

    /// The camera the scene is rendered from, or `None` for the built-in camera.
    pub const fn active_camera(&self) -> Option<Id> {
        self.active_camera
    }

    /// Names the camera the scene is rendered from, or clears it, and bumps the revision once.
    ///
    /// `Some(id)` must name an existing camera node. `None` returns to the built-in camera. The
    /// document keeps no viewport state here: an active camera only says which authored camera
    /// frames the scene ([ADR 0036]).
    ///
    /// [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md
    pub fn set_active_camera(&mut self, camera: Option<Id>) -> Result<Revision, ProjectError> {
        if let Some(id) = camera
            && self.camera(id).is_none()
        {
            return Err(ProjectError::CameraNotFound { id });
        }
        let next_revision = self.next_revision()?;
        self.active_camera = camera;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Finds a panel or a nested container below the root.
    ///
    /// The root container is not a [`Node`]; use [`Project::container`] with the root id to reach
    /// it.
    pub fn node(&self, id: Id) -> Option<&Node> {
        find_node(&self.root, id)
    }

    /// The id of the container that directly owns `id`, or `None` for the root and unknown ids.
    pub fn parent_of(&self, id: Id) -> Option<Id> {
        find_parent(&self.root, id)
    }

    /// The given nodes that have no ancestor in the same list, in their original order.
    ///
    /// Unknown ids are skipped and a repeated id is kept once. The root counts as a top-most node;
    /// callers that cannot move it filter it out themselves. A multi-selection transform uses this
    /// so a moved container is not moved again through one of its descendants.
    pub fn top_most_nodes(&self, ids: &[Id]) -> Vec<Id> {
        let selected: HashSet<Id> = ids.iter().copied().filter(|id| self.contains_node(*id)).collect();
        let mut kept = HashSet::new();
        let mut top_most = Vec::new();
        for id in ids {
            if !selected.contains(id) || !kept.insert(*id) {
                continue;
            }
            let mut ancestor = self.parent_of(*id);
            let covered = loop {
                let Some(current) = ancestor else {
                    break false;
                };
                if selected.contains(&current) {
                    break true;
                }
                ancestor = self.parent_of(current);
            };
            if !covered {
                top_most.push(*id);
            }
        }
        top_most
    }

    /// Whether the id names the root or any node below it.
    ///
    /// Every node kind counts: a container, a panel, an object, a camera or a light. The root is
    /// included because it owns the document frame.
    pub fn contains_node(&self, id: Id) -> bool {
        id == self.root.id() || self.node(id).is_some()
    }

    /// The world placement of a node: every placement from the root down to it, composed.
    ///
    /// The root contributes its own placement, which is normally the identity. Returns `None` for
    /// an unknown id or a tree deeper than [`Project::MAX_ASSEMBLY_DEPTH`].
    pub fn world_placement(&self, id: Id) -> Option<Placement> {
        fn walk(node: &Node, id: Id, parent: Placement, depth: u32) -> Option<Placement> {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return None;
            }
            let world = Placement::compose(parent, node.placement()).ok()?;
            if node.id() == id {
                return Some(world);
            }
            let children = node.children()?;
            for child in children {
                if let Some(found) = walk(child, id, world, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            None
        }

        let root_world = self.root.placement();
        if self.root.id() == id {
            return Some(root_world);
        }
        for child in self.root.children() {
            if let Some(found) = walk(child, id, root_world, 1) {
                return Some(found);
            }
        }
        None
    }

    /// The ids of the nodes that produce geometry at or below `id`, in tree order.
    ///
    /// A panel and an object both produce a drawable, so each yields itself; a container yields
    /// every geometry node in its subtree. Cameras and lights carry no geometry and are skipped.
    /// Returns `None` for an unknown id; the list is empty for a container with no geometry.
    pub fn geometry_ids_under(&self, id: Id) -> Option<Vec<Id>> {
        fn collect(children: &[Node], ids: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                match child {
                    Node::Panel(panel) => ids.push(panel.id()),
                    Node::Object(object) => ids.push(object.id()),
                    Node::Container(container) => collect(container.children(), ids, depth.saturating_add(1)),
                    Node::Camera(_) | Node::Light(_) => {}
                }
            }
        }

        if self.panel(id).is_some() || self.object(id).is_some() {
            return Some(vec![id]);
        }
        let container = self.container(id)?;
        let mut ids = Vec::new();
        collect(container.children(), &mut ids, 0);
        Some(ids)
    }

    /// The stored manipulation pivot of a node, in its own frame.
    ///
    /// A panel keeps its local zero, the minimum corner of its nominal rectangle, until the user
    /// moves the pivot; a container created with children starts at the centre of their local
    /// bounding box and an empty container starts at its own origin. The root is not selectable and
    /// always has a zero pivot. Returns `None` for an unknown id.
    pub fn pivot(&self, id: Id) -> Option<Pivot> {
        if id == self.root.id() {
            return Some(self.root.pivot());
        }
        match self.node(id)? {
            Node::Panel(panel) => Some(panel.pivot()),
            Node::Container(container) => Some(container.pivot()),
            Node::Object(object) => Some(object.pivot()),
            Node::Camera(camera) => Some(camera.pivot()),
            Node::Light(light) => Some(light.pivot()),
        }
    }

    /// The manipulation pivot in the node's own frame, as millimetres for gizmos.
    pub fn pivot_local(&self, id: Id) -> Option<DVec3> {
        Some(self.pivot(id)?.to_local())
    }

    /// The manipulation pivot in world space: [`Self::pivot_local`] through the node's world placement.
    ///
    /// Both the translate and the rotate gizmo draw and rotate around this point. Returns `None` for
    /// an unknown id.
    pub fn pivot_world(&self, id: Id) -> Option<DVec3> {
        let local = self.pivot_local(id)?;
        Some(self.world_placement(id)?.transform_point(local))
    }

    /// The centre of the node's local axis-aligned bounding box, in its own frame.
    ///
    /// A panel's box is its nominal rectangle times its last known thickness. A container's box is
    /// the union of the nominal panel boxes below it, each transformed by its placement relative to
    /// the container. A container with no panels has no bounds and returns `None`.
    pub fn bounds_center_local(&self, id: Id) -> Option<DVec3> {
        if let Some(panel) = self.panel(id) {
            let size = panel.nominal_size();
            let half = DVec3::new(
                size.length().millimetres_f64(),
                size.width().millimetres_f64(),
                panel.last_known_thickness().millimetres_f64(),
            ) * 0.5;
            return Some(half);
        }
        if let Some(object) = self.object(id) {
            let (min, max) = object_bounds_mm(object.geometry())?;
            return Some((min + max) * 0.5);
        }
        let (_, container) = find_container(&self.root, id, 0)?;
        let (min, max) = panel_bounds(container.children(), Placement::IDENTITY, 0)?;
        Some((min + max) * 0.5)
    }

    /// Moves a node's pivot within its own frame and bumps the revision once.
    ///
    /// The pivot is authored data: changing it never moves the node, and changing the geometry never
    /// moves it. The root defines the world frame and has no pivot to edit.
    pub fn set_node_pivot(&mut self, id: Id, pivot: Pivot) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        match node {
            Node::Panel(panel) => panel.set_pivot(pivot),
            Node::Container(container) => container.set_pivot(pivot),
            Node::Object(object) => object.set_pivot(pivot),
            Node::Camera(camera) => camera.set_pivot(pivot),
            Node::Light(light) => light.set_pivot(pivot),
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Sets a node's pivot to the centre of its local bounds and bumps the revision once.
    ///
    /// A container with no panels has no bounds and is centred at its own origin. Returns
    /// [`ProjectError::RootImmutable`] for the root.
    pub fn center_node_pivot(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let center = self.bounds_center_local(id).unwrap_or(DVec3::ZERO);
        let pivot = Pivot::from_local(center).unwrap_or(Pivot::ZERO);
        self.set_node_pivot(id, pivot)
    }

    /// Checks that `nodes` can be wrapped in one new group.
    ///
    /// Grouping needs at least two distinct non-root nodes that share one parent container, and the
    /// new level must stay inside [`Project::MAX_ASSEMBLY_DEPTH`]; the name predates unified
    /// containers and is kept as a stable public name.
    pub fn validate_group(&self, nodes: &[Id]) -> Result<(), ProjectError> {
        self.group_parent(nodes).map(|_| ())
    }

    fn group_parent(&self, nodes: &[Id]) -> Result<(Id, u32), ProjectError> {
        if nodes.len() < 2 {
            return Err(ProjectError::TooFewNodes { minimum: 2, found: nodes.len() });
        }
        let mut seen = HashSet::new();
        let mut parent = None;
        for id in nodes {
            let id = *id;
            if !seen.insert(id.get()) {
                return Err(ProjectError::DuplicateNode { id });
            }
            if id == self.root.id() {
                return Err(ProjectError::RootImmutable { id });
            }
            if self.node(id).is_none() {
                return Err(ProjectError::NodeNotFound { id });
            }
            let node_parent = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
            match parent {
                Some(expected) if expected != node_parent => return Err(ProjectError::NodesNotSiblings),
                Some(_) => {}
                None => parent = Some(node_parent),
            }
        }
        let parent_id = parent.ok_or(ProjectError::TooFewNodes { minimum: 2, found: nodes.len() })?;
        let (parent_depth, _) =
            find_container(&self.root, parent_id, 1).ok_or(ProjectError::ContainerNotFound { id: parent_id })?;
        for id in nodes {
            let node = self.node(*id).ok_or(ProjectError::NodeNotFound { id: *id })?;
            let deepest = parent_depth
                .checked_add(node_height(node))
                .ok_or(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH })?;
            if deepest > Project::MAX_ASSEMBLY_DEPTH {
                return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
            }
        }
        Ok((parent_id, parent_depth))
    }

    /// Checks whether a panel can take a new nominal size without invalidating its machining.
    pub fn validate_panel_resize(&self, id: Id, nominal_size: PanelNominalSize) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let geometry = panel.geometry_for(nominal_size, panel.last_known_thickness())?;
        validate_panel_machining(panel, geometry)
    }

    /// Replaces a panel's nominal size and bumps the revision once.
    ///
    /// Existing machining is not moved or scaled. A cutout that would fall outside the new core or
    /// a blind cutout that would become deeper than the new thickness rejects the resize. The
    /// material, fallback thickness and edge banding are preserved.
    pub fn set_panel_nominal_size(&mut self, id: Id, nominal_size: PanelNominalSize) -> Result<Revision, ProjectError> {
        self.validate_panel_resize(id, nominal_size)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_nominal_size(nominal_size);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a ring can replace a contour panel's outline.
    ///
    /// The ring is first moved to the panel-local zero corner, exactly as
    /// [`Panel::anchored_contour`] does for a new panel, and its bounds must form a usable panel
    /// size. Every ring banding record must still name a segment the ring has: an edit that drops a
    /// banded segment is refused instead of silently dropping the band.
    pub fn validate_panel_outline(&self, id: Id, contour: &Contour) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if !panel.is_contour() {
            return Err(ProjectError::PanelNotContour { id });
        }
        let anchored = Panel::anchored_contour(contour)?;
        Panel::contour_size(&anchored)?;
        validate_ring_banding(&anchored, panel.ring_banding())?;
        Ok(())
    }

    /// Replaces a contour panel's outline and bumps the revision once.
    ///
    /// The new ring is anchored at the panel-local zero corner (ADR 0009). When the edit moves the
    /// minimum corner, the panel placement, pivot and machining shift by the same offset so the
    /// world-space shape and every feature stay where the user drew them. A rectangle panel never
    /// converts; [`Project::validate_panel_outline`] rejects it before anything changes.
    pub fn set_panel_outline(&mut self, id: Id, contour: Contour) -> Result<Revision, ProjectError> {
        self.validate_panel_outline(id, &contour)?;
        let (min, _) = contour.bounds_micrometres();
        let anchored = Panel::anchored_contour(&contour)?;
        let nominal_size = Panel::contour_size(&anchored)?;
        let next_revision = self.next_revision()?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.move_frame_to(min)?;
        panel.set_nominal_size(nominal_size);
        panel.set_outline(PanelOutline::Contour { contour: anchored });
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Renames a panel and bumps the revision once.
    ///
    /// The name is user-visible text; callers validate that it is not blank before calling this
    /// method. The panel id and every other property stay unchanged.
    pub fn rename_panel(&mut self, id: Id, name: impl Into<String>) -> Result<Revision, ProjectError> {
        if self.panel(id).is_none() {
            return Err(ProjectError::PanelNotFound { id });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_name(name);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Sets the panel material and refreshes its fallback thickness.
    ///
    /// A material or catalogue change may invalidate machining; that is a diagnostic, not a
    /// command error. Only a non-positive resulting core rejects the change.
    pub fn set_panel_material(
        &mut self,
        id: Id,
        material: MaterialId,
        thickness: Length,
    ) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        panel.geometry_for(panel.nominal_size(), thickness)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_material(Some(material));
        panel.set_thickness(thickness);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a banding can be placed without making the core non-positive.
    pub fn validate_edge_banding(&self, id: Id, band: EdgeBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.apply_edge_banding(band)?;
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces banding on one edge segment and moves it to the end of the order.
    pub fn set_edge_banding(&mut self, id: Id, band: EdgeBanding) -> Result<Revision, ProjectError> {
        self.validate_edge_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let edge = band.edge();
        panel.apply_edge_banding(band)?;
        panel.add_band_source(SegmentSource::Edge(edge));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from one edge segment and bumps the revision once.
    pub fn clear_edge_banding(&mut self, id: Id, edge: EdgeId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if !panel.edge_banding().iter().any(|band| band.edge() == edge) {
            return Err(ProjectError::EdgeBandingNotFound { panel: id, edge });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.clear_edge_banding_path(edge)?;
        panel.remove_band_source(SegmentSource::Edge(edge));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Validates a corner-banding record without changing the document.
    ///
    /// A banding belongs to a segment, so the corner must currently have a chamfer or radius.
    pub fn validate_corner_banding(&self, id: Id, band: CornerBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let corner = band.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(ProjectError::UnsupportedCornerBanding { panel: id, corner });
        }
        if panel.corner_feature(corner).is_none() {
            return Err(ProjectError::CornerFeatureNotFound { panel: id, corner });
        }
        let mut candidate = panel.clone();
        candidate.upsert_corner_banding(band);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces corner banding and moves it to the end of the order.
    pub fn set_corner_banding(&mut self, id: Id, band: CornerBanding) -> Result<Revision, ProjectError> {
        self.validate_corner_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let corner = band.corner();
        panel.upsert_corner_banding(band);
        panel.add_band_source(SegmentSource::Corner(corner));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from a corner segment and bumps the revision once.
    pub fn clear_corner_banding(&mut self, id: Id, corner: VertexId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.corner_banding_at(corner).is_none() {
            return Err(ProjectError::CornerBandingNotFound { panel: id, corner });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_corner_banding(corner);
        panel.remove_band_source(SegmentSource::Corner(corner));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a corner treatment can be applied without invalidating the panel.
    ///
    /// Banding does not block a geometry change: edge records stay on their edges and a corner
    /// record stays while its corner segment exists.
    pub fn validate_corner_feature(&self, id: Id, feature: CornerFeature) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.upsert_corner_feature(feature);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces a corner treatment and bumps the revision once.
    pub fn set_corner_feature(&mut self, id: Id, feature: CornerFeature) -> Result<Revision, ProjectError> {
        self.validate_corner_feature(id, feature)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.upsert_corner_feature(feature);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a corner treatment and bumps the revision once.
    ///
    /// The corner banding belongs to the corner segment, so it is removed together with the
    /// treatment; adjacent edge banding is untouched.
    pub fn clear_corner_feature(&mut self, id: Id, corner: VertexId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.corner_feature(corner).is_none() {
            return Err(ProjectError::CornerFeatureNotFound { panel: id, corner });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_corner_banding(corner);
        panel.remove_band_source(SegmentSource::Corner(corner));
        panel.remove_corner_feature(corner);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a bowed edge can be applied without invalidating the panel.
    ///
    /// Banding does not block a geometry change; existing edge-banding records stay on their edges.
    pub fn validate_edge_bow(&self, id: Id, bow: EdgeBow) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.upsert_edge_bow(bow);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces a bowed edge and bumps the revision once.
    pub fn set_edge_bow(&mut self, id: Id, bow: EdgeBow) -> Result<Revision, ProjectError> {
        self.validate_edge_bow(id, bow)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.upsert_edge_bow(bow);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a bowed edge and bumps the revision once.
    pub fn clear_edge_bow(&mut self, id: Id, edge: EdgeId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.edge_bow(edge).is_none() {
            return Err(ProjectError::EdgeBowNotFound { panel: id, edge });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_edge_bow(edge);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether ring banding can be set without invalidating the panel.
    pub fn validate_ring_banding(&self, id: Id, band: RingBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.apply_ring_banding(band)?;
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces banding on one ring segment and moves it to the end of the order.
    pub fn set_ring_banding(&mut self, id: Id, band: RingBanding) -> Result<Revision, ProjectError> {
        self.validate_ring_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let segment = band.segment();
        panel.apply_ring_banding(band)?;
        panel.add_band_source(SegmentSource::Ring(segment));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from one ring segment and bumps the revision once.
    pub fn clear_ring_banding(&mut self, id: Id, segment: ContourSegmentId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.ring_banding_at(segment).is_none() {
            return Err(ProjectError::RingBandingNotFound { panel: id, segment });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_ring_banding(segment);
        panel.remove_band_source(SegmentSource::Ring(segment));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a cutout can be added to a panel without changing the document.
    pub fn validate_cutout(&self, panel: Id, shape: &CutoutShape, depth: CutoutDepth) -> Result<(), ProjectError> {
        let panel = self.panel(panel).ok_or(ProjectError::PanelNotFound { id: panel })?;
        validate_new_cutout(panel, shape, depth)
    }

    /// Adds a cutout to a panel and bumps the revision once.
    ///
    /// The cutout id is allocated from the document; callers must not invent one. A cutout outside
    /// the panel, a blind cutout that is not shallower than the thickness, or an overlap with an
    /// existing cutout is rejected before any id is consumed.
    pub fn add_cutout(
        &mut self,
        panel: Id,
        shape: CutoutShape,
        depth: CutoutDepth,
        entry: FaceSide,
    ) -> Result<Id, ProjectError> {
        self.validate_cutout(panel, &shape, depth)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let id = self.allocate_id()?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        panel_node.push_machining(Machining::Cutout(Cutout::new(id, shape, depth, entry)));
        self.revision = next_revision;
        Ok(id)
    }

    /// Checks the schema version, nesting depth, global id uniqueness and the id allocator.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.schema_version != Self::CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: Self::CURRENT_SCHEMA_VERSION,
            });
        }

        let max_id = scan_tree(&self.root)?;
        if self.next_id.get() <= max_id {
            return Err(ProjectError::NextIdNotAfterMax { next: self.next_id, max: max_id });
        }
        Ok(())
    }

    /// Adds a panel to an existing container and bumps the revision once.
    ///
    /// Panels may live in any container, whether an ordinary group or a furniture component; a
    /// group is only organisational. The panel id is allocated from the document; callers must not
    /// invent one.
    pub fn add_panel(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        nominal_size: PanelNominalSize,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let panel = Panel::new(id, name, nominal_size, material, last_known_thickness, placement)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Panel(panel));
        self.revision = next_revision;
        Ok(id)
    }

    /// Adds a panel whose outline is a contour ring selected in the scene.
    ///
    /// The ring is moved so its minimum corner is the panel-local zero, which is the panel frame
    /// anchor (ADR 0009); a ring that is already anchored is unchanged apart from rounding. The
    /// panel is created in one revision with the ring as its outline, so a rectangle panel never
    /// converts into a contour panel and the reverse is impossible too.
    pub fn add_panel_contour(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        contour: &Contour,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let anchored = Panel::anchored_contour(contour)?;
        let panel = Panel::new_contour(id, name, anchored, material, last_known_thickness, placement)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Panel(panel));
        self.revision = next_revision;
        Ok(id)
    }

    /// The contour of a constructive object that a new panel can inherit.
    ///
    /// A missing node, a mesh object or a node that is not a constructive object is an error the
    /// user can read; nothing is derived when the payload has no extrusion.
    pub fn object_contour(&self, id: Id) -> Result<&Contour, ProjectError> {
        let object = self.object(id).ok_or(ProjectError::NodeNotFound { id })?;
        let constructive = object.geometry().as_constructive().ok_or(ProjectError::NotConstructiveObject { id })?;
        constructive.contour().ok_or(ProjectError::NotConstructiveObject { id })
    }

    /// Checks whether an object's contour can become a panel outline without changing the document.
    pub fn validate_panel_contour(&self, parent: Id, source: Id) -> Result<(), ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let anchored = Panel::anchored_contour(self.object_contour(source)?)?;
        Panel::contour_size(&anchored)?;
        Ok(())
    }

    /// Adds a geometry object to an existing container and bumps the revision once.
    ///
    /// The object id is allocated from the document; callers must not invent one. The parent may
    /// be the root or any nested container.
    pub fn add_object(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        geometry: Geometry,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let object = Object::new(id, name, placement, geometry);
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Object(object));
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces the contour of a constructive object and bumps the revision once.
    ///
    /// A node that is not a constructive object is [`ProjectError::NotConstructiveObject`]; an
    /// invalid contour is refused before the document changes.
    pub fn set_object_contour(&mut self, id: Id, contour: Contour) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotConstructiveObject { id });
        };
        let Some(constructive) = object.geometry_mut().as_constructive_mut() else {
            return Err(ProjectError::NotConstructiveObject { id });
        };
        constructive.set_contour(contour)?;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a constructive payload with a baked mesh and bumps the revision once.
    ///
    /// The caller evaluates the feature history and passes the resulting mesh; the id, name,
    /// placement, pivot and properties are unchanged and the features are dropped
    /// ([ADR 0037]). A node that is not a constructive object is refused.
    ///
    /// [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
    pub fn collapse_object(&mut self, id: Id, mesh: Mesh) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotConstructiveObject { id });
        };
        if object.geometry().as_constructive().is_none() {
            return Err(ProjectError::NotConstructiveObject { id });
        }
        *object.geometry_mut() = Geometry::Mesh(mesh);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Adds an authored camera to an existing container and bumps the revision once.
    ///
    /// The projection is validated by [`CameraNode::new`]; nothing changes when it is refused.
    pub fn add_camera(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        projection: CameraProjection,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let camera = CameraNode::new(id, name, placement, projection)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Camera(camera));
        self.revision = next_revision;
        Ok(id)
    }

    /// Adds an authored light to an existing container and bumps the revision once.
    pub fn add_light(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        kind: LightKind,
        color: [f32; 3],
        intensity: f32,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let light = LightNode::new(id, name, placement, kind, color, intensity)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Light(light));
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces a camera's projection and bumps the revision once.
    ///
    /// The projection is validated before anything changes; an id that is not a camera is
    /// [`ProjectError::CameraNotFound`].
    pub fn set_camera_projection(&mut self, id: Id, projection: CameraProjection) -> Result<Revision, ProjectError> {
        if !projection.is_valid() {
            return Err(CameraError::InvalidProjection.into());
        }
        let next_revision = self.next_revision()?;
        let Some(camera) = find_node_mut(&mut self.root, id).and_then(Node::as_camera_mut) else {
            return Err(ProjectError::CameraNotFound { id });
        };
        camera.set_projection(projection)?;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a light's kind, colour and intensity and bumps the revision once.
    ///
    /// The values are validated before anything changes; an id that is not a light is
    /// [`ProjectError::LightNotFound`].
    pub fn set_light_params(
        &mut self,
        id: Id,
        kind: LightKind,
        color: [f32; 3],
        intensity: f32,
    ) -> Result<Revision, ProjectError> {
        if !valid_light_color(color) {
            return Err(LightError::InvalidColor.into());
        }
        if !valid_light_intensity(intensity) {
            return Err(LightError::InvalidIntensity.into());
        }
        let next_revision = self.next_revision()?;
        let Some(light) = find_node_mut(&mut self.root, id).and_then(Node::as_light_mut) else {
            return Err(ProjectError::LightNotFound { id });
        };
        light.set_params(kind, color, intensity)?;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Moves mesh vertices to absolute local positions and bumps the revision once.
    ///
    /// The batch is validated before anything changes: an empty batch, a repeated index or an
    /// index outside the mesh rejects the whole call, so a partial move can never reach the
    /// document. A constructive object has no editable mesh and is refused; it becomes a mesh
    /// object only through `object.collapse`.
    pub fn move_mesh_vertices(&mut self, id: Id, moves: &[(u32, [Length; 3])]) -> Result<Revision, ProjectError> {
        if moves.is_empty() {
            return Err(ProjectError::NoVertexMoves);
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Some(object) = node.as_object_mut() else {
            return Err(ProjectError::NotAMeshObject { id });
        };
        let Some(mesh) = object.geometry_mut().as_mesh_mut() else {
            return Err(ProjectError::NotAMeshObject { id });
        };
        let count = mesh.positions().len();
        let mut seen = HashSet::with_capacity(moves.len());
        let mut resolved = Vec::with_capacity(moves.len());
        for (vertex, position) in moves {
            if !seen.insert(*vertex) {
                return Err(ProjectError::DuplicateVertex { object: id, vertex: *vertex });
            }
            let Ok(index) = usize::try_from(*vertex) else {
                return Err(MeshError::VertexIndexOutOfBounds { vertex: usize::MAX, count }.into());
            };
            if index >= count {
                return Err(MeshError::VertexIndexOutOfBounds { vertex: index, count }.into());
            }
            resolved.push((index, *position));
        }
        for (index, position) in resolved {
            mesh.set_position(index, position)?;
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Adds an empty container marked as a furniture component and bumps the revision once.
    ///
    /// The parent may be the root or any nested container. Adding a child below
    /// [`Project::MAX_ASSEMBLY_DEPTH`] is rejected.
    pub fn add_component(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        let Some((depth, _)) = find_container(&self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        if depth >= Self::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Self::MAX_ASSEMBLY_DEPTH });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let mut container = Container::new(id, name, placement, Vec::new());
        container.set_component_marker(true)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Container(container));
        self.revision = next_revision;
        Ok(id)
    }

    /// Renames any node, including the root container, and bumps the revision once.
    ///
    /// Callers validate that the name is not blank; the model keeps user-visible text verbatim.
    pub fn rename_node(&mut self, id: Id, name: impl Into<String>) -> Result<Revision, ProjectError> {
        let name = name.into();
        let next_revision = self.next_revision()?;
        if id == self.root.id() {
            self.root.set_name(name);
            self.revision = next_revision;
            return Ok(next_revision);
        }
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        node.set_name(name);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a panel or container placement and bumps the revision once.
    ///
    /// The root container defines the world frame and cannot be moved. The placement stays relative to
    /// the parent container, so moving a container moves all of its leaves.
    pub fn set_node_placement(&mut self, id: Id, placement: Placement) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        node.set_placement(placement);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces several node placements at once and bumps the revision once.
    ///
    /// The whole batch is validated before anything changes: it must not be empty, every node must
    /// exist, none may be the root, and no node may appear twice. Listing a container together with
    /// one of its descendants is allowed here; a caller that moves a selection computes the
    /// placements and should first keep only [`Project::top_most_nodes`].
    pub fn set_node_placements(&mut self, placements: &[(Id, Placement)]) -> Result<Revision, ProjectError> {
        if placements.is_empty() {
            return Err(ProjectError::NoPlacements);
        }
        let mut seen = HashSet::new();
        for (id, _) in placements {
            if !seen.insert(*id) {
                return Err(ProjectError::DuplicateNode { id: *id });
            }
            if *id == self.root.id() {
                return Err(ProjectError::RootImmutable { id: *id });
            }
            if !self.contains_node(*id) {
                return Err(ProjectError::NodeNotFound { id: *id });
            }
        }
        let next_revision = self.next_revision()?;
        for (id, placement) in placements {
            let Some(node) = find_node_mut(&mut self.root, *id) else {
                return Err(ProjectError::NodeNotFound { id: *id });
            };
            node.set_placement(*placement);
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a panel or a container, with its children, and bumps the revision once.
    ///
    /// The root cannot be removed. Removed ids are never reused.
    pub fn remove_node(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        if self.node(id).is_none() {
            return Err(ProjectError::NodeNotFound { id });
        }
        let next_revision = self.next_revision()?;
        if !remove_child(&mut self.root, id) {
            return Err(ProjectError::NodeNotFound { id });
        }
        // A deleted active camera, or a container that held it, returns the scene to the built-in
        // camera instead of leaving a dangling reference in the document.
        if self.active_camera.is_some_and(|camera| self.camera(camera).is_none()) {
            self.active_camera = None;
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Deep-copies a panel or a container under the same parent and returns the copy's id.
    ///
    /// Every copied node receives a fresh id from the document allocator. The copy keeps its kind,
    /// placement and geometry; only the copied root's name gets a `copy` suffix. The new node is
    /// inserted directly after the original.
    pub fn duplicate_node(&mut self, id: Id) -> Result<Id, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let original = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let parent_id = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
        let next_revision = self.next_revision()?;
        let count = count_nodes(&original);
        let ids = self.allocate_ids(count)?;
        let mut copy = original;
        let name = format!("{} copy", copy.name());
        copy.set_name(name);
        let new_id = assign_new_ids(&mut copy, &mut ids.into_iter())?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let Some(index) = children.iter().position(|child| child.id() == id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        children.insert(index.saturating_add(1), copy);
        self.revision = next_revision;
        Ok(new_id)
    }

    /// Wraps sibling nodes in one new group and returns the new group id.
    ///
    /// All selected nodes stay in their existing order and keep their placements. The new group
    /// has an identity placement relative to the shared parent, so no leaf moves in world space.
    pub fn group_nodes(&mut self, nodes: &[Id], name: impl Into<String>) -> Result<Id, ProjectError> {
        let (parent_id, _) = self.group_parent(nodes)?;
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let name = name.into();
        let selected: HashSet<u64> = nodes.iter().map(|id| id.get()).collect();
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let mut moved = Vec::new();
        let mut rest = Vec::new();
        let mut insert_at = None;
        for child in children.drain(..) {
            if selected.contains(&child.id().get()) {
                if insert_at.is_none() {
                    insert_at = Some(rest.len());
                }
                moved.push(child);
            } else {
                rest.push(child);
            }
        }
        let index = insert_at.unwrap_or(rest.len());
        rest.insert(index, Node::Container(Container::new(id, name, Placement::IDENTITY, moved)));
        *children = rest;
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces an ordinary container with its children and bumps the revision once.
    ///
    /// The children keep their world transforms: each child placement is composed with the removed
    /// container's placement. A container marked as a furniture component has its own identity and
    /// cannot be ungrouped by this command; use [`Project::convert_to_group`] first. Panels are
    /// atomic leaves and cannot be ungrouped. The root cannot be ungrouped.
    pub fn ungroup_group(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let node = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let Node::Container(container) = node else {
            return Err(ProjectError::NotAnOrdinaryContainer { id });
        };
        if container.is_component() {
            return Err(ProjectError::NotAComponent { id });
        }
        self.flatten_container(container)
    }

    /// Clears the component marker and prototype keys, then flattens the container.
    ///
    /// The container becomes an ordinary group and its children take its place in the parent, as
    /// with [`Project::ungroup_group`]. Nothing else about the children changes.
    pub fn convert_to_group(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let node = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let Node::Container(mut container) = node else {
            return Err(ProjectError::NotAnOrdinaryContainer { id });
        };
        container.clear_component_marker();
        self.flatten_container(container)
    }

    /// Moves a container's children into its parent and removes the container.
    fn flatten_container(&mut self, container: Container) -> Result<Revision, ProjectError> {
        let id = container.id();
        let parent_id = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
        let next_revision = self.next_revision()?;
        let container_placement = container.placement();
        let mut children = Vec::with_capacity(container.children().len());
        for mut child in container.into_children() {
            let placement = Placement::compose(container_placement, child.placement())
                .map_err(|_| ProjectError::PlacementCompose { id })?;
            child.set_placement(placement);
            children.push(child);
        }
        let Some((_depth, parent)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let Some(index) = parent.iter().position(|child| child.id() == id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        parent.remove(index);
        parent.splice(index..index, children);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// The property bag of a node, including the root.
    pub fn properties(&self, id: Id) -> Option<&Properties> {
        if id == self.root.id() {
            return Some(self.root.properties());
        }
        match self.node(id)? {
            Node::Container(container) => Some(container.properties()),
            Node::Panel(panel) => Some(panel.properties()),
            Node::Object(object) => Some(object.properties()),
            Node::Camera(camera) => Some(camera.properties()),
            Node::Light(light) => Some(light.properties()),
        }
    }

    /// The furniture type stored on a node; unclassified when the node has no type.
    pub fn furntype(&self, id: Id) -> Option<FurnType> {
        match self.properties(id)?.get(SYSTEM_FURNTYPE) {
            Some(PropertyValue::FurnType { value }) => Some(*value),
            Some(_) => None,
            None => Some(FurnType::UNCLASSIFIED),
        }
    }

    /// Sets one property on one node and bumps the revision once.
    pub fn set_property(&mut self, id: Id, key: &str, value: PropertyValue) -> Result<Revision, ProjectError> {
        let changes = [PropertyChange { node: id, key: key.to_owned(), value }];
        self.set_properties(&changes)
    }

    /// Sets several properties as one document change: one revision and one undo entry.
    pub fn set_properties(&mut self, changes: &[PropertyChange]) -> Result<Revision, ProjectError> {
        if changes.is_empty() {
            return Err(ProjectError::NoPropertyChanges);
        }
        let root = self.root.id();
        let mut seen: HashSet<(u64, &str)> = HashSet::new();
        for change in changes {
            if change.node == root {
                return Err(ProjectError::RootImmutable { id: change.node });
            }
            if !seen.insert((change.node.get(), change.key.as_str())) {
                return Err(ProjectError::DuplicateProperty { node: change.node, key: change.key.clone() });
            }
            if !self.contains_node(change.node) {
                return Err(ProjectError::NodeNotFound { id: change.node });
            }
            validate_property_value(&change.key, &change.value)?;
        }
        let next_revision = self.next_revision()?;
        for change in changes {
            let Some(node) = find_node_mut(&mut self.root, change.node) else {
                return Err(ProjectError::NodeNotFound { id: change.node });
            };
            match node {
                Node::Container(container) => container.set_property(&change.key, change.value.clone())?,
                Node::Panel(panel) => panel.set_property(&change.key, change.value.clone())?,
                Node::Object(object) => object.set_property(&change.key, change.value.clone())?,
                Node::Camera(camera) => camera.set_property(&change.key, change.value.clone())?,
                Node::Light(light) => light.set_property(&change.key, change.value.clone())?,
            }
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes one property from one node and bumps the revision once.
    pub fn clear_property(&mut self, id: Id, key: &str) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let removed = match node {
            Node::Container(container) => container.clear_property(key),
            Node::Panel(panel) => panel.clear_property(key),
            Node::Object(object) => object.clear_property(key),
            Node::Camera(camera) => camera.clear_property(key),
            Node::Light(light) => light.clear_property(key),
        };
        if !removed {
            return Err(ProjectError::PropertyNotFound { node: id, key: key.to_owned() });
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Selects the nodes that match a query, in tree order.
    pub fn query(&self, query: &NodeQuery) -> Vec<Id> {
        fn matches(properties: &Properties, kind: StructuralKind, query: &NodeQuery) -> bool {
            if let Some(wanted) = query.kind()
                && wanted != kind
            {
                return false;
            }
            if let Some(wanted) = query.furntype() {
                let found = match properties.get(SYSTEM_FURNTYPE) {
                    Some(PropertyValue::FurnType { value }) => *value,
                    _ => FurnType::UNCLASSIFIED,
                };
                if !found.is_ancestor_or_self(wanted) {
                    return false;
                }
            }
            if let Some(predicate) = query.property()
                && properties.get(predicate.key()) != Some(predicate.value())
            {
                return false;
            }
            true
        }
        fn walk(children: &[Node], query: &NodeQuery, ids: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                match child {
                    Node::Container(container) => {
                        if matches(container.properties(), StructuralKind::Container, query) {
                            ids.push(container.id());
                        }
                        walk(container.children(), query, ids, depth.saturating_add(1));
                    }
                    Node::Panel(panel) => {
                        if matches(panel.properties(), StructuralKind::Panel, query) {
                            ids.push(panel.id());
                        }
                    }
                    Node::Object(object) => {
                        if matches(object.properties(), StructuralKind::Object, query) {
                            ids.push(object.id());
                        }
                    }
                    Node::Camera(camera) => {
                        if matches(camera.properties(), StructuralKind::Camera, query) {
                            ids.push(camera.id());
                        }
                    }
                    Node::Light(light) => {
                        if matches(light.properties(), StructuralKind::Light, query) {
                            ids.push(light.id());
                        }
                    }
                }
            }
        }
        let mut ids = Vec::new();
        walk(self.root.children(), query, &mut ids, 1);
        ids
    }

    /// Advances the revision by one and returns it.
    pub fn bump_revision(&mut self) -> Result<Revision, ProjectError> {
        let next = self.next_revision()?;
        self.revision = next;
        Ok(next)
    }

    /// Replaces the user-visible content from a snapshot and advances the revision.
    ///
    /// Undo and redo use this instead of replacing the whole project: the revision stays
    /// monotonic for caches, and the id allocator never moves backwards, so allocated ids are
    /// never reused after an undo.
    pub fn restore_from(&mut self, snapshot: &Project) -> Result<Revision, ProjectError> {
        let next_revision = self.next_revision()?;
        self.schema_version = snapshot.schema_version;
        self.next_id = self.next_id.max(snapshot.next_id);
        self.root = snapshot.root.clone();
        self.active_camera = snapshot.active_camera;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Compares user-visible content, ignoring the revision and the id allocator.
    pub fn same_content(&self, other: &Project) -> bool {
        self.schema_version == other.schema_version
            && self.root == other.root
            && self.active_camera == other.active_camera
    }

    fn next_revision(&self) -> Result<Revision, ProjectError> {
        self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)
    }

    fn allocate_id(&mut self) -> Result<Id, ProjectError> {
        let id = self.next_id;
        let next = id.get().checked_add(1).ok_or(ProjectError::IdSpaceExhausted)?;
        self.next_id = Id::new(next).ok_or(ProjectError::IdSpaceExhausted)?;
        Ok(id)
    }

    /// Reserves `count` consecutive ids before any tree mutation starts.
    ///
    /// The allocator must still be able to produce the next id after the last reserved one, so a
    /// full id space fails here instead of leaving a half-copied subtree behind.
    fn allocate_ids(&mut self, count: u64) -> Result<Vec<Id>, ProjectError> {
        let last = self.next_id.get().checked_add(count.saturating_sub(1)).ok_or(ProjectError::IdSpaceExhausted)?;
        if last == u64::MAX {
            return Err(ProjectError::IdSpaceExhausted);
        }
        let capacity = usize::try_from(count).map_err(|_| ProjectError::IdSpaceExhausted)?;
        let mut ids = Vec::with_capacity(capacity);
        for _ in 0..count {
            ids.push(self.allocate_id()?);
        }
        Ok(ids)
    }
}

/// Validates the tree and returns the largest id it uses.
fn scan_tree(root: &Container) -> Result<u64, ProjectError> {
    let mut seen = HashSet::new();
    let mut max_id = 0u64;
    let mut stack = vec![(root, 1u32)];
    while let Some((container, depth)) = stack.pop() {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        let id = container.id();
        if !seen.insert(id.get()) {
            return Err(ProjectError::DuplicateId(id));
        }
        max_id = max_id.max(id.get());
        for child in container.children() {
            match child {
                Node::Container(container) => {
                    stack.push((container, depth.saturating_add(1)));
                }
                Node::Panel(panel) => {
                    if !seen.insert(panel.id().get()) {
                        return Err(ProjectError::DuplicateId(panel.id()));
                    }
                    max_id = max_id.max(panel.id().get());
                    validate_panel_banding(panel)?;
                    for machining in panel.machining() {
                        match machining {
                            Machining::Cutout(cutout) => {
                                if !seen.insert(cutout.id().get()) {
                                    return Err(ProjectError::DuplicateId(cutout.id()));
                                }
                                max_id = max_id.max(cutout.id().get());
                            }
                        }
                    }
                }
                Node::Object(object) => {
                    if !seen.insert(object.id().get()) {
                        return Err(ProjectError::DuplicateId(object.id()));
                    }
                    max_id = max_id.max(object.id().get());
                }
                Node::Camera(camera) => {
                    if !seen.insert(camera.id().get()) {
                        return Err(ProjectError::DuplicateId(camera.id()));
                    }
                    max_id = max_id.max(camera.id().get());
                }
                Node::Light(light) => {
                    if !seen.insert(light.id().get()) {
                        return Err(ProjectError::DuplicateId(light.id()));
                    }
                    max_id = max_id.max(light.id().get());
                }
            }
        }
    }
    Ok(max_id)
}

/// The id immediately after the largest used id.
fn next_id_after(max_id: u64) -> Result<Id, ProjectError> {
    let next = max_id.checked_add(1).ok_or(ProjectError::IdSpaceExhausted)?;
    Id::new(next).ok_or(ProjectError::IdSpaceExhausted)
}

/// Finds any container, including the root.
fn validate_panel_banding(panel: &Panel) -> Result<(), ProjectError> {
    let mut seen = HashSet::new();
    for band in panel.edge_banding() {
        if !matches!(band.edge().get(), 1..=4) {
            return Err(ProjectError::UnsupportedEdgeBanding { panel: panel.id(), edge: band.edge() });
        }
        if !seen.insert(band.edge().get()) {
            return Err(ProjectError::DuplicateEdgeBanding { panel: panel.id(), edge: band.edge() });
        }
    }
    Ok(())
}

fn validate_panel_machining(panel: &Panel, geometry: PanelGeometry) -> Result<(), ProjectError> {
    for machining in panel.machining() {
        let Machining::Cutout(cutout) = machining;
        validate_cutout_geometry(panel.id(), geometry, cutout.shape(), cutout.depth())?;
    }
    validate_cutout_overlaps(panel.id(), panel.machining())?;
    validate_core(panel, geometry, None)
}

fn validate_new_cutout(panel: &Panel, shape: &CutoutShape, depth: CutoutDepth) -> Result<(), ProjectError> {
    if panel.has_contour_features() {
        return Err(ProjectError::ContourMachiningUnsupported { panel: panel.id() });
    }
    let geometry = panel.geometry_with_fallbacks()?;
    validate_cutout_geometry(panel.id(), geometry, shape, depth)?;
    for machining in panel.machining() {
        let Machining::Cutout(existing) = machining;
        let both_through = matches!(depth, CutoutDepth::Through) && matches!(existing.depth(), CutoutDepth::Through);
        if !both_through && cutout_shapes_overlap(shape, existing.shape())? {
            return Err(ProjectError::CutoutOverlap { panel: panel.id(), other: existing.id() });
        }
    }
    let candidate = match (depth, shape) {
        (CutoutDepth::Through, CutoutShape::Rectangle(rect)) => Some(*rect),
        _ => None,
    };
    validate_core(panel, geometry, candidate)
}

fn validate_cutout_geometry(
    panel: Id,
    geometry: PanelGeometry,
    shape: &CutoutShape,
    depth: CutoutDepth,
) -> Result<(), ProjectError> {
    match depth {
        CutoutDepth::Through => {
            let reaches = match shape {
                CutoutShape::Rectangle(rect) => geometry.cutout_reaches_core(*rect),
                CutoutShape::Contour(contour) => geometry.contour_inside_core(contour),
            };
            if !reaches {
                return Err(ProjectError::CutoutOutsidePanel { panel });
            }
        }
        CutoutDepth::Blind { depth } => {
            let fits = match shape {
                CutoutShape::Rectangle(rect) => geometry.cutout_fits_core(*rect),
                CutoutShape::Contour(contour) => geometry.contour_fits_core(contour),
            };
            if !fits {
                return Err(ProjectError::CutoutOutsidePanel { panel });
            }
            let thickness = geometry.thickness();
            if !depth.is_positive() || depth >= thickness {
                return Err(ProjectError::CutoutDepthInvalid { panel, depth, thickness });
            }
        }
    }
    Ok(())
}

fn validate_cutout_overlaps(panel: Id, machining: &[Machining]) -> Result<(), ProjectError> {
    let cutouts: Vec<&Cutout> = machining
        .iter()
        .map(|machining| {
            let Machining::Cutout(cutout) = machining;
            cutout
        })
        .collect();
    for (index, cutout) in cutouts.iter().enumerate() {
        for other in &cutouts[..index] {
            let both_through =
                matches!(cutout.depth(), CutoutDepth::Through) && matches!(other.depth(), CutoutDepth::Through);
            if !both_through && cutout_shapes_overlap(cutout.shape(), other.shape())? {
                return Err(ProjectError::CutoutOverlap { panel, other: other.id() });
            }
        }
    }
    Ok(())
}

fn validate_core(panel: &Panel, geometry: PanelGeometry, extra: Option<PanelRect>) -> Result<(), ProjectError> {
    match core_violation_of(panel, geometry, extra) {
        Some(CoreViolation::Removed) => Err(ProjectError::CutoutRemovesCore { panel: panel.id() }),
        Some(CoreViolation::Split) => Err(ProjectError::CutoutSplitsCore { panel: panel.id() }),
        None => Ok(()),
    }
}

/// Whether two cutout outlines share area.
///
/// A rectangle pair uses the exact rectangle test. A contour takes part as a chord polygon sampled
/// at the self-intersection resolution, so a contour against a rectangle or another contour is
/// decided by edge contact and vertex containment. The chord-pair budget keeps a hostile pair from
/// buying unbounded work.
fn cutout_shapes_overlap(first: &CutoutShape, second: &CutoutShape) -> Result<bool, ProjectError> {
    if let (CutoutShape::Rectangle(a), CutoutShape::Rectangle(b)) = (first, second) {
        return Ok(rects_overlap(*a, *b));
    }
    let first = cutout_polygon(first)?;
    let second = cutout_polygon(second)?;
    polygons_overlap(&first, &second)
}

/// The outline as a closed chord polygon in whole micrometres.
fn cutout_polygon(shape: &CutoutShape) -> Result<Vec<[i64; 2]>, ProjectError> {
    match shape {
        CutoutShape::Rectangle(rect) => Ok(rect_polygon(*rect)),
        CutoutShape::Contour(contour) => {
            let points = contour.sampled_points(SELF_INTERSECTION_TOLERANCE_MICROMETRES)?;
            Ok(points.into_iter().map(|point| [point[0].micrometres(), point[1].micrometres()]).collect())
        }
    }
}

fn rect_polygon(rect: PanelRect) -> Vec<[i64; 2]> {
    let x0 = rect.x().micrometres();
    let y0 = rect.y().micrometres();
    let x1 = x0.saturating_add(rect.width().micrometres());
    let y1 = y0.saturating_add(rect.height().micrometres());
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn polygons_overlap(first: &[[i64; 2]], second: &[[i64; 2]]) -> Result<bool, ProjectError> {
    let first_count = first.len();
    let second_count = second.len();
    if first_count < 3 || second_count < 3 {
        return Ok(false);
    }
    if !SegBounds::of(first).overlaps(SegBounds::of(second)) {
        return Ok(false);
    }
    if first_count.saturating_mul(second_count) > MAX_SELF_INTERSECTION_PAIRS {
        return Err(ContourError::SamplingTooDetailed { max: MAX_SELF_INTERSECTION_PAIRS }.into());
    }
    for index in 0..first_count {
        let (Some(a), Some(b)) = (first.get(index), first.get((index + 1) % first_count)) else {
            continue;
        };
        for other in 0..second_count {
            let (Some(c), Some(d)) = (second.get(other), second.get((other + 1) % second_count)) else {
                continue;
            };
            if segments_touch(*a, *b, *c, *d) {
                return Ok(true);
            }
        }
    }
    Ok(first.iter().any(|point| point_in_polygon(*point, second))
        || second.iter().any(|point| point_in_polygon(*point, first)))
}

/// Whether a point lies inside a chord polygon, by the even-odd rule.
///
/// A point on the boundary is decided by the edge test the caller runs first; here only a point
/// strictly inside toggles the parity.
fn point_in_polygon(point: [i64; 2], polygon: &[[i64; 2]]) -> bool {
    let count = polygon.len();
    if count < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..count {
        let (Some(current), Some(previous)) = (polygon.get(index), polygon.get((index + count - 1) % count)) else {
            continue;
        };
        let (a, b) = (*previous, *current);
        if (a[1] > point[1]) == (b[1] > point[1]) {
            continue;
        }
        let dx = i128::from(b[0]) - i128::from(a[0]);
        let dy = i128::from(b[1]) - i128::from(a[1]);
        let cross = dx * (i128::from(point[1]) - i128::from(a[1])) - (i128::from(point[0]) - i128::from(a[0])) * dy;
        if (cross > 0) == (dy > 0) {
            inside = !inside;
        }
    }
    inside
}

/// Why a project cannot be loaded, constructed or mutated.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProjectError {
    /// The file was written by a newer or otherwise unsupported schema.
    #[error("unsupported schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion {
        /// The version found in the document.
        found: u32,
        /// The version supported by this build.
        supported: u32,
    },
    /// Two nodes in the tree use the same id.
    #[error("duplicate document id {0}")]
    DuplicateId(Id),
    /// A geometry payload is invalid.
    #[error("invalid geometry: {0}")]
    InvalidGeometry(#[from] MeshError),
    /// A constructive contour or feature list is invalid.
    #[error("invalid constructive geometry: {0}")]
    InvalidConstructive(#[from] ConstructiveError),
    /// An authored camera could not be built.
    #[error(transparent)]
    Camera(#[from] CameraError),
    /// An authored light could not be built.
    #[error(transparent)]
    Light(#[from] LightError),
    /// The requested node is not an authored camera.
    #[error("camera {id} not found")]
    CameraNotFound {
        /// The missing camera id.
        id: Id,
    },
    /// The requested node is not an authored light.
    #[error("light {id} not found")]
    LightNotFound {
        /// The missing light id.
        id: Id,
    },
    /// The node tree is nested more deeply than the supported limit.
    #[error("container nesting exceeds {max} levels")]
    TooDeep {
        /// The maximum supported depth.
        max: u32,
    },
    /// The requested parent container is not in the tree.
    #[error("container {id} not found")]
    ContainerNotFound {
        /// The missing container id.
        id: Id,
    },
    /// The requested panel is not in the tree.
    #[error("panel {id} not found")]
    PanelNotFound {
        /// The missing panel id.
        id: Id,
    },
    /// The requested node is not in the tree, or it is the root container.
    #[error("node {id} not found below the root")]
    NodeNotFound {
        /// The missing node id.
        id: Id,
    },
    /// The root container cannot be deleted, moved, grouped, ungrouped or dissolved.
    #[error("root container {id} cannot be changed this way")]
    RootImmutable {
        /// The root container id.
        id: Id,
    },
    /// Ungrouping was requested for a node that is not an ordinary container.
    ///
    /// A panel is an atomic leaf; a component has its own identity and is
    /// converted to a group before `node.ungroup` will flatten it.
    #[error("node {id} is not an ordinary container and cannot be ungrouped")]
    NotAnOrdinaryContainer {
        /// The node that is not an ordinary container.
        id: Id,
    },
    /// Ungrouping was requested for a furniture component.
    #[error("node {id} is a furniture component and cannot be ungrouped; convert it to a group first")]
    NotAComponent {
        /// The component that was not flattened.
        id: Id,
    },
    /// A property batch contained no changes.
    #[error("no property changes were given")]
    NoPropertyChanges,
    /// The same node and property are listed twice in one batch.
    #[error("property `{key}` on node {node} is changed more than once")]
    DuplicateProperty {
        /// The node with the repeated change.
        node: Id,
        /// The repeated property key.
        key: String,
    },
    /// The requested property is not stored on the node.
    #[error("node {node} has no property `{key}`")]
    PropertyNotFound {
        /// The node that was asked.
        node: Id,
        /// The missing property key.
        key: String,
    },
    /// A property key or value violates the document property rules.
    #[error(transparent)]
    Property(#[from] PropertyError),
    /// Grouping needs nodes that share one parent container.
    #[error("nodes selected for grouping do not share one parent container")]
    NodesNotSiblings,
    /// The same node was listed more than once.
    #[error("node {id} is selected more than once")]
    DuplicateNode {
        /// The repeated node id.
        id: Id,
    },
    /// A placement batch contained no entries.
    #[error("no node placements were given")]
    NoPlacements,
    /// A vertex batch contained no entries.
    #[error("no mesh vertex moves were given")]
    NoVertexMoves,
    /// The same mesh vertex is moved more than once in one batch.
    #[error("vertex {vertex} of object {object} is moved more than once")]
    DuplicateVertex {
        /// The object whose vertex was repeated.
        object: Id,
        /// The repeated vertex index.
        vertex: u32,
    },
    /// The node is not a mesh object, so it has no editable vertices.
    #[error("node {id} is not a mesh object")]
    NotAMeshObject {
        /// The node that was asked.
        id: Id,
    },
    /// The node is not a constructive object, so it has no contour or feature history.
    #[error("node {id} is not a constructive object")]
    NotConstructiveObject {
        /// The node that was asked.
        id: Id,
    },
    /// The node is not a contour panel, so it has no outline ring to replace.
    #[error("panel {id} is not a contour panel")]
    PanelNotContour {
        /// The panel that was asked.
        id: Id,
    },
    /// A contour edit moves the panel frame beyond what the placement or pivot can represent.
    #[error("the frame of panel {panel} cannot follow its new outline")]
    PanelOutlineFrame {
        /// The panel whose frame could not move.
        panel: Id,
    },
    /// Grouping needs at least two nodes.
    #[error("grouping needs at least {minimum} nodes, got {found}")]
    TooFewNodes {
        /// The smallest accepted selection.
        minimum: u32,
        /// The number of nodes supplied.
        found: usize,
    },
    /// A placement could not be composed while a container was ungrouped.
    #[error("container {id} has a placement that cannot be composed with a child")]
    PlacementCompose {
        /// The container whose placement could not be applied.
        id: Id,
    },
    /// A stored panel dimension is not usable.
    #[error(transparent)]
    PanelDimension(#[from] PanelDimensionError),
    /// The panel cannot be resolved into core, overall and thickness.
    #[error(transparent)]
    PanelGeometry(#[from] PanelGeometryError),
    /// The panel's nominal contour cannot be built or grouped.
    #[error(transparent)]
    PanelContour(#[from] PanelContourError),
    /// A panel's stored fields or outline cannot be read as a panel.
    #[error(transparent)]
    Panel(#[from] PanelError),
    /// A document contour is not valid.
    #[error(transparent)]
    Contour(#[from] ContourError),
    /// Edge banding refers to an edge outside the rectangular v1 outline.
    #[error("edge banding on panel {panel} uses unsupported edge {edge}")]
    UnsupportedEdgeBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The rejected edge.
        edge: EdgeId,
    },
    /// A tangent path ended up partially banded or with differing specs.
    #[error("panel {panel} has incompatible banding along one tangent path")]
    IncompatiblePathBanding {
        /// The panel whose path is inconsistent.
        panel: Id,
    },
    /// Two banding records cover the same edge.
    #[error("panel {panel} has duplicate edge banding on edge {edge}")]
    DuplicateEdgeBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The duplicated edge.
        edge: EdgeId,
    },
    /// The requested edge has no banding to clear.
    #[error("panel {panel} has no edge banding on edge {edge}")]
    EdgeBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The edge that was requested.
        edge: EdgeId,
    },
    /// The requested ring segment has no banding to clear.
    #[error("panel {panel} has no ring banding on segment {segment}")]
    RingBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The ring segment that was requested.
        segment: ContourSegmentId,
    },
    /// The requested corner has no treatment to clear.
    #[error("panel {panel} has no corner feature on corner {corner}")]
    CornerFeatureNotFound {
        /// The panel that owns the feature.
        panel: Id,
        /// The corner that was requested.
        corner: VertexId,
    },
    /// The requested corner has no banding to clear.
    #[error("panel {panel} has no corner banding on corner {corner}")]
    CornerBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The corner that was requested.
        corner: VertexId,
    },
    /// Corner banding uses a corner outside the rectangular outline.
    #[error("panel {panel} uses unsupported corner {corner}")]
    UnsupportedCornerBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The rejected corner.
        corner: VertexId,
    },
    /// The requested edge has no bow to clear.
    #[error("panel {panel} has no bow on edge {edge}")]
    EdgeBowNotFound {
        /// The panel that owns the bow.
        panel: Id,
        /// The edge that was requested.
        edge: EdgeId,
    },
    /// A through cutout does not reach the core, or a blind cutout is not fully inside it.
    #[error("cutout on panel {panel} lies outside the core")]
    CutoutOutsidePanel {
        /// The panel that owns the cutout.
        panel: Id,
    },
    /// A blind cutout depth is zero, negative or not shallower than the panel.
    #[error("blind cutout depth {depth} on panel {panel} must be positive and less than thickness {thickness}")]
    CutoutDepthInvalid {
        /// The panel that owns the cutout.
        panel: Id,
        /// The rejected depth.
        depth: Length,
        /// The current panel thickness.
        thickness: Length,
    },
    /// A blind cutout overlaps another cutout on the same panel.
    #[error("cutout on panel {panel} overlaps cutout {other}")]
    CutoutOverlap {
        /// The panel that owns the cutouts.
        panel: Id,
        /// The other cutout in the conflicting pair.
        other: Id,
    },
    /// The through cutouts would remove the whole core.
    #[error("cutouts on panel {panel} would remove the whole core")]
    CutoutRemovesCore {
        /// The panel that owns the cutouts.
        panel: Id,
    },
    /// The through cutouts would split the remaining core into disconnected pieces.
    #[error("cutout on panel {panel} would split the core into disconnected pieces")]
    CutoutSplitsCore {
        /// The panel that owns the cutouts.
        panel: Id,
    },
    /// A machining coordinate overflows while a legacy document is migrated.
    #[error("machining on panel {panel} overflows while migrating to the nominal frame")]
    MachiningCoordinateOverflow {
        /// The panel whose machining could not be translated.
        panel: Id,
    },
    /// A shaped panel cannot take machining until the contour boolean layer lands.
    #[error("shaped panel {panel} does not support machining yet")]
    ContourMachiningUnsupported {
        /// The shaped panel.
        panel: Id,
    },
    /// The next id would collide with an id already used by the tree.
    #[error("next document id {next} must be greater than the largest used id {max}")]
    NextIdNotAfterMax {
        /// The allocator position from the document.
        next: Id,
        /// The largest id found in the tree.
        max: u64,
    },
    /// Every document id has been used.
    #[error("document id space exhausted")]
    IdSpaceExhausted,
    /// Every document revision has been used.
    #[error("document revision space exhausted")]
    RevisionExhausted,
}

/// Serialized form of [`Project`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectWire {
    schema_version: u32,
    revision: Revision,
    #[serde(default)]
    next_id: Option<Id>,
    root: Container,
    #[serde(default)]
    active_camera: Option<Id>,
}

impl TryFrom<ProjectWire> for Project {
    type Error = ProjectError;

    fn try_from(wire: ProjectWire) -> Result<Self, Self::Error> {
        // Version 1 stored a `size` on every panel; `PanelWire` migrates it to nominal size plus a
        // fallback. Version 2 stored machining in core-local coordinates; the migration below
        // translates it to the nominal panel frame. Version 7 unified the two container kinds;
        // `Node` reads a version-6 assembly as a component container. All versions are normalized
        // to the current one on load.
        if !(1..=Project::CURRENT_SCHEMA_VERSION).contains(&wire.schema_version) {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: wire.schema_version,
                supported: Project::CURRENT_SCHEMA_VERSION,
            });
        }
        let mut root = wire.root;
        if wire.schema_version <= 2 {
            migrate_machining_to_nominal(&mut root, 1)?;
        }
        if wire.schema_version <= 5 {
            // Before schema 6 the pivot was derived from the current tree. Materialize the new
            // default once so it stays stable: panels keep the local origin, non-empty containers
            // start at the centre of their local panel bounds.
            migrate_container_pivots(&mut root, 1)?;
        }
        root.set_pivot(Pivot::ZERO);
        let max_id = scan_tree(&root)?;
        let next_id = match wire.next_id {
            Some(next_id) if next_id.get() <= max_id => {
                return Err(ProjectError::NextIdNotAfterMax { next: next_id, max: max_id });
            }
            Some(next_id) => next_id,
            None => next_id_after(max_id)?,
        };
        let active_camera = match wire.active_camera {
            Some(id) if find_node(&root, id).is_some_and(|node| node.as_camera().is_some()) => Some(id),
            Some(id) => return Err(ProjectError::CameraNotFound { id }),
            None => None,
        };
        Ok(Self {
            schema_version: Project::CURRENT_SCHEMA_VERSION,
            revision: wire.revision,
            next_id,
            root,
            active_camera,
        })
    }
}

/// Translates version-2 core-local machining into the nominal panel frame.
///
/// The conversion adds the stored fallback core origin to every cutout, so a loaded file keeps the
/// hole at the same physical position. The migration is bounded by the same depth limit as the
/// document tree.
fn migrate_machining_to_nominal(root: &mut Container, depth: u32) -> Result<(), ProjectError> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
    }
    fn walk(children: &mut [Node], depth: u32) -> Result<(), ProjectError> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        for child in children {
            match child {
                Node::Container(container) => walk(container.children_mut(), depth.saturating_add(1))?,
                Node::Panel(panel) => panel.migrate_machining_from_core()?,
                Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
            }
        }
        Ok(())
    }
    walk(root.children_mut(), depth.saturating_add(1))
}

/// Materializes the default pivot on containers saved before schema 6.
///
/// Panels keep their zero default from [`PanelWire`]. The walk is bounded by the same depth limit
/// as the document tree.
fn migrate_container_pivots(root: &mut Container, depth: u32) -> Result<(), ProjectError> {
    fn walk(children: &mut [Node], depth: u32) -> Result<(), ProjectError> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        for child in children {
            match child {
                Node::Container(container) => {
                    container.set_pivot(default_container_pivot(container.children()));
                    walk(container.children_mut(), depth.saturating_add(1))?;
                }
                Node::Panel(_) | Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
            }
        }
        Ok(())
    }
    walk(root.children_mut(), depth.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    use furnicraft_geom::DQuat;

    use crate::fixtures::*;

    #[test]
    fn clearing_an_unbanded_edge_is_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let Some((_depth, panel)) = find_panel_mut(&mut project.root, panel_id, 1) else {
            return;
        };
        panel.upsert_corner_feature(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius"),
        );
        panel
            .apply_edge_banding(
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("apply band");
        assert_eq!(
            project.clear_edge_banding(panel_id, RectEdge::Top.id()),
            Err(ProjectError::EdgeBandingNotFound { panel: panel_id, edge: RectEdge::Top.id() })
        );
        assert_eq!(project.panel(panel_id).expect("panel").edge_banding().len(), 1);
    }

    #[test]
    fn projects_start_at_the_current_schema_and_revision() {
        let project = sample_project();
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        assert_eq!(project.revision(), Revision::ZERO);
        assert_eq!(project.root().id(), Id::new(1).expect("id"));
        assert_eq!(project.next_id().get(), 5);
        assert_eq!(project.root().children().len(), 2);
    }

    #[test]
    fn projects_add_panels_and_assemblies_with_fresh_ids() {
        let mut project = sample_project();
        let root_id = project.root().id();

        let panel_id = project
            .add_panel(root_id, "Back", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("add panel");
        assert_eq!(panel_id.get(), 5);
        assert_eq!(project.revision().get(), 1);
        assert_eq!(project.root().children().len(), 3);

        let assembly_id = project.add_component(root_id, "Carcass", Placement::IDENTITY).expect("add assembly");
        assert_eq!(assembly_id.get(), 6);
        assert_eq!(project.revision().get(), 2);
        assert_eq!(project.container(assembly_id).expect("new component").name(), "Carcass");
        assert!(project.is_component(assembly_id));
        assert!(project.validate().is_ok());
    }

    #[test]
    fn project_resizes_panels_and_reports_missing_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(4).expect("id");
        let nominal = PanelNominalSize::new(millimetres(900), millimetres(600)).expect("valid size");

        let revision = project.set_panel_nominal_size(panel_id, nominal).expect("resize panel");
        assert_eq!(revision.get(), 1);
        assert_eq!(project.revision(), revision);
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), nominal);

        let missing = Id::new(999).expect("id");
        assert_eq!(project.set_panel_nominal_size(missing, nominal), Err(ProjectError::PanelNotFound { id: missing }));
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn project_adds_cutouts_and_allocates_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));

        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        let panel = project.panel(panel_id).expect("panel exists");
        assert_eq!(panel.machining().len(), 1);
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);
        assert_eq!(cutout.entry(), FaceSide::A);
        assert!(project.validate().is_ok());

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn cutouts_reject_invalid_depth_bounds_and_overlap_without_side_effects() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: Length::ZERO }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid { panel: panel_id, depth: Length::ZERO, thickness: millimetres(18) })
        );
        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: millimetres(18) }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid {
                panel: panel_id,
                depth: millimetres(18),
                thickness: millimetres(18)
            })
        );
        let outside = CutoutShape::Rectangle(sample_rect(750, 10, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, outside, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let first = project
            .add_cutout(panel_id, shape, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::B)
            .expect("first cutout");
        assert_eq!(first.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);

        let overlapping = CutoutShape::Rectangle(sample_rect(50, 20, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: first })
        );
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        assert_eq!(project.panel(panel_id).expect("panel exists").machining().len(), 1);
    }

    #[test]
    fn overlapping_through_cutouts_and_boundary_notches_are_accepted() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let first = CutoutShape::Rectangle(sample_rect(100, 100, 100, 50));
        let second = CutoutShape::Rectangle(sample_rect(150, 120, 100, 50));
        // The 700 mm core ends at x = 700, so this cutout opens a notch in the right edge.
        let notch = CutoutShape::Rectangle(sample_rect(650, 10, 100, 50));

        project.add_cutout(panel_id, first, CutoutDepth::Through, FaceSide::A).expect("first through cutout");
        project.add_cutout(panel_id, second, CutoutDepth::Through, FaceSide::A).expect("overlapping through cutout");
        project.add_cutout(panel_id, notch, CutoutDepth::Through, FaceSide::A).expect("boundary notch");

        let panel = project.panel(panel_id).expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(geometry), None);
        assert_eq!(panel.machining().len(), 3);
    }

    #[test]
    fn cutouts_that_split_or_empty_the_core_are_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let strip = CutoutShape::Rectangle(sample_rect(0, 200, 700, 50));
        assert_eq!(
            project.add_cutout(panel_id, strip, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutSplitsCore { panel: panel_id })
        );

        let whole = CutoutShape::Rectangle(sample_rect(0, 0, 700, 500));
        assert_eq!(
            project.add_cutout(panel_id, whole, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutRemovesCore { panel: panel_id })
        );
        assert!(project.panel(panel_id).expect("panel").machining().is_empty());
    }

    #[test]
    fn panel_resize_validates_existing_cutouts() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(600, 10, 50, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");

        let smaller = PanelNominalSize::new(millimetres(500), millimetres(500)).expect("valid size");
        assert_eq!(
            project.validate_panel_resize(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(
            project.set_panel_nominal_size(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), sample_nominal());
        assert_eq!(project.revision().get(), 1);

        let blind_rect = CutoutShape::Rectangle(sample_rect(10, 10, 50, 50));
        let other_panel = Id::new(4).expect("id");
        project
            .add_cutout(other_panel, blind_rect, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::A)
            .expect("blind cutout");
        let material = MaterialId::new(1).expect("material id");
        let revision = project.set_panel_material(other_panel, material, millimetres(5)).expect("material change");
        assert_eq!(revision.get(), 3);
        let panel = project.panel(other_panel).expect("panel exists");
        assert_eq!(panel.material(), Some(material));
        assert_eq!(panel.last_known_thickness(), millimetres(5));
        assert_eq!(panel.geometry(millimetres(5)).expect("resolved geometry").thickness(), millimetres(5));
    }

    #[test]
    fn panels_load_without_machining() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let mut value = serde_json::to_value(&panel).expect("serialize panel");
        value.as_object_mut().expect("panel is an object").remove("machining");
        let decoded: Panel = serde_json::from_value(value).expect("older panel without machining");
        assert_eq!(decoded, panel);
        assert!(decoded.machining().is_empty());
    }

    #[test]
    fn panel_rects_reject_non_positive_dimensions() {
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, Length::ZERO, millimetres(10)),
            Err(PanelRectError::NonPositive { field: "width", value: Length::ZERO })
        );
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, millimetres(10), Length::ZERO),
            Err(PanelRectError::NonPositive { field: "height", value: Length::ZERO })
        );
        assert!(serde_json::from_str::<PanelRect>(r#"{"x":0,"y":0,"width":0,"height":10000}"#).is_err());
    }

    #[test]
    fn cutout_ids_participate_in_next_id_validation() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let text = serde_json::to_string(&project).expect("serialize project");
        let invalid = text.replacen("\"next_id\":6", &format!("\"next_id\":{}", cutout_id.get()), 1);
        let error = serde_json::from_str::<Project>(&invalid).expect_err("next id must exceed cutout id");
        assert!(error.to_string().contains("must be greater"), "error was {error}");
    }

    #[test]
    fn failed_mutations_do_not_consume_ids_or_bump_revisions() {
        let mut project = sample_project();
        let next_id = project.next_id();
        let revision = project.revision();
        let missing = Id::new(999).expect("id");

        assert_eq!(
            project.add_panel(missing, "Orphan", sample_nominal(), None, sample_thickness(), Placement::IDENTITY),
            Err(ProjectError::ContainerNotFound { id: missing })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn projects_reject_assemblies_below_the_depth_limit() {
        let mut nested = Vec::new();
        let mut deepest = None;
        for index in 0..(Project::MAX_ASSEMBLY_DEPTH - 1) {
            let id = Id::new(u64::from(index) + 2).expect("id");
            if deepest.is_none() {
                deepest = Some(id);
            }
            let container = Container::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![container.into()];
        }
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        let deepest = deepest.expect("at least one nested container");
        let mut project = Project::new(root).expect("tree at the depth limit");
        assert_eq!(
            project.add_component(deepest, "Too deep", Placement::IDENTITY),
            Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH })
        );
        assert_eq!(project.revision(), Revision::ZERO);
    }

    #[test]
    fn projects_bump_revisions_explicitly() {
        let mut project = sample_project();
        assert_eq!(project.bump_revision().expect("bump revision").get(), 1);
        assert_eq!(project.revision().get(), 1);
    }

    #[test]
    fn project_restore_keeps_revisions_and_ids_monotonic() {
        let original = sample_project();
        let mut project = sample_project();
        let created = project
            .add_panel(project.root().id(), "Back", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("add panel");
        let after = project.clone();

        let revision = project.restore_from(&original).expect("restore original");
        assert_eq!(revision.get(), 2);
        assert_eq!(project.root().children().len(), 2);
        assert!(project.next_id().get() > created.get());
        assert!(project.same_content(&original));
        assert!(!project.same_content(&after));

        let revision = project.restore_from(&after).expect("restore edited");
        assert_eq!(revision.get(), 3);
        assert_eq!(project.root().children().len(), 3);
        assert_eq!(project.next_id(), after.next_id());
    }

    #[test]
    fn projects_round_trip_through_json() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn reserved_property_keys_are_validated_on_load() {
        let invalid = r#"{
            "schema_version": 7,
            "revision": 0,
            "next_id": 2,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [],
                "properties": {"component": {"kind": "text", "value": "yes"}}
            }
        }"#;
        let error = serde_json::from_str::<Project>(invalid).expect_err("invalid reserved property");
        assert!(error.to_string().contains("must be a boolean"), "error was {error}");
    }

    #[test]
    fn projects_load_without_next_id() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let mut value: serde_json::Value = serde_json::from_str(&text).expect("parse project");
        value.as_object_mut().expect("project is an object").remove("next_id");
        let decoded: Project = serde_json::from_value(value).expect("older v1 project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn projects_reject_next_id_at_or_below_the_largest_id() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let invalid = text.replacen("\"next_id\":5", "\"next_id\":4", 1);
        let error = serde_json::from_str::<Project>(&invalid).expect_err("invalid next id");
        assert!(error.to_string().contains("must be greater"), "error was {error}");
    }

    #[test]
    fn projects_reject_duplicate_ids() {
        let duplicate = Id::new(1).expect("id");
        let panel = sample_panel(duplicate, "Panel");
        let root = Container::new(duplicate, "Root", Placement::IDENTITY, vec![panel.into()]);
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(duplicate)));
    }

    #[test]
    fn projects_reject_excessive_nesting() {
        let mut nested = Vec::new();
        for index in 0..Project::MAX_ASSEMBLY_DEPTH {
            let id = Id::new(u64::from(index) + 2).expect("id");
            let container = Container::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![container.into()];
        }
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        assert!(matches!(
            Project::new(root),
            Err(ProjectError::TooDeep { max }) if max == Project::MAX_ASSEMBLY_DEPTH
        ));
    }

    #[test]
    fn projects_reject_unsupported_schema_and_unknown_fields() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");

        let newer = text.replacen(
            &format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION),
            &format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION + 1),
            1,
        );
        let error = serde_json::from_str::<Project>(&newer).expect_err("unsupported schema");
        assert!(error.to_string().contains("unsupported schema version"), "error was {error}");

        let prefix = text.strip_suffix('}').expect("project JSON is an object");
        let extra = format!("{prefix},\"extra\":1}}");
        assert!(serde_json::from_str::<Project>(&extra).is_err());
    }

    #[test]
    fn version_one_panels_migrate_to_nominal_geometry() {
        let legacy = r#"{
            "schema_version": 1,
            "revision": 0,
            "next_id": 3,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Shelf",
                    "size": {"length": 700000, "width": 500000, "thickness": 18000},
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version one");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = project.panel(Id::new(2).expect("id")).expect("migrated panel");
        assert_eq!(panel.nominal_size(), sample_nominal());
        assert_eq!(panel.material(), None);
        assert_eq!(panel.last_known_thickness(), millimetres(18));
        assert!(panel.edge_banding().is_empty());
        assert!(project.validate().is_ok());
    }

    #[test]
    fn version_four_root_assembly_loads_as_a_group() {
        let legacy = r#"{
            "schema_version": 4,
            "revision": 2,
            "next_id": 4,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"assembly": {
                    "id": 2,
                    "name": "Drawer",
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "children": [{"panel": {
                        "id": 3,
                        "name": "Front",
                        "nominal_size": {"length": 500000, "width": 200000},
                        "last_known_thickness": 18000,
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                    }}]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version four");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let root = Id::new(1).expect("root id");
        let drawer = Id::new(2).expect("drawer id");
        assert!(project.container(root).is_some());
        assert!(!project.is_component(root));
        assert!(project.is_component(drawer));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.pivot(root), Some(Pivot::ZERO));
        assert_eq!(
            project.pivot(drawer),
            Some(Pivot::new(
                Length::from_micrometres(250_000),
                Length::from_micrometres(100_000),
                Length::from_micrometres(9_000),
            )),
            "an older file materializes the centred container default"
        );
        assert_eq!(project.pivot(Id::new(3).expect("panel id")), Some(Pivot::ZERO));
        assert!(project.validate().is_ok());
    }

    #[test]
    fn version_two_machining_migrates_from_core_to_nominal_coordinates() {
        let legacy = r#"{
            "schema_version": 2,
            "revision": 4,
            "next_id": 6,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Panel",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "last_known_thickness": 18000,
                    "machining": [{"cutout": {
                        "id": 5,
                        "shape": {"rectangle": {"x": 1000, "y": 1000, "width": 10000, "height": 10000}},
                        "depth": "through",
                        "entry": "A"
                    }}],
                    "edge_banding": [
                        {
                            "edge": 1,
                            "material": 1,
                            "included_in_size": true,
                            "last_known_thickness": 2000,
                            "last_known_width": 22000
                        },
                        {
                            "edge": 4,
                            "material": 1,
                            "included_in_size": true,
                            "last_known_thickness": 2000,
                            "last_known_width": 22000
                        }
                    ]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version two");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = project.panel(Id::new(2).expect("id")).expect("migrated panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));

        let text = serde_json::to_string(&project).expect("serialize migrated project");
        let decoded: Project = serde_json::from_str(&text).expect("reload migrated project");
        let panel = decoded.panel(Id::new(2).expect("id")).expect("reloaded panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));
    }

    #[test]
    fn cutout_coordinates_are_nominal_and_do_not_move_with_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        for edge in [1, 4] {
            let band =
                EdgeBanding::new(EdgeId::new(edge).expect("edge id"), material, true, millimetres(2), millimetres(22))
                    .expect("valid banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        let geometry = project.panel(panel_id).expect("panel").geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));

        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 20, 20));
        project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let panel = project.panel(panel_id).expect("panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &shape);

        // Nominal coordinates: the 2 mm included bands put the core origin at (2, 2), so this
        // cutout lies entirely in the band and would remove no core material.
        let under_the_band = CutoutShape::Rectangle(sample_rect(0, 0, 1, 1));
        assert_eq!(
            project.add_cutout(panel_id, under_the_band, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    /// A rectangular ring around `(x, y)` of `width` x `height` millimetres.
    fn ring(x: i64, y: i64, width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(x), millimetres(y)],
            [millimetres(x + width), millimetres(y)],
            [millimetres(x + width), millimetres(y + height)],
            [millimetres(x), millimetres(y + height)],
        ])
        .expect("ring")
    }

    #[test]
    fn contour_cutout_is_stored_and_round_trips() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Contour(ring(100, 100, 100, 60));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add contour cutout");
        let panel = project.panel(panel_id).expect("panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
        let panel = decoded.panel(panel_id).expect("reloaded panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &shape);
    }

    #[test]
    fn contour_cutout_must_stay_inside_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let next_id = project.next_id();
        let revision = project.revision();

        // A through ring must be strictly inside: touching the edge could open a notch the
        // rectangle-only core trace cannot see.
        for shape in [ring(-10, 100, 120, 60), ring(0, 100, 100, 60), ring(690, 100, 20, 60)] {
            assert_eq!(
                project.add_cutout(panel_id, CutoutShape::Contour(shape), CutoutDepth::Through, FaceSide::A),
                Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
            );
        }
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        // A blind ring may open at the edge like a blind rectangle, but it may not cross it.
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(0, 100, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("blind contour cutout at the edge");
        assert_eq!(
            project.add_cutout(
                panel_id,
                CutoutShape::Contour(ring(-10, 300, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::B,
            ),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn contour_cutout_overlap_follows_the_ring_not_its_bounds() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let blind = CutoutDepth::Blind { depth: millimetres(8) };
        project
            .add_cutout(panel_id, CutoutShape::Rectangle(sample_rect(100, 100, 50, 50)), blind, FaceSide::A)
            .expect("rect cutout");

        let overlapping = CutoutShape::Contour(ring(120, 120, 100, 60));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: Id::new(5).expect("id") })
        );

        // A C-shaped ring around the rectangle's bounding box leaves the notch free.
        let c_shape = Contour::from_polygon(vec![
            [millimetres(50), millimetres(50)],
            [millimetres(250), millimetres(50)],
            [millimetres(250), millimetres(90)],
            [millimetres(90), millimetres(90)],
            [millimetres(90), millimetres(210)],
            [millimetres(250), millimetres(210)],
            [millimetres(250), millimetres(250)],
            [millimetres(50), millimetres(250)],
        ])
        .expect("C-shaped ring");
        project
            .add_cutout(panel_id, CutoutShape::Contour(c_shape), blind, FaceSide::A)
            .expect("a ring that surrounds the bounds but not the rectangle");

        // Two contours are compared the same way: a crossing ring is an overlap, a nested one is
        // not (a through pair may overlap, so use blind here).
        let crossing = CutoutShape::Contour(ring(200, 220, 100, 60));
        assert!(matches!(
            project.add_cutout(panel_id, crossing, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel, .. }) if panel == panel_id
        ));
        let nested = CutoutShape::Contour(ring(160, 120, 20, 20));
        project.add_cutout(panel_id, nested, blind, FaceSide::A).expect("a ring inside the C notch");
    }

    #[test]
    fn contour_cutout_is_refused_when_a_resize_moves_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(100, 100, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("contour cutout");
        let small = PanelNominalSize::new(millimetres(150), millimetres(500)).expect("small size");
        assert_eq!(
            project.validate_panel_resize(panel_id, small),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn an_older_schema_without_a_contour_cutout_still_loads() {
        let project = sample_project();
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        wire["schema_version"] = serde_json::json!(13);
        let decoded: Project = serde_json::from_value(wire).expect("load a schema 13 file");
        assert_eq!(decoded.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        assert_eq!(decoded, project);
    }

    #[test]
    fn nodes_can_be_renamed_and_placed() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::from_rotation_z(0.5)).expect("placement");

        assert_eq!(project.rename_node(panel, "Top").expect("rename panel").get(), 1);
        assert_eq!(project.rename_node(drawer, "Drawer box").expect("rename assembly").get(), 2);
        assert_eq!(project.rename_node(root, "Cabinet").expect("rename root").get(), 3);
        assert_eq!(project.panel(panel).expect("panel").name(), "Top");
        assert_eq!(project.container(drawer).expect("assembly").name(), "Drawer box");
        assert_eq!(project.root().name(), "Cabinet");

        assert_eq!(project.set_node_placement(panel, placement).expect("place panel").get(), 4);
        assert_eq!(project.set_node_placement(drawer, placement).expect("place assembly").get(), 5);
        assert_eq!(project.panel(panel).expect("panel").placement(), placement);
        assert_eq!(project.container(drawer).expect("assembly").placement(), placement);

        assert_eq!(project.set_node_placement(root, placement), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision().get(), 5);
    }

    #[test]
    fn placement_batches_validate_before_mutating_and_bump_once() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");
        let moved = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let raised = Placement::new(DVec3::new(0.0, 0.0, 25.0), DQuat::IDENTITY).expect("placement");
        let revision = project.revision().get();

        assert_eq!(project.set_node_placements(&[]), Err(ProjectError::NoPlacements));
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (panel, raised)]),
            Err(ProjectError::DuplicateNode { id: panel })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (root, raised)]),
            Err(ProjectError::RootImmutable { id: root })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (missing, raised)]),
            Err(ProjectError::NodeNotFound { id: missing })
        );
        assert_eq!(project.revision().get(), revision);
        assert_eq!(project.panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert_eq!(project.container(drawer).expect("assembly").placement(), Placement::IDENTITY);

        assert_eq!(
            project.set_node_placements(&[(panel, moved), (drawer, raised)]).expect("batch").get(),
            revision + 1
        );
        assert_eq!(project.panel(panel).expect("panel").placement(), moved);
        assert_eq!(project.container(drawer).expect("assembly").placement(), raised);
        assert_eq!(project.panel(front).expect("front").placement(), Placement::IDENTITY);
    }

    #[test]
    fn top_most_nodes_drop_descendants_repeats_and_unknown_ids() {
        let project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");

        assert!(project.top_most_nodes(&[]).is_empty());
        assert_eq!(project.top_most_nodes(&[front, panel]), vec![front, panel]);
        assert_eq!(project.top_most_nodes(&[panel, drawer, front]), vec![panel, drawer]);
        assert_eq!(project.top_most_nodes(&[front, drawer]), vec![drawer]);
        assert_eq!(project.top_most_nodes(&[panel, panel, missing]), vec![panel]);
        assert_eq!(project.top_most_nodes(&[root, drawer, panel]), vec![root]);
    }

    #[test]
    fn removing_a_node_cascades_and_protects_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let next_id = project.next_id();

        assert_eq!(project.remove_node(drawer).expect("remove assembly").get(), 1);
        assert!(project.container(drawer).is_none());
        assert!(project.panel(front).is_none());
        assert!(project.panel(panel).is_some());
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.next_id(), next_id);

        assert_eq!(project.remove_node(root), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(
            project.remove_node(Id::new(999).expect("id")),
            Err(ProjectError::NodeNotFound { id: Id::new(999).expect("id") })
        );
        assert_eq!(project.revision().get(), 1);
    }

    #[test]
    fn duplicating_a_node_remaps_every_id() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");

        let copy = project.duplicate_node(panel).expect("duplicate panel");
        assert_eq!(copy, Id::new(5).expect("id"));
        let copied_panel = project.panel(copy).expect("copied panel");
        assert_eq!(copied_panel.name(), "Shelf copy");
        assert_eq!(copied_panel.nominal_size(), project.panel(panel).expect("panel").nominal_size());
        assert_eq!(project.root().children().len(), 3);

        let copied_drawer = project.duplicate_node(drawer).expect("duplicate assembly");
        assert_eq!(copied_drawer, Id::new(6).expect("id"));
        let assembly = project.container(copied_drawer).expect("copied assembly");
        assert_eq!(assembly.name(), "Drawer copy");
        assert_eq!(assembly.children().len(), 1);
        let copied_front = assembly.children().first().expect("copied child").id();
        assert_ne!(copied_front, front);
        assert_eq!(project.node(copied_front).expect("child").name(), "Front");
        assert_eq!(project.parent_of(copied_front), Some(copied_drawer));
        project.validate().expect("ids stay unique");
        assert_eq!(project.root().id(), Id::new(1).expect("id"));
        assert_eq!(
            project.duplicate_node(project.root().id()),
            Err(ProjectError::RootImmutable { id: project.root().id() })
        );
    }

    #[test]
    fn grouping_siblings_preserves_order_and_world_placements() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(0.0, 20.0, 0.0), DQuat::from_rotation_z(0.25)).expect("drawer placement");
        let front_placement = Placement::new(DVec3::new(5.0, 0.0, 0.0), DQuat::IDENTITY).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");
        let before = Placement::compose(drawer_placement, front_placement).expect("world before");
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel, front], "Nested"), Err(ProjectError::NodesNotSiblings));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        assert_eq!(group_id, Id::new(5).expect("id"));
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.root().children().first().expect("group").id(), group_id);
        let group = project.container(group_id).expect("group");
        assert_eq!(group.placement(), Placement::IDENTITY);
        assert_eq!(group.children().len(), 2);
        assert_eq!(group.children()[0].id(), panel);
        assert_eq!(group.children()[1].id(), drawer);
        assert_eq!(project.parent_of(panel), Some(group_id));
        assert_eq!(project.parent_of(drawer), Some(group_id));
        assert_eq!(project.panel(panel).expect("panel").placement(), panel_placement);
        assert_eq!(project.container(drawer).expect("drawer").placement(), drawer_placement);
        let after = Placement::compose(drawer_placement, front_placement).expect("world after");
        let point = Placement::compose(Placement::IDENTITY, after).expect("root world");
        let point = point.transform_point(DVec3::ZERO);
        let expected = before.transform_point(DVec3::ZERO);
        assert!((point - expected).length() < 1e-12, "point was {point:?}, expected {expected:?}");
        assert_eq!(project.revision().get(), revision.get() + 1);
    }

    #[test]
    fn grouping_rejects_duplicates_a_short_selection_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel], "Group"), Err(ProjectError::TooFewNodes { minimum: 2, found: 1 }));
        assert_eq!(project.group_nodes(&[panel, panel], "Group"), Err(ProjectError::DuplicateNode { id: panel }));
        assert_eq!(project.group_nodes(&[root, panel], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.group_nodes(&[root, root], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn ungrouping_flattens_children_and_keeps_their_world_placements() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::from_rotation_z(0.5)).expect("drawer placement");
        let front_placement =
            Placement::new(DVec3::new(0.0, 5.0, 0.0), DQuat::from_rotation_x(0.25)).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        let group_placement =
            Placement::new(DVec3::new(50.0, 60.0, 70.0), DQuat::from_rotation_y(0.3)).expect("group placement");
        project.set_node_placement(group_id, group_placement).expect("place group");
        let revision = project.revision();

        assert_eq!(project.ungroup_group(group_id).expect("ungroup").get(), revision.get() + 1);
        assert!(project.container(group_id).is_none());
        assert_eq!(project.parent_of(panel), Some(root));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.parent_of(front), Some(drawer));
        assert_eq!(project.root().children().len(), 2);

        let expected_panel = Placement::compose(group_placement, panel_placement).expect("panel world");
        assert_eq!(project.panel(panel).expect("panel").placement(), expected_panel);
        let expected_drawer = Placement::compose(group_placement, drawer_placement).expect("drawer world");
        assert_eq!(project.container(drawer).expect("drawer").placement(), expected_drawer);
        let expected_front = Placement::compose(expected_drawer, front_placement).expect("front world");
        assert_eq!(project.world_placement(front).expect("front world"), expected_front);
        assert_eq!(project.panel(front).expect("front").placement(), front_placement);

        assert_eq!(project.ungroup_group(drawer), Err(ProjectError::NotAComponent { id: drawer }));
        assert_eq!(project.ungroup_group(panel), Err(ProjectError::NotAnOrdinaryContainer { id: panel }));
        assert_eq!(project.ungroup_group(root), Err(ProjectError::RootImmutable { id: root }));
    }

    #[test]
    fn projects_set_and_clear_properties_with_one_revision_per_batch() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let drawer = Id::new(3).expect("drawer");
        let revision = project.revision();

        let next = project.set_property(panel, "material.finish", PropertyValue::text("oak")).expect("set property");
        assert_eq!(next.get(), revision.get() + 1);
        assert_eq!(project.properties(panel).and_then(|p| p.get("material.finish")), Some(&PropertyValue::text("oak")));

        let next = project
            .set_properties(&[
                PropertyChange { node: panel, key: "edge".to_owned(), value: PropertyValue::integer(2) },
                PropertyChange { node: drawer, key: "drawer.count".to_owned(), value: PropertyValue::integer(3) },
            ])
            .expect("batch");
        assert_eq!(next.get(), revision.get() + 2);
        assert_eq!(project.revision(), next);
        assert_eq!(project.properties(drawer).and_then(|p| p.get("drawer.count")), Some(&PropertyValue::integer(3)));
    }

    #[test]
    fn project_property_batches_reject_duplicates_unknown_nodes_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel");
        let missing = Id::new(99).expect("id");
        let revision = project.revision();
        let change = |node, key: &str| PropertyChange { node, key: key.to_owned(), value: PropertyValue::integer(1) };

        assert_eq!(project.set_properties(&[]), Err(ProjectError::NoPropertyChanges));
        assert_eq!(
            project.set_properties(&[change(panel, "a"), change(panel, "a")]),
            Err(ProjectError::DuplicateProperty { node: panel, key: "a".to_owned() })
        );
        assert_eq!(project.set_properties(&[change(missing, "a")]), Err(ProjectError::NodeNotFound { id: missing }));
        assert_eq!(project.set_properties(&[change(root, "a")]), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision(), revision);

        assert_eq!(
            project.clear_property(panel, "missing"),
            Err(ProjectError::PropertyNotFound { node: panel, key: "missing".to_owned() })
        );
        assert_eq!(project.revision(), revision);
        project.set_property(panel, "a", PropertyValue::integer(1)).expect("set");
        assert_eq!(project.clear_property(panel, "a").expect("clear").get(), revision.get() + 2);
        assert!(!project.properties(panel).expect("properties").contains_key("a"));
    }

    #[test]
    fn a_furniture_type_marks_a_container_as_a_component() {
        let mut project = sample_project();
        let root = project.root().id();
        let group = project.group_nodes(&[Id::new(2).expect("id"), Id::new(3).expect("id")], "G").expect("group");
        assert!(!project.is_component(group));

        project.set_property(group, SYSTEM_FURNTYPE, PropertyValue::furn_type(furntype("12.03.02"))).expect("type");
        assert!(project.is_component(group));
        project.set_property(group, SYSTEM_COMPONENT, PropertyValue::bool(false)).expect("false marker");
        assert!(project.is_component(group), "an explicit false does not erase the furniture type");
        assert_eq!(project.furntype(group), Some(furntype("12.03.02")));
        assert_eq!(project.furntype(root), Some(FurnType::UNCLASSIFIED));

        assert_eq!(project.ungroup_group(group), Err(ProjectError::NotAComponent { id: group }));

        project.convert_to_group(group).expect("convert to group");
        assert!(project.container(group).is_none());
        assert!(project.properties(Id::new(2).expect("id")).is_some());
    }

    #[test]
    fn queries_select_by_kind_furniture_type_and_property() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let front = Id::new(4).expect("front");
        let drawer = Id::new(3).expect("drawer");
        project.set_property(drawer, SYSTEM_FURNTYPE, PropertyValue::furn_type(furntype("12.03.02"))).expect("type");
        project.set_property(panel, "role", PropertyValue::text("shelf")).expect("role");

        assert_eq!(project.query(&NodeQuery::any()), vec![panel, drawer, front]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Container)), vec![drawer]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Panel)), vec![panel, front]);
        assert_eq!(
            project.query(&NodeQuery::any().with_furntype_under(furntype("12"))),
            vec![drawer],
            "a query on a group matches every type below it"
        );
        assert_eq!(project.query(&NodeQuery::any().with_furntype_under(furntype("12.03.02"))), vec![drawer]);
        assert!(project.query(&NodeQuery::any().with_furntype_under(furntype("13"))).is_empty());
        assert_eq!(
            project.query(&NodeQuery::any().with_furntype_under(FurnType::UNCLASSIFIED)),
            vec![panel, front],
            "unclassified is a sentinel, not the root of the type tree"
        );
        assert_eq!(project.query(&NodeQuery::any().with_property("role", PropertyValue::text("shelf"))), vec![panel]);
        assert!(
            project
                .query(
                    &NodeQuery::any()
                        .with_kind(StructuralKind::Panel)
                        .with_property("role", PropertyValue::text("shelf"))
                )
                .len()
                == 1
        );
    }

    #[test]
    fn version_six_containers_migrate_to_one_container_kind() {
        let legacy = r#"{
            "schema_version": 6,
            "revision": 3,
            "next_id": 4,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [
                    {"assembly": {
                        "id": 2,
                        "name": "Drawer",
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                        "children": []
                    }},
                    {"group": {
                        "id": 3,
                        "name": "Parts",
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                        "children": []
                    }}
                ]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version six");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let drawer = Id::new(2).expect("id");
        let parts = Id::new(3).expect("id");
        assert!(project.is_component(drawer));
        assert!(!project.is_component(parts));
        assert_eq!(project.container(drawer).expect("drawer").name(), "Drawer");
        assert_eq!(project.container(parts).expect("parts").name(), "Parts");

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"container\""));
        assert!(!text.contains("\"assembly\""));
        assert!(!text.contains("\"group\":"));
        let decoded: Project = serde_json::from_str(&text).expect("round trip");
        assert_eq!(decoded, project);
    }
}
