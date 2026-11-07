//! The central area as a tree of views.
//!
//! One view is the default; the bar turns it into two vertical, two horizontal or four, and every
//! border between two views is draggable. Each leaf owns a camera and the preset it is in, and the
//! active leaf is the one *Home*, *Frame all* and the inspector-style view actions act on.
//!
//! Everything here is view state: the tree, the ratios, the cameras and the active leaf never reach
//! the document (ADR 0029). The regions are plain rectangles in points, so a test can tile them
//! without a window or a GPU, and the shell turns each one into a scissored render pass.

use eframe::egui;
use furnicraft_scene::Camera;
use furnicraft_ui::display::OpeningView;

/// Smallest side a view may have, in points.
///
/// A border drag stops here, so a view can never be squeezed to nothing and a lost pointer cannot
/// hide a camera the user cannot find again.
pub(crate) const MIN_VIEW_POINTS: f32 = 120.0;

/// Lowest share of the area one side of a border may take.
pub(crate) const MIN_RATIO: f32 = 0.15;

/// Highest share of the area one side of a border may take.
pub(crate) const MAX_RATIO: f32 = 0.85;

/// Half-width of the band that grabs a border, in points.
///
/// The band is wider than the painted line so a border is easy to catch; the views still tile the
/// area exactly, because the band is only an interaction target and never a gap.
pub(crate) const DIVIDER_HIT_POINTS: f32 = 4.0;

/// The most views the workspace holds.
///
/// It is the renderer's uniform slot count, so a layout that wants another view is a compile-time
/// change in both places instead of a view silently dropped at runtime.
pub(crate) const MAX_LEAVES: usize = furnicraft_render::MAX_VIEWS;

/// The ratio a fresh split takes.
const DEFAULT_RATIO: f32 = 0.5;

/// A stable identity for one view; the cameras are keyed by it, so a drag or a relayout never
/// swaps one view's camera for another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct LeafId(u32);

/// The direction a split divides its area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitAxis {
    /// Two views side by side, divided by a vertical border.
    Columns,
    /// Two views one above the other, divided by a horizontal border.
    Rows,
}

impl SplitAxis {
    /// The length of `rect` along the split, in points.
    fn length(self, rect: egui::Rect) -> f32 {
        match self {
            Self::Columns => rect.width(),
            Self::Rows => rect.height(),
        }
    }

    /// The per-frame pointer delta projected onto the split, in points.
    fn along(self, delta: egui::Vec2) -> f32 {
        match self {
            Self::Columns => delta.x,
            Self::Rows => delta.y,
        }
    }

    /// The resize cursor a border of this split shows.
    pub(crate) const fn cursor(self) -> egui::CursorIcon {
        match self {
            Self::Columns => egui::CursorIcon::ResizeHorizontal,
            Self::Rows => egui::CursorIcon::ResizeVertical,
        }
    }
}

/// The layouts the bar offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LayoutKind {
    /// One view filling the area.
    #[default]
    Single,
    /// Two views side by side.
    TwoColumns,
    /// Two views one above the other.
    TwoRows,
    /// Two columns of two views.
    Quad,
}

impl LayoutKind {
    /// Every layout in the order the bar draws them.
    pub(crate) const ALL: [Self; 4] = [Self::Single, Self::TwoColumns, Self::TwoRows, Self::Quad];

    /// The user-visible name, used as the caption and the tooltip.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Single => "One view",
            Self::TwoColumns => "Two vertical",
            Self::TwoRows => "Two horizontal",
            Self::Quad => "Four views",
        }
    }

    /// The number of views the layout holds.
    pub(crate) const fn leaves(self) -> usize {
        match self {
            Self::Single => 1,
            Self::TwoColumns | Self::TwoRows => 2,
            Self::Quad => 4,
        }
    }
}

/// The tree itself: leaves carry only an id, so the layout structure and the view state stay apart.
#[derive(Clone, Debug, PartialEq)]
enum Node {
    Leaf(LeafId),
    Split { axis: SplitAxis, ratio: f32, first: Box<Node>, second: Box<Node> },
}

