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

use furnicraft_geom::{DQuat, DVec3};
use furnicraft_units::Length;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

mod catalog;
mod panel_contour;
mod panel_features;

pub use catalog::{Catalog, CatalogError, EdgeBandingMaterial, MaterialId, PanelMaterial};
pub use panel_contour::PanelContourError;
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

/// The root of a saved document: its schema version, revision, id allocation and assembly tree.
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
    root: Assembly,
}

impl Project {
    /// The schema version written by this build.
    pub const CURRENT_SCHEMA_VERSION: u32 = 2;

    /// Maximum nesting depth of assemblies, including the root at depth one.
    pub const MAX_ASSEMBLY_DEPTH: u32 = 64;

    /// Creates a project from a root assembly and validates its tree.
    pub fn new(root: Assembly) -> Result<Self, ProjectError> {
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

    /// The root assembly.
    pub const fn root(&self) -> &Assembly {
        &self.root
    }

    /// Finds an assembly anywhere in the tree.
    pub fn assembly(&self, id: Id) -> Option<&Assembly> {
        find_assembly_with_depth(&self.root, id, 1).map(|(_, assembly)| assembly)
    }

    /// Finds a panel anywhere in the tree.
    pub fn panel(&self, id: Id) -> Option<&Panel> {
        find_panel_with_depth(&self.root, id, 1).map(|(_, panel)| panel)
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
        candidate.upsert_edge_banding(band);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces banding on an edge and moves it to the end of the application order.
    pub fn set_edge_banding(&mut self, id: Id, band: EdgeBanding) -> Result<Revision, ProjectError> {
        self.validate_edge_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.upsert_edge_banding(band);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from an edge and bumps the revision once.
    pub fn clear_edge_banding(&mut self, id: Id, edge: EdgeId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if !panel.edge_banding().iter().any(|band| band.edge() == edge) {
            return Err(ProjectError::EdgeBandingNotFound { panel: id, edge });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_edge_banding(edge);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a corner treatment can be applied without invalidating the panel.
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
    pub fn clear_corner_feature(&mut self, id: Id, corner: VertexId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.corner_feature(corner).is_none() {
            return Err(ProjectError::CornerFeatureNotFound { panel: id, corner });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_corner_feature(corner);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a bowed edge can be applied without invalidating the panel.
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

    /// Adds a panel to an existing assembly and bumps the revision once.
    ///
    /// The panel id is allocated from the document; callers must not invent one.
    pub fn add_panel(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        nominal_size: PanelNominalSize,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if self.assembly(parent).is_none() {
            return Err(ProjectError::AssemblyNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let panel = Panel::new(id, name, nominal_size, material, last_known_thickness, placement)?;
        let Some((_depth, assembly)) = find_assembly_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::AssemblyNotFound { id: parent });
        };
        assembly.children.push(Node::Panel(panel));
        self.revision = next_revision;
        Ok(id)
    }

    /// Adds an empty nested assembly and bumps the revision once.
    ///
    /// Adding a child below [`Project::MAX_ASSEMBLY_DEPTH`] is rejected.
    pub fn add_assembly(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        let Some((depth, _)) = find_assembly_with_depth(&self.root, parent, 1) else {
            return Err(ProjectError::AssemblyNotFound { id: parent });
        };
        if depth >= Self::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Self::MAX_ASSEMBLY_DEPTH });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let assembly = Assembly::new(id, name, placement, Vec::new());
        let Some((_depth, parent_assembly)) = find_assembly_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::AssemblyNotFound { id: parent });
        };
        parent_assembly.children.push(Node::Assembly(assembly));
        self.revision = next_revision;
        Ok(id)
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
}

/// Validates the tree and returns the largest id it uses.
fn scan_tree(root: &Assembly) -> Result<u64, ProjectError> {
    let mut seen = HashSet::new();
    let mut max_id = 0u64;
    let mut stack = vec![(root, 1u32)];
    while let Some((assembly, depth)) = stack.pop() {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        if !seen.insert(assembly.id.get()) {
            return Err(ProjectError::DuplicateId(assembly.id));
        }
        max_id = max_id.max(assembly.id.get());
        for child in &assembly.children {
            match child {
                Node::Assembly(child) => stack.push((child, depth.saturating_add(1))),
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

fn find_assembly_with_depth(assembly: &Assembly, id: Id, depth: u32) -> Option<(u32, &Assembly)> {
    if assembly.id == id {
        return Some((depth, assembly));
    }
    for child in &assembly.children {
        if let Node::Assembly(child) = child
            && let Some(found) = find_assembly_with_depth(child, id, depth.saturating_add(1))
        {
            return Some(found);
        }
    }
    None
}

fn find_assembly_mut(assembly: &mut Assembly, id: Id, depth: u32) -> Option<(u32, &mut Assembly)> {
    if assembly.id == id {
        return Some((depth, assembly));
    }
    for child in &mut assembly.children {
        if let Node::Assembly(child) = child
            && let Some(found) = find_assembly_mut(child, id, depth.saturating_add(1))
        {
            return Some(found);
        }
    }
    None
}

fn find_panel_with_depth(assembly: &Assembly, id: Id, depth: u32) -> Option<(u32, &Panel)> {
    for child in &assembly.children {
        match child {
            Node::Panel(panel) if panel.id == id => return Some((depth, panel)),
            Node::Assembly(child) => {
                if let Some(found) = find_panel_with_depth(child, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
}

fn find_panel_mut(assembly: &mut Assembly, id: Id, depth: u32) -> Option<(u32, &mut Panel)> {
    for child in &mut assembly.children {
        match child {
            Node::Panel(panel) if panel.id == id => return Some((depth, panel)),
            Node::Assembly(child) => {
                if let Some(found) = find_panel_mut(child, id, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            Node::Panel(_) => {}
        }
    }
    None
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
    if panel.has_contour_features() && !panel.machining().is_empty() {
        return Err(ProjectError::ContourMachiningUnsupported { panel: panel.id() });
    }
    let mut validated: Vec<&Cutout> = Vec::new();
    for machining in panel.machining() {
        match machining {
            Machining::Cutout(cutout) => {
                validate_cutout_geometry(panel.id(), geometry, cutout.shape(), cutout.depth())?;
                for other in &validated {
                    if rects_overlap(cutout_rect(cutout.shape()), cutout_rect(other.shape())) {
                        return Err(ProjectError::CutoutOverlap { panel: panel.id(), other: other.id() });
                    }
                }
                validated.push(cutout);
            }
        }
    }
    Ok(())
}

fn validate_new_cutout(panel: &Panel, shape: CutoutShape, depth: CutoutDepth) -> Result<(), ProjectError> {
    if panel.has_contour_features() {
        return Err(ProjectError::ContourMachiningUnsupported { panel: panel.id() });
    }
    let geometry = panel.geometry_with_fallbacks()?;
    validate_cutout_geometry(panel.id(), geometry, shape, depth)?;
    for machining in panel.machining() {
        match machining {
            Machining::Cutout(existing) => {
                if rects_overlap(cutout_rect(shape), cutout_rect(existing.shape())) {
                    return Err(ProjectError::CutoutOverlap { panel: panel.id(), other: existing.id() });
                }
            }
        }
    }
    Ok(())
}

fn validate_cutout_geometry(
    panel: Id,
    geometry: PanelGeometry,
    shape: CutoutShape,
    depth: CutoutDepth,
) -> Result<(), ProjectError> {
    let rect = cutout_rect(shape);
    if !rect_inside(rect, geometry) {
        return Err(ProjectError::CutoutOutsidePanel { panel });
    }
    if let CutoutDepth::Blind { depth } = depth {
        let thickness = geometry.thickness();
        if !depth.is_positive() || depth >= thickness {
            return Err(ProjectError::CutoutDepthInvalid { panel, depth, thickness });
        }
    }
    Ok(())
}

fn cutout_rect(shape: CutoutShape) -> PanelRect {
    match shape {
        CutoutShape::Rectangle(rect) => rect,
    }
}

fn rect_inside(rect: PanelRect, geometry: PanelGeometry) -> bool {
    let x0 = rect.x().micrometres() as i128;
    let y0 = rect.y().micrometres() as i128;
    let x1 = x0 + rect.width().micrometres() as i128;
    let y1 = y0 + rect.height().micrometres() as i128;
    let panel_x = geometry.core_length().micrometres() as i128;
    let panel_y = geometry.core_width().micrometres() as i128;
    x0 >= 0 && y0 >= 0 && x1 <= panel_x && y1 <= panel_y
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
    /// The assembly tree is nested more deeply than the supported limit.
    #[error("assembly nesting exceeds {max} levels")]
    TooDeep {
        /// The maximum supported depth.
        max: u32,
    },
    /// The requested parent assembly is not in the tree.
    #[error("assembly {id} not found")]
    AssemblyNotFound {
        /// The missing assembly id.
        id: Id,
    },
    /// The requested panel is not in the tree.
    #[error("panel {id} not found")]
    PanelNotFound {
        /// The missing panel id.
        id: Id,
    },
    /// A stored panel dimension is not usable.
    #[error(transparent)]
    PanelDimension(#[from] PanelDimensionError),
    /// The panel cannot be resolved into core, overall and thickness.
    #[error(transparent)]
    PanelGeometry(#[from] PanelGeometryError),
    /// Edge banding refers to an edge outside the rectangular v1 outline.
    #[error("edge banding on panel {panel} uses unsupported edge {edge}")]
    UnsupportedEdgeBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The rejected edge.
        edge: EdgeId,
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
    /// The requested edge has no bow to clear.
    #[error("panel {panel} has no bow on edge {edge}")]
    EdgeBowNotFound {
        /// The panel that owns the bow.
        panel: Id,
        /// The edge that was requested.
        edge: EdgeId,
    },
    /// A cutout is not fully inside the panel outline.
    #[error("cutout on panel {panel} lies outside the panel")]
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
    /// Two cutouts on the same panel overlap.
    #[error("cutout on panel {panel} overlaps cutout {other}")]
    CutoutOverlap {
        /// The panel that owns the cutouts.
        panel: Id,
        /// The other cutout in the conflicting pair.
        other: Id,
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
    root: Assembly,
}

impl TryFrom<ProjectWire> for Project {
    type Error = ProjectError;

    fn try_from(wire: ProjectWire) -> Result<Self, Self::Error> {
        // Version 1 stored a `size` on every panel; `PanelWire` migrates it to nominal size plus a
        // fallback. Both versions are normalized to the current one on load.
        if wire.schema_version != 1 && wire.schema_version != Project::CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: wire.schema_version,
                supported: Project::CURRENT_SCHEMA_VERSION,
            });
        }
        let max_id = scan_tree(&wire.root)?;
        let next_id = match wire.next_id {
            Some(next_id) if next_id.get() <= max_id => {
                return Err(ProjectError::NextIdNotAfterMax { next: next_id, max: max_id });
            }
            Some(next_id) => next_id,
            None => next_id_after(max_id)?,
        };
        Ok(Self { schema_version: Project::CURRENT_SCHEMA_VERSION, revision: wire.revision, next_id, root: wire.root })
    }
}

/// A node in the product tree.
///
/// An assembly groups panels and nested assemblies. In v1 every node is unique and owns its
/// children; shared definitions and instances are deferred.
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

    /// The panels and nested assemblies in this assembly.
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
            corner_features: Vec::new(),
            edge_bows: Vec::new(),
        })
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

    /// Machining owned by this panel, in core-local coordinates.
    pub fn machining(&self) -> &[Machining] {
        &self.machining
    }

    /// Edge banding in application order.
    pub fn edge_banding(&self) -> &[EdgeBanding] {
        &self.edge_banding
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
    /// catalogue. `band_thickness` is called once per band in application order.
    pub fn geometry_with_banding<F>(
        &self,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&EdgeBanding) -> Length,
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
        self.geometry_for_with(nominal_size, thickness, EdgeBanding::last_known_thickness)
    }

    /// Resolves geometry for a nominal size with a caller-provided banding thickness.
    pub fn geometry_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        thickness: Length,
        band_thickness: F,
    ) -> Result<PanelGeometry, PanelGeometryError>
    where
        F: Fn(&EdgeBanding) -> Length,
    {
        validate_dimension("thickness", thickness, Panel::MAX_THICKNESS).map_err(PanelGeometryError::Dimension)?;

        if self.has_contour_features() {
            if !self.edge_banding.is_empty() {
                return Err(PanelGeometryError::UnsupportedContourBanding);
            }
            if !self.machining.is_empty() {
                return Err(PanelGeometryError::UnsupportedContourMachining);
            }
            let contour = self.nominal_contour_for(nominal_size).map_err(|_| PanelGeometryError::Contour)?;
            let (min, max) = contour.bounds().ok_or(PanelGeometryError::Contour)?;
            let core_length = length_from_millimetres(max.x - min.x)?;
            let core_width = length_from_millimetres(max.y - min.y)?;
            return PanelGeometry::new(
                Length::ZERO,
                Length::ZERO,
                core_length,
                core_width,
                thickness,
                core_length,
                core_width,
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
            let thickness = band_thickness(band);
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

    fn upsert_edge_banding(&mut self, band: EdgeBanding) {
        let edge = band.edge();
        self.edge_banding.retain(|existing| existing.edge() != edge);
        self.edge_banding.push(band);
    }

    fn remove_edge_banding(&mut self, edge: EdgeId) -> bool {
        let before = self.edge_banding.len();
        self.edge_banding.retain(|existing| existing.edge() != edge);
        self.edge_banding.len() != before
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
        let mut panel = Self::new(wire.id, wire.name, nominal_size, material, thickness, wire.placement)?;
        panel.machining = wire.machining;
        panel.edge_banding = edge_banding;
        panel.corner_features = wire.corner_features;
        panel.edge_bows = wire.edge_bows;
        Ok(panel)
    }
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
/// Coordinates are core-local: `(0, 0)` is the core zero, not the nominal minimum corner, with
/// `+X` along the length and `+Y` along the width. Machining therefore stays on the board when an
/// included edge band moves the core origin. `entry` names the face the cut enters from; a blind
/// cutout removes material from that face up to its depth, while a through cutout removes the full
/// thickness.
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

    /// The cutout outline in core-local coordinates.
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
    /// A rectangle whose minimum corner is at `rect.x`, `rect.y` from the core zero.
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

/// A rectangle in core-local XY coordinates.
///
/// `x` and `y` are the minimum corner relative to the core zero; `width` and `height` are
/// positive. Cutout validation checks that the rectangle stays within the core outline.
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
    /// A nested assembly.
    Assembly(Assembly),
    /// A panel.
    Panel(Panel),
}

impl From<Assembly> for Node {
    fn from(assembly: Assembly) -> Self {
        Self::Assembly(assembly)
    }
}

impl From<Panel> for Node {
    fn from(panel: Panel) -> Self {
        Self::Panel(panel)
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
    /// Contour panels do not support edge banding yet.
    #[error("shaped panels do not support edge banding yet")]
    UnsupportedContourBanding,
    /// Contour panels do not support machining yet.
    #[error("shaped panels do not support machining yet")]
    UnsupportedContourMachining,
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
        let root =
            Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![shelf.into(), drawer.into()]);
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
    fn shaping_rejects_banding_and_machining_until_contour_offsets_land() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = CornerFeature::new(
            RectCorner::BottomLeft.id(),
            CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(20) },
        )
        .expect("corner feature");

        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(20))
                    .expect("banding"),
            )
            .expect("set banding");
        assert!(matches!(
            project.set_corner_feature(panel_id, corner),
            Err(ProjectError::PanelGeometry(PanelGeometryError::UnsupportedContourBanding))
        ));

        let mut shaped = sample_project();
        shaped.set_corner_feature(panel_id, corner).expect("set corner");
        assert!(matches!(
            shaped.add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
                CutoutDepth::Through,
                FaceSide::A
            ),
            Err(ProjectError::ContourMachiningUnsupported { panel }) if panel == panel_id
        ));
        assert!(matches!(
            shaped.set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), sample_material(), true, millimetres(2), millimetres(20))
                    .expect("banding")
            ),
            Err(ProjectError::PanelGeometry(PanelGeometryError::UnsupportedContourBanding))
        ));
    }

    #[test]
    fn shaped_panel_with_machining_is_a_geometry_error() {
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
        assert_eq!(panel.geometry_with_fallbacks(), Err(PanelGeometryError::UnsupportedContourMachining));
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
        let outside = CutoutShape::Rectangle(sample_rect(650, 10, 100, 50));
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
    fn panel_resize_validates_existing_cutouts() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(600, 10, 50, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");

        let smaller = PanelNominalSize::new(millimetres(620), millimetres(500)).expect("valid size");
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
            Err(ProjectError::AssemblyNotFound { id: missing })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn projects_reject_assemblies_below_the_depth_limit() {
        let mut root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let deepest = root.id();
        for index in 0..(Project::MAX_ASSEMBLY_DEPTH - 1) {
            let id = Id::new(u64::from(index) + 2).expect("id");
            root = Assembly::new(id, "Nested", Placement::IDENTITY, vec![root.into()]);
        }
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
        let root = Assembly::new(duplicate, "Root", Placement::IDENTITY, vec![panel.into()]);
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(duplicate)));
    }

    #[test]
    fn projects_reject_excessive_nesting() {
        let mut root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        for index in 0..Project::MAX_ASSEMBLY_DEPTH {
            let id = Id::new(u64::from(index) + 2).expect("id");
            root = Assembly::new(id, "Nested", Placement::IDENTITY, vec![root.into()]);
        }
        assert!(matches!(
            Project::new(root),
            Err(ProjectError::TooDeep { max }) if max == Project::MAX_ASSEMBLY_DEPTH
        ));
    }

    #[test]
    fn projects_reject_unsupported_schema_and_unknown_fields() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");

        let newer = text.replacen("\"schema_version\":2", "\"schema_version\":3", 1);
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
}
