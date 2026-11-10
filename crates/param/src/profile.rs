//! One conversion from a planar ring to a kernel profile.
//!
//! Every planar shape reaches the geometry kernel through this module: the rectangular core, the
//! shaped panel core, a cutout tool and a constructive extrusion. A document [`model::Contour`]
//! becomes an analytic [`Contour`], that contour becomes a [`Region`] whose rings stay exact, and the
//! region is sampled into a [`Profile`]. [`profile_of_region`] is the one place where an arc turns
//! into a polyline and [`contour_tolerance`] is the one chord tolerance it uses.
//!
//! The document keeps micrometres; the conversion to millimetres happens in `model::Contour`, at
//! the geometry boundary. This module never sees a kernel type from the document.

use furnicraft_geom::{Contour, DVec2, Region, Segment};
use furnicraft_kernel::Profile;
use furnicraft_model::MAX_FACE_VERTICES;

use crate::ParamError;

/// Chord tolerance used when an analytic contour becomes a kernel polyline, in millimetres.
///
/// This is the floor: a shape smaller than its own tolerance never needs a coarser chord.
pub(crate) const CONTOUR_SAMPLE_TOLERANCE_MM: f64 = 0.05;

/// The largest chord tolerance of any shape, as a fraction of its bounding diagonal.
///
/// A chord deviation of `1e-4` of the shape is invisible at both ends of the range: a 600 mm panel
/// stays on the `0.05 mm` floor, while a 10 m outline samples at about `1 mm`. Without it the point
/// count follows the radius instead of the shape, and a legal ring of a few large arcs overflows one
/// extrusion cap (ADR 0062).
const CONTOUR_TOLERANCE_FRACTION: f64 = 1.0e-4;

/// The region of a rectangle core or cutout.
pub(crate) fn region_of_rectangle(width: f64, height: f64) -> Result<Region, ParamError> {
    Ok(Region::rectangle(width, height)?)
}

/// The region of an analytic contour, with its arc still an arc.
pub(crate) fn region_of_contour(contour: &Contour) -> Result<Region, ParamError> {
    Ok(Region::new(contour.clone(), Vec::new())?)
}

/// The region of a document contour, converted exactly.
pub(crate) fn region_of_document_contour(contour: &furnicraft_model::Contour) -> Result<Region, ParamError> {
    region_of_contour(&contour.to_geometry()?)
}

/// The kernel profile of a region, sampled by the one sampler and one chord tolerance.
///
/// A ring of many arcs can sample past one extrusion cap even at a shape-relative chord, because the
/// count grows with the number of features, not only with the size. Such a ring is sampled again
/// with a coarser chord until it fits, so a legal ring never becomes a body that draws nothing. The
/// relaxation is bounded and only ever coarser, so a shape that already fits is never re-sampled.
pub(crate) fn profile_of_region(region: &Region) -> Result<Profile, ParamError> {
    let tolerance = fit_tolerance(contour_tolerance(region), |tolerance| {
        let samples = region.sample(tolerance)?;
        let (outer, holes) = samples.into_parts();
        Ok(widest_ring(&Profile::new(outer, holes)?))
    })?;
    let samples = region.sample(tolerance)?;
    let (outer, holes) = samples.into_parts();
    Ok(Profile::new(outer, holes)?)
}

/// The coarsest-needed chord at or above `start` whose ring fits one cap.
///
/// `widest_at` returns the widest ring the sampler produces at a chord. The search only ever
/// coarsens, so a ring that already fits keeps the caller's chord exactly; a ring no chord in the
/// budget can fit is a [`ParamError::ContourTooDetailed`], never a silent truncation.
fn fit_tolerance(start: f64, mut widest_at: impl FnMut(f64) -> Result<usize, ParamError>) -> Result<f64, ParamError> {
    let mut tolerance = start;
    let mut widest = widest_at(tolerance)?;
    let mut rounds = 0;
    while widest > MAX_FACE_VERTICES && rounds < MAX_TOLERANCE_RELAXATIONS {
        rounds += 1;
        tolerance *= TOLERANCE_RELAXATION;
        widest = widest_at(tolerance)?;
    }
    if widest > MAX_FACE_VERTICES {
        return Err(ParamError::ContourTooDetailed { points: widest, max: MAX_FACE_VERTICES });
    }
    Ok(tolerance)
}

/// How many times a ring that overflows the cap may be re-sampled coarser.
///
/// Six doublings span a factor of 64: the worst legal ring is one 1024-segment ring of arcs, which
/// this reaches long before the bound. The bound exists so a hostile shape fails as an error.
const MAX_TOLERANCE_RELAXATIONS: u32 = 6;

/// How much coarser each relaxation is. Doubling keeps the loop short and the ring's quality even.
const TOLERANCE_RELAXATION: f64 = 2.0;

