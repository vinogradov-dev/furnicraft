//! Which node a viewport click selects and which container a creation tool lands in.
//!
//! Both are rules over document data: they read the tree, the open set and the armed container and
//! return ids. They never change the document; the editor stores the answer as its selection, or
//! uses it as the `parent` of a creation command.
//!
//! The contract: a pick takes a document id and returns a document id or `None`; a creation target
//! is always a container that is in the tree, the root by default. The open set is view state
//! ([ADR 0029]), so the picking rule takes it as an argument instead of reading it from the project.
//!
//! It refuses nothing: an unknown id has no pick and no child, and a stale arm falls back to the
//! root rather than failing.
//!
//! Changes when: the picking rule or the creation-target rule changes.
//!
//! It does not do: the selection state, the open set or the armed container, which the editor
//! owns, and the conversion of a world placement into the parent frame, which is
//! [`Project::placement_in_parent`].
//!
//! [ADR 0022]: ../docs/decisions/0022-closed-containers-pick-as-one-object.md
//! [ADR 0029]: ../docs/decisions/0029-view-state-belongs-to-the-session.md

use std::collections::HashSet;

use furnicraft_model::{Id, Project};

/// The node a viewport click should select: the outermost closed container above the hit node.
///
/// Walking up stops at the nearest open container, so an open level exposes its children. The root
/// is not a selectable node; a top-level panel or container is selected directly (ADR 0022).
pub(crate) fn resolve_viewport_pick(project: &Project, open: &HashSet<Id>, hit: Id) -> Option<Id> {
    if !project.contains_node(hit) {
        return None;
    }
    if project.is_container(hit) {
        return Some(hit);
    }
    let root = project.root().id();
    let mut node = hit;
    while let Some(parent) = project.parent_of(node) {
        if parent == root || open.contains(&parent) {
            break;
        }
        node = parent;
    }
    Some(node)
}

/// The container every creation tool lands in.
///
/// The scene root is the default parent, whatever the selection is: choosing a node must not
/// silently change where a new path, panel, assembly, primitive, camera or light goes (ADR 0056).
/// A child is created through an explicit entry instead, which arms a container; the arm falls back
/// to the root as soon as that node is gone or cannot own children. Extruding a path is not a
/// creation tool, so it stays a sibling of its path.
pub(crate) fn creation_target(project: &Project, armed: Option<Id>) -> Id {
    let root = project.root().id();
    armed.filter(|id| *id != root && project.is_container(*id)).unwrap_or(root)
}

/// The direct child of `container` whose subtree contains `node`.
pub(crate) fn child_containing(project: &Project, container: Id, node: Id) -> Option<Id> {
    if node == container {
        return None;
    }
    let mut current = node;
    for _ in 0..Project::MAX_ASSEMBLY_DEPTH {
        let parent = project.parent_of(current)?;
        if parent == container {
            return Some(current);
        }
        current = parent;
    }
    None
}
