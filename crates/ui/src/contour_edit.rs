//! Contour edit state: the working copy of a panel outline, an object contour, a path chain or a
//! cutout ring.
//!
//! This is view state. Entering a contour edit reads the stored ring or chain into a draft; moving a
//! corner, dragging a grip or splitting a side only changes the draft, so the document, its revision
//! and the undo history stay untouched until the user applies. Applying dispatches exactly one
//! command (`panel.outline.set`, `object.contour.replace`, `object.path.replace`, or
//! `machining.update` for a cutout ring), so the whole edit is one undo entry. Leaving without
//! applying drops the draft and changes nothing.
//!
//! A ring addresses a corner by the segment that starts on it, a chain by the corner the path names;
//! both take the same two corner modes (ADR 0075), and a figure keeps the constraint its tag gives
//! it. A collapse is a segment dragged to zero length: the draft keeps a legal sliver so the ring
//! stays valid, a ring of three refuses to lose a side, and the removal happens on apply. A path is
//! stored as the chain it is, not as a ring, so an open edit stays open (ADR 0040, 0073).
//!
//! It refuses an edit on a target that no longer stores a ring or chain, a corner that names no
//! segment and a collapse that would leave a ring below three segments; the working copy stays as
//! it was, so the user can retry or cancel.
//!
//! It does not build a new figure: the drawing tools and their draft live in `draw`.
//!
//! Changes when: the corner, grip, split or collapse rule of an existing contour changes.

use furnicraft_engine::object_cmds::{OBJECT_CONTOUR_REPLACE_ID, OBJECT_PATH_REPLACE_ID};
use furnicraft_engine::panel_cmds::{MACHINING_UPDATE_ID, OUTLINE_SET_ID};
use furnicraft_engine::{ObjectContourReplace, ObjectPathReplace, PanelMachiningUpdate, PanelOutlineSet, Session};
use furnicraft_model::{
    Contour, ContourEditError, ContourSegment, ContourSegmentEntry, ContourSegmentId, CornerMove, Cutout, CutoutShape,
    Id, MachiningId, MachiningParams, Path, PathCorner, Project,
};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::contour_grips::{ContourGrip, GripValue};

/// The node whose contour the editor is changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourTarget {
    /// The outline ring of a contour panel.
    Panel(Id),
    /// The contour of a constructive object.
    Object(Id),
    /// The open or closed chain of a path object.
    Path(Id),
    /// The ring of a contour cutout on a panel.
    ///
    /// The panel is also the node whose placement frames the ring, because machining coordinates are
    /// nominal panel-local.
    Cutout {
        /// The panel that owns the cutout.
        panel: Id,
        /// The stable machining record the ring belongs to.
        machining: MachiningId,
    },
}

impl ContourTarget {
    /// The node that owns the edited contour.
    pub const fn id(self) -> Id {
        match self {
            Self::Panel(id) | Self::Object(id) | Self::Path(id) => id,
            Self::Cutout { panel, .. } => panel,
        }
    }

    /// The ring of a contour cutout, when this target names one.
    fn cutout_ring(self, project: &Project) -> Option<&Contour> {
        let Self::Cutout { panel, machining } = self else {
            return None;
        };
        let record = project.panel(panel)?.machining_by_id(machining)?;
        match record.cutout()?.shape() {
            CutoutShape::Contour(contour) => Some(contour),
            CutoutShape::Rectangle(_) => None,
        }
    }

    /// The draft this target stores, or `None` when the node is gone or has none.
    pub fn draft(self, project: &Project) -> Option<ContourDraft> {
        match self {
            Self::Panel(id) => project.panel(id)?.outline_contour().cloned().map(ContourDraft::Ring),
            Self::Object(id) => project.object_contour(id).ok().map(ContourDraft::Ring),
            Self::Path(id) => project.object_path(id).ok().cloned().map(ContourDraft::Chain),
            Self::Cutout { .. } => self.cutout_ring(project).cloned().map(ContourDraft::Ring),
        }
    }

