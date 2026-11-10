//! The frame contract, checked in one place.
//!
//! A node's frame is chosen once, at creation: the content's anchor becomes the local zero, the
//! translation is that anchor's parent-frame position, the rotation is kept and the pivot starts at
//! zero (ADR 0082). A panel anchors at its base corner instead (ADR 0083); a closed ring anchors at
//! its bounds centre and an open chain at its first point. Nothing after creation recomputes an
//! object's frame: a geometry edit and a move never touch the stored pivot, a move changes only the
//! translation, and a turn about the pivot keeps `pivot_world` fixed (ADR 0026, ADR 0027).
//!
//! The creation commands in `furnicraft-engine` and the shell call these same model operations, so
//! this module is the single contract they rely on.
//!
//! Changes when: the frame rule, the pivot rule or a creation command's anchor changes.

use furnicraft_geom::{DQuat, DVec3};
use furnicraft_units::Length;

use crate::fixtures::*;
use crate::frame::children_center;
use crate::*;

/// A parent frame away from the origin and turned, so a rebase cannot pass by coincidence.
fn parent_frame() -> Placement {
    Placement::new(DVec3::new(-120.0, 75.0, 12.0), DQuat::from_rotation_z(0.6)).expect("a moved and turned frame")
}

/// A length pair as the millimetre geometry works in.
fn point_of(pair: [Length; 2]) -> DVec3 {
    DVec3::new(pair[0].millimetres_f64(), pair[1].millimetres_f64(), 0.0)
}

/// A micrometre pair as the millimetre geometry works in.
fn micrometre_point(pair: [i64; 2]) -> DVec3 {
    DVec3::new(
        Length::from_micrometres(pair[0]).millimetres_f64(),
        Length::from_micrometres(pair[1]).millimetres_f64(),
        0.0,
    )
}

fn assert_close(left: DVec3, right: DVec3, what: &str) {
    assert!((left - right).length() < 1.0e-3, "{what}: {left:?} is not {right:?}");
}

/// The world points of a path's segment ends, in millimetres.
fn path_world_points(path: &Path, placement: Placement) -> Vec<DVec3> {
    let mut points = Vec::new();
    for entry in path.entries() {
        let segment = entry.segment();
        for point in [segment.start(), segment.end()].into_iter().flatten() {
            points.push(placement.transform_point(point_of(point)));
        }
    }
    points
}

/// The world points of a contour's segment ends, in millimetres.
fn contour_world_points(contour: &Contour, placement: Placement) -> Vec<DVec3> {
    let mut points = Vec::new();
    for entry in contour.entries() {
        let segment = entry.segment();
        for point in [segment.start(), segment.end()].into_iter().flatten() {
            points.push(placement.transform_point(point_of(point)));
        }
    }
    points
}

/// The stored frame of a node: its placement (translation and rotation) and its pivot.
fn frame_of(project: &Project, id: Id) -> (Placement, Pivot) {
    let placement = project.node(id).expect("a node").placement();
    let pivot = project.pivot(id).expect("a pivot");
    (placement, pivot)
}

/// A second valid nominal size for the resize case.
fn sample_resized_nominal() -> PanelNominalSize {
    PanelNominalSize::new(millimetres(900), millimetres(600)).expect("a valid nominal size")
}

/// The frame every creation leaves: the pivot on the local zero, named by the translation.
fn assert_created_frame(project: &Project, id: Id, expected_translation: DVec3) {
    assert_eq!(project.pivot_local(id), Some(DVec3::ZERO), "a creation leaves the pivot at the local zero");
    let world = project.world_placement(id).expect("a world placement");
    assert_close(world.translation(), expected_translation, "the translation is the anchor's parent-frame position");
    assert_close(
        project.pivot_world(id).expect("a world pivot"),
        expected_translation,
        "the world pivot is the anchor",
    );
}

