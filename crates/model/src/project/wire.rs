//! The serialized envelope of a project and the migrations older schemas need.
//!
//! Loading goes through a wire struct, validates the version and the tree, and only then yields a
//! [`Project`]. Unknown top-level fields are rejected; the reserved property keys are validated on
//! the same pass. A version-2 file stores cutouts in core coordinates and a version-6 file stores
//! assemblies and groups as one container kind; both migrate here, never in the document.

use serde::Deserialize;

use crate::project::Project;
use crate::tree::{default_container_pivot, find_node};
use crate::*;

use super::{next_id_after, scan_tree};

/// Serialized form of [`Project`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectWire {
    schema_version: u32,
    revision: Revision,
    #[serde(default)]
    next_id: Option<Id>,
    root: Container,
    #[serde(default)]
    active_camera: Option<Id>,
}

impl TryFrom<ProjectWire> for Project {
    type Error = ProjectError;

    fn try_from(wire: ProjectWire) -> Result<Self, Self::Error> {
        // Version 1 stored a `size` on every panel; `PanelWire` migrates it to nominal size plus a
        // fallback. Version 2 stored machining in core-local coordinates; the migration below
        // translates it to the nominal panel frame. Version 7 unified the two container kinds;
        // `Node` reads a version-6 assembly as a component container. All versions are normalized
        // to the current one on load.
        if !(1..=Project::CURRENT_SCHEMA_VERSION).contains(&wire.schema_version) {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: wire.schema_version,
                supported: Project::CURRENT_SCHEMA_VERSION,
            });
        }
        let mut root = wire.root;
        if wire.schema_version <= 2 {
            migrate_machining_to_nominal(&mut root, 1)?;
        }
        if wire.schema_version <= 5 {
            // Before schema 6 the pivot was derived from the current tree. Materialize the old
            // default once so it stays stable: panels keep the local origin, non-empty containers
            // start at the centre of their local panel bounds. This is the only writer of such a
            // centre; a container created today is born at zero and no pass re-normalizes it.
            migrate_container_pivots(&mut root, 1)?;
        }
        root.set_pivot(PivotLocal::ZERO);
        let max_id = scan_tree(&root)?;
        let next_id = match wire.next_id {
            Some(next_id) if next_id.get() <= max_id => {
                return Err(ProjectError::NextIdNotAfterMax { next: next_id, max: max_id });
            }
            Some(next_id) => next_id,
            None => next_id_after(max_id)?,
        };
        let active_camera = match wire.active_camera {
            Some(id) if find_node(&root, id).is_some_and(|node| node.as_camera().is_some()) => Some(id),
            Some(id) => return Err(ProjectError::CameraNotFound { id }),
            None => None,
        };
        Ok(Self {
            schema_version: Project::CURRENT_SCHEMA_VERSION,
            revision: wire.revision,
            next_id,
            root,
            active_camera,
        })
    }
}

/// Translates version-2 core-local machining into the nominal panel frame.
///
/// The conversion adds the stored fallback core origin to every cutout, so a loaded file keeps the
/// hole at the same physical position. The migration is bounded by the same depth limit as the
/// document tree.
fn migrate_machining_to_nominal(root: &mut Container, depth: u32) -> Result<(), ProjectError> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
    }
    fn walk(children: &mut [Node], depth: u32) -> Result<(), ProjectError> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        for child in children {
            match child {
                Node::Container(container) => walk(container.children_mut(), depth.saturating_add(1))?,
                Node::Panel(panel) => panel.migrate_machining_from_core()?,
                Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
            }
        }
        Ok(())
    }
    walk(root.children_mut(), depth.saturating_add(1))
}

