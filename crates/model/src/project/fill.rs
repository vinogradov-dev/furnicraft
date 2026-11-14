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
//!   A wall that meets the board at an angle gives the outline edge it bounds an **end bevel**
//!   (ADR 0107): the outline is the section at the wide face, and every edge whose wall tilts gets
//!   the angle that sets the far face back into the wall's plane, so the board meets a leaned
//!   board flush instead of cutting in or leaving a wedge of air (ADR 0108).
//!
//! Nothing here is stored: the plan is a query, like the niche it starts from.

use std::f64::consts::{FRAC_PI_2, PI};

use furnicraft_geom::{DQuat, DVec2, DVec3};
use furnicraft_units::{Degrees, Length};

use crate::*;

/// How far a wall may lean out of the board's own plane before its end face is not an end face.
///
/// A plane whose normal lies this close to the board's thickness axis bounds no outline edge, which
/// is the same threshold the niche's section cut uses to leave such a plane to its hold test.
const LEVEL_WALL: f64 = 1.0e-3;

/// How far a measured corner may sit off the wall line it was cut from, in millimetres.
///
/// The section's corners are whole micrometres, so a corner is a fraction of one off the exact
/// intersection of the two lines it came from; five micrometres leaves room and still separates
/// two walls that are not the same wall.
const WALL_TOLERANCE: f64 = 5.0e-3;

/// The steepest end face a fill gives its part.
///
/// A wall leaned past a mitre is nearer the board's own plane than to a side face, so the "end face"
/// that would meet it is a taper across the large face — a variable-thickness board, which is not
/// this feature — and the void it leaves across the slab is a wedge (N12). `Shape` then takes the
/// slab section with no end bevel; `Fit` answers the same cut (ADR 0109).
const MAX_FILL_BEVEL: Degrees = Degrees::from_millidegrees(45_000);

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
    /// The plan's far-face polygon, in the same frame and the same order as [`FillPlan::corners`].
    ///
    /// The base ring with every beveled edge set back by its wall's own angle, so the ghost draws
    /// the rim the finished panel ends at rather than the base rim lifted by the thickness.
    pub far_corners: Vec<DVec2>,
    /// The end bevels the walls ask for, in ring-segment order.
    ///
    /// Empty when every wall meets the board square, which keeps [`FillMode::Shape`] on the standard
    /// panel whenever the section is one a rectangle draws. A beveled part is always a contour.
    pub bevels: Vec<EdgeBevel>,
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
    /// The walls tilt against each other, so no one flat board meets them all.
    ///
    /// `Shape` falls back to the slab section without bevels, so [`Project::fill_plan`] does not
    /// answer this today; the variant is the classifier's own refusal and stays for a caller that
    /// asks a bevel question directly (ADR 0109).
    WallsDisagree,
    /// A wall lies so nearly level with the board that no end bevel can meet it.
    ///
    /// `Shape` falls back to the slab section, as above; the classifier's own refusal.
    Wedge,
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
        let (part, placement) = board(&niche, local, gap, mode, free, fill, thickness)?;
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
            far_corners: part.far_corners,
            bevels: part.bevels,
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
    /// The far-face polygon in the same frame and order.
    far_corners: Vec<DVec2>,
    /// The end bevels, one per beveled ring segment.
    bevels: Vec<EdgeBevel>,
}

/// How a fill's board sits in the product frame: which product axis each local axis runs along.
///
/// The shelf is the flat stance, the post is the shelf turned a quarter-turn to the right about `+Y`
/// (ADR 0105), so the board's own axes are a permutation of the product's with `+1` or `-1` on local
/// `Y` and `Z`. A fill may turn a board half a turn about its own length axis, which negates both
/// (ADR 0108), and that is the whole vocabulary this needs.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stance {
    /// The product axis the board's local `X` runs along.
    u: usize,
    /// The product axis the board's local `Y` runs along.
    v: usize,
    /// The product axis the board's local `Z` (its thickness) runs along.
    t: usize,
    /// Whether the board's local `Y` runs against the product's `v` axis.
    against_y: bool,
    /// Whether the board's local `Z` runs against the product's `t` axis.
    against_z: bool,
    /// Whether the board was turned about its length axis to take the other wide face.
    other_face: bool,
    /// The turn the stance applies before any reference-face turn.
    turn: DQuat,
}

impl Stance {
    /// The same axes seen from the other large face: the board turned about its own length axis.
    const fn turned(self) -> Self {
        Self { against_y: !self.against_y, against_z: !self.against_z, other_face: !self.other_face, ..self }
    }

