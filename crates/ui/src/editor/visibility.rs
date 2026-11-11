//! Visibility and lock: the per-session flags the scene tree draws.
//!
//! The flags themselves are [`crate::ViewFlags`]; this module is the editor's face on them, next to
//! the selection and the open set, and the two questions the rest of the editor asks about a lock:
//! which selected node is locked, and what sentence a refused edit says.
//!
//! Refuses nothing by itself: every method here only records view state. The refusal of a locked
//! gesture belongs to the caller, because it is a guard on a gesture and not a document rule
//! (ADR 0065).
//!
//! Changes when: a flag is added, or a surface needs another question answered about it.

use furnicraft_model::{Id, Project};
use furnicraft_text::{Lang, Message};

use super::Editor;
use crate::view_flags::ViewFlags;

impl Editor {
    /// The session's visibility and lock flags.
    pub fn flags(&self) -> &ViewFlags {
        &self.flags
    }

    /// A counter that moves whenever the visible set changes, so a rebuild can tell.
    pub fn visibility_revision(&self) -> u64 {
        self.flags.visibility_revision()
    }

    /// Whether the node is locked for this session.
    pub fn is_locked(&self, id: Id) -> bool {
        self.flags.is_locked(id)
    }

    /// Hides or shows the node; a document rebuild follows through [`Editor::visibility_revision`].
    pub fn set_hidden(&mut self, id: Id, hidden: bool) {
        self.flags.set_hidden(id, hidden);
    }

    /// Locks or unlocks the node.
    pub fn set_locked(&mut self, id: Id, locked: bool) {
        self.flags.set_locked(id, locked);
    }

    /// Shows every node again.
    pub fn show_all(&mut self) {
        self.flags.show_all();
    }

    /// Hides every branch except the one that holds the node.
    pub fn isolate(&mut self, project: &Project, node: Id) -> bool {
        self.flags.isolate(project, node)
    }

    /// The first selected node that is locked, in selection order.
    ///
    /// The viewport asks this before it updates a transform gizmo, so a locked node never starts a
    /// drag: the guard is on the gesture, before any command is dispatched.
    pub fn locked_selection(&self, project: &Project) -> Option<Id> {
        self.selection.ids().iter().copied().find(|id| self.flags.is_locked(*id) && project.contains_node(*id))
    }

    /// The sentence a refused edit says, when the node is locked.
    pub fn locked_refusal(&self, project: &Project, node: Id, lang: Lang) -> Option<String> {
        if !self.flags.is_locked(node) {
            return None;
        }
        let name = project.node(node).map(furnicraft_model::Node::name).unwrap_or(Message::TheNode.text(lang));
        Some(Message::LockedNode.with(lang, [name]))
    }

    /// Drops flags for nodes that no longer exist, like the open set.
    pub fn retain_flags(&mut self, project: &Project) {
        self.flags.retain_existing(project);
    }
}
