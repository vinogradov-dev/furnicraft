//! The contour edit mode: the working copy of a panel ring, a path, a cutout ring or a body's ring.
//!
//! The mode owns the pointer while it is live. Every operation here works on the draft held by
//! `crate::contour_edit::ContourEdit` and changes nothing in the document; `apply_contour_edit`
//! turns the working copy into one command, and a rejected command keeps the draft so the user can
//! fix it or cancel.
//!
//! Refuses an operation with `ContourEditError` when no draft is live or the named segment, corner
//! or grip is not part of it. A live edit of a node that left the selection, or that no longer
//! stores a contour, ends at the next `retain_contour_edit`.
//!
//! Changes when: a grip, a corner or a segment of the working copy gets a new operation.

use furnicraft_engine::Session;
use furnicraft_model::{ContourEditError, ContourSegmentId, CornerMove, PathCorner, Project};
use furnicraft_units::Length;

use super::Editor;
use crate::contour_edit::{ContourEdit, ContourTarget};
use crate::contour_grips::{ContourGrip, GripValue};

impl Editor {
    /// The live contour edit, when the editor is reshaping a panel outline or an object contour.
    pub fn contour_edit(&self) -> Option<&ContourEdit> {
        self.contour_edit.as_ref()
    }

    /// Starts a contour edit on the selected panel or object.
    ///
    /// The mode needs exactly one selected node that stores a contour, and it owns the pointer while
    /// it is live: starting it leaves mesh mode, and an already running edit is never replaced
    /// silently. Returns whether the mode started.
    pub fn start_contour_edit(&mut self, project: &Project, target: ContourTarget) -> bool {
        if self.contour_edit.is_some() || self.selection.ids() != [target.id()] {
            return false;
        }
        let Some(edit) = ContourEdit::start(project, target) else {
            return false;
        };
        self.mesh_edit = None;
        self.drawing = None;
        self.contour_edit = Some(edit);
        true
    }

    /// Leaves the contour edit and drops the working copy; the document never changed.
    pub fn end_contour_edit(&mut self) {
        self.contour_edit = None;
    }

    /// Leaves the contour edit when its target vanished or the selection moved elsewhere.
    ///
    /// A live edit is never allowed to outlive the node it reshapes: applying it later would fail
    /// and the toolbar would describe a node the inspector no longer shows.
    pub fn retain_contour_edit(&mut self, project: &Project) {
        let Some(target) = self.contour_edit.as_ref().map(ContourEdit::target) else {
            return;
        };
        if self.selection.ids() != [target.id()] || !ContourEdit::is_editable(project, target) {
            self.contour_edit = None;
        }
    }

    /// Moves one corner of the working copy, or reports why the draft refused the point.
    pub fn contour_edit_move_corner(
        &mut self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<(), ContourEditError> {
        self.contour_edit_move_corner_at(PathCorner::Start(segment), point, CornerMove::Free)
    }

    /// Moves the path corner named by `corner` of the working copy, following `motion`.
    pub fn contour_edit_move_corner_at(
        &mut self,
        corner: PathCorner,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::UnknownSegment { segment: corner.segment() });
        };
        edit.move_corner_at(corner, point, motion)
    }

    /// Drags one grip of the working copy, following the motion the drag resolved.
    pub fn contour_edit_drag_grip(
        &mut self,
        grip: ContourGrip,
        point: [Length; 2],
        motion: CornerMove,
    ) -> Result<(), ContourEditError> {
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::Untagged);
        };
        edit.drag_grip(grip, point, motion)
    }

    /// Applies a number typed at one grip of the working copy.
    pub fn contour_edit_set_grip_value(&mut self, grip: ContourGrip, value: GripValue) -> Result<(), ContourEditError> {
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::Untagged);
        };
        edit.set_grip_value(grip, value)
    }

    /// Applies a typed length to an along-contour corner, moving it along the segment.
    ///
    /// Works for a path and for a ring: the corner moves along the motion segment's own support,
    /// and a zero length records a collapse for Apply to turn into a segment removal.
    pub fn contour_edit_along_length(
        &mut self,
        grip: ContourGrip,
        segment: ContourSegmentId,
        length: Length,
    ) -> Result<(), ContourEditError> {
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::Untagged);
        };
        let ContourGrip::Corner(corner) = grip else {
            return Err(ContourEditError::InvalidValue);
        };
        // The far end is fixed by where the corner sat when the edit began.
        let start = edit.base().grip_position(grip).ok_or(ContourEditError::InvalidValue)?;
        let far_end = edit.base().motion_far_end(segment, start).ok_or(ContourEditError::InvalidValue)?;
        let curve = edit.draft().segment(segment).ok_or(ContourEditError::UnknownSegment { segment })?;
        let target = curve.point_at_distance_from(far_end, length)?;
        edit.move_corner_at(corner, target, CornerMove::Along(segment))?;
        // A typed length of zero (or within the collapse threshold) records a collapse.
        if length.micrometres() <= 2 {
            edit.record_collapse(segment);
        } else {
            edit.clear_collapse();
        }
        Ok(())
    }

    /// Records that an along-contour drag collapsed `segment` to zero length.
    pub fn contour_edit_record_collapse(&mut self, segment: ContourSegmentId) {
        if let Some(edit) = self.contour_edit.as_mut() {
            edit.record_collapse(segment);
        }
    }

    /// Clears a recorded collapse.
    pub fn contour_edit_clear_collapse(&mut self) {
        if let Some(edit) = self.contour_edit.as_mut() {
            edit.clear_collapse();
        }
    }

    /// The current value of one grip, for a field that follows a drag.
    pub fn contour_edit_grip_value(&self, grip: ContourGrip) -> Option<GripValue> {
        self.contour_edit.as_ref()?.grip_value(grip)
    }

    /// Splits one side of the working copy and returns the new segment id.
    pub fn contour_edit_split(
        &mut self,
        segment: ContourSegmentId,
        point: [Length; 2],
    ) -> Result<ContourSegmentId, ContourEditError> {
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        edit.split_segment(segment, point)
    }

    /// Throws the working copy away without leaving the mode.
    pub fn reset_contour_edit(&mut self) {
        if let Some(edit) = self.contour_edit.as_mut() {
            edit.reset();
        }
    }

    /// Applies the working copy as one command and leaves the mode on success.
    ///
    /// A rejected edit keeps the draft alive so the user can fix it or cancel; the document and the
    /// undo history are untouched because the command validates before it mutates.
    pub fn apply_contour_edit(&mut self, session: &mut Session) -> Result<(), String> {
        let Some(edit) = self.contour_edit.as_ref() else {
            return Ok(());
        };
        // An untouched draft would dispatch the same ring and leave an empty undo entry behind.
        if edit.is_dirty() {
            edit.apply(session)?;
        }
        self.contour_edit = None;
        Ok(())
    }
}
