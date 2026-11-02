//! Queries over the node tree: structural kind, furniture type subtree and property equality.
//!
//! The same query feeds selection, reports and, later, CNC rules, so a report and a machining
//! program cannot disagree about which nodes match.

use serde::{Deserialize, Serialize};

use crate::{FurnType, PropertyValue};

/// The structural kind a [`NodeQuery`] filters on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralKind {
    /// A flat panel leaf.
    Panel,
    /// A container, whether an ordinary group or a furniture component.
    Container,
    /// A free-form geometry object.
    Object,
    /// An authored camera.
    Camera,
    /// An authored light.
    Light,
}

/// A property predicate: the named property must equal the given value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyPredicate {
    key: String,
    value: PropertyValue,
}

impl PropertyPredicate {
    /// Creates an equality predicate for one property.
    pub fn new(key: impl Into<String>, value: PropertyValue) -> Self {
        Self { key: key.into(), value }
    }

    /// The property key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The required value.
    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }
}

/// A query over the document tree: structural kind, furniture type subtree and property equality.
///
/// The same query feeds selection, reports and, later, CNC rules, so a report and a machining
/// program cannot disagree about which nodes match. An empty query matches every node below the
/// root.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeQuery {
    #[serde(default)]
    kind: Option<StructuralKind>,
    #[serde(default)]
    furntype: Option<FurnType>,
    #[serde(default)]
    property: Option<PropertyPredicate>,
}

impl NodeQuery {
    /// A query that matches every node below the root.
    pub const fn any() -> Self {
        Self { kind: None, furntype: None, property: None }
    }

    /// Restricts the query to one structural kind.
    pub const fn with_kind(mut self, kind: StructuralKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Restricts the query to a furniture type and its descendants.
    pub const fn with_furntype_under(mut self, furntype: FurnType) -> Self {
        self.furntype = Some(furntype);
        self
    }

    /// Adds a property equality predicate.
    pub fn with_property(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.property = Some(PropertyPredicate::new(key, value));
        self
    }

    /// Whether the query has no filters and matches every node.
    pub const fn is_empty(&self) -> bool {
        self.kind.is_none() && self.furntype.is_none() && self.property.is_none()
    }

    /// The structural kind filter, if any.
    pub const fn kind(&self) -> Option<StructuralKind> {
        self.kind
    }

    /// The furniture type filter, if any.
    pub const fn furntype(&self) -> Option<FurnType> {
        self.furntype
    }

    /// The property predicate, if any.
    pub const fn property(&self) -> Option<&PropertyPredicate> {
        self.property.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_has_no_filters() {
        let query = NodeQuery::any();
        assert!(query.is_empty());
        assert_eq!(query.kind(), None);
        assert_eq!(query.furntype(), None);
        assert!(query.property().is_none());
    }

    #[test]
    fn a_query_keeps_the_filters_it_was_given_and_round_trips() {
        let query = NodeQuery::any()
            .with_kind(StructuralKind::Panel)
            .with_furntype_under("12.03".parse().expect("furniture type"))
            .with_property("width", PropertyValue::integer(600));
        assert!(!query.is_empty());
        assert_eq!(query.kind(), Some(StructuralKind::Panel));
        assert_eq!(query.furntype().map(|value| value.value()), Some(120_300));
        assert_eq!(query.property().map(|predicate| predicate.key()), Some("width"));

        let text = serde_json::to_string(&query).expect("serialize query");
        assert_eq!(serde_json::from_str::<NodeQuery>(&text).expect("load query"), query);
    }
}
