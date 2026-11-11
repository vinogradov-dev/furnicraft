//! The scene tree.
//!
//! The tree mirrors the document hierarchy: containers contain panels and other containers. A panel
//! is an atomic leaf, so it is never shown with children and can never be ungrouped. A container is
//! an ordinary group or a furniture component; the component badge and its furniture type come from
//! the node's properties. Selection is view state and is shared with the viewport.
//!
//! Every row carries the session's eye and padlock. They are view state keyed by node `Id` (ADR
//! 0065), so the tree writes them directly instead of dispatching a command; a hidden subtree is
//! skipped by both the scene builder and picking, and a locked node is refused a drag and a
//! destructive edit by the shell.

use egui::collapsing_header::CollapsingState;
use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Ui, Vec2};
use furnicraft_engine::Session;
use furnicraft_model::{Container, Id, Node, Panel, Project};

use crate::icons;
use crate::theme;
use furnicraft_text::Message;

use crate::view_flags::ViewFlags;

/// The glyph size of a row's visibility and lock toggles, in points.
const TOGGLE_GLYPH: f32 = 16.0;

/// The cell a toggle owns at the right of a row.
///
/// Wide enough for the caption of an asset that is still a placeholder, so the four labels do not
/// resize the row as they swap.
const TOGGLE_CELL: f32 = 34.0;

/// The face size of a toggle whose asset is still an empty placeholder.
const TOGGLE_TEXT_SIZE: f32 = 9.0;

/// The selected document nodes, in click order. The last one is the primary selection.
#[derive(Debug, Default)]
pub(crate) struct Selection {
    ids: Vec<Id>,
}

impl Selection {
    /// The node whose card is shown and whose context actions are enabled.
    pub(crate) fn primary(&self) -> Option<Id> {
        self.ids.last().copied()
    }

    /// All selected node ids, oldest first.
    pub(crate) fn ids(&self) -> &[Id] {
        &self.ids
    }

    /// Whether the node is selected.
    pub(crate) fn contains(&self, id: Id) -> bool {
        self.ids.contains(&id)
    }

    /// Replaces the selection with one node.
    pub(crate) fn replace(&mut self, id: Id) {
        self.ids.clear();
        self.ids.push(id);
    }

    /// Replaces the selection with several nodes, in the given order.
    #[cfg(test)]
    pub(crate) fn replace_all(&mut self, ids: Vec<Id>) {
        self.ids = ids;
    }

    /// Empties the selection.
    pub(crate) fn clear(&mut self) {
        self.ids.clear();
    }

    /// Adds the node to the selection, or removes it when it is already selected.
    pub(crate) fn toggle(&mut self, id: Id) {
        if let Some(index) = self.ids.iter().position(|selected| *selected == id) {
            self.ids.remove(index);
        } else {
            self.ids.push(id);
        }
    }

    /// Drops ids that no longer exist, for example after undo or delete.
    pub(crate) fn retain_existing(&mut self, project: &Project) {
        self.ids.retain(|id| project.contains_node(*id));
    }

    /// Selects the first panel in depth-first order.
    pub(crate) fn select_first_panel(&mut self, project: &Project) {
        if let Some(panel) = first_panel(project.root()) {
            self.replace(panel);
        }
    }
}

/// One structural or view action the tree asks the editor to run.
///
/// The tree shows and restructures nodes; it never creates one. Creation is a `ToolAction` from the
/// ribbon, so there is exactly one place a new node starts. `AddChild` only arms a container for the
/// next creation, which is a view-state decision, not a second creation path (ADR 0056).
///
/// The four view entries are session state, not document edits (ADR 0065): the editor applies them
/// without a command, and they leave no undo entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeAction {
    AddChild { parent: Id },
    Duplicate { node: Id },
    Delete { node: Id },
    Group { nodes: Vec<Id> },
    Ungroup { node: Id },
    ConvertToGroup { node: Id },
    SetHidden { node: Id, hidden: bool },
    SetLocked { node: Id, locked: bool },
    Isolate { node: Id },
    ShowAll,
}

/// Draws the container tree, returning the actions the user requested.
///
/// The tree owns no toolbar and no heading: the surface it sits on draws its own header, and every
/// node action is reached from the row's context menu. The one multi-node action, grouping, is
/// offered there while two or more nodes are selected.
pub(crate) fn show(
    ui: &mut Ui,
    session: &Session,
    selection: &mut Selection,
    flags: &mut ViewFlags,
) -> Vec<TreeAction> {
    let project = session.project();
    let mut actions = Vec::new();
    let group = (selection.ids().len() >= 2 && project.validate_group(selection.ids()).is_ok())
        .then(|| selection.ids().to_vec());
    let group = group.as_deref().unwrap_or(&[]);
    show_container(ui, project, project.root(), selection, flags, &mut actions, group, 0);
    actions
}

