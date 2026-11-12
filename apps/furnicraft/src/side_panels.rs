//! The floating side panels: opaque chrome over the scene, folded to a 34 pt strip or hidden.
//!
//! Both panels are `egui` areas on the middle layer, so the rendered scene runs under them and the
//! shared opacity lets a little of the furniture through. [`Panels`] owns the two states and the
//! memory that makes *collapse all* and *hide all* return exactly what the user had. [`Chrome`] is
//! the frame's geometry: the two panel rectangles, the ribbon band and the scene rectangle the split
//! views tile — a rectangle the panels never narrow (U72).
//!
//! Changes when: the panel geometry, the header row, the folded strip, the fold and hide gestures or
//! the opacity of the chrome changes.

use eframe::egui;
use furnicraft_text::Message;
use furnicraft_ui::settings::PanelState;
use furnicraft_ui::{icons, theme};

/// The width of an unfolded panel, in points.
pub(crate) const EXPANDED_WIDTH: f32 = 226.0;

/// The width of a folded panel's strip, in points.
pub(crate) const STRIP_WIDTH: f32 = theme::COLLAPSED_PANEL_WIDTH;

/// The gap between a panel and the viewport edge, in points.
pub(crate) const INSET: f32 = 8.0;

/// The corner radius of a floating panel, in points.
const RADIUS: u8 = 8;

/// How long the ribbon band and a panel width take to reach their new size, in seconds.
pub(crate) const ANIMATION_SECONDS: f32 = 0.15;

/// Which edge a folding side panel sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelSide {
    Left,
    Right,
}

/// What a folding side panel's header row draws: the title and the icon the folded strip keeps.
///
/// A header carries no count: the tree and the card already say what they hold, and a number that
/// tracks the document would cost a scan on every rebuild.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PanelHead {
    pub(crate) title: Message,
    pub(crate) icon: icons::Icon,
}

/// What one panel gesture asked the shell to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelEvent {
    /// Nothing was clicked.
    #[default]
    None,
    /// The header asked to fold the panel.
    Fold,
    /// The strip asked to unfold it.
    Restore,
}

/// The pair of side panels and the memories of the last *collapse all* and *hide all*.
///
/// The two memories are separate: collapsing while hidden then showing again must return the fold
/// the panels had, and hiding while collapsed then showing again must bring back the strips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Panels {
    /// The scene tree on the left.
    pub(crate) scene: PanelState,
    /// The inspector on the right.
    pub(crate) inspector: PanelState,
    /// The exact pair to restore on the next *collapse all* press.
    collapsed: Option<(PanelState, PanelState)>,
    /// The exact pair to restore on the next *hide all* press.
    hidden: Option<(PanelState, PanelState)>,
}

impl Panels {
    /// The pair a workspace settings file describes, without any memory of a prior gesture.
    pub(crate) const fn from_settings(panels: &furnicraft_ui::settings::PanelsSettings) -> Self {
        Self { scene: panels.scene, inspector: panels.inspector, collapsed: None, hidden: None }
    }

    /// Whether at least one panel is on screen.
    pub(crate) const fn any_visible(&self) -> bool {
        self.scene.visible || self.inspector.visible
    }

    /// Whether both panels are hidden.
    pub(crate) const fn all_hidden(&self) -> bool {
        !self.scene.visible && !self.inspector.visible
    }

    /// Whether both panels are folded.
    pub(crate) const fn all_collapsed(&self) -> bool {
        !self.scene.expanded && !self.inspector.expanded
    }

    /// Collapses both panels, or restores the pair the last collapse pushed aside.
    ///
    /// The second press is the exact inverse: visibility and fold of both panels return as they
    /// were, so a user who folds one panel, collapses both and expands both is back where they
    /// started.
    pub(crate) fn toggle_collapse_all(&mut self) {
        if self.all_collapsed() {
            match self.collapsed.take() {
                Some((scene, inspector)) => {
                    self.scene = scene;
                    self.inspector = inspector;
                }
                // Nothing was pushed aside — the pair was already folded when the workspace opened
                // — so the press asks for the unfolded panels instead of doing nothing.
                None => {
                    self.scene.expanded = true;
                    self.inspector.expanded = true;
                }
            }
            return;
        }
        self.collapsed = Some((self.scene, self.inspector));
        self.scene.expanded = false;
        self.inspector.expanded = false;
    }