/// Materializes the legacy default pivot offset on containers saved before schema 6.
///
/// Panels keep their zero default from [`PanelWire`]. The walk is bounded by the same depth limit
/// as the document tree.
fn migrate_container_pivots(root: &mut Container, depth: u32) -> Result<(), ProjectError> {
    fn walk(children: &mut [Node], depth: u32) -> Result<(), ProjectError> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
        }
        for child in children {
            match child {
                Node::Container(container) => {
                    container.set_pivot(default_container_pivot(container.children()));
                    walk(container.children_mut(), depth.saturating_add(1))?;
                }
                Node::Panel(_) | Node::Object(_) | Node::Camera(_) | Node::Light(_) => {}
            }
        }
        Ok(())
    }
    walk(root.children_mut(), depth.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn reserved_property_keys_are_validated_on_load() {
        let invalid = r#"{
            "schema_version": 7,
            "revision": 0,
            "next_id": 2,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [],
                "properties": {"component": {"kind": "text", "value": "yes"}}
            }
        }"#;
        let error = serde_json::from_str::<Project>(invalid).expect_err("invalid reserved property");
        assert!(error.to_string().contains("must be a boolean"), "error was {error}");
    }

    #[test]
    fn projects_reject_unsupported_schema_and_unknown_fields() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");

        let newer = text.replacen(
            &format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION),
            &format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION + 1),
            1,
        );
        let error = serde_json::from_str::<Project>(&newer).expect_err("unsupported schema");
        assert!(error.to_string().contains("unsupported schema version"), "error was {error}");

        let prefix = text.strip_suffix('}').expect("project JSON is an object");
        let extra = format!("{prefix},\"extra\":1}}");
        assert!(serde_json::from_str::<Project>(&extra).is_err());
    }

    #[test]
    fn version_one_panels_migrate_to_nominal_geometry() {
        let legacy = r#"{
            "schema_version": 1,
            "revision": 0,
            "next_id": 3,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Shelf",
                    "size": {"length": 700000, "width": 500000, "thickness": 18000},
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version one");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = project.panel(Id::new(2).expect("id")).expect("migrated panel");
        assert_eq!(panel.nominal_size(), sample_nominal());
        assert_eq!(panel.material(), None);
        assert_eq!(panel.last_known_thickness(), millimetres(18));
        assert!(panel.edge_banding().is_empty());
        assert!(project.validate().is_ok());
    }

    #[test]
    fn version_four_root_assembly_loads_as_a_group() {
        let legacy = r#"{
            "schema_version": 4,
            "revision": 2,
            "next_id": 4,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"assembly": {
                    "id": 2,
                    "name": "Drawer",
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "children": [{"panel": {
                        "id": 3,
                        "name": "Front",
                        "nominal_size": {"length": 500000, "width": 200000},
                        "last_known_thickness": 18000,
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
                    }}]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version four");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let root = Id::new(1).expect("root id");
        let drawer = Id::new(2).expect("drawer id");
        assert!(project.container(root).is_some());
        assert!(!project.is_component(root));
        assert!(project.is_component(drawer));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.pivot_local(root), Some(PivotLocal::ZERO));
        assert_eq!(
            project.pivot_local(drawer),
            Some(PivotLocal::new(
                Length::from_micrometres(250_000),
                Length::from_micrometres(100_000),
                Length::from_micrometres(9_000),
            )),
            "an older file materializes the centred container default"
        );
        assert_eq!(project.pivot_local(Id::new(3).expect("panel id")), Some(PivotLocal::ZERO));
        assert!(project.validate().is_ok());
    }

    #[test]
    fn version_two_machining_migrates_from_core_to_nominal_coordinates() {
        let legacy = r#"{
            "schema_version": 2,
            "revision": 4,
            "next_id": 6,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Panel",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "last_known_thickness": 18000,
                    "machining": [{"cutout": {
                        "id": 5,
                        "shape": {"rectangle": {"x": 1000, "y": 1000, "width": 10000, "height": 10000}},
                        "depth": "through",
                        "entry": "A"
                    }}],
                    "edge_banding": [
                        {
                            "edge": 1,
                            "material": 1,
                            "included_in_size": true,
                            "last_known_thickness": 2000,
                            "last_known_width": 22000
                        },
                        {
                            "edge": 4,
                            "material": 1,
                            "included_in_size": true,
                            "last_known_thickness": 2000,
                            "last_known_width": 22000
                        }
                    ]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version two");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = project.panel(Id::new(2).expect("id")).expect("migrated panel");
        let machining = &panel.machining()[0];
        assert_eq!(machining.id().get(), 5);
        assert_eq!(machining.anchor().as_face(), Some(FaceSide::A));
        let cutout = machining.cutout().expect("a legacy record is a cutout");
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));

        let text = serde_json::to_string(&project).expect("serialize migrated project");
        let decoded: Project = serde_json::from_str(&text).expect("reload migrated project");
        let panel = decoded.panel(Id::new(2).expect("id")).expect("reloaded panel");
        let cutout = panel.machining()[0].cutout().expect("a migrated record is a cutout");
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));
    }

    #[test]
    fn version_fifteen_cutout_records_load_as_face_anchored_machining() {
        let legacy = r#"{
            "schema_version": 15,
            "revision": 2,
            "next_id": 6,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Shelf",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "last_known_thickness": 18000,
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "machining": [{"cutout": {
                        "id": 5,
                        "shape": {"rectangle": {"x": 10000, "y": 20000, "width": 100000, "height": 50000}},
                        "depth": {"blind": {"depth": 8000}},
                        "entry": "B"
                    }}]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("load a schema 15 file");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = project.panel(Id::new(2).expect("id")).expect("panel");
        let machining = &panel.machining()[0];
        assert_eq!(machining.id().get(), 5);
        assert_eq!(machining.anchor().as_face(), Some(FaceSide::B));
        let cutout = machining.cutout().expect("a legacy record is a cutout");
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(10, 20, 100, 50)));
        assert_eq!(cutout.depth(), CutoutDepth::Blind { depth: millimetres(8) });

        let text = serde_json::to_string(&project).expect("write the current form");
        assert!(text.contains("\"anchor\""), "the record writes its anchor: {text}");
        assert!(text.contains("\"params\""), "the record writes tagged parameters: {text}");
        assert!(!text.contains("\"cutout\":{\"id\""), "the legacy form is not written back: {text}");
        let again: Project = serde_json::from_str(&text).expect("reload the current form");
        assert_eq!(again, project);
    }

    #[test]
    fn duplicate_machining_ids_are_rejected_on_load() {
        let project = sample_project();
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        let panel = &mut wire["root"]["children"][0]["panel"];
        panel["machining"] = serde_json::json!([
            {
                "id": 2,
                "anchor": {"kind": "face", "side": "A"},
                "params": {
                    "kind": "cutout",
                    "shape": {"rectangle": {"x": 10_000, "y": 10_000, "width": 100_000, "height": 50_000}},
                    "depth": "through"
                }
            }
        ]);
        let error = serde_json::from_value::<Project>(wire).expect_err("a machining id repeats the panel id");
        assert!(error.to_string().contains("duplicate"), "error was {error}");
    }

    #[test]
    fn an_older_schema_without_a_contour_cutout_still_loads() {
        let project = sample_project();
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        wire["schema_version"] = serde_json::json!(13);
        let decoded: Project = serde_json::from_value(wire).expect("load a schema 13 file");
        assert_eq!(decoded.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        assert_eq!(decoded, project);
    }

    #[test]
    fn version_sixteen_files_without_a_path_still_load() {
        let mut project = sample_project();
        let root = project.root().id();
        let constructive =
            Constructive::extrude(rectangle_contour(100, 50).into(), millimetres(18)).expect("constructive object");
        project
            .add_object(root, "Countertop", Placement::IDENTITY, Geometry::Constructive(constructive))
            .expect("a version 16 file has constructive objects");

        let mut wire = serde_json::to_value(&project).expect("serialize project");
        wire["schema_version"] = serde_json::json!(16);
        let decoded: Project = serde_json::from_value(wire).expect("load a schema 16 file");
        assert_eq!(decoded.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        assert_eq!(decoded, project, "a file without a path loads unchanged");
        assert!(
            !serde_json::to_value(&decoded).expect("serialize").to_string().contains("\"kind\":\"path\""),
            "the migrated file writes no path payload"
        );
    }

    #[test]
    fn version_fourteen_panel_features_move_into_the_outline() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let corner =
            CornerFeature::new(RectCorner::BottomLeft.id(), CornerFeatureKind::Radius { radius: millimetres(8) })
                .expect("radius");
        project.set_corner_feature(panel_id, corner).expect("set corner feature");
        project
            .set_edge_bow(panel_id, EdgeBow::new(RectEdge::Bottom.id(), millimetres(5)).expect("bow"))
            .expect("set bow");

        let mut wire = serde_json::to_value(&project).expect("serialize project");
        wire["schema_version"] = serde_json::json!(14);
        let panel = &mut wire["root"]["children"][0]["panel"];
        let outline = panel["outline"].as_object_mut().expect("outline is an object");
        let corner_features = outline.remove("corner_features").expect("embedded corner features");
        let edge_bows = outline.remove("edge_bows").expect("embedded bows");
        panel["corner_features"] = corner_features;
        panel["edge_bows"] = edge_bows;

        let decoded: Project = serde_json::from_value(wire).expect("load a schema 14 file");
        assert_eq!(decoded.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let panel = decoded.panel(panel_id).expect("panel");
        assert_eq!(panel.corner_features(), &[corner]);
        assert_eq!(panel.edge_bows().len(), 1);

        let text = serde_json::to_string(&decoded).expect("serialize migrated project");
        assert!(text.contains("\"corner_features\":[{\"corner\":1"), "the feature is written inside the outline");
        let again: Project = serde_json::from_str(&text).expect("reload migrated project");
        assert_eq!(again, decoded);
    }

    #[test]
    fn version_six_containers_migrate_to_one_container_kind() {
        let legacy = r#"{
            "schema_version": 6,
            "revision": 3,
            "next_id": 4,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [
                    {"assembly": {
                        "id": 2,
                        "name": "Drawer",
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                        "children": []
                    }},
                    {"group": {
                        "id": 3,
                        "name": "Parts",
                        "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                        "children": []
                    }}
                ]
            }
        }"#;
        let project: Project = serde_json::from_str(legacy).expect("migrate version six");
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        let drawer = Id::new(2).expect("id");
        let parts = Id::new(3).expect("id");
        assert!(project.is_component(drawer));
        assert!(!project.is_component(parts));
        assert_eq!(project.container(drawer).expect("drawer").name(), "Drawer");
        assert_eq!(project.container(parts).expect("parts").name(), "Parts");

        let text = serde_json::to_string(&project).expect("serialize");
        assert!(text.contains("\"container\""));
        assert!(!text.contains("\"assembly\""));
        assert!(!text.contains("\"group\":"));
        let decoded: Project = serde_json::from_str(&text).expect("round trip");
        assert_eq!(decoded, project);
    }
}
