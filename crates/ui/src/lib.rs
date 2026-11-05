//! The egui shell: panels, widgets and input wiring.
//!
//! Thin by design: actions are dispatched to the engine as commands, and the UI never mutates the
//! document directly.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod camera_card;
mod command;
mod container_card;
mod contour_edit;
pub mod icons;
mod light_card;
mod machining;
mod mesh_edit;
mod object_card;
mod panel_card;
mod pivot_card;
mod placement_card;
mod properties_card;
mod scene_tree;
mod section;
pub mod toolbars;
pub mod ui_scale;

use std::collections::HashSet;

use egui::{Color32, Ui};
use furnicraft_engine::node_cmds::{
    ASSEMBLY_CREATE_ID, NODE_COMPONENT_CONVERT_TO_GROUP_ID, NODE_DELETE_ID, NODE_DUPLICATE_ID, NODE_GROUP_ID,
    NODE_UNGROUP_ID,
};
use furnicraft_engine::object_cmds::{OBJECT_EXTRUDE_CREATE_ID, OBJECT_PRIMITIVE_CREATE_ID};
use furnicraft_engine::panel_cmds::CREATE_ID as PANEL_CREATE_ID;
use furnicraft_engine::scene_cmds::{CAMERA_CREATE_ID, LIGHT_CREATE_ID};
use furnicraft_engine::{
    AssemblyCreate, CameraCreate, LightCreate, NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup,
    NodeUngroup, ObjectExtrudeCreate, ObjectPrimitiveCreate, PanelCreate, PrimitiveKind, Session,
};
use furnicraft_model::{
    CameraProjection, ContourEditError, ContourSegmentId, Id, LightKind, NodeQuery, PanelNominalSize, Path, Placement,
    Project,
};
use furnicraft_units::Length;

use crate::camera_card::CameraCard;
use crate::command::{dispatch, json};
use crate::container_card::ContainerCard;
use crate::light_card::LightCard;
use crate::mesh_edit::is_editable_mesh;
use crate::object_card::{ObjectAction, ObjectCard};
use crate::panel_card::{PanelAction, PanelCard};
use crate::pivot_card::PivotCard;
use crate::placement_card::PlacementCard;
use crate::properties_card::PropertiesCard;
use crate::scene_tree::{Selection, TreeAction};
use crate::toolbars::ToolAction;

pub use contour_edit::{ContourEdit, ContourTarget, DrawDraft, DrawDraftError, DrawTool, DrawnPath};
pub use mesh_edit::{MeshEdit, MeshElement};
pub use toolbars::ToolPanel;

/// Default length of a newly created panel, in millimetres.
const NEW_PANEL_LENGTH_MM: i64 = 600;
/// Default width of a newly created panel, in millimetres.
const NEW_PANEL_WIDTH_MM: i64 = 400;

/// A primitive waiting for its work-plane click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingPrimitive {
    /// The container that receives the node.
    parent: Id,
    /// Which primitive to build.
    kind: PrimitiveKind,
    /// Full extent: an edge, a diameter and a height, or a diameter.
    size: Length,
}

/// Selection and draft state owned by the editor panels.
#[derive(Debug, Default)]
pub struct Editor {
    selection: Selection,
    open: HashSet<Id>,
    mesh_edit: Option<MeshEdit>,
    contour_edit: Option<ContourEdit>,
    drawing: Option<DrawDraft>,
    pending_primitive: Option<PendingPrimitive>,
    toolbars: toolbars::Toolbars,
    card: PanelCard,
    container_card: ContainerCard,
    object_card: ObjectCard,
    camera_card: CameraCard,
    light_card: LightCard,
    placement_card: PlacementCard,
    pivot_card: PivotCard,
    properties_card: PropertiesCard,
    type_filter: String,
    error: Option<String>,
    initialized: bool,
}

impl Editor {
    /// Creates an editor with no node selected; the scene tree selects the first panel on first show.
    pub fn new() -> Self {
        Self::default()
    }

    /// The primary selected node, if any.
    pub fn selected_node(&self) -> Option<Id> {
        self.selection.primary()
    }

    /// Every selected node in selection order, oldest first; the last one is the primary.
    ///
    /// Multi-selection is view state, so a gizmo drag moves all of these nodes as one document
    /// change while the inspector keeps showing the primary.
    pub fn selected_nodes(&self) -> &[Id] {
        self.selection.ids()
    }

    /// The mesh edit session, when the editor is in sub-object mode.
    pub fn mesh_edit(&self) -> Option<&MeshEdit> {
        self.mesh_edit.as_ref()
    }

    /// Enters vertex editing on the primary selection.
    ///
    /// The mode needs exactly one selected node and that node must carry a baked mesh; a panel,
    /// a container or a constructive object refuses it. Returns whether the mode started.
    pub fn start_mesh_edit(&mut self, project: &Project) -> bool {
        if self.selection.ids().len() != 1 {
            return false;
        }
        let Some(object) = self.selection.primary() else {
            return false;
        };
        if !is_editable_mesh(project, object) {
            return false;
        }
        self.contour_edit = None;
        self.drawing = None;
        self.mesh_edit = Some(MeshEdit::new(object));
        true
    }

    /// Leaves vertex editing and drops the vertex selection.
    pub fn end_mesh_edit(&mut self) {
        self.mesh_edit = None;
    }

    /// Selects a vertex, replacing the selection or adding to it.
    ///
    /// Returns `false` when the editor is not in mesh mode. The caller only passes indices that
    /// were picked from the edited mesh, so the index is not validated here.
    pub fn select_mesh_vertex(&mut self, vertex: u32, additive: bool) -> bool {
        let Some(edit) = self.mesh_edit.as_mut() else {
            return false;
        };
        if additive {
            edit.toggle(vertex);
        } else {
            edit.select(vertex);
        }
        true
    }

