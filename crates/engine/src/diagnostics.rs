//! Derived panel diagnostics.
//!
//! Problems that a document can keep — a missing material, a non-positive core or machining that
//! no longer fits — are computed from the project and catalogue on demand. They are never stored in
//! the document and never make a scene unreadable.

use furnicraft_model::{
    Catalog, CoreViolation, CutoutDepth, CutoutShape, EdgeId, Id, Machining, MaterialId, PanelGeometryError, PanelRect,
    Project, ProjectError, VertexId,
};
use furnicraft_units::Length;

/// One issue found while resolving a panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelDiagnostic {
    /// The panel has no material assigned.
    NoMaterial,
    /// The panel's material id is absent from the catalogue.
    MissingMaterial {
        /// The missing material id.
        material: MaterialId,
    },
    /// Edge banding refers to a material that is absent from the catalogue.
    MissingBandingMaterial {
        /// The edge whose banding is unresolved.
        edge: EdgeId,
        /// The missing material id.
        material: MaterialId,
    },
    /// Corner banding refers to a material that is absent from the catalogue.
    MissingCornerBandingMaterial {
        /// The corner whose banding is unresolved.
        corner: VertexId,
        /// The missing material id.
        material: MaterialId,
    },
    /// The resolved core has a non-positive length or width.
    NonPositiveCore,
    /// A through cutout does not reach the core, or a blind cutout is not fully inside it.
    MachiningOutsideCore {
        /// The cutout that does not fit.
        cutout: Id,
    },
    /// The through cutouts remove the whole core.
    CoreRemoved,
    /// The through cutouts split the remaining core into disconnected pieces.
    CoreDisconnected,
    /// A blind cutout is at least as deep as the resolved panel thickness.
    BlindDepthNotLessThanThickness {
        /// The cutout that is too deep.
        cutout: Id,
        /// The requested blind depth.
        depth: Length,
        /// The resolved panel thickness.
        thickness: Length,
    },
}

/// Resolves the issues of one panel against a catalogue.
///
/// A missing panel has no issues to report and yields an empty list. The function is read-only and
/// can run on every rebuild.
pub fn diagnose_panel(project: &Project, catalog: &Catalog, panel_id: Id) -> Vec<PanelDiagnostic> {
    let Some(panel) = project.panel(panel_id) else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    match panel.material() {
        None => diagnostics.push(PanelDiagnostic::NoMaterial),
        Some(material_id) => {
            if catalog.panel_material(material_id).is_none() {
                diagnostics.push(PanelDiagnostic::MissingMaterial { material: material_id });
            }
        }
    }

    for band in panel.edge_banding() {
        if catalog.edge_material(band.material()).is_none() {
            diagnostics.push(PanelDiagnostic::MissingBandingMaterial { edge: band.edge(), material: band.material() });
        }
    }
    for band in panel.corner_banding() {
        if catalog.edge_material(band.material()).is_none() {
            diagnostics.push(PanelDiagnostic::MissingCornerBandingMaterial {
                corner: band.corner(),
                material: band.material(),
            });
        }
    }

    let geometry = match crate::resolve::panel_geometry(project, catalog, panel_id) {
        Ok(geometry) => geometry,
        Err(ProjectError::PanelGeometry(PanelGeometryError::NonPositive { .. })) => {
            diagnostics.push(PanelDiagnostic::NonPositiveCore);
            return diagnostics;
        }
        Err(_) => return diagnostics,
    };

    for machining in panel.machining() {
        let Machining::Cutout(cutout) = machining;
        let rect = cutout_rect(cutout.shape());
        match cutout.depth() {
            CutoutDepth::Through => {
                if !geometry.cutout_reaches_core(rect) {
                    diagnostics.push(PanelDiagnostic::MachiningOutsideCore { cutout: cutout.id() });
                }
            }
            CutoutDepth::Blind { depth } => {
                if !geometry.cutout_fits_core(rect) {
                    diagnostics.push(PanelDiagnostic::MachiningOutsideCore { cutout: cutout.id() });
                }
                if depth >= geometry.thickness() {
                    diagnostics.push(PanelDiagnostic::BlindDepthNotLessThanThickness {
                        cutout: cutout.id(),
                        depth,
                        thickness: geometry.thickness(),
                    });
                }
            }
        }
    }
    match panel.core_violation(geometry) {
        Some(CoreViolation::Removed) => diagnostics.push(PanelDiagnostic::CoreRemoved),
        Some(CoreViolation::Split) => diagnostics.push(PanelDiagnostic::CoreDisconnected),
        None => {}
    }

    diagnostics
}

