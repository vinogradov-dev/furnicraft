//! The furniture move rule: how far one drag along a world axis may travel.
//!
//! The translate gizmo proposes a world distance and this module answers with the distance the
//! document allows. Three facts decide it and none of them is a code literal:
//!
//! - the **mount** trait of a product's class: a `Floor` product keeps its height, a `Wall` product
//!   moves freely (ADR 0097);
//! - **contact**: a shell stops at another shell and a body at another body, while containment of
//!   one shell in another stays legal (ADR 0102);
//! - **snapping**: the moved box's faces fall onto the floor, onto the virtual faces of the product
//!   frames around it, onto a neighbouring part's face, or onto the 1 mm grid.
//!
//! The answer is derived from the placements the document holds, which during a drag are still the
//! drag's start placements, so the preview and the single `node.placement.set_many` on release
//! cannot disagree. Nothing is stored and nothing is mutated.
//!
//! A drag in which no product is involved — free geometry, a panel at the root — is not a furniture
//! move: [`Project::move_rule`] answers `None` and the gizmo keeps its exact micrometre move.

use furnicraft_geom::DVec3;

use crate::project::Project;
use crate::tree::{object_bounds_mm, panel_bounds};
use crate::*;

/// The world axis a furniture drag runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveAxis {
    /// The world X axis.
    X,
    /// The world Y axis.
    Y,
    /// The world Z axis, up.
    Z,
}

impl MoveAxis {
    /// The two axes perpendicular to this one, in frame order.
    const fn perpendicular(self) -> [Self; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::X, Self::Z],
            Self::Z => [Self::X, Self::Y],
        }
    }
}

/// How far a furniture drag may go, and what stopped it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveLimit {
    /// The signed distance the drag may travel along its world axis, in millimetres.
    ///
    /// It is a whole number of micrometres, which is the document's unit, so the preview and the
    /// committed placement are the same number.
    pub distance_mm: f64,
    /// The node whose surface the drag stopped at, when contact stopped it before the pointer did.
    pub contact: Option<Id>,
}

/// The grid step of a furniture move, in millimetres.
///
/// The maker's own clearance is one millimetre (ADR 0102), so a furniture drag lands on the whole
/// millimetre unless a surface captures it first.
const GRID_MM: f64 = 1.0;

/// How near a surface must be, in millimetres, to capture the drag.
const SNAP_RADIUS_MM: f64 = 5.0;

impl Project {
    /// The rule that measures a drag of `moving`, or `None` when the drag is not furniture.
    ///
    /// A drag is a furniture move when every top-most moved node is a product or sits inside one.
    /// The surfaces and the mount trait are read here, once, so the drag can ask the rule for every
    /// pointer position without walking the tree again: nothing the rule measured moves while the
    /// pointer travels.
    pub fn move_rule(&self, moving: &[Id]) -> Option<MoveRule> {
        let root = self.root.id();
        let targets: Vec<Id> = self.top_most_nodes(moving).into_iter().filter(|id| *id != root).collect();
        if targets.is_empty() || !targets.iter().all(|id| self.is_furniture(*id)) {
            return None;
        }
        let movers = self.mover_boxes(&targets);
        if movers.is_empty() {
            return None;
        }
        Some(MoveRule { floor_bound: self.is_floor_bound(&targets), obstacles: self.obstacle_boxes(&targets), movers })
    }

    /// Whether `id` is furniture: a product, or a node inside a product.
    fn is_furniture(&self, id: Id) -> bool {
        self.container(id).is_some_and(Container::is_product) || self.product_of(id).is_some()
    }

    /// Whether any of the products stands on the floor by its class's `mount` trait.
    fn is_floor_bound(&self, products: &[Id]) -> bool {
        products.iter().any(|id| {
            self.container(*id).is_some_and(Container::is_product)
                && self.class(*id).and_then(|class| class.traits()).and_then(|traits| traits.mount())
                    == Some(Mount::Floor)
        })
    }

    /// The world box each moved node carries.
    fn mover_boxes(&self, targets: &[Id]) -> Vec<SurfaceBox> {
        targets.iter().filter_map(|id| self.mover_box(*id)).collect()
    }

