//! One conversion from a planar ring to a kernel profile.
//!
//! Every planar shape reaches the geometry kernel through this module: the rectangular core, the
//! shaped panel core, a cutout tool and a constructive extrusion. A document [`model::Contour`]
//! becomes an analytic [`Contour`], that contour becomes a [`Region`] whose rings stay exact, and the
//! region is sampled into a [`Profile`]. [`profile_of_region`] is the one place where an arc turns
//! into a polyline and [`CONTOUR_SAMPLE_TOLERANCE_MM`] is the one chord tolerance it uses.
//!
//! The document keeps micrometres; the conversion to millimetres happens in `model::Contour`, at
//! the geometry boundary. This module never sees a kernel type from the document.

use furnicraft_geom::{Contour, Region};
use furnicraft_kernel::Profile;
use furnicraft_model::MAX_FACE_VERTICES;

use crate::ParamError;

/// Chord tolerance used when an analytic contour becomes a kernel polyline, in millimetres.
pub(crate) const CONTOUR_SAMPLE_TOLERANCE_MM: f64 = 0.05;

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
pub(crate) fn profile_of_region(region: &Region) -> Result<Profile, ParamError> {
    let samples = region.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    let (outer, holes) = samples.into_parts();
    Ok(Profile::new(outer, holes)?)
}

/// The kernel profile of an analytic contour, sampled by the one sampler.
pub(crate) fn profile_of_contour(contour: &Contour) -> Result<Profile, ParamError> {
    profile_of_region(&region_of_contour(contour)?)
}

/// The kernel profile of a rectangle at its own origin.
pub(crate) fn profile_of_rectangle(width: f64, height: f64) -> Result<Profile, ParamError> {
    profile_of_region(&region_of_rectangle(width, height)?)
}

/// The kernel profile of a document contour, refused when one extrusion cap cannot hold it.
///
/// The cap of a single extrusion is one document face and its vertices are the sampled outer ring,
/// so a contour that samples to more points than [`MAX_FACE_VERTICES`] can never be drawn. A command
/// that stores a ring calls this before it mutates the document, so an object never ends up with a
/// contour that evaluates to nothing.
pub(crate) fn profile_of_document_contour(contour: &furnicraft_model::Contour) -> Result<Profile, ParamError> {
    let profile = profile_of_region(&region_of_document_contour(contour)?)?;
    if profile.outer.len() > MAX_FACE_VERTICES {
        return Err(ParamError::ContourTooDetailed { points: profile.outer.len(), max: MAX_FACE_VERTICES });
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Contour as DocumentContour, ContourSegment};
    use furnicraft_units::{Degrees, Length};

    use super::*;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

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
}