fn container_label(ui: &Ui, project: &Project, container: &Container) -> RichText {
    let name = container.name();
    if container.id() == project.root().id() {
        return RichText::new(name).strong();
    }
    if !container.is_component() {
        return RichText::new(name);
    }
    let badge = match project.furntype(container.id()) {
        Some(furntype) if !furntype.is_unclassified() => furntype.to_string(),
        _ => Message::TreeBadgeComponent.text(crate::language(ui.ctx())).to_owned(),
    };
    RichText::new(format!("{name}  [{badge}]"))
}

#[allow(clippy::too_many_arguments)]
fn show_container(
    ui: &mut Ui,
    project: &Project,
    container: &Container,
    selection: &mut Selection,
    flags: &mut ViewFlags,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
    depth: u32,
) {
    let id = container.id();
    let is_root = id == project.root().id();
    let is_component = container.is_component();
    let state_id = ui.make_persistent_id(("scene_container", id.get()));
    let state = CollapsingState::load_with_default_open(ui.ctx(), state_id, depth < 2);
    let label = container_label(ui, project, container);
    let (_toggle, header, _) =
        state.show_header(ui, |ui| row(ui, id, label, selection.contains(id), flags, !is_root)).body(|ui| {
            for child in container.children() {
                show_node(ui, project, child, selection, flags, actions, group, depth.saturating_add(1));
            }
        });
    if header.inner.clicked() {
        select(ui, selection, id);
    }
    // The row drew its own toggles a moment ago, so the menu captions read the flags as they are
    // now instead of a copy taken before the row.
    let hidden = flags.is_hidden(id);
    let locked = flags.is_locked(id);
    header.inner.context_menu(|ui| {
        container_menu(ui, id, is_root, is_component, hidden, locked, group, actions);
    });
}

#[allow(clippy::too_many_arguments)]
fn show_node(
    ui: &mut Ui,
    project: &Project,
    node: &Node,
    selection: &mut Selection,
    flags: &mut ViewFlags,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
    depth: u32,
) {
    match node {
        Node::Container(container) => {
            show_container(ui, project, container, selection, flags, actions, group, depth);
        }
        Node::Panel(panel) => show_panel(ui, panel, selection, flags, actions, group),
        Node::Object(object) => show_object(ui, object, selection, flags, actions, group),
        Node::Camera(camera) => {
            show_leaf(ui, camera.id(), camera.name(), Message::TreeBadgeCamera, selection, flags, actions, group);
        }
        Node::Light(light) => {
            show_leaf(ui, light.id(), light.name(), Message::TreeBadgeLight, selection, flags, actions, group);
        }
    }
}

/// A camera or a light row: a named leaf with a kind tag and the usual duplicate/delete actions.
#[allow(clippy::too_many_arguments)]
fn show_leaf(
    ui: &mut Ui,
    id: Id,
    name: &str,
    kind: Message,
    selection: &mut Selection,
    flags: &mut ViewFlags,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
) {
    let label = RichText::new(format!("{name}  [{}]", kind.text(crate::language(ui.ctx()))));
    let hidden = flags.is_hidden(id);
    let locked = flags.is_locked(id);
    let response = row(ui, id, label, selection.contains(id), flags, true);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, hidden, locked, group, actions));
}

fn show_object(
    ui: &mut Ui,
    object: &furnicraft_model::Object,
    selection: &mut Selection,
    flags: &mut ViewFlags,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
) {
    let id = object.id();
    let lang = crate::language(ui.ctx());
    let kind = match object.geometry() {
        furnicraft_model::Geometry::Mesh(_) => Message::TreeBadgeMesh,
        furnicraft_model::Geometry::Primitive(primitive) => match primitive.kind() {
            furnicraft_model::PrimitiveKind::Cube => Message::TreeBadgeCube,
            furnicraft_model::PrimitiveKind::Cylinder => Message::TreeBadgeCylinder,
            furnicraft_model::PrimitiveKind::Sphere => Message::TreeBadgeSphere,
        },
        furnicraft_model::Geometry::Constructive(_) => Message::TreeBadgeContour,
        furnicraft_model::Geometry::Path(_) => Message::TreeBadgePath,
    };
    let label = RichText::new(format!("{}  [{}]", object.name(), kind.text(lang)));
    let hidden = flags.is_hidden(id);
    let locked = flags.is_locked(id);
    let response = row(ui, id, label, selection.contains(id), flags, true);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| object_menu(ui, id, hidden, locked, group, actions));
}