    /// The board's rotation in the product frame.
    fn rotation(self) -> DQuat {
        if self.other_face { self.turn * DQuat::from_rotation_x(PI) } else { self.turn }
    }

    /// The product coordinate of the board's base face for a slab starting at `value`.
    fn face(self, value: f64, gap: f64) -> f64 {
        if self.against_z { value + gap } else { value }
    }

    /// The tilt of a wall's `t` normal in the board's own frame.
    fn along(self, normal: f64) -> f64 {
        if self.against_z { -normal } else { normal }
    }
}

/// The stance a fill's board takes before the walls decide which face is the wide one.
fn stance(fill: Fill) -> Stance {
    match fill {
        Fill::WidthAndDepth => {
            Stance { u: 0, v: 1, t: 2, against_y: false, against_z: false, other_face: false, turn: DQuat::IDENTITY }
        }
        Fill::HeightAndDepth => Stance {
            u: 2,
            v: 1,
            t: 0,
            against_y: false,
            against_z: true,
            other_face: false,
            turn: DQuat::from_rotation_y(-FRAC_PI_2),
        },
    }
}

/// The board a click would create in `fill`'s stance: the part and its placement.
///
/// `value` is the free-axis position, which is the slab's low end in the product frame; the board
/// occupies one thickness from there. [`FillMode::Fit`] reads the section the slab takes at both
/// ends, so it fits whichever way the walls lean; [`FillMode::Shape`] reads the wide face and
/// bevels every edge whose wall tilts.
fn board(
    niche: &Niche,
    local: DVec3,
    gap: f64,
    mode: FillMode,
    free: Option<f64>,
    fill: Fill,
    thickness: Length,
) -> Result<(Part, Placement), FillRefusal> {
    let stance = stance(fill);
    let low = niche.local_min[stance.t];
    let high = niche.local_max()[stance.t] - gap;
    if high < low {
        return Err(FillRefusal::TooThin);
    }
    let value = free.map_or_else(|| follow(local[stance.t], low, high), |free| free.clamp(low, high));
    let (stance, polygon, angles) = match mode {
        FillMode::Fit => {
            let footprint =
                niche.cut(stance.t, (value, value + gap), (stance.u, stance.v)).ok_or(FillRefusal::TooSmall)?;
            (stance, footprint, Vec::new())
        }
        FillMode::Shape => match reference_face(niche, value, gap, stance) {
            Ok(stance) => {
                let face = stance.face(value, gap);
                let section = niche.cut(stance.t, (face, face), (stance.u, stance.v)).ok_or(FillRefusal::TooSmall)?;
                let angles = wall_bevels(niche, &section, face, &stance);
                (stance, section, angles)
            }
            // The wall planes here cannot all be met by one flat end face: two walls lean against
            // each other, or a wall lies nearly level with the board. The shape is still useful and
            // still fits if it takes the section the whole slab leaves — the same cut Fit reads —
            // and no end bevel, so the maker gets the shape of the niche with a sliver at a wall
            // instead of a refusal (ADR 0109).
            Err(FillRefusal::WallsDisagree | FillRefusal::Wedge) => {
                let section =
                    niche.cut(stance.t, (value, value + gap), (stance.u, stance.v)).ok_or(FillRefusal::TooSmall)?;
                (stance, section, Vec::new())
            }
            Err(refusal) => return Err(refusal),
        },
    };
    let (part, anchor) = part_of(&polygon, mode, &stance, &angles, thickness)?;
    // The panel's zero corner is the outline's own base corner, which the product frame answers from
    // the stance: the lower end of each axis the board runs with and the higher one where it runs
    // against, then the base face along the thickness.
    let mut translation = DVec3::ZERO;
    translation[stance.u] = anchor.x;
    translation[stance.v] = anchor.y;
    translation[stance.t] = stance.face(value, gap);
    let placement = Placement::new(translation, stance.rotation()).map_err(|_| FillRefusal::NoFrame)?;
    Ok((part, placement))
}