impl Node {
    /// Builds the tree of `kind` from `ids`, which are in depth-first order.
    fn of(kind: LayoutKind, ids: &[LeafId]) -> Option<Self> {
        let leaf = |index: usize| ids.get(index).copied().map(Node::Leaf);
        let split = |axis: SplitAxis, ratio: f32, first: Node, second: Node| Node::Split {
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        };
        match kind {
            LayoutKind::Single => leaf(0),
            LayoutKind::TwoColumns => Some(split(SplitAxis::Columns, DEFAULT_RATIO, leaf(0)?, leaf(1)?)),
            LayoutKind::TwoRows => Some(split(SplitAxis::Rows, DEFAULT_RATIO, leaf(0)?, leaf(1)?)),
            LayoutKind::Quad => {
                let first = split(SplitAxis::Rows, DEFAULT_RATIO, leaf(0)?, leaf(1)?);
                let second = split(SplitAxis::Rows, DEFAULT_RATIO, leaf(2)?, leaf(3)?);
                Some(split(SplitAxis::Columns, DEFAULT_RATIO, first, second))
            }
        }
    }

    fn regions(&self, rect: egui::Rect, out: &mut Vec<(LeafId, egui::Rect)>) {
        match self {
            Self::Leaf(id) => out.push((*id, rect)),
            Self::Split { axis, ratio, first, second } => {
                let (first_rect, second_rect) = split_rect(rect, *axis, *ratio);
                first.regions(first_rect, out);
                second.regions(second_rect, out);
            }
        }
    }

    fn dividers(&self, rect: egui::Rect, path: &mut Vec<u8>, out: &mut Vec<Divider>) {
        let Self::Split { axis, ratio, first, second } = self else {
            return;
        };
        let (first_rect, second_rect) = split_rect(rect, *axis, *ratio);
        out.push(Divider {
            path: path.clone(),
            axis: *axis,
            ratio: *ratio,
            rect,
            first: first_rect,
            second: second_rect,
        });

        path.push(0);
        first.dividers(first_rect, path, out);
        path.pop();
        path.push(1);
        second.dividers(second_rect, path, out);
        path.pop();
    }

    /// The split node at `path`, and the axis and ratio it holds.
    fn split_at(&self, path: &[u8]) -> Option<(SplitAxis, f32)> {
        let mut node = self;
        for step in path {
            let Self::Split { first, second, .. } = node else {
                return None;
            };
            node = if *step == 0 { first } else { second };
        }
        match node {
            Self::Split { axis, ratio, .. } => Some((*axis, *ratio)),
            Self::Leaf(_) => None,
        }
    }

    /// Sets the ratio of the split at `path`.
    fn set_ratio(&mut self, path: &[u8], ratio: f32) -> bool {
        let mut node = self;
        for step in path {
            let Self::Split { first, second, .. } = node else {
                return false;
            };
            node = if *step == 0 { first } else { second };
        }
        match node {
            Self::Split { ratio: stored, .. } => {
                *stored = ratio;
                true
            }
            Self::Leaf(_) => false,
        }
    }
}

/// One view: its own camera and the preset it is in.
#[derive(Clone, Debug)]
pub(crate) struct Leaf {
    id: LeafId,
    camera: Camera,
    /// The preset the camera is in, or `None` once the user has looked elsewhere.
    preset: Option<OpeningView>,
}

impl Leaf {
    /// The leaf's camera.
    pub(crate) const fn camera(&self) -> Camera {
        self.camera
    }

    /// Points the leaf along a preset direction.
    ///
    /// A preset writes yaw and pitch only: the target and the distance stay, so switching presets
    /// never moves the model or changes its size (ADR 0049 keeps the projection too).
    pub(crate) fn apply_preset(&mut self, preset: OpeningView) {
        let [x, y, z] = preset.direction();
        self.camera.look_from(furnicraft_geom::DVec3::new(x, y, z));
        self.preset = Some(preset);
    }

    /// Fits the leaf on the whole scene, keeping the direction it was looking from.
    ///
    /// A leaf that still sits in a preset keeps that preset's direction; a leaf the user has orbited
    /// keeps its own. The projection is untouched, so framing never changes the mode (ADR 0049).
    pub(crate) fn frame(&mut self, home: Camera) {
        let Some(preset) = self.preset else {
            let direction = self.camera.eye() - self.camera.target();
            self.camera = home;
            self.camera.look_from(direction);
            return;
        };
        self.camera = home;
        self.apply_preset(preset);
    }

    /// Returns the leaf to *Axonometric* without refitting the scene.
    pub(crate) fn home(&mut self) {
        self.apply_preset(OpeningView::Axonometric);
    }

