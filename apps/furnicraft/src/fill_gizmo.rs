//! The empty-space tool: the ghost of the part the next click would create.
//!
//! A post or a shelf fills the niche under the pointer. The model answers the measurement
//! ([`Project::niche`], ADR 0103); this module turns the answer into the one panel the click would
//! create and paints it, and touches nothing else. The ghost carries exactly the outline, size,
//! placement and class the command receives, so what the maker sees is what the history records —
//! one `panel.create` per click, one undo entry.
//!
//! `Fill::HeightAndDepth` stands a part up as a post — the niche's height becomes the panel's
//! length, its depth the width, and the material's thickness runs along the free axis, where the
//! pointer sits. `Fill::WidthAndDepth` lays a part down as a shelf — the niche's width and depth
//! become the panel's sides and the pointer picks the height. The free-axis position moves in whole
//! millimetres; the other sides are the measured void exactly, so a part is flush with the walls it
//! was cut to.
//!
//! A shelf in a splayed niche takes the niche's section as its outline (ADR 0099): the model answers
//! a polygon and the click creates a contour panel with exactly that ring, through the same
//! `panel.create`. A post is a flat board across the depth, so a niche whose section is not a
//! rectangle has no post to offer and the tool says so instead of cutting a board into the walls.
//!
//! The ghost is shell paint, not the session preview overlay: an overlay entry is keyed by the id of
//! a node the document holds (ADR 0093), and a part that does not exist yet has none.

use std::f64::consts::FRAC_PI_2;

use eframe::egui::{self, FontId, Pos2, Rect, Stroke, Ui};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::{Contour, Fill, Id, Niche, NicheRefusal, Panel, PanelNominalSize, Placement, Project};
use furnicraft_scene::{Camera, Ray};
use furnicraft_ui::theme;
use furnicraft_units::Length;

use crate::gizmo_math::screen_point;

/// How far the pointer may move, in millimetres, before the problem is measured again.
const MEASURE_STEP_MM: f64 = 0.5;

/// The panel the next click would create.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FillGhost {
    /// The open product the panel lands in.
    pub(crate) parent: Id,
    /// The ring the panel is born with, when the niche's section is not a rectangle.
    pub(crate) outline: Option<Contour>,
    /// The panel's nominal length and width, exactly as the command will store them.
    pub(crate) nominal_size: PanelNominalSize,
    /// The panel's placement in the parent frame, exactly as the command will store it.
    pub(crate) placement: Placement,
    /// The void the part was measured against.
    pub(crate) niche: Niche,
    /// The panel's three extents, in millimetres, for the callouts.
    pub(crate) size_mm: DVec3,
    /// The outline's corners on the panel's base face, in world millimetres.
    world: Vec<DVec3>,
    /// The same corners on the panel's far face.
    top: Vec<DVec3>,
    /// The panel's own length and width directions in the world, for the callouts.
    axes: [DVec3; 2],
}

impl FillGhost {
    /// The midpoint of the panel's low face along its thickness axis, in world millimetres.
    pub(crate) fn low_face(&self) -> Option<DVec3> {
        midpoint(&self.world)
    }

    /// The midpoint of the panel's far face along its thickness axis, in world millimetres.
    pub(crate) fn high_face(&self) -> Option<DVec3> {
        midpoint(&self.top)
    }
}

/// The average of a ring's corners, or `None` when it is empty or not finite.
fn midpoint(points: &[DVec3]) -> Option<DVec3> {
    if points.is_empty() {
        return None;
    }
    let mut sum = DVec3::ZERO;
    for point in points {
        sum += *point;
    }
    let middle = sum / points.len() as f64;
    middle.is_finite().then_some(middle)
}

/// The empty-space tool's live state: the ghost, or the reason there is none.
#[derive(Debug, Default)]
pub(crate) struct FillGizmo {
    ghost: Option<FillGhost>,
    refusal: Option<String>,
    /// The point, the fill and the exact free coordinate the ghost was measured at, so a still
    /// pointer re-measures nothing.
    measured: Option<(Id, DVec3, Fill, Length, Option<f64>)>,
}

