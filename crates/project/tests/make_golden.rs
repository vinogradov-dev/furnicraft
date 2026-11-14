//! Regenerates the one checked-in golden file from the current serializer.
//!
//! The golden is the compatibility contract of the scene file: it is written by this build, it must
//! keep loading as the schema moves, and a schema bump regenerates it instead of leaving a second
//! file behind (ADR 0096). Run it deliberately, then review the diff:
//!
//! ```sh
//! cargo test -p furnicraft-project --test make_golden -- --ignored
//! ```
//!
//! Nothing in the normal test run touches the source tree, because the test is ignored by default.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;

use furnicraft_model::{
    CameraNode, CameraProjection, Constructive, Container, CutoutDepth, CutoutShape, EdgeBanding, EdgeBevel, EdgeId,
    EdgeRef, FaceSide, Geometry, Id, LightKind, LightNode, MaterialId, Object, Panel, PanelNominalSize, PanelRect,
    Path, Placement, Primitive, Project,
};
use furnicraft_project::SceneFile;
use furnicraft_units::{Degrees, Length};
use serde_json::Value;

fn millimetres(value: i64) -> Length {
    Length::from_millimetres(value).expect("whole millimetres fit a length")
}

fn id(value: u64) -> Id {
    Id::new(value).expect("a non-zero id")
}

/// The fixture every property of the current layout is checked against.
fn scene() -> SceneFile {
    let shelf = Panel::new(
        id(2),
        "Shelf",
        PanelNominalSize::new(millimetres(700), millimetres(500)).expect("size"),
        None,
        millimetres(18),
        Placement::IDENTITY,
    )
    .expect("the shelf is valid");
    let front = Panel::new(
        id(9),
        "Front",
        PanelNominalSize::new(millimetres(600), millimetres(140)).expect("size"),
        None,
        millimetres(18),
        Placement::IDENTITY,
    )
    .expect("the front is valid");

    let closed = Path::polyline(
        vec![
            [Length::ZERO, Length::ZERO],
            [millimetres(600), Length::ZERO],
            [millimetres(600), millimetres(400)],
            [Length::ZERO, millimetres(400)],
        ],
        true,
    )
    .expect("a closed ring");
    let constructive = Constructive::extrude(closed, millimetres(18)).expect("an extrusion");
    let drawn = Path::polyline(vec![[Length::ZERO, Length::ZERO], [millimetres(120), millimetres(40)]], false)
        .expect("an open path");

    let mut drawer = Container::new(id(8), "Drawer", Placement::IDENTITY, vec![front.into()]);
    drawer.set_component_marker(true).expect("the drawer is a component");
    let root = Container::new(
        id(1),
        "Root",
        Placement::IDENTITY,
        vec![
            shelf.into(),
            Object::new(id(3), "Top", Placement::IDENTITY, Geometry::Constructive(constructive)).into(),
            Object::new(id(4), "Sketch", Placement::IDENTITY, Geometry::Path(drawn)).into(),
            Object::new(
                id(5),
                "Ball",
                Placement::IDENTITY,
                Geometry::Primitive(Primitive::sphere(millimetres(50), 32, 16).expect("a sphere")),
            )
            .into(),
            CameraNode::new(id(6), "Presentation", Placement::IDENTITY, CameraProjection::DEFAULT)
                .expect("a camera")
                .into(),
            LightNode::new(id(7), "Key", Placement::IDENTITY, LightKind::Sun, [1.0, 0.95, 0.9], 2.0)
                .expect("a light")
                .into(),
            drawer.into(),
        ],
    );
    let mut project = Project::new(root).expect("the scene is valid");
    project
        .set_edge_banding(
            id(2),
            EdgeBanding::new(
                EdgeId::new(1).expect("edge"),
                MaterialId::new(1).expect("material"),
                false,
                millimetres(2),
                millimetres(22),
            )
            .expect("banding"),
        )
        .expect("banding on the shelf");
    project
        .add_cutout(
            id(2),
            CutoutShape::Rectangle(
                PanelRect::new(millimetres(50), millimetres(60), millimetres(200), millimetres(90)).expect("a cutout"),
            ),
            CutoutDepth::Blind { depth: millimetres(8) },
            FaceSide::A,
        )
        .expect("a cutout on the shelf");
    project
        .set_panel_bevel(
            id(2),
            EdgeBevel::new(EdgeRef::Rect { edge: EdgeId::new(4).expect("edge") }, Degrees::from_millidegrees(45_000))
                .expect("a 45 degree end bevel"),
        )
        .expect("bevel on the shelf");
    project.set_active_camera(Some(id(6))).expect("an active camera");

    let mut extra = BTreeMap::new();
    extra.insert("generator".to_owned(), Value::String("furnicraft golden".to_owned()));
    SceneFile::new(project).with_extra(extra)
}

#[test]
#[ignore = "writes the checked-in golden; run it deliberately when the schema changes"]
fn regenerate_the_golden_file() {
    let text = scene().to_json().expect("the scene serializes");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/scene.fcs");
    fs::write(&path, text).expect("write the golden file");
}