/// The stance whose base face is the wide one: the face a cut-only bevel can keep as the outline.
///
/// With `n` a wall's free-side normal in the board's own frame, face `B` answers
/// `n_xy . xy >= offset` and face `A` answers `n_xy . xy >= offset - n_z t`, so a wall with
/// `n_z < 0` narrows the board at `A` and leaves `B` the wide face. A cell whose walls all tilt the
/// other way is filled from `A` instead, which is the same board turned half a turn about its own
/// length axis: a placement, not a new stance (ADR 0108). Walls that disagree leave a void no flat
/// board fills, and so does a wall so nearly level with the board that its end face would be
/// steeper than a bevel may be.
fn reference_face(niche: &Niche, value: f64, gap: f64, stance: Stance) -> Result<Stance, FillRefusal> {
    let mut cuts = false;
    let mut grows = false;
    for plane in &niche.planes {
        let flat = plane.normal[stance.u].hypot(plane.normal[stance.v]);
        if flat < LEVEL_WALL {
            // A plane level with the board bounds no edge of the section; it only has to hold over
            // the whole slab, or the void is a wedge across the thickness.
            if [value, value + gap].into_iter().any(|end| plane.normal[stance.t] * end - plane.offset < -WALL_TOLERANCE)
            {
                return Err(FillRefusal::Wedge);
            }
            continue;
        }
        let Some(bevel) = wall_bevel(plane, &stance) else {
            continue;
        };
        if bevel >= MAX_FILL_BEVEL {
            return Err(FillRefusal::Wedge);
        }
        if stance.along(plane.normal[stance.t]) < 0.0 {
            cuts = true;
        } else {
            grows = true;
        }
    }
    match (cuts, grows) {
        (true, true) => Err(FillRefusal::WallsDisagree),
        (false, true) => Ok(stance.turned()),
        _ => Ok(stance),
    }
}

/// The bevel one wall asks for, in degrees, or `None` when it meets the board square.
///
/// A plane level with the board bounds no edge at all, and an angle that rounds to zero millidegrees
/// is not storable, so neither gets a record.
fn wall_bevel(plane: &NichePlane, stance: &Stance) -> Option<Degrees> {
    let flat = plane.normal[stance.u].hypot(plane.normal[stance.v]);
    if flat < LEVEL_WALL {
        return None;
    }
    let bevel = Degrees::from_radians((plane.normal[stance.t] / flat).abs().atan())?;
    (bevel.millidegrees() > 0).then_some(bevel)
}

/// The bevel each edge of the section asks for, in the section's own corner order.
///
/// An edge of a convex section lies on one of the cell's wall lines, so the wall that bounds it is
/// the plane whose flat normal faces the edge's outward side and whose line the edge's start point
/// sits on. The plane's normal points into the void and so back at the board, which is why it is the
/// edge's outward normal turned around. A wall level with the board bounds no edge, so it never
/// answers one.
fn wall_bevels(niche: &Niche, section: &[DVec2], face: f64, stance: &Stance) -> Vec<Option<Degrees>> {
    let mut bevels = Vec::with_capacity(section.len());
    for index in 0..section.len() {
        let (Some(start), Some(end)) = (section.get(index), section.get((index + 1) % section.len())) else {
            bevels.push(None);
            continue;
        };
        let direction = *end - *start;
        let length = direction.length();
        let Some(outward) = (length > 1.0e-9).then(|| DVec2::new(direction.y, -direction.x) / length) else {
            bevels.push(None);
            continue;
        };
        let mut found = None;
        for plane in &niche.planes {
            let Some(bevel) = wall_bevel(plane, stance) else {
                continue;
            };
            let flat = DVec2::new(plane.normal[stance.u], plane.normal[stance.v]);
            let width = flat.length();
            if width <= 0.0 || !width.is_finite() {
                continue;
            }
            let normal = flat / width;
            if normal.dot(outward) > -1.0 + 1.0e-6 {
                continue;
            }
            let offset = (plane.offset - plane.normal[stance.t] * face) / width;
            if (normal.dot(*start) - offset).abs() > WALL_TOLERANCE {
                continue;
            }
            found = Some(bevel);
            break;
        }
        bevels.push(found);
    }
    bevels
}

