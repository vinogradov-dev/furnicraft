//! Typed properties on a node and the reserved system keys.
//!
//! Physical values use the domain units; a bare number is never a property value. A node's
//! properties are an ordered map, and the reserved system keys are validated on every write and on
//! every load, so a component marker or a prototype record cannot be coerced into the wrong kind.

use std::collections::BTreeMap;

use furnicraft_units::{Degrees, Length};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{FurnType, Id};

/// The reserved system property keys of a node (ADR 0034).
pub const SYSTEM_COMPONENT: &str = "component";
/// The furniture type of a node; a non-zero value marks a container as a component (ADR 0035).
pub const SYSTEM_FURNTYPE: &str = "furntype";
/// The prototype a component was created from, when a user script exists.
pub const SYSTEM_PROTOTYPE_ID: &str = "prototype.id";
/// The version of the prototype parameters.
pub const SYSTEM_PROTOTYPE_VERSION: &str = "prototype.version";
/// The nested prototype parameter record, as a script wrote it.
pub const SYSTEM_PROTOTYPE_PARAMETERS: &str = "prototype.parameters";

/// A typed property value attached to a node.
///
/// Physical values use the domain units; a bare number is never a property value. `Structured` is
/// the escape hatch for nested prototype parameters, which are free-form by design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropertyValue {
    /// A boolean flag.
    Bool {
        /// The flag value.
        value: bool,
    },
    /// A signed integer count.
    Integer {
        /// The integer value.
        value: i64,
    },
    /// A physical length in micrometres.
    Length {
        /// The length value.
        value: Length,
    },
    /// An angle in millidegrees.
    Angle {
        /// The angle value.
        value: Degrees,
    },
    /// User-visible text.
    Text {
        /// The text value.
        value: String,
    },
    /// A choice between labelled options.
    Choice {
        /// The stable option key.
        key: String,
        /// The user-visible option label.
        label: String,
    },
    /// A reference to another document id.
    Reference {
        /// The referenced id.
        id: Id,
    },
    /// A furniture type code.
    #[serde(rename = "furntype")]
    FurnType {
        /// The furniture type.
        value: FurnType,
    },
    /// A nested, free-form structured record, usually prototype parameters.
    Structured {
        /// The nested value.
        value: Value,
    },
}

impl PropertyValue {
    /// Builds a boolean property value.
    pub const fn bool(value: bool) -> Self {
        Self::Bool { value }
    }

    /// Builds an integer property value.
    pub const fn integer(value: i64) -> Self {
        Self::Integer { value }
    }

    /// Builds a length property value.
    pub const fn length(value: Length) -> Self {
        Self::Length { value }
    }

    /// Builds an angle property value.
    pub const fn angle(value: Degrees) -> Self {
        Self::Angle { value }
    }

    /// Builds a text property value.
    pub fn text(value: &str) -> Self {
        Self::Text { value: value.to_owned() }
    }

    /// Builds a choice property value.
    pub fn choice(key: &str, label: &str) -> Self {
        Self::Choice { key: key.to_owned(), label: label.to_owned() }
    }

    /// Builds a reference property value.
    pub const fn reference(id: Id) -> Self {
        Self::Reference { id }
    }

    /// Builds a furniture type property value.
    pub const fn furn_type(value: FurnType) -> Self {
        Self::FurnType { value }
    }

    /// Builds a structured property value.
    pub const fn structured(value: Value) -> Self {
        Self::Structured { value }
    }

    /// The property kind name used in serialized form and diagnostics.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Bool { .. } => "bool",
            Self::Integer { .. } => "integer",
            Self::Length { .. } => "length",
            Self::Angle { .. } => "angle",
            Self::Text { .. } => "text",
            Self::Choice { .. } => "choice",
            Self::Reference { .. } => "reference",
            Self::FurnType { .. } => "furntype",
            Self::Structured { .. } => "structured",
        }
    }
}

/// An ordered map of property keys to typed values on one node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Properties(BTreeMap<String, PropertyValue>);

impl<'de> Deserialize<'de> for Properties {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let stored = BTreeMap::<String, PropertyValue>::deserialize(deserializer)?;
        for (key, value) in &stored {
            validate_property_value(key, value).map_err(de::Error::custom)?;
        }
        Ok(Self(stored))
    }
}

