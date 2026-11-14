//! The niche probe: the free space around a point, as the cell its walls bound.
//!
//! A niche is the space a piece of furniture reserves (ADR 0099); it is derived from the document
//! every time it is asked for and never stored (ADR 0102, ADR 0103). Two questions answer it:
//!
//! - [`Project::probe`] casts one ray from a point and reports the face it stops at — its distance,
//!   its plane and the furniture entity that owns it: a panel face, a geometry body, another
//!   product's shell, or a virtual face of the envelope of the piece the point sits in;
//! - [`Project::niche`] builds the free space around the point as a convex cell: the product's six
//!   envelope planes, clipped by the plane of every face a probe meets, then probed again from the
//!   cell's own corners until a round adds no plane. The cell answers the box the callouts draw,
//!   the supporting planes and corners a tool measures against, and the section a board of a given
//!   thickness takes ([`Niche::cut`]).
//!
//! The cell is why a board turned in space is measured at all: a leaned back wall, a ceiling that
//! comes down towards the back, a piece turned off the world axes (ADR 0106). The cell is built in
//! the product's own frame, so a turned product is measured like any other. A void that is not
//! convex — one holding something the cell would swallow — is refused with a reason the tool can
//! report rather than approximated.
//!
//! Both are pure queries: no session, no renderer, no document write. The surfaces are the same
//! boxes the furniture move rule measures against (`super::surface`), so the tool that fills a
//! niche and the tool that drags a piece cannot disagree about where a surface is.

use furnicraft_geom::{DVec2, DVec3, Region};

use super::surface::{FaceHit, SurfaceBox, SurfaceKind, surface_box};
use crate::tree::object_bounds_mm;
use crate::*;

/// Tolerance for a measurement that is already a whole micrometre, in millimetres.
const SECTION_TOLERANCE: f64 = 1.0e-6;

/// Smallest share of a plane's normal that must lie in the plane it is cut with.
///
/// A plane almost level with the cut answers a line far outside the void, so it is left to the
/// constant test instead of producing a corner at a huge coordinate.
const SECTION_NORMAL: f64 = 1.0e-3;

/// Smallest sine of the angle between two planes that still meet in a corner or a line.
const SECTION_ANGLE: f64 = 1.0e-6;

/// Cosine above which two plane normals count as the same direction.
const PLANE_PARALLEL: f64 = 1.0 - 1.0e-6;

/// Cosine above which a plane's normal counts as level with an axis.
const NORMAL_LEVEL: f64 = 1.0e-6;

/// How far inside the free space a wall's coverage is tested, in millimetres.
const COVER_INSET: f64 = 1.0e-3;

/// Tolerance for comparing a cell corner with a plane, a face or a box, in millimetres.
///
/// The probes report whole micrometres, so a corner where two walls meet can land a micrometre
/// outside the face that owns one of them: that is rounding, not a gap.
const COORDINATE_TOLERANCE: f64 = 2.0e-3;

/// The rounds one cell build may run before it refuses to spin.
const MAX_CELL_ROUNDS: usize = 6;

/// The supporting planes one cell may carry.
const MAX_CELL_PLANES: usize = 16;

/// The directions one round may probe.
const MAX_CELL_PROBES: usize = 64;

/// The corners one cell or one cut may have.
const MAX_CELL_CORNERS: usize = 64;

/// What one probe stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeHit {
    /// The node that owns the surface: a product for its shell or envelope, a panel, an object.
    pub id: Id,
    /// Which kind of surface the probe met.
    pub kind: SurfaceKind,
}

/// The face a probe met, in world millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeFace {
    /// The point where the ray meets the face.
    pub point: DVec3,
    /// The face's unit normal, pointing back along the ray.
    ///
    /// The free space therefore lies in the half-space `normal · (point - hit) >= 0`, which is what
    /// lets a cell be built from probed faces.
    pub normal: DVec3,
}

/// The answer of one probe: the distance to the first face and what it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// The distance along the probe direction, in millimetres, a whole number of micrometres.
    pub distance_mm: f64,
    /// The surface the ray stopped at.
    pub hit: ProbeHit,
    /// The face the ray stopped at.
    pub face: ProbeFace,
}

/// One supporting plane of a niche's free space, in the product's own frame.
///
/// The free space lies in the half-space `normal · point >= offset`: the normal points into the
/// void and the offset is in millimetres, so every point of the cell satisfies every plane of
/// [`Niche::planes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NichePlane {
    /// The plane's unit normal, pointing into the free space.
    pub normal: DVec3,
    /// The plane's offset along that normal, in millimetres.
    pub offset: f64,
}

impl NichePlane {
    /// The signed distance from `point` to this plane, positive on the free side.
    fn distance_to(self, point: DVec3) -> f64 {
        self.normal.dot(point) - self.offset
    }
}

/// The shape of a niche.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NicheShape {
    /// The free space is exactly the box its corners span.
    Box,
    /// The free space is one section from its floor to its ceiling, stored as a ring in the
    /// product's own XY plane.
    ///
    /// The ring is the outline a part lying in that plane takes: a shelf in a case whose posts stand
    /// at an angle is a trapezoid, not the rectangle around it (ADR 0099). Its coordinates are the
    /// product frame's, in the document's micrometres.
    Section(Contour),
    /// The free space is a cell whose section changes along the product's height.
    ///
    /// A ceiling that comes down towards the back, a wall leaned across the section: one ring would
    /// be false somewhere in the void, so the planes and corners answer instead, and a board takes
    /// the cut it needs ([`Niche::cut`]).
    Sloped,
}

/// Why a point has no usable niche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NicheRefusal {
    /// The point is outside every product, or is not a usable number.
    Outside,
    /// The free space around the point is not one convex cell: it holds something, or a wall does
    /// not reach the whole face the cell would need.
    Unrepresentable,
}

/// The empty space around a point inside a product.
///
/// Everything here is derived: the query stores nothing in the document. `local_min` and `size` are
/// the box the free space occupies, in the product's own frame, which is the frame a panel is placed
/// in; `min` is the same corner in world millimetres, so a tool never re-derives a position from a
/// cursor and a size. `planes` and `corners` are the exact cell, and `shape` names the case a board
/// reads directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Niche {
    /// The product whose void was measured.
    pub product: Id,
    /// The box's smallest corner in the product's own frame, millimetres.
    pub local_min: DVec3,
    /// The box's size along the product's own axes, millimetres.
    pub size: DVec3,
    /// The same smallest corner in world millimetres.
    pub min: DVec3,
    /// The shape of the free space.
    pub shape: NicheShape,
    /// The cell's supporting planes, in the product's own frame.
    ///
    /// Together with [`Niche::corners`] this is the exact free space: every plane's free half-space,
    /// intersected.
    pub planes: Vec<NichePlane>,
    /// The cell's corners, in the product's own frame and whole micrometres.
    pub corners: Vec<DVec3>,
}

