//! The nominal panel outline: a rectangle with corner treatments and bowed edges.
//!
//! The outline is counter-clockwise and starts on edge 1 after corner 1. Edge `i` runs from corner
//! `i` to corner `i + 1`; a corner treatment is inserted after the edge that ends there. The builder
//! keeps the document free of kernel types and leaves core/outer offsets to the parametric layer.

use std::f64::consts::FRAC_PI_2;

use furnicraft_geom::{Arc, Contour, ContourError, DVec2, Line, Segment};
use thiserror::Error;

use crate::{CornerFeature, CornerFeatureKind, EdgeId, Panel, PanelNominalSize, RectCorner, RectEdge};

/// Why a panel's nominal contour cannot be built.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum PanelContourError {
    /// A line, arc or closed contour is not geometrically valid.
    #[error(transparent)]
    Geometry(#[from] ContourError),
    /// Corner treatments consume the whole edge between them.
    #[error("corner features leave no straight edge on edge {edge}")]
    FeatureOverlap {
        /// The edge whose clear length is not positive.
        edge: EdgeId,
    },
    /// A bow is deeper than half of its clear chord.
    #[error("bow on edge {edge} is too deep for its clear edge")]
    BowTooDeep {
        /// The bowed edge.
        edge: EdgeId,
    },
    /// A corner treatment produced endpoints that do not meet its neighbours.
    #[error("corner feature geometry is inconsistent")]
    FeatureGeometry,
    /// The contour has no finite bounds.
    #[error("panel contour bounds are not finite")]
    InvalidBounds,
}

impl Panel {
    /// Builds the nominal outline in panel-local millimetres.
    ///
    /// Positive bow sagitta is outward. The outline is counter-clockwise and its bounds are the
    /// nominal rectangle extended by outward bows; a radius or chamfer does not move the rectangle
    /// bounds unless it consumes a whole side, which is rejected.
    pub fn nominal_contour(&self) -> Result<Contour, PanelContourError> {
        self.nominal_contour_for(self.nominal_size())
    }

    /// Builds the nominal outline for a candidate size, used by resize validation.
    pub fn nominal_contour_for(&self, nominal_size: PanelNominalSize) -> Result<Contour, PanelContourError> {
        let length = nominal_size.length().millimetres_f64();
        let width = nominal_size.width().millimetres_f64();
        let corners =
            [DVec2::new(0.0, 0.0), DVec2::new(length, 0.0), DVec2::new(length, width), DVec2::new(0.0, width)];
        let directions = [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y];
        let edge_lengths = [length, width, length, width];

        let mut segments = Vec::with_capacity(8);
        for index in 0..4 {
            let edge = RectEdge::ALL[index];
            let edge_id = edge.id();
            let start_corner = RectCorner::ALL[index];
            let end_corner = RectCorner::ALL[(index + 1) % 4];
            let direction = directions[index];
            let start_setback = self
                .corner_feature(start_corner.id())
                .map(|feature| outgoing_setback(feature.feature()))
                .unwrap_or(0.0);
            let end_setback =
                self.corner_feature(end_corner.id()).map(|feature| incoming_setback(feature.feature())).unwrap_or(0.0);
            let clear = edge_lengths[index] - start_setback - end_setback;
            if !clear.is_finite() || clear <= 1.0e-6 {
                return Err(PanelContourError::FeatureOverlap { edge: edge_id });
            }

            let start = corners[index] + direction * start_setback;
            let end = corners[(index + 1) % 4] - direction * end_setback;
            let edge_segment = match self.edge_bow(edge_id) {
                Some(bow) => {
                    // The contour is counter-clockwise, so its left side is the material side.
                    // A positive document sagitta bows outward, which is to the right of travel.
                    let sagitta = -bow.sagitta().millimetres_f64();
                    match Arc::from_endpoints_sagitta(start, end, sagitta) {
                        Ok(arc) => Segment::Arc(arc),
                        Err(ContourError::SagittaTooDeep) => {
                            return Err(PanelContourError::BowTooDeep { edge: edge_id });
                        }
                        Err(error) => return Err(PanelContourError::Geometry(error)),
                    }
                }
                None => Segment::Line(Line::new(start, end)?),
            };
            segments.push(edge_segment);

            if let Some(feature) = self.corner_feature(end_corner.id()) {
                segments.push(corner_segment(
                    feature,
                    end,
                    corners[(index + 1) % 4],
                    directions[index],
                    directions[(index + 1) % 4],
                )?);
            }
        }

        let contour = Contour::new(segments, true)?;
        if contour.is_counter_clockwise() != Some(true) {
            return Err(PanelContourError::FeatureGeometry);
        }
        Ok(contour)
    }
}