impl FillGizmo {
    /// Drops the ghost and the refusal, for example when the tool is disarmed.
    pub(crate) fn reset(&mut self) {
        self.ghost = None;
        self.refusal = None;
        self.measured = None;
    }

    /// The part the next click would create, if any.
    pub(crate) fn ghost(&self) -> Option<&FillGhost> {
        self.ghost.as_ref()
    }

    /// The point the ghost was last measured at, or `None` when nothing was.
    ///
    /// A distance field floats over the scene, and a pointer on it is chrome: the ghost keeps the
    /// point it was measured at instead of jumping to the field's own position.
    pub(crate) fn measured_point(&self) -> Option<DVec3> {
        self.measured.map(|(_, point, ..)| point)
    }

    /// Measures the void around `point` and records the ghost, unless nothing moved.
    ///
    /// Returns whether the ghost or the refusal changed, so the caller can ask for a repaint.
    pub(crate) fn hover(
        &mut self,
        project: &Project,
        product: Id,
        point: DVec3,
        fill: Fill,
        thickness: Length,
        free: Option<f64>,
    ) -> bool {
        if self.measured.is_some_and(
            |(measured_product, measured_point, measured_fill, measured_thickness, measured_free)| {
                measured_product == product
                    && measured_fill == fill
                    && measured_thickness == thickness
                    && measured_free == free
                    && (measured_point - point).length() < MEASURE_STEP_MM
            },
        ) {
            return false;
        }
        self.measured = Some((product, point, fill, thickness, free));
        match plan_at(project, product, point, fill, thickness, free) {
            Ok(ghost) => {
                self.ghost = Some(ghost);
                self.refusal = None;
            }
            Err(reason) => {
                self.ghost = None;
                self.refusal = Some(reason);
            }
        }
        true
    }

    /// Paints the ghost's wireframe and the three sizes, in the theme's accent.
    ///
    /// The outline is a wireframe, not a solid: it must read through the panels around it and never
    /// hide the void it measures. A rectangle draws its twelve edges; a shaped shelf draws its two
    /// rims and one edge per corner, so a trapezoid reads as a trapezoid before the click.
    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, camera: Camera) {
        let painter = ui.painter().with_clip_rect(rect);
        let Some(ghost) = self.ghost.as_ref() else {
            // An armed tool with no ghost says why: the pointer is outside the open piece, the void
            // is too small for the material, or its section is not one shape.
            if let Some(reason) = self.refusal.as_deref() {
                label(ui, &painter, rect.center(), reason);
            }
            return;
        };
        let ink = theme::accent(ui);
        let stroke = Stroke::new(1.6, ink);
        let count = ghost.world.len();
        if count < 3 || ghost.top.len() != count {
            return;
        }
        for index in 0..count {
            let next = (index + 1) % count;
            let (Some(base), Some(far), Some(corner), Some(raised)) =
                (ghost.world.get(index), ghost.world.get(next), ghost.top.get(index), ghost.top.get(next))
            else {
                continue;
            };
            for (from, to) in [(*base, *far), (*corner, *raised), (*base, *corner)] {
                if let (Some(start), Some(end)) = (screen_point(camera, rect, from), screen_point(camera, rect, to)) {
                    painter.line_segment([start, end], stroke);
                }
            }
        }
        for (axis, millimetres) in [(0usize, ghost.size_mm.x), (1, ghost.size_mm.y)] {
            let Some(edge) = widest_edge(&ghost.world, ghost.axes[axis]) else {
                continue;
            };
            let Some(next) = ghost.world.get((edge + 1) % count) else {
                continue;
            };
            let Some(start) = ghost.world.get(edge) else {
                continue;
            };
            let Some(midpoint) = screen_point(camera, rect, (*start + *next) * 0.5) else {
                continue;
            };
            label(ui, &painter, midpoint, &format!("{millimetres:.0}"));
        }
        if let (Some(base), Some(raised)) = (ghost.world.first(), ghost.top.first())
            && let Some(midpoint) = screen_point(camera, rect, (*base + *raised) * 0.5)
        {
            label(ui, &painter, midpoint, &format!("{:.0}", ghost.size_mm.z));
        }
    }
}

