//! The box tool: the base rectangle, the pulled height and the grips of a selected box.
//!
//! A box is drawn the way the extrusion is (ADR 0101): the first click anchors the base rectangle
//! on the active work plane, the second fixes it, the pointer then pulls the height along the plane's
//! normal, and the click after that — or the command bar's ✓ — commits one `object.primitive.create`.
//! Nothing reaches the document before the commit, so Escape costs nothing, and the wireframe prism
//! the pointer pulls is shell paint: an overlay entry is keyed by the id of a node the document
//! holds, and a box that does not exist yet has none (ADR 0093, ADR 0104).
//!
//! The same three lengths have a second owner: a product frame reserves a box and stores it as its
//! envelope (ADR 0097), so the grips read and write either field — a cuboid's parameters or an
//! envelope — through the owner's own command, and neither owner has a private editor.
//!
//! A selected box shows grips: its corners move both horizontal sides together and the six face
//! centres move one side each. A solid box shows the four corners of the base rectangle it was drawn
//! from; a frame shows all eight, because a cabinet is tall and its top corners are the ones at eye
//! level. A grip moves in 1 mm steps, and dragging a minimum side also moves the placement, so the
//! opposite side stays where it is in the world — the compensation a panel's corner and an origin
//! rebase use (ADR 0082, ADR 0088). A drag writes through the session's edit scope, so the whole
//! gesture is one undo entry (ADR 0092).
//!
//! [ADR 0082]: ../docs/decisions/0082-the-local-zero-is-the-creation-anchor.md
//! [ADR 0088]: ../docs/decisions/0088-the-origin-is-authored-once-and-rebased-explicitly.md
//! [ADR 0092]: ../docs/decisions/0092-a-field-is-a-number-with-its-unit-outside.md
//! [ADR 0093]: ../docs/decisions/0093-a-preview-fills-a-session-overlay.md
//! [ADR 0097]: ../docs/decisions/0097-a-product-is-a-container-with-an-authored-envelope.md
//! [ADR 0101]: ../docs/decisions/0101-a-frame-and-a-box-are-one-tool.md

use eframe::egui::{self, Color32, FontId, Pos2, Response, Stroke, Ui};
use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID;
use furnicraft_engine::object_cmds::OBJECT_PRIMITIVE_REPLACE_ID;
use furnicraft_engine::product_cmds::{PRODUCT_ENVELOPE_SET_ID, ProductEnvelopeSet};
use furnicraft_engine::{EditScope, NodePlacementSet, ObjectPrimitiveReplace, Session};
use furnicraft_geom::{DVec2, DVec3};
use furnicraft_model::{Envelope, Id, Placement, Primitive, Project};
use furnicraft_scene::Camera;
use furnicraft_ui::{BoxDraft, BoxOwner, Editor};
use furnicraft_units::Length;

use crate::gizmo_math::{ACTIVE_COLOR, WorkPlane, ray_plane, ray_to, screen_point};

/// Colour of the base rectangle while its second corner is being drawn.
const BASE_COLOR: Color32 = Color32::from_rgb(120, 200, 255);
/// Colour of the pulled prism and of an idle grip.
const BOX_COLOR: Color32 = Color32::from_rgb(255, 220, 96);
/// Colour of a live size label.
const SIZE_COLOR: Color32 = Color32::from_rgb(245, 245, 245);
/// Fill behind a live size label.
const SIZE_BACKGROUND: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 190);
/// Radius of one grip handle, in points.
const GRIP_RADIUS: f32 = 4.5;
/// Pointer distance that grabs a grip, in points.
const GRIP_HIT_RADIUS: f32 = 9.0;
/// Smallest side a grip or a pull leaves, in millimetres: a box keeps three positive sides.
pub(crate) const MIN_SIDE_MM: i64 = 1;

/// Largest side a grip or a pull leaves, in millimetres: the primitive bound the document validates.
pub(crate) const MAX_SIDE_MM: i64 = furnicraft_model::MAX_PRIMITIVE_SIZE.micrometres() / 1_000;

/// What one frame of the box creation gesture asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxEvent {
    /// A click on the work plane: it anchors the base or fixes it.
    Click([Length; 2]),
    /// A click with the base fixed: the ✓ of the gesture, which commits the box.
    Commit,
}

/// A work-plane point as a float vector in millimetres.
fn plane_millimetres(point: [Length; 2]) -> DVec2 {
    DVec2::new(point[0].millimetres_f64(), point[1].millimetres_f64())
}

/// A millimetre coordinate as a document length, rounded to the micrometre.
fn length_micrometres(millimetres: f64) -> Option<Length> {
    if !millimetres.is_finite() || millimetres.abs() > i64::MAX as f64 / 1_000.0 {
        return None;
    }
    Some(Length::from_micrometres((millimetres * 1_000.0).round() as i64))
}

/// The plane point under the pointer, in work-plane millimetres rounded to the micrometre.
pub(crate) fn plane_point(
    camera: Camera,
    viewport: egui::Rect,
    plane: WorkPlane,
    pointer: Pos2,
) -> Option<[Length; 2]> {
    let world = ray_plane(ray_to(pointer, viewport, camera), DVec3::ZERO, plane.normal())?;
    let local = plane.local(world);
    Some([length_micrometres(local.x)?, length_micrometres(local.y)?])
}

