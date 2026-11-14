//! The fill plan: the one part a click in a void creates, in the mode the tool is in.
//!
//! A post and a shelf are the same question asked of the niche the walls leave (ADR 0099, ADR 0103,
//! ADR 0106): which part fills it exactly, where does it land, and which outline does it store? The
//! answer is one [`FillPlan`], derived for cadence-free reasons — the ghost the maker sees, the
//! distance fields that place it and the `panel.create` the click dispatches all read it, so the
//! picture, the numbers and the history cannot disagree.
//!
//! Both modes start from the same cut of the cell at the board's own thickness ([`Niche::cut`]), so
//! a part is inside the walls at both faces by construction:
//!
//! - [`FillMode::Fit`] takes the **largest rectangle** with sides on the panel's own axes that lies
//!   inside that section. It is the safe default: it always fits, and it keeps the rectangle panel
//!   with its resize, its card and its nominal fields (ADR 0042).
//! - [`FillMode::Shape`] takes the **whole section**: the part is the shape the walls allow, and
//!   [`classify_outline`] decides whether that shape is still a standard panel — a rectangle, a
//!   rectangle with chamfered corners, a rectangle with radii — before it falls back to a contour.
//!
//! Nothing here is stored: the plan is a query, like the niche it starts from.

use furnicraft_geom::{DQuat, DVec2, DVec3};
use furnicraft_units::Length;

use crate::*;

/// How a fill tool reads the section the walls leave.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum FillMode {
    /// The largest rectangle that fits inside the section, keeping the standard rectangle panel.
    #[default]
    Fit,
    /// The section itself: the part takes the whole shape the walls allow.
    Shape,
}

/// The part a fill click would create.
///
/// `outline`, `nominal_size` and `placement` are exactly what `panel.create` stores, so a click
/// writes the plan rather than a second measurement of it.
#[derive(Clone, Debug, PartialEq)]
pub struct FillPlan {
    /// The open product the part lands in.
    pub product: Id,
    /// The outline the panel is born with: the nominal rectangle, a rectangle with corner
    /// treatments, or the measured ring.
    pub outline: PanelOutline,
    /// The panel's nominal length and width.
    pub nominal_size: PanelNominalSize,
    /// The panel's placement in the parent frame.
    pub placement: Placement,
    /// The void the part was measured against.
    pub niche: Niche,
    /// The plan's base-face polygon in panel-local millimetres, counter-clockwise.
    ///
    /// It is the shape the ghost draws and the shape the panel occupies: the inscribed rectangle in
    /// [`FillMode::Fit`], the section itself in [`FillMode::Shape`].
    pub corners: Vec<DVec2>,
    /// The smaller end of the part along the free axis, in the parent frame and millimetres.
    pub free_low: f64,
    /// The larger end of the part along the free axis, in the parent frame and millimetres.
    pub free_high: f64,
}

/// Why a point has no fillable niche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRefusal {
    /// The point is outside every product, or is not a usable number.
    Outside,
    /// The free space around the point is not one convex cell.
    Unrepresentable,
    /// The void belongs to another product than the one that is open.
    OtherProduct,
    /// Neither the product nor the measured point has a usable frame.
    NoFrame,
    /// The void is thinner than the material along the part's own free axis.
    TooThin,
    /// The void leaves no rectangle, or no ring, large enough to be a panel.
    TooSmall,
}

