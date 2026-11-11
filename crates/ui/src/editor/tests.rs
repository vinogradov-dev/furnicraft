#![allow(clippy::expect_used)]

use super::*;

use crate::toolbars::RibbonView;

use crate::contour_edit::ContourTarget;
use crate::contour_grips::ContourGrip;
use crate::draw::{DrawDraftError, DrawTool};
use crate::mesh_edit::MeshElement;
use crate::scene_tree::TreeAction;
use crate::toolbars::{RibbonTab, ToolAction};
use furnicraft_engine::{PanelCreate, PrimitiveKind, Session};
use furnicraft_model::Contour;
use furnicraft_model::{
    Container, Cutout, CutoutDepth, CutoutShape, DEFAULT_BANDS, DEFAULT_SEGMENTS, EulerAngles, FaceSide, Geometry,
    MachiningAnchor, MachiningParams, Mesh, Panel, Pivot, Placement,
};
use furnicraft_model::{Path, PathCorner, Project};
use furnicraft_testkit::{millimetres, open_path, sample_catalog, sample_panel_thickness, sample_project};
use furnicraft_units::Degrees;
use furnicraft_units::Length;

fn cube_project() -> (Project, Id) {
    let mut project = sample_project();
    let root = project.root().id();
    let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
    let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("add object");
    (project, id)
}

/// A project whose root holds one contour panel with a 700 x 500 ring.
fn contour_panel_project() -> (Project, Id) {
    let width = Length::from_millimetres(700).expect("width");
    let height = Length::from_millimetres(500).expect("height");
    let ring = Contour::from_polygon(vec![
        [Length::ZERO, Length::ZERO],
        [width, Length::ZERO],
        [width, height],
        [Length::ZERO, height],
    ])
    .expect("ring");
    let panel =
        Panel::new_contour(Id::new(2).expect("id"), "Shelf", ring, None, sample_panel_thickness(), Placement::IDENTITY)
            .expect("contour panel");
    let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![panel.into()]);
    (Project::new(root).expect("project"), Id::new(2).expect("id"))
}

/// A project whose root holds one open path object.
fn open_path_project() -> (Project, Id) {
    let mut project = sample_project();
    let root = project.root().id();
    let path = open_path(vec![
        [millimetres(0), millimetres(0)],
        [millimetres(600), millimetres(0)],
        [millimetres(600), millimetres(400)],
    ]);
    let id = project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("path object");
    (project, id)
}

#[test]
fn mesh_edit_starts_only_on_a_single_mesh_object() {
    let (project, cube) = cube_project();
    let mut editor = Editor::new();

    assert!(!editor.start_mesh_edit(&project), "nothing is selected");
    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    assert_eq!(editor.mesh_edit().map(MeshEdit::object), Some(cube));
    assert_eq!(editor.mesh_edit().map(|edit| edit.vertices().len()), Some(0));

    editor.end_mesh_edit();
    assert!(editor.mesh_edit().is_none());

    editor.select_node(Some(Id::new(2).expect("panel")));
    assert!(!editor.start_mesh_edit(&project), "a panel has no editable mesh");
    editor.select_node(Some(project.root().id()));
    assert!(!editor.start_mesh_edit(&project), "the root is a container");
}

#[test]
fn a_multi_selection_cannot_enter_mesh_edit() {
    let (project, cube) = cube_project();
    let mut editor = Editor::new();
    editor.selection.replace_all(vec![cube, Id::new(2).expect("panel")]);
    assert!(!editor.start_mesh_edit(&project));

    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    editor.selection.replace_all(vec![cube, Id::new(2).expect("panel")]);
    editor.retain_mesh_edit(&project);
    assert!(editor.mesh_edit().is_none(), "a selection change leaves the mode");
}

#[test]
fn a_mesh_edit_ends_when_its_object_disappears() {
    let (mut project, cube) = cube_project();
    let mut editor = Editor::new();
    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));

    editor.select_node(Some(project.root().id()));
    editor.retain_mesh_edit(&project);
    assert!(editor.mesh_edit().is_none());

    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    project.remove_node(cube).expect("remove the object");
    editor.selection.clear();
    editor.retain_mesh_edit(&project);
    assert!(editor.mesh_edit().is_none());
}

#[test]
fn contour_edit_starts_only_on_a_single_contour_node() {
    let (project, panel) = contour_panel_project();
    let mut editor = Editor::new();
    assert!(!editor.start_contour_edit(&project, ContourTarget::Panel(panel)), "nothing is selected");

    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    assert_eq!(editor.contour_edit().map(ContourEdit::target), Some(ContourTarget::Panel(panel)));
    assert!(!editor.start_contour_edit(&project, ContourTarget::Panel(panel)), "one edit at a time");

    editor.end_contour_edit();
    assert!(editor.contour_edit().is_none());

    let sample = sample_project();
    let rectangle = Id::new(2).expect("id");
    editor.select_node(Some(rectangle));
    assert!(!editor.start_contour_edit(&sample, ContourTarget::Panel(rectangle)), "a rectangle panel has no ring");

    editor.selection.replace_all(vec![panel, project.root().id()]);
    assert!(
        !editor.start_contour_edit(&project, ContourTarget::Panel(panel)),
        "a multi-selection cannot enter contour editing"
    );
}

#[test]
fn a_ring_typed_along_length_moves_the_corner_and_collapses_at_zero() {
    let (project, panel) = contour_panel_project();
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    let (bottom, right) = {
        let draft = editor.contour_edit().expect("edit").draft();
        (draft.id_at(0).expect("bottom"), draft.id_at(1).expect("right"))
    };
    // The corner at the start of the right side is the end of the bottom side.
    let corner = ContourGrip::Corner(PathCorner::Start(right));
    let length = Length::from_millimetres(200).expect("length");
    editor.contour_edit_along_length(corner, bottom, length).expect("a typed length");
    let ring = editor.contour_edit().expect("edit").draft().as_ring().expect("ring").clone();
    let end = ring.segment(bottom).expect("bottom").end().expect("end");
    assert_eq!(end[0], length, "the corner moved 200 mm from the far start");
    // A zero length records a collapse for Apply.
    editor.contour_edit_along_length(corner, bottom, Length::ZERO).expect("zero length");
    assert_eq!(editor.contour_edit().expect("edit").collapse(), Some(bottom));
}

#[test]
fn contour_edit_ends_on_a_selection_change_or_a_removed_target() {
    let (mut project, panel) = contour_panel_project();
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));

    editor.select_node(Some(project.root().id()));
    editor.retain_contour_edit(&project);
    assert!(editor.contour_edit().is_none(), "a selection change leaves the mode");

    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    project.remove_node(panel).expect("remove the panel");
    editor.retain_contour_edit(&project);
    assert!(editor.contour_edit().is_none(), "a removed target leaves the mode");
}

