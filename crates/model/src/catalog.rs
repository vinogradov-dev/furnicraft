//! The external material catalogue.
//!
//! Materials are not part of the scene document: a project stores only stable material ids and
//! fallbacks. This module defines the catalogue data and its invariants; loading a file is the
//! application shell's job.

use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroU64;

use furnicraft_units::Length;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PanelSize;

/// A stable catalogue identifier.
///
/// Material ids belong to the catalogue, not to [`crate::Project`]'s id space. They are non-zero,
/// unique within a catalogue and never reused, so an old scene cannot silently point at a new
/// material with the same number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialId(NonZeroU64);

impl MaterialId {
    /// Creates an id from a non-zero raw value.
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The numeric value.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for MaterialId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A panel-body material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PanelMaterialWire")]
pub struct PanelMaterial {
    id: MaterialId,
    name: String,
    kind: String,
    nominal_thickness: Length,
    color: String,
}

impl PanelMaterial {
    /// Creates a validated panel material.
    pub fn new(
        id: MaterialId,
        name: impl Into<String>,
        kind: impl Into<String>,
        nominal_thickness: Length,
        color: impl Into<String>,
    ) -> Result<Self, CatalogError> {
        let name = name.into();
        let kind = kind.into();
        validate_text(id, "name", &name)?;
        validate_text(id, "kind", &kind)?;
        validate_dimension(id, "nominal_thickness", nominal_thickness, PanelSize::MAX_THICKNESS)?;
        Ok(Self { id, name, kind, nominal_thickness, color: color.into() })
    }

    /// The stable material id.
    pub const fn id(&self) -> MaterialId {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The material kind, such as `chipboard` or `mdf`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The nominal board thickness.
    pub const fn nominal_thickness(&self) -> Length {
        self.nominal_thickness
    }

    /// The colour or decor marker.
    pub fn color(&self) -> &str {
        &self.color
    }
}

/// A ready-made edge-banding material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EdgeBandingMaterialWire")]
pub struct EdgeBandingMaterial {
    id: MaterialId,
    name: String,
    kind: String,
    thickness: Length,
    width: Length,
    color: String,
}

impl EdgeBandingMaterial {
    /// Creates a validated edge-banding material.
    pub fn new(
        id: MaterialId,
        name: impl Into<String>,
        kind: impl Into<String>,
        thickness: Length,
        width: Length,
        color: impl Into<String>,
    ) -> Result<Self, CatalogError> {
        let name = name.into();
        let kind = kind.into();
        validate_text(id, "name", &name)?;
        validate_text(id, "kind", &kind)?;
        validate_dimension(id, "thickness", thickness, PanelSize::MAX_LENGTH)?;
        validate_dimension(id, "width", width, PanelSize::MAX_THICKNESS)?;
        Ok(Self { id, name, kind, thickness, width, color: color.into() })
    }

    /// The stable material id.
    pub const fn id(&self) -> MaterialId {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The material kind, such as `abs` or `pvc`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The banding profile thickness in the panel plane.
    pub const fn thickness(&self) -> Length {
        self.thickness
    }

    /// The banding profile width across the panel thickness.
    pub const fn width(&self) -> Length {
        self.width
    }

    /// The colour or decor marker.
    pub fn color(&self) -> &str {
        &self.color
    }
}

/// An external, read-only collection of materials.
///
/// The catalogue is not stored in a scene and is not part of [`crate::Project`]. A session receives
/// one immutable instance; material ids are unique across both lists and are never reused.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CatalogWire")]
pub struct Catalog {
    panel_materials: Vec<PanelMaterial>,
    edge_materials: Vec<EdgeBandingMaterial>,
}

impl Catalog {
    /// Creates a catalogue after checking that every material id is unique.
    pub fn new(
        panel_materials: Vec<PanelMaterial>,
        edge_materials: Vec<EdgeBandingMaterial>,
    ) -> Result<Self, CatalogError> {
        let mut ids = HashSet::new();
        for material in &panel_materials {
            if !ids.insert(material.id) {
                return Err(CatalogError::DuplicateId(material.id));
            }
        }
        for material in &edge_materials {
            if !ids.insert(material.id) {
                return Err(CatalogError::DuplicateId(material.id));
            }
        }
        Ok(Self { panel_materials, edge_materials })
    }

    /// The panel materials, in catalogue order.
    pub fn panel_materials(&self) -> &[PanelMaterial] {
        &self.panel_materials
    }

    /// The edge-banding materials, in catalogue order.
    pub fn edge_materials(&self) -> &[EdgeBandingMaterial] {
        &self.edge_materials
    }

    /// Looks up a panel material by id.
    pub fn panel_material(&self, id: MaterialId) -> Option<&PanelMaterial> {
        self.panel_materials.iter().find(|material| material.id == id)
    }

    /// Looks up an edge-banding material by id.
    pub fn edge_material(&self, id: MaterialId) -> Option<&EdgeBandingMaterial> {
        self.edge_materials.iter().find(|material| material.id == id)
    }
}

/// Why a catalogue or one of its records is not usable.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    /// A required text field is empty.
    #[error("material {material} has an empty {field}")]
    EmptyField {
        /// The material being validated.
        material: MaterialId,
        /// The rejected field.
        field: &'static str,
    },
    /// A dimension is zero or negative.
    #[error("material {material} {field} must be positive, got {value}")]
    NonPositive {
        /// The material being validated.
        material: MaterialId,
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
    },
    /// A dimension exceeds the supported maximum.
    #[error("material {material} {field} must not exceed {max}, got {value}")]
    TooLarge {
        /// The material being validated.
        material: MaterialId,
        /// The rejected dimension.
        field: &'static str,
        /// The rejected value.
        value: Length,
        /// The largest accepted value.
        max: Length,
    },
    /// Two materials use the same id.
    #[error("duplicate material id {0}")]
    DuplicateId(MaterialId),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelMaterialWire {
    id: MaterialId,
    name: String,
    kind: String,
    nominal_thickness: Length,
    color: String,
}

impl TryFrom<PanelMaterialWire> for PanelMaterial {
    type Error = CatalogError;

