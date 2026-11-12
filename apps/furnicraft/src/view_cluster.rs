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
    pub(crate) const fn name(self) -> furnicraft_text::Message {
        match self {
            Self::Orbit => furnicraft_text::Message::ShortcutOrbit,
            Self::Pan => furnicraft_text::Message::ShortcutPan,
            Self::Zoom => furnicraft_text::Message::ShortcutZoom,
        }
    }

    /// The gesture the status bar asks for once the mode is armed.
    pub(crate) const fn gesture(self) -> furnicraft_text::Message {
        match self {
            Self::Orbit => furnicraft_text::Message::GestureOrbit,
            Self::Pan => furnicraft_text::Message::GesturePan,
            Self::Zoom => furnicraft_text::Message::GestureZoom,
        }
    }

    /// The tooltip and the accessible name.
    pub(crate) const fn label(self) -> furnicraft_text::Message {
        match self {
            Self::Orbit => furnicraft_text::Message::NavOrbitLabel,
            Self::Pan => furnicraft_text::Message::NavPanLabel,
            Self::Zoom => furnicraft_text::Message::NavZoomLabel,
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
/// The cluster keeps the switches that change what the view draws from one frame to the next; the
/// strengths of the floor grid, the outlines and the contact shadow are workspace defaults and live
/// in `furnicraft_ui::settings::ViewportSettings`, which the cluster links to instead of drawing
/// three sliders over the scene (U64). The display style is not here either: it belongs to the
/// active leaf, so two views can differ (U63).
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
    /// Whether the contact-shadow pass draws (U44).
    pub(crate) contact_shadows: bool,
}

impl Default for ViewCluster {
    fn default() -> Self {
        Self { navigation: None, settings_open: false, layout_open: false, show_angles: false, contact_shadows: true }
    }
}

impl ViewCluster {
    /// Whether the orientation widget draws its angle and projection readout.
    pub(crate) fn angles_visible(&self) -> bool {
        self.show_angles
    }

    /// The frame's contact-shadow settings from the cluster's switch and the workspace strength.
    ///
    /// A change is read at draw time, so the switch and the strength never re-upload geometry.
    pub(crate) fn shadow_settings(&self, strength_percent: u32) -> furnicraft_scene::ContactShadows {
        furnicraft_scene::ContactShadows {
            visible: self.contact_shadows,
            // A percentage read through an integer keeps the domain free of floats; the clamp
            // guards a hand-edited settings file that skipped `sanitize`.
            strength: (strength_percent.min(100) as f32 / 100.0).clamp(0.0, 1.0),
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
    /// Open Settings on the *View* page, where the display strengths live.
    OpenSettings,
}

/// The point the cluster's bottom centre is pinned to: the bottom centre of `viewport`, raised by
/// `offset`.
///
/// The anchor is derived from the viewport rect rather than from the window, so folding a side panel
/// moves the cluster with the surface it floats over instead of leaving it over the new panel.
pub(crate) fn cluster_anchor(viewport: egui::Rect, offset: f32) -> egui::Pos2 {
    egui::pos2(viewport.center().x, viewport.bottom() - offset)
}

/// The fill of the floating cluster and its popovers at the shared chrome opacity.
///
/// The cluster sits over the scene and must stay readable even when the panels are set almost
/// transparent, so its own alpha never drops below 80 %.
pub(crate) fn cluster_fill(ui: &egui::Ui, opacity: u32) -> egui::Color32 {
    theme::at_opacity(theme::floating(ui), opacity.clamp(80, 100))
}

/// Draws the cluster over `viewport` and applies its own view-state changes.
pub(crate) fn show(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    state: &mut ViewCluster,
    view: ClusterView,
    opacity: u32,
) -> Option<ClusterAction> {
    let mut action = None;
    let response = egui::Area::new(egui::Id::unique("view_cluster"))
        .order(egui::Order::Middle)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(cluster_anchor(viewport, theme::FLOATING_OFFSET))
        .constrain_to(viewport)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(cluster_fill(ui, opacity))
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
                    if icons::tool_button(
                        ui,
                        icons::view::FRAME_ALL,
                        furnicraft_text::Message::ToolFrameAll.text(furnicraft_ui::language(ui.ctx())),
                        Some("F"),
                        true,
                        false,
                    )
                    .clicked()
                    {
                        action = Some(ClusterAction::FrameAll);
                    }
                    if icons::tool_button(
                        ui,
                        icons::view::HOME,
                        furnicraft_text::Message::HeaderHome.text(furnicraft_ui::language(ui.ctx())),
                        None,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        action = Some(ClusterAction::Home);
                    }
                    theme::divider(ui);
                    if let Some(style) = style_buttons(ui, view.style, furnicraft_ui::language(ui.ctx())) {
                        action = Some(ClusterAction::SetStyle(style));
                    }
                    theme::divider(ui);
                    let open = state.layout_open;
                    if icons::tool_button(
                        ui,
                        icons::view::LAYOUT,
                        furnicraft_text::Message::Layout.text(furnicraft_ui::language(ui.ctx())),
                        None,
                        true,
                        open,
                    )
                    .clicked()
                    {
                        state.layout_open = !state.layout_open;
                        state.settings_open = false;
                    }
                    // A single view already fills the workspace, so there is nothing to expand: the
                    // button is drawn disabled instead of entering a mode it cannot leave.
                    let (icon, label) = if view.maximized {
                        (icons::view::RESTORE, furnicraft_text::Message::Restore)
                    } else {
                        (icons::view::MAXIMIZE, furnicraft_text::Message::Expand)
                    };
                    if icons::tool_button(
                        ui,
                        icon,
                        label.text(furnicraft_ui::language(ui.ctx())),
                        Some("Alt+W"),
                        view.layout.leaves() > 1,
                        view.maximized,
                    )
                    .clicked()
                    {
                        action = Some(ClusterAction::ToggleMaximize);
                    }
                    let open = state.settings_open;
                    if icons::tool_button(
                        ui,
                        icons::view::SLIDERS,
                        furnicraft_text::Message::DisplaySettings.text(furnicraft_ui::language(ui.ctx())),
                        None,
                        true,
                        open,
                    )
                    .clicked()
                    {
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
        } else if let Some(asked) = layout_popover(ui, viewport, rect, view, opacity) {
            state.layout_open = false;
            action = Some(asked);
        }
    }
    if state.settings_open {
        if ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            state.settings_open = false;
        } else if let Some(asked) = settings_popover(ui, viewport, rect, state, view, opacity) {
            action = Some(asked);
        }
    }
    action
}

/// The orbit, pan and zoom buttons; the armed one draws pressed and a second click disarms it.
fn navigation_buttons(ui: &mut egui::Ui, state: &mut ViewCluster) {
    for mode in NavigationMode::ALL {
        let on = state.navigation == Some(mode);
        if icons::tool_button_labelled(
            ui,
            mode.icon(),
            navigation_caption(mode).text(furnicraft_ui::language(ui.ctx())),
            mode.label().text(furnicraft_ui::language(ui.ctx())),
            true,
            on,
        )
        .clicked()
        {
            state.navigation = (!on).then_some(mode);
        }
    }
}

/// The short placeholder face of one navigation mode.
const fn navigation_caption(mode: NavigationMode) -> furnicraft_text::Message {
    match mode {
        NavigationMode::Orbit => furnicraft_text::Message::ShortcutOrbit,
        NavigationMode::Pan => furnicraft_text::Message::ShortcutPan,
        NavigationMode::Zoom => furnicraft_text::Message::ShortcutZoom,
    }
}

/// The five display styles; the view's own style draws pressed and a click asks for a change.
fn style_buttons(ui: &mut egui::Ui, current: ShadingStyle, lang: furnicraft_text::Lang) -> Option<ShadingStyle> {
    let mut asked = None;
    for style in ShadingStyle::ALL {
        let on = current == style;
        if icons::tool_button_labelled(
            ui,
            shading_icon(style),
            shading_caption(style, lang),
            shading_label(style, lang),
            true,
            on,
        )
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
    opacity: u32,
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
                .fill(cluster_fill(ui, opacity))
                .stroke(egui::Stroke::new(1.0, theme::line(ui)))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(10));
            frame.show(ui, |ui| {
                ui.set_width(220.0);
                ui.label(
                    egui::RichText::new(furnicraft_text::Message::Layout.text(furnicraft_ui::language(ui.ctx())))
                        .strong(),
                );
                for kind in LayoutKind::ALL {
                    let on = view.layout == kind;
                    if ui
                        .add(egui::Button::new(kind.label().text(furnicraft_ui::language(ui.ctx()))).selected(on))
                        .clicked()
                    {
                        action = Some(ClusterAction::SetLayout(kind));
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(furnicraft_text::Message::ViewPreset.text(furnicraft_ui::language(ui.ctx())))
                        .strong(),
                );
                for preset in OpeningView::ALL {
                    if preset_row(ui, preset, view.preset == Some(preset), furnicraft_ui::language(ui.ctx())) {
                        action = Some(ClusterAction::SetPreset(preset));
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        furnicraft_text::Message::ViewsOfMax
                            .with(furnicraft_ui::language(ui.ctx()), [view.layout.leaves(), MAX_LEAVES]),
                    )
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
fn preset_row(ui: &mut egui::Ui, view: OpeningView, on: bool, lang: furnicraft_text::Lang) -> bool {
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
            view.label(lang),
            egui::FontId::proportional(13.0),
            visuals.fg_stroke.color,
        );
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, view.label(lang)));
    }
    response.clicked()
}

/// The display-settings popover above the cluster: the angle switch, the projection, the contact
/// shadow and the link to the page that owns the strengths.
fn settings_popover(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    cluster: egui::Rect,
    state: &mut ViewCluster,
    view: ClusterView,
    opacity: u32,
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
                .fill(cluster_fill(ui, opacity))
                .stroke(egui::Stroke::new(1.0, theme::line(ui)))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(10));
            frame.show(ui, |ui| {
                ui.set_width(230.0);
                ui.label(
                    egui::RichText::new(
                        furnicraft_text::Message::DisplayHeading.text(furnicraft_ui::language(ui.ctx())),
                    )
                    .strong(),
                );
                let angles = ui.checkbox(
                    &mut state.show_angles,
                    furnicraft_text::Message::ShowProjectionAngles.text(furnicraft_ui::language(ui.ctx())),
                );
                angles.on_hover_text(
                    if state.angles_visible() {
                        furnicraft_text::Message::ProjectionAnglesOn
                    } else {
                        furnicraft_text::Message::ProjectionAnglesOff
                    }
                    .text(furnicraft_ui::language(ui.ctx())),
                );
                let (icon, hint) = projection_button(view.projection);
                let toggle = icons::tool_button(
                    ui,
                    icon,
                    hint.text(furnicraft_ui::language(ui.ctx())),
                    Some("O"),
                    view.owns_view,
                    false,
                );
                if toggle
                    .on_disabled_hover_text(
                        furnicraft_text::Message::AuthoredCameraOwns.text(furnicraft_ui::language(ui.ctx())),
                    )
                    .clicked()
                {
                    action = Some(ClusterAction::ToggleProjection);
                }
                ui.checkbox(
                    &mut state.contact_shadows,
                    furnicraft_text::Message::ContactShadows.text(furnicraft_ui::language(ui.ctx())),
                )
                .on_hover_text(furnicraft_text::Message::ContactShadowsHint.text(furnicraft_ui::language(ui.ctx())));
                ui.separator();
                // The strengths are workspace defaults, not a scene opinion: the popover links to the
                // one page that edits them instead of drawing three sliders over the model (U64).
                if ui
                    .button(furnicraft_text::Message::DisplayConfigure.text(furnicraft_ui::language(ui.ctx())))
                    .on_hover_text(
                        furnicraft_text::Message::DisplayConfigureHint.text(furnicraft_ui::language(ui.ctx())),
                    )
                    .clicked()
                {
                    state.settings_open = false;
                    action = Some(ClusterAction::OpenSettings);
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        furnicraft_text::Message::WorkPlane.with(furnicraft_ui::language(ui.ctx()), [view.work_plane]),
                    )
                    .small()
                    .color(theme::text_dim(ui)),
                );
            });
        });
    action
}

