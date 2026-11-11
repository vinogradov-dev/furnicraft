//! Visibility and lock, the two flags a scene row carries for this session only.
//!
//! Both are view state keyed by node `Id`: they say what the current user is looking at and what
//! the current user may touch, not what the furniture is, so toggling either costs no document
//! revision, leaves no undo entry and never reaches the scene file (ADR 0065). They are pruned the
//! way the open-container set is, so a node that vanishes takes its flags with it and an undone
//! deletion starts visible and unlocked.
//!
//! The set is *explicit*: hiding a container does not write a flag on each child. The consumers
//! that must agree — the scene builder, picking, the light rig and the camera glyphs — ask
//! [`ViewFlags::hides`], which walks the parent chain, so a child of a hidden container disappears
//! even though only the container is in the set.
//!
//! Changes when: a flag is introduced, or the rule that hides a subtree changes.

use std::collections::HashSet;

use furnicraft_model::{Id, Project};

/// The hidden and locked nodes of one editor session.
#[derive(Clone, Debug, Default)]
pub struct ViewFlags {
    hidden: HashSet<Id>,
    locked: HashSet<Id>,
    /// Counts hidden-set changes; the scene rebuilds when it moves and the document revision did
    /// not, because a hidden subtree changes the frame without touching the document.
    visibility_revision: u64,
}

impl ViewFlags {
    /// The nodes whose subtrees are hidden, as the user set them.
    pub fn hidden(&self) -> &HashSet<Id> {
        &self.hidden
    }

    /// Whether the node itself carries the hidden flag.
    pub fn is_hidden(&self, id: Id) -> bool {
        self.hidden.contains(&id)
    }

    /// Whether the node itself carries the lock flag.
    pub fn is_locked(&self, id: Id) -> bool {
        self.locked.contains(&id)
    }

    /// Whether the node is hidden, by its own flag or by an ancestor's.
    ///
    /// This is the question every consumer that projects the whole tree asks: the frame builder,
    /// picking, the light rig and the camera glyphs. The walk is bounded by the document's nesting
    /// limit, so a malformed tree cannot spin here.
    pub fn hides(&self, project: &Project, id: Id) -> bool {
        let mut current = Some(id);
        for _ in 0..=Project::MAX_ASSEMBLY_DEPTH {
            let Some(node) = current else {
                return false;
            };
            if self.hidden.contains(&node) {
                return true;
            }
            current = project.parent_of(node);
        }
        false
    }

    /// A counter that moves whenever the visible set changes; the scene rebuilds on a change.
    pub fn visibility_revision(&self) -> u64 {
        self.visibility_revision
    }

    /// Hides or shows the node, returning whether the set changed.
    pub fn set_hidden(&mut self, id: Id, hidden: bool) -> bool {
        let changed = if hidden { self.hidden.insert(id) } else { self.hidden.remove(&id) };
        if changed {
            self.bump();
        }
        changed
    }

    /// Locks or unlocks the node, returning whether the set changed.
    ///
    /// Locking changes no frame, so it does not move the visibility revision.
    pub fn set_locked(&mut self, id: Id, locked: bool) -> bool {
        if locked { self.locked.insert(id) } else { self.locked.remove(&id) }
    }

    /// Shows every node.
    pub fn show_all(&mut self) {
        if !self.hidden.is_empty() {
            self.hidden.clear();
            self.bump();
        }
    }

    /// Hides every branch except the one that leads to `node`.
    ///
    /// Each ancestor keeps the child on the path visible and hides its other children, so the whole
    /// point of the isolation is that the target's own subtree stays visible. Isolating the root
    /// shows everything again, because the root has no siblings to hide.
    pub fn isolate(&mut self, project: &Project, node: Id) -> bool {
        if !project.contains_node(node) {
            return false;
        }
        self.hidden.clear();
        let mut current = node;
        for _ in 0..=Project::MAX_ASSEMBLY_DEPTH {
            let Some(parent) = project.parent_of(current) else {
                break;
            };
            if let Some(container) = project.container(parent) {
                for child in container.children() {
                    if child.id() != current {
                        self.hidden.insert(child.id());
                    }
                }
            }
            current = parent;
        }
        self.bump();
        true
    }

