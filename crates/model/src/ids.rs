//! Stable document identifiers and the revision counter.
//!
//! Ids are non-zero and never reused while a document is edited, so commands, hardware references
//! and the undo stack refer to a container, panel or catalogue entry without holding a pointer.
//! The small typed ids ([`VertexId`], [`EdgeId`], [`ContourSegmentId`]) label positions inside one
//! panel or ring and share the same non-zero rule.

use std::fmt;
use std::num::NonZeroU64;

use serde::{Deserialize, Deserializer, Serialize};

/// A stable document identifier.
///
/// Identifiers are never reused while a document is edited, so commands, hardware references and
/// the undo stack can refer to a container, panel or catalogue entry without holding a pointer.
/// Zero is reserved for "no entity" and is rejected on construction and deserialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(NonZeroU64);

impl Id {
    /// Creates an id from a non-zero raw value.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A monotonically increasing document revision.
///
/// The revision changes after every document mutation and keys every derived cache. It starts at
/// zero and never wraps: [`Revision::checked_next`] returns `None` at the end of the range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// The initial revision of an unmodified document.
    pub const ZERO: Self = Self(0);

    /// The raw numeric value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next revision, or `None` if the counter would overflow.
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

macro_rules! stable_id {
    ($(#[$meta:meta])* $name:ident, $description:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates an id from a non-zero value.
            pub const fn new(value: u32) -> Option<Self> {
                if value == 0 { None } else { Some(Self(value)) }
            }

            /// The numeric value.
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Builds an id from a value that is known to be non-zero inside this crate.
            ///
            /// Only the id types with const constructors call this; the others reach it never, which
            /// is why the allow is part of the generated item rather than of one invocation.
            #[allow(dead_code)]
            pub(crate) const fn from_known(value: u32) -> Self {
                Self(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u32::deserialize(deserializer)?;
                Self::new(value)
                    .ok_or_else(|| <D::Error as serde::de::Error>::custom(concat!($description, " must be non-zero")))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0)
            }
        }
    };
}

stable_id!(
    /// A corner id of a rectangular panel outline.
    VertexId,
    "vertex id"
);
stable_id!(
    /// An edge id of a rectangular panel outline.
    EdgeId,
    "edge id"
);
stable_id!(
    /// A stable id of one segment of a constructive contour.
    ///
    /// Ids are unique inside their ring and never reused, so a command can name a segment while the
    /// ring is edited. They are labels, not keys: their numbers carry no order.
    ContourSegmentId,
    "contour segment id"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_non_zero_and_serialize_as_numbers() {
        assert!(Id::new(0).is_none());
        let id = Id::new(42).expect("non-zero id");
        assert_eq!(id.get(), 42);
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "42");
        assert_eq!(serde_json::from_str::<Id>("42").expect("deserialize id"), id);
        assert!(serde_json::from_str::<Id>("0").is_err());
        assert_eq!(std::mem::size_of::<Option<Id>>(), std::mem::size_of::<u64>());
    }

    #[test]
    fn revisions_are_monotonic_and_do_not_wrap() {
        assert_eq!(Revision::ZERO.get(), 0);
        let one = Revision::ZERO.checked_next().expect("first revision");
        assert_eq!(one.get(), 1);
        assert_eq!(one.checked_next().expect("second revision").get(), 2);

        assert_eq!(Revision(u64::MAX).checked_next(), None);
        assert_eq!(serde_json::from_str::<Revision>("7").expect("deserialize revision").get(), 7);
    }

    #[test]
    fn ids_reject_zero_and_serialize_as_numbers() {
        assert!(VertexId::new(0).is_none());
        assert!(EdgeId::new(0).is_none());
        assert!(serde_json::from_str::<VertexId>("0").is_err());
        assert!(serde_json::from_str::<EdgeId>("0").is_err());

        let id = EdgeId::new(3).expect("non-zero edge id");
        assert_eq!(serde_json::to_string(&id).expect("serialize id"), "3");
        let decoded: EdgeId = serde_json::from_str("3").expect("deserialize id");
        assert_eq!(decoded, id);
    }
}
