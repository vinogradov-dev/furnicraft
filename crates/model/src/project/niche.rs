//! The niche probe: how far the free space around a point reaches, and what shape it has.
//!
//! A niche is the space a piece of furniture reserves (ADR 0099); it is derived from the document
//! every time it is asked for and never stored (ADR 0102, ADR 0103). Two questions answer it:
//!
//! - [`Project::probe`] casts one ray from a point and reports the face it stops at — its distance,
//!   its plane and the furniture entity that owns it: a panel face, a geometry body, another
//!   product's shell, or a virtual face of the envelope of the piece the point sits in;
//! - [`Project::niche`] casts six rays along a product's own axes, builds the cross-section of the
//!   free space in the product's own XY plane, and extrudes it along the product's height. A
//!   rectangular case gives the rectangle; two posts standing at an angle give a trapezoid, which a
//!   shelf can take as its outline (ADR 0099).
//!
//! Both are pure queries: no session, no renderer, no document write. The surfaces are the same
//! boxes the furniture move rule measures against (`super::surface`), so the tool that fills a
//! niche and the tool that drags a piece cannot disagree about where a surface is.
//!
//! A product turned off the world axes has no world-box answer, so [`Project::niche`] refuses it
//! instead of returning a loose box. A free section that is not one straight extruded polygon —
//! a wall that stops halfway, a shape with a notch — is refused as well, with a reason the tool can
//! report rather than approximate.

use furnicraft_geom::{DVec2, DVec3, Region};

use super::surface::{FaceHit, SurfaceBox, SurfaceKind, surface_box};
use crate::tree::object_bounds_mm;
use crate::*;

/// Tolerance for deciding that a product's axes lie on the world axes.
const AXIS_ALIGNMENT: f64 = 1.0e-9;

/// Tolerance for a measurement that is already a whole micrometre, in millimetres.
const SECTION_TOLERANCE: f64 = 1.0e-6;

/// Smallest share of a face normal that must lie in the section plane for it to bound a wall.
///
/// A face almost level with the plane answers a line far outside the void, so it is refused rather
/// than allowed to produce a corner at a huge coordinate.
const SECTION_NORMAL: f64 = 1.0e-3;

/// Smallest sine of the angle between two section walls that still meets in a corner.
const SECTION_ANGLE: f64 = 1.0e-6;

/// How far inside the void a wall's coverage is tested, in millimetres.
const COVER_INSET: f64 = 1.0e-3;

/// Tolerance for comparing a section corner with a face, in millimetres.
///
/// The probes report whole micrometres, so a corner where two walls meet can land a micrometre
/// outside the face that owns one of them: that is rounding, not a gap.
const COORDINATE_TOLERANCE: f64 = 2.0e-3;

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
    /// lets a section be built from probed faces.
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

/// The shape of a niche.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NicheShape {
    /// The free section is the rectangle the four side probes enclose.
    Box,
    /// The free section is a polygon, stored as a ring in the product's own XY plane.
    ///
    /// The ring is the outline a part lying in that plane takes: a shelf in a case whose posts stand
    /// at an angle is a trapezoid, not the rectangle around it (ADR 0099). Its coordinates are the
    /// product frame's, in the document's micrometres.
    Section(Contour),
}

/// Why a point has no usable niche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NicheRefusal {
    /// The point is outside every product, or is not a usable number.
    Outside,
    /// The product is turned off the world axes, so its own box is not a world box.
    Turned,
    /// The free space around the point is not one straight extruded polygon.
    Unrepresentable,
}

/// The empty space around a point inside a product.
///
/// Everything here is derived: the query stores nothing in the document. `local_min` and `size` are
/// the box the free space occupies, in the product's own frame, which is the frame a panel is placed
/// in; `min` is the same corner in world millimetres, so a tool never re-derives a position from a
/// cursor and a size. `shape` carries the exact section when it is not that box.
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

    /// The free section's polygon, when the niche is not a plain box.
    ///
    /// The ring is in the product's own XY plane, the plane a shelf's outline lies in.
    pub fn section(&self) -> Option<&Contour> {
        match &self.shape {
            NicheShape::Section(ring) => Some(ring),
            NicheShape::Box => None,
        }
    }
}

