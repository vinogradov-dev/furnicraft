//! Test support: the fixtures every crate builds its tests from.
//!
//! Compiled for this crate's own tests and, through the `testing` feature, for `furnicraft-testkit`,
//! which re-exports [`millimetres`], [`open_path`] and [`sample_project`]. It is the one place those
//! three are defined (C7): a crate's tests may not reach back into `furnicraft-testkit`, because a
//! dev-dependency on a crate that already depends on this one would compile this crate a second time
//! and every model type would come in two incompatible versions.
//!
//! Nothing here is product code; the feature is off in a release build.

use crate::{Container, Id, Panel, PanelNominalSize, Path, Placement, Project};
use furnicraft_units::Length;

/// Whole millimetres as an exact length; the unit every fixture is written in.
#[allow(clippy::expect_used)]
pub fn millimetres(value: i64) -> Length {
    Length::from_millimetres(value).expect("whole millimetres fit a length")
}

/// An open polyline through `points` in millimetres — the drawn chain a test edits.
#[allow(clippy::expect_used)]
pub fn open_path(points: Vec<[Length; 2]>) -> Path {
    Path::polyline(points, false).expect("an open path")
}

/// A small valid project: a root, one panel and one component container with a panel.
#[allow(clippy::expect_used)]
pub fn sample_project() -> Project {
    let size = PanelNominalSize::new(millimetres(700), millimetres(500)).expect("sample panel size is within limits");
    let shelf = Panel::new(Id::new(2).expect("non-zero id"), "Shelf", size, None, millimetres(18), Placement::IDENTITY)
        .expect("sample shelf is valid");
    let front = Panel::new(Id::new(4).expect("non-zero id"), "Front", size, None, millimetres(18), Placement::IDENTITY)
        .expect("sample front is valid");
    let mut drawer =
        Container::new(Id::new(3).expect("non-zero id"), "Drawer", Placement::IDENTITY, vec![front.into()]);
    drawer.set_component_marker(true).expect("sample drawer is a component");
    let root = Container::new(
        Id::new(1).expect("non-zero id"),
        "Root",
        Placement::IDENTITY,
        vec![shelf.into(), drawer.into()],
    );
    Project::new(root).expect("sample project is valid")
}