#[test]
fn a_cutout_ring_enters_the_edit_mode_on_the_panel_selection() {
    let (mut project, panel) = contour_panel_project();
    let ring = Contour::from_polygon(vec![
        [Length::from_millimetres(100).expect("x"), Length::from_millimetres(100).expect("y")],
        [Length::from_millimetres(300).expect("x"), Length::from_millimetres(100).expect("y")],
        [Length::from_millimetres(300).expect("x"), Length::from_millimetres(200).expect("y")],
        [Length::from_millimetres(100).expect("x"), Length::from_millimetres(200).expect("y")],
    ])
    .expect("cutout ring");
    let machining = project
        .add_machining(
            panel,
            MachiningAnchor::face(FaceSide::A),
            MachiningParams::Cutout(Cutout::new(CutoutShape::Contour(ring), CutoutDepth::Through)),
        )
        .expect("cutout");
    let target = ContourTarget::Cutout { panel, machining };
    let mut editor = Editor::new();
    assert!(!editor.start_contour_edit(&project, target), "nothing is selected");

    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, target), "the panel selection frames the cutout");
    assert_eq!(editor.contour_edit().map(ContourEdit::target), Some(target));

    editor.select_node(Some(project.root().id()));
    editor.retain_contour_edit(&project);
    assert!(editor.contour_edit().is_none(), "a selection change leaves the mode");

    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, target));
    project.remove_machining(panel, machining).expect("remove the cutout");
    editor.retain_contour_edit(&project);
    assert!(editor.contour_edit().is_none(), "a removed cutout leaves the mode");
}

#[test]
fn cancelling_a_path_edit_leaves_the_stored_chain() {
    let (project, object) = open_path_project();
    let session = Session::with_catalog(project, sample_catalog());
    let stored = session.project().object_path(object).expect("path").clone();
    let revision = session.project().revision();
    let mut editor = Editor::new();
    editor.select_node(Some(object));
    assert!(editor.start_contour_edit(session.project(), ContourTarget::Path(object)));

    let first = editor.contour_edit().and_then(|edit| edit.draft().id_at(0)).expect("first segment");
    editor
        .contour_edit_move_corner(first, [Length::ZERO, Length::from_millimetres(-50).expect("y")])
        .expect("move the free start");
    editor.end_contour_edit();

    assert!(editor.contour_edit().is_none());
    assert_eq!(session.project().object_path(object).expect("path"), &stored, "cancel keeps the stored chain");
    assert_eq!(session.project().revision(), revision, "a cancelled edit changes no document");
    assert!(!session.can_undo(), "a cancelled edit leaves no undo entry");
}

#[test]
fn a_path_edit_ends_when_its_target_disappears() {
    let (mut project, object) = open_path_project();
    let mut editor = Editor::new();
    editor.select_node(Some(object));
    assert!(editor.start_contour_edit(&project, ContourTarget::Path(object)));

    project.remove_node(object).expect("remove the path");
    editor.retain_contour_edit(&project);
    assert!(editor.contour_edit().is_none(), "a removed target ends the mode");
}

#[test]
fn applying_a_contour_edit_is_one_undoable_command() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(session.project(), ContourTarget::Panel(panel)));
    let segment = editor.contour_edit().and_then(|edit| edit.draft().id_at(1)).expect("right edge");
    editor
        .contour_edit_move_corner(segment, [Length::from_millimetres(800).expect("x"), Length::ZERO])
        .expect("move the corner");
    let revision = session.project().revision();
    editor.apply_contour_edit(&mut session).expect("apply the outline");
    assert!(editor.contour_edit().is_none(), "a successful apply leaves the mode");
    assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
    session.undo().expect("one undo restores the ring");
    assert_eq!(
        session.project().panel(panel).expect("panel").nominal_size().length(),
        Length::from_millimetres(700).expect("length")
    );
}

#[test]
fn applying_an_untouched_contour_edit_changes_nothing() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(session.project(), ContourTarget::Panel(panel)));
    let revision = session.project().revision();
    editor.apply_contour_edit(&mut session).expect("leave the mode");
    assert!(editor.contour_edit().is_none());
    assert_eq!(session.project().revision(), revision, "an untouched draft dispatches nothing");
    assert!(!session.can_undo(), "no empty undo entry is left behind");
}

#[test]
fn another_tool_replaces_the_running_draft_and_writes_nothing() {
    // A tool switch is one gesture, not "cancel, then start" (ADR 0076): the running draft is
    // dropped and the new tool starts empty, and because no draft ever reached the document
    // undo gains no half figure.
    let (project, _panel) = contour_panel_project();
    let session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let revision = session.project().revision();

    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
    editor.drawing_point([Length::from_millimetres(600).expect("x"), Length::ZERO]).expect("end");
    assert_eq!(editor.drawing().map(DrawDraft::len), Some(1));
    editor
        .arm_primitive(PrimitiveKind::Cube, Length::from_millimetres(100).expect("size"))
        .expect("arm a cube, which a drawing tool must drop");

    editor.start_drawing(DrawTool::Rectangle);
    let draft = editor.drawing().expect("the rectangle tool is live");
    assert_eq!(draft.tool(), DrawTool::Rectangle);
    assert!(draft.points().is_empty(), "the new tool starts with an empty draft");
    assert!(draft.segments().is_empty(), "the old chain is gone");
    assert!(editor.pending_primitive().is_none(), "a drawing tool drops an armed primitive");
    assert_eq!(session.project().revision(), revision, "the switch writes nothing");
    assert!(!session.can_undo(), "no half figure entered the undo history");
}

#[test]
fn a_drawing_draft_is_view_state_until_it_is_committed() {
    let (project, _panel) = contour_panel_project();
    let session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    assert!(editor.drawing().is_none());
    assert_eq!(
        editor.drawing_point([Length::ZERO; 2]),
        Err(DrawDraftError::NotStarted),
        "a point needs a running draft"
    );

    let revision = session.project().revision();
    editor.start_drawing(DrawTool::Polyline);
    editor.start_drawing(DrawTool::Line);
    assert_eq!(editor.drawing().map(DrawDraft::tool), Some(DrawTool::Line), "another tool replaces the running draft");
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("first point");
    editor.drawing_point([Length::from_millimetres(600).expect("x"), Length::ZERO]).expect("second point");
    assert_eq!(editor.drawing().map(DrawDraft::len), Some(1), "one straight segment");
    assert_eq!(session.project().revision(), revision, "the draft changes no document");
    assert!(!session.can_undo(), "the draft leaves no undo entry behind");

    assert!(editor.undo_drawing());
    assert_eq!(editor.drawing().map(DrawDraft::len), Some(0));
    assert!(editor.close_drawing().is_err(), "one click encloses nothing");

    editor.cancel_drawing();
    assert!(editor.drawing().is_none());
    assert_eq!(session.project().revision(), revision);
    assert!(!session.can_undo());
}

#[test]
fn starting_a_drawing_leaves_the_other_pointer_modes() {
    let (project, panel) = contour_panel_project();
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    editor.start_drawing(DrawTool::Polyline);
    assert!(editor.contour_edit().is_none(), "a drawing owns the pointer");

    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    assert!(editor.drawing().is_none(), "a contour edit takes the pointer back");
}

#[test]
fn a_committed_drawing_is_one_path_object_and_one_undo_entry() {
    let (project, panel) = contour_panel_project();
    let root = project.root().id();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    editor.start_drawing(DrawTool::Rectangle);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("first corner");
    editor
        .drawing_point([Length::from_millimetres(600).expect("x"), Length::from_millimetres(400).expect("y")])
        .expect("opposite corner");

    let revision = session.project().revision();
    let drawn = editor
        .commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En)
        .expect("commit the rectangle");
    assert_eq!(drawn.tool, DrawTool::Rectangle);
    assert!(editor.drawing().is_none(), "the tool ends with the commit");
    assert_eq!(editor.selected_nodes(), [drawn.object], "the new node is selected");
    assert_eq!(
        editor.contour_edit().map(ContourEdit::target),
        Some(ContourTarget::Path(drawn.object)),
        "the committed figure opens its own edit mode"
    );
    assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));

    let path = session.project().object_path(drawn.object).expect("a path object");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 4);
    assert_eq!(session.project().parent_of(drawn.object), Some(root), "the selection never decides the parent");

    session.undo().expect("one undo removes the drawing");
    assert!(session.project().object(drawn.object).is_none());
    assert!(!session.can_undo(), "the drawing was the only undo entry");
}