    fn try_from(wire: PanelMaterialWire) -> Result<Self, Self::Error> {
        Self::new(wire.id, wire.name, wire.kind, wire.nominal_thickness, wire.color)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeBandingMaterialWire {
    id: MaterialId,
    name: String,
    kind: String,
    thickness: Length,
    width: Length,
    color: String,
}

impl TryFrom<EdgeBandingMaterialWire> for EdgeBandingMaterial {
    type Error = CatalogError;

    fn try_from(wire: EdgeBandingMaterialWire) -> Result<Self, Self::Error> {
        Self::new(wire.id, wire.name, wire.kind, wire.thickness, wire.width, wire.color)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogWire {
    panel_materials: Vec<PanelMaterial>,
    edge_materials: Vec<EdgeBandingMaterial>,
}

impl TryFrom<CatalogWire> for Catalog {
    type Error = CatalogError;

    fn try_from(wire: CatalogWire) -> Result<Self, Self::Error> {
        Self::new(wire.panel_materials, wire.edge_materials)
    }
}

fn validate_text(id: MaterialId, field: &'static str, value: &str) -> Result<(), CatalogError> {
    if value.trim().is_empty() {
        return Err(CatalogError::EmptyField { material: id, field });
    }
    Ok(())
}

fn validate_dimension(id: MaterialId, field: &'static str, value: Length, max: Length) -> Result<(), CatalogError> {
    if !value.is_positive() {
        return Err(CatalogError::NonPositive { material: id, field, value });
    }
    if value > max {
        return Err(CatalogError::TooLarge { material: id, field, value, max });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> MaterialId {
        MaterialId::new(value).expect("test ids are non-zero")
    }

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn panel_material() -> PanelMaterial {
        PanelMaterial::new(id(1), "Chipboard 18 mm, oak", "chipboard", millimetres(18), "#b98a5a")
            .expect("valid panel material")
    }

    fn edge_material() -> EdgeBandingMaterial {
        EdgeBandingMaterial::new(id(101), "ABS 2 mm, oak", "abs", millimetres(2), millimetres(22), "#7a4f2a")
            .expect("valid edge material")
    }

    #[test]
    fn material_ids_reject_zero() {
        assert_eq!(MaterialId::new(0), None);
        assert!(serde_json::from_str::<MaterialId>("0").is_err());
        assert_eq!(serde_json::to_string(&id(7)).expect("serialize id"), "7");
    }

    #[test]
    fn catalogue_accepts_unique_materials_and_looks_them_up() {
        let catalog = Catalog::new(vec![panel_material()], vec![edge_material()]).expect("unique ids");
        assert_eq!(catalog.panel_material(id(1)).map(PanelMaterial::id), Some(id(1)));
        assert_eq!(catalog.edge_material(id(101)).map(EdgeBandingMaterial::id), Some(id(101)));
        assert!(catalog.panel_material(id(101)).is_none());
    }

    #[test]
    fn duplicate_ids_are_rejected_across_the_whole_catalogue() {
        let duplicate =
            PanelMaterial::new(id(1), "Second", "mdf", millimetres(18), "#fff").expect("valid panel material");
        assert_eq!(Catalog::new(vec![panel_material(), duplicate], Vec::new()), Err(CatalogError::DuplicateId(id(1))));

        let edge_duplicate = EdgeBandingMaterial::new(id(1), "Edge", "abs", millimetres(1), millimetres(20), "#000")
            .expect("valid edge material");
        assert_eq!(Catalog::new(vec![panel_material()], vec![edge_duplicate]), Err(CatalogError::DuplicateId(id(1))));
    }

    #[test]
    fn records_reject_empty_text_and_bad_dimensions() {
        assert_eq!(
            PanelMaterial::new(id(1), " ", "chipboard", millimetres(18), "#fff"),
            Err(CatalogError::EmptyField { material: id(1), field: "name" })
        );
        assert_eq!(
            PanelMaterial::new(id(1), "Board", "", millimetres(18), "#fff"),
            Err(CatalogError::EmptyField { material: id(1), field: "kind" })
        );
        assert_eq!(
            EdgeBandingMaterial::new(id(101), "Edge", "abs", Length::ZERO, millimetres(22), "#fff"),
            Err(CatalogError::NonPositive { material: id(101), field: "thickness", value: Length::ZERO })
        );
        let too_wide = Length::from_micrometres(PanelSize::MAX_THICKNESS.micrometres() + 1);
        assert_eq!(
            EdgeBandingMaterial::new(id(101), "Edge", "abs", millimetres(2), too_wide, "#fff"),
            Err(CatalogError::TooLarge {
                material: id(101),
                field: "width",
                value: too_wide,
                max: PanelSize::MAX_THICKNESS,
            })
        );
    }

    #[test]
    fn catalogue_round_trips_and_rejects_unknown_fields() {
        let catalog = Catalog::new(vec![panel_material()], vec![edge_material()]).expect("unique ids");
        let text = serde_json::to_string(&catalog).expect("serialize catalogue");
        let decoded: Catalog = serde_json::from_str(&text).expect("deserialize catalogue");
        assert_eq!(decoded, catalog);
        assert!(serde_json::from_str::<Catalog>(r#"{"panel_materials":[],"edge_materials":[],"extra":1}"#).is_err());
    }
}
