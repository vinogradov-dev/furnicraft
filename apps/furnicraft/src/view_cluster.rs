//! The floating viewport cluster: navigation, framing, display style and display settings.
//!
//! The cluster floats over the scene instead of living in a bar, so the viewport starts at the model
//! and not at a wall of labels (U40). It is anchored to the bottom centre of the central panel and
//! follows that panel when a side panel folds or resizes; nothing here is document data. The shell
//! owns the camera, the gizmos and the drawing tools, so the cluster only reports the actions it
//! cannot perform itself — framing, the projection toggle and the layout entry. Its own settings,
//! the armed navigation mode and the display style are view state on [`ViewCluster`].

use eframe::egui;
use furnicraft_scene::Projection;
use furnicraft_ui::display::ShadingStyle;
use furnicraft_ui::{icons, theme};

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
/// The three strengths are the display-settings sliders. They are view state like the style: the
/// grid, edge and contact-shadow passes read them as those passes land (U43 and U44), and no value
/// ever reaches the scene file.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewCluster {
    /// The navigation gesture armed for the next primary-button drag, or `None`.
    pub(crate) navigation: Option<NavigationMode>,
    /// The display style the viewport draws with.
    pub(crate) style: ShadingStyle,
    /// Whether panel and silhouette edges are drawn (U43 uses the value).
    pub(crate) edges: bool,
    /// Whether the display-settings popover is open.
    pub(crate) settings_open: bool,
    /// Whether the orientation widget draws the pitch/yaw/roll and projection readout (U42).
    pub(crate) show_angles: bool,
    /// Grid strength, 0–100.
    pub(crate) grid_strength: f32,
    /// Edge strength, 0–100.
    pub(crate) edge_strength: f32,
    /// Contact-shadow strength, 0–100.
    pub(crate) shadow_strength: f32,
}

impl Default for ViewCluster {
    fn default() -> Self {
        Self {
            navigation: None,
            style: ShadingStyle::default(),
            edges: true,
            settings_open: false,
            show_angles: false,
            grid_strength: 60.0,
            edge_strength: 70.0,
            shadow_strength: 40.0,
        }
    }
}

impl ViewCluster {
    /// Whether the orientation widget draws its angle and projection readout.
    pub(crate) fn angles_visible(&self) -> bool {
        self.show_angles
    }
}

/// What the cluster reads from the shell for one frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterView {
    /// Whether the viewport owns its navigation and projection.
    pub(crate) owns_view: bool,
    /// The projection the viewport is drawing with.
    pub(crate) projection: Projection,
    /// The work plane the pointer maps onto, for the settings popover.
    pub(crate) work_plane: &'static str,
}

/// What the cluster asks the shell to do; everything else it changes on its own state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClusterAction {
    /// Frame the whole scene.
    FrameAll,
    /// Return to the home view.
    Home,
    /// Switch between perspective and orthographic.
    ToggleProjection,
    /// Open the split-view layout (U42).
    Layout,
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
                    style_buttons(ui, state);
                    theme::divider(ui);
                    if icons::tool_button(ui, icons::view::LAYOUT, "Layout", None, true, false).clicked() {
                        action = Some(ClusterAction::Layout);
                    }
                    let open = state.settings_open;
                    if icons::tool_button(ui, icons::view::SLIDERS, "Display settings", None, true, open).clicked() {
                        state.settings_open = !state.settings_open;
                    }
                });
            });
        });
    let rect = response.response.rect;

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

/// The five display styles; the active one draws pressed.
fn style_buttons(ui: &mut egui::Ui, state: &mut ViewCluster) {
    for style in ShadingStyle::ALL {
        let on = state.style == style;
        if icons::tool_button_labelled(ui, style.icon(), style.caption(), style.label(), true, on).clicked() {
            state.style = style;
        }
    }
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
                    "The orientation readout is drawn under the cube (U42)"
                } else {
                    "Draw the pitch, yaw, roll and the projection under the cube (U42)"
                });
                let (icon, hint) = projection_button(view.projection);
                let toggle = icons::tool_button(ui, icon, hint, Some("O"), view.owns_view, false);
                if toggle.on_disabled_hover_text("An authored camera owns the projection").clicked() {
                    action = Some(ClusterAction::ToggleProjection);
                }
                ui.separator();
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
