//! Themed icon assets and the two icon buttons.
//!
//! `assets/icons/**` is embedded at compile time, one module per theme. A zero-byte file is a
//! documented placeholder, so an icon button whose artwork has not been drawn yet falls back to its
//! text label and the surface stays usable. A drawn icon is rasterised by the `egui_extras` SVG
//! loader at `pt × pixels_per_point`, tinted with the widget's foreground colour and uploaded as a
//! `TextureHandle`, so one white master serves every theme (`assets/README.md`, ADR 0055). Its ink
//! stays inside the fixed `3..21` footprint and covers 20–45 % of it; the tests below read both off
//! the raster (`assets/icons/STYLE_GUIDE.md` §7.4).

use egui::{
    Button, Color32, CornerRadius, FontFamily, FontId, Response, Role, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, Vec2, WidgetInfo, pos2,
};

/// Point size of an inline or menu icon, before the UI scale turns it into pixels.
pub const ICON_POINT_SIZE: f32 = 16.0;

/// Point size of a toolbar tool icon, before the UI scale turns it into pixels.
pub const TOOL_ICON_POINT_SIZE: f32 = 20.0;

/// Point size of a ribbon button's icon, before the UI scale turns it into pixels.
pub const RIBBON_ICON_POINT_SIZE: f32 = 22.0;

/// Point size of a ribbon button's caption, before the UI scale turns it into pixels.
pub const RIBBON_CAPTION_POINT_SIZE: f32 = 10.5;

/// The most rows a ribbon caption may take before it elides.
///
/// One: the owner's rule is that a caption which does not fit is cut with `…`, never wrapped into a
/// second line that would make one cell taller than its neighbours. The reference does wrap one long
/// tool name, but it also pays for that with its own row height; ours stays one line and one cell.
pub const RIBBON_CAPTION_ROWS: usize = 1;

/// The directory that owns an icon: the surface that shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Theme {
    /// The ribbon's *Sketch* tab.
    Contour,
    /// The ribbon's *Model* tab, primitives group.
    Primitives,
    /// The ribbon's *Model* tab, bodies group.
    Bodies,
    /// The ribbon's *Scene* tab.
    Scene,
    /// The ribbon's *View* tab and the viewport strip.
    View,
    /// Menus, dialogs, the header and the shortcut reference.
    Actions,
    /// The window row of the header: minimize, maximize, restore, close.
    Window,
}

impl Theme {
    /// Every theme, in the order of the asset tree.
    pub const ALL: [Self; 7] =
        [Self::Contour, Self::Primitives, Self::Bodies, Self::Scene, Self::View, Self::Actions, Self::Window];

    /// The directory name of this theme, which is also the first URI segment of its icons.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contour => "contour",
            Self::Primitives => "primitives",
            Self::Bodies => "bodies",
            Self::Scene => "scene",
            Self::View => "view",
            Self::Actions => "actions",
            Self::Window => "window",
        }
    }

    /// Every icon in this theme, for tests and asset audits.
    pub const fn icons(self) -> &'static [Icon] {
        match self {
            Self::Contour => contour::ALL,
            Self::Primitives => primitives::ALL,
            Self::Bodies => bodies::ALL,
            Self::Scene => scene::ALL,
            Self::View => view::ALL,
            Self::Actions => actions::ALL,
            Self::Window => window::ALL,
        }
    }
}

/// One embedded icon: its theme, its file stem and its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    theme: Theme,
    name: &'static str,
    uri: &'static str,
    asset: &'static [u8],
}

impl Icon {
    const fn new(theme: Theme, name: &'static str, uri: &'static str, asset: &'static [u8]) -> Self {
        Self { theme, name, uri, asset }
    }

    /// The theme directory that owns this icon.
    pub const fn theme(self) -> Theme {
        self.theme
    }

    /// The file stem, which is the icon id inside its theme.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The stable `bytes://` URI that names this icon in the texture cache.
    pub const fn uri(self) -> &'static str {
        self.uri
    }

    /// The embedded SVG file, empty while the icon is a placeholder.
    pub const fn asset(self) -> &'static [u8] {
        self.asset
    }

    /// Whether the asset carries a drawing; a placeholder is an empty file.
    pub const fn is_drawn(self) -> bool {
        !self.asset.is_empty()
    }
}

/// Declares one theme module: every icon file as a constant, plus `ALL` for audits.
macro_rules! icons {
    ($theme:ident, $theme_name:literal, $module:ident { $($constant:ident => $file:literal),* $(,)? }) => {
        #[doc = concat!("The `", $theme_name, "` theme.")]
        pub mod $module {
            use super::{Icon, Theme};

            $(
                #[doc = concat!("The `", $theme_name, "/", $file, "` icon.")]
                pub const $constant: Icon = Icon::new(
                    Theme::$theme,
                    $file,
                    concat!("bytes://icons/", $theme_name, "/", $file, ".svg"),
                    include_bytes!(concat!(
                        "../../../assets/icons/",
                        $theme_name,
                        "/",
                        $file,
                        ".svg"
                    )),
                );
            )*

            /// Every icon in this theme, for tests and asset audits.
            pub const ALL: &[Icon] = &[$($constant),*];
        }
    };
}