/// The drawn figures the tools commit, one per kind the ribbon offers.
fn drawn_figures() -> Vec<(&'static str, Path)> {
    vec![
        (
            "line",
            Path::polyline(vec![[millimetres(600), millimetres(400)], [millimetres(700), millimetres(450)]], false)
                .expect("a line"),
        ),
        (
            "polyline",
            Path::polyline(
                vec![
                    [millimetres(610), millimetres(410)],
                    [millimetres(700), millimetres(450)],
                    [millimetres(650), millimetres(520)],
                ],
                false,
            )
            .expect("a polyline"),
        ),
        (
            "arc",
            Path::arc_from_center(
                [millimetres(500), millimetres(400)],
                [millimetres(600), millimetres(400)],
                [millimetres(500), millimetres(500)],
            )
            .expect("an arc"),
        ),
        (
            "rectangle",
            Path::rectangle([millimetres(600), millimetres(400)], [millimetres(700), millimetres(500)])
                .expect("a rectangle"),
        ),
        ("circle", Path::circle([millimetres(650), millimetres(450)], millimetres(50)).expect("a circle")),
        (
            "polygon",
            Path::regular_polygon([millimetres(650), millimetres(450)], [millimetres(700), millimetres(450)], 6)
                .expect("a polygon"),
        ),
    ]
}

#[test]
fn every_drawn_figure_anchors_its_creation_pivot_at_the_local_zero() {
    let frame = parent_frame();
    for (name, path) in drawn_figures() {
        let mut project = sample_project();
        let root = project.root().id();
        let anchor = path.anchor(FrameAnchor::CreationPivot).expect("a creation pivot");
        let expected_translation = frame.transform_point(point_of([anchor[0], anchor[1]]));
        let before = path_world_points(&path, frame);

        let rebased = rebase_at_anchor(path, frame, FrameAnchor::CreationPivot).expect("rebase a figure");
        let stored = rebased.content.clone();
        let id = project
            .add_object_with_pivot(root, name, rebased.placement, Geometry::Path(rebased.content), rebased.pivot)
            .expect("add the figure");

        assert_eq!(
            stored.anchor(FrameAnchor::CreationPivot),
            Ok([Length::ZERO; 3]),
            "{name}: the creation anchor is the local zero"
        );
        assert_created_frame(&project, id, expected_translation);
        let after = path_world_points(&stored, project.world_placement(id).expect("world placement"));
        assert_eq!(after.len(), before.len(), "{name}: no segment is lost");
        for (before, after) in before.iter().zip(&after) {
            assert_close(*before, *after, name);
        }
    }
}

#[test]
fn a_contour_and_the_body_it_grows_anchor_at_their_creation_pivot() {
    let frame = parent_frame();
    let ring = ring(300, 200, 140, 90);
    let anchor = ring.anchor(FrameAnchor::CreationPivot).expect("the ring's centre");
    let expected_translation = frame.transform_point(point_of([anchor[0], anchor[1]]));
    let before = contour_world_points(&ring, frame);

    let rebased = rebase_at_anchor(ring, frame, FrameAnchor::CreationPivot).expect("rebase a ring");
    assert_eq!(rebased.content.anchor(FrameAnchor::CreationPivot), Ok([Length::ZERO; 3]));
    let body = Constructive::extrude(rebased.content.into(), sample_thickness()).expect("extrude the ring");

    let mut project = sample_project();
    let root = project.root().id();
    let id = project
        .add_object_with_pivot(root, "Body", rebased.placement, Geometry::Constructive(body), rebased.pivot)
        .expect("add the body");
    assert_created_frame(&project, id, expected_translation);

    let stored = project.object(id).expect("the body").geometry().as_constructive().expect("a constructive payload");
    let after = contour_world_points(&stored.contour().expect("the stored ring"), project.world_placement(id).unwrap());
    for (before, after) in before.iter().zip(&after) {
        assert_close(*before, *after, "the body keeps the ring's world shape");
    }
}