    /// Hides both panels, or restores the pair the last hide pushed aside.
    pub(crate) fn toggle_hide_all(&mut self) {
        if self.all_hidden() {
            match self.hidden.take() {
                Some((scene, inspector)) => {
                    self.scene = scene;
                    self.inspector = inspector;
                }
                // A workspace that opened with both panels hidden gets them back unfolded.
                None => {
                    self.scene = PanelState::default();
                    self.inspector = PanelState::default();
                }
            }
            return;
        }
        self.hidden = Some((self.scene, self.inspector));
        self.scene.visible = false;
        self.inspector.visible = false;
    }

    /// Forgets the memories, so a settings change does not restore an older pair by surprise.
    pub(crate) fn forget_memory(&mut self) {
        self.collapsed = None;
        self.hidden = None;
    }
}

/// The floating rectangles of both panels for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PanelRects {
    /// The scene tree, or `None` while it is hidden.
    pub(crate) left: Option<egui::Rect>,
    /// The inspector, or `None` while it is hidden.
    pub(crate) right: Option<egui::Rect>,
}

impl PanelRects {
    /// The rectangles the two panels occupy in `viewport`.
    ///
    /// `top` is the first point below the ribbon band ([`panel_top`]); `widths` are the animated
    /// widths, so a fold grows and shrinks over [`ANIMATION_SECONDS`] instead of jumping.
    pub(crate) fn of(viewport: egui::Rect, top: f32, panels: &Panels, widths: (f32, f32)) -> Self {
        Self {
            left: panel_rect(viewport, PanelSide::Left, top, panels.scene, widths.0),
            right: panel_rect(viewport, PanelSide::Right, top, panels.inspector, widths.1),
        }
    }

    /// The rectangle one side covers, or `None` while that panel is hidden.
    ///
    /// The orientation widget asks by side, because the corner it may draw in is the one the panel
    /// otherwise covers (U73).
    pub(crate) fn of_side(&self, side: PanelSide) -> Option<egui::Rect> {
        match side {
            PanelSide::Left => self.left,
            PanelSide::Right => self.right,
        }
    }

    /// Every rectangle the pointer and the split views must treat as chrome.
    pub(crate) fn panels(&self) -> impl Iterator<Item = egui::Rect> {
        [PanelSide::Left, PanelSide::Right].into_iter().filter_map(|side| self.of_side(side))
    }
}

/// The first point a panel may start at: the ribbon band plus the gap.
///
/// Pure, so a golden can pin the two ribbon heights the workspace draws (`72` and `29` points).
pub(crate) const fn panel_top(ribbon_height: f32) -> f32 {
    ribbon_height + INSET
}

/// The rectangle one panel covers, or `None` while it is hidden.
///
/// The panel is inset from the viewport edge and stops [`INSET`] above the bottom, so the status bar
/// stays outside the chrome. A viewport too short for the band leaves a zero-height panel rather
/// than a rectangle outside the surface.
pub(crate) fn panel_rect(
    viewport: egui::Rect,
    side: PanelSide,
    top: f32,
    state: PanelState,
    width: f32,
) -> Option<egui::Rect> {
    if !state.visible {
        return None;
    }
    let width = width.clamp(0.0, EXPANDED_WIDTH.max(STRIP_WIDTH));
    let x = match side {
        PanelSide::Left => viewport.left() + INSET,
        PanelSide::Right => viewport.right() - INSET - width,
    };
    let top = top.clamp(viewport.top(), viewport.bottom());
    let bottom = (viewport.bottom() - INSET).max(top);
    Some(egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + width, bottom)))
}

/// The frame geometry every pointer hit and every split view reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Chrome {
    /// The two floating panels.
    pub(crate) panels: PanelRects,
    /// The band the floating ribbon covers.
    pub(crate) ribbon: egui::Rect,
}