icons!(Contour, "contour", contour {
    LINE => "line",
    POLYLINE => "polyline",
    ARC => "arc",
    ARC_CENTRE => "arc-centre",
    RECTANGLE => "rectangle",
    RECTANGLE_CENTRE => "rectangle-centre",
    RECTANGLE_CENTRE_DIRECTED => "rectangle-centre-directed",
    CIRCLE => "circle",
    CIRCLE_THREE_POINTS => "circle-three-points",
    POLYGON => "polygon",
    CLOSE => "close",
    CANCEL => "cancel",
});

icons!(Primitives, "primitives", primitives {
    CUBE => "cube",
    CYLINDER => "cylinder",
    SPHERE => "sphere",
    CONE => "cone",
});

icons!(Bodies, "bodies", bodies {
    EXTRUDE => "extrude",
    REVOLVE => "revolve",
    SWEEP => "sweep",
});

icons!(Scene, "scene", scene {
    PANEL => "panel",
    ASSEMBLY => "assembly",
    CAMERA => "camera",
    LIGHT => "light",
});

icons!(View, "view", view {
    ORTHOGRAPHIC => "orthographic",
    PERSPECTIVE => "perspective",
    FRAME_ALL => "frame-all",
    HOME => "home",
    GRID_SNAP => "grid-snap",
    ORBIT => "orbit",
    PAN => "pan",
    ZOOM => "zoom",
    LAYOUT => "layout",
    SHADED => "shaded",
    SHADED_EDGES => "shaded-edges",
    WIREFRAME => "wireframe",
    XRAY => "xray",
    TRANSLUCENT => "translucent",
    SLIDERS => "sliders",
});

icons!(Actions, "actions", actions {
    CONFIRM => "confirm",
    DISMISS => "dismiss",
    CANCEL => "cancel",
    UNDO => "undo",
    REDO => "redo",
    DELETE => "delete",
    ADD => "add",
    REMOVE => "remove",
    DUPLICATE => "duplicate",
    RENAME => "rename",
    COPY => "copy",
    PASTE => "paste",
    FILE_NEW => "file-new",
    FILE_OPEN => "file-open",
    FILE_SAVE => "file-save",
    FILE_EXPORT => "file-export",
    FILE_IMPORT => "file-import",
    PRINT => "print",
    SETTINGS => "settings",
    SEARCH => "search",
    VISIBLE => "visible",
    HIDDEN => "hidden",
    LOCK => "lock",
    UNLOCK => "unlock",
    EXPAND => "expand",
    COLLAPSE => "collapse",
    WARNING => "warning",
    ERROR => "error",
    INFO => "info",
    HELP => "help",
});

icons!(Window, "window", window {
    MINIMIZE => "minimize",
    MAXIMIZE => "maximize",
    RESTORE => "restore",
    CLOSE => "close",
});

/// The product mark, drawn by the header next to the product name.
///
/// It lives in `assets/logo.svg`, not in a theme: a mark is not an icon of a surface, and it keeps
/// its own drawing even when a theme is renamed. Like an icon it is a white master that the theme
/// multiplies, so one file serves every variant.
pub const LOGO: &[u8] = include_bytes!("../../../assets/logo.svg");

/// The stable texture URI of [`LOGO`].
pub const LOGO_URI: &str = "bytes://logo.svg";

/// A square icon button at an explicit point size, for the header's own rhythm.
///
/// The tool strip and the menus keep [`ICON_POINT_SIZE`]; a header row is taller, so its buttons ask
/// for their size instead of inheriting the inline one.
pub fn icon_button_at(
    ui: &mut Ui,
    icon: Icon,
    point_size: f32,
    label: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    icon_button_sized(ui, icon, point_size + 2.0 * ui.spacing().button_padding.y, point_size, label, enabled, selected)
}

/// A square icon button of a given `cell` size, with the glyph at `point_size` inside it.
///
/// A header row is laid out on its own rhythm: the text button and the icon buttons beside it have to
/// be one height, so the caller names the cell and the glyph separately instead of letting the icon
/// decide the cell. A placeholder asset takes the same cell and draws its label.
pub fn icon_button_sized(
    ui: &mut Ui,
    icon: Icon,
    cell: f32,
    point_size: f32,
    label: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    if !icon.is_drawn() {
        return ui
            .add_enabled(enabled, Button::new(label).min_size(Vec2::splat(cell)).selected(selected))
            .on_hover_text(label);
    }
    let response = icon_widget_sized(ui, icon, cell, point_size, label, enabled, selected);
    if label.is_empty() { response } else { response.on_hover_text(label) }
}