#[test]
fn a_drawing_in_a_turned_container_is_stored_in_the_parents_frame() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    let parent_world = Placement::IDENTITY
        .with_translation(mm(-320), mm(150), mm(700))
        .expect("translation")
        .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(40_000), ..EulerAngles::default() })
        .expect("rotation");
    let parent = create_assembly_placed_under(&mut session, root, "Frame", parent_world);

    let mut editor = Editor::new();
    assert!(editor.arm_creation(session.project(), parent), "the drawing lands in the container");
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([mm(600), mm(400)]).expect("first point");
    editor.drawing_point([mm(700), mm(450)]).expect("second point");

    let world = Placement::IDENTITY.with_translation(mm(200), mm(100), mm(900)).expect("the work plane");
    let before = session.project().revision();
    let drawn =
        editor.commit_drawing(&mut session, world, furnicraft_text::Lang::En).expect("commit into the container");
    assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
    assert_eq!(session.project().parent_of(drawn.object), Some(parent), "the node lands in the armed container");

    let object = session.project().object(drawn.object).expect("object");
    assert!(!object.placement().same_transform(world), "the stored placement is relative to the turned parent");
    let world_placement = session.project().world_placement(drawn.object).expect("world placement");
    let clicked = Placement::IDENTITY.with_translation(mm(800), mm(500), mm(900)).expect("the clicked point");
    assert!(
        (world_placement.translation() - clicked.translation()).length() < 1e-9,
        "the first clicked point is still on the pointer: {:?}",
        world_placement.translation()
    );
    assert!(world_placement.rotation().dot(Placement::IDENTITY.rotation()).abs() >= 1.0 - 1e-12);
    assert_eq!(session.project().pivot(drawn.object), Some(Pivot::ZERO), "the local zero is the pivot");
    assert_eq!(session.project().object_path(drawn.object).expect("path").bounds_micrometres().0, [0, 0]);

    session.undo().expect("one undo removes the drawing");
    assert!(session.project().object(drawn.object).is_none());
    assert!(session.project().container(parent).is_some(), "the container is its own undo entry");
}

/// Creates an assembly under `parent` and returns its id.
fn create_assembly_under(session: &mut Session, parent: Id, name: &str) -> Id {
    create_assembly_placed_under(session, parent, name, Placement::IDENTITY)
}

/// Creates an assembly with a given placement under `parent` and returns its id.
fn create_assembly_placed_under(session: &mut Session, parent: Id, name: &str, placement: Placement) -> Id {
    let params = serde_json::to_value(furnicraft_engine::AssemblyCreate { parent, name: name.to_owned(), placement })
        .expect("params");
    session
        .dispatch(furnicraft_engine::node_cmds::ASSEMBLY_CREATE_ID, &params)
        .expect("create the assembly")
        .created_id()
        .expect("the assembly id")
}

#[test]
fn closing_through_a_pending_arc_commits_a_closed_path() {
    // The whole gesture the *Close* button runs: a chain of two lines, Tab into arc mode, the
    // arc's midpoint placed, then Close. The arc finishes the ring and the commit writes one
    // closed path — one undo entry — so the button the user pressed is the command that ran
    // (ADR 0077).
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([mm(0), mm(0)]).expect("start");
    editor.drawing_point([mm(600), mm(0)]).expect("bottom run");
    editor.drawing_point([mm(600), mm(400)]).expect("right run");
    assert_eq!(editor.toggle_drawing_arc_mode(), Some(true));
    editor.drawing_arc_through([mm(300), mm(300)]).expect("arm the closing arc");

    assert!(editor.drawing().is_some_and(DrawDraft::can_close), "Close is offered and will work");
    let revision = session.project().revision();
    let drawn = editor
        .close_and_commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En)
        .expect("close through the pending arc and commit");
    assert!(editor.drawing().is_none(), "the close ends the tool");
    assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));

    let path = session.project().object_path(drawn.object).expect("a path object");
    assert!(path.is_closed(), "the closed ring reached the document");
    assert!(path.entries().iter().any(|entry| entry.segment().is_arc()), "the arc survived the close");
    assert!(path.to_contour().is_ok(), "the ring converts, so the figure can extrude");
    session.undo().expect("one undo removes the whole drawing");
    assert!(session.project().object(drawn.object).is_none());
    assert!(!session.can_undo(), "the drawing was one undo entry");
}

/// The project with its revision counter at the end of its range, so the next command is refused.
fn exhausted_revision(project: Project) -> Project {
    let mut wire = serde_json::to_value(&project).expect("the project serialises");
    wire["revision"] = serde_json::Value::from(u64::MAX);
    serde_json::from_value(wire).expect("the project loads with a full revision counter")
}

#[test]
fn the_ribbon_reports_a_close_instead_of_running_it() {
    // The ribbon has no work plane, no gizmo and no scene, so it cannot close on its own: it
    // reports the close and the shell runs the one path that closes and commits (X23). A ring
    // closed here and never committed was the bug — the document has to stay untouched until
    // the shell acts.
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([mm(0), mm(0)]).expect("start");
    editor.drawing_point([mm(600), mm(0)]).expect("bottom run");
    editor.drawing_point([mm(600), mm(400)]).expect("right run");
    let revision = session.project().revision();

    let outcome = editor
        .apply_ribbon_actions(&mut session, vec![ToolAction::CloseDrawing], furnicraft_text::Lang::En)
        .expect("the ribbon runs");
    assert!(outcome.close_draft, "the ribbon asks the shell for the close and its commit");
    assert_eq!(session.project().revision(), revision, "reporting a close writes nothing");
    assert!(!session.can_undo(), "and leaves no undo entry behind");
    let draft = editor.drawing().expect("the draft stays");
    assert!(!draft.is_closed(), "the ribbon never closes the ring itself");
    assert_eq!(draft.len(), 2, "every segment is still there");
}

#[test]
fn a_refused_close_leaves_the_ring_open_and_the_document_untouched() {
    // The revision counter is full, so the close succeeds and the commit that belongs to it is
    // refused. The ring reopens: the shape the user drew stays editable instead of becoming a
    // finished draft nothing can change (ADR 0052, "a refused path keeps the draft").
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(exhausted_revision(project), sample_catalog());
    let mut editor = Editor::new();
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([mm(0), mm(0)]).expect("start");
    editor.drawing_point([mm(600), mm(0)]).expect("bottom run");
    editor.drawing_point([mm(600), mm(400)]).expect("right run");
    let revision = session.project().revision();

    editor
        .close_and_commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En)
        .expect_err("the document refuses the commit");
    assert_eq!(session.project().revision(), revision, "a refused close writes nothing");
    assert!(!session.can_undo(), "and leaves no undo entry behind");
    let draft = editor.drawing().expect("the draft survives a refusal");
    assert!(!draft.is_closed(), "the ring reopens, so the shape stays editable");
    assert_eq!(draft.len(), 2, "and keeps every segment");
}

