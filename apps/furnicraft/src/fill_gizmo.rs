//! The empty-space tool: the ghost of the part the next click would create.
//!
//! A post or a shelf fills the niche under the pointer. The model answers the measurement
//! ([`Project::niche`], ADR 0103); this module turns the answer into the one panel the click would
//! create and paints it, and touches nothing else. The ghost carries exactly the size, placement and
//! class the command receives, so what the maker sees is what the history records — one
//! `panel.create` per click, one undo entry.
//!
//! `Fill::HeightAndDepth` stands a part up as a post — the niche's height becomes the panel's
//! length, its depth the width, and the material's thickness runs along the free axis, where the
//! pointer sits. `Fill::WidthAndDepth` lays a part down as a shelf — the niche's width and depth
//! become the panel's sides and the pointer picks the height. The free-axis position moves in whole
//! millimetres; the other sides are the measured void exactly, so a part is flush with the walls it
//! was cut to.
//!
//! The ghost is shell paint, not the session preview overlay: an overlay entry is keyed by the id of
//! a node the document holds (ADR 0093), and a part that does not exist yet has none.

use std::f64::consts::FRAC_PI_2;

use eframe::egui::{self, FontId, Pos2, Rect, Stroke, Ui};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::{Fill, Id, Niche, PanelNominalSize, Placement, Project};
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
    /// The panel's nominal length and width, exactly as the command will store them.
    pub(crate) nominal_size: PanelNominalSize,
    /// The panel's placement in the parent frame, exactly as the command will store it.
    pub(crate) placement: Placement,
    /// The void the part was measured against.
    pub(crate) niche: Niche,
    /// The panel's three extents, in millimetres, for the callouts.
    pub(crate) size_mm: DVec3,
    /// The eight corners of the panel in world millimetres, for painting.
    world: [DVec3; 8],
}

/// The empty-space tool's live state: the ghost, or the reason there is none.
#[derive(Debug, Default)]
pub(crate) struct FillGizmo {
    ghost: Option<FillGhost>,
    refusal: Option<String>,
    /// The point and the fill the ghost was measured at, so a still pointer re-measures nothing.
    measured: Option<(Id, DVec3, Fill, Length)>,
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
    ) -> bool {
        if self.measured.is_some_and(|(measured_product, measured_point, measured_fill, measured_thickness)| {
            measured_product == product
                && measured_fill == fill
                && measured_thickness == thickness
                && (measured_point - point).length() < MEASURE_STEP_MM
        }) {
            return false;
        }
        self.measured = Some((product, point, fill, thickness));
        match plan(project, product, point, fill, thickness) {
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
    /// The box is an outline, not a solid: it must read through the panels around it and never hide
    /// the void it measures.
    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, camera: Camera) {
        let painter = ui.painter().with_clip_rect(rect);
        let Some(ghost) = self.ghost.as_ref() else {
            // An armed tool with no ghost says why: the pointer is outside the open piece, or the
            // void is too small for the material.
            if let Some(reason) = self.refusal.as_deref() {
                label(ui, &painter, rect.center(), reason);
            }
            return;
        };
        let ink = theme::accent(ui);
        let stroke = Stroke::new(1.6, ink);
        for (a, b) in EDGES {
            let (Some(from), Some(to)) =
                (screen_point(camera, rect, ghost.world[a]), screen_point(camera, rect, ghost.world[b]))
            else {
                continue;
            };
            painter.line_segment([from, to], stroke);
        }
        // The three sizes, at the midpoint of the edges that carry them: length, width, thickness.
        for (edge, millimetres) in [(0usize, ghost.size_mm.x), (2, ghost.size_mm.y), (4, ghost.size_mm.z)] {
            let Some(midpoint) = screen_point(camera, rect, (ghost.world[0] + ghost.world[edge]) * 0.5) else {
                continue;
            };
            label(ui, &painter, midpoint, &format!("{millimetres:.0}"));
        }
    }
}