    /// The world box one moved node carries: a product's envelope, a panel's or object's own
    /// bounds, and for any other container the union of the parts below it.
    fn mover_box(&self, id: Id) -> Option<SurfaceBox> {
        if let Some(container) = self.container(id) {
            if container.is_product() {
                return self.envelope_box(id);
            }
            let (min, max) = panel_bounds(container.children(), Placement::IDENTITY, 0)?;
            return surface_box(id, min, max, self.world_placement(id)?, false);
        }
        if let Some(panel) = self.panel(id) {
            let size = panel.nominal_size();
            let max = DVec3::new(
                size.length().millimetres_f64(),
                size.width().millimetres_f64(),
                panel.last_known_thickness().millimetres_f64(),
            );
            return surface_box(id, DVec3::ZERO, max, self.world_placement(id)?, false);
        }
        let object = self.object(id)?;
        let (min, max) = object_bounds_mm(object.geometry())?;
        surface_box(id, min, max, self.world_placement(id)?, false)
    }

    /// The world box of a product's authored envelope.
    fn envelope_box(&self, id: Id) -> Option<SurfaceBox> {
        let envelope = self.container(id)?.envelope()?;
        let size =
            DVec3::new(envelope.x().millimetres_f64(), envelope.y().millimetres_f64(), envelope.z().millimetres_f64());
        surface_box(id, DVec3::ZERO, size, self.world_placement(id)?, true)
    }

    /// The surfaces a furniture drag may meet: every other product's envelope, and every other
    /// panel and object.
    fn obstacle_boxes(&self, targets: &[Id]) -> Vec<SurfaceBox> {
        let mut boxes: Vec<SurfaceBox> = self
            .products()
            .into_iter()
            .filter(|product| !covered_by(self, *product, targets))
            .filter_map(|product| self.envelope_box(product))
            .collect();
        self.body_boxes(&self.root, targets, &mut boxes, 1);
        boxes
    }

    /// Collects the panels and objects below `container` that stay behind when `targets` move.
    fn body_boxes(&self, container: &Container, targets: &[Id], boxes: &mut Vec<SurfaceBox>, depth: u32) {
        if depth > Self::MAX_ASSEMBLY_DEPTH {
            return;
        }
        for child in container.children() {
            match child {
                Node::Panel(_) | Node::Object(_) => {
                    if covered_by(self, child.id(), targets) {
                        continue;
                    }
                    if let Some(box_) = self.mover_box(child.id()) {
                        boxes.push(box_);
                    }
                }
                Node::Container(inner) => self.body_boxes(inner, targets, boxes, depth.saturating_add(1)),
                Node::Camera(_) | Node::Light(_) => {}
            }
        }
    }
}

/// What one furniture drag is measured against, taken once when the drag starts.
///
/// The document does not change while the pointer travels, so the moved boxes, the other surfaces
/// and the mount trait are read once and every later proposal is answered from them. Nothing is
/// stored in the document and nothing is mutated.
#[derive(Clone, Debug)]
pub struct MoveRule {
    /// The world box each moved node carries at the start of the drag.
    movers: Vec<SurfaceBox>,
    /// The surfaces that stay behind: the other products' envelopes, panels and objects.
    obstacles: Vec<SurfaceBox>,
    /// Whether a moved product stands on the floor by its class's `mount` trait.
    floor_bound: bool,
}

impl MoveRule {
    /// The distance a proposal may travel along a world axis.
    ///
    /// The proposal is zeroed on the vertical when a moved product is floor-bound, then snapped to
    /// the 1 mm grid, the floor or a surface within [`SNAP_RADIUS_MM`], and finally cut at the first
    /// surface in the way; the contact is reported so a caller can say what stopped the drag. A
    /// proposal that is not finite comes back as no movement.
    pub fn limit(&self, axis: MoveAxis, proposed_mm: f64) -> MoveLimit {
        if !proposed_mm.is_finite() {
            return MoveLimit { distance_mm: 0.0, contact: None };
        }
        // A floor-bound product keeps its height: the mount trait forbids the vertical drag, and
        // saying so here keeps the rule out of the gizmo.
        let distance = if axis == MoveAxis::Z && self.floor_bound { 0.0 } else { proposed_mm };
        let snapped = snap_distance(axis, &self.movers, &self.obstacles, distance);
        let limit = stop_at_contact(axis, snapped, &self.movers, &self.obstacles);
        MoveLimit { distance_mm: quantised(limit.distance_mm), contact: limit.contact }
    }
}