impl Niche {
    /// The box's far corner in the product's own frame, millimetres.
    pub fn local_max(&self) -> DVec3 {
        self.local_min + self.size
    }

    /// One side of the box along the product's own axes, in millimetres.
    pub fn side_mm(&self, axis: usize) -> f64 {
        match axis {
            0 => self.size.x,
            1 => self.size.y,
            _ => self.size.z,
        }
    }

    /// The free section's polygon, when the free space is one section from floor to ceiling.
    ///
    /// The ring is in the product's own XY plane, the plane a shelf's outline lies in.
    pub fn section(&self) -> Option<&Contour> {
        match &self.shape {
            NicheShape::Section(ring) => Some(ring),
            NicheShape::Box | NicheShape::Sloped => None,
        }
    }

    /// The free space a board of the given thickness takes, as a convex polygon.
    ///
    /// The cell is cut by the slab between `low` and `high` along the product's `axis` and the cut is
    /// projected on the two other axes, in `(u, v)` order: a shelf asks for `axis = 2` and
    /// `(u, v) = (0, 1)`, a board standing across the void for `axis = 0` and `(u, v) = (2, 1)`.
    /// Every plane's half-space is evaluated at the slab's own two ends, so a wall that leans across
    /// the slab limits the part at the face that would cut into it.
    ///
    /// The points are whole micrometres in the product's frame, running counter-clockwise in
    /// `(u, v)`. A slab that leaves no area — outside the void, or thinner than the gap the walls
    /// leave — answers `None`, as does an impossible axis order.
    pub fn cut(&self, axis: usize, (low, high): (f64, f64), (u, v): (usize, usize)) -> Option<Vec<DVec2>> {
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        let polygon = cut_planes(&self.planes, axis, low, high, u, v)?;
        Some(polygon.into_iter().map(quantised_point).collect())
    }
}

/// The surfaces one point is measured against.
struct Surroundings {
    /// The innermost envelope the point sits in, measured as an envelope rather than a shell.
    own: Option<(Id, SurfaceBox)>,
    /// Every other surface a ray can stop at: the bodies and the other products' shells.
    surfaces: Vec<SurfaceBox>,
}

/// The free space around a point, as the convex cell its supporting planes bound.
///
/// The corners are always derived from the planes, so a plane and the cell it cuts cannot disagree.
#[derive(Clone, Debug)]
struct Cell {
    planes: Vec<NichePlane>,
    /// The corners, sorted by their coordinates so the same cell always reports the same list.
    corners: Vec<DVec3>,
}

impl Cell {
    /// The cell inside a product's envelope: its six planes, before any part is met.
    fn envelope(size: DVec3) -> Self {
        let planes = vec![
            NichePlane { normal: DVec3::X, offset: 0.0 },
            NichePlane { normal: DVec3::NEG_X, offset: -size.x },
            NichePlane { normal: DVec3::Y, offset: 0.0 },
            NichePlane { normal: DVec3::NEG_Y, offset: -size.y },
            NichePlane { normal: DVec3::Z, offset: 0.0 },
            NichePlane { normal: DVec3::NEG_Z, offset: -size.z },
        ];
        let mut cell = Self { planes, corners: Vec::new() };
        cell.rebuild();
        cell
    }

    /// Rebuilds the corners from the planes, answering whether the planes bound a usable cell.
    ///
    /// Every triple of planes that meets inside all of them is a corner: a convex cell is the
    /// intersection of its planes, and it needs at least four corners to enclose anything.
    fn rebuild(&mut self) -> bool {
        let mut corners: Vec<DVec3> = Vec::new();
        for (index, first) in self.planes.iter().enumerate() {
            for (second_index, second) in self.planes.iter().enumerate().skip(index.saturating_add(1)) {
                for third in self.planes.iter().skip(second_index.saturating_add(1)) {
                    let Some(point) = corner_of(first, second, third) else {
                        continue;
                    };
                    if self.planes.iter().any(|plane| plane.distance_to(point) < -COORDINATE_TOLERANCE) {
                        continue;
                    }
                    if corners.iter().any(|seen| (*seen - point).length() <= COORDINATE_TOLERANCE) {
                        continue;
                    }
                    corners.push(point);
                }
            }
        }
        if corners.len() < 4 || corners.len() > MAX_CELL_CORNERS {
            self.corners.clear();
            return false;
        }
        corners.sort_by(corner_order);
        self.corners = corners;
        true
    }

    /// Whether the point lies in every plane, allowing the micrometre the corners are stored in.
    fn contains(&self, point: DVec3) -> bool {
        self.corners.len() >= 4 && self.planes.iter().all(|plane| plane.distance_to(point) >= -COORDINATE_TOLERANCE)
    }

    /// Whether the cell is one section from its floor to its ceiling, so one ring describes it.
    ///
    /// A cell whose planes are all either flat or upright is the section between two heights; one
    /// whose plane leans across both is not.
    fn is_prism(&self) -> bool {
        self.planes.iter().all(|plane| {
            let normal = plane.normal;
            let flat = normal.x.abs() <= NORMAL_LEVEL && normal.y.abs() <= NORMAL_LEVEL;
            let upright = normal.z.abs() <= NORMAL_LEVEL;
            flat || upright
        })
    }

    /// Whether `plane` removes any part of the cell; a plane the cell already lies on changes nothing.
    fn cuts(&self, plane: NichePlane) -> bool {
        let known = self.planes.iter().any(|existing| {
            existing.normal.dot(plane.normal) >= PLANE_PARALLEL
                && (existing.offset - plane.offset).abs() <= COORDINATE_TOLERANCE
        });
        if known {
            return false;
        }
        self.corners.iter().any(|corner| plane.distance_to(*corner) < -COORDINATE_TOLERANCE)
    }

    /// The cell cut by one more plane, or `None` when the cut leaves no cell or one plane too many.
    fn clipped(&self, plane: NichePlane) -> Option<Self> {
        if self.planes.len() >= MAX_CELL_PLANES {
            return None;
        }
        let mut cut = self.clone();
        cut.planes.push(plane);
        cut.rebuild().then_some(cut)
    }

    /// The directions the next round probes from `point` in the product's frame.
    ///
    /// Each face normal finds the wall behind it and each corner finds whatever cuts the corner off.
    /// The first round also probes the envelope's twenty-six axis, edge and corner directions, so a
    /// wall standing between two cell corners is met before any corner exists to point at it.
    fn directions(&self, point: DVec3, first: bool) -> Vec<DVec3> {
        let mut directions: Vec<DVec3> = Vec::new();
        let mut add = |direction: DVec3| {
            let Some(unit) = direction.try_normalize() else {
                return;
            };
            if directions.len() >= MAX_CELL_PROBES {
                return;
            }
            if directions.iter().any(|seen| (*seen - unit).length() <= SECTION_ANGLE) {
                return;
            }
            directions.push(unit);
        };
        for plane in &self.planes {
            add(plane.normal);
        }
        for corner in &self.corners {
            add(*corner - point);
        }
        if first {
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        let direction = DVec3::new(x as f64, y as f64, z as f64);
                        if direction.length_squared() > 0.0 {
                            add(direction);
                        }
                    }
                }
            }
        }
        directions
    }
}