/// One wall of a niche section: a line in the product's own plane and the surface that owns it.
#[derive(Clone, Copy, Debug)]
struct Wall {
    /// A point on the line, in the product frame's millimetres.
    point: DVec2,
    /// The line's unit normal, pointing into the free space.
    normal: DVec2,
    /// The surface the probe met, for the extent checks.
    surface: SurfaceBox,
    /// The surface's own axis the face lies on.
    axis: usize,
}

impl Wall {
    /// The signed distance from a point in the product plane to this wall, positive on the free side.
    fn distance_to(self, point: DVec2) -> f64 {
        self.normal.dot(point - self.point)
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
        let (unit, surface, face) = self.first_face(point, direction)?;
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

    /// The empty space around `point`, as one extruded polygon.
    ///
    /// The product is the innermost envelope that contains the point, and the section is taken in
    /// that product's own XY plane: four rays along its own axes stop at the walls, the wall planes
    /// bound the polygon, and the void's floor and ceiling come from the two vertical probes. A
    /// rectangular case answers [`NicheShape::Box`]; a slanted wall answers the polygon it forms.
    /// A point outside every product, a turned product and a section that is not one straight
    /// polygon are refused rather than approximated (ADR 0099).
    pub fn niche(&self, point: DVec3) -> Result<Niche, NicheRefusal> {
        if !point.is_finite() {
            return Err(NicheRefusal::Outside);
        }
        let (product, _) = self.product_containing(point).ok_or(NicheRefusal::Outside)?;
        let placement = self.world_placement(product).ok_or(NicheRefusal::Outside)?;
        if !axis_aligned(placement) {
            return Err(NicheRefusal::Turned);
        }
        let inverse = placement.inverse().map_err(|_| NicheRefusal::Outside)?;
        let local = inverse.transform_point(point);
        if !local.is_finite() {
            return Err(NicheRefusal::Outside);
        }

        // The four walls of the section, in probe order: +X, -X, +Y, -Y.
        let mut walls = [None; 4];
        for (index, direction) in [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y].into_iter().enumerate() {
            walls[index] = self.wall(point, local, placement, inverse, direction);
        }
        let [Some(right), Some(left), Some(back), Some(front)] = walls else {
            return Err(NicheRefusal::Unrepresentable);
        };
        let downward =
            self.probe(point, -placement.transform_vector(DVec3::Z)).ok_or(NicheRefusal::Unrepresentable)?.distance_mm;
        let upward =
            self.probe(point, placement.transform_vector(DVec3::Z)).ok_or(NicheRefusal::Unrepresentable)?.distance_mm;
        // The cursor rarely sits on a whole micrometre inside the product frame, so the two ends are
        // rounded once here: the void's floor and ceiling are surfaces, not the point's arithmetic.
        let floor = quantised(local.z - downward);
        let ceiling = quantised(local.z + upward);
        if !(floor.is_finite() && ceiling.is_finite()) || ceiling - floor <= SECTION_TOLERANCE {
            return Err(NicheRefusal::Unrepresentable);
        }
        let corners = self
            .section(local, placement, [right, left, back, front], floor, ceiling)
            .ok_or(NicheRefusal::Unrepresentable)?;

        let (mut low, mut high) = (corners[0], corners[0]);
        for corner in corners {
            low = low.min(corner);
            high = high.max(corner);
        }
        let local_min = DVec3::new(low.x, low.y, floor);
        let size = DVec3::new(high.x - low.x, high.y - low.y, ceiling - floor);
        if !(size.is_finite() && size.min_element() > 0.0) {
            return Err(NicheRefusal::Unrepresentable);
        }
        let world_min = placement.transform_point(local_min);
        if !world_min.is_finite() {
            return Err(NicheRefusal::Unrepresentable);
        }
        let boxed = [right, left, back, front];
        let shape = if is_box(corners, local, boxed) {
            NicheShape::Box
        } else {
            NicheShape::Section(ring(corners).ok_or(NicheRefusal::Unrepresentable)?)
        };
        Ok(Niche { product, local_min, size, min: world_min, shape })
    }

    /// Builds the section polygon from the four walls, or `None` when they do not bound one.
    ///
    /// The corners are the pairwise intersections in the order the walls face each other, so a
    /// rectangular void gives its four corners and a splayed one gives the trapezoid. The polygon is
    /// accepted only when every corner is on the free side of every wall, every edge lies inside the
    /// face that stopped its probe, and each wall's plane is still a surface at the void's floor and
    /// ceiling: that is what makes the free space one extruded shape and not an approximation of a
    /// more complicated void.
    fn section(
        &self,
        local: DVec3,
        placement: Placement,
        walls: [Wall; 4],
        floor: f64,
        ceiling: f64,
    ) -> Option<[DVec2; 4]> {
        let [right, left, back, front] = walls;
        let mut corners = [
            intersection(right, back)?,
            intersection(back, left)?,
            intersection(left, front)?,
            intersection(front, right)?,
        ];
        for corner in corners {
            if !corner.is_finite() || walls.iter().any(|wall| wall.distance_to(corner) < -COORDINATE_TOLERANCE) {
                return None;
            }
        }
        let ground = DVec2::new(local.x, local.y);
        // The point may sit on a wall: the tool's cursor rests on the envelope's near face. It may
        // not sit behind one, which would mean the probes and the polygon disagree.
        if walls.iter().any(|wall| wall.distance_to(ground) < -COORDINATE_TOLERANCE) {
            return None;
        }
        if signed_area(corners) <= SECTION_TOLERANCE {
            return None;
        }
        // The edge from one corner to the next lies on the wall they both belong to: `back`, `left`,
        // `front` and `right` in that order.
        let edges = [(2usize, 0usize, 1usize), (1, 1, 2), (3, 2, 3), (0, 3, 0)];
        for (wall_index, from, to) in edges {
            let wall = walls.get(wall_index).copied()?;
            let start = *corners.get(from)?;
            let end = *corners.get(to)?;
            for corner in [start, end] {
                if !within_face(wall, placement, corner, local.z) {
                    return None;
                }
            }
            let midpoint = (start + end) * 0.5;
            if !self.covered(wall, placement, midpoint, floor + COVER_INSET)
                || !self.covered(wall, placement, midpoint, ceiling - COVER_INSET)
            {
                return None;
            }
        }
        for corner in &mut corners {
            *corner = quantised_point(*corner);
        }
        Some(corners)
    }

    /// Whether a wall's plane still holds a surface at `height`, at the section point `at`.
    ///
    /// A board that stops halfway leaves the plane behind it empty, so the free space is not one
    /// extruded shape. A second surface on the same plane — the envelope a panel is flush with —
    /// keeps it a wall, which is why the plane and not the surface is tested.
    fn covered(&self, wall: Wall, placement: Placement, at: DVec2, height: f64) -> bool {
        // The ray starts just inside the free space and looks at the wall, because a ray that starts
        // on a face and points into the solid measures the far face, not the near one.
        let point = placement.transform_point(DVec3::new(
            at.x + wall.normal.x * COVER_INSET,
            at.y + wall.normal.y * COVER_INSET,
            height,
        ));
        let outward = -placement.transform_vector(DVec3::new(wall.normal.x, wall.normal.y, 0.0));
        self.probe(point, outward).is_some_and(|probe| probe.distance_mm <= COVER_INSET * 1.5)
    }

    /// One wall of the section: where the ray along a product axis stops, as a line in the plane.
    ///
    /// The face plane is cut with the section plane through the measured point, so a face that is
    /// itself tilted still answers the line the free space is bounded by.
    fn wall(
        &self,
        point: DVec3,
        local: DVec3,
        placement: Placement,
        inverse: Placement,
        direction: DVec3,
    ) -> Option<Wall> {
        let (unit, surface, face) = self.first_face(point, placement.transform_vector(direction))?;
        let hit = inverse.transform_point(point + unit * quantised(face.distance));
        let normal = inverse.transform_vector(face.normal);
        if !hit.is_finite() || !normal.is_finite() {
            return None;
        }
        let flat = DVec2::new(normal.x, normal.y);
        let length = flat.length();
        if !length.is_finite() || length < SECTION_NORMAL {
            // A face that is level with the section plane bounds no wall of it.
            return None;
        }
        let unit = flat / length;
        let ground = DVec2::new(local.x, local.y);
        let on_face = DVec2::new(hit.x, hit.y);
        // The face plane cut with the section plane through the measured point, written as a line
        // with a unit normal: `unit · (XY - on_face) = offset`. A tilted face still answers a line.
        let offset = -normal.z * (local.z - hit.z) / length;
        let point = ground - unit * (unit.dot(ground - on_face) - offset);
        if !point.is_finite() {
            return None;
        }
        Some(Wall { point, normal: unit, surface, axis: face.axis })
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
    fn surfaces_around(&self, point: DVec3) -> (Option<(Id, SurfaceBox)>, Vec<SurfaceBox>) {
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
        (own, surfaces)
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
    fn first_face(&self, point: DVec3, direction: DVec3) -> Option<(DVec3, SurfaceBox, FaceHit)> {
        if !point.is_finite() || !direction.is_finite() {
            return None;
        }
        let length = direction.length();
        if !length.is_finite() || length <= f64::EPSILON {
            return None;
        }
        let unit = direction / length;
        let (own, surfaces) = self.surfaces_around(point);
        let mut best: Option<(SurfaceBox, FaceHit)> = None;
        if let Some((_, envelope)) = own
            && let Some(face) = envelope.face_hit(point, unit)
        {
            let envelope = SurfaceBox { kind: SurfaceKind::Envelope, ..envelope };
            best = Some((envelope, face));
        }
        for surface in surfaces {
            if own.is_some_and(|(product, _)| product == surface.id) {
                continue;
            }
            let Some(face) = surface.face_hit(point, unit) else {
                continue;
            };
            if best.is_none_or(|(_, nearest)| face.distance < nearest.distance) {
                best = Some((surface, face));
            }
        }
        best.map(|(surface, face)| (unit, surface, face))
    }
}

/// Whether every corner of `corners` is the box corner the four probe distances describe.
fn is_box(corners: [DVec2; 4], local: DVec3, walls: [Wall; 4]) -> bool {
    let ground = DVec2::new(local.x, local.y);
    let [right, left, back, front] = walls;
    let low = DVec2::new(ground.x - left.distance_to(ground), ground.y - front.distance_to(ground));
    let high = DVec2::new(ground.x + right.distance_to(ground), ground.y + back.distance_to(ground));
    let expected = [high, DVec2::new(low.x, high.y), low, DVec2::new(high.x, low.y)];
    corners.iter().zip(expected).all(|(corner, box_corner)| {
        (corner.x - box_corner.x).abs() <= COORDINATE_TOLERANCE
            && (corner.y - box_corner.y).abs() <= COORDINATE_TOLERANCE
    })
}

/// The section corners as the document ring a panel stores as its outline.
///
/// The polygon is measured as a geometry region first — a degenerate or self-crossing section is
/// refused there — and the document ring is built from the same corners, rounded to the micrometre.
fn ring(corners: [DVec2; 4]) -> Option<Contour> {
    Region::from_loops(corners.to_vec(), Vec::new()).ok()?;
    let points = corners.into_iter().map(document_point).collect::<Option<Vec<_>>>()?;
    Contour::from_polygon(points).ok()
}

/// One point of the section as the document's micrometres.
fn document_point(point: DVec2) -> Option<[Length; 2]> {
    let micrometres = |value: f64| -> Option<i64> {
        let rounded = (value * 1000.0).round();
        if !rounded.is_finite() || rounded.abs() > i64::MAX as f64 {
            return None;
        }
        Some(rounded as i64)
    };
    Some([Length::from_micrometres(micrometres(point.x)?), Length::from_micrometres(micrometres(point.y)?)])
}

/// Where two wall lines meet, or `None` when they are parallel.
fn intersection(first: Wall, second: Wall) -> Option<DVec2> {
    let determinant = first.normal.x * second.normal.y - first.normal.y * second.normal.x;
    if !determinant.is_finite() || determinant.abs() < SECTION_ANGLE {
        return None;
    }
    let first_offset = first.normal.dot(first.point);
    let second_offset = second.normal.dot(second.point);
    let x = (first_offset * second.normal.y - second_offset * first.normal.y) / determinant;
    let y = (first.normal.x * second_offset - second.normal.x * first_offset) / determinant;
    let corner = DVec2::new(x, y);
    corner.is_finite().then_some(corner)
}

/// The signed area of the polygon; positive when its corners run counter-clockwise.
fn signed_area(corners: [DVec2; 4]) -> f64 {
    let mut total = 0.0;
    for index in 0..corners.len() {
        let (Some(start), Some(end)) = (corners.get(index), corners.get((index + 1) % corners.len())) else {
            continue;
        };
        total += start.x * end.y - end.x * start.y;
    }
    total * 0.5
}

/// Whether a section corner lies on the face of the wall that stopped the probe.
///
/// The corner is on the face's supporting line by construction; a wall that ends before the corner —
/// a half-depth panel — does not own the whole edge, so the void is not one extruded shape.
fn within_face(wall: Wall, placement: Placement, corner: DVec2, z: f64) -> bool {
    let world = placement.transform_point(DVec3::new(corner.x, corner.y, z));
    let Ok(inverse) = wall.surface.world.inverse() else {
        return false;
    };
    let local = inverse.transform_point(world);
    if !local.is_finite() {
        return false;
    }
    for axis in 0..3 {
        if axis == wall.axis {
            continue;
        }
        let values = local.to_array();
        let low = wall.surface.local_min.to_array();
        let high = wall.surface.local_max.to_array();
        let (Some(value), Some(low), Some(high)) = (values.get(axis), low.get(axis), high.get(axis)) else {
            return false;
        };
        if *value < low - COORDINATE_TOLERANCE || *value > high + COORDINATE_TOLERANCE {
            return false;
        }
    }
    true
}

/// A point rounded to the micrometre the document stores.
fn quantised_point(point: DVec2) -> DVec2 {
    DVec2::new(quantised(point.x), quantised(point.y))
}

/// Whether the placement's rotation maps each local axis onto a world axis.
fn axis_aligned(placement: Placement) -> bool {
    for unit in [DVec3::X, DVec3::Y, DVec3::Z] {
        let mapped = placement.transform_vector(unit);
        if !mapped.is_finite() {
            return false;
        }
        let components = [mapped.x.abs(), mapped.y.abs(), mapped.z.abs()];
        let strong = components.iter().filter(|value| **value > 0.5).count();
        let weak = components.iter().filter(|value| **value <= 0.5).count();
        let aligned = components.iter().any(|value| (*value - 1.0).abs() <= AXIS_ALIGNMENT);
        if strong != 1 || weak != 2 || !aligned {
            return false;
        }
    }
    true
}

/// Rounds a measurement to the micrometre the document stores.
fn quantised(value: f64) -> f64 {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded.is_finite() { rounded } else { 0.0 }
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

    /// A standing post whose inner face runs through `through`, angled so that face's normal
    /// pointing into the free space is `normal` (both in the product's XY plane, millimetres).
    ///
    /// The post is 720 mm tall, 18 mm thick and wide enough to cross the whole depth: the panel's
    /// local X runs down the height, its local Y along the depth and its local Z through the
    /// thickness, so a rotation about Y stands it up and a rotation about Z splays it.
    fn angled_post(project: &mut Project, parent: Id, through: (f64, f64), normal: (f64, f64)) -> Id {
        let direction = (-normal.1, normal.0);
        // The panel is 1200 mm wide, so the start of its local Y is placed to centre that span on
        // the case's depth instead of hanging off one side.
        let start_y = 280.0 - 600.0 * direction.1;
        let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
        let angle = normal.1.atan2(normal.0);
        let rotation = DQuat::from_rotation_z(angle) * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let translation = DVec3::new(start_x, start_y, 720.0) - DVec3::new(normal.0, normal.1, 0.0) * 18.0;
        let placement = Placement::new(translation, rotation).expect("a placement");
        let nominal = PanelNominalSize::new(millimetres(720), millimetres(1200)).expect("a post size");
        project.add_panel(parent, "Post", nominal, None, millimetres(18), placement).expect("add a post")
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
            [millimetres(400), millimetres(560)],
            [millimetres(200), millimetres(560)],
            [millimetres(100), millimetres(0)],
            [millimetres(500), millimetres(0)],
        ];
        assert_eq!(corners, expected, "the trapezoid's own vertices");
        assert_eq!(niche.local_min, DVec3::new(100.0, 0.0, 0.0));
        assert_eq!(niche.size, DVec3::new(400.0, 560.0, 720.0), "the box around the section");
    }

    /// Places a mirrored pair of angled posts whose inner faces pass through (100, 0) and (500, 0).
    fn angular_pair(project: &mut Project, piece: Id, left_normal: (f64, f64), right_normal: (f64, f64)) {
        angled_post(project, piece, (100.0, 0.0), left_normal);
        angled_post(project, piece, (500.0, 0.0), right_normal);
    }

    #[test]
    fn a_wall_that_stops_halfway_is_refused() {
        // A post that reaches only 300 mm up does not bound one extruded shape, so a point beside it
        // has no niche the tool could cut a shelf to.
        let (mut project, piece) = empty_piece();
        // A standing panel 300 mm tall, 18 mm thick: local X runs down its height, so a rotation
        // about Y stands it up with its face across the case.
        let nominal = PanelNominalSize::new(millimetres(300), millimetres(560)).expect("a short post");
        let rotation = furnicraft_geom::DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let placement = Placement::new(DVec3::new(300.0, 0.0, 300.0), rotation).expect("a place");
        project.add_panel(piece, "Stub", nominal, None, millimetres(18), placement).expect("add a stub");
        let refusal = project.niche(DVec3::new(150.0, 280.0, 200.0)).expect_err("half a wall is not a wall");
        assert_eq!(refusal, NicheRefusal::Unrepresentable);
    }

    #[test]
    fn a_turned_piece_is_refused_by_the_box_query_but_not_by_the_probe() {
        let mut project = clean_project();
        let piece = product(&mut project, (600, 560, 720), (0, 0, 0));
        let turned = Placement::IDENTITY
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(45_000), ..EulerAngles::default() })
            .expect("a turn");
        project.set_node_placement(piece, turned).expect("turn the piece");
        let inside = DVec3::new(100.0, 100.0, 100.0);
        assert!(project.probe(inside, DVec3::Z).is_some(), "a probe is a ray and still answers");
        assert_eq!(project.niche(inside), Err(NicheRefusal::Turned), "a box would not fit a turned piece");
    }

    #[test]
    fn a_point_outside_every_product_has_no_niche() {
        let (project, _) = empty_piece();
        assert_eq!(project.niche(DVec3::new(-100.0, 0.0, 0.0)), Err(NicheRefusal::Outside));
        assert_eq!(project.niche(DVec3::splat(f64::NAN)), Err(NicheRefusal::Outside));
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
