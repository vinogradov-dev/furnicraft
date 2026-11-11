//! One visual standard for the whole window: palette, spacing, rounding and type.
//!
//! The editor is a CAD surface, not a form: it is dark, dense and quiet, and the accent appears
//! only where there is an action, a selection or a result (`assets/icons/STYLE_GUIDE.md` §1, §5.2).
//! Everything the shell draws — the header, the ribbon, the panels and the viewport bar — takes its
//! colour and its rhythm from here instead of the `egui` defaults, so a side panel and the ribbon
//! share one grid.
//!
//! The theme is presentation only. It stores nothing in the document and holds no widget state; a
//! colour change is a rebuild of the [`egui::Style`], not a document revision.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals, style::WidgetVisuals, style::Widgets, vec2,
};

/// The darkest chrome: the window frame around the panels.
pub const CHROME: Color32 = Color32::from_rgb(0x1a, 0x1c, 0x20);

/// A raised surface: the header, the ribbon body and the panel headers.
pub const SURFACE: Color32 = Color32::from_rgb(0x22, 0x25, 0x2b);

/// The panel background, a shade above [`SURFACE`] where content sits.
pub const PANEL: Color32 = Color32::from_rgb(0x27, 0x2a, 0x31);

/// A control's resting fill, and the hovered one.
pub const CONTROL: Color32 = Color32::from_rgb(0x2f, 0x32, 0x38);
/// A hovered control.
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x39, 0x3d, 0x44);
/// A pressed control.
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x24);

/// The translucent fill of a floating cluster over the viewport: [`SURFACE`] at about 88 %.
///
/// A floating cluster sits over the drawn scene, so it has to keep the scene readable behind it
/// without losing the contrast of its own controls. The alpha is premultiplied, which is what
/// `egui` expects from a `Color32`.
pub const FLOATING: Color32 = Color32::from_rgba_premultiplied(0x1e, 0x21, 0x26, 224);

/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xdd, 0xdf, 0xe2);

/// Secondary text: hints, units, group captions.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x95, 0x99, 0xa0);

/// Text that cannot be acted on.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x5f, 0x63, 0x69);

/// A hairline between sections.
pub const LINE: Color32 = Color32::from_rgb(0x36, 0x39, 0x3e);

/// The action accent: the armed tool, the active tab, the selection.
pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x9f, 0xd4);

/// Accent as a background, once it has to carry text or an icon.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x1f, 0x6e, 0x94);

/// A destructive action's text, only while it can be taken.
pub const DANGER: Color32 = Color32::from_rgb(0xd9, 0x5c, 0x5c);

/// A warning text.
pub const WARN: Color32 = Color32::from_rgb(0xd8, 0xa4, 0x3c);

/// One step of the spacing scale, in points.
pub const STEP: f32 = 4.0;

/// The height of the header row, in points.
pub const HEADER_HEIGHT: f32 = 36.0;

/// The height of one control in the header row, in points.
///
/// A text button (`File`) and an icon button (`Save`, `Undo`, `Home`) are two faces of one control, so
/// they share a height and take their width from what they carry: the header then reads as one row of
/// equal chips instead of a text button that sits a few points lower than its neighbours.
pub const HEADER_CONTROL_HEIGHT: f32 = 26.0;

/// The width of a header text button, in points, so `File` is not wider than `Save`.
pub const HEADER_TEXT_WIDTH: f32 = 52.0;

/// The height of a ribbon group's tool row, in points.
///
/// Tall enough for the reference's proportion: the icon, the gap under it and the caption form one
/// block of about 43 pt, which leaves about 6 pt of air above and below in a 56 pt row. A shorter row
/// pushed the caption against the bottom edge of its cell.
pub const RIBBON_ROW_HEIGHT: f32 = 56.0;

