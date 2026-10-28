//! Shared math and presentation for the viewport transform gizmos.
//!
//! The translate and rotate gizmos project world points with the camera, hit-test their handles in
//! screen space and compute one placement per moved node; the shell dispatches them as a single
//! `node.placement.set_many`. Keeping the axis colours, the projection and the commit types here
//! means the two gizmos cannot drift apart.

use eframe::egui;
use egui::{Color32, Pos2, Vec2};
use furnicraft_geom::{DQuat, DVec3};
use furnicraft_model::{Id, Placement, Project};
use furnicraft_scene::{Camera, Ray};

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

    /// Every panel that moves with the selection, in target order.
    pub(crate) fn panels(&self) -> Vec<Id> {
        let mut panels = Vec::new();
        for target in self.move_targets() {
            panels.extend(self.project.panel_ids_under(target.node).unwrap_or_default());
        }
        panels
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
                pivot_local: project.pivot_local(node)?,
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
    /// Shift the listed panels by a world-space delta.
    Translation {
        /// Panels to move.
        panels: &'a [Id],
        /// World-space shift.
        delta: DVec3,
    },
    /// Turn the listed panels about a world-space pivot.
    Rotation {
        /// Panels to turn.
        panels: &'a [Id],
        /// World-space point the turn is centred on.
        pivot: DVec3,
        /// World-space turn.
        rotation: DQuat,
    },
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Container, Panel, Project};
    use furnicraft_testkit::{sample_panel_nominal_size, sample_panel_thickness};

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

    fn target<'a>(project: &'a Project, selection: &'a [Id]) -> Target<'a> {
        Target { camera: Camera::default(), project, selection }
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
    fn a_target_carries_the_parent_frame_and_the_stored_pivot() {
        let project = nested_project();
        let cabinet = id(2);
        let targets = move_targets(&project, &[cabinet]);
        let target = targets.first().copied().expect("one target");

        assert_eq!(target.start_local, project.node(cabinet).expect("node").placement());
        assert_eq!(target.parent, project.world_placement(project.root().id()).expect("root world"));
        assert_eq!(target.pivot_local, project.pivot_local(cabinet).expect("pivot"));
        assert_ne!(target.pivot_local, DVec3::ZERO, "a container with children is centred on its bounds");
    }

    #[test]
    fn the_gizmo_anchors_on_the_last_movable_node_and_lists_its_panels() {
        let project = nested_project();
        let (cabinet, outer, inner) = (id(2), id(3), id(4));
        let selection = [outer, cabinet, inner];
        let gizmo_target = target(&project, &selection);

        assert_eq!(gizmo_target.anchor(), Some(cabinet), "the last movable node anchors the gizmo");
        assert_eq!(gizmo_target.anchor_pivot(), project.pivot_world(cabinet));
        assert_eq!(gizmo_target.panels(), vec![outer, inner], "each panel is listed once, in target order");

        let inner_only = [inner];
        let leaf_target = target(&project, &inner_only);
        assert_eq!(leaf_target.anchor(), Some(inner), "a selected leaf is its own anchor");
        assert_eq!(leaf_target.panels(), vec![inner]);
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
    fn screen_segments_reject_degenerate_input() {
        assert!(distance_to_segment(Pos2::ZERO, Pos2::ZERO, Pos2::ZERO).is_none());
        assert_eq!(distance_to_segment(Pos2::new(0.0, 3.0), Pos2::ZERO, Pos2::new(10.0, 0.0)), Some(3.0));
        assert!(screen_direction(Vec2::ZERO).is_none());
        assert_eq!(screen_direction(Vec2::new(0.0, 4.0)), Some(Vec2::new(0.0, 1.0)));
    }
}