/// The height under the pointer, measured along the plane's normal from the base corner.
///
/// The pull reads a position on the axis rather than a screen distance, so the same gesture near
/// the camera and far from it produces the same number of millimetres. The answer is a whole number
/// of millimetres inside the primitive bounds: a pull never leaves a side the document refuses.
pub(crate) fn pulled_height(
    camera: Camera,
    viewport: egui::Rect,
    plane: WorkPlane,
    corner: [Length; 2],
    pointer: Pos2,
) -> Option<Length> {
    let origin = plane.world(plane_millimetres(corner));
    let parameter = ray_to(pointer, viewport, camera).closest_parameter_on_line(origin, plane.normal())?;
    if !parameter.is_finite() {
        return None;
    }
    let whole = parameter.round().clamp(MIN_SIDE_MM as f64, MAX_SIDE_MM as f64) as i64;
    Length::from_millimetres(whole)
}

/// The work plane's frame with its origin on the box's own zero: the base rectangle's minimum corner.
pub(crate) fn base_placement(plane: WorkPlane, draft: &BoxDraft) -> Option<Placement> {
    let base = draft.base()?;
    plane.placement_at(plane_millimetres(base.corner))
}

/// The frame and the three sides the ✓ would commit — exactly what the ghost shows.
pub(crate) fn commit_frame(plane: WorkPlane, draft: &BoxDraft) -> Option<(Placement, [Length; 3])> {
    Some((base_placement(plane, draft)?, draft.sizes()?))
}

/// Reads one click of the box creation gesture.
///
/// The gizmo never touches the document: it reports the click or the commit, and the editor's draft
/// holds the numbers, exactly as the drawing gizmo hands its clicks to the drawing draft.
pub(crate) fn update_box(
    ui: &Ui,
    response: &Response,
    viewport: egui::Rect,
    camera: Camera,
    plane: WorkPlane,
    draft: &BoxDraft,
) -> Option<BoxEvent> {
    if !response.clicked_by(egui::PointerButton::Primary) {
        return None;
    }
    if draft.is_pulling() {
        // The click that accepts the height is the gesture's own ✓; the command bar accepts the
        // same pull, and a second click finds no draft to commit.
        return Some(BoxEvent::Commit);
    }
    let corner = plane_point(camera, viewport, plane, response.interact_pointer_pos()?)?;
    ui.ctx().request_repaint();
    Some(BoxEvent::Click(corner))
}

/// The four corners of the rectangle between two opposite corners, in plane millimetres.
fn rectangle_corners(first: [Length; 2], second: [Length; 2]) -> [[Length; 2]; 4] {
    let span = |index: usize| -> (i64, i64) {
        let start = first.get(index).map_or(0, |value| value.micrometres());
        let end = second.get(index).map_or(0, |value| value.micrometres());
        (start.min(end), start.max(end))
    };
    let (min_x, max_x) = span(0);
    let (min_y, max_y) = span(1);
    let point = |x: i64, y: i64| [Length::from_micrometres(x), Length::from_micrometres(y)];
    [point(min_x, min_y), point(max_x, min_y), point(max_x, max_y), point(min_x, max_y)]
}

/// Paints the base rectangle being drawn and the prism its height pulls.
pub(crate) fn paint_box(
    ui: &Ui,
    viewport: egui::Rect,
    camera: Camera,
    plane: WorkPlane,
    pointer: Option<Pos2>,
    draft: &BoxDraft,
) {
    let Some(first) = draft.first() else {
        return;
    };
    let painter = ui.painter().with_clip_rect(viewport);
    let project = |point: [Length; 2], height: Length| -> Option<Pos2> {
        let local = plane_millimetres(point);
        let world = plane.world(local) + plane.normal() * height.millimetres_f64();
        screen_point(camera, viewport, world)
    };
    // The base rectangle while it is being drawn; once it is fixed the prism's bottom ring is the
    // same rectangle, so drawing it again would only hide it.
    if draft.base().is_none() {
        let mut base = Vec::with_capacity(5);
        for corner in rectangle_corners(first, draft.live().unwrap_or(first)) {
            match project(corner, Length::ZERO) {
                Some(position) => base.push(position),
                None => return,
            }
        }
        if let Some(&first_point) = base.first() {
            base.push(first_point);
            painter.add(egui::Shape::line(base, Stroke::new(1.6, BASE_COLOR)));
        }
        return;
    }
    let Some(sizes) = draft.sizes() else {
        return;
    };
    let Some(base) = draft.base() else {
        return;
    };
    // The pulled prism: four verticals and the two rings, which is the box the ✓ builds.
    let stroke = Stroke::new(1.8, BOX_COLOR);
    let mut low_ring = Vec::with_capacity(5);
    let mut high_ring = Vec::with_capacity(5);
    for corner in rectangle_corners(
        base.corner,
        [
            Length::from_micrometres(base.corner[0].micrometres().saturating_add(sizes[0].micrometres())),
            Length::from_micrometres(base.corner[1].micrometres().saturating_add(sizes[1].micrometres())),
        ],
    ) {
        let (Some(low), Some(high)) = (project(corner, Length::ZERO), project(corner, sizes[2])) else {
            return;
        };
        painter.line_segment([low, high], stroke);
        low_ring.push(low);
        high_ring.push(high);
    }
    for ring in [low_ring, high_ring] {
        let mut closed = ring.clone();
        if let Some(&first_point) = ring.first() {
            closed.push(first_point);
        }
        painter.add(egui::Shape::line(closed, stroke));
    }
    if let Some(pointer) = pointer {
        paint_size(&painter, pointer, &format!("{} × {} × {}", sizes[0], sizes[1], sizes[2]));
    }
}

