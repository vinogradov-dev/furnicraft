//! The floating viewport cluster: navigation, framing, display style, layout and display settings.
//!
//! The cluster floats over the scene instead of living in a bar, so the viewport starts at the model
//! and not at a wall of labels (U40). It is anchored to the bottom centre of the viewport it belongs
//! to and follows that view when a side panel folds or the workspace splits; nothing here is
//! document data. The shell owns the cameras, the gizmos and the drawing tools, so the cluster only
//! reports the actions it cannot perform itself — framing, the projection toggle, the layout choice
//! and the display style, which belongs to the active leaf. Its own settings and the armed
//! navigation mode are view state on [`ViewCluster`].

use eframe::egui;
use furnicraft_scene::Projection;
use furnicraft_ui::display::{OpeningView, ShadingStyle, shading_caption, shading_icon, shading_label};
use furnicraft_ui::{icons, theme};

use crate::view_layout::{LayoutKind, MAX_LEAVES};

/// The navigation gesture a primary-button drag performs while it is armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationMode {
    /// Turn the camera about its target.
    Orbit,
    /// Move the camera across the view plane.
    Pan,
    /// Move the camera along its forward axis.
    Zoom,
}

impl NavigationMode {
    /// Every mode in the order the cluster draws them.
    pub(crate) const ALL: [Self; 3] = [Self::Orbit, Self::Pan, Self::Zoom];

    /// The short name the status bar shows.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Orbit => "Orbit",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }

    /// The gesture the status bar asks for once the mode is armed.
    pub(crate) const fn gesture(self) -> &'static str {
        match self {
            Self::Orbit => "Drag in the viewport to orbit  ·  Esc done",
            Self::Pan => "Drag in the viewport to pan  ·  Esc done",
            Self::Zoom => "Drag vertically to zoom  ·  Esc done",
        }
    }

    /// The tooltip and the accessible name.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Orbit => "Orbit  ·  drag in the viewport",
            Self::Pan => "Pan  ·  drag in the viewport",
            Self::Zoom => "Zoom  ·  drag vertically",
        }
    }

    /// The icon asset that names the gesture once it is drawn.
    pub(crate) const fn icon(self) -> icons::Icon {
        match self {
            Self::Orbit => icons::view::ORBIT,
            Self::Pan => icons::view::PAN,
            Self::Zoom => icons::view::ZOOM,
        }
    }
}

/// The cluster's view state.
///
/// The three strengths are the display-settings sliders. They are view state: the grid, edge and
/// contact-shadow passes read them as those passes land (U43 and U44), and no value ever reaches the
/// scene file. The display style is not here: it belongs to the active leaf, so two views can differ
/// (U63).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewCluster {
    /// The navigation gesture armed for the next primary-button drag, or `None`.
    pub(crate) navigation: Option<NavigationMode>,
    /// Whether the display-settings popover is open.
    pub(crate) settings_open: bool,
    /// Whether the split-view choices are open.
    pub(crate) layout_open: bool,
    /// Whether the orientation widget draws the pitch/yaw/roll and projection readout (U42).
    pub(crate) show_angles: bool,
    /// Whether the edge pass draws every triangle edge instead of only the form edges.
    ///
    /// A diagnostic view state beside the style: it changes which cached edge list the renderer
    /// reads, so a switch re-uploads the edge buffer but never touches the document.
    pub(crate) all_edges: bool,
    /// Grid strength, 0–100.
    pub(crate) grid_strength: f32,
    /// Edge strength, 0–100.
    pub(crate) edge_strength: f32,
    /// Contact-shadow strength, 0–100.
    pub(crate) shadow_strength: f32,
    /// Whether the contact-shadow pass draws (U44).
    pub(crate) contact_shadows: bool,
}

impl Default for ViewCluster {
    fn default() -> Self {
        Self {
            navigation: None,
            settings_open: false,
            layout_open: false,
            show_angles: false,
            all_edges: false,
            grid_strength: 60.0,
            edge_strength: 70.0,
            shadow_strength: 40.0,
            contact_shadows: true,
        }
    }
}

