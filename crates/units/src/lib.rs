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
    fn serde_round_trip_keeps_micrometres() {
        let length = Length::from_micrometres(-12_345);
        let text = serde_json::to_string(&length).expect("serialize length");
        assert_eq!(text, "-12345");
        let decoded: Length = serde_json::from_str(&text).expect("deserialize length");
        assert_eq!(decoded, length);
    }
}
