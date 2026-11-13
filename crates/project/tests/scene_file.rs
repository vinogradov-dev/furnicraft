//! Acceptance tests for the scene-file envelope, its golden file and atomic saving.
//!
//! One golden file carries the current schema: it is written by this build and must keep loading
//! after a change to the code. A file with any other schema number is a readable error, never a
//! migration — before the first release the format has no past (ADR 0096). Regenerate the golden
//! with `cargo test -p furnicraft-project --test make_golden -- --ignored`.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use furnicraft_model::{Container, Id, LightKind, NodeQuery, Placement, Project, StructuralKind};
use furnicraft_project::{MAX_SCENE_BYTES, SceneFile, SceneFileError};
use furnicraft_units::Length;

const GOLDEN: &str = include_str!("golden/scene.fcs");

/// A unique directory under the system temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);
        let name = format!(
            "furnicraft-scene-{tag}-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).expect("create the temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An id that the golden scene uses.
fn id(value: u64) -> Id {
    Id::new(value).expect("a non-zero id")
}

#[test]
fn the_golden_file_loads_at_the_current_schema_and_round_trips() {
    let scene = SceneFile::parse(GOLDEN).expect("the golden loads");
    assert_eq!(
        scene.document().schema_version(),
        Project::CURRENT_SCHEMA_VERSION,
        "the golden is written by this build"
    );

    let rewritten = scene.to_json().expect("serialize");
    let again = SceneFile::parse(&rewritten).expect("the rewritten scene loads");
    assert_eq!(again.document(), scene.document(), "the document survives a round trip");
    assert_eq!(again.extra(), scene.extra(), "the unknown section survives a round trip");
    assert_eq!(rewritten, GOLDEN, "the golden is exactly what this build writes");
}

#[test]
fn the_golden_file_carries_every_node_kind() {
    let scene = SceneFile::parse(GOLDEN).expect("the golden loads");
    let document = scene.document();

    let panel = document.panel(id(2)).expect("the shelf loads");
    assert_eq!(panel.machining().len(), 1, "the shelf carries one machining record");
    assert_eq!(panel.edge_banding().len(), 1, "the shelf carries one banding strip");
    assert_eq!(panel.nominal_size().length(), Length::from_millimetres(700).expect("length"));

    let top = document.object(id(3)).expect("the extrusion loads");
    let constructive = top.geometry().as_constructive().expect("a constructive payload");
    assert_eq!(constructive.contour().expect("the ring").segments().len(), 4);
    assert_eq!(constructive.height(), Some(Length::from_millimetres(18).expect("height")));

    let sketch = document.object(id(4)).expect("the drawn path loads");
    let path = sketch.geometry().as_path().expect("a path payload");
    assert_eq!(path.entries().len(), 1, "the chain keeps its segment");
    assert!(!path.is_closed(), "the drawn chain is open");

    let ball = document.object(id(5)).expect("the primitive loads");
    assert!(ball.geometry().as_primitive().is_some(), "a primitive stores its parameters");

    let camera = document.camera(id(6)).expect("the camera loads");
    assert_eq!(document.active_camera(), Some(camera.id()), "the file names its active camera");
    let light = document.light(id(7)).expect("the light loads");
    assert_eq!(light.kind(), LightKind::Sun);

    let drawer = document.container(id(8)).expect("the component loads");
    assert!(drawer.is_component(), "a container with a class is a component");
    assert!(document.panel(id(9)).is_some(), "the component holds its panel");
}

#[test]
fn the_golden_file_preserves_an_unknown_top_level_section() {
    let scene = SceneFile::parse(GOLDEN).expect("the golden loads");
    assert!(scene.extra().contains_key("generator"), "an unknown top-level section is preserved");

    let rewritten = scene.to_json().expect("serialize");
    assert!(rewritten.contains("\"generator\""), "the unknown section is written back");
    let again = SceneFile::parse(&rewritten).expect("the rewritten scene loads");
    assert_eq!(again.extra(), scene.extra());
}

#[test]
fn a_scene_saved_from_a_document_keeps_the_sections_it_was_loaded_with() {
    let loaded = SceneFile::parse(GOLDEN).expect("the golden loads");
    let saved = SceneFile::new(loaded.document().clone()).with_extra(loaded.extra().clone());

    assert_eq!(saved.extra(), loaded.extra());
    assert!(saved.to_json().expect("serialize").contains("\"generator\""));
}

#[test]
fn an_unknown_field_inside_the_document_is_a_readable_error() {
    let text = GOLDEN.replacen("\"revision\":", "\"future_field\": 1,\n    \"revision\":", 1);
    assert_ne!(text, GOLDEN, "the fixture must carry the revision line");
    let error = SceneFile::parse(&text).expect_err("an unknown document field must be rejected");
    assert!(error.to_string().contains("unknown field"), "error was {error}");
}

#[test]
fn a_newer_format_version_is_a_readable_error() {
    let text = GOLDEN.replacen("\"format_version\": 1", "\"format_version\": 2", 1);
    assert_ne!(text, GOLDEN);
    let error = SceneFile::parse(&text).expect_err("a newer envelope must be rejected");
    assert!(matches!(error, SceneFileError::UnsupportedFormatVersion { found: 2, .. }), "error was {error}");
}

#[test]
fn any_other_document_schema_is_the_same_readable_error() {
    let current = Project::CURRENT_SCHEMA_VERSION;
    for other in [current + 1, current - 1] {
        let text =
            GOLDEN.replacen(&format!("\"schema_version\": {current}"), &format!("\"schema_version\": {other}"), 1);
        assert_ne!(text, GOLDEN, "the fixture must carry the schema line");
        let error = SceneFile::parse(&text).expect_err("another schema is refused");
        assert!(error.to_string().contains("unsupported schema version"), "error was {error}");
        assert!(error.to_string().contains(&other.to_string()), "the error names the number found");
    }
}

#[test]
fn a_missing_or_foreign_format_is_a_readable_error() {
    let missing = GOLDEN.replacen("\"format\": \"furnicraft.scene\",", "", 1);
    assert_ne!(missing, GOLDEN);
    assert!(matches!(SceneFile::parse(&missing), Err(SceneFileError::MissingFormat)));

    let foreign = GOLDEN.replacen("\"format\": \"furnicraft.scene\"", "\"format\": \"another.app\"", 1);
    assert_ne!(foreign, GOLDEN);
    assert!(matches!(
        SceneFile::parse(&foreign),
        Err(SceneFileError::WrongFormat { found, expected: "furnicraft.scene" }) if found == "another.app"
    ));
}

#[test]
fn the_old_pivot_field_and_the_old_container_tags_are_unknown() {
    let pivot = GOLDEN.replacen("\"pivot_local\"", "\"pivot\"", 1);
    assert_ne!(pivot, GOLDEN, "the fixture must carry a pivot");
    assert!(SceneFile::parse(&pivot).is_err(), "the old field name is not accepted");

    let assembly = GOLDEN.replacen("\"container\": {", "\"assembly\": {", 1);
    assert_ne!(assembly, GOLDEN, "the fixture must carry a container");
    assert!(SceneFile::parse(&assembly).is_err(), "the old container tag is not accepted");
}

#[test]
fn saving_and_reading_a_file_round_trips_the_document() {
    let dir = TempDir::new("round-trip");
    let target = dir.path().join("scene.fcs");
    let scene = SceneFile::parse(GOLDEN).expect("the golden loads");

    scene.write(&target).expect("write");
    let loaded = SceneFile::read(&target).expect("read");

    assert_eq!(loaded.document(), scene.document());
    assert_eq!(loaded.extra(), scene.extra());
    assert!(!dir.path().join("scene.fcs.tmp").exists(), "the temporary file is renamed away");
}

#[test]
fn a_failed_write_leaves_the_previous_file_intact() {
    let dir = TempDir::new("atomic");
    let target = dir.path().join("scene.fcs");
    let original = SceneFile::parse(GOLDEN).expect("the golden loads");
    original.write(&target).expect("first write");
    let before = fs::read_to_string(&target).expect("read the saved file");

    // A directory on the temporary path makes the write fail after the target already exists.
    fs::create_dir(dir.path().join("scene.fcs.tmp")).expect("create the blocking directory");
    let mut replacement = original.document().clone();
    replacement.set_active_camera(None).expect("clear the camera");
    let error = SceneFile::new(replacement).write(&target).expect_err("the write must fail");

    assert!(matches!(error, SceneFileError::Write { .. }), "error was {error}");
    assert_eq!(fs::read_to_string(&target).expect("read the old file"), before);
}

#[test]
fn a_saved_file_holds_only_the_envelope_and_the_document() {
    let scene = SceneFile::parse(GOLDEN).expect("the golden loads");
    let text = scene.to_json().expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let object = value.as_object().expect("the envelope is an object");
    let document =
        object.get("document").and_then(serde_json::Value::as_object).expect("the envelope carries the document");

    for forbidden in ["order", "orders", "catalogue", "undo", "redo", "selection", "viewport", "diagnostics"] {
        assert!(!document.contains_key(forbidden), "a scene file must not carry a `{forbidden}` section");
    }
    for key in object.keys() {
        assert!(
            matches!(key.as_str(), "format" | "format_version" | "document" | "generator"),
            "unexpected top-level section `{key}`"
        );
    }
}

#[test]
fn a_file_above_the_size_limit_is_rejected_before_it_is_read() {
    let dir = TempDir::new("too-large");
    let target = dir.path().join("scene.fcs");
    let file = fs::File::create(&target).expect("create");
    file.set_len(MAX_SCENE_BYTES + 1).expect("extend sparsely");

    let error = SceneFile::read(&target).expect_err("an oversized file must be rejected");
    assert!(matches!(error, SceneFileError::TooLarge { .. }), "error was {error}");
}

#[test]
fn an_empty_scene_has_no_camera_or_light() {
    let mut document =
        Project::new(Container::new(id(1), "Root", Placement::IDENTITY, Vec::new())).expect("an empty scene");
    document.set_active_camera(None).expect("no camera");
    let scene = SceneFile::new(document);
    let text = scene.to_json().expect("serialize");
    let again = SceneFile::parse(&text).expect("reload");

    assert_eq!(again.document().active_camera(), None);
    assert!(again.document().query(&NodeQuery::any().with_kind(StructuralKind::Camera)).is_empty());
    assert!(again.document().query(&NodeQuery::any().with_kind(StructuralKind::Light)).is_empty());
    assert!(again.document().query(&NodeQuery::any().with_kind(StructuralKind::Object)).is_empty());
    assert!(again.document().root().children().is_empty(), "the empty scene holds no nodes");
}