    /// Whether the target still stores a draft this mode can edit.
    pub fn is_editable(self, project: &Project) -> bool {
        match self {
            Self::Panel(id) => project.panel(id).and_then(|panel| panel.outline_contour()).is_some(),
            Self::Object(id) => project.object_contour(id).is_ok(),
            Self::Path(id) => project.object_path(id).is_ok(),
            Self::Cutout { .. } => self.cutout_ring(project).is_some(),
        }
    }
}

/// What a card entry into the contour edit mode can do.
///
/// The editor owns one live edit at a time, so a card section must know whether it is the live
/// target, whether another target owns the viewport, or whether the selection rule blocks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContourEntry {
    /// This target is the live edit.
    Editing,
    /// Another contour target owns the viewport.
    Busy,
    /// The selection is not exactly the target's node.
    SelectSingle,
    /// The entry button can start the edit.
    Ready,
}

impl ContourEntry {
    /// The entry state for `mine`, given the live target and the selection rule.
    pub(crate) fn of(target: Option<ContourTarget>, mine: ContourTarget, single_selection: bool) -> Self {
        match target {
            Some(current) if current == mine => Self::Editing,
            Some(_) => Self::Busy,
            None if !single_selection => Self::SelectSingle,
            None => Self::Ready,
        }
    }
}

/// The working copy of a contour edit: a closed ring or a path chain.
///
/// A panel outline and a constructive contour are rings; a path is a chain that may be open. Both
/// store [`ContourSegmentEntry`] values under the same stable ids, so an edit addresses a segment
/// either way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContourDraft {
    /// A closed ring: a panel outline or a constructive contour.
    Ring(Contour),
    /// An open or closed chain: a path object.
    Chain(Path),
}

impl ContourDraft {
    /// The chain elements with their ids, in travel order.
    pub fn entries(&self) -> &[ContourSegmentEntry] {
        match self {
            Self::Ring(contour) => contour.entries(),
            Self::Chain(path) => path.entries(),
        }
    }

    /// The segment with this id.
    pub fn segment(&self, id: ContourSegmentId) -> Option<ContourSegment> {
        match self {
            Self::Ring(contour) => contour.segment(id),
            Self::Chain(path) => path.segment(id),
        }
    }

    /// The position of a segment id in travel order.
    pub fn index_of(&self, id: ContourSegmentId) -> Option<usize> {
        match self {
            Self::Ring(contour) => contour.index_of(id),
            Self::Chain(path) => path.index_of(id),
        }
    }

    /// The id of the segment at this position in travel order.
    pub fn id_at(&self, index: usize) -> Option<ContourSegmentId> {
        match self {
            Self::Ring(contour) => contour.id_at(index),
            Self::Chain(path) => path.id_at(index),
        }
    }

    /// The number of segments in the draft.
    pub fn segment_count(&self) -> usize {
        self.entries().len()
    }

    /// Whether the draft is a closed chain.
    pub const fn is_closed(&self) -> bool {
        match self {
            Self::Ring(_) => true,
            Self::Chain(path) => path.is_closed(),
        }
    }

    /// The ring, when the draft is one.
    pub const fn as_ring(&self) -> Option<&Contour> {
        match self {
            Self::Ring(contour) => Some(contour),
            Self::Chain(_) => None,
        }
    }

    /// The chain, when the draft is one.
    pub const fn as_chain(&self) -> Option<&Path> {
        match self {
            Self::Ring(_) => None,
            Self::Chain(path) => Some(path),
        }
    }
}

/// One live contour edit: the target, the draft it started from and the working copy.
///
/// The base draft lets the viewport show what changed and lets [`ContourEdit::reset`] throw the
/// working copy away without leaving the mode. A corner or a split only ever edits the working copy.
#[derive(Clone, Debug)]
pub struct ContourEdit {
    target: ContourTarget,
    base: ContourDraft,
    draft: ContourDraft,
    /// The segment an along-contour drag collapsed to zero length, removed on Apply.
    collapse: Option<ContourSegmentId>,
}

