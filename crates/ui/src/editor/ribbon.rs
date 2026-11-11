//! The ribbon: its open state, its active tab, the tool buttons and the actions they stand for.
//!
//! The ribbon draws from one `RibbonContext` and reports what it asked for; the drawing, the
//! creation and the extrusion the buttons start are commands, while the view actions and the close
//! of a draft belong to the shell that owns the view camera and the work plane (ADR 0052). The
//! primitive size and the extrude height are session view state, so a field's last value carries
//! into the next tool.
//!
//! Refuses a panel or an assembly creation without a catalogue entry, an extrusion of an open path,
//! and an extrude height the command rejects; a refused action is the command's sentence and leaves
//! the field untouched.
//!
//! Changes when: a tool appears on the ribbon, or a button's action changes.

use egui::Ui;

use furnicraft_engine::object_cmds::{OBJECT_EXTRUDE_HEIGHT_SET_ID, OBJECT_PATH_EXTRUDE_ID};
use furnicraft_engine::{ObjectExtrudeHeightSet, ObjectPathExtrude, Session};
use furnicraft_model::{Id, Path};
use furnicraft_units::Length;

use super::Editor;
use crate::command::{dispatch, json};
use crate::toolbars::{self, ToolAction};

impl Editor {
    /// Draws the ribbon: the row of tabs and the tool groups of the active tab.
    ///
    /// The open body, the active tab and the numeric fields are view state owned by the editor, so
    /// they survive a selection change and a document command and never reach the scene file. A
    /// creation tool lands at the scene root unless a container was armed through an explicit entry
    /// (ADR 0056). The tool that owns the next click draws pressed, so the ribbon never hides what
    /// the pointer will do. The view actions it returns — frame all, the projection, the grid snap —
    /// belong to the shell, which owns the view camera and the drawing gizmo. So does the *Close* of
    /// a draft: the ribbon reports it, because closing owns the work plane, the gizmo state and the
    /// scene rebuild (ADR 0052).
    pub fn show_ribbon(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        view: toolbars::RibbonView,
    ) -> Result<toolbars::RibbonOutcome, String> {
        self.retain_creation_parent(session.project());
        let has_panel_material = !session.catalog().panel_materials().is_empty();
        let selected = self.selection.primary();
        let selected_path = selected.and_then(|id| session.project().object_path(id).ok()).map(Path::is_closed);
        let active = toolbars::tool_activity(
            self.drawing.as_ref().map(|draft| draft.tool()),
            self.pending_primitive.map(|pending| pending.kind),
        );
        let draft = self.drawing.as_ref().map(|draft| (draft.tool(), draft.len(), draft.is_closed()));
        let context = toolbars::RibbonContext {
            has_panel_material,
            selected,
            selected_path,
            active,
            perspective: view.perspective,
            owns_view: view.owns_view,
            grid: view.grid,
            draft,
        };
        let mut actions = Vec::new();
        toolbars::show(ui, &mut self.toolbars, context, &mut actions);
        self.apply_ribbon_actions(session, actions)
    }

