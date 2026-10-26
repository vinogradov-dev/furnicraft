//! Lengths, angles and unit conversion.
//!
//! All persisted dimensions are integer micrometres (`Length`); angles shown to the user are integer millidegrees (`Degrees`). Floating-point millimetres and radians exist only at the geometry and rendering boundary.
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
        parse_fixed_point(text, LENGTH_SUFFIXES).map(Self).map_err(LengthParseError::from)
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

/// A rotation angle in integer millidegrees.
///
/// Degrees are the human-facing unit for placements and the inspector; the document stores
/// rotations as unit quaternions. The value is signed because Euler angles may be negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Degrees(i64);

impl Degrees {
    /// Zero degrees.
    pub const ZERO: Self = Self(0);

    /// Builds an angle from an exact number of millidegrees.
    pub const fn from_millidegrees(millidegrees: i64) -> Self {
        Self(millidegrees)
    }

    /// The exact value in millidegrees.
    pub const fn millidegrees(self) -> i64 {
        self.0
    }

    /// The value in degrees as a float.
    ///
    /// This is a boundary conversion for geometry and rendering only. Never store the result in
    /// the document or in a command parameter.
    pub fn degrees_f64(self) -> f64 {
        self.0 as f64 / 1_000.0
    }

    /// The value in radians as a float.
    ///
    /// This is a boundary conversion for geometry and rendering only.
    pub fn radians_f64(self) -> f64 {
        self.degrees_f64().to_radians()
    }

    /// Converts radians to degrees, rounded to the nearest millidegree.
    ///
    /// Returns `None` for a non-finite value or one outside the range of [`Degrees`].
    pub fn from_radians(radians: f64) -> Option<Self> {
        if !radians.is_finite() {
            return None;
        }
        let millidegrees = radians.to_degrees() * 1_000.0;
        if !millidegrees.is_finite() || millidegrees < i64::MIN as f64 || millidegrees > i64::MAX as f64 {
            return None;
        }
        Some(Self(millidegrees.round() as i64))
    }

    /// Parses an angle from decimal degrees, with an optional `°` or `deg` suffix.
    ///
    /// The parser works on digits, not floats, so `30.5` becomes exactly 30 500 millidegrees and a
    /// value with more than three fractional digits is rejected instead of being rounded. A leading
    /// `+` or `-` is accepted, as are a bare fraction (`.5`) and a trailing point (`30.`).
    pub fn parse_degrees(text: &str) -> Result<Self, DegreesParseError> {
        parse_fixed_point(text, DEGREE_SUFFIXES).map(Self).map_err(DegreesParseError::from)
    }
}

/// Why text could not be parsed as a degree angle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DegreesParseError {
    /// The input was empty or contained only a unit suffix.
    #[error("enter an angle in degrees")]
    Empty,
    /// The input is not a decimal number with an optional degree suffix.
    #[error("not a valid degree value")]
    Invalid,
    /// The input has more than three fractional digits.
    #[error("use at most three decimal places in degrees")]
    TooPrecise,
    /// The input does not fit a [`Degrees`].
    #[error("angle is outside the supported range")]
    OutOfRange,
}

/// Why text could not be parsed as a fixed-point decimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecimalParseError {
    Empty,
    Invalid,
    TooPrecise,
    OutOfRange,
}

impl From<DecimalParseError> for LengthParseError {
    fn from(error: DecimalParseError) -> Self {
        match error {
            DecimalParseError::Empty => Self::Empty,
            DecimalParseError::Invalid => Self::Invalid,
            DecimalParseError::TooPrecise => Self::TooPrecise,
            DecimalParseError::OutOfRange => Self::OutOfRange,
        }
    }
}

impl From<DecimalParseError> for DegreesParseError {
    fn from(error: DecimalParseError) -> Self {
        match error {
            DecimalParseError::Empty => Self::Empty,
            DecimalParseError::Invalid => Self::Invalid,
            DecimalParseError::TooPrecise => Self::TooPrecise,
            DecimalParseError::OutOfRange => Self::OutOfRange,
        }
    }
}

const LENGTH_SUFFIXES: &[&str] = &["mm", "MM", "Mm", "mM"];
const DEGREE_SUFFIXES: &[&str] = &["degrees", "DEGREES", "Degrees", "deg", "DEG", "Deg", "°"];

