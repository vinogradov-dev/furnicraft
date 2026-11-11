//! The open scenes: their order, the active tab and the policy a close follows.
//!
//! One tab holds one document and nothing else, so activating a tab swaps the whole workspace — the
//! session, the undo history, the selection, the cameras and the renderer target — without reading
//! or writing a file (U45). This module is the policy alone: a list, an active id and what a close
//! means. It carries no window, no GPU and no document, so a test drives every rule with a plain
//! payload, exactly as `view_layout` does with rectangles.
//!
//! The first tab lives in its own field, not in a `Vec`: the workspace never has zero tabs (closing
//! the last one leaves an empty scene), so holding the first tab apart is the type-level proof that
//! [`Tabs::active`] has a document to hand back.
//!
//! Changes when: what a tab holds, the order it is drawn in or the rule a close follows changes —
//! not when a document gains a feature.

/// A stable identity for one open scene.
///
/// An id is handed out once and never reused, so a dialog or a deferred action that names a tab can
/// never land on a document that took the identity of the one it meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TabId(pub(crate) u64);

/// What a close did with the tab.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloseOutcome<T> {
    /// The tab was dropped and a neighbour became active; the shell frees the returned document.
    Closed(T),
    /// The document has unsaved changes: ask first, then call [`Tabs::discard`].
    NeedsConfirmation(TabId),
    /// It was the last tab, so the workspace keeps a tab and the shell swaps in an empty scene
    /// rather than leaving no document at all.
    LastTab(TabId),
    /// The id names no open tab, so there is nothing to close.
    Missing(TabId),
}

/// One open scene: its identity and the document behind it.
#[derive(Debug)]
struct Tab<T> {
    id: TabId,
    document: T,
}

/// The open scenes in the order the tab row draws them.
#[derive(Debug)]
pub(crate) struct Tabs<T> {
    /// The oldest open tab. It exists even when every other tab is closed, which is what makes
    /// [`Tabs::active`] total.
    first: Tab<T>,
    /// Every tab opened after it, in row order.
    rest: Vec<Tab<T>>,
    /// The id of the active tab: `first.id`, or the id of one entry of `rest`.
    active: TabId,
    /// The next identity to hand out. Ids are never reused, so it only grows.
    next: u64,
}

impl<T> Tabs<T> {
    /// Opens the workspace with its first tab, which is also the active one.
    pub(crate) fn new(first: T) -> Self {
        let tab = Tab { id: TabId(0), document: first };
        Self { active: tab.id, first: tab, rest: Vec::new(), next: 1 }
    }