/// The narrowest a ribbon cell may be, in points.
///
/// The cell is the same for every tool of a row and grows with the window between this floor and
/// [`RIBBON_BUTTON_MAX_WIDTH`], because a caption must never size its own button (ADR 0067). The
/// floor is set by the longest caption the ribbon can draw: the Russian `Прямоугольник` measures
/// 91 pt at [`crate::icons::RIBBON_CAPTION_POINT_SIZE`], and the longest U49 caption measures 98,
/// so the cell padding brings the floor to 106, rounded up to 108.
pub const RIBBON_BUTTON_MIN_WIDTH: f32 = 108.0;

/// The widest a ribbon cell may be, in points.
///
/// Past this a row of tools stops reading as a route through the tools and starts reading as a row of
/// wide plates; the owner's reference at full screen has a step of about 105 pt.
pub const RIBBON_BUTTON_MAX_WIDTH: f32 = 112.0;

/// The pitch of one ribbon row: the width one tool cell takes.
///
/// Every cell of a ribbon is the same width, taken from one measurement of the window and the widest
/// tab and clamped to [`RIBBON_BUTTON_MIN_WIDTH`]..[`RIBBON_BUTTON_MAX_WIDTH`]. The owner's two
/// observations are both rules here: a caption never sizes its own button, and a maximised window
/// still grows the cells — but only up to a maximum, past which the row would read as plates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RibbonMetrics {
    /// The width of one tool cell, in points.
    pub cell: f32,
}

impl RibbonMetrics {
    /// Measures the cells of a ribbon being drawn into `ui` on a tab of `columns` tools.
    ///
    /// `columns` is the widest tab's count rather than the active tab's, so the cells do not jump when
    /// the user switches tab; the ribbon's own margins and the hairlines between groups come off the
    /// available width first. A window so narrow that the share falls under the floor gives the floor,
    /// and the body scrolls instead of clipping a group.
    pub fn measure(ui: &egui::Ui, columns: usize) -> Self {
        let columns = columns.max(1) as f32;
        let gaps = (columns - 1.0) * (1.0 + 2.0 * STEP);
        let inner = (ui.available_width() - gaps - 2.0 * RIBBON_BUTTON_PADDING).max(0.0);
        Self { cell: (inner / columns).clamp(RIBBON_BUTTON_MIN_WIDTH, RIBBON_BUTTON_MAX_WIDTH) }
    }
}

/// The horizontal padding inside a ribbon cell, in points.
pub const RIBBON_BUTTON_PADDING: f32 = STEP;

/// The gap between a ribbon button's icon and its caption, in points.
///
/// The reference has more air here than a spacing step: the caption sits clearly under the glyph
/// rather than touching it, which is what makes a two-line caption still read as one label.
pub const RIBBON_CAPTION_GAP: f32 = 6.0;

/// The narrowest a ribbon tab may be, so a short name does not become a stamp.
pub const RIBBON_TAB_MIN_WIDTH: f32 = 64.0;

/// The height of the ribbon's tab row, in points.
pub const RIBBON_TAB_HEIGHT: f32 = 32.0;

/// The height of the caption band under a ribbon group, in points.
///
/// The band carries the 10.5 pt group name in capitals and is type-derived, so it is the one row
/// height that is not a multiple of [`STEP`]: a caption is a line of small type, not a control.
pub const RIBBON_CAPTION_HEIGHT: f32 = 14.0;

/// The height of the viewport bar at the bottom of the window, in points.
pub const STATUS_HEIGHT: f32 = 28.0;

/// The height of a side panel's header row, in points.
///
/// One control tall, like a header chip, so the title and the fold chevron sit on the same line the
/// header uses for its own buttons.
pub const PANEL_HEADER_HEIGHT: f32 = 26.0;

/// The width of a collapsed side panel's strip, in points.
pub const COLLAPSED_PANEL_WIDTH: f32 = 26.0;

/// The air between a floating cluster and the edge of the surface it floats over, in points.
pub const FLOATING_OFFSET: f32 = 12.0;

/// The rounding of a control, in pixels.
pub const RADIUS: u8 = 4;

/// The title of the window, and the word the header draws.
pub const PRODUCT_NAME: &str = "FurniCraft";