/// Parses a decimal with at most three fractional digits into thousandths.
fn parse_fixed_point(text: &str, suffixes: &[&str]) -> Result<i64, DecimalParseError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(DecimalParseError::Empty);
    }
    let number = strip_suffix(trimmed, suffixes);
    let number = number.trim();
    if number.is_empty() {
        return Err(DecimalParseError::Empty);
    }

    let (negative, number) = match number.as_bytes().first() {
        Some(b'-') => (true, &number[1..]),
        Some(b'+') => (false, &number[1..]),
        _ => (false, number),
    };
    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
        Some(_) => return Err(DecimalParseError::Invalid),
        None => (number, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err(DecimalParseError::Invalid);
    }
    if !whole.bytes().all(|byte| byte.is_ascii_digit()) || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DecimalParseError::Invalid);
    }
    if fraction.len() > 3 {
        return Err(DecimalParseError::TooPrecise);
    }

    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| DecimalParseError::OutOfRange)? };
    let mut thousandths = 0_i64;
    for byte in fraction.bytes() {
        thousandths = thousandths
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(byte - b'0')))
            .ok_or(DecimalParseError::OutOfRange)?;
    }
    for _ in fraction.len()..3 {
        thousandths = thousandths.checked_mul(10).ok_or(DecimalParseError::OutOfRange)?;
    }

    let value = whole
        .checked_mul(1_000)
        .and_then(|value| value.checked_add(thousandths))
        .ok_or(DecimalParseError::OutOfRange)?;
    if negative { value.checked_neg().ok_or(DecimalParseError::OutOfRange) } else { Ok(value) }
}

fn strip_suffix<'a>(text: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes.iter().find_map(|suffix| text.strip_suffix(suffix)).unwrap_or(text)
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

impl fmt::Display for Degrees {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let absolute = self.0.unsigned_abs();
        let degrees = absolute / 1_000;
        let millidegrees = absolute % 1_000;

        if millidegrees == 0 {
            return write!(formatter, "{sign}{degrees}°");
        }

        let fraction = format!("{millidegrees:03}");
        let fraction = fraction.trim_end_matches('0');
        write!(formatter, "{sign}{degrees}.{fraction}°")
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

    #[test]
    fn degrees_convert_from_millidegrees_and_radians() {
        assert_eq!(Degrees::from_millidegrees(30_500).millidegrees(), 30_500);
        assert!((Degrees::from_millidegrees(30_500).degrees_f64() - 30.5).abs() < 1.0e-12);
        assert_eq!(Degrees::from_radians(std::f64::consts::FRAC_PI_2), Some(Degrees::from_millidegrees(90_000)));
        assert!((Degrees::from_millidegrees(90_000).radians_f64() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
        assert_eq!(Degrees::from_radians(f64::NAN), None);
        assert_eq!(Degrees::from_radians(f64::INFINITY), None);
        assert_eq!(Degrees::from_radians(1.0e300), None);
    }

    #[test]
    fn display_uses_trimmed_degrees() {
        assert_eq!(Degrees::from_millidegrees(30_000).to_string(), "30°");
        assert_eq!(Degrees::from_millidegrees(30_500).to_string(), "30.5°");
        assert_eq!(Degrees::from_millidegrees(-1).to_string(), "-0.001°");
        assert_eq!(Degrees::ZERO.to_string(), "0°");
    }

    #[test]
    fn parses_decimal_degrees_without_floats() {
        let cases = [
            ("30", 30_000),
            ("30.5°", 30_500),
            (" 0.001 ", 1),
            (".5deg", 500),
            ("30.", 30_000),
            ("+3", 3_000),
            ("-2.25 DEG", -2_250),
            ("45 degrees", 45_000),
        ];
        for (text, millidegrees) in cases {
            assert_eq!(Degrees::parse_degrees(text), Ok(Degrees::from_millidegrees(millidegrees)), "parsing {text:?}");
        }
    }

    #[test]
    fn rejects_invalid_or_oversized_degree_text() {
        assert_eq!(Degrees::parse_degrees(""), Err(DegreesParseError::Empty));
        assert_eq!(Degrees::parse_degrees(" ° "), Err(DegreesParseError::Empty));
        assert_eq!(Degrees::parse_degrees("abc"), Err(DegreesParseError::Invalid));
        assert_eq!(Degrees::parse_degrees("1.2.3"), Err(DegreesParseError::Invalid));
        assert_eq!(Degrees::parse_degrees("1.2345"), Err(DegreesParseError::TooPrecise));
        assert_eq!(Degrees::parse_degrees("9999999999999999999"), Err(DegreesParseError::OutOfRange));
    }

    #[test]
    fn degree_display_round_trips_through_the_parser() {
        for millidegrees in [0, 1, 500, 30_000, -12_345, i64::MAX] {
            let degrees = Degrees::from_millidegrees(millidegrees);
            assert_eq!(Degrees::parse_degrees(&degrees.to_string()), Ok(degrees));
        }
    }

    #[test]
    fn serde_round_trip_keeps_millidegrees() {
        let degrees = Degrees::from_millidegrees(-12_345);
        let text = serde_json::to_string(&degrees).expect("serialize degrees");
        assert_eq!(text, "-12345");
        let decoded: Degrees = serde_json::from_str(&text).expect("deserialize degrees");
        assert_eq!(decoded, degrees);
    }
}