#[test]
fn a_primitive_is_born_centred_on_its_own_origin() {
    let primitive = Primitive::cube(millimetres(100)).expect("a cube");
    let (min, max) = primitive.bounds_micrometres();
    assert_eq!(min, [-50_000; 3], "the cube's bounds start at minus half its edge");
    assert_eq!(max, [50_000; 3], "the cube's bounds end at plus half its edge");

    let mut project = sample_project();
    let root = project.root().id();
    let id =
        project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Primitive(primitive)).expect("add the cube");

    assert_eq!(project.pivot(id), Some(Pivot::ZERO), "a primitive keeps the zero pivot");
    assert_eq!(project.world_placement(id), Some(Placement::IDENTITY), "the primitive lands at its parent origin");
    assert_eq!(project.pivot_world(id), Some(DVec3::ZERO));
}

#[test]
fn a_panel_is_born_with_its_base_corner_at_the_local_zero() {
    let frame = parent_frame();
    let mut project = sample_project();
    let root = project.root().id();

    let rectangle =
        project.add_panel(root, "Side", sample_nominal(), None, sample_thickness(), frame).expect("a rectangle panel");
    assert_eq!(project.pivot(rectangle), Some(Pivot::ZERO), "a rectangle panel is born at its base corner");
    assert_eq!(project.panel(rectangle).expect("panel").placement(), frame, "the anchor is the given placement");
    assert_eq!(project.panel(rectangle).expect("panel").nominal_size(), sample_nominal());

    let ring = ring(300, 200, 140, 90);
    let corner = ring.bounds_micrometres().0;
    let expected_translation = frame.transform_point(micrometre_point(corner));
    let panel =
        project.add_panel_contour(root, "Top", &ring, None, sample_thickness(), frame).expect("a contour panel");
    assert_eq!(project.pivot(panel), Some(Pivot::ZERO), "a contour panel is born at its base corner");
    let stored = project.panel(panel).expect("panel").outline_contour().expect("a contour outline");
    assert_eq!(stored.bounds_micrometres().0, [0, 0], "the base corner is the local zero");
    assert_created_frame(&project, panel, expected_translation);
}

#[test]
fn a_group_puts_the_children_centre_in_its_translation_and_keeps_their_world_place() {
    let mut project = sample_project();
    let shelf = Id::new(2).expect("shelf id");
    let drawer = Id::new(3).expect("drawer id");
    let front = Id::new(4).expect("front id");
    project.set_node_placement(shelf, parent_frame()).expect("place the shelf");
    project
        .set_node_placement(
            front,
            Placement::new(DVec3::new(10.0, -4.0, 2.0), DQuat::from_rotation_x(0.3)).expect("front"),
        )
        .expect("place the front");

    let selected: Vec<Node> =
        [shelf, drawer].into_iter().map(|id| project.node(id).expect("a selected node").clone()).collect();
    let centre = children_center(&selected).expect("the children's union centre");
    let expected_world = point_of([centre[0], centre[1]]) + DVec3::Z * centre[2].millimetres_f64();
    let before = [
        project.world_placement(shelf).expect("shelf world"),
        project.world_placement(drawer).expect("drawer world"),
        project.world_placement(front).expect("front world"),
    ];

    let group = project.group_nodes(&[shelf, drawer], "Group").expect("group the selection");

    assert_eq!(project.pivot(group), Some(Pivot::ZERO), "a group is anchored by its translation, not its pivot");
    assert_created_frame(&project, group, expected_world);
    for (id, before) in [(shelf, before[0]), (drawer, before[1]), (front, before[2])] {
        assert_close(
            project.world_placement(id).expect("world placement").translation(),
            before.translation(),
            "grouping moves no world point",
        );
        assert_eq!(
            project.world_placement(id).expect("world placement").rotation(),
            before.rotation(),
            "grouping keeps every world rotation"
        );
    }
}