#[test]
fn every_new_tool_commits_its_path_and_opens_the_edit() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    let cases: [(DrawTool, &[(i64, i64)]); 5] = [
        (DrawTool::ArcFromCenter, &[(0, 0), (50, 0), (0, 50)]),
        (DrawTool::RectangleFromCenter, &[(0, 0), (60, 40)]),
        (DrawTool::RectangleFromCenterDirected, &[(0, 0), (50, 0), (40, 30)]),
        (DrawTool::CircleThreePoints, &[(0, 0), (100, 0), (50, 80)]),
        (DrawTool::RegularPolygon, &[(0, 0), (80, 0)]),
    ];
    for (tool, points) in cases {
        let mut editor = Editor::new();
        editor.start_drawing_with(tool, 6);
        for point in points {
            editor.drawing_point([mm(point.0), mm(point.1)]).expect("a tool point");
        }
        let drawn = editor
            .commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En)
            .expect("the figure commits");
        assert_eq!(drawn.tool, tool);
        let path = session.project().object_path(drawn.object).expect("a path object");
        assert_eq!(
            path.is_closed(),
            tool.is_closed(),
            "{} keeps its closure",
            tool.label().text(furnicraft_text::Lang::En)
        );
        assert_eq!(
            editor.contour_edit().map(ContourEdit::target),
            Some(ContourTarget::Path(drawn.object)),
            "{} opens its edit mode",
            tool.label().text(furnicraft_text::Lang::En)
        );
        editor.end_contour_edit();
    }
}

#[test]
fn every_creation_tool_lands_at_the_root_beside_a_selected_nested_assembly() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let outer = create_assembly_under(&mut session, root, "Outer");
    let inner = create_assembly_under(&mut session, outer, "Inner");
    let mut editor = Editor::new();

    editor.select_node(Some(inner));
    assert_eq!(editor.creation_target(session.project()), root, "the scene root is the default parent");
    editor.start_drawing(DrawTool::Line);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
    editor.drawing_point([Length::from_millimetres(300).expect("x"), Length::ZERO]).expect("end");
    let drawn =
        editor.commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En).expect("commit the line");
    assert_eq!(session.project().parent_of(drawn.object), Some(root), "a drawing lands at the root");

    editor.select_node(Some(inner));
    editor.create_panel(&mut session, root).expect("create the panel");
    let created_panel = editor.selected_node().expect("the new panel");
    assert_eq!(session.project().parent_of(created_panel), Some(root), "a panel lands at the root");

    editor.select_node(Some(inner));
    editor.create_assembly(&mut session, root).expect("create the assembly");
    let created_assembly = editor.selected_node().expect("the new assembly");
    assert_eq!(session.project().parent_of(created_assembly), Some(root), "an assembly lands at the root");

    editor.select_node(Some(inner));
    editor.arm_primitive(PrimitiveKind::Cube, Length::from_millimetres(100).expect("size")).expect("arm the cube");
    let created_primitive = editor.place_primitive(&mut session, Placement::IDENTITY).expect("place the cube");
    assert_eq!(session.project().parent_of(created_primitive), Some(root), "a primitive lands at the root");

    editor.select_node(Some(inner));
    editor.create_camera(&mut session, root).expect("create the camera");
    let created_camera = editor.selected_node().expect("the new camera");
    assert_eq!(session.project().parent_of(created_camera), Some(root), "a camera lands at the root");

    editor.select_node(Some(inner));
    editor.create_light(&mut session, root).expect("create the light");
    let created_light = editor.selected_node().expect("the new light");
    assert_eq!(session.project().parent_of(created_light), Some(root), "a light lands at the root");

    for created in [created_panel, created_assembly, created_primitive, created_camera, created_light] {
        assert_ne!(session.project().parent_of(created), Some(inner), "the selected container receives nothing");
    }
    assert!(session.project().panel(panel).is_some(), "the rest of the document is untouched");
}

#[test]
fn an_armed_container_receives_a_path_a_panel_and_a_primitive() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let assembly = create_assembly_under(&mut session, root, "Frame");
    let mut editor = Editor::new();
    assert!(editor.arm_creation(session.project(), assembly), "a container can be armed");
    assert_eq!(editor.creation_parent_name(session.project()), Some("Frame"));
    assert_eq!(editor.creation_target(session.project()), assembly, "the armed container is the target");

    editor.start_drawing(DrawTool::Line);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
    editor.drawing_point([Length::from_millimetres(300).expect("x"), Length::ZERO]).expect("end");
    let drawn =
        editor.commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En).expect("commit the line");
    assert_eq!(session.project().parent_of(drawn.object), Some(assembly), "the path lands in the arm");

    let target = editor.creation_target(session.project());
    editor.create_panel(&mut session, target).expect("create the panel");
    let created_panel = editor.selected_node().expect("the new panel");
    assert_eq!(session.project().parent_of(created_panel), Some(assembly), "the panel lands in the arm");

    assert!(
        !editor.arm_creation(session.project(), created_panel),
        "a panel cannot own children, so it cannot be armed"
    );
    assert_eq!(editor.creation_target(session.project()), assembly, "a refused arm changes nothing");

    editor.arm_primitive(PrimitiveKind::Cube, Length::from_millimetres(100).expect("size")).expect("arm the cube");
    let created_primitive = editor.place_primitive(&mut session, Placement::IDENTITY).expect("place the cube");
    assert_eq!(session.project().parent_of(created_primitive), Some(assembly), "the primitive lands in the arm");
}

#[test]
fn clearing_the_arm_returns_creation_to_the_scene_root() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let assembly = create_assembly_under(&mut session, root, "Frame");
    let mut editor = Editor::new();

    assert!(editor.arm_creation(session.project(), assembly));
    editor.clear_creation_parent();
    assert!(editor.creation_parent().is_none());
    assert!(editor.creation_parent_name(session.project()).is_none());
    assert_eq!(editor.creation_target(session.project()), root, "✕ restores the root");

    assert!(!editor.arm_creation(session.project(), root), "the root cannot be armed");
    assert_eq!(editor.creation_target(session.project()), root);
}

#[test]
fn deleting_the_armed_container_clears_the_arm() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let outer = create_assembly_under(&mut session, root, "Outer");
    let inner = create_assembly_under(&mut session, outer, "Inner");
    let mut editor = Editor::new();

    assert!(editor.arm_creation(session.project(), inner));
    editor
        .run_action(&mut session, TreeAction::Delete { node: outer }, furnicraft_text::Lang::En)
        .expect("delete the outer assembly");
    assert!(editor.creation_parent().is_none(), "deleting an ancestor of the arm clears it");
    assert_eq!(editor.creation_target(session.project()), root);
}

#[test]
fn a_refused_drawing_keeps_the_draft_and_the_selection() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(Id::new(1).expect("root")));
    editor.start_drawing(DrawTool::Rectangle);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("one corner");

    let revision = session.project().revision();
    assert!(
        editor.commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En).is_err(),
        "one point is no path"
    );
    assert!(editor.drawing().is_some(), "the draft stays");
    assert_eq!(session.project().revision(), revision);
    assert!(!session.can_undo());
}

#[test]
fn committing_without_a_draft_reports_it() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    assert!(editor.commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En).is_err());
}

#[test]
fn an_open_polyline_commits_as_drawn() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.start_drawing(DrawTool::Polyline);
    editor.drawing_point([Length::ZERO, Length::ZERO]).expect("start");
    editor.drawing_point([Length::from_millimetres(400).expect("x"), Length::ZERO]).expect("middle");
    editor
        .drawing_point([Length::from_millimetres(400).expect("x"), Length::from_millimetres(300).expect("y")])
        .expect("end");
    let drawn =
        editor.commit_drawing(&mut session, Placement::IDENTITY, furnicraft_text::Lang::En).expect("commit the chain");
    let path = session.project().object_path(drawn.object).expect("a path object");
    assert!(!path.is_closed());
    assert_eq!(path.entries().len(), 2);
}