/// Paints the live size next to the cursor on a readable background.
fn paint_size(painter: &egui::Painter, pointer: Pos2, label: &str) {
    let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.0), SIZE_COLOR);
    let padding = egui::vec2(5.0, 3.0);
    let rect = egui::Rect::from_min_size(pointer + egui::vec2(14.0, 10.0), galley.size() + padding * 2.0);
    painter.rect_filled(rect, 3.0, SIZE_BACKGROUND);
    painter.galley(rect.min + padding, galley, SIZE_COLOR);
}

/// Commits the box gesture as one `object.primitive.create` and clears the draft.
pub(crate) fn commit_box(session: &mut Session, editor: &mut Editor, plane: WorkPlane) -> Result<Id, String> {
    let draft = editor.box_draft().copied().ok_or_else(|| "No box is being drawn.".to_owned())?;
    let (placement, _) =
        commit_frame(plane, &draft).ok_or_else(|| "The box has no base corner or height.".to_owned())?;
    editor.commit_box(session, placement)
}

/// One side of a selected box, named the way a grip moves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxSide {
    /// The side along the box's local X.
    Width,
    /// The side along the box's local Y.
    Depth,
    /// The side along the box's local Z.
    Height,
}

impl BoxSide {
    /// The local axis of this side.
    pub(crate) const fn axis(self) -> usize {
        match self {
            Self::Width => 0,
            Self::Depth => 1,
            Self::Height => 2,
        }
    }

    /// The unit vector of this side in the box's own frame.
    const fn unit(self) -> DVec3 {
        match self {
            Self::Width => DVec3::X,
            Self::Depth => DVec3::Y,
            Self::Height => DVec3::Z,
        }
    }
}

/// One grip handle of a selected box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxGrip {
    /// A corner of the base rectangle: it moves the two horizontal sides together.
    Corner {
        /// Whether the corner sits on the minimum X side.
        min_x: bool,
        /// Whether the corner sits on the minimum Y side.
        min_y: bool,
        /// Whether the corner sits on the top of the box rather than on its base.
        top: bool,
    },
    /// One face centre: it moves one side.
    Face {
        /// Which side the face belongs to.
        side: BoxSide,
        /// Whether it is the maximum face; a minimum face also moves the placement.
        max: bool,
    },
}

/// A selected box read from the document: the node, the field its sides live in, its frames and its
/// three sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxBody {
    /// The node whose field the grips edit.
    node: Id,
    /// Which field that is: a primitive's parameters or a product's envelope.
    owner: BoxOwner,
    /// The node's placement in its parent frame, the one a compensation rewrites.
    placement: Placement,
    /// The node's placement in the world, the one the pointer is projected into.
    world: Placement,
    /// The stored width, depth and height.
    sides: [Length; 3],
}

/// Whether one node is a box the grips can edit.
pub(crate) fn is_box(project: &Project, node: Id) -> bool {
    BoxBody::read(project, node).is_some()
}

impl BoxBody {
    /// Reads a box, or `None` when the node is neither a cuboid primitive nor a product frame.
    pub(crate) fn read(project: &Project, node: Id) -> Option<Self> {
        let world = project.world_placement(node)?;
        if let Some(object) = project.object(node)
            && let Some(Primitive::Cuboid(cuboid)) = object.geometry().as_primitive()
        {
            let sides = cuboid.sides();
            return Some(Self { node, owner: BoxOwner::Cuboid, placement: object.placement(), world, sides });
        }
        let container = project.container(node).filter(|container| container.is_product())?;
        let sides = container.envelope()?.components();
        Some(Self { node, owner: BoxOwner::Product, placement: container.placement(), world, sides })
    }

    /// The local point of a grip handle, in millimetres.
    fn local_point(&self, grip: BoxGrip) -> Option<DVec3> {
        let [width, depth, height] = self.sides.map(Length::millimetres_f64);
        let point = match grip {
            BoxGrip::Corner { min_x, min_y, top } => DVec3::new(
                if min_x { 0.0 } else { width },
                if min_y { 0.0 } else { depth },
                if top { height } else { 0.0 },
            ),
            BoxGrip::Face { side, max } => match (side, max) {
                (BoxSide::Width, true) => DVec3::new(width, depth / 2.0, height / 2.0),
                (BoxSide::Width, false) => DVec3::new(0.0, depth / 2.0, height / 2.0),
                (BoxSide::Depth, true) => DVec3::new(width / 2.0, depth, height / 2.0),
                (BoxSide::Depth, false) => DVec3::new(width / 2.0, 0.0, height / 2.0),
                (BoxSide::Height, true) => DVec3::new(width / 2.0, depth / 2.0, height),
                (BoxSide::Height, false) => DVec3::new(width / 2.0, depth / 2.0, 0.0),
            },
        };
        point.is_finite().then_some(point)
    }

    /// The world point of a grip handle.
    pub(crate) fn grip_point(&self, grip: BoxGrip) -> Option<DVec3> {
        self.local_point(grip).map(|local| self.world.transform_point(local))
    }