    /// Drops the flags of nodes that no longer exist, for example after a delete or an undo.
    pub fn retain_existing(&mut self, project: &Project) {
        let hidden = self.hidden.len();
        self.hidden.retain(|id| project.contains_node(*id));
        self.locked.retain(|id| project.contains_node(*id));
        if self.hidden.len() != hidden {
            self.bump();
        }
    }

    fn bump(&mut self) {
        self.visibility_revision = self.visibility_revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Container, Node, Placement};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn id(value: u64) -> Id {
        Id::new(value).expect("non-zero id")
    }

    #[test]
    fn a_toggle_is_view_state_and_moves_no_document_revision() {
        let project = sample_project();
        let revision = project.revision();
        let mut flags = ViewFlags::default();
        let shelf = id(2);

        assert!(!flags.is_hidden(shelf));
        assert!(flags.set_hidden(shelf, true));
        assert!(flags.is_hidden(shelf));
        assert!(!flags.set_hidden(shelf, true), "a repeated hide changes nothing");
        flags.set_locked(shelf, true);
        assert!(flags.is_locked(shelf));
        assert!(flags.set_hidden(shelf, false));
        assert!(!flags.is_hidden(shelf));
        assert_eq!(project.revision(), revision, "the flags never reach the document (ADR 0065)");
    }

    #[test]
    fn the_visibility_revision_moves_only_when_the_visible_set_changes() {
        let mut flags = ViewFlags::default();
        let start = flags.visibility_revision();
        flags.set_locked(id(2), true);
        assert_eq!(flags.visibility_revision(), start, "a lock changes no frame");
        flags.set_hidden(id(2), true);
        assert_ne!(flags.visibility_revision(), start, "a hidden subtree does");
        let hidden = flags.visibility_revision();
        flags.set_hidden(id(2), true);
        assert_eq!(flags.visibility_revision(), hidden, "a no-op keeps the frame");
    }

    #[test]
    fn a_hidden_container_hides_its_children_through_the_parent_chain() {
        let project = sample_project();
        let mut flags = ViewFlags::default();
        let drawer = id(3);
        let front = id(4);

        assert!(!flags.hides(&project, front));
        flags.set_hidden(drawer, true);
        assert!(flags.hides(&project, drawer));
        assert!(flags.hides(&project, front), "a child of a hidden container is hidden too");
        assert!(!flags.hides(&project, id(2)), "a sibling of the hidden container is untouched");
        assert!(!flags.is_hidden(front), "the child carries no flag of its own");
    }

    #[test]
    fn isolate_hides_every_other_branch_and_show_all_restores_them() {
        let inner = Container::new(
            id(20),
            "Inner",
            Placement::IDENTITY,
            vec![Node::Container(Container::new(id(21), "Target", Placement::IDENTITY, Vec::new()))],
        );
        let root = Container::new(
            id(1),
            "Root",
            Placement::IDENTITY,
            vec![
                Node::Container(inner),
                Node::Container(Container::new(id(30), "Other", Placement::IDENTITY, Vec::new())),
            ],
        );
        let project = Project::new(root).expect("project");
        let mut flags = ViewFlags::default();

        assert!(flags.isolate(&project, id(21)));
        assert!(!flags.hides(&project, id(21)), "the target stays visible");
        assert!(!flags.hides(&project, id(20)), "an ancestor stays on the path");
        assert!(flags.hides(&project, id(30)), "a sibling branch disappears");
        assert!(!flags.isolate(&project, id(999)), "an unknown node isolates nothing");

        flags.show_all();
        assert!(flags.hidden().is_empty());
        assert!(!flags.hides(&project, id(30)));
    }

    #[test]
    fn a_deleted_node_takes_its_flags_with_it() {
        let mut project = sample_project();
        let mut flags = ViewFlags::default();
        let front = id(4);
        flags.set_hidden(front, true);
        flags.set_locked(front, true);

        project.remove_node(front).expect("remove the front");
        flags.retain_existing(&project);
        assert!(!flags.is_hidden(front));
        assert!(!flags.is_locked(front));
    }
}
