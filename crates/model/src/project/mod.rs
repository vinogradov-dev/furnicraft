//! The saved document: schema version, revision, id allocation and the container tree.
//!
//! This module is the composition point of the document. It owns [`Project`] and the core
//! operations that every other command builds on; the serialized envelope lives in `wire`, the
//! errors in `error` and the read-only lookups in `query`. Commands are grouped by the value family
//! they mutate.

use std::collections::HashSet;

use furnicraft_geom::DVec3;
use furnicraft_units::Length;
use serde::{Deserialize, Serialize};

mod error;
mod query;
mod wire;

pub use error::ProjectError;
pub(crate) use wire::ProjectWire;

use crate::contour::{MAX_SELF_INTERSECTION_PAIRS, SELF_INTERSECTION_TOLERANCE_MICROMETRES, SegBounds, segments_touch};
use crate::light::{valid_light_color, valid_light_intensity};
use crate::panel::{core_violation_of, rects_overlap, validate_ring_banding};
use crate::properties::validate_property_value;
use crate::tree::{
    assign_new_ids, count_nodes, find_container, find_container_mut, find_node_mut, find_panel_mut, node_height,
    remove_child,
};
use crate::*;

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