/// Paints the product mark as a square image in `color`.
///
/// The mark is not a button: it carries no action and takes no focus. It is rasterised through the
/// same cache as an icon so a theme change re-tints it instead of loading a second file.
pub fn mark(ui: &mut Ui, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect)
        && let Some(texture) = logo_texture(ui.ctx(), size, color)
    {
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }
    response
}

/// The rasterised mark at `size` points and `color`, cached like an icon.
fn logo_texture(ctx: &egui::Context, point_size: f32, color: Color32) -> Option<TextureHandle> {
    let px = pixel_size(point_size, ctx.pixels_per_point());
    let id = egui::Id::unique(("furnicraft.logo", px, color.to_array()));
    if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = egui_extras::image::load_svg_bytes_with_size(
        LOGO,
        egui::load::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: false },
        &Default::default(),
    )
    .ok()?;
    let texture = ctx.load_texture(LOGO_URI, tint(image, color), TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Draws `icon` as a square button of [`ICON_POINT_SIZE`] in the widget's foreground colour.
///
/// An empty placeholder asset draws `label` as the button text instead, so a surface whose artwork
/// is still arriving stays usable. For a drawn icon `label` is the tooltip and the accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    if !icon.is_drawn() {
        return ui.button(label);
    }
    let response = icon_widget(ui, icon, ICON_POINT_SIZE, label, true, false);
    if label.is_empty() { response } else { response.on_hover_text(label) }
}

/// A tool button: a [`TOOL_ICON_POINT_SIZE`] icon with a hint that names the tool and its shortcut.
///
/// The hint is the tooltip of a drawn icon. An icon whose asset is still a placeholder becomes the
/// button text itself, shortcut included, because a tooltip alone would leave the button unreadable.
/// `enabled` disables the button without changing either face, so a panel can keep its layout while
/// a tool is unavailable. `on` marks the tool that owns the next click: the button draws pressed with
/// the theme's selection fill, which is a widget state and not a second SVG asset.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, shortcut: Option<&str>, enabled: bool, on: bool) -> Response {
    let hint = tool_hint(label, shortcut);
    if !icon.is_drawn() {
        return ui.add_enabled(enabled, Button::new(hint.clone()).selected(on)).on_hover_text(hint);
    }
    icon_widget(ui, icon, TOOL_ICON_POINT_SIZE, &hint, enabled, on).on_hover_text(hint)
}

/// A tool button with a short face and a longer hint.
///
/// A floating cluster has to stay narrow, so its placeholder face uses the short `caption` while the
/// tooltip keeps the full name. A drawn icon ignores the caption for its face and uses `hint` only.
pub fn tool_button_labelled(ui: &mut Ui, icon: Icon, caption: &str, hint: &str, enabled: bool, on: bool) -> Response {
    if !icon.is_drawn() {
        return ui.add_enabled(enabled, Button::new(caption).selected(on)).on_hover_text(hint);
    }
    icon_widget(ui, icon, TOOL_ICON_POINT_SIZE, hint, enabled, on).on_hover_text(hint)
}

/// The readable hint of a tool button: the label and, when it has one, its shortcut.
pub fn tool_hint(label: &str, shortcut: Option<&str>) -> String {
    match shortcut {
        Some(shortcut) if !shortcut.is_empty() => format!("{label}  ({shortcut})"),
        _ => label.to_owned(),
    }
}