impl Project {
    /// The distance from `point` to the first furniture face along `direction`.
    ///
    /// The ray stops at a panel face, a geometry body, another product's shell, or a virtual face of
    /// the envelope of the innermost product that contains the point. It never passes through a
    /// surface (ADR 0102) and it is measured in world millimetres, quantised to the document's
    /// micrometre. A point that is not finite, a direction that is zero or not finite, and a ray
    /// that meets nothing all answer `None`.
    pub fn probe(&self, point: DVec3, direction: DVec3) -> Option<Probe> {
        let surroundings = self.surroundings_around(point);
        self.probe_within(point, direction, &surroundings)
    }

    /// The empty space around `point`, as the convex cell its walls bound.
    ///
    /// The product is the innermost envelope that contains the point, and the cell is built in that
    /// product's own frame: the envelope's six planes, clipped by the plane of every face a probe
    /// meets, probed again from the cell's own corners until a round adds no plane. A rectangular
    /// case answers [`NicheShape::Box`]; a case with splayed posts answers the trapezoid as
    /// [`NicheShape::Section`]; a leaned wall or ceiling answers [`NicheShape::Sloped`] with the cell
    /// itself. A point outside every product is refused, and so is a void that is not one convex
    /// cell (ADR 0099, ADR 0106).
    pub fn niche(&self, point: DVec3) -> Result<Niche, NicheRefusal> {
        if !point.is_finite() {
            return Err(NicheRefusal::Outside);
        }
        let (product, envelope) = self.product_containing(point).ok_or(NicheRefusal::Outside)?;
        let placement = self.world_placement(product).ok_or(NicheRefusal::Outside)?;
        let inverse = placement.inverse().map_err(|_| NicheRefusal::Outside)?;
        let local = inverse.transform_point(point);
        if !local.is_finite() || !within_box(local, envelope.local_min, envelope.local_max) {
            return Err(NicheRefusal::Outside);
        }
        let surroundings = self.surroundings_around(point);
        let mut cell = Cell::envelope(envelope.local_max - envelope.local_min);
        let mut settled = false;
        for round in 0..MAX_CELL_ROUNDS {
            let mut added = false;
            for direction in cell.directions(local, round == 0) {
                let world = placement.transform_vector(direction);
                let Some(probe) = self.probe_within(point, world, &surroundings) else {
                    continue;
                };
                let Some(plane) = plane_of(probe.face, inverse) else {
                    continue;
                };
                if !cell.cuts(plane) {
                    continue;
                }
                let Some(cut) = cell.clipped(plane) else {
                    return Err(NicheRefusal::Unrepresentable);
                };
                if !self.covers_face(&cut, plane, placement, &surroundings) {
                    return Err(NicheRefusal::Unrepresentable);
                }
                cell = cut;
                added = true;
            }
            if !added {
                settled = true;
                break;
            }
        }
        if !settled || !cell.contains(local) || self.holds_body(&cell, &surroundings, inverse) {
            return Err(NicheRefusal::Unrepresentable);
        }
        let corners: Vec<DVec3> = cell.corners.iter().copied().map(quantised_corner).collect();
        let Some(first) = corners.first().copied() else {
            return Err(NicheRefusal::Unrepresentable);
        };
        let low = corners.iter().copied().fold(first, DVec3::min);
        let high = corners.iter().copied().fold(first, DVec3::max);
        let size = high - low;
        let world_min = placement.transform_point(low);
        if !(size.is_finite() && size.min_element() > 0.0) || !world_min.is_finite() {
            return Err(NicheRefusal::Unrepresentable);
        }
        let shape = cell_shape(&cell, &corners, low, high)?;
        Ok(Niche { product, local_min: low, size, min: world_min, shape, planes: cell.planes, corners })
    }

    /// Whether a surface the cell did not cut against stands inside it.
    ///
    /// A body or another product's shell inside the cell makes the free space non-convex: a board cut
    /// to the cell would cut into it, so the void is refused until the query can answer several
    /// cells. Only a corner strictly inside counts, so the wall that bounded the cell — its own
    /// corners lie on the boundary — is not mistaken for an obstacle.
    fn holds_body(&self, cell: &Cell, surroundings: &Surroundings, inverse: Placement) -> bool {
        surroundings.surfaces.iter().any(|surface| {
            surface_corners(*surface).into_iter().any(|corner| {
                let local = inverse.transform_point(corner);
                local.is_finite() && cell.planes.iter().all(|plane| plane.distance_to(local) > COORDINATE_TOLERANCE)
            })
        })
    }

    /// Whether the wall that bounded the cell at `plane` still covers it.
    ///
    /// The face of the cell that lies on the plane is sampled at each of its corners — moved just
    /// inside so a sample never sits on the edge of the board it tests — and at its middle. A board
    /// that stops halfway leaves the plane empty above its end, which is what refuses it.
    fn covers_face(&self, cell: &Cell, plane: NichePlane, placement: Placement, surroundings: &Surroundings) -> bool {
        let on_face: Vec<DVec3> = cell
            .corners
            .iter()
            .copied()
            .filter(|corner| plane.distance_to(*corner).abs() <= COORDINATE_TOLERANCE)
            .collect();
        if on_face.len() < 3 {
            return false;
        }
        let Some(middle) = midpoint(&on_face) else {
            return false;
        };
        let mut samples: Vec<DVec3> = Vec::with_capacity(on_face.len().saturating_mul(2).saturating_add(1));
        for corner in &on_face {
            if let Some(direction) = (middle - *corner).try_normalize() {
                samples.push(*corner + direction * COVER_INSET);
            }
        }
        samples.push(middle);
        samples.iter().all(|sample| self.covered(plane, *sample, placement, surroundings))
    }

    /// Whether a surface still stands on `plane` at the product-frame point `at`.
    ///
    /// A second surface on the same plane — the envelope a panel is flush with — keeps it a wall,
    /// which is why the plane and not the surface is tested. The ray starts just inside the free
    /// space and looks at the wall, because a ray that starts on a face and points into the solid
    /// measures the far face, not the near one.
    fn covered(&self, plane: NichePlane, at: DVec3, placement: Placement, surroundings: &Surroundings) -> bool {
        let point = placement.transform_point(at + plane.normal * COVER_INSET);
        let outward = -placement.transform_vector(plane.normal);
        self.probe_within(point, outward, surroundings).is_some_and(|probe| probe.distance_mm <= COVER_INSET * 1.5)
    }

    /// The distance to the first furniture face along `direction`, against known surroundings.
    fn probe_within(&self, point: DVec3, direction: DVec3, surroundings: &Surroundings) -> Option<Probe> {
        let (unit, surface, face) = self.first_face(point, direction, surroundings)?;
        let distance = quantised(face.distance);
        let hit_point = point + unit * distance;
        if !hit_point.is_finite() {
            return None;
        }
        Some(Probe {
            distance_mm: distance,
            hit: ProbeHit { id: surface.id, kind: surface.kind },
            face: ProbeFace { point: hit_point, normal: face.normal },
        })
    }

