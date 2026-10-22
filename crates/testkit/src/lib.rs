//! Test fixtures and helpers shared by FurniCraft crates.
//!
//! Development-only: nothing in the product may depend on this crate.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use furnicraft_model::{Assembly, Id, Panel, PanelSize, Placement, Project};
use furnicraft_units::Length;

/// A 700 x 500 x 18 mm panel used by tests that need a valid size.
#[allow(clippy::expect_used)]
pub fn sample_panel_size() -> PanelSize {
    PanelSize::new(
        Length::from_millimetres(700).expect("700 mm fits a length"),
        Length::from_millimetres(500).expect("500 mm fits a length"),
        Length::from_millimetres(18).expect("18 mm fits a length"),
    )
    .expect("sample panel size is within limits")
}

/// A second valid panel size, used by resize tests.
#[allow(clippy::expect_used)]
pub fn sample_resized_panel_size() -> PanelSize {
    PanelSize::new(
        Length::from_millimetres(900).expect("900 mm fits a length"),
        Length::from_millimetres(600).expect("600 mm fits a length"),
        Length::from_millimetres(20).expect("20 mm fits a length"),
    )
    .expect("sample panel size is within limits")
}

/// A small valid project: a root, one panel and one nested assembly with a panel.
#[allow(clippy::expect_used)]
pub fn sample_project() -> Project {
    let size = sample_panel_size();
    let shelf = Panel::new(Id::new(2).expect("non-zero id"), "Shelf", size, Placement::IDENTITY);
    let front = Panel::new(Id::new(4).expect("non-zero id"), "Front", size, Placement::IDENTITY);
    let drawer = Assembly::new(Id::new(3).expect("non-zero id"), "Drawer", Placement::IDENTITY, vec![front.into()]);
    let root =
        Assembly::new(Id::new(1).expect("non-zero id"), "Root", Placement::IDENTITY, vec![shelf.into(), drawer.into()]);
    Project::new(root).expect("sample project is valid")
}