/// One furniture surface as an axis-aligned world box, in millimetres.
#[derive(Clone, Copy, Debug)]
struct SurfaceBox {
    id: Id,
    min: DVec3,
    max: DVec3,
    /// A product's envelope is a shell; a panel, an object or any other container is a body.
    shell: bool,
}

/// One axis of a [`SurfaceBox`].
#[derive(Clone, Copy, Debug)]
struct Span {
    min: f64,
    max: f64,
}

impl SurfaceBox {
    /// The box's extent on `axis`.
    fn span(self, axis: MoveAxis) -> Span {
        match axis {
            MoveAxis::X => Span { min: self.min.x, max: self.max.x },
            MoveAxis::Y => Span { min: self.min.y, max: self.max.y },
            MoveAxis::Z => Span { min: self.min.z, max: self.max.z },
        }
    }
}

/// A local box through a node's world placement, as the world box around its eight corners.
///
/// A turned body has no axis-aligned sides any more, so the tightest box around the corners is
/// used, exactly as the product placement query does. A corner that is not finite is no box.
fn surface_box(id: Id, min: DVec3, max: DVec3, world: Placement, shell: bool) -> Option<SurfaceBox> {
    let mut box_ = SurfaceBox { id, min: DVec3::splat(f64::INFINITY), max: DVec3::splat(f64::NEG_INFINITY), shell };
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                let corner = world.transform_point(DVec3::new(x, y, z));
                if !corner.is_finite() {
                    return None;
                }
                box_.min = box_.min.min(corner);
                box_.max = box_.max.max(corner);
            }
        }
    }
    Some(box_)
}

/// Whether `node` is one of `targets` or sits below one of them.
fn covered_by(project: &Project, node: Id, targets: &[Id]) -> bool {
    let mut current = Some(node);
    let mut depth = 0;
    while let Some(id) = current {
        if targets.contains(&id) {
            return true;
        }
        if depth >= Project::MAX_ASSEMBLY_DEPTH {
            return false;
        }
        current = project.parent_of(id);
        depth = depth.saturating_add(1);
    }
    false
}

/// The proposed distance after snapping, in whole millimetres unless a surface captures it.
///
/// A face of the moved box that is within [`SNAP_RADIUS_MM`] of a candidate surface wins: the
/// floor, or either face of an obstacle along the axis — a product's virtual face or a part's own.
/// Only a surface that faces the moving box across it counts, so a part in the far corner of the
/// scene cannot pull a drag. The nearest capture wins a tie by the order of the walk, so the
/// answer never depends on the direction the pointer approached from.
fn snap_distance(axis: MoveAxis, movers: &[SurfaceBox], obstacles: &[SurfaceBox], proposed: f64) -> f64 {
    if proposed == 0.0 {
        return 0.0;
    }
    let [first, second] = axis.perpendicular();
    let overlap = |a: Span, b: Span| a.min < b.max && b.min < a.max;
    let mut best: Option<(f64, f64)> = None;
    for mover in movers {
        let mover_first = mover.span(first);
        let mover_second = mover.span(second);
        let mut capture = |candidate: f64| {
            for own in [mover.span(axis).min, mover.span(axis).max] {
                let distance = candidate - own;
                let offset = (distance - proposed).abs();
                // A face behind the pointer would pull the drag backwards, which is not a snap but
                // a twitch: only a surface ahead of the proposal captures it.
                let reverses = distance * proposed < 0.0;
                if reverses || !distance.is_finite() || !offset.is_finite() || offset > SNAP_RADIUS_MM {
                    continue;
                }
                if best.is_none_or(|(nearest, _)| offset < nearest) {
                    best = Some((offset, distance));
                }
            }
        };
        for obstacle in obstacles {
            if !overlap(mover_first, obstacle.span(first)) || !overlap(mover_second, obstacle.span(second)) {
                continue;
            }
            capture(obstacle.span(axis).min);
            capture(obstacle.span(axis).max);
        }
        if axis == MoveAxis::Z {
            // The floor lies under every piece, so it captures without a perpendicular test.
            capture(0.0);
        }
    }
    best.map_or_else(|| round_to_grid(proposed), |(_, distance)| distance)
}

