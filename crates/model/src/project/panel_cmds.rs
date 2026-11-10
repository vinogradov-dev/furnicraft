//! Commands that change a panel: size, outline, material, banding, corner features and cutouts.
//!
//! Every command validates a cloned candidate before it touches the tree, so a rejected edit leaves
//! the document, its revision and the id counter exactly as they were.

use crate::contour::{MAX_SELF_INTERSECTION_PAIRS, SELF_INTERSECTION_TOLERANCE_MICROMETRES, SegBounds, segments_touch};
use crate::panel::{core_violation_of, rects_overlap, validate_ring_banding};
use crate::tree::{find_container_mut, find_panel_mut};
use furnicraft_geom::Region;

use super::scan_tree;
use crate::*;

impl Project {
    /// Checks whether a panel can take a new nominal size without invalidating its machining.
    pub fn validate_panel_resize(&self, id: Id, nominal_size: PanelNominalSize) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let geometry = panel.geometry_for(nominal_size, panel.last_known_thickness())?;
        validate_panel_machining(panel, &geometry)
    }

    /// Replaces a panel's nominal size and bumps the revision once.
    ///
    /// Existing machining is not moved or scaled. A cutout that would fall outside the new core or
    /// a blind cutout that would become deeper than the new thickness rejects the resize. The
    /// material, fallback thickness and edge banding are preserved.
    pub fn set_panel_nominal_size(&mut self, id: Id, nominal_size: PanelNominalSize) -> Result<Revision, ProjectError> {
        self.validate_panel_resize(id, nominal_size)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_nominal_size(nominal_size);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a ring can replace a contour panel's outline.
    ///
    /// The ring is first moved to the panel-local zero corner, exactly as
    /// [`Panel::anchored_contour`] does for a new panel, and its bounds must form a usable panel
    /// size. Every ring banding record must still name a segment the ring has: an edit that drops a
    /// banded segment is refused instead of silently dropping the band.
    pub fn validate_panel_outline(&self, id: Id, contour: &Contour) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if !panel.is_contour() {
            return Err(ProjectError::PanelNotContour { id });
        }
        let anchored = Panel::anchored_contour(contour)?;
        Panel::contour_size(&anchored)?;
        validate_ring_banding(&anchored, panel.ring_banding())?;
        Ok(())
    }

    /// Replaces a contour panel's outline and bumps the revision once.
    ///
    /// The new ring is anchored at the panel-local zero corner (ADR 0009). When the edit moves the
    /// minimum corner, the panel placement, pivot and machining shift by the same offset so the
    /// world-space shape and every feature stay where the user drew them. A rectangle panel never
    /// converts; [`Project::validate_panel_outline`] rejects it before anything changes.
    pub fn set_panel_outline(&mut self, id: Id, contour: Contour) -> Result<Revision, ProjectError> {
        self.validate_panel_outline(id, &contour)?;
        let (min, _) = contour.bounds_micrometres();
        let anchored = Panel::anchored_contour(&contour)?;
        let nominal_size = Panel::contour_size(&anchored)?;
        let next_revision = self.next_revision()?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.move_frame_to(min)?;
        panel.set_nominal_size(nominal_size);
        panel.set_outline(PanelOutline::Contour { contour: anchored });
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a rectangle panel's outline can bake into a contour ring.
    ///
    /// The conversion is one-way: the corner features and bows become curve segments, every edge
    /// and corner band moves onto the ring segment it covered (ADR 0061), and the resulting ring
    /// must still resolve to a panel geometry. A panel that already stores a ring is refused; so is
    /// a corner band whose corner has no segment.
    pub fn validate_panel_contour_conversion(&self, id: Id) -> Result<(), ProjectError> {
        self.converted_panel(id).map(|_| ())
    }

    /// Replaces a rectangle panel's outline with the ring it draws, in one revision and one undo.
    ///
    /// The ring is anchored at the panel-local zero corner like any contour panel (ADR 0009), so an
    /// outward bow that extended the outline moves the frame; the placement, pivot and machining
    /// follow by the same offset so the world-space shape stays fixed. The corner and bow
    /// parameters are gone, but undo restores the rectangle with its features.
    pub fn convert_panel_to_contour(&mut self, id: Id) -> Result<Revision, ProjectError> {
        let candidate = self.converted_panel(id)?;
        let next_revision = self.next_revision()?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        *panel = candidate;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Builds and validates the ring panel a rectangle bakes into, without touching the tree.
    fn converted_panel(&self, id: Id) -> Result<Panel, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let baked = panel.to_contour_ring()?;
        let (min, _) = baked.contour().bounds_micrometres();
        let anchored = Panel::anchored_contour(baked.contour())?;
        let nominal_size = Panel::contour_size(&anchored)?;
        let mut candidate = panel.clone();
        candidate.move_frame_to(min)?;
        candidate.set_nominal_size(nominal_size);
        candidate.set_outline(PanelOutline::Contour { contour: anchored });
        candidate.set_contour_banding(baked.banding().to_vec(), baked.order().to_vec());
        candidate.geometry_with_fallbacks()?;
        Ok(candidate)
    }

    /// Renames a panel and bumps the revision once.
    ///
    /// The name is user-visible text; callers validate that it is not blank before calling this
    /// method. The panel id and every other property stay unchanged.
    pub fn rename_panel(&mut self, id: Id, name: impl Into<String>) -> Result<Revision, ProjectError> {
        if self.panel(id).is_none() {
            return Err(ProjectError::PanelNotFound { id });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_name(name);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Sets the panel material and refreshes its fallback thickness.
    ///
    /// A material or catalogue change may invalidate machining; that is a diagnostic, not a
    /// command error. Only a non-positive resulting core rejects the change.
    pub fn set_panel_material(
        &mut self,
        id: Id,
        material: MaterialId,
        thickness: Length,
    ) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        panel.geometry_for(panel.nominal_size(), thickness)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.set_material(Some(material));
        panel.set_thickness(thickness);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a banding can be placed without making the core non-positive.
    pub fn validate_edge_banding(&self, id: Id, band: EdgeBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.apply_edge_banding(band)?;
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces banding on one edge segment and moves it to the end of the order.
    pub fn set_edge_banding(&mut self, id: Id, band: EdgeBanding) -> Result<Revision, ProjectError> {
        self.validate_edge_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let edge = band.edge();
        panel.apply_edge_banding(band)?;
        panel.add_band_source(SegmentSource::Edge(edge));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from one edge segment and bumps the revision once.
    pub fn clear_edge_banding(&mut self, id: Id, edge: EdgeId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if !panel.edge_banding().iter().any(|band| band.edge() == edge) {
            return Err(ProjectError::EdgeBandingNotFound { panel: id, edge });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.clear_edge_banding_path(edge)?;
        panel.remove_band_source(SegmentSource::Edge(edge));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Validates a corner-banding record without changing the document.
    ///
    /// A banding belongs to a segment, so the corner must currently have a chamfer or radius.
    pub fn validate_corner_banding(&self, id: Id, band: CornerBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let corner = band.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(ProjectError::UnsupportedCornerBanding { panel: id, corner });
        }
        if panel.corner_feature(corner).is_none() {
            return Err(ProjectError::CornerFeatureNotFound { panel: id, corner });
        }
        let mut candidate = panel.clone();
        candidate.upsert_corner_banding(band);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces corner banding and moves it to the end of the order.
    pub fn set_corner_banding(&mut self, id: Id, band: CornerBanding) -> Result<Revision, ProjectError> {
        self.validate_corner_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let corner = band.corner();
        panel.upsert_corner_banding(band);
        panel.add_band_source(SegmentSource::Corner(corner));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from a corner segment and bumps the revision once.
    pub fn clear_corner_banding(&mut self, id: Id, corner: VertexId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.corner_banding_at(corner).is_none() {
            return Err(ProjectError::CornerBandingNotFound { panel: id, corner });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_corner_banding(corner);
        panel.remove_band_source(SegmentSource::Corner(corner));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a corner treatment can be applied without invalidating the panel.
    ///
    /// Banding does not block a geometry change: edge records stay on their edges and a corner
    /// record stays while its corner segment exists.
    pub fn validate_corner_feature(&self, id: Id, feature: CornerFeature) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.upsert_corner_feature(feature);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces a corner treatment and bumps the revision once.
    pub fn set_corner_feature(&mut self, id: Id, feature: CornerFeature) -> Result<Revision, ProjectError> {
        self.validate_corner_feature(id, feature)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.upsert_corner_feature(feature);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a corner treatment and bumps the revision once.
    ///
    /// The corner banding belongs to the corner segment, so it is removed together with the
    /// treatment; adjacent edge banding is untouched.
    pub fn clear_corner_feature(&mut self, id: Id, corner: VertexId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.corner_feature(corner).is_none() {
            return Err(ProjectError::CornerFeatureNotFound { panel: id, corner });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_corner_banding(corner);
        panel.remove_band_source(SegmentSource::Corner(corner));
        panel.remove_corner_feature(corner);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether a bowed edge can be applied without invalidating the panel.
    ///
    /// Banding does not block a geometry change; existing edge-banding records stay on their edges.
    pub fn validate_edge_bow(&self, id: Id, bow: EdgeBow) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.upsert_edge_bow(bow);
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces a bowed edge and bumps the revision once.
    pub fn set_edge_bow(&mut self, id: Id, bow: EdgeBow) -> Result<Revision, ProjectError> {
        self.validate_edge_bow(id, bow)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.upsert_edge_bow(bow);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a bowed edge and bumps the revision once.
    pub fn clear_edge_bow(&mut self, id: Id, edge: EdgeId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.edge_bow(edge).is_none() {
            return Err(ProjectError::EdgeBowNotFound { panel: id, edge });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_edge_bow(edge);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks whether ring banding can be set without invalidating the panel.
    pub fn validate_ring_banding(&self, id: Id, band: RingBanding) -> Result<(), ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        let mut candidate = panel.clone();
        candidate.apply_ring_banding(band)?;
        candidate.geometry_with_fallbacks()?;
        Ok(())
    }

    /// Adds or replaces banding on one ring segment and moves it to the end of the order.
    pub fn set_ring_banding(&mut self, id: Id, band: RingBanding) -> Result<Revision, ProjectError> {
        self.validate_ring_banding(id, band.clone())?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        let segment = band.segment();
        panel.apply_ring_banding(band)?;
        panel.add_band_source(SegmentSource::Ring(segment));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes banding from one ring segment and bumps the revision once.
    pub fn clear_ring_banding(&mut self, id: Id, segment: ContourSegmentId) -> Result<Revision, ProjectError> {
        let panel = self.panel(id).ok_or(ProjectError::PanelNotFound { id })?;
        if panel.ring_banding_at(segment).is_none() {
            return Err(ProjectError::RingBandingNotFound { panel: id, segment });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel)) = find_panel_mut(&mut self.root, id, 1) else {
            return Err(ProjectError::PanelNotFound { id });
        };
        panel.remove_ring_banding(segment);
        panel.remove_band_source(SegmentSource::Ring(segment));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Adds a face-anchored cutout and bumps the revision once.
    ///
    /// This is the one-operation shorthand over [`Project::add_machining`]: it builds
    /// [`MachiningParams::Cutout`] and a face anchor from the entry side. The same validation runs,
    /// so the shorthand cannot accept a cutout the generic command would reject.
    pub fn add_cutout(
        &mut self,
        panel: Id,
        shape: CutoutShape,
        depth: CutoutDepth,
        entry: FaceSide,
    ) -> Result<MachiningId, ProjectError> {
        self.add_machining(panel, MachiningAnchor::face(entry), MachiningParams::Cutout(Cutout::new(shape, depth)))
    }

    /// Checks whether one machining record can be added to a panel without changing the document.
    pub fn validate_machining(
        &self,
        panel: Id,
        anchor: MachiningAnchor,
        params: &MachiningParams,
    ) -> Result<(), ProjectError> {
        let panel = self.panel(panel).ok_or(ProjectError::PanelNotFound { id: panel })?;
        validate_machining_entry(panel, anchor, params, None)
    }

    /// Adds a machining record to a panel and bumps the revision once.
    ///
    /// The record id is allocated from the document; callers must not invent one. A cutout outside
    /// the panel, a blind cutout that is not shallower than the thickness, or an overlap with an
    /// existing cutout is rejected before any id is consumed.
    pub fn add_machining(
        &mut self,
        panel: Id,
        anchor: MachiningAnchor,
        params: MachiningParams,
    ) -> Result<MachiningId, ProjectError> {
        self.validate_machining(panel, anchor, &params)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let id = self.allocate_id()?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        let machining = MachiningId::from_id(id);
        panel_node.push_machining(Machining::new(machining, anchor, params));
        self.revision = next_revision;
        Ok(machining)
    }

    /// Checks whether replacing one machining record is valid without changing the document.
    pub fn validate_machining_update(
        &self,
        panel: Id,
        machining: MachiningId,
        anchor: MachiningAnchor,
        params: &MachiningParams,
    ) -> Result<(), ProjectError> {
        let panel_node = self.panel(panel).ok_or(ProjectError::PanelNotFound { id: panel })?;
        if panel_node.machining_at(machining).is_none() {
            return Err(ProjectError::MachiningNotFound { panel, machining });
        }
        validate_machining_entry(panel_node, anchor, params, Some(machining))
    }

    /// Replaces one machining record in place and bumps the revision once.
    ///
    /// The id and the position in the list are kept, so an edit never reorders the pipeline. The
    /// new anchor and parameters are validated as a whole before anything changes.
    pub fn update_machining(
        &mut self,
        panel: Id,
        machining: MachiningId,
        anchor: MachiningAnchor,
        params: MachiningParams,
    ) -> Result<Revision, ProjectError> {
        self.validate_machining_update(panel, machining, anchor, &params)?;
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        let Some((index, _existing)) = panel_node.machining_at(machining) else {
            return Err(ProjectError::MachiningNotFound { panel, machining });
        };
        panel_node.replace_machining_at(index, Machining::new(machining, anchor, params));
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes one machining record and bumps the revision once.
    ///
    /// Removing an operation cannot invalidate the remaining ones: their geometry and the core
    /// trace only lose a subtraction.
    pub fn remove_machining(&mut self, panel: Id, machining: MachiningId) -> Result<Revision, ProjectError> {
        let panel_node = self.panel(panel).ok_or(ProjectError::PanelNotFound { id: panel })?;
        let Some((index, _existing)) = panel_node.machining_at(machining) else {
            return Err(ProjectError::MachiningNotFound { panel, machining });
        };
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        panel_node.remove_machining_at(index);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Moves one machining record to a final list index and bumps the revision once.
    ///
    /// The index is the position after the move, in `0..len`. Ids never change, so a caller can
    /// keep referring to the record.
    pub fn move_machining(
        &mut self,
        panel: Id,
        machining: MachiningId,
        index: usize,
    ) -> Result<Revision, ProjectError> {
        let panel_node = self.panel(panel).ok_or(ProjectError::PanelNotFound { id: panel })?;
        let len = panel_node.machining().len();
        let Some((from, _existing)) = panel_node.machining_at(machining) else {
            return Err(ProjectError::MachiningNotFound { panel, machining });
        };
        if index >= len {
            return Err(ProjectError::MachiningIndexOutOfRange { panel, machining, index, len });
        }
        let next_revision = self.revision.checked_next().ok_or(ProjectError::RevisionExhausted)?;
        let Some((_depth, panel_node)) = find_panel_mut(&mut self.root, panel, 1) else {
            return Err(ProjectError::PanelNotFound { id: panel });
        };
        panel_node.move_machining_to(from, index);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Checks the schema version, nesting depth, global id uniqueness and the id allocator.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.schema_version != Self::CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: Self::CURRENT_SCHEMA_VERSION,
            });
        }

        let max_id = scan_tree(&self.root)?;
        if self.next_id.get() <= max_id {
            return Err(ProjectError::NextIdNotAfterMax { next: self.next_id, max: max_id });
        }
        Ok(())
    }

    /// Adds a panel to an existing container and bumps the revision once.
    ///
    /// Panels may live in any container, whether an ordinary group or a furniture component; a
    /// group is only organisational. The panel id is allocated from the document; callers must not
    /// invent one.
    pub fn add_panel(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        nominal_size: PanelNominalSize,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let panel = Panel::new(id, name, nominal_size, material, last_known_thickness, placement)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Panel(panel));
        self.revision = next_revision;
        Ok(id)
    }

    /// Adds a panel whose outline is a contour ring selected in the scene.
    ///
    /// The ring is rebased so its bounds base corner is the panel-local zero (ADR 0083): the ring
    /// moves by that corner and the placement becomes the parent-frame position of the corner, so
    /// no world point moves and the pivot starts at zero. A ring that already starts at zero is
    /// unchanged apart from rounding. The panel is created in one revision with the ring as its
    /// outline, so a rectangle panel never converts into a contour panel and the reverse is
    /// impossible too.
    pub fn add_panel_contour(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        contour: &Contour,
        material: Option<MaterialId>,
        last_known_thickness: Length,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let rebased = rebase_at_anchor(contour.clone(), placement, FrameAnchor::BASE_CORNER)
            .map_err(|_| ProjectError::CreationFrame { id })?;
        let panel = Panel::new_contour(id, name, rebased.content, material, last_known_thickness, rebased.placement)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Panel(panel));
        self.revision = next_revision;
        Ok(id)
    }
}

/// Validates edge banding on a panel.
pub(super) fn validate_panel_banding(panel: &Panel) -> Result<(), ProjectError> {
    let mut seen = HashSet::new();
    for band in panel.edge_banding() {
        if !matches!(band.edge().get(), 1..=4) {
            return Err(ProjectError::UnsupportedEdgeBanding { panel: panel.id(), edge: band.edge() });
        }
        if !seen.insert(band.edge().get()) {
            return Err(ProjectError::DuplicateEdgeBanding { panel: panel.id(), edge: band.edge() });
        }
    }
    Ok(())
}

fn validate_panel_machining(panel: &Panel, geometry: &PanelGeometry) -> Result<(), ProjectError> {
    for machining in panel.machining() {
        let Some(cutout) = machining.cutout() else {
            continue;
        };
        validate_cutout_geometry(panel.id(), geometry, cutout.shape(), cutout.depth())?;
    }
    validate_cutout_overlaps(panel.id(), panel.machining())?;
    validate_core(panel, geometry, None, None)
}

fn validate_machining_entry(
    panel: &Panel,
    anchor: MachiningAnchor,
    params: &MachiningParams,
    exclude: Option<MachiningId>,
) -> Result<(), ProjectError> {
    let geometry = panel.geometry_with_fallbacks()?;
    if !params.descriptor().capabilities().accepts_anchor(anchor) {
        return Err(ProjectError::MachiningAnchorUnsupported { panel: panel.id(), anchor });
    }
    match params {
        MachiningParams::Cutout(cutout) => {
            validate_cutout_geometry(panel.id(), &geometry, cutout.shape(), cutout.depth())?;
            for machining in panel.machining() {
                if exclude == Some(machining.id()) {
                    continue;
                }
                let Some(existing) = machining.cutout() else {
                    continue;
                };
                let both_through =
                    matches!(cutout.depth(), CutoutDepth::Through) && matches!(existing.depth(), CutoutDepth::Through);
                if !both_through && cutout_shapes_overlap(cutout.shape(), existing.shape())? {
                    return Err(ProjectError::CutoutOverlap { panel: panel.id(), other: machining.id() });
                }
            }
            let candidate = match cutout.depth() {
                CutoutDepth::Through => Some(cutout.shape().region()?),
                CutoutDepth::Blind { .. } => None,
            };
            validate_core(panel, &geometry, candidate.as_ref(), exclude)
        }
    }
}

fn validate_cutout_geometry(
    panel: Id,
    geometry: &PanelGeometry,
    shape: &CutoutShape,
    depth: CutoutDepth,
) -> Result<(), ProjectError> {
    let region = shape.region()?;
    match depth {
        CutoutDepth::Through => {
            if !geometry.cutout_reaches_core(&region)? {
                return Err(ProjectError::CutoutOutsidePanel { panel });
            }
        }
        CutoutDepth::Blind { depth } => {
            if !geometry.cutout_fits_core(&region)? {
                return Err(ProjectError::CutoutOutsidePanel { panel });
            }
            let thickness = geometry.thickness();
            if !depth.is_positive() || depth >= thickness {
                return Err(ProjectError::CutoutDepthInvalid { panel, depth, thickness });
            }
        }
    }
    Ok(())
}

fn validate_cutout_overlaps(panel: Id, machining: &[Machining]) -> Result<(), ProjectError> {
    let cutouts: Vec<(MachiningId, &Cutout)> =
        machining.iter().filter_map(|machining| machining.cutout().map(|cutout| (machining.id(), cutout))).collect();
    for (index, (_id, cutout)) in cutouts.iter().enumerate() {
        for (other_id, other) in &cutouts[..index] {
            let both_through =
                matches!(cutout.depth(), CutoutDepth::Through) && matches!(other.depth(), CutoutDepth::Through);
            if !both_through && cutout_shapes_overlap(cutout.shape(), other.shape())? {
                return Err(ProjectError::CutoutOverlap { panel, other: *other_id });
            }
        }
    }
    Ok(())
}

fn validate_core(
    panel: &Panel,
    geometry: &PanelGeometry,
    extra: Option<&Region>,
    exclude: Option<MachiningId>,
) -> Result<(), ProjectError> {
    match core_violation_of(panel, geometry, extra, exclude)? {
        Some(CoreViolation::Removed) => Err(ProjectError::CutoutRemovesCore { panel: panel.id() }),
        Some(CoreViolation::Split) => Err(ProjectError::CutoutSplitsCore { panel: panel.id() }),
        None => Ok(()),
    }
}

/// Whether two cutout outlines share area.
///
/// A rectangle pair uses the exact rectangle test. A contour takes part as a chord polygon sampled
/// at the self-intersection resolution, so a contour against a rectangle or another contour is
/// decided by edge contact and vertex containment. The chord-pair budget keeps a hostile pair from
/// buying unbounded work.
fn cutout_shapes_overlap(first: &CutoutShape, second: &CutoutShape) -> Result<bool, ProjectError> {
    if let (CutoutShape::Rectangle(a), CutoutShape::Rectangle(b)) = (first, second) {
        return Ok(rects_overlap(*a, *b));
    }
    let first = cutout_polygon(first)?;
    let second = cutout_polygon(second)?;
    polygons_overlap(&first, &second)
}

/// The outline as a closed chord polygon in whole micrometres.
fn cutout_polygon(shape: &CutoutShape) -> Result<Vec<[i64; 2]>, ProjectError> {
    match shape {
        CutoutShape::Rectangle(rect) => Ok(rect_polygon(*rect)),
        CutoutShape::Contour(contour) => {
            let points = contour.sampled_points(SELF_INTERSECTION_TOLERANCE_MICROMETRES)?;
            Ok(points.into_iter().map(|point| [point[0].micrometres(), point[1].micrometres()]).collect())
        }
    }
}

fn rect_polygon(rect: PanelRect) -> Vec<[i64; 2]> {
    let x0 = rect.x().micrometres();
    let y0 = rect.y().micrometres();
    let x1 = x0.saturating_add(rect.width().micrometres());
    let y1 = y0.saturating_add(rect.height().micrometres());
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn polygons_overlap(first: &[[i64; 2]], second: &[[i64; 2]]) -> Result<bool, ProjectError> {
    let first_count = first.len();
    let second_count = second.len();
    if first_count < 3 || second_count < 3 {
        return Ok(false);
    }
    if !SegBounds::of(first).overlaps(SegBounds::of(second)) {
        return Ok(false);
    }
    if first_count.saturating_mul(second_count) > MAX_SELF_INTERSECTION_PAIRS {
        return Err(ContourError::SamplingTooDetailed { max: MAX_SELF_INTERSECTION_PAIRS }.into());
    }
    for index in 0..first_count {
        let (Some(a), Some(b)) = (first.get(index), first.get((index + 1) % first_count)) else {
            continue;
        };
        for other in 0..second_count {
            let (Some(c), Some(d)) = (second.get(other), second.get((other + 1) % second_count)) else {
                continue;
            };
            if segments_touch(*a, *b, *c, *d) {
                return Ok(true);
            }
        }
    }
    Ok(first.iter().any(|point| point_in_polygon(*point, second))
        || second.iter().any(|point| point_in_polygon(*point, first)))
}

/// Whether a point lies inside a chord polygon, by the even-odd rule.
///
/// A point on the boundary is decided by the edge test the caller runs first; here only a point
/// strictly inside toggles the parity.
fn point_in_polygon(point: [i64; 2], polygon: &[[i64; 2]]) -> bool {
    let count = polygon.len();
    if count < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..count {
        let (Some(current), Some(previous)) = (polygon.get(index), polygon.get((index + count - 1) % count)) else {
            continue;
        };
        let (a, b) = (*previous, *current);
        if (a[1] > point[1]) == (b[1] > point[1]) {
            continue;
        }
        let dx = i128::from(b[0]) - i128::from(a[0]);
        let dy = i128::from(b[1]) - i128::from(a[1]);
        let cross = dx * (i128::from(point[1]) - i128::from(a[1])) - (i128::from(point[0]) - i128::from(a[0])) * dy;
        if (cross > 0) == (dy > 0) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    use furnicraft_geom::{DQuat, DVec3};

    #[test]
    fn a_new_panel_is_anchored_at_its_base_corner_in_a_nested_turned_parent() {
        let mut project = sample_project();
        let root = project.root().id();
        let parent_placement =
            Placement::new(DVec3::new(-320.0, 150.0, 700.0), DQuat::from_rotation_z(0.7)).expect("parent");
        let parent = project.add_component(root, "Frame", parent_placement).expect("container");

        let at_origin = project
            .add_panel(parent, "Back", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("panel at the parent origin");
        let panel = project.panel(at_origin).expect("panel");
        assert_eq!(panel.placement().translation(), DVec3::ZERO, "a new panel is at the parent origin");
        assert!(panel.pivot().is_zero(), "the base corner is the pivot");
        assert_eq!(
            project.pivot_world(at_origin).expect("world pivot"),
            parent_placement.translation(),
            "a zero local pivot is the parent origin"
        );

        let placement =
            Placement::new(DVec3::new(50.0, 20.0, 30.0), DQuat::from_rotation_z(-0.4)).expect("panel placement");
        let turned = project
            .add_panel(parent, "Top", sample_nominal(), None, sample_thickness(), placement)
            .expect("turned panel");
        let world = project.world_placement(turned).expect("world placement");
        let expected = Placement::compose(parent_placement, placement).expect("expected world");
        assert!(world.same_transform(expected));
        assert_eq!(
            project.pivot_world(turned).expect("world pivot"),
            world.transform_point(DVec3::ZERO),
            "a turned placement turns about the base corner, not the bounds centre"
        );
        assert_eq!(project.pivot(turned), Some(Pivot::ZERO));
    }

    #[test]
    fn clearing_an_unbanded_edge_is_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                    .expect("radius"),
            )
            .expect("corner feature");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("apply band");
        assert_eq!(
            project.clear_edge_banding(panel_id, RectEdge::Top.id()),
            Err(ProjectError::EdgeBandingNotFound { panel: panel_id, edge: RectEdge::Top.id() })
        );
        assert_eq!(project.panel(panel_id).expect("panel").edge_banding().len(), 1);
    }

    #[test]
    fn project_resizes_panels_and_reports_missing_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(4).expect("id");
        let nominal = PanelNominalSize::new(millimetres(900), millimetres(600)).expect("valid size");

        let revision = project.set_panel_nominal_size(panel_id, nominal).expect("resize panel");
        assert_eq!(revision.get(), 1);
        assert_eq!(project.revision(), revision);
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), nominal);

        let missing = Id::new(999).expect("id");
        assert_eq!(project.set_panel_nominal_size(missing, nominal), Err(ProjectError::PanelNotFound { id: missing }));
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn project_adds_cutouts_and_allocates_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));

        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        let panel = project.panel(panel_id).expect("panel exists");
        assert_eq!(panel.machining().len(), 1);
        let machining = &panel.machining()[0];
        assert_eq!(machining.id(), cutout_id);
        assert_eq!(machining.anchor().as_face(), Some(FaceSide::A));
        let cutout = machining.cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);
        assert!(project.validate().is_ok());

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn cutouts_reject_invalid_depth_bounds_and_overlap_without_side_effects() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: Length::ZERO }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid { panel: panel_id, depth: Length::ZERO, thickness: millimetres(18) })
        );
        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: millimetres(18) }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid {
                panel: panel_id,
                depth: millimetres(18),
                thickness: millimetres(18)
            })
        );
        let outside = CutoutShape::Rectangle(sample_rect(750, 10, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, outside, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let first = project
            .add_cutout(panel_id, shape, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::B)
            .expect("first cutout");
        assert_eq!(first.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);

        let overlapping = CutoutShape::Rectangle(sample_rect(50, 20, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: first })
        );
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        assert_eq!(project.panel(panel_id).expect("panel exists").machining().len(), 1);
    }

    #[test]
    fn overlapping_through_cutouts_and_boundary_notches_are_accepted() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let first = CutoutShape::Rectangle(sample_rect(100, 100, 100, 50));
        let second = CutoutShape::Rectangle(sample_rect(150, 120, 100, 50));
        // The 700 mm core ends at x = 700, so this cutout opens a notch in the right edge.
        let notch = CutoutShape::Rectangle(sample_rect(650, 10, 100, 50));

        project.add_cutout(panel_id, first, CutoutDepth::Through, FaceSide::A).expect("first through cutout");
        project.add_cutout(panel_id, second, CutoutDepth::Through, FaceSide::A).expect("overlapping through cutout");
        project.add_cutout(panel_id, notch, CutoutDepth::Through, FaceSide::A).expect("boundary notch");

        let panel = project.panel(panel_id).expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(&geometry), Ok(None));
        assert_eq!(panel.machining().len(), 3);
    }

    #[test]
    fn cutouts_that_split_or_empty_the_core_are_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let strip = CutoutShape::Rectangle(sample_rect(0, 200, 700, 50));
        assert_eq!(
            project.add_cutout(panel_id, strip, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutSplitsCore { panel: panel_id })
        );

        let whole = CutoutShape::Rectangle(sample_rect(0, 0, 700, 500));
        assert_eq!(
            project.add_cutout(panel_id, whole, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutRemovesCore { panel: panel_id })
        );
        assert!(project.panel(panel_id).expect("panel").machining().is_empty());
    }

    #[test]
    fn panel_resize_validates_existing_cutouts() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(600, 10, 50, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");

        let smaller = PanelNominalSize::new(millimetres(500), millimetres(500)).expect("valid size");
        assert_eq!(
            project.validate_panel_resize(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(
            project.set_panel_nominal_size(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), sample_nominal());
        assert_eq!(project.revision().get(), 1);

        let blind_rect = CutoutShape::Rectangle(sample_rect(10, 10, 50, 50));
        let other_panel = Id::new(4).expect("id");
        project
            .add_cutout(other_panel, blind_rect, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::A)
            .expect("blind cutout");
        let material = MaterialId::new(1).expect("material id");
        let revision = project.set_panel_material(other_panel, material, millimetres(5)).expect("material change");
        assert_eq!(revision.get(), 3);
        let panel = project.panel(other_panel).expect("panel exists");
        assert_eq!(panel.material(), Some(material));
        assert_eq!(panel.last_known_thickness(), millimetres(5));
        assert_eq!(panel.geometry(millimetres(5)).expect("resolved geometry").thickness(), millimetres(5));
    }

    #[test]
    fn cutout_ids_participate_in_next_id_validation() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let text = serde_json::to_string(&project).expect("serialize project");
        let invalid = text.replacen("\"next_id\":6", &format!("\"next_id\":{}", cutout_id.get()), 1);
        let error = serde_json::from_str::<Project>(&invalid).expect_err("next id must exceed cutout id");
        assert!(error.to_string().contains("must be greater"), "error was {error}");
    }

    #[test]
    fn cutout_coordinates_are_nominal_and_do_not_move_with_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        for edge in [1, 4] {
            let band =
                EdgeBanding::new(EdgeId::new(edge).expect("edge id"), material, true, millimetres(2), millimetres(22))
                    .expect("valid banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        let geometry = project.panel(panel_id).expect("panel").geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));

        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 20, 20));
        project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let panel = project.panel(panel_id).expect("panel");
        let cutout = panel.machining()[0].cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &shape);

        // Nominal coordinates: the 2 mm included bands put the core origin at (2, 2), so this
        // cutout lies entirely in the band and would remove no core material.
        let under_the_band = CutoutShape::Rectangle(sample_rect(0, 0, 1, 1));
        assert_eq!(
            project.add_cutout(panel_id, under_the_band, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn contour_cutout_is_stored_and_round_trips() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Contour(ring(100, 100, 100, 60));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add contour cutout");
        let panel = project.panel(panel_id).expect("panel");
        let machining = &panel.machining()[0];
        assert_eq!(machining.id(), cutout_id);
        let cutout = machining.cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
        let panel = decoded.panel(panel_id).expect("reloaded panel");
        let cutout = panel.machining()[0].cutout().expect("a reloaded cutout record");
        assert_eq!(cutout.shape(), &shape);
    }

    #[test]
    fn through_contour_cutout_may_open_a_notch_and_blind_stays_inside() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");

        // A through ring is traced against the core exactly, so it may touch or cross the boundary
        // and open a notch; the core trace decides whether the remaining material is still one
        // positive component.
        for shape in [ring(-10, 100, 120, 60), ring(0, 100, 100, 60), ring(690, 100, 20, 60)] {
            project
                .add_cutout(panel_id, CutoutShape::Contour(shape), CutoutDepth::Through, FaceSide::A)
                .expect("through ring that opens a notch");
        }

        // A through ring that misses the core removes nothing and is rejected.
        let next_id = project.next_id();
        let revision = project.revision();
        assert_eq!(
            project.add_cutout(
                panel_id,
                CutoutShape::Contour(ring(-200, 100, 100, 60)),
                CutoutDepth::Through,
                FaceSide::A
            ),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        // A blind ring may open at the edge like a blind rectangle, but it may not cross it.
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(0, 300, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("blind contour cutout at the edge");
        assert_eq!(
            project.add_cutout(
                panel_id,
                CutoutShape::Contour(ring(-10, 300, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::B,
            ),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn contour_cutout_overlap_follows_the_ring_not_its_bounds() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let blind = CutoutDepth::Blind { depth: millimetres(8) };
        project
            .add_cutout(panel_id, CutoutShape::Rectangle(sample_rect(100, 100, 50, 50)), blind, FaceSide::A)
            .expect("rect cutout");

        let overlapping = CutoutShape::Contour(ring(120, 120, 100, 60));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: MachiningId::new(5).expect("id") })
        );

        // A C-shaped ring around the rectangle's bounding box leaves the notch free.
        let c_shape = Contour::from_polygon(vec![
            [millimetres(50), millimetres(50)],
            [millimetres(250), millimetres(50)],
            [millimetres(250), millimetres(90)],
            [millimetres(90), millimetres(90)],
            [millimetres(90), millimetres(210)],
            [millimetres(250), millimetres(210)],
            [millimetres(250), millimetres(250)],
            [millimetres(50), millimetres(250)],
        ])
        .expect("C-shaped ring");
        project
            .add_cutout(panel_id, CutoutShape::Contour(c_shape), blind, FaceSide::A)
            .expect("a ring that surrounds the bounds but not the rectangle");

        // Two contours are compared the same way: a crossing ring is an overlap, a nested one is
        // not (a through pair may overlap, so use blind here).
        let crossing = CutoutShape::Contour(ring(200, 220, 100, 60));
        assert!(matches!(
            project.add_cutout(panel_id, crossing, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel, .. }) if panel == panel_id
        ));
        let nested = CutoutShape::Contour(ring(160, 120, 20, 20));
        project.add_cutout(panel_id, nested, blind, FaceSide::A).expect("a ring inside the C notch");
    }

    #[test]
    fn contour_cutout_is_refused_when_a_resize_moves_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(100, 100, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("contour cutout");
        let small = PanelNominalSize::new(millimetres(150), millimetres(500)).expect("small size");
        assert_eq!(
            project.validate_panel_resize(panel_id, small),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn machining_records_add_update_move_and_remove_in_one_ordered_list() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let first = project
            .add_machining(
                panel_id,
                MachiningAnchor::face(FaceSide::A),
                MachiningParams::Cutout(Cutout::new(
                    CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)),
                    CutoutDepth::Through,
                )),
            )
            .expect("first record");
        let second = project
            .add_cutout(
                panel_id,
                CutoutShape::Rectangle(sample_rect(200, 200, 100, 50)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::B,
            )
            .expect("second record");
        assert_eq!(project.revision().get(), 2);

        let changed = CutoutShape::Rectangle(sample_rect(20, 20, 80, 40));
        let revision = project
            .update_machining(
                panel_id,
                first,
                MachiningAnchor::face(FaceSide::B),
                MachiningParams::Cutout(Cutout::new(changed.clone(), CutoutDepth::Blind { depth: millimetres(5) })),
            )
            .expect("update record");
        assert_eq!(revision.get(), 3);
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.machining().iter().map(|entry| entry.id()).collect::<Vec<_>>(), vec![first, second]);
        let updated = panel.machining_by_id(first).expect("updated record");
        assert_eq!(updated.anchor().as_face(), Some(FaceSide::B));
        assert_eq!(updated.cutout().expect("cutout").shape(), &changed);
        assert_eq!(updated.cutout().expect("cutout").depth(), CutoutDepth::Blind { depth: millimetres(5) });

        let revision = project.move_machining(panel_id, second, 0).expect("move record");
        assert_eq!(revision.get(), 4);
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.machining().iter().map(|entry| entry.id()).collect::<Vec<_>>(), vec![second, first]);

        let revision = project.remove_machining(panel_id, first).expect("remove record");
        assert_eq!(revision.get(), 5);
        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(panel.machining().len(), 1);
        assert_eq!(panel.machining()[0].id(), second);
        assert!(project.validate().is_ok());
    }

    #[test]
    fn machining_commands_reject_bad_anchors_ids_and_indices_without_side_effects() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let anchor = MachiningAnchor::edge(EdgeRef::Rect { edge: RectEdge::Bottom.id() });
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let next_id = project.next_id();
        let revision = project.revision();
        assert_eq!(
            project.add_machining(
                panel_id,
                anchor,
                MachiningParams::Cutout(Cutout::new(shape.clone(), CutoutDepth::Through)),
            ),
            Err(ProjectError::MachiningAnchorUnsupported { panel: panel_id, anchor })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let missing = MachiningId::new(99).expect("id");
        let params = MachiningParams::Cutout(Cutout::new(shape.clone(), CutoutDepth::Through));
        assert_eq!(
            project.update_machining(panel_id, missing, MachiningAnchor::face(FaceSide::A), params.clone()),
            Err(ProjectError::MachiningNotFound { panel: panel_id, machining: missing })
        );
        assert_eq!(
            project.remove_machining(panel_id, missing),
            Err(ProjectError::MachiningNotFound { panel: panel_id, machining: missing })
        );
        assert_eq!(
            project.move_machining(panel_id, missing, 0),
            Err(ProjectError::MachiningNotFound { panel: panel_id, machining: missing })
        );

        let record = project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("record");
        assert_eq!(
            project.move_machining(panel_id, record, 1),
            Err(ProjectError::MachiningIndexOutOfRange { panel: panel_id, machining: record, index: 1, len: 1 })
        );
        assert_eq!(project.panel(panel_id).expect("panel").machining().len(), 1);
    }

    #[test]
    fn a_machining_update_validates_the_replaced_record() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let blind = CutoutDepth::Blind { depth: millimetres(8) };
        project
            .add_cutout(panel_id, CutoutShape::Rectangle(sample_rect(10, 10, 100, 50)), blind, FaceSide::A)
            .expect("first cutout");
        let second = project
            .add_cutout(panel_id, CutoutShape::Rectangle(sample_rect(200, 200, 100, 50)), blind, FaceSide::A)
            .expect("second cutout");
        let revision = project.revision();

        // Moving the second cutout onto the first would overlap it; an update replaces the old
        // geometry, so the check reads the candidate list, not the old record.
        assert_eq!(
            project.update_machining(
                panel_id,
                second,
                MachiningAnchor::face(FaceSide::A),
                MachiningParams::Cutout(Cutout::new(CutoutShape::Rectangle(sample_rect(20, 20, 100, 50)), blind,)),
            ),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: MachiningId::new(5).expect("id") })
        );
        assert_eq!(project.revision(), revision);
        let panel = project.panel(panel_id).expect("panel");
        let unchanged = panel.machining_by_id(second).expect("second cutout");
        assert_eq!(
            unchanged.cutout().expect("cutout").shape(),
            &CutoutShape::Rectangle(sample_rect(200, 200, 100, 50))
        );
    }

    #[test]
    fn converting_a_rectangle_bakes_one_ring_in_one_revision() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner = RectCorner::TopRight.id();
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(corner, CornerFeatureKind::Radius { radius: millimetres(40) }).expect("radius"),
            )
            .expect("corner");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("edge band");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, sample_material(), false, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("corner band");

        let revision = project.revision();
        let next = project.convert_panel_to_contour(panel_id).expect("convert");
        assert_eq!(next, revision.checked_next().expect("revision"));
        assert_eq!(project.revision(), next);

        let panel = project.panel(panel_id).expect("panel");
        assert!(panel.is_contour());
        assert_eq!(panel.nominal_size(), sample_nominal());
        assert_eq!(panel.outline_contour().expect("ring").bounds_micrometres(), ([0, 0], [700_000, 500_000]));
        assert!(panel.edge_banding().is_empty());
        assert!(panel.corner_banding().is_empty());
        assert_eq!(panel.ring_banding().len(), 2);
        assert_eq!(panel.band_order().len(), 2);
        let ring = panel.outline_contour().expect("ring");
        for band in panel.ring_banding() {
            assert!(ring.segment(band.segment()).is_some(), "band {band:?} names a missing ring segment");
        }
        assert!(panel.geometry_with_fallbacks().is_ok());

        let stored = project.revision();
        assert_eq!(
            project.convert_panel_to_contour(panel_id),
            Err(ProjectError::PanelContour(PanelContourError::AlreadyContour))
        );
        assert_eq!(project.revision(), stored, "a refused conversion changes nothing");
    }

    #[test]
    fn an_outward_bow_moves_the_frame_without_moving_the_world_shape() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .set_edge_bow(panel_id, EdgeBow::new(RectEdge::Bottom.id(), millimetres(10)).expect("bow"))
            .expect("bow");
        let before =
            project.panel(panel_id).expect("panel").nominal_contour().expect("outline").bounds().expect("bounds");
        assert!((before.0.y + 10.0).abs() < 1e-9, "outward bow lowers the bottom: {before:?}");

        project.convert_panel_to_contour(panel_id).expect("convert");
        let panel = project.panel(panel_id).expect("panel");
        let (min, max) = panel.outline_contour().expect("ring").bounds_micrometres();
        assert_eq!(min, [0, 0]);
        assert_eq!(max, [700_000, 510_000]);
        let translation = panel.placement().translation();
        assert!(translation.x.abs() < 1e-9, "the frame does not move horizontally: {}", translation.x);
        assert!((translation.y + 10.0).abs() < 1e-9, "the frame moves with the anchored ring: {}", translation.y);
        let world_min_y = min[1] as f64 / 1000.0 + translation.y;
        let world_max_y = max[1] as f64 / 1000.0 + translation.y;
        assert!((world_min_y - before.0.y).abs() < 1e-9, "world bottom {world_min_y}");
        assert!((world_max_y - before.1.y).abs() < 1e-9, "world top {world_max_y}");
    }
}