impl ContourEdit {
    /// Starts editing `target`, or returns `None` when it has no ring or chain.
    pub fn start(project: &Project, target: ContourTarget) -> Option<Self> {
        let base = target.draft(project)?;
        Some(Self { target, base: base.clone(), draft: base, collapse: None })
    }

    /// Whether the target still has a ring or chain to edit.
    pub fn is_editable(project: &Project, target: ContourTarget) -> bool {
        target.is_editable(project)
    }

    /// The node being edited.
    pub const fn target(&self) -> ContourTarget {
        self.target
    }

    /// The draft the edit started from.
    pub const fn base(&self) -> &ContourDraft {
        &self.base
    }

    /// The working copy the viewport shows.
    pub const fn draft(&self) -> &ContourDraft {
        &self.draft
    }

    /// Whether the working copy differs from the stored draft.
    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    /// Throws the working copy away and starts again from the stored draft.
    pub fn reset(&mut self) {
        self.draft = self.base.clone();
        self.collapse = None;
    }

    /// Records that an along-contour drag collapsed `segment` to zero length.
    ///
    /// The segment is removed when the edit is applied; until then the draft keeps a legal sliver
    /// so the ring or chain stays valid and the user can drag the corner back.
    pub fn record_collapse(&mut self, segment: ContourSegmentId) {
        self.collapse = Some(segment);
    }

    /// Clears a recorded collapse, for example when the corner is dragged back off the far end.
    pub fn clear_collapse(&mut self) {
        self.collapse = None;
    }

    /// The segment a collapse is waiting to remove, if any.
    pub const fn collapse(&self) -> Option<ContourSegmentId> {
        self.collapse
    }

    /// The draft with the collapsed segment removed, or an error when that would leave too few.
    fn draft_with_collapse(&self) -> Result<ContourDraft, String> {
        let Some(segment) = self.collapse else {
            return Ok(self.draft.clone());
        };
        match &self.draft {
            ContourDraft::Ring(contour) => {
                if contour.entries().len() <= 3 {
                    return Err("a ring keeps at least three segments".to_owned());
                }
                let removed = contour.remove_segment(segment).map_err(|error| error.to_string())?;
                Ok(ContourDraft::Ring(removed))
            }
            ContourDraft::Chain(path) => {
                let removed = path.remove_segment(segment).map_err(|error| error.to_string())?;
                Ok(ContourDraft::Chain(removed))
            }
        }
    }

    /// Moves the corner at the start of `segment` to `point` in the target's local plane.
    pub fn move_corner(&mut self, segment: ContourSegmentId, point: [Length; 2]) -> Result<(), ContourEditError> {
        self.move_corner_at(PathCorner::Start(segment), point, CornerMove::Free)
    }

    /// Moves the corner named by `corner` to `point` in the target's local plane, following `motion`.
    ///
    /// A ring addresses a corner by the segment that starts on it, so an end names the next
    /// segment's start; a chain uses the corner the path edit names. A ring and a chain take the
    /// same two modes, so a grip behaves the same on a panel outline and on a drawn figure.
    pub fn move_corner_at(
        &mut self,
        corner: PathCorner,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        match &mut self.draft {
            ContourDraft::Ring(contour) => {
                let segment = ring_corner_segment(contour, corner)?;
                let moved = contour.move_corner_with(segment, point, motion)?;
                *contour = moved;
            }
            ContourDraft::Chain(path) => {
                let moved = path.move_corner_at(corner, point, motion)?;
                *path = moved;
            }
        }
        Ok(())
    }

    /// Every grip the working copy shows, in painting order.
    pub fn grips(&self) -> Vec<ContourGrip> {
        self.draft.grips()
    }