    /// The innermost product whose envelope contains `point`, with that envelope's world box.
    fn product_containing(&self, point: DVec3) -> Option<(Id, SurfaceBox)> {
        let mut innermost: Option<(Id, SurfaceBox, f64)> = None;
        for product in self.products() {
            let Some(envelope) = self.envelope_surface(product) else {
                continue;
            };
            if !envelope.contains(point) {
                continue;
            }
            let volume = (envelope.max - envelope.min).length_squared();
            if innermost.is_none_or(|(_, _, best)| volume < best) {
                innermost = Some((product, envelope, volume));
            }
        }
        innermost.map(|(product, envelope, _)| (product, envelope))
    }

    /// Every surface around `point` and the innermost envelope that contains it.
    ///
    /// Panels and objects are bodies; a product's authored box is a shell. The innermost envelope is
    /// also returned so a probe can report it as an envelope face rather than as the shell of the
    /// piece it is standing in.
    fn surroundings_around(&self, point: DVec3) -> Surroundings {
        let mut surfaces = Vec::new();
        for product in self.products() {
            if let Some(envelope) = self.envelope_surface(product) {
                surfaces.push(envelope);
            }
        }
        self.body_surfaces(&self.root, &mut surfaces, 1);
        let own = self.product_containing(point);
        // The innermost envelope is measured as an envelope, not as one more shell; keeping it in
        // the list would let a tie choose the shell and report the wrong kind.
        let own_id = own.map(|(product, _)| product);
        surfaces.retain(|surface| Some(surface.id) != own_id);
        Surroundings { own, surfaces }
    }

    /// The world box of a product's authored envelope, as a shell surface.
    fn envelope_surface(&self, id: Id) -> Option<SurfaceBox> {
        let envelope = self.container(id)?.envelope()?;
        let size =
            DVec3::new(envelope.x().millimetres_f64(), envelope.y().millimetres_f64(), envelope.z().millimetres_f64());
        surface_box(id, DVec3::ZERO, size, self.world_placement(id)?, SurfaceKind::Shell)
    }

    /// Collects the panels and objects below `container` as body surfaces.
    fn body_surfaces(&self, container: &Container, surfaces: &mut Vec<SurfaceBox>, depth: u32) {
        if depth > Self::MAX_ASSEMBLY_DEPTH {
            return;
        }
        for child in container.children() {
            match child {
                Node::Panel(panel) => {
                    let size = panel.nominal_size();
                    let max = DVec3::new(
                        size.length().millimetres_f64(),
                        size.width().millimetres_f64(),
                        panel.last_known_thickness().millimetres_f64(),
                    );
                    if let Some(world) = self.world_placement(panel.id())
                        && let Some(surface) = surface_box(panel.id(), DVec3::ZERO, max, world, SurfaceKind::Panel)
                    {
                        surfaces.push(surface);
                    }
                }
                Node::Object(object) => {
                    if let Some((min, max)) = object_bounds_mm(object.geometry())
                        && let Some(world) = self.world_placement(object.id())
                        && let Some(surface) = surface_box(object.id(), min, max, world, SurfaceKind::Body)
                    {
                        surfaces.push(surface);
                    }
                }
                Node::Container(inner) => self.body_surfaces(inner, surfaces, depth.saturating_add(1)),
                Node::Camera(_) | Node::Light(_) => {}
            }
        }
    }

    /// The first surface and face a normalized ray meets.
    fn first_face(
        &self,
        point: DVec3,
        direction: DVec3,
        surroundings: &Surroundings,
    ) -> Option<(DVec3, SurfaceBox, FaceHit)> {
        if !point.is_finite() || !direction.is_finite() {
            return None;
        }
        let length = direction.length();
        if !length.is_finite() || length <= f64::EPSILON {
            return None;
        }
        let unit = direction / length;
        let own = surroundings.own;
        let mut best: Option<(SurfaceBox, FaceHit)> = None;
        if let Some((_, envelope)) = own
            && let Some(face) = envelope.face_hit(point, unit)
        {
            let envelope = SurfaceBox { kind: SurfaceKind::Envelope, ..envelope };
            best = Some((envelope, face));
        }
        for surface in &surroundings.surfaces {
            if own.is_some_and(|(product, _)| product == surface.id) {
                continue;
            }
            let Some(face) = surface.face_hit(point, unit) else {
                continue;
            };
            if best.is_none_or(|(_, nearest)| face.distance < nearest.distance) {
                best = Some((*surface, face));
            }
        }
        best.map(|(surface, face)| (unit, surface, face))
    }
}

/// The eight true corners of a surface box, in world millimetres.
///
/// The corners are the board's own, not the world box around a turned board: a slanted wall meets the
/// cell on its face, and the loose box around it would reach across the boundary.
fn surface_corners(surface: SurfaceBox) -> Vec<DVec3> {
    let mut corners = Vec::with_capacity(8);
    for x in [surface.local_min.x, surface.local_max.x] {
        for y in [surface.local_min.y, surface.local_max.y] {
            for z in [surface.local_min.z, surface.local_max.z] {
                corners.push(surface.world.transform_point(DVec3::new(x, y, z)));
            }
        }
    }
    corners
}

/// The shape a cell answers with: a box, one section, or the sloped cell itself.
fn cell_shape(cell: &Cell, corners: &[DVec3], low: DVec3, high: DVec3) -> Result<NicheShape, NicheRefusal> {
    if is_box(corners, low, high) {
        return Ok(NicheShape::Box);
    }
    if !cell.is_prism() {
        return Ok(NicheShape::Sloped);
    }
    let polygon = cut_planes(&cell.planes, 2, low.z, low.z, 0, 1).ok_or(NicheRefusal::Unrepresentable)?;
    ring(polygon).map(NicheShape::Section).ok_or(NicheRefusal::Unrepresentable)
}

/// Whether the corners are exactly the eight corners of the box they span.
fn is_box(corners: &[DVec3], low: DVec3, high: DVec3) -> bool {
    corners.len() == 8
        && corners.iter().all(|corner| {
            [0usize, 1, 2].iter().all(|axis| {
                let value = corner[*axis];
                (value - low[*axis]).abs() <= COORDINATE_TOLERANCE
                    || (value - high[*axis]).abs() <= COORDINATE_TOLERANCE
            })
        })
}

/// The plane of a probed face, in the product's frame.
fn plane_of(face: ProbeFace, inverse: Placement) -> Option<NichePlane> {
    let hit = inverse.transform_point(face.point);
    let normal = inverse.transform_vector(face.normal).try_normalize()?;
    if !hit.is_finite() {
        return None;
    }
    Some(NichePlane { normal, offset: normal.dot(hit) })
}