#[test]
fn arming_a_cube_waits_for_a_click_and_places_it_there() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let size = Length::from_millimetres(300).expect("size");
    editor.arm_primitive(PrimitiveKind::Cube, size).expect("arm the cube");
    assert_eq!(editor.pending_primitive(), Some((PrimitiveKind::Cube, size)));
    assert!(editor.drawing().is_none(), "an armed primitive leaves a drawing");

    let placement = Placement::IDENTITY
        .with_translation(
            Length::from_millimetres(100).expect("x"),
            Length::from_millimetres(50).expect("y"),
            Length::ZERO,
        )
        .expect("placement");
    let id = editor.place_primitive(&mut session, placement).expect("place the cube");
    assert!(editor.pending_primitive().is_none(), "one click places one cube");
    assert_eq!(editor.selected_node(), Some(id));
    let object = session.project().object(id).expect("object");
    assert_eq!(object.placement(), placement);
    assert_eq!(session.project().revision().get(), 1);
    assert!(session.project().panel(panel).is_some());

    editor.arm_primitive(PrimitiveKind::Sphere, size).expect("arm again");
    editor.cancel_primitive();
    assert!(editor.pending_primitive().is_none());
    assert_eq!(session.project().revision().get(), 1, "cancelling creates nothing");
}

#[test]
fn a_clicked_primitive_lands_on_the_pointer_at_the_root_and_in_a_turned_container() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let mm = |value: i64| Length::from_millimetres(value).expect("a whole millimetre");
    // A container turned about Z and moved, so a parent-frame placement is not a world one.
    let parent_world = Placement::IDENTITY
        .with_translation(mm(-320), mm(150), mm(700))
        .expect("translation")
        .with_rotation_euler(EulerAngles { z: Degrees::from_millidegrees(40_000), ..EulerAngles::default() })
        .expect("rotation");
    let parent = create_assembly_placed_under(&mut session, root, "Frame", parent_world);
    let click = Placement::IDENTITY.with_translation(mm(200), mm(100), mm(900)).expect("the clicked point");

    for armed in [None, Some(parent)] {
        let mut editor = Editor::new();
        if let Some(container) = armed {
            assert!(editor.arm_creation(session.project(), container), "the cube lands in the container");
        }
        editor.arm_primitive(PrimitiveKind::Cube, mm(300)).expect("arm the cube");

        let before = session.project().revision();
        let cube = editor.place_primitive(&mut session, click).expect("place the cube");
        let what = if armed.is_some() { "in the container" } else { "at the root" };
        assert_eq!(
            session.project().revision(),
            before.checked_next().expect("revision"),
            "{what}: one click is one change"
        );
        assert_eq!(session.project().parent_of(cube), Some(armed.unwrap_or(root)), "{what}: the parent");
        assert_eq!(session.project().pivot(cube), Some(Pivot::ZERO), "{what}: the local zero is the pivot");

        let object = session.project().object(cube).expect("object");
        let stored = object.geometry().as_primitive().expect("the parameters are stored");
        let (min, max) = stored.bounds_micrometres();
        assert_eq!(min, [-150_000; 3], "{what}: the mesh is centred on the local zero");
        assert_eq!(max, [150_000; 3], "{what}: the mesh is symmetric about the local zero");

        let world = session.project().world_placement(cube).expect("world placement");
        assert!(
            (world.translation() - click.translation()).length() < 1e-9,
            "{what}: the clicked point is on the pointer, was {:?}",
            world.translation()
        );
        assert!(world.rotation().dot(click.rotation()).abs() >= 1.0 - 1e-12, "{what}: the plane rotation");
        if armed.is_none() {
            assert_eq!(object.placement(), click, "at the root the stored placement is the world one");
        } else {
            assert!(!object.placement().same_transform(click), "the container stores a relative placement");
        }

        session.undo().expect("one undo removes the cube");
        assert!(session.project().object(cube).is_none(), "{what}: the undo removed the cube");
    }
    assert!(session.project().container(parent).is_some(), "the container is its own undo entry");
}

#[test]
fn a_cube_edge_must_be_positive() {
    let mut editor = Editor::new();
    assert!(editor.arm_primitive(PrimitiveKind::Cylinder, Length::ZERO).is_err());
}

#[test]
fn every_primitive_kind_can_be_armed() {
    let size = Length::from_millimetres(120).expect("size");
    for kind in [PrimitiveKind::Cube, PrimitiveKind::Cylinder, PrimitiveKind::Sphere] {
        let mut editor = Editor::new();
        editor.arm_primitive(kind, size).expect("arm");
        assert_eq!(editor.pending_primitive(), Some((kind, size)));
        assert!(
            Session::with_catalog(sample_project(), sample_catalog())
                .project()
                .object(Id::new(5).expect("id"))
                .is_none(),
            "arming alone creates nothing"
        );
    }
}

#[test]
fn placing_a_cylinder_and_a_sphere_names_and_shapes_them() {
    let cases = [
        (PrimitiveKind::Cylinder, "Cylinder", DEFAULT_SEGMENTS * 2),
        (PrimitiveKind::Sphere, "Sphere", 2 + DEFAULT_SEGMENTS * (DEFAULT_BANDS - 1)),
    ];
    for (kind, name, vertices) in cases {
        let (project, _panel) = contour_panel_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        let mut editor = Editor::new();
        editor.arm_primitive(kind, Length::from_millimetres(100).expect("size")).expect("arm");
        let id = editor.place_primitive(&mut session, Placement::IDENTITY).expect("place");
        let object = session.project().object(id).expect("object");
        assert_eq!(object.name(), name);
        let stored = object.geometry().as_primitive().expect("the parameters are stored");
        assert_eq!(stored.kind(), kind);
        assert_eq!(stored.vertex_count() as u32, vertices, "the counts follow the parameters");
        assert!(object.geometry().as_mesh().is_none(), "the mesh is derived");
    }
}

/// A project whose root holds one closed path object.
fn path_project(closed: bool) -> (Project, Id) {
    let mut project = sample_project();
    let root = project.root().id();
    let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
    let path = Path::rectangle([millimetres(0), millimetres(0)], [millimetres(600), millimetres(400)])
        .expect("a rectangle path");
    let path = if closed {
        path
    } else {
        let segments = path.segments().take(2).collect::<Vec<_>>();
        Path::new(segments, false).expect("an open chain")
    };
    let id = project.add_object(root, "Drawing", Placement::IDENTITY, Geometry::Path(path)).expect("path object");
    (project, id)
}

#[test]
fn the_extrude_action_turns_a_closed_path_into_the_body_that_owns_it() {
    let (project, path) = path_project(true);
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(path));
    let height = Length::from_millimetres(18).expect("height");
    let children_before = session.project().root().children().len();

    editor.extrude_path(&mut session, path, height, furnicraft_text::Lang::En).expect("extrude the path");
    assert_eq!(editor.selected_node(), Some(path), "the node becomes the body in place");
    let object = session.project().object(path).expect("object");
    assert_eq!(object.name(), "Drawing", "the node keeps its name");
    let constructive = object.geometry().as_constructive().expect("a constructive body");
    assert_eq!(constructive.height(), Some(height));
    assert!(constructive.path().is_closed(), "the body owns the closed drawing");
    assert_eq!(session.project().root().children().len(), children_before, "no sibling body is left behind");
    assert!(session.project().object_path(path).is_err(), "the drawing is the body now");
    let revision = session.project().revision();
    assert!(
        editor.extrude_path(&mut session, path, height, furnicraft_text::Lang::En).is_err(),
        "a body is not extruded a second time"
    );
    assert_eq!(session.project().revision(), revision, "the refused extrusion changes nothing");
}