impl Chrome {
    /// Whether `point` lies on chrome rather than on the scene.
    pub(crate) fn covers(&self, point: egui::Pos2) -> bool {
        self.ribbon.contains(point) || self.panels.panels().any(|rect| rect.contains(point))
    }

    /// The part of the viewport the scene owns.
    ///
    /// The split views tile this rectangle, and a border drag or a viewpress inside the panels is
    /// ignored. It depends on the ribbon band only: the panels float over the scene, so folding,
    /// hiding, collapsing or restoring one changes no view rectangle and no camera (U72). The band
    /// is excluded at the top because the scene is never framed or picked under it (U51).
    pub(crate) fn content(&self, viewport: egui::Rect) -> egui::Rect {
        let top = self.ribbon.bottom().clamp(viewport.top(), viewport.bottom());
        let content = egui::Rect::from_min_max(egui::pos2(viewport.left(), top), viewport.max);
        if content.width() > 0.0 && content.height() > 0.0 { content } else { viewport }
    }
}

/// The fill of a panel at the shared chrome opacity.
///
/// The ribbon, both panels and the view cluster all take this one setting, so changing it repaints
/// the whole chrome together; the panel's own header uses [`theme::surface`] at the same alpha.
pub(crate) fn panel_fill(ui: &egui::Ui, opacity: u32) -> egui::Color32 {
    theme::at_opacity(theme::panel(ui), opacity)
}

/// Where one panel draws this frame: its edge, its area id, its rectangle, its state and its head.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PanelSlot<'a> {
    /// The edge the panel sits on.
    pub(crate) side: PanelSide,
    /// The area id, distinct per panel.
    pub(crate) id: &'a str,
    /// The rectangle the chrome covers.
    pub(crate) rect: egui::Rect,
    /// Whether the panel is shown and unfolded.
    pub(crate) state: PanelState,
    /// The title and icon the header and the strip draw.
    pub(crate) head: PanelHead,
}

/// Draws one panel body in a scroller that never widens the panel.
///
/// A panel's rectangle is a contract the shell reads twice — [`Chrome::covers`] turns a press on it
/// into chrome, and the orientation widget is placed clear of it. A body wider than the panel would
/// therefore paint over the scene the widget was told is free, and a press on it would reach the
/// scene instead of the panel. The scroller bounds the body to the panel's inner width and gives a
/// row too wide for it a horizontal scrollbar instead of growing the panel (U73).
pub(crate) fn scroll_body(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::both().show(ui, body);
}

/// Draws one floating panel into `rect` and returns the gesture its chrome asked for.
///
/// `opacity` is the shared chrome percentage, so the ribbon, the panels and the cluster fade
/// together. The panel's own body is handed in by the caller and never mutates the document
/// directly.
pub(crate) fn show(
    ctx: &egui::Context,
    slot: PanelSlot<'_>,
    opacity: u32,
    body: impl FnOnce(&mut egui::Ui),
) -> PanelEvent {
    let PanelSlot { side, id, rect, state, head } = slot;
    if !state.visible {
        return PanelEvent::None;
    }
    let mut event = PanelEvent::None;
    egui::Area::new(egui::Id::unique(id)).order(egui::Order::Middle).fixed_pos(rect.min).show(ctx, |ui| {
        let sizing = ui.is_sizing_pass();
        let fill = panel_fill(ui, opacity);
        let frame = egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(RADIUS));
        frame.show(ui, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            // The whole panel is chrome: a click on a gap or a margin belongs to the panel, not to
            // the view behind it. It senses a drag as well as a click because the viewport does, so
            // a hover-sensing gap cannot let a drawing gesture start underneath.
            if !sizing {
                ui.interact(
                    egui::Rect::from_min_size(ui.min_rect().min, rect.size()),
                    egui::Id::unique((id, "chrome")),
                    egui::Sense::click_and_drag(),
                );
            }
            // A fold animates the width, so the strip appears only when the panel has reached it;
            // while the rectangle is still wider than a strip the body keeps drawing.
            if rect.width() > STRIP_WIDTH + 0.5 {
                if panel_header(ui, side, &head, opacity) {
                    event = PanelEvent::Fold;
                }
                rule(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(theme::STEP as i8, theme::STEP as i8))
                    .show(ui, |ui| body(ui));
            } else if collapsed_strip(ui, side, &head) {
                event = PanelEvent::Restore;
            }
        });
    });
    event
}

