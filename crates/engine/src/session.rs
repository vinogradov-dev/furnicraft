//! The editing session: document, command dispatch and undo/redo.

use std::sync::Arc;

use furnicraft_model::{BandingSpec, Catalog, EdgeBanding, Geometry, Id, PanelGeometry, Project, ProjectError};
use furnicraft_param::EdgeBandProfile;
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandRegistry};
use crate::diagnostics::{PanelDiagnostic, diagnose_panel};
use crate::preview::Preview;
use crate::resolve;

/// One edit by the user — a drag or a run of keystrokes — as a single undo entry.
///
/// A field commits while the user drags it, so one command runs many times with a new value each
/// frame. Those dispatches belong to one scope, so undo returns the value the field had before the
/// hand touched it instead of the frame before the last one (ADR 0092).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditScope(u64);

/// The edit scope that is open right now, and whether it already owns an undo entry.
#[derive(Clone, Copy, Debug)]
struct OpenEdit {
    scope: EditScope,
    /// Whether the scope's first dispatch has pushed the snapshot taken before the edit.
    snapshot_pushed: bool,
}

/// A document plus its undo/redo history and saved-content marker.
#[derive(Debug)]
pub struct Session {
    project: Project,
    saved_project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    dirty: bool,
    catalog: Arc<Catalog>,
    edit: Option<OpenEdit>,
    next_scope: u64,
    /// The overlay a previewing operation fills; see [`Preview`].
    preview: Preview,
}

impl Session {
    /// Creates a clean session around a document with an empty catalogue.
    pub fn new(project: Project) -> Self {
        Self::with_catalog(project, Catalog::default())
    }

    /// Creates a clean session around a document and its read-only material catalogue.
    pub fn with_catalog(project: Project, catalog: Catalog) -> Self {
        let saved_project = project.clone();
        Self {
            project,
            saved_project,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            catalog: Arc::new(catalog),
            edit: None,
            next_scope: 0,
            preview: Preview::new(),
        }
    }

    /// The current document. Commands are the only way to mutate it.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The overlay a previewing operation fills, drawn instead of the document's geometry.
    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    /// Shows `geometry` for `node` until the overlay is cleared.
    ///
    /// This is the one way a previewing operation reaches the viewport: it writes no document field,
    /// moves no revision and leaves no undo entry, so the single command behind the ✓ is the only
    /// thing the user can undo.
    pub fn set_preview_geometry(&mut self, node: Id, geometry: Geometry) {
        self.preview.set(node, geometry);
    }

    /// Drops the overlay, so the viewport draws the document again.
    pub fn clear_preview(&mut self) {
        self.preview.clear();
    }

    /// The counter the shell compares with the version its uploaded frame was built from.
    pub fn preview_version(&self) -> u64 {
        self.preview.version()
    }

    /// The geometry the viewport draws for `node`: the preview when the overlay holds one, else the
    /// document's own.
    pub fn drawn_geometry(&self, node: Id) -> Option<&Geometry> {
        self.preview.geometry(node).or_else(|| self.project.object(node).map(|object| object.geometry()))
    }

    /// The session-scoped, read-only material catalogue.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Resolves one panel's geometry against the session catalogue.
    ///
    /// Catalogue material dimensions take precedence; the stored fallbacks are used only when a
    /// material is missing. This keeps rendering, diagnostics and the panel card on one source of
    /// truth.
    pub fn panel_geometry(&self, panel_id: Id) -> Result<PanelGeometry, ProjectError> {
        resolve::panel_geometry(&self.project, &self.catalog, panel_id)
    }

    /// Resolves a banding profile against the session catalogue.
    ///
    /// The catalogue supplies both the thickness that widens the core and the visible band width;
    /// stored fallbacks cover a material that is missing from the catalogue.
    pub fn band_profile(&self, band: &EdgeBanding) -> EdgeBandProfile {
        self.band_spec_profile(&band.spec())
    }

    /// Resolves a banding profile for either an edge or a corner owner.
    pub fn band_spec_profile(&self, spec: &BandingSpec) -> EdgeBandProfile {
        EdgeBandProfile::new(resolve::spec_thickness(spec, &self.catalog), resolve::spec_width(spec, &self.catalog))
    }

    /// Resolves the derived issues of one panel against the session catalogue.
    pub fn panel_diagnostics(&self, panel_id: Id) -> Vec<PanelDiagnostic> {
        diagnose_panel(&self.project, &self.catalog, panel_id)
    }

    /// Whether an earlier document state can be restored.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether an undone command can be reapplied.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the document differs from the last saved content.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks the current content as saved.
    pub fn mark_saved(&mut self) {
        self.saved_project = self.project.clone();
        self.dirty = false;
    }

    /// Opens an edit scope: every command dispatched inside it shares one undo entry.
    ///
    /// A new scope replaces the one that was open, so a field that begins an edit never inherits the
    /// history of another. The scope closes on [`Session::end_edit`], on any plain dispatch and on
    /// undo or redo.
    pub fn begin_edit(&mut self) -> EditScope {
        let scope = self.edit_scope();
        self.edit = Some(OpenEdit { scope, snapshot_pushed: false });
        scope
    }

    /// A fresh scope token, for a caller that dispatches later through [`Session::dispatch_in`].
    ///
    /// The token alone groups nothing: the history starts at the first dispatch that carries it, so
    /// a card can mint one before it borrows the document and only use it if the user edits.
    pub fn edit_scope(&mut self) -> EditScope {
        self.next_scope = self.next_scope.wrapping_add(1);
        EditScope(self.next_scope)
    }

    /// Closes the open edit scope, keeping its single undo entry.
    pub fn end_edit(&mut self) {
        self.edit = None;
    }

    /// Drops the open edit scope and restores the document it started from.
    ///
    /// This is what Escape during a drag means: the edit never happened, so it leaves no undo entry
    /// and no redo entry either. Returns whether anything was restored; a scope that dispatched
    /// nothing, or one that is not open, has nothing to undo.
    pub fn cancel_edit(&mut self, scope: EditScope) -> bool {
        let Some(open) = self.edit else {
            return false;
        };
        if open.scope != scope {
            return false;
        }
        self.edit = None;
        if !open.snapshot_pushed {
            return false;
        }
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        if self.project.restore_from(&snapshot).is_err() {
            self.undo.push(snapshot);
            return false;
        }
        self.redo.clear();
        self.dirty = !self.project.same_content(&self.saved_project);
        true
    }

    /// Dispatches a command by id with JSON parameters.
    ///
    /// The command runs on a copy of the document. A successful command becomes one undo entry;
    /// a failed command leaves the current document untouched. A dispatch outside an edit scope
    /// closes the scope, so it is never merged into another field's edit.
    pub fn dispatch(&mut self, id: &str, params: &Value) -> Result<CommandOutcome, CommandError> {
        let (outcome, previous) = self.run(id, params)?;
        self.edit = None;
        self.record(previous);
        Ok(outcome)
    }

    /// Dispatches a command as part of one edit, so the whole edit is one undo entry.
    ///
    /// The first dispatch in `scope` pushes the snapshot taken before it; the next ones replace the
    /// document without touching the history. A scope that is no longer open — because the edit
    /// ended, another field began one, or undo ran — starts a fresh entry instead of merging into
    /// history it does not own.
    pub fn dispatch_in(&mut self, scope: EditScope, id: &str, params: &Value) -> Result<CommandOutcome, CommandError> {
        let (outcome, previous) = self.run(id, params)?;
        match self.edit {
            Some(open) if open.scope == scope && open.snapshot_pushed && !self.undo.is_empty() => {
                // The entry already holds the state before the edit; `previous` is a step inside it.
            }
            _ => {
                self.record(previous);
                self.edit = Some(OpenEdit { scope, snapshot_pushed: true });
            }
        }
        Ok(outcome)
    }

    /// Runs one command on a copy of the document and installs the result.
    ///
    /// Returns the command's outcome and the document as it was before it, which is what the history
    /// keeps; a failed command leaves the current document untouched.
    fn run(&mut self, id: &str, params: &Value) -> Result<(CommandOutcome, Project), CommandError> {
        let spec = CommandRegistry::new().get(id).ok_or_else(|| CommandError::UnknownCommand { id: id.to_owned() })?;
        let mut working = self.project.clone();
        let outcome = spec.run(&mut working, &self.catalog, params)?;
        let previous = std::mem::replace(&mut self.project, working);
        Ok((outcome, previous))
    }

    /// Pushes the state before a command and drops the redo branch it invalidates.
    fn record(&mut self, previous: Project) {
        self.undo.push(previous);
        self.redo.clear();
        self.dirty = !self.project.same_content(&self.saved_project);
    }

    /// Restores the content from before the last command and advances the revision.
    pub fn undo(&mut self) -> Result<(), CommandError> {
        self.edit = None;
        let snapshot = self.undo.pop().ok_or(CommandError::NothingToUndo)?;
        let current = self.project.clone();
        if let Err(error) = self.project.restore_from(&snapshot) {
            self.undo.push(snapshot);
            return Err(CommandError::Model(error));
        }
        self.redo.push(current);
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(())
    }

    /// Reapplies the last undone command and advances the revision.
    pub fn redo(&mut self) -> Result<(), CommandError> {
        self.edit = None;
        let snapshot = self.redo.pop().ok_or(CommandError::NothingToRedo)?;
        let current = self.project.clone();
        if let Err(error) = self.project.restore_from(&snapshot) {
            self.redo.push(snapshot);
            return Err(CommandError::Model(error));
        }
        self.undo.push(current);
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use furnicraft_model::{
        Catalog, ClassCode, Constructive, Contour, ContourSegmentId, CornerFeatureKind, Cutout, CutoutDepth,
        CutoutShape, EdgeBandingMaterial, EdgeId, FaceSide, FrameAnchor, Geometry, Id, MachiningAnchor,
        MachiningParams, PanelContourError, PanelMaterial, PivotLocal, Placement, ProjectError, PropertyValue,
        RectCorner, RectEdge, Revision, SYSTEM_CLASS, SegmentSource, VertexId,
    };
    use furnicraft_testkit::{
        sample_band_thickness, sample_band_width, sample_blind_depth, sample_catalog, sample_cutout_rect,
        sample_edge_cutout_rect, sample_edge_material_id, sample_material_id, sample_narrow_panel_nominal_size,
        sample_notch_cutout_rect, sample_outside_cutout_rect, sample_overlapping_cutout_rect,
        sample_panel_nominal_size, sample_panel_thickness, sample_project, sample_resized_panel_nominal_size,
        sample_thin_panel_thickness, sample_wide_band_thickness,
    };
    use furnicraft_units::Length;

    use crate::class_rules::{ClassRefusal, Dimension};
    use crate::node_cmds::{
        AssemblyCreate, NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup, NodeOriginRebase,
        NodePivotCenter, NodePivotSet, NodePlacement, NodePlacementSet, NodePlacementSetMany, NodePlacementWorldZero,
        NodeProperty, NodePropertyClear, NodePropertySet, NodePropertySetMany, NodeRename, NodeUngroup,
    };
    use crate::panel_cmds::{
        PanelBowClear, PanelBowSet, PanelContourCreate, PanelCornerBandingClear, PanelCornerBandingSet,
        PanelCornerClear, PanelCornerSet, PanelCreate, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMachiningAdd,
        PanelMaterialSet, PanelOutlineSet, PanelOutlineToContour, PanelRename, PanelResize, PanelRingBandingClear,
        PanelRingBandingSet,
    };

    fn sample_session() -> Session {
        Session::with_catalog(sample_project(), sample_catalog())
    }

    fn panel_create(session: &Session, name: &str) -> PanelCreate {
        PanelCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            nominal_size: sample_panel_nominal_size(),
            material: sample_material_id(),
            placement: Placement::IDENTITY,
        }
    }

