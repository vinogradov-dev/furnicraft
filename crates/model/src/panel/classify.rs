//! Deciding when a measured shape is still a standard rectangle panel.
//!
//! A contour panel loses `panel.resize`, its rectangle card and its nominal fields (ADR 0042), so a
//! part is only a contour when nothing simpler draws the same shape. This module answers that one
//! question: given a finished ring, is there a nominal rectangle — plain, or with corner chamfers or
//! radii — whose generator draws exactly that ring?
//!
//! The answer is found by generating and comparing, never by judging angles: every candidate is
//! built by [`PanelOutline::ring_for`], the same generator the panel's geometry uses, and accepted
//! only when it matches the ring segment by segment within [`RING_TOLERANCE_MM`] (one micrometre).
//! A shape is therefore never called standard on a guess — a nearly rectangular section with a
//! slanted side stays a contour, because its ring and the candidate's ring disagree by more than the
//! document can round away.
//!
//! Changes when: the classifier gains a candidate family, or the ring generator changes what a
//! corner treatment draws.

use furnicraft_geom::{Contour as GeometryRing, DVec2, Segment};

use crate::{Contour, CornerFeature, CornerFeatureKind, Length, PanelNominalSize, PanelOutline, RectCorner};

/// Tolerance for comparing two rings, in millimetres: one micrometre, the document's own step.
pub const RING_TOLERANCE_MM: f64 = 1.0e-3;

/// The standard rectangle outline that draws exactly `ring`, or `None` when no rectangle does.
///
/// `ring` is anchored at the panel-local zero corner, as a panel outline is. A plain rectangle
/// answers a [`PanelOutline::Rect`] with no features; a rectangle with corner cuts or radii answers
/// one carrying those features; anything else — a trapezoid, a shape whose cut consumes a whole
/// edge, a ring a treatment cannot draw — answers `None`, and the caller keeps the ring as a
/// contour.
pub fn classify_outline(ring: &Contour) -> Option<PanelOutline> {
    let (min, max) = ring.bounds_micrometres();
    if min != [0, 0] {
        return None;
    }
    let length = Length::from_micrometres(max[0].checked_sub(min[0])?);
    let width = Length::from_micrometres(max[1].checked_sub(min[1])?);
    let nominal_size = PanelNominalSize::new(length, width).ok()?;
    let target = ring.to_geometry().ok()?;

    let plain = PanelOutline::Rect { corner_features: Vec::new(), edge_bows: Vec::new() };
    if draws_exactly(&plain, nominal_size, &target) {
        return Some(plain);
    }
    let features = corner_features_of(&target, nominal_size)?;
    let candidate = PanelOutline::Rect { corner_features: features, edge_bows: Vec::new() };
    draws_exactly(&candidate, nominal_size, &target).then_some(candidate)
}

/// Whether `outline`'s generator draws the target ring.
fn draws_exactly(outline: &PanelOutline, nominal_size: PanelNominalSize, target: &GeometryRing) -> bool {
    let Ok(candidate) = outline.ring_for(nominal_size) else {
        return false;
    };
    rings_match(&candidate, target)
}

/// Whether two rings carry the same segments, allowing any starting segment.
///
/// A generated ring starts at the nominal zero corner and a measured one starts wherever the walls
/// put it, so every rotation is tried. The segment kinds must agree: a chamfer cannot stand in for
/// a radius, and a line cannot stand in for an arc.
fn rings_match(candidate: &GeometryRing, target: &GeometryRing) -> bool {
    let (candidate, target) = (candidate.segments(), target.segments());
    if candidate.len() != target.len() || candidate.is_empty() {
        return false;
    }
    (0..candidate.len()).any(|shift| {
        (0..candidate.len()).all(|index| {
            let other = target.get((index + shift) % target.len()).copied();
            other.is_some_and(|other| segments_match(candidate[index], other))
        })
    })
}

/// Whether two segments are the same curve within the document's step.
fn segments_match(first: Segment, second: Segment) -> bool {
    match (first, second) {
        (Segment::Line(_), Segment::Line(_)) => near(first.start(), second.start()) && near(first.end(), second.end()),
        (Segment::Arc(first), Segment::Arc(second)) => {
            near(first.center(), second.center())
                && (first.radius() - second.radius()).abs() <= RING_TOLERANCE_MM
                && near(first.start(), second.start())
                && near(first.end(), second.end())
        }
        _ => false,
    }
}

/// Whether two points are within the document's step of each other.
fn near(first: DVec2, second: DVec2) -> bool {
    (first - second).length() <= RING_TOLERANCE_MM
}