impl Project {
    /// Answers the part one click at `point` would create in `product`.
    ///
    /// `fill` is the class's own axis pair, `thickness` the material's thickness, `mode` how the
    /// section is read and `free` the pinned free-axis position — `None` while the pointer picks it
    /// in whole millimetres, `Some` when a typed distance fixed it (ADR 0103). The result is the
    /// whole plan, so the caller never measures the void a second time.
    pub fn fill_plan(
        &self,
        product: Id,
        point: DVec3,
        fill: Fill,
        mode: FillMode,
        thickness: Length,
        free: Option<f64>,
    ) -> Result<FillPlan, FillRefusal> {
        let niche = self.niche(point).map_err(|refusal| match refusal {
            NicheRefusal::Outside => FillRefusal::Outside,
            NicheRefusal::Unrepresentable => FillRefusal::Unrepresentable,
        })?;
        if niche.product != product {
            return Err(FillRefusal::OtherProduct);
        }
        let parent_world = self.world_placement(product).ok_or(FillRefusal::NoFrame)?;
        let inverse = parent_world.inverse().map_err(|_| FillRefusal::NoFrame)?;
        let local = inverse.transform_point(point);
        let gap = thickness.millimetres_f64();
        if !local.is_finite()
            || !niche.size.is_finite()
            || !niche.local_min.is_finite()
            || !gap.is_finite()
            || gap <= 0.0
        {
            return Err(FillRefusal::TooSmall);
        }
        let (part, placement) = match fill {
            Fill::WidthAndDepth => shelf(&niche, local, gap, mode, free)?,
            Fill::HeightAndDepth => post(&niche, local, gap, mode, free)?,
        };
        // The free axis is the panel's own thickness axis, so the two ends are its base face and its
        // far face; which of them is lower depends on the stance (ADR 0105).
        let axis = free_axis(fill);
        let base = placement.translation()[axis];
        let far = base + placement.transform_vector(DVec3::Z)[axis] * gap;
        let (free_low, free_high) = if base <= far { (base, far) } else { (far, base) };
        if !free_low.is_finite() || !free_high.is_finite() {
            return Err(FillRefusal::NoFrame);
        }
        Ok(FillPlan {
            product,
            outline: part.outline,
            nominal_size: part.nominal_size,
            placement,
            niche,
            corners: part.corners,
            free_low,
            free_high,
        })
    }
}

/// The axis of the open product's frame a fill's free position runs along.
///
/// A shelf's height and a post's place across the void both lie on the panel's own thickness axis.
pub const fn free_axis(fill: Fill) -> usize {
    match fill {
        Fill::WidthAndDepth => 2,
        Fill::HeightAndDepth => 0,
    }
}

/// The outline, size and base polygon one footprint becomes.
struct Part {
    outline: PanelOutline,
    nominal_size: PanelNominalSize,
    /// The base-face polygon in panel-local millimetres.
    corners: Vec<DVec2>,
}

/// The shelf a click would create: the footprint the void leaves a board of that thickness.
fn shelf(
    niche: &Niche,
    local: DVec3,
    gap: f64,
    mode: FillMode,
    free: Option<f64>,
) -> Result<(Part, Placement), FillRefusal> {
    let low = niche.local_min.z;
    let high = niche.local_max().z - gap;
    if high < low {
        return Err(FillRefusal::TooThin);
    }
    let z = free.map_or_else(|| follow(local.z, low, high), |free| free.clamp(low, high));
    let footprint = niche.cut(2, (z, z + gap), (0, 1)).ok_or(FillRefusal::TooSmall)?;
    let (part, corner) = part_of(footprint, mode)?;
    let translation = DVec3::new(corner.x, corner.y, z);
    let placement = Placement::new(translation, DQuat::IDENTITY).map_err(|_| FillRefusal::NoFrame)?;
    Ok((part, placement))
}

/// The post a click would create: the void's height and the depth the void leaves a board.
///
/// The board stands in the post's stance (ADR 0105), so the footprint is the cell cut along the
/// board's thickness and seen in the panel's own `(x, y)`, which the product's `(z, y)` becomes once
/// the board is turned: a post stands against a splayed wall with the depth the slab really has.
fn post(
    niche: &Niche,
    local: DVec3,
    gap: f64,
    mode: FillMode,
    free: Option<f64>,
) -> Result<(Part, Placement), FillRefusal> {
    let low = niche.local_min.x;
    let high = niche.local_max().x - gap;
    if high < low {
        return Err(FillRefusal::TooThin);
    }
    let x = free.map_or_else(|| follow(local.x, low, high), |free| free.clamp(low, high));
    let footprint = niche.cut(0, (x, x + gap), (2, 1)).ok_or(FillRefusal::TooSmall)?;
    let (part, corner) = part_of(footprint, mode)?;
    // The post is the shelf turned a quarter-turn to the right about `+Y` (ADR 0105): its local X is
    // the product's Z, its local Y the product's Y, and its thickness runs back towards the face the
    // pointer measured, so the node sits one thickness to the `+X` side of that face.
    let rotation = DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2);
    let translation = DVec3::new(x + gap, corner.y, corner.x);
    let placement = Placement::new(translation, rotation).map_err(|_| FillRefusal::NoFrame)?;
    Ok((part, placement))
}