/// The part a footprint becomes in `mode`, with the section point the panel is anchored at.
fn part_of(
    footprint: &[DVec2],
    mode: FillMode,
    stance: &Stance,
    angles: &[Option<Degrees>],
    thickness: Length,
) -> Result<(Part, DVec2), FillRefusal> {
    let Some(first) = footprint.first().copied() else {
        return Err(FillRefusal::TooSmall);
    };
    if footprint.iter().any(|point| !point.is_finite()) {
        return Err(FillRefusal::TooSmall);
    }
    let low = footprint.iter().copied().fold(first, DVec2::min);
    let high = footprint.iter().copied().fold(first, DVec2::max);
    match mode {
        FillMode::Fit => {
            let (corner, size) = largest_rectangle(footprint).ok_or(FillRefusal::TooSmall)?;
            let nominal_size = PanelNominalSize::new(
                length_of(size.x).ok_or(FillRefusal::TooSmall)?,
                length_of(size.y).ok_or(FillRefusal::TooSmall)?,
            )
            .map_err(|_| FillRefusal::TooSmall)?;
            let corners =
                vec![DVec2::ZERO, DVec2::new(size.x, 0.0), DVec2::new(size.x, size.y), DVec2::new(0.0, size.y)];
            let outline = PanelOutline::Rect { corner_features: Vec::new(), edge_bows: Vec::new() };
            Ok((
                Part { outline, nominal_size, corners: corners.clone(), far_corners: corners, bevels: Vec::new() },
                corner,
            ))
        }
        FillMode::Shape => {
            let ring = local_ring(footprint, angles, stance, low, high);
            let contour = contour_of(&ring.points)?;
            let nominal_size = Panel::contour_size(&contour).map_err(|_| FillRefusal::TooSmall)?;
            let mut bevels = Vec::new();
            for (index, bevel) in ring.bevels.iter().enumerate() {
                let Some(bevel) = *bevel else {
                    continue;
                };
                let Some(segment) = contour.id_at(index) else {
                    return Err(FillRefusal::TooSmall);
                };
                let record = EdgeBevel::new(EdgeRef::Ring { segment }, bevel).map_err(|_| FillRefusal::Wedge)?;
                bevels.push(record);
            }
            // A beveled board is a contour whatever its polygon looks like: the rectangle generator
            // has no place for an end bevel, so the classifier only serves the square cases.
            let outline = if bevels.is_empty() {
                classify_outline(&contour).unwrap_or(PanelOutline::Contour { contour })
            } else {
                PanelOutline::Contour { contour }
            };
            let far_corners = far_face(&outline, &bevels, &ring.points, thickness)?;
            let anchor = DVec2::new(low.x, if stance.against_y { high.y } else { low.y });
            Ok((Part { outline, nominal_size, corners: ring.points, far_corners, bevels }, anchor))
        }
    }
}

/// A section in the panel's own plane, anchored at its zero corner, with the bevel on each edge.
struct LocalRing {
    /// The ring's corners, counter-clockwise from the panel's zero corner.
    points: Vec<DVec2>,
    /// The bevel the edge from `points[i]` to the next corner asks for, if any.
    bevels: Vec<Option<Degrees>>,
}

/// The section as the panel's own ring.
///
/// The board's own `Y` may run against the section's second axis, which mirrors the polygon and so
/// reverses its travel direction; the ring is turned back round, and each bevel travels with the
/// edge it belongs to so the two stay in step.
fn local_ring(footprint: &[DVec2], angles: &[Option<Degrees>], stance: &Stance, low: DVec2, high: DVec2) -> LocalRing {
    let mut points: Vec<DVec2> = footprint
        .iter()
        .map(|point| DVec2::new(point.x - low.x, if stance.against_y { high.y - point.y } else { point.y - low.y }))
        .collect();
    let mut bevels: Vec<Option<Degrees>> =
        (0..footprint.len()).map(|index| angles.get(index).copied().flatten()).collect();
    if stance.against_y {
        let count = points.len();
        points = (0..count).filter_map(|index| points.get((count - index) % count).copied()).collect();
        bevels = (0..count).filter_map(|index| bevels.get((count - 1 - index) % count).copied()).collect();
    }
    LocalRing { points, bevels }
}

/// The section as the document ring a panel stores, anchored at `(0, 0)`.
fn contour_of(points: &[DVec2]) -> Result<Contour, FillRefusal> {
    let corners = points
        .iter()
        .map(|point| Some([document_length(point.x)?, document_length(point.y)?]))
        .collect::<Option<Vec<_>>>()
        .ok_or(FillRefusal::TooSmall)?;
    Contour::from_polygon(corners).map_err(|_| FillRefusal::TooSmall)
}

/// The far face's polygon: the base ring with every beveled edge set back by its own angle.
fn far_face(
    outline: &PanelOutline,
    bevels: &[EdgeBevel],
    corners: &[DVec2],
    thickness: Length,
) -> Result<Vec<DVec2>, FillRefusal> {
    let Some(ring) = outline.contour() else {
        return Ok(corners.to_vec());
    };
    let mut setbacks = vec![0.0; corners.len()];
    for bevel in bevels {
        let EdgeRef::Ring { segment } = bevel.source() else {
            continue;
        };
        let Some(index) = ring.index_of(segment) else {
            continue;
        };
        let Some(slot) = setbacks.get_mut(index) else {
            continue;
        };
        *slot = bevel.setback(thickness).ok_or(FillRefusal::Wedge)?.millimetres_f64();
    }
    set_back(corners, &setbacks).ok_or(FillRefusal::TooSmall)
}