fn object_menu(ui: &mut Ui, id: Id, hidden: bool, locked: bool, group: &[Id], actions: &mut Vec<TreeAction>) {
    group_entry(ui, group, actions);
    view_menu(ui, id, hidden, locked, false, actions);
    if ui.button(Message::CmdDuplicate.text(crate::language(ui.ctx()))).clicked() {
        actions.push(TreeAction::Duplicate { node: id });
        ui.close();
    }
    if ui.button(Message::CmdDelete.text(crate::language(ui.ctx()))).clicked() {
        actions.push(TreeAction::Delete { node: id });
        ui.close();
    }
}

fn show_panel(
    ui: &mut Ui,
    panel: &Panel,
    selection: &mut Selection,
    flags: &mut ViewFlags,
    actions: &mut Vec<TreeAction>,
    group: &[Id],
) {
    let id = panel.id();
    let hidden = flags.is_hidden(id);
    let locked = flags.is_locked(id);
    let response = row(ui, id, RichText::new(panel.name()), selection.contains(id), flags, true);
    if response.clicked() {
        select(ui, selection, id);
    }
    response.context_menu(|ui| panel_menu(ui, id, hidden, locked, group, actions));
}

/// The one multi-node entry: grouping the current selection.
///
/// The tree toolbar that used to carry it is gone, so a group is created from any row's context
/// menu while two or more nodes are selected. The menu only offers it when the selection is valid.
fn group_entry(ui: &mut Ui, group: &[Id], actions: &mut Vec<TreeAction>) {
    if group.is_empty() {
        return;
    }
    if ui.button(Message::TreeGroupSelection.text(crate::language(ui.ctx()))).clicked() {
        actions.push(TreeAction::Group { nodes: group.to_vec() });
        ui.close();
    }
    ui.separator();
}

fn select(ui: &Ui, selection: &mut Selection, id: Id) {
    let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.shift);
    if additive {
        selection.toggle(id);
    } else {
        selection.replace(id);
    }
}

#[allow(clippy::too_many_arguments)]
fn container_menu(
    ui: &mut Ui,
    id: Id,
    is_root: bool,
    is_component: bool,
    hidden: bool,
    locked: bool,
    group: &[Id],
    actions: &mut Vec<TreeAction>,
) {
    group_entry(ui, group, actions);
    view_menu(ui, id, hidden, locked, is_root, actions);
    if !is_root {
        if ui
            .button(Message::TreeAddChild.text(crate::language(ui.ctx())))
            .on_hover_text(Message::TreeAddChildHint.text(crate::language(ui.ctx())))
            .clicked()
        {
            actions.push(TreeAction::AddChild { parent: id });
            ui.close();
        }
        ui.separator();
        if ui.button(Message::CmdDuplicate.text(crate::language(ui.ctx()))).clicked() {
            actions.push(TreeAction::Duplicate { node: id });
            ui.close();
        }
        if is_component {
            if ui.button(Message::CmdConvertToGroup.text(crate::language(ui.ctx()))).clicked() {
                actions.push(TreeAction::ConvertToGroup { node: id });
                ui.close();
            }
        } else if ui.button(Message::CmdUngroup.text(crate::language(ui.ctx()))).clicked() {
            actions.push(TreeAction::Ungroup { node: id });
            ui.close();
        }
        if ui.button(Message::CmdDelete.text(crate::language(ui.ctx()))).clicked() {
            actions.push(TreeAction::Delete { node: id });
            ui.close();
        }
    }
}

fn panel_menu(ui: &mut Ui, id: Id, hidden: bool, locked: bool, group: &[Id], actions: &mut Vec<TreeAction>) {
    group_entry(ui, group, actions);
    view_menu(ui, id, hidden, locked, false, actions);
    if ui.button(Message::CmdDuplicate.text(crate::language(ui.ctx()))).clicked() {
        actions.push(TreeAction::Duplicate { node: id });
        ui.close();
    }
    if ui.button(Message::CmdDelete.text(crate::language(ui.ctx()))).clicked() {
        actions.push(TreeAction::Delete { node: id });
        ui.close();
    }
}

