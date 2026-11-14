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
mod class;
mod container;
mod contour;
mod contour_edit;
mod contour_frames;
#[cfg(test)]
mod fixtures;
mod frame;
mod ids;
mod light;
mod mesh;
mod node;
mod object;
mod panel;
mod panel_features;
mod placement;
mod primitive;
mod project;
mod properties;
mod query;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tree;

pub use camera::{CameraError, CameraNode, CameraProjection};
pub use catalog::{Catalog, CatalogError, EdgeBandingMaterial, MaterialId, PanelMaterial};
pub use class::{
    CLASS_MAX_CODE, ClassCode, ClassCodeError, ClassPrefix, ClassRow, Extent, Fill, Level, Measure, Mount, Nesting,
    Opening, Operation, OperationSet, Supply, TraitSet, registry,
};
pub use container::{Container, Envelope, EnvelopeError};
pub use contour::{
    Contour, ContourError, ContourSegment, ContourSegmentEntry, ContourSegmentError, MAX_CONTOUR_COORDINATE,
    MAX_CONTOUR_SEGMENTS, MAX_POLYGON_SIDES, MIN_POLYGON_SIDES, Path, PathError, PathShape, joint_tolerance_mm,
};
pub use contour_edit::{ContourEditError, CornerMove, PathCorner};
pub use contour_frames::{ArcFrame, CircleFrame, PolygonFrame, RectangleFrame};
pub use frame::{Anchored, FrameAnchor, FrameError, Rebased, rebase_origin_at};
pub use ids::{ContourSegmentId, EdgeId, Id, MachiningId, Revision, VertexId};
pub use light::{LightError, LightKind, LightNode};
pub use mesh::{MAX_FACE_VERTICES, MAX_MESH_FACES, MAX_MESH_VERTICES, Mesh, MeshError};
pub use node::Node;
pub use object::{Constructive, ConstructiveError, Extrude, Feature, Geometry, MAX_OBJECT_FEATURES, Object};
pub use panel::{
    BakedOutline, BandingSpec, ContourPath, CoreViolation, CornerBanding, Cutout, CutoutDepth, CutoutShape,
    EdgeBanding, EdgeRef, EntryFace, FaceSide, MACHINING_DESCRIPTORS, Machining, MachiningAnchor,
    MachiningCapabilities, MachiningCategory, MachiningDescriptor, MachiningKind, MachiningParams, MachiningStage,
    Panel, PanelContourError, PanelDimensionError, PanelError, PanelGeometry, PanelGeometryError, PanelNominalSize,
    PanelOutline, PanelRect, PanelRectError, RING_TOLERANCE_MM, RectCorner, RectEdge, RingBanding, SegmentSource,
    classify_outline,
};
pub use panel_features::{CornerFeature, CornerFeatureKind, EdgeBow};
pub use placement::{EulerAngles, PivotLocal, Placement, PlacementError};
pub use primitive::{
    Cuboid, Cylinder, DEFAULT_BANDS, DEFAULT_SEGMENTS, MAX_BANDS, MAX_PRIMITIVE_SIZE, MAX_SEGMENTS, MIN_BANDS,
    MIN_PRIMITIVE_SIZE, MIN_SEGMENTS, Primitive, PrimitiveError, PrimitiveKind, Sphere,
};
pub use project::{
    MoveAxis, MoveLimit, MoveRule, Niche, NichePlane, NicheRefusal, NicheShape, Probe, ProbeFace, ProbeHit, Project,
    ProjectError, SurfaceKind,
};
pub use properties::{
    Properties, PropertyChange, PropertyError, PropertyValue, SYSTEM_CLASS, SYSTEM_COMPONENT, SYSTEM_PROTOTYPE_ID,
    SYSTEM_PROTOTYPE_PARAMETERS, SYSTEM_PROTOTYPE_VERSION,
};
pub use query::{NodeQuery, PropertyPredicate, StructuralKind};
