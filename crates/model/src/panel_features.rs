//! Contour features on a rectangular panel: corner treatments and bowed edges.
//!
//! A feature is document data. It describes the nominal outline, not the resolved core or the
//! banding solid. Coordinates and distances are in panel-local millimetres once the geometry layer
//! converts them; this module stores validated integer lengths.

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};

use crate::{EdgeId, Panel, PanelDimensionError, VertexId};

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

fn validate_feature_dimension(field: &'static str, value: Length) -> Result<(), PanelDimensionError> {
    if !value.is_positive() {
        return Err(PanelDimensionError::NonPositive { field, value });
    }
    if value > Panel::MAX_LENGTH {
        return Err(PanelDimensionError::TooLarge { field, value, max: Panel::MAX_LENGTH });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{RectCorner, RectEdge};
    use serde_json::json;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

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
}