    /// Runs the actions one drawn ribbon asked for and reports what the shell must still do.
    ///
    /// Split from [`Self::show_ribbon`] so the mapping from a clicked tool to its effect is testable
    /// without a window: the *Close* of a draft is reported as [`toolbars::RibbonOutcome::close_draft`]
    /// rather than run, because only the shell owns the work plane and the gizmo state.
    pub(super) fn apply_ribbon_actions(
        &mut self,
        session: &mut Session,
        actions: Vec<ToolAction>,
    ) -> Result<toolbars::RibbonOutcome, String> {
        let target = self.creation_target(session.project());
        let mut outcome = toolbars::RibbonOutcome::default();
        for action in actions {
            match action {
                ToolAction::Draw { tool, sides } => {
                    self.start_drawing_with(tool, sides);
                    outcome.started_drawing = true;
                }
                ToolAction::CreatePanel => self.create_panel(session, target)?,
                ToolAction::CreateAssembly => self.create_assembly(session, target)?,
                ToolAction::CreatePrimitive { kind, size } => self.arm_primitive(kind, size)?,
                ToolAction::CreateCamera => self.create_camera(session, target)?,
                ToolAction::CreateLight => self.create_light(session, target)?,
                ToolAction::ExtrudePath { object, height } => {
                    self.extrude_path(session, object, height)?;
                    outcome.extruded = Some((object, height));
                }
                ToolAction::ClearCreationParent => self.clear_creation_parent(),
                ToolAction::CloseDrawing => outcome.close_draft = true,
                ToolAction::CancelDrawing => self.cancel_drawing(),
                ToolAction::FrameAll => outcome.view = Some(toolbars::RibbonViewAction::FrameAll),
                ToolAction::ToggleProjection => {
                    outcome.view = Some(toolbars::RibbonViewAction::ToggleProjection);
                }
                ToolAction::ToggleGrid => outcome.view = Some(toolbars::RibbonViewAction::ToggleGrid),
            }
        }
        Ok(outcome)
    }

    /// The regular polygon side count the ribbon field holds.
    ///
    /// The key handler starts a polygon with the same count the ribbon would, so a keyboard
    /// shortcut and a button click never build different figures.
    pub fn polygon_sides(&self) -> u32 {
        self.toolbars.polygon_sides()
    }

    /// Extrudes a selected path into the body that owns it, in place.
    ///
    /// Only a closed path encloses a ring, so an open one is refused with the reason from
    /// `Path::to_contour` before a command runs. The node becomes the body and keeps its id, name,
    /// placement and pivot: the drawing is not left behind as a second object, and the selection
    /// stays on the one node the user was editing. A live contour edit of that node ends here: the
    /// node is a body now, and its pull arrow would compete with the corner grips for the pointer.
    pub(super) fn extrude_path(&mut self, session: &mut Session, object: Id, height: Length) -> Result<(), String> {
        session
            .project()
            .object_path(object)
            .map_err(|error| error.to_string())?
            .to_contour()
            .map_err(|error| format!("The selected path cannot be extruded: {error}"))?;
        let params = json(&ObjectPathExtrude { object, height })?;
        dispatch(session, OBJECT_PATH_EXTRUDE_ID, &params)?;
        self.end_contour_edit();
        self.select_node(Some(object));
        Ok(())
    }

    /// Sets one body's extrusion height and remembers it as the next default.
    ///
    /// The height is session view state, not document data: the ribbon field carries it into the
    /// next extrusion, while the document only ever sees the command. A refused height leaves both
    /// the document and the field untouched.
    pub fn set_extrude_height(&mut self, session: &mut Session, object: Id, height: Length) -> Result<(), String> {
        let params = json(&ObjectExtrudeHeightSet { object, height })?;
        dispatch(session, OBJECT_EXTRUDE_HEIGHT_SET_ID, &params)?;
        self.toolbars.set_extrude_height_text(height.to_string());
        Ok(())
    }

    /// The height the ribbon will hand the next extrusion.
    pub fn extrude_height_text(&self) -> &str {
        self.toolbars.extrude_height_text()
    }

    /// Whether the ribbon's body is open.
    pub fn ribbon_open(&self) -> bool {
        self.toolbars.is_open()
    }

    /// Opens or closes the ribbon's body and returns its new state.
    pub fn set_ribbon_open(&mut self, open: bool) -> bool {
        self.toolbars.set_open(open)
    }

    /// The tab whose groups the ribbon draws.
    pub fn ribbon_tab(&self) -> toolbars::RibbonTab {
        self.toolbars.tab()
    }

    /// Opens one ribbon tab.
    pub fn set_ribbon_tab(&mut self, tab: toolbars::RibbonTab) -> toolbars::RibbonTab {
        self.toolbars.set_tab(tab)
    }
}