/// One theme variant: every chrome and text colour the window draws with.
///
/// The palette is the source of truth for both variants. [`DARK`] holds the values the consts above
/// name, so a widget never has to know which variant is active: the accessors below read the
/// [`egui::Visuals`] that [`apply_with`] installed, and the same call site draws correctly in dark
/// and light mode. The accent is data, so the settings window can replace it without a second
/// palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Whether this is a dark variant, as `egui` records it.
    pub dark: bool,
    /// The window frame behind the panels.
    pub chrome: Color32,
    /// The header, the ribbon body and the panel headers.
    pub surface: Color32,
    /// The panel background where content sits.
    pub panel: Color32,
    /// A control at rest, hovered and pressed.
    pub control: Color32,
    /// A hovered control.
    pub control_hover: Color32,
    /// A pressed control.
    pub control_active: Color32,
    /// A translucent cluster over the viewport.
    pub floating: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text: hints, units and group captions.
    pub text_dim: Color32,
    /// Text that cannot be acted on.
    pub text_disabled: Color32,
    /// A hairline between sections.
    pub line: Color32,
    /// The action accent.
    pub accent: Color32,
    /// The accent used as a filled background.
    pub accent_fill: Color32,
    /// A destructive action's text.
    pub danger: Color32,
    /// A warning text.
    pub warn: Color32,
}

impl Palette {
    /// The same palette with the user's accent.
    ///
    /// The filled accent is derived from the chosen colour, so a pale accent still carries white
    /// text: it is folded towards the dark end while the accent line itself stays as picked.
    pub fn with_accent(self, accent: Color32) -> Self {
        let fold = |channel: u8| ((u16::from(channel) * 2) / 3).min(255) as u8;
        Self { accent, accent_fill: Color32::from_rgb(fold(accent.r()), fold(accent.g()), fold(accent.b())), ..self }
    }
}

/// The dark palette the editor has drawn with since U38.
pub const DARK: Palette = Palette {
    dark: true,
    chrome: CHROME,
    surface: SURFACE,
    panel: PANEL,
    control: CONTROL,
    control_hover: CONTROL_HOVER,
    control_active: CONTROL_ACTIVE,
    floating: FLOATING,
    text: TEXT,
    text_dim: TEXT_DIM,
    text_disabled: TEXT_DISABLED,
    line: LINE,
    accent: ACCENT,
    accent_fill: ACCENT_FILL,
    danger: DANGER,
    warn: WARN,
};

/// The light palette: the same rhythm on paper instead of slate.
pub const LIGHT: Palette = Palette {
    dark: false,
    chrome: Color32::from_rgb(0xe6, 0xe9, 0xed),
    surface: Color32::from_rgb(0xf1, 0xf3, 0xf5),
    panel: Color32::from_rgb(0xfa, 0xfb, 0xfc),
    control: Color32::from_rgb(0xe2, 0xe6, 0xea),
    control_hover: Color32::from_rgb(0xd5, 0xda, 0xdf),
    control_active: Color32::from_rgb(0xc6, 0xcc, 0xd2),
    floating: Color32::from_rgba_premultiplied(0xf1, 0xf3, 0xf5, 224),
    text: Color32::from_rgb(0x1f, 0x23, 0x28),
    text_dim: Color32::from_rgb(0x5c, 0x63, 0x6b),
    text_disabled: Color32::from_rgb(0xa0, 0xa6, 0xad),
    line: Color32::from_rgb(0xcd, 0xd2, 0xd8),
    accent: ACCENT,
    accent_fill: ACCENT_FILL,
    danger: Color32::from_rgb(0xc0, 0x39, 0x2b),
    warn: Color32::from_rgb(0x8a, 0x62, 0x00),
};

/// The primary text colour of the active theme.
pub fn text(ui: &egui::Ui) -> Color32 {
    ui.visuals().override_text_color.unwrap_or(DARK.text)
}

/// The secondary text colour of the active theme.
pub fn text_dim(ui: &egui::Ui) -> Color32 {
    ui.visuals().weak_text_color.unwrap_or(DARK.text_dim)
}