/// Where three planes meet, or `None` when they do not meet in a point.
fn corner_of(first: &NichePlane, second: &NichePlane, third: &NichePlane) -> Option<DVec3> {
    let across = second.normal.cross(third.normal);
    let determinant = first.normal.dot(across);
    if !determinant.is_finite() || determinant.abs() < SECTION_ANGLE {
        return None;
    }
    let point = (across * first.offset
        + third.normal.cross(first.normal) * second.offset
        + first.normal.cross(second.normal) * third.offset)
        / determinant;
    point.is_finite().then_some(point)
}

/// The cell cut by a slab along `axis`, as the convex polygon seen in the `(u, v)` axes.
///
/// Every plane's half-space is evaluated at the slab's own two ends, so the polygon is exactly the
/// set of `(u, v)` points whose whole slab lies in the cell. The corners are the pairwise meeting
/// points of the cut lines; the cell's own planes keep the polygon bounded.
fn cut_planes(planes: &[NichePlane], axis: usize, low: f64, high: f64, u: usize, v: usize) -> Option<Vec<DVec2>> {
    if axis > 2 || u > 2 || v > 2 || axis == u || u == v || axis == v || !low.is_finite() || !high.is_finite() {
        return None;
    }
    let (low, high) = if low <= high { (low, high) } else { (high, low) };
    if planes.len() < 4 {
        return None;
    }
    // Each plane as a line in the cut's own plane: `normal · (u, v) >= offset`.
    let mut lines: Vec<(DVec2, f64)> = Vec::with_capacity(planes.len());
    for plane in planes {
        let normal = plane.normal.to_array();
        let along = normal[axis];
        // The slab end that leaves the least room on this plane's free side.
        let base = if along >= 0.0 { along * low } else { along * high };
        let offset = plane.offset - base;
        let flat = DVec2::new(normal[u], normal[v]);
        if flat.length() < SECTION_NORMAL {
            // The plane is level with the cut: it holds over the whole cut or not at all.
            if base < plane.offset - COORDINATE_TOLERANCE {
                return None;
            }
            continue;
        }
        lines.push((flat, offset));
    }
    let mut points: Vec<DVec2> = Vec::new();
    for (index, first) in lines.iter().enumerate() {
        for second in lines.iter().skip(index.saturating_add(1)) {
            let determinant = first.0.x * second.0.y - first.0.y * second.0.x;
            if !determinant.is_finite() || determinant.abs() < SECTION_ANGLE {
                continue;
            }
            let point = DVec2::new(
                (first.1 * second.0.y - second.1 * first.0.y) / determinant,
                (first.0.x * second.1 - second.0.x * first.1) / determinant,
            );
            if !point.is_finite() {
                continue;
            }
            if lines.iter().any(|(normal, offset)| normal.dot(point) - offset < -COORDINATE_TOLERANCE) {
                continue;
            }
            if points.iter().any(|seen| (*seen - point).length() <= COORDINATE_TOLERANCE) {
                continue;
            }
            points.push(point);
        }
    }
    if points.len() < 3 || points.len() > MAX_CELL_CORNERS {
        return None;
    }
    order_ring(&mut points)?;
    Some(points)
}

/// Puts a convex polygon's corners in counter-clockwise order around their own centroid.
fn order_ring(points: &mut [DVec2]) -> Option<()> {
    let middle = centre(points)?;
    points.sort_by(|first, second| {
        let left = (first.y - middle.y).atan2(first.x - middle.x);
        let right = (second.y - middle.y).atan2(second.x - middle.x);
        left.total_cmp(&right).then((*first - middle).length_squared().total_cmp(&(*second - middle).length_squared()))
    });
    (signed_area(points) > SECTION_TOLERANCE).then_some(())
}

/// The signed area of a polygon; positive when its corners run counter-clockwise.
fn signed_area(points: &[DVec2]) -> f64 {
    let mut total = 0.0;
    for index in 0..points.len() {
        let (Some(start), Some(end)) = (points.get(index), points.get((index + 1) % points.len())) else {
            continue;
        };
        total += start.x * end.y - end.x * start.y;
    }
    total * 0.5
}

/// The average of a polygon's corners, or `None` when there are none or it is not finite.
fn centre(points: &[DVec2]) -> Option<DVec2> {
    if points.is_empty() {
        return None;
    }
    let mut sum = DVec2::ZERO;
    for point in points {
        sum += *point;
    }
    let middle = sum / points.len() as f64;
    middle.is_finite().then_some(middle)
}

/// The average of a set of points, or `None` when there are none or it is not finite.
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

/// The polygon as the document ring a panel stores as its outline.
///
/// The polygon is measured as a geometry region first — a degenerate or self-crossing section is
/// refused there — and the document ring is built from the same corners, rounded to the micrometre.
fn ring(points: Vec<DVec2>) -> Option<Contour> {
    Region::from_loops(points.clone(), Vec::new()).ok()?;
    let points = points.into_iter().map(document_point).collect::<Option<Vec<_>>>()?;
    Contour::from_polygon(points).ok()
}

/// One point of a section as the document's micrometres.
fn document_point(point: DVec2) -> Option<[Length; 2]> {
    Some([Length::from_micrometres(micrometres(point.x)?), Length::from_micrometres(micrometres(point.y)?)])
}

/// A millimetre measurement as the document's whole micrometres, or `None` when it is not a number.
fn micrometres(value: f64) -> Option<i64> {
    let rounded = (value * 1000.0).round();
    if !rounded.is_finite() || rounded.abs() > i64::MAX as f64 {
        return None;
    }
    Some(rounded as i64)
}

/// Whether `point` lies inside the box, allowing the micrometre the document stores.
fn within_box(point: DVec3, min: DVec3, max: DVec3) -> bool {
    (0..3)
        .all(|axis| point[axis] >= min[axis] - COORDINATE_TOLERANCE && point[axis] <= max[axis] + COORDINATE_TOLERANCE)
}

/// The order corners are listed in, so the same cell always reports the same list.
fn corner_order(first: &DVec3, second: &DVec3) -> std::cmp::Ordering {
    first.x.total_cmp(&second.x).then(first.y.total_cmp(&second.y)).then(first.z.total_cmp(&second.z))
}

/// A point rounded to the micrometre the document stores.
fn quantised_point(point: DVec2) -> DVec2 {
    DVec2::new(quantised(point.x), quantised(point.y))
}

/// A cell corner rounded to the micrometre the document stores.
fn quantised_corner(point: DVec3) -> DVec3 {
    DVec3::new(quantised(point.x), quantised(point.y), quantised(point.z))
}

