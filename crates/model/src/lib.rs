//! The FurniCraft document: assemblies, panels, machining, hardware, materials and dimensions.
//!
//! Pure serializable data plus its invariants. No geometry, no GPU, no UI, no file I/O. The exact
//! document schema is designed in M1; see `PROJECT.md`, "Open model questions".
//!
//! This module currently defines the document skeleton: ids and revisions, the assembly tree,
//! minimal panel records, validated rectangular dimensions, rigid placements and the external
//! material catalogue. The two large faces, stable edge and vertex ids, and the local numbering of
//! a rectangular outline are here as well; geometry, edge banding and machining arrive later.
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
use std::fmt;
use std::num::NonZeroU64;

use furnicraft_geom::{DQuat, DVec2, DVec3, EulerRot, Rect, regions_from_rects, subtract_rects};
use furnicraft_units::{Degrees, Length};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

mod catalog;
mod panel_contour;
mod panel_features;

pub use catalog::{Catalog, CatalogError, EdgeBandingMaterial, MaterialId, PanelMaterial};
pub use panel_contour::{ContourPath, PanelContourError, SegmentSource};
pub use panel_features::{CornerFeature, CornerFeatureKind, EdgeBow};

/// A stable document identifier.
///
/// Identifiers are never reused while a document is edited, so commands, hardware references and
/// the undo stack can refer to an assembly, panel or catalogue entry without holding a pointer.
/// Zero is reserved for "no entity" and is rejected on construction and deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(NonZeroU64);

impl Id {
    /// Creates an id from a non-zero raw value.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A monotonically increasing document revision.
///
/// The revision changes after every document mutation and keys every derived cache. It starts at
/// zero and never wraps: [`Revision::checked_next`] returns `None` at the end of the range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// The initial revision of an unmodified document.
    pub const ZERO: Self = Self(0);

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next revision, or `None` if the counter would overflow.
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Intrinsic XYZ Euler angles in degrees, the inspector's view of a placement rotation.
///
/// The document stores a unit quaternion; Euler angles are derived on read and never serialized.
/// Each angle is in millidegrees, so text entered in the inspector round-trips exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EulerAngles {
    /// Rotation about the local X axis, in degrees.
    pub x: Degrees,
    /// Rotation about the local Y axis, in degrees.
    pub y: Degrees,
    /// Rotation about the local Z axis, in degrees.
    pub z: Degrees,
}

impl EulerAngles {
    /// The axis order used by [`Placement::rotation_euler`] and [`Placement::with_rotation_euler`].
    pub const ORDER_LABEL: &'static str = "XYZ";

    /// Wraps each angle into `(-180°, 180°]`, the principal range the inspector displays.
    ///
    /// A full turn about one axis is the identity, so wrapping each angle does not change the
    /// rotation the triple describes.
    pub fn wrapped(self) -> Self {
        Self { x: wrap_degrees(self.x), y: wrap_degrees(self.y), z: wrap_degrees(self.z) }
    }
}

fn wrap_degrees(degrees: Degrees) -> Degrees {
    let wrapped = degrees.millidegrees().rem_euclid(360_000);
    Degrees::from_millidegrees(if wrapped > 180_000 { wrapped - 360_000 } else { wrapped })
}

/// A rigid placement relative to a parent frame.
///
/// Placement is translation plus rotation, never scale or shear. Rotations are stored as a unit
/// quaternion; construction and deserialization reject a zero or non-unit quaternion instead of
/// silently normalising it. The serialized rotation is `[x, y, z, w]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PlacementWire", into = "PlacementWire")]
pub struct Placement {
    translation: DVec3,
    rotation: DQuat,
}

impl Placement {
    /// The identity placement: no translation and no rotation.
    pub const IDENTITY: Self = Self { translation: DVec3::ZERO, rotation: DQuat::IDENTITY };

    /// Creates a placement after checking that every component is finite and the rotation is unit.
    pub fn new(translation: DVec3, rotation: DQuat) -> Result<Self, PlacementError> {
        if !translation.is_finite() {
            return Err(PlacementError::NonFiniteTranslation);
        }
        if !rotation.is_finite() {
            return Err(PlacementError::NonFiniteRotation);
        }
        let length = rotation.length();
        if !length.is_finite() || (length - 1.0).abs() > ROTATION_TOLERANCE {
            return Err(PlacementError::NonUnitRotation { length });
        }
        Ok(Self { translation, rotation })
    }

    /// The translation part, in the parent frame.
    pub const fn translation(self) -> DVec3 {
        self.translation
    }

    /// The unit rotation part, in the parent frame.
    pub const fn rotation(self) -> DQuat {
        self.rotation
    }

    /// Composes a child placement into its parent's frame: `parent ∘ child`.
    ///
    /// A point expressed in the child frame is first transformed by `child`, then by `parent`.
    pub fn compose(parent: Self, child: Self) -> Result<Self, PlacementError> {
        let rotation = (parent.rotation * child.rotation).normalize();
        let translation = parent.rotation * child.translation + parent.translation;
        Self::new(translation, rotation)
    }

    /// Applies the placement to a point.
    pub fn transform_point(self, point: DVec3) -> DVec3 {
        self.rotation * point + self.translation
    }

    /// Applies only the rotation to a direction vector.
    pub fn transform_vector(self, vector: DVec3) -> DVec3 {
        self.rotation * vector
    }

    /// The translation as exact micrometre lengths, rounded to the nearest micrometre.
    ///
    /// This is the boundary conversion for editing a placement as integer lengths. It returns
    /// `None` when a component is NaN, infinite or outside the range of [`Length`].
    pub fn translation_lengths(self) -> Option<[Length; 3]> {
        fn to_length(value: f64) -> Option<Length> {
            if !value.is_finite() {
                return None;
            }
            let micrometres = (value * 1_000.0).round();
            if !(micrometres >= i64::MIN as f64 && micrometres < i64::MAX as f64) {
                return None;
            }
            Some(Length::from_micrometres(micrometres as i64))
        }
        Some([to_length(self.translation.x)?, to_length(self.translation.y)?, to_length(self.translation.z)?])
    }

    /// Replaces the translation from exact lengths, keeping the rotation.
    ///
    /// This converts to millimetres at the geometry boundary; callers keep placement coordinates
    /// as [`Length`] until this point.
    pub fn with_translation(self, x: Length, y: Length, z: Length) -> Result<Self, PlacementError> {
        Self::new(DVec3::new(x.millimetres_f64(), y.millimetres_f64(), z.millimetres_f64()), self.rotation)
    }

    /// The rotation as intrinsic XYZ Euler angles, rounded to the nearest millidegree.
    ///
    /// Euler angles are a presentation of the stored unit quaternion; the quaternion remains the
    /// source of truth. At Y = ±90° the X and Z angles are not unique.
    pub fn rotation_euler(self) -> EulerAngles {
        let (x, y, z) = self.rotation.to_euler(EulerRot::XYZ);
        EulerAngles {
            x: Degrees::from_radians(x).unwrap_or_default(),
            y: Degrees::from_radians(y).unwrap_or_default(),
            z: Degrees::from_radians(z).unwrap_or_default(),
        }
        .wrapped()
    }

    /// Replaces the rotation from intrinsic XYZ Euler angles, keeping the translation.
    ///
    /// The quaternion is re-derived from the angles, so values finer than a millidegree are lost;
    /// this is the write side of the inspector. The angles are wrapped to `(-180°, 180°]` first.
    pub fn with_rotation_euler(self, angles: EulerAngles) -> Result<Self, PlacementError> {
        let angles = angles.wrapped();
        let rotation =
            DQuat::from_euler(EulerRot::XYZ, angles.x.radians_f64(), angles.y.radians_f64(), angles.z.radians_f64())
                .normalize();
        Self::new(self.translation, rotation)
    }

    /// Whether two placements describe the same rigid transform.
    ///
    /// Quaternions `q` and `-q` are the same rotation, so a sign-only difference is not a change.
    /// The translation must match exactly; the rotation tolerance is well below the inspector's
    /// millidegree display precision.
    pub fn same_transform(self, other: Self) -> bool {
        self.translation == other.translation && self.rotation.dot(other.rotation).abs() >= 1.0 - 1.0e-12
    }
}

/// Why a placement is not a valid rigid transform.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum PlacementError {
    /// The translation contains a NaN or an infinity.
    #[error("placement translation must be finite")]
    NonFiniteTranslation,
    /// The rotation contains a NaN or an infinity.
    #[error("placement rotation must be finite")]
    NonFiniteRotation,
    /// The rotation quaternion is not unit length.
    #[error("placement rotation must be a unit quaternion, got length {length}")]
    NonUnitRotation {
        /// The length of the rejected quaternion.
        length: f64,
    },
}

/// Accepted deviation from unit length for a rotation quaternion.
const ROTATION_TOLERANCE: f64 = 1e-9;

/// Serialized form of [`Placement`]; validation happens in its `TryFrom` implementation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementWire {
    translation: [f64; 3],
    rotation: [f64; 4],
}

impl From<Placement> for PlacementWire {
    fn from(placement: Placement) -> Self {
        let rotation = placement.rotation;
        Self {
            translation: placement.translation.to_array(),
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
        }
    }
}

impl TryFrom<PlacementWire> for Placement {
    type Error = PlacementError;

    fn try_from(wire: PlacementWire) -> Result<Self, Self::Error> {
        let translation = DVec3::from_array(wire.translation);
        let rotation = DQuat::from_xyzw(wire.rotation[0], wire.rotation[1], wire.rotation[2], wire.rotation[3]);
        Self::new(translation, rotation)
    }
}

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
    root: Group,
}

impl Project {
    /// The schema version written by this build.
    ///
    /// Version 3 moved panel-owned machining from core-local to nominal panel-local coordinates.
    /// Version 5 splits assembly components from organisational groups.
    pub const CURRENT_SCHEMA_VERSION: u32 = 5;

    /// Maximum nesting depth of containers, including the root at depth one.
    pub const MAX_ASSEMBLY_DEPTH: u32 = 64;

    /// Creates a project from a root group and validates its tree.
    pub fn new(root: Group) -> Result<Self, ProjectError> {
        let max_id = scan_tree(&root)?;
        let next_id = next_id_after(max_id)?;
        Ok(Self { schema_version: Self::CURRENT_SCHEMA_VERSION, revision: Revision::ZERO, next_id, root })
    }

    /// The schema version stored in the document.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// The current document revision.
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// The next id that [`Project::add_panel`] or [`Project::add_assembly`] will allocate.
    pub const fn next_id(&self) -> Id {
        self.next_id
    }

    /// The root group.
    ///
    /// The root is an organisational container, not a furniture component. It defines the world
    /// frame and cannot be deleted, moved, grouped, ungrouped or dissolved.
    pub const fn root(&self) -> &Group {
        &self.root
    }

    /// Finds an assembly component anywhere in the tree.
    pub fn assembly(&self, id: Id) -> Option<&Assembly> {
        match self.container(id)? {
            ContainerRef::Assembly(assembly) => Some(assembly),
            ContainerRef::Group(_) => None,
        }
    }

    /// Finds a group anywhere in the tree, including the root.
    pub fn group(&self, id: Id) -> Option<&Group> {
        match self.container(id)? {
            ContainerRef::Group(group) => Some(group),
            ContainerRef::Assembly(_) => None,
        }
    }

    /// Whether the id names an assembly, a group, or a panel.
    pub fn is_container(&self, id: Id) -> bool {
        self.container(id).is_some()
    }

    /// Finds a panel anywhere in the tree.
    pub fn panel(&self, id: Id) -> Option<&Panel> {
        find_panel_with_depth(&self.root, id, 1).map(|(_, panel)| panel)
    }

    /// Finds a panel, group or nested assembly below the root.
    ///
    /// The root group is not a [`Node`]; use [`Project::group`] with the root id to reach it.
    pub fn node(&self, id: Id) -> Option<&Node> {
        find_node(&self.root, id)
    }

    /// The id of the container that directly owns `id`, or `None` for the root and unknown ids.
    pub fn parent_of(&self, id: Id) -> Option<Id> {
        find_parent(&self.root, id)
    }

    /// Whether the id names a container or a panel.
    pub fn contains_node(&self, id: Id) -> bool {
        self.is_container(id) || self.panel(id).is_some()
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
        if self.root.id == id {
            return Some(root_world);
        }
        for child in self.root.children() {
            if let Some(found) = walk(child, id, root_world, 1) {
                return Some(found);
            }
        }
        None
    }

    /// The ids of the panels at or below `id`, in scene order.
    ///
    /// A panel yields itself; a container yields every panel in its subtree. Returns `None` for an
    /// unknown id. The list is empty for a container with no panels.
    pub fn panel_ids_under(&self, id: Id) -> Option<Vec<Id>> {
        fn collect(children: &[Node], ids: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                match child {
                    Node::Panel(panel) => ids.push(panel.id()),
                    Node::Assembly(assembly) => collect(&assembly.children, ids, depth.saturating_add(1)),
                    Node::Group(group) => collect(&group.children, ids, depth.saturating_add(1)),
                }
            }
        }