fn corner_segment(
    feature: CornerFeature,
    incoming_point: DVec2,
    corner: DVec2,
    incoming_direction: DVec2,
    outgoing_direction: DVec2,
) -> Result<Segment, PanelContourError> {
    match feature.feature() {
        CornerFeatureKind::Radius { radius } => {
            let radius = radius.millimetres_f64();
            let outgoing_point = corner + outgoing_direction * radius;
            let left = DVec2::new(-incoming_direction.y, incoming_direction.x);
            // For a counter-clockwise 90-degree corner the arc center is one radius inside the
            // incoming edge from its tangent point.
            let center = incoming_point + left * radius;
            let radial = incoming_point - center;
            let start_angle = radial.y.atan2(radial.x);
            let arc = Arc::new(center, radius, start_angle, FRAC_PI_2)?;
            if (arc.end() - outgoing_point).length() > 1.0e-6 {
                return Err(PanelContourError::FeatureGeometry);
            }
            Ok(Segment::Arc(arc))
        }
        CornerFeatureKind::Chamfer { outgoing, .. } => {
            let outgoing_point = corner + outgoing_direction * outgoing.millimetres_f64();
            Ok(Segment::Line(Line::new(incoming_point, outgoing_point)?))
        }
    }
}

fn outgoing_setback(feature: CornerFeatureKind) -> f64 {
    match feature {
        CornerFeatureKind::Radius { radius } => radius.millimetres_f64(),
        CornerFeatureKind::Chamfer { outgoing, .. } => outgoing.millimetres_f64(),
    }
}

fn incoming_setback(feature: CornerFeatureKind) -> f64 {
    match feature {
        CornerFeatureKind::Radius { radius } => radius.millimetres_f64(),
        CornerFeatureKind::Chamfer { incoming, .. } => incoming.millimetres_f64(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{EdgeBow, Id, PanelNominalSize, Placement, RectCorner, RectEdge};
    use furnicraft_units::Length;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn panel(length: i64, width: i64) -> Panel {
        Panel::new(
            Id::new(7).expect("panel id"),
            "panel",
            PanelNominalSize::new(millimetres(length), millimetres(width)).expect("nominal size"),
            None,
            millimetres(18),
            Placement::IDENTITY,
        )
        .expect("panel")
    }

    fn area_after_radius(panel: &Panel) -> f64 {
        let contour = panel.nominal_contour().expect("contour");
        contour.signed_area().expect("area")
    }

    #[test]
    fn plain_panel_is_the_nominal_rectangle() {
        let panel = panel(100, 60);
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 4);
        assert_eq!(contour.signed_area().expect("area"), 6000.0);
        assert_eq!(contour.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 60.0))));
    }

    #[test]
    fn radius_replaces_one_corner_and_keeps_tangency() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(10) })
                .expect("radius"),
        );
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 5);
        assert_eq!(contour.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 60.0))));
        let area = area_after_radius(&panel);
        let expected = 6000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 100.0;
        assert!((area - expected).abs() < 1e-9, "area {area}, expected {expected}");
    }

    #[test]
    fn chamfer_uses_independent_setbacks() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(
                RectCorner::BottomRight.id(),
                CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(5) },
            )
            .expect("chamfer"),
        );
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 5);
        let expected = 6000.0 - 0.5 * 20.0 * 5.0;
        assert!((contour.signed_area().expect("area") - expected).abs() < 1e-9);
    }

    #[test]
    fn outward_bow_extends_the_bounds_and_inward_bow_does_not() {
        let mut outward = panel(100, 60);
        outward.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(10)).expect("bow"));
        let (min, max) = outward.nominal_contour().expect("contour").bounds().expect("bounds");
        assert!((min.y + 10.0).abs() < 1e-9, "min {min:?}");
        assert!((max.y - 60.0).abs() < 1e-9, "max {max:?}");

        let mut inward = panel(100, 60);
        inward.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(-10)).expect("bow"));
        let (min, _) = inward.nominal_contour().expect("contour").bounds().expect("bounds");
        assert!(min.y.abs() < 1e-9, "min {min:?}");
    }

    #[test]
    fn corner_features_that_consume_an_edge_are_rejected() {
        let mut panel = panel(100, 60);
        for corner in [RectCorner::BottomLeft, RectCorner::BottomRight] {
            panel.corner_features.push(
                CornerFeature::new(corner.id(), CornerFeatureKind::Radius { radius: millimetres(50) }).expect("radius"),
            );
        }
        assert!(matches!(
            panel.nominal_contour(),
            Err(PanelContourError::FeatureOverlap { edge }) if edge == RectEdge::Bottom.id()
        ));
    }

    #[test]
    fn too_deep_bow_is_rejected() {
        let mut panel = panel(100, 60);
        panel.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(51)).expect("bow"));
        assert!(matches!(
            panel.nominal_contour(),
            Err(PanelContourError::BowTooDeep { edge }) if edge == RectEdge::Bottom.id()
        ));
    }
}