    /// Every grip a selected box shows: its corners and its six faces.
    pub(crate) fn grips(&self) -> Vec<BoxGrip> {
        // A solid box was drawn from its base rectangle, so the four corners of that rectangle are
        // the handles that turn it. A frame reserves a tall space whose top corners are the ones at
        // eye level, so it carries all eight; each corner drags in its own horizontal plane and
        // changes the same two sides (docs/domain/products.md §9).
        let tops: &[bool] = match self.owner {
            BoxOwner::Cuboid => &[false],
            BoxOwner::Product => &[false, true],
        };
        let mut grips = Vec::with_capacity(tops.len() * 4 + 6);
        for top in tops {
            for min_x in [true, false] {
                for min_y in [true, false] {
                    grips.push(BoxGrip::Corner { min_x, min_y, top: *top });
                }
            }
        }
        for side in [BoxSide::Width, BoxSide::Depth, BoxSide::Height] {
            for max in [true, false] {
                grips.push(BoxGrip::Face { side, max });
            }
        }
        grips
    }
}

/// The field one grip drag writes, chosen by who owns the box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BoxChange {
    /// A cuboid's geometry parameters.
    Primitive(Primitive),
    /// A product's authored envelope.
    Envelope(Envelope),
}

/// The box a grip drag would write: its new sides and, for a moved zero, its new placement.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GripEdit {
    /// The node whose field changes.
    pub(crate) node: Id,
    /// The new sides, in the field that owns them.
    pub(crate) change: BoxChange,
    /// The compensated placement, when the dragged side moves the box's own zero.
    pub(crate) placement: Option<Placement>,
}

/// One grip drag, with the edit scope that makes the whole gesture one undo entry.
#[derive(Clone, Copy, Debug)]
struct GripDrag {
    node: Id,
    grip: BoxGrip,
    scope: EditScope,
}

/// What one frame of the grips asks the shell to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GripUpdate {
    /// Nothing is under the pointer and no drag is live.
    Idle,
    /// A press landed on this handle: the shell mints the edit scope and calls [`BoxGrips::begin`].
    Press(BoxGrip),
    /// The live drag reaches this edit.
    Edit(GripEdit),
}

/// The grips of the selected box and the drag that is live on one of them.
#[derive(Debug, Default)]
pub(crate) struct BoxGrips {
    drag: Option<GripDrag>,
    hover: Option<BoxGrip>,
}

impl BoxGrips {
    /// Whether a grip owns the pointer, so no other gizmo may start.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The edit scope a live drag writes through, for Escape.
    pub(crate) fn scope(&self) -> Option<EditScope> {
        self.drag.map(|drag| drag.scope)
    }

    /// Ends a drag; the caller has already given the document its last word.
    pub(crate) fn end(&mut self) {
        self.drag = None;
    }

    /// Reads the pointer over the grips of `selected`.
    ///
    /// A press on a handle asks the shell for the edit scope; a pointer move inside a drag returns
    /// the edit it reaches; the release ends the gesture without another command. Anything else
    /// reports [`GripUpdate::Idle`].
    pub(crate) fn update(
        &mut self,
        ui: &Ui,
        viewport: egui::Rect,
        camera: Camera,
        project: &Project,
        selected: Option<Id>,
        pointer: Option<Pos2>,
    ) -> GripUpdate {
        let Some(pointer) = pointer else {
            return GripUpdate::Idle;
        };
        if let Some(drag) = self.drag {
            let Some(body) = BoxBody::read(project, drag.node) else {
                self.drag = None;
                return GripUpdate::Idle;
            };
            let edit = self.edit_for(drag, body, camera, viewport, pointer);
            if !ui.input(|input| input.pointer.primary_down()) {
                // The release keeps what the last move wrote: one drag is one edit, one undo entry.
                self.drag = None;
            }
            return edit.map_or(GripUpdate::Idle, GripUpdate::Edit);
        }
        let Some(body) = selected.and_then(|node| BoxBody::read(project, node)) else {
            self.hover = None;
            return GripUpdate::Idle;
        };
        let hit = self.hit(camera, viewport, &body, pointer);
        self.hover = hit;
        match hit {
            Some(grip) if ui.input(|input| input.pointer.primary_pressed()) => GripUpdate::Press(grip),
            _ => GripUpdate::Idle,
        }
    }

    /// Starts a drag on the grip the pointer is over, with the scope that makes it one undo entry.
    pub(crate) fn begin(&mut self, node: Id, grip: BoxGrip, scope: EditScope) {
        self.drag = Some(GripDrag { node, grip, scope });
    }