impl ViewCluster {
    /// Whether the orientation widget draws its angle and projection readout.
    pub(crate) fn angles_visible(&self) -> bool {
        self.show_angles
    }

    /// The frame's contact-shadow settings from the cluster's view state.
    ///
    /// A change is read at draw time, so the switch and the strength never re-upload geometry.
    pub(crate) fn shadow_settings(&self) -> furnicraft_scene::ContactShadows {
        furnicraft_scene::ContactShadows {
            visible: self.contact_shadows,
            strength: (self.shadow_strength / 100.0).clamp(0.0, 1.0),
            ..furnicraft_scene::ContactShadows::default()
        }
    }
}

/// What the cluster reads from the shell for one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterView {
    /// Whether the viewport owns its navigation and projection.
    pub(crate) owns_view: bool,
    /// The projection the target view is drawing with.
    pub(crate) projection: Projection,
    /// The work plane the pointer maps onto, for the settings popover.
    pub(crate) work_plane: &'static str,
    /// The layout the workspace is in.
    pub(crate) layout: LayoutKind,
    /// Whether one view fills the whole workspace.
    pub(crate) maximized: bool,
    /// The preset the target view is in, if it has not been orbited since.
    pub(crate) preset: Option<OpeningView>,
    /// The display style of the target view; the style buttons draw it pressed.
    pub(crate) style: ShadingStyle,
}

/// What the cluster asks the shell to do; everything else it changes on its own state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClusterAction {
    /// Frame the whole scene in the target view.
    FrameAll,
    /// Return the target view to the axonometric home view.
    Home,
    /// Switch the target view between perspective and orthographic.
    ToggleProjection,
    /// Divide the workspace into this layout.
    SetLayout(LayoutKind),
    /// Expand the active view over the whole workspace, or return the tree.
    ToggleMaximize,
    /// Point the target view along a standard preset.
    SetPreset(OpeningView),
    /// Draw the target view with this display style.
    SetStyle(ShadingStyle),
}

/// The point the cluster's bottom centre is pinned to: the bottom centre of `viewport`, raised by
/// `offset`.
///
/// The anchor is derived from the viewport rect rather than from the window, so folding a side panel
/// moves the cluster with the surface it floats over instead of leaving it over the new panel.
pub(crate) fn cluster_anchor(viewport: egui::Rect, offset: f32) -> egui::Pos2 {
    egui::pos2(viewport.center().x, viewport.bottom() - offset)
}

/// Draws the cluster over `viewport` and applies its own view-state changes.
pub(crate) fn show(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    state: &mut ViewCluster,
    view: ClusterView,
) -> Option<ClusterAction> {
    let mut action = None;
    let response = egui::Area::new(egui::Id::unique("view_cluster"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(cluster_anchor(viewport, theme::FLOATING_OFFSET))
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(theme::floating(ui))
                .stroke(egui::Stroke::new(1.0, theme::line(ui)))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(6, 4));
            frame.show(ui, |ui| {
                // The cluster floats over the viewport, so it may never grow wider than the surface
                // it covers: the wrapped row folds onto a second line in a narrow window instead of
                // being clipped by the central panel.
                ui.set_max_width((viewport.width() - 2.0 * theme::FLOATING_OFFSET).max(0.0));
                ui.horizontal_wrapped(|ui| {
                    navigation_buttons(ui, state);
                    theme::divider(ui);
                    if icons::tool_button(ui, icons::view::FRAME_ALL, "Frame all", Some("F"), true, false).clicked() {
                        action = Some(ClusterAction::FrameAll);
                    }
                    if icons::tool_button(ui, icons::view::HOME, "Home", None, true, false).clicked() {
                        action = Some(ClusterAction::Home);
                    }
                    theme::divider(ui);
                    if let Some(style) = style_buttons(ui, view.style) {
                        action = Some(ClusterAction::SetStyle(style));
                    }
                    theme::divider(ui);
                    let open = state.layout_open;
                    if icons::tool_button(ui, icons::view::LAYOUT, "Layout", None, true, open).clicked() {
                        state.layout_open = !state.layout_open;
                        state.settings_open = false;
                    }
                    // A single view already fills the workspace, so there is nothing to expand: the
                    // button is drawn disabled instead of entering a mode it cannot leave.
                    let (icon, label) = if view.maximized {
                        (icons::view::RESTORE, "Restore")
                    } else {
                        (icons::view::MAXIMIZE, "Expand")
                    };
                    if icons::tool_button(ui, icon, label, Some("Alt+W"), view.layout.leaves() > 1, view.maximized)
                        .clicked()
                    {
                        action = Some(ClusterAction::ToggleMaximize);
                    }
                    let open = state.settings_open;
                    if icons::tool_button(ui, icons::view::SLIDERS, "Display settings", None, true, open).clicked() {
                        state.settings_open = !state.settings_open;
                        state.layout_open = false;
                    }
                });
            });
        });
    let rect = response.response.rect;

    if state.layout_open {
        if ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            state.layout_open = false;
        } else if let Some(asked) = layout_popover(ui, viewport, rect, view) {
            state.layout_open = false;
            action = Some(asked);
        }
    }
    if state.settings_open {
        if ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            state.settings_open = false;
        } else if let Some(asked) = settings_popover(ui, viewport, rect, state, view) {
            action = Some(asked);
        }
    }
    action
}

