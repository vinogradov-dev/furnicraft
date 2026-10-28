//! Acceptance tests for the scene-file envelope, its golden files and atomic saving.
//!
//! The checked-in goldens are the compatibility contract: a version 7 file has no objects, a
//! version 8 file has one, and both must keep loading as the schema moves on.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use furnicraft_model::{Geometry, Id, NodeQuery, Project, StructuralKind};
use furnicraft_project::{MAX_SCENE_BYTES, SceneFile, SceneFileError};

const V7: &str = include_str!("golden/scene-v7.fcs");
const V8: &str = include_str!("golden/scene-v8.fcs");
const V9: &str = include_str!("golden/scene-v9.fcs");
const V10: &str = include_str!("golden/scene-v10.fcs");

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

#[test]
fn every_golden_file_loads_and_round_trips() {
    for (name, text) in [("version 7", V7), ("version 8", V8), ("version 9", V9), ("version 10", V10)] {
        let scene = SceneFile::parse(text).expect(name);
        assert_eq!(
            scene.document().schema_version(),
            Project::CURRENT_SCHEMA_VERSION,
            "{name} must migrate to the current schema"
        );
        let rewritten = scene.to_json().expect(name);
        let again = SceneFile::parse(&rewritten).expect(name);
        assert_eq!(again.document(), scene.document(), "{name} document must survive a round trip");
        assert_eq!(again.extra(), scene.extra(), "{name} extra sections must survive a round trip");
    }
}

#[test]
fn a_version_seven_file_loads_with_a_panel_and_no_objects() {
    let scene = SceneFile::parse(V7).expect("version 7");

    let panel = Id::new(3).expect("id");
    assert!(scene.document().panel(panel).is_some(), "the panel must load");
    let objects = scene.document().query(&NodeQuery::any().with_kind(StructuralKind::Object));
    assert!(objects.is_empty(), "a version 7 file has no geometry objects");
}

#[test]
fn a_version_eight_file_loads_the_object_and_preserves_unknown_sections() {
    let scene = SceneFile::parse(V8).expect("version 8");

    let object = scene.document().object(Id::new(4).expect("id")).expect("the object must load");
    assert_eq!(object.name(), "Bracket");
    assert!(matches!(object.geometry(), Geometry::Mesh(_)));
    assert!(scene.extra().contains_key("generator"), "an unknown top-level section is preserved");

    let rewritten = scene.to_json().expect("serialize");
    assert!(rewritten.contains("\"generator\""), "the unknown section must be written back");
}

#[test]
fn a_version_nine_file_loads_cameras_lights_and_the_active_camera() {
    let scene = SceneFile::parse(V9).expect("version 9");
    let document = scene.document();

    let camera = document.camera(Id::new(2).expect("id")).expect("the camera must load");
    assert_eq!(camera.name(), "Presentation");
    assert_eq!(document.active_camera(), Some(camera.id()));
    let light = document.light(Id::new(3).expect("id")).expect("the light must load");
    assert_eq!(light.name(), "Key");
    assert_eq!(light.kind(), furnicraft_model::LightKind::Sun);
    assert_eq!(light.intensity(), 2.0);
}

#[test]
fn a_version_ten_file_loads_a_constructive_object() {
    let scene = SceneFile::parse(V10).expect("version 10");
    let object = scene.document().object(Id::new(2).expect("id")).expect("the object must load");
    assert_eq!(object.name(), "Countertop");
    let constructive = object.geometry().as_constructive().expect("a constructive payload");
    let contour = constructive.contour().expect("the contour");
    assert_eq!(contour.points().len(), 4);
    assert_eq!(constructive.height(), Some(furnicraft_units::Length::from_millimetres(18).expect("height")));
    assert!(object.geometry().as_mesh().is_none(), "the mesh of a constructive object is derived, not stored");

    let rewritten = scene.to_json().expect("serialize");
    let again = SceneFile::parse(&rewritten).expect("the rewritten version 10 file loads");
    assert_eq!(again.document(), scene.document());
}