#[test]
fn a_duplicate_copies_the_local_frame_exactly() {
    let mut project = sample_project();
    let root = project.root().id();
    let object = project.add_object(root, "Block", parent_frame(), Geometry::Mesh(box_mesh())).expect("add an object");
    let pivot = Pivot::new(millimetres(5), millimetres(-7), millimetres(3));
    project.set_node_pivot(object, pivot).expect("author the pivot");

    let copy = project.duplicate_node(object).expect("duplicate the object");

    assert_eq!(
        project.object(copy).expect("copy").placement(),
        project.object(object).expect("original").placement(),
        "the copy keeps the local translation and rotation"
    );
    assert_eq!(project.pivot(copy), Some(pivot), "the copy keeps the authored pivot");
    assert_eq!(project.pivot_world(copy), project.pivot_world(object), "the copy grabs where the original does");
}

#[test]
fn a_geometry_edit_leaves_an_objects_frame_alone() {
    let frame = parent_frame();
    let pivot = Pivot::new(millimetres(-3), millimetres(4), millimetres(9));
    let mut project = sample_project();
    let root = project.root().id();

    let path = Path::rectangle([millimetres(600), millimetres(400)], [millimetres(700), millimetres(500)])
        .expect("a rectangle");
    let rebased = rebase_at_anchor(path, frame, FrameAnchor::CreationPivot).expect("rebase the drawing");
    let path_id = project
        .add_object_with_pivot(root, "Drawn", rebased.placement, Geometry::Path(rebased.content), rebased.pivot)
        .expect("add the drawing");
    project.set_node_pivot(path_id, pivot).expect("author the pivot");
    let before = frame_of(&project, path_id);
    let edited = Path::rectangle([millimetres(590), millimetres(390)], [millimetres(720), millimetres(510)])
        .expect("an edited rectangle");
    project.set_object_path(path_id, edited).expect("edit the drawing");
    assert_eq!(frame_of(&project, path_id), before, "a path edit keeps the frame");

    let ring = ring(300, 200, 140, 90);
    let rebased = rebase_at_anchor(ring, frame, FrameAnchor::CreationPivot).expect("rebase the ring");
    let body = Constructive::extrude(rebased.content.into(), sample_thickness()).expect("extrude the ring");
    let body_id = project
        .add_object_with_pivot(root, "Body", rebased.placement, Geometry::Constructive(body), rebased.pivot)
        .expect("add the body");
    project.set_node_pivot(body_id, pivot).expect("author the pivot");
    let before = frame_of(&project, body_id);
    let edited = Path::rectangle([millimetres(250), millimetres(150)], [millimetres(480), millimetres(330)])
        .expect("an edited ring");
    project.set_object_body_path(body_id, edited).expect("edit the body's path");
    project.set_object_extrusion_height(body_id, millimetres(24)).expect("edit the height");
    assert_eq!(frame_of(&project, body_id), before, "a body edit keeps the frame");

    let primitive = Primitive::cube(millimetres(100)).expect("a cube");
    let primitive_id = project.add_object(root, "Cube", frame, Geometry::Primitive(primitive)).expect("add the cube");
    project.set_node_pivot(primitive_id, pivot).expect("author the pivot");
    let before = frame_of(&project, primitive_id);
    let replaced = Primitive::sphere(millimetres(60), 24, 12).expect("a sphere");
    project.set_object_primitive(primitive_id, replaced).expect("replace the parameters");
    assert_eq!(frame_of(&project, primitive_id), before, "a primitive edit keeps the frame");

    let panel = project
        .add_panel(root, "Side", sample_nominal(), None, sample_thickness(), frame)
        .expect("add a rectangle panel");
    let before = frame_of(&project, panel);
    project.set_panel_nominal_size(panel, sample_resized_nominal()).expect("resize the panel");
    assert_eq!(frame_of(&project, panel), before, "a resize keeps the frame");
}

