//! Contour features on a rectangular panel: corner treatments, bowed edges and end bevels.
//!
//! A feature is document data. It describes the nominal outline and the ends cut from it, not the
//! resolved core or the banding solid. Coordinates and distances are in panel-local millimetres once
//! the geometry layer converts them; this module stores validated integer lengths and angles.

use std::collections::HashSet;

use furnicraft_units::{Degrees, Length};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EdgeId, EdgeRef, Panel, PanelDimensionError, PanelError, VertexId};

/// A corner treatment applied to a nominal rectangle corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CornerFeatureKind {
    /// A circular arc tangent to the two edges that meet at the corner.
    Radius {
        /// Radius of the corner arc.
        radius: Length,
    },
    /// A straight cut across the corner.
    ///
    /// `incoming` is the setback along the edge that ends at the corner; `outgoing` is the setback
    /// along the edge that starts there. Different setbacks give a non-45-degree chamfer.
    Chamfer {
        /// Setback along the edge that ends at the corner.
        incoming: Length,
        /// Setback along the edge that starts at the corner.
        outgoing: Length,
    },
}

impl CornerFeatureKind {
    /// Validates the dimensions of the feature.
    pub fn validate(self) -> Result<(), PanelDimensionError> {
        match self {
            Self::Radius { radius } => validate_feature_dimension("corner_radius", radius),
            Self::Chamfer { incoming, outgoing } => {
                validate_feature_dimension("chamfer_incoming", incoming)?;
                validate_feature_dimension("chamfer_outgoing", outgoing)
            }
        }
    }
}

/// One corner treatment addressed by a stable outline vertex id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CornerFeatureWire")]
pub struct CornerFeature {
    corner: VertexId,
    feature: CornerFeatureKind,
}

impl CornerFeature {
    /// Creates a corner feature with validated dimensions.
    pub fn new(corner: VertexId, feature: CornerFeatureKind) -> Result<Self, PanelDimensionError> {
        feature.validate()?;
        Ok(Self { corner, feature })
    }

    /// The nominal outline corner this feature applies to.
    pub const fn corner(self) -> VertexId {
        self.corner
    }

