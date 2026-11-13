//! Creation: the armed container, the armed primitive, the box tool and the five `create` commands.
//!
//! Every creation lands in the container armed here, or at the scene root when nothing is armed
//! (ADR 0056). A cylinder or a sphere is armed rather than created, because its placement is the
//! next work-plane click; the arm is resolved at the click, so clearing or moving it changes where
//! the primitive lands. A box is drawn instead: the ribbon starts a draft whose base rectangle and
//! pulled height become one `object.primitive.create` (ADR 0101). A create command's node becomes
//! the selection.
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
use furnicraft_model::{
    CameraProjection, ClassCode, Contour, Envelope, Id, LightKind, MaterialId, PanelNominalSize, Placement, Primitive,
    Project,
};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use super::Editor;
use crate::box_tool::{BoxDraft, BoxDraftError, BoxOwner};
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
        self.create_panel_sized(session, parent, "Panel".to_owned(), nominal_size, None, Placement::IDENTITY, None)
    }

    /// Dispatches `panel.create` for a part of exactly this size, placement and class.
    ///
    /// The empty-space tool measures the void and hands the answer here, so the panel that is
    /// created is exactly the ghost the maker saw; the class travels with the same command, so the
    /// click is one undo entry rather than a create followed by an assignment. The material stays
    /// the catalogue's first, as it is for every created panel.
    ///
    /// A ring makes the part a contour panel whose outline is that ring, which is how a shelf takes
    /// the trapezoid a splayed niche reserves (ADR 0042, ADR 0099); a rectangle panel carries none.
    #[allow(clippy::too_many_arguments)]
    pub fn create_panel_sized(
        &mut self,
        session: &mut Session,
        parent: Id,
        name: String,
        nominal_size: PanelNominalSize,
        outline: Option<Contour>,
        placement: Placement,
        class: Option<ClassCode>,
    ) -> Result<(), String> {
        let material = Self::panel_material(session)?;
        let params = json(&PanelCreate { parent, name, nominal_size, material, placement, class, outline })?;
        let outcome = dispatch(session, PANEL_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
    }

    /// The material every created panel takes: the catalogue's first panel material.
    fn panel_material(session: &Session) -> Result<MaterialId, String> {
        session
            .catalog()
            .panel_materials()
            .first()
            .map(|material| material.id())
            .ok_or_else(|| "Add a panel material to the catalogue first.".to_owned())
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

    /// Arms a primitive tool: a click places a cylinder or a sphere, a box starts its base.
    ///
    /// The panel asks for a click instead of creating at the origin: the tool is a placement, and a
    /// stale primitive at the world origin is never what the user wanted. Arming leaves the other
    /// pointer modes, because the next click creates rather than selects. The parent is resolved at
    /// the click, so clearing or moving the arm changes where the primitive lands. A box has three
    /// independent sides, so one extent cannot describe it: the tool draws its base and pulls its
    /// height instead (ADR 0101).
    pub(super) fn arm_primitive(&mut self, kind: PrimitiveKind, size: Length) -> Result<(), String> {
        if kind == PrimitiveKind::Cuboid {
            self.arm_box(BoxOwner::Cuboid);
            return Ok(());
        }
        if size.micrometres() <= 0 {
            return Err("The primitive size must be positive.".to_owned());
        }
        self.leave_pointer_modes();
        self.pending_primitive = Some(PendingPrimitive { kind, size });
        Ok(())
    }

    /// Arms the box tool for one owner: the next clicks draw its base rectangle and pull its height.
    ///
    /// A box has three independent sides, so it is drawn rather than placed by one click from one
    /// extent (ADR 0101). The owner is the field the ✓ will write — a cuboid's primitive or a
    /// product's envelope — so one gesture serves both and the shell reads the draft, not the tool.
    /// Arming drops the other pointer modes but writes nothing.
    pub fn arm_box(&mut self, owner: BoxOwner) {
        self.leave_pointer_modes();
        self.box_draft = Some(BoxDraft::new(owner));
    }

    /// Drops every pointer mode a creation tool replaces.
    fn leave_pointer_modes(&mut self) {
        self.mesh_edit = None;
        self.contour_edit = None;
        self.drawing = None;
        self.fill = None;
        self.pending_primitive = None;
        self.box_draft = None;
    }

    /// The primitive waiting for a work-plane click: its kind and size.
    pub fn pending_primitive(&self) -> Option<(PrimitiveKind, Length)> {
        self.pending_primitive.map(|pending| (pending.kind, pending.size))
    }

    /// The box being drawn, when the box tool is armed or its height is being pulled.
    pub fn box_draft(&self) -> Option<&BoxDraft> {
        self.box_draft.as_ref()
    }

    /// The owner the box tool would write, while its draft lives; this is what arms the ribbon cell.
    pub fn box_tool(&self) -> Option<BoxOwner> {
        self.box_draft.as_ref().map(BoxDraft::owner)
    }

    /// The corner the live base rectangle runs to.
    pub fn set_box_corner(&mut self, corner: [Length; 2]) {
        if let Some(draft) = self.box_draft.as_mut() {
            draft.set_live(corner);
        }
    }

    /// A click on the work plane: anchors the base, then fixes it, then does nothing.
    pub fn click_box(&mut self, corner: [Length; 2]) -> Result<bool, BoxDraftError> {
        self.box_draft.as_mut().ok_or(BoxDraftError::NoBase)?.click(corner)
    }

    /// Sets the height the box pull shows.
    pub fn pull_box_height(&mut self, height: Length) -> Result<(), BoxDraftError> {
        self.box_draft.as_mut().ok_or(BoxDraftError::NoBase)?.set_height(height)
    }

    /// Drops a box draft without creating anything.
    pub fn cancel_box(&mut self) {
        self.box_draft = None;
    }

    /// Drops an armed primitive without creating anything.
    pub fn cancel_primitive(&mut self) {
        self.pending_primitive = None;
        self.box_draft = None;
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

    /// Creates the box the draft describes through one command, chosen by its owner.
    ///
    /// `placement` is the work plane's *world* frame with its origin on the base rectangle's
    /// minimum corner, so the box's own zero is that corner; the command stores the placement
    /// relative to the parent. A solid goes to `object.primitive.create`; a frame goes to
    /// `product.create` at the scene root with the placeholder class, exactly as the card's create
    /// does, and is opened for its parts (ADR 0098, ADR 0101). The whole gesture is one undo entry.
    pub fn commit_box(&mut self, session: &mut Session, placement: Placement, lang: Lang) -> Result<Id, String> {
        let Some(draft) = self.box_draft else {
            return Err(BoxDraftError::NoBase.to_string());
        };
        let Some([width, depth, height]) = draft.sizes() else {
            return Err(BoxDraftError::NoBase.to_string());
        };
        match draft.owner() {
            BoxOwner::Cuboid => {
                let primitive = Primitive::cuboid(width, depth, height).map_err(|error| error.to_string())?;
                let parent = self.creation_target(session.project());
                let placement =
                    session.project().placement_in_parent(parent, placement).map_err(|error| error.to_string())?;
                self.create_primitive_node(session, parent, primitive, placement)?;
            }
            BoxOwner::Product => {
                // A frame is a top-level piece: its parent is the root whatever container is armed.
                let parent = session.project().root().id();
                let placement =
                    session.project().placement_in_parent(parent, placement).map_err(|error| error.to_string())?;
                self.create_drawn_product(session, parent, [width, depth, height], placement, lang)?;
            }
        }
        self.box_draft = None;
        self.selection.primary().ok_or_else(|| "The box command created no node.".to_owned())
    }

    /// Dispatches `product.create` for a frame drawn in the viewport, then opens it.
    ///
    /// The class is the registry's general-product placeholder, because a drawn gesture names no
    /// type; the card is the entry that does. The name is that class's label in the current
    /// language, the same name the card gives when its class is left alone, so the card and the
    /// gesture produce the same document. Opening the new frame is a session view operation that
    /// follows the command, exactly as it does from the card (ADR 0098, ADR 0100).
    fn create_drawn_product(
        &mut self,
        session: &mut Session,
        parent: Id,
        sizes: [Length; 3],
        placement: Placement,
        lang: Lang,
    ) -> Result<(), String> {
        let [width, depth, height] = sizes;
        let envelope = Envelope::new(width, depth, height).map_err(|error| error.to_string())?;
        let class = ProductCreate::PLACEHOLDER_CLASS;
        let name = class.label().map_or_else(|| Message::CmdCreateProduct.text(lang), |label| label.text(lang));
        let params = json(&ProductCreate { parent, name: name.to_owned(), class: Some(class), envelope, placement })?;
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

    /// Dispatches `object.primitive.create` for one validated primitive at `placement`.
    fn create_primitive_node(
        &mut self,
        session: &mut Session,
        parent: Id,
        primitive: Primitive,
        placement: Placement,
    ) -> Result<(), String> {
        let name = primitive.kind().label();
        let params = json(&ObjectPrimitiveCreate { parent, name: name.to_owned(), primitive, placement })?;
        let outcome = dispatch(session, OBJECT_PRIMITIVE_CREATE_ID, &params)?;
        self.select_created(outcome.created_id());
        Ok(())
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
        let primitive = Primitive::from_kind(kind, size).map_err(|error| error.to_string())?;
        self.create_primitive_node(session, parent, primitive, placement)
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