/// The raised surface fill of the active theme.
pub fn surface(ui: &egui::Ui) -> Color32 {
    ui.visuals().window_fill
}

/// The panel fill of the active theme.
pub fn panel(ui: &egui::Ui) -> Color32 {
    ui.visuals().panel_fill
}

/// The darkest chrome of the active theme.
pub fn chrome(ui: &egui::Ui) -> Color32 {
    ui.visuals().extreme_bg_color
}

/// A control's resting fill.
pub fn control(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.inactive.bg_fill
}

/// A hovered control's fill.
pub fn control_hover(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.hovered.bg_fill
}

/// A pressed control's fill.
pub fn control_active(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.active.bg_fill
}

/// The translucent fill of a floating cluster.
pub fn floating(ui: &egui::Ui) -> Color32 {
    ui.visuals().faint_bg_color
}

/// A hairline between sections.
pub fn line(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}

/// The action accent.
pub fn accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().hyperlink_color
}

/// The accent as a filled background.
pub fn accent_fill(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// A destructive action's text.
pub fn danger(ui: &egui::Ui) -> Color32 {
    ui.visuals().error_fg_color
}

/// A warning text.
pub fn warn(ui: &egui::Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

/// Applies the dark FurniCraft visuals and spacing to `style`.
pub fn apply(style: &mut egui::Style) {
    apply_with(style, DARK);
}

/// Applies one palette and the shared spacing and type scale to `style`.
pub fn apply_with(style: &mut egui::Style, palette: Palette) {
    style.visuals = visuals(palette);
    style.spacing.item_spacing = vec2(STEP * 1.5, STEP);
    style.spacing.button_padding = vec2(STEP * 2.0, STEP);
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.indent = 14.0;
    style.spacing.interact_size = vec2(0.0, 20.0);
    style.spacing.icon_width = 14.0;
    style.spacing.icon_width_inner = 8.0;
    style.spacing.slider_width = 120.0;
    style.spacing.combo_width = 120.0;
    style.spacing.text_edit_width = 96.0;
    style.text_styles = text_styles();
    // A tooltip is read once and should not look like a panel.
    style.visuals.window_shadow =
        egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(120) };
    style.visuals.popup_shadow =
        egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(120) };
}

/// The type scale: dense enough for a ribbon, readable at 100 %.
fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    use TextStyle::{Body, Button, Heading, Monospace, Small};
    [
        (Heading, FontId::new(13.0, FontFamily::Proportional)),
        (Body, FontId::new(12.0, FontFamily::Proportional)),
        (Button, FontId::new(12.0, FontFamily::Proportional)),
        (Small, FontId::new(10.5, FontFamily::Proportional)),
        (Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into_iter()
    .collect()
}

/// The palette as `egui` expects it: one widget visual per interaction state.
fn visuals(palette: Palette) -> Visuals {
    let mut visuals = if palette.dark { Visuals::dark() } else { Visuals::light() };
    visuals.dark_mode = palette.dark;
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.line);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    visuals.faint_bg_color = palette.floating;
    visuals.extreme_bg_color = palette.chrome;
    visuals.code_bg_color = palette.chrome;
    visuals.override_text_color = Some(palette.text);
    visuals.weak_text_color = Some(palette.text_dim);
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.danger;
    visuals.selection = egui::style::Selection { bg_fill: palette.accent_fill, stroke: Stroke::new(1.0, palette.text) };
    // A dense editor draws its own separators; the default indent vline doubles them.
    visuals.indent_has_left_vline = false;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.text_cursor = egui::style::TextCursorStyle { stroke: Stroke::new(1.0, palette.text), ..Default::default() };
    visuals.widgets = widgets(palette);
    visuals
}