/// The rim edge that runs furthest along `axis`, for a size callout.
fn widest_edge(points: &[DVec3], axis: DVec3) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for index in 0..points.len() {
        let (Some(start), Some(end)) = (points.get(index), points.get((index + 1) % points.len())) else {
            continue;
        };
        let span = (*end - *start).dot(axis).abs();
        if best.is_none_or(|(widest, _)| span > widest) {
            best = Some((span, index));
        }
    }
    best.map(|(_, index)| index)
}

/// Measures the void at `point` and answers the part a click would create.
///
/// The point has to be inside `product`, which is the open piece: a niche belongs to the working
/// product (ADR 0098). The measurement is the model's ([`Project::niche`]); the free-axis position
/// is the only number this function rounds, because it follows the pointer in whole millimetres.
#[cfg(test)]
pub(crate) fn plan(
    project: &Project,
    product: Id,
    point: DVec3,
    fill: Fill,
    thickness: Length,
) -> Result<FillGhost, String> {
    plan_at(project, product, point, fill, thickness, None)
}

/// The axis of the open product's frame the fill's free position runs along.
///
/// A shelf's height and a post's place across the void both lie on the panel's own thickness axis,
/// so the distance fields measure the two faces the ghost draws (`world` and `top`).
pub(crate) const fn free_axis(fill: Fill) -> usize {
    match fill {
        Fill::WidthAndDepth => 2,
        Fill::HeightAndDepth => 0,
    }
}

/// Measures the void at `point` with the free position fixed at `free`, in the product's frame.
///
/// `free` is `None` while the pointer picks the position. A typed distance pins it, so the ghost
/// lands exactly there instead of following the next pointer movement (ADR 0103).
pub(crate) fn plan_at(
    project: &Project,
    product: Id,
    point: DVec3,
    fill: Fill,
    thickness: Length,
    free: Option<f64>,
) -> Result<FillGhost, String> {
    let niche = project.niche(point).map_err(refusal_sentence)?;
    if niche.product != product {
        return Err("The void belongs to another product; open it to fill it.".to_owned());
    }
    let parent_world = project.world_placement(product).ok_or_else(|| "The open product has no frame.".to_owned())?;
    let inverse = parent_world.inverse().map_err(|_| "The open product has no inverse frame.".to_owned())?;
    let local = inverse.transform_point(point);
    let minimum = niche.local_min;
    let maximum = niche.local_max();
    let gap = thickness.millimetres_f64();
    if !local.is_finite() || !minimum.is_finite() || !maximum.is_finite() || !gap.is_finite() || gap <= 0.0 {
        return Err("The void cannot be measured.".to_owned());
    }

    let (outline, nominal_size, placement) = match fill {
        Fill::WidthAndDepth => shelf(&niche, local, minimum, maximum, gap, free)?,
        Fill::HeightAndDepth => post(&niche, local, minimum, maximum, gap, free)?,
    };
    let size_mm = DVec3::new(nominal_size.length().millimetres_f64(), nominal_size.width().millimetres_f64(), gap);
    let length = size_mm.x;
    let width = size_mm.y;
    let world_placement = Placement::compose(parent_world, placement)
        .map_err(|_| "The part cannot be placed in the open product.".to_owned())?;
    let base = |x: f64, y: f64| world_placement.transform_point(DVec3::new(x, y, 0.0));
    let world = match outline.as_ref() {
        Some(ring) => ring_corners(ring, base)?,
        None => vec![(0.0, 0.0), (length, 0.0), (length, width), (0.0, width)]
            .into_iter()
            .map(|(x, y)| base(x, y))
            .collect(),
    };
    // The far face is the base face moved by the material thickness along the panel's own Z axis: a
    // shelf's thickness is vertical, a post's runs across the void.
    let lift = world_placement.transform_vector(DVec3::Z) * gap;
    let top = world.iter().map(|corner| *corner + lift).collect::<Vec<_>>();
    if world.iter().chain(top.iter()).any(|corner| !corner.is_finite()) {
        return Err("The part cannot be placed in the open product.".to_owned());
    }
    let axes = [world_placement.transform_vector(DVec3::X), world_placement.transform_vector(DVec3::Y)];
    Ok(FillGhost { parent: product, outline, nominal_size, placement, niche, size_mm, world, top, axes })
}