    /// How many scenes are open. Never zero.
    ///
    /// Nothing in the window asks for it: the count of tabs is the length of the row the header
    /// draws, and the close policy is what keeps it above zero. The tests read it to assert exactly
    /// that, so it exists for them.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        1 + self.rest.len()
    }

    /// The identity of the active tab.
    pub(crate) fn active_id(&self) -> TabId {
        self.active
    }

    /// The active document.
    ///
    /// An id that names no tab is a broken invariant, not a user state: the first tab stands in
    /// rather than the workspace showing nothing (AGENTS.md §4.4).
    pub(crate) fn active(&self) -> &T {
        self.rest.iter().find(|tab| tab.id == self.active).map_or(&self.first.document, |tab| &tab.document)
    }

    /// The active document, mutably.
    pub(crate) fn active_mut(&mut self) -> &mut T {
        let active = self.active;
        let index = self.rest.iter().position(|tab| tab.id == active);
        match index.and_then(|index| self.rest.get_mut(index)) {
            Some(tab) => &mut tab.document,
            None => &mut self.first.document,
        }
    }

    /// The document `id` names, if that tab is open.
    pub(crate) fn document(&self, id: TabId) -> Option<&T> {
        if self.first.id == id {
            return Some(&self.first.document);
        }
        self.rest.iter().find(|tab| tab.id == id).map(|tab| &tab.document)
    }

    /// Every open tab in drawing order, with its identity.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (TabId, &T)> {
        std::iter::once((self.first.id, &self.first.document))
            .chain(self.rest.iter().map(|tab| (tab.id, &tab.document)))
    }

    /// Every open tab in drawing order, mutably, so the window can hand a chrome change to all of
    /// them without a second code path per tab.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (TabId, &mut T)> {
        std::iter::once((self.first.id, &mut self.first.document))
            .chain(self.rest.iter_mut().map(|tab| (tab.id, &mut tab.document)))
    }

    /// The id of the first tab whose document `matches`, for re-opening a scene that is already
    /// open: the owner's rule is to activate that tab, never to make a second copy of one file.
    pub(crate) fn find(&self, matches: impl Fn(&T) -> bool) -> Option<TabId> {
        let first = matches(&self.first.document).then_some(self.first.id);
        first.or_else(|| self.rest.iter().find(|tab| matches(&tab.document)).map(|tab| tab.id))
    }

    /// Adds a document at the end of the row and activates it.
    pub(crate) fn add(&mut self, document: T) -> TabId {
        let id = TabId(self.next);
        self.next = self.next.saturating_add(1);
        self.rest.push(Tab { id, document });
        self.active = id;
        id
    }

    /// Makes `id` the active tab. Returns whether it is open.
    pub(crate) fn activate(&mut self, id: TabId) -> bool {
        let open = self.first.id == id || self.rest.iter().any(|tab| tab.id == id);
        if open {
            self.active = id;
        }
        open
    }

    /// Closes `id`, asking first when its document has unsaved changes.
    ///
    /// `unsaved` comes from the document, which is the only owner of the dirty marker; the policy
    /// here decides what a dirty close means, not whether the document is dirty.
    pub(crate) fn close(&mut self, id: TabId, unsaved: bool) -> CloseOutcome<T> {
        match unsaved {
            true => match self.first.id == id || self.rest.iter().any(|tab| tab.id == id) {
                true => CloseOutcome::NeedsConfirmation(id),
                false => CloseOutcome::Missing(id),
            },
            false => self.remove(id),
        }
    }

    /// Closes `id` after the user confirmed discarding its unsaved changes.
    pub(crate) fn discard(&mut self, id: TabId) -> CloseOutcome<T> {
        self.remove(id)
    }

    /// Swaps the document of `id` and returns the one it replaced, for the empty scene the last
    /// close leaves behind.
    pub(crate) fn replace(&mut self, id: TabId, document: T) -> Option<T> {
        if self.first.id == id {
            return Some(std::mem::replace(&mut self.first.document, document));
        }
        self.rest.iter_mut().find(|tab| tab.id == id).map(|tab| std::mem::replace(&mut tab.document, document))
    }

    /// Drops the tab and picks the neighbour that becomes active.
    ///
    /// Closing the active tab activates the one that took its place in the row, or the last tab
    /// when it was the last one; closing an inactive tab leaves the active one alone.
    fn remove(&mut self, id: TabId) -> CloseOutcome<T> {
        if self.first.id == id {
            if self.rest.is_empty() {
                return CloseOutcome::LastTab(id);
            }
            // The next tab in the row becomes the standing first tab, so `active` keeps reading a
            // document however the close was reached.
            let promoted = self.rest.remove(0);
            let dropped = std::mem::replace(&mut self.first, promoted);
            return CloseOutcome::Closed(dropped.document);
        }
        let Some(index) = self.rest.iter().position(|tab| tab.id == id) else {
            return CloseOutcome::Missing(id);
        };
        let tab = self.rest.remove(index);
        if tab.id == self.active {
            let neighbour = index.min(self.rest.len().saturating_sub(1));
            match self.rest.get(neighbour) {
                Some(tab) => self.active = tab.id,
                None => self.active = self.first.id,
            }
        }
        CloseOutcome::Closed(tab.document)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A stand-in for a document: the four things a tab must keep its own copy of — the file it
    /// came from, its undo history and dirty marker, its selection and its camera.
    #[derive(Debug, PartialEq, Eq)]
    struct Scene {
        path: Option<&'static str>,
        undo: Vec<&'static str>,
        dirty: bool,
        selection: Vec<&'static str>,
        camera: &'static str,
    }

    impl Scene {
        fn new(path: Option<&'static str>, camera: &'static str) -> Self {
            Self { path, undo: Vec::new(), dirty: false, selection: Vec::new(), camera }
        }

        /// One document edit: pushes an undo entry and marks the scene dirty.
        fn edit(&mut self, command: &'static str) {
            self.undo.push(command);
            self.dirty = true;
        }

        fn undo(&mut self) {
            self.undo.pop();
            self.dirty = !self.undo.is_empty();
        }

        fn select(&mut self, node: &'static str) {
            self.selection = vec![node];
        }

        fn look_from(&mut self, camera: &'static str) {
            self.camera = camera;
        }
    }

    fn two_scenes() -> Tabs<Scene> {
        let mut tabs = Tabs::new(Scene::new(Some("cabinet.fcs"), "axon"));
        tabs.add(Scene::new(Some("shelf.fcs"), "front"));
        tabs.activate(TabId(0));
        tabs
    }

    #[test]
    fn the_first_tab_is_active_and_a_new_one_takes_over() {
        let mut tabs = Tabs::new(Scene::new(Some("cabinet.fcs"), "axon"));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active_id(), TabId(0), "the first tab starts active");
        let id = tabs.add(Scene::new(Some("shelf.fcs"), "front"));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.active_id(), id, "a freshly opened scene is the one the user is looking at");
        assert_eq!(tabs.active().path, Some("shelf.fcs"));
        assert_eq!(tabs.document(id).map(|scene| scene.path), Some(Some("shelf.fcs")), "an id names its document");
        assert!(tabs.document(TabId(9)).is_none(), "and an id that is not open names nothing");
        assert!(tabs.activate(TabId(0)), "an open tab can be activated again");
        assert_eq!(tabs.active().path, Some("cabinet.fcs"));
        assert!(!tabs.activate(TabId(99)), "an id that is not open activates nothing");
    }

    #[test]
    fn a_tab_keeps_its_own_history_and_dirty_marker() {
        let mut tabs = two_scenes();
        tabs.active_mut().edit("panel.create");
        tabs.active_mut().edit("panel.resize");
        assert_eq!(tabs.len(), 2);

        tabs.activate(TabId(1));
        assert_eq!(tabs.active().undo, Vec::<&str>::new(), "the second scene never saw the first one's edits");
        assert!(!tabs.active().dirty, "and it is not dirty");
        tabs.active_mut().edit("object.extrude.height.set");

        tabs.activate(TabId(0));
        assert_eq!(tabs.active().undo, vec!["panel.create", "panel.resize"], "the first scene kept its undo stack");
        assert!(tabs.active().dirty, "and its dirty marker");
        tabs.active_mut().undo();
        assert_eq!(tabs.active().undo, vec!["panel.create"], "undo acts on the active tab only");

        tabs.activate(TabId(1));
        assert_eq!(tabs.active().undo, vec!["object.extrude.height.set"], "the other tab's history is untouched");
        assert!(tabs.active().dirty);
    }

    #[test]
    fn switching_tabs_restores_the_selection_and_the_camera() {
        let mut tabs = two_scenes();
        tabs.active_mut().select("Carcass");
        tabs.active_mut().look_from("front");

        tabs.activate(TabId(1));
        assert!(tabs.active().selection.is_empty(), "the second scene has its own selection");
        assert_eq!(tabs.active().camera, "front", "and its own camera");
        tabs.active_mut().select("Shelf");
        tabs.active_mut().look_from("top");

        tabs.activate(TabId(0));
        assert_eq!(tabs.active().selection, vec!["Carcass"], "switching back restores the selection");
        assert_eq!(tabs.active().camera, "front", "and the camera it was left in");

        tabs.activate(TabId(1));
        assert_eq!(tabs.active().selection, vec!["Shelf"]);
        assert_eq!(tabs.active().camera, "top");
    }

    #[test]
    fn closing_a_tab_with_unsaved_changes_asks_before_dropping_it() {
        let mut tabs = two_scenes();
        tabs.active_mut().edit("panel.create");
        assert_eq!(tabs.close(TabId(0), true), CloseOutcome::NeedsConfirmation(TabId(0)));
        assert_eq!(tabs.len(), 2, "the question drops nothing");
        assert_eq!(tabs.paths(), [Some("cabinet.fcs"), Some("shelf.fcs")], "and neither scene is touched");
    }

    #[test]
    fn a_confirmed_discard_drops_only_that_tab() {
        let mut tabs = two_scenes();
        tabs.active_mut().edit("panel.create");
        tabs.active_mut().edit("panel.resize");
        let closed = tabs.close(TabId(0), true);
        assert_eq!(closed, CloseOutcome::NeedsConfirmation(TabId(0)));

        let dropped = tabs.discard(TabId(0));
        assert!(matches!(dropped, CloseOutcome::Closed(_)), "a confirmed close drops the tab: {dropped:?}");
        if let CloseOutcome::Closed(scene) = dropped {
            assert_eq!(scene.undo.len(), 2, "the discarded document comes back so the shell can free it");
        }
        assert_eq!(tabs.len(), 1, "only the closed tab is gone");
        assert_eq!(tabs.active().path, Some("shelf.fcs"), "and its neighbour is what remains");
        assert_eq!(tabs.active().undo, Vec::<&str>::new());
    }

    #[test]
    fn closing_the_last_tab_keeps_a_tab_for_the_empty_scene() {
        let mut tabs = Tabs::new(Scene::new(Some("cabinet.fcs"), "axon"));
        tabs.active_mut().edit("panel.create");
        assert_eq!(tabs.close(TabId(0), true), CloseOutcome::NeedsConfirmation(TabId(0)));
        assert_eq!(tabs.discard(TabId(0)), CloseOutcome::LastTab(TabId(0)));
        assert_eq!(tabs.len(), 1, "the workspace never runs out of tabs");

        let replaced = tabs.replace(TabId(0), Scene::new(None, "axon")).expect("the tab is still open");
        assert_eq!(replaced.path, Some("cabinet.fcs"), "the old document is handed back to be freed");
        assert_eq!(tabs.active().path, None, "an emptied workspace is an untitled scene");
        assert!(!tabs.active().dirty);
    }

    #[test]
    fn closing_an_inactive_tab_leaves_the_active_one_alone() {
        let mut tabs = two_scenes();
        tabs.activate(TabId(1));
        assert!(matches!(tabs.close(TabId(0), false), CloseOutcome::Closed(_)));
        assert_eq!(tabs.active_id(), TabId(1), "the tab the user is looking at does not move");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.close(TabId(42), false), CloseOutcome::Missing(TabId(42)), "an unknown id closes nothing");
    }

    #[test]
    fn the_neighbour_becomes_active_when_the_active_tab_closes() {
        let mut tabs = Tabs::new(Scene::new(None, "axon"));
        let second = tabs.add(Scene::new(Some("b.fcs"), "axon"));
        let third = tabs.add(Scene::new(Some("c.fcs"), "axon"));
        assert_eq!(tabs.active_id(), third);
        assert!(matches!(tabs.close(third, false), CloseOutcome::Closed(_)));
        assert_eq!(tabs.active_id(), second, "the row closes up and the neighbour takes over");
    }

    #[test]
    fn a_closed_tab_never_lends_its_identity_to_the_next_one() {
        let mut tabs = Tabs::new(Scene::new(None, "axon"));
        let first_new = tabs.add(Scene::new(Some("b.fcs"), "axon"));
        assert!(matches!(tabs.close(first_new, false), CloseOutcome::Closed(_)));
        let second_new = tabs.add(Scene::new(Some("c.fcs"), "axon"));
        assert_ne!(first_new, second_new, "an id names one document for the life of the session");
    }

    #[test]
    fn a_scene_that_is_already_open_names_its_tab() {
        let tabs = two_scenes();
        assert_eq!(tabs.find(|scene| scene.path == Some("shelf.fcs")), Some(TabId(1)));
        assert_eq!(tabs.find(|scene| scene.path == Some("cabinet.fcs")), Some(TabId(0)));
        assert_eq!(tabs.find(|scene| scene.path == Some("dresser.fcs")), None);
    }

    /// The file names of the open tabs, in row order, for the assertions above.
    impl Tabs<Scene> {
        fn paths(&self) -> Vec<Option<&'static str>> {
            self.iter().map(|(_, scene)| scene.path).collect()
        }
    }
}
