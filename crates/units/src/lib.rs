//! Lengths and unit conversion.
//!
//! All persisted dimensions are integer micrometres (`Length`); floating-point millimetres exist only at the geometry and rendering boundary.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A physical length in integer micrometres.
///
/// The value is signed because placements and offsets may be negative; dimensions such as a
/// panel thickness are validated separately where they are used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Length(i64);

impl Length {
    /// Zero length.
    pub const ZERO: Self = Self(0);

    /// Builds a length from an exact number of micrometres.
    pub const fn from_micrometres(micrometres: i64) -> Self {
        Self(micrometres)
    }

    /// Builds a length from whole millimetres, returning `None` on overflow.
    ///
    /// Use [`Length::from_micrometres`] for sub-millimetre precision, or parse user input
    /// through `furnicraft-units` rather than converting floats in the domain.
    pub fn from_millimetres(millimetres: i64) -> Option<Self> {
        millimetres.checked_mul(1_000).map(Self)
    }

    /// The exact value in micrometres.
    pub const fn micrometres(self) -> i64 {
        self.0
    }

    /// The value in millimetres as a float.
    ///
    /// This is a boundary conversion for geometry and rendering only. Never store the result
    /// in the document or in a command parameter.
    pub fn millimetres_f64(self) -> f64 {
        self.0 as f64 / 1_000.0
    }

    /// Parses a length from decimal millimetres, with an optional `mm` suffix.
    ///
    /// The parser works on digits, not floats, so `18.5` becomes exactly 18 500 micrometres and a
    /// value with more than three fractional digits is rejected instead of being rounded. A leading
    /// `+` or `-` is accepted, as are a bare fraction (`.5`) and a trailing point (`18.`).
    pub fn parse_millimetres(text: &str) -> Result<Self, LengthParseError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(LengthParseError::Empty);
        }
        let number = strip_millimetre_suffix(trimmed);
        let number = number.trim();
        if number.is_empty() {
            return Err(LengthParseError::Empty);
        }

        let (negative, number) = match number.as_bytes().first() {
            Some(b'-') => (true, &number[1..]),
            Some(b'+') => (false, &number[1..]),
            _ => (false, number),
        };
        let (whole, fraction) = match number.split_once('.') {
            Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
            Some(_) => return Err(LengthParseError::Invalid),
            None => (number, ""),
        };
        if whole.is_empty() && fraction.is_empty() {
            return Err(LengthParseError::Invalid);
        }
        if !whole.bytes().all(|byte| byte.is_ascii_digit()) || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(LengthParseError::Invalid);
        }
        if fraction.len() > 3 {
            return Err(LengthParseError::TooPrecise);
        }

        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| LengthParseError::OutOfRange)? };
        let mut fraction_micrometres = 0_i64;
        for byte in fraction.bytes() {
            fraction_micrometres = fraction_micrometres
                .checked_mul(10)
                .and_then(|value| value.checked_add(i64::from(byte - b'0')))
                .ok_or(LengthParseError::OutOfRange)?;
        }
        for _ in fraction.len()..3 {
            fraction_micrometres = fraction_micrometres.checked_mul(10).ok_or(LengthParseError::OutOfRange)?;
        }

        let micrometres = whole
            .checked_mul(1_000)
            .and_then(|value| value.checked_add(fraction_micrometres))
            .ok_or(LengthParseError::OutOfRange)?;
        let micrometres =
            if negative { micrometres.checked_neg().ok_or(LengthParseError::OutOfRange)? } else { micrometres };
        Ok(Self(micrometres))
    }

    /// True for the zero length.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// True for a strictly positive length.
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// True for a strictly negative length.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Adds two lengths, returning `None` on overflow.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Subtracts `other` from `self`, returning `None` on overflow.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
}