/// The shelf a click would create: the void's section as its outline, the pointer as its height.
///
/// A rectangular section keeps the rectangle panel the tool has always created; a shaped one is a
/// contour panel whose ring is the measured section (ADR 0042). Both are born from the same
/// `panel.create`, so the maker sees one gesture and one undo entry either way.
fn shelf(
    niche: &Niche,
    local: DVec3,
    minimum: DVec3,
    maximum: DVec3,
    gap: f64,
    free: Option<f64>,
) -> Result<(Option<Contour>, PanelNominalSize, Placement), String> {
    let low = minimum.z;
    let high = maximum.z - gap;
    if high < low {
        return Err("The void is thinner than the material.".to_owned());
    }
    let z = free.map_or_else(|| follow(local.z, low, high), |free| free.clamp(low, high));
    let Some(ring) = niche.section() else {
        let translation = DVec3::new(minimum.x, minimum.y, z);
        let placement = Placement::new(translation, DQuat::IDENTITY)
            .map_err(|_| "The shelf cannot be placed in the void.".to_owned())?;
        let size = niche.size;
        let nominal_size = PanelNominalSize::new(
            length_of(size.x).ok_or_else(place_too_small)?,
            length_of(size.y).ok_or_else(place_too_small)?,
        )
        .map_err(|error| error.to_string())?;
        return Ok((None, nominal_size, placement));
    };
    let outline = Panel::anchored_contour(ring).map_err(|error| error.to_string())?;
    let nominal_size = Panel::contour_size(&outline).map_err(|error| error.to_string())?;
    let (min, _) = ring.bounds_micrometres();
    let translation = DVec3::new(
        Length::from_micrometres(min[0]).millimetres_f64(),
        Length::from_micrometres(min[1]).millimetres_f64(),
        z,
    );
    let placement = Placement::new(translation, DQuat::IDENTITY)
        .map_err(|_| "The shelf cannot be placed in the void.".to_owned())?;
    Ok((Some(outline), nominal_size, placement))
}

/// The post a click would create: the void's height and depth, the pointer along its width.
///
/// A post is a flat board across the depth, so a section that is not a rectangle — two posts standing
/// at an angle, for instance — offers no face for it to stand against. The tool refuses that void
/// instead of cutting a board into the walls.
fn post(
    niche: &Niche,
    local: DVec3,
    minimum: DVec3,
    maximum: DVec3,
    gap: f64,
    free: Option<f64>,
) -> Result<(Option<Contour>, PanelNominalSize, Placement), String> {
    if niche.section().is_some() {
        return Err("The empty space is not a rectangle; a post does not fit it.".to_owned());
    }
    let low = minimum.x;
    let high = maximum.x - gap;
    if high < low {
        return Err("The void is narrower than the material.".to_owned());
    }
    let x = free.map_or_else(|| follow(local.x, low, high), |free| free.clamp(low, high));
    // Local X points down the standing panel and local Z is its thickness, so the panel is placed
    // with its top corner on the void's ceiling and its near face on the pointer.
    let rotation = DQuat::from_rotation_y(FRAC_PI_2);
    let translation = DVec3::new(x, minimum.y, maximum.z);
    let placement =
        Placement::new(translation, rotation).map_err(|_| "The post cannot be placed in the void.".to_owned())?;
    let nominal_size = PanelNominalSize::new(
        length_of(niche.size.z).ok_or_else(place_too_small)?,
        length_of(niche.size.y).ok_or_else(place_too_small)?,
    )
    .map_err(|error| error.to_string())?;
    Ok((None, nominal_size, placement))
}

/// The reason a void cannot be filled, in the maker's words.
fn refusal_sentence(refusal: NicheRefusal) -> String {
    match refusal {
        NicheRefusal::Outside => "Point at the empty space inside the open product.".to_owned(),
        NicheRefusal::Turned => "The open product is turned; its empty space cannot be measured yet.".to_owned(),
        NicheRefusal::Unrepresentable => {
            "The empty space here is not one straight shape; point at a plainer gap.".to_owned()
        }
    }
}

