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

use furnicraft_model::{
    Assembly, Catalog, EdgeBandingMaterial, Id, MaterialId, Panel, PanelMaterial, PanelNominalSize, PanelRect,
    Placement, Project,
};
use furnicraft_units::Length;

/// The stable id of the sample panel material.
#[allow(clippy::expect_used)]
pub fn sample_material_id() -> MaterialId {
    MaterialId::new(1).expect("non-zero material id")
}

/// The stable id of the sample edge-banding material.
#[allow(clippy::expect_used)]
pub fn sample_edge_material_id() -> MaterialId {
    MaterialId::new(101).expect("non-zero material id")
}

/// The fallback thickness of a sample panel.
pub const fn sample_panel_thickness() -> Length {
    Length::from_micrometres(18_000)
}

/// A 5 mm fallback thickness for material-change diagnostics.
pub const fn sample_thin_panel_thickness() -> Length {
    Length::from_micrometres(5_000)
}

/// A 500 mm banding thickness, wide enough to empty the sample core.
pub const fn sample_wide_band_thickness() -> Length {
    Length::from_micrometres(500_000)
}

/// A 2 mm banding profile thickness in the panel plane.
pub const fn sample_band_thickness() -> Length {
    Length::from_micrometres(2_000)
}

/// A 22 mm banding width.
pub const fn sample_band_width() -> Length {
    Length::from_micrometres(22_000)
}

/// A 700 x 500 mm nominal panel used by tests that need a valid size.
#[allow(clippy::expect_used)]
pub fn sample_panel_nominal_size() -> PanelNominalSize {
    PanelNominalSize::new(
        Length::from_millimetres(700).expect("700 mm fits a length"),
        Length::from_millimetres(500).expect("500 mm fits a length"),
    )
    .expect("sample panel size is within limits")
}

/// A second valid nominal panel size, used by resize tests.
#[allow(clippy::expect_used)]
pub fn sample_resized_panel_nominal_size() -> PanelNominalSize {
    PanelNominalSize::new(
        Length::from_millimetres(900).expect("900 mm fits a length"),
        Length::from_millimetres(600).expect("600 mm fits a length"),
    )
    .expect("sample panel size is within limits")
}

/// A 620 x 500 mm nominal panel, narrow enough to invalidate a cutout near the right edge.
#[allow(clippy::expect_used)]
pub fn sample_narrow_panel_nominal_size() -> PanelNominalSize {
    PanelNominalSize::new(
        Length::from_millimetres(620).expect("620 mm fits a length"),
        Length::from_millimetres(500).expect("500 mm fits a length"),
    )
    .expect("sample panel size is within limits")
}

/// A catalogue with an 18 mm chipboard and a 2 x 22 mm PVC edge.
#[allow(clippy::expect_used)]
pub fn sample_catalog() -> Catalog {
    let panel = PanelMaterial::new(sample_material_id(), "Chipboard", "chipboard", sample_panel_thickness(), "white")
        .expect("sample panel material is valid");
    let edge = EdgeBandingMaterial::new(
        sample_edge_material_id(),
        "PVC edge",
        "pvc",
        Length::from_millimetres(2).expect("2 mm fits a length"),
        Length::from_millimetres(22).expect("22 mm fits a length"),
        "white",
    )
    .expect("sample edge material is valid");
    Catalog::new(vec![panel], vec![edge]).expect("sample catalogue ids are unique")
}

/// A 100 x 50 mm cutout rectangle at `(10, 10)` on the sample panel.
#[allow(clippy::expect_used)]
pub fn sample_cutout_rect() -> PanelRect {
    PanelRect::new(
        Length::from_millimetres(10).expect("10 mm fits a length"),
        Length::from_millimetres(10).expect("10 mm fits a length"),
        Length::from_millimetres(100).expect("100 mm fits a length"),
        Length::from_millimetres(50).expect("50 mm fits a length"),
    )
    .expect("sample cutout rectangle is valid")
}

/// A rectangle that overlaps [`sample_cutout_rect`] on the sample panel.
#[allow(clippy::expect_used)]
pub fn sample_overlapping_cutout_rect() -> PanelRect {
    PanelRect::new(
        Length::from_millimetres(50).expect("50 mm fits a length"),
        Length::from_millimetres(20).expect("20 mm fits a length"),
        Length::from_millimetres(100).expect("100 mm fits a length"),
        Length::from_millimetres(50).expect("50 mm fits a length"),
    )
    .expect("overlapping cutout rectangle is valid")
}

/// A 50 x 50 mm rectangle near the right edge of the 700 mm sample panel.
#[allow(clippy::expect_used)]
pub fn sample_edge_cutout_rect() -> PanelRect {
    PanelRect::new(
        Length::from_millimetres(600).expect("600 mm fits a length"),
        Length::from_millimetres(10).expect("10 mm fits a length"),
        Length::from_millimetres(50).expect("50 mm fits a length"),
        Length::from_millimetres(50).expect("50 mm fits a length"),
    )
    .expect("edge cutout rectangle is valid")
}

/// A rectangle that runs past the 700 mm sample panel.
#[allow(clippy::expect_used)]
pub fn sample_outside_cutout_rect() -> PanelRect {
    PanelRect::new(
        Length::from_millimetres(650).expect("650 mm fits a length"),
        Length::from_millimetres(10).expect("10 mm fits a length"),
        Length::from_millimetres(100).expect("100 mm fits a length"),
        Length::from_millimetres(50).expect("50 mm fits a length"),
    )
    .expect("outside cutout rectangle is valid")
}

/// An 8 mm blind depth for the 18 mm sample panel.
#[allow(clippy::expect_used)]
pub fn sample_blind_depth() -> Length {
    Length::from_millimetres(8).expect("8 mm fits a length")
}

/// A small valid project: a root, one panel and one nested assembly with a panel.
#[allow(clippy::expect_used)]
pub fn sample_project() -> Project {
    let size = sample_panel_nominal_size();
    let shelf = Panel::new(
        Id::new(2).expect("non-zero id"),
        "Shelf",
        size,
        None,
        sample_panel_thickness(),
        Placement::IDENTITY,
    )
    .expect("sample shelf is valid");
    let front = Panel::new(
        Id::new(4).expect("non-zero id"),
        "Front",
        size,
        None,
        sample_panel_thickness(),
        Placement::IDENTITY,
    )
    .expect("sample front is valid");
    let drawer = Assembly::new(Id::new(3).expect("non-zero id"), "Drawer", Placement::IDENTITY, vec![front.into()]);
    let root =
        Assembly::new(Id::new(1).expect("non-zero id"), "Root", Placement::IDENTITY, vec![shelf.into(), drawer.into()]);
    Project::new(root).expect("sample project is valid")
}
