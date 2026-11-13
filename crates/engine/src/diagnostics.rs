//! Derived panel diagnostics.
//!
//! Problems that a document can keep — a missing material, a non-positive core or machining that
//! no longer fits — are computed from the project and catalogue on demand. They are never stored in
//! the document and never make a scene unreadable.

use furnicraft_model::{
    Catalog, ContourSegmentId, CoreViolation, CutoutDepth, EdgeId, Id, MachiningId, MaterialId, PanelGeometryError,
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
    /// Ring banding refers to a material that is absent from the catalogue.
    MissingRingBandingMaterial {
        /// The ring segment whose banding is unresolved.
        segment: ContourSegmentId,
        /// The missing material id.
        material: MaterialId,
    },
    /// The resolved core has a non-positive length or width.
    NonPositiveCore,
    /// The outline or its edge banding does not form a valid contour ring.
    InvalidOutline,
    /// A through cutout does not reach the core, or a blind cutout is not fully inside it.
    MachiningOutsideCore {
        /// The machining record that does not fit.
        machining: MachiningId,
    },
    /// The through cutouts remove the whole core.
    CoreRemoved,
    /// The through cutouts split the remaining core into disconnected pieces.
    CoreDisconnected,
    /// A blind cutout is at least as deep as the resolved panel thickness.
    BlindDepthNotLessThanThickness {
        /// The machining record that is too deep.
        machining: MachiningId,
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
    for band in panel.ring_banding() {
        if catalog.edge_material(band.material()).is_none() {
            diagnostics.push(PanelDiagnostic::MissingRingBandingMaterial {
                segment: band.segment(),
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
        Err(ProjectError::PanelGeometry(PanelGeometryError::Contour)) => {
            diagnostics.push(PanelDiagnostic::InvalidOutline);
            return diagnostics;
        }
        Err(_) => return diagnostics,
    };

    for machining in panel.machining() {
        let Some(cutout) = machining.cutout() else {
            continue;
        };
        // Every outline is traced exactly against the core: a through cut may open a notch at the
        // edge, while a blind cut may touch the edge but not cross it. A shape whose exact region
        // cannot be built is reported as outside rather than skipped silently.
        let Ok(region) = cutout.shape().region() else {
            diagnostics.push(PanelDiagnostic::MachiningOutsideCore { machining: machining.id() });
            continue;
        };
        let outside = match cutout.depth() {
            CutoutDepth::Through => !geometry.cutout_reaches_core(&region).unwrap_or(false),
            CutoutDepth::Blind { .. } => !geometry.cutout_fits_core(&region).unwrap_or(false),
        };
        if outside {
            diagnostics.push(PanelDiagnostic::MachiningOutsideCore { machining: machining.id() });
        }
        if let CutoutDepth::Blind { depth } = cutout.depth()
            && depth >= geometry.thickness()
        {
            diagnostics.push(PanelDiagnostic::BlindDepthNotLessThanThickness {
                machining: machining.id(),
                depth,
                thickness: geometry.thickness(),
            });
        }
    }
    if let Ok(violation) = panel.core_violation(&geometry) {
        match violation {
            Some(CoreViolation::Removed) => diagnostics.push(PanelDiagnostic::CoreRemoved),
            Some(CoreViolation::Split) => diagnostics.push(PanelDiagnostic::CoreDisconnected),
            None => {}
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use furnicraft_model::{
        Catalog, Contour, CornerBanding, CornerFeature, CornerFeatureKind, CutoutShape, EdgeBanding, EdgeId, FaceSide,
        MaterialId, PanelMaterial, Placement, RectCorner, RingBanding,
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
    fn missing_ring_banding_material_is_a_diagnostic() {
        let mut project = sample_project();
        let length = Length::from_millimetres(700).expect("length");
        let width = Length::from_millimetres(500).expect("width");
        let ring = Contour::from_polygon(vec![
            [Length::ZERO, Length::ZERO],
            [length, Length::ZERO],
            [length, width],
            [Length::ZERO, width],
        ])
        .expect("ring");
        let panel = project
            .add_panel_contour(
                project.root().id(),
                "Contour",
                &ring,
                None,
                Length::from_millimetres(18).expect("thickness"),
                Placement::IDENTITY,
            )
            .expect("contour panel");
        let segment = project
            .panel(panel)
            .expect("panel")
            .outline_contour()
            .expect("ring")
            .entries()
            .first()
            .expect("segment")
            .id();
        let missing = MaterialId::new(999).expect("material id");
        let band = RingBanding::new(
            segment,
            missing,
            false,
            Length::from_micrometres(2_000),
            Length::from_micrometres(22_000),
        )
        .expect("band");
        project.set_ring_banding(panel, band).expect("ring banding");

        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel);
        assert!(diagnostics.contains(&PanelDiagnostic::MissingRingBandingMaterial { segment, material: missing }));
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
    fn a_broken_banding_is_a_diagnostic_not_a_load_error() {
        let broken = r#"{
            "schema_version": 22,
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
        assert!(diagnostics.contains(&PanelDiagnostic::InvalidOutline));
    }

    #[test]
    fn machining_outside_the_core_is_a_diagnostic() {
        let broken = r#"{
            "schema_version": 22,
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
                    "machining": [{
                        "id": 3,
                        "anchor": {"kind": "face", "side": "A"},
                        "params": {
                            "kind": "cutout",
                            "shape": {"rectangle": {"x": 900000, "y": 10000, "width": 100000, "height": 50000}},
                            "depth": "through"
                        }
                    }],
                    "edge_banding": []
                }}]
            }
        }"#;
        let project: Project = serde_json::from_str(broken).expect("broken project loads");
        assert!(project.validate().is_ok());
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(
            diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore {
                machining: MachiningId::new(3).expect("cutout id")
            })
        );
    }

    fn project_with_through_cutout(x: i64, y: i64, width: i64, height: i64) -> Project {
        let value = serde_json::json!({
            "schema_version": 22,
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
                    "machining": [{
                        "id": 3,
                        "anchor": {"kind": "face", "side": "A"},
                        "params": {
                            "kind": "cutout",
                            "shape": {"rectangle": {"x": x, "y": y, "width": width, "height": height}},
                            "depth": "through"
                        }
                    }],
                    "edge_banding": []
                }}]
            }
        });
        serde_json::from_value(value).expect("project loads")
    }

    /// The current ring form of a closed polygon, with one segment per edge and its ids.
    fn ring(points: &[[i64; 2]]) -> serde_json::Value {
        let count = points.len();
        let segments = (0..count)
            .map(|index| {
                let start = points.get(index).copied().unwrap_or([0, 0]);
                let end = points.get((index + 1) % count).copied().unwrap_or([0, 0]);
                serde_json::json!({"kind": "line", "id": index + 1, "start": start, "end": end})
            })
            .collect::<Vec<_>>();
        serde_json::json!({"segments": segments, "next_segment_id": count + 1})
    }

    fn project_with_contour_cutout(points: &[[i64; 2]], depth: serde_json::Value) -> Project {
        let value = serde_json::json!({
            "schema_version": 22,
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
                    "machining": [{
                        "id": 3,
                        "anchor": {"kind": "face", "side": "A"},
                        "params": {
                            "kind": "cutout",
                            "shape": {"contour": ring(points)},
                            "depth": depth
                        }
                    }],
                    "edge_banding": []
                }}]
            }
        });
        serde_json::from_value(value).expect("project loads")
    }

    #[test]
    fn a_through_contour_cutout_that_misses_the_core_is_a_diagnostic() {
        let machining = MachiningId::new(3).expect("id");
        let outside = [[-200_000, 100_000], [-100_000, 100_000], [-100_000, 200_000], [-200_000, 200_000]];

        let project = project_with_contour_cutout(&outside, serde_json::json!("through"));
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { machining }));

        // A through ring at the edge opens a notch and is legal; so is a blind ring there.
        let at_the_edge = [[0, 100_000], [100_000, 100_000], [100_000, 200_000], [0, 200_000]];
        let project = project_with_contour_cutout(&at_the_edge, serde_json::json!("through"));
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(!diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { machining }));

        // A blind ring may open at the edge, because it removes no through material.
        let project = project_with_contour_cutout(&at_the_edge, serde_json::json!({"blind": {"depth": 8_000}}));
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(!diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { machining }));

        // A blind ring may touch the edge but not cross it.
        let crossing = [[-10_000, 100_000], [100_000, 100_000], [100_000, 200_000], [-10_000, 200_000]];
        let project = project_with_contour_cutout(&crossing, serde_json::json!({"blind": {"depth": 8_000}}));
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { machining }));

        let inside = [[100_000, 100_000], [200_000, 100_000], [200_000, 150_000], [100_000, 150_000]];
        let project = project_with_contour_cutout(&inside, serde_json::json!("through"));
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(!diagnostics.contains(&PanelDiagnostic::MachiningOutsideCore { machining }));
    }

    #[test]
    fn a_boundary_notch_is_not_an_outside_cutout() {
        let project = project_with_through_cutout(650_000, 10_000, 100_000, 50_000);
        let diagnostics = diagnose_panel(&project, &sample_catalog(), panel_id());
        assert!(
            !diagnostics
                .contains(&PanelDiagnostic::MachiningOutsideCore { machining: MachiningId::new(3).expect("id") })
        );
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
