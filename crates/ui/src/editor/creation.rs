//! Creation: the armed container, the armed primitive and the five `create` commands.
//!
//! Every creation lands in the container armed here, or at the scene root when nothing is armed
//! (ADR 0056). A primitive is armed rather than created, because its placement is the next
//! work-plane click; the arm is resolved at the click, so clearing or moving it changes where the
//! primitive lands. A create command's node becomes the selection.
//!
//! Refuses an arm for the root or for a node that cannot own children, a primitive size that is not
//! positive, and a panel creation without a panel material in the catalogue; each reason is a
//! sentence for the user. An arm whose container left the document is dropped by
//! `retain_creation_parent`.
//!
//! Changes when: a creation tool, its command or the rule that places it changes.

use furnicraft_engine::node_cmds::ASSEMBLY_CREATE_ID;
use furnicraft_engine::object_cmds::OBJECT_PRIMITIVE_CREATE_ID;
use furnicraft_engine::panel_cmds::CREATE_ID as PANEL_CREATE_ID;
use furnicraft_engine::product_cmds::{PRODUCT_CREATE_ID, ProductCreate};
use furnicraft_engine::scene_cmds::{CAMERA_CREATE_ID, LIGHT_CREATE_ID};
use furnicraft_engine::{
    AssemblyCreate, CameraCreate, LightCreate, ObjectPrimitiveCreate, PanelCreate, PrimitiveKind, Session,
};
use furnicraft_model::{CameraProjection, ClassCode, Id, LightKind, PanelNominalSize, Placement, Project};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use super::Editor;
use crate::command::{dispatch, json};
use crate::product_card::ProductDraft;
use crate::target::creation_target;

/// A primitive waiting for its work-plane click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingPrimitive {
    /// Which primitive to build.
    pub(super) kind: PrimitiveKind,
    /// Full extent: an edge, a diameter and a height, or a diameter.
    pub(super) size: Length,
}

impl Editor {
    /// The container armed to receive the next created nodes, if any.
    pub fn creation_parent(&self) -> Option<Id> {
        self.creation_parent
    }

    /// Arms `parent` to receive the next created nodes.
    ///
    /// The scene root is the default and cannot be armed, because clearing the arm would return to
    /// the same place; a node that cannot own children cannot be armed either. Returns whether the
    /// arm took.
    pub fn arm_creation(&mut self, project: &Project, parent: Id) -> bool {
        if parent == project.root().id() || !project.is_container(parent) {
            return false;
        }
        self.creation_parent = Some(parent);
        true
    }

    /// Drops the arm; the next creation lands at the scene root again.
    pub fn clear_creation_parent(&mut self) {
        self.creation_parent = None;
    }

    /// Opens a product as the session's working product and arms it for the next creation.
    ///
    /// Opening is a session view operation, never a document edit (ADR 0098); the arm is the
    /// editor's own view state, so the next panel, assembly or fitting lands inside the product
    /// without another gesture. Returns whether the id was a product at all.
    pub fn open_product(&mut self, session: &mut Session, id: Id) -> bool {
        if !session.open_product(id) {
            return false;
        }
        self.arm_creation(session.project(), id);
        true
    }

    /// Leaves the open product and drops the arm, so nothing is armed to receive a part.
    pub fn close_product(&mut self, session: &mut Session) {
        session.close_product();
        self.clear_creation_parent();
        self.cancel_fill();
    }

    /// Drops an arm whose container no longer exists.
    ///
    /// Deleting the container, deleting an ancestor, or undoing the container's creation all take
    /// the armed id out of the document; the editor must not keep pointing at a dead node.
    pub fn retain_creation_parent(&mut self, project: &Project) {
        let root = project.root().id();
        if self.creation_parent.is_some_and(|id| id == root || !project.is_container(id)) {
            self.creation_parent = None;
        }
    }

    /// The container every creation tool lands in: the armed one while it exists, else the root.
    pub fn creation_target(&self, project: &Project) -> Id {
        creation_target(project, self.creation_parent)
    }

