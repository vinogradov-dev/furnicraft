//! The overlay a previewing operation fills: the geometry the viewport draws instead of the
//! document's, with no revision, no undo entry and nothing to save.
//!
//! An operation that shows its result before it commits — the extrusion pull today, a boolean
//! later — cannot write that result into the document: a drag would push one undo entry per frame
//! and mark the file dirty before the user has accepted anything. So the preview lives beside the
//! document. A [`Preview`] maps a node id to the geometry that node would have, the viewport
//! evaluates that geometry in place of the stored one, and the single command behind the ✓ writes
//! the real thing. Dropping the overlay makes the next frame draw the document again, which is what
//! ✕ and Escape do.
//!
//! The overlay is view-side state that happens to live in the session: it moves no
//! [`furnicraft_model::Project`] revision, leaves the history alone, never reaches a file and is
//! dropped with the session.

use std::collections::BTreeMap;

use furnicraft_model::{Geometry, Id};

/// The node geometry a previewing operation shows in place of the document's.
///
/// Entries are keyed by node id and a later [`Preview::set`] replaces an earlier one, so a drag that
/// runs for many frames holds exactly one entry per previewed node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    entries: BTreeMap<Id, Geometry>,
    version: u64,
}

impl Preview {
    /// An empty overlay: the viewport draws the document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows `geometry` for `node` until it is replaced or the overlay is cleared.
    pub fn set(&mut self, node: Id, geometry: Geometry) {
        self.entries.insert(node, geometry);
        self.version = self.version.wrapping_add(1);
    }

    /// Drops every entry. Clearing an empty overlay moves nothing, so a cancel with nothing to
    /// cancel does not invalidate a frame that is already correct.
    pub fn clear(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.entries.clear();
        self.version = self.version.wrapping_add(1);
    }

    /// The geometry previewed for `node`, or `None` when the document's is drawn.
    pub fn geometry(&self, node: Id) -> Option<&Geometry> {
        self.entries.get(&node)
    }

    /// Whether the overlay previews nothing, so the viewport is the document's.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many nodes the overlay previews.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// A counter that moves whenever the overlay changes.
    ///
    /// The shell compares it with the version its uploaded frame was built from, exactly as it
    /// compares the document revision: a preview is a reason to rebuild the scene and nothing else.
    pub fn version(&self) -> u64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Constructive, Extrude, Feature, Geometry, Path, Placement};
    use furnicraft_testkit::{millimetres, sample_project};

    use super::*;
    use crate::Session;

    /// A rectangle 100 × 50 mm extruded 100 mm at the scene root.
    fn body(project: &mut furnicraft_model::Project, height: i64) -> (Id, Constructive) {
        let root = project.root().id();
        let path = Path::rectangle([millimetres(20), millimetres(10)], [millimetres(120), millimetres(60)])
            .expect("a rectangle");
        let body = Constructive::new(path, vec![Feature::Extrude(Extrude::new(millimetres(height)).expect("height"))])
            .expect("a constructive body");
        let id =
            project.add_object(root, "Body", Placement::IDENTITY, Geometry::Constructive(body.clone())).expect("body");
        (id, body)
    }

    #[test]
    fn a_preview_moves_no_revision_and_leaves_no_history() {
        let mut project = sample_project();
        let (object, mut previewed) = body(&mut project, 100);
        let mut session = Session::new(project);
        let revision = session.project().revision();
        previewed.set_height(millimetres(250)).expect("a preview height");

        session.set_preview_geometry(object, Geometry::Constructive(previewed));

        assert_eq!(session.project().revision(), revision, "a preview is not an edit");
        assert!(!session.can_undo(), "a preview is not an undo entry");
        assert!(!session.is_dirty(), "a preview never marks the file dirty");
        assert_eq!(session.preview().len(), 1);
        let drawn = session.drawn_geometry(object).expect("a geometry");
        assert_eq!(
            drawn.as_constructive().and_then(Constructive::height),
            Some(millimetres(250)),
            "the viewport draws the preview"
        );
        assert_eq!(
            session
                .project()
                .object(object)
                .and_then(|node| node.geometry().as_constructive())
                .and_then(Constructive::height),
            Some(millimetres(100)),
            "the document keeps the height it had"
        );
    }

    #[test]
    fn clearing_a_preview_restores_the_documents_geometry() {
        let mut project = sample_project();
        let (object, mut previewed) = body(&mut project, 100);
        let mut session = Session::new(project);
        previewed.set_height(millimetres(250)).expect("a preview height");
        session.set_preview_geometry(object, Geometry::Constructive(previewed));
        let version = session.preview_version();

        session.clear_preview();

        assert!(session.preview().is_empty());
        assert!(session.preview_version() > version, "the shell must rebuild the frame");
        assert_eq!(
            session.drawn_geometry(object).and_then(Geometry::as_constructive).and_then(Constructive::height),
            Some(millimetres(100)),
            "the pre-command state is back"
        );
    }

    #[test]
    fn a_second_set_replaces_the_first_and_moves_the_version() {
        let mut preview = Preview::new();
        let node = Id::new(7).expect("id");
        let path = Path::rectangle([millimetres(0), millimetres(0)], [millimetres(10), millimetres(10)]).expect("path");
        preview.set(node, Geometry::Path(path.clone()));
        let first = preview.version();
        preview.set(node, Geometry::Path(path));
        assert_eq!(preview.len(), 1, "one node, one entry");
        assert!(preview.version() > first);
    }

    #[test]
    fn clearing_an_empty_overlay_moves_nothing() {
        let mut preview = Preview::new();
        let version = preview.version();
        preview.clear();
        assert_eq!(preview.version(), version, "a cancel with nothing to cancel rebuilds no frame");
    }
}