#[test]
fn a_version_eight_file_has_no_active_camera() {
    let scene = SceneFile::parse(V8).expect("version 8");
    assert_eq!(scene.document().active_camera(), None);
    assert!(scene.document().query(&NodeQuery::any().with_kind(StructuralKind::Camera)).is_empty());
    assert!(scene.document().query(&NodeQuery::any().with_kind(StructuralKind::Light)).is_empty());
}

#[test]
fn a_scene_saved_from_a_document_keeps_the_sections_it_was_loaded_with() {
    let loaded = SceneFile::parse(V8).expect("version 8");
    let saved = SceneFile::new(loaded.document().clone()).with_extra(loaded.extra().clone());

    assert_eq!(saved.extra(), loaded.extra());
    assert!(saved.to_json().expect("serialize").contains("\"generator\""));
}

#[test]
fn an_unknown_field_inside_the_document_is_a_readable_error() {
    let text = V8.replacen("\"schema_version\": 8,", "\"schema_version\": 8,\n    \"future_field\": 1,", 1);
    assert_ne!(text, V8, "the fixture must contain the schema line");
    let error = SceneFile::parse(&text).expect_err("an unknown document field must be rejected");
    assert!(error.to_string().contains("unknown field"), "error was {error}");
}

#[test]
fn a_newer_format_version_is_a_readable_error() {
    let text = V8.replacen("\"format_version\": 1", "\"format_version\": 2", 1);
    assert_ne!(text, V8);
    let error = SceneFile::parse(&text).expect_err("a newer envelope must be rejected");
    assert!(matches!(error, SceneFileError::UnsupportedFormatVersion { found: 2, .. }), "error was {error}");
}

#[test]
fn a_newer_document_schema_is_a_readable_error() {
    let newer = Project::CURRENT_SCHEMA_VERSION + 1;
    let text = V8.replacen("\"schema_version\": 8", &format!("\"schema_version\": {newer}"), 1);
    assert_ne!(text, V8);
    let error = SceneFile::parse(&text).expect_err("a newer document must be rejected");
    assert!(error.to_string().contains("unsupported schema version"), "error was {error}");
}

#[test]
fn a_missing_or_foreign_format_is_a_readable_error() {
    let missing = V8.replacen("\"format\": \"furnicraft.scene\",", "", 1);
    assert_ne!(missing, V8);
    assert!(matches!(SceneFile::parse(&missing), Err(SceneFileError::MissingFormat)));

    let foreign = V8.replacen("\"format\": \"furnicraft.scene\"", "\"format\": \"another.app\"", 1);
    assert_ne!(foreign, V8);
    assert!(matches!(
        SceneFile::parse(&foreign),
        Err(SceneFileError::WrongFormat { found, expected: "furnicraft.scene" }) if found == "another.app"
    ));
}

#[test]
fn saving_and_reading_a_file_round_trips_the_document() {
    let dir = TempDir::new("round-trip");
    let target = dir.path().join("scene.fcs");
    let scene = SceneFile::parse(V8).expect("version 8");

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
    let original = SceneFile::parse(V8).expect("version 8");
    original.write(&target).expect("first write");
    let before = fs::read_to_string(&target).expect("read the saved file");

    // A directory on the temporary path makes the write fail after the target already exists.
    fs::create_dir(dir.path().join("scene.fcs.tmp")).expect("create the blocking directory");
    let replacement = SceneFile::parse(V7).expect("version 7");
    let error = replacement.write(&target).expect_err("the write must fail");

    assert!(matches!(error, SceneFileError::Write { .. }), "error was {error}");
    assert_eq!(fs::read_to_string(&target).expect("read the old file"), before);
}

#[test]
fn a_saved_file_holds_only_the_envelope_and_the_document() {
    let scene = SceneFile::parse(V8).expect("version 8");
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