/// A hairline across the panel, quieter than `ui.separator()` and on the spacing grid.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, theme::line(ui)));
}

/// The header of an unfolded side panel: the title and the fold glyph.
///
/// The glyph points to the edge the panel folds to — left on a left panel, right on a right panel —
/// so the control always faces the viewport the panel gives back. Returns whether it was clicked.
fn panel_header(ui: &mut egui::Ui, side: PanelSide, head: &PanelHead, opacity: u32) -> bool {
    let mut fold = false;
    egui::Frame::new()
        .fill(theme::at_opacity(theme::surface(ui), opacity))
        .corner_radius(egui::CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 })
        .inner_margin(egui::Margin::symmetric(theme::STEP as i8, 0))
        .show(ui, |ui| {
            ui.set_height(theme::PANEL_HEADER_HEIGHT);
            ui.horizontal_centered(|ui| {
                if side == PanelSide::Right {
                    fold = fold_glyph(ui, side);
                }
                ui.label(egui::RichText::new(head.title.text(furnicraft_ui::language(ui.ctx()))).strong());
                if side == PanelSide::Left {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        fold = fold_glyph(ui, side);
                    });
                }
            });
        });
    fold
}

/// The neutral glyph that folds an unfolded panel, pointing to the edge it folds to.
fn fold_glyph(ui: &mut egui::Ui, side: PanelSide) -> bool {
    let icon = match side {
        PanelSide::Left => icons::actions::PANEL_LEFT,
        PanelSide::Right => icons::actions::PANEL_RIGHT,
    };
    icons::icon_button_at(ui, icon, 12.0, Message::FoldPanel.text(furnicraft_ui::language(ui.ctx())), true, false)
        .clicked()
}

/// The glyph the folded strip shows: the fold mirrored, so it points at the viewport it restores to.
fn restore_icon(side: PanelSide) -> icons::Icon {
    match side {
        PanelSide::Left => icons::actions::PANEL_RIGHT,
        PanelSide::Right => icons::actions::PANEL_LEFT,
    }
}