/// The largest point count of the outer ring or any hole.
pub(crate) fn widest_ring(profile: &Profile) -> usize {
    profile.holes.iter().map(Vec::len).fold(profile.outer.len(), usize::max)
}

/// The closed outline of one band strip: the outer boundary and the reversed inner one.
///
/// The two boundaries share one chord, taken from the strip's own extent so their arcs agree.
/// A strip is a face of a band body, so the cap applies to it like any other ring.
pub(crate) fn strip_points(outer: &Segment, inner: &Segment) -> Result<Vec<DVec2>, ParamError> {
    let bounds = union_bounds(Some(outer.bounds()), Some(inner.bounds()));
    let tolerance = fit_tolerance(tolerance_for_bounds(bounds), |tolerance| {
        let mut points = Contour::new(vec![*outer], false)?.sample(tolerance)?;
        points.extend(Contour::new(vec![*inner], false)?.sample(tolerance)?.into_iter().rev());
        Ok(points.len())
    })?;
    let mut points = Contour::new(vec![*outer], false)?.sample(tolerance)?;
    points.extend(Contour::new(vec![*inner], false)?.sample(tolerance)?.into_iter().rev());
    Ok(points)
}

/// The chord tolerance for one region: the floor, or a fraction of the shape, whichever is coarser.
///
/// A fixed tolerance spends a fixed number of points per unit of radius, so a large ring overflows a
/// face even when its geometry is simple. Tying the chord to the bounding diagonal keeps the point
/// count a function of the shape, and the deviation stays under a tenth of a percent of the size.
pub(crate) fn contour_tolerance(region: &Region) -> f64 {
    tolerance_for_bounds(region.bounds())
}

/// The chord tolerance for a shape that already knows its bounds.
pub(crate) fn tolerance_for_bounds(bounds: Option<(DVec2, DVec2)>) -> f64 {
    let Some((min, max)) = bounds else {
        return CONTOUR_SAMPLE_TOLERANCE_MM;
    };
    let diagonal = max - min;
    let tolerance = diagonal.length() * CONTOUR_TOLERANCE_FRACTION;
    if tolerance.is_finite() && tolerance > CONTOUR_SAMPLE_TOLERANCE_MM {
        tolerance
    } else {
        CONTOUR_SAMPLE_TOLERANCE_MM
    }
}

