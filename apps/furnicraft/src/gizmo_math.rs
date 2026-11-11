//! Shared math and presentation for the viewport.
//!
//! The translate and rotate gizmos project world points with the camera, hit-test their handles in
//! screen space and compute one placement per moved node; the shell dispatches them as a single
//! `node.placement.set_many`. The work plane turns a pointer into the world plane a drawing lands
//! on, and the snap functions turn that raw point into the point a drawing tool stores. Keeping the
//! axis colours, the projection, the work plane, the snapping and the commit types here means the
//! gizmos and the drawing tools cannot drift apart.

use std::f64::consts::FRAC_PI_2;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Shape, Stroke, Ui, Vec2};
use furnicraft_geom::{DMat4, DQuat, DVec2, DVec3};
use furnicraft_model::{Id, Placement, Project};
use furnicraft_scene::{Camera, MAX_PITCH, MIN_PITCH, Projection, Ray};
use furnicraft_units::Length;

/// Length of an axis handle, in points.
pub(crate) const HANDLE_LENGTH: f32 = 84.0;

/// Radius inside which a pointer hits a handle, in points.
pub(crate) const HIT_RADIUS: f32 = 8.0;

/// Part of a handle next to the origin that never starts a drag, in points.
const ORIGIN_DEAD_ZONE: f32 = 10.0;

/// Axes closer than this to the view direction are not drawn.
const VIEW_ALIGNMENT_LIMIT: f64 = 0.985;

/// The work-plane grid step, in millimetres.
pub(crate) const SNAP_GRID_MM: f64 = 10.0;

/// How close the orbit angles must be to a standard view, in radians.
///
/// A snapped view is exact; any orbit moves the camera far outside this tolerance and becomes an
/// axonometric view. A top or bottom view sits exactly at its pole (ADR 0057), so its pitch matches
/// the limit within this tolerance.
const STANDARD_VIEW_TOLERANCE: f64 = 1.0e-9;

/// Gamma-space colours for the three axes.
const X_COLOR: Color32 = Color32::from_rgb(232, 80, 80);
const Y_COLOR: Color32 = Color32::from_rgb(92, 204, 116);
const Z_COLOR: Color32 = Color32::from_rgb(92, 140, 255);

/// Colour of the handle under the pointer or being dragged.
pub(crate) const ACTIVE_COLOR: Color32 = Color32::from_rgb(255, 220, 96);

/// One local or world axis of a gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// All three axes in display order.
    pub(crate) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The unit vector of this axis in the frame the gizmo draws it in.
    pub(crate) fn direction(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }

    /// The one-letter label shown next to a handle.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    /// The gamma-space colour of this axis.
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::X => X_COLOR,
            Self::Y => Y_COLOR,
            Self::Z => Z_COLOR,
        }
    }
}

/// What a gizmo points at in one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Target<'a> {
    /// The camera that projects the handles.
    pub(crate) camera: Camera,
    /// The document whose nodes are being placed.
    pub(crate) project: &'a Project,
    /// The selected nodes, oldest first; the last movable one anchors the gizmo.
    pub(crate) selection: &'a [Id],
}

impl Target<'_> {
    /// The movable nodes the active gizmo acts on, in selection order.
    pub(crate) fn move_targets(&self) -> Vec<MoveTarget> {
        move_targets(self.project, self.selection)
    }

    /// The node the gizmo sits on: the last movable node in selection order.
    ///
    /// Choosing the last one keeps the gizmo on the primary selection whenever that node is
    /// movable; when the primary is the root or covered by a selected ancestor, the gizmo falls
    /// back to the ancestor that will actually move.
    pub(crate) fn anchor(&self) -> Option<Id> {
        self.move_targets().last().map(|target| target.node)
    }

    /// The world pivot of the node the gizmo sits on.
    pub(crate) fn anchor_pivot(&self) -> Option<DVec3> {
        self.project.pivot_world(self.anchor()?)
    }

    /// Every drawable that moves with the selection, in target order.
    pub(crate) fn drawables(&self) -> Vec<Id> {
        let mut drawables = Vec::new();
        for target in self.move_targets() {
            drawables.extend(self.project.geometry_ids_under(target.node).unwrap_or_default());
        }
        drawables
    }
}

/// One node a gizmo drag moves, with the frame it started in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveTarget {
    /// The node whose stored placement changes.
    pub(crate) node: Id,
    /// Its placement relative to the parent at the start of the drag.
    pub(crate) start_local: Placement,
    /// The parent's world placement at the start of the drag.
    pub(crate) parent: Placement,
    /// The node's stored pivot in its own frame.
    pub(crate) pivot_local: DVec3,
}

/// Movable nodes of a selection: the top-most ones, without the root and without the unknown.
///
/// A container and one of its descendants can both be selected; only the container moves, so the
/// descendant is not shifted twice. Order follows the selection, so the last entry is the primary.
pub(crate) fn move_targets(project: &Project, selection: &[Id]) -> Vec<MoveTarget> {
    let root = project.root().id();
    project
        .top_most_nodes(selection)
        .into_iter()
        .filter(|node| *node != root)
        .filter_map(|node| {
            let parent = project.parent_of(node)?;
            Some(MoveTarget {
                node,
                start_local: project.node(node)?.placement(),
                parent: project.world_placement(parent)?,
                pivot_local: project.pivot_local_mm(node)?,
            })
        })
        .collect()
}

/// One node's new placement, computed by a gizmo and dispatched as part of one command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacementChange {
    /// The node whose placement changed.
    pub(crate) node: Id,
    /// The new placement relative to its parent.
    pub(crate) placement: Placement,
}

/// The placements a finished drag asks the shell to dispatch.
///
/// A drag on a multi-selection produces one commit with one change per moved node, so the shell
/// dispatches a single `node.placement.set_many` and the whole drag is one undo entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacementCommit {
    /// The changed nodes; never empty.
    pub(crate) changes: Vec<PlacementChange>,
}

/// Converts a world-space shift into one placement per target, or `None` when nothing moved.
///
/// The shift is converted into each node's parent frame, so a selection whose nodes have different
/// parents still moves rigidly. Every translation is rounded to whole micrometres, which is the
/// precision the inspector shows and the document keeps.
pub(crate) fn commit_translation(
    targets: &[MoveTarget],
    delta_world: DVec3,
) -> Result<Option<PlacementCommit>, String> {
    if delta_world == DVec3::ZERO {
        return Ok(None);
    }
    if !delta_world.is_finite() {
        return Err("the drag produced a non-finite translation".to_owned());
    }
    let mut changes = Vec::new();
    for target in targets {
        let delta_parent = target.parent.rotation().inverse() * delta_world;
        let translation = target.start_local.translation() + delta_parent;
        if !translation.is_finite() {
            return Err("the drag moved the node to a non-finite position".to_owned());
        }
        let placement =
            Placement::new(translation, target.start_local.rotation()).map_err(|error| error.to_string())?;
        let placement = quantised(placement)?;
        if placement != target.start_local {
            changes.push(PlacementChange { node: target.node, placement });
        }
    }
    Ok(unchanged_or(changes))
}