/// The folded panel: a 34 pt strip with the restore glyph, the panel's icon and its title turned to
/// read along the strip. A click anywhere on the strip restores the panel.
fn collapsed_strip(ui: &mut egui::Ui, side: PanelSide, head: &PanelHead) -> bool {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() { theme::control_hover(ui) } else { theme::surface(ui) };
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, fill);

    let restore = restore_icon(side);
    let header_top = rect.top();
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, header_top + theme::PANEL_HEADER_HEIGHT / 2.0),
        egui::Vec2::splat(14.0),
    );
    icons::draw_icon(ui, restore, glyph_rect, 14.0, theme::text_dim(ui));
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, glyph_rect.bottom() + theme::STEP + 7.0),
        egui::Vec2::splat(14.0),
    );
    icons::draw_icon(ui, head.icon, icon_rect, 14.0, theme::text_dim(ui));

    // The title is turned so it reads bottom to top. `with_angle` turns about the galley's top-left
    // corner, so the corner is offset by the rotated half-size to centre the turned text.
    let dim = theme::text_dim(ui);
    let galley = ui.painter().layout_no_wrap(
        head.title.text(furnicraft_ui::language(ui.ctx())).to_owned(),
        egui::FontId::proportional(12.0),
        dim,
    );
    let size = galley.size();
    let center = egui::pos2(rect.center().x, rect.center().y + theme::PANEL_HEADER_HEIGHT / 2.0);
    let pos = egui::pos2(center.x - size.y / 2.0, center.y + size.x / 2.0);
    ui.painter().add(egui::epaint::TextShape::new(pos, galley, dim).with_angle(-std::f32::consts::FRAC_PI_2));
    let hint = Message::OpenPanel
        .with(furnicraft_ui::language(ui.ctx()), [head.title.text(furnicraft_ui::language(ui.ctx()))]);
    response.on_hover_text(hint).clicked()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::test_support::drawn_text;

    /// The window every test draws into.
    const SCREEN: egui::Vec2 = egui::vec2(1000.0, 600.0);

    /// A header whose title and icon stand for a real panel.
    fn head() -> PanelHead {
        PanelHead { title: Message::PanelScene, icon: icons::scene::ASSEMBLY }
    }

    /// A context on the real theme with animation off, so one frame draws its final state.
    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        ctx
    }

    fn input() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)), ..Default::default() }
    }

    /// The bounding rect of every rasterised glyph a frame painted.
    fn drawn_glyphs(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
        fn walk(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Mesh(mesh) => out.push(mesh.calc_bounds()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// A viewport the size of the test window, below a 72 pt ribbon.
    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), SCREEN)
    }

    /// The frame geometry of `panels` under a ribbon band of `ribbon_height` points.
    fn chrome_with(ribbon_height: f32, panels: &Panels) -> Chrome {
        let viewport = viewport();
        let top = viewport.top() + panel_top(ribbon_height);
        let ribbon = egui::Rect::from_min_size(viewport.min, egui::vec2(viewport.width(), ribbon_height));
        let width = |state: PanelState| if state.expanded { EXPANDED_WIDTH } else { STRIP_WIDTH };
        Chrome { panels: PanelRects::of(viewport, top, panels, (width(panels.scene), width(panels.inspector))), ribbon }
    }

    /// The frame geometry of both panels shown at their expanded width under a 72 pt ribbon.
    fn chrome() -> Chrome {
        chrome_with(furnicraft_ui::toolbars::height(true), &Panels::default())
    }

    /// The pair where both panels are folded to their strips.
    fn folded() -> Panels {
        let mut panels = Panels::default();
        panels.scene.expanded = false;
        panels.inspector.expanded = false;
        panels
    }

    /// The pair where both panels are hidden.
    fn hidden() -> Panels {
        let mut panels = Panels::default();
        panels.scene.visible = false;
        panels.inspector.visible = false;
        panels
    }

    #[test]
    fn a_panel_body_never_widens_the_panel() {
        // The inspector's rows are wider than the panel, so its body scrolls sideways: a painted
        // rect wider than `PanelRects` would cover the scene and steal the widget's free corner.
        let ctx = context();
        let input = input();
        let mut width = 0.0_f32;
        for _ in 0..2 {
            let output = ctx.run_ui(input.clone(), |ui| {
                ui.set_width(EXPANDED_WIDTH);
                let left = ui.min_rect().left();
                scroll_body(ui, |ui| {
                    ui.allocate_exact_size(egui::vec2(400.0, 20.0), egui::Sense::hover());
                });
                width = ui.min_rect().right() - left;
            });
            output.drop_without_applying_deltas();
        }
        assert!(width <= EXPANDED_WIDTH + 1.0, "the panel keeps its width: {width}");
    }

    #[test]
    fn the_panel_top_follows_the_ribbon_band_and_keeps_the_gap() {
        // The two heights the workspace draws, pinned as goldens: the panels never jump when the
        // ribbon folds, because both read the same animated band.
        for (open, band) in [(true, 72.0), (false, 29.0)] {
            assert!((furnicraft_ui::toolbars::height(open) - band).abs() < f32::EPSILON, "the band golden");
            assert!((panel_top(band) - (band + INSET)).abs() < f32::EPSILON);
        }
        assert!(panel_top(72.0) > panel_top(29.0), "folding the ribbon lifts the panels");
    }

    #[test]
    fn the_panels_are_inset_from_three_edges_and_hidden_at_zero_width() {
        let viewport = viewport();
        let top = panel_top(72.0);
        for side in [PanelSide::Left, PanelSide::Right] {
            let expanded = panel_rect(viewport, side, top, PanelState::default(), EXPANDED_WIDTH).expect("shown");
            assert!(expanded.width() >= EXPANDED_WIDTH - f32::EPSILON && expanded.width() <= EXPANDED_WIDTH, "width");
            assert!((expanded.top() - top).abs() < f32::EPSILON, "{side:?} starts below the ribbon");
            assert!(
                (expanded.bottom() - (viewport.bottom() - INSET)).abs() < f32::EPSILON,
                "{side:?} clears the status bar"
            );
            match side {
                PanelSide::Left => assert!((expanded.left() - (viewport.left() + INSET)).abs() < f32::EPSILON),
                PanelSide::Right => assert!((expanded.right() - (viewport.right() - INSET)).abs() < f32::EPSILON),
            }
            let strip = panel_rect(viewport, side, top, PanelState { visible: true, expanded: false }, STRIP_WIDTH)
                .expect("shown");
            assert!((strip.width() - STRIP_WIDTH).abs() < f32::EPSILON, "a folded panel is a strip");
            assert!(
                panel_rect(viewport, side, top, PanelState { visible: false, expanded: true }, EXPANDED_WIDTH)
                    .is_none(),
                "a hidden panel covers nothing"
            );
        }
    }

    #[test]
    fn hide_all_and_collapse_all_round_trip_exactly() {
        let mut panels = Panels::default();
        panels.inspector.expanded = false;

        panels.toggle_hide_all();
        assert!(!panels.any_visible(), "the first press hides both");
        panels.toggle_hide_all();
        assert_eq!(panels.scene, PanelState::default());
        assert_eq!(panels.inspector, PanelState { visible: true, expanded: false }, "the exact pair returns");

        panels.toggle_collapse_all();
        assert!(panels.all_collapsed(), "the first press folds both");
        panels.toggle_collapse_all();
        assert_eq!(panels.scene, PanelState::default(), "the fold of the scene tree returns");
        assert_eq!(panels.inspector, PanelState { visible: true, expanded: false }, "and the inspector's");
    }

    #[test]
    fn a_workspace_that_opens_hidden_or_folded_can_be_brought_back() {
        use furnicraft_ui::settings::PanelsSettings;

        let hidden = PanelsSettings {
            scene: PanelState { visible: false, expanded: true },
            inspector: PanelState { visible: false, expanded: true },
        };
        let mut panels = Panels::from_settings(&hidden);
        panels.toggle_hide_all();
        assert_eq!(panels.scene, PanelState::default(), "the press shows a workspace that opened hidden");
        assert_eq!(panels.inspector, PanelState::default());

        let folded = PanelsSettings {
            scene: PanelState { visible: true, expanded: false },
            inspector: PanelState { visible: true, expanded: false },
        };
        let mut panels = Panels::from_settings(&folded);
        panels.toggle_collapse_all();
        assert!(!panels.all_collapsed(), "the press unfolds a workspace that opened folded");
    }

    #[test]
    fn hiding_a_collapsed_pair_restores_the_strips_and_not_the_expanded_panels() {
        let mut panels = Panels::default();
        panels.toggle_collapse_all();
        panels.toggle_hide_all();
        panels.toggle_hide_all();
        assert!(panels.all_collapsed(), "the strips are back, not the unfolded panels");
    }

    #[test]
    fn a_point_on_the_chrome_is_never_a_viewport_hit() {
        let chrome = chrome();
        for panel in chrome.panels.panels() {
            assert!(chrome.covers(panel.center()), "the middle of a panel is chrome");
            assert!(chrome.covers(panel.min + egui::vec2(1.0, 1.0)), "and so is its corner");
        }
        assert!(chrome.covers(chrome.ribbon.center()), "the ribbon band is chrome");
        let content = chrome.content(viewport());
        assert!(!chrome.covers(content.center()), "the middle of the scene is not");
        assert!((content.left() - viewport().left()).abs() < f32::EPSILON, "the scene keeps the full width");
        assert!((content.right() - viewport().right()).abs() < f32::EPSILON);
        assert!(content.top() >= chrome.ribbon.bottom(), "and starts below the ribbon");
    }

    #[test]
    fn the_scene_rect_ignores_the_panels_and_starts_below_the_ribbon() {
        let viewport = viewport();
        let chrome = chrome();
        let content = chrome.content(viewport);
        // The panels float over the scene: neither their state nor their width moves an edge.
        for panels in [Panels::default(), folded(), hidden()] {
            let shifted = chrome_with(furnicraft_ui::toolbars::height(true), &panels).content(viewport);
            assert!((shifted.left() - viewport.left()).abs() < f32::EPSILON, "the scene starts at the left edge");
            assert!((shifted.right() - viewport.right()).abs() < f32::EPSILON, "and ends at the right one");
            assert_eq!(shifted, content, "a panel change moves no edge");
        }
        // Both ribbon states put the top at the measured band and give the rest to the scene.
        for open in [true, false] {
            let band = furnicraft_ui::toolbars::height(open);
            let banded = chrome_with(band, &Panels::default()).content(viewport);
            assert!((banded.top() - band).abs() < f32::EPSILON, "the band sets the top (open = {open})");
            assert!((banded.height() - (viewport.height() - band)).abs() < f32::EPSILON);
            assert!((banded.width() - viewport.width()).abs() < f32::EPSILON, "a panel never narrows it");
        }
        assert!(content.width() > 0.0 && content.height() > 0.0, "a half-open window still has a scene");
    }

    #[test]
    fn a_press_on_a_panel_is_chrome_and_a_press_beside_it_reaches_the_scene() {
        use furnicraft_scene::Camera;
        use furnicraft_ui::display::{OpeningView, ShadingStyle};

        use crate::view_layout::{Layout, LayoutKind};

        let chrome = chrome();
        let viewport = viewport();
        let content = chrome.content(viewport);
        for panel in chrome.panels.panels() {
            // One point past the panel's inner edge, towards the middle of the scene.
            let beside = match panel.center().x < viewport.center().x {
                true => egui::pos2(panel.right() + 1.0, panel.center().y),
                false => egui::pos2(panel.left() - 1.0, panel.center().y),
            };
            assert!(chrome.covers(panel.center()), "a click on the panel is chrome and starts no gesture");
            assert!(!chrome.covers(beside), "a click one point past the panel is the scene");
            assert!(content.contains(beside), "because the panel floats over the scene, it does not shrink it");
        }
        // A border stays grabbable from the scene, and the chrome guard never hides it.
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        layout.set_kind(LayoutKind::TwoColumns);
        let divider = layout.dividers(content).into_iter().next().expect("one border");
        assert!(content.contains(divider.hit_rect().center()), "the border band is inside the scene");
        assert!(!chrome.covers(divider.hit_rect().center()), "and not under a panel");
        assert!(layout.press(content, divider.hit_rect().center(), true), "a press there grabs the border");
    }

    /// Runs `frames` frames of one panel and returns the last, dropping the earlier ones the way a
    /// window would: an unapplied texture delta in a dropped frame is a panic in epaint.
    fn run_panel(ctx: &egui::Context, side: PanelSide, state: PanelState, id: &str, frames: usize) -> egui::FullOutput {
        let mut last = None;
        for _ in 0..frames.max(1) {
            let next = ctx.run_ui(input(), |_ui| {
                let width = if state.expanded { EXPANDED_WIDTH } else { STRIP_WIDTH };
                let rect = panel_rect(viewport(), side, panel_top(72.0), state, width).expect("shown");
                let event = show(ctx, PanelSlot { side, id, rect, state, head: head() }, 92, |ui| {
                    ui.label("BODY-CONTENT");
                });
                assert_eq!(event, PanelEvent::None, "a frame with no click asks for nothing");
            });
            if let Some(previous) = last.replace(next) {
                previous.drop_without_applying_deltas();
            }
        }
        last.expect("one frame")
    }

    #[test]
    fn a_panel_header_draws_its_title_and_no_count() {
        let ctx = context();
        for side in [PanelSide::Left, PanelSide::Right] {
            let output = run_panel(&ctx, side, PanelState::default(), "header_test", 3);
            let text = drawn_text(&output.shapes);
            output.drop_without_applying_deltas();
            assert!(text.iter().any(|line| line.contains("Scene")), "{side:?} names itself: {text:?}");
            assert!(
                !text.iter().any(|line| line.chars().any(|c| c.is_ascii_digit())),
                "{side:?} draws no count: {text:?}"
            );
        }
    }

    #[test]
    fn a_folded_panel_draws_its_strip_and_no_body() {
        let ctx = context();
        let folded = PanelState { visible: true, expanded: false };
        let output = run_panel(&ctx, PanelSide::Left, folded, "fold_test", 3);
        let text = drawn_text(&output.shapes);
        let glyphs = drawn_glyphs(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(!text.iter().any(|line| line.contains("BODY-CONTENT")), "a folded panel draws no body");
        assert!(text.iter().any(|line| line.contains("Scene")), "the strip keeps the turned title");
        assert!(!glyphs.is_empty(), "the strip draws the restore glyph and the panel icon");
    }

    #[test]
    fn the_chrome_shares_one_opacity() {
        let ctx = context();
        let output = ctx.run_ui(input(), |ui| {
            let mut previous = None;
            for percent in [55, 92, 100] {
                let fill = panel_fill(ui, percent);
                assert_eq!(
                    fill.a(),
                    theme::at_opacity(theme::panel(ui), percent).a(),
                    "the panel takes the one chrome setting"
                );
                if let Some(previous) = previous.replace(fill) {
                    assert!(fill.a() > previous.a(), "a higher setting is more opaque");
                }
                let cluster = crate::view_cluster::cluster_fill(ui, percent);
                assert_eq!(
                    cluster.a(),
                    theme::at_opacity(theme::floating(ui), percent.max(80)).a(),
                    "the cluster keeps at least 80 % so it stays readable at {percent} %"
                );
            }
            // At 55 % the panel is translucent, so the furniture the image paints under it shows
            // through instead of the panel blending with the clear colour (U72).
            let faint = panel_fill(ui, 55).a();
            assert!(faint > 0 && faint < 255, "a 55 % panel lets the scene through: alpha {faint}");
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn folding_or_hiding_a_panel_changes_no_view_rect_and_no_aspect() {
        use furnicraft_scene::Camera;
        use furnicraft_ui::display::{OpeningView, ShadingStyle};

        use crate::strip::view_region;
        use crate::view_layout::{Layout, LayoutKind};

        let band = furnicraft_ui::toolbars::height(true);
        let reference = chrome().content(viewport());
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        for kind in LayoutKind::ALL {
            layout.set_kind(kind);
            let expected = layout.regions(reference);
            for panels in [Panels::default(), folded(), hidden()] {
                let content = chrome_with(band, &panels).content(viewport());
                assert_eq!(content, reference, "{kind:?}: the scene rect is the same");
                let regions = layout.regions(content);
                assert_eq!(regions, expected, "{kind:?}: no view moves");
                for (_, rect) in &regions {
                    // The renderer takes the camera's aspect from the view region, so an unchanged
                    // point rectangle is an unchanged aspect and an unchanged camera.
                    let region = view_region(content, *rect, 1.0);
                    let aspect = f64::from(region.width) / f64::from(region.height);
                    let expected_aspect = f64::from(rect.width()) / f64::from(rect.height());
                    assert!((aspect - expected_aspect).abs() < 1.0e-3, "{kind:?}: the camera keeps its aspect");
                }
            }
        }
    }

    #[test]
    fn the_split_views_tile_the_scene_rect_exactly() {
        use furnicraft_scene::Camera;
        use furnicraft_ui::display::{OpeningView, ShadingStyle};

        use crate::view_layout::{Layout, LayoutKind};

        let chrome = chrome();
        let content = chrome.content(viewport());
        let mut layout = Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        for kind in [LayoutKind::TwoColumns, LayoutKind::TwoRows, LayoutKind::Quad] {
            layout.set_kind(kind);
            let regions = layout.regions(content);
            assert_eq!(regions.len(), kind.leaves(), "{kind:?} tiles with its own leaves");
            for (_, rect) in &regions {
                assert!(content.contains_rect(*rect), "{kind:?} tiles only the scene rect: {rect:?}");
            }
            let area: f32 = regions.iter().map(|(_, rect)| rect.width() * rect.height()).sum();
            assert!((area - content.width() * content.height()).abs() < 1.0, "{kind:?} covers the rect exactly");
        }
        // A border keeps a hit band inside the scene, so a split stays grabbable next to a panel.
        layout.set_kind(LayoutKind::TwoColumns);
        let divider = layout.dividers(content).into_iter().next().expect("one border");
        assert!(content.contains(divider.hit_rect().center()), "the border band is inside the scene");
    }
}
