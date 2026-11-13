//! Authored light objects: a sun or a point light with a colour and an intensity.
//!
//! The built-in key light is a default, not a node. A light the user adds is document data with
//! undo, like a camera ([ADR 0036]).
//!
//! [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Id, PivotLocal, Placement, Properties, PropertyError, PropertyValue};

/// The kinds of light a scene can carry.
///
/// A sun lights the whole scene from a direction; a point light radiates from its placement. The
/// built-in key light is a default, not a node ([ADR 0036]).
///
/// [ADR 0036]: ../docs/decisions/0036-cameras-and-lights-are-scene-objects.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    /// A directional light, like the sun.
    Sun,
    /// A light radiating from a point.
    Point,
}

/// A light the user authored.
///
/// Like a camera it is a scene object with a parent, placement, properties, selection and undo. The
/// built-in key light is used when the scene has no light node.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LightNode {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default)]
    pivot_local: PivotLocal,
    #[serde(default)]
    properties: Properties,
    kind: LightKind,
    color: [f32; 3],
    intensity: f32,
}

impl LightNode {
    /// Largest accepted intensity.
    pub const MAX_INTENSITY: f32 = 1_000_000.0;

    /// Creates a light after validating its colour and intensity.
    pub fn new(
        id: Id,
        name: impl Into<String>,
        placement: Placement,
        kind: LightKind,
        color: [f32; 3],
        intensity: f32,
    ) -> Result<Self, LightError> {
        if !valid_light_color(color) {
            return Err(LightError::InvalidColor);
        }
        if !valid_light_intensity(intensity) {
            return Err(LightError::InvalidIntensity);
        }
        Ok(Self {
            id,
            name: name.into(),
            placement,
            pivot_local: PivotLocal::ZERO,
            properties: Properties::new(),
            kind,
            color,
            intensity,
        })
    }

    /// The stable light id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The placement relative to the parent frame.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The manipulation pivot in the light's own frame.
    pub const fn pivot_local(&self) -> PivotLocal {
        self.pivot_local
    }

    /// The light kind.
    pub const fn kind(&self) -> LightKind {
        self.kind
    }

    /// The linear RGB colour.
    pub const fn color(&self) -> [f32; 3] {
        self.color
    }

    /// The intensity multiplier.
    pub const fn intensity(&self) -> f32 {
        self.intensity
    }

    /// Replaces the kind, colour and intensity after validating the colour and intensity.
    pub fn set_params(&mut self, kind: LightKind, color: [f32; 3], intensity: f32) -> Result<(), LightError> {
        if !valid_light_color(color) {
            return Err(LightError::InvalidColor);
        }
        if !valid_light_intensity(intensity) {
            return Err(LightError::InvalidIntensity);
        }
        self.kind = kind;
        self.color = color;
        self.intensity = intensity;
        Ok(())
    }

    /// Replaces the user-visible name; the caller has already rejected a blank one.
    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Replaces the placement relative to the parent container.
    pub(crate) fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    /// Replaces the stored manipulation pivot.
    pub(crate) fn set_pivot(&mut self, pivot_local: PivotLocal) {
        self.pivot_local = pivot_local;
    }

    /// Replaces the stable id when a subtree is copied.
    pub(crate) fn set_node_id(&mut self, id: Id) {
        self.id = id;
    }

    /// The typed properties of this light.
    pub const fn properties(&self) -> &Properties {
        &self.properties
    }

    /// Stores one validated property.
    pub fn set_property(&mut self, key: &str, value: PropertyValue) -> Result<(), PropertyError> {
        self.properties.insert(key, value).map(|_| ())
    }

    /// Removes one property; returns whether it was present.
    pub fn clear_property(&mut self, key: &str) -> bool {
        self.properties.remove(key).is_some()
    }
}

pub(crate) fn valid_light_color(color: [f32; 3]) -> bool {
    color.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

pub(crate) fn valid_light_intensity(intensity: f32) -> bool {
    intensity.is_finite() && (0.0..=LightNode::MAX_INTENSITY).contains(&intensity)
}

/// A serialization failure of a light node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum LightError {
    /// A colour component is not finite or outside `0.0..=1.0`.
    #[error("light colour must be finite and inside the unit range")]
    InvalidColor,
    /// The intensity is not finite or outside its range.
    #[error("light intensity must be finite and non-negative")]
    InvalidIntensity,
}

/// Serialized form of [`LightNode`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LightNodeWire {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default)]
    pivot_local: PivotLocal,
    #[serde(default)]
    properties: Properties,
    kind: LightKind,
    color: [f32; 3],
    intensity: f32,
}

impl TryFrom<LightNodeWire> for LightNode {
    type Error = LightError;

    fn try_from(wire: LightNodeWire) -> Result<Self, Self::Error> {
        if !valid_light_color(wire.color) {
            return Err(LightError::InvalidColor);
        }
        if !valid_light_intensity(wire.intensity) {
            return Err(LightError::InvalidIntensity);
        }
        Ok(Self {
            id: wire.id,
            name: wire.name,
            placement: wire.placement,
            pivot_local: wire.pivot_local,
            properties: wire.properties,
            kind: wire.kind,
            color: wire.color,
            intensity: wire.intensity,
        })
    }
}

impl<'de> Deserialize<'de> for LightNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        LightNodeWire::deserialize(deserializer)?.try_into().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn a_light_colour_or_intensity_outside_its_range_is_rejected_on_build_and_load() {
        let id = Id::new(6).expect("id");
        assert_eq!(
            LightNode::new(id, "Key", Placement::IDENTITY, LightKind::Point, [1.5, 0.0, 0.0], 1.0),
            Err(LightError::InvalidColor)
        );
        assert_eq!(
            LightNode::new(id, "Key", Placement::IDENTITY, LightKind::Point, [1.0, 1.0, 1.0], f32::NAN),
            Err(LightError::InvalidIntensity)
        );
        assert_eq!(
            LightNode::new(id, "Key", Placement::IDENTITY, LightKind::Point, [0.0; 3], -1.0),
            Err(LightError::InvalidIntensity)
        );

        let light = LightNode::new(id, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("light");
        let text = serde_json::to_string(&light).expect("serialize");
        let too_bright = text.replace("\"intensity\":1.0", "\"intensity\":-2.0");
        assert_ne!(too_bright, text);
        assert!(serde_json::from_str::<LightNode>(&too_bright).is_err());
    }
}