/// The corner treatments a target ring implies, when every corner can carry one.
///
/// Each nominal corner owns at most one segment between its two edges: a straight cut is a chamfer
/// with the setbacks its ends measure, an arc centred on the corner is a radius. A corner with no
/// such segment is untreated. The result is a *proposal*: the caller generates its ring and compares
/// it with the target, so a proposal that misses anything is refused there rather than trusted here.
fn corner_features_of(target: &GeometryRing, nominal_size: PanelNominalSize) -> Option<Vec<CornerFeature>> {
    let length = nominal_size.length().millimetres_f64();
    let width = nominal_size.width().millimetres_f64();
    // Each corner with the unit direction of the edge that ends there and of the edge that starts
    // there, in the ring's counter-clockwise travel order.
    let corners = [
        (DVec2::new(0.0, 0.0), DVec2::NEG_Y, DVec2::X, RectCorner::BottomLeft),
        (DVec2::new(length, 0.0), DVec2::X, DVec2::Y, RectCorner::BottomRight),
        (DVec2::new(length, width), DVec2::Y, DVec2::NEG_X, RectCorner::TopRight),
        (DVec2::new(0.0, width), DVec2::NEG_X, DVec2::NEG_Y, RectCorner::TopLeft),
    ];
    let mut features = Vec::new();
    for (corner, incoming, outgoing, id) in corners {
        let mut found: Option<CornerFeatureKind> = None;
        for segment in target.segments() {
            let Some(kind) = feature_at(*segment, corner, incoming, outgoing) else {
                continue;
            };
            if found.is_some() {
                // Two segments claim one corner: the ring is not a rectangle with treatments.
                return None;
            }
            found = Some(kind);
        }
        if let Some(kind) = found {
            features.push(CornerFeature::new(id.id(), kind).ok()?);
        }
    }
    (!features.is_empty()).then_some(features)
}

/// The treatment one ring segment draws at a nominal corner, when it is that corner's treatment.
///
/// The treatment runs from a point on the incoming edge to a point on the outgoing one, so both
/// ends must lie on the rays that leave the corner along those edges.
fn feature_at(segment: Segment, corner: DVec2, incoming: DVec2, outgoing: DVec2) -> Option<CornerFeatureKind> {
    let into = segment.start() - corner;
    let out = segment.end() - corner;
    let coming = -into.dot(incoming);
    let going = out.dot(outgoing);
    if coming <= RING_TOLERANCE_MM || going <= RING_TOLERANCE_MM {
        return None;
    }
    if (into + incoming * coming).length() > RING_TOLERANCE_MM || (out - outgoing * going).length() > RING_TOLERANCE_MM
    {
        return None;
    }
    match segment {
        Segment::Line(_) => {
            Some(CornerFeatureKind::Chamfer { incoming: length_of(coming)?, outgoing: length_of(going)? })
        }
        Segment::Arc(arc) => {
            // A radius is tangent to both edges, so its centre sits one radius inside the corner
            // along each of them, and both setbacks are that same radius.
            let radius = coming.min(going);
            let left = DVec2::new(-incoming.y, incoming.x);
            let centre = corner - incoming * radius + left * radius;
            if (arc.center() - centre).length() > RING_TOLERANCE_MM
                || (coming - going).abs() > RING_TOLERANCE_MM
                || (arc.radius() - radius).abs() > RING_TOLERANCE_MM
            {
                return None;
            }
            Some(CornerFeatureKind::Radius { radius: length_of(radius)? })
        }
    }
}