/// The view entries every row menu carries: the eye, the padlock, *Isolate* and *Show all*.
///
/// The root is a frame rather than an object: hiding or locking it would hide or freeze the whole
/// scene, so it only offers *Show all*.
fn view_menu(ui: &mut Ui, id: Id, hidden: bool, locked: bool, is_root: bool, actions: &mut Vec<TreeAction>) {
    if !is_root {
        let lang = crate::language(ui.ctx());
        if ui.button(if hidden { Message::TreeShow } else { Message::TreeHide }.text(lang)).clicked() {
            actions.push(TreeAction::SetHidden { node: id, hidden: !hidden });
            ui.close();
        }
        if ui.button(if locked { Message::TreeUnlock } else { Message::TreeLock }.text(lang)).clicked() {
            actions.push(TreeAction::SetLocked { node: id, locked: !locked });
            ui.close();
        }
        ui.separator();
    }
    if ui
        .button(Message::TreeIsolate.text(crate::language(ui.ctx())))
        .on_hover_text(Message::TreeIsolateHint.text(crate::language(ui.ctx())))
        .clicked()
    {
        actions.push(TreeAction::Isolate { node: id });
        ui.close();
    }
    if ui
        .button(Message::TreeShowAll.text(crate::language(ui.ctx())))
        .on_hover_text(Message::TreeShowAllHint.text(crate::language(ui.ctx())))
        .clicked()
    {
        actions.push(TreeAction::ShowAll);
        ui.close();
    }
    ui.separator();
}

/// One row: its label on the left and the eye and padlock on the right.
///
/// The two toggles are revealed on hover, or kept visible while their flag is set, so a row at rest
/// reads as a name and a hidden or locked row still says why it behaves differently. They are
/// painted inside the row's own rectangle and reserve their cells at all times, so a hover never
/// moves the text. `toggles` is false for the root, which is a frame and not an object.
fn row(ui: &mut Ui, id: Id, label: RichText, selected: bool, flags: &mut ViewFlags, toggles: bool) -> egui::Response {
    let height = ui.spacing().interact_size.y.max(TOGGLE_GLYPH + 2.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let row_id = ui.make_persistent_id(("scene_row", id.get()));
    if !ui.is_rect_visible(rect) {
        return ui.interact(rect, row_id, Sense::click());
    }
    let hidden = flags.is_hidden(id);
    let locked = flags.is_locked(id);
    let reserved = if toggles { 2.0 * TOGGLE_CELL } else { 0.0 };
    let label_right = (rect.right() - reserved).max(rect.left());
    let label_rect = egui::Rect::from_min_max(rect.min, egui::pos2(label_right, rect.bottom()));
    let response = ui.interact(label_rect, row_id, Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let visuals = ui.style().interact_selectable(&response, selected);
    // The wash covers the toggles' cells too, so a hovered row reads as one row rather than as a
    // label that stops before two floating captions.
    if selected || hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS), visuals.weak_bg_fill);
    }
    let color = if hidden { theme::text_dim(ui) } else { visuals.text_color() };
    let label = if hidden { label.color(color) } else { label };
    let full = label.text().to_owned();
    let padding = ui.spacing().button_padding.x;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (label_rect.width() - 2.0 * padding).max(0.0),
        egui::TextStyle::Button,
    );
    let elided = galley.elided;
    let position = egui::pos2(label_rect.left() + padding, label_rect.center().y - 0.5 * galley.size().y);
    ui.painter().with_clip_rect(label_rect).galley(position, galley, color);
    // A name that does not fit is elided with `…`, so the full name is one hover away.
    let response = if elided { response.on_hover_text(full) } else { response };
    if toggles && rect.width() > reserved {
        let eye = egui::Rect::from_min_size(
            egui::pos2(rect.right() - reserved, rect.top()),
            egui::vec2(TOGGLE_CELL, rect.height()),
        );
        let lock =
            egui::Rect::from_min_size(egui::pos2(eye.right(), rect.top()), egui::vec2(TOGGLE_CELL, rect.height()));
        if hovered || hidden {
            let (icon, caption) = if hidden {
                (icons::actions::HIDDEN, Message::TreeShow)
            } else {
                (icons::actions::VISIBLE, Message::TreeHide)
            };
            if toggle(ui, eye, id, "visibility", icon, caption) {
                flags.set_hidden(id, !hidden);
            }
        }
        if hovered || locked {
            let (icon, caption) = if locked {
                (icons::actions::UNLOCK, Message::TreeUnlock)
            } else {
                (icons::actions::LOCK, Message::TreeLock)
            };
            if toggle(ui, lock, id, "lock", icon, caption) {
                flags.set_locked(id, !locked);
            }
        }
    }
    response
}