/// The sentence for a void too small for a panel.
fn place_too_small() -> String {
    "The void is too small for a panel.".to_owned()
}

/// A ring's corners in the world, on the panel's base face.
fn ring_corners(ring: &Contour, base: impl Fn(f64, f64) -> DVec3) -> Result<Vec<DVec3>, String> {
    let points = ring.sampled_points(1.0).map_err(|error| error.to_string())?;
    let mut corners = Vec::with_capacity(points.len());
    for [x, y] in points {
        corners.push(base(x.millimetres_f64(), y.millimetres_f64()));
    }
    if corners.len() < 3 {
        return Err("The outline of the void is not a ring.".to_owned());
    }
    Ok(corners)
}

/// The pointer's place along the free axis: whole millimetres, kept inside the void.
fn follow(value: f64, low: f64, high: f64) -> f64 {
    value.round().clamp(low, high)
}

/// A whole-millimetre measurement as the document's length, or `None` when it is unusable.
///
/// The model measures to the micrometre, but the free axis moves in whole millimetres (ADR 0103);
/// the measured sides keep their micrometre precision, so a part stays flush with the void.
fn length_of(millimetres: f64) -> Option<Length> {
    if !millimetres.is_finite() || millimetres <= 0.0 {
        return None;
    }
    let micrometres = (millimetres * 1000.0).round();
    if !micrometres.is_finite() || micrometres > i64::MAX as f64 {
        return None;
    }
    Some(Length::from_micrometres(micrometres as i64))
}

/// Paints one size at `position`, on a quiet plate so it stays legible over the scene.
fn label(ui: &Ui, painter: &egui::Painter, position: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::monospace(11.0), theme::text(ui));
    let rect = Rect::from_center_size(position, galley.size() + egui::vec2(6.0, 2.0));
    painter.rect_filled(rect, 3.0, theme::surface(ui));
    painter.galley(rect.center() - galley.size() * 0.5, galley, theme::text(ui));
}

/// The world box of a product's authored envelope, or `None` when it has no envelope.
pub(crate) fn envelope_box(project: &Project, product: Id) -> Option<(DVec3, DVec3)> {
    let envelope = project.container(product)?.envelope()?;
    let size =
        DVec3::new(envelope.x().millimetres_f64(), envelope.y().millimetres_f64(), envelope.z().millimetres_f64());
    let world = project.world_placement(product)?;
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for x in [0.0, size.x] {
        for y in [0.0, size.y] {
            for z in [0.0, size.z] {
                let corner = world.transform_point(DVec3::new(x, y, z));
                if !corner.is_finite() {
                    return None;
                }
                min = min.min(corner);
                max = max.max(corner);
            }
        }
    }
    Some((min, max))
}