/// A convex ring with each edge's line moved into the body by that edge's own distance.
///
/// The moved lines meet at the inset corners, so the result is the ring the far face keeps. A ring
/// that collapses answers `None`.
fn set_back(corners: &[DVec2], setbacks: &[f64]) -> Option<Vec<DVec2>> {
    let count = corners.len();
    if count < 3 || setbacks.len() != count {
        return None;
    }
    let mut lines: Vec<(DVec2, DVec2)> = Vec::with_capacity(count);
    for index in 0..count {
        let start = *corners.get(index)?;
        let end = *corners.get((index + 1) % count)?;
        let direction = end - start;
        let length = direction.length();
        if !length.is_finite() || length <= 1.0e-9 {
            return None;
        }
        let inward = DVec2::new(-direction.y, direction.x) / length;
        let setback = *setbacks.get(index)?;
        if !setback.is_finite() || setback < 0.0 {
            return None;
        }
        lines.push((start + inward * setback, direction / length));
    }
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let (previous, previous_direction) = *lines.get((index + count - 1) % count)?;
        let (current, current_direction) = *lines.get(index)?;
        let denominator = previous_direction.x * current_direction.y - previous_direction.y * current_direction.x;
        if !denominator.is_finite() {
            return None;
        }
        if denominator.abs() < 1.0e-9 {
            // Two edges in line: their moved lines are one line, and its start point is on it.
            points.push(current);
            continue;
        }
        let across = current - previous;
        let step = (across.x * current_direction.y - across.y * current_direction.x) / denominator;
        let point = previous + previous_direction * step;
        if !point.is_finite() {
            return None;
        }
        points.push(point);
    }
    (area2(&points) > 1.0e-9).then_some(points)
}