/// Converts a world-space turn about `pivot_world` into one placement per target.
///
/// The anchor is the last target: its stored pivot defines the centre exactly, and every other
/// target is turned about the same world point. A node whose translation already sits on the centre
/// keeps its translation bit-for-bit; every other translation is rounded to whole micrometres.
pub(crate) fn commit_rotation(
    targets: &[MoveTarget],
    pivot_world: DVec3,
    rotation_world: DQuat,
) -> Result<Option<PlacementCommit>, String> {
    if !rotation_world.is_finite() || (rotation_world.length() - 1.0).abs() > 1.0e-9 {
        return Err("the drag produced a non-finite rotation".to_owned());
    }
    let anchor = targets.last().map(|target| target.node);
    let mut changes = Vec::new();
    for target in targets {
        let parent_rotation = target.parent.rotation();
        let delta_parent = (parent_rotation.inverse() * rotation_world * parent_rotation).normalize();
        if !delta_parent.is_finite() {
            return Err("the drag produced a non-finite rotation".to_owned());
        }
        let centre = if Some(target.node) == anchor {
            target.start_local.transform_point(target.pivot_local)
        } else {
            parent_rotation.inverse() * (pivot_world - target.parent.translation())
        };
        if !centre.is_finite() {
            return Err("the drag produced a non-finite rotation centre".to_owned());
        }
        let rotation = (delta_parent * target.start_local.rotation()).normalize();
        if !rotation.is_finite() {
            return Err("the drag produced a non-finite rotation".to_owned());
        }
        let start_translation = target.start_local.translation();
        let placement = if centre == start_translation {
            // A turn about the node's own translation keeps it exactly.
            Placement::new(start_translation, rotation).map_err(|error| error.to_string())?
        } else {
            let translation = centre + delta_parent * (start_translation - centre);
            if !translation.is_finite() {
                return Err("the drag moved the node to a non-finite position".to_owned());
            }
            quantised(Placement::new(translation, rotation).map_err(|error| error.to_string())?)?
        };
        if !placement.same_transform(target.start_local) {
            changes.push(PlacementChange { node: target.node, placement });
        }
    }
    Ok(unchanged_or(changes))
}

/// Packs the changed nodes into a commit, or `None` when a drag ended where it started.
fn unchanged_or(changes: Vec<PlacementChange>) -> Option<PlacementCommit> {
    if changes.is_empty() { None } else { Some(PlacementCommit { changes }) }
}

/// Rounds a placement's translation to whole micrometres at the editing boundary.
fn quantised(placement: Placement) -> Result<Placement, String> {
    let [x, y, z] = placement
        .translation_lengths()
        .ok_or_else(|| "the drag moved the node outside the editable millimetre range".to_owned())?;
    placement.with_translation(x, y, z).map_err(|error| error.to_string())
}

/// The scene change a gizmo drag is previewing, without touching the document.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Preview<'a> {
    /// Shift the listed drawables by a world-space delta.
    Translation {
        /// Drawables to move.
        drawables: &'a [Id],
        /// World-space shift.
        delta: DVec3,
    },
    /// Turn the listed drawables about a world-space pivot.
    Rotation {
        /// Drawables to turn.
        drawables: &'a [Id],
        /// World-space point the turn is centred on.
        pivot: DVec3,
        /// World-space turn.
        rotation: DQuat,
    },
}

impl Preview<'_> {
    /// The drawable ids the preview moves.
    pub(crate) fn drawables(&self) -> &[Id] {
        match self {
            Self::Translation { drawables, .. } | Self::Rotation { drawables, .. } => drawables,
        }
    }

    /// The world placement a node would have if the preview were committed.
    ///
    /// A camera or a light has no drawable, so the scene frame cannot preview it through its
    /// geometry. The same rigid transform that moves the drawables is applied to the node's world
    /// placement instead: a translation shifts it and a rotation turns it about the drag pivot.
    /// Returns `None` when the result is not a valid placement.
    pub(crate) fn moved_world(&self, world: Placement) -> Option<Placement> {
        match self {
            Self::Translation { delta, .. } => Placement::new(world.translation() + delta, world.rotation()).ok(),
            Self::Rotation { pivot, rotation, .. } => {
                let translation = *pivot + *rotation * (world.translation() - *pivot);
                Placement::new(translation, (*rotation * world.rotation()).normalize()).ok()
            }
        }
    }
}

/// Bit-exact identity of a preview, used to skip an unchanged re-upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewKey {
    /// A translation preview: node and delta bits.
    Translation {
        /// The node being dragged.
        node: Id,
        /// `x`, `y`, `z` of the delta as raw bits.
        delta: [u64; 3],
    },
    /// A rotation preview: node and turn bits.
    Rotation {
        /// The node being dragged.
        node: Id,
        /// The accumulated local turn as raw bits.
        angle: u64,
    },
    /// A vertex-move preview: the edited object and the local offset in micrometres.
    Vertex {
        /// The object whose vertices move.
        object: Id,
        /// The quantised local offset per axis.
        offset: [i64; 3],
    },
}

/// The world point under a viewport position.
pub(crate) fn ray_to(pointer: Pos2, viewport: egui::Rect, camera: Camera) -> Ray {
    let (x, y) = normalized_pointer(pointer, viewport);
    camera.ray_ndc(x, y, aspect(viewport))
}

/// A world point in viewport points, or `None` when it is behind the camera.
pub(crate) fn screen_point(camera: Camera, viewport: egui::Rect, point: DVec3) -> Option<Pos2> {
    let ndc = camera.project_ndc(point, aspect(viewport))?;
    let x = viewport.left() + (ndc.x as f32 + 1.0) * 0.5 * viewport.width();
    let y = viewport.top() + (1.0 - ndc.y as f32) * 0.5 * viewport.height();
    (x.is_finite() && y.is_finite()).then_some(Pos2::new(x, y))
}

/// Rounds a millimetre coordinate to a whole micrometre, or `None` when it is not representable.
pub(crate) fn length_from_millimetres(value: f64) -> Option<Length> {
    let micrometres = value * 1_000.0;
    if !micrometres.is_finite() || micrometres < i64::MIN as f64 || micrometres > i64::MAX as f64 {
        return None;
    }
    Some(Length::from_micrometres(micrometres.round() as i64))
}

/// Distance from `point` to a screen-space segment.
pub(crate) fn distance_to_segment(point: Pos2, start: Pos2, end: Pos2) -> Option<f32> {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let along = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    Some((point - (start + segment * along)).length())
}