/// The five widget states every control shares.
fn widgets(palette: Palette) -> Widgets {
    Widgets {
        noninteractive: WidgetVisuals {
            bg_fill: palette.panel,
            weak_bg_fill: palette.panel,
            bg_stroke: Stroke::new(1.0, palette.line),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, palette.text),
            expansion: 0.0,
        },
        inactive: WidgetVisuals {
            bg_fill: palette.control,
            weak_bg_fill: palette.control,
            bg_stroke: Stroke::NONE,
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, palette.text),
            expansion: 0.0,
        },
        hovered: WidgetVisuals {
            bg_fill: palette.control_hover,
            weak_bg_fill: palette.control_hover,
            bg_stroke: Stroke::new(1.0, palette.line),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, palette.text),
            expansion: 0.0,
        },
        active: WidgetVisuals {
            bg_fill: palette.control_active,
            weak_bg_fill: palette.control_active,
            bg_stroke: Stroke::new(1.0, palette.accent),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, palette.text),
            expansion: 0.0,
        },
        open: WidgetVisuals {
            bg_fill: palette.control_active,
            weak_bg_fill: palette.control_active,
            bg_stroke: Stroke::new(1.0, palette.line),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, palette.text),
            expansion: 0.0,
        },
    }
}

/// Installs the dark theme on `ctx` for both variants and returns nothing a caller must keep.
///
/// `egui` keeps a dark and a light style; the application installs both so a later theme switch
/// never finds half a window on the default colours. [`install_with`] is the settings-aware entry
/// point; this one keeps the pre-U41 behaviour for tests and the first frame.
pub fn install(ctx: &egui::Context) {
    ctx.all_styles_mut(apply);
}

/// Installs both variants from the chosen theme preference and accent.
///
/// The active variant is `egui`'s business: it keeps the two styles apart and swaps them when the
/// preference changes, so the settings window only has to call this once per change. An accent is
/// applied to both palettes so a switch from dark to light keeps the same action colour.
pub fn install_with(ctx: &egui::Context, preference: egui::ThemePreference, accent: [u8; 3]) {
    let accent = Color32::from_rgb(accent[0], accent[1], accent[2]);
    let dark = DARK.with_accent(accent);
    let light = LIGHT.with_accent(accent);
    ctx.style_mut_of(egui::Theme::Dark, |style| apply_with(style, dark));
    ctx.style_mut_of(egui::Theme::Light, |style| apply_with(style, light));
    ctx.set_theme(preference);
}

/// A frame for one chrome surface: the fill and the hairline that closes it.
pub fn surface_frame(fill: Color32) -> egui::Frame {
    egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(6, 2))
}

/// A vertical hairline that separates two meaning groups, with breathing room on both sides.
///
/// `ui.separator()` inside a horizontal row stretches to the row and takes the widget stroke; a
/// chrome divider is shorter and quieter, so the header and the ribbon draw this one.
pub fn divider(ui: &mut egui::Ui) {
    divider_of_height(ui, 16.0);
}

