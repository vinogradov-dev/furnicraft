//! Test support: the fixtures every crate builds its tests from.
//!
//! Compiled for this crate's own tests and, through the `testing` feature, for `furnicraft-testkit`,
//! which re-exports [`millimetres`], [`open_path`], [`cube_mesh`] and [`sample_project`]. It is the
//! one place those four are defined (C7): a crate's tests may not reach back into `furnicraft-testkit`, because a
//! dev-dependency on a crate that already depends on this one would compile this crate a second time
//! and every model type would come in two incompatible versions.
//!
//! Nothing here is product code; the feature is off in a release build.

use crate::{ClassCode, Container, Envelope, Id, Mesh, Panel, PanelNominalSize, Path, Placement, Project};
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

/// A closed box of `edge` centred on the local origin.
///
/// Picking, gizmo and cache tests need a solid around the origin rather than the document's own
/// base-corner box, so the fixture names the centring it wants instead of borrowing the primitive's
/// anchor.
#[allow(clippy::expect_used)]
pub fn cube_mesh(edge: Length) -> Mesh {
    let half = Length::from_micrometres(edge.micrometres() / 2);
    let low = Length::from_micrometres(-half.micrometres());
    Mesh::cuboid([low; 3], [half; 3]).expect("a cube has a positive edge")
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

/// The sample project with one product beside its other nodes, and that product's id.
///
/// The product is a 600 x 560 x 720 mm box with the placeholder class and no children yet, so a
/// test opens it and creates furniture inside it (ADR 0098). Every other fixture and id stays the
/// same, which keeps the guard's change to the tests that create parts in one place.
#[allow(clippy::expect_used)]
pub fn sample_product_project() -> (Project, Id) {
    let mut project = sample_project();
    let root = project.root().id();
    let envelope = Envelope::new(millimetres(600), millimetres(560), millimetres(720)).expect("a valid box");
    let product = project
        .add_product(root, "Cabinet", ClassCode::GENERAL_PRODUCT, envelope, Placement::IDENTITY)
        .expect("a product beside the sample nodes");
    (project, product)
}