/// The icon and the hint of the projection toggle.
///
/// The icon draws the mode the view is *in*, and the hint names the switch, so the control never
/// claims a state the camera has not reached.
pub(crate) fn projection_button(projection: Projection) -> (icons::Icon, furnicraft_text::Message) {
    match projection {
        Projection::Perspective { .. } => (icons::view::PERSPECTIVE, furnicraft_text::Message::PerspectiveViewHint),
        Projection::Orthographic { .. } => (icons::view::ORTHOGRAPHIC, furnicraft_text::Message::OrthographicViewHint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::drawn_text;

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

    #[test]
    fn the_display_popover_keeps_the_switches_and_links_to_the_settings_page() {
        // The strengths moved to Settings › View, so the popover keeps its switches and offers the
        // one link that reaches them; the triangle diagnostic left the cluster with the sliders (U64).
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))),
            ..Default::default()
        };
        let mut state = ViewCluster { settings_open: true, ..ViewCluster::default() };
        let view = ClusterView {
            owns_view: true,
            projection: Projection::Perspective { fov_y: 0.9 },
            work_plane: "XY",
            layout: LayoutKind::Single,
            maximized: false,
            preset: None,
            style: ShadingStyle::default(),
        };
        // An area is drawn invisibly on its sizing pass, so the second frame is the one that shows it.
        for _ in 0..2 {
            let output = ctx.run_ui(input.clone(), |ui| {
                show(ui, viewport, &mut state, view, 92);
            });
            output.drop_without_applying_deltas();
        }
        let output = ctx.run_ui(input.clone(), |ui| {
            show(ui, viewport, &mut state, view, 92);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(
            texts.iter().any(|text| text.contains("Configure…")),
            "the popover links to the settings page: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("triangle edge")),
            "the diagnostic left the cluster with the strength sliders: {texts:?}"
        );
        for switch in ["Show projection and angles", "Contact shadows"] {
            assert!(texts.iter().any(|text| text.contains(switch)), "the switch stays: {switch} in {texts:?}");
        }
    }
}