    /// Orbits the leaf, which leaves its preset name.
    pub(crate) fn orbit(&mut self, delta_yaw: f64, delta_pitch: f64) {
        self.camera.orbit(delta_yaw, delta_pitch);
        self.preset = None;
    }

    /// Looks from an arbitrary direction, which leaves its preset name.
    pub(crate) fn look_from(&mut self, direction: furnicraft_geom::DVec3) {
        self.camera.look_from(direction);
        self.preset = None;
    }

    /// Pans the leaf; the view direction and its preset name are untouched.
    pub(crate) fn pan_pixels(&mut self, delta_x: f64, delta_y: f64, viewport_height: f64) {
        self.camera.pan_pixels(delta_x, delta_y, viewport_height);
    }

    /// Zooms the leaf; the view direction and its preset name are untouched.
    pub(crate) fn zoom(&mut self, factor: f64) {
        self.camera.zoom(factor);
    }

    /// Switches the projection, which is not a change of direction.
    pub(crate) fn toggle_projection(&mut self) {
        self.camera.toggle_projection();
    }
}

/// One border between two views, as the shell paints and grabs it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Divider {
    /// Where the border sits in the tree.
    pub(crate) path: Vec<u8>,
    /// The direction it divides.
    pub(crate) axis: SplitAxis,
    /// The share of the split's area the first side holds.
    pub(crate) ratio: f32,
    /// The area of the split this border belongs to.
    pub(crate) rect: egui::Rect,
    /// The first view's rectangle.
    pub(crate) first: egui::Rect,
    /// The second view's rectangle.
    pub(crate) second: egui::Rect,
}

impl Divider {
    /// The border line itself, in the middle of the two views.
    fn line(&self) -> (egui::Pos2, egui::Pos2) {
        match self.axis {
            SplitAxis::Columns => {
                let x = self.first.right();
                (egui::pos2(x, self.rect.top()), egui::pos2(x, self.rect.bottom()))
            }
            SplitAxis::Rows => {
                let y = self.first.bottom();
                (egui::pos2(self.rect.left(), y), egui::pos2(self.rect.right(), y))
            }
        }
    }

    /// The band that grabs the border: wide enough to hit, but not part of the layout.
    pub(crate) fn hit_rect(&self) -> egui::Rect {
        let (start, end) = self.line();
        egui::Rect::from_two_pos(start, end).expand2(match self.axis {
            SplitAxis::Columns => egui::vec2(DIVIDER_HIT_POINTS, 0.0),
            SplitAxis::Rows => egui::vec2(0.0, DIVIDER_HIT_POINTS),
        })
    }

    /// Whether `point` grabs this border.
    pub(crate) fn is_hit(&self, point: egui::Pos2) -> bool {
        self.hit_rect().contains(point)
    }

    /// The ratio bounds the current area allows.
    ///
    /// A side keeps at least [`MIN_VIEW_POINTS`] and its share stays inside
    /// [`MIN_RATIO`]..[`MAX_RATIO`]. An area too small for both rules keeps the share band only, so a
    /// narrow window still has a draggable border instead of a stuck one.
    fn bounds(&self) -> (f32, f32) {
        let extent = self.axis.length(self.rect).max(1.0);
        let minimum = (MIN_VIEW_POINTS / extent).max(MIN_RATIO);
        let maximum = (1.0 - MIN_VIEW_POINTS / extent).min(MAX_RATIO);
        if minimum <= maximum { (minimum, maximum) } else { (MIN_RATIO, MAX_RATIO) }
    }
}

/// One continuous border drag.
#[derive(Clone, Debug)]
struct DividerDrag {
    path: Vec<u8>,
    axis: SplitAxis,
    /// The share the pointer asks for, before the area clamps it.
    raw: f32,
}

/// The workspace's views: the tree, the leaves, the active leaf and any live border drag.
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    root: Node,
    leaves: Vec<Leaf>,
    active: LeafId,
    kind: LayoutKind,
    next: u32,
    drag: Option<DividerDrag>,
}

impl Layout {
    /// A single view holding `camera`.
    pub(crate) fn new(camera: Camera) -> Self {
        let id = LeafId(1);
        Self {
            root: Node::Leaf(id),
            leaves: vec![Leaf { id, camera, preset: None }],
            active: id,
            kind: LayoutKind::Single,
            next: 2,
            drag: None,
        }
    }

    /// The layout the tree currently holds.
    pub(crate) const fn kind(&self) -> LayoutKind {
        self.kind
    }