/// The signed area of a polygon, positive when its corners run counter-clockwise.
fn area2(points: &[DVec2]) -> f64 {
    let mut total = 0.0;
    for index in 0..points.len() {
        let (Some(start), Some(end)) = (points.get(index), points.get((index + 1) % points.len())) else {
            continue;
        };
        total += start.x * end.y - end.x * start.y;
    }
    total * 0.5
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

    /// A board standing as a wall across the case's back, its inner face through `y = through` at
    /// `z = 0` and leaned by `degrees` about `+X`.
    ///
    /// The free space is in front of it. A positive lean brings the face forward as it rises, so the
    /// void narrows towards the top; a negative one widens it.
    fn leaned_wall(project: &mut Project, product: Id, through: f64, degrees: f64) {
        let lean = degrees.to_radians();
        let wall = DQuat::from_rotation_z(-FRAC_PI_2) * DQuat::from_rotation_y(-FRAC_PI_2);
        let rotation = DQuat::from_rotation_x(lean) * wall;
        let placement = Placement::new(DVec3::new(0.0, through, 0.0), rotation).expect("a wall placement");
        // The wall stance runs its length up the case and its width across it, so the nominal size
        // is the case's height by its width; the panel is longer than the case so its face still
        // covers the whole void once it leans.
        let nominal = PanelNominalSize::new(millimetres(1600), millimetres(600)).expect("a wall size");
        project.add_panel(product, "Wall", nominal, None, millimetres(18), placement).expect("add a wall");
    }

    /// A board leaning 10 degrees over the case, its underside the void's ceiling.
    fn leaned_ceiling(project: &mut Project, product: Id) {
        let lean = 10.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        let along = DVec3::new(0.0, cos, -sin);
        let away = DVec3::new(0.0, sin, cos);
        let rotation = DQuat::from_rotation_axes(along.cross(away), along, away);
        let placement = Placement::new(DVec3::new(0.0, 0.0, 699.5), rotation).expect("a ceiling placement");
        let nominal = PanelNominalSize::new(millimetres(600), millimetres(620)).expect("a ceiling size");
        project.add_panel(product, "Ceiling", nominal, None, millimetres(18), placement).expect("add a ceiling");
    }

    /// Whether both faces of the plan's body lie inside the void it was measured against.
    fn inside(plan: &FillPlan, thickness: Length) -> bool {
        const TOLERANCE: f64 = 2.0e-3;
        let gap = thickness.millimetres_f64();
        let mut points = Vec::new();
        for (corners, z) in [(&plan.corners, 0.0), (&plan.far_corners, gap)] {
            for corner in corners {
                points.push(plan.placement.transform_point(DVec3::new(corner.x, corner.y, z)));
            }
        }
        points.iter().all(|point| {
            point.is_finite()
                && plan.niche.planes.iter().all(|plane| plane.normal.dot(*point) - plane.offset >= -TOLERANCE)
        })
    }

    /// The base ring's edge that carries `bevel`, as its two corners in panel-local millimetres.
    fn beveled_edge(plan: &FillPlan, bevel: EdgeBevel) -> (DVec2, DVec2) {
        let ring = plan.outline.contour().expect("a beveled part is a contour");
        let EdgeRef::Ring { segment } = bevel.source() else {
            panic!("a fill bevels ring segments");
        };
        let index = ring.index_of(segment).expect("the bevel names a segment of the ring");
        let count = plan.corners.len();
        (plan.corners[index], plan.corners[(index + 1) % count])
    }

    /// How far the far edge under `bevel` sits from the wall plane `normal . p = offset`, in mm.
    fn far_edge_distance(plan: &FillPlan, bevel: EdgeBevel, normal: DVec3, offset: f64, thickness: Length) -> f64 {
        let (start, end) = beveled_edge(plan, bevel);
        let direction = end - start;
        let length = direction.length();
        let inward = DVec2::new(-direction.y, direction.x) / length;
        let setback = bevel.setback(thickness).expect("a setback").millimetres_f64();
        let gap = thickness.millimetres_f64();
        let mut worst: f64 = 0.0;
        for corner in [start, end] {
            let moved = corner + inward * setback;
            let point = plan.placement.transform_point(DVec3::new(moved.x, moved.y, gap));
            worst = worst.max((normal.dot(point) - offset).abs());
        }
        worst
    }

    #[test]
    fn a_shelf_against_a_leaned_wall_gets_the_walls_own_bevel() {
        let (mut project, product) = empty_product(600, 560, 720);
        leaned_wall(&mut project, product, 560.0, 10.0);
        let thickness = millimetres(18);
        let shaped = plan(&project, product, Fill::WidthAndDepth, FillMode::Shape, thickness).expect("a shaped shelf");
        assert!(shaped.outline.contour().is_some(), "a beveled board is a contour whatever its polygon looks like");
        assert_eq!(shaped.bevels.len(), 1, "only the leaning wall asks for one: {:?}", shaped.bevels);
        let bevel = shaped.bevels[0];
        assert_eq!(bevel.angle().millidegrees(), 10_000, "the wall's own lean");
        let lean = 10.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        // The wall's free-side normal and the offset of its plane through (0, 560, 0).
        let normal = DVec3::new(0.0, -cos, -sin);
        let distance = far_edge_distance(&shaped, bevel, normal, normal.dot(DVec3::new(0.0, 560.0, 0.0)), thickness);
        assert!(distance <= 1.0e-3, "the far edge lies in the wall's plane, off by {distance} mm");
        assert!(inside(&shaped, thickness), "and the whole board stays in the void");
        assert!(shaped.placement.transform_vector(DVec3::Z).z > 0.0, "face B is the wide one, so the board stands");

        // A plumb wall in the same case asks for nothing and keeps the standard panel.
        let (mut project, product) = empty_product(600, 560, 720);
        leaned_wall(&mut project, product, 560.0, 0.0);
        let square = plan(&project, product, Fill::WidthAndDepth, FillMode::Shape, thickness).expect("a square shelf");
        assert!(square.bevels.is_empty(), "a plumb wall needs no bevel: {:?}", square.bevels);
        assert_eq!(square.corners, square.far_corners, "so the far face is the base rim lifted");
        assert!(matches!(square.outline, PanelOutline::Rect { .. }), "and the part is the standard panel");
    }

    #[test]
    fn a_wall_leaned_the_other_way_turns_the_board_and_still_meets_it() {
        let (mut project, product) = empty_product(600, 560, 720);
        // The wall stands inside the case, so its face keeps bounding the void as it leans back.
        leaned_wall(&mut project, product, 400.0, -10.0);
        let thickness = millimetres(18);
        let shaped = plan(&project, product, Fill::WidthAndDepth, FillMode::Shape, thickness).expect("a shaped shelf");
        assert_eq!(shaped.bevels.len(), 1, "the wall still asks for its one bevel: {:?}", shaped.bevels);
        let bevel = shaped.bevels[0];
        assert_eq!(bevel.angle().millidegrees(), 10_000, "the same angle from the other side");
        // The wide face is the top here, so the board is turned about its length axis to keep the
        // broad face as the stored outline (ADR 0108).
        assert!(shaped.placement.transform_vector(DVec3::Z).z < 0.0, "the board is turned: {:?}", shaped.placement);
        let lean = 10.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        let normal = DVec3::new(0.0, -cos, sin);
        let distance = far_edge_distance(&shaped, bevel, normal, normal.dot(DVec3::new(0.0, 400.0, 0.0)), thickness);
        assert!(distance <= 1.0e-3, "the far edge lies in the wall's plane, off by {distance} mm");
        assert!(inside(&shaped, thickness), "and the whole board stays in the void");
    }

    #[test]
    fn a_void_whose_walls_lean_against_each_other_has_no_flat_board() {
        let shelf = stance(Fill::WidthAndDepth);
        let lean = 10.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        let narrows = NichePlane { normal: DVec3::new(0.0, -cos, -sin), offset: -cos * 100.0 };
        let widens = NichePlane { normal: DVec3::new(0.0, -cos, sin), offset: -cos * 500.0 };
        let floor = NichePlane { normal: DVec3::Z, offset: 0.0 };
        let walls = |planes: Vec<NichePlane>| Niche {
            product: Id::new(1).expect("an id"),
            local_min: DVec3::ZERO,
            size: DVec3::new(600.0, 560.0, 720.0),
            min: DVec3::ZERO,
            shape: NicheShape::Sloped,
            planes,
            corners: Vec::new(),
        };
        assert_eq!(
            reference_face(&walls(vec![narrows, widens, floor]), 100.0, 18.0, shelf),
            Err(FillRefusal::WallsDisagree),
            "one wall narrows the board at face A and the other widens it there"
        );
        assert_eq!(
            reference_face(&walls(vec![NichePlane { normal: DVec3::Z, offset: 110.0 }]), 100.0, 18.0, shelf),
            Err(FillRefusal::Wedge),
            "a floor inside the slab leaves no board at any face"
        );
        assert!(
            reference_face(&walls(vec![widens, floor]), 100.0, 18.0, shelf).is_ok_and(|stance| stance.other_face),
            "a wall that widens alone turns the board"
        );
        assert!(
            reference_face(&walls(vec![narrows, floor]), 100.0, 18.0, shelf).is_ok_and(|stance| !stance.other_face),
            "and one that narrows leaves it as it stands"
        );
    }

    #[test]
    fn a_shelf_under_a_leaned_ceiling_takes_the_slab_section_in_the_shape_mode() {
        // The ceiling is nearly level with the board, so no end face can meet it; the board still
        // fills the section the whole slab leaves, with no bevel, in both modes (ADR 0109).
        let (mut project, product) = empty_product(600, 560, 720);
        leaned_ceiling(&mut project, product);
        let thickness = millimetres(18);
        let shaped = plan(&project, product, Fill::WidthAndDepth, FillMode::Shape, thickness)
            .expect("the shape falls back to the slab section");
        assert!(shaped.bevels.is_empty(), "a level wall asks for no end face: {:?}", shaped.bevels);
        assert_eq!(shaped.corners, shaped.far_corners, "the far face is the base rim lifted");
        let fitted = plan(&project, product, Fill::WidthAndDepth, FillMode::Fit, thickness).expect("a fit plan");
        assert_eq!(shaped.nominal_size, fitted.nominal_size, "the slab section is the fit's rectangle here");
    }

    #[test]
    fn a_splayed_case_under_a_short_top_offers_every_build() {
        // The owner's case: two boards turned in the product frame and a top that does not reach
        // the open front. Every build must answer instead of refusing the void (ADR 0109).
        let (mut project, product) = splayed_product();
        let top = PanelNominalSize::new(millimetres(600), millimetres(400)).expect("a short top");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(0), millimetres(0), millimetres(702))
            .expect("a top placement");
        project.add_panel(product, "Top", top, None, millimetres(18), placement).expect("add a top");
        let thickness = millimetres(18);
        for fill in [Fill::WidthAndDepth, Fill::HeightAndDepth] {
            for mode in [FillMode::Fit, FillMode::Shape] {
                let planned = plan(&project, product, fill, mode, thickness)
                    .unwrap_or_else(|refusal| panic!("{fill:?}/{mode:?} refused: {refusal:?}"));
                assert!(planned.corners.len() >= 3, "{fill:?}/{mode:?} has a footprint");
                assert!(planned.nominal_size.length().millimetres_f64() > 0.0);
            }
        }
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
        assert!(shaped.bevels.is_empty(), "the posts stand plumb, so no edge needs a bevel");
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
            assert!(plan.bevels.is_empty(), "{mode:?} asks for no bevel in a square void");
            assert_eq!(plan.corners, plan.far_corners, "{mode:?} ends the board over its own rim");
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
    fn a_turned_board_keeps_its_length_axis_and_takes_the_other_face() {
        for fill in [Fill::WidthAndDepth, Fill::HeightAndDepth] {
            let stance = stance(fill);
            let turned = stance.turned();
            let (standing, over) = (stance.rotation(), turned.rotation());
            // The length axis is untouched, which is what makes the turn a placement of the same
            // stance rather than a fourth one (ADR 0105, ADR 0108).
            assert!((over.mul_vec3(DVec3::X) - standing.mul_vec3(DVec3::X)).length() <= 1.0e-9, "{fill:?}");
            assert!((over.mul_vec3(DVec3::Z) + standing.mul_vec3(DVec3::Z)).length() <= 1.0e-9, "{fill:?}");
            assert!((over.mul_vec3(DVec3::Y) + standing.mul_vec3(DVec3::Y)).length() <= 1.0e-9, "{fill:?}");
            // The base face moves to the other end of the slab the board takes.
            let (first, other) = if stance.against_z { (28.0, 10.0) } else { (10.0, 28.0) };
            assert!((stance.face(10.0, 18.0) - first).abs() <= 1.0e-9, "{fill:?}");
            assert!((turned.face(10.0, 18.0) - other).abs() <= 1.0e-9, "{fill:?}");
        }
    }

    #[test]
    fn the_free_axis_is_the_panel_thickness_axis_of_each_fill() {
        assert_eq!(free_axis(Fill::WidthAndDepth), 2);
        assert_eq!(free_axis(Fill::HeightAndDepth), 0);
    }
}

