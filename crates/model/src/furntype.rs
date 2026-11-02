//! The numeric furniture type: a strict `GG.TT.VV` tree of group, type and variant.
//!
//! The stored form is one integer code; the dotted form is a presentation produced by
//! [`fmt::Display`] and accepted by [`FromStr`]. Zero means "unclassified" and is a sentinel, not a
//! group.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// The largest furniture type code: two digits for group, type and variant.
pub const FURNTYPE_MAX_CODE: u32 = 999_999;

/// Why a furniture type code or its text form is not usable.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FurnTypeError {
    /// The numeric code is outside the supported range.
    #[error("furniture type {value} is outside the supported range 0 to {max}")]
    OutOfRange {
        /// The rejected code.
        value: u32,
        /// The largest supported code.
        max: u32,
    },
    /// The text form is not a valid code.
    #[error("furniture type `{text}` is not a valid code")]
    Invalid {
        /// The rejected text.
        text: String,
    },
}

/// A numeric furniture type in the strict `GG.TT.VV` tree.
///
/// The value is the stored integer code, from `0` (unclassified) to [`FurnType::MAX`]. The dotted
/// form is a presentation only: it is produced by [`fmt::Display`] and accepted by `FromStr`, never
/// stored. A zero suffix means "the type itself, without a variant".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FurnType(u32);

impl FurnType {
    /// The unclassified type, `000000`.
    pub const UNCLASSIFIED: Self = Self(0);

    /// The largest assignable furniture type.
    pub const MAX: Self = Self(999_999);

    /// Builds a type from its numeric code, or `None` when the code is not a valid tree position.
    ///
    /// A valid code has a non-zero group; a variant requires a type. Zero is
    /// [`FurnType::UNCLASSIFIED`].
    pub const fn from_code(code: u32) -> Option<Self> {
        if code == 0 {
            return Some(Self(0));
        }
        let group = code / 10_000;
        let type_ = (code / 100) % 100;
        let variant = code % 100;
        if code <= 999_999 && group != 0 && (variant == 0 || type_ != 0) { Some(Self(code)) } else { None }
    }

    /// The numeric code without separators, the only stored form.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Whether the type is unclassified.
    pub const fn is_unclassified(self) -> bool {
        self.0 == 0
    }

    /// The parent type: the current type without its variant, or the group without its type.
    pub const fn parent(self) -> Self {
        let variant = self.0 % 100;
        let type_ = (self.0 / 100) % 100;
        if variant != 0 {
            Self(self.0 - variant)
        } else if type_ != 0 {
            Self(self.0 - type_ * 100)
        } else {
            Self(0)
        }
    }

    /// Whether `ancestor` is the type itself or one of its ancestors.
    ///
    /// `000000` is a sentinel, not a group: it matches only unclassified types, so a report can ask
    /// for "everything without a type" instead of matching the whole tree.
    pub fn is_ancestor_or_self(self, ancestor: Self) -> bool {
        if ancestor.is_unclassified() {
            return self.is_unclassified();
        }
        let mut current = self;
        loop {
            if current == ancestor {
                return true;
            }
            if current.is_unclassified() {
                return false;
            }
            current = current.parent();
        }
    }
}

impl fmt::Display for FurnType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let group = self.0 / 10_000;
        let type_ = (self.0 / 100) % 100;
        let variant = self.0 % 100;
        if variant != 0 {
            write!(formatter, "{group}.{:02}.{:02}", type_, variant)
        } else if type_ != 0 {
            write!(formatter, "{group}.{:02}", type_)
        } else {
            write!(formatter, "{group}")
        }
    }
}