/// Rounds a measurement to the micrometre the document stores.
fn quantised(value: f64) -> f64 {
    match micrometres(value) {
        Some(whole) => whole as f64 / 1000.0,
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::{DQuat, DVec3};
    use furnicraft_units::Degrees;

    use crate::fixtures::*;
    use crate::*;

    /// A product of `size` millimetres with its base-min corner at `at` in the root frame.
    fn product(project: &mut Project, size: (i64, i64, i64), at: (i64, i64, i64)) -> Id {
        let root = project.root().id();
        let (width, depth, height) = size;
        let envelope =
            Envelope::new(millimetres(width), millimetres(depth), millimetres(height)).expect("a valid envelope");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(at.0), millimetres(at.1), millimetres(at.2))
            .expect("a placement");
        project.add_product(root, "Piece", ClassCode::GENERAL_PRODUCT, envelope, placement).expect("add product")
    }

    /// A panel of `length x width x thickness` millimetres inside `parent`, based at `at`.
    fn panel(project: &mut Project, parent: Id, at: (i64, i64, i64), size: (i64, i64, i64)) -> Id {
        let nominal = PanelNominalSize::new(millimetres(size.0), millimetres(size.1)).expect("a nominal size");
        let placement = Placement::IDENTITY
            .with_translation(millimetres(at.0), millimetres(at.1), millimetres(at.2))
            .expect("a placement");
        project.add_panel(parent, "Part", nominal, None, millimetres(size.2), placement).expect("add panel")
    }

    /// A board of `length x width x thickness` millimetres, turned by `rotation` and based at `at`.
    fn board(project: &mut Project, parent: Id, at: DVec3, rotation: DQuat, size: (i64, i64, i64)) -> Id {
        let nominal = PanelNominalSize::new(millimetres(size.0), millimetres(size.1)).expect("a board size");
        let placement = Placement::new(at, rotation).expect("a placement");
        project.add_panel(parent, "Board", nominal, None, millimetres(size.2), placement).expect("add a board")
    }

    /// A standing post whose inner face runs through `through`, angled so that face's normal
    /// pointing into the free space is `normal` (both in the product's XY plane, millimetres).
    ///
    /// The post is 720 mm tall, 18 mm thick and wide enough to cross the whole depth. It stands in
    /// the post's stance (ADR 0105): the panel's local X runs up the height, its local Y along the
    /// depth, and its local Z through the thickness away from the free space, so a quarter-turn about
    /// Y stands it up and a rotation about Z splays it. The face the free space touches is the
    /// board's own `z = 0` face and the node's base corner sits on the void's floor.
    fn angled_post(project: &mut Project, parent: Id, through: (f64, f64), normal: (f64, f64)) -> Id {
        let direction = (-normal.1, normal.0);
        // The panel is 1200 mm wide, so the start of its local Y is placed to centre that span on
        // the case's depth instead of hanging off one side.
        let start_y = 280.0 - 600.0 * direction.1;
        let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
        let angle = normal.1.atan2(normal.0);
        let rotation = DQuat::from_rotation_z(angle) * DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2);
        let translation = DVec3::new(start_x, start_y, 0.0);
        let placement = Placement::new(translation, rotation).expect("a placement");
        let nominal = PanelNominalSize::new(millimetres(720), millimetres(1200)).expect("a post size");
        project.add_panel(parent, "Post", nominal, None, millimetres(18), placement).expect("add a post")
    }

    /// A board standing up and leaning `lean` radians forward, its face on the piece's back edge.
    ///
    /// Its length climbs the leaned face, its thickness runs away from the void and its width runs
    /// back along the product's X, so the free space touches the board's own `z = 0` face.
    fn leaned_back_wall(project: &mut Project, parent: Id) -> Id {
        let lean = 15.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        let up = DVec3::new(0.0, -sin, cos);
        let away = DVec3::new(0.0, cos, sin);
        let rotation = DQuat::from_rotation_axes(up.cross(away), up, away);
        board(project, parent, DVec3::new(600.0, 560.0, 0.0), rotation, (600, 900, 18))
    }

    /// A board lying over the void and coming down towards the back: its underside is the void's
    /// ceiling, 699.5 mm above the floor at the front and 98 mm lower at the back.
    fn leaned_ceiling(project: &mut Project, parent: Id) -> Id {
        let lean = 10.0_f64.to_radians();
        let (sin, cos) = lean.sin_cos();
        let along = DVec3::new(0.0, cos, -sin);
        let away = DVec3::new(0.0, sin, cos);
        let rotation = DQuat::from_rotation_axes(along.cross(away), along, away);
        board(project, parent, DVec3::new(0.0, 0.0, 699.5), rotation, (600, 620, 18))
    }

    /// An empty document with a root and nothing else, so a product's own box is the only surface.
    fn clean_project() -> Project {
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        Project::new(root).expect("an empty project")
    }

    /// An empty product of `600 x 560 x 720` millimetres at the origin.
    fn empty_piece() -> (Project, Id) {
        let mut project = clean_project();
        let id = product(&mut project, (600, 560, 720), (0, 0, 0));
        (project, id)
    }

    /// Whether a point is inside every supporting plane of the niche.
    fn fits(niche: &Niche, point: DVec3) -> bool {
        niche.planes.iter().all(|plane| plane.distance_to(point) >= -super::COORDINATE_TOLERANCE)
    }

    #[test]
    fn a_probe_stops_at_the_envelope_virtual_face() {
        let (project, piece) = empty_piece();
        let point = DVec3::new(300.0, 280.0, 300.0);
        let answer = project.probe(point, DVec3::Z).expect("the envelope stops the ray");
        assert_eq!(answer.distance_mm, 420.0, "the roof is 720 - 300 away");
        assert_eq!(answer.hit, ProbeHit { id: piece, kind: SurfaceKind::Envelope });
        assert!((answer.face.normal - -DVec3::Z).length() < 1.0e-9, "the face looks back down the ray");
        assert!((answer.face.point - DVec3::new(300.0, 280.0, 720.0)).length() < 1.0e-9);
        let down = project.probe(point, -DVec3::Z).expect("the floor is an envelope face too");
        assert_eq!(down.distance_mm, 300.0);
        assert_eq!(down.hit.kind, SurfaceKind::Envelope);
    }

    #[test]
    fn a_probe_stops_at_a_panel_and_reports_it() {
        let (mut project, piece) = empty_piece();
        let partition = panel(&mut project, piece, (300, 0, 0), (18, 560, 720));
        let answer = project.probe(DVec3::new(100.0, 280.0, 300.0), DVec3::X).expect("the panel stops the ray");
        assert_eq!(answer.distance_mm, 200.0);
        assert_eq!(answer.hit, ProbeHit { id: partition, kind: SurfaceKind::Panel });
    }

    #[test]
    fn a_probe_stops_at_another_product_shell_and_at_a_body() {
        let mut project = clean_project();
        let outer = product(&mut project, (2000, 1000, 1000), (0, 0, 0));
        let inner = product(&mut project, (300, 300, 300), (1000, 400, 400));
        let answer = project.probe(DVec3::new(500.0, 500.0, 500.0), DVec3::X).expect("the inner shell stops the ray");
        assert_eq!(answer.distance_mm, 500.0);
        assert_eq!(answer.hit, ProbeHit { id: inner, kind: SurfaceKind::Shell });
        assert_ne!(answer.hit.id, outer, "the shell of the piece the point sits in is not reported as a hit");

        // The same ray meets a geometry body before the shell.
        let root = project.root().id();
        let object = project
            .add_object(
                root,
                "Block",
                Placement::IDENTITY
                    .with_translation(millimetres(600), millimetres(495), millimetres(495))
                    .expect("a placement"),
                sample_object(90).geometry().clone(),
            )
            .expect("add object");
        let answer = project.probe(DVec3::new(500.0, 500.0, 500.0), DVec3::X).expect("the body stops the ray");
        assert_eq!(answer.distance_mm, 100.0);
        assert_eq!(answer.hit, ProbeHit { id: object, kind: SurfaceKind::Body });
    }

    #[test]
    fn a_niche_shrinks_around_a_part_on_the_probed_axis() {
        let (mut project, piece) = empty_piece();
        // A sheet from x = 300 to x = 318 stands across the piece.
        panel(&mut project, piece, (300, 0, 0), (18, 560, 720));
        let niche = project.niche(DVec3::new(450.0, 280.0, 300.0)).expect("a niche to the right of the sheet");
        assert_eq!(niche.product, piece);
        assert_eq!(niche.local_min, DVec3::new(318.0, 0.0, 0.0), "the sheet is the near wall");
        assert_eq!(niche.size, DVec3::new(282.0, 560.0, 720.0));
        assert_eq!(niche.local_max(), DVec3::new(600.0, 560.0, 720.0));
        assert_eq!(niche.shape, NicheShape::Box);

        let niche = project.niche(DVec3::new(150.0, 280.0, 300.0)).expect("a niche to the left of the sheet");
        assert_eq!(niche.local_min, DVec3::ZERO);
        assert_eq!(niche.size, DVec3::new(300.0, 560.0, 720.0), "it ends at the sheet's near face");
    }

    #[test]
    fn a_shelf_splits_the_piece_into_two_niches() {
        let (mut project, piece) = empty_piece();
        // A full-width shelf 18 mm thick, its underside at z = 350.
        panel(&mut project, piece, (0, 0, 350), (600, 560, 18));
        let below = project.niche(DVec3::new(300.0, 280.0, 100.0)).expect("the niche under the shelf");
        assert_eq!(below.local_min, DVec3::ZERO);
        assert_eq!(below.size, DVec3::new(600.0, 560.0, 350.0));
        assert_eq!(below.corners.len(), 8, "a box has eight corners");
        let above = project.niche(DVec3::new(300.0, 280.0, 500.0)).expect("the niche above the shelf");
        assert_eq!(above.local_min, DVec3::new(0.0, 0.0, 368.0), "the shelf's top face");
        assert_eq!(above.size, DVec3::new(600.0, 560.0, 352.0));
    }

    #[test]
    fn a_niche_reports_a_world_corner_and_a_shape() {
        let mut project = clean_project();
        product(&mut project, (600, 560, 720), (100, 200, 0));
        let niche = project.niche(DVec3::new(400.0, 480.0, 300.0)).expect("a niche");
        assert_eq!(niche.local_min, DVec3::ZERO, "the free box starts at the piece's own corner");
        assert_eq!(niche.min, DVec3::new(100.0, 200.0, 0.0), "and it reports that corner in the world");
        assert_eq!(niche.shape, NicheShape::Box);
        assert_eq!(niche.size, DVec3::new(600.0, 560.0, 720.0));
        assert_eq!(niche.side_mm(2), 720.0);
        assert_eq!(niche.planes.len(), 6, "the envelope's six planes bound an empty piece");
        assert_eq!(niche.corners.len(), 8);
    }

    #[test]
    fn two_angled_posts_leave_a_trapezoid_section() {
        // The inner faces run from (100, 0) to (200, 560) and from (500, 0) to (400, 560): the free
        // space between them narrows towards the back, which is a trapezoid and not a box.
        let (mut project, piece) = empty_piece();
        let slant = 100.0 / 560.0_f64.hypot(100.0);
        let drop = 560.0 / 560.0_f64.hypot(100.0);
        angular_pair(&mut project, piece, (drop, -slant), (-drop, -slant));
        let niche = project.niche(DVec3::new(300.0, 280.0, 300.0)).expect("a niche between the posts");
        let ring = niche.section().expect("the section is a polygon, not the box around it");
        let corners: Vec<[Length; 2]> = ring.sampled_points(1.0).expect("the ring is line only");
        let expected = [
            [millimetres(100), millimetres(0)],
            [millimetres(500), millimetres(0)],
            [millimetres(400), millimetres(560)],
            [millimetres(200), millimetres(560)],
        ];
        assert_eq!(corners, expected, "the trapezoid's own vertices, counter-clockwise from its corner");
        assert_eq!(niche.local_min, DVec3::new(100.0, 0.0, 0.0));
        assert_eq!(niche.size, DVec3::new(400.0, 560.0, 720.0), "the box around the section");
        assert_eq!(niche.corners.len(), 8, "two upright walls keep the cell a prism");
    }

    /// Places a mirrored pair of angled posts whose inner faces pass through (100, 0) and (500, 0).
    fn angular_pair(project: &mut Project, piece: Id, left_normal: (f64, f64), right_normal: (f64, f64)) {
        angled_post(project, piece, (100.0, 0.0), left_normal);
        angled_post(project, piece, (500.0, 0.0), right_normal);
    }

    #[test]
    fn a_leaned_back_wall_bounds_a_sloped_cell() {
        let (mut project, piece) = empty_piece();
        leaned_back_wall(&mut project, piece);
        let niche = project.niche(DVec3::new(300.0, 200.0, 300.0)).expect("a cell in front of a leaned wall");
        assert_eq!(niche.shape, NicheShape::Sloped, "one ring would be false somewhere in the void");
        assert!(niche.section().is_none());
        assert_eq!(niche.local_min, DVec3::ZERO);
        assert_eq!(niche.size, DVec3::new(600.0, 560.0, 720.0), "the box around the cell");
        // The wall runs from (y = 560, z = 0) to (y = 367.2, z = 720): the top of the void is
        // 192.8 mm shallower than its floor.
        let top = niche.corners.iter().copied().filter(|corner| corner.z > 700.0);
        let deepest = top.map(|corner| corner.y).fold(f64::NEG_INFINITY, f64::max);
        assert!((deepest - 367.2).abs() < 1.0, "the cell's back edge at the top is {deepest}");
        for corner in &niche.corners {
            assert!(fits(&niche, *corner), "a corner left the cell: {corner:?}");
        }
    }

    #[test]
    fn a_shelf_under_a_leaned_wall_is_cut_where_the_wall_leans_in() {
        let (mut project, piece) = empty_piece();
        leaned_back_wall(&mut project, piece);
        let niche = project.niche(DVec3::new(300.0, 200.0, 300.0)).expect("a cell");
        // The wall leans 0.268 mm into the void per millimetre of height, so a shelf stops on the
        // wall's face at its own top instead of cutting into it.
        let low = niche.cut(2, (100.0, 118.0), (0, 1)).expect("a low shelf's footprint");
        let deepest = low.iter().map(|point| point.y).fold(f64::MIN, f64::max);
        assert!((deepest - 528.4).abs() < 1.0, "the low shelf ends on the wall at 118 mm: {deepest}");
        // The same shelf 500 mm higher is 134 mm shallower, which is the wall's own rake.
        let high = niche.cut(2, (600.0, 618.0), (0, 1)).expect("a high shelf's footprint");
        let deepest = high.iter().map(|point| point.y).fold(f64::MIN, f64::max);
        assert!((deepest - 394.4).abs() < 1.0, "the shelf ends on the wall at 618 mm: {deepest}");
        assert!(high.len() == 4, "the cut is still one rectangle: {high:?}");
    }

    #[test]
    fn a_leaned_ceiling_is_measured_over_its_whole_footprint() {
        let (mut project, piece) = empty_piece();
        leaned_ceiling(&mut project, piece);
        // The cursor's own height means nothing: the same cell answers from the front and the back.
        let front = project.niche(DVec3::new(300.0, 20.0, 300.0)).expect("a cell under the ceiling");
        let back = project.niche(DVec3::new(300.0, 540.0, 300.0)).expect("the same cell, further back");
        assert_eq!(front.shape, NicheShape::Sloped);
        assert_eq!(front.size, back.size, "the box is the void's, not the cursor's");
        assert!((front.size.z - 699.5).abs() < 0.5, "the highest point of the ceiling: {}", front.size.z);
        let shallow = |niche: &Niche| {
            niche
                .corners
                .iter()
                .copied()
                .filter(|corner| corner.y > 500.0)
                .map(|corner| corner.z)
                .fold(f64::MIN, f64::max)
        };
        assert!((shallow(&front) - 601.4).abs() < 1.0, "the ceiling is 98 mm lower at the back: {}", shallow(&front));
    }

    #[test]
    fn a_shelf_under_a_leaned_ceiling_stops_where_it_would_cut_in() {
        let (mut project, piece) = empty_piece();
        leaned_ceiling(&mut project, piece);
        let niche = project.niche(DVec3::new(300.0, 540.0, 300.0)).expect("a cell");
        let low = niche.cut(2, (100.0, 118.0), (0, 1)).expect("a low shelf's footprint");
        let deepest = low.iter().map(|point| point.y).fold(f64::MIN, f64::max);
        assert!((deepest - 560.0).abs() < 1.0, "a low shelf reaches the back: {deepest}");
        let high = niche.cut(2, (590.0, 608.0), (0, 1)).expect("a high shelf's footprint");
        let deepest = high.iter().map(|point| point.y).fold(f64::MIN, f64::max);
        assert!((deepest - 519.0).abs() < 1.5, "a shelf at 608 mm stops under the ceiling: {deepest}");
    }

    #[test]
    fn a_turned_piece_is_measured_in_its_own_frame() {
        let mut project = clean_project();
        let piece = product(&mut project, (600, 560, 720), (0, 0, 0));
        let turn = Placement::IDENTITY
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(45_000), ..EulerAngles::default() })
            .expect("a turn");
        project.set_node_placement(piece, turn).expect("turn the piece");
        let inside = turn.transform_point(DVec3::new(300.0, 280.0, 300.0));
        let niche = project.niche(inside).expect("a turned piece has a niche of its own");
        assert_eq!(niche.local_min, DVec3::ZERO, "measured in the piece's frame, not the world box");
        assert_eq!(niche.size, DVec3::new(600.0, 560.0, 720.0));
        assert_eq!(niche.shape, NicheShape::Box);
        assert_eq!(niche.min, DVec3::ZERO, "and the world corner is the piece's own zero corner");
    }

    #[test]
    fn a_body_standing_inside_the_cell_is_refused() {
        // A 90 mm block sits between the probe directions of a point in an otherwise empty piece: the
        // cell would swallow it, and a board cut to the cell would cut into it.
        let (mut project, _piece) = empty_piece();
        let root = project.root().id();
        project
            .add_object(
                root,
                "Block",
                Placement::IDENTITY
                    .with_translation(millimetres(500), millimetres(450), millimetres(200))
                    .expect("a placement"),
                sample_object(90).geometry().clone(),
            )
            .expect("add object");
        let point = DVec3::new(300.0, 280.0, 300.0);
        assert!(project.probe(point, DVec3::X).is_some(), "the piece itself still answers a probe");
        assert_eq!(project.niche(point), Err(NicheRefusal::Unrepresentable), "a void holding a body is not a cell");
    }

    #[test]
    fn a_wall_that_stops_halfway_is_refused() {
        // A post that reaches only 300 mm up does not bound one face of a cell, so a point beside it
        // has no niche the tool could cut a shelf to.
        let (mut project, piece) = empty_piece();
        let nominal = PanelNominalSize::new(millimetres(300), millimetres(560)).expect("a short post");
        let rotation = furnicraft_geom::DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2);
        let placement = Placement::new(DVec3::new(318.0, 0.0, 0.0), rotation).expect("a place");
        project.add_panel(piece, "Stub", nominal, None, millimetres(18), placement).expect("add a stub");
        let refusal = project.niche(DVec3::new(150.0, 280.0, 200.0)).expect_err("half a wall is not a wall");
        assert_eq!(refusal, NicheRefusal::Unrepresentable);
    }

    #[test]
    fn a_point_outside_every_product_has_no_niche() {
        let (project, _) = empty_piece();
        assert_eq!(project.niche(DVec3::new(-100.0, 0.0, 0.0)), Err(NicheRefusal::Outside));
        assert_eq!(project.niche(DVec3::splat(f64::NAN)), Err(NicheRefusal::Outside));
    }

    #[test]
    fn a_cut_needs_three_axes_and_a_finite_slab() {
        let (project, _) = empty_piece();
        let niche = project.niche(DVec3::new(300.0, 280.0, 300.0)).expect("a niche");
        assert!(niche.cut(2, (100.0, 118.0), (0, 1)).is_some());
        assert_eq!(niche.cut(2, (100.0, 118.0), (2, 1)), None, "the cut cannot be seen along its own axis");
        assert_eq!(niche.cut(3, (100.0, 118.0), (0, 1)), None);
        assert_eq!(niche.cut(2, (f64::NAN, 0.0), (0, 1)), None);
        assert_eq!(niche.cut(2, (800.0, 820.0), (0, 1)), None, "a slab above the void has no footprint");
    }

    #[test]
    fn a_probe_needs_a_direction_and_finite_numbers() {
        let (project, _) = empty_piece();
        let point = DVec3::new(300.0, 280.0, 300.0);
        assert_eq!(project.probe(point, DVec3::ZERO), None);
        assert_eq!(project.probe(DVec3::splat(f64::INFINITY), DVec3::X), None);
        assert_eq!(project.probe(point, DVec3::splat(f64::NAN)), None);
    }
}