/// One 16 pt row toggle: its glyph, or its caption while the asset is an empty placeholder.
fn toggle(ui: &mut Ui, rect: egui::Rect, id: Id, kind: &str, icon: icons::Icon, caption: Message) -> bool {
    let response = ui.interact(rect, ui.make_persistent_id(("scene_toggle", id.get(), kind)), Sense::click());
    let wash = if response.is_pointer_button_down_on() {
        theme::control_active(ui)
    } else if response.hovered() {
        theme::control_hover(ui)
    } else {
        Color32::TRANSPARENT
    };
    if wash != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS), wash);
    }
    let ink = if response.hovered() { theme::text(ui) } else { theme::text_dim(ui) };
    if icon.is_drawn() {
        let glyph = egui::Rect::from_center_size(rect.center(), Vec2::splat(TOGGLE_GLYPH));
        icons::paint_icon(ui, icon, glyph, TOGGLE_GLYPH, ink);
    } else {
        ui.painter().with_clip_rect(rect).text(
            rect.center(),
            Align2::CENTER_CENTER,
            caption.text(crate::language(ui.ctx())),
            FontId::proportional(TOGGLE_TEXT_SIZE),
            ink,
        );
    }
    response.on_hover_text(caption.text(crate::language(ui.ctx()))).clicked()
}

/// Returns the first panel in depth-first order, used to open the card on a useful node.
pub(crate) fn first_panel(root: &Container) -> Option<Id> {
    first_panel_in(root.children())
}