#[cfg(test)]
mod turned {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::fixtures::*;

    #[test]
    fn a_splayed_case_turned_in_two_axes_offers_every_build() {
        let root = Container::new(Id::new(1).expect("an id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("a project");
        let envelope = Envelope::new(millimetres(600), millimetres(560), millimetres(720)).expect("an envelope");
        let product = project
            .add_product(project.root().id(), "Piece", ClassCode::GENERAL_PRODUCT, envelope, Placement::IDENTITY)
            .expect("a product");
        let slant = 100.0 / 560.0_f64.hypot(100.0);
        let drop = 560.0 / 560.0_f64.hypot(100.0);
        for (through, normal) in [((100.0, 0.0), (drop, -slant)), ((500.0, 0.0), (-drop, -slant))] {
            let direction = (-normal.1, normal.0);
            let start_y = 280.0 - 600.0 * direction.1;
            let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
            let rotation = DQuat::from_rotation_y(10.0_f64.to_radians())
                * DQuat::from_rotation_z(normal.1.atan2(normal.0))
                * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
            let translation = DVec3::new(start_x, start_y, 720.0) - DVec3::new(normal.0, normal.1, 0.0) * 18.0;
            let placement = Placement::new(translation, rotation).expect("a post placement");
            let size = PanelNominalSize::new(millimetres(720), millimetres(1200)).expect("a post size");
            project.add_panel(product, "Post", size, None, millimetres(18), placement).expect("the post lands");
        }
        let top = PanelNominalSize::new(millimetres(600), millimetres(400)).expect("a short top");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(0), millimetres(0), millimetres(702))
            .expect("a top placement");
        project.add_panel(product, "Top", top, None, millimetres(18), placement).expect("add a top");
        let point = DVec3::new(300.0, 280.0, 360.0);
        for fill in [Fill::WidthAndDepth, Fill::HeightAndDepth] {
            for mode in [FillMode::Fit, FillMode::Shape] {
                let planned = project
                    .fill_plan(product, point, fill, mode, millimetres(18), None)
                    .unwrap_or_else(|refusal| panic!("{fill:?}/{mode:?} refused: {refusal:?}"));
                assert!(planned.corners.len() >= 3, "{fill:?}/{mode:?} has a footprint");
            }
        }
    }
}