    /// The edit one pointer position asks for, or `None` when it changes nothing.
    ///
    /// A base corner moves inside the base plane, so the pointer is projected onto it and both
    /// horizontal sides read their coordinate from there. A face moves along one axis, so the
    /// pointer is read as a position on that axis — the same rule the extrusion pull uses, and the
    /// reason a face can be dragged from anywhere in the view rather than only across itself.
    fn edit_for(
        &self,
        drag: GripDrag,
        body: BoxBody,
        camera: Camera,
        viewport: egui::Rect,
        pointer: Pos2,
    ) -> Option<GripEdit> {
        let ray = ray_to(pointer, viewport, camera);
        let inverse = body.world.inverse().ok()?;
        let mut sides = body.sides;
        let shift = match drag.grip {
            BoxGrip::Corner { min_x, min_y, top } => {
                // The drag plane is the horizontal one through the corner: a base corner reads the
                // floor and a frame's top corner the ceiling, so a tall piece is edited where the
                // maker can reach it.
                let height = if top { body.sides[2].millimetres_f64() } else { 0.0 };
                let base = body.world.transform_point(DVec3::new(0.0, 0.0, height));
                let normal = body.world.transform_vector(DVec3::Z).try_normalize()?;
                let hit = ray_plane(ray, base, normal)?;
                let local = inverse.transform_point(hit);
                if !local.is_finite() {
                    return None;
                }
                let (width, dx) = dragged_side(sides[0], local.x, min_x)?;
                let (depth, dy) = dragged_side(sides[1], local.y, min_y)?;
                sides[0] = width;
                sides[1] = depth;
                DVec3::new(dx, dy, 0.0)
            }
            BoxGrip::Face { side, max } => {
                let index = side.axis();
                let current = *sides.get(index)?;
                let offset = if max { current.millimetres_f64() } else { 0.0 };
                let [width, depth, height] = body.sides.map(Length::millimetres_f64);
                // The axis runs through the face centre, so the pointer's position on it is the
                // face's own coordinate whichever component the drag is measured in.
                let centre = match index {
                    0 => DVec3::new(offset, depth / 2.0, height / 2.0),
                    1 => DVec3::new(width / 2.0, offset, height / 2.0),
                    _ => DVec3::new(width / 2.0, depth / 2.0, offset),
                };
                let origin = body.world.transform_point(centre);
                let direction = body.world.transform_vector(side.unit());
                let parameter = ray.closest_parameter_on_line(origin, direction)?;
                let (length, moved) = dragged_side(current, offset + parameter, !max)?;
                *sides.get_mut(index)? = length;
                side.unit() * moved
            }
        };
        if sides == body.sides {
            return None;
        }
        let [width, depth, height] = sides;
        let change = match body.owner {
            BoxOwner::Cuboid => BoxChange::Primitive(Primitive::cuboid(width, depth, height).ok()?),
            BoxOwner::Product => BoxChange::Envelope(Envelope::new(width, depth, height).ok()?),
        };
        let placement = if shift.length_squared() > 0.0 {
            let moved = body.placement.transform_vector(shift);
            Placement::new(body.placement.translation() + moved, body.placement.rotation()).ok()
        } else {
            None
        };
        Some(GripEdit { node: drag.node, change, placement })
    }

    /// The grip under the pointer, if any.
    fn hit(&self, camera: Camera, viewport: egui::Rect, body: &BoxBody, pointer: Pos2) -> Option<BoxGrip> {
        let mut best: Option<(f32, BoxGrip)> = None;
        for grip in body.grips() {
            let Some(screen) = body.grip_point(grip).and_then(|world| screen_point(camera, viewport, world)) else {
                continue;
            };
            let distance = screen.distance(pointer);
            if distance <= GRIP_HIT_RADIUS && best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, grip));
            }
        }
        best.map(|(_, grip)| grip)
    }

    /// Paints the grips of the selected box.
    pub(crate) fn paint(&self, ui: &Ui, viewport: egui::Rect, camera: Camera, project: &Project, selected: Option<Id>) {
        let Some(body) = selected.and_then(|node| BoxBody::read(project, node)) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(viewport);
        for grip in body.grips() {
            let Some(screen) = body.grip_point(grip).and_then(|world| screen_point(camera, viewport, world)) else {
                continue;
            };
            let active = self.hover == Some(grip) || self.drag.is_some_and(|drag| drag.grip == grip);
            let color = if active { ACTIVE_COLOR } else { BOX_COLOR };
            painter.circle_filled(screen, GRIP_RADIUS, Color32::from_black_alpha(120));
            painter.circle_stroke(screen, GRIP_RADIUS, Stroke::new(1.6, color));
            if active {
                painter.circle_filled(screen, GRIP_RADIUS - 1.6, color);
            }
        }
    }
}

/// The side a drag reaches, and how far the box's own zero moves so the opposite side stays put.
///
/// A maximum side is the dragged coordinate itself; a minimum side is the distance from the dragged
/// coordinate to the opposite face, and the zero moves along with it (ADR 0082, ADR 0101).
fn dragged_side(current: Length, coordinate_mm: f64, min: bool) -> Option<(Length, f64)> {
    if !coordinate_mm.is_finite() {
        return None;
    }
    let reached = if min { current.millimetres_f64() - coordinate_mm } else { coordinate_mm };
    let whole = reached.round().clamp(MIN_SIDE_MM as f64, MAX_SIDE_MM as f64) as i64;
    let length = Length::from_millimetres(whole)?;
    let shift = if min { current.millimetres_f64() - length.millimetres_f64() } else { 0.0 };
    Some((length, shift))
}