/// Rounds a distance to the micrometre the document stores, so the preview and the placement that
/// records it are the same number.
fn quantised(value: f64) -> f64 {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// Rounds a drag distance to the furniture grid.
fn round_to_grid(value: f64) -> f64 {
    let rounded = (value / GRID_MM).round() * GRID_MM;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// The snapped distance cut at the first surface in its way.
fn stop_at_contact(axis: MoveAxis, distance: f64, movers: &[SurfaceBox], obstacles: &[SurfaceBox]) -> MoveLimit {
    let sign = if distance < 0.0 { -1.0 } else { 1.0 };
    if distance == 0.0 {
        return MoveLimit { distance_mm: 0.0, contact: None };
    }
    let mut nearest: Option<(f64, Id)> = None;
    for mover in movers {
        for obstacle in obstacles {
            // A body stops at a body and a shell at a shell (ADR 0102).
            if mover.shell != obstacle.shell {
                continue;
            }
            let Some(limit) = blocking(axis, sign, mover, obstacle) else {
                continue;
            };
            if nearest.is_none_or(|(best, _)| limit < best) {
                nearest = Some((limit, obstacle.id));
            }
        }
    }
    match nearest {
        Some((limit, contact)) if distance.abs() > limit => {
            MoveLimit { distance_mm: limit * sign, contact: Some(contact) }
        }
        _ => MoveLimit { distance_mm: distance, contact: None },
    }
}

/// The distance towards `sign` before `moving` meets `obstacle`, or `None` when this pair does not
/// block the drag.
///
/// The two boxes must share a volume across the movement — touching faces are contact, not overlap.
/// Along it, a body may only stay clear of another body, while one shell may sit inside another:
/// a drag that keeps the containment stops at the wall it would cross, and a pair that already
/// overlaps one another partially is not the tool's to police.
fn blocking(axis: MoveAxis, sign: f64, moving: &SurfaceBox, obstacle: &SurfaceBox) -> Option<f64> {
    let [first, second] = axis.perpendicular();
    let overlap = |a: Span, b: Span| a.min < b.max && b.min < a.max;
    let moving_first = moving.span(first);
    let obstacle_first = obstacle.span(first);
    let moving_second = moving.span(second);
    let obstacle_second = obstacle.span(second);
    if !overlap(moving_first, obstacle_first) || !overlap(moving_second, obstacle_second) {
        return None;
    }
    let contains = |outer: Span, inner: Span| outer.min <= inner.min && inner.max <= outer.max;
    let shells = moving.shell && obstacle.shell;
    let moving_span = moving.span(axis);
    let obstacle_span = obstacle.span(axis);
    let inside = obstacle_span.min <= moving_span.min && moving_span.max <= obstacle_span.max;
    let holds = moving_span.min <= obstacle_span.min && obstacle_span.max <= moving_span.max;
    let contained =
        shells && inside && contains(obstacle_first, moving_first) && contains(obstacle_second, moving_second);
    let containing =
        shells && holds && contains(moving_first, obstacle_first) && contains(moving_second, obstacle_second);
    let distance = if sign >= 0.0 {
        if moving_span.max <= obstacle_span.min {
            obstacle_span.min - moving_span.max
        } else if moving_span.min >= obstacle_span.max {
            return None;
        } else if contained {
            obstacle_span.max - moving_span.max
        } else if containing {
            obstacle_span.min - moving_span.min
        } else {
            return None;
        }
    } else if moving_span.min >= obstacle_span.max {
        moving_span.min - obstacle_span.max
    } else if moving_span.max <= obstacle_span.min {
        return None;
    } else if contained {
        moving_span.min - obstacle_span.min
    } else if containing {
        moving_span.max - obstacle_span.max
    } else {
        return None;
    };
    distance.is_finite().then_some(distance)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_units::Degrees;

    use crate::fixtures::*;
    use crate::*;

    /// A product of `width x depth x height` millimetres, standing at `(x, y, z)` in the root frame.
    fn product(project: &mut Project, code: &str, size: (i64, i64, i64), at: (i64, i64, i64)) -> Id {
        let root = project.root().id();
        let (width, depth, height) = size;
        let envelope =
            Envelope::new(millimetres(width), millimetres(depth), millimetres(height)).expect("a valid envelope");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(at.0), millimetres(at.1), millimetres(at.2))
            .expect("a floor placement");
        project.add_product(root, "Piece", class(code), envelope, placement).expect("add product")
    }

    /// A `700 x 500 x 18` millimetre panel inside `parent`, with its base corner at `at`.
    fn panel(project: &mut Project, parent: Id, name: &str, at: (i64, i64, i64)) -> Id {
        let placement = Placement::IDENTITY
            .with_translation(millimetres(at.0), millimetres(at.1), millimetres(at.2))
            .expect("a panel placement");
        project.add_panel(parent, name, sample_nominal(), None, sample_thickness(), placement).expect("add panel")
    }

    /// The sample scene with one empty product of `600 x 560 x 720` millimetres at the origin.
    fn product_project(code: &str) -> (Project, Id) {
        let mut project = sample_project();
        let id = product(&mut project, code, (600, 560, 720), (0, 0, 0));
        (project, id)
    }

    fn rule(project: &Project, moving: &[Id]) -> MoveRule {
        project.move_rule(moving).expect("a furniture move")
    }

    fn limit(project: &Project, moving: &[Id], axis: MoveAxis, proposed: f64) -> MoveLimit {
        rule(project, moving).limit(axis, proposed)
    }

    #[test]
    fn the_mount_trait_decides_whether_a_product_keeps_its_height() {
        let (floor, floor_id) = product_project("30.01.01");
        assert_eq!(
            limit(&floor, &[floor_id], MoveAxis::Z, 120.0),
            MoveLimit { distance_mm: 0.0, contact: None },
            "a base cabinet stands on the floor and cannot be lifted"
        );

        let (wall, wall_id) = product_project("30.01.02");
        assert_eq!(
            limit(&wall, &[wall_id], MoveAxis::Z, 120.0),
            MoveLimit { distance_mm: 120.0, contact: None },
            "a wall cabinet hangs where the maker puts it"
        );

        // The same node, not a second one: the row decides, never the container or the command.
        let mut changed = floor;
        changed
            .set_property(floor_id, SYSTEM_CLASS, PropertyValue::class(class("30.01.02")))
            .expect("a wall class keeps the envelope");
        assert_eq!(
            limit(&changed, &[floor_id], MoveAxis::Z, 120.0),
            MoveLimit { distance_mm: 120.0, contact: None },
            "the same product may be hung once its class says so"
        );
    }

    #[test]
    fn a_floor_bound_product_still_slides_and_snaps_on_the_floor() {
        let (project, id) = product_project("30.01.01");
        assert_eq!(limit(&project, &[id], MoveAxis::X, 297.4), MoveLimit { distance_mm: 297.0, contact: None });
    }

    #[test]
    fn a_shell_stops_at_contact_with_another_shell() {
        let (mut project, cabinet) = product_project("30.01.01");
        let neighbour = product(&mut project, "30.01.01", (600, 560, 720), (1000, 0, 0));
        // The cabinet spans x = 0..600 and the neighbour starts at 1000: the drag stops at 400.
        assert_eq!(
            limit(&project, &[cabinet], MoveAxis::X, 1000.0),
            MoveLimit { distance_mm: 400.0, contact: Some(neighbour) }
        );
        // The neighbour may come back the same way and stop at its own contact.
        assert_eq!(
            limit(&project, &[neighbour], MoveAxis::X, -1000.0),
            MoveLimit { distance_mm: -400.0, contact: Some(cabinet) }
        );
    }

    #[test]
    fn a_contained_shell_moves_inside_until_it_would_cross_a_wall() {
        let mut project = sample_project();
        let outer = product(&mut project, "30.01.02", (2000, 1000, 2000), (0, 0, 0));
        let inner = product(&mut project, "30.01.02", (600, 500, 700), (700, 200, 500));
        assert_eq!(
            limit(&project, &[inner], MoveAxis::Z, 1500.0),
            MoveLimit { distance_mm: 800.0, contact: Some(outer) },
            "a shelf unit inside a niche moves freely until its top meets the ceiling"
        );
        assert_eq!(
            limit(&project, &[inner], MoveAxis::Z, -1500.0),
            MoveLimit { distance_mm: -500.0, contact: Some(outer) },
            "and down until it stands on the floor of the outer piece"
        );
    }

    #[test]
    fn a_body_stops_at_a_body_and_not_at_a_shell() {
        let (mut project, piece) = product_project("30.01.01");
        // Away from the sample panels at the origin, so only the two panels below are measured.
        let shelf = panel(&mut project, piece, "Shelf", (0, 1000, 0));
        let other = panel(&mut project, piece, "Other", (1000, 1000, 0));
        assert_eq!(
            limit(&project, &[shelf], MoveAxis::X, 1000.0),
            MoveLimit { distance_mm: 300.0, contact: Some(other) }
        );

        // A shell is not a body: the same shelf slides up to a neighbouring frame untouched.
        let (mut project, piece) = product_project("30.01.01");
        let shelf = panel(&mut project, piece, "Shelf", (0, 0, 0));
        let _frame = product(&mut project, "30.01.01", (600, 560, 720), (1000, 0, 0));
        assert_eq!(limit(&project, &[shelf], MoveAxis::X, 1000.0), MoveLimit { distance_mm: 1000.0, contact: None });
    }

    #[test]
    fn a_drag_lands_on_the_grid_and_on_a_face() {
        // A lone part inside a piece: nothing to meet, so the 1 mm grid decides.
        let (mut project, piece) = product_project("30.01.01");
        let shelf = panel(&mut project, piece, "Shelf", (0, 1000, 0));
        assert_eq!(limit(&project, &[shelf], MoveAxis::X, 97.4), MoveLimit { distance_mm: 97.0, contact: None });

        // A neighbour frame's virtual face at x = 1000 captures the shelf's far face, and a body
        // never stops at a shell, so the face — not the grid — is the answer.
        let _frame = product(&mut project, "30.01.01", (600, 560, 720), (1000, 0, 0));
        let shelf = panel(&mut project, piece, "Second shelf", (0, 0, 0));
        assert_eq!(
            limit(&project, &[shelf], MoveAxis::X, 297.4),
            MoveLimit { distance_mm: 300.0, contact: None },
            "the shelf's far face lands on the neighbouring frame's virtual face, not on the grid"
        );

        // A neighbouring part's face captures the same drag: the drag lands on it, not on 297.
        let _beside = panel(&mut project, piece, "Beside", (1000, 0, 0));
        assert_eq!(
            limit(&project, &[shelf], MoveAxis::X, 297.4),
            MoveLimit { distance_mm: 300.0, contact: None },
            "a neighbouring part face is a snap target too"
        );
    }

    #[test]
    fn a_wall_product_lands_on_the_floor() {
        let mut project = sample_project();
        let hanging = product(&mut project, "30.01.02", (600, 560, 720), (0, 0, 3));
        assert_eq!(
            limit(&project, &[hanging], MoveAxis::Z, -1.5),
            MoveLimit { distance_mm: -3.0, contact: None },
            "a piece 1.5 mm above the floor lands on it"
        );
    }

    #[test]
    fn a_drag_outside_furniture_is_not_constrained() {
        // A loose panel at the root: no product, so the gizmo keeps its exact move.
        let project = sample_project();
        let loose = Id::new(2).expect("id");
        assert!(project.move_rule(&[loose]).is_none());
        assert!(project.move_rule(&[]).is_none());
    }

    #[test]
    fn a_non_finite_proposal_moves_nothing() {
        let (project, id) = product_project("30.01.01");
        assert_eq!(rule(&project, &[id]).limit(MoveAxis::X, f64::NAN), MoveLimit { distance_mm: 0.0, contact: None });
    }

    #[test]
    fn a_turned_product_is_measured_by_its_world_box() {
        let (mut project, cabinet) = product_project("30.01.01");
        let neighbour = product(&mut project, "30.01.01", (600, 560, 720), (1000, 0, 0));
        // Turn the cabinet a quarter about Z: its depth (560) now runs along +X, so its world box
        // reaches x = 560 and the drag may travel 440 mm before contact.
        let turned = Placement::IDENTITY
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(-90_000), ..EulerAngles::default() })
            .expect("a quarter turn");
        project.set_node_placement(cabinet, turned).expect("turn the cabinet");
        assert_eq!(
            limit(&project, &[cabinet], MoveAxis::X, 1000.0),
            MoveLimit { distance_mm: 440.0, contact: Some(neighbour) }
        );
    }
}