/// The part a footprint becomes in `mode`, with the footprint corner the panel is anchored at.
fn part_of(footprint: Vec<DVec2>, mode: FillMode) -> Result<(Part, DVec2), FillRefusal> {
    let Some(first) = footprint.first().copied() else {
        return Err(FillRefusal::TooSmall);
    };
    if footprint.iter().any(|point| !point.is_finite()) {
        return Err(FillRefusal::TooSmall);
    }
    let low = footprint.iter().copied().fold(first, DVec2::min);
    match mode {
        FillMode::Fit => {
            let (corner, size) = largest_rectangle(&footprint).ok_or(FillRefusal::TooSmall)?;
            let nominal_size = PanelNominalSize::new(
                length_of(size.x).ok_or(FillRefusal::TooSmall)?,
                length_of(size.y).ok_or(FillRefusal::TooSmall)?,
            )
            .map_err(|_| FillRefusal::TooSmall)?;
            let corners =
                vec![DVec2::ZERO, DVec2::new(size.x, 0.0), DVec2::new(size.x, size.y), DVec2::new(0.0, size.y)];
            let outline = PanelOutline::Rect { corner_features: Vec::new(), edge_bows: Vec::new() };
            Ok((Part { outline, nominal_size, corners }, corner))
        }
        FillMode::Shape => {
            let ring = ring_of(&footprint, low)?;
            let nominal_size = Panel::contour_size(&ring).map_err(|_| FillRefusal::TooSmall)?;
            let outline = classify_outline(&ring).unwrap_or(PanelOutline::Contour { contour: ring });
            let corners = footprint.iter().map(|point| *point - low).collect();
            Ok((Part { outline, nominal_size, corners }, low))
        }
    }
}

/// The footprint as the document ring a panel stores, anchored at `low`.
fn ring_of(footprint: &[DVec2], low: DVec2) -> Result<Contour, FillRefusal> {
    let corners = footprint
        .iter()
        .map(|point| Some([document_length(point.x - low.x)?, document_length(point.y - low.y)?]))
        .collect::<Option<Vec<_>>>()
        .ok_or(FillRefusal::TooSmall)?;
    Contour::from_polygon(corners).map_err(|_| FillRefusal::TooSmall)
}