/// Applies one grip edit through the session's edit scope, so the drag is one undo entry.
///
/// The owner picks the command: a cuboid writes `object.primitive.replace` and a frame
/// `product.envelope.set`, and a moved zero writes `node.placement.set` for both. Both commands run
/// in the drag's scope, so undo returns the whole gesture (ADR 0092).
pub(crate) fn apply_grip_edit(session: &mut Session, edit: GripEdit, scope: EditScope) -> Result<(), String> {
    match edit.change {
        BoxChange::Primitive(primitive) => {
            let params = serde_json::to_value(ObjectPrimitiveReplace { object: edit.node, primitive })
                .map_err(|error| format!("could not encode the primitive parameters: {error}"))?;
            session.dispatch_in(scope, OBJECT_PRIMITIVE_REPLACE_ID, &params).map_err(|error| error.to_string())?;
        }
        BoxChange::Envelope(envelope) => {
            let params = serde_json::to_value(ProductEnvelopeSet { node: edit.node, envelope })
                .map_err(|error| format!("could not encode the envelope: {error}"))?;
            session.dispatch_in(scope, PRODUCT_ENVELOPE_SET_ID, &params).map_err(|error| error.to_string())?;
        }
    }
    if let Some(placement) = edit.placement {
        let params = serde_json::to_value(NodePlacementSet { node: edit.node, placement })
            .map_err(|error| format!("could not encode the placement: {error}"))?;
        session.dispatch_in(scope, NODE_PLACEMENT_SET_ID, &params).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::Geometry;
    use furnicraft_testkit::{millimetres, sample_catalog, sample_product_project, sample_project};

    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn box_project() -> (Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let cuboid = Primitive::cuboid(millimetres(600), millimetres(400), millimetres(720)).expect("a box");
        let id = project.add_object(root, "Box", Placement::IDENTITY, Geometry::Primitive(cuboid)).expect("add");
        (project, id)
    }

    /// The sides of a grip edit, whichever field owns them.
    fn edited_sides(edit: &GripEdit) -> [f64; 3] {
        match &edit.change {
            BoxChange::Primitive(primitive) => match primitive {
                Primitive::Cuboid(cuboid) => cuboid.sides().map(Length::millimetres_f64),
                other => panic!("a grip edit of a box must stay a box: {other:?}"),
            },
            BoxChange::Envelope(envelope) => envelope.components().map(Length::millimetres_f64),
        }
    }

    #[test]
    fn a_selected_box_shows_four_base_corners_and_six_faces() {
        let (project, id) = box_project();
        let body = BoxBody::read(&project, id).expect("a box body");
        assert_eq!(body.grips().len(), 10, "four base corners and six faces");
        assert_eq!(body.grip_point(BoxGrip::Corner { min_x: true, min_y: true, top: false }), Some(DVec3::ZERO));
        assert_eq!(
            body.grip_point(BoxGrip::Corner { min_x: false, min_y: false, top: false }),
            Some(DVec3::new(600.0, 400.0, 0.0)),
            "the far corner of the base rectangle, not the top one"
        );
        assert!(
            !body.grips().iter().any(|grip| matches!(grip, BoxGrip::Corner { top: true, .. })),
            "a solid box shows no top corner, even though its geometry has one"
        );
        assert_eq!(
            body.grip_point(BoxGrip::Face { side: BoxSide::Height, max: true }),
            Some(DVec3::new(300.0, 200.0, 720.0)),
            "the centre of the top face"
        );
    }

    #[test]
    fn a_selected_frame_shows_eight_corners_and_six_faces() {
        let (project, id) = sample_product_project();
        let body = BoxBody::read(&project, id).expect("a frame body");
        assert_eq!(body.grips().len(), 14, "eight corners and six faces");
        let far_top = body.grip_point(BoxGrip::Corner { min_x: false, min_y: false, top: true }).expect("a top corner");
        assert_eq!(far_top, DVec3::new(600.0, 560.0, 720.0), "the far corner at the top of the reserved box");
        // A node that is neither a product nor a cuboid has no box.
        let root = project.root().id();
        assert!(!is_box(&project, root), "the root reserves nothing");
        assert!(BoxBody::read(&project, Id::new(99).expect("id")).is_none(), "an id the document does not hold");
    }

    #[test]
    fn a_maximum_side_is_the_dragged_coordinate_and_a_minimum_side_moves_the_zero() {
        let (side, shift) = dragged_side(millimetres(600), 700.0, false).expect("a wider box");
        assert_eq!((side, shift), (millimetres(700), 0.0), "a max side grows outward");

        let (side, shift) = dragged_side(millimetres(600), 100.0, true).expect("a narrower box");
        assert_eq!(side, millimetres(500), "the min side is 100 mm from the far face");
        assert_eq!(shift, 100.0, "the zero moves with it, so the far face stays put");

        let (side, _) = dragged_side(millimetres(600), -400.0, true).expect("a drag past the far face");
        assert_eq!(side, millimetres(1_000), "the far face stops the side at a metre");
        assert!(dragged_side(millimetres(600), f64::NAN, false).is_none(), "a hostile coordinate is refused");
    }

    #[test]
    fn the_pulled_height_reads_millimetres_along_the_normal() {
        let camera = Camera::new(DVec3::new(0.0, 0.0, 360.0), 3_000.0).expect("camera");
        let world = DVec3::new(0.0, 0.0, 720.0);
        let pointer = screen_point(camera, viewport(), world).expect("the point is on screen");
        let height = pulled_height(camera, viewport(), WorkPlane::WORLD_XY, [Length::ZERO, Length::ZERO], pointer)
            .expect("height");
        assert!((height.millimetres_f64() - 720.0).abs() <= 1.0, "the pull reads the axis, was {height}");
    }

    #[test]
    fn a_pull_below_the_plane_stops_at_the_smallest_side() {
        let camera = Camera::new(DVec3::new(0.0, 0.0, 360.0), 3_000.0).expect("camera");
        let pointer = screen_point(camera, viewport(), DVec3::new(0.0, 0.0, -400.0)).expect("on screen");
        let height = pulled_height(camera, viewport(), WorkPlane::WORLD_XY, [Length::ZERO, Length::ZERO], pointer)
            .expect("height");
        assert_eq!(height, millimetres(MIN_SIDE_MM));
    }

    #[test]
    fn the_committed_frame_is_the_ghost_the_maker_saw() {
        let mut editor = Editor::new();
        editor.arm_box();
        editor.click_box([millimetres(600), millimetres(400)]).expect("anchor");
        editor.click_box([millimetres(0), millimetres(0)]).expect("base");
        editor.pull_box_height(millimetres(720)).expect("pull");
        let (placement, sizes) =
            commit_frame(WorkPlane::WORLD_XY, editor.box_draft().expect("a draft")).expect("frame");
        assert_eq!(sizes, [millimetres(600), millimetres(400), millimetres(720)]);
        assert!(placement.translation().length() < 1.0e-9, "the base corner is the plane origin here");
    }

    #[test]
    fn a_grip_drag_writes_one_undo_entry() {
        let (project, id) = box_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let before = session.project().revision().get();
        let scope = session.edit_scope();
        let first = Primitive::cuboid(millimetres(650), millimetres(400), millimetres(720)).expect("box");
        let last = Primitive::cuboid(millimetres(700), millimetres(400), millimetres(720)).expect("box");
        let edit =
            |primitive: Primitive| GripEdit { node: id, change: BoxChange::Primitive(primitive), placement: None };
        apply_grip_edit(&mut session, edit(first), scope).expect("first");
        apply_grip_edit(&mut session, edit(last.clone()), scope).expect("last");
        session.end_edit();
        assert_eq!(session.project().revision().get(), before + 2, "each frame is one command, both in one entry");
        assert_eq!(session.project().object_primitive(id), Ok(&last));
        session.undo().expect("one undo returns the whole drag");
        assert_eq!(
            session.project().object_primitive(id),
            Ok(&Primitive::cuboid(millimetres(600), millimetres(400), millimetres(720)).expect("box"))
        );
    }

    #[test]
    fn a_corner_grip_moves_two_sides_and_a_face_grip_one() {
        let (project, id) = box_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let project = session.project().clone();
        let camera = Camera::new(DVec3::new(300.0, 200.0, 360.0), 3_000.0).expect("camera");
        let grips = BoxGrips::default();
        let body = BoxBody::read(&project, id).expect("a box body");
        let scope = session.edit_scope();
        let pointer_at = |world: DVec3| screen_point(camera, viewport(), world).expect("on screen");

        // A screen point carries a pixel of rounding, which is a few millimetres at this distance.
        let close = |edit: &GripEdit, wanted: [f64; 3]| {
            for (side, wanted) in edited_sides(edit).iter().zip(wanted) {
                assert!((side - wanted).abs() <= 5.0, "side {side} is not {wanted}");
            }
        };

        // The far corner of the base: both horizontal sides follow it.
        let drag = GripDrag { node: id, grip: BoxGrip::Corner { min_x: false, min_y: false, top: false }, scope };
        let edit =
            grips.edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(700.0, 500.0, 0.0))).expect("an edit");
        close(&edit, [700.0, 500.0, 720.0]);
        assert_eq!(edit.placement, None, "a maximum corner does not move the box's zero");

        // One face centre: one side follows it, the other two stay.
        let drag = GripDrag { node: id, grip: BoxGrip::Face { side: BoxSide::Width, max: true }, scope };
        let edit = grips
            .edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(700.0, 200.0, 360.0)))
            .expect("an edit");
        close(&edit, [700.0, 400.0, 720.0]);
        assert_eq!(edit.placement, None);

        // The floor moves down: the height grows and the box's zero moves with the floor.
        let drag = GripDrag { node: id, grip: BoxGrip::Face { side: BoxSide::Height, max: false }, scope };
        let edit = grips
            .edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(300.0, 200.0, -100.0)))
            .expect("an edit");
        close(&edit, [600.0, 400.0, 820.0]);
        let placement = edit.placement.expect("the floor moved, so the zero did too");
        assert!(
            (placement.translation() - DVec3::new(0.0, 0.0, -100.0)).length() <= 5.0,
            "the zero followed the floor down, was {:?}",
            placement.translation()
        );
    }

    #[test]
    fn a_minimum_face_drag_keeps_the_opposite_face_still_in_the_world() {
        let (project, id) = box_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let scope = session.edit_scope();
        // Drag the min-X face 100 mm to the left: the side grows to 700 and the zero moves 100 mm
        // in the box's own frame, so the far face does not move.
        let primitive = Primitive::cuboid(millimetres(700), millimetres(400), millimetres(720)).expect("box");
        let (_, shift) = dragged_side(millimetres(600), -100.0, true).expect("a wider box");
        let placement = Placement::new(DVec3::new(shift, 0.0, 0.0), default_rotation()).expect("placement");
        let change = BoxChange::Primitive(primitive);
        apply_grip_edit(&mut session, GripEdit { node: id, change, placement: Some(placement) }, scope)
            .expect("the drag");
        session.end_edit();
        let world = session.project().world_placement(id).expect("world placement");
        let far = world.transform_point(DVec3::new(700.0, 0.0, 0.0));
        assert!((far - DVec3::new(600.0, 0.0, 0.0)).length() < 1.0e-9, "the far face stayed put, was {far:?}");
    }

    /// The envelope a frame stores, in whole millimetres.
    fn envelope_sides(session: &Session, id: Id) -> [f64; 3] {
        let container = session.project().container(id).expect("the product container");
        container.envelope().expect("an authored envelope").components().map(Length::millimetres_f64)
    }

    #[test]
    fn a_frame_corner_grip_changes_two_sizes_and_a_face_grip_one() {
        let (project, id) = sample_product_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let camera = Camera::new(DVec3::new(300.0, 280.0, 360.0), 3_000.0).expect("camera");
        let grips = BoxGrips::default();
        let body = BoxBody::read(session.project(), id).expect("a frame body");
        let scope = session.edit_scope();
        let pointer_at = |world: DVec3| screen_point(camera, viewport(), world).expect("on screen");

        // A base corner: width and depth follow the pointer, the height stays.
        let drag = GripDrag { node: id, grip: BoxGrip::Corner { min_x: false, min_y: false, top: false }, scope };
        let edit =
            grips.edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(700.0, 660.0, 0.0))).expect("an edit");
        let sides = edited_sides(&edit);
        assert!((sides[0] - 700.0).abs() <= 5.0, "the width follows the corner: {sides:?}");
        assert!((sides[1] - 660.0).abs() <= 5.0, "the depth follows the corner: {sides:?}");
        assert_eq!(sides[2], 720.0, "the height is not a corner's business: {sides:?}");
        assert_eq!(edit.placement, None, "a maximum corner does not move the frame's zero");

        // The top corner drags in the plane through the top: the same two sides, not the height.
        let drag = GripDrag { node: id, grip: BoxGrip::Corner { min_x: false, min_y: false, top: true }, scope };
        let top = grips
            .edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(640.0, 600.0, 720.0)))
            .expect("an edit");
        let top_sides = edited_sides(&top);
        assert!((top_sides[0] - 640.0).abs() <= 5.0, "a top corner reads the ceiling: {top_sides:?}");
        assert!((top_sides[1] - 600.0).abs() <= 5.0, "and the same two sides: {top_sides:?}");
        assert_eq!(top_sides[2], 720.0, "and leaves the height alone: {top_sides:?}");

        // One face: one side, the other two stay.
        let drag = GripDrag { node: id, grip: BoxGrip::Face { side: BoxSide::Depth, max: true }, scope };
        let edit = grips
            .edit_for(drag, body, camera, viewport(), pointer_at(DVec3::new(300.0, 700.0, 360.0)))
            .expect("an edit");
        let sides = edited_sides(&edit);
        assert_eq!((sides[0], sides[2]), (600.0, 720.0), "a face moves one side: {sides:?}");
        assert!((sides[1] - 700.0).abs() <= 5.0, "the depth follows the face: {sides:?}");
    }

    #[test]
    fn a_frame_corner_drag_writes_one_undo_entry() {
        let (project, id) = sample_product_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let before = session.project().revision().get();
        let scope = session.edit_scope();
        let envelope = |width: i64, depth: i64| {
            Envelope::new(millimetres(width), millimetres(depth), millimetres(720)).expect("an envelope")
        };
        for (width, depth) in [(640, 600), (700, 660)] {
            let change = BoxChange::Envelope(envelope(width, depth));
            apply_grip_edit(&mut session, GripEdit { node: id, change, placement: None }, scope).expect("a drag frame");
        }
        session.end_edit();
        assert_eq!(session.project().revision().get(), before + 2, "each frame is one command, both in one entry");
        assert_eq!(envelope_sides(&session, id), [700.0, 660.0, 720.0]);
        session.undo().expect("one undo returns the whole drag");
        assert_eq!(envelope_sides(&session, id), [600.0, 560.0, 720.0], "the frame the drag started from is back");
    }

    #[test]
    fn a_frame_minimum_face_drag_keeps_the_opposite_face_still_in_the_world() {
        let (project, id) = sample_product_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let scope = session.edit_scope();
        // Drag the min-X face 100 mm to the left: the reserved box grows to 700 and the zero moves
        // 100 mm in the frame's own axis, so the far face does not move.
        let (_, shift) = dragged_side(millimetres(600), -100.0, true).expect("a wider frame");
        let envelope =
            Envelope::new(millimetres(700), millimetres(560), millimetres(720)).expect("the dragged envelope");
        let placement = Placement::new(DVec3::new(shift, 0.0, 0.0), default_rotation()).expect("placement");
        let change = BoxChange::Envelope(envelope);
        apply_grip_edit(&mut session, GripEdit { node: id, change, placement: Some(placement) }, scope)
            .expect("the drag");
        session.end_edit();
        let world = session.project().world_placement(id).expect("world placement");
        let far = world.transform_point(DVec3::new(700.0, 0.0, 0.0));
        assert!((far - DVec3::new(600.0, 0.0, 0.0)).length() < 1.0e-9, "the far face stayed put, was {far:?}");
        assert_eq!(envelope_sides(&session, id)[0], 700.0);
    }

    /// The identity rotation a placement needs in a test that only moves an origin.
    fn default_rotation() -> furnicraft_geom::DQuat {
        furnicraft_geom::DQuat::IDENTITY
    }
}