        if self.panel(id).is_some() {
            return Some(vec![id]);
        }
        let container = self.container(id)?;
        let mut ids = Vec::new();
        collect(container.children(), &mut ids, 0);
        Some(ids)
    }

    /// The manipulation pivot in the node's own frame.
    ///
    /// A panel's pivot is its local zero at the minimum corner of the nominal rectangle. A container's
    /// pivot is the minimum corner of the local axis-aligned box around the nominal boxes of the panels
    /// below it. An empty container falls back to its own origin. Because the result is expressed in
    /// the node's frame, the pivot moves and turns with the node instead of being recomputed from a
    /// world-axis-aligned box after every edit.
    pub fn pivot_local(&self, id: Id) -> Option<DVec3> {
        if self.panel(id).is_some() {
            return Some(DVec3::ZERO);
        }
        let (_, container) = find_container(&self.root, id, 0)?;
        Some(panel_corner_min(container.children(), Placement::IDENTITY, 0).unwrap_or(DVec3::ZERO))
    }

    /// The manipulation pivot in world space: [`Self::pivot_local`] through the node's world placement.
    ///
    /// Both the translate and the rotate gizmo draw and rotate around this point. Returns `None` for
    /// an unknown id.
    pub fn pivot_world(&self, id: Id) -> Option<DVec3> {
        let local = self.pivot_local(id)?;
        Some(self.world_placement(id)?.transform_point(local))
    }

    /// The container with the given id, including the root group.
    fn container(&self, id: Id) -> Option<ContainerRef<'_>> {
        find_container(&self.root, id, 1).map(|(_, container)| container)
    }

    /// Checks that `nodes` can be wrapped in one new group.
    ///
    /// Grouping needs at least two distinct non-root nodes that share one parent container, and the
    /// new level must stay inside [`Project::MAX_ASSEMBLY_DEPTH`].
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
            if id == self.root.id {
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
        panel.nominal_size = nominal_size;
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
        panel.name = name.into();
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
        panel.material = Some(material);
        panel.last_known_thickness = thickness;
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

    /// Checks whether a cutout can be added to a panel without changing the document.
    pub fn validate_cutout(&self, panel: Id, shape: CutoutShape, depth: CutoutDepth) -> Result<(), ProjectError> {
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
        self.validate_cutout(panel, shape, depth)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let id = self.allocate_id()?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        panel_node.machining.push(Machining::Cutout(Cutout::new(id, shape, depth, entry)));
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
    /// Panels may live in a group as well as in an assembly; a group is only an organisational
    /// container. The panel id is allocated from the document; callers must not invent one.
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

    /// Adds an empty assembly component and bumps the revision once.
    ///
    /// The parent may be the root or any nested container. Adding a child below
    /// [`Project::MAX_ASSEMBLY_DEPTH`] is rejected.
    pub fn add_assembly(
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
        let assembly = Assembly::new(id, name, placement, Vec::new());
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Assembly(assembly));
        self.revision = next_revision;
        Ok(id)
    }

    /// Renames any node, including the root group, and bumps the revision once.
    ///
    /// Callers validate that the name is not blank; the model keeps user-visible text verbatim.
    pub fn rename_node(&mut self, id: Id, name: impl Into<String>) -> Result<Revision, ProjectError> {
        let name = name.into();
        let next_revision = self.next_revision()?;
        if id == self.root.id {
            self.root.name = name;
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

    /// Replaces a panel's, group's or assembly's placement and bumps the revision once.
    ///
    /// The root group defines the world frame and cannot be moved. The placement stays relative to
    /// the parent container, so moving a container moves all of its leaves.
    pub fn set_node_placement(&mut self, id: Id, placement: Placement) -> Result<Revision, ProjectError> {
        if id == self.root.id {
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

    /// Removes a panel or a container, with its children, and bumps the revision once.
    ///
    /// The root cannot be removed. Removed ids are never reused.
    pub fn remove_node(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id {
            return Err(ProjectError::RootImmutable { id });
        }
        if self.node(id).is_none() {
            return Err(ProjectError::NodeNotFound { id });
        }
        let next_revision = self.next_revision()?;
        if !remove_child(&mut self.root, id) {
            return Err(ProjectError::NodeNotFound { id });
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
        if id == self.root.id {
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
        rest.insert(index, Node::Group(Group::new(id, name, Placement::IDENTITY, moved)));
        *children = rest;
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces a group with its children and bumps the revision once.
    ///
    /// The children keep their world transforms: each child placement is composed with the removed
    /// group's placement. An assembly is a furniture component with its own identity and cannot be
    /// ungrouped by this command; a future dissolve command handles that separately. Panels are
    /// atomic leaves and cannot be ungrouped. The root group cannot be ungrouped.
    pub fn ungroup_group(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id {
            return Err(ProjectError::RootImmutable { id });
        }
        let node = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let Node::Group(group) = node else {
            return Err(ProjectError::NotAGroup { id });
        };
        let parent_id = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
        let next_revision = self.next_revision()?;
        let mut children = Vec::with_capacity(group.children.len());
        for mut child in group.children {
            let placement = Placement::compose(group.placement, child.placement())
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
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Compares user-visible content, ignoring the revision and the id allocator.
    pub fn same_content(&self, other: &Project) -> bool {
        self.schema_version == other.schema_version && self.root == other.root
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
/// A borrowed container, used by tree walks that treat assemblies and groups alike.
#[derive(Clone, Copy)]
enum ContainerRef<'a> {
    Assembly(&'a Assembly),
    Group(&'a Group),
}

impl<'a> ContainerRef<'a> {
    const fn id(self) -> Id {
        match self {
            Self::Assembly(assembly) => assembly.id,
            Self::Group(group) => group.id,
        }
    }

    fn children(self) -> &'a [Node] {
        match self {
            Self::Assembly(assembly) => &assembly.children,
            Self::Group(group) => &group.children,
        }
    }
}

/// Validates the tree and returns the largest id it uses.
fn scan_tree(root: &Group) -> Result<u64, ProjectError> {
    let mut seen = HashSet::new();
    let mut max_id = 0u64;
    let mut stack = vec![(ContainerRef::Group(root), 1u32)];
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
                Node::Assembly(assembly) => {
                    stack.push((ContainerRef::Assembly(assembly), depth.saturating_add(1)));
                }
                Node::Group(group) => {
                    stack.push((ContainerRef::Group(group), depth.saturating_add(1)));
                }
                Node::Panel(panel) => {
                    if !seen.insert(panel.id.get()) {
                        return Err(ProjectError::DuplicateId(panel.id));
                    }
                    max_id = max_id.max(panel.id.get());
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

/// Finds any container, including the root group.
fn find_container(root: &Group, id: Id, depth: u32) -> Option<(u32, ContainerRef<'_>)> {
    if root.id == id {
        return Some((depth, ContainerRef::Group(root)));
    }
    find_container_in(&root.children, id, depth.saturating_add(1))
}

fn find_container_in(children: &[Node], id: Id, depth: u32) -> Option<(u32, ContainerRef<'_>)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        match child {
            Node::Assembly(assembly) => {
                if assembly.id == id {
                    return Some((depth, ContainerRef::Assembly(assembly)));
                }
                if let Some(found) = find_container_in(&assembly.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Group(group) => {
                if group.id == id {
                    return Some((depth, ContainerRef::Group(group)));
                }
                if let Some(found) = find_container_in(&group.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
}

/// Finds any container's child list for mutation.
fn find_container_mut(root: &mut Group, id: Id, depth: u32) -> Option<(u32, &mut Vec<Node>)> {
    if root.id == id {
        return Some((depth, &mut root.children));
    }
    find_container_in_mut(&mut root.children, id, depth.saturating_add(1))
}

fn find_container_in_mut(children: &mut [Node], id: Id, depth: u32) -> Option<(u32, &mut Vec<Node>)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        match child {
            Node::Assembly(assembly) => {
                if assembly.id == id {
                    return Some((depth, &mut assembly.children));
                }
                if let Some(found) = find_container_in_mut(&mut assembly.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Group(group) => {
                if group.id == id {
                    return Some((depth, &mut group.children));
                }
                if let Some(found) = find_container_in_mut(&mut group.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
}

fn find_panel_with_depth(root: &Group, id: Id, depth: u32) -> Option<(u32, &Panel)> {
    find_panel_in(&root.children, id, depth)
}

fn find_panel_in(children: &[Node], id: Id, depth: u32) -> Option<(u32, &Panel)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        match child {
            Node::Panel(panel) if panel.id == id => return Some((depth, panel)),
            Node::Assembly(assembly) => {
                if let Some(found) = find_panel_in(&assembly.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Group(group) => {
                if let Some(found) = find_panel_in(&group.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
}

fn find_panel_mut(root: &mut Group, id: Id, depth: u32) -> Option<(u32, &mut Panel)> {
    find_panel_in_mut(&mut root.children, id, depth)
}

fn find_panel_in_mut(children: &mut [Node], id: Id, depth: u32) -> Option<(u32, &mut Panel)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        match child {
            Node::Panel(panel) if panel.id == id => return Some((depth, panel)),
            Node::Assembly(assembly) => {
                if let Some(found) = find_panel_in_mut(&mut assembly.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Group(group) => {
                if let Some(found) = find_panel_in_mut(&mut group.children, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
}

fn find_node(root: &Group, id: Id) -> Option<&Node> {
    find_node_in(&root.children, id)
}

fn find_node_in(children: &[Node], id: Id) -> Option<&Node> {
    for child in children {
        if child.id() == id {
            return Some(child);
        }
        if let Some(children) = child.children()
            && let Some(found) = find_node_in(children, id)
        {
            return Some(found);
        }
    }
    None
}

fn find_node_mut(root: &mut Group, id: Id) -> Option<&mut Node> {
    find_node_in_mut(&mut root.children, id)
}

fn find_node_in_mut(children: &mut [Node], id: Id) -> Option<&mut Node> {
    for child in children {
        if child.id() == id {
            return Some(child);
        }
        if let Some(children) = child.children_mut()
            && let Some(found) = find_node_in_mut(children, id)
        {
            return Some(found);
        }
    }
    None
}

fn find_parent(root: &Group, id: Id) -> Option<Id> {
    fn walk(children: &[Node], id: Id, parent: Id) -> Option<Id> {
        for child in children {
            if child.id() == id {
                return Some(parent);
            }
            if let Some(children) = child.children()
                && let Some(found) = walk(children, id, child.id())
            {
                return Some(found);
            }
        }
        None
    }
    walk(&root.children, id, root.id)
}

/// The minimum corner of the nominal panel boxes below `children`, in their parent's frame.
///
/// Returns `None` when the subtree holds no panel or no finite corner. The walk starts from an
/// identity parent, so each child's own placement is relative to the frame being measured and no
/// world lookup is needed per panel.
fn panel_corner_min(children: &[Node], parent: Placement, depth: u32) -> Option<DVec3> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    let mut min: Option<DVec3> = None;
    for child in children {
        let Ok(relative) = Placement::compose(parent, child.placement()) else {
            continue;
        };
        match child {
            Node::Panel(panel) => {
                let size = panel.nominal_size();
                let length = size.length().millimetres_f64();
                let width = size.width().millimetres_f64();
                let thickness = panel.last_known_thickness().millimetres_f64();
                for x in [0.0, length] {
                    for y in [0.0, width] {
                        for z in [0.0, thickness] {
                            let point = relative.transform_point(DVec3::new(x, y, z));
                            if point.is_finite() {
                                min = Some(min.map_or(point, |current| current.min(point)));
                            }
                        }
                    }
                }
            }
            Node::Assembly(assembly) => {
                if let Some(found) = panel_corner_min(&assembly.children, relative, depth.saturating_add(1)) {
                    min = Some(min.map_or(found, |current| current.min(found)));
                }
            }
            Node::Group(group) => {
                if let Some(found) = panel_corner_min(&group.children, relative, depth.saturating_add(1)) {
                    min = Some(min.map_or(found, |current| current.min(found)));
                }
            }
        }
    }
    min
}

fn node_height(node: &Node) -> u32 {
    match node {
        Node::Panel(_) => 1,
        Node::Assembly(assembly) => assembly.children.iter().map(node_height).max().unwrap_or(0).saturating_add(1),
        Node::Group(group) => group.children.iter().map(node_height).max().unwrap_or(0).saturating_add(1),
    }
}

fn count_nodes(node: &Node) -> u64 {
    match node {
        Node::Panel(_) => 1,
        Node::Assembly(assembly) => {
            1u64.saturating_add(assembly.children.iter().map(count_nodes).fold(0, u64::saturating_add))
        }
        Node::Group(group) => 1u64.saturating_add(group.children.iter().map(count_nodes).fold(0, u64::saturating_add)),
    }
}

fn remove_child(root: &mut Group, id: Id) -> bool {
    remove_child_in(&mut root.children, id)
}

fn remove_child_in(children: &mut Vec<Node>, id: Id) -> bool {
    if let Some(index) = children.iter().position(|child| child.id() == id) {
        children.remove(index);
        return true;
    }
    for child in children {
        if let Some(nested) = child.children_mut()
            && remove_child_in(nested, id)
        {
            return true;
        }
    }
    false
}

fn assign_new_ids(node: &mut Node, ids: &mut impl Iterator<Item = Id>) -> Result<Id, ProjectError> {
    let id = ids.next().ok_or(ProjectError::IdSpaceExhausted)?;
    match node {
        Node::Panel(panel) => panel.id = id,
        Node::Assembly(assembly) => {
            assembly.id = id;
            for child in &mut assembly.children {
                assign_new_ids(child, ids)?;
            }
        }
        Node::Group(group) => {
            group.id = id;
            for child in &mut group.children {
                assign_new_ids(child, ids)?;
            }
        }
    }
    Ok(id)
}

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

fn validate_new_cutout(panel: &Panel, shape: CutoutShape, depth: CutoutDepth) -> Result<(), ProjectError> {
    if panel.has_contour_features() {
        return Err(ProjectError::ContourMachiningUnsupported { panel: panel.id() });
    }
    let geometry = panel.geometry_with_fallbacks()?;
    validate_cutout_geometry(panel.id(), geometry, shape, depth)?;
    for machining in panel.machining() {
        let Machining::Cutout(existing) = machining;
        let both_through = matches!(depth, CutoutDepth::Through) && matches!(existing.depth(), CutoutDepth::Through);
        if !both_through && rects_overlap(cutout_rect(shape), cutout_rect(existing.shape())) {
            return Err(ProjectError::CutoutOverlap { panel: panel.id(), other: existing.id() });
        }
    }
    let candidate = matches!(depth, CutoutDepth::Through).then(|| cutout_rect(shape));
    validate_core(panel, geometry, candidate)
}

fn validate_cutout_geometry(
    panel: Id,
    geometry: PanelGeometry,
    shape: CutoutShape,
    depth: CutoutDepth,
) -> Result<(), ProjectError> {
    let rect = cutout_rect(shape);
    match depth {
        CutoutDepth::Through => {
            if !geometry.cutout_reaches_core(rect) {
                return Err(ProjectError::CutoutOutsidePanel { panel });
            }
        }
        CutoutDepth::Blind { depth } => {
            if !geometry.cutout_fits_core(rect) {
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
            if !both_through && rects_overlap(cutout_rect(cutout.shape()), cutout_rect(other.shape())) {
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

/// The core minus the union of the through cutouts, traced into material regions.
fn core_violation_of(panel: &Panel, geometry: PanelGeometry, extra: Option<PanelRect>) -> Option<CoreViolation> {
    let core = geometry.core_rect()?;
    let mut cuts = Vec::new();
    for machining in panel.machining() {
        let Machining::Cutout(cutout) = machining;
        if matches!(cutout.depth(), CutoutDepth::Through)
            && let Some(rect) = geom_rect(cutout_rect(cutout.shape()))
        {
            cuts.push(rect);
        }
    }
    if let Some(rect) = extra.and_then(geom_rect) {
        cuts.push(rect);
    }
    if cuts.is_empty() {
        return None;
    }
    let regions = regions_from_rects(&subtract_rects(core, &cuts));
    if regions.is_empty() {
        Some(CoreViolation::Removed)
    } else if regions.len() > 1 {
        Some(CoreViolation::Split)
    } else if regions.first().is_some_and(|region| region.area() <= 0.0) {
        Some(CoreViolation::Removed)
    } else {
        None
    }
}

fn cutout_rect(shape: CutoutShape) -> PanelRect {
    match shape {
        CutoutShape::Rectangle(rect) => rect,
    }
}

/// The rectangle in millimetres, or `None` for a degenerate rectangle, which validated input rules out.
fn geom_rect(rect: PanelRect) -> Option<Rect> {
    let x = rect.x().millimetres_f64();
    let y = rect.y().millimetres_f64();
    let width = rect.width().millimetres_f64();
    let height = rect.height().millimetres_f64();
    Rect::new(DVec2::new(x, y), DVec2::new(x + width, y + height)).ok()
}

fn rect_inside(rect: PanelRect, core: PanelRect) -> bool {
    let x0 = rect.x().micrometres() as i128;
    let y0 = rect.y().micrometres() as i128;
    let x1 = x0 + rect.width().micrometres() as i128;
    let y1 = y0 + rect.height().micrometres() as i128;
    let core_x0 = core.x().micrometres() as i128;
    let core_y0 = core.y().micrometres() as i128;
    let core_x1 = core_x0 + core.width().micrometres() as i128;
    let core_y1 = core_y0 + core.height().micrometres() as i128;
    x0 >= core_x0 && y0 >= core_y0 && x1 <= core_x1 && y1 <= core_y1
}

fn rects_overlap(a: PanelRect, b: PanelRect) -> bool {
    let a_x0 = a.x().micrometres() as i128;
    let a_y0 = a.y().micrometres() as i128;
    let a_x1 = a_x0 + a.width().micrometres() as i128;
    let a_y1 = a_y0 + a.height().micrometres() as i128;
    let b_x0 = b.x().micrometres() as i128;
    let b_y0 = b.y().micrometres() as i128;
    let b_x1 = b_x0 + b.width().micrometres() as i128;
    let b_y1 = b_y0 + b.height().micrometres() as i128;
    a_x0 < b_x1 && b_x0 < a_x1 && a_y0 < b_y1 && b_y0 < a_y1
}

/// Why the through cutouts of a panel do not leave a usable core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreViolation {
    /// The through cutouts remove the whole core.
    Removed,
    /// The remaining core is split into disconnected material pieces.
    Split,
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
    /// The requested node is not in the tree, or it is the root group.
    #[error("node {id} not found below the root")]
    NodeNotFound {
        /// The missing node id.
        id: Id,
    },
    /// The root group cannot be deleted, moved, grouped, ungrouped or dissolved.
    #[error("root group {id} cannot be changed this way")]
    RootImmutable {
        /// The root group id.
        id: Id,
    },
    /// Ungrouping was requested for a node that is not a group.
    ///
    /// A panel is an atomic leaf; an assembly is a furniture component with its own identity and is
    /// dismantled by a separate operation rather than by `node.ungroup`.
    #[error("node {id} is not a group and cannot be ungrouped")]
    NotAGroup {
        /// The node that is not a group.
        id: Id,
    },
    /// Grouping needs nodes that share one parent container.
    #[error("nodes selected for grouping do not share one parent container")]
    NodesNotSiblings,
    /// The same node was listed more than once.
    #[error("node {id} is selected more than once")]
    DuplicateNode {
        /// The repeated node id.
        id: Id,
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
    root: Group,
}

impl TryFrom<ProjectWire> for Project {
    type Error = ProjectError;

    fn try_from(wire: ProjectWire) -> Result<Self, Self::Error> {
        // Version 1 stored a `size` on every panel; `PanelWire` migrates it to nominal size plus a
        // fallback. Version 2 stored machining in core-local coordinates; the migration below
        // translates it to the nominal panel frame. All versions are normalized to the current one
        // on load.
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
        let max_id = scan_tree(&root)?;
        let next_id = match wire.next_id {
            Some(next_id) if next_id.get() <= max_id => {
                return Err(ProjectError::NextIdNotAfterMax { next: next_id, max: max_id });
            }
            Some(next_id) => next_id,
            None => next_id_after(max_id)?,
        };
        Ok(Self { schema_version: Project::CURRENT_SCHEMA_VERSION, revision: wire.revision, next_id, root })
    }
}

/// Translates version-2 core-local machining into the nominal panel frame.
///
/// The conversion adds the stored fallback core origin to every cutout, so a loaded file keeps the
/// hole at the same physical position. The migration is bounded by the same depth limit as the
/// document tree.
fn migrate_machining_to_nominal(root: &mut Group, depth: u32) -> Result<(), ProjectError> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
    }
    fn walk(children: &mut [Node], depth: u32) -> Result<(), ProjectError> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        for child in children {
            match child {
                Node::Assembly(assembly) => walk(&mut assembly.children, depth.saturating_add(1))?,
                Node::Group(group) => walk(&mut group.children, depth.saturating_add(1))?,
                Node::Panel(panel) => panel.migrate_machining_from_core()?,
            }
        }
        Ok(())
    }
    walk(&mut root.children, depth.saturating_add(1))
}

/// A node in the product tree.
///
/// An assembly is a meaningful furniture or industrial component: a cabinet, a drawer box, a
/// runner, a hinge. It owns its children, placement and, in a later milestone, a furniture type
/// with type-specific properties and operations. An assembly is not ungrouped by `node.ungroup`;
/// dismantling it is a separate operation with its own rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    id: Id,
    name: String,
    placement: Placement,
    children: Vec<Node>,
}

impl Assembly {
    /// Creates an assembly. Global id uniqueness is checked by [`Project::validate`].
    pub fn new(id: Id, name: impl Into<String>, placement: Placement, children: Vec<Node>) -> Self {
        Self { id, name: name.into(), placement, children }
    }

    /// The stable assembly id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The placement relative to the parent frame.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The panels, groups and nested assemblies in this assembly.
    pub fn children(&self) -> &[Node] {
        &self.children
    }
}

/// An organisational container.
///
/// A group holds any nodes and exists to move or work with them together. It has no furniture type
/// and no type-specific properties or operations. Unlike an assembly it can be flattened back into
/// its parent with `node.ungroup`. The project root is a group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    id: Id,
    name: String,
    placement: Placement,
    children: Vec<Node>,
}

impl Group {
    /// Creates a group. Global id uniqueness is checked by [`Project::validate`].
    pub fn new(id: Id, name: impl Into<String>, placement: Placement, children: Vec<Node>) -> Self {
        Self { id, name: name.into(), placement, children }
    }

    /// The stable group id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The placement relative to the parent frame.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The panels, assemblies and nested groups in this group.
    pub fn children(&self) -> &[Node] {
        &self.children
    }
}

/// A flat panel in the product tree.
///
/// The document stores the nominal outline, the selected material and fallbacks, not the resolved
/// core. The core origin, core size, overall size and effective thickness are derived by
/// [`Panel::geometry`] from the panel, its edge banding and the catalogue value supplied by the
/// caller. The panel-local frame stays anchored at the nominal minimum corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PanelWire")]
pub struct Panel {
    id: Id,
    name: String,
    nominal_size: PanelNominalSize,
    material: Option<MaterialId>,
    last_known_thickness: Length,
    placement: Placement,
    machining: Vec<Machining>,
    edge_banding: Vec<EdgeBanding>,
    corner_banding: Vec<CornerBanding>,
    /// Banded segments in application order; the last one is drawn on top.
    band_order: Vec<SegmentSource>,
    corner_features: Vec<CornerFeature>,
    edge_bows: Vec<EdgeBow>,
}

impl Panel {
    /// Largest accepted nominal panel length or width: ten metres.
    pub const MAX_LENGTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted nominal panel width: ten metres.
    pub const MAX_WIDTH: Length = Length::from_micrometres(10_000_000);
    /// Largest accepted panel thickness: one metre.
    pub const MAX_THICKNESS: Length = Length::from_micrometres(1_000_000);

    /// Creates a panel with a validated fallback thickness.
    ///
    /// Global id uniqueness and the catalogue lookup are checked later. A material id may be
    /// missing from the catalogue; the fallback thickness keeps the panel drawable.
    pub fn new(
        id: Id,
        name: impl Into<String>,
        nominal_size: PanelNominalSize,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", last_known_thickness, Self::MAX_THICKNESS)?;
        Ok(Self {
            id,
            name: name.into(),
            nominal_size,
            material,
            last_known_thickness,
            placement,
            machining: Vec::new(),
            edge_banding: Vec::new(),
            corner_banding: Vec::new(),
            band_order: Vec::new(),
            corner_features: Vec::new(),
            edge_bows: Vec::new(),
        })
    }

    /// Banded segments in application order; the last one is drawn on top.
    pub fn band_order(&self) -> &[SegmentSource] {
        &self.band_order
    }

    fn add_band_source(&mut self, source: SegmentSource) {
        if !self.band_order.contains(&source) {
            self.band_order.push(source);
        }
    }

    fn remove_band_source(&mut self, source: SegmentSource) {
        self.band_order.retain(|existing| *existing != source);
    }

    /// The stable panel id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The nominal panel length and width.
    pub const fn nominal_size(&self) -> PanelNominalSize {
        self.nominal_size
    }

    /// The selected panel material, if any.
    pub const fn material(&self) -> Option<MaterialId> {
        self.material
    }

    /// The fallback thickness used when the material is absent.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The placement relative to the parent frame, anchored at the nominal minimum corner.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// Machining owned by this panel, in nominal panel-local coordinates.
    pub fn machining(&self) -> &[Machining] {
        &self.machining
    }

    /// Checks whether the through cutouts leave one connected core with positive area.
    ///
    /// `Some` means the cutouts would empty the core or split it into disconnected pieces, so a
    /// command must be rejected and a loaded panel must be reported as a problem panel. Blind
    /// cutouts are not part of the 2D union (ADR 0009); a shaped panel returns `None` because its
    /// core is not a rectangle.
    pub fn core_violation(&self, geometry: PanelGeometry) -> Option<CoreViolation> {
        if self.has_contour_features() {
            return None;
        }
        core_violation_of(self, geometry, None)
    }

    /// Edge banding in application order.
    pub fn edge_banding(&self) -> &[EdgeBanding] {
        &self.edge_banding
    }

    /// Corner banding in application order.
    pub fn corner_banding(&self) -> &[CornerBanding] {
        &self.corner_banding
    }

    /// The corner banding record on a chamfer corner, if present.
    pub fn corner_banding_at(&self, corner: VertexId) -> Option<CornerBanding> {
        self.corner_banding.iter().find(|band| band.corner() == corner).cloned()
    }

    /// Whether any edge or corner is banded.
    pub fn has_banding(&self) -> bool {
        !self.edge_banding.is_empty() || !self.corner_banding.is_empty()
    }

    /// Corner treatments in application order.
    pub fn corner_features(&self) -> &[CornerFeature] {
        &self.corner_features
    }

    /// Bowed edges in application order.
    pub fn edge_bows(&self) -> &[EdgeBow] {
        &self.edge_bows
    }

    /// Whether the nominal outline is not a plain rectangle.
    pub fn has_contour_features(&self) -> bool {
        !self.corner_features.is_empty() || !self.edge_bows.is_empty()
    }

    /// The corner treatment on a vertex, if present.
    pub fn corner_feature(&self, corner: VertexId) -> Option<CornerFeature> {
        self.corner_features.iter().copied().find(|feature| feature.corner() == corner)
    }

    /// The bow on an edge, if present.
    pub fn edge_bow(&self, edge: EdgeId) -> Option<EdgeBow> {
        self.edge_bows.iter().copied().find(|bow| bow.edge() == edge)
    }

    /// Resolves the geometry with the supplied panel thickness and fallback banding thicknesses.
    pub fn geometry(&self, thickness: Length) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry_for(self.nominal_size, thickness)
    }

    /// Resolves the geometry with a caller-provided banding thickness.
    ///
    /// The engine uses this to pass catalogue-resolved profiles without making the model read the
    /// catalogue. `band_thickness` is called once per banding path; a rectangular panel has one band
    /// per edge, while a tangent path counts once.
    pub fn geometry_with_banding<F>(
        &self,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        self.geometry_for_with(self.nominal_size, thickness, band_thickness)
    }

    /// Resolves the geometry for a different nominal size, used by a resize before mutation.
    ///
    /// Banding thickness comes from the stored fallbacks.
    pub fn geometry_for(
        &self,
        nominal_size: PanelNominalSize,
        thickness: Length,
    ) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry_for_with(nominal_size, thickness, |spec| spec.last_known_thickness())
    }

    /// Resolves geometry for a nominal size with a caller-provided banding thickness.
    pub fn geometry_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        validate_dimension("thickness", thickness, Panel::MAX_THICKNESS).map_err(PanelGeometryError::Dimension)?;

        if self.has_contour_features() {
            let core =
                self.core_contour_for_with(nominal_size, &band_thickness).map_err(|_| PanelGeometryError::Contour)?;
            let (core_min, core_max) = core.bounds().ok_or(PanelGeometryError::Contour)?;
            let (overall_min, overall_max) =
                self.outer_bounds_for_with(nominal_size, &band_thickness).map_err(|_| PanelGeometryError::Contour)?;
            let core_length = length_from_millimetres(core_max.x - core_min.x)?;
            let core_width = length_from_millimetres(core_max.y - core_min.y)?;
            let overall_length = length_from_millimetres(overall_max.x - overall_min.x)?;
            let overall_width = length_from_millimetres(overall_max.y - overall_min.y)?;
            return PanelGeometry::new(
                Length::ZERO,
                Length::ZERO,
                core_length,
                core_width,
                thickness,
                overall_length,
                overall_width,
            );
        }

        let mut included_bottom = Length::ZERO;
        let mut included_right = Length::ZERO;
        let mut included_top = Length::ZERO;
        let mut included_left = Length::ZERO;
        let mut total_bottom = Length::ZERO;
        let mut total_right = Length::ZERO;
        let mut total_top = Length::ZERO;
        let mut total_left = Length::ZERO;

        for band in &self.edge_banding {
            let thickness = band_thickness(&band.spec());
            match band.edge().get() {
                1 => {
                    total_bottom = checked_add("bottom_banding", total_bottom, thickness)?;
                    if band.included_in_size() {
                        included_bottom = checked_add("included_bottom", included_bottom, thickness)?;
                    }
                }
                2 => {
                    total_right = checked_add("right_banding", total_right, thickness)?;
                    if band.included_in_size() {
                        included_right = checked_add("included_right", included_right, thickness)?;
                    }
                }
                3 => {
                    total_top = checked_add("top_banding", total_top, thickness)?;
                    if band.included_in_size() {
                        included_top = checked_add("included_top", included_top, thickness)?;
                    }
                }
                4 => {
                    total_left = checked_add("left_banding", total_left, thickness)?;
                    if band.included_in_size() {
                        included_left = checked_add("included_left", included_left, thickness)?;
                    }
                }
                _ => return Err(PanelGeometryError::UnsupportedEdge { edge: band.edge() }),
            }
        }

        let core_length = checked_sub_many("core_length", nominal_size.length(), [included_left, included_right])?;
        let core_width = checked_sub_many("core_width", nominal_size.width(), [included_bottom, included_top])?;
        let overall_length = checked_add_many("overall_length", core_length, [total_left, total_right])?;
        let overall_width = checked_add_many("overall_width", core_width, [total_bottom, total_top])?;

        // The panel frame is anchored at the nominal minimum corner. An included band takes its
        // thickness out of the low side of the core, so the core origin moves inward by that amount.
        PanelGeometry::new(
            included_left,
            included_bottom,
            core_length,
            core_width,
            thickness,
            overall_length,
            overall_width,
        )
    }

    /// Resolves geometry using the stored fallback thickness.
    pub fn geometry_with_fallbacks(&self) -> Result<PanelGeometry, PanelGeometryError> {
        self.geometry(self.last_known_thickness)
    }

    /// Translates version-2 core-local cutouts into the nominal panel frame.
    ///
    /// The legacy core origin is derived from the included bottom and left banding fallbacks. A
    /// panel with a shaped contour has no stored core origin, so its machining is left unchanged.
    fn migrate_machining_from_core(&mut self) -> Result<(), ProjectError> {
        if self.machining.is_empty() || self.has_contour_features() {
            return Ok(());
        }
        let mut core_x = Length::ZERO;
        let mut core_y = Length::ZERO;
        for band in &self.edge_banding {
            if !band.included_in_size() {
                continue;
            }
            match band.edge().get() {
                1 => {
                    core_y = core_y
                        .checked_add(band.last_known_thickness())
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                }
                4 => {
                    core_x = core_x
                        .checked_add(band.last_known_thickness())
                        .ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
                }
                _ => {}
            }
        }
        if core_x == Length::ZERO && core_y == Length::ZERO {
            return Ok(());
        }
        for machining in &mut self.machining {
            let Machining::Cutout(cutout) = machining;
            let CutoutShape::Rectangle(rect) = cutout.shape;
            let x = rect.x().checked_add(core_x).ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
            let y = rect.y().checked_add(core_y).ok_or(ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
            let shape = PanelRect::new(x, y, rect.width(), rect.height())
                .map_err(|_| ProjectError::MachiningCoordinateOverflow { panel: self.id })?;
            cutout.shape = CutoutShape::Rectangle(shape);
        }
        Ok(())
    }

    fn upsert_corner_feature(&mut self, feature: CornerFeature) {
        let corner = feature.corner();
        self.corner_features.retain(|existing| existing.corner() != corner);
        self.corner_features.push(feature);
    }

    fn remove_corner_feature(&mut self, corner: VertexId) -> bool {
        let before = self.corner_features.len();
        self.corner_features.retain(|existing| existing.corner() != corner);
        self.corner_features.len() != before
    }

    fn upsert_edge_bow(&mut self, bow: EdgeBow) {
        let edge = bow.edge();
        self.edge_bows.retain(|existing| existing.edge() != edge);
        self.edge_bows.push(bow);
    }

    fn remove_edge_bow(&mut self, edge: EdgeId) -> bool {
        let before = self.edge_bows.len();
        self.edge_bows.retain(|existing| existing.edge() != edge);
        self.edge_bows.len() != before
    }

    fn upsert_corner_banding(&mut self, band: CornerBanding) {
        let corner = band.corner();
        self.corner_banding.retain(|existing| existing.corner() != corner);
        self.corner_banding.push(band);
    }

    fn remove_corner_banding(&mut self, corner: VertexId) -> bool {
        let before = self.corner_banding.len();
        self.corner_banding.retain(|existing| existing.corner() != corner);
        self.corner_banding.len() != before
    }
}

/// Why a serialized panel cannot be read as the current schema.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PanelError {
    /// A current panel is missing its nominal size.
    #[error("panel has no nominal size")]
    MissingNominalSize,
    /// A current panel is missing its fallback thickness.
    #[error("panel has no last_known_thickness")]
    MissingThickness,
    /// A panel mixes the legacy `size` field with current fields.
    #[error("panel mixes the legacy size field with the current schema")]
    AmbiguousSize,
    /// A stored dimension is not usable.
    #[error(transparent)]
    Dimension(#[from] PanelDimensionError),
    /// A corner feature was stored twice for the same corner.
    #[error("panel has duplicate corner feature on corner {corner}")]
    DuplicateCornerFeature {
        /// The duplicated corner.
        corner: VertexId,
    },
    /// An edge bow was stored twice for the same edge.
    #[error("panel has duplicate bow on edge {edge}")]
    DuplicateEdgeBow {
        /// The duplicated edge.
        edge: EdgeId,
    },
    /// A corner feature uses a corner outside the rectangular outline.
    #[error("panel corner feature uses unsupported corner {corner}")]
    UnsupportedCornerFeature {
        /// The rejected corner.
        corner: VertexId,
    },
    /// A bow uses an edge outside the rectangular outline.
    #[error("panel bow uses unsupported edge {edge}")]
    UnsupportedEdgeBow {
        /// The rejected edge.
        edge: EdgeId,
    },
    /// Corner banding was stored twice for the same corner.
    #[error("panel has duplicate corner banding on corner {corner}")]
    DuplicateCornerBanding {
        /// The duplicated corner.
        corner: VertexId,
    },
    /// Corner banding uses a corner outside the rectangular outline.
    #[error("panel corner banding uses unsupported corner {corner}")]
    UnsupportedCornerBanding {
        /// The rejected corner.
        corner: VertexId,
    },
}

/// Serialized form of [`Panel`]; it accepts the v1 `size` field for migration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelWire {
    id: Id,
    name: String,
    #[serde(default)]
    nominal_size: Option<PanelNominalSize>,
    #[serde(default)]
    material: Option<MaterialId>,
    #[serde(default)]
    last_known_thickness: Option<Length>,
    placement: Placement,
    #[serde(default)]
    machining: Vec<Machining>,
    #[serde(default)]
    edge_banding: Vec<EdgeBanding>,
    #[serde(default)]
    corner_banding: Vec<CornerBanding>,
    #[serde(default)]
    band_order: Vec<SegmentSource>,
    #[serde(default)]
    corner_features: Vec<CornerFeature>,
    #[serde(default)]
    edge_bows: Vec<EdgeBow>,
    #[serde(default)]
    size: Option<LegacyPanelSize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPanelSize {
    length: Length,
    width: Length,
    thickness: Length,
}

impl TryFrom<PanelWire> for Panel {
    type Error = PanelError;

    fn try_from(wire: PanelWire) -> Result<Self, Self::Error> {
        let (nominal_size, material, thickness, edge_banding) = match wire.size {
            Some(size) => {
                if wire.nominal_size.is_some()
                    || wire.material.is_some()
                    || wire.last_known_thickness.is_some()
                    || !wire.edge_banding.is_empty()
                    || !wire.corner_banding.is_empty()
                    || !wire.band_order.is_empty()
                    || !wire.corner_features.is_empty()
                    || !wire.edge_bows.is_empty()
                {
                    return Err(PanelError::AmbiguousSize);
                }
                let nominal_size = PanelNominalSize::new(size.length, size.width)?;
                (nominal_size, None, size.thickness, Vec::new())
            }
            None => {
                let nominal_size = wire.nominal_size.ok_or(PanelError::MissingNominalSize)?;
                let thickness = wire.last_known_thickness.ok_or(PanelError::MissingThickness)?;
                (nominal_size, wire.material, thickness, wire.edge_banding)
            }
        };

        validate_contour_features(&wire.corner_features, &wire.edge_bows)?;
        validate_corner_banding(&wire.corner_banding)?;
        let mut panel = Self::new(wire.id, wire.name, nominal_size, material, thickness, wire.placement)?;
        panel.machining = wire.machining;
        panel.edge_banding = edge_banding;
        panel.corner_banding = wire.corner_banding;
        panel.band_order = normalize_band_order(wire.band_order, &panel.edge_banding, &panel.corner_banding);
        panel.corner_features = wire.corner_features;
        panel.edge_bows = wire.edge_bows;
        Ok(panel)
    }
}

/// Keeps one entry per band record and appends records that older files did not order.
///
/// Schema 3 stored edge and corner banding in two separate lists; the migration uses their order,
/// edges first and corners second.
fn normalize_band_order(
    order: Vec<SegmentSource>,
    edges: &[EdgeBanding],
    corners: &[CornerBanding],
) -> Vec<SegmentSource> {
    let mut normalized = Vec::new();
    for source in order {
        let present = match source {
            SegmentSource::Edge(edge) => edges.iter().any(|band| band.edge() == edge),
            SegmentSource::Corner(corner) => corners.iter().any(|band| band.corner() == corner),
        };
        if present && !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    for band in edges {
        let source = SegmentSource::Edge(band.edge());
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    for band in corners {
        let source = SegmentSource::Corner(band.corner());
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    normalized
}

fn validate_corner_banding(bands: &[CornerBanding]) -> Result<(), PanelError> {
    let mut corners = HashSet::new();
    for band in bands {
        let corner = band.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(PanelError::UnsupportedCornerBanding { corner });
        }
        if !corners.insert(corner.get()) {
            return Err(PanelError::DuplicateCornerBanding { corner });
        }
    }
    Ok(())
}

fn validate_contour_features(features: &[CornerFeature], bows: &[EdgeBow]) -> Result<(), PanelError> {
    let mut corners = HashSet::new();
    for feature in features {
        let corner = feature.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(PanelError::UnsupportedCornerFeature { corner });
        }
        if !corners.insert(corner.get()) {
            return Err(PanelError::DuplicateCornerFeature { corner });
        }
    }
    let mut edges = HashSet::new();
    for bow in bows {
        let edge = bow.edge();
        if !(1..=4).contains(&edge.get()) {
            return Err(PanelError::UnsupportedEdgeBow { edge });
        }
        if !edges.insert(edge.get()) {
            return Err(PanelError::DuplicateEdgeBow { edge });
        }
    }
    Ok(())
}

/// A machining feature owned by a panel.
///
/// Holes generated by hardware will join this list as a separate variant with a hardware source;
/// this first variant is a cutout authored directly on the panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Machining {
    /// A rectangular volume removed from the panel.
    Cutout(Cutout),
}

/// A cutout in a panel.
///
/// Coordinates are nominal panel-local: `(0, 0)` is the minimum corner of the nominal rectangle,
/// with `+X` along the length and `+Y` along the width. The core origin and included edge banding
/// never move the panel frame. `entry` names the face the cut enters from; a blind cutout removes
/// material from that face up to its depth, while a through cutout removes the full thickness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cutout {
    id: Id,
    shape: CutoutShape,
    depth: CutoutDepth,
    entry: FaceSide,
}

impl Cutout {
    pub(crate) fn new(id: Id, shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Self {
        Self { id, shape, depth, entry }
    }

    /// The stable cutout id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The cutout outline in nominal panel-local coordinates.
    pub const fn shape(&self) -> CutoutShape {
        self.shape
    }

    /// How deep the cutout goes.
    pub const fn depth(&self) -> CutoutDepth {
        self.depth
    }

    /// The face the cut enters from.
    pub const fn entry(&self) -> FaceSide {
        self.entry
    }
}

/// The outline of a cutout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoutShape {
    /// A rectangle whose minimum corner is at `rect.x`, `rect.y` from the nominal panel zero.
    Rectangle(PanelRect),
}

/// How much material a cutout removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoutDepth {
    /// Removes the full panel thickness.
    Through,
    /// Removes material from the entry face by `depth`.
    Blind {
        /// Depth measured from the entry face.
        depth: Length,
    },
}

/// A rectangle in nominal panel-local XY coordinates.
///
/// `x` and `y` are the minimum corner relative to the nominal panel zero; `width` and `height`
/// are positive. Cutout validation checks that the rectangle stays within the core outline, which
/// may start at a positive `(core_x, core_y)` when an included band trims the low edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PanelRectWire")]
pub struct PanelRect {
    x: Length,
    y: Length,
    width: Length,
    height: Length,
}

impl PanelRect {
    /// Creates a rectangle with positive width and height.
    pub fn new(x: Length, y: Length, width: Length, height: Length) -> Result<Self, PanelRectError> {
        if !width.is_positive() {
            return Err(PanelRectError::NonPositive { field: "width", value: width });
        }
        if !height.is_positive() {
            return Err(PanelRectError::NonPositive { field: "height", value: height });
        }
        Ok(Self { x, y, width, height })
    }

    /// The minimum X coordinate.
    pub const fn x(self) -> Length {
        self.x
    }

    /// The minimum Y coordinate.
    pub const fn y(self) -> Length {
        self.y
    }

    /// The extent along X.
    pub const fn width(self) -> Length {
        self.width
    }

    /// The extent along Y.
    pub const fn height(self) -> Length {
        self.height
    }
}

/// Why a panel rectangle is not usable.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelRectError {
    /// Width or height is zero or negative.
    #[error("cutout {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
}

/// Serialized form of [`PanelRect`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelRectWire {
    x: Length,
    y: Length,
    width: Length,
    height: Length,
}

impl TryFrom<PanelRectWire> for PanelRect {
    type Error = PanelRectError;

    fn try_from(wire: PanelRectWire) -> Result<Self, Self::Error> {
        Self::new(wire.x, wire.y, wire.width, wire.height)
    }
}

/// A child of an assembly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    /// A nested assembly component.
    Assembly(Assembly),
    /// An organisational group.
    Group(Group),
    /// A panel.
    Panel(Panel),
}

impl From<Assembly> for Node {
    fn from(assembly: Assembly) -> Self {
        Self::Assembly(assembly)
    }
}

impl From<Group> for Node {
    fn from(group: Group) -> Self {
        Self::Group(group)
    }
}

impl From<Panel> for Node {
    fn from(panel: Panel) -> Self {
        Self::Panel(panel)
    }
}

impl Node {
    /// The stable node id, shared by panels and both container kinds.
    pub const fn id(&self) -> Id {
        match self {
            Self::Assembly(assembly) => assembly.id,
            Self::Group(group) => group.id,
            Self::Panel(panel) => panel.id,
        }
    }

    /// The user-visible node name.
    pub fn name(&self) -> &str {
        match self {
            Self::Assembly(assembly) => &assembly.name,
            Self::Group(group) => &group.name,
            Self::Panel(panel) => &panel.name,
        }
    }

    /// The placement relative to the parent container.
    pub const fn placement(&self) -> Placement {
        match self {
            Self::Assembly(assembly) => assembly.placement,
            Self::Group(group) => group.placement,
            Self::Panel(panel) => panel.placement,
        }
    }

    /// The children of a container, or `None` for a panel.
    pub fn children(&self) -> Option<&[Node]> {
        match self {
            Self::Assembly(assembly) => Some(&assembly.children),
            Self::Group(group) => Some(&group.children),
            Self::Panel(_) => None,
        }
    }

    fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Self::Assembly(assembly) => Some(&mut assembly.children),
            Self::Group(group) => Some(&mut group.children),
            Self::Panel(_) => None,
        }
    }

    /// The nested assembly, or `None` for a group or panel.
    pub const fn as_assembly(&self) -> Option<&Assembly> {
        match self {
            Self::Assembly(assembly) => Some(assembly),
            Self::Group(_) | Self::Panel(_) => None,
        }
    }

    /// The nested group, or `None` for an assembly or panel.
    pub const fn as_group(&self) -> Option<&Group> {
        match self {
            Self::Group(group) => Some(group),
            Self::Assembly(_) | Self::Panel(_) => None,
        }
    }

    /// The panel, or `None` for a container.
    pub const fn as_panel(&self) -> Option<&Panel> {
        match self {
            Self::Panel(panel) => Some(panel),
            Self::Assembly(_) | Self::Group(_) => None,
        }
    }

    fn set_name(&mut self, name: String) {
        match self {
            Self::Assembly(assembly) => assembly.name = name,
            Self::Group(group) => group.name = name,
            Self::Panel(panel) => panel.name = name,
        }
    }

    fn set_placement(&mut self, placement: Placement) {
        match self {
            Self::Assembly(assembly) => assembly.placement = placement,
            Self::Group(group) => group.placement = placement,
            Self::Panel(panel) => panel.placement = placement,
        }
    }
}

/// One of the two large faces of a flat panel.
///
/// `A` is the face at `z = thickness`; `B` is the face at `z = 0`. `+Z` points from `B` to `A`.
/// The labels are local to the panel: they travel with it through a placement and never mean
/// "world up" or "world front".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaceSide {
    /// The face at `z = thickness` (+Z).
    A,
    /// The face at `z = 0` (-Z).
    B,
}

impl FaceSide {
    /// The other face.
    pub const fn opposite(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// Human-readable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }

    /// Sign of the face normal along the panel-local Z axis.
    pub const fn normal_sign(self) -> i8 {
        match self {
            Self::A => 1,
            Self::B => -1,
        }
    }
}

/// The nominal length and width stored on a panel.
///
/// Thickness is not part of the nominal size: it comes from the material, or from the panel's
/// fallback when the material is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "PanelNominalSizeWire")]
pub struct PanelNominalSize {
    length: Length,
    width: Length,
}

impl PanelNominalSize {
    /// Creates a nominal size after checking that both sides are positive and within limits.
    pub fn new(length: Length, width: Length) -> Result<Self, PanelDimensionError> {
        validate_dimension("length", length, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_WIDTH)?;
        Ok(Self { length, width })
    }

    /// The nominal length along local X.
    pub const fn length(self) -> Length {
        self.length
    }

    /// The nominal width along local Y.
    pub const fn width(self) -> Length {
        self.width
    }
}

/// The resolved dimensions of a panel.
///
/// The panel-local frame is anchored at the minimum corner of the **nominal** rectangle. The core
/// occupies `x` from `core_x` to `core_x + core_length`, `y` from `core_y` to
/// `core_y + core_width`, and `z` from `0` to `thickness`. `core_x` and `core_y` are the
/// included banding thicknesses on the left and bottom edges; they are zero when those edges are
/// unbanded or not included in the size. Overall dimensions are the extents of the finished panel
/// including all banding; when a band is not included they may extend outside the nominal rectangle.
/// A shaped panel builds its core contour directly: its reported `core_x` and `core_y` are zero and
/// the core extents come from the bounds of the offset path assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelGeometry {
    core_x: Length,
    core_y: Length,
    core_length: Length,
    core_width: Length,
    thickness: Length,
    overall_length: Length,
    overall_width: Length,
}

impl PanelGeometry {
    /// Creates resolved geometry after checking the core, its nominal origin and the thickness.
    pub fn new(
        core_x: Length,
        core_y: Length,
        core_length: Length,
        core_width: Length,
        thickness: Length,
        overall_length: Length,
        overall_width: Length,
    ) -> Result<Self, PanelGeometryError> {
        require_non_negative("core_x", core_x)?;
        require_non_negative("core_y", core_y)?;
        require_positive("core_length", core_length)?;
        require_positive("core_width", core_width)?;
        validate_dimension("thickness", thickness, Panel::MAX_THICKNESS).map_err(PanelGeometryError::Dimension)?;
        if overall_length < core_length {
            return Err(PanelGeometryError::OverallTooSmall {
                field: "overall_length",
                value: overall_length,
                core: core_length,
            });
        }
        if overall_width < core_width {
            return Err(PanelGeometryError::OverallTooSmall {
                field: "overall_width",
                value: overall_width,
                core: core_width,
            });
        }
        Ok(Self { core_x, core_y, core_length, core_width, thickness, overall_length, overall_width })
    }

    /// Whether a cutout rectangle shares a positive area with the core.
    ///
    /// A through cutout that does not reach the core would remove nothing and is rejected.
    pub fn cutout_reaches_core(self, rect: PanelRect) -> bool {
        self.core_panel_rect().is_some_and(|core| rects_overlap(rect, core))
    }

    /// Whether a cutout rectangle lies fully inside the core.
    ///
    /// Blind cutouts stay inside the core until the 3D edge-pocket stage.
    pub fn cutout_fits_core(self, rect: PanelRect) -> bool {
        self.core_panel_rect().is_some_and(|core| rect_inside(rect, core))
    }

    fn core_panel_rect(self) -> Option<PanelRect> {
        PanelRect::new(self.core_x, self.core_y, self.core_length, self.core_width).ok()
    }

    fn core_rect(self) -> Option<Rect> {
        geom_rect(self.core_panel_rect()?)
    }

    /// The core origin along local X, relative to the nominal minimum corner.
    pub const fn core_x(self) -> Length {
        self.core_x
    }

    /// The core origin along local Y, relative to the nominal minimum corner.
    pub const fn core_y(self) -> Length {
        self.core_y
    }

    /// The core length along local X.
    pub const fn core_length(self) -> Length {
        self.core_length
    }

    /// The core width along local Y.
    pub const fn core_width(self) -> Length {
        self.core_width
    }

    /// The effective panel thickness along local Z.
    pub const fn thickness(self) -> Length {
        self.thickness
    }

    /// The overall length including edge banding.
    pub const fn overall_length(self) -> Length {
        self.overall_length
    }

    /// The overall width including edge banding.
    pub const fn overall_width(self) -> Length {
        self.overall_width
    }
}

/// Why a nominal panel size or a banding profile is not usable.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelDimensionError {
    /// A dimension is zero or negative.
    #[error("panel {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// A dimension exceeds the supported maximum.
    #[error("panel {field} must not exceed {max}, got {value}")]
    TooLarge {
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
        /// The largest accepted value for this dimension.
        max: Length,
    },
}

/// Why a panel cannot be resolved into core, overall and thickness.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PanelGeometryError {
    /// A stored dimension is not usable.
    #[error(transparent)]
    Dimension(#[from] PanelDimensionError),
    /// The resulting core is zero or negative.
    #[error("panel {field} must be positive, got {value}")]
    NonPositive {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// The core origin is negative relative to the nominal minimum corner.
    #[error("panel {field} must not be negative, got {value}")]
    NegativeOrigin {
        /// The rejected field.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// An overall dimension is smaller than its core.
    #[error("panel {field} {value} is smaller than the core {core}")]
    OverallTooSmall {
        /// The rejected field.
        field: &'static str,
        /// The rejected overall value.
        value: Length,
        /// The core value.
        core: Length,
    },
    /// A sum or difference overflowed.
    #[error("panel {field} overflows")]
    Overflow {
        /// The overflowing field.
        field: &'static str,
    },
    /// Edge banding refers to an edge outside the rectangular v1 outline.
    #[error("edge banding on unsupported edge {edge}")]
    UnsupportedEdge {
        /// The rejected edge.
        edge: EdgeId,
    },
    /// The nominal contour of a shaped panel is not valid.
    #[error("the panel contour is invalid")]
    Contour,
}

/// The catalogued and fallback values that describe one band.
///
/// Edge and corner banding share these values but keep separate stable owners. The catalogued
/// material is resolved by the engine; the fallback dimensions keep the panel readable when the
/// material is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandingSpec {
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl BandingSpec {
    /// Creates a spec with validated fallback dimensions.
    pub fn new(
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The banding material id.
    pub const fn material(self) -> MaterialId {
        self.material
    }

    /// Whether this band is included in the panel's nominal size.
    pub const fn included_in_size(self) -> bool {
        self.included_in_size
    }

    /// The fallback band thickness.
    pub const fn last_known_thickness(self) -> Length {
        self.last_known_thickness
    }

    /// The fallback band width.
    pub const fn last_known_width(self) -> Length {
        self.last_known_width
    }
}

/// One edge-banding strip in application order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EdgeBandingWire")]
pub struct EdgeBanding {
    edge: EdgeId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl EdgeBanding {
    /// Creates a banding record with validated fallback dimensions.
    pub fn new(
        edge: EdgeId,
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { edge, material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The edge this strip covers.
    pub const fn edge(&self) -> EdgeId {
        self.edge
    }

    /// The banding material id.
    pub const fn material(&self) -> MaterialId {
        self.material
    }

    /// Whether this strip is included in the panel's nominal size.
    pub const fn included_in_size(&self) -> bool {
        self.included_in_size
    }

    /// The fallback banding thickness.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The fallback banding width.
    pub const fn last_known_width(&self) -> Length {
        self.last_known_width
    }

    /// The shared banding values without the edge owner.
    pub const fn spec(&self) -> BandingSpec {
        BandingSpec {
            material: self.material,
            included_in_size: self.included_in_size,
            last_known_thickness: self.last_known_thickness,
            last_known_width: self.last_known_width,
        }
    }
}

/// One corner-banding strip on a corner segment.
///
/// A chamfer or radius is its own contour segment, so its banding is keyed by the corner vertex.
/// The record is removed together with the segment when the corner becomes a point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CornerBandingWire")]
pub struct CornerBanding {
    corner: VertexId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl CornerBanding {
    /// Creates a corner banding record with validated fallback dimensions.
    pub fn new(
        corner: VertexId,
        material: MaterialId,
        included_in_size: bool,
        thickness: Length,
        width: Length,
    ) -> Result<Self, PanelDimensionError> {
        validate_dimension("thickness", thickness, Panel::MAX_LENGTH)?;
        validate_dimension("width", width, Panel::MAX_THICKNESS)?;
        Ok(Self { corner, material, included_in_size, last_known_thickness: thickness, last_known_width: width })
    }

    /// The chamfer corner this strip covers.
    pub const fn corner(&self) -> VertexId {
        self.corner
    }

    /// The banding material id.
    pub const fn material(&self) -> MaterialId {
        self.material
    }

    /// Whether this strip is included in the panel's nominal size.
    pub const fn included_in_size(&self) -> bool {
        self.included_in_size
    }

    /// The fallback band thickness.
    pub const fn last_known_thickness(&self) -> Length {
        self.last_known_thickness
    }

    /// The fallback band width.
    pub const fn last_known_width(&self) -> Length {
        self.last_known_width
    }

    /// The shared banding values without the corner owner.
    pub const fn spec(&self) -> BandingSpec {
        BandingSpec {
            material: self.material,
            included_in_size: self.included_in_size,
            last_known_thickness: self.last_known_thickness,
            last_known_width: self.last_known_width,
        }
    }
}

fn length_from_millimetres(value: f64) -> Result<Length, PanelGeometryError> {
    let micrometres = (value * 1000.0).round();
    if !micrometres.is_finite() || micrometres < 0.0 || micrometres > i64::MAX as f64 {
        return Err(PanelGeometryError::Contour);
    }
    Ok(Length::from_micrometres(micrometres as i64))
}

fn validate_dimension(field: &'static str, value: Length, max: Length) -> Result<(), PanelDimensionError> {
    if !value.is_positive() {
        return Err(PanelDimensionError::NonPositive { field, value });
    }
    if value > max {
        return Err(PanelDimensionError::TooLarge { field, value, max });
    }
    Ok(())
}

fn require_positive(field: &'static str, value: Length) -> Result<(), PanelGeometryError> {
    if !value.is_positive() {
        return Err(PanelGeometryError::NonPositive { field, value });
    }
    Ok(())
}

fn require_non_negative(field: &'static str, value: Length) -> Result<(), PanelGeometryError> {
    if value.is_negative() {
        return Err(PanelGeometryError::NegativeOrigin { field, value });
    }
    Ok(())
}

fn checked_add(field: &'static str, left: Length, right: Length) -> Result<Length, PanelGeometryError> {
    left.checked_add(right).ok_or(PanelGeometryError::Overflow { field })
}

fn checked_add_many(
    field: &'static str,
    base: Length,
    values: impl IntoIterator<Item = Length>,
) -> Result<Length, PanelGeometryError> {
    let mut total = base;
    for value in values {
        total = checked_add(field, total, value)?;
    }
    Ok(total)
}

fn checked_sub_many(
    field: &'static str,
    base: Length,
    values: impl IntoIterator<Item = Length>,
) -> Result<Length, PanelGeometryError> {
    let mut total = base;
    for value in values {
        total = total.checked_sub(value).ok_or(PanelGeometryError::Overflow { field })?;
    }
    Ok(total)
}

/// Serialized form of [`PanelNominalSize`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelNominalSizeWire {
    length: Length,
    width: Length,
}

impl TryFrom<PanelNominalSizeWire> for PanelNominalSize {
    type Error = PanelDimensionError;

    fn try_from(wire: PanelNominalSizeWire) -> Result<Self, Self::Error> {
        Self::new(wire.length, wire.width)
    }
}

/// Serialized form of [`EdgeBanding`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBandingWire {
    edge: EdgeId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl TryFrom<EdgeBandingWire> for EdgeBanding {
    type Error = PanelDimensionError;

    fn try_from(wire: EdgeBandingWire) -> Result<Self, Self::Error> {
        Self::new(wire.edge, wire.material, wire.included_in_size, wire.last_known_thickness, wire.last_known_width)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CornerBandingWire {
    corner: VertexId,
    material: MaterialId,
    included_in_size: bool,
    last_known_thickness: Length,
    last_known_width: Length,
}

impl TryFrom<CornerBandingWire> for CornerBanding {
    type Error = PanelDimensionError;

    fn try_from(wire: CornerBandingWire) -> Result<Self, Self::Error> {
        Self::new(wire.corner, wire.material, wire.included_in_size, wire.last_known_thickness, wire.last_known_width)
    }
}

macro_rules! panel_id {
    ($name:ident, $description:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates an id from a non-zero value.
            pub const fn new(value: u32) -> Option<Self> {
                if value == 0 { None } else { Some(Self(value)) }
            }

            /// The numeric value.
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Builds an id from a value that is known to be non-zero inside this crate.
            pub(crate) const fn from_known(value: u32) -> Self {
                Self(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u32::deserialize(deserializer)?;
                Self::new(value)
                    .ok_or_else(|| <D::Error as serde::de::Error>::custom(concat!($description, " must be non-zero")))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0)
            }
        }
    };
}

panel_id!(VertexId, "vertex id");
panel_id!(EdgeId, "edge id");

/// A corner of a rectangular panel, numbered counter-clockwise when viewed from face `A`.
///
/// Corner 1 is the nominal minimum corner at `(0, 0)`. The core may start at a positive
/// `(core_x, core_y)` when the bottom or left banding is included in the size. The enum is a
/// convenience for rectangular panels; contour panels use [`VertexId`] directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RectCorner {
    /// `(0, 0)`, the nominal zero.
    BottomLeft = 1,
    /// `(nominal length, 0)`.
    BottomRight = 2,
    /// `(nominal length, nominal width)`.
    TopRight = 3,
    /// `(0, nominal width)`.
    TopLeft = 4,
}

impl RectCorner {
    /// Corners in counter-clockwise order.
    pub const ALL: [Self; 4] = [Self::BottomLeft, Self::BottomRight, Self::TopRight, Self::TopLeft];

    /// The stable id, equal to the human number.
    pub const fn id(self) -> VertexId {
        VertexId::from_known(self as u32)
    }

    /// The human number as a string.
    pub const fn label(self) -> &'static str {
        match self {
            Self::BottomLeft => "1",
            Self::BottomRight => "2",
            Self::TopRight => "3",
            Self::TopLeft => "4",
        }
    }

    /// The next corner counter-clockwise.
    pub const fn next(self) -> Self {
        match self {
            Self::BottomLeft => Self::BottomRight,
            Self::BottomRight => Self::TopRight,
            Self::TopRight => Self::TopLeft,
            Self::TopLeft => Self::BottomLeft,
        }
    }
}

/// A side face (end) of a rectangular panel.
///
/// Edge `i` runs from corner `i` to corner `i + 1`, wrapping at four. Thus:
/// edge 1 is the bottom (`-Y`), edge 2 the right (`+X`), edge 3 the top (`+Y`), edge 4 the left
/// (`-X`), viewed from face `A`. Contour panels use [`EdgeId`] directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RectEdge {
    /// Between corners 1 and 2 (`-Y`).
    Bottom = 1,
    /// Between corners 2 and 3 (`+X`).
    Right = 2,
    /// Between corners 3 and 4 (`+Y`).
    Top = 3,
    /// Between corners 4 and 1 (`-X`).
    Left = 4,
}

impl RectEdge {
    /// Edges in counter-clockwise order, following their start corners.
    pub const ALL: [Self; 4] = [Self::Bottom, Self::Right, Self::Top, Self::Left];

    /// The stable id, equal to the human number.
    pub const fn id(self) -> EdgeId {
        EdgeId::from_known(self as u32)
    }

    /// The human number as a string.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bottom => "1",
            Self::Right => "2",
            Self::Top => "3",
            Self::Left => "4",
        }
    }

    /// The corner at which the edge starts.
    pub const fn start(self) -> RectCorner {
        match self {
            Self::Bottom => RectCorner::BottomLeft,
            Self::Right => RectCorner::BottomRight,
            Self::Top => RectCorner::TopRight,
            Self::Left => RectCorner::TopLeft,
        }
    }

    /// The corner at which the edge ends.
    pub const fn end(self) -> RectCorner {
        match self {
            Self::Bottom => RectCorner::BottomRight,
            Self::Right => RectCorner::TopRight,
            Self::Top => RectCorner::TopLeft,
            Self::Left => RectCorner::BottomLeft,
        }
    }

    /// The edge between two adjacent corners, or `None` for a diagonal or equal pair.
    pub const fn between(start: RectCorner, end: RectCorner) -> Option<Self> {
        match (start, end) {
            (RectCorner::BottomLeft, RectCorner::BottomRight) => Some(Self::Bottom),
            (RectCorner::BottomRight, RectCorner::TopRight) => Some(Self::Right),
            (RectCorner::TopRight, RectCorner::TopLeft) => Some(Self::Top),
            (RectCorner::TopLeft, RectCorner::BottomLeft) => Some(Self::Left),
            _ => None,
        }
    }
}

/// The face through which a machining operation enters the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryFace {
    /// A large face, `A` or `B`.
    Face(FaceSide),
    /// A side face identified by its stable edge id.
    Edge(EdgeId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_sides_are_opposites() {
        assert_eq!(FaceSide::A.opposite(), FaceSide::B);
        assert_eq!(FaceSide::B.opposite(), FaceSide::A);
        assert_eq!(FaceSide::A.normal_sign(), 1);
        assert_eq!(FaceSide::B.normal_sign(), -1);
        assert_eq!(FaceSide::A.label(), "A");
    }

    #[test]
    fn rectangle_corners_are_counter_clockwise_from_zero() {
        assert_eq!(RectCorner::ALL.map(RectCorner::label), ["1", "2", "3", "4"]);
        assert_eq!(RectCorner::ALL.map(|corner| corner.id().get()), [1, 2, 3, 4]);
        assert_eq!(RectCorner::BottomLeft.next(), RectCorner::BottomRight);
        assert_eq!(RectCorner::BottomRight.next(), RectCorner::TopRight);
        assert_eq!(RectCorner::TopRight.next(), RectCorner::TopLeft);
        assert_eq!(RectCorner::TopLeft.next(), RectCorner::BottomLeft);
    }

    #[test]
    fn rectangle_edges_connect_consecutive_corners() {
        for edge in RectEdge::ALL {
            assert_eq!(edge.start().next(), edge.end());
            assert_eq!(RectEdge::between(edge.start(), edge.end()), Some(edge));
        }
        assert_eq!(RectEdge::between(RectCorner::BottomLeft, RectCorner::TopLeft), None);
    }

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    #[test]
    fn ids_are_non_zero_and_serialize_as_numbers() {
        assert!(Id::new(0).is_none());
        let id = Id::new(42).expect("non-zero id");
        assert_eq!(id.get(), 42);
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "42");
        assert_eq!(serde_json::from_str::<Id>("42").expect("deserialize id"), id);
        assert!(serde_json::from_str::<Id>("0").is_err());
        assert_eq!(std::mem::size_of::<Option<Id>>(), std::mem::size_of::<u64>());
    }

    #[test]
    fn revisions_are_monotonic_and_do_not_wrap() {
        assert_eq!(Revision::ZERO.get(), 0);
        let one = Revision::ZERO.checked_next().expect("first revision");
        assert_eq!(one.get(), 1);
        assert_eq!(one.checked_next().expect("second revision").get(), 2);

        assert_eq!(Revision(u64::MAX).checked_next(), None);
        assert_eq!(serde_json::from_str::<Revision>("7").expect("deserialize revision").get(), 7);
    }

    #[test]
    fn identity_placement_preserves_points_and_vectors() {
        let point = DVec3::new(1.0, 2.0, 3.0);
        assert_eq!(Placement::IDENTITY.transform_point(point), point);
        assert_eq!(Placement::IDENTITY.transform_vector(DVec3::X), DVec3::X);
    }

    #[test]
    fn placements_compose_translation_and_rotation() {
        let parent = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("parent placement");
        let child = Placement::new(DVec3::new(0.0, 5.0, 0.0), DQuat::IDENTITY).expect("child placement");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::ZERO);
        assert!((point - DVec3::new(10.0, 5.0, 0.0)).length() < 1e-12, "point was {point:?}");

        let parent =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)).expect("rotated parent");
        let child = Placement::new(DVec3::new(1.0, 0.0, 0.0), DQuat::IDENTITY).expect("translated child");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::ZERO);
        assert!((point - DVec3::new(0.0, 1.0, 0.0)).length() < 1e-12, "point was {point:?}");

        let parent =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2)).expect("rotated parent");
        let child =
            Placement::new(DVec3::ZERO, DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)).expect("rotated child");
        let world = Placement::compose(parent, child).expect("world placement");
        let point = world.transform_point(DVec3::Y);
        assert!((point - DVec3::Z).length() < 1e-12, "point was {point:?}");
    }

    #[test]
    fn world_placement_composes_the_chain_and_lists_panels() {
        let mut project = sample_project();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let shelf = Id::new(2).expect("shelf id");
        let root = project.root().id();

        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2))
                    .expect("drawer placement"),
            )
            .expect("place drawer");
        project
            .set_node_placement(
                front,
                Placement::new(DVec3::new(1.0, 0.0, 0.0), DQuat::IDENTITY).expect("front placement"),
            )
            .expect("place front");

        let world = project.world_placement(front).expect("front world placement");
        let origin = world.transform_point(DVec3::ZERO);
        assert!((origin - DVec3::new(10.0, 1.0, 0.0)).length() < 1.0e-12, "origin was {origin:?}");
        let world = project.world_placement(drawer).expect("drawer world placement");
        assert!((world.transform_point(DVec3::ZERO) - DVec3::new(10.0, 0.0, 0.0)).length() < 1.0e-12);
        assert_eq!(project.world_placement(root), Some(Placement::IDENTITY));
        assert_eq!(project.world_placement(Id::new(999).expect("id")), None);

        assert_eq!(project.panel_ids_under(root), Some(vec![shelf, front]));
        assert_eq!(project.panel_ids_under(drawer), Some(vec![front]));
        assert_eq!(project.panel_ids_under(shelf), Some(vec![shelf]));
        assert_eq!(project.panel_ids_under(Id::new(999).expect("id")), None);
    }

    #[test]
    fn pivots_use_the_first_corner_of_the_local_bounds() {
        let mut project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let root = project.root().id();
        project
            .set_node_placement(
                shelf,
                Placement::new(DVec3::new(100.0, 200.0, 300.0), DQuat::IDENTITY).expect("shelf placement"),
            )
            .expect("place shelf");
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("drawer placement"),
            )
            .expect("place drawer");
        project
            .set_node_placement(
                front,
                Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::IDENTITY).expect("front placement"),
            )
            .expect("place front");

        assert_eq!(project.pivot_local(shelf), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(shelf), Some(DVec3::new(100.0, 200.0, 300.0)));
        assert_eq!(project.pivot_local(drawer), Some(DVec3::new(1.0, 2.0, 3.0)));
        assert_eq!(project.pivot_world(drawer), Some(DVec3::new(11.0, 22.0, 33.0)));
        assert_eq!(project.pivot_local(root), Some(DVec3::new(11.0, 22.0, 33.0)));
        assert_eq!(project.pivot_world(root), Some(DVec3::new(11.0, 22.0, 33.0)));

        let empty_placement = Placement::new(DVec3::new(-5.0, 7.0, 9.0), DQuat::IDENTITY).expect("empty placement");
        let empty = project.add_assembly(root, "Empty", empty_placement).expect("add empty assembly");
        assert_eq!(project.pivot_local(empty), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(empty), Some(DVec3::new(-5.0, 7.0, 9.0)));
        assert_eq!(project.pivot_local(Id::new(999).expect("id")), None);
        assert_eq!(project.pivot_world(Id::new(999).expect("id")), None);
    }

    #[test]
    fn a_container_pivot_is_local_and_turns_with_the_container() {
        let mut project = sample_project();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("drawer placement"),
            )
            .expect("place drawer");
        project
            .set_node_placement(
                front,
                Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::IDENTITY).expect("front placement"),
            )
            .expect("place front");
        assert_eq!(project.pivot_local(drawer), Some(DVec3::new(1.0, 2.0, 3.0)));

        let turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), turn).expect("turned drawer placement"),
            )
            .expect("turn drawer");

        let local = project.pivot_local(drawer).expect("local pivot");
        assert!((local - DVec3::new(1.0, 2.0, 3.0)).length() < 1.0e-12, "the local pivot must not jump, was {local:?}");
        let world = project.pivot_world(drawer).expect("world pivot");
        assert!((world - DVec3::new(8.0, 21.0, 33.0)).length() < 1.0e-12, "world pivot was {world:?}");
    }

    #[test]
    fn placements_reject_non_finite_and_non_unit_input() {
        assert_eq!(
            Placement::new(DVec3::new(f64::NAN, 0.0, 0.0), DQuat::IDENTITY),
            Err(PlacementError::NonFiniteTranslation)
        );
        assert_eq!(
            Placement::new(DVec3::ZERO, DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0)),
            Err(PlacementError::NonFiniteRotation)
        );
        assert!(matches!(
            Placement::new(DVec3::ZERO, DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0)),
            Err(PlacementError::NonUnitRotation { length }) if (length - 2.0).abs() < 1e-12
        ));
    }

    #[test]
    fn placements_round_trip_and_validate_on_load() {
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let text = serde_json::to_string(&placement).expect("serialize placement");
        assert!(text.contains(r#""translation":[1.0,2.0,3.0]"#), "json was {text}");
        assert_eq!(serde_json::from_str::<Placement>(&text).expect("deserialize placement"), placement);
        assert!(
            serde_json::from_str::<Placement>(r#"{"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,2.0]}"#).is_err()
        );
        assert!(
            serde_json::from_str::<Placement>(
                r#"{"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0],"extra":1}"#
            )
            .is_err()
        );
    }

    #[test]
    fn placements_round_trip_through_exact_translation_lengths() {
        let placement =
            Placement::new(DVec3::new(-2.5, 0.001, 1200.0), DQuat::from_rotation_y(0.5)).expect("placement");
        let [x, y, z] = placement.translation_lengths().expect("representable translation");
        assert_eq!(x, Length::from_micrometres(-2_500));
        assert_eq!(y, Length::from_micrometres(1));
        assert_eq!(z, Length::from_micrometres(1_200_000));

        let edited = placement.with_translation(x, y, z).expect("rebuild placement");
        assert_eq!(edited, placement);
        assert_eq!(edited.rotation(), placement.rotation());

        let moved = placement
            .with_translation(Length::from_millimetres(10).expect("length"), Length::ZERO, Length::ZERO)
            .expect("move placement");
        assert_eq!(moved.translation(), DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.rotation(), placement.rotation());
    }

    #[test]
    fn translation_lengths_reject_values_outside_the_editable_range() {
        let huge = Placement::new(DVec3::new(1e300, 0.0, 0.0), DQuat::IDENTITY).expect("finite placement");
        assert!(huge.translation_lengths().is_none());
        assert!(Placement::IDENTITY.translation_lengths().is_some());
    }

    #[test]
    fn identity_rotation_has_zero_euler_angles() {
        let angles = Placement::IDENTITY.rotation_euler();
        assert_eq!(angles, EulerAngles::default());
        assert_eq!(angles.x, Degrees::ZERO);
        assert_eq!(angles.y, Degrees::ZERO);
        assert_eq!(angles.z, Degrees::ZERO);
    }

    #[test]
    fn euler_angles_use_intrinsic_xyz_order() {
        let rotation = DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)
            * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_4)
            * DQuat::from_rotation_z(std::f64::consts::FRAC_PI_8);
        let placement = Placement::new(DVec3::ZERO, rotation).expect("placement");
        let angles = placement.rotation_euler();
        assert_eq!(angles.x, Degrees::from_millidegrees(90_000));
        assert_eq!(angles.y, Degrees::from_millidegrees(45_000));
        assert_eq!(angles.z, Degrees::from_millidegrees(22_500));
    }

    #[test]
    fn euler_angles_round_trip_and_preserve_translation() {
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let angles = placement.rotation_euler();
        let rebuilt = placement.with_rotation_euler(angles).expect("rebuild rotation");
        assert_eq!(rebuilt.translation(), placement.translation());
        assert!(rebuilt.rotation().dot(placement.rotation()).abs() > 1.0 - 1.0e-9);

        let turned = placement
            .with_rotation_euler(EulerAngles {
                x: Degrees::from_millidegrees(30_000),
                y: Degrees::from_millidegrees(-15_500),
                z: Degrees::from_millidegrees(90_000),
            })
            .expect("turn placement");
        assert_eq!(turned.translation(), placement.translation());
        assert!((turned.rotation().length() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn euler_angles_at_gimbal_lock_recompose_the_same_rotation() {
        let rotation = DQuat::from_rotation_x(0.4)
            * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2)
            * DQuat::from_rotation_z(0.7);
        let placement = Placement::new(DVec3::ZERO, rotation).expect("placement");
        let angles = placement.rotation_euler();
        let rebuilt = placement.with_rotation_euler(angles).expect("rebuild rotation");
        assert!(rebuilt.rotation().dot(placement.rotation()).abs() > 1.0 - 1.0e-9);
    }

    #[test]
    fn euler_angles_wrap_full_turns_into_the_principal_range() {
        let full_turn = EulerAngles {
            x: Degrees::from_millidegrees(360_000),
            y: Degrees::from_millidegrees(-360_000),
            z: Degrees::from_millidegrees(720_000),
        };
        assert_eq!(full_turn.wrapped(), EulerAngles::default());
        let placement = Placement::IDENTITY.with_rotation_euler(full_turn).expect("wrap placement");
        assert!(placement.same_transform(Placement::IDENTITY));

        let wrapped = EulerAngles {
            x: Degrees::from_millidegrees(540_000),
            y: Degrees::from_millidegrees(-181_000),
            z: Degrees::from_millidegrees(180_000),
        }
        .wrapped();
        assert_eq!(wrapped.x, Degrees::from_millidegrees(180_000));
        assert_eq!(wrapped.y, Degrees::from_millidegrees(179_000));
        assert_eq!(wrapped.z, Degrees::from_millidegrees(180_000));
    }

    #[test]
    fn same_transform_ignores_the_quaternion_sign_and_rejects_real_changes() {
        let rotation = DQuat::from_rotation_z(0.3);
        let placement = Placement::new(DVec3::new(1.0, 2.0, 3.0), rotation).expect("placement");
        let flipped = Placement::new(DVec3::new(1.0, 2.0, 3.0), -rotation).expect("flipped placement");
        assert!(placement.same_transform(flipped));

        let moved = Placement::new(DVec3::new(1.0, 2.0, 4.0), rotation).expect("moved placement");
        assert!(!placement.same_transform(moved));
        let turned = Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::from_rotation_z(0.31)).expect("turned placement");
        assert!(!placement.same_transform(turned));
    }

    fn sample_nominal() -> PanelNominalSize {
        PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid nominal size")
    }

    fn sample_thickness() -> Length {
        millimetres(18)
    }

    fn sample_panel(id: Id, name: &str) -> Panel {
        Panel::new(id, name, sample_nominal(), None, sample_thickness(), Placement::IDENTITY).expect("valid panel")
    }

    fn sample_project() -> Project {
        let shelf = sample_panel(Id::new(2).expect("id"), "Shelf");
        let front = sample_panel(Id::new(4).expect("id"), "Front");
        let drawer = Assembly::new(Id::new(3).expect("id"), "Drawer", Placement::IDENTITY, vec![front.into()]);
        let root = Group::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![shelf.into(), drawer.into()]);
        Project::new(root).expect("valid project")
    }

    fn sample_material() -> MaterialId {
        MaterialId::new(1).expect("material id")
    }

    #[test]
    fn corner_features_and_bows_survive_a_project_round_trip() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner =
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(25) })
                .expect("corner feature");
        project.set_corner_feature(panel_id, corner).expect("set corner");
        project
            .set_edge_bow(panel_id, EdgeBow::new(RectEdge::Top.id(), millimetres(12)).expect("bow"))
            .expect("set bow");

        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert_eq!(panel.corner_feature(RectCorner::TopRight.id()), Some(corner));
        assert_eq!(panel.edge_bow(RectEdge::Top.id()).expect("bow").sagitta(), millimetres(12));

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);

        let geometry = panel.geometry_with_fallbacks().expect("contour geometry");
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(512));
        assert_eq!(geometry.overall_width(), millimetres(512));
    }

    #[test]
    fn shaping_keeps_banding_and_machining() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = CornerFeature::new(
            RectCorner::BottomLeft.id(),
            CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(20) },
        )
        .expect("corner feature");

        project
            .add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
                CutoutDepth::Through,
                FaceSide::A,
            )
            .expect("cutout on the rectangular panel");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(20))
                    .expect("banding"),
            )
            .expect("set banding");
        project.set_corner_feature(panel_id, corner).expect("shape a banded panel with machining");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert_eq!(panel.machining().len(), 1);
        assert!(panel.edge_banding().iter().any(|band| band.edge() == RectEdge::Bottom.id()));
        panel.geometry_with_fallbacks().expect("shaped geometry with machining");

        // Adding new machining to a shaped panel is still the staged contour rule.
        assert!(matches!(
            project.add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(200, 200, 100, 50)),
                CutoutDepth::Through,
                FaceSide::A
            ),
            Err(ProjectError::ContourMachiningUnsupported { panel }) if panel == panel_id
        ));
    }

    #[test]
    fn merging_paths_keeps_the_existing_banding() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(20))
                    .expect("banding"),
            )
            .expect("band the right edge");
        let radius =
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius");
        project.set_corner_feature(panel_id, radius).expect("shape a banded panel");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert!(panel.edge_banding().iter().any(|band| band.edge() == RectEdge::Right.id()));
        panel.geometry_with_fallbacks().expect("rounded banded geometry");
    }

    #[test]
    fn corner_banding_requires_a_segment_and_goes_with_it() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::BottomLeft.id();
        let band = CornerBanding::new(corner, sample_material(), false, millimetres(2), millimetres(22))
            .expect("corner banding");
        assert_eq!(
            project.set_corner_banding(panel_id, band.clone()),
            Err(ProjectError::CornerFeatureNotFound { panel: panel_id, corner })
        );

        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(15) },
                )
                .expect("chamfer"),
            )
            .expect("shape the corner");
        project.set_corner_banding(panel_id, band).expect("band the chamfer segment");
        assert!(project.panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        project.clear_corner_feature(panel_id, corner).expect("remove the segment");
        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.corner_feature(corner).is_none());
        assert!(panel.corner_banding_at(corner).is_none());
    }

    #[test]
    fn shaped_panel_with_machining_resizes() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: millimetres(8) })
                    .expect("radius"),
            )
            .expect("shape the panel");
        let bigger = PanelNominalSize::new(millimetres(900), millimetres(600)).expect("valid size");
        project.set_panel_nominal_size(panel_id, bigger).expect("resize a shaped panel with machining");
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.nominal_size(), bigger);
        assert!(panel.geometry_with_fallbacks().is_ok());
    }

    #[test]
    fn per_segment_banding_insets_only_the_banded_segments() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        for corner in RectCorner::ALL {
            project
                .set_corner_feature(
                    panel_id,
                    CornerFeature::new(corner.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                        .expect("radius"),
                )
                .expect("set radius");
        }
        for edge in RectEdge::ALL {
            project
                .set_edge_banding(
                    panel_id,
                    EdgeBanding::new(edge.id(), sample_material(), true, millimetres(2), millimetres(22))
                        .expect("band"),
                )
                .expect("band the edge");
        }
        for corner in RectCorner::ALL {
            project
                .set_corner_banding(
                    panel_id,
                    CornerBanding::new(corner.id(), sample_material(), true, millimetres(2), millimetres(22))
                        .expect("corner band"),
                )
                .expect("band the corner");
        }
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.band_order().len(), 8);
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("non-included band");
        let panel = project.panel(panel_id).expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(502));
    }

    #[test]
    fn band_order_tracks_application_and_parameter_edits() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let bottom = EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(22))
            .expect("bottom");
        let top = EdgeBanding::new(RectEdge::Top.id(), sample_material(), true, millimetres(2), millimetres(22))
            .expect("top");
        project.set_edge_banding(panel_id, bottom.clone()).expect("band bottom");
        project.set_edge_banding(panel_id, top.clone()).expect("band top");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Edge(RectEdge::Top.id())]
        );

        let edited = EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), false, millimetres(2), millimetres(22))
            .expect("bottom");
        project.set_edge_banding(panel_id, edited.clone()).expect("edit bottom");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Edge(RectEdge::Top.id())],
            "a parameter edit keeps the order"
        );

        project.clear_edge_banding(panel_id, RectEdge::Bottom.id()).expect("clear bottom");
        assert_eq!(project.panel(panel_id).expect("panel").band_order(), &[SegmentSource::Edge(RectEdge::Top.id())]);
        project.set_edge_banding(panel_id, edited).expect("re-add bottom");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Top.id()), SegmentSource::Edge(RectEdge::Bottom.id())]
        );
    }

    #[test]
    fn corner_banding_joins_the_order_and_leaves_with_its_segment() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(22))
                    .expect("bottom"),
            )
            .expect("band bottom");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(15) },
                )
                .expect("chamfer"),
            )
            .expect("shape the corner");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, sample_material(), false, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("band the corner");
        assert_eq!(
            project.panel(panel_id).expect("panel").band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Corner(corner)]
        );

        project.clear_corner_feature(panel_id, corner).expect("remove the corner segment");
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.band_order(), &[SegmentSource::Edge(RectEdge::Bottom.id())]);
        assert!(panel.corner_banding_at(corner).is_none());
    }

    #[test]
    fn schema_three_banding_migrates_to_an_ordered_segment_list() {
        let value = serde_json::json!({
            "schema_version": 3,
            "revision": 0,
            "next_id": 20,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Panel",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "last_known_thickness": 18000,
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "edge_banding": [{
                        "edge": 1,
                        "material": 1,
                        "included_in_size": true,
                        "last_known_thickness": 2000,
                        "last_known_width": 22000
                    }],
                    "corner_banding": [{
                        "corner": 2,
                        "material": 1,
                        "included_in_size": false,
                        "last_known_thickness": 2000,
                        "last_known_width": 22000
                    }],
                    "corner_features": [{"corner": 2, "feature": {"kind": "chamfer", "incoming": 10000, "outgoing": 15000}}]
                }}]
            }
        });
        let project: Project = serde_json::from_value(value).expect("v3 project loads");
        let panel = project.panel(Id::new(2).expect("id")).expect("panel");
        assert_eq!(
            panel.band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Corner(RectCorner::BottomRight.id())]
        );
    }

    #[test]
    fn a_partially_banded_path_is_reported() {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Shaped");
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius"),
        );
        panel.edge_banding.push(
            EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                .expect("band"),
        );
        assert!(panel.has_path_banding_conflict().expect("paths"));
        panel.edge_banding.push(
            EdgeBanding::new(RectEdge::Top.id(), sample_material(), false, millimetres(2), millimetres(22))
                .expect("band"),
        );
        assert!(!panel.has_path_banding_conflict().expect("paths"));
    }

    #[test]
    fn clearing_an_unbanded_edge_is_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let Some((_depth, panel)) = find_panel_mut(&mut project.root, panel_id, 1) else {
            return;
        };
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                .expect("radius"),
        );
        panel.edge_banding.push(
            EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                .expect("band"),
        );
        assert_eq!(
            project.clear_edge_banding(panel_id, RectEdge::Top.id()),
            Err(ProjectError::EdgeBandingNotFound { panel: panel_id, edge: RectEdge::Top.id() })
        );
        assert_eq!(project.panel(panel_id).expect("panel").edge_banding().len(), 1);
    }

    #[test]
    fn corner_banding_load_validation_rejects_inconsistent_records() {
        let base = |corner_features: serde_json::Value, corner_banding: serde_json::Value| {
            serde_json::json!({
                "id": 2,
                "name": "Panel",
                "nominal_size": {"length": 700000, "width": 500000},
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "last_known_thickness": 18000,
                "corner_features": corner_features,
                "corner_banding": corner_banding,
            })
        };
        let chamfer =
            serde_json::json!([{"corner": 2, "feature": {"kind": "chamfer", "incoming": 10000, "outgoing": 20000}}]);
        let band = serde_json::json!([{
            "corner": 2,
            "material": 1,
            "included_in_size": true,
            "last_known_thickness": 2000,
            "last_known_width": 22000
        }]);
        let panel: Panel = serde_json::from_value(base(chamfer.clone(), band.clone())).expect("valid panel");
        let corner = RectCorner::BottomRight.id();
        assert!(panel.corner_banding_at(corner).is_some());

        let duplicate = serde_json::json!([band[0].clone(), band[0].clone(),]);
        let duplicate: PanelWire = serde_json::from_value(base(chamfer.clone(), duplicate)).expect("panel wire");
        assert_eq!(Panel::try_from(duplicate), Err(PanelError::DuplicateCornerBanding { corner }));

        let radius = serde_json::json!([{"corner": 2, "feature": {"kind": "radius", "radius": 10000}}]);
        let radius: PanelWire = serde_json::from_value(base(radius, band.clone())).expect("panel wire");
        let radius = Panel::try_from(radius).expect("corner banding survives a radius");
        assert!(radius.corner_banding_at(corner).is_some());

        let outside = serde_json::json!({"corner": 5, "material": 1, "included_in_size": true, "last_known_thickness": 2000, "last_known_width": 22000});
        let outside: PanelWire =
            serde_json::from_value(base(serde_json::json!([]), serde_json::json!([outside]))).expect("panel wire");
        assert_eq!(
            Panel::try_from(outside),
            Err(PanelError::UnsupportedCornerBanding { corner: VertexId::new(5).expect("corner") })
        );
    }

    #[test]
    fn shaped_panel_with_machining_resolves_geometry() {
        let mut panel = sample_panel(Id::new(2).expect("id"), "Shaped");
        panel.corner_features.push(
            CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: millimetres(8) })
                .expect("radius"),
        );
        panel.machining.push(Machining::Cutout(Cutout::new(
            Id::new(5).expect("id"),
            CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
            CutoutDepth::Through,
            FaceSide::A,
        )));
        let geometry = panel.geometry_with_fallbacks().expect("shaped geometry with machining");
        assert!(geometry.core_length().is_positive());
        assert!(geometry.core_width().is_positive());
    }

    #[test]
    fn panel_load_rejects_duplicate_and_out_of_range_contour_features() {
        let base = |corner_features: serde_json::Value, edge_bows: serde_json::Value| {
            serde_json::json!({
                "id": 2,
                "name": "Panel",
                "nominal_size": {"length": 700000, "width": 500000},
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "last_known_thickness": 18000,
                "corner_features": corner_features,
                "edge_bows": edge_bows,
            })
        };

        let duplicate = base(
            serde_json::json!([
                {"corner": 1, "feature": {"kind": "radius", "radius": 10000}},
                {"corner": 1, "feature": {"kind": "chamfer", "incoming": 10000, "outgoing": 20000}}
            ]),
            serde_json::json!([]),
        );
        let duplicate: PanelWire = serde_json::from_value(duplicate).expect("panel wire");
        assert_eq!(
            Panel::try_from(duplicate),
            Err(PanelError::DuplicateCornerFeature { corner: RectCorner::BottomLeft.id() })
        );

        let unsupported = base(
            serde_json::json!([{"corner": 5, "feature": {"kind": "radius", "radius": 10000}}]),
            serde_json::json!([]),
        );
        let unsupported: PanelWire = serde_json::from_value(unsupported).expect("panel wire");
        assert_eq!(
            Panel::try_from(unsupported),
            Err(PanelError::UnsupportedCornerFeature { corner: VertexId::new(5).expect("corner") })
        );

        let bad_bow = base(
            serde_json::json!([]),
            serde_json::json!([{"edge": 1, "sagitta": 1000}, {"edge": 1, "sagitta": 2000}]),
        );
        let bad_bow: PanelWire = serde_json::from_value(bad_bow).expect("panel wire");
        assert_eq!(Panel::try_from(bad_bow), Err(PanelError::DuplicateEdgeBow { edge: RectEdge::Bottom.id() }));
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

        let assembly_id = project.add_assembly(root_id, "Carcass", Placement::IDENTITY).expect("add assembly");
        assert_eq!(assembly_id.get(), 6);
        assert_eq!(project.revision().get(), 2);
        assert_eq!(project.assembly(assembly_id).expect("new assembly").name(), "Carcass");
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

    fn sample_rect(x: i64, y: i64, width: i64, height: i64) -> PanelRect {
        PanelRect::new(millimetres(x), millimetres(y), millimetres(width), millimetres(height))
            .expect("valid cutout rectangle")
    }

    #[test]
    fn project_adds_cutouts_and_allocates_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));

        let cutout_id = project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        let panel = project.panel(panel_id).expect("panel exists");
        assert_eq!(panel.machining().len(), 1);
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), shape);
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
            project.add_cutout(panel_id, shape, CutoutDepth::Blind { depth: Length::ZERO }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid { panel: panel_id, depth: Length::ZERO, thickness: millimetres(18) })
        );
        assert_eq!(
            project.add_cutout(panel_id, shape, CutoutDepth::Blind { depth: millimetres(18) }, FaceSide::A),
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
        let cutout_id = project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
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
            let assembly = Assembly::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![assembly.into()];
        }
        let root = Group::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        let deepest = deepest.expect("at least one nested assembly");
        let mut project = Project::new(root).expect("tree at the depth limit");
        assert_eq!(
            project.add_assembly(deepest, "Too deep", Placement::IDENTITY),
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
        let root = Group::new(duplicate, "Root", Placement::IDENTITY, vec![panel.into()]);
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(duplicate)));
    }

    #[test]
    fn projects_reject_excessive_nesting() {
        let mut nested = Vec::new();
        for index in 0..Project::MAX_ASSEMBLY_DEPTH {
            let id = Id::new(u64::from(index) + 2).expect("id");
            let assembly = Assembly::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![assembly.into()];
        }
        let root = Group::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        assert!(matches!(
            Project::new(root),
            Err(ProjectError::TooDeep { max }) if max == Project::MAX_ASSEMBLY_DEPTH
        ));
    }

    #[test]
    fn projects_reject_unsupported_schema_and_unknown_fields() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");

        let newer = text.replacen("\"schema_version\":5", "\"schema_version\":6", 1);
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
        assert!(project.group(root).is_some());
        assert!(project.assembly(root).is_none());
        assert!(project.assembly(drawer).is_some());
        assert_eq!(project.parent_of(drawer), Some(root));
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
        assert_eq!(cutout.shape(), CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));

        let text = serde_json::to_string(&project).expect("serialize migrated project");
        let decoded: Project = serde_json::from_str(&text).expect("reload migrated project");
        let panel = decoded.panel(Id::new(2).expect("id")).expect("reloaded panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));
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
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let panel = project.panel(panel_id).expect("panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), shape);

        // Nominal coordinates: the 2 mm included bands put the core origin at (2, 2), so this
        // cutout lies entirely in the band and would remove no core material.
        let under_the_band = CutoutShape::Rectangle(sample_rect(0, 0, 1, 1));
        assert_eq!(
            project.add_cutout(panel_id, under_the_band, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn panel_nominal_size_accepts_positive_dimensions() {
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        assert_eq!(size.length(), millimetres(700));
        assert_eq!(size.width(), millimetres(500));
    }

    #[test]
    fn panel_nominal_size_rejects_non_positive_and_oversized_dimensions() {
        let zero = Length::ZERO;
        let negative = millimetres(-1);
        assert_eq!(
            PanelNominalSize::new(zero, millimetres(500)),
            Err(PanelDimensionError::NonPositive { field: "length", value: zero })
        );
        assert_eq!(
            PanelNominalSize::new(millimetres(700), negative),
            Err(PanelDimensionError::NonPositive { field: "width", value: negative })
        );

        let too_long = Length::from_micrometres(Panel::MAX_LENGTH.micrometres() + 1);
        assert_eq!(
            PanelNominalSize::new(too_long, millimetres(500)),
            Err(PanelDimensionError::TooLarge { field: "length", value: too_long, max: Panel::MAX_LENGTH })
        );

        let too_wide = Length::from_micrometres(Panel::MAX_WIDTH.micrometres() + 1);
        assert_eq!(
            PanelNominalSize::new(millimetres(700), too_wide),
            Err(PanelDimensionError::TooLarge { field: "width", value: too_wide, max: Panel::MAX_WIDTH })
        );

        assert!(PanelNominalSize::new(Panel::MAX_LENGTH, Panel::MAX_WIDTH).is_ok());
    }

    #[test]
    fn panel_nominal_size_serde_round_trip_and_validation() {
        let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size");
        let text = serde_json::to_string(&size).expect("serialize panel size");
        assert_eq!(text, r#"{"length":700000,"width":500000}"#);
        assert_eq!(serde_json::from_str::<PanelNominalSize>(&text).expect("deserialize panel size"), size);

        assert!(serde_json::from_str::<PanelNominalSize>(r#"{"length":0,"width":500000}"#).is_err());
        assert!(serde_json::from_str::<PanelNominalSize>(r#"{"length":10000001,"width":500000}"#).is_err());
        assert!(
            serde_json::from_str::<PanelNominalSize>(r#"{"length":700000,"width":500000,"unexpected":1}"#).is_err()
        );
    }

    #[test]
    fn panel_geometry_uses_nominal_size_and_thickness() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let geometry = panel.geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(500));
        assert_eq!(geometry.thickness(), millimetres(18));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn all_included_edges_inset_the_core_inside_the_nominal_envelope() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        for edge in 1..=4 {
            let band =
                EdgeBanding::new(EdgeId::new(edge).expect("edge id"), material, true, millimetres(2), millimetres(22))
                    .expect("valid banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }

        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(696));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn panel_geometry_rejects_a_negative_core_origin() {
        let error = PanelGeometry::new(
            millimetres(-1),
            Length::ZERO,
            millimetres(700),
            millimetres(500),
            sample_thickness(),
            millimetres(700),
            millimetres(500),
        )
        .expect_err("negative core origin");
        assert!(matches!(error, PanelGeometryError::NegativeOrigin { field: "core_x", .. }));
    }

    #[test]
    fn panel_geometry_accepts_catalogue_resolved_banding_thickness() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        let bottom =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(2), millimetres(22))
                .expect("valid bottom banding");
        project.set_edge_banding(panel_id, bottom).expect("set bottom banding");
        let panel = project.panel(panel_id).expect("panel");

        let geometry = panel.geometry_with_banding(sample_thickness(), |_| millimetres(4)).expect("resolved geometry");
        assert_eq!(geometry.core_y(), millimetres(4));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_width(), millimetres(500));
    }

    #[test]
    fn panel_geometry_respects_included_banding_and_application_order() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");

        let bottom =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(2), millimetres(22))
                .expect("valid bottom banding");
        project.set_edge_banding(panel_id, bottom).expect("set bottom banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), Length::ZERO);
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let left = EdgeBanding::new(EdgeId::new(4).expect("edge id"), material, false, millimetres(1), millimetres(22))
            .expect("valid left banding");
        project.set_edge_banding(panel_id, left).expect("set left banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), Length::ZERO);
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(700));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(701));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let left_included =
            EdgeBanding::new(EdgeId::new(4).expect("edge id"), material, true, millimetres(1), millimetres(22))
                .expect("valid left banding");
        project.set_edge_banding(panel_id, left_included).expect("set left banding");
        let geometry = project.panel(panel_id).expect("panel").geometry(sample_thickness()).expect("resolved geometry");
        assert_eq!(geometry.core_x(), millimetres(1));
        assert_eq!(geometry.core_y(), millimetres(2));
        assert_eq!(geometry.core_length(), millimetres(699));
        assert_eq!(geometry.core_width(), millimetres(498));
        assert_eq!(geometry.overall_length(), millimetres(700));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let bands = project.panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0].edge().get(), 1);
        assert_eq!(bands[1].edge().get(), 4);
    }

    #[test]
    fn panel_banding_cannot_make_the_core_non_positive() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        let too_wide =
            EdgeBanding::new(EdgeId::new(1).expect("edge id"), material, true, millimetres(500), millimetres(22))
                .expect("valid banding dimensions");
        assert!(matches!(
            project.set_edge_banding(panel_id, too_wide),
            Err(ProjectError::PanelGeometry(PanelGeometryError::NonPositive { field: "core_width", .. }))
        ));
        assert!(project.panel(panel_id).expect("panel").edge_banding().is_empty());
    }

    #[test]
    fn panel_banding_serde_round_trip_and_validation() {
        let band = EdgeBanding::new(
            EdgeId::new(1).expect("edge id"),
            MaterialId::new(99).expect("material id"),
            true,
            millimetres(2),
            millimetres(22),
        )
        .expect("valid banding");
        let text = serde_json::to_string(&band).expect("serialize banding");
        assert_eq!(serde_json::from_str::<EdgeBanding>(&text).expect("deserialize banding"), band);
        assert!(
            serde_json::from_str::<EdgeBanding>(
                r#"{"edge":1,"material":99,"included_in_size":true,"last_known_thickness":0,"last_known_width":22000}"#
            )
            .is_err()
        );
    }

    #[test]
    fn ids_reject_zero_and_serialize_as_numbers() {
        assert!(VertexId::new(0).is_none());
        assert!(EdgeId::new(0).is_none());
        assert!(serde_json::from_str::<VertexId>("0").is_err());
        assert!(serde_json::from_str::<EdgeId>("0").is_err());

        let id = EdgeId::new(3).expect("non-zero edge id");
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "3");
        let decoded: EdgeId = serde_json::from_str("3").expect("deserialize id");
        assert_eq!(decoded, id);
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
        assert_eq!(project.assembly(drawer).expect("assembly").name(), "Drawer box");
        assert_eq!(project.root().name(), "Cabinet");

        assert_eq!(project.set_node_placement(panel, placement).expect("place panel").get(), 4);
        assert_eq!(project.set_node_placement(drawer, placement).expect("place assembly").get(), 5);
        assert_eq!(project.panel(panel).expect("panel").placement(), placement);
        assert_eq!(project.assembly(drawer).expect("assembly").placement(), placement);

        assert_eq!(project.set_node_placement(root, placement), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision().get(), 5);
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
        assert!(project.assembly(drawer).is_none());
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
        let assembly = project.assembly(copied_drawer).expect("copied assembly");
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
        let group = project.group(group_id).expect("group");
        assert_eq!(group.placement(), Placement::IDENTITY);
        assert_eq!(group.children().len(), 2);
        assert_eq!(group.children()[0].id(), panel);
        assert_eq!(group.children()[1].id(), drawer);
        assert_eq!(project.parent_of(panel), Some(group_id));
        assert_eq!(project.parent_of(drawer), Some(group_id));
        assert_eq!(project.panel(panel).expect("panel").placement(), panel_placement);
        assert_eq!(project.assembly(drawer).expect("drawer").placement(), drawer_placement);
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
        assert!(project.group(group_id).is_none());
        assert_eq!(project.parent_of(panel), Some(root));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.parent_of(front), Some(drawer));
        assert_eq!(project.root().children().len(), 2);

        let expected_panel = Placement::compose(group_placement, panel_placement).expect("panel world");
        assert_eq!(project.panel(panel).expect("panel").placement(), expected_panel);
        let expected_drawer = Placement::compose(group_placement, drawer_placement).expect("drawer world");
        assert_eq!(project.assembly(drawer).expect("drawer").placement(), expected_drawer);
        let expected_front = Placement::compose(expected_drawer, front_placement).expect("front world");
        assert_eq!(project.world_placement(front).expect("front world"), expected_front);
        assert_eq!(project.panel(front).expect("front").placement(), front_placement);

        assert_eq!(project.ungroup_group(drawer), Err(ProjectError::NotAGroup { id: drawer }));
        assert_eq!(project.ungroup_group(panel), Err(ProjectError::NotAGroup { id: panel }));
        assert_eq!(project.ungroup_group(root), Err(ProjectError::RootImmutable { id: root }));
    }
}