    /// Clears the vertex selection while staying in mesh mode.
    pub fn clear_mesh_vertices(&mut self) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.clear();
        }
    }

    /// Selects a face, replacing the selection or adding to it.
    ///
    /// Returns `false` when the editor is not in mesh mode. The caller only passes indices that
    /// were picked from the edited mesh, so the index is not validated here.
    pub fn select_mesh_face(&mut self, face: u32, additive: bool) -> bool {
        let Some(edit) = self.mesh_edit.as_mut() else {
            return false;
        };
        if additive {
            edit.toggle_face(face);
        } else {
            edit.select_face(face);
        }
        true
    }

    /// Clears the face selection while staying in mesh mode.
    pub fn clear_mesh_faces(&mut self) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.clear_faces();
        }
    }

    /// Switches between vertex and face selection.
    pub fn set_mesh_element(&mut self, element: MeshElement) {
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.set_element(element);
        }
    }

    /// Leaves mesh mode when its object vanished or the selection moved elsewhere, and drops
    /// vertex and face indices that the current mesh no longer has.
    pub fn retain_mesh_edit(&mut self, project: &Project) {
        let Some(object) = self.mesh_edit.as_ref().map(MeshEdit::object) else {
            return;
        };
        if self.selection.ids() != [object] || !is_editable_mesh(project, object) {
            self.mesh_edit = None;
            return;
        }
        let (vertices, faces) = project
            .object(object)
            .and_then(|object| object.geometry().as_mesh())
            .map_or((0, 0), |mesh| (mesh.positions().len(), mesh.faces().len()));
        if let Some(edit) = self.mesh_edit.as_mut() {
            edit.retain_vertices(vertices);
            edit.retain_faces(faces);
        }
    }

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
        let Some(edit) = self.contour_edit.as_mut() else {
            return Err(ContourEditError::UnknownSegment { segment });
        };
        edit.move_corner(segment, point)
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

    /// The live drawing draft, when a drawing tool is building a chain.
    pub fn drawing(&self) -> Option<&DrawDraft> {
        self.drawing.as_ref()
    }

    /// Starts an empty drawing draft with `tool` and leaves the other pointer modes.
    ///
    /// A running draft is never replaced silently; the caller ends or commits it first. Returns
    /// whether the draft started.
    pub fn start_drawing(&mut self, tool: DrawTool) -> bool {
        if self.drawing.is_some() {
            return false;
        }
        self.mesh_edit = None;
        self.contour_edit = None;
        self.drawing = Some(DrawDraft::new(tool));
        true
    }

    /// Appends a work-plane point to the running draft.
    pub fn drawing_point(&mut self, point: [Length; 2]) -> Result<(), DrawDraftError> {
        self.drawing.as_mut().ok_or(DrawDraftError::NotStarted)?.push(point)
    }

    /// Undoes the last drawing action: a close first, then the last point.
    ///
    /// Returns `false` when no draft is running or it is empty and open.
    pub fn undo_drawing(&mut self) -> bool {
        self.drawing.as_mut().is_some_and(DrawDraft::undo)
    }

    /// Commits the running draft as one path object and drops it.
    ///
    /// The drawing lands at the scene root, like every other creation tool, and is placed on
    /// `placement`, which maps the work plane onto the object's local XY. The new node becomes the
    /// selection, so the tool ends with what it just drew visible in the inspector. A refused path
    /// leaves the draft and the selection untouched.
    pub fn commit_drawing(&mut self, session: &mut Session, placement: Placement) -> Result<DrawnPath, String> {
        let Some(draft) = self.drawing.as_ref() else {
            return Err(DrawDraftError::NotStarted.to_string());
        };
        let parent = creation_root(session.project());
        let object = draft.commit(session, parent, placement)?;
        let tool = draft.tool();
        self.drawing = None;
        self.selection.replace(object);
        // A deliberate selection also counts as the first show, so the tree does not replace the
        // freshly drawn node with the first panel on the next frame.
        self.initialized = true;
        Ok(DrawnPath { object, tool })
    }

    /// Closes the running draft back to its first point.
    pub fn close_drawing(&mut self) -> Result<(), DrawDraftError> {
        self.drawing.as_mut().ok_or(DrawDraftError::NotStarted)?.close()
    }

    /// Drops the running draft; the document never changed.
    pub fn cancel_drawing(&mut self) {
        self.drawing = None;
    }

    /// Replaces the selection with one node, or clears it for `None`.
    ///
    /// A deliberate selection also counts as the first show, so the tree does not replace it with
    /// the first panel afterwards.
    pub fn select_node(&mut self, node: Option<Id>) {
        self.initialized = true;
        match node {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Selects the result of a viewport click on `hit`.
    ///
    /// The viewport returns the panel under the pointer; a closed container above it is selected
    /// instead, so a click picks the cabinet rather than one of its boards. A click outside the
    /// scene clears the selection.
    pub fn select_viewport_hit(&mut self, project: &Project, hit: Option<Id>) {
        self.initialized = true;
        match hit.and_then(|hit| resolve_viewport_pick(project, &self.open, hit)) {
            Some(node) => self.selection.replace(node),
            None => self.selection.clear(),
        }
    }

    /// Enters one level below the picked node: opens it and selects the child that owns `hit`.
    ///
    /// A double-click on a closed container reveals its contents; double-clicking a panel has no
    /// effect because a panel has no children.
    pub fn enter_viewport_level(&mut self, project: &Project, hit: Option<Id>) {
        let Some(hit) = hit else {
            return;
        };
        self.initialized = true;
        let Some(selected) = resolve_viewport_pick(project, &self.open, hit) else {
            return;
        };
        self.selection.replace(selected);
        if !project.is_container(selected) {
            return;
        }
        self.open.insert(selected);
        if let Some(child) = child_containing(project, selected, hit) {
            self.selection.replace(child);
        }
    }

    /// Closes one level and returns the selection to the container that owns it.
    ///
    /// The current node closes when it is an open container; otherwise the nearest open ancestor
    /// closes. With no open ancestor this does nothing.
    pub fn exit_level(&mut self, project: &Project) {
        let Some(selected) = self.selection.primary() else {
            return;
        };
        if project.is_container(selected) && self.open.remove(&selected) {
            if let Some(parent) = project.parent_of(selected)
                && parent != project.root().id()
            {
                self.selection.replace(parent);
            }
            return;
        }
        let mut node = selected;
        while let Some(parent) = project.parent_of(node) {
            if parent == project.root().id() {
                break;
            }
            if self.open.remove(&parent) {
                self.selection.replace(parent);
                return;
            }
            node = parent;
        }
    }

    /// Drops open flags for nodes that no longer exist or no longer own children.
    fn retain_open(&mut self, project: &Project) {
        self.open.retain(|id| project.is_container(*id));
    }

    /// Draws the themed toolbar strip: one collapsible panel per tool theme.
    ///
    /// The open panels are view state owned by the editor, so they survive a selection change and a
    /// document command and never reach the scene file. Every creation tool lands at the scene
    /// root; a child is created only through an explicit entry (ADR 0056). The tool that owns the
    /// next click draws pressed, so the strip never hides what the pointer will do. Returns whether a
    /// drawing tool started, so the shell can reset its own pointer state.
    pub fn show_toolbars(&mut self, ui: &mut Ui, session: &mut Session) -> Result<bool, String> {
        let parent = creation_root(session.project());
        let has_panel_material = !session.catalog().panel_materials().is_empty();
        let selected = self.selection.primary();
        let selected_path = selected.and_then(|id| session.project().object_path(id).ok()).map(Path::is_closed);
        let active = toolbars::tool_activity(
            self.drawing.as_ref().map(|draft| draft.tool()),
            self.pending_primitive.map(|pending| pending.kind),
        );
        let mut actions = Vec::new();
        toolbars::show(ui, &mut self.toolbars, has_panel_material, selected, selected_path, active, &mut actions);
        let mut started_drawing = false;
        for action in actions {
            match action {
                ToolAction::Draw(tool) => {
                    if !self.start_drawing(tool) {
                        return Err("A drawing is already in progress; finish or cancel it first.".to_owned());
                    }
                    started_drawing = true;
                }
                ToolAction::CreatePanel => self.create_panel(session, parent)?,
                ToolAction::CreateAssembly => self.create_assembly(session, parent)?,
                ToolAction::CreatePrimitive { kind, size } => self.arm_primitive(parent, kind, size)?,
                ToolAction::CreateCamera => self.create_camera(session, parent)?,
                ToolAction::CreateLight => self.create_light(session, parent)?,
                ToolAction::ExtrudePath { object, height } => self.extrude_path(session, object, height)?,
            }
        }
        Ok(started_drawing)
    }

    /// Dispatches `panel.create` with the catalogue's first material and the default size.
    fn create_panel(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let material = session
            .catalog()
            .panel_materials()
            .first()
            .ok_or_else(|| "Add a panel material to the catalogue first.".to_owned())?
            .id();
        let nominal_size = default_panel_size()?;
        let params = json(&PanelCreate {
            parent,
            name: "Panel".to_owned(),
            nominal_size,
            material,
            placement: Placement::IDENTITY,
        })?;
        let outcome = dispatch(session, PANEL_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Dispatches `assembly.create`.
    fn create_assembly(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let params = json(&AssemblyCreate { parent, name: "Assembly".to_owned(), placement: Placement::IDENTITY })?;
        let outcome = dispatch(session, ASSEMBLY_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Arms a primitive that the next work-plane click places.
    ///
    /// The panel asks for a click instead of creating at the origin: the tool is a placement, and a
    /// stale primitive at the world origin is never what the user wanted. Arming leaves the other
    /// pointer modes, because the next click creates rather than selects.
    fn arm_primitive(&mut self, parent: Id, kind: PrimitiveKind, size: Length) -> Result<(), String> {
        if size.micrometres() <= 0 {
            return Err("The primitive size must be positive.".to_owned());
        }
        self.mesh_edit = None;
        self.contour_edit = None;
        self.drawing = None;
        self.pending_primitive = Some(PendingPrimitive { parent, kind, size });
        Ok(())
    }

    /// The primitive waiting for a work-plane click: its parent, kind and size.
    pub fn pending_primitive(&self) -> Option<(Id, PrimitiveKind, Length)> {
        self.pending_primitive.map(|pending| (pending.parent, pending.kind, pending.size))
    }

    /// Drops an armed primitive without creating anything.
    pub fn cancel_primitive(&mut self) {
        self.pending_primitive = None;
    }

    /// Creates the armed primitive at `placement` and selects it.
    pub fn place_primitive(&mut self, session: &mut Session, placement: Placement) -> Result<Id, String> {
        let pending = self.pending_primitive.ok_or_else(|| "No primitive is waiting for a click.".to_owned())?;
        self.create_primitive(session, pending.parent, pending.kind, pending.size, placement)?;
        self.pending_primitive = None;
        self.selection.primary().ok_or_else(|| "The primitive command created no node.".to_owned())
    }

    /// Dispatches `object.primitive.create` for one primitive at `placement`.
    fn create_primitive(
        &mut self,
        session: &mut Session,
        parent: Id,
        kind: PrimitiveKind,
        size: Length,
        placement: Placement,
    ) -> Result<(), String> {
        let name = match kind {
            PrimitiveKind::Cube => "Cube",
            PrimitiveKind::Cylinder => "Cylinder",
            PrimitiveKind::Sphere => "Sphere",
        };
        let params = json(&ObjectPrimitiveCreate { parent, name: name.to_owned(), primitive: kind, size, placement })?;
        let outcome = dispatch(session, OBJECT_PRIMITIVE_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Extrudes a selected path into a body in the path's own frame.
    ///
    /// Only a closed path encloses a ring, so an open one is refused with the reason from
    /// `Path::to_contour` before a command runs. The body is a sibling of the path with the path's
    /// placement, because the ring is stored in the path's local XY; the path stays so the user can
    /// extrude again or edit the drawing.
    fn extrude_path(&mut self, session: &mut Session, object: Id, height: Length) -> Result<(), String> {
        let project = session.project();
        let path = project.object_path(object).map_err(|error| error.to_string())?;
        let contour = path.to_contour().map_err(|error| format!("The selected path cannot be extruded: {error}"))?;
        let parent = project.parent_of(object).unwrap_or(project.root().id());
        let placement = project.node(object).map_or(Placement::IDENTITY, |node| node.placement());
        let name = project.object(object).map_or_else(|| "Body".to_owned(), |object| format!("{} body", object.name()));
        let params = json(&ObjectExtrudeCreate { parent, name, contour, height, placement })?;
        let outcome = dispatch(session, OBJECT_EXTRUDE_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Dispatches `camera.create`.
    fn create_camera(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let params = json(&CameraCreate {
            parent,
            name: "Camera".to_owned(),
            placement: Placement::IDENTITY,
            projection: CameraProjection::DEFAULT,
        })?;
        let outcome = dispatch(session, CAMERA_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Dispatches `light.create` with a sun light.
    fn create_light(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let params = json(&LightCreate {
            parent,
            name: "Light".to_owned(),
            placement: Placement::IDENTITY,
            kind: LightKind::Sun,
            color: [1.0, 0.97, 0.92],
            intensity: 1.0,
        })?;
        let outcome = dispatch(session, LIGHT_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Selects the node a create command returned and marks the editor as initialised.
    fn select_created(&mut self, id: Option<Id>) {
        let Some(id) = id else {
            return;
        };
        self.selection.replace(id);
        self.initialized = true;
    }

    /// Whether the panel of the toolbar strip is open.
    pub fn toolbar_open(&self, panel: ToolPanel) -> bool {
        self.toolbars.is_open(panel)
    }

    /// Opens or closes one panel of the toolbar strip.
    pub fn set_toolbar_open(&mut self, panel: ToolPanel, open: bool) -> bool {
        self.toolbars.set_open(panel, open)
    }

    /// Draws the scene tree and dispatches the tree actions.
    pub fn show_scene_tree(&mut self, ui: &mut Ui, session: &mut Session) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        if !self.initialized {
            self.selection.select_first_panel(session.project());
            self.initialized = true;
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
            ui.separator();
        }
        let actions = scene_tree::show(ui, session, &mut self.selection, &mut self.type_filter);
        if actions.is_empty() {
            return;
        }
        self.error = None;
        for action in actions {
            if let Err(error) = self.run_action(session, action) {
                self.error = Some(error);
                break;
            }
        }
        ui.ctx().request_repaint();
    }

    /// Draws the inspector card for the primary selection.
    pub fn show_card(&mut self, ui: &mut Ui, session: &mut Session) {
        self.retain_mesh_edit(session.project());
        self.retain_contour_edit(session.project());
        let Some(primary) = self.selection.primary() else {
            ui.label("Select a node in the scene tree.");
            return;
        };
        if session.project().parent_of(primary).is_some() {
            self.placement_card.show(ui, session, primary);
            self.pivot_card.show(ui, session, primary);
        }
        if session.project().panel(primary).is_some() {
            let contour_editing = self.contour_edit.as_ref().is_some_and(|edit| edit.target().id() == primary);
            let single_selection = self.selection.ids().len() == 1;
            let action = self.card.show(ui, session, Some(primary), contour_editing, single_selection);
            if action == PanelAction::StartContour {
                self.start_contour_edit(session.project(), ContourTarget::Panel(primary));
            }
        } else if session.project().object(primary).is_some() {
            let single_selection = self.selection.ids().len() == 1;
            let contour_editing = self.contour_edit.as_ref().is_some_and(|edit| edit.target().id() == primary);
            let action =
                self.object_card.show(ui, session, primary, self.mesh_edit.as_ref(), contour_editing, single_selection);
            match action {
                ObjectAction::None => {}
                ObjectAction::Toggle => {
                    if self.mesh_edit.is_some() {
                        self.end_mesh_edit();
                    } else {
                        self.start_mesh_edit(session.project());
                    }
                }
                ObjectAction::Element(element) => self.set_mesh_element(element),
                ObjectAction::StartContour => {
                    self.start_contour_edit(session.project(), ContourTarget::Object(primary));
                }
            }
        } else if session.project().camera(primary).is_some() {
            self.camera_card.show(ui, session, primary);
        } else if session.project().light(primary).is_some() {
            self.light_card.show(ui, session, primary);
        } else if session.project().is_container(primary) {
            self.container_card.show(ui, session, primary);
        } else {
            ui.label("Select a node in the scene tree.");
        }
        self.properties_card.show(ui, session, primary);
    }

    fn run_action(&mut self, session: &mut Session, action: TreeAction) -> Result<(), String> {
        match action {
            TreeAction::Duplicate { node } => {
                let params = json(&NodeDuplicate { node })?;
                let outcome = dispatch(session, NODE_DUPLICATE_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
            }
            TreeAction::Delete { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeDelete { node })?;
                dispatch(session, NODE_DELETE_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::Group { nodes } => {
                let params = json(&NodeGroup { nodes, name: "Group".to_owned() })?;
                let outcome = dispatch(session, NODE_GROUP_ID, &params)?;
                if let Some(id) = outcome.created_id() {
                    self.selection.replace(id);
                }
            }
            TreeAction::Ungroup { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeUngroup { node })?;
                dispatch(session, NODE_UNGROUP_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::ConvertToGroup { node } => {
                let parent = session.project().parent_of(node);
                let params = json(&NodeComponentConvertToGroup { node })?;
                dispatch(session, NODE_COMPONENT_CONVERT_TO_GROUP_ID, &params)?;
                self.after_node_removed(session, parent);
            }
            TreeAction::SelectUnderType { furntype } => {
                let ids = session.project().query(&NodeQuery::any().with_furntype_under(furntype));
                if ids.is_empty() {
                    return Err(format!("No node has furniture type {furntype} or a sub-type."));
                }
                self.selection.replace_all(ids);
            }
            TreeAction::Undo => {
                session.undo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
                self.retain_open(session.project());
            }
            TreeAction::Redo => {
                session.redo().map_err(|error| error.to_string())?;
                self.selection.retain_existing(session.project());
                self.retain_open(session.project());
            }
        }
        Ok(())
    }

    fn after_node_removed(&mut self, session: &Session, parent: Option<Id>) {
        self.selection.retain_existing(session.project());
        self.retain_open(session.project());
        if self.selection.primary().is_none()
            && let Some(parent) = parent
            && session.project().contains_node(parent)
        {
            self.selection.replace(parent);
        }
    }
}

/// The node a viewport click should select: the outermost closed container above the hit panel.
///
/// Walking up stops at the nearest open container, so an open level exposes its children. The root
/// is not a selectable node; a top-level panel or container is selected directly.
fn resolve_viewport_pick(project: &Project, open: &HashSet<Id>, hit: Id) -> Option<Id> {
    if !project.contains_node(hit) {
        return None;
    }
    if project.is_container(hit) {
        return Some(hit);
    }
    let root = project.root().id();
    let mut node = hit;
    while let Some(parent) = project.parent_of(node) {
        if parent == root || open.contains(&parent) {
            break;
        }
        node = parent;
    }
    Some(node)
}

/// The container every creation tool lands in.
///
/// The scene root is the only default parent, whatever the selection is: choosing a node must not
/// silently change where a new path, panel, assembly, primitive, camera or light goes (ADR 0056).
/// A child is created through an explicit entry instead. Extruding a path is not a creation tool,
/// so it stays a sibling of its path.
fn creation_root(project: &Project) -> Id {
    project.root().id()
}

/// The direct child of `container` whose subtree contains `node`.
fn child_containing(project: &Project, container: Id, node: Id) -> Option<Id> {
    if node == container {
        return None;
    }
    let mut current = node;
    for _ in 0..Project::MAX_ASSEMBLY_DEPTH {
        let parent = project.parent_of(current)?;
        if parent == container {
            return Some(current);
        }
        current = parent;
    }
    None
}

fn default_panel_size() -> Result<PanelNominalSize, String> {
    let length = Length::from_millimetres(NEW_PANEL_LENGTH_MM)
        .ok_or_else(|| "default panel length is out of range".to_owned())?;
    let width =
        Length::from_millimetres(NEW_PANEL_WIDTH_MM).ok_or_else(|| "default panel width is out of range".to_owned())?;
    PanelNominalSize::new(length, width).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use furnicraft_engine::NodePropertySet;
    use furnicraft_engine::node_cmds::NODE_PROPERTY_SET_ID;
    use furnicraft_model::Contour;
    use furnicraft_model::{
        Container, DEFAULT_BANDS, DEFAULT_SEGMENTS, FurnType, Geometry, Mesh, Panel, Placement, PropertyValue,
        SYSTEM_FURNTYPE,
    };
    use furnicraft_testkit::{sample_catalog, sample_panel_thickness, sample_project};

    fn cube_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("add object");
        (project, id)
    }

    /// A project whose root holds one contour panel with a 700 x 500 ring.
    fn contour_panel_project() -> (Project, Id) {
        let width = Length::from_millimetres(700).expect("width");
        let height = Length::from_millimetres(500).expect("height");
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [width, Length::ZERO],
            [width, height],
            [Length::ZERO, height],
        ])
        .expect("ring");
        let panel = Panel::new_contour(
            Id::new(2).expect("id"),
            "Shelf",
            ring,
            None,
            sample_panel_thickness(),
            Placement::IDENTITY,
        )
        .expect("contour panel");
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
        (Project::new(root).expect("project"), Id::new(2).expect("id"))
    }

    #[test]
    fn mesh_edit_starts_only_on_a_single_mesh_object() {
        let (project, cube) = cube_project();
        let mut editor = Editor::new();

        assert!(!editor.start_mesh_edit(&project), "nothing is selected");
        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        assert_eq!(editor.mesh_edit().map(MeshEdit::object), Some(cube));
        assert_eq!(editor.mesh_edit().map(|edit| edit.vertices().len()), Some(0));

        editor.end_mesh_edit();
        assert!(editor.mesh_edit().is_none());

        editor.select_node(Some(Id::new(2).expect("panel")));
        assert!(!editor.start_mesh_edit(&project), "a panel has no editable mesh");
        editor.select_node(Some(project.root().id()));
        assert!(!editor.start_mesh_edit(&project), "the root is a container");
    }

    #[test]
    fn a_multi_selection_cannot_enter_mesh_edit() {
        let (project, cube) = cube_project();
        let mut editor = Editor::new();
        editor.selection.replace_all(vec![cube, Id::new(2).expect("panel")]);
        assert!(!editor.start_mesh_edit(&project));

        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        editor.selection.replace_all(vec![cube, Id::new(2).expect("panel")]);
        editor.retain_mesh_edit(&project);
        assert!(editor.mesh_edit().is_none(), "a selection change leaves the mode");
    }

    #[test]
    fn a_mesh_edit_ends_when_its_object_disappears() {
        let (mut project, cube) = cube_project();
        let mut editor = Editor::new();
        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));

        editor.select_node(Some(project.root().id()));
        editor.retain_mesh_edit(&project);
        assert!(editor.mesh_edit().is_none());

        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        project.remove_node(cube).expect("remove the object");
        editor.selection.clear();
        editor.retain_mesh_edit(&project);
        assert!(editor.mesh_edit().is_none());
    }

    #[test]
    fn contour_edit_starts_only_on_a_single_contour_node() {
        let (project, panel) = contour_panel_project();
        let mut editor = Editor::new();
        assert!(!editor.start_contour_edit(&project, ContourTarget::Panel(panel)), "nothing is selected");

        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
        assert_eq!(editor.contour_edit().map(ContourEdit::target), Some(ContourTarget::Panel(panel)));
        assert!(!editor.start_contour_edit(&project, ContourTarget::Panel(panel)), "one edit at a time");

        editor.end_contour_edit();
        assert!(editor.contour_edit().is_none());

        let sample = sample_project();
        let rectangle = Id::new(2).expect("id");
        editor.select_node(Some(rectangle));
        assert!(!editor.start_contour_edit(&sample, ContourTarget::Panel(rectangle)), "a rectangle panel has no ring");

        editor.selection.replace_all(vec![panel, project.root().id()]);
        assert!(
            !editor.start_contour_edit(&project, ContourTarget::Panel(panel)),
            "a multi-selection cannot enter contour editing"
        );
    }

    #[test]
    fn contour_edit_ends_on_a_selection_change_or_a_removed_target() {
        let (mut project, panel) = contour_panel_project();
        let mut editor = Editor::new();
        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));

        editor.select_node(Some(project.root().id()));
        editor.retain_contour_edit(&project);
        assert!(editor.contour_edit().is_none(), "a selection change leaves the mode");

        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
        project.remove_node(panel).expect("remove the panel");
        editor.retain_contour_edit(&project);
        assert!(editor.contour_edit().is_none(), "a removed target leaves the mode");
    }

    #[test]
    fn applying_a_contour_edit_is_one_undoable_command() {
        let (project, panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(session.project(), ContourTarget::Panel(panel)));
        let segment = editor.contour_edit().and_then(|edit| edit.draft().id_at(1)).expect("right edge");
        editor
            .contour_edit_move_corner(segment, [Length::from_millimetres(800).expect("x"), Length::ZERO])
            .expect("move the corner");
        let revision = session.project().revision();
        editor.apply_contour_edit(&mut session).expect("apply the outline");
        assert!(editor.contour_edit().is_none(), "a successful apply leaves the mode");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("one undo restores the ring");
        assert_eq!(
            session.project().panel(panel).expect("panel").nominal_size().length(),
            Length::from_millimetres(700).expect("length")
        );
    }

    #[test]
    fn applying_an_untouched_contour_edit_changes_nothing() {
        let (project, panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(session.project(), ContourTarget::Panel(panel)));
        let revision = session.project().revision();
        editor.apply_contour_edit(&mut session).expect("leave the mode");
        assert!(editor.contour_edit().is_none());
        assert_eq!(session.project().revision(), revision, "an untouched draft dispatches nothing");
        assert!(!session.can_undo(), "no empty undo entry is left behind");
    }

    #[test]
    fn a_drawing_draft_is_view_state_until_it_is_committed() {
        let (project, _panel) = contour_panel_project();
        let session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        assert!(editor.drawing().is_none());
        assert_eq!(
            editor.drawing_point([Length::ZERO; 2]),
            Err(DrawDraftError::NotStarted),
            "a point needs a running draft"
        );

        let revision = session.project().revision();
        assert!(editor.start_drawing(DrawTool::Polyline));
        assert!(!editor.start_drawing(DrawTool::Polyline), "a running draft is not replaced");
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("first point");
        editor.drawing_point([Length::from_millimetres(600).expect("x"), Length::ZERO]).expect("second point");
        assert_eq!(editor.drawing().map(DrawDraft::len), Some(2));
        assert_eq!(session.project().revision(), revision, "the draft changes no document");
        assert!(!session.can_undo(), "the draft leaves no undo entry behind");

        assert!(editor.undo_drawing());
        assert_eq!(editor.drawing().map(DrawDraft::len), Some(1));
        assert!(editor.close_drawing().is_err(), "two points enclose nothing");

        editor.cancel_drawing();
        assert!(editor.drawing().is_none());
        assert_eq!(session.project().revision(), revision);
        assert!(!session.can_undo());
    }

    #[test]
    fn starting_a_drawing_leaves_the_other_pointer_modes() {
        let (project, panel) = contour_panel_project();
        let mut editor = Editor::new();
        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
        assert!(editor.start_drawing(DrawTool::Polyline));
        assert!(editor.contour_edit().is_none(), "a drawing owns the pointer");

        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
        assert!(editor.drawing().is_none(), "a contour edit takes the pointer back");
    }

    #[test]
    fn a_committed_drawing_is_one_path_object_and_one_undo_entry() {
        let (project, panel) = contour_panel_project();
        let root = project.root().id();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(panel));
        assert!(editor.start_drawing(DrawTool::Rectangle));
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("first corner");
        editor
            .drawing_point([Length::from_millimetres(600).expect("x"), Length::from_millimetres(400).expect("y")])
            .expect("opposite corner");

        let revision = session.project().revision();
        let drawn = editor.commit_drawing(&mut session, Placement::IDENTITY).expect("commit the rectangle");
        assert_eq!(drawn.tool, DrawTool::Rectangle);
        assert!(editor.drawing().is_none(), "the tool ends with the commit");
        assert_eq!(editor.selected_nodes(), [drawn.object], "the new node is selected");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));

        let path = session.project().object_path(drawn.object).expect("a path object");
        assert!(path.is_closed());
        assert_eq!(path.entries().len(), 4);
        assert_eq!(session.project().parent_of(drawn.object), Some(root), "the selection never decides the parent");

        session.undo().expect("one undo removes the drawing");
        assert!(session.project().object(drawn.object).is_none());
        assert!(!session.can_undo(), "the drawing was the only undo entry");
    }

    /// Creates an assembly under `parent` and returns its id.
    fn create_assembly_under(session: &mut Session, parent: Id, name: &str) -> Id {
        let params = serde_json::to_value(furnicraft_engine::AssemblyCreate {
            parent,
            name: name.to_owned(),
            placement: Placement::IDENTITY,
        })
        .expect("params");
        session
            .dispatch(furnicraft_engine::node_cmds::ASSEMBLY_CREATE_ID, &params)
            .expect("create the assembly")
            .created_id()
            .expect("the assembly id")
    }

    #[test]
    fn every_creation_tool_lands_at_the_root_beside_a_selected_nested_assembly() {
        let (project, panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let root = session.project().root().id();
        let outer = create_assembly_under(&mut session, root, "Outer");
        let inner = create_assembly_under(&mut session, outer, "Inner");
        let mut editor = Editor::new();

        editor.select_node(Some(inner));
        assert_eq!(creation_root(session.project()), root, "the scene root is the only default parent");
        assert!(editor.start_drawing(DrawTool::Line));
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
        editor.drawing_point([Length::from_millimetres(300).expect("x"), Length::ZERO]).expect("end");
        let drawn = editor.commit_drawing(&mut session, Placement::IDENTITY).expect("commit the line");
        assert_eq!(session.project().parent_of(drawn.object), Some(root), "a drawing lands at the root");

        editor.select_node(Some(inner));
        editor.create_panel(&mut session, root).expect("create the panel");
        let created_panel = editor.selected_node().expect("the new panel");
        assert_eq!(session.project().parent_of(created_panel), Some(root), "a panel lands at the root");

        editor.select_node(Some(inner));
        editor.create_assembly(&mut session, root).expect("create the assembly");
        let created_assembly = editor.selected_node().expect("the new assembly");
        assert_eq!(session.project().parent_of(created_assembly), Some(root), "an assembly lands at the root");

        editor.select_node(Some(inner));
        editor
            .arm_primitive(root, PrimitiveKind::Cube, Length::from_millimetres(100).expect("size"))
            .expect("arm the cube");
        let created_primitive = editor.place_primitive(&mut session, Placement::IDENTITY).expect("place the cube");
        assert_eq!(session.project().parent_of(created_primitive), Some(root), "a primitive lands at the root");

        editor.select_node(Some(inner));
        editor.create_camera(&mut session, root).expect("create the camera");
        let created_camera = editor.selected_node().expect("the new camera");
        assert_eq!(session.project().parent_of(created_camera), Some(root), "a camera lands at the root");

        editor.select_node(Some(inner));
        editor.create_light(&mut session, root).expect("create the light");
        let created_light = editor.selected_node().expect("the new light");
        assert_eq!(session.project().parent_of(created_light), Some(root), "a light lands at the root");

        for created in [created_panel, created_assembly, created_primitive, created_camera, created_light] {
            assert_ne!(session.project().parent_of(created), Some(inner), "the selected container receives nothing");
        }
        assert!(session.project().panel(panel).is_some(), "the rest of the document is untouched");
    }

    #[test]
    fn a_refused_drawing_keeps_the_draft_and_the_selection() {
        let (project, _panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(Id::new(1).expect("root")));
        assert!(editor.start_drawing(DrawTool::Rectangle));
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("one corner");

        let revision = session.project().revision();
        assert!(editor.commit_drawing(&mut session, Placement::IDENTITY).is_err(), "one point is no path");
        assert!(editor.drawing().is_some(), "the draft stays");
        assert_eq!(session.project().revision(), revision);
        assert!(!session.can_undo());
    }

    #[test]
    fn committing_without_a_draft_reports_it() {
        let (project, _panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        assert!(editor.commit_drawing(&mut session, Placement::IDENTITY).is_err());
    }

    #[test]
    fn an_open_polyline_commits_as_drawn() {
        let (project, _panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        assert!(editor.start_drawing(DrawTool::Polyline));
        editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
        editor.drawing_point([Length::from_millimetres(400).expect("x"), Length::ZERO]).expect("middle");
        editor
            .drawing_point([Length::from_millimetres(400).expect("x"), Length::from_millimetres(300).expect("y")])
            .expect("end");
        let drawn = editor.commit_drawing(&mut session, Placement::IDENTITY).expect("commit the chain");
        let path = session.project().object_path(drawn.object).expect("a path object");
        assert!(!path.is_closed());
        assert_eq!(path.entries().len(), 2);
    }

    #[test]
    fn arming_a_cube_waits_for_a_click_and_places_it_there() {
        let (project, panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        let root = Id::new(1).expect("root");
        let size = Length::from_millimetres(300).expect("size");
        editor.arm_primitive(root, PrimitiveKind::Cube, size).expect("arm the cube");
        assert_eq!(editor.pending_primitive(), Some((root, PrimitiveKind::Cube, size)));
        assert!(editor.drawing().is_none(), "an armed primitive leaves a drawing");

        let placement = Placement::IDENTITY
            .with_translation(
                Length::from_millimetres(100).expect("x"),
                Length::from_millimetres(50).expect("y"),
                Length::ZERO,
            )
            .expect("placement");
        let id = editor.place_primitive(&mut session, placement).expect("place the cube");
        assert!(editor.pending_primitive().is_none(), "one click places one cube");
        assert_eq!(editor.selected_node(), Some(id));
        let object = session.project().object(id).expect("object");
        assert_eq!(object.placement(), placement);
        assert_eq!(session.project().revision().get(), 1);
        assert!(session.project().panel(panel).is_some());

        editor.arm_primitive(root, PrimitiveKind::Sphere, size).expect("arm again");
        editor.cancel_primitive();
        assert!(editor.pending_primitive().is_none());
        assert_eq!(session.project().revision().get(), 1, "cancelling creates nothing");
    }

    #[test]
    fn a_cube_edge_must_be_positive() {
        let mut editor = Editor::new();
        assert!(editor.arm_primitive(Id::new(1).expect("root"), PrimitiveKind::Cylinder, Length::ZERO).is_err());
    }

    #[test]
    fn every_primitive_kind_can_be_armed() {
        let size = Length::from_millimetres(120).expect("size");
        for kind in [PrimitiveKind::Cube, PrimitiveKind::Cylinder, PrimitiveKind::Sphere] {
            let mut editor = Editor::new();
            let root = Id::new(1).expect("root");
            editor.arm_primitive(root, kind, size).expect("arm");
            assert_eq!(editor.pending_primitive(), Some((root, kind, size)));
            assert!(
                Session::with_catalog(sample_project(), sample_catalog())
                    .project()
                    .object(Id::new(5).expect("id"))
                    .is_none(),
                "arming alone creates nothing"
            );
        }
    }

    #[test]
    fn placing_a_cylinder_and_a_sphere_names_and_shapes_them() {
        let cases = [
            (PrimitiveKind::Cylinder, "Cylinder", DEFAULT_SEGMENTS * 2),
            (PrimitiveKind::Sphere, "Sphere", 2 + DEFAULT_SEGMENTS * (DEFAULT_BANDS - 1)),
        ];
        for (kind, name, vertices) in cases {
            let (project, _panel) = contour_panel_project();
            let mut session = Session::with_catalog(project, sample_catalog());
            let mut editor = Editor::new();
            let root = Id::new(1).expect("root");
            editor.arm_primitive(root, kind, Length::from_millimetres(100).expect("size")).expect("arm");
            let id = editor.place_primitive(&mut session, Placement::IDENTITY).expect("place");
            let object = session.project().object(id).expect("object");
            assert_eq!(object.name(), name);
            let stored = object.geometry().as_primitive().expect("the parameters are stored");
            assert_eq!(stored.kind(), kind);
            assert_eq!(stored.vertex_count() as u32, vertices, "the counts follow the parameters");
            assert!(object.geometry().as_mesh().is_none(), "the mesh is derived");
        }
    }

    /// A project whose root holds one closed path object.
    fn path_project(closed: bool) -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let path = Path::rectangle([millimetres(0), millimetres(0)], [millimetres(600), millimetres(400)])
            .expect("a rectangle path");
        let path = if closed {
            path
        } else {
            let segments = path.segments().take(2).collect::<Vec<_>>();
            Path::new(segments, false).expect("an open chain")
        };
        let id = project.add_object(root, "Drawing", Placement::IDENTITY, Geometry::Path(path)).expect("path object");
        (project, id)
    }

    #[test]
    fn the_extrude_action_turns_a_closed_path_into_a_body() {
        let (project, path) = path_project(true);
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(path));
        let height = Length::from_millimetres(18).expect("height");

        editor.extrude_path(&mut session, path, height).expect("extrude the path");
        let body = editor.selected_node().expect("the new body is selected");
        let object = session.project().object(body).expect("object");
        assert_eq!(object.name(), "Drawing body");
        let constructive = object.geometry().as_constructive().expect("a constructive body");
        assert_eq!(constructive.height(), Some(height));
        assert_eq!(session.project().parent_of(body), session.project().parent_of(path));
        assert!(session.project().object_path(path).is_ok(), "the drawing stays so it can be extruded again");
        let revision = session.project().revision();
        editor.extrude_path(&mut session, path, height).expect("extrude the path");
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
    }

    #[test]
    fn one_undo_removes_the_body_and_keeps_the_drawing() {
        let (project, path) = path_project(true);
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.select_node(Some(path));
        editor
            .extrude_path(&mut session, path, Length::from_millimetres(18).expect("height"))
            .expect("extrude the path");
        let body = editor.selected_node().expect("the new body is selected");

        editor.run_action(&mut session, TreeAction::Undo).expect("undo");
        assert!(session.project().object(body).is_none(), "undo removes the body");
        assert_eq!(editor.selected_node(), None, "undo drops the selection of a vanished object");
        assert!(session.project().object_path(path).is_ok(), "the drawing the body came from stays");
        assert!(!session.can_undo(), "one extrusion is one undo step");
    }

    #[test]
    fn an_open_path_is_refused_with_a_readable_reason() {
        let (project, path) = path_project(false);
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        let revision = session.project().revision();
        let error = editor
            .extrude_path(&mut session, path, Length::from_millimetres(18).expect("height"))
            .expect_err("an open chain is no ring");
        assert!(error.contains("open path"), "the message names the problem: {error}");
        assert_eq!(session.project().revision(), revision, "a refused extrusion changes nothing");
        assert!(!session.can_undo());
    }

    #[test]
    fn a_body_keeps_the_paths_placement_and_frame() {
        let (project, path) = path_project(true);
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        let placement = Placement::IDENTITY
            .with_translation(
                Length::from_millimetres(100).expect("x"),
                Length::from_millimetres(50).expect("y"),
                Length::ZERO,
            )
            .expect("placement");
        let params =
            crate::command::json(&furnicraft_engine::NodePlacementSet { node: path, placement }).expect("params");
        session.dispatch(furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID, &params).expect("move the drawing");
        editor.extrude_path(&mut session, path, Length::from_millimetres(18).expect("height")).expect("extrude");
        let body = editor.selected_node().expect("the body");
        assert_eq!(session.project().node(body).expect("body node").placement(), placement);
    }

    #[test]
    fn the_extrude_height_field_parses_a_positive_length() {
        let mut state = crate::toolbars::Toolbars::new();
        assert_eq!(state.extrude_height_text(), "18");
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(18).expect("height")));
        state.set_extrude_height_text("-2");
        assert!(state.extrude_height().is_err());
        state.set_extrude_height_text("thick");
        assert!(state.extrude_height().is_err());
    }

    #[test]
    fn the_primitive_size_field_parses_millimetres() {
        let mut state = crate::toolbars::Toolbars::new();
        assert_eq!(state.primitive_size_text(), "200");
        assert_eq!(state.primitive_size(), Ok(Length::from_millimetres(200).expect("length")));
        state.set_primitive_size_text("18.5");
        assert_eq!(state.primitive_size(), Ok(Length::from_micrometres(18_500)));
        state.set_primitive_size_text("0");
        assert!(state.primitive_size().is_err());
        state.set_primitive_size_text("tall");
        assert!(state.primitive_size().is_err());
    }

    #[test]
    fn the_toolbar_creates_the_cube_in_the_selected_container() {
        let (project, panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        let root = Id::new(1).expect("root");
        editor.select_node(Some(root));
        editor.arm_primitive(root, PrimitiveKind::Cube, Length::from_millimetres(200).expect("size")).expect("arm");
        editor.place_primitive(&mut session, Placement::IDENTITY).expect("cube");
        let cube = editor.selected_node().expect("the new cube is selected");
        assert!(session.project().object(cube).is_some());
        assert!(session.project().panel(panel).is_some());
        assert_eq!(session.project().parent_of(cube), Some(root));
        assert!(editor.initialized, "the fresh selection survives the next frame");
    }

    #[test]
    fn the_drawing_shortcuts_name_their_tool() {
        let cases = [
            (egui::Key::L, DrawTool::Line),
            (egui::Key::P, DrawTool::Polyline),
            (egui::Key::A, DrawTool::Arc),
            (egui::Key::R, DrawTool::Rectangle),
            (egui::Key::C, DrawTool::Circle),
        ];
        let mut editor = Editor::new();
        for (key, tool) in cases {
            assert_eq!(crate::toolbars::shortcut(tool), Some(key), "{} starts {}", key.name(), tool.label());
            assert!(editor.start_drawing(tool));
            assert_eq!(editor.drawing().map(DrawDraft::tool), Some(tool));
            editor.cancel_drawing();
        }
        assert!(editor.drawing().is_none());
    }

    #[test]
    fn entering_one_edit_mode_leaves_the_other() {
        let (mut project, cube) = cube_project();
        let width = Length::from_millimetres(700).expect("width");
        let height = Length::from_millimetres(500).expect("height");
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [width, Length::ZERO],
            [width, height],
            [Length::ZERO, height],
        ])
        .expect("ring");
        let root = project.root().id();
        let panel = project
            .add_panel_contour(root, "Top", &ring, None, sample_panel_thickness(), Placement::IDENTITY)
            .expect("contour panel");

        let mut editor = Editor::new();
        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        editor.select_node(Some(panel));
        assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
        assert!(editor.mesh_edit().is_none(), "contour editing replaces mesh editing");

        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        assert!(editor.contour_edit().is_none(), "mesh editing replaces contour editing");
    }

    #[test]
    fn vertex_selection_replaces_toggles_and_prunes_stale_indices() {
        let (project, cube) = cube_project();
        let mut editor = Editor::new();
        assert!(!editor.select_mesh_vertex(0, false), "not in mesh mode");
        editor.clear_mesh_vertices();

        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        assert!(editor.select_mesh_vertex(3, false));
        assert!(editor.select_mesh_vertex(4, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([3, 4].as_slice()));
        assert!(editor.select_mesh_vertex(3, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4].as_slice()));

        assert!(editor.select_mesh_vertex(8, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4, 8].as_slice()));
        editor.retain_mesh_edit(&project);
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4].as_slice()), "vertex 8 is outside the cube");

        editor.clear_mesh_vertices();
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([].as_slice()));
    }

    #[test]
    fn face_selection_replaces_toggles_and_prunes_stale_indices() {
        let (project, cube) = cube_project();
        let mut editor = Editor::new();
        assert!(!editor.select_mesh_face(0, false), "not in mesh mode");

        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));
        assert_eq!(editor.mesh_edit().map(MeshEdit::element), Some(MeshElement::Vertex));
        editor.set_mesh_element(MeshElement::Face);
        assert_eq!(editor.mesh_edit().map(MeshEdit::element), Some(MeshElement::Face));

        assert!(editor.select_mesh_face(2, false));
        assert!(editor.select_mesh_face(4, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([2, 4].as_slice()));
        assert!(editor.select_mesh_face(2, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4].as_slice()));

        assert!(editor.select_mesh_face(6, true));
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4, 6].as_slice()));
        editor.retain_mesh_edit(&project);
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4].as_slice()), "face 6 is outside the cube");

        editor.clear_mesh_faces();
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([].as_slice()));
    }

    #[test]
    fn switching_element_clears_the_selection_of_the_other_kind() {
        let (project, cube) = cube_project();
        let mut editor = Editor::new();
        editor.select_node(Some(cube));
        assert!(editor.start_mesh_edit(&project));

        assert!(editor.select_mesh_vertex(1, false));
        editor.set_mesh_element(MeshElement::Face);
        assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([].as_slice()));

        assert!(editor.select_mesh_face(0, false));
        editor.set_mesh_element(MeshElement::Vertex);
        assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([].as_slice()));
    }

    #[test]
    fn picking_replaces_the_tree_selection_and_an_empty_click_clears_it() {
        let mut editor = Editor::new();
        assert_eq!(editor.selected_node(), None);
        assert!(editor.selected_nodes().is_empty());
        let node = Id::new(2).expect("id");
        editor.select_node(Some(node));
        assert_eq!(editor.selected_node(), Some(node));
        assert_eq!(editor.selected_nodes(), [node], "the primary is the only selected node");
        editor.select_node(None);
        assert_eq!(editor.selected_node(), None);
        assert!(editor.selected_nodes().is_empty());
    }

    #[test]
    fn selecting_a_furniture_type_subtree_replaces_the_selection() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        set_furntype(&mut session, drawer, "12.03.02");
        set_furntype(&mut session, front, "12.03.01");

        let mut editor = Editor::new();
        editor
            .run_action(&mut session, TreeAction::SelectUnderType { furntype: "12.03".parse().expect("type") })
            .expect("select subtree");
        assert_eq!(editor.selected_nodes(), [drawer, front]);
        assert_eq!(editor.selected_node(), Some(front), "the last match is primary");

        assert!(
            editor
                .run_action(&mut session, TreeAction::SelectUnderType { furntype: "13".parse().expect("type") })
                .is_err()
        );
        assert_eq!(editor.selected_nodes(), [drawer, front], "a miss leaves the selection alone");

        editor.selection.clear();
        editor
            .run_action(&mut session, TreeAction::SelectUnderType { furntype: "12".parse().expect("type") })
            .expect("select group");
        assert_eq!(editor.selected_nodes(), [drawer, front]);
    }

    fn set_furntype(session: &mut Session, node: Id, furntype: &str) {
        let furntype: FurnType = furntype.parse().expect("valid furntype");
        let params =
            json(&NodePropertySet { node, key: SYSTEM_FURNTYPE.to_owned(), value: PropertyValue::furn_type(furntype) })
                .expect("params");
        dispatch(session, NODE_PROPERTY_SET_ID, &params).expect("set furntype");
    }

    fn grouped_project() -> (Project, Id, Id, Id) {
        let mut project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let drawer = Id::new(3).expect("drawer id");
        let group = project.group_nodes(&[shelf, drawer], "Group").expect("group siblings");
        (project, group, drawer, Id::new(4).expect("front id"))
    }

    #[test]
    fn a_viewport_click_selects_the_outermost_closed_container() {
        let (project, group, _drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.select_viewport_hit(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(group));
        assert!(!editor.open.contains(&group));

        let mut project = project;
        let extra = project
            .add_panel(
                project.root().id(),
                "Top",
                default_panel_size().expect("size"),
                None,
                Length::from_millimetres(18).expect("thickness"),
                Placement::IDENTITY,
            )
            .expect("add top panel");
        editor.select_viewport_hit(&project, Some(extra));
        assert_eq!(editor.selected_node(), Some(extra));

        editor.select_viewport_hit(&project, None);
        assert_eq!(editor.selected_node(), None);

        editor.select_viewport_hit(&project, Some(Id::new(999).expect("id")));
        assert_eq!(editor.selected_node(), None);
    }

    #[test]
    fn double_click_enters_one_level_and_reveals_the_child_under_the_pointer() {
        let (project, group, drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(drawer));
        assert!(editor.open.contains(&group));

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front));
        assert!(editor.open.contains(&drawer));

        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front), "a panel has no level to enter");
    }

    #[test]
    fn escape_closes_one_level_and_returns_to_the_parent_container() {
        let (project, group, drawer, front) = grouped_project();
        let mut editor = Editor::new();

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), None, "nothing is open yet");

        editor.enter_viewport_level(&project, Some(front));
        editor.enter_viewport_level(&project, Some(front));
        assert_eq!(editor.selected_node(), Some(front));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(drawer));
        assert!(!editor.open.contains(&drawer));
        assert!(editor.open.contains(&group));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(group));
        assert!(!editor.open.contains(&group));

        editor.exit_level(&project);
        assert_eq!(editor.selected_node(), Some(group), "the top level stays selected");
    }

    #[test]
    fn creating_a_cube_dispatches_the_primitive_command_and_selects_it() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let mut editor = Editor::new();
        let root = session.project().root().id();

        editor.arm_primitive(root, PrimitiveKind::Cube, Length::from_millimetres(200).expect("size")).expect("arm");
        editor.place_primitive(&mut session, Placement::IDENTITY).expect("create cube");

        let id = editor.selected_node().expect("the new object is selected");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), "Cube");
        let primitive = object.geometry().as_primitive().expect("the parameters are stored");
        assert_eq!(primitive.mesh_counts(), (8, 6), "a cube keeps six faces");
        assert_eq!(session.project().revision().get(), 1, "one command is one revision");

        editor.run_action(&mut session, TreeAction::Undo).expect("undo");
        assert_eq!(editor.selected_node(), None, "undo drops the selection of a vanished object");
        assert!(session.project().object(id).is_none());
    }

    #[test]
    fn the_toolbar_marks_the_tool_that_owns_the_next_click() {
        let (project, _panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        let root = session.project().root().id();
        let ctx = egui::Context::default();

        let mut fill = None;
        let idle = ctx.run_ui(Default::default(), |ui| {
            fill = Some(ui.visuals().selection.bg_fill);
            editor.show_toolbars(ui, &mut session).expect("toolbars");
        });
        let fill = fill.expect("the strip ran");
        assert_eq!(pressed_fill(&idle.shapes, fill), 0, "an idle editor presses no tool");
        idle.drop_without_applying_deltas();

        assert!(editor.start_drawing(DrawTool::Line));
        let drawing = ctx.run_ui(Default::default(), |ui| {
            editor.show_toolbars(ui, &mut session).expect("toolbars");
        });
        assert_eq!(pressed_fill(&drawing.shapes, fill), 1, "the running draft presses its own tool");
        drawing.drop_without_applying_deltas();

        editor.cancel_drawing();
        let size = Length::from_millimetres(crate::toolbars::DEFAULT_PRIMITIVE_SIZE_MM).expect("size");
        editor.arm_primitive(root, PrimitiveKind::Cube, size).expect("arm");
        let primitive = ctx.run_ui(Default::default(), |ui| {
            editor.show_toolbars(ui, &mut session).expect("toolbars");
        });
        assert_eq!(pressed_fill(&primitive.shapes, fill), 1, "the waiting primitive presses its own button");
        primitive.drop_without_applying_deltas();
    }

    /// Counts the painted rects filled with `fill`, the selection fill of a pressed button.
    fn pressed_fill(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> usize {
        fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut usize) {
            match shape {
                egui::Shape::Rect(rect) if rect.fill == fill => *out += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, fill, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, fill, &mut out);
        }
        out
    }

    #[test]
    fn creating_a_camera_or_a_light_selects_it_and_one_undo_removes_it() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let mut editor = Editor::new();
        let root = session.project().root().id();

        editor.create_camera(&mut session, root).expect("create camera");
        let camera = editor.selected_node().expect("the camera is selected");
        assert_eq!(session.project().camera(camera).expect("camera").name(), "Camera");
        assert_eq!(session.project().revision().get(), 1);

        editor.create_light(&mut session, root).expect("create light");
        let light = editor.selected_node().expect("the light is selected");
        assert_eq!(session.project().light(light).expect("light").name(), "Light");
        assert_eq!(session.project().revision().get(), 2);

        editor.run_action(&mut session, TreeAction::Undo).expect("undo");
        assert!(session.project().light(light).is_none());
        assert!(editor.selected_node().is_none());
    }

    #[test]
    fn a_created_camera_and_light_stay_selected_on_the_next_frame() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let mut editor = Editor::new();
        let root = session.project().root().id();

        editor.create_camera(&mut session, root).expect("create camera");
        let camera = editor.selected_node().expect("the camera is selected");
        // The tree retains the selection at the start of every frame; a scene node must survive it.
        editor.selection.retain_existing(session.project());
        assert_eq!(editor.selected_node(), Some(camera));

        editor.create_light(&mut session, root).expect("create light");
        let light = editor.selected_node().expect("the light is selected");
        editor.selection.retain_existing(session.project());
        assert_eq!(editor.selected_node(), Some(light));
    }
}