/// A ribbon button: a [`RIBBON_ICON_POINT_SIZE`] icon over a [`RIBBON_CAPTION_POINT_SIZE`] caption.
///
/// The ribbon is the one surface with a row tall enough to name a tool under its glyph, so a tool
/// reads without a hover and a group reads without a heading above it. `caption` is the word painted
/// under the icon and `hint` is the tooltip and the accessible name, which can be longer than the
/// caption — the polyline's caption is its name while its hint also names the arc gesture. `on` marks
/// the tool that owns the next click, and `enabled` keeps the face while a tool is unavailable.
///
/// The cell is `width` wide and [`theme::RIBBON_ROW_HEIGHT`] tall and comes from the row's one
/// measurement (ADR 0067), so every tool of the ribbon shares it however long its caption is. A
/// caption wider than the cell's inner width is elided with `…` rather than allowed to widen the
/// button. An empty placeholder asset draws the caption as the button's text in the same cell, so a
/// half-drawn icon set keeps the rhythm instead of leaving a ragged row.
pub fn ribbon_button(
    ui: &mut Ui,
    width: f32,
    icon: Icon,
    caption: &str,
    hint: &str,
    enabled: bool,
    on: bool,
) -> Response {
    let inner_width = (width - 2.0 * crate::theme::RIBBON_BUTTON_PADDING).max(0.0);
    let galley = caption_galley(ui, caption, inner_width);
    let size = Vec2::new(width, crate::theme::RIBBON_ROW_HEIGHT);
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            if ui.is_rect_visible(rect) {
                let (fill, stroke, color) = ribbon_face(ui, &response, on);
                ui.painter().rect(rect, CornerRadius::same(crate::theme::RADIUS), fill, stroke, StrokeKind::Inside);
                // The glyph and its caption are one block, centred in the cell, so a cell never looks
                // top-heavy and a one-line caption and a two-line one share their optical centre.
                let block = if icon.is_drawn() {
                    RIBBON_ICON_POINT_SIZE + crate::theme::RIBBON_CAPTION_GAP + galley.size().y
                } else {
                    galley.size().y
                };
                let top = rect.center().y - block / 2.0;
                if icon.is_drawn() {
                    let icon_rect = egui::Rect::from_min_size(
                        pos2(rect.center().x - RIBBON_ICON_POINT_SIZE / 2.0, top),
                        Vec2::splat(RIBBON_ICON_POINT_SIZE),
                    );
                    paint_icon(ui, icon, icon_rect, RIBBON_ICON_POINT_SIZE, color);
                    let caption_pos = pos2(
                        rect.center().x - galley.size().x / 2.0,
                        icon_rect.bottom() + crate::theme::RIBBON_CAPTION_GAP,
                    );
                    ui.painter().galley(caption_pos, galley, color);
                } else {
                    // A placeholder asset has no glyph, so its caption takes the centre the glyph
                    // would have had and the cell stays the shape the drawn buttons give it.
                    let caption_pos = pos2(rect.center().x - galley.size().x / 2.0, top);
                    ui.painter().galley(caption_pos, galley, color);
                }
            }
            response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), on, hint));
            response
        })
        .inner;
    if hint.is_empty() { response } else { response.on_hover_text(hint) }
}

/// The caption of a ribbon cell, laid out for the fixed inner width.
///
/// The layout is built once for the resting face and painted in the state's foreground, so a hover
/// or a press recolours it instead of re-rounding the words. A caption that does not fit its cell is
/// elided with `…` — the owner's rule — rather than widening the button or leaving the row.
fn caption_galley(ui: &mut Ui, caption: &str, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        caption.to_owned(),
        FontId::new(RIBBON_CAPTION_POINT_SIZE, FontFamily::Proportional),
        Color32::PLACEHOLDER,
        max_width,
    );
    job.wrap.max_rows = RIBBON_CAPTION_ROWS;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Paints one cell's frame and fill for its state.
///
/// The ribbon's button, the icon-only tool button and a labelled button share this, so a hover or a
/// press cannot look like three different controls depending on which surface drew it.
fn paint_cell(ui: &Ui, rect: egui::Rect, visuals: &egui::style::WidgetVisuals) {
    ui.painter().rect(
        rect.expand(visuals.expansion),
        visuals.corner_radius,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        StrokeKind::Inside,
    );
}

/// The face of a ribbon button: a quiet cell at rest, a hover fill, and the accent once armed.
///
/// The reference keeps its tool row calm — the panel's own fill, with a plinth only under the pointer
/// or under the armed tool — so a row of eight tools reads as one surface instead of a keyboard. The
/// cell's size is fixed either way, so a tool that is not hovered does not move its neighbours.
fn ribbon_face(ui: &Ui, response: &Response, on: bool) -> (Color32, Stroke, Color32) {
    if on {
        let visuals = ui.visuals().selection;
        return (visuals.bg_fill, visuals.stroke, Color32::WHITE);
    }
    if response.is_pointer_button_down_on() {
        return (crate::theme::control_active(ui), Stroke::NONE, crate::theme::text(ui));
    }
    if response.hovered() {
        return (crate::theme::control_hover(ui), Stroke::NONE, crate::theme::text(ui));
    }
    (Color32::TRANSPARENT, Stroke::NONE, crate::theme::text(ui))
}

/// Paints `icon` inside `rect` at `point_size`, tinted with `color`, without a button frame.
///
/// A folded side panel draws its icon in the middle of a 26 pt strip where a button would not fit;
/// the raster path is the same one the buttons use, so the asset, the cache and the theme colour are
/// shared. A placeholder asset draws nothing.
pub fn draw_icon(ui: &Ui, icon: Icon, rect: egui::Rect, point_size: f32, color: Color32) {
    if icon.is_drawn() {
        paint_icon(ui, icon, rect, point_size, color);
    }
}

/// Paints `icon` inside `rect` at `point_size`, tinted with `color`.
fn paint_icon(ui: &Ui, icon: Icon, rect: egui::Rect, point_size: f32, color: Color32) {
    let px = pixel_size(point_size, ui.pixels_per_point());
    if let Some(texture) = icon_texture(ui.ctx(), icon, px, color) {
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }
}