/// Why text could not be parsed as a millimetre length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum LengthParseError {
    /// The input was empty or contained only a unit suffix.
    #[error("enter a length in millimetres")]
    Empty,
    /// The input is not a decimal number with an optional millimetre suffix.
    #[error("not a valid millimetre value")]
    Invalid,
    /// The input has more than three fractional digits.
    #[error("use at most three decimal places in millimetres")]
    TooPrecise,
    /// The input does not fit a [`Length`].
    #[error("length is outside the supported range")]
    OutOfRange,
}

fn strip_millimetre_suffix(text: &str) -> &str {
    ["mm", "MM", "Mm", "mM"].iter().find_map(|suffix| text.strip_suffix(suffix)).unwrap_or(text)
}

impl fmt::Display for Length {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let absolute = self.0.unsigned_abs();
        let millimetres = absolute / 1_000;
        let micrometres = absolute % 1_000;

        if micrometres == 0 {
            return write!(formatter, "{sign}{millimetres} mm");
        }

        let fraction = format!("{micrometres:03}");
        let fraction = fraction.trim_end_matches('0');
        write!(formatter, "{sign}{millimetres}.{fraction} mm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millimetres_are_converted_to_micrometres() {
        assert_eq!(Length::from_millimetres(18), Some(Length::from_micrometres(18_000)));
        assert_eq!(Length::from_millimetres(-2), Some(Length::from_micrometres(-2_000)));
        assert_eq!(Length::from_millimetres(i64::MAX), None);
    }

    #[test]
    fn arithmetic_is_checked() {
        let a = Length::from_micrometres(1_000);
        let b = Length::from_micrometres(2_500);
        assert_eq!(a.checked_add(b), Some(Length::from_micrometres(3_500)));
        assert_eq!(b.checked_sub(a), Some(Length::from_micrometres(1_500)));
        assert_eq!(Length::from_micrometres(i64::MAX).checked_add(a), None);
    }

    #[test]
    fn display_uses_trimmed_millimetres() {
        assert_eq!(Length::from_micrometres(18_000).to_string(), "18 mm");
        assert_eq!(Length::from_micrometres(18_500).to_string(), "18.5 mm");
        assert_eq!(Length::from_micrometres(-1_250).to_string(), "-1.25 mm");
        assert_eq!(Length::ZERO.to_string(), "0 mm");
    }

    #[test]
    fn parses_decimal_millimetres_without_floats() {
        let cases = [
            ("720", 720_000),
            ("18.5 mm", 18_500),
            (" 0.001 ", 1),
            (".5mm", 500),
            ("18.", 18_000),
            ("+3", 3_000),
            ("-2.25 MM", -2_250),
        ];
        for (text, micrometres) in cases {
            assert_eq!(Length::parse_millimetres(text), Ok(Length::from_micrometres(micrometres)), "parsing {text:?}");
        }
    }

    #[test]
    fn rejects_invalid_or_oversized_text() {
        assert_eq!(Length::parse_millimetres(""), Err(LengthParseError::Empty));
        assert_eq!(Length::parse_millimetres(" mm "), Err(LengthParseError::Empty));
        assert_eq!(Length::parse_millimetres("abc"), Err(LengthParseError::Invalid));
        assert_eq!(Length::parse_millimetres("1.2.3"), Err(LengthParseError::Invalid));
        assert_eq!(Length::parse_millimetres("1.2345"), Err(LengthParseError::TooPrecise));
        assert_eq!(Length::parse_millimetres("9999999999999999999"), Err(LengthParseError::OutOfRange));
    }

    #[test]
    fn display_round_trips_through_the_parser() {
        for micrometres in [0, 1, 500, 18_000, -12_345, i64::MAX] {
            let length = Length::from_micrometres(micrometres);
            assert_eq!(Length::parse_millimetres(&length.to_string()), Ok(length));
        }
    }

    #[test]
    fn serde_round_trip_keeps_micrometres() {
        let length = Length::from_micrometres(-12_345);
        let text = serde_json::to_string(&length).expect("serialize length");
        assert_eq!(text, "-12345");
        let decoded: Length = serde_json::from_str(&text).expect("deserialize length");
        assert_eq!(decoded, length);
    }
}
