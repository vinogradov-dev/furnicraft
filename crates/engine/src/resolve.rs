//! Catalogue-aware resolution of panel geometry.
//!
//! The model stores fallback thicknesses so a missing material never makes a scene unreadable. The
//! session resolves the catalogue first and uses those fallbacks only when a material is absent.

use furnicraft_model::{Catalog, EdgeBanding, Id, Panel, PanelGeometry, Project, ProjectError};
use furnicraft_units::Length;

/// The resolved panel thickness: catalogue material first, stored fallback otherwise.
pub(crate) fn panel_thickness(panel: &Panel, catalog: &Catalog) -> Length {
    panel
        .material()
        .and_then(|id| catalog.panel_material(id))
        .map_or(panel.last_known_thickness(), |material| material.nominal_thickness())
}

/// The resolved banding thickness: catalogue material first, stored fallback otherwise.
pub(crate) fn band_thickness(band: &EdgeBanding, catalog: &Catalog) -> Length {
    catalog.edge_material(band.material()).map_or(band.last_known_thickness(), |material| material.thickness())
}

/// The resolved banding width: catalogue material first, stored fallback otherwise.
pub(crate) fn band_width(band: &EdgeBanding, catalog: &Catalog) -> Length {
    catalog.edge_material(band.material()).map_or(band.last_known_width(), |material| material.width())
}

/// Resolves one panel's core, overall and effective thickness.
pub(crate) fn panel_geometry(
    project: &Project,
    catalog: &Catalog,
    panel_id: Id,
) -> Result<PanelGeometry, ProjectError> {
    let panel = project.panel(panel_id).ok_or(ProjectError::PanelNotFound { id: panel_id })?;
    let thickness = panel_thickness(panel, catalog);
    Ok(panel.geometry_with_banding(thickness, |band| band_thickness(band, catalog))?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use furnicraft_model::{EdgeBandingMaterial, PanelMaterial};
    use furnicraft_testkit::{sample_edge_material_id, sample_material_id, sample_panel_thickness, sample_project};
    use furnicraft_units::Length;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("millimetres fit")
    }

    #[test]
    fn catalogue_values_override_stored_fallbacks() {
        let mut project = sample_project();
        let panel_id = Id::new(2).expect("id");
        let edge_id = furnicraft_model::EdgeId::new(1).expect("edge");
        let band =
            EdgeBanding::new(edge_id, sample_edge_material_id(), true, millimetres(2), millimetres(22)).expect("band");
        project.set_edge_banding(panel_id, band).expect("banding");
        project.set_panel_material(panel_id, sample_material_id(), sample_panel_thickness()).expect("material");

        let panel_material = PanelMaterial::new(sample_material_id(), "Thick board", "mdf", millimetres(25), "white")
            .expect("panel material");
        let edge_material = EdgeBandingMaterial::new(
            sample_edge_material_id(),
            "Thick edge",
            "abs",
            millimetres(4),
            millimetres(30),
            "white",
        )
        .expect("edge material");
        let catalog = Catalog::new(vec![panel_material], vec![edge_material]).expect("catalog");

        let geometry = panel_geometry(&project, &catalog, panel_id).expect("resolved geometry");
        assert_eq!(geometry.thickness(), millimetres(25));
        assert_eq!(geometry.core_y(), millimetres(4));
        assert_eq!(geometry.core_width(), millimetres(496));
        assert_eq!(geometry.overall_width(), millimetres(500));

        let panel = project.panel(panel_id).expect("panel");
        assert_eq!(band_width(&panel.edge_banding()[0], &catalog), millimetres(30));
    }

    #[test]
    fn missing_materials_keep_the_stored_fallbacks() {
        let project = sample_project();
        let catalog = Catalog::default();
        let geometry = panel_geometry(&project, &catalog, Id::new(2).expect("id")).expect("resolved geometry");
        assert_eq!(geometry.thickness(), sample_panel_thickness());
    }

    #[test]
    fn missing_panels_are_reported() {
        let project = sample_project();
        let error =
            panel_geometry(&project, &Catalog::default(), Id::new(999).expect("id")).expect_err("missing panel");
        assert!(matches!(error, ProjectError::PanelNotFound { id } if id == Id::new(999).expect("id")));
    }
}