#[test]
fn a_panel_outline_edit_reanchors_the_frame_and_keeps_the_world_shape() {
    let frame = parent_frame();
    let mut project = sample_project();
    let root = project.root().id();
    let id = project
        .add_panel_contour(root, "Top", &ring(100, 50, 200, 120), None, sample_thickness(), frame)
        .expect("add a contour panel");
    let before = project.panel(id).expect("panel").placement();
    let edited = ring(140, 80, 260, 150);
    let min = edited.bounds_micrometres().0;
    let shifted_min = micrometre_point(min);
    let before_world = contour_world_points(&edited, before);

    project.set_panel_outline(id, edited.clone()).expect("edit the outline");

    let after = project.panel(id).expect("panel").placement();
    assert_eq!(project.pivot(id), Some(Pivot::ZERO), "the derived pivot stays at the base corner");
    assert_eq!(after.rotation(), before.rotation(), "an outline edit keeps the rotation");
    assert_close(after.translation(), before.transform_point(shifted_min), "the translation follows the new corner");
    let stored = project.panel(id).expect("panel").outline_contour().expect("a contour outline");
    assert_eq!(stored.bounds_micrometres().0, [0, 0], "the new base corner is the local zero");
    let after_world = contour_world_points(stored, after);
    assert_eq!(after_world.len(), before_world.len(), "no segment is lost");
    for (before, after) in before_world.iter().zip(&after_world) {
        assert_close(*before, *after, "the world shape stays where the user drew it");
    }
}

#[test]
fn a_move_changes_only_the_translation() {
    let frame = parent_frame();
    let pivot = Pivot::new(millimetres(8), millimetres(-2), millimetres(4));
    let mut project = sample_project();
    let root = project.root().id();
    let id = project.add_object(root, "Block", frame, Geometry::Mesh(box_mesh())).expect("add an object");
    project.set_node_pivot(id, pivot).expect("author the pivot");
    let before_pivot = project.pivot_world(id).expect("world pivot");
    let delta = DVec3::new(30.0, -12.0, 5.0);
    let moved = Placement::new(frame.translation() + delta, frame.rotation()).expect("the moved placement");

    project.set_node_placement(id, moved).expect("move the object");

    assert_eq!(project.object(id).expect("object").placement(), moved);
    assert_eq!(project.pivot(id), Some(pivot), "a move keeps the authored pivot");
    let expected_pivot = before_pivot + delta;
    assert_close(project.pivot_world(id).expect("world pivot"), expected_pivot, "the pivot moves with the node");
}

#[test]
fn a_turn_about_the_pivot_keeps_the_pivot_world_fixed() {
    let frame = parent_frame();
    let pivot = Pivot::new(millimetres(8), millimetres(-2), millimetres(4));
    let mut project = sample_project();
    let root = project.root().id();
    let id = project.add_object(root, "Block", frame, Geometry::Mesh(box_mesh())).expect("add an object");
    project.set_node_pivot(id, pivot).expect("author the pivot");
    let centre = project.pivot_world(id).expect("world pivot");
    let turn = DQuat::from_rotation_z(0.7);
    let translation = centre + turn * (frame.translation() - centre);
    let turned = Placement::new(translation, (turn * frame.rotation()).normalize()).expect("a turned placement");

    project.set_node_placement(id, turned).expect("turn the object");

    assert_close(project.pivot_world(id).expect("world pivot"), centre, "a turn about the pivot keeps it fixed");
    assert_eq!(project.pivot(id), Some(pivot), "a turn keeps the local pivot");
    assert_eq!(project.object(id).expect("object").placement().rotation(), turned.rotation());

    let panel =
        project.add_panel(root, "Side", sample_nominal(), None, sample_thickness(), frame).expect("add a panel");
    let start = project.panel(panel).expect("panel").placement();
    let untouched = Placement::new(start.translation(), (turn * start.rotation()).normalize()).expect("turned");
    project.set_node_placement(panel, untouched).expect("turn the panel");
    assert_eq!(
        project.panel(panel).expect("panel").placement().translation(),
        start.translation(),
        "at a zero pivot a turn leaves the translation bit-for-bit"
    );
}