/// One square button that paints a rasterised icon, its frame following the widget state.
///
/// The frame and the foreground colour come from the same [`egui::WidgetVisuals`] a text button
/// would use, so hover, press, selection and the disabled opacity need no second styling path.
fn icon_widget(ui: &mut Ui, icon: Icon, point_size: f32, label: &str, enabled: bool, selected: bool) -> Response {
    let cell = point_size + 2.0 * ui.spacing().button_padding.y;
    icon_widget_sized(ui, icon, cell, point_size, label, enabled, selected)
}

/// One cell of `cell` points that paints a rasterised `point_size` icon, its frame following the state.
///
/// The cell and the glyph are separate because a header row and a tool strip disagree about how much
/// air a button carries: the header wants one height for a text button and an icon button, the strip
/// lets the glyph decide.
fn icon_widget_sized(
    ui: &mut Ui,
    icon: Icon,
    cell: f32,
    point_size: f32,
    label: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact_selectable(&response, selected);
            paint_cell(ui, rect, &visuals);
            let icon_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(point_size));
            paint_icon(ui, icon, icon_rect, point_size, visuals.fg_stroke.color);
        }
        response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
        response
    })
    .inner
}

/// The raster side of one icon at this UI scale, in pixels; never zero and bounded.
fn pixel_size(point_size: f32, pixels_per_point: f32) -> u32 {
    let px = (point_size * pixels_per_point).round();
    if !px.is_finite() || px < 1.0 {
        return 1;
    }
    if px > 512.0 { 512 } else { px as u32 }
}