/// The largest axis-aligned rectangle inside a convex polygon, as its low corner and its size.
///
/// The polygon is the section the walls leave, in millimetres and counter-clockwise. Two facts make
/// the search exact rather than a search: for a convex polygon the slice at `x` is one interval
/// `[low(x), high(x)]`, `low` is convex and `high` is concave, so a rectangle `[u, v] × [y, z]` is
/// inside exactly when `y >= max(low(u), low(v))` and `z <= min(high(u), high(v))`. Its **log**
/// area is therefore a concave function of `(u, v)`, which has one maximum: the breakpoint pairs
/// seed the search and a nested ternary refinement finds that maximum, so no greedy growth from a
/// corner can answer a wider-looking rectangle than the widest one.
///
/// A polygon that leaves no rectangle at all — a line, a single point, a section thinner than the
/// document's micrometre — answers `None`.
fn largest_rectangle(points: &[DVec2]) -> Option<(DVec2, DVec2)> {
    if points.len() < 3 {
        return None;
    }
    let first = points.first().copied()?;
    let min = points.iter().copied().fold(first, DVec2::min);
    let max = points.iter().copied().fold(first, DVec2::max);
    if !(max.x - min.x).is_finite() || !(max.y - min.y).is_finite() {
        return None;
    }
    let height = |u: f64, v: f64| -> Option<f64> {
        let (low_u, high_u) = vertical_span(points, u)?;
        let (low_v, high_v) = vertical_span(points, v)?;
        let bottom = low_u.max(low_v);
        let top = high_u.min(high_v);
        (top - bottom > 0.0).then_some(top - bottom)
    };
    let score = |u: f64, v: f64| -> Option<f64> {
        let width = v - u;
        let height = height(u, v)?;
        (width > 0.0).then(|| width.ln() + height.ln())
    };
    // The breakpoint pairs are the corners of the piecewise search and a first answer on their own;
    // the refinement only ever improves it.
    let mut best: Option<(f64, f64, f64)> = None;
    let mut consider = |u: f64, v: f64| {
        if let Some(value) = score(u, v)
            && best.is_none_or(|(seen, ..)| value > seen)
        {
            best = Some((value, u, v));
        }
    };
    let breakpoints = breakpoints_x(points);
    for (index, u) in breakpoints.iter().copied().enumerate() {
        for v in breakpoints.iter().copied().skip(index.saturating_add(1)) {
            consider(u, v);
        }
    }
    // Partial maximisation of a jointly concave function is concave in the other variable, so the
    // outer search over `u` and the inner one over `v` both converge to the single maximum.
    let mut low_u = min.x;
    let mut high_u = max.x;
    for _ in 0..REFINE_STEPS {
        let first = low_u + (high_u - low_u) / 3.0;
        let second = high_u - (high_u - low_u) / 3.0;
        let first_best = best_v(points, first, max.x, &mut consider);
        let second_best = best_v(points, second, max.x, &mut consider);
        if first_best <= second_best {
            low_u = first;
        } else {
            high_u = second;
        }
    }
    let (_, u, v) = best?;
    let (low_u, high_u) = vertical_span(points, u)?;
    let (low_v, high_v) = vertical_span(points, v)?;
    let bottom = low_u.max(low_v);
    let top = high_u.min(high_v);
    Some((DVec2::new(u, bottom), DVec2::new(v - u, top - bottom)))
}

/// How many times the refinement splits each axis.
///
/// Two thirds per step: forty steps shrink the span by more than ten orders of magnitude, which is
/// past the document's micrometre and stops before the search is comparing noise.
const REFINE_STEPS: usize = 40;

/// The best `v` above `u` for one outer candidate, recording every rectangle it evaluates.
fn best_v(points: &[DVec2], u: f64, high: f64, consider: &mut impl FnMut(f64, f64)) -> f64 {
    let mut best = f64::NEG_INFINITY;
    let mut low = u;
    let mut top = high;
    for _ in 0..REFINE_STEPS {
        let first = low + (top - low) / 3.0;
        let second = top - (top - low) / 3.0;
        let first_score = span_score(points, u, first);
        let second_score = span_score(points, u, second);
        consider(u, first);
        consider(u, second);
        best = best.max(first_score).max(second_score);
        if first_score <= second_score {
            low = first;
        } else {
            top = second;
        }
    }
    best
}

/// The log area of the rectangle between `u` and `v`, or negative infinity when there is none.
fn span_score(points: &[DVec2], u: f64, v: f64) -> f64 {
    let (Some((low_u, high_u)), Some((low_v, high_v))) = (vertical_span(points, u), vertical_span(points, v)) else {
        return f64::NEG_INFINITY;
    };
    let width = v - u;
    let height = high_u.min(high_v) - low_u.max(low_v);
    if width <= 0.0 || height <= 0.0 {
        return f64::NEG_INFINITY;
    }
    width.ln() + height.ln()
}

