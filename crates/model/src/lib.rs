//! The FurniCraft document: the container tree, panels, machining, banding, materials and the scene
//! objects the user authors.
//!
//! Pure serializable data plus its invariants. No geometry kernel, no GPU, no UI, no file I/O. The
//! crate root is a facade: each module owns one family of values or rules and its public items are
//! re-exported here, so callers keep writing `furnicraft_model::Type`.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::HashSet;

use furnicraft_units::Length;

mod camera;
mod catalog;
mod container;
mod contour;
mod contour_edit;
#[cfg(test)]
mod fixtures;
mod furntype;
mod ids;
mod light;
mod mesh;
mod node;
mod object;
mod panel;
mod panel_features;
mod placement;
mod project;
mod properties;
mod query;
mod tree;

pub use camera::{CameraError, CameraNode, CameraProjection};
pub use catalog::{Catalog, CatalogError, EdgeBandingMaterial, MaterialId, PanelMaterial};
pub use container::Container;
pub use contour::{
    Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, MAX_CONTOUR_COORDINATE,
    MAX_CONTOUR_SEGMENTS,
};
pub use contour_edit::ContourEditError;
pub use furntype::{FURNTYPE_MAX_CODE, FurnType, FurnTypeError};
pub use ids::{ContourSegmentId, EdgeId, Id, Revision, VertexId};
pub use light::{LightError, LightKind, LightNode};
pub use mesh::{MAX_FACE_VERTICES, MAX_MESH_FACES, MAX_MESH_VERTICES, Mesh, MeshError};
pub use node::Node;
pub use object::{Constructive, ConstructiveError, Extrude, Feature, Geometry, MAX_OBJECT_FEATURES, Object};
pub use panel::{
    BandingSpec, ContourPath, CoreViolation, CornerBanding, Cutout, CutoutDepth, CutoutShape, EdgeBanding, EntryFace,
    FaceSide, Machining, Panel, PanelContourError, PanelDimensionError, PanelError, PanelGeometry, PanelGeometryError,
    PanelNominalSize, PanelOutline, PanelRect, PanelRectError, RectCorner, RectEdge, RingBanding, SegmentSource,
};
pub use panel_features::{CornerFeature, CornerFeatureKind, EdgeBow};
pub use placement::{EulerAngles, Pivot, Placement, PlacementError};
pub use project::{Project, ProjectError};
pub use properties::{
    Properties, PropertyChange, PropertyError, PropertyValue, SYSTEM_COMPONENT, SYSTEM_FURNTYPE, SYSTEM_PROTOTYPE_ID,
    SYSTEM_PROTOTYPE_PARAMETERS, SYSTEM_PROTOTYPE_VERSION,
};
pub use query::{NodeQuery, PropertyPredicate, StructuralKind};

#[cfg(test)]
mod tests {
    use super::*;

    use furnicraft_geom::{DQuat, DVec3};

    use crate::fixtures::*;