impl FromStr for FurnType {
    type Err = FurnTypeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let invalid = || FurnTypeError::Invalid { text: text.to_owned() };
        if text.is_empty() {
            return Err(invalid());
        }
        if !text.contains('.') {
            let raw = text.parse::<u32>().map_err(|_| invalid())?;
            // One or two digits name a group, the display form of a group code; a longer run is a
            // raw integer code. Both round-trip through [`fmt::Display`].
            let code = if text.len() <= 2 { raw * 10_000 } else { raw };
            return FurnType::from_code(code).ok_or(FurnTypeError::OutOfRange { value: code, max: FURNTYPE_MAX_CODE });
        }
        let mut parts = text.split('.');
        let group_text = parts.next().ok_or_else(invalid)?;
        let type_text = parts.next();
        let variant_text = parts.next();
        if parts.next().is_some() {
            return Err(invalid());
        }
        let parse_part = |part: &str, digits: usize| -> Result<u32, FurnTypeError> {
            if part.is_empty() || part.len() > digits || !part.chars().all(|c| c.is_ascii_digit()) {
                return Err(invalid());
            }
            let mut value = 0u32;
            for c in part.chars() {
                value = value * 10 + u32::from(c as u8 - b'0');
            }
            Ok(value)
        };
        let group = parse_part(group_text, 2)?;
        if group == 0 {
            return Err(invalid());
        }
        let type_ = match type_text {
            Some(part) => parse_part(part, 2)?,
            None => 0,
        };
        let variant = match variant_text {
            Some(part) => parse_part(part, 2)?,
            None => 0,
        };
        let code = group * 10_000 + type_ * 100 + variant;
        FurnType::from_code(code).ok_or(FurnTypeError::OutOfRange { value: code, max: FURNTYPE_MAX_CODE })
    }
}

impl Serialize for FurnType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for FurnType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = u32::deserialize(deserializer)?;
        FurnType::from_code(code).ok_or_else(|| {
            de::Error::custom(format_args!(
                "furniture type {code} is outside the supported range 0 to {FURNTYPE_MAX_CODE}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fixtures::furntype;

    #[test]
    fn furniture_types_encode_three_levels_and_parents() {
        let cabinet = furntype("12.03.02");
        assert_eq!(cabinet.value(), 120_302);
        assert_eq!(cabinet.to_string(), "12.03.02");
        assert_eq!(cabinet.parent(), furntype("12.03"));
        assert_eq!(cabinet.parent().parent(), furntype("12"));
        assert_eq!(cabinet.parent().parent().parent(), FurnType::UNCLASSIFIED);
        assert!(cabinet.is_ancestor_or_self(cabinet));
        assert!(cabinet.is_ancestor_or_self(furntype("12")));
        assert!(!furntype("12").is_ancestor_or_self(cabinet));
        assert!(!cabinet.is_ancestor_or_self(FurnType::UNCLASSIFIED));
        assert!(FurnType::UNCLASSIFIED.is_ancestor_or_self(FurnType::UNCLASSIFIED));
        assert_eq!(furntype("12").value(), 120_000);
        assert_eq!(format!("{}", furntype("12")), "12");
        assert_eq!(format!("{}", FurnType::UNCLASSIFIED), "0");
    }

    #[test]
    fn furniture_types_reject_bad_text_and_out_of_range_codes() {
        assert_eq!(FurnType::from_code(1_000_000), None);
        assert_eq!(FurnType::from_code(FURNTYPE_MAX_CODE), Some(FurnType::MAX));
        assert!("".parse::<FurnType>().is_err());
        assert!("0.0.0".parse::<FurnType>().is_err());
        assert!("12.3.4.5".parse::<FurnType>().is_err());
        assert!("12.123".parse::<FurnType>().is_err());
        assert!("twelve".parse::<FurnType>().is_err());
        assert_eq!("120302".parse::<FurnType>().expect("bare code"), furntype("12.03.02"));
        assert_eq!("12.03".parse::<FurnType>().expect("type form"), furntype("12.03"));
    }

    #[test]
    fn furniture_types_serialize_as_integers() {
        let value = serde_json::to_value(furntype("12.03.02")).expect("serialize type");
        assert_eq!(value, serde_json::json!(120_302));
        let decoded: FurnType = serde_json::from_value(value).expect("deserialize type");
        assert_eq!(decoded, furntype("12.03.02"));
        assert!(serde_json::from_value::<FurnType>(serde_json::json!(1_000_000)).is_err());
    }
}