/// The point where a ray meets a plane, or `None` when they are parallel or the plane is behind.
pub(crate) fn ray_plane(ray: Ray, plane_point: DVec3, normal: DVec3) -> Option<DVec3> {
    let denominator = ray.direction().dot(normal);
    if denominator.abs() < 1.0e-9 {
        return None;
    }
    let distance = (plane_point - ray.origin()).dot(normal) / denominator;
    if !distance.is_finite() || distance <= 0.0 {
        return None;
    }
    Some(ray.origin() + ray.direction() * distance)
}

/// A right-handed world plane the drawing tools place points on.
///
/// The plane passes through the world origin, so a drawing is stored in world-aligned plane
/// coordinates. `u` and `v` follow the screen of the view that selected the plane: a shape drawn to
/// the right and up lands on positive `u` and `v` in both the viewport and the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorkPlane {
    normal: DVec3,
    u: DVec3,
    v: DVec3,
}

impl WorkPlane {
    /// World XY: the plane a perspective or axonometric view draws on.
    pub(crate) const WORLD_XY: Self = Self { normal: DVec3::Z, u: DVec3::X, v: DVec3::Y };

    /// The plane an axis-aligned orthographic view draws on, or world XY for every other view.
    ///
    /// A top or bottom view is recognised by its pitch at a pole; a side view needs an exact
    /// quarter-turn yaw, because any other yaw is an axonometric view and draws on world XY.
    pub(crate) fn from_camera(camera: Camera) -> Self {
        if !matches!(camera.projection(), Projection::Orthographic { .. }) {
            return Self::WORLD_XY;
        }
        let pitch = camera.pitch();
        if (pitch - MAX_PITCH).abs() <= STANDARD_VIEW_TOLERANCE {
            return Self { normal: DVec3::Z, u: DVec3::X, v: DVec3::Y };
        }
        if (pitch - MIN_PITCH).abs() <= STANDARD_VIEW_TOLERANCE {
            return Self { normal: -DVec3::Z, u: DVec3::X, v: -DVec3::Y };
        }
        if pitch.abs() > STANDARD_VIEW_TOLERANCE {
            return Self::WORLD_XY;
        }
        let quarter = camera.yaw() / FRAC_PI_2;
        let nearest = quarter.round();
        if (quarter - nearest).abs() > STANDARD_VIEW_TOLERANCE {
            return Self::WORLD_XY;
        }
        match (nearest as i64).rem_euclid(4) {
            0 => Self { normal: DVec3::X, u: DVec3::Y, v: DVec3::Z },
            1 => Self { normal: DVec3::Y, u: -DVec3::X, v: DVec3::Z },
            2 => Self { normal: -DVec3::X, u: -DVec3::Y, v: DVec3::Z },
            _ => Self { normal: -DVec3::Y, u: DVec3::X, v: DVec3::Z },
        }
    }

    /// The plane normal, pointing from the plane towards the viewer in a standard view.
    pub(crate) const fn normal(self) -> DVec3 {
        self.normal
    }

    /// The world point at plane coordinates `local`.
    #[allow(dead_code, reason = "the commit and snap steps place local points in U13-U17")]
    pub(crate) fn world(self, local: DVec2) -> DVec3 {
        self.u * local.x + self.v * local.y
    }

    /// The plane coordinates of a world point on the plane.
    pub(crate) fn local(self, world: DVec3) -> DVec2 {
        DVec2::new(world.dot(self.u), world.dot(self.v))
    }

    /// A placement whose local XY is this plane at the world origin.
    ///
    /// Local X maps to `u`, local Y to `v` and local Z to the normal, so a path stored in plane
    /// coordinates keeps its orientation in the document. The translation is zero because the plane
    /// passes through the world origin; a later origin-carrying plane adds it here only.
    pub(crate) fn placement(self) -> Option<Placement> {
        let basis = DMat4::from_cols(
            self.u.extend(0.0),
            self.v.extend(0.0),
            self.normal().extend(0.0),
            DVec3::ZERO.extend(1.0),
        );
        let rotation = DQuat::from_mat4(&basis).normalize();
        Placement::new(DVec3::ZERO, rotation).ok()
    }

    /// A placement whose local origin sits at plane coordinates `local`.
    ///
    /// The rotation is the plane's own, so the object's local XY lies on the work plane while its
    /// origin is the clicked point.
    #[allow(dead_code, reason = "the primitive placement consumes it in the shell in U21")]
    pub(crate) fn placement_at(self, local: DVec2) -> Option<Placement> {
        let rotation = self.placement()?.rotation();
        Placement::new(self.world(local), rotation).ok()
    }

    /// The world point where a ray meets the plane, or `None` when it is parallel or behind.
    #[allow(dead_code, reason = "the drawing tools start from these points in U13-U14")]
    pub(crate) fn ray_point(self, ray: Ray) -> Option<DVec3> {
        ray_plane(ray, DVec3::ZERO, self.normal())
    }

    /// The plane coordinates under a viewport position.
    #[allow(dead_code, reason = "the drawing tools consume the mapping in U13-U14")]
    pub(crate) fn pointer_point(self, camera: Camera, viewport: egui::Rect, pointer: Pos2) -> Option<DVec2> {
        self.ray_point(ray_to(pointer, viewport, camera)).map(|world| self.local(world))
    }

    /// The plane name shown in the strip: `XY`, `XZ` or `YZ`.
    pub(crate) fn label(self) -> &'static str {
        let normal = self.normal();
        if normal.x.abs() > 0.5 {
            "YZ"
        } else if normal.y.abs() > 0.5 {
            "XZ"
        } else {
            "XY"
        }
    }
}

/// How a drawing tool constrains the coordinates the pointer produces.
///
/// This is the grid switch, not a switch for every snap: the ends of the draft and the axis lock
/// stay available in both modes, because they answer a different question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "the drawing tool sets the mode in U14; both variants are already matched here")]
pub(crate) enum SnapMode {
    /// The point is not rounded to the grid.
    Free,
    /// The point lands on the [`SNAP_GRID_MM`] work-plane grid.
    Grid,
}

/// What captured the pointer, so the viewport can mark it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapTarget {
    /// Nothing: the raw pointer point.
    Free,
    /// A point already in the draft.
    End,
    /// The 10 mm work-plane grid.
    Grid,
    /// The u or v axis through the previous point.
    Axis,
}

/// A plane point and what it snapped to, in work-plane millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SnappedPoint {
    /// The point a drawing tool stores.
    pub(crate) point: DVec2,
    /// Why the point moved, or [`SnapTarget::Free`] when it did not.
    pub(crate) target: SnapTarget,
}