fn cutout_rect(shape: CutoutShape) -> PanelRect {
    match shape {
        CutoutShape::Rectangle(rect) => rect,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use furnicraft_model::{
        Catalog, CornerBanding, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeId, FaceSide, MaterialId,
        PanelMaterial, RectCorner,
    };
    use furnicraft_testkit::{
        sample_blind_depth, sample_catalog, sample_material_id, sample_panel_thickness, sample_project,
        sample_thin_panel_thickness,
    };

    fn panel_id() -> Id {
        Id::new(2).expect("id")
    }

    #[test]
    fn missing_materials_are_diagnostics() {
        let mut project = sample_project();
        let missing = MaterialId::new(999).expect("material id");
        let band = EdgeBanding::new(
            EdgeId::new(1).expect("edge"),
            missing,
            false,
            Length::from_micrometres(2_000),
            Length::from_micrometres(22_000),
        )
        .expect("band");
        project.set_edge_banding(panel_id(), band).expect("banding");

        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::NoMaterial));
        assert!(diagnostics.contains(&PanelDiagnostic::MissingBandingMaterial {
            edge: EdgeId::new(1).expect("edge"),
            material: missing,
        }));
    }

    #[test]
    fn missing_corner_banding_material_is_a_diagnostic() {
        let mut project = sample_project();
        let corner = RectCorner::BottomRight.id();
        project
            .set_corner_feature(
                panel_id(),
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer {
                        incoming: Length::from_millimetres(12).expect("incoming"),
                        outgoing: Length::from_millimetres(8).expect("outgoing"),
                    },
                )
                .expect("chamfer"),
            )
            .expect("set chamfer");
        let missing = MaterialId::new(999).expect("material id");
        project
            .set_corner_banding(
                panel_id(),
                CornerBanding::new(
                    corner,
                    missing,
                    false,
                    Length::from_micrometres(2_000),
                    Length::from_micrometres(22_000),
                )
                .expect("corner band"),
            )
            .expect("set corner banding");
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::MissingCornerBandingMaterial { corner, material: missing }));
    }

    #[test]
    fn material_selection_clears_the_missing_material_diagnostic() {
        let mut project = sample_project();
        project.set_panel_material(panel_id(), sample_material_id(), sample_panel_thickness()).expect("material");
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(!diagnostics.contains(&PanelDiagnostic::NoMaterial));
    }

    #[test]
    fn a_shallow_material_marks_a_blind_cutout() {
        let mut project = sample_project();
        project
            .add_cutout(
                panel_id(),
                CutoutShape::Rectangle(furnicraft_testkit::sample_cutout_rect()),
                CutoutDepth::Blind { depth: sample_blind_depth() },
                FaceSide::A,
            )
            .expect("blind cutout");
        project.set_panel_material(panel_id(), sample_material_id(), sample_thin_panel_thickness()).expect("material");
        let thin =
            PanelMaterial::new(sample_material_id(), "Thin board", "mdf", sample_thin_panel_thickness(), "white")
                .expect("thin material");
        let catalog = Catalog::new(vec![thin], Vec::new()).expect("catalog");
        let diagnostics = diagnose_panel(&project, &catalog, panel_id());
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            PanelDiagnostic::BlindDepthNotLessThanThickness { depth, thickness, .. }
                if *depth == sample_blind_depth() && *thickness == sample_thin_panel_thickness()
        )));
    }

    #[test]
    fn non_positive_cores_are_diagnostics_not_load_errors() {
        let broken = r#"{
            "schema_version": 2,
            "revision": 0,
            "next_id": 20,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Broken",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "last_known_thickness": 18000,
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "machining": [],
                    "edge_banding": [{
                        "edge": 1,
                        "material": 999,
                        "included_in_size": true,
                        "last_known_thickness": 500000,
                        "last_known_width": 22000
                    }]
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(broken).expect("broken project loads");
        assert!(project.validate().is_ok());
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::NonPositiveCore));
    }

    #[test]
    fn machining_outside_the_core_is_a_diagnostic() {
        let broken = r#"{
            "schema_version": 2,
            "revision": 0,
            "next_id": 20,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Panel",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "last_known_thickness": 18000,
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "machining": [{"cutout": {
                        "id": 3,
                        "shape": {"rectangle": {"x": 900000, "y": 10000, "width": 100000, "height": 50000}},
                        "depth": "through",
                        "entry": "A"
                    }}],
                    "edge_banding": []
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(broken).expect("broken project loads");
        assert!(project.validate().is_ok());
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(
            diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { cutout: Id::new(3).expect("cutout id") })
        );
    }

    fn project_with_through_cutout(x: i64, y: i64, width: i64, height: i64) -> Project {
        let value = serde_json::json!({
            "schema_version": 3,
            "revision": 0,
            "next_id": 20,
            "root": {
                "id": 1,
                "name": "Root",
                "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                "children": [{"panel": {
                    "id": 2,
                    "name": "Panel",
                    "nominal_size": {"length": 700000, "width": 500000},
                    "last_known_thickness": 18000,
                    "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]},
                    "machining": [{"cutout": {
                        "id": 3,
                        "shape": {"rectangle": {"x": x, "y": y, "width": width, "height": height}},
                        "depth": "through",
                        "entry": "A"
                    }}],
                    "edge_banding": []
                }}]
            }
        });
        serde_json::from_value(value).expect("project loads")
    }

    #[test]
    fn a_boundary_notch_is_not_an_outside_cutout() {
        let project = project_with_through_cutout(650_000, 10_000, 100_000, 50_000);
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(!diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { cutout: Id::new(3).expect("id") }));
        assert!(!diagnostics.contains(&PanelDiagnostic::CoreDisconnected));
        assert!(!diagnostics.contains(&PanelDiagnostic::CoreRemoved));
    }

    #[test]
    fn split_and_removed_cores_are_diagnostics() {
        let split = project_with_through_cutout(0, 200_000, 700_000, 50_000);
        let diagnostics = diagnose_panel(&split, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::CoreDisconnected));

        let removed = project_with_through_cutout(0, 0, 700_000, 500_000);
        let diagnostics = diagnose_panel(&removed, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::CoreRemoved));
    }

    #[test]
    fn missing_panels_have_no_diagnostics() {
        let project = sample_project();
        assert!(diagnose_panel(&project, &sample_catalog(), Id::new(999).expect("id")).is_empty());
    }
}