/// The smallest box around two bounds, either of which may be missing.
pub(crate) fn union_bounds(first: Option<(DVec2, DVec2)>, second: Option<(DVec2, DVec2)>) -> Option<(DVec2, DVec2)> {
    match (first, second) {
        (Some((first_min, first_max)), Some((second_min, second_max))) => {
            Some((first_min.min(second_min), first_max.max(second_max)))
        }
        (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
        (None, None) => None,
    }
}

/// The kernel profile of a rectangle at its own origin.
pub(crate) fn profile_of_rectangle(width: f64, height: f64) -> Result<Profile, ParamError> {
    profile_of_region(&region_of_rectangle(width, height)?)
}

/// The kernel profile of a document contour, sampled so one extrusion cap can hold it.
///
/// The cap of a single extrusion is one document face and its vertices are the sampled ring. A
/// command that stores a ring calls this before it mutates the document, so a legal ring always
/// becomes an object that draws, and only a shape no chord can fit is refused.
pub(crate) fn profile_of_document_contour(contour: &furnicraft_model::Contour) -> Result<Profile, ParamError> {
    profile_of_region(&region_of_document_contour(contour)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Contour as DocumentContour, ContourSegment};
    use furnicraft_testkit::millimetres;
    use furnicraft_units::{Degrees, Length};

    use super::*;

    fn rounded_plate() -> DocumentContour {
        let arc = ContourSegment::arc(
            [millimetres(80), millimetres(30)],
            millimetres(20),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        DocumentContour::new(vec![
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"),
            ContourSegment::line([millimetres(100), Length::ZERO], arc.start().expect("start")).expect("line"),
            arc,
            ContourSegment::line(arc.end().expect("end"), [Length::ZERO, millimetres(50)]).expect("line"),
            ContourSegment::line([Length::ZERO, millimetres(50)], [Length::ZERO, Length::ZERO]).expect("line"),
        ])
        .expect("rounded plate")
    }

    #[test]
    fn a_rectangle_region_becomes_its_four_corner_profile() {
        let profile = profile_of_rectangle(100.0, 50.0).expect("rectangle profile");
        assert_eq!(profile.outer.len(), 4);
        assert!(profile.holes.is_empty());
        assert!((profile.area() - 5000.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_document_ring_keeps_its_shape_and_samples_its_arc_once() {
        let profile = profile_of_document_contour(&rounded_plate()).expect("ring profile");
        assert!(profile.outer.len() > 5, "the arc is sampled into the polyline");
        assert!(profile.holes.is_empty());
        // The plate is 100 x 50 with a 20 mm quarter round cut out of one corner. The sampled
        // polygon is inscribed, so it is slightly smaller than the analytic area.
        let rounded_corner = 20.0 * 20.0 - std::f64::consts::PI * 20.0 * 20.0 / 4.0;
        assert!((profile.area() - (5000.0 - rounded_corner)).abs() < 2.0, "area was {}", profile.area());
    }

    #[test]
    fn a_region_keeps_its_holes_in_the_profile() {
        let outer = vec![
            furnicraft_geom::DVec2::new(0.0, 0.0),
            furnicraft_geom::DVec2::new(10.0, 0.0),
            furnicraft_geom::DVec2::new(10.0, 10.0),
            furnicraft_geom::DVec2::new(0.0, 10.0),
        ];
        let hole = vec![
            furnicraft_geom::DVec2::new(2.0, 2.0),
            furnicraft_geom::DVec2::new(4.0, 2.0),
            furnicraft_geom::DVec2::new(4.0, 4.0),
            furnicraft_geom::DVec2::new(2.0, 4.0),
        ];
        let region = Region::from_loops(outer, vec![hole]).expect("region with a hole");
        let profile = profile_of_region(&region).expect("profile");
        assert_eq!(profile.outer.len(), 4);
        assert_eq!(profile.holes.len(), 1);
        assert!((profile.area() - 96.0).abs() < 1.0e-9);
    }

    /// A "C" band: two concentric arcs joined by two radial lines.
    ///
    /// The ring is valid and simple. At the fixed 0.05 mm chord it sampled to more than one face
    /// could hold; the shape-relative chord is what makes it an ordinary solid (X2).
    fn band(outer_radius: i64, inner_radius: i64) -> DocumentContour {
        let sweep = Degrees::from_millidegrees(350_000);
        let outer = ContourSegment::arc([Length::ZERO, Length::ZERO], millimetres(outer_radius), Degrees::ZERO, sweep)
            .expect("outer arc");
        let inner = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(inner_radius),
            sweep,
            Degrees::from_millidegrees(-350_000),
        )
        .expect("inner arc");
        DocumentContour::new(vec![
            outer,
            ContourSegment::line(outer.end().expect("outer end"), inner.start().expect("inner start"))
                .expect("inward line"),
            inner,
            ContourSegment::line(inner.end().expect("inner end"), outer.start().expect("outer start"))
                .expect("outward line"),
        ])
        .expect("a simple band")
    }

    #[test]
    fn a_large_legal_ring_fits_one_extrusion_cap() {
        // Two arcs of a few metres were the reported failure: 350 degrees of a 5 m radius is about
        // 30 m of arc, which at a fixed 0.05 mm chord is thousands of points.
        let profile = profile_of_document_contour(&band(5_000, 4_000)).expect("a large ring is buildable");
        assert!(profile.outer.len() < MAX_FACE_VERTICES, "the cap holds the ring, it held {}", profile.outer.len());
        assert!(profile.outer.len() > 100, "the arcs are still sampled into a polyline");
    }

    #[test]
    fn the_chord_grows_with_the_shape_and_stops_at_its_floor() {
        let small = Region::rectangle(100.0, 50.0).expect("rectangle");
        assert_eq!(contour_tolerance(&small), CONTOUR_SAMPLE_TOLERANCE_MM, "a panel stays on the floor");
        let large = Region::rectangle(10_000.0, 10_000.0).expect("large rectangle");
        let tolerance = contour_tolerance(&large);
        assert!(tolerance > CONTOUR_SAMPLE_TOLERANCE_MM, "a ten-metre shape samples coarser: {tolerance}");
        // A tenth of a percent of the size keeps the deviation invisible at any scale.
        assert!(tolerance <= 14_300.0 * CONTOUR_TOLERANCE_FRACTION, "the chord stays proportional");
    }

    #[test]
    fn a_ring_that_overflows_the_cap_is_re_sampled_coarser_until_it_fits() {
        // `fit_tolerance` is the bounded search. Twenty thousand points at the floor need five
        // doublings, and the first chord that fits wins rather than the coarsest one.
        let start = CONTOUR_SAMPLE_TOLERANCE_MM;
        let points = |tolerance: f64| Ok((1_000.0 / tolerance) as usize);
        let fitted =
            fit_tolerance(start, points).expect("twenty thousand points at the floor fit after a few doublings");
        assert!(points(fitted).expect("a count") <= MAX_FACE_VERTICES, "the returned chord fits");
        assert!(
            points(fitted / TOLERANCE_RELAXATION).expect("a count") > MAX_FACE_VERTICES,
            "and no coarser one was needed"
        );

        let hopeless = fit_tolerance(start, |_| Ok(MAX_FACE_VERTICES + 1));
        assert!(matches!(hopeless, Err(ParamError::ContourTooDetailed { .. })), "a hopeless ring is an error");
    }
}