    fn cutout_request(panel: Id, shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> PanelMachiningAdd {
        PanelMachiningAdd {
            panel,
            anchor: MachiningAnchor::face(entry),
            params: MachiningParams::Cutout(Cutout::new(shape, depth)),
        }
    }

    /// A session whose root holds a constructive object with a 700 x 500 rectangular contour.
    fn session_with_contour_source() -> (Session, Id) {
        let mut project = sample_project();
        let width = Length::from_millimetres(700).expect("width");
        let height = Length::from_millimetres(500).expect("height");
        let contour = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [width, Length::ZERO],
            [width, height],
            [Length::ZERO, height],
        ])
        .expect("ring");
        let constructive =
            Constructive::extrude(contour.into(), sample_panel_thickness()).expect("constructive object");
        let source = project
            .add_object(project.root().id(), "Counter", Placement::IDENTITY, Geometry::Constructive(constructive))
            .expect("contour source");
        (Session::with_catalog(project, sample_catalog()), source)
    }

    fn contour_panel_create(session: &Session, source: Id, name: &str) -> PanelContourCreate {
        PanelContourCreate {
            parent: session.project().root().id(),
            name: name.to_owned(),
            source,
            material: sample_material_id(),
            placement: Placement::IDENTITY,
        }
    }

    /// A session whose root holds a 700 x 500 contour panel built from a constructive contour.
    fn session_with_contour_panel() -> (Session, Id) {
        let (mut session, source) = session_with_contour_source();
        let create = json_params(&contour_panel_create(&session, source, "Top"));
        let panel = session
            .dispatch("panel.contour.create", &create)
            .expect("create contour panel")
            .created_id()
            .expect("the command creates a panel");
        (session, panel)
    }

    fn json_params<T: serde::Serialize>(value: &T) -> Value {
        serde_json::to_value(value).expect("params serialize")
    }

    fn placement(x: f64, y: f64, z: f64) -> Placement {
        serde_json::from_value(serde_json::json!({"translation": [x, y, z], "rotation": [0.0, 0.0, 0.0, 1.0]}))
            .expect("placement")
    }

    #[test]
    fn dispatch_panel_create_changes_the_document_once() {
        let mut session = sample_session();
        let params = json_params(&panel_create(&session, "Back"));

        let outcome = session.dispatch("panel.create", &params).expect("create panel");

        assert_eq!(outcome.created_id(), Some(Id::new(5).expect("id")));
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().root().children().len(), 3);
        assert!(session.can_undo());
        assert!(!session.can_redo());
        assert!(session.is_dirty());
    }

    #[test]
    fn contour_panel_create_uses_a_scene_contour_and_leaves_rect_panels_alone() {
        let (mut session, source) = session_with_contour_source();
        let before = session.project().revision().get();
        let request = contour_panel_create(&session, source, "Top");
        let params = json_params(&request);
        let registry = CommandRegistry::new();
        assert!(registry.get("panel.contour.create").expect("registered").enabled(&session, &params));

        let outcome = session.dispatch("panel.contour.create", &params).expect("create contour panel");
        let created = outcome.created_id().expect("the command creates a panel");
        let panel = session.project().panel(created).expect("created panel");
        assert!(panel.is_contour());
        assert_eq!(panel.nominal_size(), sample_panel_nominal_size());
        assert!(!session.project().panel(Id::new(2).expect("id")).expect("shelf").is_contour());
        assert_eq!(session.project().revision().get(), before + 1);

        session.undo().expect("undo");
        assert!(session.project().panel(created).is_none(), "one undo removes the contour panel");
        assert_eq!(session.project().revision().get(), before + 2);
    }

    #[test]
    fn contour_panel_create_rejects_a_source_that_is_not_a_contour() {
        let (mut session, source) = session_with_contour_source();
        let before = session.project().revision();
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.contour.create").expect("registered");

        let panel_source = Id::new(2).expect("id");
        let request = contour_panel_create(&session, panel_source, "Top");
        assert!(!spec.enabled(&session, &json_params(&request)));
        let error =
            session.dispatch("panel.contour.create", &json_params(&request)).expect_err("a panel is not a contour");
        assert!(matches!(error, CommandError::Model(ProjectError::NodeNotFound { id }) if id == panel_source));
        assert_eq!(session.project().revision(), before);

        let blank = contour_panel_create(&session, source, "   ");
        assert!(!spec.enabled(&session, &json_params(&blank)));
        assert!(matches!(
            session.dispatch("panel.contour.create", &json_params(&blank)),
            Err(CommandError::InvalidParams { command: "panel.contour.create", .. })
        ));

        let mut unknown_material = contour_panel_create(&session, source, "Top");
        unknown_material.material = furnicraft_model::MaterialId::new(9_999).expect("id");
        assert!(!spec.enabled(&session, &json_params(&unknown_material)));
        assert_eq!(session.project().revision(), before);
    }

    #[test]
    fn contour_panel_create_lands_exactly_over_its_source_in_a_turned_container() {
        use furnicraft_model::{EulerAngles, Placement};
        use furnicraft_units::Degrees;

        let mm = |value: i64| Length::from_millimetres(value).expect("millimetres");
        let mut project = sample_project();
        let root = project.root().id();
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [mm(700), Length::ZERO],
            [mm(700), mm(500)],
            [Length::ZERO, mm(500)],
        ])
        .expect("ring");
        let body = Constructive::extrude(ring.into(), sample_panel_thickness()).expect("body");
        let source_world = Placement::IDENTITY
            .with_translation(mm(50), mm(20), Length::ZERO)
            .expect("translation")
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(90_000), ..EulerAngles::default() })
            .expect("rotation");
        let source = project.add_object(root, "Counter", source_world, Geometry::Constructive(body)).expect("source");
        let parent_world = Placement::IDENTITY
            .with_translation(mm(-320), mm(150), mm(700))
            .expect("translation")
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(40_000), ..EulerAngles::default() })
            .expect("rotation");
        let parent = project.add_component(root, "Frame", parent_world).expect("container");
        let mut session = Session::with_catalog(project, sample_catalog());

        let create = PanelContourCreate {
            parent,
            name: "Top".to_owned(),
            source,
            material: sample_material_id(),
            placement: Placement::IDENTITY,
        };
        let params = json_params(&create);
        let registry = CommandRegistry::new();
        assert!(registry.get("panel.contour.create").expect("registered").enabled(&session, &params));

        let before = session.project().revision();
        let panel = session
            .dispatch("panel.contour.create", &params)
            .expect("create contour panel")
            .created_id()
            .expect("a panel");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        assert_eq!(session.project().parent_of(panel), Some(parent));
        assert_eq!(session.project().pivot_local(panel), Some(PivotLocal::ZERO), "the base corner is the pivot");

        let source_world = session.project().world_placement(source).expect("source world");
        let world = session.project().world_placement(panel).expect("panel world");
        assert!(
            (world.translation() - source_world.translation()).length() < 1e-9,
            "the panel's base corner is the source's: {:?} vs {:?}",
            world.translation(),
            source_world.translation()
        );
        assert!(world.rotation().dot(source_world.rotation()).abs() >= 1.0 - 1e-12, "the source's turn is kept");
        assert!(
            !world.same_transform(session.project().panel(panel).expect("panel").placement()),
            "the stored placement is relative to the turned container"
        );

        session.undo().expect("one undo removes the panel");
        assert!(session.project().panel(panel).is_none());
    }

    #[test]
    fn panel_outline_set_edits_a_contour_panel_in_one_undo_step() {
        let (mut session, source) = session_with_contour_source();
        let create = json_params(&contour_panel_create(&session, source, "Top"));
        let created = session
            .dispatch("panel.contour.create", &create)
            .expect("create contour panel")
            .created_id()
            .expect("the command creates a panel");
        let original = session.project().panel(created).expect("panel").outline_contour().expect("ring").clone();
        let segment = original.id_at(1).expect("the right edge");
        let moved = original
            .move_corner(segment, [Length::from_millimetres(800).expect("x"), Length::ZERO])
            .expect("move the top of the right edge");
        let request = PanelOutlineSet { panel: created, contour: moved.clone() };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.outline.set").expect("registered");
        assert!(spec.enabled(&session, &json_params(&request)));

        let before = session.project().revision();
        session.dispatch("panel.outline.set", &json_params(&request)).expect("set the outline");
        let panel = session.project().panel(created).expect("panel");
        assert_eq!(panel.outline_contour(), Some(&moved));
        assert_eq!(panel.nominal_size().length(), Length::from_millimetres(800).expect("length"));
        assert_eq!(
            session.project().pivot_local(created),
            Some(PivotLocal::ZERO),
            "the outline edit keeps the pivot derived"
        );
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));

        // The re-anchored panel is a valid document: a save and load keeps it as it is.
        let text = serde_json::to_string(session.project()).expect("serialize the edited project");
        let decoded: furnicraft_model::Project = serde_json::from_str(&text).expect("reload the edited project");
        assert_eq!(decoded.panel(created).expect("reloaded panel").outline_contour(), Some(&moved));
        assert_eq!(decoded.pivot_local(created), Some(PivotLocal::ZERO));

        session.undo().expect("one undo restores the whole edit");
        assert_eq!(session.project().panel(created).expect("panel").outline_contour(), Some(&original));

        let rectangle = Id::new(2).expect("id");
        let refused = PanelOutlineSet { panel: rectangle, contour: moved };
        let stored = session.project().revision();
        assert!(!spec.enabled(&session, &json_params(&refused)));
        assert!(matches!(
            session.dispatch("panel.outline.set", &json_params(&refused)),
            Err(CommandError::Model(ProjectError::PanelNotContour { id })) if id == rectangle
        ));
        assert_eq!(session.project().revision(), stored, "a refused outline changes nothing");
        session.undo().expect("the refused command left no undo entry");
        assert!(session.project().panel(created).is_none(), "undo removes the creation, nothing was recorded between");
        assert!(!session.can_undo());
    }

    #[test]
    fn panel_outline_to_contour_bakes_the_ring_and_one_undo_restores_the_rectangle() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        session
            .dispatch(
                "panel.corner.set",
                &json_params(&PanelCornerSet {
                    panel: panel_id,
                    corner,
                    feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(40).expect("radius") },
                }),
            )
            .expect("set corner");
        session
            .dispatch(
                "panel.edge_banding.set",
                &json_params(&PanelEdgeBandingSet {
                    panel: panel_id,
                    edge: RectEdge::Right.id(),
                    material: sample_edge_material_id(),
                    included_in_size: false,
                }),
            )
            .expect("edge band");
        session
            .dispatch(
                "panel.corner_banding.set",
                &json_params(&PanelCornerBandingSet {
                    panel: panel_id,
                    corner,
                    material: sample_edge_material_id(),
                    included_in_size: false,
                }),
            )
            .expect("corner band");

        let request = PanelOutlineToContour { panel: panel_id };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.outline.to_contour").expect("registered");
        assert!(spec.enabled(&session, &json_params(&request)));

        let before = session.project().revision();
        let outcome = session.dispatch("panel.outline.to_contour", &json_params(&request)).expect("bake the outline");
        assert_eq!(outcome.created_id(), None);
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));

        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.is_contour());
        assert!(panel.edge_banding().is_empty());
        assert!(panel.corner_banding().is_empty());
        assert!(panel.corner_feature(corner).is_none());
        assert_eq!(panel.ring_banding().len(), 2);
        let ring = panel.outline_contour().expect("ring");
        assert!(panel.ring_banding().iter().all(|band| ring.segment(band.segment()).is_some()));
        assert_eq!(
            panel
                .band_order()
                .iter()
                .filter_map(|source| match source {
                    SegmentSource::Ring(segment) => Some(*segment),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            panel.ring_banding().iter().map(|band| band.segment()).collect::<Vec<_>>(),
            "the baked bands keep their application order"
        );

        // A ring has no rectangle left to bake.
        let stored = session.project().revision();
        assert!(!spec.enabled(&session, &json_params(&request)));
        assert!(matches!(
            session.dispatch("panel.outline.to_contour", &json_params(&request)),
            Err(CommandError::Model(ProjectError::PanelContour(PanelContourError::AlreadyContour)))
        ));
        assert_eq!(session.project().revision(), stored, "a refused bake changes nothing");

        session.undo().expect("undo");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(!panel.is_contour());
        assert!(panel.corner_feature(corner).is_some());
        assert_eq!(panel.edge_banding().len(), 1);
        assert_eq!(panel.corner_banding().len(), 1);
        assert!(panel.ring_banding().is_empty());
    }

    #[test]
    fn dispatch_panel_rename_changes_the_name_and_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").name().to_owned();
        let params = json_params(&PanelRename { panel: panel_id, name: "Left side".to_owned() });

        let outcome = session.dispatch("panel.rename", &params).expect("rename panel");
        assert_eq!(outcome.created_id(), None);
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().panel(panel_id).expect("panel").name(), "Left side");
        assert!(session.is_dirty());

        session.undo().expect("undo");
        assert_eq!(session.project().panel(panel_id).expect("panel").name(), before);

        session.redo().expect("redo");
        assert_eq!(session.project().panel(panel_id).expect("panel").name(), "Left side");
    }

    #[test]
    fn rename_rejects_blank_names_and_missing_panels_without_mutation() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").name().to_owned();
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.rename").expect("registered");

        let blank = PanelRename { panel: panel_id, name: "   ".to_owned() };
        assert!(!spec.enabled(&session, &json_params(&blank)));
        let error = session.dispatch("panel.rename", &json_params(&blank)).expect_err("blank name");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.rename", .. }));
        assert_eq!(session.project().panel(panel_id).expect("panel").name(), before);

        let missing = PanelRename { panel: Id::new(999).expect("id"), name: "Missing".to_owned() };
        assert!(!spec.enabled(&session, &json_params(&missing)));
        let error = session.dispatch("panel.rename", &json_params(&missing)).expect_err("missing panel");
        assert!(matches!(error, CommandError::Model(ProjectError::PanelNotFound { .. })));
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn panel_rename_params_round_trip_and_reject_unknown_fields() {
        let params = PanelRename { panel: Id::new(2).expect("id"), name: "Top".to_owned() };
        let value = json_params(&params);
        let decoded: PanelRename = serde_json::from_value(value.clone()).expect("round trip");
        assert_eq!(decoded, params);

        let mut object = value.as_object().cloned().expect("object");
        object.insert("extra".to_owned(), Value::Bool(true));
        assert!(serde_json::from_value::<PanelRename>(Value::Object(object)).is_err());
    }

    #[test]
    fn undo_and_redo_restore_the_document_and_ids() {
        let mut session = sample_session();
        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2);
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().next_id().get(), 6);
        assert!(!session.can_undo());
        assert!(session.can_redo());
        assert!(!session.is_dirty());

        session.redo().expect("redo");
        assert_eq!(session.project().revision().get(), 3);
        assert_eq!(session.project().root().children().len(), 3);
        assert_eq!(session.project().next_id().get(), 6);
        assert!(session.can_undo());
        assert!(!session.can_redo());
        assert!(session.is_dirty());
    }

    #[test]
    fn an_edit_scope_makes_a_drag_one_undo_entry() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").nominal_size();

        let scope = session.begin_edit();
        for width in [720, 740, 760] {
            let size = furnicraft_model::PanelNominalSize::new(
                Length::from_millimetres(700).expect("length"),
                Length::from_millimetres(width).expect("width"),
            )
            .expect("a size");
            let params = json_params(&PanelResize { panel: panel_id, nominal_size: size });
            session.dispatch_in(scope, "panel.resize", &params).expect("resize inside the edit");
        }
        session.end_edit();

        let after = Length::from_millimetres(760).expect("width");
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size().width(), after);
        session.undo().expect("undo the whole edit");
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), before);
        assert!(!session.can_undo(), "a drag is one undo entry, not one per frame");
    }

    #[test]
    fn a_minted_scope_groups_the_dispatches_that_carry_it() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").nominal_size();

        // A card mints a token before it borrows the document; the history starts at the first
        // dispatch that carries it, so no `begin_edit` is needed.
        let scope = session.edit_scope();
        for width in [720, 740] {
            let size = furnicraft_model::PanelNominalSize::new(
                Length::from_millimetres(700).expect("length"),
                Length::from_millimetres(width).expect("width"),
            )
            .expect("a size");
            let params = json_params(&PanelResize { panel: panel_id, nominal_size: size });
            session.dispatch_in(scope, "panel.resize", &params).expect("resize inside the edit");
        }
        session.undo().expect("undo the edit");
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), before);
        assert!(!session.can_undo());
    }

    #[test]
    fn two_edit_scopes_are_two_undo_entries() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");

        let first = session.begin_edit();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        session.dispatch_in(first, "panel.resize", &params).expect("first edit");
        session.end_edit();

        let second = session.begin_edit();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_panel_nominal_size() });
        session.dispatch_in(second, "panel.resize", &params).expect("second edit");
        session.end_edit();

        session.undo().expect("undo the second edit");
        assert_eq!(
            session.project().panel(panel_id).expect("panel").nominal_size(),
            sample_resized_panel_nominal_size()
        );
        session.undo().expect("undo the first edit");
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), sample_panel_nominal_size());
        assert!(!session.can_undo());
    }

    #[test]
    fn cancelling_an_edit_restores_the_document_and_leaves_no_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").nominal_size();

        let scope = session.begin_edit();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        session.dispatch_in(scope, "panel.resize", &params).expect("resize inside the edit");
        assert!(session.cancel_edit(scope), "the edit had something to restore");

        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), before);
        assert!(!session.can_undo(), "a cancelled edit is not undoable");
        assert!(!session.can_redo(), "and it is not redoable either");
        assert!(!session.is_dirty());
        assert!(!session.cancel_edit(scope), "a second cancel has nothing to restore");
    }

    #[test]
    fn an_edit_that_dispatched_nothing_has_nothing_to_cancel() {
        let mut session = sample_session();
        let scope = session.begin_edit();
        assert!(!session.cancel_edit(scope));
        assert!(!session.can_undo());
        let other = session.begin_edit();
        assert!(!session.cancel_edit(scope), "another scope does not own that edit");
        assert!(!session.cancel_edit(other), "an edit that dispatched nothing has nothing to restore");
    }

    #[test]
    fn a_plain_dispatch_closes_the_open_edit() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let scope = session.begin_edit();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        session.dispatch_in(scope, "panel.resize", &params).expect("inside the edit");

        let params = json_params(&Box::new(panel_create(&session, "Other")));
        session.dispatch("panel.create", &params).expect("a command outside the edit");

        // The scope no longer owns history, so its next dispatch opens a new entry instead of
        // folding into the one it never pushed.
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_panel_nominal_size() });
        session.dispatch_in(scope, "panel.resize", &params).expect("after a plain command");
        session.undo().expect("undo the last edit");
        assert_eq!(
            session.project().panel(panel_id).expect("panel").nominal_size(),
            sample_resized_panel_nominal_size()
        );
        session.undo().expect("undo the plain command");
        assert_eq!(session.project().root().children().len(), 2);
    }

    #[test]
    fn undo_closes_an_open_edit() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let scope = session.begin_edit();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        session.dispatch_in(scope, "panel.resize", &params).expect("resize inside the edit");
        session.undo().expect("undo during the edit");

        let params = json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        session.dispatch_in(scope, "panel.resize", &params).expect("a new edit with the old scope");
        session.undo().expect("undo it");
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), sample_panel_nominal_size());
    }

    #[test]
    fn dispatch_panel_resize_updates_size_and_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let before = session.project().panel(panel_id).expect("panel").nominal_size();
        let after = sample_resized_panel_nominal_size();
        let params = json_params(&PanelResize { panel: panel_id, nominal_size: after });

        let outcome = session.dispatch("panel.resize", &params).expect("resize panel");
        assert_eq!(outcome.created_id(), None);
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), after);
        assert!(session.is_dirty());

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2);
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), before);
        assert!(!session.is_dirty());

        session.redo().expect("redo");
        assert_eq!(session.project().revision().get(), 3);
        assert_eq!(session.project().panel(panel_id).expect("panel").nominal_size(), after);
        assert!(session.is_dirty());
    }

    #[test]
    fn resize_missing_panel_is_reported_without_mutation() {
        let mut session = sample_session();
        let missing = Id::new(999).expect("id");
        let params = json_params(&PanelResize { panel: missing, nominal_size: sample_resized_panel_nominal_size() });

        let error = session.dispatch("panel.resize", &params).expect_err("missing panel");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::PanelNotFound { id }) if id == missing
        ));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn resize_invalid_size_is_rejected_before_mutation() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let mut params =
            json_params(&PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() });
        let size = params
            .as_object_mut()
            .and_then(|object| object.get_mut("nominal_size"))
            .and_then(Value::as_object_mut)
            .expect("nominal_size is an object");
        size.insert("length".to_owned(), Value::from(0));

        let error = session.dispatch("panel.resize", &params).expect_err("invalid size");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.resize", .. }));
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn dispatch_cutout_create_changes_the_document_once() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_cutout_rect());
        let params = json_params(&cutout_request(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A));

        let outcome = session.dispatch("panel.machining.add", &params).expect("create cutout");
        let cutout_id = outcome.created_machining_id().expect("cutout id");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().next_id().get(), 6);
        let machining = session.project().panel(panel_id).expect("panel").machining();
        assert_eq!(machining.len(), 1);
        let record = &machining[0];
        assert_eq!(record.id(), cutout_id);
        assert_eq!(record.anchor().as_face(), Some(FaceSide::A));
        let cutout = record.cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);
        assert!(session.is_dirty());

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2);
        assert!(session.project().panel(panel_id).expect("panel").machining().is_empty());
        assert_eq!(session.project().next_id().get(), 6);
        assert!(!session.is_dirty());

        session.redo().expect("redo");
        assert_eq!(session.project().revision().get(), 3);
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 1);
        assert!(session.is_dirty());
    }

    fn cutout_ring(x: i64, y: i64, width: i64, height: i64) -> Contour {
        let point =
            |x: i64, y: i64| [Length::from_millimetres(x).expect("mm"), Length::from_millimetres(y).expect("mm")];
        Contour::from_polygon(vec![
            point(x, y),
            point(x + width, y),
            point(x + width, y + height),
            point(x, y + height),
        ])
        .expect("cutout ring")
    }

    #[test]
    fn dispatch_cutout_create_accepts_a_contour_ring() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Contour(cutout_ring(100, 100, 100, 50));
        let request = cutout_request(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A);

        let outcome = session.dispatch("panel.machining.add", &json_params(&request)).expect("create contour cutout");
        let cutout_id = outcome.created_machining_id().expect("cutout id");
        let machining = session.project().panel(panel_id).expect("panel").machining();
        let record = &machining[0];
        assert_eq!(record.id(), cutout_id);
        assert_eq!(record.cutout().expect("a cutout record").shape(), &shape);

        // A through ring that crosses the core edge opens a notch and is accepted; a ring that
        // misses the core entirely is refused before any id is consumed.
        let crossing = cutout_request(
            panel_id,
            CutoutShape::Contour(cutout_ring(-10, 200, 100, 50)),
            CutoutDepth::Through,
            FaceSide::A,
        );
        session.dispatch("panel.machining.add", &json_params(&crossing)).expect("notch cutout");
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 2);

        let next_id = session.project().next_id();
        let outside = cutout_request(
            panel_id,
            CutoutShape::Contour(cutout_ring(-200, 200, 100, 50)),
            CutoutDepth::Through,
            FaceSide::A,
        );
        let error = session.dispatch("panel.machining.add", &json_params(&outside)).expect_err("ring outside the core");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::CutoutOutsidePanel { panel }) if panel == panel_id
        ));
        assert_eq!(session.project().next_id(), next_id);
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 2);
    }

    #[test]
    fn dispatch_blind_cutout_records_entry_and_depth() {
        let mut session = sample_session();
        let panel_id = Id::new(4).expect("id");
        let shape = CutoutShape::Rectangle(sample_cutout_rect());
        let depth = CutoutDepth::Blind { depth: sample_blind_depth() };
        let params = json_params(&cutout_request(panel_id, shape, depth, FaceSide::B));

        session.dispatch("panel.machining.add", &params).expect("create blind cutout");
        let machining = session.project().panel(panel_id).expect("panel").machining();
        let record = &machining[0];
        assert_eq!(record.anchor().as_face(), Some(FaceSide::B));
        assert_eq!(record.cutout().expect("a cutout record").depth(), depth);
    }

    #[test]
    fn cutout_outside_or_invalid_depth_is_reported_without_mutation() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");

        let outside = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_outside_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        let error = session.dispatch("panel.machining.add", &json_params(&outside)).expect_err("outside cutout");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::CutoutOutsidePanel { panel }) if panel == panel_id
        ));
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert_eq!(session.project().next_id().get(), 5);

        let thickness = sample_panel_thickness();
        let invalid_depth = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutDepth::Blind { depth: thickness },
            FaceSide::A,
        );
        let error = session.dispatch("panel.machining.add", &json_params(&invalid_depth)).expect_err("invalid depth");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::CutoutDepthInvalid { panel, depth, thickness: rejected })
                if panel == panel_id && depth == thickness && rejected == thickness
        ));
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert_eq!(session.project().next_id().get(), 5);
    }

    #[test]
    fn blind_overlap_is_reported_without_consuming_an_id() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let first = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutDepth::Blind { depth: sample_blind_depth() },
            FaceSide::A,
        );
        let first_id = session
            .dispatch("panel.machining.add", &json_params(&first))
            .expect("first cutout")
            .created_machining_id()
            .expect("cutout id");
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().next_id().get(), 6);

        let overlapping = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_overlapping_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        let error =
            session.dispatch("panel.machining.add", &json_params(&overlapping)).expect_err("overlapping cutout");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::CutoutOverlap { panel, other })
                if panel == panel_id && other == first_id
        ));
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().next_id().get(), 6);
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 1);
    }

    #[test]
    fn overlapping_through_cutouts_and_a_notch_are_accepted() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let requests = [
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutShape::Rectangle(sample_overlapping_cutout_rect()),
            CutoutShape::Rectangle(sample_notch_cutout_rect()),
        ];
        for shape in requests {
            let request = cutout_request(panel_id, shape, CutoutDepth::Through, FaceSide::A);
            session.dispatch("panel.machining.add", &json_params(&request)).expect("through cutout");
        }
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 3);
    }

    #[test]
    fn cutout_enabled_checks_panel_and_geometry() {
        let mut session = sample_session();
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.machining.add").expect("panel.machining.add is registered");
        let panel_id = Id::new(2).expect("id");

        let valid =
            cutout_request(panel_id, CutoutShape::Rectangle(sample_cutout_rect()), CutoutDepth::Through, FaceSide::A);
        assert!(spec.enabled(&session, &json_params(&valid)));

        let outside = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_outside_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        assert!(!spec.enabled(&session, &json_params(&outside)));

        let missing = cutout_request(
            Id::new(999).expect("id"),
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        assert!(!spec.enabled(&session, &json_params(&missing)));
        assert!(!spec.enabled(&session, &Value::Null));

        session.dispatch("panel.machining.add", &json_params(&valid)).expect("first cutout");
        let overlapping = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_overlapping_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        assert!(spec.enabled(&session, &json_params(&overlapping)));
    }

    #[test]
    fn dispatch_material_set_changes_the_panel_and_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let params = json_params(&PanelMaterialSet { panel: panel_id, material: sample_material_id() });

        let outcome = session.dispatch("panel.material.set", &params).expect("set material");
        assert_eq!(outcome.created_id(), None);
        assert_eq!(session.project().revision().get(), 1);
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(panel.material(), Some(sample_material_id()));
        assert_eq!(panel.last_known_thickness(), sample_panel_thickness());
        assert!(session.is_dirty());

        session.undo().expect("undo");
        assert_eq!(session.project().revision().get(), 2);
        assert_eq!(session.project().panel(panel_id).expect("panel").material(), None);
        assert!(!session.is_dirty());

        session.redo().expect("redo");
        assert_eq!(session.project().panel(panel_id).expect("panel").material(), Some(sample_material_id()));
        assert!(session.is_dirty());
    }

    #[test]
    fn material_set_accepts_machining_that_no_longer_fits() {
        let panel_material =
            PanelMaterial::new(sample_material_id(), "Thin board", "mdf", sample_thin_panel_thickness(), "white")
                .expect("thin material");
        let catalog = Catalog::new(vec![panel_material], Vec::new()).expect("catalog");
        let mut session = Session::with_catalog(sample_project(), catalog);
        let panel_id = Id::new(4).expect("id");
        let blind = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutDepth::Blind { depth: sample_blind_depth() },
            FaceSide::A,
        );
        session.dispatch("panel.machining.add", &json_params(&blind)).expect("blind cutout");

        let params = json_params(&PanelMaterialSet { panel: panel_id, material: sample_material_id() });
        session.dispatch("panel.material.set", &params).expect("thinner material is accepted");
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(panel.last_known_thickness(), sample_thin_panel_thickness());
        assert_eq!(panel.machining().len(), 1, "machining data is preserved");
    }

    #[test]
    fn unknown_materials_are_rejected_without_mutation() {
        let mut session = sample_session();
        let missing = furnicraft_model::MaterialId::new(999).expect("id");
        let params = json_params(&PanelMaterialSet { panel: Id::new(2).expect("id"), material: missing });
        let error = session.dispatch("panel.material.set", &params).expect_err("unknown material");
        assert!(matches!(error, CommandError::UnknownMaterial { material } if material == missing));
        assert_eq!(session.project().revision(), Revision::ZERO);

        let registry = CommandRegistry::new();
        let spec = registry.get("panel.material.set").expect("registered");
        assert!(!spec.enabled(&session, &params));

        let mut create = panel_create(&session, "Back");
        create.material = missing;
        assert!(!registry.get("panel.create").expect("registered").enabled(&session, &json_params(&create)));
    }

    #[test]
    fn edge_banding_set_orders_replaces_and_clears() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let bottom = EdgeId::new(1).expect("edge id");
        let right = EdgeId::new(2).expect("edge id");

        let first = PanelEdgeBandingSet {
            panel: panel_id,
            edge: bottom,
            material: sample_edge_material_id(),
            included_in_size: true,
        };
        session.dispatch("panel.edge_banding.set", &json_params(&first)).expect("first banding");
        assert_eq!(session.project().revision().get(), 1);
        let geometry = session.project().panel(panel_id).expect("panel").geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_y(), sample_band_thickness());
        assert_eq!(
            geometry.core_width(),
            sample_panel_nominal_size().width().checked_sub(sample_band_thickness()).expect("sample core fits")
        );
        assert_eq!(geometry.overall_width(), sample_panel_nominal_size().width());

        let second = PanelEdgeBandingSet {
            panel: panel_id,
            edge: right,
            material: sample_edge_material_id(),
            included_in_size: false,
        };
        session.dispatch("panel.edge_banding.set", &json_params(&second)).expect("second banding");
        let bands = session.project().panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.iter().map(|band| band.edge().get()).collect::<Vec<_>>(), vec![1, 2]);

        session.dispatch("panel.edge_banding.set", &json_params(&first)).expect("reapply bottom");
        let bands = session.project().panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.iter().map(|band| band.edge().get()).collect::<Vec<_>>(), vec![2, 1]);

        let clear = PanelEdgeBandingClear { panel: panel_id, edge: bottom };
        session.dispatch("panel.edge_banding.clear", &json_params(&clear)).expect("clear bottom");
        let bands = session.project().panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].edge(), right);

        session.undo().expect("undo clear");
        let bands = session.project().panel(panel_id).expect("panel").edge_banding();
        assert_eq!(bands.iter().map(|band| band.edge().get()).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn edge_banding_set_rejects_a_band_that_empties_the_core() {
        let edge_material = EdgeBandingMaterial::new(
            sample_edge_material_id(),
            "Wide edge",
            "pvc",
            sample_wide_band_thickness(),
            sample_band_width(),
            "white",
        )
        .expect("wide edge material");
        let catalog = Catalog::new(Vec::new(), vec![edge_material]).expect("catalog");
        let mut session = Session::with_catalog(sample_project(), catalog);
        let params = PanelEdgeBandingSet {
            panel: Id::new(2).expect("id"),
            edge: EdgeId::new(1).expect("edge id"),
            material: sample_edge_material_id(),
            included_in_size: true,
        };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.edge_banding.set").expect("registered");
        assert!(!spec.enabled(&session, &json_params(&params)));

        let error = session.dispatch("panel.edge_banding.set", &json_params(&params)).expect_err("empty core");
        assert!(matches!(error, CommandError::Model(ProjectError::PanelGeometry(_))));
        assert!(session.project().panel(Id::new(2).expect("id")).expect("panel").edge_banding().is_empty());
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn edge_banding_clear_of_a_missing_edge_is_rejected() {
        let mut session = sample_session();
        let params = PanelEdgeBandingClear { panel: Id::new(2).expect("id"), edge: EdgeId::new(1).expect("edge id") };
        let error = session.dispatch("panel.edge_banding.clear", &json_params(&params)).expect_err("missing band");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::EdgeBandingNotFound { panel, edge })
                if panel == Id::new(2).expect("id") && edge == EdgeId::new(1).expect("edge id")
        ));
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn corner_and_bow_commands_change_the_contour_and_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        let edge = RectEdge::Top.id();
        let radius = Length::from_millimetres(25).expect("radius");
        let sagitta = Length::from_millimetres(12).expect("sagitta");

        let set_corner = PanelCornerSet { panel: panel_id, corner, feature: CornerFeatureKind::Radius { radius } };
        session.dispatch("panel.corner.set", &json_params(&set_corner)).expect("set corner");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.corner_feature(corner).is_some());
        assert_eq!(session.project().revision().get(), 1);

        let set_bow = PanelBowSet { panel: panel_id, edge, sagitta };
        session.dispatch("panel.bow.set", &json_params(&set_bow)).expect("set bow");
        let panel = session.project().panel(panel_id).expect("panel");
        assert_eq!(panel.edge_bow(edge).expect("bow").sagitta(), sagitta);
        assert_eq!(session.project().revision().get(), 2);

        session.undo().expect("undo bow");
        assert!(session.project().panel(panel_id).expect("panel").edge_bow(edge).is_none());
        session.undo().expect("undo corner");
        assert!(session.project().panel(panel_id).expect("panel").corner_feature(corner).is_none());

        session.redo().expect("redo corner");
        session.redo().expect("redo bow");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.corner_feature(corner).is_some());
        assert!(panel.edge_bow(edge).is_some());

        let clear_bow = PanelBowClear { panel: panel_id, edge };
        session.dispatch("panel.bow.clear", &json_params(&clear_bow)).expect("clear bow");
        let clear_corner = PanelCornerClear { panel: panel_id, corner };
        session.dispatch("panel.corner.clear", &json_params(&clear_corner)).expect("clear corner");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(!panel.has_contour_features());
    }

    #[test]
    fn shaped_panels_accept_edge_banding() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::BottomLeft.id();
        let set_corner = PanelCornerSet {
            panel: panel_id,
            corner,
            feature: CornerFeatureKind::Chamfer {
                incoming: Length::from_millimetres(10).expect("incoming"),
                outgoing: Length::from_millimetres(20).expect("outgoing"),
            },
        };
        session.dispatch("panel.corner.set", &json_params(&set_corner)).expect("set corner");

        let band = PanelEdgeBandingSet {
            panel: panel_id,
            edge: EdgeId::new(1).expect("edge"),
            material: sample_edge_material_id(),
            included_in_size: true,
        };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.edge_banding.set").expect("registered");
        assert!(spec.enabled(&session, &json_params(&band)));
        session.dispatch("panel.edge_banding.set", &json_params(&band)).expect("band shaped panel");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.edge_banding().iter().any(|band| band.edge() == EdgeId::new(1).expect("edge")));
        assert_eq!(panel.corner_banding().len(), 0);
    }

    #[test]
    fn shaping_that_merges_banded_paths_keeps_their_records() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let band = PanelEdgeBandingSet {
            panel: panel_id,
            edge: EdgeId::new(2).expect("edge"),
            material: sample_edge_material_id(),
            included_in_size: false,
        };
        session.dispatch("panel.edge_banding.set", &json_params(&band)).expect("band right edge");

        let set_corner = PanelCornerSet {
            panel: panel_id,
            corner: RectCorner::TopRight.id(),
            feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(20).expect("radius") },
        };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.corner.set").expect("registered");
        assert!(spec.enabled(&session, &json_params(&set_corner)));
        session.dispatch("panel.corner.set", &json_params(&set_corner)).expect("shape a banded panel");

        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.has_contour_features());
        assert!(panel.edge_banding().iter().any(|band| band.edge() == EdgeId::new(2).expect("edge")));
        panel.geometry_with_fallbacks().expect("rounded banded geometry");
    }

    #[test]
    fn corner_banding_set_and_clear_change_history() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::BottomRight.id();
        let set_corner = PanelCornerSet {
            panel: panel_id,
            corner,
            feature: CornerFeatureKind::Chamfer {
                incoming: Length::from_millimetres(15).expect("incoming"),
                outgoing: Length::from_millimetres(10).expect("outgoing"),
            },
        };
        session.dispatch("panel.corner.set", &json_params(&set_corner)).expect("set corner");

        let set = PanelCornerBandingSet {
            panel: panel_id,
            corner,
            material: sample_edge_material_id(),
            included_in_size: true,
        };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.corner_banding.set").expect("registered");
        assert!(spec.enabled(&session, &json_params(&set)));
        session.dispatch("panel.corner_banding.set", &json_params(&set)).expect("set corner banding");
        let panel = session.project().panel(panel_id).expect("panel");
        let band = panel.corner_banding_at(corner).expect("corner band");
        assert_eq!(band.material(), sample_edge_material_id());
        assert!(panel.has_banding());
        assert_eq!(session.project().revision().get(), 2);

        session.undo().expect("undo corner banding");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_none());
        session.redo().expect("redo corner banding");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        let clear = PanelCornerBandingClear { panel: panel_id, corner };
        session.dispatch("panel.corner_banding.clear", &json_params(&clear)).expect("clear corner banding");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_none());
    }

    #[test]
    fn corner_banding_goes_with_its_segment() {
        let mut session = sample_session();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        let set_radius = PanelCornerSet {
            panel: panel_id,
            corner,
            feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(20).expect("radius") },
        };
        session.dispatch("panel.corner.set", &json_params(&set_radius)).expect("set radius");
        let set_band = PanelCornerBandingSet {
            panel: panel_id,
            corner,
            material: sample_edge_material_id(),
            included_in_size: false,
        };
        session.dispatch("panel.corner_banding.set", &json_params(&set_band)).expect("band a radius corner");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        let set_chamfer = PanelCornerSet {
            panel: panel_id,
            corner,
            feature: CornerFeatureKind::Chamfer {
                incoming: Length::from_millimetres(10).expect("incoming"),
                outgoing: Length::from_millimetres(15).expect("outgoing"),
            },
        };
        session.dispatch("panel.corner.set", &json_params(&set_chamfer)).expect("change to chamfer");
        assert!(session.project().panel(panel_id).expect("panel").corner_banding_at(corner).is_some());

        let clear_corner = PanelCornerClear { panel: panel_id, corner };
        session.dispatch("panel.corner.clear", &json_params(&clear_corner)).expect("clear the corner");
        let panel = session.project().panel(panel_id).expect("panel");
        assert!(panel.corner_feature(corner).is_none());
        assert!(panel.corner_banding_at(corner).is_none(), "the banding is removed with its segment");
    }

    #[test]
    fn ring_banding_set_and_clear_change_history() {
        let (mut session, panel_id) = session_with_contour_panel();
        let segment = session
            .project()
            .panel(panel_id)
            .expect("panel")
            .outline_contour()
            .expect("ring")
            .entries()
            .first()
            .expect("segment")
            .id();
        let set = PanelRingBandingSet {
            panel: panel_id,
            segment,
            material: sample_edge_material_id(),
            included_in_size: true,
        };
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.ring_banding.set").expect("registered");
        assert!(spec.enabled(&session, &json_params(&set)));
        let before = session.project().revision();

        session.dispatch("panel.ring_banding.set", &json_params(&set)).expect("band the ring segment");
        let panel = session.project().panel(panel_id).expect("panel");
        let band = panel.ring_banding_at(segment).expect("ring band");
        assert_eq!(band.material(), sample_edge_material_id());
        assert!(band.included_in_size());
        assert_eq!(panel.band_order(), &[SegmentSource::Ring(segment)]);
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));

        session.undo().expect("one undo removes the band");
        assert!(session.project().panel(panel_id).expect("panel").ring_banding_at(segment).is_none());
        session.redo().expect("redo");
        assert!(session.project().panel(panel_id).expect("panel").ring_banding_at(segment).is_some());

        let clear = PanelRingBandingClear { panel: panel_id, segment };
        let clear_spec = registry.get("panel.ring_banding.clear").expect("registered");
        assert!(clear_spec.enabled(&session, &json_params(&clear)));
        session.dispatch("panel.ring_banding.clear", &json_params(&clear)).expect("clear the ring band");
        assert!(session.project().panel(panel_id).expect("panel").ring_banding_at(segment).is_none());
    }

    #[test]
    fn ring_banding_refuses_a_rectangle_and_an_unknown_segment() {
        let registry = CommandRegistry::new();
        let set_spec = registry.get("panel.ring_banding.set").expect("registered");
        let clear_spec = registry.get("panel.ring_banding.clear").expect("registered");

        let mut session = sample_session();
        let rectangle = Id::new(2).expect("id");
        let segment = ContourSegmentId::new(1).expect("segment");
        let set = PanelRingBandingSet {
            panel: rectangle,
            segment,
            material: sample_edge_material_id(),
            included_in_size: false,
        };
        let before = session.project().revision();
        assert!(!set_spec.enabled(&session, &json_params(&set)));
        assert!(matches!(
            session.dispatch("panel.ring_banding.set", &json_params(&set)),
            Err(CommandError::Model(ProjectError::PanelContour(_)))
        ));
        assert_eq!(session.project().revision(), before, "a refused command changes nothing");

        let (mut session, panel_id) = session_with_contour_panel();
        let missing = ContourSegmentId::new(99).expect("segment");
        let set = PanelRingBandingSet {
            panel: panel_id,
            segment: missing,
            material: sample_edge_material_id(),
            included_in_size: false,
        };
        assert!(!set_spec.enabled(&session, &json_params(&set)));
        let before = session.project().revision();
        assert!(matches!(
            session.dispatch("panel.ring_banding.set", &json_params(&set)),
            Err(CommandError::Model(ProjectError::PanelContour(_)))
        ));
        assert!(session.project().panel(panel_id).expect("panel").ring_banding().is_empty());
        assert_eq!(session.project().revision(), before);

        let clear = PanelRingBandingClear { panel: panel_id, segment: missing };
        assert!(!clear_spec.enabled(&session, &json_params(&clear)));
        assert!(matches!(
            session.dispatch("panel.ring_banding.clear", &json_params(&clear)),
            Err(CommandError::Model(ProjectError::RingBandingNotFound { panel, segment }))
                if panel == panel_id && segment == missing
        ));
    }

    #[test]
    fn corner_and_bow_params_reject_unknown_fields_and_bad_ids() {
        let value = serde_json::json!({
            "panel": 2,
            "corner": 1,
            "feature": {"kind": "radius", "radius": 10000},
            "extra": true
        });
        assert!(serde_json::from_value::<PanelCornerSet>(value).is_err());

        let bad_corner = PanelCornerSet {
            panel: Id::new(2).expect("id"),
            corner: VertexId::new(5).expect("id"),
            feature: CornerFeatureKind::Radius { radius: Length::from_millimetres(10).expect("radius") },
        };
        let mut session = sample_session();
        let registry = CommandRegistry::new();
        assert!(!registry.get("panel.corner.set").expect("registered").enabled(&session, &json_params(&bad_corner)));
        assert!(matches!(
            session.dispatch("panel.corner.set", &json_params(&bad_corner)),
            Err(CommandError::InvalidParams { command: "panel.corner.set", .. })
        ));

        let bad_edge = PanelBowSet {
            panel: Id::new(2).expect("id"),
            edge: EdgeId::new(5).expect("id"),
            sagitta: Length::from_millimetres(5).expect("sagitta"),
        };
        assert!(matches!(
            session.dispatch("panel.bow.set", &json_params(&bad_edge)),
            Err(CommandError::InvalidParams { command: "panel.bow.set", .. })
        ));
    }

    #[test]
    fn mark_saved_tracks_content() {
        let mut session = sample_session();
        assert!(!session.is_dirty());

        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");
        assert!(session.is_dirty());

        session.mark_saved();
        assert!(!session.is_dirty());

        session.undo().expect("undo");
        assert!(session.is_dirty());
    }

    #[test]
    fn new_command_clears_redo() {
        let mut session = sample_session();
        let params = json_params(&panel_create(&session, "Back"));
        session.dispatch("panel.create", &params).expect("create panel");
        session.undo().expect("undo");
        assert!(session.can_redo());

        let params = json_params(&panel_create(&session, "Side"));
        session.dispatch("panel.create", &params).expect("create another panel");
        assert!(!session.can_redo());
    }

    #[test]
    fn empty_history_reports_readable_errors() {
        let mut session = sample_session();
        assert_eq!(session.undo(), Err(CommandError::NothingToUndo));
        assert_eq!(session.redo(), Err(CommandError::NothingToRedo));
    }

    #[test]
    fn unknown_command_does_not_change_the_document() {
        let mut session = sample_session();
        let error = session.dispatch("missing.command", &Value::Null).expect_err("unknown command");
        assert!(matches!(error, CommandError::UnknownCommand { ref id } if id == "missing.command"));
        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(!session.is_dirty());
    }

    #[test]
    fn invalid_params_do_not_change_the_document() {
        let mut session = sample_session();
        let error = session.dispatch("panel.create", &Value::Null).expect_err("invalid params");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.create", .. }));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn out_of_range_panel_size_is_rejected_before_mutation() {
        let mut session = sample_session();
        let mut params = json_params(&panel_create(&session, "Back"));
        let size = params
            .as_object_mut()
            .and_then(|object| object.get_mut("nominal_size"))
            .and_then(Value::as_object_mut)
            .expect("nominal_size is an object");
        size.insert("length".to_owned(), Value::from(0));

        let error = session.dispatch("panel.create", &params).expect_err("invalid size");
        assert!(matches!(error, CommandError::InvalidParams { command: "panel.create", .. }));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn empty_panel_name_is_rejected_before_mutation() {
        let mut session = sample_session();
        let params = json_params(&panel_create(&session, "   "));
        let error = session.dispatch("panel.create", &params).expect_err("empty name");
        assert!(matches!(
            error,
            CommandError::InvalidParams { command: "panel.create", ref message } if message.contains("name")
        ));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn missing_parent_is_reported_without_mutation() {
        let mut session = sample_session();
        let mut request = panel_create(&session, "Orphan");
        let missing = Id::new(999).expect("id");
        request.parent = missing;
        let params = json_params(&request);

        let error = session.dispatch("panel.create", &params).expect_err("missing parent");
        assert!(matches!(
            error,
            CommandError::Model(ProjectError::ContainerNotFound { id }) if id == missing
        ));
        assert_eq!(session.project().root().children().len(), 2);
        assert_eq!(session.project().revision(), Revision::ZERO);
    }

    #[test]
    fn enabled_rejects_wrong_state_and_params() {
        let session = sample_session();
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.create").expect("panel.create is registered");

        let valid = panel_create(&session, "Back");
        assert!(spec.enabled(&session, &json_params(&valid)));

        let mut missing = panel_create(&session, "Orphan");
        missing.parent = Id::new(999).expect("id");
        assert!(!spec.enabled(&session, &json_params(&missing)));

        let empty = panel_create(&session, " ");
        assert!(!spec.enabled(&session, &json_params(&empty)));
        assert!(!spec.enabled(&session, &Value::Null));
    }

    #[test]
    fn resize_enabled_checks_the_panel_and_machining() {
        let mut session = sample_session();
        let registry = CommandRegistry::new();
        let spec = registry.get("panel.resize").expect("panel.resize is registered");
        let panel_id = Id::new(2).expect("id");

        let valid = PanelResize { panel: panel_id, nominal_size: sample_resized_panel_nominal_size() };
        assert!(spec.enabled(&session, &json_params(&valid)));

        let missing =
            PanelResize { panel: Id::new(999).expect("id"), nominal_size: sample_resized_panel_nominal_size() };
        assert!(!spec.enabled(&session, &json_params(&missing)));
        assert!(!spec.enabled(&session, &Value::Null));

        let cutout = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_edge_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        session.dispatch("panel.machining.add", &json_params(&cutout)).expect("add edge cutout");
        let too_narrow = PanelResize { panel: panel_id, nominal_size: sample_narrow_panel_nominal_size() };
        assert!(!spec.enabled(&session, &json_params(&too_narrow)));
    }

    #[test]
    fn panel_create_params_round_trip_and_reject_unknown_fields() {
        let session = sample_session();
        let request = panel_create(&session, "Back");
        let value = serde_json::to_value(&request).expect("serialize params");
        let decoded: PanelCreate = serde_json::from_value(value.clone()).expect("deserialize params");
        assert_eq!(decoded, request);

        let mut object = value.as_object().cloned().expect("params are an object");
        object.insert("extra".to_owned(), Value::Null);
        assert!(serde_json::from_value::<PanelCreate>(Value::Object(object)).is_err());
    }

    #[test]
    fn assembly_create_rename_and_delete_are_single_undo_steps() {
        let mut session = sample_session();
        let root = session.project().root().id();
        let create = AssemblyCreate { parent: root, name: "Box".to_owned(), placement: Placement::IDENTITY };
        let outcome = session.dispatch("assembly.create", &json_params(&create)).expect("create assembly");
        let assembly = outcome.created_id().expect("created id");
        assert_eq!(session.project().container(assembly).expect("assembly").name(), "Box");
        assert_eq!(session.project().root().children().len(), 3);

        session.undo().expect("undo create");
        assert!(session.project().container(assembly).is_none());
        session.redo().expect("redo create");
        assert!(session.project().container(assembly).is_some());

        let rename = NodeRename { node: assembly, name: "Drawer box".to_owned() };
        session.dispatch("node.rename", &json_params(&rename)).expect("rename assembly");
        assert_eq!(session.project().container(assembly).expect("assembly").name(), "Drawer box");

        let delete = NodeDelete { node: assembly };
        session.dispatch("node.delete", &json_params(&delete)).expect("delete assembly");
        assert!(session.project().container(assembly).is_none());
        session.undo().expect("undo delete");
        assert_eq!(session.project().container(assembly).expect("assembly").name(), "Drawer box");
    }

    #[test]
    fn duplicate_group_and_ungroup_dispatch_through_the_engine() {
        let mut session = sample_session();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");

        let duplicate = NodeDuplicate { node: panel };
        let copy = session
            .dispatch("node.duplicate", &json_params(&duplicate))
            .expect("duplicate panel")
            .created_id()
            .expect("copy id");
        assert_ne!(copy, panel);
        assert_eq!(session.project().panel(copy).expect("copy").name(), "Shelf copy");

        let group = NodeGroup { nodes: vec![panel, drawer], name: "Cabinet".to_owned() };
        let group_id = session
            .dispatch("node.group", &json_params(&group))
            .expect("group siblings")
            .created_id()
            .expect("group id");
        assert_eq!(session.project().parent_of(panel), Some(group_id));
        assert_eq!(session.project().parent_of(drawer), Some(group_id));

        let ungroup = NodeUngroup { node: group_id };
        session.dispatch("node.ungroup", &json_params(&ungroup)).expect("ungroup");
        assert!(session.project().container(group_id).is_none());
        assert_eq!(session.project().parent_of(panel), Some(session.project().root().id()));
        assert_eq!(session.project().parent_of(drawer), Some(session.project().root().id()));
    }

    #[test]
    fn grouping_is_one_undo_entry_anchored_at_the_childrens_centre() {
        let mut session = sample_session();
        let root = session.project().root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("drawer id");
        let before_panel = session.project().world_placement(panel).expect("panel world");
        let before_drawer = session.project().world_placement(drawer).expect("drawer world");
        let revision = session.project().revision();

        let group = NodeGroup { nodes: vec![panel, drawer], name: "Cabinet".to_owned() };
        let group_id = session
            .dispatch("node.group", &json_params(&group))
            .expect("group siblings")
            .created_id()
            .expect("group id");

        // One command, one revision: the anchor and the shifted children are written together.
        assert_eq!(session.project().revision().get(), revision.get() + 1);
        let container = session.project().container(group_id).expect("group");
        assert_ne!(container.placement().translation().length(), 0.0, "the group is anchored at the centre");
        assert_eq!(session.project().pivot_local(group_id), Some(PivotLocal::ZERO));
        let world_panel = session.project().world_placement(panel).expect("panel world after");
        assert!(
            (world_panel.translation() - before_panel.translation()).length() < 1.1e-3,
            "the panel keeps its world transform"
        );
        let world_drawer = session.project().world_placement(drawer).expect("drawer world after");
        assert!((world_drawer.translation() - before_drawer.translation()).length() < 1.1e-3);

        session.undo().expect("undo grouping");
        assert!(session.project().container(group_id).is_none());
        assert_eq!(session.project().parent_of(panel), Some(root));
        assert_eq!(session.project().parent_of(drawer), Some(root));
        assert_eq!(session.project().world_placement(panel), Some(before_panel));
        assert_eq!(session.project().world_placement(drawer), Some(before_drawer));
    }

    #[test]
    fn property_commands_edit_values_as_single_undo_steps() {
        let mut session = sample_session();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("drawer id");
        let revision = session.project().revision();

        let set = NodePropertySet { node: panel, key: "finish".to_owned(), value: PropertyValue::text("oak") };
        session.dispatch("node.property.set", &json_params(&set)).expect("set property");
        assert_eq!(session.project().revision().get(), revision.get() + 1);
        assert_eq!(
            session.project().properties(panel).and_then(|properties| properties.get("finish")),
            Some(&PropertyValue::text("oak"))
        );
        session.undo().expect("undo set");
        assert!(session.project().properties(panel).expect("properties").is_empty());

        let revision = session.project().revision();
        let many = NodePropertySetMany {
            properties: vec![
                NodeProperty { node: panel, key: "finish".to_owned(), value: PropertyValue::text("oak") },
                NodeProperty { node: drawer, key: "drawer.count".to_owned(), value: PropertyValue::integer(3) },
            ],
        };
        session.dispatch("node.property.set_many", &json_params(&many)).expect("set properties");
        assert_eq!(session.project().revision().get(), revision.get() + 1);
        assert_eq!(
            session.project().properties(drawer).and_then(|properties| properties.get("drawer.count")),
            Some(&PropertyValue::integer(3))
        );
        session.undo().expect("undo batch");
        assert!(!session.project().properties(drawer).expect("properties").contains_key("drawer.count"));

        session.dispatch("node.property.set", &json_params(&set)).expect("set property again");
        let clear = NodePropertyClear { node: panel, key: "finish".to_owned() };
        session.dispatch("node.property.clear", &json_params(&clear)).expect("clear property");
        assert!(!session.project().properties(panel).expect("properties").contains_key("finish"));
        session.undo().expect("undo clear");
        assert_eq!(
            session.project().properties(panel).and_then(|properties| properties.get("finish")),
            Some(&PropertyValue::text("oak"))
        );
    }

    #[test]
    fn property_command_enabled_checks_reject_bad_params_and_protect_the_root() {
        let session = sample_session();
        let registry = CommandRegistry::new();
        let root = session.project().root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("drawer id");
        let set = registry.get("node.property.set").expect("registered");
        let clear = registry.get("node.property.clear").expect("registered");
        let many = registry.get("node.property.set_many").expect("registered");

        let good = NodePropertySet { node: panel, key: "finish".to_owned(), value: PropertyValue::text("oak") };
        assert!(set.enabled(&session, &json_params(&good)));
        assert!(!set.enabled(&session, &json_params(&NodePropertySet { node: root, ..good.clone() })));
        assert!(
            !set.enabled(&session, &json_params(&NodePropertySet { node: Id::new(999).expect("id"), ..good.clone() }))
        );
        assert!(!set.enabled(&session, &Value::Null));

        assert!(!clear.enabled(&session, &json_params(&NodePropertyClear { node: panel, key: "finish".to_owned() })));
        assert!(!clear.enabled(&session, &json_params(&NodePropertyClear { node: root, key: "finish".to_owned() })));

        let batch = |properties| NodePropertySetMany { properties };
        assert!(many.enabled(
            &session,
            &json_params(&batch(vec![
                NodeProperty { node: panel, key: "a".to_owned(), value: PropertyValue::integer(1) },
                NodeProperty { node: drawer, key: "a".to_owned(), value: PropertyValue::integer(1) },
            ]))
        ));
        assert!(!many.enabled(&session, &json_params(&batch(Vec::new()))));
        assert!(!many.enabled(
            &session,
            &json_params(&batch(vec![
                NodeProperty { node: panel, key: "a".to_owned(), value: PropertyValue::integer(1) },
                NodeProperty { node: panel, key: "a".to_owned(), value: PropertyValue::integer(2) },
            ]))
        ));
        assert!(!many.enabled(
            &session,
            &json_params(&batch(vec![NodeProperty {
                node: root,
                key: "a".to_owned(),
                value: PropertyValue::integer(1),
            }]))
        ));
    }

    #[test]
    fn a_furniture_type_makes_a_component_and_conversion_flattens_it() {
        let mut session = sample_session();
        let root = session.project().root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("drawer id");
        let registry = CommandRegistry::new();
        let ungroup = registry.get("node.ungroup").expect("registered");
        let convert = registry.get("node.component.convert_to_group").expect("registered");

        let group_id = session
            .dispatch("node.group", &json_params(&NodeGroup { nodes: vec![panel, drawer], name: "G".to_owned() }))
            .expect("group siblings")
            .created_id()
            .expect("group id");
        assert!(ungroup.enabled(&session, &json_params(&NodeUngroup { node: group_id })));
        assert!(!convert.enabled(&session, &json_params(&NodeComponentConvertToGroup { node: group_id })));

        let class: ClassCode = "30.11".parse().expect("class");
        session
            .dispatch(
                "node.property.set",
                &json_params(&NodePropertySet {
                    node: group_id,
                    key: SYSTEM_CLASS.to_owned(),
                    value: PropertyValue::class(class),
                }),
            )
            .expect("class the container");
        assert!(session.project().is_component(group_id));
        assert!(!ungroup.enabled(&session, &json_params(&NodeUngroup { node: group_id })));
        assert!(!ungroup.enabled(&session, &json_params(&NodeUngroup { node: drawer })));
        assert!(convert.enabled(&session, &json_params(&NodeComponentConvertToGroup { node: group_id })));

        assert!(session.dispatch("node.ungroup", &json_params(&NodeUngroup { node: group_id })).is_err());
        session
            .dispatch("node.component.convert_to_group", &json_params(&NodeComponentConvertToGroup { node: group_id }))
            .expect("convert to group");
        assert!(session.project().container(group_id).is_none());
        assert_eq!(session.project().parent_of(panel), Some(root));
        assert_eq!(session.project().parent_of(drawer), Some(root));

        session.undo().expect("undo conversion");
        assert!(session.project().container(group_id).is_some());
        assert!(session.project().is_component(group_id));
    }

    #[test]
    fn node_placement_changes_only_the_target_and_keeps_history() {
        let mut session = sample_session();
        let panel = Id::new(2).expect("panel id");
        let placement: Placement = serde_json::from_value(
            serde_json::json!({"translation": [10.0, 20.0, 30.0], "rotation": [0.0, 0.0, 0.0, 1.0]}),
        )
        .expect("placement");
        let request = NodePlacementSet { node: panel, placement };
        session.dispatch("node.placement.set", &json_params(&request)).expect("place panel");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), placement);
        session.undo().expect("undo placement");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
    }

    #[test]
    fn a_placement_batch_is_one_revision_and_one_undo_step() {
        let mut session = sample_session();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let moved = placement(10.0, 0.0, 0.0);
        let raised = placement(0.0, 0.0, 25.0);
        let request = NodePlacementSetMany {
            placements: vec![
                NodePlacement { node: panel, placement: moved },
                NodePlacement { node: drawer, placement: raised },
            ],
        };
        let params = json_params(&request);
        let spec = CommandRegistry::new().get("node.placement.set_many").expect("registered");
        assert!(spec.enabled(&session, &params));
        let revision = session.project().revision();

        session.dispatch("node.placement.set_many", &params).expect("place selection");

        assert_eq!(session.project().revision().get(), revision.get() + 1);
        assert_eq!(session.project().panel(panel).expect("panel").placement(), moved);
        assert_eq!(session.project().container(drawer).expect("assembly").placement(), raised);

        session.undo().expect("undo batch");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert_eq!(session.project().container(drawer).expect("assembly").placement(), Placement::IDENTITY);
        session.redo().expect("redo batch");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), moved);
        assert_eq!(session.project().container(drawer).expect("assembly").placement(), raised);
    }

    #[test]
    fn world_zero_clears_a_nested_translation_and_keeps_everything_else() {
        let mut project = sample_project();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let turn = std::f64::consts::FRAC_PI_4;
        let turned: Placement = serde_json::from_value(serde_json::json!({
            "translation": [100.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, turn.sin(), turn.cos()]
        }))
        .expect("turned placement");
        project.set_node_placement(drawer, turned).expect("turn the drawer");
        project.set_node_placement(front, placement(10.0, 0.0, 0.0)).expect("move the front");

        let before = project.panel(front).expect("front panel").clone();
        let before_world = project.world_placement(front).expect("world placement");
        let before_pivot = project.pivot_local_mm(front).expect("local pivot");
        let zeroed =
            before.placement().with_translation(Length::ZERO, Length::ZERO, Length::ZERO).expect("zero translation");
        let mut expected = project.clone();
        expected.set_node_placement(front, zeroed).expect("expected world-zero placement");

        let mut session = Session::with_catalog(project, sample_catalog());
        session
            .dispatch("node.placement.world_zero", &json_params(&NodePlacementWorldZero { node: front }))
            .expect("zero the nested translation");

        let after = session.project().panel(front).expect("front panel");
        assert_eq!(after.placement().translation_lengths(), Some([Length::ZERO; 3]));
        assert_eq!(after.placement().rotation(), before.placement().rotation(), "the rotation must stay bit-identical");
        assert_eq!(
            session.project().world_placement(front).expect("world placement").rotation(),
            before_world.rotation(),
            "the composed world rotation must not change"
        );
        assert_eq!(session.project().pivot_local_mm(front), Some(before_pivot), "the local pivot must not move");
        assert_eq!(session.project(), &expected, "only the placement may change");

        session.undo().expect("undo world zero");
        assert_eq!(session.project().panel(front).expect("front panel").placement(), before.placement());
        assert!(!session.can_undo(), "world zero must be exactly one undo entry");
    }

    #[test]
    fn world_zero_at_the_root_lands_on_the_world_origin_and_refuses_the_root_itself() {
        let mut session = sample_session();
        let panel = Id::new(2).expect("panel id");
        let root = session.project().root().id();
        let turned: Placement = serde_json::from_value(serde_json::json!({
            "translation": [5.0, 7.0, 9.0],
            "rotation": [0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2]
        }))
        .expect("turned placement");
        session
            .dispatch("node.placement.set", &json_params(&NodePlacementSet { node: panel, placement: turned }))
            .expect("place the panel");

        let spec = CommandRegistry::new().get("node.placement.world_zero").expect("registered");
        assert!(spec.enabled(&session, &json_params(&NodePlacementWorldZero { node: panel })));
        assert!(!spec.enabled(&session, &json_params(&NodePlacementWorldZero { node: root })));
        assert!(!spec.enabled(&session, &json_params(&NodePlacementWorldZero { node: Id::new(999).expect("id") })));

        session
            .dispatch("node.placement.world_zero", &json_params(&NodePlacementWorldZero { node: panel }))
            .expect("zero a root-level translation");
        let world = session.project().world_placement(panel).expect("world placement");
        assert_eq!(world.translation(), furnicraft_model::Placement::IDENTITY.translation());
        assert_eq!(world.rotation(), turned.rotation());

        let error = session
            .dispatch("node.placement.world_zero", &json_params(&NodePlacementWorldZero { node: root }))
            .expect_err("the root has no placement");
        assert!(matches!(error, CommandError::Model(ProjectError::RootImmutable { .. })));
    }

    #[test]
    fn an_invalid_placement_batch_is_rejected_without_mutation() {
        let mut session = sample_session();
        let root = session.project().root().id();
        let panel = Id::new(2).expect("panel id");
        let missing = Id::new(999).expect("id");
        let moved = placement(10.0, 0.0, 0.0);
        let spec = CommandRegistry::new().get("node.placement.set_many").expect("registered");
        let revision = session.project().revision();

        let empty = NodePlacementSetMany { placements: Vec::new() };
        assert!(!spec.enabled(&session, &json_params(&empty)));
        assert!(session.dispatch("node.placement.set_many", &json_params(&empty)).is_err());

        let duplicate = NodePlacementSetMany {
            placements: vec![
                NodePlacement { node: panel, placement: moved },
                NodePlacement { node: panel, placement: moved },
            ],
        };
        assert!(!spec.enabled(&session, &json_params(&duplicate)));
        assert!(session.dispatch("node.placement.set_many", &json_params(&duplicate)).is_err());

        let with_root = NodePlacementSetMany {
            placements: vec![
                NodePlacement { node: panel, placement: moved },
                NodePlacement { node: root, placement: moved },
            ],
        };
        assert!(!spec.enabled(&session, &json_params(&with_root)));
        assert!(session.dispatch("node.placement.set_many", &json_params(&with_root)).is_err());

        let with_missing = NodePlacementSetMany {
            placements: vec![
                NodePlacement { node: panel, placement: moved },
                NodePlacement { node: missing, placement: moved },
            ],
        };
        assert!(!spec.enabled(&session, &json_params(&with_missing)));
        assert!(session.dispatch("node.placement.set_many", &json_params(&with_missing)).is_err());

        assert_eq!(session.project().revision(), revision);
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
    }

    #[test]
    fn node_command_enabled_checks_reject_invalid_selections() {
        let session = sample_session();
        let registry = CommandRegistry::new();
        let root = session.project().root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");

        let create = registry.get("assembly.create").expect("registered");
        assert!(create.enabled(
            &session,
            &json_params(&AssemblyCreate { parent: root, name: "Box".to_owned(), placement: Placement::IDENTITY })
        ));
        assert!(!create.enabled(
            &session,
            &json_params(&AssemblyCreate {
                parent: Id::new(999).expect("id"),
                name: "Box".to_owned(),
                placement: Placement::IDENTITY,
            })
        ));

        let delete = registry.get("node.delete").expect("registered");
        assert!(delete.enabled(&session, &json_params(&NodeDelete { node: panel })));
        assert!(!delete.enabled(&session, &json_params(&NodeDelete { node: root })));

        let group = registry.get("node.group").expect("registered");
        assert!(group.enabled(&session, &json_params(&NodeGroup { nodes: vec![panel, drawer], name: "G".to_owned() })));
        assert!(!group.enabled(&session, &json_params(&NodeGroup { nodes: vec![panel], name: "G".to_owned() })));
        assert!(!group.enabled(&session, &json_params(&NodeGroup { nodes: vec![panel, panel], name: "G".to_owned() })));

        let ungroup = registry.get("node.ungroup").expect("registered");
        let mut session = session;
        let group_id = session
            .dispatch("node.group", &json_params(&NodeGroup { nodes: vec![panel, drawer], name: "Group".to_owned() }))
            .expect("group siblings")
            .created_id()
            .expect("group id");
        assert!(ungroup.enabled(&session, &json_params(&NodeUngroup { node: group_id })));
        assert!(!ungroup.enabled(&session, &json_params(&NodeUngroup { node: panel })));
        assert!(!ungroup.enabled(&session, &json_params(&NodeUngroup { node: drawer })));
        assert!(!ungroup.enabled(&session, &json_params(&NodeUngroup { node: root })));

        assert!(!registry.get("node.group").expect("registered").enabled(&session, &Value::Null));
    }

    #[test]
    fn origin_rebase_moves_the_origin_and_keeps_world_geometry_and_pivot() {
        let mut session = sample_session();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let pivot = PivotLocal::new(
            Length::from_micrometres(1_500),
            Length::from_micrometres(-2_500),
            Length::from_micrometres(3_000),
        );
        session
            .dispatch("node.pivot.set", &json_params(&NodePivotSet { node: drawer, pivot }))
            .expect("author the pivot");
        let before_origin = session.project().origin_world(drawer).expect("world origin");
        let before_pivot = session.project().pivot_world(drawer).expect("world pivot");
        let before_front = session.project().world_placement(front).expect("front world");

        session
            .dispatch(
                "node.origin.rebase",
                &json_params(&NodeOriginRebase { node: drawer, strategy: FrameAnchor::CenterOfBounds }),
            )
            .expect("rebase the drawer origin");

        let after_origin = session.project().origin_world(drawer).expect("world origin");
        assert!((after_origin - before_origin).length() > 1.0, "the origin moved to the children's centre");
        assert!(
            (session.project().pivot_world(drawer).expect("world pivot") - before_pivot).length() < 1.0e-3,
            "the authored pivot_world is unchanged"
        );
        let after_front = session.project().world_placement(front).expect("front world");
        assert!(
            (after_front.translation() - before_front.translation()).length() < 1.0e-3
                && after_front.rotation().dot(before_front.rotation()).abs() > 1.0 - 1.0e-9,
            "re-basing a container moves no child in world space"
        );

        session.undo().expect("undo the rebase");
        assert_eq!(session.project().origin_world(drawer), Some(before_origin));
        assert_eq!(session.project().pivot_local(drawer), Some(pivot));
        session.undo().expect("undo the authored pivot");
        assert!(!session.can_undo(), "each operation is one undo entry");
    }

    #[test]
    fn pivot_commands_edit_one_value_and_one_undo_entry() {
        let mut session = sample_session();
        let drawer = Id::new(3).expect("drawer id");
        let root = session.project().root().id();
        let pivot = PivotLocal::new(
            Length::from_micrometres(1_500),
            Length::from_micrometres(-2_500),
            Length::from_micrometres(3_000),
        );

        session.dispatch("node.pivot.set", &json_params(&NodePivotSet { node: drawer, pivot })).expect("set pivot");
        assert_eq!(session.project().pivot_local(drawer), Some(pivot));
        assert!(session.can_undo());

        session.dispatch("node.pivot.center", &json_params(&NodePivotCenter { node: drawer })).expect("center pivot");
        assert_eq!(
            session.project().pivot_local(drawer),
            Some(PivotLocal::new(
                Length::from_micrometres(350_000),
                Length::from_micrometres(250_000),
                Length::from_micrometres(9_000),
            ))
        );

        session.undo().expect("undo centring");
        assert_eq!(session.project().pivot_local(drawer), Some(pivot));
        session.undo().expect("undo set");
        // The drawer was built with children but a container is born at its own origin; the centre
        // is what `node.pivot.center` writes, not a default.
        assert_eq!(session.project().pivot_local(drawer), Some(PivotLocal::ZERO));
        assert!(!session.can_undo(), "each pivot edit is exactly one undo entry");

        let registry = CommandRegistry::new();
        let set = registry.get("node.pivot.set").expect("registered");
        let center = registry.get("node.pivot.center").expect("registered");
        assert!(set.enabled(&session, &json_params(&NodePivotSet { node: drawer, pivot })));
        assert!(!set.enabled(&session, &json_params(&NodePivotSet { node: root, pivot })));
        assert!(center.enabled(&session, &json_params(&NodePivotCenter { node: drawer })));
        assert!(!center.enabled(&session, &json_params(&NodePivotCenter { node: root })));

        let error = session
            .dispatch("node.pivot.set", &json_params(&NodePivotSet { node: root, pivot }))
            .expect_err("the root has no pivot");
        assert!(matches!(error, CommandError::Model(ProjectError::RootImmutable { .. })));

        // A panel's pivot is derived from its base corner, so neither command authors one.
        let panel = Id::new(2).expect("panel id");
        let revision = session.project().revision();
        assert!(!set.enabled(&session, &json_params(&NodePivotSet { node: panel, pivot })));
        assert!(!center.enabled(&session, &json_params(&NodePivotCenter { node: panel })));
        let error = session
            .dispatch("node.pivot.set", &json_params(&NodePivotSet { node: panel, pivot }))
            .expect_err("a panel pivot is derived");
        assert!(matches!(error, CommandError::Model(ProjectError::PanelPivotIsDerived { panel: id }) if id == panel));
        let error = session
            .dispatch("node.pivot.center", &json_params(&NodePivotCenter { node: panel }))
            .expect_err("a panel pivot is derived");
        assert!(matches!(error, CommandError::Model(ProjectError::PanelPivotIsDerived { panel: id }) if id == panel));
        assert_eq!(session.project().pivot_local(panel), Some(PivotLocal::ZERO));
        assert_eq!(session.project().revision(), revision, "the refused commands change nothing");
    }
    /// The sample shelf (id 2) classed as `code`, in a session whose root has no class.
    fn classed_shelf(code: &str) -> (Session, Id) {
        let mut session = sample_session();
        let shelf = Id::new(2).expect("id");
        let class: ClassCode = code.parse().expect("a class code");
        session
            .dispatch(
                "node.property.set",
                &json_params(&NodePropertySet {
                    node: shelf,
                    key: SYSTEM_CLASS.to_owned(),
                    value: PropertyValue::class(class),
                }),
            )
            .expect("assign the class");
        (session, shelf)
    }

    fn resize_request(panel: Id, length_mm: i64, width_mm: i64) -> Value {
        let nominal_size = furnicraft_model::PanelNominalSize::new(
            Length::from_millimetres(length_mm).expect("length"),
            Length::from_millimetres(width_mm).expect("width"),
        )
        .expect("a valid size");
        json_params(&PanelResize { panel, nominal_size })
    }

    #[test]
    fn a_worktop_resizes_by_length_only() {
        let (mut session, panel) = classed_shelf("70.01");
        let registry = CommandRegistry::new();
        let resize = registry.get("panel.resize").expect("panel.resize is registered");
        let revision = session.project().revision();

        let longer = resize_request(panel, 900, 500);
        assert!(resize.enabled(&session, &longer), "a worktop grows along its length");
        session.dispatch("panel.resize", &longer).expect("resize along the length");
        assert_eq!(
            session.project().panel(panel).expect("panel").nominal_size().length(),
            Length::from_millimetres(900).expect("length")
        );

        let wider = resize_request(panel, 900, 600);
        assert!(!resize.enabled(&session, &wider), "a worktop never stretches across its width");
        let error = session.dispatch("panel.resize", &wider).expect_err("the width is fixed");
        assert!(matches!(
            error,
            CommandError::ClassRefused { command: "panel.resize", refusal: ClassRefusal::Dimension(Dimension::Width) }
        ));
        assert_eq!(
            session.project().panel(panel).expect("panel").nominal_size().width(),
            Length::from_millimetres(500).expect("width"),
            "the refused resize changes nothing"
        );
        assert!(session.project().revision() > revision);
    }

    #[test]
    fn changing_the_class_flips_the_same_resize_command() {
        let (mut session, panel) = classed_shelf("30.11");
        let registry = CommandRegistry::new();
        let resize = registry.get("panel.resize").expect("panel.resize is registered");
        let wider = resize_request(panel, 700, 600);
        assert!(resize.enabled(&session, &wider), "a horizontal panel resizes freely");
        session.dispatch("panel.resize", &wider).expect("widen the shelf");

        let class: ClassCode = "70.01".parse().expect("the worktop code");
        session
            .dispatch(
                "node.property.set",
                &json_params(&NodePropertySet {
                    node: panel,
                    key: SYSTEM_CLASS.to_owned(),
                    value: PropertyValue::class(class),
                }),
            )
            .expect("reclass the same panel");
        let even_wider = resize_request(panel, 700, 700);
        assert!(!resize.enabled(&session, &even_wider), "the same command now refuses the width");
        assert!(matches!(
            session.dispatch("panel.resize", &even_wider).expect_err("the width is fixed"),
            CommandError::ClassRefused { command: "panel.resize", .. }
        ));
    }

    #[test]
    fn a_hinge_offers_no_resize_and_no_banding() {
        let (mut session, hinge) = classed_shelf("50.02.01");
        let registry = CommandRegistry::new();
        let revision = session.project().revision();

        let resize = registry.get("panel.resize").expect("panel.resize is registered");
        let bigger = resize_request(hinge, 800, 500);
        assert!(!resize.enabled(&session, &bigger), "a hinge is never resized");
        assert!(matches!(
            session.dispatch("panel.resize", &bigger).expect_err("a hinge keeps its size"),
            CommandError::ClassRefused { command: "panel.resize", refusal: ClassRefusal::Dimension(_) }
        ));

        let band = registry.get("panel.edge_banding.set").expect("edge banding is registered");
        let params = json_params(&PanelEdgeBandingSet {
            panel: hinge,
            edge: RectEdge::Top.id(),
            material: sample_edge_material_id(),
            included_in_size: false,
        });
        assert!(!band.enabled(&session, &params), "a fitting is never banded");
        assert!(matches!(
            session.dispatch("panel.edge_banding.set", &params).expect_err("no banding on a hinge"),
            CommandError::ClassRefused { command: "panel.edge_banding.set", refusal: ClassRefusal::Operation(_) }
        ));
        assert_eq!(session.project().revision(), revision, "the refused commands change nothing");
    }

    #[test]
    fn a_class_without_cutouts_refuses_a_cutout() {
        let (mut session, plinth) = classed_shelf("70.02");
        let shape = CutoutShape::Rectangle(sample_cutout_rect());
        let params = json_params(&cutout_request(plinth, shape, CutoutDepth::Through, FaceSide::A));
        let registry = CommandRegistry::new();
        let add = registry.get("panel.machining.add").expect("machining add is registered");
        assert!(!add.enabled(&session, &params), "a plinth admits no cutout");
        assert!(matches!(
            session.dispatch("panel.machining.add", &params).expect_err("no cutout in a plinth"),
            CommandError::ClassRefused { command: "panel.machining.add", refusal: ClassRefusal::Operation(_) }
        ));

        let (mut session, worktop) = classed_shelf("70.01");
        let params = json_params(&cutout_request(
            worktop,
            CutoutShape::Rectangle(sample_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        ));
        assert!(add.enabled(&session, &params), "a worktop takes a sink cutout");
        session.dispatch("panel.machining.add", &params).expect("a cutout in a worktop");
    }

    #[test]
    fn mounting_checks_the_host_class_on_assignment() {
        let mut session = sample_session();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let worktop: ClassCode = "70.01".parse().expect("the worktop code");
        session
            .dispatch(
                "node.property.set",
                &json_params(&NodePropertySet {
                    node: drawer,
                    key: SYSTEM_CLASS.to_owned(),
                    value: PropertyValue::class(worktop),
                }),
            )
            .expect("class the host");

        let hinge: ClassCode = "50.02".parse().expect("the cup hinge code");
        let refused = json_params(&NodePropertySet {
            node: front,
            key: SYSTEM_CLASS.to_owned(),
            value: PropertyValue::class(hinge),
        });
        let registry = CommandRegistry::new();
        let set = registry.get("node.property.set").expect("property set is registered");
        assert!(!set.enabled(&session, &refused), "a hinge does not sit in a worktop");
        assert!(matches!(
            session.dispatch("node.property.set", &refused).expect_err("a hinge does not sit in a worktop"),
            CommandError::ClassRefused { command: "node.property.set", refusal: ClassRefusal::Host { host } } if host == worktop
        ));

        let sink: ClassCode = "60.01".parse().expect("the sink code");
        let allowed = json_params(&NodePropertySet {
            node: front,
            key: SYSTEM_CLASS.to_owned(),
            value: PropertyValue::class(sink),
        });
        assert!(set.enabled(&session, &allowed), "a sink sits in a worktop");
        session.dispatch("node.property.set", &allowed).expect("assign the sink");
    }
}
