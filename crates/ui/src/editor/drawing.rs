//! The drawing mode: the draft a draw tool is building on the work plane.
//!
//! The draft is view state until its one commit: `crate::draw::DrawDraft` collects the
//! clicks, and `commit_drawing` turns the finished chain into one `object.path.create`. The shell
//! measures the work plane, so a commit receives the *world* frame and converts it to the parent's
//! frame here. A commit hands the new node to the contour edit mode.
//!
//! Refuses to commit with no draft via `DrawDraftError`, and returns the command's own sentence when
//! the document rejects the ring; a refused close reopens the ring so the user keeps the shape.
//!
//! Changes when: a draw tool's input or the one commit path changes.

use furnicraft_engine::Session;
use furnicraft_model::Placement;
use furnicraft_text::Lang;
use furnicraft_units::Length;

use super::Editor;
use crate::contour_edit::ContourTarget;
use crate::draw::{DEFAULT_POLYGON_SIDES, DrawDraft, DrawDraftError, DrawTool, DrawnPath};

impl Editor {
    /// The live drawing draft, when a drawing tool is building a chain.
    pub fn drawing(&self) -> Option<&DrawDraft> {
        self.drawing.as_ref()
    }

    /// Starts an empty drawing draft with `tool` and leaves the other pointer modes.
    ///
    /// The regular polygon uses the default side count; every other tool ignores the count, so
    /// this is the plain "start" a caller without a side field uses.
    pub fn start_drawing(&mut self, tool: DrawTool) {
        self.start_drawing_with(tool, DEFAULT_POLYGON_SIDES);
    }

    /// Starts an empty drawing draft with `tool` and the regular polygon's side count.
    ///
    /// A running draft is replaced, because it wrote nothing and undo gains no half figure
    /// (ADR 0076): choosing another tool is one gesture, not "cancel, then start". The count is
    /// clamped by [`DrawDraft::with_sides`], so a ribbon field can pass anything and the document
    /// still receives a polygon it accepts.
    pub fn start_drawing_with(&mut self, tool: DrawTool, sides: u32) {
        self.mesh_edit = None;
        self.contour_edit = None;
        self.pending_primitive = None;
        self.drawing = Some(DrawDraft::with_sides(tool, sides));
    }

    /// Flips the running polyline between straight and arc input.
    ///
    /// Returns the mode after the call, or `None` when no draft is running. Other tools have no
    /// second mode, so the mode they report stays false.
    pub fn toggle_drawing_arc_mode(&mut self) -> Option<bool> {
        self.drawing.as_mut().map(DrawDraft::toggle_arc_mode)
    }

    /// Appends a work-plane point to the running draft as a straight segment.
    pub fn drawing_point(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        self.drawing.as_mut().ok_or(DrawDraftError::NotStarted)?.push(point)
    }

    /// Starts an arc at the running draft's last end through `point`.
    ///
    /// The next [`Editor::drawing_point`] ends the arc and the segment after it starts at the stored
    /// arc end, so a chain that mixes both kinds still closes (ADR 0048).
    pub fn drawing_arc_through(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        self.drawing.as_mut().ok_or(DrawDraftError::NotStarted)?.push_arc_through(point)
    }

    /// Undoes the last drawing action: a close first, then the last point.
    ///
    /// Returns `false` when no draft is running or it is empty and open.
    pub fn undo_drawing(&mut self) -> bool {
        self.drawing.as_mut().is_some_and(DrawDraft::undo)
    }

    /// Commits the running draft as one path object and opens it for editing.
    ///
    /// The drawing lands in the armed container, or at the scene root like every other creation
    /// tool. `placement` is the work plane's *world* frame, the one the shell can measure; the
    /// command stores a placement relative to the parent, so this converts it with the parent's
    /// inverse first. The command then moves the chain so its creation pivot is the object's local
    /// zero. The new node becomes the selection and immediately enters contour edit mode with its
    /// grips, so the figure the user just drew can be reshaped without a second click. Escape or
    /// Enter leaves that mode. A refused path leaves the draft and the selection untouched.
    pub fn commit_drawing(
        &mut self,
        session: &mut Session,
        placement: Placement,
        lang: Lang,
    ) -> Result<DrawnPath, String> {
        let Some(draft) = self.drawing.as_ref() else {
            return Err(DrawDraftError::NotStarted.to_string());
        };
        let parent = self.creation_target(session.project());
        let placement = session.project().placement_in_parent(parent, placement).map_err(|error| error.to_string())?;
        let object = draft.commit(session, parent, placement, lang)?;
        let tool = draft.tool();
        self.drawing = None;
        self.selection.replace(object);
        // A deliberate selection also counts as the first show, so the tree does not replace the
        // freshly drawn node with the first panel on the next frame.
        self.initialized = true;
        // The new node is selected and editable, so the drawing hands straight over to the grips;
        // a tool that somehow produced a node without a path simply stays out of edit mode.
        self.start_contour_edit(session.project(), ContourTarget::Path(object));
        Ok(DrawnPath { object, tool })
    }

    /// Closes the running ring and commits it as one path object.
    ///
    /// This is the only operation behind *Close*, whichever surface asked for it: the ribbon, the
    /// status bar and a click on the first draft point all end here (ADR 0052). Closing and
    /// committing belong together, because a closed draft that never reached the document is a ring
    /// the user already finished and cannot edit.
    ///
    /// A refused close changes nothing — the ring stays open and editable, and no command ran. A
    /// close the document then refuses reopens the ring for the same reason: the user keeps the
    /// shape they drew and can fix it without redrawing.
    pub fn close_and_commit_drawing(
        &mut self,
        session: &mut Session,
        placement: Placement,
        lang: Lang,
    ) -> Result<DrawnPath, String> {
        self.close_drawing().map_err(|error| error.to_string())?;
        match self.commit_drawing(session, placement, lang) {
            Ok(drawn) => Ok(drawn),
            Err(error) => {
                self.reopen_drawing();
                Err(error)
            }
        }
    }

    /// Closes the running draft back to its first point.
    ///
    /// A primitive that only closes: the one operation the chrome runs is
    /// [`Self::close_and_commit_drawing`], so a closed draft cannot be left behind.
    pub(crate) fn close_drawing(&mut self) -> Result<(), DrawDraftError> {
        self.drawing.as_mut().ok_or(DrawDraftError::NotStarted)?.close()
    }

    /// Reopens a closed ring, so a refused commit leaves the drawing editable.
    fn reopen_drawing(&mut self) {
        if let Some(draft) = self.drawing.as_mut()
            && draft.is_closed()
        {
            draft.undo();
        }
    }

    /// Drops the running draft; the document never changed.
    pub fn cancel_drawing(&mut self) {
        self.drawing = None;
    }
}
