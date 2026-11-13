//! The niche probe: how far the free space around a point reaches in each direction.
//!
//! A niche is the space a piece of furniture reserves (ADR 0099); it is derived from the document
//! every time it is asked for and never stored (ADR 0102, ADR 0103). Two questions answer it:
//!
//! - [`Project::probe`] casts one ray from a point and reports the distance to the first furniture
//!   surface and what owns it — a panel face, a geometry body, another product's shell, or a
//!   virtual face of the envelope of the piece the point sits in;
//! - [`Project::niche`] casts six rays along a product's own axes and aggregates them into a box
//!   with a size, a position and a shape.
//!
//! Both are pure queries: no session, no renderer, no document write. The surfaces are the same
//! boxes the furniture move rule measures against (`super::surface`), so the tool that fills a
//! niche and the tool that drags a piece cannot disagree about where a surface is.
//!
//! Version 1 answers a box for an axis-aligned product. A product turned off the world axes has no
//! world-box answer, so [`Project::niche`] refuses it instead of returning a loose box; the shaped
//! answer is a later stage (N7).

use furnicraft_geom::DVec3;

use super::surface::{SurfaceBox, SurfaceKind, face_ahead, surface_box};
use crate::tree::object_bounds_mm;
use crate::*;

/// Tolerance for deciding that a product's axes lie on the world axes.
const AXIS_ALIGNMENT: f64 = 1.0e-9;

/// What one probe stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeHit {
    /// The node that owns the surface: a product for its shell or envelope, a panel, an object.
    pub id: Id,
    /// Which kind of surface the probe met.
    pub kind: SurfaceKind,
}

/// The answer of one probe: the distance to the first surface and what it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// The distance along the probe direction, in millimetres, a whole number of micrometres.
    pub distance_mm: f64,
    /// The surface the ray stopped at.
    pub hit: ProbeHit,
}

/// The shape of a niche.
///
/// Version 1 has one variant, a box, and the enum exists so a later stage can add the extruded
/// cross-section of an angled piece without changing the query's return type (ADR 0099).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NicheShape {
    /// The axis-aligned box of six probes.
    Box,
}

/// The empty space around a point inside a product, as the box six probes enclose.
///
/// Everything here is derived: the query stores nothing in the document. `local_min` and `size` are
/// in the product's own frame, which is the frame a panel is placed in; `min` is the anchored
/// corner in world millimetres, so a tool never re-derives a position from a cursor and a size.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub fn local_max(self) -> DVec3 {
        self.local_min + self.size
    }

    /// One side of the box along the product's own axes, in millimetres.
    pub fn side_mm(self, axis: usize) -> f64 {
        match axis {
            0 => self.size.x,
            1 => self.size.y,
            _ => self.size.z,
        }
    }
}

impl Project {
    /// The distance from `point` to the first furniture surface along `direction`.
    ///
    /// The ray stops at a panel face, a geometry body, another product's shell, or a virtual face of
    /// the envelope of the innermost product that contains the point. It never passes through a
    /// surface (ADR 0102) and it is measured in world millimetres, quantised to the document's
    /// micrometre. A point that is not finite, a direction that is zero or not finite, and a ray
    /// that meets nothing all answer `None`.
    pub fn probe(&self, point: DVec3, direction: DVec3) -> Option<Probe> {
        if !point.is_finite() || !direction.is_finite() {
            return None;
        }
        let length = direction.length();
        if !length.is_finite() || length <= f64::EPSILON {
            return None;
        }
        let direction = direction / length;
        let (own, surfaces) = self.surfaces_around(point);
        let mut best: Option<(f64, ProbeHit)> = None;
        if let Some((product, envelope)) = own
            && let Some(distance) = face_ahead(point, direction, envelope.min, envelope.max)
        {
            best = Some((distance, ProbeHit { id: product, kind: SurfaceKind::Envelope }));
        }
        for surface in &surfaces {
            if own.is_some_and(|(product, _)| product == surface.id) {
                continue;
            }
            let Some(distance) = face_ahead(point, direction, surface.min, surface.max) else {
                continue;
            };
            if best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, ProbeHit { id: surface.id, kind: surface.kind }));
            }
        }
        best.map(|(distance, hit)| Probe { distance_mm: quantised(distance), hit })
    }

    /// The empty space around `point`, as the box the six probes along a product's axes enclose.
    ///
    /// The product is the innermost envelope that contains the point; `None` when the point is
    /// outside every product. A product turned off the world axes is refused: its own box is not a
    /// world box, and approximating it would hand a tool a size that does not fit (N7 answers the
    /// shaped case). The six distances are measured through [`Project::probe`], so the niche, the
    /// distance indicator and the move rule share one idea of where a surface is.
    pub fn niche(&self, point: DVec3) -> Option<Niche> {
        if !point.is_finite() {
            return None;
        }
        let (product, _) = self.product_containing(point)?;
        let placement = self.world_placement(product)?;
        if !axis_aligned(placement) {
            return None;
        }
        let inverse = placement.inverse().ok()?;
        let local = inverse.transform_point(point);
        let mut negative = DVec3::ZERO;
        let mut positive = DVec3::ZERO;
        for axis in 0..3 {
            let unit = axis_unit(axis);
            let direction = placement.transform_vector(unit);
            positive[axis] = self.probe(point, direction)?.distance_mm;
            negative[axis] = self.probe(point, -direction)?.distance_mm;
        }
        let local_min = local - negative;
        let size = positive + negative;
        let world_min = placement.transform_point(local_min);
        if !world_min.is_finite() || !size.is_finite() {
            return None;
        }
        Some(Niche { product, local_min, size, min: world_min, shape: NicheShape::Box })
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
}

/// The unit vector of one local axis, by index.
fn axis_unit(axis: usize) -> DVec3 {
    match axis {
        0 => DVec3::X,
        1 => DVec3::Y,
        _ => DVec3::Z,
    }
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

    use furnicraft_geom::DVec3;
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
    fn a_turned_piece_is_refused_by_the_box_query_but_not_by_the_probe() {
        let mut project = clean_project();
        let piece = product(&mut project, (600, 560, 720), (0, 0, 0));
        let turned = Placement::IDENTITY
            .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(45_000), ..EulerAngles::default() })
            .expect("a turn");
        project.set_node_placement(piece, turned).expect("turn the piece");
        let inside = DVec3::new(100.0, 100.0, 100.0);
        assert!(project.probe(inside, DVec3::Z).is_some(), "a probe is a ray and still answers");
        assert_eq!(project.niche(inside), None, "a box would not fit a turned piece");
    }

    #[test]
    fn a_point_outside_every_product_has_no_niche() {
        let (project, _) = empty_piece();
        assert_eq!(project.niche(DVec3::new(-100.0, 0.0, 0.0)), None);
        assert_eq!(project.niche(DVec3::splat(f64::NAN)), None);
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