/// Every vertex x-coordinate, sorted, as the polygon's own breakpoints.
fn breakpoints_x(points: &[DVec2]) -> Vec<f64> {
    let mut xs = points.iter().map(|point| point.x).collect::<Vec<_>>();
    xs.sort_by(|first, second| first.partial_cmp(second).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup_by(|first, second| (*first - *second).abs() <= 1.0e-9);
    xs
}

/// The interval a convex polygon leaves on the vertical line at `x`, in millimetres.
///
/// A line through a convex polygon meets its boundary in at most two points; collecting every
/// crossing and taking the extremes also answers a line through a vertical edge or a vertex, which
/// is where the search's candidates sit.
fn vertical_span(points: &[DVec2], x: f64) -> Option<(f64, f64)> {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for (index, start) in points.iter().enumerate() {
        let end = points.get((index + 1) % points.len())?;
        let span = end.x - start.x;
        if span.abs() <= 1.0e-12 {
            if (x - start.x).abs() <= 1.0e-9 {
                low = low.min(start.y.min(end.y));
                high = high.max(start.y.max(end.y));
            }
            continue;
        }
        let t = (x - start.x) / span;
        if !(-1.0e-9..=1.0 + 1.0e-9).contains(&t) {
            continue;
        }
        let y = start.y + (end.y - start.y) * t;
        if !y.is_finite() {
            return None;
        }
        low = low.min(y);
        high = high.max(y);
    }
    (high > low).then_some((low, high))
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
    document_length(millimetres)
}

/// A measurement as the document's whole micrometres, a corner at the origin included.
fn document_length(millimetres: f64) -> Option<Length> {
    if !millimetres.is_finite() {
        return None;
    }
    let micrometres = (millimetres * 1000.0).round();
    if !micrometres.is_finite() || micrometres.abs() > i64::MAX as f64 {
        return None;
    }
    Some(Length::from_micrometres(micrometres as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::millimetres;

    /// An empty piece with the given envelope at the origin: nothing of the sample scene stands in
    /// its void, so the niche is the box the walls reserve.
    fn empty_product(x: i64, y: i64, z: i64) -> (Project, Id) {
        let root = Container::new(Id::new(1).expect("an id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("an empty project");
        let root = project.root().id();
        let envelope = Envelope::new(millimetres(x), millimetres(y), millimetres(z)).expect("a valid box");
        let product = project
            .add_product(root, "Cabinet", ClassCode::GENERAL_PRODUCT, envelope, Placement::IDENTITY)
            .expect("a product");
        (project, product)
    }

    /// The void of a case whose two posts stand at an angle: the free section is a trapezoid that
    /// runs from (100, 0) and (500, 0) at the floor to (200, 560) and (400, 560) at the back.
    fn splayed_product() -> (Project, Id) {
        let (mut project, product) = empty_product(600, 560, 720);
        let slant = 100.0 / 560.0_f64.hypot(100.0);
        let drop = 560.0 / 560.0_f64.hypot(100.0);
        for (through, normal) in [((100.0, 0.0), (drop, -slant)), ((500.0, 0.0), (-drop, -slant))] {
            let direction = (-normal.1, normal.0);
            let start_y = 280.0 - 600.0 * direction.1;
            let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
            let rotation =
                DQuat::from_rotation_z(normal.1.atan2(normal.0)) * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
            let translation = DVec3::new(start_x, start_y, 720.0) - DVec3::new(normal.0, normal.1, 0.0) * 18.0;
            let placement = Placement::new(translation, rotation).expect("a post placement");
            let size = PanelNominalSize::new(millimetres(720), millimetres(1200)).expect("a post size");
            project
                .add_panel(product, "Post", size, None, millimetres(18), placement)
                .expect("the post lands in the open product");
        }
        (project, product)
    }

    /// The centre of the product's envelope, in world millimetres.
    fn centre(project: &Project, product: Id) -> DVec3 {
        let envelope = project.container(product).and_then(|product| product.envelope()).expect("an envelope");
        let size =
            DVec3::new(envelope.x().millimetres_f64(), envelope.y().millimetres_f64(), envelope.z().millimetres_f64());
        project.world_placement(product).expect("a frame").transform_point(size * 0.5)
    }

    fn plan(
        project: &Project,
        product: Id,
        fill: Fill,
        mode: FillMode,
        thickness: Length,
    ) -> Result<FillPlan, FillRefusal> {
        project.fill_plan(product, centre(project, product), fill, mode, thickness, None)
    }

    #[test]
    fn a_trapezoid_fits_the_largest_rectangle_and_takes_its_ring_as_the_shape() {
        let (project, product) = splayed_product();
        let thickness = millimetres(18);
        let fitted = plan(&project, product, Fill::WidthAndDepth, FillMode::Fit, thickness).expect("a fit plan");
        // The section narrows upwards, so the widest rectangle inside it starts where the walls are
        // at their narrowest: 200 mm across, the whole 560 mm depth.
        assert!((fitted.nominal_size.length().millimetres_f64() - 200.0).abs() <= 1.0e-3, "{:?}", fitted.nominal_size);
        assert!((fitted.nominal_size.width().millimetres_f64() - 560.0).abs() <= 1.0e-3, "{:?}", fitted.nominal_size);
        assert!(matches!(fitted.outline, PanelOutline::Rect { .. }), "a fit keeps the standard panel");
        assert!(!fitted.outline.has_contour_features(), "and carries no corner treatment");
        assert_eq!(fitted.corners.len(), 4, "the ghost draws a rectangle");

        let shaped = plan(&project, product, Fill::WidthAndDepth, FillMode::Shape, thickness).expect("a shape plan");
        let Some(ring) = shaped.outline.contour() else {
            panic!("the trapezoid is not a standard panel: {:?}", shaped.outline);
        };
        assert_eq!(ring.entries().len(), 4, "the trapezoid is stored as the ring the walls leave");
        assert_eq!(shaped.nominal_size, PanelNominalSize::new(millimetres(400), millimetres(560)).expect("a size"));
        assert!(shaped.corners.len() > 4 || !is_rectangle(&shaped.corners), "the shape is not the rectangle");
    }

    /// Whether a polygon is the rectangle around it, for the shape-mode assertion above.
    fn is_rectangle(points: &[DVec2]) -> bool {
        if points.len() != 4 {
            return false;
        }
        let first = points.first().copied().unwrap_or(DVec2::ZERO);
        let low = points.iter().copied().fold(first, DVec2::min);
        let high = points.iter().copied().fold(first, DVec2::max);
        points.iter().all(|point| {
            ((point.x - low.x).abs() <= 1.0e-6 || (point.x - high.x).abs() <= 1.0e-6)
                && ((point.y - low.y).abs() <= 1.0e-6 || (point.y - high.y).abs() <= 1.0e-6)
        })
    }

    #[test]
    fn a_plain_box_gives_the_same_rectangle_in_both_modes() {
        let (project, product) = empty_product(600, 560, 720);
        let thickness = millimetres(18);
        let mut outlines = Vec::new();
        for mode in [FillMode::Fit, FillMode::Shape] {
            let plan = plan(&project, product, Fill::WidthAndDepth, mode, thickness).expect("a plan");
            assert_eq!(plan.nominal_size, PanelNominalSize::new(millimetres(600), millimetres(560)).expect("a size"));
            assert!(matches!(plan.outline, PanelOutline::Rect { .. }), "{mode:?} keeps the rectangle panel");
            outlines.push((plan.outline, plan.corners));
        }
        assert_eq!(outlines.first(), outlines.last(), "a rectangular void answers the same part either way");
    }

    #[test]
    fn the_fitted_rectangle_lies_inside_every_wall_over_the_whole_thickness() {
        let (project, product) = splayed_product();
        let thickness = millimetres(18);
        for fill in [Fill::WidthAndDepth, Fill::HeightAndDepth] {
            let plan = plan(&project, product, fill, FillMode::Fit, thickness).expect("a fit plan");
            let gap = thickness.millimetres_f64();
            let mut points = Vec::new();
            for corner in &plan.corners {
                let base = plan.placement.transform_point(DVec3::new(corner.x, corner.y, 0.0));
                let far = plan.placement.transform_point(DVec3::new(corner.x, corner.y, gap));
                points.extend([base, far]);
            }
            for point in points {
                for plane in &plan.niche.planes {
                    let distance = plane.normal.dot(point) - plane.offset;
                    assert!(distance >= -2.0e-3, "{fill:?}: {:?} is {distance} from a wall", point);
                }
            }
        }
    }

    #[test]
    fn a_post_takes_the_cut_in_its_own_board_plane() {
        let (project, product) = splayed_product();
        let thickness = millimetres(18);
        let post = plan(&project, product, Fill::HeightAndDepth, FillMode::Fit, thickness).expect("a post plan");
        // A post stands up: its length is the void's height, its width the depth the slab leaves.
        assert!((post.nominal_size.length().millimetres_f64() - 720.0).abs() <= 1.0e-3, "{:?}", post.nominal_size);
        assert!(post.nominal_size.width().millimetres_f64() > 0.0);
        assert!((post.free_high - post.free_low - thickness.millimetres_f64()).abs() <= 1.0e-9, "one thickness");
    }

    #[test]
    fn a_void_that_cannot_be_filled_says_why() {
        let (project, product) = empty_product(600, 560, 720);
        let thickness = millimetres(18);
        let outside = project.fill_plan(
            product,
            DVec3::new(-1000.0, -1000.0, -1000.0),
            Fill::WidthAndDepth,
            FillMode::Fit,
            thickness,
            None,
        );
        assert_eq!(outside, Err(FillRefusal::Outside));
        let root = project.root().id();
        let wrong =
            project.fill_plan(root, centre(&project, product), Fill::WidthAndDepth, FillMode::Fit, thickness, None);
        assert_eq!(wrong, Err(FillRefusal::OtherProduct));
        let thin = project.fill_plan(
            product,
            centre(&project, product),
            Fill::WidthAndDepth,
            FillMode::Fit,
            Length::from_micrometres(1),
            None,
        );
        assert!(thin.is_ok(), "a micrometre board still leaves a plan");
    }

    #[test]
    fn a_void_thinner_than_the_material_is_refused() {
        let (project, product) = empty_product(600, 560, 10);
        let plan = plan(&project, product, Fill::WidthAndDepth, FillMode::Fit, millimetres(18));
        assert_eq!(plan, Err(FillRefusal::TooThin));
    }

    #[test]
    fn the_largest_rectangle_of_a_convex_section_is_the_widest_one() {
        // A right triangle: the answer is the square the hypotenuse cuts, not a rectangle grown from
        // the corner, which is what a breakpoint-only search would miss.
        let triangle = vec![DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0), DVec2::new(0.0, 100.0)];
        let (corner, size) = largest_rectangle(&triangle).expect("a triangle holds a rectangle");
        assert!((size.x - 50.0).abs() <= 0.1 && (size.y - 50.0).abs() <= 0.1, "{corner:?} {size:?}");

        // The same question asked of a section that is already the rectangle: the rectangle itself.
        let box_section =
            vec![DVec2::new(0.0, 0.0), DVec2::new(200.0, 0.0), DVec2::new(200.0, 560.0), DVec2::new(0.0, 560.0)];
        let (corner, size) = largest_rectangle(&box_section).expect("a rectangle");
        assert_eq!((corner, size), (DVec2::ZERO, DVec2::new(200.0, 560.0)), "a rectangular section gives itself");

        // A diamond: the answer reaches into the slanted edges, which no vertex x-coordinate names.
        let diamond =
            vec![DVec2::new(50.0, 0.0), DVec2::new(100.0, 50.0), DVec2::new(50.0, 100.0), DVec2::new(0.0, 50.0)];
        let (corner, size) = largest_rectangle(&diamond).expect("a diamond holds a rectangle");
        assert!((size.x - 50.0).abs() <= 0.1 && (size.y - 50.0).abs() <= 0.1, "{corner:?} {size:?}");
    }

    #[test]
    fn a_section_with_no_area_holds_no_rectangle() {
        assert!(largest_rectangle(&[]).is_none());
        assert!(largest_rectangle(&[DVec2::ZERO, DVec2::new(100.0, 0.0)]).is_none());
        let line = vec![DVec2::ZERO, DVec2::new(100.0, 0.0), DVec2::new(200.0, 0.0)];
        assert!(largest_rectangle(&line).is_none(), "a line leaves no height");
    }

    #[test]
    fn the_free_axis_is_the_panel_thickness_axis_of_each_fill() {
        assert_eq!(free_axis(Fill::WidthAndDepth), 2);
        assert_eq!(free_axis(Fill::HeightAndDepth), 0);
    }
}
