//! The central area as a tree of views.
//!
//! One view is the default; the bar turns it into two vertical, two horizontal or four, and every
//! border between two views is draggable. Any view can expand over the whole workspace and return,
//! which changes only the rectangle it is given, never the tree. Each layout keeps its own tree,
//! cameras, presets, display styles, split ratios and active leaf, so switching away and back
//! restores exactly the workspace the user left (ADR 0079). Each leaf owns a camera, the preset it
//! is in and the display style it draws with, and the active leaf is the only target of orbit, pan,
//! zoom, the presets, the style buttons, selection and the gizmos.
//!
//! Everything here is view state: the trees, the ratios, the cameras and the active leaves never
//! reach the document (ADR 0029). The regions are plain rectangles in points, so a test can tile them
//! without a window or a GPU, and the shell turns each one into a scissored render pass.

use eframe::egui;
use furnicraft_scene::{Camera, SceneFrame};
use furnicraft_ui::display::{OpeningView, ShadingStyle};

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
    pub(crate) const fn label(self) -> furnicraft_text::Message {
        match self {
            Self::Single => furnicraft_text::Message::LayoutOneView,
            Self::TwoColumns => furnicraft_text::Message::LayoutTwoVertical,
            Self::TwoRows => furnicraft_text::Message::LayoutTwoHorizontal,
            Self::Quad => furnicraft_text::Message::LayoutFourViews,
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

    /// The slot this layout's remembered state occupies.
    const fn index(self) -> usize {
        match self {
            Self::Single => 0,
            Self::TwoColumns => 1,
            Self::TwoRows => 2,
            Self::Quad => 3,
        }
    }

    /// The standard preset of the leaf at `index`, in depth-first order.
    ///
    /// The split layouts open on the views their names promise: the front elevation first and the
    /// axonometric view last, with a top and a left view in the four-view corners. Only the single
    /// view follows the workspace's opening preference, because it is the view a new scene starts in
    /// (ADR 0079).
    const fn standard_view(self, index: usize, opening: OpeningView) -> OpeningView {
        match self {
            Self::Single => opening,
            Self::TwoColumns | Self::TwoRows => {
                if index == 0 {
                    OpeningView::Front
                } else {
                    OpeningView::Axonometric
                }
            }
            Self::Quad => match index {
                0 => OpeningView::Front,
                1 => OpeningView::Top,
                2 => OpeningView::Left,
                _ => OpeningView::Axonometric,
            },
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

/// One view: its own camera, the preset it is in and the display style it draws with.
#[derive(Clone, Debug)]
pub(crate) struct Leaf {
    id: LeafId,
    camera: Camera,
    /// The preset the camera is in, or `None` once the user has looked elsewhere.
    preset: Option<OpeningView>,
    /// The display style this view draws with (ADR 0070).
    style: ShadingStyle,
}

impl Leaf {
    /// The leaf's camera.
    pub(crate) const fn camera(&self) -> Camera {
        self.camera
    }

    /// The display style this view draws with.
    pub(crate) const fn style(&self) -> ShadingStyle {
        self.style
    }

    /// Changes the display style of this view alone.
    pub(crate) fn set_style(&mut self, style: ShadingStyle) {
        self.style = style;
    }

    /// Points the leaf along a preset direction.
    ///
    /// A preset writes yaw and pitch only: the target and the distance stay, so switching presets
    /// never moves the model or changes its size. A standard view is orthographic by default and
    /// converts a perspective view by the visible-height rule, while the axonometric view keeps the
    /// projection the camera already had (ADR 0049).
    pub(crate) fn apply_preset(&mut self, preset: OpeningView) {
        let [x, y, z] = preset.direction();
        let direction = furnicraft_geom::DVec3::new(x, y, z);
        self.camera.look_from_oriented(direction, furnicraft_scene::canonical_up(direction));
        if preset.is_standard() {
            self.camera.set_orthographic();
        }
        self.preset = Some(preset);
    }

    /// Fits the leaf on the whole scene, keeping the direction it was looking from.
    ///
    /// `fit` is the camera [`crate::scene_build::home_camera`] computes for the scene's bounds. A
    /// leaf that still sits in a preset keeps that preset's direction; a leaf the user has orbited
    /// keeps its own. The projection is kept as it was: a perspective view stays perspective, and
    /// an orthographic view stays orthographic with the height the fit distance shows, so framing
    /// never changes the mode (ADR 0049).
    pub(crate) fn frame(&mut self, fit: Camera) {
        let projection = self.camera.projection();
        match self.preset {
            Some(preset) => {
                self.camera = fit;
                self.apply_preset(preset);
            }
            None => {
                let direction = self.camera.eye() - self.camera.target();
                self.camera = fit;
                self.camera.look_from_oriented(direction, furnicraft_scene::canonical_up(direction));
            }
        }
        match projection {
            furnicraft_scene::Projection::Perspective { .. } => {
                if matches!(self.camera.projection(), furnicraft_scene::Projection::Orthographic { .. }) {
                    self.camera.toggle_projection();
                }
            }
            furnicraft_scene::Projection::Orthographic { .. } => self.camera.set_orthographic(),
        }
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
    ///
    /// A pole direction takes the canonical up, so clicking the top ball never keeps the yaw the
    /// camera happened to have (X20). A click on a world axis is a standard view, so it makes the
    /// camera orthographic by the same visible-height rule as a standard preset; a free direction
    /// keeps the projection.
    pub(crate) fn look_from(&mut self, direction: furnicraft_geom::DVec3) {
        self.camera.look_from_oriented(direction, furnicraft_scene::canonical_up(direction));
        if along_world_axis(direction) {
            self.camera.set_orthographic();
        }
        self.preset = None;
    }

    /// Rolls the view about its own axis, which leaves its preset name.
    ///
    /// The roll is view state; a preset, Home or Frame all writes the same field back to zero
    /// through `look_from_oriented` (ADR 0078), so a rolled view is no longer a preset basis.
    pub(crate) fn roll_to(&mut self, radians: f64) {
        self.camera.roll_to(radians);
        self.preset = None;
    }

    /// Turns the view about its own axis by `radians`, which leaves its preset name.
    pub(crate) fn roll_by(&mut self, radians: f64) {
        self.camera.roll_by(radians);
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

/// Whether `direction` points along one world axis, as the corner widget's axis balls do.
///
/// An axis ball snaps to an exact `±X`, `±Y` or `±Z` eye offset; a free orbit direction, even one
/// that happens to be near an axis, is not a standard view and keeps the projection. Non-finite or
/// zero input is never an axis.
fn along_world_axis(direction: furnicraft_geom::DVec3) -> bool {
    let Some(unit) = direction.try_normalize() else {
        return false;
    };
    let components = [unit.x.abs(), unit.y.abs(), unit.z.abs()];
    components.iter().filter(|component| **component > 1.0e-9).count() == 1
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

/// One layout's remembered state: its tree, its leaves, the active leaf and the next free id.
///
/// The state is per kind, so switching layouts never reshapes another layout's tree: a view the
/// user split, dragged or orbited is exactly where they left it when they come back (ADR 0079).
#[derive(Clone, Debug)]
struct LayoutState {
    root: Node,
    leaves: Vec<Leaf>,
    active: LeafId,
    next: u32,
}

impl LayoutState {
    /// The state a layout opens in the first time it is shown.
    ///
    /// `fit` is the camera framed on the whole scene; every leaf starts from it and is then pointed
    /// along the layout's standard preset. A fresh workspace therefore shows four useful views
    /// before the user has arranged anything.
    fn standard(kind: LayoutKind, fit: Camera, opening: OpeningView, style: ShadingStyle) -> Self {
        let mut state = Self { root: Node::Leaf(LeafId(1)), leaves: Vec::new(), active: LeafId(1), next: 1 };
        for index in 0..kind.leaves() {
            let id = state.allocate();
            let mut leaf = Leaf { id, camera: fit, preset: None, style };
            leaf.apply_preset(kind.standard_view(index, opening));
            state.leaves.push(leaf);
        }
        let ids: Vec<LeafId> = state.leaves.iter().map(|leaf| leaf.id).collect();
        state.root = Node::of(kind, &ids).unwrap_or_else(|| Node::Leaf(ids.first().copied().unwrap_or(LeafId(1))));
        state.active = ids.first().copied().unwrap_or(LeafId(1));
        state
    }

    /// The next free leaf id, which a new view in this layout would take.
    fn allocate(&mut self) -> LeafId {
        let id = LeafId(self.next);
        self.next = self.next.saturating_add(1);
        id
    }

    fn leaf(&self, id: LeafId) -> Option<&Leaf> {
        self.leaves.iter().find(|leaf| leaf.id == id)
    }

    fn leaf_mut(&mut self, id: LeafId) -> Option<&mut Leaf> {
        self.leaves.iter_mut().find(|leaf| leaf.id == id)
    }

    fn active_leaf(&self) -> &Leaf {
        // `active` is only ever set to an id the leaves hold, and `set_active` keeps it that way.
        self.leaves.iter().find(|leaf| leaf.id == self.active).unwrap_or(&self.leaves[0])
    }

    fn active_leaf_mut(&mut self) -> &mut Leaf {
        let active = self.active;
        if let Some(index) = self.leaves.iter().position(|leaf| leaf.id == active) {
            &mut self.leaves[index]
        } else {
            &mut self.leaves[0]
        }
    }
}

/// The workspace's views: one remembered state per layout and any live border drag.
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    kind: LayoutKind,
    states: [LayoutState; LayoutKind::ALL.len()],
    drag: Option<DividerDrag>,
    /// The view filling the whole workspace, or `None` while the tree is shown.
    ///
    /// The mode is temporary: it never reshapes the tree, so every ratio and camera stays where the
    /// user left it, and switching layouts leaves it (ADR 0079).
    maximized: Option<LeafId>,
}

impl Layout {
    /// One single view built from the fit camera, plus the standard state of every other layout.
    ///
    /// `fit` is the camera framed on the whole scene, `opening` is the preset of the single view,
    /// which is the view a new scene starts in, and `style` is the display style every fresh view
    /// opens with. The split layouts take their own standard presets and are shown when the user
    /// picks them (ADR 0079).
    pub(crate) fn new(fit: Camera, opening: OpeningView, style: ShadingStyle) -> Self {
        Self {
            kind: LayoutKind::Single,
            states: LayoutKind::ALL.map(|kind| LayoutState::standard(kind, fit, opening, style)),
            drag: None,
            maximized: None,
        }
    }

    /// The state of the layout currently shown.
    fn current(&self) -> &LayoutState {
        &self.states[self.kind.index()]
    }

    /// The state of the layout currently shown, for editing.
    fn current_mut(&mut self) -> &mut LayoutState {
        let index = self.kind.index();
        &mut self.states[index]
    }

    /// The layout the workspace is showing.
    pub(crate) const fn kind(&self) -> LayoutKind {
        self.kind
    }

    /// The active leaf of the shown layout.
    pub(crate) fn active(&self) -> LeafId {
        self.current().active
    }

    /// Whether one view fills the whole workspace.
    pub(crate) fn maximized(&self) -> bool {
        self.maximized.is_some()
    }

    /// Makes `leaf` active, ignoring an id the shown layout no longer holds.
    pub(crate) fn set_active(&mut self, leaf: LeafId) {
        let state = self.current_mut();
        if state.leaves.iter().any(|candidate| candidate.id == leaf) {
            state.active = leaf;
        }
    }

    /// One leaf of the shown layout by id.
    pub(crate) fn leaf(&self, id: LeafId) -> Option<&Leaf> {
        self.current().leaf(id)
    }

    /// One leaf of the shown layout by id, for navigation.
    pub(crate) fn leaf_mut(&mut self, id: LeafId) -> Option<&mut Leaf> {
        self.current_mut().leaf_mut(id)
    }

    /// The active leaf, which is always present.
    pub(crate) fn active_leaf(&self) -> &Leaf {
        self.current().active_leaf()
    }

    /// The active leaf, for a gesture that acts on it alone.
    pub(crate) fn active_leaf_mut(&mut self) -> &mut Leaf {
        self.current_mut().active_leaf_mut()
    }

    /// The preset a leaf is in, if it has not been orbited since.
    pub(crate) fn preset(&self, id: LeafId) -> Option<OpeningView> {
        self.leaf(id).and_then(|leaf| leaf.preset)
    }

    /// The camera of the active leaf.
    pub(crate) fn active_camera(&self) -> Camera {
        self.active_leaf().camera
    }

    /// The display style of the active leaf.
    pub(crate) fn active_style(&self) -> ShadingStyle {
        self.active_leaf().style
    }

    /// Changes the display style of the active leaf alone.
    pub(crate) fn set_active_style(&mut self, style: ShadingStyle) {
        self.active_leaf_mut().set_style(style);
    }

    /// Writes the active leaf's style into a frame's single-view fallback.
    ///
    /// A frame carries one style for the caller that draws it whole; the split workspace passes a
    /// style per view instead (U63).
    pub(crate) fn apply_frame_style(&self, frame: &mut SceneFrame) {
        frame.shading = self.active_style();
    }

    /// Shows `kind`, restoring the state that layout last had.
    ///
    /// Every layout keeps its own tree, cameras, presets, ratios and active leaf, so a switch is a
    /// change of which state is shown, not a reshape. The split the user leaves is the split they
    /// get back (ADR 0079).
    pub(crate) fn set_kind(&mut self, kind: LayoutKind) {
        self.kind = kind;
        self.drag = None;
        self.maximized = None;
    }

    /// Expands the active view over the whole workspace, or returns the tree.
    ///
    /// The tree is untouched: expanding only changes which rectangle the views are given, so
    /// restoring shows the same four rectangles, ratios and cameras the user left (ADR 0079). A
    /// single view already fills the workspace, so it refuses the mode instead of entering a state it
    /// cannot leave.
    pub(crate) fn toggle_maximized(&mut self) {
        if self.current().leaves.len() < 2 {
            self.maximized = None;
            return;
        }
        let active = self.active();
        self.maximized = if self.maximized == Some(active) { None } else { Some(active) };
        self.drag = None;
    }

    /// The rectangles of every view of the shown layout, in depth-first order.
    ///
    /// An expanded view owns the whole area and is the only region, so picking and the border
    /// handling see one surface instead of a hidden tree.
    pub(crate) fn regions(&self, area: egui::Rect) -> Vec<(LeafId, egui::Rect)> {
        if let Some(leaf) = self.maximized {
            return vec![(leaf, area)];
        }
        let mut regions = Vec::with_capacity(self.current().leaves.len());
        self.current().root.regions(area, &mut regions);
        regions
    }

    /// Every border between two views of the shown layout.
    ///
    /// An expanded view leaves no border: the workspace is one view, so there is nothing to resize
    /// and nothing to grab.
    pub(crate) fn dividers(&self, area: egui::Rect) -> Vec<Divider> {
        if self.maximized.is_some() {
            return Vec::new();
        }
        let mut dividers = Vec::new();
        self.current().root.dividers(area, &mut Vec::new(), &mut dividers);
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

    /// Handles a press of `primary` at `point` in the workspace.
    ///
    /// A primary press on a border starts a resize and leaves the active view alone; a press
    /// anywhere else over a view makes that view active, whichever button it is. Returns whether
    /// the press started a border drag. Only the active view ever receives a gesture, so a press is
    /// how the user points a gesture at a neighbour (ADR 0079).
    pub(crate) fn press(&mut self, area: egui::Rect, point: egui::Pos2, primary: bool) -> bool {
        if primary && self.begin_drag(area, point) {
            return true;
        }
        if self.border_at(area, point).is_none()
            && let Some((leaf, _)) = self.regions(area).into_iter().find(|(_, rect)| rect.contains(point))
        {
            self.set_active(leaf);
        }
        false
    }

    /// Starts a border drag when `point` grabs one, returning whether it did.
    pub(crate) fn begin_drag(&mut self, area: egui::Rect, point: egui::Pos2) -> bool {
        let Some(divider) = self.dividers(area).into_iter().find(|divider| divider.is_hit(point)) else {
            return false;
        };
        let raw = self.current().root.split_at(&divider.path).map(|(_, ratio)| ratio).unwrap_or(divider.ratio);
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
        self.current_mut().root.set_ratio(&path, ratio);
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
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
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
        let first = layout.current().leaves[0].id;
        let second = layout.current().leaves[1].id;
        assert_eq!(layout.active(), first);

        // A press in the right view activates it and grabs no border, whichever button it is.
        assert!(!layout.press(area, egui::pos2(700.0, 300.0), false), "a secondary press activates too");
        assert_eq!(layout.active(), second);
        layout.press(area, egui::pos2(700.0, 300.0), true);
        assert_eq!(layout.active(), second, "a primary press keeps the view it activates");
        assert_eq!(layout.active(), second);

        // A primary press on the shared edge grabs the border and leaves the active view alone.
        assert!(layout.press(area, egui::pos2(400.0, 300.0), true));
        assert_eq!(layout.active(), second, "a resize does not change the active view");
        layout.end_drag();

        // A secondary press on a border does neither: it is not a resize and not an activation.
        layout.set_active(first);
        assert!(!layout.press(area, egui::pos2(400.0, 60.0), false));
        assert_eq!(layout.active(), first, "a border swallows a press instead of activating a view");

        // The band is wider than the line, so a press a few points away still catches it.
        assert!(layout.press(area, egui::pos2(403.0, 40.0), true));
        layout.end_drag();
        assert!(!layout.press(area, egui::pos2(420.0, 40.0), true));
    }

    #[test]
    fn a_gesture_only_reaches_the_active_view_until_a_press_moves_the_target() {
        let area = area();
        let mut layout = layout(LayoutKind::Quad);
        let regions = layout.regions(area);
        let first = regions[0].0;
        let (second, second_rect) = regions[1];
        assert_eq!(layout.active(), first);
        let neighbour = layout.leaf(second).expect("second").camera();

        // A navigation gesture is written through the active leaf, so an inactive view never moves.
        layout.active_leaf_mut().orbit(0.4, 0.2);
        assert_eq!(layout.leaf(second).expect("second").camera(), neighbour, "the neighbour is untouched");
        assert_ne!(layout.active_leaf().camera(), neighbour, "the active view moved");

        // A press over the neighbour makes it the target; the gesture now moves it and not the first.
        assert!(!layout.press(area, second_rect.center(), true));
        assert_eq!(layout.active(), second);
        let first_camera = layout.leaf(first).expect("first").camera();
        layout.active_leaf_mut().orbit(-0.4, -0.2);
        assert_ne!(layout.leaf(second).expect("second").camera(), neighbour);
        assert_eq!(layout.leaf(first).expect("first").camera(), first_camera, "the press moved the target");
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
    fn a_fit_puts_a_view_on_the_scene_and_keeps_its_direction_and_projection() {
        let fit = Camera::new(furnicraft_geom::DVec3::new(10.0, 20.0, 30.0), 900.0).expect("camera");
        let mut layout = Layout::new(fit, OpeningView::Axonometric, ShadingStyle::default());
        layout.set_kind(LayoutKind::TwoColumns);
        let ids: Vec<LeafId> = layout.current().leaves.iter().map(|leaf| leaf.id).collect();
        let first = layout.leaf_mut(ids[0]).expect("first");
        first.apply_preset(OpeningView::Front);
        let second = layout.leaf_mut(ids[1]).expect("second");
        second.look_from(furnicraft_geom::DVec3::new(1.0, 1.0, 1.0));

        layout.leaf_mut(ids[0]).expect("first").frame(fit);
        layout.leaf_mut(ids[1]).expect("second").frame(fit);

        let first = layout.leaf(ids[0]).expect("first");
        assert_eq!(first.camera.target(), fit.target(), "the fit puts every view on the scene");
        assert!((first.camera.distance() - fit.distance()).abs() < 1.0e-9);
        assert_eq!(first.preset, Some(OpeningView::Front), "a view in a preset keeps it");
        assert!(first.camera.forward().dot(-furnicraft_geom::DVec3::Y) > 0.999);
        assert!(
            matches!(first.camera.projection(), furnicraft_scene::Projection::Orthographic { .. }),
            "a standard view stays orthographic"
        );

        let second = layout.leaf(ids[1]).expect("second");
        assert_eq!(second.camera.target(), fit.target());
        assert_eq!(second.preset, None, "an orbited view stays off every preset");
        let direction = furnicraft_geom::DVec3::new(1.0, 1.0, 1.0).normalize();
        assert!(second.camera.eye().dot(direction) > 0.0, "and keeps the direction it looked from");
        assert!(
            matches!(second.camera.projection(), furnicraft_scene::Projection::Perspective { .. }),
            "a free perspective view stays perspective"
        );
    }

    #[test]
    fn a_fit_keeps_an_orthographic_view_orthographic_at_the_fit_height() {
        let fit = Camera::new(furnicraft_geom::DVec3::new(10.0, 20.0, 30.0), 900.0).expect("camera");
        let mut layout = Layout::new(fit, OpeningView::Axonometric, ShadingStyle::default());
        let id = layout.active();
        let leaf = layout.leaf_mut(id).expect("the active leaf");
        // A free direction, then an orthographic projection: neither belongs to a preset.
        leaf.look_from(furnicraft_geom::DVec3::new(1.0, 2.0, 3.0));
        leaf.toggle_projection();
        assert_eq!(leaf.preset, None);

        let larger = Camera::new(furnicraft_geom::DVec3::new(10.0, 20.0, 30.0), 1800.0).expect("camera");
        leaf.frame(larger);

        assert_eq!(leaf.camera.target(), larger.target(), "the fit target is the scene centre");
        assert!((leaf.camera.distance() - larger.distance()).abs() < 1.0e-9, "the fit distance is the scene fit");
        let furnicraft_scene::Projection::Orthographic { height } = leaf.camera().projection() else {
            panic!("an orthographic view stays orthographic");
        };
        let expected = 2.0 * larger.distance() * (furnicraft_scene::DEFAULT_FOV_Y * 0.5).tan();
        assert!((height - expected).abs() < 1.0e-9, "the orthographic height is the fit height");
        let direction = furnicraft_geom::DVec3::new(1.0, 2.0, 3.0).normalize();
        assert!(leaf.camera().eye().dot(direction) > 0.0, "and keeps its free direction");
    }

    #[test]
    fn a_fit_keeps_a_standard_view_the_user_made_perspective() {
        let fit = Camera::new(furnicraft_geom::DVec3::new(10.0, 20.0, 30.0), 900.0).expect("camera");
        let mut layout = Layout::new(fit, OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        leaf.apply_preset(OpeningView::Front);
        leaf.toggle_projection();
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Perspective { .. }));

        leaf.frame(fit);

        assert!(
            matches!(leaf.camera().projection(), furnicraft_scene::Projection::Perspective { .. }),
            "the user's projection choice survives the fit"
        );
        assert!(leaf.camera().forward().dot(-furnicraft_geom::DVec3::Y) > 0.999, "and the preset direction stays");
        assert!((leaf.camera().distance() - fit.distance()).abs() < 1.0e-9);
    }

    #[test]
    fn an_expanded_view_fills_the_area_and_restore_returns_the_same_tree() {
        // Expanding is a change of rectangles, not of the tree: every split ratio, camera and preset
        // survives the round trip, so an accidental expand costs the user nothing (ADR 0079).
        let area = area();
        let mut layout = layout(LayoutKind::Quad);
        let active = layout.current().leaves[1].id;
        layout.set_active(active);
        layout.leaf_mut(active).expect("the active leaf").look_from(furnicraft_geom::DVec3::new(1.0, 2.0, 3.0));
        assert!(layout.press(area, egui::pos2(400.0, 60.0), true), "the central border is grabbed");
        layout.drag_by(area, egui::vec2(40.0, 0.0));
        layout.end_drag();
        let regions = layout.regions(area);
        let dividers = layout.dividers(area);
        let cameras: Vec<Camera> = layout.current().leaves.iter().map(Leaf::camera).collect();

        layout.toggle_maximized();
        assert!(layout.maximized());
        assert_eq!(layout.regions(area), vec![(active, area)], "the active view owns the whole area");
        assert!(layout.dividers(area).is_empty(), "an expanded view has no borders");
        assert_eq!(layout.border_at(area, egui::pos2(400.0, 60.0)), None, "nothing is left to grab");

        layout.toggle_maximized();
        assert!(!layout.maximized());
        assert_eq!(layout.regions(area), regions, "the same four rectangles return");
        assert_eq!(layout.dividers(area), dividers, "with the same ratios");
        let restored: Vec<Camera> = layout.current().leaves.iter().map(Leaf::camera).collect();
        assert_eq!(restored, cameras, "and every camera stays where the user left it");
    }

    #[test]
    fn a_layout_switch_leaves_the_expanded_mode() {
        let area = area();
        let mut layout = layout(LayoutKind::Quad);
        layout.toggle_maximized();
        assert_eq!(layout.regions(area).len(), 1);

        layout.set_kind(LayoutKind::TwoColumns);
        assert!(!layout.maximized(), "the mode belongs to the layout it was entered in");
        assert_eq!(layout.regions(area).len(), 2, "the new layout shows its own tree");
    }

    #[test]
    fn a_single_view_refuses_to_expand() {
        let area = area();
        let mut layout = layout(LayoutKind::Single);
        layout.toggle_maximized();
        assert!(!layout.maximized(), "one view is already the whole workspace");
        assert_eq!(layout.regions(area), vec![(layout.active(), area)], "the refusal keeps the single view");
    }

    #[test]
    fn a_split_and_a_join_round_trip() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let single = layout.active();
        assert_eq!(layout.current().leaves.len(), 1);

        // A split has its own leaves; the single view is still there when its layout returns.
        layout.set_kind(LayoutKind::TwoColumns);
        assert_eq!(layout.current().leaves.len(), 2);
        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.current().leaves.len(), 1, "the single layout still holds one view");
        assert_eq!(layout.active(), single);
        assert_eq!(layout.preset(single), Some(OpeningView::Axonometric), "and its own preset");

        layout.set_kind(LayoutKind::Quad);
        assert_eq!(layout.current().leaves.len(), 4);
        assert_eq!(layout.regions(area()).len(), 4);
        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.regions(area()).len(), 1, "the single view fills the area again");
    }

    #[test]
    fn every_split_layout_opens_with_its_standard_views() {
        // A layout the user has never arranged is still useful: the standard presets are the views
        // the names promise, the five standard views are orthographic, and the axonometric one keeps
        // the camera's perspective (U59, ADR 0079).
        let expected: [(LayoutKind, &[(OpeningView, bool)]); 3] = [
            (LayoutKind::TwoColumns, &[(OpeningView::Front, true), (OpeningView::Axonometric, false)]),
            (LayoutKind::TwoRows, &[(OpeningView::Front, true), (OpeningView::Axonometric, false)]),
            (
                LayoutKind::Quad,
                &[
                    (OpeningView::Front, true),
                    (OpeningView::Top, true),
                    (OpeningView::Left, true),
                    (OpeningView::Axonometric, false),
                ],
            ),
        ];
        for (kind, views) in expected {
            let layout = layout(kind);
            assert_eq!(layout.current().leaves.len(), views.len(), "{kind:?} has its number of views");
            for (index, (preset, orthographic)) in views.iter().enumerate() {
                let leaf = &layout.current().leaves[index];
                assert_eq!(layout.preset(leaf.id), Some(*preset), "{kind:?} view {index} opens on {preset:?}");
                let ortho = matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. });
                assert_eq!(ortho, *orthographic, "{kind:?} view {index} has the right projection");
                let [x, y, z] = preset.direction();
                let direction = -furnicraft_geom::DVec3::new(x, y, z).normalize();
                assert!(leaf.camera().forward().dot(direction) > 0.999, "{kind:?} view {index} looks from {preset:?}");
            }
        }

        // The single view is the settings opening view, not a fixed preset.
        let mut single = Layout::new(Camera::default(), OpeningView::Top, ShadingStyle::default());
        assert_eq!(single.preset(single.active()), Some(OpeningView::Top));
        assert!(matches!(
            single.active_leaf().camera().projection(),
            furnicraft_scene::Projection::Orthographic { .. }
        ));
        single.set_kind(LayoutKind::Quad);
        assert_eq!(single.preset(single.active()), Some(OpeningView::Front), "a split has its own presets");
    }

    #[test]
    fn a_layout_round_trip_restores_camera_preset_ratio_and_active() {
        let area = area();
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        layout.set_kind(LayoutKind::Quad);
        let regions = layout.regions(area);
        let second = regions[1].0;

        // Arrange the four-view layout: a free camera, a non-default active view and a split ratio.
        layout.leaf_mut(second).expect("second").look_from(furnicraft_geom::DVec3::new(1.0, 2.0, 3.0));
        layout.set_active(second);
        assert!(layout.press(area, egui::pos2(400.0, 60.0), true), "the central border is grabbed");
        layout.drag_by(area, egui::vec2(60.0, 0.0));
        layout.end_drag();
        let arranged = layout.regions(area);
        let camera = layout.leaf(second).expect("second").camera();
        let width = arranged[1].1.width();
        assert!((width - 400.0).abs() > 1.0, "the drag changed the ratio");

        // Leaving and returning restores the tree, its ratio, the camera and the active view.
        layout.set_kind(LayoutKind::Single);
        layout.set_kind(LayoutKind::Quad);
        assert_eq!(layout.active(), second, "the active view survives the round trip");
        assert_eq!(layout.leaf(second).expect("second").camera(), camera, "the camera survives");
        assert_eq!(layout.preset(second), None, "and its preset state");
        let restored = layout.regions(area);
        assert!((restored[1].1.width() - width).abs() < 1.0e-4, "the split ratio survives");
        assert_eq!(layout.preset(restored[3].0), Some(OpeningView::Axonometric), "the presets survive");
    }

    #[test]
    fn two_views_keep_their_own_display_style_and_only_the_active_one_changes() {
        // The display style is view state per leaf: a neighbour may be wireframe while the active
        // view is a ghost, and a style button writes the active view alone (U63).
        let mut layout = layout(LayoutKind::TwoColumns);
        let ids: Vec<LeafId> = layout.current().leaves.iter().map(|leaf| leaf.id).collect();
        layout.leaf_mut(ids[0]).expect("first").set_style(ShadingStyle::Wireframe);
        layout.leaf_mut(ids[1]).expect("second").set_style(ShadingStyle::Translucent);
        assert_eq!(layout.leaf(ids[0]).expect("first").style(), ShadingStyle::Wireframe);
        assert_eq!(layout.leaf(ids[1]).expect("second").style(), ShadingStyle::Translucent);

        layout.set_active(ids[1]);
        assert_eq!(layout.active_style(), ShadingStyle::Translucent, "the cluster reads the active leaf");
        layout.set_active_style(ShadingStyle::Shaded);
        assert_eq!(layout.leaf(ids[1]).expect("second").style(), ShadingStyle::Shaded);
        assert_eq!(
            layout.leaf(ids[0]).expect("first").style(),
            ShadingStyle::Wireframe,
            "a style change never reaches the neighbour"
        );

        // The styles travel with the rest of the layout state.
        layout.set_kind(LayoutKind::Single);
        layout.set_kind(LayoutKind::TwoColumns);
        assert_eq!(layout.leaf(ids[0]).expect("first").style(), ShadingStyle::Wireframe);
        assert_eq!(layout.leaf(ids[1]).expect("second").style(), ShadingStyle::Shaded);
    }

    #[test]
    fn a_frame_fallback_follows_the_active_leaf_while_a_neighbour_keeps_its_style() {
        // `SceneFrame::shading` is the single-view fallback; a split workspace passes a style per
        // view, so the fallback follows whichever view the cluster is editing (U63).
        let mut layout = layout(LayoutKind::TwoColumns);
        let ids: Vec<LeafId> = layout.current().leaves.iter().map(|leaf| leaf.id).collect();
        layout.leaf_mut(ids[0]).expect("first").set_style(ShadingStyle::Wireframe);
        layout.leaf_mut(ids[1]).expect("second").set_style(ShadingStyle::Translucent);

        let mut frame = SceneFrame::default();
        layout.set_active(ids[0]);
        layout.apply_frame_style(&mut frame);
        assert_eq!(frame.shading, ShadingStyle::Wireframe);

        layout.set_active(ids[1]);
        layout.apply_frame_style(&mut frame);
        assert_eq!(frame.shading, ShadingStyle::Translucent, "the fallback follows the active leaf");
        assert_eq!(layout.leaf(ids[0]).expect("first").style(), ShadingStyle::Wireframe, "the neighbour is untouched");
    }

    #[test]
    fn a_fresh_workspace_takes_the_settings_style_in_every_layout() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::Wireframe);
        for kind in LayoutKind::ALL {
            layout.set_kind(kind);
            for leaf in &layout.current().leaves {
                assert_eq!(leaf.style(), ShadingStyle::Wireframe, "{kind:?} opens on the settings style");
            }
        }
        assert_eq!(layout.active_style(), ShadingStyle::Wireframe, "the cluster reads the same default");
    }

    #[test]
    fn every_layout_keeps_its_own_display_style() {
        // A style belongs to a leaf, so a layout the user arranged keeps its look when another one is
        // shown — a tab opens on the settings default and never rewrites the others (ADR 0079, U45).
        let mut layout = layout(LayoutKind::Quad);
        layout.leaf_mut(layout.current().leaves[0].id).expect("first").set_style(ShadingStyle::Wireframe);
        layout.set_kind(LayoutKind::Single);
        layout.set_active_style(ShadingStyle::Translucent);

        assert_eq!(layout.active_style(), ShadingStyle::Translucent, "the shown layout keeps its style");
        layout.set_kind(LayoutKind::Quad);
        assert_eq!(
            layout.current().leaves[0].style(),
            ShadingStyle::Wireframe,
            "a layout the user arranged keeps its styles too"
        );
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
        let layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.current().leaves[0].clone();
        let target = leaf.camera().target();
        let distance = leaf.camera().distance();

        // The owner's eye convention: the eye offset is the camera's `-forward`.
        for (preset, forward, right, up) in [
            (OpeningView::Front, -furnicraft_geom::DVec3::Y, -furnicraft_geom::DVec3::X, furnicraft_geom::DVec3::Z),
            (OpeningView::Left, -furnicraft_geom::DVec3::X, furnicraft_geom::DVec3::Y, furnicraft_geom::DVec3::Z),
            (OpeningView::Right, furnicraft_geom::DVec3::X, -furnicraft_geom::DVec3::Y, furnicraft_geom::DVec3::Z),
            (OpeningView::Top, -furnicraft_geom::DVec3::Z, furnicraft_geom::DVec3::X, furnicraft_geom::DVec3::Y),
            (OpeningView::Bottom, furnicraft_geom::DVec3::Z, -furnicraft_geom::DVec3::X, furnicraft_geom::DVec3::Y),
        ] {
            let mut leaf = leaf.clone();
            leaf.apply_preset(preset);
            let camera = leaf.camera();
            assert_eq!(camera.target(), target, "{preset:?} keeps the target");
            assert!((camera.distance() - distance).abs() < 1.0e-9, "{preset:?} keeps the distance");
            assert!(camera.forward().dot(forward) > 0.999, "{preset:?} looks along {forward:?}");
            assert!(camera.right().dot(right) > 0.999, "{preset:?} has {right:?} to the right");
            assert!(camera.up().dot(up) > 0.999, "{preset:?} has {up:?} up");
            assert!(matches!(camera.projection(), furnicraft_scene::Projection::Orthographic { .. }));
            assert_eq!(leaf.preset, Some(preset));
        }

        // The axonometric default is the only preset that is not a standard view.
        let mut axonometric = leaf.clone();
        axonometric.apply_preset(OpeningView::Axonometric);
        assert!(matches!(axonometric.camera().projection(), furnicraft_scene::Projection::Perspective { .. }));
        assert_eq!(axonometric.preset, Some(OpeningView::Axonometric));
    }

    #[test]
    fn a_pole_view_takes_the_canonical_up_from_the_widget_and_the_presets() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let id = layout.active();
        let leaf = layout.leaf_mut(id).expect("the active leaf");
        // A yaw the pole must not inherit; the target and the distance are what a snap keeps.
        leaf.orbit(1.3, 0.0);
        let target = leaf.camera().target();
        let distance = leaf.camera().distance();

        // The top preset and the widget's `+Z` ball land on the same basis: +Y up, +X right.
        leaf.apply_preset(OpeningView::Top);
        let preset = leaf.camera();
        leaf.look_from(furnicraft_geom::DVec3::Z);
        let widget = leaf.camera();
        assert_eq!(preset, widget, "a preset and the widget agree on the top view");
        assert_eq!(widget.target(), target, "the snap does not move the target");
        assert!((widget.distance() - distance).abs() < 1.0e-9, "nor the distance");
        assert!(widget.up().dot(furnicraft_geom::DVec3::Y) > 0.999, "the top view is +Y up");
        assert!(widget.right().dot(furnicraft_geom::DVec3::X) > 0.999, "and +X right");

        // The widget's bottom ball is +Y up and -X right.
        leaf.look_from(-furnicraft_geom::DVec3::Z);
        let bottom = leaf.camera();
        assert_eq!(bottom.target(), target);
        assert!(bottom.up().dot(furnicraft_geom::DVec3::Y) > 0.999, "the bottom view is +Y up");
        assert!(bottom.right().dot(-furnicraft_geom::DVec3::X) > 0.999, "and -X right");
    }

    #[test]
    fn a_side_view_keeps_z_up() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        for preset in [OpeningView::Front, OpeningView::Right] {
            leaf.apply_preset(preset);
            assert!(
                leaf.camera().up().dot(furnicraft_geom::DVec3::Z) > 0.999,
                "{preset:?} keeps +Z up, got {:?}",
                leaf.camera().up()
            );
        }
    }

    #[test]
    fn the_home_button_restores_the_axonometric_preset() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
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
    fn a_standard_preset_and_an_axis_snap_are_orthographic() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Perspective { .. }));

        // A standard preset converts the perspective view without moving its framing.
        let target = leaf.camera().target();
        let distance = leaf.camera().distance();
        leaf.apply_preset(OpeningView::Left);
        let furnicraft_scene::Projection::Orthographic { height } = leaf.camera().projection() else {
            panic!("a standard preset is orthographic");
        };
        assert!((height - 2.0 * distance * (furnicraft_scene::DEFAULT_FOV_Y * 0.5).tan()).abs() < 1.0e-9);
        assert_eq!(leaf.camera().target(), target);
        assert!((leaf.camera().distance() - distance).abs() < 1.0e-9);

        // The corner widget's axis balls are standard views, so they do the same.
        leaf.look_from(furnicraft_geom::DVec3::X);
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));
        assert_eq!(leaf.preset, None, "a ball snap is a direction, not a named preset");
    }

    #[test]
    fn an_axonometric_preset_and_a_free_direction_keep_the_projection() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        leaf.toggle_projection();
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));

        leaf.apply_preset(OpeningView::Axonometric);
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));

        // Return to perspective and take a free direction: no axis, no projection change.
        let mut perspective = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = perspective.leaf_mut(perspective.active()).expect("the active leaf");
        leaf.look_from(furnicraft_geom::DVec3::new(1.0, 1.0, 1.0));
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Perspective { .. }));
    }

    #[test]
    fn the_projection_toggle_works_in_a_standard_view_and_keeps_its_name() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let leaf = layout.leaf_mut(layout.active()).expect("the active leaf");
        leaf.apply_preset(OpeningView::Top);
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));

        leaf.toggle_projection();
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Perspective { .. }));
        assert_eq!(leaf.preset, Some(OpeningView::Top), "the toggle is not a change of direction");

        leaf.toggle_projection();
        assert!(matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. }));
        assert_eq!(leaf.preset, Some(OpeningView::Top), "the preset name survives both ways");
    }

    #[test]
    fn a_projection_toggle_stays_in_its_own_view() {
        // The ribbon, the Display popover and the corner widget all toggle one leaf; a split must
        // keep its neighbours in the mode they were drawing with.
        let mut layout = layout(LayoutKind::Quad);
        let first = layout.current().leaves[0].id;
        assert!(matches!(
            layout.leaf(first).expect("the leaf").camera().projection(),
            furnicraft_scene::Projection::Orthographic { .. }
        ));
        layout.leaf_mut(first).expect("the leaf").toggle_projection();
        for (index, leaf) in layout.current().leaves.iter().enumerate() {
            let ortho = matches!(leaf.camera().projection(), furnicraft_scene::Projection::Orthographic { .. });
            assert_eq!(
                ortho,
                index != 0 && index != 3,
                "only the toggled view changes its projection; the standard views keep theirs"
            );
        }
    }

    #[test]
    fn a_roll_is_view_state_and_a_preset_or_home_resets_it() {
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        let id = layout.active();
        let leaf = layout.leaf_mut(id).expect("the active leaf");
        leaf.apply_preset(OpeningView::Front);
        let forward = leaf.camera().forward();

        leaf.roll_to(std::f64::consts::FRAC_PI_2);
        assert!((leaf.camera().roll() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-9);
        assert_eq!(leaf.preset, None, "a rolled view is no longer the preset's basis");
        assert!((leaf.camera().forward() - forward).length() < 1.0e-9, "the roll does not turn the view");

        // Orbit keeps the roll; Home, a preset and Frame all reset it through their direction snap.
        leaf.orbit(0.0, 0.0);
        assert!(leaf.camera().roll() > 0.0, "orbit keeps the roll");
        leaf.home();
        assert_eq!(leaf.camera().roll(), 0.0, "Home resets the roll");
        leaf.roll_by(0.3);
        assert!(leaf.camera().roll() > 0.0, "a roll can be applied again");
        leaf.apply_preset(OpeningView::Top);
        assert_eq!(leaf.camera().roll(), 0.0, "a preset resets the roll");
        leaf.roll_to(0.4);
        leaf.frame(Camera::default());
        assert_eq!(leaf.camera().roll(), 0.0, "Frame all resets the roll");
    }

    #[test]
    fn the_active_leaf_is_the_one_the_tree_holds() {
        let mut layout = layout(LayoutKind::Quad);
        let last = layout.current().leaves[3].id;
        layout.set_active(last);
        assert_eq!(layout.active(), last);
        assert_eq!(layout.active_leaf().id, last);

        // Each layout keeps its own active view; switching away and back restores the one picked.
        layout.set_kind(LayoutKind::Single);
        assert_eq!(layout.active(), layout.current().leaves[0].id, "the single layout has its own active view");
        layout.set_kind(LayoutKind::Quad);
        assert_eq!(layout.active(), last, "the four-view layout remembered it");
        layout.set_active(LeafId(999));
        assert_eq!(layout.active(), last, "an unknown id is ignored");
    }
}