    /// The active leaf.
    pub(crate) const fn active(&self) -> LeafId {
        self.active
    }

    /// Makes `leaf` active, ignoring an id the tree no longer holds.
    pub(crate) fn set_active(&mut self, leaf: LeafId) {
        if self.leaves.iter().any(|candidate| candidate.id == leaf) {
            self.active = leaf;
        }
    }

    /// One leaf by id.
    pub(crate) fn leaf(&self, id: LeafId) -> Option<&Leaf> {
        self.leaves.iter().find(|leaf| leaf.id == id)
    }

    /// One leaf by id, for navigation.
    pub(crate) fn leaf_mut(&mut self, id: LeafId) -> Option<&mut Leaf> {
        self.leaves.iter_mut().find(|leaf| leaf.id == id)
    }

    /// Every leaf, for an action that fits the whole workspace.
    pub(crate) fn leaves_mut(&mut self) -> &mut [Leaf] {
        &mut self.leaves
    }

    /// The active leaf, which is always present.
    pub(crate) fn active_leaf(&self) -> &Leaf {
        // `active` is only ever set to an id the tree holds, and `set_kind` keeps it that way.
        self.leaves.iter().find(|leaf| leaf.id == self.active).unwrap_or(&self.leaves[0])
    }

    /// The preset a leaf is in, if it has not been orbited since.
    pub(crate) fn preset(&self, id: LeafId) -> Option<OpeningView> {
        self.leaf(id).and_then(|leaf| leaf.preset)
    }

    /// The camera of the active leaf.
    pub(crate) fn active_camera(&self) -> Camera {
        self.active_leaf().camera
    }

    /// Replaces every leaf's camera, as opening a scene or refitting the workspace does.
    pub(crate) fn reset_cameras(&mut self, camera: Camera) {
        for leaf in &mut self.leaves {
            leaf.camera = camera;
            leaf.preset = None;
        }
    }

    /// Turns the workspace into `kind`, keeping the leaves it already has in order and giving a new
    /// leaf the active view's camera.
    pub(crate) fn set_kind(&mut self, kind: LayoutKind) {
        self.kind = kind;
        let wanted = kind.leaves();
        let source = self.active_camera();
        while self.leaves.len() < wanted {
            let id = LeafId(self.next);
            self.next = self.next.saturating_add(1);
            self.leaves.push(Leaf { id, camera: source, preset: None });
        }
        self.leaves.truncate(wanted);
        let ids: Vec<LeafId> = self.leaves.iter().map(|leaf| leaf.id).collect();
        if let Some(root) = Node::of(kind, &ids) {
            self.root = root;
        }
        if !self.leaves.iter().any(|leaf| leaf.id == self.active) {
            self.active = self.leaves[0].id;
        }
        self.drag = None;
    }

    /// The rectangles of every view, in depth-first order.
    pub(crate) fn regions(&self, area: egui::Rect) -> Vec<(LeafId, egui::Rect)> {
        let mut regions = Vec::with_capacity(self.leaves.len());
        self.root.regions(area, &mut regions);
        regions
    }

    /// Every border between two views.
    pub(crate) fn dividers(&self, area: egui::Rect) -> Vec<Divider> {
        let mut dividers = Vec::new();
        self.root.dividers(area, &mut Vec::new(), &mut dividers);
        dividers
    }

    /// The direction the border being dragged divides, for the resize cursor.
    pub(crate) fn dragging_axis(&self) -> Option<SplitAxis> {
        self.drag.as_ref().map(|drag| drag.axis)
    }

    /// Whether `point` grabs a border, for the resize cursor before a drag starts.
    pub(crate) fn border_at(&self, area: egui::Rect, point: egui::Pos2) -> Option<SplitAxis> {
        self.dividers(area).into_iter().find(|divider| divider.is_hit(point)).map(|divider| divider.axis)
    }

    /// Starts a border drag when `point` grabs one, returning whether it did.
    pub(crate) fn begin_drag(&mut self, area: egui::Rect, point: egui::Pos2) -> bool {
        let Some(divider) = self.dividers(area).into_iter().find(|divider| divider.is_hit(point)) else {
            return false;
        };
        let raw = self.root.split_at(&divider.path).map(|(_, ratio)| ratio).unwrap_or(divider.ratio);
        self.drag = Some(DividerDrag { path: divider.path, axis: divider.axis, raw });
        true
    }