impl Properties {
    /// Creates an empty property bag.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// The value stored under `key`, or `None` when the key is unknown.
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.0.get(key)
    }

    /// The mutable value stored under `key`, or `None` when the key is unknown.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut PropertyValue> {
        self.0.get_mut(key)
    }

    /// Whether any property is stored.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Stores or replaces the value under `key`, returning the previous value.
    ///
    /// Reserved system keys are validated here: a reserved key with the wrong value type is
    /// rejected instead of coerced.
    pub fn insert(&mut self, key: &str, value: PropertyValue) -> Result<Option<PropertyValue>, PropertyError> {
        validate_property_value(key, &value)?;
        Ok(self.0.insert(key.to_owned(), value))
    }

    /// Removes the value under `key`, returning it when present.
    pub fn remove(&mut self, key: &str) -> Option<PropertyValue> {
        self.0.remove(key)
    }

    /// Removes every property.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The stored keys in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The stored entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }
}

/// Why a property key or value is not valid for the document.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum PropertyError {
    /// The key or value violates the document property rules.
    #[error("invalid property `{key}`: {message}")]
    Invalid {
        /// The rejected property key.
        key: String,
        /// A human-readable reason.
        message: String,
    },
}

/// Validates a property value against the reserved system keys.
pub(crate) fn validate_property_value(key: &str, value: &PropertyValue) -> Result<(), PropertyError> {
    match key {
        SYSTEM_COMPONENT => match value {
            PropertyValue::Bool { .. } => Ok(()),
            _ => Err(PropertyError::Invalid { key: key.to_owned(), message: "must be a boolean".to_owned() }),
        },
        SYSTEM_FURNTYPE => match value {
            PropertyValue::FurnType { .. } => Ok(()),
            _ => Err(PropertyError::Invalid { key: key.to_owned(), message: "must be a furniture type".to_owned() }),
        },
        SYSTEM_PROTOTYPE_ID => match value {
            PropertyValue::Reference { .. } => Ok(()),
            _ => {
                Err(PropertyError::Invalid { key: key.to_owned(), message: "must be a prototype reference".to_owned() })
            }
        },
        SYSTEM_PROTOTYPE_VERSION => match value {
            PropertyValue::Integer { .. } => Ok(()),
            _ => Err(PropertyError::Invalid {
                key: key.to_owned(),
                message: "must be an integer prototype version".to_owned(),
            }),
        },
        SYSTEM_PROTOTYPE_PARAMETERS => match value {
            PropertyValue::Structured { .. } => Ok(()),
            _ => Err(PropertyError::Invalid {
                key: key.to_owned(),
                message: "must be a structured prototype parameter record".to_owned(),
            }),
        },
        _ => Ok(()),
    }
}

/// One property change applied to one node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyChange {
    /// The node that receives the value.
    pub node: Id,
    /// The property key.
    pub key: String,
    /// The new typed value.
    pub value: PropertyValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fixtures::{furntype, millimetres};

    #[test]
    fn property_values_round_trip_by_kind() {
        let values = [
            PropertyValue::bool(true),
            PropertyValue::integer(-7),
            PropertyValue::length(millimetres(600)),
            PropertyValue::angle(Degrees::from_millidegrees(45_000)),
            PropertyValue::text("oak"),
            PropertyValue::choice("left", "Left"),
            PropertyValue::reference(Id::new(9).expect("id")),
            PropertyValue::furn_type(furntype("12.03")),
            PropertyValue::structured(serde_json::json!({"width": 600})),
        ];
        for value in values {
            let json = serde_json::to_value(&value).expect("serialize property");
            assert_eq!(json["kind"], value.kind());
            let decoded: PropertyValue = serde_json::from_value(json).expect("deserialize property");
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn reserved_property_keys_reject_the_wrong_value_kind() {
        assert!(SYSTEM_COMPONENT.starts_with("component"));
        for key in [
            SYSTEM_COMPONENT,
            SYSTEM_FURNTYPE,
            SYSTEM_PROTOTYPE_ID,
            SYSTEM_PROTOTYPE_VERSION,
            SYSTEM_PROTOTYPE_PARAMETERS,
        ] {
            assert!(Properties::new().insert(key, PropertyValue::text("wrong")).is_err(), "{key} accepted text");
        }
        let mut properties = Properties::new();
        properties.insert(SYSTEM_COMPONENT, PropertyValue::bool(false)).expect("component flag");
        properties.insert(SYSTEM_FURNTYPE, PropertyValue::furn_type(furntype("12"))).expect("furntype");
        properties
            .insert(SYSTEM_PROTOTYPE_ID, PropertyValue::reference(Id::new(5).expect("id")))
            .expect("prototype id");
        properties.insert(SYSTEM_PROTOTYPE_VERSION, PropertyValue::integer(1)).expect("prototype version");
        properties
            .insert(SYSTEM_PROTOTYPE_PARAMETERS, PropertyValue::structured(serde_json::json!({})))
            .expect("prototype parameters");
        assert_eq!(properties.keys().count(), 5);
    }
}