/// A divider `height` points tall, for a row taller than the header's.
///
/// The ribbon's groups are separated by a line that runs the whole body — the tool row and the
/// caption band under it — because a line that stops halfway reads as a stray tick rather than as a
/// boundary between two groups.
pub fn divider_of_height(ui: &mut egui::Ui, height: f32) {
    ui.add_space(STEP);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, line(ui)));
    ui.add_space(STEP);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// The relative luminance of an opaque colour, ignoring the sRGB transfer curve.
    ///
    /// The editor needs a contrast floor, not a WCAG certificate: this is enough to catch a panel
    /// fill that swallows its own text.
    fn luminance(color: Color32) -> f32 {
        0.2126 * f32::from(color.r()) + 0.7152 * f32::from(color.g()) + 0.0722 * f32::from(color.b())
    }

    #[test]
    fn every_text_colour_reads_on_its_surface() {
        for (text, surface, name) in [
            (TEXT, PANEL, "body on a panel"),
            (TEXT, SURFACE, "body on a header"),
            (TEXT, CONTROL, "body on a control"),
            (TEXT_DIM, PANEL, "dim text on a panel"),
            (TEXT_DIM, SURFACE, "dim text on a header"),
        ] {
            let delta = (luminance(text) - luminance(surface)).abs();
            assert!(delta > 60.0, "{name} has too little contrast: {delta:.0}");
        }
        assert!(luminance(TEXT_DISABLED) < luminance(TEXT_DIM), "disabled text is quieter than dim text");
        assert!(luminance(TEXT) > luminance(TEXT_DIM), "body text is brighter than dim text");
    }

    #[test]
    fn the_accent_is_the_only_saturated_colour() {
        // Every chrome colour is neutral; a single channel out of line would tint the whole window.
        for color in [CHROME, SURFACE, PANEL, CONTROL, CONTROL_HOVER, CONTROL_ACTIVE, LINE, TEXT] {
            let (r, g, b) = (i32::from(color.r()), i32::from(color.g()), i32::from(color.b()));
            let spread = r.max(g).max(b) - r.min(g).min(b);
            assert!(spread <= 12, "{color:?} is not neutral");
        }
        let spread = |color: Color32| {
            let (r, g, b) = (i32::from(color.r()), i32::from(color.g()), i32::from(color.b()));
            r.max(g).max(b) - r.min(g).min(b)
        };
        assert!(spread(ACCENT) >= 100, "the accent has to read as an action, not as grey");
    }

    #[test]
    fn installing_the_theme_leaves_no_default_colour_behind() {
        let ctx = egui::Context::default();
        install(&ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let style = ctx.style_of(theme);
            assert_eq!(style.visuals.panel_fill, PANEL, "{theme:?} keeps the palette");
            assert_eq!(style.visuals.override_text_color, Some(TEXT), "{theme:?} keeps the text colour");
            assert_eq!(style.spacing.item_spacing.y, STEP, "{theme:?} keeps the rhythm");
            assert_eq!(style.text_styles.get(&TextStyle::Body).map(|font| font.size), Some(12.0));
        }
    }

    #[test]
    fn the_spacing_scale_is_one_step() {
        assert_eq!(STEP, 4.0, "the whole window is laid out on the 4-point grid");
        assert_eq!(HEADER_HEIGHT % STEP, 0.0, "the header sits on the grid");
        assert_eq!(HEADER_CONTROL_HEIGHT % STEP, 2.0, "a header control is half a step off, so it centres");
        const { assert!(HEADER_CONTROL_HEIGHT < HEADER_HEIGHT) };
        assert_eq!(STATUS_HEIGHT % STEP, 0.0, "the viewport bar sits on the grid");
        assert_eq!(RIBBON_ROW_HEIGHT % STEP, 0.0, "the ribbon tool row sits on the grid");
        assert_eq!(RIBBON_TAB_HEIGHT % STEP, 0.0, "the ribbon tab row sits on the grid");
        assert_eq!(RIBBON_CAPTION_HEIGHT, 14.0, "the caption band is one line of small type");
        // The ribbon's cell band: the floor is the longest Russian caption plus the cell's padding,
        // and the ceiling is what a maximised window may reach, so the two cannot cross.
        const CELL_FLOOR: f32 = RIBBON_BUTTON_MIN_WIDTH;
        const CELL_CEILING: f32 = RIBBON_BUTTON_MAX_WIDTH;
        const { assert!(CELL_FLOOR < CELL_CEILING) };
        const { assert!(CELL_FLOOR >= 2.0 * RIBBON_BUTTON_PADDING + 98.0) };
        // The chrome rows keep one order: the header is busiest, the bar is the quietest, and the
        // ribbon is the tallest because it carries an icon over a caption.
        const { assert!(HEADER_HEIGHT > STATUS_HEIGHT) };
        const { assert!(HEADER_HEIGHT < RIBBON_ROW_HEIGHT) };
        const { assert!(RIBBON_CAPTION_HEIGHT < RIBBON_TAB_HEIGHT) };
        const { assert!(RIBBON_TAB_HEIGHT < RIBBON_ROW_HEIGHT) };
        // A body is a tool row plus the caption band under it; the owner's reference is about 90 pt
        // once the two margins are added.
        const RIBBON_BODY: f32 = RIBBON_ROW_HEIGHT + RIBBON_CAPTION_HEIGHT;
        const { assert!(RIBBON_BODY >= 60.0 && RIBBON_BODY <= 72.0) };
    }
}