    #[test]
    fn clearing_an_unbanded_edge_is_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(20) })
                    .expect("radius"),
            )
            .expect("corner feature");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), sample_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("apply band");
        assert_eq!(
            project.clear_edge_banding(panel_id, RectEdge::Top.id()),
            Err(ProjectError::EdgeBandingNotFound { panel: panel_id, edge: RectEdge::Top.id() })
        );
        assert_eq!(project.panel(panel_id).expect("panel").edge_banding().len(), 1);
    }

    #[test]
    fn projects_start_at_the_current_schema_and_revision() {
        let project = sample_project();
        assert_eq!(project.schema_version(), Project::CURRENT_SCHEMA_VERSION);
        assert_eq!(project.revision(), Revision::ZERO);
        assert_eq!(project.root().id(), Id::new(1).expect("id"));
        assert_eq!(project.next_id().get(), 5);
        assert_eq!(project.root().children().len(), 2);
    }

    #[test]
    fn projects_add_panels_and_assemblies_with_fresh_ids() {
        let mut project = sample_project();
        let root_id = project.root().id();

        let panel_id = project
            .add_panel(root_id, "Back", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("add panel");
        assert_eq!(panel_id.get(), 5);
        assert_eq!(project.revision().get(), 1);
        assert_eq!(project.root().children().len(), 3);

        let assembly_id = project.add_component(root_id, "Carcass", Placement::IDENTITY).expect("add assembly");
        assert_eq!(assembly_id.get(), 6);
        assert_eq!(project.revision().get(), 2);
        assert_eq!(project.container(assembly_id).expect("new component").name(), "Carcass");
        assert!(project.is_component(assembly_id));
        assert!(project.validate().is_ok());
    }

    #[test]
    fn project_resizes_panels_and_reports_missing_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(4).expect("id");
        let nominal = PanelNominalSize::new(millimetres(900), millimetres(600)).expect("valid size");

        let revision = project.set_panel_nominal_size(panel_id, nominal).expect("resize panel");
        assert_eq!(revision.get(), 1);
        assert_eq!(project.revision(), revision);
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), nominal);

        let missing = Id::new(999).expect("id");
        assert_eq!(project.set_panel_nominal_size(missing, nominal), Err(ProjectError::PanelNotFound { id: missing }));
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn project_adds_cutouts_and_allocates_ids() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));

        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        assert_eq!(cutout_id.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        let panel = project.panel(panel_id).expect("panel exists");
        assert_eq!(panel.machining().len(), 1);
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);
        assert_eq!(cutout.entry(), FaceSide::A);
        assert!(project.validate().is_ok());

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn cutouts_reject_invalid_depth_bounds_and_overlap_without_side_effects() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: Length::ZERO }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid { panel: panel_id, depth: Length::ZERO, thickness: millimetres(18) })
        );
        assert_eq!(
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Blind { depth: millimetres(18) }, FaceSide::A),
            Err(ProjectError::CutoutDepthInvalid {
                panel: panel_id,
                depth: millimetres(18),
                thickness: millimetres(18)
            })
        );
        let outside = CutoutShape::Rectangle(sample_rect(750, 10, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, outside, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let first = project
            .add_cutout(panel_id, shape, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::B)
            .expect("first cutout");
        assert_eq!(first.get(), 5);
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);

        let overlapping = CutoutShape::Rectangle(sample_rect(50, 20, 100, 50));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: first })
        );
        assert_eq!(project.next_id().get(), 6);
        assert_eq!(project.revision().get(), 1);
        assert_eq!(project.panel(panel_id).expect("panel exists").machining().len(), 1);
    }

    #[test]
    fn overlapping_through_cutouts_and_boundary_notches_are_accepted() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let first = CutoutShape::Rectangle(sample_rect(100, 100, 100, 50));
        let second = CutoutShape::Rectangle(sample_rect(150, 120, 100, 50));
        // The 700 mm core ends at x = 700, so this cutout opens a notch in the right edge.
        let notch = CutoutShape::Rectangle(sample_rect(650, 10, 100, 50));

        project.add_cutout(panel_id, first, CutoutDepth::Through, FaceSide::A).expect("first through cutout");
        project.add_cutout(panel_id, second, CutoutDepth::Through, FaceSide::A).expect("overlapping through cutout");
        project.add_cutout(panel_id, notch, CutoutDepth::Through, FaceSide::A).expect("boundary notch");

        let panel = project.panel(panel_id).expect("panel");
        let geometry = panel.geometry_with_fallbacks().expect("geometry");
        assert_eq!(panel.core_violation(geometry), None);
        assert_eq!(panel.machining().len(), 3);
    }

    #[test]
    fn cutouts_that_split_or_empty_the_core_are_rejected() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let strip = CutoutShape::Rectangle(sample_rect(0, 200, 700, 50));
        assert_eq!(
            project.add_cutout(panel_id, strip, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutSplitsCore { panel: panel_id })
        );

        let whole = CutoutShape::Rectangle(sample_rect(0, 0, 700, 500));
        assert_eq!(
            project.add_cutout(panel_id, whole, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutRemovesCore { panel: panel_id })
        );
        assert!(project.panel(panel_id).expect("panel").machining().is_empty());
    }

    #[test]
    fn panel_resize_validates_existing_cutouts() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(600, 10, 50, 50));
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");

        let smaller = PanelNominalSize::new(millimetres(500), millimetres(500)).expect("valid size");
        assert_eq!(
            project.validate_panel_resize(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(
            project.set_panel_nominal_size(panel_id, smaller),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
        assert_eq!(project.panel(panel_id).expect("panel exists").nominal_size(), sample_nominal());
        assert_eq!(project.revision().get(), 1);

        let blind_rect = CutoutShape::Rectangle(sample_rect(10, 10, 50, 50));
        let other_panel = Id::new(4).expect("id");
        project
            .add_cutout(other_panel, blind_rect, CutoutDepth::Blind { depth: millimetres(8) }, FaceSide::A)
            .expect("blind cutout");
        let material = MaterialId::new(1).expect("material id");
        let revision = project.set_panel_material(other_panel, material, millimetres(5)).expect("material change");
        assert_eq!(revision.get(), 3);
        let panel = project.panel(other_panel).expect("panel exists");
        assert_eq!(panel.material(), Some(material));
        assert_eq!(panel.last_known_thickness(), millimetres(5));
        assert_eq!(panel.geometry(millimetres(5)).expect("resolved geometry").thickness(), millimetres(5));
    }

    #[test]
    fn panels_load_without_machining() {
        let panel = sample_panel(Id::new(2).expect("id"), "Shelf");
        let mut value = serde_json::to_value(&panel).expect("serialize panel");
        value.as_object_mut().expect("panel is an object").remove("machining");
        let decoded: Panel = serde_json::from_value(value).expect("older panel without machining");
        assert_eq!(decoded, panel);
        assert!(decoded.machining().is_empty());
    }

    #[test]
    fn panel_rects_reject_non_positive_dimensions() {
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, Length::ZERO, millimetres(10)),
            Err(PanelRectError::NonPositive { field: "width", value: Length::ZERO })
        );
        assert_eq!(
            PanelRect::new(Length::ZERO, Length::ZERO, millimetres(10), Length::ZERO),
            Err(PanelRectError::NonPositive { field: "height", value: Length::ZERO })
        );
        assert!(serde_json::from_str::<PanelRect>(r#"{"x":0,"y":0,"width":0,"height":10000}"#).is_err());
    }

    #[test]
    fn cutout_ids_participate_in_next_id_validation() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 100, 50));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let text = serde_json::to_string(&project).expect("serialize project");
        let invalid = text.replacen("\"next_id\":6", &format!("\"next_id\":{}", cutout_id.get()), 1);
        let error = serde_json::from_str::<Project>(&invalid).expect_err("next id must exceed cutout id");
        assert!(error.to_string().contains("must be greater"), "error was {error}");
    }

    #[test]
    fn failed_mutations_do_not_consume_ids_or_bump_revisions() {
        let mut project = sample_project();
        let next_id = project.next_id();
        let revision = project.revision();
        let missing = Id::new(999).expect("id");

        assert_eq!(
            project.add_panel(missing, "Orphan", sample_nominal(), None, sample_thickness(), Placement::IDENTITY),
            Err(ProjectError::ContainerNotFound { id: missing })
        );
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn projects_reject_assemblies_below_the_depth_limit() {
        let mut nested = Vec::new();
        let mut deepest = None;
        for index in 0..(Project::MAX_ASSEMBLY_DEPTH - 1) {
            let id = Id::new(u64::from(index) + 2).expect("id");
            if deepest.is_none() {
                deepest = Some(id);
            }
            let container = Container::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![container.into()];
        }
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        let deepest = deepest.expect("at least one nested container");
        let mut project = Project::new(root).expect("tree at the depth limit");
        assert_eq!(
            project.add_component(deepest, "Too deep", Placement::IDENTITY),
            Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH })
        );
        assert_eq!(project.revision(), Revision::ZERO);
    }

    #[test]
    fn projects_bump_revisions_explicitly() {
        let mut project = sample_project();
        assert_eq!(project.bump_revision().expect("bump revision").get(), 1);
        assert_eq!(project.revision().get(), 1);
    }

    #[test]
    fn project_restore_keeps_revisions_and_ids_monotonic() {
        let original = sample_project();
        let mut project = sample_project();
        let created = project
            .add_panel(project.root().id(), "Back", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("add panel");
        let after = project.clone();

        let revision = project.restore_from(&original).expect("restore original");
        assert_eq!(revision.get(), 2);
        assert_eq!(project.root().children().len(), 2);
        assert!(project.next_id().get() > created.get());
        assert!(project.same_content(&original));
        assert!(!project.same_content(&after));

        let revision = project.restore_from(&after).expect("restore edited");
        assert_eq!(revision.get(), 3);
        assert_eq!(project.root().children().len(), 3);
        assert_eq!(project.next_id(), after.next_id());
    }

    #[test]
    fn projects_round_trip_through_json() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
    }

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
    fn projects_load_without_next_id() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let mut value: serde_json::Value = serde_json::from_str(&text).expect("parse project");
        value.as_object_mut().expect("project is an object").remove("next_id");
        let decoded: Project = serde_json::from_value(value).expect("older v1 project");
        assert_eq!(decoded, project);
    }

    #[test]
    fn projects_reject_next_id_at_or_below_the_largest_id() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let invalid = text.replacen("\"next_id\":5", "\"next_id\":4", 1);
        let error = serde_json::from_str::<Project>(&invalid).expect_err("invalid next id");
        assert!(error.to_string().contains("must be greater"), "error was {error}");
    }

    #[test]
    fn projects_reject_duplicate_ids() {
        let duplicate = Id::new(1).expect("id");
        let panel = sample_panel(duplicate, "Panel");
        let root = Container::new(duplicate, "Root", Placement::IDENTITY, vec![panel.into()]);
        assert_eq!(Project::new(root), Err(ProjectError::DuplicateId(duplicate)));
    }

    #[test]
    fn projects_reject_excessive_nesting() {
        let mut nested = Vec::new();
        for index in 0..Project::MAX_ASSEMBLY_DEPTH {
            let id = Id::new(u64::from(index) + 2).expect("id");
            let container = Container::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![container.into()];
        }
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        assert!(matches!(
            Project::new(root),
            Err(ProjectError::TooDeep { max }) if max == Project::MAX_ASSEMBLY_DEPTH
        ));
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
        assert_eq!(project.pivot(root), Some(Pivot::ZERO));
        assert_eq!(
            project.pivot(drawer),
            Some(Pivot::new(
                Length::from_micrometres(250_000),
                Length::from_micrometres(100_000),
                Length::from_micrometres(9_000),
            )),
            "an older file materializes the centred container default"
        );
        assert_eq!(project.pivot(Id::new(3).expect("panel id")), Some(Pivot::ZERO));
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
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));

        let text = serde_json::to_string(&project).expect("serialize migrated project");
        let decoded: Project = serde_json::from_str(&text).expect("reload migrated project");
        let panel = decoded.panel(Id::new(2).expect("id")).expect("reloaded panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &CutoutShape::Rectangle(sample_rect(3, 3, 10, 10)));
    }

    #[test]
    fn cutout_coordinates_are_nominal_and_do_not_move_with_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let material = MaterialId::new(99).expect("material id");
        for edge in [1, 4] {
            let band =
                EdgeBanding::new(EdgeId::new(edge).expect("edge id"), material, true, millimetres(2), millimetres(22))
                    .expect("valid banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        let geometry = project.panel(panel_id).expect("panel").geometry_with_fallbacks().expect("geometry");
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));

        let shape = CutoutShape::Rectangle(sample_rect(10, 10, 20, 20));
        project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let panel = project.panel(panel_id).expect("panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &shape);

        // Nominal coordinates: the 2 mm included bands put the core origin at (2, 2), so this
        // cutout lies entirely in the band and would remove no core material.
        let under_the_band = CutoutShape::Rectangle(sample_rect(0, 0, 1, 1));
        assert_eq!(
            project.add_cutout(panel_id, under_the_band, CutoutDepth::Through, FaceSide::A),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    /// A rectangular ring around `(x, y)` of `width` x `height` millimetres.
    fn ring(x: i64, y: i64, width: i64, height: i64) -> Contour {
        Contour::from_polygon(vec![
            [millimetres(x), millimetres(y)],
            [millimetres(x + width), millimetres(y)],
            [millimetres(x + width), millimetres(y + height)],
            [millimetres(x), millimetres(y + height)],
        ])
        .expect("ring")
    }

    #[test]
    fn contour_cutout_is_stored_and_round_trips() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let shape = CutoutShape::Contour(ring(100, 100, 100, 60));
        let cutout_id =
            project.add_cutout(panel_id, shape.clone(), CutoutDepth::Through, FaceSide::A).expect("add contour cutout");
        let panel = project.panel(panel_id).expect("panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.id(), cutout_id);
        assert_eq!(cutout.shape(), &shape);
        assert_eq!(cutout.depth(), CutoutDepth::Through);

        let text = serde_json::to_string(&project).expect("serialize project");
        let decoded: Project = serde_json::from_str(&text).expect("deserialize project");
        assert_eq!(decoded, project);
        let panel = decoded.panel(panel_id).expect("reloaded panel");
        let Machining::Cutout(cutout) = &panel.machining()[0];
        assert_eq!(cutout.shape(), &shape);
    }

    #[test]
    fn contour_cutout_must_stay_inside_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let next_id = project.next_id();
        let revision = project.revision();

        // A through ring must be strictly inside: touching the edge could open a notch the
        // rectangle-only core trace cannot see.
        for shape in [ring(-10, 100, 120, 60), ring(0, 100, 100, 60), ring(690, 100, 20, 60)] {
            assert_eq!(
                project.add_cutout(panel_id, CutoutShape::Contour(shape), CutoutDepth::Through, FaceSide::A),
                Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
            );
        }
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        // A blind ring may open at the edge like a blind rectangle, but it may not cross it.
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(0, 100, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("blind contour cutout at the edge");
        assert_eq!(
            project.add_cutout(
                panel_id,
                CutoutShape::Contour(ring(-10, 300, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::B,
            ),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
    }

    #[test]
    fn contour_cutout_overlap_follows_the_ring_not_its_bounds() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let blind = CutoutDepth::Blind { depth: millimetres(8) };
        project
            .add_cutout(panel_id, CutoutShape::Rectangle(sample_rect(100, 100, 50, 50)), blind, FaceSide::A)
            .expect("rect cutout");

        let overlapping = CutoutShape::Contour(ring(120, 120, 100, 60));
        assert_eq!(
            project.add_cutout(panel_id, overlapping, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel: panel_id, other: Id::new(5).expect("id") })
        );

        // A C-shaped ring around the rectangle's bounding box leaves the notch free.
        let c_shape = Contour::from_polygon(vec![
            [millimetres(50), millimetres(50)],
            [millimetres(250), millimetres(50)],
            [millimetres(250), millimetres(90)],
            [millimetres(90), millimetres(90)],
            [millimetres(90), millimetres(210)],
            [millimetres(250), millimetres(210)],
            [millimetres(250), millimetres(250)],
            [millimetres(50), millimetres(250)],
        ])
        .expect("C-shaped ring");
        project
            .add_cutout(panel_id, CutoutShape::Contour(c_shape), blind, FaceSide::A)
            .expect("a ring that surrounds the bounds but not the rectangle");

        // Two contours are compared the same way: a crossing ring is an overlap, a nested one is
        // not (a through pair may overlap, so use blind here).
        let crossing = CutoutShape::Contour(ring(200, 220, 100, 60));
        assert!(matches!(
            project.add_cutout(panel_id, crossing, blind, FaceSide::A),
            Err(ProjectError::CutoutOverlap { panel, .. }) if panel == panel_id
        ));
        let nested = CutoutShape::Contour(ring(160, 120, 20, 20));
        project.add_cutout(panel_id, nested, blind, FaceSide::A).expect("a ring inside the C notch");
    }

    #[test]
    fn contour_cutout_is_refused_when_a_resize_moves_the_core() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        project
            .add_cutout(
                panel_id,
                CutoutShape::Contour(ring(100, 100, 100, 60)),
                CutoutDepth::Blind { depth: millimetres(8) },
                FaceSide::A,
            )
            .expect("contour cutout");
        let small = PanelNominalSize::new(millimetres(150), millimetres(500)).expect("small size");
        assert_eq!(
            project.validate_panel_resize(panel_id, small),
            Err(ProjectError::CutoutOutsidePanel { panel: panel_id })
        );
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
    fn nodes_can_be_renamed_and_placed() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::from_rotation_z(0.5)).expect("placement");

        assert_eq!(project.rename_node(panel, "Top").expect("rename panel").get(), 1);
        assert_eq!(project.rename_node(drawer, "Drawer box").expect("rename assembly").get(), 2);
        assert_eq!(project.rename_node(root, "Cabinet").expect("rename root").get(), 3);
        assert_eq!(project.panel(panel).expect("panel").name(), "Top");
        assert_eq!(project.container(drawer).expect("assembly").name(), "Drawer box");
        assert_eq!(project.root().name(), "Cabinet");

        assert_eq!(project.set_node_placement(panel, placement).expect("place panel").get(), 4);
        assert_eq!(project.set_node_placement(drawer, placement).expect("place assembly").get(), 5);
        assert_eq!(project.panel(panel).expect("panel").placement(), placement);
        assert_eq!(project.container(drawer).expect("assembly").placement(), placement);

        assert_eq!(project.set_node_placement(root, placement), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision().get(), 5);
    }

    #[test]
    fn placement_batches_validate_before_mutating_and_bump_once() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");
        let moved = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let raised = Placement::new(DVec3::new(0.0, 0.0, 25.0), DQuat::IDENTITY).expect("placement");
        let revision = project.revision().get();

        assert_eq!(project.set_node_placements(&[]), Err(ProjectError::NoPlacements));
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (panel, raised)]),
            Err(ProjectError::DuplicateNode { id: panel })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (root, raised)]),
            Err(ProjectError::RootImmutable { id: root })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (missing, raised)]),
            Err(ProjectError::NodeNotFound { id: missing })
        );
        assert_eq!(project.revision().get(), revision);
        assert_eq!(project.panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert_eq!(project.container(drawer).expect("assembly").placement(), Placement::IDENTITY);

        assert_eq!(
            project.set_node_placements(&[(panel, moved), (drawer, raised)]).expect("batch").get(),
            revision + 1
        );
        assert_eq!(project.panel(panel).expect("panel").placement(), moved);
        assert_eq!(project.container(drawer).expect("assembly").placement(), raised);
        assert_eq!(project.panel(front).expect("front").placement(), Placement::IDENTITY);
    }

    #[test]
    fn top_most_nodes_drop_descendants_repeats_and_unknown_ids() {
        let project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");

        assert!(project.top_most_nodes(&[]).is_empty());
        assert_eq!(project.top_most_nodes(&[front, panel]), vec![front, panel]);
        assert_eq!(project.top_most_nodes(&[panel, drawer, front]), vec![panel, drawer]);
        assert_eq!(project.top_most_nodes(&[front, drawer]), vec![drawer]);
        assert_eq!(project.top_most_nodes(&[panel, panel, missing]), vec![panel]);
        assert_eq!(project.top_most_nodes(&[root, drawer, panel]), vec![root]);
    }

    #[test]
    fn removing_a_node_cascades_and_protects_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let next_id = project.next_id();

        assert_eq!(project.remove_node(drawer).expect("remove assembly").get(), 1);
        assert!(project.container(drawer).is_none());
        assert!(project.panel(front).is_none());
        assert!(project.panel(panel).is_some());
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.next_id(), next_id);

        assert_eq!(project.remove_node(root), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(
            project.remove_node(Id::new(999).expect("id")),
            Err(ProjectError::NodeNotFound { id: Id::new(999).expect("id") })
        );
        assert_eq!(project.revision().get(), 1);
    }

    #[test]
    fn duplicating_a_node_remaps_every_id() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");

        let copy = project.duplicate_node(panel).expect("duplicate panel");
        assert_eq!(copy, Id::new(5).expect("id"));
        let copied_panel = project.panel(copy).expect("copied panel");
        assert_eq!(copied_panel.name(), "Shelf copy");
        assert_eq!(copied_panel.nominal_size(), project.panel(panel).expect("panel").nominal_size());
        assert_eq!(project.root().children().len(), 3);

        let copied_drawer = project.duplicate_node(drawer).expect("duplicate assembly");
        assert_eq!(copied_drawer, Id::new(6).expect("id"));
        let assembly = project.container(copied_drawer).expect("copied assembly");
        assert_eq!(assembly.name(), "Drawer copy");
        assert_eq!(assembly.children().len(), 1);
        let copied_front = assembly.children().first().expect("copied child").id();
        assert_ne!(copied_front, front);
        assert_eq!(project.node(copied_front).expect("child").name(), "Front");
        assert_eq!(project.parent_of(copied_front), Some(copied_drawer));
        project.validate().expect("ids stay unique");
        assert_eq!(project.root().id(), Id::new(1).expect("id"));
        assert_eq!(
            project.duplicate_node(project.root().id()),
            Err(ProjectError::RootImmutable { id: project.root().id() })
        );
    }

    #[test]
    fn grouping_siblings_preserves_order_and_world_placements() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(0.0, 20.0, 0.0), DQuat::from_rotation_z(0.25)).expect("drawer placement");
        let front_placement = Placement::new(DVec3::new(5.0, 0.0, 0.0), DQuat::IDENTITY).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");
        let before = Placement::compose(drawer_placement, front_placement).expect("world before");
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel, front], "Nested"), Err(ProjectError::NodesNotSiblings));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        assert_eq!(group_id, Id::new(5).expect("id"));
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.root().children().first().expect("group").id(), group_id);
        let group = project.container(group_id).expect("group");
        assert_eq!(group.placement(), Placement::IDENTITY);
        assert_eq!(group.children().len(), 2);
        assert_eq!(group.children()[0].id(), panel);
        assert_eq!(group.children()[1].id(), drawer);
        assert_eq!(project.parent_of(panel), Some(group_id));
        assert_eq!(project.parent_of(drawer), Some(group_id));
        assert_eq!(project.panel(panel).expect("panel").placement(), panel_placement);
        assert_eq!(project.container(drawer).expect("drawer").placement(), drawer_placement);
        let after = Placement::compose(drawer_placement, front_placement).expect("world after");
        let point = Placement::compose(Placement::IDENTITY, after).expect("root world");
        let point = point.transform_point(DVec3::ZERO);
        let expected = before.transform_point(DVec3::ZERO);
        assert!((point - expected).length() < 1e-12, "point was {point:?}, expected {expected:?}");
        assert_eq!(project.revision().get(), revision.get() + 1);
    }

    #[test]
    fn grouping_rejects_duplicates_a_short_selection_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel], "Group"), Err(ProjectError::TooFewNodes { minimum: 2, found: 1 }));
        assert_eq!(project.group_nodes(&[panel, panel], "Group"), Err(ProjectError::DuplicateNode { id: panel }));
        assert_eq!(project.group_nodes(&[root, panel], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.group_nodes(&[root, root], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn ungrouping_flattens_children_and_keeps_their_world_placements() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::from_rotation_z(0.5)).expect("drawer placement");
        let front_placement =
            Placement::new(DVec3::new(0.0, 5.0, 0.0), DQuat::from_rotation_x(0.25)).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        let group_placement =
            Placement::new(DVec3::new(50.0, 60.0, 70.0), DQuat::from_rotation_y(0.3)).expect("group placement");
        project.set_node_placement(group_id, group_placement).expect("place group");
        let revision = project.revision();

        assert_eq!(project.ungroup_group(group_id).expect("ungroup").get(), revision.get() + 1);
        assert!(project.container(group_id).is_none());
        assert_eq!(project.parent_of(panel), Some(root));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.parent_of(front), Some(drawer));
        assert_eq!(project.root().children().len(), 2);

        let expected_panel = Placement::compose(group_placement, panel_placement).expect("panel world");
        assert_eq!(project.panel(panel).expect("panel").placement(), expected_panel);
        let expected_drawer = Placement::compose(group_placement, drawer_placement).expect("drawer world");
        assert_eq!(project.container(drawer).expect("drawer").placement(), expected_drawer);
        let expected_front = Placement::compose(expected_drawer, front_placement).expect("front world");
        assert_eq!(project.world_placement(front).expect("front world"), expected_front);
        assert_eq!(project.panel(front).expect("front").placement(), front_placement);

        assert_eq!(project.ungroup_group(drawer), Err(ProjectError::NotAComponent { id: drawer }));
        assert_eq!(project.ungroup_group(panel), Err(ProjectError::NotAnOrdinaryContainer { id: panel }));
        assert_eq!(project.ungroup_group(root), Err(ProjectError::RootImmutable { id: root }));
    }

    #[test]
    fn projects_set_and_clear_properties_with_one_revision_per_batch() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let drawer = Id::new(3).expect("drawer");
        let revision = project.revision();

        let next = project.set_property(panel, "material.finish", PropertyValue::text("oak")).expect("set property");
        assert_eq!(next.get(), revision.get() + 1);
        assert_eq!(project.properties(panel).and_then(|p| p.get("material.finish")), Some(&PropertyValue::text("oak")));

        let next = project
            .set_properties(&[
                PropertyChange { node: panel, key: "edge".to_owned(), value: PropertyValue::integer(2) },
                PropertyChange { node: drawer, key: "drawer.count".to_owned(), value: PropertyValue::integer(3) },
            ])
            .expect("batch");
        assert_eq!(next.get(), revision.get() + 2);
        assert_eq!(project.revision(), next);
        assert_eq!(project.properties(drawer).and_then(|p| p.get("drawer.count")), Some(&PropertyValue::integer(3)));
    }

    #[test]
    fn project_property_batches_reject_duplicates_unknown_nodes_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel");
        let missing = Id::new(99).expect("id");
        let revision = project.revision();
        let change = |node, key: &str| PropertyChange { node, key: key.to_owned(), value: PropertyValue::integer(1) };

        assert_eq!(project.set_properties(&[]), Err(ProjectError::NoPropertyChanges));
        assert_eq!(
            project.set_properties(&[change(panel, "a"), change(panel, "a")]),
            Err(ProjectError::DuplicateProperty { node: panel, key: "a".to_owned() })
        );
        assert_eq!(project.set_properties(&[change(missing, "a")]), Err(ProjectError::NodeNotFound { id: missing }));
        assert_eq!(project.set_properties(&[change(root, "a")]), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision(), revision);

        assert_eq!(
            project.clear_property(panel, "missing"),
            Err(ProjectError::PropertyNotFound { node: panel, key: "missing".to_owned() })
        );
        assert_eq!(project.revision(), revision);
        project.set_property(panel, "a", PropertyValue::integer(1)).expect("set");
        assert_eq!(project.clear_property(panel, "a").expect("clear").get(), revision.get() + 2);
        assert!(!project.properties(panel).expect("properties").contains_key("a"));
    }

    #[test]
    fn a_furniture_type_marks_a_container_as_a_component() {
        let mut project = sample_project();
        let root = project.root().id();
        let group = project.group_nodes(&[Id::new(2).expect("id"), Id::new(3).expect("id")], "G").expect("group");
        assert!(!project.is_component(group));

        project.set_property(group, SYSTEM_FURNTYPE, PropertyValue::furn_type(furntype("12.03.02"))).expect("type");
        assert!(project.is_component(group));
        project.set_property(group, SYSTEM_COMPONENT, PropertyValue::bool(false)).expect("false marker");
        assert!(project.is_component(group), "an explicit false does not erase the furniture type");
        assert_eq!(project.furntype(group), Some(furntype("12.03.02")));
        assert_eq!(project.furntype(root), Some(FurnType::UNCLASSIFIED));

        assert_eq!(project.ungroup_group(group), Err(ProjectError::NotAComponent { id: group }));

        project.convert_to_group(group).expect("convert to group");
        assert!(project.container(group).is_none());
        assert!(project.properties(Id::new(2).expect("id")).is_some());
    }

    #[test]
    fn queries_select_by_kind_furniture_type_and_property() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let front = Id::new(4).expect("front");
        let drawer = Id::new(3).expect("drawer");
        project.set_property(drawer, SYSTEM_FURNTYPE, PropertyValue::furn_type(furntype("12.03.02"))).expect("type");
        project.set_property(panel, "role", PropertyValue::text("shelf")).expect("role");

        assert_eq!(project.query(&NodeQuery::any()), vec![panel, drawer, front]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Container)), vec![drawer]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Panel)), vec![panel, front]);
        assert_eq!(
            project.query(&NodeQuery::any().with_furntype_under(furntype("12"))),
            vec![drawer],
            "a query on a group matches every type below it"
        );
        assert_eq!(project.query(&NodeQuery::any().with_furntype_under(furntype("12.03.02"))), vec![drawer]);
        assert!(project.query(&NodeQuery::any().with_furntype_under(furntype("13"))).is_empty());
        assert_eq!(
            project.query(&NodeQuery::any().with_furntype_under(FurnType::UNCLASSIFIED)),
            vec![panel, front],
            "unclassified is a sentinel, not the root of the type tree"
        );
        assert_eq!(project.query(&NodeQuery::any().with_property("role", PropertyValue::text("shelf"))), vec![panel]);
        assert!(
            project
                .query(
                    &NodeQuery::any()
                        .with_kind(StructuralKind::Panel)
                        .with_property("role", PropertyValue::text("shelf"))
                )
                .len()
                == 1
        );
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