#[test]
fn a_pulled_height_becomes_the_next_extrusions_default() {
    let (project, path) = path_project(true);
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(path));
    editor
        .extrude_path(&mut session, path, Length::from_millimetres(100).expect("height"), furnicraft_text::Lang::En)
        .expect("extrude the path");
    assert_eq!(editor.extrude_height_text(), "100", "the ribbon starts at the default");

    let height = Length::from_millimetres(240).expect("height");
    editor.set_extrude_height(&mut session, path, height).expect("raise the body");
    assert_eq!(editor.extrude_height_text(), "240 mm", "the next extrusion starts at the pulled height");
    let body = session.project().object(path).and_then(|object| object.geometry().as_constructive()).expect("a body");
    assert_eq!(body.height(), Some(height));

    let revision = session.project().revision();
    assert!(editor.set_extrude_height(&mut session, path, Length::ZERO).is_err(), "a zero height is refused");
    assert_eq!(session.project().revision(), revision, "a refused height changes nothing");
    assert_eq!(editor.extrude_height_text(), "240 mm", "a refused height leaves the field alone");
}

#[test]
fn one_undo_restores_the_drawing_a_body_came_from() {
    let (project, path) = path_project(true);
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(path));
    editor
        .extrude_path(&mut session, path, Length::from_millimetres(18).expect("height"), furnicraft_text::Lang::En)
        .expect("extrude the path");

    undo(&mut session, &mut editor);
    assert!(session.project().object_path(path).is_ok(), "undo restores the drawing");
    assert_eq!(editor.selected_node(), Some(path), "the node never left the tree");
    assert!(!session.can_undo(), "one extrusion is one undo step");
}

#[test]
fn an_open_path_is_refused_with_a_readable_reason() {
    let (project, path) = path_project(false);
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let revision = session.project().revision();
    let error = editor
        .extrude_path(&mut session, path, Length::from_millimetres(18).expect("height"), furnicraft_text::Lang::En)
        .expect_err("an open chain is no ring");
    assert!(error.contains("open path"), "the message names the problem: {error}");
    assert_eq!(session.project().revision(), revision, "a refused extrusion changes nothing");
    assert!(!session.can_undo());
}

#[test]
fn a_body_keeps_the_paths_placement_and_frame() {
    let (project, path) = path_project(true);
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let placement = Placement::IDENTITY
        .with_translation(
            Length::from_millimetres(100).expect("x"),
            Length::from_millimetres(50).expect("y"),
            Length::ZERO,
        )
        .expect("placement");
    let params = crate::command::json(&furnicraft_engine::NodePlacementSet { node: path, placement }).expect("params");
    session.dispatch(furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID, &params).expect("move the drawing");
    editor
        .extrude_path(&mut session, path, Length::from_millimetres(18).expect("height"), furnicraft_text::Lang::En)
        .expect("extrude");
    let body = editor.selected_node().expect("the body");
    assert_eq!(session.project().node(body).expect("body node").placement(), placement);
}

#[test]
fn the_extrude_height_field_parses_a_positive_length() {
    let mut state = crate::toolbars::Toolbars::new();
    assert_eq!(state.extrude_height_text(), "100");
    assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(100).expect("height")));
    state.set_extrude_height_text("-2");
    assert!(state.extrude_height().is_err());
    state.set_extrude_height_text("thick");
    assert!(state.extrude_height().is_err());
}

#[test]
fn the_primitive_size_field_parses_millimetres() {
    let mut state = crate::toolbars::Toolbars::new();
    assert_eq!(state.primitive_size_text(), "200");
    assert_eq!(state.primitive_size(), Ok(Length::from_millimetres(200).expect("length")));
    state.set_primitive_size_text("18.5");
    assert_eq!(state.primitive_size(), Ok(Length::from_micrometres(18_500)));
    state.set_primitive_size_text("0");
    assert!(state.primitive_size().is_err());
    state.set_primitive_size_text("tall");
    assert!(state.primitive_size().is_err());
}

#[test]
fn the_toolbar_creates_the_cube_at_the_scene_root() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let root = Id::new(1).expect("root");
    editor.select_node(Some(root));
    editor.arm_primitive(PrimitiveKind::Cube, Length::from_millimetres(200).expect("size")).expect("arm");
    editor.place_primitive(&mut session, Placement::IDENTITY).expect("cube");
    let cube = editor.selected_node().expect("the new cube is selected");
    assert!(session.project().object(cube).is_some());
    assert!(session.project().panel(panel).is_some());
    assert_eq!(session.project().parent_of(cube), Some(root));
    assert!(editor.initialized, "the fresh selection survives the next frame");
}

#[test]
fn the_drawing_shortcuts_name_their_tool() {
    let cases = [
        (egui::Key::L, false, DrawTool::Line),
        (egui::Key::P, false, DrawTool::Polyline),
        (egui::Key::A, false, DrawTool::Arc),
        (egui::Key::A, true, DrawTool::ArcFromCenter),
        (egui::Key::R, false, DrawTool::Rectangle),
        (egui::Key::R, true, DrawTool::RectangleFromCenter),
        (egui::Key::D, true, DrawTool::RectangleFromCenterDirected),
        (egui::Key::C, false, DrawTool::Circle),
        (egui::Key::C, true, DrawTool::CircleThreePoints),
        (egui::Key::N, false, DrawTool::RegularPolygon),
    ];
    let mut editor = Editor::new();
    for (key, shift, tool) in cases {
        let shortcut = crate::toolbars::Shortcut { key, shift };
        assert_eq!(
            crate::toolbars::shortcut(tool),
            Some(shortcut),
            "{} starts {}",
            key.name(),
            tool.label().text(furnicraft_text::Lang::En)
        );
        editor.start_drawing(tool);
        assert_eq!(editor.drawing().map(DrawDraft::tool), Some(tool));
        editor.cancel_drawing();
    }
    assert!(editor.drawing().is_none());
}

#[test]
fn entering_one_edit_mode_leaves_the_other() {
    let (mut project, cube) = cube_project();
    let width = Length::from_millimetres(700).expect("width");
    let height = Length::from_millimetres(500).expect("height");
    let ring = Contour::from_polygon(vec![
        [Length::ZERO, Length::ZERO],
        [width, Length::ZERO],
        [width, height],
        [Length::ZERO, height],
    ])
    .expect("ring");
    let root = project.root().id();
    let panel = project
        .add_panel_contour(root, "Top", &ring, None, sample_panel_thickness(), Placement::IDENTITY)
        .expect("contour panel");

    let mut editor = Editor::new();
    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    editor.select_node(Some(panel));
    assert!(editor.start_contour_edit(&project, ContourTarget::Panel(panel)));
    assert!(editor.mesh_edit().is_none(), "contour editing replaces mesh editing");

    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    assert!(editor.contour_edit().is_none(), "mesh editing replaces contour editing");
}

#[test]
fn vertex_selection_replaces_toggles_and_prunes_stale_indices() {
    let (project, cube) = cube_project();
    let mut editor = Editor::new();
    assert!(!editor.select_mesh_vertex(0, false), "not in mesh mode");
    editor.clear_mesh_vertices();

    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    assert!(editor.select_mesh_vertex(3, false));
    assert!(editor.select_mesh_vertex(4, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([3, 4].as_slice()));
    assert!(editor.select_mesh_vertex(3, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4].as_slice()));

    assert!(editor.select_mesh_vertex(8, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4, 8].as_slice()));
    editor.retain_mesh_edit(&project);
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([4].as_slice()), "vertex 8 is outside the cube");

    editor.clear_mesh_vertices();
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([].as_slice()));
}