/// The point where a ray enters a world box, or the ray's origin when it starts inside.
///
/// This is the cursor's resting place in the void: the face of the reserved space the maker is
/// pointing at. A ray that misses the box answers `None`.
pub(crate) fn envelope_entry(ray: Ray, min: DVec3, max: DVec3) -> Option<DVec3> {
    let origin = ray.origin();
    let direction = ray.direction();
    if !origin.is_finite() || !direction.is_finite() {
        return None;
    }
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    for axis in 0..3 {
        let step = direction[axis];
        let low = min[axis];
        let high = max[axis];
        if step.abs() <= 1.0e-9 {
            if origin[axis] < low || origin[axis] > high {
                return None;
            }
            continue;
        }
        let first = (low - origin[axis]) / step;
        let second = (high - origin[axis]) / step;
        enter = enter.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    if exit < enter || exit < 0.0 {
        return None;
    }
    if enter <= 0.0 {
        return Some(origin);
    }
    let point = origin + direction * enter;
    point.is_finite().then_some(point)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{ClassCode, Container, Envelope, Id, Placement, Project};
    use furnicraft_units::Length;

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit")
    }

    fn clean_project() -> Project {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        Project::new(root).expect("an empty project")
    }

    /// An empty product of `600 x 560 x 720` millimetres at the origin.
    fn piece() -> (Project, Id) {
        let mut project = clean_project();
        let root = project.root().id();
        let envelope = Envelope::new(millimetres(600), millimetres(560), millimetres(720)).expect("a box");
        let id = project
            .add_product(root, "Cabinet", ClassCode::GENERAL_PRODUCT, envelope, Placement::IDENTITY)
            .expect("a product");
        (project, id)
    }

    fn shelf_slot(project: &mut Project, product: Id, at: (i64, i64, i64)) {
        let nominal = PanelNominalSize::new(millimetres(600), millimetres(560)).expect("a shelf size");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(at.0), millimetres(at.1), millimetres(at.2))
            .expect("a placement");
        project.add_panel(product, "Shelf", nominal, None, millimetres(18), placement).expect("add a shelf");
    }

    /// Two standing posts whose inner faces run from `(100, 0)` to `(200, 560)` and from
    /// `(500, 0)` to `(400, 560)`, both 720 mm tall and 1200 mm wide.
    fn splayed_piece() -> (Project, Id) {
        let (mut project, product) = piece();
        let slant = 100.0 / 560.0_f64.hypot(100.0);
        let drop = 560.0 / 560.0_f64.hypot(100.0);
        for (through, normal) in [((100.0, 0.0), (drop, -slant)), ((500.0, 0.0), (-drop, -slant))] {
            let direction = (-normal.1, normal.0);
            let start_y = 280.0 - 600.0 * direction.1;
            let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
            let rotation = DQuat::from_rotation_z(normal.1.atan2(normal.0)) * DQuat::from_rotation_y(FRAC_PI_2);
            let translation = DVec3::new(start_x, start_y, 720.0) - DVec3::new(normal.0, normal.1, 0.0) * 18.0;
            let placement = Placement::new(translation, rotation).expect("a placement");
            let nominal = PanelNominalSize::new(millimetres(720), millimetres(1200)).expect("a post size");
            project.add_panel(product, "Post", nominal, None, millimetres(18), placement).expect("add a post");
        }
        (project, product)
    }

    #[test]
    fn a_shelf_takes_the_void_width_and_depth_and_the_pointer_height() {
        let (project, product) = piece();
        let ghost = plan(&project, product, DVec3::new(300.0, 280.0, 351.4), Fill::WidthAndDepth, millimetres(18))
            .expect("a shelf fills the void");
        assert_eq!(ghost.nominal_size.length(), millimetres(600));
        assert_eq!(ghost.nominal_size.width(), millimetres(560));
        assert_eq!(ghost.placement.translation(), DVec3::new(0.0, 0.0, 351.0), "the height moves in whole mm");
        assert_eq!(ghost.size_mm, DVec3::new(600.0, 560.0, 18.0));
        assert_eq!(ghost.parent, product);
        assert!(ghost.outline.is_none(), "a rectangular void keeps the rectangle panel");
        // The corners are the panel's box at that placement.
        assert_eq!(ghost.world[0], DVec3::new(0.0, 0.0, 351.0));
        assert_eq!(ghost.world[2], DVec3::new(600.0, 560.0, 351.0));
        assert_eq!(ghost.top[2], DVec3::new(600.0, 560.0, 369.0));
    }

    #[test]
    fn a_post_stands_on_the_voids_floor_and_spans_its_height_and_depth() {
        let (project, product) = piece();
        let ghost = plan(&project, product, DVec3::new(250.6, 280.0, 360.0), Fill::HeightAndDepth, millimetres(18))
            .expect("a post fills the void");
        assert_eq!(ghost.nominal_size.length(), millimetres(720), "the void height is the post's length");
        assert_eq!(ghost.nominal_size.width(), millimetres(560), "and the void depth its width");
        assert_eq!(ghost.size_mm, DVec3::new(720.0, 560.0, 18.0));
        // The post's near face is on the pointer's whole-millimetre plane, its thickness runs +X.
        assert_eq!(ghost.placement.translation(), DVec3::new(251.0, 0.0, 720.0));
        assert_eq!(ghost.world[0], DVec3::new(251.0, 0.0, 720.0), "the top corner stands on the ceiling");
        assert!(
            ghost
                .world
                .iter()
                .chain(ghost.top.iter())
                .any(|corner| (*corner - DVec3::new(269.0, 0.0, 0.0)).length() < 1e-9),
            "the post's thickness runs +X from its near face: {:?}",
            ghost.world
        );
    }

    #[test]
    fn a_shelf_in_a_splayed_void_takes_the_trapezoid_outline() {
        let (project, product) = splayed_piece();
        let ghost = plan(&project, product, DVec3::new(300.0, 280.0, 360.0), Fill::WidthAndDepth, millimetres(18))
            .expect("a shelf fills the splayed void");
        let outline = ghost.outline.as_ref().expect("the shelf takes the measured section");
        assert_eq!(ghost.nominal_size.length(), millimetres(400));
        assert_eq!(ghost.nominal_size.width(), millimetres(560));
        assert_eq!(ghost.placement.translation(), DVec3::new(100.0, 0.0, 360.0), "the section's own corner");
        let corners = outline.sampled_points(1.0).expect("the ring is line only");
        assert_eq!(
            corners,
            vec![
                [millimetres(300), millimetres(560)],
                [millimetres(100), millimetres(560)],
                [millimetres(0), millimetres(0)],
                [millimetres(400), millimetres(0)],
            ],
            "the ring is anchored at its own zero corner"
        );
        // The ghost's world corners are the same ring moved to the panel's corner.
        assert!((ghost.world[0] - DVec3::new(400.0, 560.0, 360.0)).length() < 1e-9, "{:?}", ghost.world);
        assert!((ghost.world[2] - DVec3::new(100.0, 0.0, 360.0)).length() < 1e-9, "{:?}", ghost.world);
    }

    #[test]
    fn a_post_is_refused_in_a_splayed_void() {
        let (project, product) = splayed_piece();
        let refusal = plan(&project, product, DVec3::new(300.0, 280.0, 360.0), Fill::HeightAndDepth, millimetres(18))
            .expect_err("a board cannot stand against a splayed wall");
        assert!(refusal.contains("post"), "{refusal}");
    }

    #[test]
    fn a_cursor_on_the_envelope_face_still_measures_the_void() {
        let (project, product) = piece();
        let ghost = plan(&project, product, DVec3::new(300.0, 0.0, 360.0), Fill::WidthAndDepth, millimetres(18))
            .expect("the near face is inside the void");
        assert_eq!(ghost.nominal_size.length(), millimetres(600));
        assert!(ghost.outline.is_none());
    }

    #[test]
    fn a_shelf_splits_the_void_and_the_ghost_fits_one_half() {
        let (mut project, product) = piece();
        shelf_slot(&mut project, product, (0, 0, 350));
        let ghost = plan(&project, product, DVec3::new(300.0, 280.0, 600.0), Fill::WidthAndDepth, millimetres(18))
            .expect("the upper niche takes a shelf");
        assert_eq!(ghost.placement.translation(), DVec3::new(0.0, 0.0, 600.0));
        assert_eq!(ghost.niche.size.z, 352.0, "the upper void starts on the shelf's top face");
    }

    #[test]
    fn the_free_axis_is_kept_inside_the_void() {
        let (mut project, product) = piece();
        shelf_slot(&mut project, product, (0, 0, 350));
        // A cursor at the very top of the upper void would put the shelf through the roof; it is
        // clamped so the whole thickness stays inside.
        let ghost = plan(&project, product, DVec3::new(300.0, 280.0, 720.0), Fill::WidthAndDepth, millimetres(18))
            .expect("a shelf");
        assert_eq!(ghost.placement.translation().z, 720.0 - 18.0);
    }

    #[test]
    fn a_void_thinner_than_the_material_is_refused() {
        // The slot between the void's floor and the shelf is 350 mm; make it a sliver instead.
        let (mut project, product) = piece();
        shelf_slot(&mut project, product, (0, 0, 4));
        let refusal = plan(&project, product, DVec3::new(300.0, 280.0, 1.0), Fill::WidthAndDepth, millimetres(18))
            .expect_err("no shelf fits a 4 mm gap");
        assert!(refusal.contains("thinner"), "{refusal}");
    }

    #[test]
    fn a_point_outside_the_open_product_is_refused() {
        let (project, product) = piece();
        let refusal = plan(&project, product, DVec3::new(-500.0, 0.0, 0.0), Fill::WidthAndDepth, millimetres(18))
            .expect_err("no void");
        assert!(refusal.contains("empty space"), "{refusal}");
    }

    #[test]
    fn the_cursor_enters_the_envelope_at_its_near_face() {
        let ray = Ray::new(DVec3::new(300.0, -1000.0, 300.0), DVec3::Y).expect("a ray");
        let (min, max) = (DVec3::ZERO, DVec3::new(600.0, 560.0, 720.0));
        let entry = envelope_entry(ray, min, max).expect("the ray enters the box");
        assert!((entry - DVec3::new(300.0, 0.0, 300.0)).length() < 1e-9, "was {entry:?}");
        let inside = Ray::new(DVec3::new(300.0, 280.0, 300.0), DVec3::Y).expect("a ray");
        assert_eq!(envelope_entry(inside, min, max), Some(inside.origin()), "a camera inside starts where it is");
        let missing = Ray::new(DVec3::new(-50.0, -1000.0, 300.0), -DVec3::Y).expect("a ray");
        assert_eq!(envelope_entry(missing, min, max), None);
    }

    #[test]
    fn the_ghost_paints_its_wireframe_and_three_sizes_offscreen() {
        let (project, product) = piece();
        let mut gizmo = FillGizmo::default();
        assert!(gizmo.hover(
            &project,
            product,
            DVec3::new(300.0, 280.0, 360.0),
            Fill::WidthAndDepth,
            millimetres(18),
            None
        ));
        let drawn = paint_ghost(&mut gizmo);
        for size in ["600", "560", "18"] {
            assert!(drawn.0.iter().any(|text| text == size), "the {size} mm callout is missing: {:?}", drawn.0);
        }
        assert!(drawn.1 >= 12, "the ghost draws a box's twelve edges, got {}", drawn.1);
    }

    #[test]
    fn a_shaped_ghost_paints_its_polygon_offscreen() {
        let (project, product) = splayed_piece();
        let mut gizmo = FillGizmo::default();
        assert!(gizmo.hover(
            &project,
            product,
            DVec3::new(300.0, 280.0, 360.0),
            Fill::WidthAndDepth,
            millimetres(18),
            None
        ));
        let drawn = paint_ghost(&mut gizmo);
        for size in ["400", "560", "18"] {
            assert!(drawn.0.iter().any(|text| text == size), "the {size} mm callout is missing: {:?}", drawn.0);
        }
        assert!(drawn.1 >= 12, "the ghost draws a polygon prism's twelve edges, got {}", drawn.1);
    }

    /// Paints the armed tool offscreen and answers the drawn texts and line count.
    fn paint_ghost(gizmo: &mut FillGizmo) -> (Vec<String>, usize) {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                gizmo.paint(
                    ui,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
                    Camera::default(),
                );
            },
        );
        let drawn = crate::test_support::drawn_text(&output.shapes);
        fn count_lines(shape: &egui::Shape, lines: &mut usize) {
            match shape {
                egui::Shape::LineSegment { .. } => *lines += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        count_lines(shape, lines);
                    }
                }
                _ => {}
            }
        }
        let mut lines = 0;
        for clipped in &output.shapes {
            count_lines(&clipped.shape, &mut lines);
        }
        output.drop_without_applying_deltas();
        (drawn, lines)
    }

    #[test]
    fn a_turned_product_still_has_a_world_envelope_box() {
        let (mut project, product) = piece();
        let turn = Placement::IDENTITY
            .with_rotation_euler(furnicraft_model::EulerAngles {
                z: furnicraft_units::Degrees::from_millidegrees(90_000),
                ..furnicraft_model::EulerAngles::default()
            })
            .expect("a turn");
        project.set_node_placement(product, turn).expect("turn the piece");
        let (min, max) = envelope_box(&project, product).expect("a box");
        assert!((min.x + 560.0).abs() < 1e-9, "was {min:?}");
        assert!((max.x).abs() < 1e-9, "was {max:?}");
        assert!((max.y - 600.0).abs() < 1e-9, "was {max:?}");
    }
}