/// The raw pointer point and the scene facts that may capture it.
///
/// Every coordinate is in work-plane millimetres. The capture radius arrives from the caller
/// because only the camera knows how many millimetres one viewport point covers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SnapRequest<'a> {
    /// The pointer's work-plane point.
    pub(crate) point: DVec2,
    /// The previous draft point; without one the axis lock has nothing to lock to.
    pub(crate) anchor: Option<DVec2>,
    /// Points already in the draft that the pointer may join.
    pub(crate) ends: &'a [DVec2],
    /// Grid snapping.
    pub(crate) mode: SnapMode,
    /// Whether the point locks to the u or v axis through the anchor.
    pub(crate) axis_lock: bool,
    /// Capture radius around a draft end, in plane millimetres.
    pub(crate) end_radius: f64,
    /// Alt was held: skip the end and grid snaps and take the pointer as it is.
    ///
    /// The axis lock still applies, because Shift is an explicit request for a straight segment
    /// rather than an implicit snap that Alt should suppress.
    pub(crate) ignore_snaps: bool,
}

/// Which plane axis an axis-locked point follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapAxis {
    /// The u axis through the anchor: the anchor's v is kept.
    U,
    /// The v axis through the anchor: the anchor's u is kept.
    V,
}

/// Rounds both plane coordinates to the nearest grid multiple.
///
/// A non-finite coordinate is returned unchanged rather than turned into a grid value.
pub(crate) fn snap_grid(point: DVec2) -> DVec2 {
    DVec2::new(round_to_grid(point.x), round_to_grid(point.y))
}

fn round_to_grid(value: f64) -> f64 {
    let rounded = (value / SNAP_GRID_MM).round() * SNAP_GRID_MM;
    if rounded.is_finite() { rounded } else { value }
}

/// The nearest draft end inside `radius`, with the earlier one winning a tie.
///
/// A radius that is not finite and positive captures nothing, so a broken scale or a collapsed
/// viewport disables the snap instead of making every point jump.
pub(crate) fn nearest_end(point: DVec2, ends: &[DVec2], radius: f64) -> Option<DVec2> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let mut nearest: Option<(f64, DVec2)> = None;
    for end in ends.iter().copied().filter(|end| end.is_finite()) {
        let distance = (end - point).length();
        if distance <= radius && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, end));
        }
    }
    nearest.map(|(_, end)| end)
}

/// Projects a point onto the u or v axis through `anchor`, whichever offset is larger.
///
/// The offsets are compared by magnitude; a tie locks to u, so the result never depends on the
/// order of the components.
pub(crate) fn snap_axis(point: DVec2, anchor: DVec2) -> (DVec2, SnapAxis) {
    let offset = point - anchor;
    if offset.x.abs() >= offset.y.abs() {
        (DVec2::new(point.x, anchor.y), SnapAxis::U)
    } else {
        (DVec2::new(anchor.x, point.y), SnapAxis::V)
    }
}

/// Snaps a raw work-plane point for one drawing tool.
///
/// The constraints apply in a fixed order. A draft end inside `end_radius` wins: joining a chain
/// must land exactly on the joint. Otherwise an axis lock projects the point onto the nearer axis
/// through the anchor, and a grid mode rounds the free coordinate while the locked one stays on
/// the anchor. A free pointer in grid mode rounds both coordinates.
#[allow(dead_code, reason = "the drawing tools send every plane point through it in U14-U16")]
pub(crate) fn snap_point(request: SnapRequest<'_>) -> SnappedPoint {
    if !request.ignore_snaps
        && let Some(end) = nearest_end(request.point, request.ends, request.end_radius)
    {
        return SnappedPoint { point: end, target: SnapTarget::End };
    }
    let lock = match (request.axis_lock, request.anchor) {
        (true, Some(anchor)) => Some(snap_axis(request.point, anchor)),
        _ => None,
    };
    let mode = if request.ignore_snaps { SnapMode::Free } else { request.mode };
    match (mode, lock) {
        (SnapMode::Grid, Some((point, axis))) => {
            SnappedPoint { point: round_free_coordinate(point, axis), target: SnapTarget::Axis }
        }
        (SnapMode::Grid, None) => SnappedPoint { point: snap_grid(request.point), target: SnapTarget::Grid },
        (SnapMode::Free, Some((point, _))) => SnappedPoint { point, target: SnapTarget::Axis },
        (SnapMode::Free, None) => SnappedPoint { point: request.point, target: SnapTarget::Free },
    }
}

/// Rounds the coordinate that is still free, keeping the locked one exact.
fn round_free_coordinate(point: DVec2, axis: SnapAxis) -> DVec2 {
    match axis {
        SnapAxis::U => DVec2::new(round_to_grid(point.x), point.y),
        SnapAxis::V => DVec2::new(point.x, round_to_grid(point.y)),
    }
}

fn normalized_pointer(pointer: Pos2, viewport: egui::Rect) -> (f64, f64) {
    let width = f64::from(viewport.width());
    let height = f64::from(viewport.height());
    if width <= 0.0 || height <= 0.0 {
        return (0.0, 0.0);
    }
    let x = (f64::from(pointer.x - viewport.left()) / width) * 2.0 - 1.0;
    let y = 1.0 - (f64::from(pointer.y - viewport.top()) / height) * 2.0;
    (x, y)
}

fn aspect(viewport: egui::Rect) -> f64 {
    let width = f64::from(viewport.width());
    let height = f64::from(viewport.height());
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 { width / height } else { 1.0 }
}

/// A unit screen direction, or `None` when the projected segment is too short to be stable.
pub(crate) fn screen_direction(delta: Vec2) -> Option<Vec2> {
    (delta.length() >= 0.5).then(|| delta.normalized())
}

/// One projected axis handle: the axis, its screen-space tip and its unit screen direction.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisHandle {
    /// The axis this handle moves along.
    pub(crate) axis: Axis,
    /// The tip in viewport points.
    pub(crate) tip: Pos2,
    /// The unit direction from the origin towards the tip.
    pub(crate) direction: Vec2,
}

/// The projected handles of a gizmo at `origin_world`, skipping axes that point at the camera.
pub(crate) fn axis_handles(camera: Camera, viewport: egui::Rect, origin_world: DVec3, origin: Pos2) -> Vec<AxisHandle> {
    let step = (camera.distance() * 0.01).max(1.0);
    let mut handles = Vec::with_capacity(Axis::ALL.len());
    for axis in Axis::ALL {
        if axis.direction().dot(camera.forward()).abs() > VIEW_ALIGNMENT_LIMIT {
            continue;
        }
        let Some(end) = screen_point(camera, viewport, origin_world + axis.direction() * step) else {
            continue;
        };
        let Some(direction) = screen_direction(end - origin) else {
            continue;
        };
        handles.push(AxisHandle { axis, tip: origin + direction * HANDLE_LENGTH, direction });
    }
    handles
}

