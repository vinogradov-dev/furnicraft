//! Fixtures the model's own tests build on.
//!
//! A helper lives here when more than one module of this crate needs it; anything used by one
//! module stays in that module's own `tests`. The three helpers every crate shares — `millimetres`,
//! `open_path` and `sample_project` — come from [`crate::testing`].

use furnicraft_units::Length;

use crate::{
    Contour, ContourSegmentId, FurnType, Geometry, Id, MaterialId, Mesh, Object, Panel, PanelNominalSize, PanelRect,
    Placement,
};

pub(crate) use crate::testing::{millimetres, sample_project};

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

/// A rectangular ring with its minimum corner at `(x, y)`.
pub(crate) fn ring(x: i64, y: i64, width: i64, height: i64) -> Contour {
    Contour::from_polygon(vec![
        [millimetres(x), millimetres(y)],
        [millimetres(x + width), millimetres(y)],
        [millimetres(x + width), millimetres(y + height)],
        [millimetres(x), millimetres(y + height)],
    ])
    .expect("rectangle ring")
}

pub(crate) fn plate_contour() -> Contour {
    rectangle_contour(100, 50)
}

pub(crate) fn segment_id(value: u32) -> ContourSegmentId {
    ContourSegmentId::new(value).expect("a non-zero id")
}
