//! Derived panel diagnostics.
//!
//! Problems that a document can keep — a missing material, a non-positive core or machining that
//! no longer fits — are computed from the project and catalogue on demand. They are never stored in
//! the document and never make a scene unreadable.

use furnicraft_model::{
    Catalog, CutoutDepth, CutoutShape, EdgeId, Id, Machining, MaterialId, PanelGeometryError, PanelRect, Project,
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
    /// The resolved core has a non-positive length or width.
    NonPositiveCore,
    /// A cutout does not fit inside the resolved core.
    MachiningOutsideCore {
        /// The cutout that does not fit.
        cutout: Id,
    },
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
    let thickness = match panel.material() {
        None => {
            diagnostics.push(PanelDiagnostic::NoMaterial);
            panel.last_known_thickness()
        }
        Some(material_id) => match catalog.panel_material(material_id) {
            Some(material) => material.nominal_thickness(),
            None => {
                diagnostics.push(PanelDiagnostic::MissingMaterial { material: material_id });
                panel.last_known_thickness()
            }
        },
    };

    for band in panel.edge_banding() {
        if catalog.edge_material(band.material()).is_none() {
            diagnostics.push(PanelDiagnostic::MissingBandingMaterial { edge: band.edge(), material: band.material() });
        }
    }

    let geometry = match panel.geometry(thickness) {
        Ok(geometry) => geometry,
        Err(PanelGeometryError::NonPositive { .. }) => {
            diagnostics.push(PanelDiagnostic::NonPositiveCore);
            return diagnostics;
        }
        Err(_) => return diagnostics,
    };

    for machining in panel.machining() {
        let Machining::Cutout(cutout) = machining;
        if !rect_inside(cutout_rect(cutout.shape()), geometry.core_length(), geometry.core_width()) {
            diagnostics.push(PanelDiagnostic::MachiningOutsideCore { cutout: cutout.id() });
        }
        if let CutoutDepth::Blind { depth } = cutout.depth()
            && depth >= geometry.thickness()
        {
            diagnostics.push(PanelDiagnostic::BlindDepthNotLessThanThickness {
                cutout: cutout.id(),
                depth,
                thickness: geometry.thickness(),
            });
        }
    }

    diagnostics
}

fn cutout_rect(shape: CutoutShape) -> PanelRect {
    match shape {
        CutoutShape::Rectangle(rect) => rect,
    }
}

fn rect_inside(rect: PanelRect, core_length: Length, core_width: Length) -> bool {
    let x0 = rect.x().micrometres() as i128;
    let y0 = rect.y().micrometres() as i128;
    let x1 = x0 + rect.width().micrometres() as i128;
    let y1 = y0 + rect.height().micrometres() as i128;
    let panel_x = core_length.micrometres() as i128;
    let panel_y = core_width.micrometres() as i128;
    x0 >= 0 && y0 >= 0 && x1 <= panel_x && y1 <= panel_y
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use furnicraft_model::{Catalog, EdgeBanding, EdgeId, FaceSide, MaterialId, PanelMaterial};
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
                        "material": 101,
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

    #[test]
    fn missing_panels_have_no_diagnostics() {
        let project = sample_project();
        assert!(diagnose_panel(&project, &sample_catalog(), Id::new(999).expect("id")).is_empty());
    }
}
