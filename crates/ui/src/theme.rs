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

/// The height of a ribbon group's tool row, in points.
pub const RIBBON_ROW_HEIGHT: f32 = 48.0;

/// The height of the ribbon's tab row, in points.
pub const RIBBON_TAB_HEIGHT: f32 = 28.0;

/// The height of the caption band under a ribbon group, in points.
///
/// The band carries the 10.5 pt group name in capitals and is type-derived, so it is the one row
/// height that is not a multiple of [`STEP`]: a caption is a line of small type, not a control.
pub const RIBBON_CAPTION_HEIGHT: f32 = 14.0;

/// The height of the viewport bar at the bottom of the window, in points.
pub const STATUS_HEIGHT: f32 = 28.0;

/// The width of a collapsed side panel's strip, in points.
pub const COLLAPSED_PANEL_WIDTH: f32 = 26.0;

/// The rounding of a control, in pixels.
pub const RADIUS: u8 = 4;

/// The title of the window, and the word the header draws.
pub const PRODUCT_NAME: &str = "FurniCraft";

/// Applies the FurniCraft visuals and spacing to `style`.
///
/// The same function serves both theme variants of `egui`: the editor is dark, so the light variant
/// takes the same colours rather than a second palette nobody asked for. Text sizes come from the
/// desktop convention: 12 pt body, 11 pt secondary, 13 pt headings, so a 150 % display does not turn
/// the ribbon into a paragraph.
pub fn apply(style: &mut egui::Style) {
    style.visuals = visuals();
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
fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = PANEL;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    visuals.faint_bg_color = Color32::from_rgb(0x2b, 0x2f, 0x36);
    visuals.extreme_bg_color = CHROME;
    visuals.code_bg_color = CHROME;
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(TEXT_DIM);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = DANGER;
    visuals.selection = egui::style::Selection { bg_fill: ACCENT_FILL, stroke: Stroke::new(1.0, TEXT) };
    // A dense editor draws its own separators; the default indent vline doubles them.
    visuals.indent_has_left_vline = false;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.text_cursor = egui::style::TextCursorStyle { stroke: Stroke::new(1.0, TEXT), ..Default::default() };
    visuals.widgets = widgets();
    visuals
}

/// The five widget states every control shares.
fn widgets() -> Widgets {
    Widgets {
        noninteractive: WidgetVisuals {
            bg_fill: PANEL,
            weak_bg_fill: PANEL,
            bg_stroke: Stroke::new(1.0, LINE),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        inactive: WidgetVisuals {
            bg_fill: CONTROL,
            weak_bg_fill: CONTROL,
            bg_stroke: Stroke::NONE,
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        hovered: WidgetVisuals {
            bg_fill: CONTROL_HOVER,
            weak_bg_fill: CONTROL_HOVER,
            bg_stroke: Stroke::new(1.0, LINE),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        active: WidgetVisuals {
            bg_fill: CONTROL_ACTIVE,
            weak_bg_fill: CONTROL_ACTIVE,
            bg_stroke: Stroke::new(1.0, ACCENT),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        open: WidgetVisuals {
            bg_fill: CONTROL_ACTIVE,
            weak_bg_fill: CONTROL_ACTIVE,
            bg_stroke: Stroke::new(1.0, LINE),
            corner_radius: CornerRadius::same(RADIUS),
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
    }
}

/// Installs the theme on `ctx` for both variants and returns nothing a caller must keep.
///
/// `egui` keeps a dark and a light style; the system preference decides which one is active, so the
/// programme applies the palette to both and the operating system cannot leave half a window with
/// default colours.
pub fn install(ctx: &egui::Context) {
    ctx.all_styles_mut(apply);
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
    ui.add_space(STEP);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 16.0), egui::Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, LINE));
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
        assert_eq!(STATUS_HEIGHT % STEP, 0.0, "the viewport bar sits on the grid");
        assert_eq!(RIBBON_ROW_HEIGHT % STEP, 0.0, "the ribbon tool row sits on the grid");
        assert_eq!(RIBBON_TAB_HEIGHT % STEP, 0.0, "the ribbon tab row sits on the grid");
        assert_eq!(RIBBON_CAPTION_HEIGHT, 14.0, "the caption band is one line of small type");
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