/// A whole-micrometre measurement as the document's length, or `None` when it is not usable.
fn length_of(millimetres: f64) -> Option<Length> {
    if !millimetres.is_finite() || millimetres <= 0.0 {
        return None;
    }
    let micrometres = (millimetres * 1000.0).round();
    if !micrometres.is_finite() || micrometres.abs() > i64::MAX as f64 {
        return None;
    }
    let length = Length::from_micrometres(micrometres as i64);
    // A feature dimension has the same limits a corner treatment has, so a candidate is either
    // storable or refused here instead of at the panel's first geometry.
    CornerFeatureKind::Radius { radius: length }.validate().ok()?;
    Some(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContourSegment, Panel};

    /// A ring of straight segments through `points`, in whole millimetres.
    fn polygon(points: &[(i64, i64)]) -> Contour {
        let points = points
            .iter()
            .map(|(x, y)| {
                [
                    Length::from_micrometres(x.checked_mul(1000).expect("a whole millimetre")),
                    Length::from_micrometres(y.checked_mul(1000).expect("a whole millimetre")),
                ]
            })
            .collect();
        Contour::from_polygon(points).expect("a valid ring")
    }

    /// The chamfers and radii a classified outline carries, as text.
    fn features(outline: &PanelOutline) -> Vec<String> {
        outline
            .corner_features()
            .iter()
            .map(|feature| match feature.feature() {
                CornerFeatureKind::Radius { radius } => {
                    format!("radius {} at {}", radius.micrometres(), feature.corner().get())
                }
                CornerFeatureKind::Chamfer { incoming, outgoing } => {
                    format!(
                        "chamfer {}/{} at {}",
                        incoming.micrometres(),
                        outgoing.micrometres(),
                        feature.corner().get()
                    )
                }
            })
            .collect()
    }

    #[test]
    fn a_rectangle_stays_the_nominal_rectangle() {
        let outline = classify_outline(&polygon(&[(0, 0), (600, 0), (600, 400), (0, 400)])).expect("a rectangle");
        assert_eq!(outline, PanelOutline::Rect { corner_features: Vec::new(), edge_bows: Vec::new() });
    }

    #[test]
    fn a_rectangle_started_at_another_corner_is_still_the_nominal_rectangle() {
        // The walls hand the ring over starting wherever the cell's first plane put it.
        let outline = classify_outline(&polygon(&[(600, 400), (600, 0), (0, 0), (0, 400)])).expect("a rectangle");
        assert!(!outline.has_contour_features(), "the same rectangle, drawn from another corner");
    }

    #[test]
    fn one_asymmetric_corner_cut_becomes_a_rect_with_one_chamfer() {
        // The top-right corner is cut from 40 mm along the top edge to 15 mm up the right edge.
        let ring = polygon(&[(0, 0), (600, 0), (600, 385), (560, 400), (0, 400)]);
        let outline = classify_outline(&ring).expect("a chamfered rectangle");
        assert_eq!(features(&outline), vec!["chamfer 15000/40000 at 3".to_owned()]);
        assert!(matches!(outline, PanelOutline::Rect { .. }), "the panel keeps its rectangle card");
    }

    #[test]
    fn a_rounded_ring_becomes_a_rect_with_one_radius_at_each_corner() {
        // A curved section cannot come from the plane-only cell yet, so the ring is handed over
        // directly: the radius path still has to be measured, not assumed.
        let size = PanelNominalSize::new(
            Length::from_millimetres(600).expect("a length"),
            Length::from_millimetres(400).expect("a length"),
        )
        .expect("a panel size");
        let radius = 40_i64;
        let corners = [RectCorner::BottomLeft, RectCorner::BottomRight, RectCorner::TopRight, RectCorner::TopLeft];
        let features = corners
            .into_iter()
            .map(|corner| {
                CornerFeature::new(
                    corner.id(),
                    CornerFeatureKind::Radius { radius: Length::from_micrometres(radius * 1000) },
                )
                .expect("a radius")
            })
            .collect();
        let rectangle = PanelOutline::Rect { corner_features: features, edge_bows: Vec::new() };
        let ring = rectangle.ring_for(size).expect("the generator draws it");
        let document = Contour::new(
            ring.segments()
                .iter()
                .map(|segment| ContourSegment::from_geom(*segment).expect("a document segment"))
                .collect(),
        )
        .expect("a document ring");

        let outline = classify_outline(&document).expect("a rounded rectangle");
        assert_eq!(outline, rectangle, "every corner comes back as a radius");
    }

    #[test]
    fn a_cut_that_consumes_a_whole_edge_stays_a_contour() {
        // The bottom edge is 100 long and the two corner cuts take 40 and 60 of it, so no straight
        // edge is left between them: the generator refuses the pair and the ring stays a contour.
        let ring = polygon(&[(0, 40), (40, 0), (100, 20), (100, 100), (0, 100)]);
        assert!(classify_outline(&ring).is_none(), "a cut that eats an edge is not a chamfer: {ring:?}");
    }

    #[test]
    fn a_trapezoid_is_a_contour() {
        let ring = polygon(&[(0, 0), (600, 0), (500, 400), (100, 400)]);
        assert!(classify_outline(&ring).is_none(), "the slanted sides are not a corner treatment");
    }

    #[test]
    fn a_nearly_rectangular_ring_is_a_contour() {
        // A slanted side that is 2 mm out: an angle heuristic would call it standard and store a
        // ring that is not the section.
        let ring = polygon(&[(0, 0), (598, 0), (600, 400), (0, 400)]);
        assert!(classify_outline(&ring).is_none());
    }

    #[test]
    fn a_ring_that_is_not_anchored_at_the_zero_corner_is_refused() {
        assert!(classify_outline(&polygon(&[(10, 10), (610, 10), (610, 410), (10, 410)])).is_none());
    }

    #[test]
    fn the_panel_generator_and_the_classifier_agree_on_a_stored_rectangle() {
        // The same ring, reached through a real panel: `ring_for` and the panel's own outline are
        // one generator, so the classifier can never accept a shape the panel would not draw.
        let size = PanelNominalSize::new(
            Length::from_millimetres(600).expect("a length"),
            Length::from_millimetres(400).expect("a length"),
        )
        .expect("a panel size");
        let panel = Panel::new(
            crate::Id::new(1).expect("an id"),
            "Back",
            size,
            None,
            Length::from_millimetres(18).expect("a thickness"),
            crate::Placement::IDENTITY,
        )
        .expect("a panel");
        let ring = panel.nominal_contour().expect("the panel's ring");
        let ring = Contour::new(
            ring.segments()
                .iter()
                .map(|segment| ContourSegment::from_geom(*segment).expect("a document segment"))
                .collect(),
        )
        .expect("a document ring");
        assert!(matches!(classify_outline(&ring), Some(PanelOutline::Rect { .. })));
        assert_eq!(ring.bounds_micrometres(), ([0, 0], [600_000, 400_000]));
    }

    #[test]
    fn a_type_that_cannot_hold_the_measurement_is_refused() {
        assert!(length_of(0.0).is_none());
        assert!(length_of(-1.0).is_none());
        assert!(length_of(f64::NAN).is_none());
        assert_eq!(length_of(12.5).map(|length| length.micrometres()), Some(12_500));
    }
}