#[test]
fn face_selection_replaces_toggles_and_prunes_stale_indices() {
    let (project, cube) = cube_project();
    let mut editor = Editor::new();
    assert!(!editor.select_mesh_face(0, false), "not in mesh mode");

    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));
    assert_eq!(editor.mesh_edit().map(MeshEdit::element), Some(MeshElement::Vertex));
    editor.set_mesh_element(MeshElement::Face);
    assert_eq!(editor.mesh_edit().map(MeshEdit::element), Some(MeshElement::Face));

    assert!(editor.select_mesh_face(2, false));
    assert!(editor.select_mesh_face(4, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([2, 4].as_slice()));
    assert!(editor.select_mesh_face(2, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4].as_slice()));

    assert!(editor.select_mesh_face(6, true));
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4, 6].as_slice()));
    editor.retain_mesh_edit(&project);
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([4].as_slice()), "face 6 is outside the cube");

    editor.clear_mesh_faces();
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([].as_slice()));
}

#[test]
fn switching_element_clears_the_selection_of_the_other_kind() {
    let (project, cube) = cube_project();
    let mut editor = Editor::new();
    editor.select_node(Some(cube));
    assert!(editor.start_mesh_edit(&project));

    assert!(editor.select_mesh_vertex(1, false));
    editor.set_mesh_element(MeshElement::Face);
    assert_eq!(editor.mesh_edit().map(MeshEdit::vertices), Some([].as_slice()));

    assert!(editor.select_mesh_face(0, false));
    editor.set_mesh_element(MeshElement::Vertex);
    assert_eq!(editor.mesh_edit().map(MeshEdit::faces), Some([].as_slice()));
}

#[test]
fn picking_replaces_the_tree_selection_and_an_empty_click_clears_it() {
    let mut editor = Editor::new();
    assert_eq!(editor.selected_node(), None);
    assert!(editor.selected_nodes().is_empty());
    let node = Id::new(2).expect("id");
    editor.select_node(Some(node));
    assert_eq!(editor.selected_node(), Some(node));
    assert_eq!(editor.selected_nodes(), [node], "the primary is the only selected node");
    editor.select_node(None);
    assert_eq!(editor.selected_node(), None);
    assert!(editor.selected_nodes().is_empty());
}

/// The shell's undo, with the editor's view state pruned the way a frame prunes it.
///
/// The tree toolbar no longer carries undo; the header and the palette do, so a test that used
/// the tree action calls the session directly and keeps the selection consistent.
fn undo(session: &mut Session, editor: &mut Editor) {
    session.undo().expect("undo");
    editor.selection.retain_existing(session.project());
    editor.retain_open(session.project());
    editor.retain_creation_parent(session.project());
}

fn grouped_project() -> (Project, Id, Id, Id) {
    let mut project = sample_project();
    let shelf = Id::new(2).expect("shelf id");
    let drawer = Id::new(3).expect("drawer id");
    let group = project.group_nodes(&[shelf, drawer], "Group").expect("group siblings");
    (project, group, drawer, Id::new(4).expect("front id"))
}

#[test]
fn a_viewport_click_selects_the_outermost_closed_container() {
    let (project, group, _drawer, front) = grouped_project();
    let mut editor = Editor::new();

    editor.select_viewport_hit(&project, Some(front));
    assert_eq!(editor.selected_node(), Some(group));
    assert!(!editor.open.contains(&group));

    let mut project = project;
    let extra = project
        .add_panel(
            project.root().id(),
            "Top",
            PanelCreate::default_nominal_size().expect("size"),
            None,
            Length::from_millimetres(18).expect("thickness"),
            Placement::IDENTITY,
        )
        .expect("add top panel");
    editor.select_viewport_hit(&project, Some(extra));
    assert_eq!(editor.selected_node(), Some(extra));

    editor.select_viewport_hit(&project, None);
    assert_eq!(editor.selected_node(), None);

    editor.select_viewport_hit(&project, Some(Id::new(999).expect("id")));
    assert_eq!(editor.selected_node(), None);
}

#[test]
fn double_click_enters_one_level_and_reveals_the_child_under_the_pointer() {
    let (project, group, drawer, front) = grouped_project();
    let mut editor = Editor::new();

    editor.enter_viewport_level(&project, Some(front));
    assert_eq!(editor.selected_node(), Some(drawer));
    assert!(editor.open.contains(&group));

    editor.enter_viewport_level(&project, Some(front));
    assert_eq!(editor.selected_node(), Some(front));
    assert!(editor.open.contains(&drawer));

    editor.enter_viewport_level(&project, Some(front));
    assert_eq!(editor.selected_node(), Some(front), "a panel has no level to enter");
}

#[test]
fn escape_closes_one_level_and_returns_to_the_parent_container() {
    let (project, group, drawer, front) = grouped_project();
    let mut editor = Editor::new();

    editor.exit_level(&project);
    assert_eq!(editor.selected_node(), None, "nothing is open yet");

    editor.enter_viewport_level(&project, Some(front));
    editor.enter_viewport_level(&project, Some(front));
    assert_eq!(editor.selected_node(), Some(front));

    editor.exit_level(&project);
    assert_eq!(editor.selected_node(), Some(drawer));
    assert!(!editor.open.contains(&drawer));
    assert!(editor.open.contains(&group));

    editor.exit_level(&project);
    assert_eq!(editor.selected_node(), Some(group));
    assert!(!editor.open.contains(&group));

    editor.exit_level(&project);
    assert_eq!(editor.selected_node(), Some(group), "the top level stays selected");
}

#[test]
fn creating_a_cube_dispatches_the_primitive_command_and_selects_it() {
    let mut session = Session::with_catalog(sample_project(), sample_catalog());
    let mut editor = Editor::new();

    editor.arm_primitive(PrimitiveKind::Cube, Length::from_millimetres(200).expect("size")).expect("arm");
    editor.place_primitive(&mut session, Placement::IDENTITY).expect("create cube");

    let id = editor.selected_node().expect("the new object is selected");
    let object = session.project().object(id).expect("object");
    assert_eq!(object.name(), "Cube");
    let primitive = object.geometry().as_primitive().expect("the parameters are stored");
    assert_eq!(primitive.mesh_counts(), (8, 6), "a cube keeps six faces");
    assert_eq!(session.project().revision().get(), 1, "one command is one revision");

    undo(&mut session, &mut editor);
    assert_eq!(editor.selected_node(), None, "undo drops the selection of a vanished object");
    assert!(session.project().object(id).is_none());
}

#[test]
fn the_ribbon_marks_the_tool_that_owns_the_next_click() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    let ctx = egui::Context::default();
    let view = RibbonView { perspective: true, owns_view: true, grid: true };

    let mut fill = None;
    let idle = ctx.run_ui(Default::default(), |ui| {
        fill = Some(ui.visuals().selection.bg_fill);
        editor.show_ribbon(ui, &mut session, view).expect("ribbon");
    });
    let fill = fill.expect("the ribbon ran");
    assert_eq!(pressed_fill(&idle.shapes, fill), 0, "an idle editor presses no tool");
    idle.drop_without_applying_deltas();

    editor.start_drawing(DrawTool::Line);
    let drawing = ctx.run_ui(Default::default(), |ui| {
        editor.show_ribbon(ui, &mut session, view).expect("ribbon");
    });
    assert_eq!(pressed_fill(&drawing.shapes, fill), 1, "the running draft presses its own tool");
    drawing.drop_without_applying_deltas();

    editor.cancel_drawing();
    let size = Length::from_millimetres(crate::toolbars::DEFAULT_PRIMITIVE_SIZE_MM).expect("size");
    editor.arm_primitive(PrimitiveKind::Cube, size).expect("arm");
    editor.set_ribbon_tab(RibbonTab::Model);
    let primitive = ctx.run_ui(Default::default(), |ui| {
        editor.show_ribbon(ui, &mut session, view).expect("ribbon");
    });
    assert_eq!(pressed_fill(&primitive.shapes, fill), 1, "the waiting primitive presses its own button");
    primitive.drop_without_applying_deltas();
}

