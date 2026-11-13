//! The selection and the set of open containers.
//!
//! Both are view state: `Selection` keeps the multi-selection in the order the user built it and
//! the open set remembers which containers show their children. The rules that turn a viewport hit
//! into a selected node live in `crate::target`; this module only applies them.
//!
//! Refuses nothing: an empty click clears the selection and `exit_level` with no open ancestor
//! does nothing, so a gesture the scene cannot honour leaves the state unchanged.
//!
//! Changes when: a gesture selects, adds to the selection, or opens and closes a container.

use furnicraft_engine::Session;
use furnicraft_model::{Container, Id, Project};

use super::Editor;
use crate::target::{child_containing, resolve_viewport_pick};

impl Editor {
    /// The primary selected node, if any.
    pub fn selected_node(&self) -> Option<Id> {
        self.selection.primary()
    }

    /// Every selected node in selection order, oldest first; the last one is the primary.
    ///
    /// Multi-selection is view state, so a gizmo drag moves all of these nodes as one document
    /// change while the inspector keeps showing the primary.
    pub fn selected_nodes(&self) -> &[Id] {
        self.selection.ids()
    }

    /// Replaces the selection with one node, or clears it for `None`.
    ///
    /// A deliberate selection also counts as the first show, so the tree does not replace it with
    /// the first panel afterwards.
    pub fn select_node(&mut self, node: Option<Id>) {
        self.initialized = true;
        match node {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Selects the result of a viewport click on `hit`.
    ///
    /// The viewport returns the panel under the pointer; a closed container above it is selected
    /// instead, so a click picks the cabinet rather than one of its boards. A click outside the
    /// scene clears the selection.
    pub fn select_viewport_hit(&mut self, project: &Project, hit: Option<Id>) {
        self.initialized = true;
        match hit.and_then(|hit| resolve_viewport_pick(project, &self.open, hit)) {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Acts on a double click in the viewport: opens a product, or enters one level below a group.
    ///
    /// A double click on a product opens it as the session's working product (ADR 0098, ADR 0100)
    /// and arms it for creation; a double click on a group reveals its contents and selects the
    /// child under the pointer. A double click on a panel, a loose object or the root does nothing
    /// beyond the selection the first click already made — in particular it never opens a product
    /// that does not own the hit.
    ///
    /// Returns whether the open product changed, so the shell can rebuild the envelope inks in the
    /// frame the click lands in.
    pub fn enter_viewport_level(&mut self, session: &mut Session, hit: Option<Id>) -> bool {
        let Some(hit) = hit else {
            return false;
        };
        self.initialized = true;
        let project = session.project();
        let Some(selected) = resolve_viewport_pick(project, &self.open, hit) else {
            return false;
        };
        self.selection.replace(selected);
        if !project.is_container(selected) {
            return false;
        }
        if project.container(selected).is_some_and(Container::is_product) {
            return self.open_product(session, selected);
        }
        self.open.insert(selected);
        if let Some(child) = child_containing(project, selected, hit) {
            self.selection.replace(child);
        }
        false
    }

    /// Closes one level and returns the selection to the container that owns it.
    ///
    /// The current node closes when it is an open container; otherwise the nearest open ancestor
    /// closes. With no open ancestor this does nothing.
    pub fn exit_level(&mut self, project: &Project) {
        let Some(selected) = self.selection.primary() else {
            return;
        };
        if project.is_container(selected) && self.open.remove(&selected) {
            if let Some(parent) = project.parent_of(selected)
                && parent != project.root().id()
            {
                self.selection.replace(parent);
            }
            return;
        }
        let mut node = selected;
        while let Some(parent) = project.parent_of(node) {
            if parent == project.root().id() {
                break;
            }
            if self.open.remove(&parent) {
                self.selection.replace(parent);
                return;
            }
            node = parent;
        }
    }

    /// Drops open flags for nodes that no longer exist or no longer own children.
    pub(super) fn retain_open(&mut self, project: &Project) {
        self.open.retain(|id| project.is_container(*id));
    }
}