/// The axis whose handle is nearest to `pointer`, ignoring the dead zone around the origin.
pub(crate) fn hit_axis(camera: Camera, viewport: egui::Rect, origin_world: DVec3, pointer: Pos2) -> Option<Axis> {
    let origin = screen_point(camera, viewport, origin_world)?;
    if !viewport.expand(64.0).contains(origin) {
        return None;
    }
    let mut best: Option<(f32, Axis)> = None;
    for handle in axis_handles(camera, viewport, origin_world, origin) {
        let distance = distance_to_handle(pointer, origin, handle.tip);
        let Some(distance) = distance else {
            continue;
        };
        if distance <= HIT_RADIUS && best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, handle.axis));
        }
    }
    best.map(|(_, axis)| axis)
}

/// Distance from `point` to the segment, or `None` inside the dead zone next to the origin.
fn distance_to_handle(point: Pos2, start: Pos2, end: Pos2) -> Option<f32> {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let length = length_squared.sqrt();
    let along = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    if along * length < ORIGIN_DEAD_ZONE {
        return None;
    }
    Some((point - (start + segment * along)).length())
}

/// Draws three axis handles at a world point.
///
/// `hover` is the axis under the pointer and `active` the axis being dragged. While a drag is
/// active the other axes fade, so the user can see which one owns the pointer.
pub(crate) fn paint_axis_handles(
    ui: &Ui,
    viewport: egui::Rect,
    camera: Camera,
    origin_world: DVec3,
    hover: Option<Axis>,
    active: Option<Axis>,
) {
    let Some(origin) = screen_point(camera, viewport, origin_world) else {
        return;
    };
    if !viewport.expand(64.0).contains(origin) {
        return;
    }
    let handles = axis_handles(camera, viewport, origin_world, origin);
    let painter = ui.painter().with_clip_rect(viewport);
    let center_tone = if active.is_some() { Color32::from_gray(140) } else { Color32::from_gray(220) };
    painter.circle_filled(origin, 3.0, center_tone);

    for handle in handles {
        let is_active = active == Some(handle.axis);
        let hovered = hover == Some(handle.axis);
        let mut color = if is_active || hovered { ACTIVE_COLOR } else { handle.axis.color() };
        if active.is_some() && !is_active {
            color = color.gamma_multiply(0.4);
        }
        let width = if is_active || hovered { 3.5 } else { 2.2 };
        painter.line_segment([origin, handle.tip], Stroke::new(width, color));

        let perpendicular = Vec2::new(-handle.direction.y, handle.direction.x);
        let base = handle.tip - handle.direction * 9.0;
        painter.add(Shape::convex_polygon(
            vec![handle.tip, base + perpendicular * 4.0, base - perpendicular * 4.0],
            color,
            Stroke::NONE,
        ));
        let label = handle.tip + handle.direction * 12.0;
        painter.text(
            label,
            Align2::CENTER_CENTER,
            handle.axis.label(),
            FontId::proportional(12.0),
            if is_active || hovered { ACTIVE_COLOR } else { Color32::from_gray(230) },
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{CameraProjection, Container, LightKind, Panel, Project};
    use furnicraft_testkit::{sample_panel_nominal_size, sample_panel_thickness, sample_project};

    use super::*;

    /// A project with a turned container that holds a panel, next to a loose panel.
    ///
    /// Root (1) ├ Cabinet (2), turned a quarter about Z, at x = 100 ─ Inner panel (4)
    ///          └ Outer panel (3)
    fn nested_project() -> Project {
        let size = sample_panel_nominal_size();
        let thickness = sample_panel_thickness();
        let panel = |id| {
            Panel::new(Id::new(id).expect("id"), "Panel", size, None, thickness, Placement::IDENTITY).expect("panel")
        };
        let cabinet = Container::new(
            Id::new(2).expect("id"),
            "Cabinet",
            Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2))
                .expect("placement"),
            vec![panel(4).into()],
        );
        let root =
            Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![cabinet.into(), panel(3).into()]);
        Project::new(root).expect("project")
    }

    fn id(number: u64) -> Id {
        Id::new(number).expect("id")
    }

    fn assert_vector(actual: DVec3, expected: DVec3, what: &str) {
        assert!((actual - expected).length() < 1.0e-9, "{what}: actual {actual:?}, expected {expected:?}");
    }

    /// An orthographic camera snapped to the standard view that looks from `eye` towards the target.
    fn orthographic_view(eye: DVec3) -> Camera {
        let mut camera = Camera::default();
        camera.look_from(eye);
        camera.toggle_projection();
        camera
    }

    fn target<'a>(project: &'a Project, selection: &'a [Id]) -> Target<'a> {
        Target { camera: Camera::default(), project, selection }
    }

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    fn origin_screen(camera: Camera) -> Pos2 {
        screen_point(camera, viewport(), camera.target()).expect("the target is in front")
    }

    #[test]
    fn the_default_view_draws_three_distinct_handles() {
        let camera = Camera::default();
        let handles = axis_handles(camera, viewport(), camera.target(), origin_screen(camera));
        assert_eq!(handles.len(), 3);
        for handle in &handles {
            assert!(handle.tip.is_finite());
            assert!((handle.direction.length() - 1.0).abs() < 1.0e-5);
            assert!((handle.tip - origin_screen(camera)).length() > 80.0);
        }
        let z = handles.iter().find(|handle| handle.axis == Axis::Z).expect("Z handle");
        assert!(z.direction.y < -0.5, "Z should point up, direction was {:?}", z.direction);
    }

    #[test]
    fn a_view_aligned_axis_has_no_handle() {
        let mut camera = Camera::default();
        camera.look_from(DVec3::Z);
        let handles = axis_handles(camera, viewport(), camera.target(), origin_screen(camera));
        assert!(handles.iter().all(|handle| handle.axis != Axis::Z), "handles were {handles:?}");
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn the_handle_nearest_the_pointer_wins_and_the_origin_is_dead() {
        let camera = Camera::default();
        let origin_world = camera.target();
        let origin = origin_screen(camera);
        let handles = axis_handles(camera, viewport(), origin_world, origin);
        let x = handles.iter().find(|handle| handle.axis == Axis::X).expect("X handle");
        let pointer = origin + (x.tip - origin) * 0.6;
        assert_eq!(hit_axis(camera, viewport(), origin_world, pointer), Some(Axis::X));
        assert_eq!(hit_axis(camera, viewport(), origin_world, origin), None);
        assert_eq!(hit_axis(camera, viewport(), origin_world, origin + Vec2::new(300.0, 300.0)), None);
    }

    #[test]
    fn a_selection_moves_only_its_top_most_nodes() {
        let project = nested_project();
        let (cabinet, outer, inner) = (id(2), id(3), id(4));

        let targets = move_targets(&project, &[cabinet, inner, outer]);
        assert_eq!(targets.len(), 2, "the inner panel is covered by its cabinet");
        assert_eq!(targets[0].node, cabinet);
        assert_eq!(targets[1].node, outer);

        assert_eq!(move_targets(&project, &[inner]).len(), 1);
        assert!(move_targets(&project, &[project.root().id()]).is_empty(), "the root has no placement to edit");
        assert!(move_targets(&project, &[]).is_empty());
        assert!(move_targets(&project, &[id(99)]).is_empty());
    }

    #[test]
    fn a_selected_camera_and_light_are_move_targets() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("camera");
        let light = project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("light");

        let targets = move_targets(&project, &[camera, light]);
        assert_eq!(
            targets.iter().map(|target| target.node).collect::<Vec<_>>(),
            vec![camera, light],
            "a camera and a light carry a placement, so the gizmo moves them"
        );
        assert_eq!(target(&project, &[camera]).anchor(), Some(camera));
        assert!(
            target(&project, &[camera]).drawables().is_empty(),
            "an icon stands for itself and has no geometry to preview"
        );
    }

    #[test]
    fn a_preview_moves_a_camera_or_light_world_placement_like_a_drawable() {
        let turn = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let start = Placement::new(DVec3::new(100.0, 200.0, 300.0), turn).expect("placement");

        let move_delta = DVec3::new(10.0, -20.0, 5.0);
        let moved = Preview::Translation { drawables: &[], delta: move_delta }
            .moved_world(start)
            .expect("a translated placement");
        assert_eq!(moved.translation(), DVec3::new(110.0, 180.0, 305.0));
        assert_eq!(moved.rotation(), turn, "a translation keeps the rotation");

        let pivot = DVec3::new(50.0, 0.0, 0.0);
        let z_turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let turned = Preview::Rotation { drawables: &[], pivot, rotation: z_turn }
            .moved_world(start)
            .expect("a turned placement");
        let expected = pivot + z_turn * (start.translation() - pivot);
        assert!((turned.translation() - expected).length() < 1.0e-9);
        assert!(
            turned.rotation().dot(z_turn * turn).abs() > 1.0 - 1.0e-9,
            "a world turn is pre-multiplied onto the stored rotation"
        );
    }

    #[test]
    fn a_target_carries_the_parent_frame_and_the_stored_pivot() {
        let mut project = nested_project();
        let cabinet = id(2);
        project.center_node_pivot(cabinet).expect("centre the cabinet pivot");
        let targets = move_targets(&project, &[cabinet]);
        let target = targets.first().copied().expect("one target");

        assert_eq!(target.start_local, project.node(cabinet).expect("node").placement());
        assert_eq!(target.parent, project.world_placement(project.root().id()).expect("root world"));
        assert_eq!(target.pivot_local, project.pivot_local_mm(cabinet).expect("pivot"));
        assert_ne!(target.pivot_local, DVec3::ZERO, "the target carries the authored pivot");
    }

    #[test]
    fn the_gizmo_anchors_on_the_last_movable_node_and_lists_its_drawables() {
        let project = nested_project();
        let (cabinet, outer, inner) = (id(2), id(3), id(4));
        let selection = [outer, cabinet, inner];
        let gizmo_target = target(&project, &selection);

        assert_eq!(gizmo_target.anchor(), Some(cabinet), "the last movable node anchors the gizmo");
        assert_eq!(gizmo_target.anchor_pivot(), project.pivot_world(cabinet));
        assert_eq!(gizmo_target.drawables(), vec![outer, inner], "each drawable is listed once, in target order");

        let inner_only = [inner];
        let leaf_target = target(&project, &inner_only);
        assert_eq!(leaf_target.anchor(), Some(inner), "a selected leaf is its own anchor");
        assert_eq!(leaf_target.drawables(), vec![inner]);
    }

    #[test]
    fn a_world_shift_moves_every_target_rigidly_through_its_own_frame() {
        let project = nested_project();
        let selection = [id(4), id(3)];
        let targets = move_targets(&project, &selection);
        let delta = DVec3::new(10.0, 4.0, -2.0);

        let commit = commit_translation(&targets, delta).expect("valid").expect("a move");
        assert_eq!(commit.changes.len(), 2);
        for (change, target) in commit.changes.iter().zip(&targets) {
            let before = target.parent.transform_point(target.start_local.translation());
            let after = target.parent.transform_point(change.placement.translation());
            assert!((after - before - delta).length() < 1.0e-9, "world shift was {:?}", after - before);
            assert_eq!(change.placement.rotation(), target.start_local.rotation(), "a move keeps the rotation");
        }
    }

    #[test]
    fn a_world_turn_keeps_the_anchor_pivot_fixed_and_orbits_the_rest() {
        let project = nested_project();
        let selection = [id(4), id(3)];
        let targets = move_targets(&project, &selection);
        let anchor = targets.last().copied().expect("anchor target");
        let pivot = project.pivot_world(anchor.node).expect("anchor pivot");
        assert_eq!(pivot, anchor.start_local.transform_point(anchor.pivot_local));
        let turn = DQuat::from_axis_angle(DVec3::Z, std::f64::consts::FRAC_PI_2);

        let commit = commit_rotation(&targets, pivot, turn).expect("valid").expect("a turn");
        assert_eq!(commit.changes.len(), 2);
        for (change, target) in commit.changes.iter().zip(&targets) {
            let before = target.parent.transform_point(target.start_local.translation());
            let after = target.parent.transform_point(change.placement.translation());
            let expected = pivot + turn * (before - pivot);
            assert!((after - expected).length() < 1.0e-6, "node {} landed at {after:?}", target.node);
        }
        let anchor_after = project
            .world_placement(project.root().id())
            .expect("root world")
            .transform_point(commit.changes[1].placement.transform_point(anchor.pivot_local));
        assert!((anchor_after - pivot).length() < 1.0e-6, "the anchor pivot moved to {anchor_after:?}");
    }

    #[test]
    fn a_drag_that_changes_nothing_returns_no_commit() {
        let project = nested_project();
        let targets = move_targets(&project, &[id(4), id(3)]);

        assert_eq!(commit_translation(&targets, DVec3::ZERO).expect("no error"), None);
        assert_eq!(commit_rotation(&targets, DVec3::ZERO, DQuat::IDENTITY).expect("no error"), None);
        assert_eq!(commit_translation(&[], DVec3::X).expect("no error"), None);
        assert_eq!(commit_rotation(&[], DVec3::ZERO, DQuat::from_rotation_z(0.5)).expect("no error"), None);
    }

    #[test]
    fn axes_have_distinct_directions_and_colours() {
        assert_eq!(Axis::ALL.len(), 3);
        assert_eq!(Axis::X.direction(), DVec3::X);
        assert_eq!(Axis::Y.direction(), DVec3::Y);
        assert_eq!(Axis::Z.direction(), DVec3::Z);
        assert_ne!(Axis::X.color(), Axis::Y.color());
        assert_ne!(Axis::Y.color(), Axis::Z.color());
    }

    #[test]
    fn a_ray_meets_a_plane_only_in_front_of_the_camera() {
        let ray = Ray::new(DVec3::new(0.0, 0.0, 10.0), -DVec3::Z).expect("ray");
        let hit = ray_plane(ray, DVec3::ZERO, DVec3::Z).expect("hit");
        assert!(hit.length() < 1.0e-12);

        let behind = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::Z).expect("ray");
        assert!(ray_plane(behind, DVec3::ZERO, DVec3::Z).is_none());

        let parallel = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::X).expect("ray");
        assert!(ray_plane(parallel, DVec3::ZERO, DVec3::Z).is_none());
    }

    #[test]
    fn the_six_standard_views_select_their_world_plane() {
        let cases = [
            (DVec3::X, DVec3::X, DVec3::Y, DVec3::Z, "YZ"),
            (-DVec3::X, -DVec3::X, -DVec3::Y, DVec3::Z, "YZ"),
            (DVec3::Y, DVec3::Y, -DVec3::X, DVec3::Z, "XZ"),
            (-DVec3::Y, -DVec3::Y, DVec3::X, DVec3::Z, "XZ"),
            (DVec3::Z, DVec3::Z, DVec3::X, DVec3::Y, "XY"),
            (-DVec3::Z, -DVec3::Z, DVec3::X, -DVec3::Y, "XY"),
        ];
        for (eye, normal, u, v, label) in cases {
            let camera = orthographic_view(eye);
            if eye.z > 0.5 {
                assert_eq!(camera.pitch(), MAX_PITCH, "a top view reaches the pole");
            } else if eye.z < -0.5 {
                assert_eq!(camera.pitch(), MIN_PITCH, "a bottom view reaches the pole");
            }
            let plane = WorkPlane::from_camera(camera);
            assert_eq!(plane.label(), label, "looking from {eye:?}");
            assert_vector(plane.normal(), normal, "the plane normal");
            assert_vector(plane.world(DVec2::new(1.0, 0.0)), u, "the u axis");
            assert_vector(plane.world(DVec2::new(0.0, 1.0)), v, "the v axis");
            assert_vector(u.cross(v), normal, "a right-handed basis");
        }
    }

    #[test]
    fn a_perspective_or_axonometric_view_draws_on_world_xy() {
        assert_eq!(WorkPlane::from_camera(Camera::default()), WorkPlane::WORLD_XY);

        let mut axonometric = Camera::default();
        axonometric.look_from(DVec3::new(1.0, 1.0, 1.0));
        axonometric.toggle_projection();
        assert_eq!(WorkPlane::from_camera(axonometric), WorkPlane::WORLD_XY);

        let mut orbited = orthographic_view(DVec3::Y);
        orbited.orbit(0.2, 0.0);
        assert_eq!(WorkPlane::from_camera(orbited), WorkPlane::WORLD_XY, "a view off the axis is axonometric");

        let mut tilted = orthographic_view(DVec3::X);
        tilted.orbit(0.0, 0.2);
        assert_eq!(WorkPlane::from_camera(tilted), WorkPlane::WORLD_XY);
    }

    #[test]
    fn a_pointer_maps_to_the_work_plane_and_back() {
        for eye in [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y, DVec3::Z, -DVec3::Z] {
            let camera = orthographic_view(eye);
            let plane = WorkPlane::from_camera(camera);
            for local in [DVec2::ZERO, DVec2::new(120.0, -45.0), DVec2::new(-300.0, 75.0)] {
                let world = plane.world(local);
                let pixel = screen_point(camera, viewport(), world).expect("the point is in front");
                let mapped = plane.pointer_point(camera, viewport(), pixel).expect("the ray meets the plane");
                assert!((mapped - local).length() < 1.0e-3, "from {eye:?}: {mapped:?} != {local:?}");
            }
        }

        let camera = Camera::default();
        let plane = WorkPlane::from_camera(camera);
        let world = DVec3::new(100.0, 50.0, 0.0);
        let pixel = screen_point(camera, viewport(), world).expect("the point is in front");
        let mapped = plane.pointer_point(camera, viewport(), pixel).expect("the ray meets the plane");
        assert!((mapped - DVec2::new(100.0, 50.0)).length() < 1.0e-3, "a perspective view lands on world XY");
    }

    #[test]
    fn a_work_plane_placement_maps_local_xy_onto_the_plane() {
        for eye in [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y, DVec3::Z, -DVec3::Z, DVec3::new(1.0, 1.0, 1.0)] {
            let camera = if eye == DVec3::new(1.0, 1.0, 1.0) {
                let mut camera = Camera::default();
                camera.look_from(eye);
                camera
            } else {
                orthographic_view(eye)
            };
            let plane = WorkPlane::from_camera(camera);
            let placement = plane.placement().expect("a plane has a rigid placement");
            assert_vector(placement.transform_point(DVec3::ZERO), DVec3::ZERO, "the plane runs through the origin");
            assert_vector(placement.transform_point(DVec3::X), plane.u, "local X is the plane u axis");
            assert_vector(placement.transform_point(DVec3::Y), plane.v, "local Y is the plane v axis");
            assert_vector(placement.transform_point(DVec3::Z), plane.normal(), "local Z is the plane normal");
        }
    }

    #[test]
    fn a_work_plane_placement_at_a_point_moves_the_origin_only() {
        let camera = orthographic_view(DVec3::Z);
        let plane = WorkPlane::from_camera(camera);
        let local = DVec2::new(120.0, -45.0);
        let placement = plane.placement_at(local).expect("a plane point has a placement");
        assert_vector(placement.transform_point(DVec3::ZERO), DVec3::new(120.0, -45.0, 0.0), "the origin is the click");
        assert_vector(placement.transform_point(DVec3::X), DVec3::new(121.0, -45.0, 0.0), "local X is the plane u");
        assert_vector(placement.transform_point(DVec3::Y), DVec3::new(120.0, -44.0, 0.0), "local Y is the plane v");
    }

    #[test]
    fn a_work_plane_behind_or_parallel_to_the_ray_has_no_point() {
        let plane = WorkPlane::WORLD_XY;
        let behind = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::Z).expect("ray");
        assert!(plane.ray_point(behind).is_none());
        let parallel = Ray::new(DVec3::new(0.0, 0.0, 10.0), DVec3::X).expect("ray");
        assert!(plane.ray_point(parallel).is_none());
    }

    #[test]
    fn the_grid_snaps_both_plane_coordinates() {
        let cases = [
            (DVec2::new(0.0, 0.0), DVec2::new(0.0, 0.0)),
            (DVec2::new(4.9, 5.0), DVec2::new(0.0, 10.0)),
            (DVec2::new(15.0, -15.0), DVec2::new(20.0, -20.0)),
            (DVec2::new(-4.9, -6.0), DVec2::new(0.0, -10.0)),
            (DVec2::new(1234.4, -987.6), DVec2::new(1230.0, -990.0)),
        ];
        for (raw, snapped) in cases {
            assert_eq!(snap_grid(raw), snapped, "from {raw:?}");
        }
    }

    #[test]
    fn a_draft_end_captures_the_pointer_inside_the_radius() {
        let ends = [DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0)];
        let cases = [
            (DVec2::new(3.0, 4.0), Some(DVec2::new(0.0, 0.0))),
            (DVec2::new(103.0, 0.0), Some(DVec2::new(100.0, 0.0))),
            (DVec2::new(50.0, 0.0), None),
            (DVec2::new(60.0, 0.5), None),
        ];
        for (point, expected) in cases {
            assert_eq!(nearest_end(point, &ends, 5.0), expected, "at {point:?}");
        }

        assert_eq!(nearest_end(DVec2::new(2.0, 0.0), &ends, 5.0), Some(ends[0]), "the radius is inclusive");
        assert_eq!(nearest_end(DVec2::new(2.0, 0.0), &ends, 0.0), None);
        assert_eq!(nearest_end(DVec2::new(2.0, 0.0), &ends, f64::NAN), None);
        assert_eq!(
            nearest_end(DVec2::new(2.0, 0.0), &ends, f64::INFINITY),
            None,
            "an unbounded radius is a bug, not a snap"
        );
    }

    #[test]
    fn the_earlier_draft_end_wins_a_tie() {
        let tied = [DVec2::new(-10.0, 0.0), DVec2::new(10.0, 0.0)];
        assert_eq!(nearest_end(DVec2::ZERO, &tied, 20.0), Some(tied[0]));
    }

    #[test]
    fn the_axis_lock_follows_the_larger_offset() {
        let anchor = DVec2::new(100.0, 50.0);
        let cases = [
            (DVec2::new(140.0, 60.0), DVec2::new(140.0, 50.0), SnapAxis::U),
            (DVec2::new(90.0, 120.0), DVec2::new(100.0, 120.0), SnapAxis::V),
            (DVec2::new(120.0, 70.0), DVec2::new(120.0, 50.0), SnapAxis::U),
            (DVec2::new(80.0, 30.0), DVec2::new(80.0, 50.0), SnapAxis::U),
            (DVec2::new(100.0, 70.0), DVec2::new(100.0, 70.0), SnapAxis::V),
            (DVec2::new(140.0, 50.0), DVec2::new(140.0, 50.0), SnapAxis::U),
        ];
        for (raw, expected, axis) in cases {
            assert_eq!(snap_axis(raw, anchor), (expected, axis), "from {raw:?}");
        }
    }

    #[test]
    fn snapping_applies_end_then_axis_then_grid() {
        let ends = [DVec2::new(100.0, 100.0)];
        fn request<'a>(
            point: DVec2,
            anchor: Option<DVec2>,
            ends: &'a [DVec2],
            mode: SnapMode,
            axis_lock: bool,
            end_radius: f64,
        ) -> SnapRequest<'a> {
            SnapRequest { point, anchor, ends, mode, axis_lock, end_radius, ignore_snaps: false }
        }
        let cases = [
            (
                request(DVec2::new(102.0, 101.0), Some(DVec2::ZERO), &ends, SnapMode::Grid, true, 5.0),
                SnappedPoint { point: DVec2::new(100.0, 100.0), target: SnapTarget::End },
            ),
            (
                request(DVec2::new(123.0, 55.0), Some(DVec2::new(5.0, 5.0)), &[], SnapMode::Grid, true, 0.0),
                SnappedPoint { point: DVec2::new(120.0, 5.0), target: SnapTarget::Axis },
            ),
            (
                request(DVec2::new(123.0, 55.0), Some(DVec2::new(5.0, 5.0)), &[], SnapMode::Grid, false, 0.0),
                SnappedPoint { point: DVec2::new(120.0, 60.0), target: SnapTarget::Grid },
            ),
            (
                request(DVec2::new(123.0, 55.0), None, &[], SnapMode::Free, false, 0.0),
                SnappedPoint { point: DVec2::new(123.0, 55.0), target: SnapTarget::Free },
            ),
            (
                request(DVec2::new(123.0, 55.0), Some(DVec2::new(5.0, 5.0)), &[], SnapMode::Free, true, 0.0),
                SnappedPoint { point: DVec2::new(123.0, 5.0), target: SnapTarget::Axis },
            ),
        ];
        for (request, expected) in cases {
            assert_eq!(snap_point(request), expected);
        }
    }

    #[test]
    fn snapping_passes_hostile_input_through_without_panicking() {
        let non_finite = DVec2::new(f64::NAN, f64::INFINITY);
        let snapped = snap_grid(non_finite);
        assert!(snapped.x.is_nan());
        assert!(snapped.y.is_infinite());

        let request = SnapRequest {
            point: non_finite,
            anchor: None,
            ends: &[],
            mode: SnapMode::Free,
            axis_lock: false,
            end_radius: 5.0,
            ignore_snaps: false,
        };
        let result = snap_point(request);
        assert!(result.point.x.is_nan());
        assert!(result.point.y.is_infinite());
        assert_eq!(result.target, SnapTarget::Free);

        let locked = SnapRequest {
            point: non_finite,
            anchor: Some(DVec2::ZERO),
            ends: &[],
            mode: SnapMode::Grid,
            axis_lock: true,
            end_radius: 5.0,
            ignore_snaps: false,
        };
        assert!(!snap_point(locked).point.is_finite(), "a non-finite input never becomes a silent zero");
    }

    #[test]
    fn alt_skips_the_end_and_grid_snaps_but_keeps_the_axis_lock() {
        let ends = [DVec2::new(100.0, 100.0)];
        let free = |point: DVec2, anchor: Option<DVec2>, axis_lock: bool| SnapRequest {
            point,
            anchor,
            ends: &ends,
            mode: SnapMode::Grid,
            axis_lock,
            end_radius: 5.0,
            ignore_snaps: true,
        };
        // The end is close enough to capture, but Alt leaves the pointer where it is.
        assert_eq!(
            snap_point(free(DVec2::new(102.0, 101.0), Some(DVec2::ZERO), false)),
            SnappedPoint { point: DVec2::new(102.0, 101.0), target: SnapTarget::Free }
        );
        // The grid is skipped too; only the axis Shift asks for remains.
        assert_eq!(
            snap_point(free(DVec2::new(123.0, 55.0), Some(DVec2::new(5.0, 5.0)), true)),
            SnappedPoint { point: DVec2::new(123.0, 5.0), target: SnapTarget::Axis }
        );
    }

    #[test]
    fn screen_segments_reject_degenerate_input() {
        assert!(distance_to_segment(Pos2::ZERO, Pos2::ZERO, Pos2::ZERO).is_none());
        assert_eq!(distance_to_segment(Pos2::new(0.0, 3.0), Pos2::ZERO, Pos2::new(10.0, 0.0)), Some(3.0));
        assert!(screen_direction(Vec2::ZERO).is_none());
        assert_eq!(screen_direction(Vec2::new(0.0, 4.0)), Some(Vec2::new(0.0, 1.0)));
    }
}
