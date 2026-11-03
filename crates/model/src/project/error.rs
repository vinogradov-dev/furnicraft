//! Why a project cannot be loaded, constructed or mutated.
//!
//! Every failure is a readable error naming the node, panel or field that caused it. A hostile file
//! and a rejected command go through the same type, so the UI and the loader report the same reason.

use thiserror::Error;

use crate::*;

/// Why a project cannot be loaded, constructed or mutated.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProjectError {
    /// The file was written by a newer or otherwise unsupported schema.
    #[error("unsupported schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion {
        /// The version found in the document.
        found: u32,
        /// The version supported by this build.
        supported: u32,
    },
    /// Two nodes in the tree use the same id.
    #[error("duplicate document id {0}")]
    DuplicateId(Id),
    /// A geometry payload is invalid.
    #[error("invalid geometry: {0}")]
    InvalidGeometry(#[from] MeshError),
    /// A constructive contour or feature list is invalid.
    #[error("invalid constructive geometry: {0}")]
    InvalidConstructive(#[from] ConstructiveError),
    /// An authored camera could not be built.
    #[error(transparent)]
    Camera(#[from] CameraError),
    /// An authored light could not be built.
    #[error(transparent)]
    Light(#[from] LightError),
    /// The requested node is not an authored camera.
    #[error("camera {id} not found")]
    CameraNotFound {
        /// The missing camera id.
        id: Id,
    },
    /// The requested node is not an authored light.
    #[error("light {id} not found")]
    LightNotFound {
        /// The missing light id.
        id: Id,
    },
    /// The node tree is nested more deeply than the supported limit.
    #[error("container nesting exceeds {max} levels")]
    TooDeep {
        /// The maximum supported depth.
        max: u32,
    },
    /// The requested parent container is not in the tree.
    #[error("container {id} not found")]
    ContainerNotFound {
        /// The missing container id.
        id: Id,
    },
    /// The requested panel is not in the tree.
    #[error("panel {id} not found")]
    PanelNotFound {
        /// The missing panel id.
        id: Id,
    },
    /// The requested node is not in the tree, or it is the root container.
    #[error("node {id} not found below the root")]
    NodeNotFound {
        /// The missing node id.
        id: Id,
    },
    /// The root container cannot be deleted, moved, grouped, ungrouped or dissolved.
    #[error("root container {id} cannot be changed this way")]
    RootImmutable {
        /// The root container id.
        id: Id,
    },
    /// Ungrouping was requested for a node that is not an ordinary container.
    ///
    /// A panel is an atomic leaf; a component has its own identity and is
    /// converted to a group before `node.ungroup` will flatten it.
    #[error("node {id} is not an ordinary container and cannot be ungrouped")]
    NotAnOrdinaryContainer {
        /// The node that is not an ordinary container.
        id: Id,
    },
    /// Ungrouping was requested for a furniture component.
    #[error("node {id} is a furniture component and cannot be ungrouped; convert it to a group first")]
    NotAComponent {
        /// The component that was not flattened.
        id: Id,
    },
    /// A property batch contained no changes.
    #[error("no property changes were given")]
    NoPropertyChanges,
    /// The same node and property are listed twice in one batch.
    #[error("property `{key}` on node {node} is changed more than once")]
    DuplicateProperty {
        /// The node with the repeated change.
        node: Id,
        /// The repeated property key.
        key: String,
    },
    /// The requested property is not stored on the node.
    #[error("node {node} has no property `{key}`")]
    PropertyNotFound {
        /// The node that was asked.
        node: Id,
        /// The missing property key.
        key: String,
    },
    /// A property key or value violates the document property rules.
    #[error(transparent)]
    Property(#[from] PropertyError),
    /// Grouping needs nodes that share one parent container.
    #[error("nodes selected for grouping do not share one parent container")]
    NodesNotSiblings,
    /// The same node was listed more than once.
    #[error("node {id} is selected more than once")]
    DuplicateNode {
        /// The repeated node id.
        id: Id,
    },
    /// A placement batch contained no entries.
    #[error("no node placements were given")]
    NoPlacements,
    /// A vertex batch contained no entries.
    #[error("no mesh vertex moves were given")]
    NoVertexMoves,
    /// The same mesh vertex is moved more than once in one batch.
    #[error("vertex {vertex} of object {object} is moved more than once")]
    DuplicateVertex {
        /// The object whose vertex was repeated.
        object: Id,
        /// The repeated vertex index.
        vertex: u32,
    },
    /// The node is not a mesh object, so it has no editable vertices.
    #[error("node {id} is not a mesh object")]
    NotAMeshObject {
        /// The node that was asked.
        id: Id,
    },
    /// The node is not a constructive object, so it has no contour or feature history.
    #[error("node {id} is not a constructive object")]
    NotConstructiveObject {
        /// The node that was asked.
        id: Id,
    },
    /// The node is not a contour panel, so it has no outline ring to replace.
    #[error("panel {id} is not a contour panel")]
    PanelNotContour {
        /// The panel that was asked.
        id: Id,
    },
    /// A contour edit moves the panel frame beyond what the placement or pivot can represent.
    #[error("the frame of panel {panel} cannot follow its new outline")]
    PanelOutlineFrame {
        /// The panel whose frame could not move.
        panel: Id,
    },
    /// Grouping needs at least two nodes.
    #[error("grouping needs at least {minimum} nodes, got {found}")]
    TooFewNodes {
        /// The smallest accepted selection.
        minimum: u32,
        /// The number of nodes supplied.
        found: usize,
    },
    /// A placement could not be composed while a container was ungrouped.
    #[error("container {id} has a placement that cannot be composed with a child")]
    PlacementCompose {
        /// The container whose placement could not be applied.
        id: Id,
    },
    /// A stored panel dimension is not usable.
    #[error(transparent)]
    PanelDimension(#[from] PanelDimensionError),
    /// The panel cannot be resolved into core, overall and thickness.
    #[error(transparent)]
    PanelGeometry(#[from] PanelGeometryError),
    /// The panel's nominal contour cannot be built or grouped.
    #[error(transparent)]
    PanelContour(#[from] PanelContourError),
    /// A panel's stored fields or outline cannot be read as a panel.
    #[error(transparent)]
    Panel(#[from] PanelError),
    /// A document contour is not valid.
    #[error(transparent)]
    Contour(#[from] ContourError),
    /// A planar region cannot be built or combined.
    #[error(transparent)]
    Region(#[from] furnicraft_geom::RegionError),
    /// Edge banding refers to an edge outside the rectangular v1 outline.
    #[error("edge banding on panel {panel} uses unsupported edge {edge}")]
    UnsupportedEdgeBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The rejected edge.
        edge: EdgeId,
    },
    /// A tangent path ended up partially banded or with differing specs.
    #[error("panel {panel} has incompatible banding along one tangent path")]
    IncompatiblePathBanding {
        /// The panel whose path is inconsistent.
        panel: Id,
    },
    /// Two banding records cover the same edge.
    #[error("panel {panel} has duplicate edge banding on edge {edge}")]
    DuplicateEdgeBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The duplicated edge.
        edge: EdgeId,
    },
    /// The requested edge has no banding to clear.
    #[error("panel {panel} has no edge banding on edge {edge}")]
    EdgeBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The edge that was requested.
        edge: EdgeId,
    },
    /// The requested ring segment has no banding to clear.
    #[error("panel {panel} has no ring banding on segment {segment}")]
    RingBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The ring segment that was requested.
        segment: ContourSegmentId,
    },
    /// The requested corner has no treatment to clear.
    #[error("panel {panel} has no corner feature on corner {corner}")]
    CornerFeatureNotFound {
        /// The panel that owns the feature.
        panel: Id,
        /// The corner that was requested.
        corner: VertexId,
    },
    /// The requested corner has no banding to clear.
    #[error("panel {panel} has no corner banding on corner {corner}")]
    CornerBandingNotFound {
        /// The panel that owns the banding.
        panel: Id,
        /// The corner that was requested.
        corner: VertexId,
    },
    /// Corner banding uses a corner outside the rectangular outline.
    #[error("panel {panel} uses unsupported corner {corner}")]
    UnsupportedCornerBanding {
        /// The panel that owns the banding.
        panel: Id,
        /// The rejected corner.
        corner: VertexId,
    },
    /// The requested edge has no bow to clear.
    #[error("panel {panel} has no bow on edge {edge}")]
    EdgeBowNotFound {
        /// The panel that owns the bow.
        panel: Id,
        /// The edge that was requested.
        edge: EdgeId,
    },
    /// A through cutout does not reach the core, or a blind cutout is not fully inside it.
    #[error("cutout on panel {panel} lies outside the core")]
    CutoutOutsidePanel {
        /// The panel that owns the cutout.
        panel: Id,
    },
    /// A blind cutout depth is zero, negative or not shallower than the panel.
    #[error("blind cutout depth {depth} on panel {panel} must be positive and less than thickness {thickness}")]
    CutoutDepthInvalid {
        /// The panel that owns the cutout.
        panel: Id,
        /// The rejected depth.
        depth: Length,
        /// The current panel thickness.
        thickness: Length,
    },
    /// A blind cutout overlaps another cutout on the same panel.
    #[error("cutout on panel {panel} overlaps cutout {other}")]
    CutoutOverlap {
        /// The panel that owns the cutouts.
        panel: Id,
        /// The other cutout in the conflicting pair.
        other: Id,
    },
    /// The through cutouts would remove the whole core.
    #[error("cutouts on panel {panel} would remove the whole core")]
    CutoutRemovesCore {
        /// The panel that owns the cutouts.
        panel: Id,
    },
    /// The through cutouts would split the remaining core into disconnected pieces.
    #[error("cutout on panel {panel} would split the core into disconnected pieces")]
    CutoutSplitsCore {
        /// The panel that owns the cutouts.
        panel: Id,
    },
    /// A machining coordinate overflows while a legacy document is migrated.
    #[error("machining on panel {panel} overflows while migrating to the nominal frame")]
    MachiningCoordinateOverflow {
        /// The panel whose machining could not be translated.
        panel: Id,
    },
    /// A shaped panel cannot take machining until the contour boolean layer lands.
    #[error("shaped panel {panel} does not support machining yet")]
    ContourMachiningUnsupported {
        /// The shaped panel.
        panel: Id,
    },
    /// The next id would collide with an id already used by the tree.
    #[error("next document id {next} must be greater than the largest used id {max}")]
    NextIdNotAfterMax {
        /// The allocator position from the document.
        next: Id,
        /// The largest id found in the tree.
        max: u64,
    },
    /// Every document id has been used.
    #[error("document id space exhausted")]
    IdSpaceExhausted,
    /// Every document revision has been used.
    #[error("document revision space exhausted")]
    RevisionExhausted,
}
