//! The editing session: document, command dispatch and undo/redo.

use std::sync::Arc;

use furnicraft_model::{BandingSpec, Catalog, EdgeBanding, Id, PanelGeometry, Project, ProjectError};
use furnicraft_param::EdgeBandProfile;
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandRegistry};
use crate::diagnostics::{PanelDiagnostic, diagnose_panel};
use crate::resolve;

/// A document plus its undo/redo history and saved-content marker.
#[derive(Debug)]
pub struct Session {
    project: Project,
    saved_project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    dirty: bool,
    catalog: Arc<Catalog>,
}

impl Session {
    /// Creates a clean session around a document with an empty catalogue.
    pub fn new(project: Project) -> Self {
        Self::with_catalog(project, Catalog::default())
    }

    /// Creates a clean session around a document and its read-only material catalogue.
    pub fn with_catalog(project: Project, catalog: Catalog) -> Self {
        let saved_project = project.clone();
        Self { project, saved_project, undo: Vec::new(), redo: Vec::new(), dirty: false, catalog: Arc::new(catalog) }
    }

    /// The current document. Commands are the only way to mutate it.
    pub fn project(&self) -> &Project {
        &self.project
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

    /// Dispatches a command by id with JSON parameters.
    ///
    /// The command runs on a copy of the document. A successful command becomes one undo entry;
    /// a failed command leaves the current document untouched.
    pub fn dispatch(&mut self, id: &str, params: &Value) -> Result<CommandOutcome, CommandError> {
        let spec = CommandRegistry::new().get(id).ok_or_else(|| CommandError::UnknownCommand { id: id.to_owned() })?;
        let mut working = self.project.clone();
        let outcome = spec.run(&mut working, &self.catalog, params)?;
        let previous = std::mem::replace(&mut self.project, working);
        self.undo.push(previous);
        self.redo.clear();
        self.dirty = !self.project.same_content(&self.saved_project);
        Ok(outcome)
    }

    /// Restores the content from before the last command and advances the revision.
    pub fn undo(&mut self) -> Result<(), CommandError> {
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
        Catalog, CornerFeatureKind, CutoutDepth, CutoutShape, EdgeBandingMaterial, EdgeId, FaceSide, Id, Machining,
        PanelMaterial, Placement, ProjectError, RectCorner, RectEdge, Revision, VertexId,
    };
    use furnicraft_testkit::{
        sample_band_thickness, sample_band_width, sample_blind_depth, sample_catalog, sample_cutout_rect,
        sample_edge_cutout_rect, sample_edge_material_id, sample_material_id, sample_narrow_panel_nominal_size,
        sample_notch_cutout_rect, sample_outside_cutout_rect, sample_overlapping_cutout_rect,
        sample_panel_nominal_size, sample_panel_thickness, sample_project, sample_resized_panel_nominal_size,
        sample_thin_panel_thickness, sample_wide_band_thickness,
    };
    use furnicraft_units::Length;

    use crate::node_cmds::{
        AssemblyCreate, NodeDelete, NodeDuplicate, NodeGroup, NodePlacementSet, NodeRename, NodeUngroup,
    };
    use crate::panel_cmds::{
        CutoutCreate, PanelBowClear, PanelBowSet, PanelCornerBandingClear, PanelCornerBandingSet, PanelCornerClear,
        PanelCornerSet, PanelCreate, PanelEdgeBandingClear, PanelEdgeBandingSet, PanelMaterialSet, PanelRename,
        PanelResize,
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

    fn cutout_request(panel: Id, shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> CutoutCreate {
        CutoutCreate { panel, shape, depth, entry }
    }

    fn json_params<T: serde::Serialize>(value: &T) -> Value {
        serde_json::to_value(value).expect("params serialize")
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
        let params = json_params(&cutout_request(panel_id, shape, CutoutDepth::Through, FaceSide::A));

        let outcome = session.dispatch("cutout.create", &params).expect("create cutout");
        let cutout_id = outcome.created_id().expect("cutout id");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().next_id().get(), 6);
        let machining = session.project().panel(panel_id).expect("panel").machining();
        assert_eq!(machining.len(), 1);
        let Machining::Cutout(cutout) = &machining[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);
        assert_eq!(cutout.entry(), FaceSide::A);
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

    #[test]
    fn dispatch_blind_cutout_records_entry_and_depth() {
        let mut session = sample_session();
        let panel_id = Id::new(4).expect("id");
        let shape = CutoutShape::Rectangle(sample_cutout_rect());
        let depth = CutoutDepth::Blind { depth: sample_blind_depth() };
        let params = json_params(&cutout_request(panel_id, shape, depth, FaceSide::B));

        session.dispatch("cutout.create", &params).expect("create blind cutout");
        let machining = session.project().panel(panel_id).expect("panel").machining();
        let Machining::Cutout(cutout) = &machining[0];
        assert_eq!(cutout.depth(), depth);
        assert_eq!(cutout.entry(), FaceSide::B);
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
        let error = session.dispatch("cutout.create", &json_params(&outside)).expect_err("outside cutout");
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
        let error = session.dispatch("cutout.create", &json_params(&invalid_depth)).expect_err("invalid depth");
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
            .dispatch("cutout.create", &json_params(&first))
            .expect("first cutout")
            .created_id()
            .expect("cutout id");
        assert_eq!(session.project().revision().get(), 1);
        assert_eq!(session.project().next_id().get(), 6);

        let overlapping = cutout_request(
            panel_id,
            CutoutShape::Rectangle(sample_overlapping_cutout_rect()),
            CutoutDepth::Through,
            FaceSide::A,
        );
        let error = session.dispatch("cutout.create", &json_params(&overlapping)).expect_err("overlapping cutout");
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
            session.dispatch("cutout.create", &json_params(&request)).expect("through cutout");
        }
        assert_eq!(session.project().panel(panel_id).expect("panel").machining().len(), 3);
    }

    #[test]
    fn cutout_enabled_checks_panel_and_geometry() {
        let mut session = sample_session();
        let registry = CommandRegistry::new();
        let spec = registry.get("cutout.create").expect("cutout.create is registered");
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

        session.dispatch("cutout.create", &json_params(&valid)).expect("first cutout");
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
        session.dispatch("cutout.create", &json_params(&blind)).expect("blind cutout");

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
        session.dispatch("cutout.create", &json_params(&cutout)).expect("add edge cutout");
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
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Box");
        assert_eq!(session.project().root().children().len(), 3);

        session.undo().expect("undo create");
        assert!(session.project().assembly(assembly).is_none());
        session.redo().expect("redo create");
        assert!(session.project().assembly(assembly).is_some());

        let rename = NodeRename { node: assembly, name: "Drawer box".to_owned() };
        session.dispatch("node.rename", &json_params(&rename)).expect("rename assembly");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer box");

        let delete = NodeDelete { node: assembly };
        session.dispatch("node.delete", &json_params(&delete)).expect("delete assembly");
        assert!(session.project().assembly(assembly).is_none());
        session.undo().expect("undo delete");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer box");
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
        assert!(session.project().group(group_id).is_none());
        assert_eq!(session.project().parent_of(panel), Some(session.project().root().id()));
        assert_eq!(session.project().parent_of(drawer), Some(session.project().root().id()));
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
}