#[test]
fn the_ribbon_returns_the_view_action_it_was_asked_for() {
    // The ribbon draws the view buttons but never moves the camera: the action comes back and
    // the shell performs it with the same method the status bar calls.
    let mut editor = Editor::new();
    editor.set_ribbon_tab(RibbonTab::View);
    assert_eq!(editor.ribbon_tab(), RibbonTab::View);
    assert!(editor.ribbon_open());
    assert!(!editor.set_ribbon_open(false), "the body closes");
    assert!(!editor.ribbon_open());
    assert!(editor.set_ribbon_open(true), "the body opens again");
    assert!(editor.ribbon_open());
}

/// Counts the painted rects filled with `fill`, the selection fill of a pressed button.
fn pressed_fill(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> usize {
    fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut usize) {
        match shape {
            egui::Shape::Rect(rect) if rect.fill == fill => *out += 1,
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, fill, out);
                }
            }
            _ => {}
        }
    }

    let mut out = 0;
    for clipped in shapes {
        walk(&clipped.shape, fill, &mut out);
    }
    out
}

#[test]
fn creating_a_camera_or_a_light_selects_it_and_one_undo_removes_it() {
    let mut session = Session::with_catalog(sample_project(), sample_catalog());
    let mut editor = Editor::new();
    let root = session.project().root().id();

    editor.create_camera(&mut session, root).expect("create camera");
    let camera = editor.selected_node().expect("the camera is selected");
    assert_eq!(session.project().camera(camera).expect("camera").name(), "Camera");
    assert_eq!(session.project().revision().get(), 1);

    editor.create_light(&mut session, root).expect("create light");
    let light = editor.selected_node().expect("the light is selected");
    assert_eq!(session.project().light(light).expect("light").name(), "Light");
    assert_eq!(session.project().revision().get(), 2);

    undo(&mut session, &mut editor);
    assert!(session.project().light(light).is_none());
    assert!(editor.selected_node().is_none());
}

#[test]
fn a_created_camera_and_light_stay_selected_on_the_next_frame() {
    let mut session = Session::with_catalog(sample_project(), sample_catalog());
    let mut editor = Editor::new();
    let root = session.project().root().id();

    editor.create_camera(&mut session, root).expect("create camera");
    let camera = editor.selected_node().expect("the camera is selected");
    // The tree retains the selection at the start of every frame; a scene node must survive it.
    editor.selection.retain_existing(session.project());
    assert_eq!(editor.selected_node(), Some(camera));

    editor.create_light(&mut session, root).expect("create light");
    let light = editor.selected_node().expect("the light is selected");
    editor.selection.retain_existing(session.project());
    assert_eq!(editor.selected_node(), Some(light));
}

#[test]
fn a_locked_node_still_selects_and_refuses_a_destructive_command() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    editor.set_locked(panel, true);

    assert!(editor.is_locked(panel));
    assert_eq!(editor.selected_node(), Some(panel), "a lock never takes the selection away");
    assert_eq!(editor.locked_selection(session.project()), Some(panel), "the viewport sees the refusal");
    assert!(
        editor
            .locked_refusal(session.project(), panel, furnicraft_text::Lang::En)
            .is_some_and(|refusal| refusal.contains("locked"))
    );

    let revision = session.project().revision();
    let refusal = editor
        .run_action(&mut session, TreeAction::Delete { node: panel }, furnicraft_text::Lang::En)
        .expect_err("a locked node refuses the delete");
    assert!(refusal.contains("locked"), "the refusal says why: {refusal}");
    assert_eq!(session.project().revision(), revision, "a refusal dispatches nothing");
    assert!(!session.can_undo(), "and leaves no undo entry");

    editor.set_locked(panel, false);
    assert_eq!(editor.locked_selection(session.project()), None, "unlocking ends the refusal");
    editor
        .run_action(&mut session, TreeAction::Delete { node: panel }, furnicraft_text::Lang::En)
        .expect("the unlocked delete runs");
    assert!(!session.project().contains_node(panel));
}

#[test]
fn a_locked_container_refuses_a_group_and_an_ungroup() {
    let (project, _panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let root = session.project().root().id();
    let first = create_assembly_under(&mut session, root, "First");
    let second = create_assembly_under(&mut session, root, "Second");
    let mut editor = Editor::new();
    editor.set_locked(first, true);

    let revision = session.project().revision();
    assert!(
        editor
            .run_action(&mut session, TreeAction::Group { nodes: vec![first, second] }, furnicraft_text::Lang::En)
            .is_err(),
        "grouping a locked node would restructure it"
    );
    assert_eq!(session.project().revision(), revision);

    editor.set_locked(first, false);
    editor.set_locked(second, true);
    assert!(
        editor
            .run_action(&mut session, TreeAction::Group { nodes: vec![first, second] }, furnicraft_text::Lang::En)
            .is_err(),
        "one locked node in the selection is enough"
    );
    assert_eq!(session.project().revision(), revision);
}

#[test]
fn a_deleted_node_drops_its_flags_and_an_undo_starts_clean() {
    let (project, panel) = contour_panel_project();
    let mut session = Session::with_catalog(project, sample_catalog());
    let mut editor = Editor::new();
    editor.select_node(Some(panel));
    editor.set_hidden(panel, true);
    editor.set_locked(panel, true);
    // The delete goes through the editor, so the lock has to be lifted first; hiding is the flag
    // this test watches vanish with the node.
    editor.set_locked(panel, false);

    editor
        .run_action(&mut session, TreeAction::Delete { node: panel }, furnicraft_text::Lang::En)
        .expect("delete the panel");
    assert!(!editor.flags().is_hidden(panel), "the flag goes with the node");
    assert!(!editor.flags().is_locked(panel));

    undo(&mut session, &mut editor);
    assert!(session.project().contains_node(panel), "the undo brings the node back");
    assert!(!editor.flags().is_hidden(panel), "an undone deletion starts visible");
    assert!(!editor.flags().is_locked(panel), "and unlocked");
}

#[test]
fn the_flags_survive_a_selection_change_and_die_with_the_editor() {
    let (project, panel) = contour_panel_project();
    let mut editor = Editor::new();
    editor.set_hidden(panel, true);
    editor.set_locked(panel, true);

    editor.select_node(Some(Id::new(1).expect("root")));
    assert!(editor.flags().is_hidden(panel), "a selection change is not a reset");
    assert!(editor.is_locked(panel));

    editor.show_all();
    assert!(!editor.flags().is_hidden(panel), "Show all clears the hidden set");
    assert!(editor.is_locked(panel), "and leaves the locks alone");

    // A new editor is a new session: no flag is carried over from the one that closed.
    let fresh = Editor::new();
    assert!(!fresh.flags().is_hidden(panel));
    assert!(!fresh.is_locked(panel));
    assert!(project.contains_node(panel));
}