/// The cached texture of `icon` at `px` pixels and `color`, rasterising it on the first request.
///
/// The colour is baked into the uploaded pixels rather than applied as a paint tint, so the theme
/// colour is what the texture contains and a hover or press only changes the cache key.
fn icon_texture(ctx: &egui::Context, icon: Icon, px: u32, color: Color32) -> Option<TextureHandle> {
    let id = egui::Id::unique(("furnicraft.icon", icon.uri(), px, color.to_array()));
    if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = egui_extras::image::load_svg_bytes_with_size(
        icon.asset(),
        egui::load::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: false },
        &Default::default(),
    )
    .ok()?;
    let texture = ctx.load_texture(icon.uri(), tint(image, color), TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Multiplies a white master by `color`, the same product the painter would apply to the vertex.
fn tint(mut image: egui::ColorImage, color: Color32) -> egui::ColorImage {
    for pixel in &mut image.pixels {
        *pixel = Color32::from_rgba_premultiplied(
            multiply(pixel.r(), color.r()),
            multiply(pixel.g(), color.g()),
            multiply(pixel.b(), color.b()),
            multiply(pixel.a(), color.a()),
        );
    }
    image
}

/// One channel of a premultiplied colour times the same channel of a tint, rounded.
fn multiply(channel: u8, tint: u8) -> u8 {
    ((u16::from(channel) * u16::from(tint) + 127) / 255) as u8
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashSet;

    use egui::epaint::{ClippedShape, ImageData, Shape};

    use super::*;

    /// A minimal valid white-ink drawing, used where the real files are still placeholders.
    const SAMPLE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" fill="#fff"/></svg>"##;

    /// The 24-unit design grid.
    const GRID: f32 = 24.0;

    /// The fixed footprint of the style guide, in grid units: `3..21` on both axes.
    const FOOTPRINT_MIN: f32 = 3.0;
    const FOOTPRINT_MAX: f32 = 21.0;

    /// The raster size the footprint and coverage are measured at.
    const AUDIT_PX: usize = 64;

    /// What one drawn icon measures on the grid.
    struct Footprint {
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        /// Alpha-weighted ink as a fraction of the fixed 18 × 18 footprint.
        coverage: f32,
    }

    /// Rasterises `icon` and measures its ink bounds and tone against the fixed footprint.
    ///
    /// A pixel is ink above alpha 16, the threshold `assets/icons/AUDIT.md` used. Coverage is
    /// alpha-weighted, so a passive face at 15 % adds a sixth of its area rather than a solid block.
    fn footprint(icon: Icon) -> Footprint {
        let image = egui_extras::image::load_svg_bytes_with_size(
            icon.asset(),
            egui::load::SizeHint::Size {
                width: AUDIT_PX as u32,
                height: AUDIT_PX as u32,
                maintain_aspect_ratio: false,
            },
            &Default::default(),
        )
        .expect("a drawn icon rasterises");
        assert_eq!(image.size, [AUDIT_PX, AUDIT_PX], "the audit raster is square");
        let unit = GRID / AUDIT_PX as f32;
        let mut ink = 0.0_f64;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (AUDIT_PX, AUDIT_PX, 0_usize, 0_usize);
        for (index, pixel) in image.pixels.iter().enumerate() {
            ink += f64::from(pixel.a()) / 255.0 * f64::from(unit) * f64::from(unit);
            if pixel.a() <= 16 {
                continue;
            }
            let x = index % AUDIT_PX;
            let y = index / AUDIT_PX;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        if max_x < min_x {
            return Footprint { min_x: 0.0, max_x: 0.0, min_y: 0.0, max_y: 0.0, coverage: 0.0 };
        }
        let footprint = f64::from(FOOTPRINT_MAX - FOOTPRINT_MIN).powi(2);
        Footprint {
            min_x: min_x as f32 * unit,
            max_x: (max_x + 1) as f32 * unit,
            min_y: min_y as f32 * unit,
            max_y: (max_y + 1) as f32 * unit,
            coverage: (ink / footprint) as f32,
        }
    }

    /// An icon whose asset is empty, like every placeholder in the tree today.
    fn placeholder() -> Icon {
        Icon::new(Theme::Actions, "confirm", "bytes://icons/actions/confirm.svg", &[])
    }

    /// An icon with a real drawing, for the rasteriser tests.
    fn drawn() -> Icon {
        Icon::new(Theme::Actions, "confirm", "bytes://icons/actions/confirm.svg", SAMPLE_SVG)
    }

    /// Collects the text of every text shape, including nested ones.
    fn drawn_text(shapes: &[ClippedShape]) -> Vec<String> {
        fn walk(shape: &Shape, out: &mut Vec<String>) {
            match shape {
                Shape::Text(text) => out.push(text.galley.text().to_owned()),
                Shape::Vec(shapes) => {
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

    #[test]
    fn every_icon_uri_names_its_theme_and_file() {
        let mut seen = HashSet::new();
        for theme in Theme::ALL {
            for icon in theme.icons() {
                assert_eq!(icon.uri(), format!("bytes://icons/{}/{}.svg", icon.theme().as_str(), icon.name()));
                assert!(seen.insert(icon.uri()), "duplicate icon uri {}", icon.uri());
            }
        }
        assert_eq!(seen.len(), 72);
    }

    #[test]
    fn every_drawn_asset_is_a_24_grid_svg() {
        for theme in Theme::ALL {
            for icon in theme.icons() {
                if !icon.is_drawn() {
                    continue;
                }
                let svg = std::str::from_utf8(icon.asset()).expect("an icon asset is UTF-8");
                let name = format!("{}/{}", icon.theme().as_str(), icon.name());
                assert!(svg.starts_with("<svg"), "{name} does not start with an SVG root");
                assert!(svg.contains("viewBox=\"0 0 24 24\""), "{name} is not drawn on the 24 grid");
                assert!(svg.contains("#fff"), "{name} carries no white ink");
                for forbidden in [
                    "<text",
                    "<image",
                    "<style",
                    "<defs",
                    "<mask",
                    "<clipPath",
                    "<filter",
                    "<linearGradient",
                    "<radialGradient",
                    " opacity=\"",
                ] {
                    assert!(!svg.contains(forbidden), "{name} contains {forbidden}");
                }
                let mut at = 0;
                while let Some(found) = svg[at..].find("fill-opacity=\"") {
                    let start = at + found + "fill-opacity=\"".len();
                    let rest = &svg[start..];
                    let end = rest.find('"').expect("a fill-opacity attribute is quoted");
                    let value: f32 = rest[..end].parse().expect("fill-opacity is a number");
                    assert!(
                        (0.12..=0.18).contains(&value) || (0.45..=0.60).contains(&value),
                        "{name} paints a fill at {value}, outside the passive and result bands"
                    );
                    at = start + end;
                }
                let raster = egui_extras::image::load_svg_bytes_with_size(
                    icon.asset(),
                    egui::load::SizeHint::Width(16),
                    &Default::default(),
                );
                assert!(raster.is_ok(), "{name} does not rasterise: {:?}", raster.err());
            }
        }
    }

    #[test]
    fn every_drawn_icon_holds_its_footprint_and_coverage() {
        for theme in Theme::ALL {
            for icon in theme.icons() {
                if !icon.is_drawn() {
                    continue;
                }
                let name = format!("{}/{}", icon.theme().as_str(), icon.name());
                let ink = footprint(*icon);
                assert!(
                    ink.min_x >= FOOTPRINT_MIN
                        && ink.max_x <= FOOTPRINT_MAX
                        && ink.min_y >= FOOTPRINT_MIN
                        && ink.max_y <= FOOTPRINT_MAX,
                    "{name} leaves the 3..21 footprint: {:.2}..{:.2} x {:.2}..{:.2}",
                    ink.min_x,
                    ink.max_x,
                    ink.min_y,
                    ink.max_y
                );
                assert!(
                    (0.20..=0.45).contains(&ink.coverage),
                    "{name} covers {:.1}% of the footprint, outside 20..45%",
                    ink.coverage * 100.0
                );
                for px in [16, 20] {
                    let raster = egui_extras::image::load_svg_bytes_with_size(
                        icon.asset(),
                        egui::load::SizeHint::Width(px),
                        &Default::default(),
                    )
                    .expect("a drawn icon rasterises small");
                    assert!(raster.pixels.iter().any(|pixel| pixel.a() > 0), "{name} is empty at {px} px");
                }
            }
        }
    }

    #[test]
    fn a_ribbon_cell_is_one_size_for_every_caption() {
        // The owner's complaint: the cell grew with its caption. One cell for a short caption, a long
        // one and an elided one, so a row of tools is one pitch.
        const { assert!(RIBBON_CAPTION_ROWS == 1) };
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let cell = crate::theme::RIBBON_BUTTON_MAX_WIDTH;
        let mut sizes = Vec::new();
        for caption in ["Arc", "Polyline", "Rectangle", "Orthographic", "Прямоугольник"] {
            let mut size = Vec2::ZERO;
            let output = ctx.run_ui(Default::default(), |ui| {
                let response = ribbon_button(ui, cell, drawn(), caption, caption, true, false);
                size = response.rect.size();
            });
            output.drop_without_applying_deltas();
            sizes.push((caption, size));
        }
        let expected = Vec2::new(cell, crate::theme::RIBBON_ROW_HEIGHT);
        for (caption, size) in sizes {
            assert_eq!(size, expected, "{caption} draws a {size:?} cell, not the shared {expected:?}");
        }
    }

    #[test]
    fn a_caption_that_does_not_fit_is_elided() {
        // The cell's inner width is the only width a caption gets; a word past it is cut with `…`
        // instead of widening the button, which is what makes the row one pitch.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let inner = crate::theme::RIBBON_BUTTON_MIN_WIDTH - 2.0 * crate::theme::RIBBON_BUTTON_PADDING;
        let mut elided = true;
        let output = ctx.run_ui(Default::default(), |ui| {
            elided = caption_galley(ui, "Прямоугольник и ещё одно длинное слово", inner).elided;
        });
        output.drop_without_applying_deltas();
        assert!(elided, "a caption wider than the cell is elided");
    }

    #[test]
    fn every_ribbon_caption_of_the_interface_fits_its_cell() {
        // Russian is the language that decides the cell's width (U49), so the longest caption of the
        // ribbon tools has to fit without elision or the fixed cell would be a regression. The list is
        // the present ribbon, captioned in Russian: the drawing tools, the draft pair, the primitives,
        // the extrusion, the scene creators and the three view tools.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let inner = crate::theme::RIBBON_BUTTON_MIN_WIDTH - 2.0 * crate::theme::RIBBON_BUTTON_PADDING;
        let captions = [
            "Линия",
            "Полилиния",
            "Дуга",
            "Прямоугольник",
            "Окружность",
            "Замкнуть",
            "Отмена",
            "Куб",
            "Цилиндр",
            "Сфера",
            "Выдавить",
            "Панель",
            "Сборка",
            "Камера",
            "Свет",
            "Вписать",
            "Ортогональная",
            "Перспективная",
            "Сетка",
        ];
        for caption in captions {
            let mut galley = None;
            let output = ctx.run_ui(Default::default(), |ui| {
                galley = Some(caption_galley(ui, caption, inner));
            });
            output.drop_without_applying_deltas();
            let galley = galley.expect("the caption was laid out");
            assert!(!galley.elided, "{caption} does not fit the narrowest cell; raise RIBBON_BUTTON_MIN_WIDTH");
            assert!(
                galley.size().x <= inner + 0.5,
                "{caption} is {} pt wide, the cell allows {inner}",
                galley.size().x
            );
        }
    }

    #[test]
    fn a_placeholder_keeps_the_cell_of_a_drawn_button() {
        // A half-drawn icon set must not leave a ragged row: the text fallback takes the same cell.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sizes = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            let cell = crate::theme::RIBBON_BUTTON_MAX_WIDTH;
            sizes.push(ribbon_button(ui, cell, drawn(), "Line", "Line  (L)", true, false).rect.size());
            sizes.push(ribbon_button(ui, cell, placeholder(), "Line", "Line  (L)", true, false).rect.size());
        });
        output.drop_without_applying_deltas();
        assert_eq!(sizes[0], sizes[1], "the placeholder and the drawing share one cell");
        assert_eq!(sizes[0].x, crate::theme::RIBBON_BUTTON_MAX_WIDTH);
    }

    #[test]
    fn a_tool_hint_names_the_shortcut_and_a_placeholder_shows_it() {
        assert_eq!(tool_hint("Line", Some("L")), "Line  (L)");
        assert_eq!(tool_hint("Line", None), "Line");
        assert_eq!(tool_hint("Line", Some("")), "Line");

        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            tool_button(ui, placeholder(), "Line", Some("L"), true, false);
        });
        let has_hint = drawn_text(&output.shapes).iter().any(|text| text == "Line  (L)");
        output.drop_without_applying_deltas();
        assert!(has_hint, "an empty icon falls back to the labelled shortcut");
    }

    #[test]
    fn a_disabled_tool_button_still_draws_its_label() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            let response = tool_button(ui, placeholder(), "Circle", Some("C"), false, false);
            assert!(!response.enabled(), "the caller disabled the tool");
        });
        let has_hint = drawn_text(&output.shapes).iter().any(|text| text == "Circle  (C)");
        output.drop_without_applying_deltas();
        assert!(has_hint);
    }

    #[test]
    fn an_empty_asset_draws_the_text_label() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            icon_button(ui, placeholder(), "Confirm");
        });

        let has_label = drawn_text(&output.shapes).iter().any(|text| text == "Confirm");
        output.drop_without_applying_deltas();
        assert!(has_label);
    }

    /// Runs one icon button under `visuals` at `pixels_per_point` and returns the uploaded colours.
    fn uploaded_icons(
        visuals: egui::Visuals,
        pixels_per_point: f32,
        draw: impl FnMut(&mut Ui),
    ) -> Vec<egui::ColorImage> {
        let ctx = egui::Context::default();
        ctx.set_visuals(visuals);
        ctx.set_pixels_per_point(pixels_per_point);
        let output = ctx.run_ui(Default::default(), draw);
        let images = output
            .textures_delta
            .set
            .values()
            .flatten()
            .map(|delta| match &delta.image {
                ImageData::Color(image) => image.as_ref().clone(),
            })
            .collect();
        output.drop_without_applying_deltas();
        images
    }

    /// The square rasters of an icon; the font atlas is a colour image too.
    fn icon_rasters(images: &[egui::ColorImage], px: usize) -> Vec<&egui::ColorImage> {
        images.iter().filter(|image| image.size == [px, px]).collect()
    }

    #[test]
    fn the_theme_colour_reaches_the_icon_pixels() {
        let px = TOOL_ICON_POINT_SIZE as usize;
        let dark = uploaded_icons(egui::Visuals::dark(), 1.0, |ui| {
            tool_button(ui, drawn(), "Confirm", None, true, false);
        });
        let light = uploaded_icons(egui::Visuals::light(), 1.0, |ui| {
            tool_button(ui, drawn(), "Confirm", None, true, false);
        });
        let dark = icon_rasters(&dark, px);
        let light = icon_rasters(&light, px);

        assert_eq!(dark.len(), 1, "one icon uploads one texture");
        assert_eq!(light.len(), 1, "one icon uploads one texture");
        let (dark_color, light_color) = (
            egui::Visuals::dark().widgets.inactive.fg_stroke.color,
            egui::Visuals::light().widgets.inactive.fg_stroke.color,
        );
        assert_ne!(dark_color, light_color, "the two themes disagree on the ink");
        assert!(dark[0].pixels.contains(&dark_color), "the dark raster is not the theme colour");
        assert!(light[0].pixels.contains(&light_color), "the light raster is not the theme colour");

        let dark_alpha: Vec<u8> = dark[0].pixels.iter().map(|pixel| pixel.a()).collect();
        let light_alpha: Vec<u8> = light[0].pixels.iter().map(|pixel| pixel.a()).collect();
        assert_eq!(dark_alpha, light_alpha, "a theme change must not change the silhouette");
    }

    #[test]
    fn the_raster_follows_the_ui_scale_and_the_button_size() {
        for pixels_per_point in [1.0_f32, 1.25, 2.0] {
            for point_size in [TOOL_ICON_POINT_SIZE, ICON_POINT_SIZE] {
                let images = uploaded_icons(egui::Visuals::dark(), pixels_per_point, |ui| {
                    if point_size == TOOL_ICON_POINT_SIZE {
                        tool_button(ui, drawn(), "Confirm", None, true, false);
                    } else {
                        icon_button(ui, drawn(), "Confirm");
                    }
                });
                let expected = (point_size * pixels_per_point).round() as usize;
                let images = icon_rasters(&images, expected);
                assert_eq!(images.len(), 1, "one icon uploads one texture");
                assert_eq!(
                    images[0].size,
                    [expected, expected],
                    "{point_size} pt at {pixels_per_point} ppp rasterises at {expected} px"
                );
            }
        }
    }

    #[test]
    fn one_icon_size_and_colour_is_rasterised_once() {
        let images = uploaded_icons(egui::Visuals::dark(), 1.0, |ui| {
            tool_button(ui, drawn(), "Confirm", None, true, false);
            tool_button(ui, drawn(), "Confirm", None, true, false);
        });
        let images = icon_rasters(&images, TOOL_ICON_POINT_SIZE as usize);
        assert_eq!(images.len(), 1, "the second button reuses the cached texture");
    }
}