    /// Advances the border drag by this frame's pointer delta.
    pub(crate) fn drag_by(&mut self, area: egui::Rect, delta: egui::Vec2) {
        let Some(path) = self.drag.as_ref().map(|drag| drag.path.clone()) else {
            return;
        };
        let Some(split) = self.dividers(area).into_iter().find(|divider| divider.path == path) else {
            return;
        };
        let extent = split.axis.length(split.rect).max(1.0);
        let (minimum, maximum) = split.bounds();
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.raw += split.axis.along(delta) / extent;
        let ratio = drag.raw.clamp(minimum, maximum);
        self.root.set_ratio(&path, ratio);
    }

    /// Ends a border drag.
    pub(crate) fn end_drag(&mut self) {
        self.drag = None;
    }
}

/// Splits `rect` into the two views of `axis` at `ratio`, with no gap: the views tile their area, and
/// the border is drawn and grabbed on the shared edge.
fn split_rect(rect: egui::Rect, axis: SplitAxis, ratio: f32) -> (egui::Rect, egui::Rect) {
    let ratio = if ratio.is_finite() { ratio.clamp(0.0, 1.0) } else { DEFAULT_RATIO };
    match axis {
        SplitAxis::Columns => {
            let x = rect.left() + rect.width() * ratio;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom())),
                egui::Rect::from_min_max(egui::pos2(x, rect.top()), rect.max),
            )
        }
        SplitAxis::Rows => {
            let y = rect.top() + rect.height() * ratio;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), y)),
                egui::Rect::from_min_max(egui::pos2(rect.left(), y), rect.max),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn area() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    fn layout(kind: LayoutKind) -> Layout {
        let mut layout = Layout::new(Camera::default());
        layout.set_kind(kind);
        layout
    }

    #[test]
    fn every_layout_fits_the_workspace_and_its_slots() {
        // `MAX_LEAVES` is the renderer's slot count; each offered layout has to fit it, so a new
        // layout cannot quietly draw fewer views than it claims.
        for kind in LayoutKind::ALL {
            assert!(kind.leaves() <= MAX_LEAVES, "{kind:?} fits the workspace");
        }
        assert_eq!(LayoutKind::Quad.leaves(), MAX_LEAVES, "four views use every slot");
    }

    #[test]
    fn a_press_on_a_border_resizes_and_a_press_in_a_view_activates_it() {
        // The shell's rule: a press is either a border grab or a view activation, never both. A
        // border press must not steal the click that selects an object.
        let area = area();
        let mut layout = layout(LayoutKind::TwoColumns);
        let first = layout.leaves[0].id;
        let second = layout.leaves[1].id;
        assert_eq!(layout.active(), first);

        // A press in the right view activates it and grabs no border.
        assert!(!layout.begin_drag(area, egui::pos2(700.0, 300.0)));
        layout.set_active(second);
        assert_eq!(layout.active(), second);

        // A press on the shared edge grabs the border and leaves the active view alone.
        assert!(layout.begin_drag(area, egui::pos2(400.0, 300.0)));
        assert_eq!(layout.active(), second, "a resize does not change the active view");
        layout.end_drag();

        // The band is wider than the line, so a press a few points away still catches it.
        assert!(layout.begin_drag(area, egui::pos2(403.0, 40.0)));
        layout.end_drag();
        assert!(!layout.begin_drag(area, egui::pos2(420.0, 40.0)));
    }

    #[test]
    fn a_border_hover_names_the_axis_of_its_cursor() {
        let area = area();
        let columns = layout(LayoutKind::TwoColumns);
        assert_eq!(columns.border_at(area, egui::pos2(400.0, 100.0)), Some(SplitAxis::Columns));
        assert_eq!(columns.border_at(area, egui::pos2(100.0, 100.0)), None);

        let rows = layout(LayoutKind::TwoRows);
        assert_eq!(rows.border_at(area, egui::pos2(100.0, 300.0)), Some(SplitAxis::Rows));

        let quad = layout(LayoutKind::Quad);
        assert_eq!(quad.dividers(area).len(), 3, "a four-view tree has three borders");
    }

    #[test]
    fn a_fit_moves_every_view_close_and_keeps_its_direction() {
        let home = Camera::new(furnicraft_geom::DVec3::new(10.0, 20.0, 30.0), 900.0).expect("camera");
        let mut layout = Layout::new(home);
        layout.set_kind(LayoutKind::TwoColumns);
        let ids: Vec<LeafId> = layout.leaves.iter().map(|leaf| leaf.id).collect();
        let first = layout.leaf_mut(ids[0]).expect("first");
        first.apply_preset(OpeningView::Front);
        let second = layout.leaf_mut(ids[1]).expect("second");
        second.look_from(furnicraft_geom::DVec3::new(1.0, 1.0, 1.0));

        for leaf in layout.leaves_mut() {
            leaf.frame(home);
        }

        let first = layout.leaf(ids[0]).expect("first");
        assert_eq!(first.camera.target(), home.target(), "the fit puts every view on the scene");
        assert!((first.camera.distance() - home.distance()).abs() < 1.0e-9);
        assert_eq!(first.preset, Some(OpeningView::Front), "a view in a preset keeps it");
        assert!(first.camera.forward().dot(furnicraft_geom::DVec3::Y) > 0.999);

        let second = layout.leaf(ids[1]).expect("second");
        assert_eq!(second.camera.target(), home.target());
        assert_eq!(second.preset, None, "an orbited view stays off every preset");
        let direction = furnicraft_geom::DVec3::new(1.0, 1.0, 1.0).normalize();
        assert!(second.camera.eye().dot(direction) > 0.0, "and keeps the direction it looked from");
    }

    #[test]
    fn a_split_and_a_join_round_trip() {
        let mut layout = Layout::new(Camera::default());
        let first = layout.active();
        assert_eq!(layout.leaves.len(), 1);

        layout.set_kind(LayoutKind::TwoColumns);
        assert_eq!(layout.leaves.len(), 2);
        assert_eq!(layout.leaves[0].id, first, "the first view keeps its identity and its camera");

        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.leaves.len(), 1, "a join leaves one view");
        assert_eq!(layout.leaves[0].id, first);
        assert_eq!(layout.active(), first, "the join keeps the first view active");

        layout.set_kind(LayoutKind::Quad);
        assert_eq!(layout.leaves.len(), 4);
        assert_eq!(layout.regions(area()).len(), 4);
        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.regions(area()).len(), 1, "the single view fills the area again");
    }

    #[test]
    fn four_views_tile_the_area_exactly() {
        let area = area();
        let regions = layout(LayoutKind::Quad).regions(area);
        assert_eq!(regions.len(), 4);
        let covered: f32 = regions.iter().map(|(_, rect)| rect.width() * rect.height()).sum();
        assert!((covered - area.width() * area.height()).abs() < 1.0e-3, "no gap and no overlap: {covered}");
        for (_, rect) in &regions {
            assert!(area.contains_rect(*rect), "{rect:?} stays inside the viewport");
        }
        // Two columns of two rows, in depth-first order: the first two share the left column and
        // the last two the right one, and the border between a pair is shared, not gapped.
        assert!((regions[0].1.width() - regions[1].1.width()).abs() < 1.0e-4);
        assert!((regions[0].1.left() - regions[1].1.left()).abs() < 1.0e-4);
        assert!((regions[2].1.left() - regions[3].1.left()).abs() < 1.0e-4);
        assert!(regions[2].1.left() > regions[0].1.right() - 1.0e-4, "the columns are side by side");
        assert!((regions[0].1.bottom() - regions[1].1.top()).abs() < 1.0e-4, "a border is shared, not gapped");
        assert!((regions[1].1.bottom() - area.bottom()).abs() < 1.0e-4);
    }

    #[test]
    fn a_drag_clamps_at_both_ends_and_never_collapses_a_view() {
        let area = area();
        let mut layout = layout(LayoutKind::TwoColumns);
        assert!(layout.begin_drag(area, egui::pos2(400.0, 300.0)), "the border is grabbed in its band");

        // Dragging far past the left edge stops at the larger of the legal share and 120 pt.
        layout.drag_by(area, egui::vec2(-10_000.0, 0.0));
        let regions = layout.regions(area);
        let left = regions[0].1.width();
        assert!(left >= MIN_VIEW_POINTS - 1.0e-3, "the first view keeps its 120 pt: {left}");
        assert!(left / area.width() >= MIN_RATIO - 1.0e-6);

        layout.drag_by(area, egui::vec2(20_000.0, 0.0));
        let right = layout.regions(area)[1].1.width();
        assert!(right >= MIN_VIEW_POINTS - 1.0e-3, "the second view keeps its 120 pt: {right}");
        assert!(right / area.width() >= MIN_RATIO - 1.0e-6);

        // The views still tile the area after both extremes.
        let covered: f32 = layout.regions(area).iter().map(|(_, rect)| rect.width()).sum();
        assert!((covered - area.width()).abs() < 1.0e-3);

        layout.end_drag();
        assert!(layout.dragging_axis().is_none(), "the drag ends with the release");
        assert!(!layout.begin_drag(area, egui::pos2(400.0, 20.0)), "away from the border nothing is grabbed");
    }

    #[test]
    fn a_row_drag_only_moves_along_its_axis() {
        let area = area();
        let mut layout = layout(LayoutKind::TwoRows);
        assert!(layout.begin_drag(area, egui::pos2(400.0, 300.0)));
        layout.drag_by(area, egui::vec2(400.0, -100.0));
        let regions = layout.regions(area);
        assert!((regions[0].1.width() - area.width()).abs() < 1.0e-4, "a horizontal border leaves the width alone");
        assert!(regions[0].1.height() < 300.0, "and moves the shared edge up");
    }

    #[test]
    fn a_preset_turns_the_view_and_never_moves_the_model() {
        let layout = Layout::new(Camera::default());
        let leaf = layout.leaves[0].clone();
        let target = leaf.camera().target();
        let distance = leaf.camera().distance();

        for (preset, forward) in [
            (OpeningView::Front, furnicraft_geom::DVec3::Y),
            (OpeningView::Right, -furnicraft_geom::DVec3::X),
            (OpeningView::Top, -furnicraft_geom::DVec3::Z),
        ] {
            let mut leaf = leaf.clone();
            leaf.apply_preset(preset);
            let camera = leaf.camera();
            assert_eq!(camera.target(), target, "{preset:?} keeps the target");
            assert!((camera.distance() - distance).abs() < 1.0e-9, "{preset:?} keeps the distance");
            assert!(camera.forward().dot(forward) > 0.999, "{preset:?} looks along {forward:?}");
            assert!((camera.right().length() - 1.0).abs() < 1.0e-9);
            assert!((camera.up().length() - 1.0).abs() < 1.0e-9);
            assert!(camera.right().dot(camera.forward()).abs() < 1.0e-9);
            assert!(camera.up().dot(camera.forward()).abs() < 1.0e-9);
            assert_eq!(leaf.preset, Some(preset));
        }

        // A front elevation is right-handed: screen right is world +X and screen up is world +Z.
        let mut front = layout.leaves[0].clone();
        front.apply_preset(OpeningView::Front);
        assert!(front.camera().right().dot(furnicraft_geom::DVec3::X) > 0.999);
        assert!(front.camera().up().dot(furnicraft_geom::DVec3::Z) > 0.999);
    }

    #[test]
    fn the_home_button_restores_the_axonometric_preset() {
        let mut layout = Layout::new(Camera::default());
        let id = layout.active();
        let leaf = layout.leaf_mut(id).expect("the active leaf");
        leaf.apply_preset(OpeningView::Top);
        assert_eq!(leaf.preset, Some(OpeningView::Top));
        leaf.orbit(0.4, 0.1);
        assert_eq!(leaf.preset, None, "an orbit leaves every preset");

        leaf.home();
        assert_eq!(leaf.preset, Some(OpeningView::Axonometric));
        let expected = OpeningView::Axonometric.direction();
        let forward = leaf.camera().forward();
        assert!(
            forward.dot(-furnicraft_geom::DVec3::new(expected[0], expected[1], expected[2])) > 0.999,
            "home looks from the axonometric direction"
        );
    }

    #[test]
    fn a_preset_leaves_the_projection_alone() {
        let mut layout = Layout::new(Camera::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        leaf.toggle_projection();
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));
        leaf.apply_preset(OpeningView::Front);
        assert!(
            matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }),
            "a preset writes yaw and pitch only"
        );
    }

    #[test]
    fn the_active_leaf_is_the_one_the_tree_holds() {
        let mut layout = layout(LayoutKind::Quad);
        let last = layout.leaves[3].id;
        layout.set_active(last);
        assert_eq!(layout.active(), last);
        assert_eq!(layout.active_leaf().id, last);

        // A join that drops the active leaf falls back to the view that remains.
        layout.set_active(last);
        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.active(), layout.leaves[0].id);
        layout.set_active(LeafId(999));
        assert_eq!(layout.active(), layout.leaves[0].id, "an unknown id is ignored");
    }
}