    /// Drags `grip` to `point` in the target's local plane.
    ///
    /// A tagged figure keeps its own constraint, so a circle quadrant scales the circle and a
    /// rectangle midpoint moves one side; a plain chain takes [`CornerMove`] like any corner.
    pub fn drag_grip(
        &mut self,
        grip: ContourGrip,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        let mut draft = self.draft.clone();
        draft.drag_grip(grip, point, motion)?;
        self.draft = draft;
        Ok(())
    }

    /// The value a numeric field starts with at `grip`, when the grip takes a number.
    pub fn grip_value(&self, grip: ContourGrip) -> Option<GripValue> {
        self.draft.grip_value(grip)
    }

    /// Applies a number typed at `grip`.
    ///
    /// The working copy is replaced only when the value builds a legal figure, so a refused number
    /// leaves the draft as it was and the field can stay open.
    pub fn set_grip_value(&mut self, grip: ContourGrip, value: GripValue) -> Result<(), ContourEditError> {
        let mut draft = self.draft.clone();
        draft.set_grip_value(grip, value)?;
        self.draft = draft;
        Ok(())
    }

    /// Splits `segment` at `point` and returns the id of the new part.
    pub fn split_segment(
        &mut self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<ContourSegmentId, ContourEditError> {
        match &mut self.draft {
            ContourDraft::Ring(contour) => {
                let (split, inserted) = contour.split_segment(segment, point)?;
                *contour = split;
                Ok(inserted)
            }
            ContourDraft::Chain(path) => {
                let (split, inserted) = path.split_segment_at(segment, point)?;
                *path = split;
                Ok(inserted)
            }
        }
    }

    /// Applies the working copy as one command.
    ///
    /// The caller owns the mode and drops it only on success; a rejected edit leaves the draft in
    /// place so the user can fix it or cancel.
    pub fn apply(&self, session: &mut Session) -> Result<(), String> {
        let draft = self.draft_with_collapse()?;
        match (self.target, &draft) {
            (ContourTarget::Panel(panel), ContourDraft::Ring(contour)) => {
                let params = json(&PanelOutlineSet { panel, contour: contour.clone() })?;
                dispatch(session, OUTLINE_SET_ID, &params).map(|_| ())
            }
            (ContourTarget::Object(object), ContourDraft::Ring(contour)) => {
                let params = json(&ObjectContourReplace { object, path: contour.clone().into() })?;
                dispatch(session, OBJECT_CONTOUR_REPLACE_ID, &params).map(|_| ())
            }
            (ContourTarget::Path(object), ContourDraft::Chain(path)) => {
                let params = json(&ObjectPathReplace { object, path: path.clone() })?;
                dispatch(session, OBJECT_PATH_REPLACE_ID, &params).map(|_| ())
            }
            (ContourTarget::Cutout { panel, machining }, ContourDraft::Ring(contour)) => {
                let record = session
                    .project()
                    .panel(panel)
                    .and_then(|panel| panel.machining_by_id(machining))
                    .ok_or_else(|| format!("machining {machining} is gone"))?;
                let cutout = record.cutout().ok_or_else(|| format!("machining {machining} is not a cutout"))?;
                let params =
                    MachiningParams::Cutout(Cutout::new(CutoutShape::Contour(contour.clone()), cutout.depth()));
                let request = PanelMachiningUpdate { panel, machining, anchor: record.anchor(), params };
                let params = json(&request)?;
                dispatch(session, MACHINING_UPDATE_ID, &params).map(|_| ())
            }
            _ => Err("the contour draft does not match its target".to_owned()),
        }
    }
}

/// The segment whose start names the same corner as `corner` on a closed ring.
fn ring_corner_segment(contour: &Contour, corner: PathCorner) -> Result<ContourSegmentId, ContourEditError> {
    let segment = corner.segment();
    if !corner.is_end() {
        return Ok(segment);
    }
    let index = contour.index_of(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
    let count = contour.entries().len();
    if count == 0 {
        return Err(ContourEditError::UnknownSegment { segment });
    }
    contour.id_at((index + 1) % count).ok_or(ContourEditError::UnknownSegment { segment })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::NodeDelete;
    use furnicraft_engine::PanelMachiningAdd;
    use furnicraft_engine::node_cmds::NODE_DELETE_ID;
    use furnicraft_engine::panel_cmds::MACHINING_ADD_ID;
    use furnicraft_model::{
        Constructive, Container, CutoutDepth, FaceSide, Geometry, MachiningAnchor, Panel, Placement, Project,
    };
    use furnicraft_testkit::{millimetres, open_path, sample_catalog, sample_panel_thickness, sample_project};

    use super::*;

    fn rectangle(width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(width), millimetres(0)],
            [millimetres(width), millimetres(height)],
            [millimetres(0), millimetres(height)],
        ])
        .expect("rectangle")
    }

    /// A project whose root holds one contour panel.
    fn panel_project() -> (Project, Id) {
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Shelf",
            rectangle(700, 500),
            None,
            sample_panel_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        (Project::new(root).expect("project"), Id::new(2).expect("id"))
    }

    /// A project whose root holds one constructive object with a rectangular contour.
    fn object_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let constructive =
            Constructive::extrude(rectangle(700, 500).into(), sample_panel_thickness()).expect("constructive object");
        let id = project
            .add_object(root, "Counter", Placement::IDENTITY, Geometry::Constructive(constructive))
            .expect("object");
        (project, id)
    }

    /// A rectangle inside the sample panel, with its minimum corner at `(x, y)`.
    fn offset_rectangle(x: i64, y: i64, width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(x), millimetres(y)],
            [millimetres(x + width), millimetres(y)],
            [millimetres(x + width), millimetres(y + height)],
            [millimetres(x), millimetres(y + height)],
        ])
        .expect("offset rectangle")
    }

    /// A session whose sample panel owns one blind contour cutout, anchored on face B.
    fn cutout_session() -> (Session, Id, MachiningId) {
        let mut session = session(sample_project());
        let panel = Id::new(2).expect("panel id");
        let request = PanelMachiningAdd {
            panel,
            anchor: MachiningAnchor::face(FaceSide::B),
            params: MachiningParams::Cutout(Cutout::new(
                CutoutShape::Contour(offset_rectangle(100, 100, 200, 100)),
                CutoutDepth::Blind { depth: millimetres(6) },
            )),
        };
        let params = json(&request).expect("params");
        let outcome = session.dispatch(MACHINING_ADD_ID, &params).expect("add the contour cutout");
        let machining = outcome.created_machining_id().expect("the command created a record");
        (session, panel, machining)
    }

    /// A project whose root holds one path object.
    fn path_project(path: Path) -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let id = project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("path object");
        (project, id)
    }

    /// A 600 × 400 mm three-point chain, open at both ends.
    fn sketch_path() -> Path {
        open_path(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(600), millimetres(0)],
            [millimetres(600), millimetres(400)],
        ])
    }

    fn session(project: Project) -> Session {
        Session::with_catalog(project, sample_catalog())
    }

    #[test]
    fn a_panel_edit_works_on_a_copy_and_applies_in_one_command() {
        let (project, panel) = panel_project();
        let mut session = session(project);
        let target = ContourTarget::Panel(panel);
        let mut edit = ContourEdit::start(session.project(), target).expect("a contour panel is editable");
        assert_eq!(edit.target(), target);
        assert!(!edit.is_dirty());

        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(800), millimetres(0)]).expect("move the corner");
        assert!(edit.is_dirty());
        assert_eq!(edit.draft().as_ring().expect("ring").bounds_micrometres().1[0], millimetres(800).micrometres());
        assert_eq!(
            session.project().panel(panel).expect("panel").outline_contour(),
            edit.base().as_ring(),
            "the document still has the stored ring"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the outline");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), edit.draft().as_ring());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the ring");
        assert_eq!(session.project().panel(panel).expect("panel").outline_contour(), edit.base().as_ring());
    }

    #[test]
    fn reset_and_split_edit_only_the_draft() {
        let (project, panel) = panel_project();
        let mut edit = ContourEdit::start(&project, ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        let inserted = edit.split_segment(segment, [millimetres(350), millimetres(0)]).expect("split");
        assert_eq!(edit.draft().segment_count(), 5);
        assert!(edit.draft().index_of(inserted).is_some());
        assert!(edit.is_dirty());

        edit.reset();
        assert!(!edit.is_dirty());
        assert_eq!(edit.draft(), edit.base());
        assert_eq!(edit.draft().segment_count(), 4);
    }

    #[test]
    fn an_object_edit_replaces_the_object_contour() {
        let (project, object) = object_project();
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Object(object)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        edit.move_corner(segment, [millimetres(-100), millimetres(0)]).expect("move the corner");
        edit.apply(&mut session).expect("apply the contour");
        assert_eq!(
            session.project().object_contour(object).expect("contour"),
            edit.draft().as_ring().expect("ring").clone(),
            "an object contour is stored exactly as edited"
        );
    }

    #[test]
    fn a_rectangle_panel_has_no_contour_to_edit() {
        let project = sample_project();
        let rectangle_panel = Id::new(2).expect("id");
        assert!(ContourEdit::start(&project, ContourTarget::Panel(rectangle_panel)).is_none());
        assert!(!ContourEdit::is_editable(&project, ContourTarget::Panel(rectangle_panel)));
        assert!(ContourEdit::start(&project, ContourTarget::Object(Id::new(99).expect("id"))).is_none());
    }

    #[test]
    fn a_contour_entry_knows_the_live_edit_and_the_selection_rule() {
        let panel = Id::new(2).expect("id");
        let machining = MachiningId::new(10).expect("id");
        let mine = ContourTarget::Cutout { panel, machining };
        assert_eq!(ContourEntry::of(None, mine, true), ContourEntry::Ready);
        assert_eq!(ContourEntry::of(None, mine, false), ContourEntry::SelectSingle);
        assert_eq!(ContourEntry::of(Some(mine), mine, false), ContourEntry::Editing);
        assert_eq!(
            ContourEntry::of(Some(ContourTarget::Panel(panel)), mine, true),
            ContourEntry::Busy,
            "another target owns the viewport"
        );
    }

    #[test]
    fn a_cutout_ring_edit_is_one_revision_and_one_undo() {
        let (mut session, panel, machining) = cutout_session();
        let target = ContourTarget::Cutout { panel, machining };
        assert_eq!(target.id(), panel, "the panel frames the ring");
        assert!(ContourEdit::is_editable(session.project(), target));
        let stored = offset_rectangle(100, 100, 200, 100);
        let mut edit = ContourEdit::start(session.project(), target).expect("a contour cutout is editable");
        assert_eq!(edit.base().as_ring(), Some(&stored));
        assert!(!edit.is_dirty());

        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(340), millimetres(100)]).expect("move the corner");
        assert!(edit.is_dirty());
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(stored.clone())),
            "the document still has the stored ring"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the cutout ring");
        let record = session
            .project()
            .panel(panel)
            .and_then(|panel| panel.machining_by_id(machining))
            .expect("the record stays");
        let cutout = record.cutout().expect("a cutout record");
        assert_eq!(cutout.shape(), &CutoutShape::Contour(edit.draft().as_ring().expect("ring").clone()));
        assert_eq!(record.anchor().as_face(), Some(FaceSide::B), "the anchor is unchanged");
        assert_eq!(cutout.depth(), CutoutDepth::Blind { depth: millimetres(6) }, "the depth is unchanged");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));

        session.undo().expect("one undo restores the ring");
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(stored)),
        );
    }

    #[test]
    fn a_cutout_ring_that_leaves_the_core_is_refused() {
        let (mut session, panel, machining) = cutout_session();
        let target = ContourTarget::Cutout { panel, machining };
        let mut edit = ContourEdit::start(session.project(), target).expect("editable");
        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(900), millimetres(100)]).expect("move the corner out");

        let revision = session.project().revision();
        assert!(edit.apply(&mut session).is_err(), "the ring no longer fits the core");
        assert_eq!(session.project().revision(), revision, "a refused update changes nothing");
        assert!(edit.is_dirty(), "the draft stays so the user can fix it");
        assert_eq!(
            session
                .project()
                .panel(panel)
                .and_then(|panel| panel.machining_by_id(machining))
                .and_then(|record| record.cutout())
                .map(|cutout| cutout.shape().clone()),
            Some(CutoutShape::Contour(offset_rectangle(100, 100, 200, 100))),
            "the stored ring is untouched"
        );
    }

    #[test]
    fn a_rectangle_cutout_has_no_ring_to_edit() {
        let mut session = session(sample_project());
        let panel = Id::new(2).expect("panel id");
        let request = PanelMachiningAdd {
            panel,
            anchor: MachiningAnchor::face(FaceSide::A),
            params: MachiningParams::Cutout(Cutout::new(
                CutoutShape::Rectangle(furnicraft_testkit::sample_cutout_rect()),
                CutoutDepth::Through,
            )),
        };
        let params = json(&request).expect("params");
        let machining = session
            .dispatch(MACHINING_ADD_ID, &params)
            .expect("add the rectangle cutout")
            .created_machining_id()
            .expect("record");
        let target = ContourTarget::Cutout { panel, machining };
        assert!(ContourEdit::start(session.project(), target).is_none());
        assert!(!ContourEdit::is_editable(session.project(), target));
    }

    #[test]
    fn an_open_path_edit_is_one_revision_and_one_undo() {
        let path = sketch_path();
        let (project, object) = path_project(path.clone());
        let mut session = session(project);
        let target = ContourTarget::Path(object);
        assert_eq!(target.id(), object);
        assert!(ContourEdit::is_editable(session.project(), target));
        let mut edit = ContourEdit::start(session.project(), target).expect("a path is editable");
        assert_eq!(edit.base(), &ContourDraft::Chain(path.clone()));
        assert!(!edit.is_dirty());

        let last = edit.draft().entries().last().map(|entry| entry.id()).expect("last segment");
        edit.move_corner_at(PathCorner::End(last), [millimetres(700), millimetres(500)], CornerMove::Free)
            .expect("move the free end");
        assert!(edit.is_dirty());
        assert_eq!(
            session.project().object_path(object).expect("path"),
            &path,
            "the document still has the stored chain"
        );

        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the path");
        let stored = session.project().object_path(object).expect("path");
        assert_eq!(Some(stored), edit.draft().as_chain(), "a path is stored exactly as edited");
        assert!(!stored.is_closed());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the chain");
        assert_eq!(session.project().object_path(object).expect("path"), &path);
        assert!(!session.can_undo(), "the edit was the only undo entry");
    }

    #[test]
    fn a_closed_path_stays_closed_through_an_edit() {
        let path =
            Path::rectangle([millimetres(0), millimetres(0)], [millimetres(700), millimetres(500)]).expect("rectangle");
        let (project, object) = path_project(path);
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Path(object)).expect("editable");
        assert!(edit.draft().is_closed());

        let first = edit.draft().id_at(0).expect("first segment");
        edit.move_corner(first, [millimetres(-50), millimetres(-20)]).expect("move a corner");
        edit.apply(&mut session).expect("apply the path");

        let stored = session.project().object_path(object).expect("path");
        assert!(stored.is_closed(), "a replacement keeps the closed flag");
        assert_eq!(Some(stored), edit.draft().as_chain());
    }

    #[test]
    fn splitting_a_path_side_keeps_it_open_and_one_undo() {
        let path = sketch_path();
        let (project, object) = path_project(path);
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Path(object)).expect("editable");
        let first = edit.draft().id_at(0).expect("first segment");

        let inserted = edit.split_segment(first, [millimetres(300), millimetres(0)]).expect("split the bottom");
        assert_eq!(edit.draft().segment_count(), 3);
        let revision = session.project().revision();
        edit.apply(&mut session).expect("apply the split");

        let stored = session.project().object_path(object).expect("path");
        assert!(!stored.is_closed());
        assert_eq!(stored.entries().len(), 3);
        assert!(stored.index_of(inserted).is_some());
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores two segments");
        assert_eq!(session.project().object_path(object).expect("path").entries().len(), 2);
    }

    #[test]
    fn a_constructive_target_does_not_edit_a_path() {
        let (project, object) = path_project(sketch_path());
        assert!(ContourEdit::start(&project, ContourTarget::Object(object)).is_none());
        assert!(!ContourEdit::is_editable(&project, ContourTarget::Object(object)));
    }

    #[test]
    fn a_rejected_apply_leaves_the_document_alone() {
        let (project, panel) = panel_project();
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(1).expect("right edge");
        edit.move_corner(segment, [millimetres(800), millimetres(0)]).expect("move the corner");

        let params = serde_json::to_value(NodeDelete { node: panel }).expect("params");
        session.dispatch(NODE_DELETE_ID, &params).expect("delete the panel under the edit");
        let revision = session.project().revision();
        assert!(edit.apply(&mut session).is_err(), "the panel is gone");
        assert_eq!(session.project().revision(), revision, "a failed apply changes nothing");
        assert!(edit.is_dirty(), "the draft stays so the user can cancel or retry");
    }

    #[test]
    fn a_collapsed_segment_is_removed_on_apply_and_joins_the_nodes() {
        let (project, panel) = panel_project();
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Panel(panel)).expect("editable");
        let segment = edit.draft().id_at(0).expect("bottom edge");
        // A collapse records the segment to remove; the draft itself still has four sides.
        edit.record_collapse(segment);
        assert_eq!(edit.collapse(), Some(segment));
        assert_eq!(edit.draft().segment_count(), 4);
        edit.apply(&mut session).expect("apply removes the collapsed side");
        let ring = session.project().panel(panel).expect("panel").outline_contour().expect("ring");
        assert_eq!(ring.entries().len(), 3, "the collapsed side is gone and its nodes joined");
        session.undo().expect("one undo restores the whole edit");
        assert_eq!(
            session.project().panel(panel).expect("panel").outline_contour().expect("ring").entries().len(),
            4,
            "one undo restores the ring"
        );
    }

    #[test]
    fn a_ring_of_three_refuses_a_collapse() {
        // A triangular contour panel: removing one side would leave two, below the ring minimum.
        let triangle = Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(300), millimetres(0)],
            [millimetres(150), millimetres(260)],
        ])
        .expect("triangle");
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Triangle",
            triangle,
            None,
            sample_panel_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        let project = Project::new(root).expect("project");
        let mut session = session(project);
        let mut edit =
            ContourEdit::start(session.project(), ContourTarget::Panel(Id::new(2).expect("id"))).expect("editable");
        let segment = edit.draft().id_at(0).expect("first side");
        edit.record_collapse(segment);
        let error = edit.apply(&mut session).expect_err("a ring of three cannot lose a side");
        assert!(error.contains("three"), "the error names the minimum: {error}");
        assert_eq!(edit.draft().segment_count(), 3, "the draft is untouched");
    }

    #[test]
    fn a_path_collapse_removes_the_segment_and_keeps_one_undo_entry() {
        let (project, object) = path_project(sketch_path());
        let mut session = session(project);
        let mut edit = ContourEdit::start(session.project(), ContourTarget::Path(object)).expect("editable");
        let segment = edit.draft().id_at(0).expect("first segment");
        let revision = session.project().revision();
        edit.record_collapse(segment);
        edit.apply(&mut session).expect("apply removes the collapsed segment");
        let path = session.project().object_path(object).expect("path").clone();
        assert_eq!(path.entries().len(), 1, "one segment was removed");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"), "one undo entry");
        session.undo().expect("undo restores the path");
        assert_eq!(session.project().object_path(object).expect("path").entries().len(), 2);
    }
}
