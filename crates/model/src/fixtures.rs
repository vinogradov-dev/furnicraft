//! Shared test fixtures for the model crate.
//!
//! A helper lives here when more than one module's tests need it; anything used by one module stays
//! in that module's own `tests`.

use furnicraft_units::Length;

use crate::FurnType;

/// Whole millimetres as an exact length.
pub(crate) fn millimetres(value: i64) -> Length {
    Length::from_millimetres(value).expect("whole millimetres fit a length")
}

/// Parses the dotted text form of a furniture type.
pub(crate) fn furntype(text: &str) -> FurnType {
    text.parse().expect("valid furniture type")
}