/// The orbit, pan and zoom buttons; the armed one draws pressed and a second click disarms it.
fn navigation_buttons(ui: &mut egui::Ui, state: &mut ViewCluster) {
    for mode in NavigationMode::ALL {
        let on = state.navigation == Some(mode);
        if icons::tool_button_labelled(ui, mode.icon(), navigation_caption(mode), mode.label(), true, on).clicked() {
            state.navigation = (!on).then_some(mode);
        }
    }
}

/// The short placeholder face of one navigation mode.
const fn navigation_caption(mode: NavigationMode) -> &'static str {
    match mode {
        NavigationMode::Orbit => "Orbit",
        NavigationMode::Pan => "Pan",
        NavigationMode::Zoom => "Zoom",
    }
}

/// The five display styles; the view's own style draws pressed and a click asks for a change.
fn style_buttons(ui: &mut egui::Ui, current: ShadingStyle) -> Option<ShadingStyle> {
    let mut asked = None;
    for style in ShadingStyle::ALL {
        let on = current == style;
        if icons::tool_button_labelled(ui, shading_icon(style), shading_caption(style), shading_label(style), true, on)
            .clicked()
        {
            asked = Some(style);
        }
    }
    asked
}

/// The split-view choices above the cluster: one view, two vertical, two horizontal and four.
///
/// The active layout draws pressed, so the bar and the workspace agree without a second label. A
/// choice is always present — the button that returns to one view is the same row — so a border is
/// never the only way out of a split.
fn layout_popover(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    cluster: egui::Rect,
    view: ClusterView,
) -> Option<ClusterAction> {
    let mut action = None;
    let anchor = egui::pos2(cluster.center().x, cluster.top() - theme::STEP);
    egui::Area::new(egui::Id::unique("view_cluster_layout"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(anchor)
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(theme::floating(ui))
                .stroke(egui::Stroke::new(1.0, theme::line(ui)))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(10));
            frame.show(ui, |ui| {
                ui.set_width(220.0);
                ui.label(egui::RichText::new("Layout").strong());
                for kind in LayoutKind::ALL {
                    let on = view.layout == kind;
                    if ui.add(egui::Button::new(kind.label()).selected(on)).clicked() {
                        action = Some(ClusterAction::SetLayout(kind));
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("View preset").strong());
                for preset in OpeningView::ALL {
                    if preset_row(ui, preset, view.preset == Some(preset)) {
                        action = Some(ClusterAction::SetPreset(preset));
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{} of {MAX_LEAVES} views · drag a border to resize",
                        view.layout.leaves()
                    ))
                    .small()
                    .color(theme::text_dim(ui)),
                );
            });
        });
    action
}

