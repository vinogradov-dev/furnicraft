//! The saved document: schema version, revision, id allocation and the container tree.
//!
//! This module is the composition point of the document. It owns [`Project`] and the core
//! operations that every other command builds on; the serialized envelope lives in `wire`, the
//! errors in `error` and the read-only lookups in `query`. Commands are grouped by the value family
//! they mutate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

mod camera_light_cmds;
mod error;
#[cfg(test)]
mod frame_contract;
mod move_query;
mod niche;
mod node_cmds;
mod object_cmds;
mod panel_cmds;
mod product_cmds;
mod property_cmds;
mod query;
mod surface;
mod wire;

pub use error::ProjectError;
pub use move_query::{MoveAxis, MoveLimit, MoveRule};
pub use niche::{Niche, NicheShape, Probe, ProbeHit};
pub use surface::SurfaceKind;
pub(crate) use wire::ProjectWire;

use crate::*;
use panel_cmds::validate_panel_banding;

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
    /// The schema version written by this build, and the only one it reads.
    ///
    /// A file with any other number is refused with one readable sentence, older and newer alike:
    /// before the first release the format has no past, so there is no migration and no alias for a
    /// field an earlier layout used (ADR 0096). Bumping this number is the whole procedure.
    pub const CURRENT_SCHEMA_VERSION: u32 = 22;

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
        root.set_pivot(PivotLocal::ZERO);
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
    let mut stack = vec![(root, 1u32, false)];
    while let Some((container, depth, inside_product)) = stack.pop() {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        let id = container.id();
        if !seen.insert(id.get()) {
            return Err(ProjectError::DuplicateId(id));
        }
        max_id = max_id.max(id.get());
        let is_product = container.is_product();
        if depth == 1 && is_product {
            return Err(ProjectError::RootIsAProduct { id });
        }
        match (is_product, container.envelope()) {
            (true, None) => return Err(ProjectError::ProductClassWithoutEnvelope { node: id }),
            (true, Some(_)) if inside_product => return Err(ProjectError::ProductInsideProduct { product: id }),
            (false, Some(_)) => return Err(ProjectError::EnvelopeWithoutProductClass { node: id }),
            _ => {}
        }
        let inside_product = inside_product || is_product;
        for child in container.children() {
            match child {
                Node::Container(container) => {
                    stack.push((container, depth.saturating_add(1), inside_product));
                }
                Node::Panel(panel) => {
                    if !seen.insert(panel.id().get()) {
                        return Err(ProjectError::DuplicateId(panel.id()));
                    }
                    max_id = max_id.max(panel.id().get());
                    if !panel.pivot_local().is_zero() {
                        return Err(ProjectError::PanelPivotIsDerived { panel: panel.id() });
                    }
                    validate_panel_banding(panel)?;
                    for machining in panel.machining() {
                        if !seen.insert(machining.id().get()) {
                            return Err(ProjectError::DuplicateId(machining.id().as_id()));
                        }
                        max_id = max_id.max(machining.id().get());
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

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

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
        let root = Container::new(duplicate, "Root", Placement::IDENTITY, vec![panel.into()]);
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(duplicate)));
    }

    #[test]
    fn the_old_pivot_field_name_is_an_unknown_field() {
        let project = sample_project();
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        let panel = &mut wire["root"]["children"][0]["panel"];
        let fields = panel.as_object_mut().expect("the panel is an object");
        fields.remove("pivot_local");
        fields.insert("pivot".to_owned(), serde_json::json!([1_000, 0, 0]));

        let error = serde_json::from_value::<Project>(wire).expect_err("the old name is not accepted");
        assert!(error.to_string().contains("unknown field"), "error was {error}");
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
}