#[test]
fn world_zero_clears_the_parent_translation_and_keeps_the_frame() {
    let mut project = sample_project();
    let drawer = Id::new(3).expect("drawer id");
    let front = Id::new(4).expect("front id");
    project.set_node_placement(drawer, parent_frame()).expect("turn the container");
    let local = Placement::new(DVec3::new(25.0, -8.0, 3.0), DQuat::from_rotation_y(0.4)).expect("a local placement");
    project.set_node_placement(front, local).expect("place the front");
    let before = project.panel(front).expect("front").clone();
    let before_world = project.world_placement(front).expect("world placement");

    let zeroed = local.with_translation(Length::ZERO, Length::ZERO, Length::ZERO).expect("a zero translation");
    project.set_node_placement(front, zeroed).expect("zero the translation");

    let after = project.panel(front).expect("front");
    assert_eq!(after.placement(), zeroed);
    assert_eq!(after.placement().rotation(), before.placement().rotation(), "world-zero keeps the rotation");
    assert_eq!(after.pivot(), before.pivot(), "world-zero keeps the local pivot");
    assert_eq!(after.outline(), before.outline(), "world-zero keeps the local vertices");
    assert_eq!(
        project.world_placement(front).expect("world placement").rotation(),
        before_world.rotation(),
        "the composed world rotation does not change"
    );
}

#[test]
fn undo_and_redo_restore_the_frame_bit_for_bit() {
    let mut project = sample_project();
    let root = project.root().id();
    let before = project.clone();

    let ring = ring(300, 200, 140, 90);
    let rebased = rebase_at_anchor(ring, parent_frame(), FrameAnchor::CreationPivot).expect("rebase the ring");
    let body = Constructive::extrude(rebased.content.into(), sample_thickness()).expect("extrude the ring");
    let id = project
        .add_object_with_pivot(root, "Body", rebased.placement, Geometry::Constructive(body), rebased.pivot)
        .expect("add the body");
    let pivot = Pivot::new(millimetres(1), millimetres(2), millimetres(3));
    project.set_node_pivot(id, pivot).expect("author the pivot");
    let edited = frame_of(&project, id);
    let after = project.clone();

    project.restore_from(&before).expect("undo the creation");
    assert!(project.same_content(&before), "undo restores the document bit for bit");
    assert!(project.object(id).is_none(), "the created node is gone after undo");

    project.restore_from(&after).expect("redo the creation");
    assert!(project.same_content(&after), "redo restores the document bit for bit");
    assert_eq!(frame_of(&project, id), edited);
}

#[test]
fn a_round_trip_at_the_current_schema_keeps_every_frame_invariant() {
    let mut project = sample_project();
    let root = project.root().id();
    let frame = parent_frame();
    let path = Path::circle([millimetres(650), millimetres(450)], millimetres(50)).expect("a circle");
    let rebased = rebase_at_anchor(path, frame, FrameAnchor::CreationPivot).expect("rebase the circle");
    let drawing = project
        .add_object_with_pivot(root, "Circle", rebased.placement, Geometry::Path(rebased.content), rebased.pivot)
        .expect("add the circle");
    let pivot = Pivot::new(millimetres(4), millimetres(5), millimetres(6));
    project.set_node_pivot(drawing, pivot).expect("author the pivot");
    let panel = project
        .add_panel_contour(root, "Top", &ring(300, 200, 140, 90), None, sample_thickness(), frame)
        .expect("add a contour panel");
    let group = project.group_nodes(&[panel, drawing], "Pair").expect("group the new nodes");
    let ids = [panel, drawing, group];

    let text = serde_json::to_string(&project).expect("serialize the project");
    let decoded: Project = serde_json::from_str(&text).expect("reload the project");

    assert_eq!(decoded, project, "the round trip keeps the whole document");
    for id in ids {
        assert_eq!(decoded.pivot(id), project.pivot(id), "the stored pivot survives the round trip");
        assert_eq!(decoded.node(id).expect("node").placement(), project.node(id).expect("node").placement());
        assert_eq!(decoded.world_placement(id), project.world_placement(id));
        assert_eq!(decoded.pivot_world(id), project.pivot_world(id));
    }
}