/// The icon of a standard view: a screen plane with the direction the eye looks from.
pub(crate) fn preset_icon(view: OpeningView) -> icons::Icon {
    match view {
        OpeningView::Axonometric => icons::view::AXON,
        OpeningView::Front => icons::view::FRONT,
        OpeningView::Left => icons::view::LEFT,
        OpeningView::Top => icons::view::TOP,
        OpeningView::Right => icons::view::RIGHT,
        OpeningView::Bottom => icons::view::BOTTOM,
    }
}

/// One row of the preset list: the view's glyph, its name and the selected face.
///
/// A popover row is not a text button: the preset list is the one place where a named view is a
/// glyph the user learns, so the row draws the icon and keeps the name beside it as the label.
fn preset_row(ui: &mut egui::Ui, view: OpeningView, on: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, on);
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(theme::RADIUS),
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let glyph =
            egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::Vec2::splat(16.0));
        icons::draw_icon(ui, preset_icon(view), glyph, 16.0, visuals.fg_stroke.color);
        ui.painter().text(
            egui::pos2(rect.left() + 28.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            view.label(),
            egui::FontId::proportional(13.0),
            visuals.fg_stroke.color,
        );
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, view.label()));
    }
    response.clicked()
}

/// The display-settings popover above the cluster: the angle switch, the projection and three
/// strengths.
fn settings_popover(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    cluster: egui::Rect,
    state: &mut ViewCluster,
    view: ClusterView,
) -> Option<ClusterAction> {
    let mut action = None;
    let anchor = egui::pos2(cluster.center().x, cluster.top() - theme::STEP);
    egui::Area::new(egui::Id::unique("view_cluster_settings"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(anchor)
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(theme::floating(ui))
                .stroke(egui::Stroke::new(1.0, theme::line(ui)))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(10));
            frame.show(ui, |ui| {
                ui.set_width(230.0);
                ui.label(egui::RichText::new("Display").strong());
                let angles = ui.checkbox(&mut state.show_angles, "Show projection and angles");
                angles.on_hover_text(if state.angles_visible() {
                    "The orientation readout is drawn under the cube"
                } else {
                    "Draw the pitch, yaw, roll and the projection under the cube"
                });
                let (icon, hint) = projection_button(view.projection);
                let toggle = icons::tool_button(ui, icon, hint, Some("O"), view.owns_view, false);
                if toggle.on_disabled_hover_text("An authored camera owns the projection").clicked() {
                    action = Some(ClusterAction::ToggleProjection);
                }
                ui.checkbox(&mut state.all_edges, "Show every triangle edge")
                    .on_hover_text("Diagnostic: keeps the triangulation diagonals the form-edge view hides.");
                ui.separator();
                ui.checkbox(&mut state.contact_shadows, "Contact shadows")
                    .on_hover_text("A soft shadow under every object that rests on the floor.");
                for (value, label) in [
                    (&mut state.grid_strength, "Grid"),
                    (&mut state.edge_strength, "Edges"),
                    (&mut state.shadow_strength, "Shadow"),
                ] {
                    ui.add(egui::Slider::new(value, 0.0..=100.0).text(label).fixed_decimals(0));
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Work plane · {}", view.work_plane)).small().color(theme::text_dim(ui)),
                );
            });
        });
    action
}

/// The icon and the hint of the projection toggle.
///
/// The icon draws the mode the view is *in*, and the hint names the switch, so the control never
/// claims a state the camera has not reached.
pub(crate) fn projection_button(projection: Projection) -> (icons::Icon, &'static str) {
    match projection {
        Projection::Perspective { .. } => (icons::view::PERSPECTIVE, "Perspective view; switch to orthographic  (O)"),
        Projection::Orthographic { .. } => (icons::view::ORTHOGRAPHIC, "Orthographic view; switch to perspective  (O)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_preset_has_its_own_drawn_glyph() {
        let mut seen = std::collections::HashSet::new();
        for view in OpeningView::ALL {
            let icon = preset_icon(view);
            assert!(icon.is_drawn(), "{view:?} still draws a placeholder");
            assert!(seen.insert(icon.uri()), "{view:?} shares its glyph with another preset");
        }
        assert_eq!(seen.len(), OpeningView::ALL.len(), "the six presets own six glyphs");
    }
}