/// The twelve edges of the panel box, as corner index pairs.
///
/// Corner `i` has bit 0 on for the far length, bit 1 for the far width and bit 2 for the far
/// thickness, so an edge joins two corners that differ in exactly one bit and every edge is listed
/// once.
const EDGES: [(usize, usize); 12] =
    [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// Measures the void at `point` and answers the part a click would create.
///
/// The point has to be inside `product`, which is the open piece: a niche belongs to the working
/// product (ADR 0098). The measurement is the model's ([`Project::niche`]); the free-axis position
/// is the only number this function rounds, because it follows the pointer in whole millimetres.
pub(crate) fn plan(
    project: &Project,
    product: Id,
    point: DVec3,
    fill: Fill,
    thickness: Length,
) -> Result<FillGhost, String> {
    let niche = project.niche(point).ok_or_else(|| "Point at the empty space inside the open product.".to_owned())?;
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

    let (length_mm, width_mm, placement) = match fill {
        Fill::WidthAndDepth => {
            // A shelf spans the void's width and depth; the pointer picks its height.
            let low = minimum.z;
            let high = maximum.z - gap;
            if high < low {
                return Err("The void is thinner than the material.".to_owned());
            }
            let z = follow(local.z, low, high);
            let translation = DVec3::new(minimum.x, minimum.y, z);
            let placement = Placement::new(translation, DQuat::IDENTITY)
                .map_err(|_| "The shelf cannot be placed in the void.".to_owned())?;
            (niche.size.x, niche.size.y, placement)
        }
        Fill::HeightAndDepth => {
            // A post spans the void's height and depth; the pointer picks its place along the width.
            // Local X points down the standing panel and local Z is its thickness, so the panel is
            // placed with its top corner on the void's ceiling and its near face on the pointer.
            let low = minimum.x;
            let high = maximum.x - gap;
            if high < low {
                return Err("The void is narrower than the material.".to_owned());
            }
            let x = follow(local.x, low, high);
            let rotation = DQuat::from_rotation_y(FRAC_PI_2);
            let translation = DVec3::new(x, minimum.y, maximum.z);
            let placement = Placement::new(translation, rotation)
                .map_err(|_| "The post cannot be placed in the void.".to_owned())?;
            (niche.size.z, niche.size.y, placement)
        }
    };

    let length = length_of(length_mm).ok_or_else(|| "The void is too small for a panel.".to_owned())?;
    let width = length_of(width_mm).ok_or_else(|| "The void is too small for a panel.".to_owned())?;
    let nominal_size = PanelNominalSize::new(length, width).map_err(|error| error.to_string())?;
    let corners = panel_corners(length, width, thickness);
    let world_placement = Placement::compose(parent_world, placement)
        .map_err(|_| "The part cannot be placed in the open product.".to_owned())?;
    let world = corners.map(|corner| world_placement.transform_point(corner));
    if world.iter().any(|corner| !corner.is_finite()) {
        return Err("The part cannot be placed in the open product.".to_owned());
    }
    Ok(FillGhost {
        parent: product,
        nominal_size,
        placement,
        niche,
        size_mm: DVec3::new(length.millimetres_f64(), width.millimetres_f64(), thickness.millimetres_f64()),
        world,
    })
}

/// The panel's eight corners in its own frame, from `[0,0,0]` to `[length, width, thickness]`.
///
/// Corner `i` sets bit 0 for the far length, bit 1 for the far width and bit 2 for the far
/// thickness, which is the order [`EDGES`] relies on.
fn panel_corners(length: Length, width: Length, thickness: Length) -> [DVec3; 8] {
    let size = DVec3::new(length.millimetres_f64(), width.millimetres_f64(), thickness.millimetres_f64());
    [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(size.x, 0.0, 0.0),
        DVec3::new(0.0, size.y, 0.0),
        DVec3::new(size.x, size.y, 0.0),
        DVec3::new(0.0, 0.0, size.z),
        DVec3::new(size.x, 0.0, size.z),
        DVec3::new(0.0, size.y, size.z),
        DVec3::new(size.x, size.y, size.z),
    ]
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
        // The corners are the panel's box at that placement.
        assert_eq!(ghost.world[0], DVec3::new(0.0, 0.0, 351.0));
        assert_eq!(ghost.world[7], DVec3::new(600.0, 560.0, 369.0));
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
        assert!(ghost.world.iter().any(|corner| (*corner - DVec3::new(269.0, 0.0, 0.0)).length() < 1e-9));
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
        assert!(gizmo.hover(&project, product, DVec3::new(300.0, 280.0, 360.0), Fill::WidthAndDepth, millimetres(18),));
        let ctx = egui::Context::default();
        let mut drawn = Vec::new();
        let mut lines = 0;
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
        drawn.extend(crate::test_support::drawn_text(&output.shapes));
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
        for clipped in &output.shapes {
            count_lines(&clipped.shape, &mut lines);
        }
        assert!(lines >= 12, "the ghost draws the box's twelve edges, got {lines}");
        for size in ["600", "560", "18"] {
            assert!(drawn.iter().any(|text| text == size), "the {size} mm callout is missing: {drawn:?}");
        }
        output.drop_without_applying_deltas();
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
