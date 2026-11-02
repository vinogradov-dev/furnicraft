//! The serialized envelope of a project and the migrations older schemas need.
//!
//! Loading goes through a wire struct, validates the version and the tree, and only then yields a
//! [`Project`]. Unknown top-level fields are rejected; the reserved property keys are validated on
//! the same pass. A version-2 file stores cutouts in core coordinates and a version-6 file stores
//! assemblies and groups as one container kind; both migrate here, never in the document.

use serde::Deserialize;

use crate::project::Project;
use crate::tree::{default_container_pivot, find_node};
use crate::*;

use super::{next_id_after, scan_tree};

/// Serialized form of [`Project`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectWire {
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
