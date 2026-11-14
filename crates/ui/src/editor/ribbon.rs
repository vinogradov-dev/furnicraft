//! The ribbon: its open state, its active tab, the tool buttons and the actions they stand for.
//!
//! The ribbon draws from one `RibbonContext` and reports what it asked for; the drawing, the
//! creation and the extrusion the buttons start are commands, while the view actions and the close
//! of a draft belong to the shell that owns the view camera and the work plane (ADR 0052). The
//! primitive size, the side count and the extrude height are typed session defaults, so the last
//! value a tool used carries into the next one.
//!
//! Refuses a panel or an assembly creation without a catalogue entry and an extrusion of an open
//! path; a refused action is the command's sentence and leaves the default untouched.
//!
//! Changes when: a tool appears on the ribbon, or a button's action changes.

use egui::Ui;

use furnicraft_engine::object_cmds::{OBJECT_EXTRUDE_HEIGHT_SET_ID, OBJECT_PATH_EXTRUDE_ID};
use furnicraft_engine::{ObjectExtrudeHeightSet, ObjectPathExtrude, Session};
use furnicraft_model::{Id, Path};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use super::Editor;
use crate::box_tool::BoxOwner;
use crate::command::{dispatch, json};
use crate::toolbars::{self, ToolAction};

impl Editor {
    /// Draws the ribbon: the row of tabs and the tool groups of the active tab.
    ///
    /// The open body, the active tab and the typed defaults are view state owned by the editor, so
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
            self.box_tool(),
            self.fill_arm().map(|arm| (arm.class, arm.mode)),
        );
        let draft = self.drawing.as_ref().map(|draft| (draft.tool(), draft.len(), draft.is_closed()));
        let context = toolbars::RibbonContext {
            has_panel_material,
            product_open: session.active_product().is_some(),
            selected,
            selected_path,
            active,
            perspective: view.perspective,
            owns_view: view.owns_view,
            grid: view.grid,
            draft,
        };
        let mut actions = Vec::new();
        let list_rect = toolbars::show(ui, &mut self.toolbars, context, &mut actions);
        let mut outcome = self.apply_ribbon_actions(session, actions, crate::language(ui.ctx()))?;
        outcome.list_rect = list_rect;
        Ok(outcome)
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
        lang: Lang,
    ) -> Result<toolbars::RibbonOutcome, String> {
        let target = self.creation_target(session.project());
        let mut outcome = toolbars::RibbonOutcome::default();
        for action in actions {
            match action {
                ToolAction::Draw { tool, sides } => {
                    self.start_drawing_with(tool, sides);
                    outcome.started_drawing = true;
                }
                ToolAction::CreateProductCard => outcome.open_product_card = true,
                ToolAction::DrawProductFrame => self.arm_box(BoxOwner::Product),
                ToolAction::CreatePanel => self.create_panel(session, target)?,
                ToolAction::Fill { class, mode } => {
                    // A second click on the same method drops the tool; the arm is a mode, not a
                    // one-shot, because a maker places several posts in one piece. Picking the other
                    // method of the same cell arms it with that mode instead of dropping the tool.
                    if self.fill_arm().is_some_and(|arm| arm.class == class && arm.mode == mode) {
                        self.cancel_fill();
                    } else {
                        self.arm_fill(session, class, mode, lang)?;
                    }
                }
                ToolAction::CreateAssembly => self.create_assembly(session, target)?,
                ToolAction::CreatePrimitive { kind, size } => self.arm_primitive(kind, size)?,
                ToolAction::CreateCamera => self.create_camera(session, target)?,
                ToolAction::CreateLight => self.create_light(session, target)?,
                ToolAction::ExtrudePath { object, height } => {
                    self.extrude_path(session, object, height, lang)?;
                    outcome.extruded = Some((object, height));
                }
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

    /// The regular polygon side count a new polygon starts with.
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
    pub(super) fn extrude_path(
        &mut self,
        session: &mut Session,
        object: Id,
        height: Length,
        lang: Lang,
    ) -> Result<(), String> {
        session
            .project()
            .object_path(object)
            .map_err(|error| error.to_string())?
            .to_contour()
            .map_err(|error| Message::ExtrudeRefused.with(lang, [error]))?;
        let params = json(&ObjectPathExtrude { object, height })?;
        dispatch(session, OBJECT_PATH_EXTRUDE_ID, &params)?;
        self.end_contour_edit();
        self.select_node(Some(object));
        Ok(())
    }

    /// Sets one body's extrusion height and remembers it as the next default.
    ///
    /// The height is a typed session default, not document data: it carries into the next
    /// extrusion, while the document only ever sees the command. A refused height leaves both the
    /// document and the default untouched.
    pub fn set_extrude_height(&mut self, session: &mut Session, object: Id, height: Length) -> Result<(), String> {
        let params = json(&ObjectExtrudeHeightSet { object, height })?;
        dispatch(session, OBJECT_EXTRUDE_HEIGHT_SET_ID, &params)?;
        self.toolbars.set_extrude_height(height);
        Ok(())
    }

    /// The height the ribbon will hand the next extrusion.
    pub const fn extrude_height(&self) -> Length {
        self.toolbars.extrude_height()
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
    pub fn ribbon_tab(&self) -> &'static toolbars::spec::TabSpec {
        self.toolbars.tab()
    }

    /// Opens one ribbon tab.
    pub fn set_ribbon_tab(&mut self, tab: &'static toolbars::spec::TabSpec) -> &'static toolbars::spec::TabSpec {
        self.toolbars.set_tab(tab)
    }

    /// Whether a ribbon variant list is open, so the shell can leave Escape and Enter to it.
    ///
    /// The ribbon draws after the shell reads the keys, so the shell asks the previous frame's
    /// state: a list that was open when the key was pressed owns that key (U67).
    pub fn variant_list_open(&self) -> bool {
        self.toolbars.list().is_some()
    }

    /// The methods the workspace pinned, by tool id.
    ///
    /// A pin is a workspace setting, not scene data: the editor holds the copy the shell handed it
    /// when the tab opened, and the shell writes it back when it changes (U67).
    pub fn pinned_variants(&self) -> &std::collections::BTreeMap<String, String> {
        self.toolbars.pinned_variants()
    }

    /// Replaces the pinned methods, from the settings file or from the shell's own sync.
    pub fn set_pinned_variants(&mut self, pinned: std::collections::BTreeMap<String, String>) {
        self.toolbars.set_pinned_variants(pinned);
    }

    /// Starts the drawing tool a key names, honouring the method pinned for that tool.
    ///
    /// The key table names drawing tools, and a tool with a list has one key: pressing it runs the
    /// pinned method without opening the list, so the ribbon stays quiet while the keyboard works
    /// (U67).
    pub fn start_shortcut(&mut self, tool: crate::draw::DrawTool, sides: u32) {
        let tool = toolbars::shortcut_method(tool, self.toolbars.pinned_variants());
        self.start_drawing_with(tool, sides);
    }
}