    /// The armed container's name, for the strip chip; `None` when nothing is armed.
    pub fn creation_parent_name<'a>(&self, project: &'a Project) -> Option<&'a str> {
        let armed = self.creation_parent?;
        if armed == project.root().id() || !project.is_container(armed) {
            return None;
        }
        project.container(armed).map(|container| container.name())
    }

    /// Dispatches `panel.create` with the catalogue's first material and the default size.
    pub(super) fn create_panel(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let nominal_size = PanelCreate::default_nominal_size().map_err(|error| error.to_string())?;
        self.create_panel_sized(session, parent, "Panel".to_owned(), nominal_size, Placement::IDENTITY, None)
    }

    /// Dispatches `panel.create` for a part of exactly this size, placement and class.
    ///
    /// The empty-space tool measures the void and hands the answer here, so the panel that is
    /// created is exactly the ghost the maker saw; the class travels with the same command, so the
    /// click is one undo entry rather than a create followed by an assignment. The material stays
    /// the catalogue's first, as it is for every created panel.
    pub fn create_panel_sized(
        &mut self,
        session: &mut Session,
        parent: Id,
        name: String,
        nominal_size: PanelNominalSize,
        placement: Placement,
        class: Option<ClassCode>,
    ) -> Result<(), String> {
        let material = session
            .catalog()
            .panel_materials()
            .first()
            .ok_or_else(|| "Add a panel material to the catalogue first.".to_owned())?
            .id();
        let params = json(&PanelCreate { parent, name, nominal_size, material, placement, class })?;
        let outcome = dispatch(session, PANEL_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Creates the product frame the card describes and opens it, in one user action.
    ///
    /// The command is `product.create`; opening the new frame is a session view operation that
    /// follows it (ADR 0098, ADR 0100). The frame lands at the deterministic product placement —
    /// the origin in an empty scene, else beside the nearest piece and clear of every envelope —
    /// and the container is armed, so the next part is born inside it without another gesture.
    /// A frame is a top-level piece, so its parent is the scene root.
    pub fn create_product(&mut self, session: &mut Session, draft: ProductDraft, lang: Lang) -> Result<(), String> {
        let project = session.project();
        let root = project.root().id();
        let placement = project.product_placement(draft.envelope);
        let name = draft
            .class
            .label()
            .map(|label| label.text(lang))
            .unwrap_or(Message::CmdCreateProduct.text(lang))
            .to_owned();
        let params =
            json(&ProductCreate { parent: root, name, class: Some(draft.class), envelope: draft.envelope, placement })?;
        let outcome = dispatch(session, PRODUCT_CREATE_ID, &params)?;
        let Some(id) = outcome.created_id() else {
            return Ok(());
        };
        session.open_product(id);
        self.arm_creation(session.project(), id);
        self.selection.replace(id);
        self.initialized = true;
        Ok(())
    }

    /// Dispatches `assembly.create`.
    pub(super) fn create_assembly(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
        let params = json(&AssemblyCreate { parent, name: "Assembly".to_owned(), placement: Placement::IDENTITY })?;
        let outcome = dispatch(session, ASSEMBLY_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// Arms a primitive that the next work-plane click places.
    ///
    /// The panel asks for a click instead of creating at the origin: the tool is a placement, and a
    /// stale primitive at the world origin is never what the user wanted. Arming leaves the other
    /// pointer modes, because the next click creates rather than selects. The parent is resolved at
    /// the click, so clearing or moving the arm changes where the primitive lands.
    pub(super) fn arm_primitive(&mut self, kind: PrimitiveKind, size: Length) -> Result<(), String> {
        if size.micrometres() <= 0 {
            return Err("The primitive size must be positive.".to_owned());
        }
        self.mesh_edit = None;
        self.contour_edit = None;
        self.drawing = None;
        self.fill = None;
        self.pending_primitive = Some(PendingPrimitive { kind, size });
        Ok(())
    }

    /// The primitive waiting for a work-plane click: its kind and size.
    pub fn pending_primitive(&self) -> Option<(PrimitiveKind, Length)> {
        self.pending_primitive.map(|pending| (pending.kind, pending.size))
    }

    /// Drops an armed primitive without creating anything.
    pub fn cancel_primitive(&mut self) {
        self.pending_primitive = None;
    }

    /// Creates the armed primitive in the creation target and selects it.
    ///
    /// `placement` is the work plane's *world* frame, the one the shell can measure; the command
    /// stores a placement relative to the parent, so this converts it with the parent's inverse
    /// first, exactly like [`Self::commit_drawing`]. A primitive's mesh is centred on its local
    /// zero with a zero pivot, so the stored translation is the clicked point in the parent frame.
    pub fn place_primitive(&mut self, session: &mut Session, placement: Placement) -> Result<Id, String> {
        let pending = self.pending_primitive.ok_or_else(|| "No primitive is waiting for a click.".to_owned())?;
        let parent = self.creation_target(session.project());
        let placement = session.project().placement_in_parent(parent, placement).map_err(|error| error.to_string())?;
        self.create_primitive(session, parent, pending.kind, pending.size, placement)?;
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

    /// Dispatches `camera.create`.
    pub(super) fn create_camera(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
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
    pub(super) fn create_light(&mut self, session: &mut Session, parent: Id) -> Result<(), String> {
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
}
