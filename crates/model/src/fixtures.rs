//! Shared test fixtures for the model crate.
//!
//! A helper lives here when more than one module's tests need it; anything used by one module stays
//! in that module's own `tests`.

use furnicraft_units::Length;

use crate::{
    Container, Contour, ContourSegmentId, FurnType, Geometry, Id, MaterialId, Mesh, Node, Object, Panel,
    PanelNominalSize, PanelRect, Placement, Project,
};

/// Whole millimetres as an exact length.
pub(crate) fn millimetres(value: i64) -> Length {
    Length::from_millimetres(value).expect("whole millimetres fit a length")
}

/// Parses the dotted text form of a furniture type.
pub(crate) fn furntype(text: &str) -> FurnType {
    text.parse().expect("valid furniture type")
}

pub(crate) fn sample_nominal() -> PanelNominalSize {
    PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid nominal size")
}

pub(crate) fn sample_thickness() -> Length {
    millimetres(18)
}

pub(crate) fn sample_panel(id: Id, name: &str) -> Panel {
    Panel::new(id, name, sample_nominal(), None, sample_thickness(), Placement::IDENTITY).expect("valid panel")
}

pub(crate) fn sample_project() -> Project {
    let shelf = sample_panel(Id::new(2).expect("id"), "Shelf");
    let front = sample_panel(Id::new(4).expect("id"), "Front");
    let drawer = component(Id::new(3).expect("id"), "Drawer", Placement::IDENTITY, vec![front.into()]);
    let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![shelf.into(), drawer.into()]);
    Project::new(root).expect("valid project")
}

pub(crate) fn component(id: Id, name: &str, placement: Placement, children: Vec<Node>) -> Container {
    let mut container = Container::new(id, name, placement, children);
    container.set_component_marker(true).expect("component marker");
    container
}

pub(crate) fn sample_material() -> MaterialId {
    MaterialId::new(1).expect("material id")
}

pub(crate) fn box_mesh() -> Mesh {
    let p = millimetres;
    Mesh::new(
        vec![
            [p(0), p(0), p(0)],
            [p(10), p(0), p(0)],
            [p(10), p(10), p(0)],
            [p(0), p(10), p(0)],
            [p(0), p(0), p(10)],
            [p(10), p(0), p(10)],
            [p(10), p(10), p(10)],
            [p(0), p(10), p(10)],
        ],
        vec![
            vec![0, 3, 2, 1],
            vec![4, 5, 6, 7],
            vec![0, 1, 5, 4],
            vec![1, 2, 6, 5],
            vec![2, 3, 7, 6],
            vec![3, 0, 4, 7],
        ],
    )
    .expect("valid box mesh")
}

pub(crate) fn sample_object(id: u64) -> Object {
    Object::new(Id::new(id).expect("id"), format!("Object {id}"), Placement::IDENTITY, Geometry::Mesh(box_mesh()))
}

pub(crate) fn sample_rect(x: i64, y: i64, width: i64, height: i64) -> PanelRect {
    PanelRect::new(millimetres(x), millimetres(y), millimetres(width), millimetres(height))
        .expect("valid cutout rectangle")
}

pub(crate) fn rectangle_contour(width: i64, height: i64) -> Contour {
    Contour::from_polygon(vec![
        [millimetres(0), millimetres(0)],
        [millimetres(width), millimetres(0)],
        [millimetres(width), millimetres(height)],
        [millimetres(0), millimetres(height)],
    ])
    .expect("rectangle")
}

pub(crate) fn plate_contour() -> Contour {
    rectangle_contour(100, 50)
}

pub(crate) fn segment_id(value: u32) -> ContourSegmentId {
    ContourSegmentId::new(value).expect("a non-zero id")
}