fn first_panel_in(children: &[Node]) -> Option<Id> {
    for child in children {
        match child {
            Node::Panel(panel) => return Some(panel.id()),
            Node::Container(container) => {
                if let Some(id) = first_panel_in(container.children()) {
                    return Some(id);
                }
            }
            Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{CameraProjection, Id, LightKind, Node, Panel, PanelNominalSize, Placement};
    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    /// Every text a run drew, in paint order; the row captions and labels are all this test needs.
    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// The number of painted icon rasters: a drawn toggle is a texture mesh, not text.
    fn drawn_images(shapes: &[egui::epaint::ClippedShape]) -> usize {
        fn walk(shape: &egui::Shape, out: &mut usize) {
            match shape {
                egui::Shape::Mesh(_) => *out += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// One run's texts and its painted icon rasters.
    fn draw(session: &Session, flags: &mut ViewFlags) -> (Vec<String>, usize) {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut selection = Selection::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, session, &mut selection, flags);
        });
        let texts = drawn_text(&output.shapes);
        let images = drawn_images(&output.shapes);
        output.drop_without_applying_deltas();
        (texts, images)
    }

    fn panel(id: u64) -> Panel {
        Panel::new(
            Id::new(id).expect("id"),
            format!("Panel {id}"),
            PanelNominalSize::new(
                furnicraft_units::Length::from_millimetres(600).expect("length"),
                furnicraft_units::Length::from_millimetres(400).expect("width"),
            )
            .expect("size"),
            None,
            furnicraft_units::Length::from_millimetres(18).expect("thickness"),
            Placement::IDENTITY,
        )
        .expect("panel")
    }

    fn container(id: u64, children: Vec<Node>) -> Container {
        Container::new(Id::new(id).expect("id"), format!("Container {id}"), Placement::IDENTITY, children)
    }

    #[test]
    fn a_hidden_container_still_lists_its_children() {
        // Hiding a branch is a viewport decision, not a tree one: the row stays where the user can
        // find it and switch it back on (ADR 0065).
        let session = Session::with_catalog(sample_project(), sample_catalog());
        let mut flags = ViewFlags::default();
        flags.set_hidden(Id::new(3).expect("drawer id"), true);
        let (texts, _) = draw(&session, &mut flags);
        assert!(texts.iter().any(|text| text.contains("Front")), "the hidden drawer still lists its front: {texts:?}");
        assert!(texts.iter().any(|text| text.contains("Shelf")), "a sibling branch is listed too: {texts:?}");
    }

    #[test]
    fn a_row_at_rest_reveals_nothing_and_a_flagged_row_keeps_its_toggle() {
        let session = Session::with_catalog(sample_project(), sample_catalog());
        let shelf = Id::new(2).expect("shelf id");
        let mut flags = ViewFlags::default();
        let (resting, resting_images) = draw(&session, &mut flags);
        assert!(
            !resting.iter().any(|text| text == "Hide" || text == "Lock"),
            "the eye and the padlock wait for the pointer: {resting:?}"
        );
        assert_eq!(resting_images, 0, "a row at rest paints no toggle glyph");

        flags.set_hidden(shelf, true);
        let (hidden, hidden_images) = draw(&session, &mut flags);
        assert!(!hidden.iter().any(|text| text == "Show"), "the eye is a glyph, not a caption: {hidden:?}");
        assert_eq!(hidden_images, 1, "a hidden row keeps its eye");
        assert!(
            !hidden.iter().any(|text| text == "Lock"),
            "an unlocked row still hides its padlock until hovered: {hidden:?}"
        );

        flags.set_locked(shelf, true);
        flags.set_hidden(shelf, false);
        let (locked, locked_images) = draw(&session, &mut flags);
        assert!(!locked.iter().any(|text| text == "Unlock"), "the padlock is a glyph, not a caption: {locked:?}");
        assert_eq!(locked_images, 1, "a locked row keeps its padlock");
    }

    #[test]
    fn first_panel_is_depth_first() {
        let project = sample_project();
        let expected = Id::new(2).expect("id");
        assert_eq!(first_panel(project.root()), Some(expected));
        assert_eq!(first_panel(&container(10, Vec::new())), None);
    }

    #[test]
    fn first_panel_descends_into_nested_containers() {
        let nested = container(30, vec![Node::Panel(panel(21)), Node::Panel(panel(22))]);
        let root = container(10, vec![Node::Container(nested), Node::Panel(panel(11))]);
        assert_eq!(first_panel(&root), Some(Id::new(21).expect("id")));
    }

    #[test]
    fn selection_replaces_and_toggles() {
        let first = Id::new(2).expect("id");
        let second = Id::new(3).expect("id");
        let mut selection = Selection::default();
        assert_eq!(selection.primary(), None);

        selection.replace(first);
        assert!(selection.contains(first));
        assert_eq!(selection.primary(), Some(first));

        selection.toggle(second);
        assert_eq!(selection.ids(), &[first, second]);
        assert_eq!(selection.primary(), Some(second));

        selection.toggle(first);
        assert_eq!(selection.ids(), &[second]);
        assert_eq!(selection.primary(), Some(second));

        selection.replace(first);
        assert_eq!(selection.ids(), &[first]);

        selection.clear();
        assert_eq!(selection.primary(), None);
        assert!(selection.ids().is_empty());
    }

    #[test]
    fn selection_drops_removed_nodes() {
        let project = sample_project();
        let mut selection = Selection::default();
        selection.replace(Id::new(2).expect("id"));
        selection.toggle(Id::new(999).expect("id"));
        selection.retain_existing(&project);
        assert_eq!(selection.ids(), &[Id::new(2).expect("id")]);
        selection.replace(Id::new(3).expect("id"));
        selection.retain_existing(&project);
        assert!(selection.contains(Id::new(3).expect("id")));
    }

    /// The tree is a view and a set of structural edits only.
    ///
    /// Creation is not a `TreeAction`: the toolbar strip owns every tool, so the tree cannot grow a
    /// second creation path by adding a button back. Arming a container creates nothing, so
    /// `AddChild` is allowed and is checked here alongside the rest.
    #[test]
    fn the_tree_has_no_creation_action() {
        let kinds = [
            TreeAction::AddChild { parent: Id::new(3).expect("id") },
            TreeAction::Duplicate { node: Id::new(2).expect("id") },
            TreeAction::Delete { node: Id::new(2).expect("id") },
            TreeAction::Group { nodes: Vec::new() },
            TreeAction::Ungroup { node: Id::new(2).expect("id") },
            TreeAction::ConvertToGroup { node: Id::new(2).expect("id") },
        ];
        for action in kinds {
            let debug = format!("{action:?}");
            assert!(!debug.starts_with("Create"), "the tree must not create, got {debug}");
        }
    }

    #[test]
    fn selection_keeps_authored_cameras_and_lights() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let light = project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("light");

        let mut selection = Selection::default();
        selection.replace_all(vec![camera, light]);
        selection.retain_existing(&project);
        assert_eq!(
            selection.ids(),
            &[camera, light],
            "a camera and a light are scene nodes, so the next frame keeps them selected"
        );
    }
}