    /// The kind and dimensions of the treatment.
    pub const fn feature(self) -> CornerFeatureKind {
        self.feature
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CornerFeatureWire {
    corner: VertexId,
    feature: CornerFeatureKind,
}

impl TryFrom<CornerFeatureWire> for CornerFeature {
    type Error = PanelDimensionError;

    fn try_from(wire: CornerFeatureWire) -> Result<Self, Self::Error> {
        Self::new(wire.corner, wire.feature)
    }
}

/// A signed bow on one nominal edge.
///
/// A positive sagitta bows outward from the panel, a negative sagitta bows inward. The magnitude is
/// limited to half the clear edge length by the contour builder; the schema only rejects zero and
/// absurdly large values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EdgeBowWire")]
pub struct EdgeBow {
    edge: EdgeId,
    sagitta: Length,
}

impl EdgeBow {
    /// Creates a bow with a non-zero, bounded sagitta.
    pub fn new(edge: EdgeId, sagitta: Length) -> Result<Self, PanelDimensionError> {
        if sagitta.is_zero() {
            return Err(PanelDimensionError::NonPositive { field: "bow_sagitta", value: sagitta });
        }
        let magnitude = if sagitta.is_negative() {
            let micrometres = sagitta.micrometres().checked_neg().ok_or(PanelDimensionError::TooLarge {
                field: "bow_sagitta",
                value: sagitta,
                max: Panel::MAX_LENGTH,
            })?;
            Length::from_micrometres(micrometres)
        } else {
            sagitta
        };
        validate_feature_dimension("bow_sagitta", magnitude)?;
        Ok(Self { edge, sagitta })
    }

    /// The edge this bow replaces.
    pub const fn edge(self) -> EdgeId {
        self.edge
    }

    /// The signed sagitta: positive outward, negative inward.
    pub const fn sagitta(self) -> Length {
        self.sagitta
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBowWire {
    edge: EdgeId,
    sagitta: Length,
}

impl TryFrom<EdgeBowWire> for EdgeBow {
    type Error = PanelDimensionError;

    fn try_from(wire: EdgeBowWire) -> Result<Self, Self::Error> {
        Self::new(wire.edge, wire.sagitta)
    }
}

/// The steepest end bevel this build accepts: one degree short of a square cut turned inside out.
///
/// At ninety degrees the far face's edge would run to infinity, so the cap keeps the setback
/// finite and the record meaningful. The command also refuses an angle whose setback would leave no
/// material at the far face; this constant is only the value the schema can represent.
pub const MAX_BEVEL_ANGLE: Degrees = Degrees::from_millidegrees(89_000);

/// Why an angle is not a usable end bevel.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum BevelError {
    /// The angle is zero or negative: the end stays square.
    #[error("a bevel angle must be positive; {angle} leaves the end square")]
    NotPositive {
        /// The rejected angle.
        angle: Degrees,
    },
    /// The angle is at or past the cap, where the far face has no edge left.
    #[error("a bevel angle of {angle} is too steep; it must stay below {max}")]
    TooSteep {
        /// The rejected angle.
        angle: Degrees,
        /// The steepest accepted angle.
        max: Degrees,
    },
}

/// One end bevel: a signed angle about the line of one outline edge.
///
/// The outline is the edge on face `B` (`z = 0`). The end face tilts across the thickness, so face
/// `A`'s edge is set back by `thickness * tan(angle)` towards the middle of the panel. A positive
/// angle removes material and the body never grows past its outline, which keeps the nominal size
/// its stock size; a negative angle is refused rather than unioned on (N12 chooses the reference
/// face).
///
/// The value is a topology reference plus an angle, never a plane: one angle through the edge line
/// determines the plane exactly, and the edge line supplies the rest of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EdgeBevelWire")]
pub struct EdgeBevel {
    source: EdgeRef,
    angle: Degrees,
}

impl EdgeBevel {
    /// Creates a bevel on one outline edge with an angle inside the accepted band.
    pub fn new(source: EdgeRef, angle: Degrees) -> Result<Self, BevelError> {
        if angle <= Degrees::ZERO {
            return Err(BevelError::NotPositive { angle });
        }
        if angle >= MAX_BEVEL_ANGLE {
            return Err(BevelError::TooSteep { angle, max: MAX_BEVEL_ANGLE });
        }
        Ok(Self { source, angle })
    }

    /// The outline edge this bevel belongs to.
    pub const fn source(self) -> EdgeRef {
        self.source
    }

    /// The angle the end face tilts across the panel thickness.
    pub const fn angle(self) -> Degrees {
        self.angle
    }

    /// The same validated angle on another edge, used when a rectangle outline bakes into a ring.
    pub(crate) const fn on_source(self, source: EdgeRef) -> Self {
        Self { source, angle: self.angle }
    }

    /// How far face `A`'s edge is set back from the outline, in micrometres.
    ///
    /// Returns `None` when the product does not fit a [`Length`]; the cap on the angle keeps that
    /// out of reach for a panel-sized thickness, and the caller still reports it instead of
    /// assuming.
    pub fn setback(self, thickness: Length) -> Option<Length> {
        let millimetres = thickness.millimetres_f64() * self.angle.radians_f64().tan();
        if !millimetres.is_finite() {
            return None;
        }
        let micrometres = (millimetres * 1_000.0).round();
        if !(0.0..=i64::MAX as f64).contains(&micrometres) {
            return None;
        }
        Some(Length::from_micrometres(micrometres as i64))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBevelWire {
    source: EdgeRef,
    angle: Degrees,
}

impl TryFrom<EdgeBevelWire> for EdgeBevel {
    type Error = BevelError;

    fn try_from(wire: EdgeBevelWire) -> Result<Self, Self::Error> {
        Self::new(wire.source, wire.angle)
    }
}

fn validate_feature_dimension(field: &'static str, value: Length) -> Result<(), PanelDimensionError> {
    if !value.is_positive() {
        return Err(PanelDimensionError::NonPositive { field, value });
    }
    if value > Panel::MAX_LENGTH {
        return Err(PanelDimensionError::TooLarge { field, value, max: Panel::MAX_LENGTH });
    }
    Ok(())
}

pub(crate) fn validate_contour_features(features: &[CornerFeature], bows: &[EdgeBow]) -> Result<(), PanelError> {
    let mut corners = HashSet::new();
    for feature in features {
        let corner = feature.corner();
        if !(1..=4).contains(&corner.get()) {
            return Err(PanelError::UnsupportedCornerFeature { corner });
        }
        if !corners.insert(corner.get()) {
            return Err(PanelError::DuplicateCornerFeature { corner });
        }
    }
    let mut edges = HashSet::new();
    for bow in bows {
        let edge = bow.edge();
        if !(1..=4).contains(&edge.get()) {
            return Err(PanelError::UnsupportedEdgeBow { edge });
        }
        if !edges.insert(edge.get()) {
            return Err(PanelError::DuplicateEdgeBow { edge });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::testing::millimetres;

    use super::*;
    use crate::panel::PanelWire;
    use crate::{RectCorner, RectEdge};
    use serde_json::json;

    #[test]
    fn corner_features_round_trip_and_validate() {
        let radius =
            CornerFeature::new(RectCorner::BottomRight.id(), CornerFeatureKind::Radius { radius: millimetres(12) })
                .expect("radius");
        let text = serde_json::to_string(&radius).expect("serialize");
        assert_eq!(serde_json::from_str::<CornerFeature>(&text).expect("deserialize"), radius);

        let chamfer = CornerFeature::new(
            RectCorner::TopLeft.id(),
            CornerFeatureKind::Chamfer { incoming: millimetres(8), outgoing: millimetres(15) },
        )
        .expect("chamfer");
        assert_eq!(
            chamfer.feature(),
            CornerFeatureKind::Chamfer { incoming: millimetres(8), outgoing: millimetres(15) }
        );

        assert!(matches!(
            CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: Length::ZERO }),
            Err(PanelDimensionError::NonPositive { field: "corner_radius", .. })
        ));
        assert!(matches!(
            CornerFeature::new(
                RectCorner::BottomLeft.id(),
                CornerFeatureKind::Chamfer { incoming: Length::ZERO, outgoing: millimetres(10) }
            ),
            Err(PanelDimensionError::NonPositive { field: "chamfer_incoming", .. })
        ));
    }

    #[test]
    fn feature_json_rejects_unknown_fields_and_zero_lengths() {
        let unknown = json!({
            "corner": 1,
            "feature": {"kind": "radius", "radius": 10, "extra": 1}
        });
        assert!(serde_json::from_value::<CornerFeature>(unknown).is_err());

        let zero = json!({
            "corner": 1,
            "feature": {"kind": "radius", "radius": 0}
        });
        assert!(serde_json::from_value::<CornerFeature>(zero).is_err());
    }

    #[test]
    fn bows_round_trip_with_both_signs() {
        let outward = EdgeBow::new(RectEdge::Bottom.id(), millimetres(6)).expect("outward bow");
        let inward = EdgeBow::new(RectEdge::Right.id(), millimetres(-4)).expect("inward bow");
        assert_eq!(outward.sagitta(), millimetres(6));
        assert_eq!(inward.sagitta(), millimetres(-4));

        for bow in [outward, inward] {
            let text = serde_json::to_string(&bow).expect("serialize");
            assert_eq!(serde_json::from_str::<EdgeBow>(&text).expect("deserialize"), bow);
        }

        assert!(matches!(
            EdgeBow::new(RectEdge::Left.id(), Length::ZERO),
            Err(PanelDimensionError::NonPositive { field: "bow_sagitta", .. })
        ));
        assert!(serde_json::from_value::<EdgeBow>(json!({"edge": 1, "sagitta": 0})).is_err());
        assert!(serde_json::from_value::<EdgeBow>(json!({"edge": 1, "sagitta": 10, "extra": 1})).is_err());
    }

    #[test]
    fn an_end_bevel_carries_a_bounded_angle_and_round_trips() {
        let source = EdgeRef::Rect { edge: RectEdge::Right.id() };
        let bevel = EdgeBevel::new(source, Degrees::from_millidegrees(45_000)).expect("a 45 degree bevel");
        assert_eq!(bevel.source(), source);
        assert_eq!(bevel.angle(), Degrees::from_millidegrees(45_000));

        let text = serde_json::to_string(&bevel).expect("serialize");
        assert_eq!(serde_json::from_str::<EdgeBevel>(&text).expect("deserialize"), bevel);
        assert!(text.contains("\"angle\":45000"), "the angle is millidegrees: {text}");

        // The setback is the thickness times the tangent, rounded to the micrometre.
        assert_eq!(bevel.setback(millimetres(18)), Some(millimetres(18)));
        let shallow = EdgeBevel::new(source, Degrees::from_millidegrees(26_565)).expect("a shallow bevel");
        assert_eq!(shallow.setback(millimetres(20)), Some(millimetres(10)));

        for refused in [Degrees::ZERO, Degrees::from_millidegrees(-10_000)] {
            assert_eq!(EdgeBevel::new(source, refused), Err(BevelError::NotPositive { angle: refused }));
        }
        for steep in [MAX_BEVEL_ANGLE, Degrees::from_millidegrees(90_000)] {
            assert_eq!(EdgeBevel::new(source, steep), Err(BevelError::TooSteep { angle: steep, max: MAX_BEVEL_ANGLE }));
        }
        assert!(EdgeBevel::new(source, Degrees::from_millidegrees(88_999)).is_ok(), "just under the cap");

        assert!(
            serde_json::from_value::<EdgeBevel>(json!({
                "source": {"kind": "rect", "edge": 1},
                "angle": 89_000
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<EdgeBevel>(json!({
                "source": {"kind": "rect", "edge": 1},
                "angle": 45_000,
                "extra": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn panel_load_rejects_duplicate_and_out_of_range_contour_features() {
        let base = |corner_features: serde_json::Value, edge_bows: serde_json::Value| {
            serde_json::json!({
                "id": 2,
                "name": "Panel",
                "nominal_size": {"length": 700000, "width": 500000},
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "last_known_thickness": 18000,
                "outline": {"kind": "rect", "corner_features": corner_features, "edge_bows": edge_bows},
            })
        };

        let duplicate = base(
            serde_json::json!([
                {"corner": 1, "feature": {"kind": "radius", "radius": 10000}},
                {"corner": 1, "feature": {"kind": "chamfer", "incoming": 10000, "outgoing": 20000}}
            ]),
            serde_json::json!([]),
        );
        let duplicate: PanelWire = serde_json::from_value(duplicate).expect("panel wire");
        assert_eq!(
            Panel::try_from(duplicate),
            Err(PanelError::DuplicateCornerFeature { corner: RectCorner::BottomLeft.id() })
        );

        let unsupported = base(
            serde_json::json!([{"corner": 5, "feature": {"kind": "radius", "radius": 10000}}]),
            serde_json::json!([]),
        );
        let unsupported: PanelWire = serde_json::from_value(unsupported).expect("panel wire");
        assert_eq!(
            Panel::try_from(unsupported),
            Err(PanelError::UnsupportedCornerFeature { corner: VertexId::new(5).expect("corner") })
        );

        let bad_bow = base(
            serde_json::json!([]),
            serde_json::json!([{"edge": 1, "sagitta": 1000}, {"edge": 1, "sagitta": 2000}]),
        );
        let bad_bow: PanelWire = serde_json::from_value(bad_bow).expect("panel wire");
        assert_eq!(Panel::try_from(bad_bow), Err(PanelError::DuplicateEdgeBow { edge: RectEdge::Bottom.id() }));
    }
}
