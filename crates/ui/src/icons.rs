//! Themed icon assets and the two icon buttons.
//!
//! `assets/icons/**` is embedded at compile time, one module per theme. A zero-byte file is a
//! documented placeholder, so an icon button whose artwork has not been drawn yet falls back to its
//! text label and the surface stays usable. A drawn icon is rasterised by the `egui_extras` SVG
//! loader at `pt × pixels_per_point` and uploaded as a `TextureHandle`, so one master serves every
//! theme (`assets/README.md`, ADR 0055, ADR 0090).
//!
//! A master carries the literal role inks of a six-entry table (`plan/notes/U70.md`): `draw`,
//! `modify`, `model`, `scene`, `view`, `danger`. Before the upload the rasteriser maps each literal
//! to the active variant's ink through a [`RoleRamp`], so the file says what an element means and the
//! theme decides how bright it is — dark and light need no second asset. Below a 20 px raster, or
//! when the workspace asks for one ink, the alpha silhouette is multiplied by the widget's colour
//! instead: the same master, today's tint path, no colour at all in the drawing. The ink stays
//! inside the fixed `3..21` footprint and covers 20–45 % of it; the tests below read both off the
//! raster (`assets/icons/STYLE_GUIDE.md` §7.4).

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

/// The semantic family of an icon: the role its ink carries and the meaning it speaks for.
///
/// A family is not a theme directory — `contour/arc` is [`Family::Draw`] while `actions/delete` is
/// [`Family::Danger`] — because the surface an icon appears on and the thing it does are two
/// different facts. Every master writes its family's [`Family::master_hex`]; [`RoleRamp`] turns
/// that literal into the ink of the active variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Create or modify sketch geometry: lines, arcs, circles, contours.
    Draw,
    /// Transform and edit commands: undo, copy, paste, rename, visibility.
    Modify,
    /// Solids, bodies and booleans.
    Model,
    /// Panels, hardware, cameras and lights.
    Scene,
    /// Navigation, projection and display, and the neutral chrome that names no edit.
    View,
    /// Destructive commands only.
    Danger,
}

impl Family {
    /// Every family, in the order of the role table.
    pub const ALL: [Self; 6] = [Self::Draw, Self::Modify, Self::Model, Self::Scene, Self::View, Self::Danger];

    /// The literal a master carries for this family: the dark-variant ink of the role table.
    ///
    /// The dark column is the master's own value, so a dark theme maps every literal onto itself.
    pub const fn master_hex(self) -> &'static str {
        match self {
            Self::Draw => "#58C8DC",
            Self::Modify => "#A98BF0",
            Self::Model => "#6E9EF7",
            Self::Scene => "#55C7A4",
            Self::View => "#B8C2CE",
            Self::Danger => "#E06A6A",
        }
    }
}

/// The six role inks of one theme variant.
///
/// The ramps are the role table of `plan/notes/U70.md`; a preset changes the neutrals and the
/// accent, never the meaning of an icon, so both variants are preset-independent. The dark ramp is
/// the master table itself, and the light ramp folds each hue until it reads on paper — every ink
/// clears 3:1 against the panel and the ribbon of all three presets (asserted below).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRamp {
    /// The ink of [`Family::Draw`].
    pub draw: Color32,
    /// The ink of [`Family::Modify`].
    pub modify: Color32,
    /// The ink of [`Family::Model`].
    pub model: Color32,
    /// The ink of [`Family::Scene`].
    pub scene: Color32,
    /// The ink of [`Family::View`].
    pub view: Color32,
    /// The ink of [`Family::Danger`].
    pub danger: Color32,
}

impl RoleRamp {
    /// The dark variant's inks, the values every master writes.
    pub const DARK: Self = Self {
        draw: Color32::from_rgb(0x58, 0xC8, 0xDC),
        modify: Color32::from_rgb(0xA9, 0x8B, 0xF0),
        model: Color32::from_rgb(0x6E, 0x9E, 0xF7),
        scene: Color32::from_rgb(0x55, 0xC7, 0xA4),
        view: Color32::from_rgb(0xB8, 0xC2, 0xCE),
        danger: Color32::from_rgb(0xE0, 0x6A, 0x6A),
    };

    /// The light variant's inks, folded until they read on the paper variants.
    pub const LIGHT: Self = Self {
        draw: Color32::from_rgb(0x1C, 0x7C, 0x92),
        modify: Color32::from_rgb(0x6B, 0x4B, 0xB0),
        model: Color32::from_rgb(0x2E, 0x5C, 0xB8),
        scene: Color32::from_rgb(0x23, 0x7A, 0x5E),
        view: Color32::from_rgb(0x5A, 0x66, 0x74),
        danger: Color32::from_rgb(0xB3, 0x3A, 0x3A),
    };

    /// The ramp of the variant `dark` selects.
    pub const fn for_variant(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }

    /// The ink of `family`.
    pub const fn ink(self, family: Family) -> Color32 {
        match family {
            Family::Draw => self.draw,
            Family::Modify => self.modify,
            Family::Model => self.model,
            Family::Scene => self.scene,
            Family::View => self.view,
            Family::Danger => self.danger,
        }
    }
}

/// How the widget state biases an icon's ink.
///
/// The role path owns the hue, so a state cannot hand it a second colour: a hover or a press takes
/// one emphasis step away from the surface, a disabled icon keeps its hue at `disabled_alpha`, and a
/// chosen toggle draws the theme's foreground because the accent fill behind it is chrome, not an
/// icon role (`plan/notes/U70.md`). The one-ink path ignores the tone: its caller already passes the
/// widget state's own colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum IconTone {
    /// The resting face.
    #[default]
    Rest,
    /// Under the pointer: one step of the role ink away from the surface.
    Hover,
    /// Pressed: the same one step; the cell's own fill says which state it is.
    Press,
    /// Unavailable: the role ink at `disabled_alpha`.
    Disabled,
    /// Chosen: the theme's foreground over the accent fill, never a role ink.
    Selected,
}

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

/// One embedded icon: its theme, its family, its file stem and its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    theme: Theme,
    family: Family,
    name: &'static str,
    uri: &'static str,
    asset: &'static [u8],
}

impl Icon {
    const fn new(theme: Theme, family: Family, name: &'static str, uri: &'static str, asset: &'static [u8]) -> Self {
        Self { theme, family, name, uri, asset }
    }

    /// The theme directory that owns this icon.
    pub const fn theme(self) -> Theme {
        self.theme
    }

    /// The semantic family whose ink this icon carries.
    pub const fn family(self) -> Family {
        self.family
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
///
/// `$default` is the family of the theme; an icon names its own family after the file when the
/// surface it lives on and the meaning it carries disagree (`actions/delete` is danger).
macro_rules! icons {
    ($theme:ident, $theme_name:literal, $default:ident, $module:ident { $($constant:ident => $file:literal $(: $family:ident)?),* $(,)? }) => {
        #[doc = concat!("The `", $theme_name, "` theme.")]
        pub mod $module {
            use super::{Family, Icon, Theme};

            $(
                #[doc = concat!("The `", $theme_name, "/", $file, "` icon.")]
                pub const $constant: Icon = Icon::new(
                    Theme::$theme,
                    family_of!($default $(, $family)?),
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

/// The family a theme starts with, or the one an icon names for itself.
macro_rules! family_of {
    ($default:ident) => {
        Family::$default
    };
    ($default:ident, $family:ident) => {
        Family::$family
    };
}

icons!(Contour, "contour", Draw, contour {
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
    TRIM => "trim",
    EXTEND => "extend",
    OFFSET => "offset",
    FILLET => "fillet",
    CHAMFER => "chamfer",
    BREAK => "break",
    CONSTRUCTION_HORIZONTAL => "construction-horizontal",
    CONSTRUCTION_VERTICAL => "construction-vertical",
    CONSTRUCTION_PARALLEL => "construction-parallel",
    CONSTRUCTION_ANGLE => "construction-angle",
    DIMENSION_LINEAR => "dimension-linear": Scene,
    DIMENSION_ANGULAR => "dimension-angular": Scene,
    DIMENSION_RADIUS => "dimension-radius": Scene,
    BOOLEAN_UNION => "boolean-union": Model,
    BOOLEAN_SUBTRACT => "boolean-subtract": Model,
    BOOLEAN_INTERSECT => "boolean-intersect": Model,
    CLOSE => "close",
    CANCEL => "cancel",
});

icons!(Primitives, "primitives", Model, primitives {
    CUBE => "cube",
    CYLINDER => "cylinder",
    SPHERE => "sphere",
    CONE => "cone",
});

icons!(Bodies, "bodies", Model, bodies {
    EXTRUDE => "extrude",
    REVOLVE => "revolve",
    SWEEP => "sweep",
});

icons!(Scene, "scene", Scene, scene {
    PANEL => "panel",
    ASSEMBLY => "assembly",
    CAMERA => "camera",
    LIGHT => "light",
});

icons!(View, "view", View, view {
    ORTHOGRAPHIC => "orthographic",
    PERSPECTIVE => "perspective",
    FRAME_ALL => "frame-all",
    MAXIMIZE => "maximize",
    RESTORE => "restore",
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
    AXON => "axon",
    FRONT => "front",
    LEFT => "left",
    TOP => "top",
    RIGHT => "right",
    BOTTOM => "bottom",
});

icons!(Actions, "actions", View, actions {
    CONFIRM => "confirm",
    DISMISS => "dismiss",
    CANCEL => "cancel",
    UNDO => "undo": Modify,
    REDO => "redo": Modify,
    DELETE => "delete": Danger,
    ADD => "add": Modify,
    REMOVE => "remove": Danger,
    DUPLICATE => "duplicate": Modify,
    RENAME => "rename": Modify,
    COPY => "copy": Modify,
    PASTE => "paste": Modify,
    FILE_NEW => "file-new",
    FILE_OPEN => "file-open",
    FILE_SAVE => "file-save",
    FILE_EXPORT => "file-export",
    FILE_IMPORT => "file-import",
    PRINT => "print",
    SETTINGS => "settings",
    SEARCH => "search",
    COMMAND => "command",
    VISIBLE => "visible": Modify,
    HIDDEN => "hidden": Modify,
    LOCK => "lock": Modify,
    UNLOCK => "unlock": Modify,
    EXPAND => "expand",
    COLLAPSE => "collapse",
    WARNING => "warning",
    ERROR => "error",
    INFO => "info",
    HELP => "help",
});

icons!(Window, "window", View, window {
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

/// A frameless icon button: the glyph alone at rest, a faint wash under the pointer or while pressed.
///
/// The corner widget keeps a quiet face, so it cannot use [`icon_button`], whose cell is a plinth.
/// The cell is still allocated, so the control keeps its rhythm and its hit area; only the paint is
/// quiet. An empty placeholder asset becomes a frameless text button, so a missing drawing never
/// leaves a blank click target.
pub fn flat_icon_button(ui: &mut Ui, icon: Icon, cell: f32, point_size: f32, label: &str, enabled: bool) -> Response {
    if !icon.is_drawn() {
        return ui
            .add_enabled(enabled, Button::new(label).min_size(Vec2::splat(cell)).frame(false))
            .on_hover_text(label);
    }
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
            paint_flat_icon(ui, rect, &response, icon, point_size, label, enabled);
            response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
            response
        })
        .inner;
    if label.is_empty() { response } else { response.on_hover_text(label) }
}

/// The same flat button at an exact rectangle, for a surface that owns its own layout.
///
/// The corner widget computes its controls in viewport coordinates, so the layout cannot place the
/// button; `id` keeps the interaction stable across frames. A placeholder asset paints its `label`
/// in the cell instead of a glyph.
pub fn flat_icon_button_at(
    ui: &Ui,
    rect: egui::Rect,
    id: egui::Id,
    icon: Icon,
    point_size: f32,
    label: &str,
) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    if ui.is_rect_visible(rect) {
        paint_flat_icon(ui, rect, &response, icon, point_size, label, true);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
    if label.is_empty() { response } else { response.on_hover_text(label) }
}

/// The flat face: the glyph over a wash that appears only under the hand.
fn paint_flat_icon(
    ui: &Ui,
    rect: egui::Rect,
    response: &Response,
    icon: Icon,
    point_size: f32,
    label: &str,
    enabled: bool,
) {
    let wash = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        crate::theme::control_active(ui)
    } else if response.hovered() {
        crate::theme::control_hover(ui)
    } else {
        Color32::TRANSPARENT
    };
    if wash != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(crate::theme::RADIUS), wash);
    }
    let ink = if enabled { crate::theme::text(ui) } else { crate::theme::text_dim(ui) };
    if icon.is_drawn() {
        let icon_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(point_size));
        let tone = widget_tone(ui, response, enabled, false);
        paint_icon_toned(ui, icon, icon_rect, point_size, ink, tone);
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(point_size * 0.75),
            ink,
        );
    }
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
                let (fill, stroke, color, tone) = ribbon_face(ui, &response, on, enabled);
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
                    paint_icon_toned(ui, icon, icon_rect, RIBBON_ICON_POINT_SIZE, color, tone);
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
fn ribbon_face(ui: &Ui, response: &Response, on: bool, enabled: bool) -> (Color32, Stroke, Color32, IconTone) {
    if !enabled {
        return (Color32::TRANSPARENT, Stroke::NONE, crate::theme::text_dim(ui), IconTone::Disabled);
    }
    if on {
        let visuals = ui.visuals().selection;
        return (visuals.bg_fill, visuals.stroke, Color32::WHITE, IconTone::Selected);
    }
    if response.is_pointer_button_down_on() {
        return (crate::theme::control_active(ui), Stroke::NONE, crate::theme::text(ui), IconTone::Press);
    }
    if response.hovered() {
        return (crate::theme::control_hover(ui), Stroke::NONE, crate::theme::text(ui), IconTone::Hover);
    }
    (Color32::TRANSPARENT, Stroke::NONE, crate::theme::text(ui), IconTone::Rest)
}

/// Paints `icon` inside `rect` at `point_size`, tinted with `color`, without a button frame.
///
/// A folded side panel draws its icon in the middle of a 26 pt strip where a button would not fit;
/// the raster path is the same one the buttons use, so the asset, the cache and the theme colour are
/// shared. A placeholder asset draws nothing. `color` is the one-ink fallback's tint; a raster wide
/// enough for a role ink takes its hue from the icon's family instead.
pub fn draw_icon(ui: &Ui, icon: Icon, rect: egui::Rect, point_size: f32, color: Color32) {
    if icon.is_drawn() {
        paint_icon(ui, icon, rect, point_size, color);
    }
}

/// Paints `icon` inside `rect` at `point_size` at rest: its role ink, or `color` in one-ink mode.
pub(crate) fn paint_icon(ui: &Ui, icon: Icon, rect: egui::Rect, point_size: f32, color: Color32) {
    paint_icon_toned(ui, icon, rect, point_size, color, IconTone::Rest);
}

/// Paints `icon` inside `rect` at `point_size` in the ink `tone` asks for.
///
/// A raster of at least [`settings::ICON_COLOR_MIN_PX`] pixels in [`settings::IconColor::Auto`] — or
/// any raster in `Always` — carries the icon's role ink; below that, in `Never`, and on a chosen
/// toggle the master's alpha is multiplied by `color` instead. The disabled state is one alpha
/// multiply of the role texture, never a second table.
pub(crate) fn paint_icon_toned(ui: &Ui, icon: Icon, rect: egui::Rect, point_size: f32, color: Color32, tone: IconTone) {
    let px = pixel_size(point_size, ui.pixels_per_point());
    let dark = ui.visuals().dark_mode;
    let mode = crate::icon_color(ui.ctx());
    let roles = mode.uses_roles(px) && tone != IconTone::Selected;
    if let Some(texture) = icon_texture(ui.ctx(), icon, px, color, dark, roles, tone) {
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let tint = if roles && tone == IconTone::Disabled { disabled_tint(ui) } else { Color32::WHITE };
        ui.painter().image(texture.id(), rect, uv, tint);
    }
}

/// A white vertex colour at the style's disabled alpha, which multiplies the texture by it.
fn disabled_tint(ui: &Ui) -> Color32 {
    let alpha = ui.style().visuals.disabled_alpha().clamp(0.0, 1.0);
    Color32::from_white_alpha((alpha * 255.0).round() as u8)
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
            let tone = widget_tone(ui, &response, enabled, selected);
            paint_icon_toned(ui, icon, icon_rect, point_size, visuals.fg_stroke.color, tone);
        }
        response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
        response
    })
    .inner
}

/// The tone a widget's own state asks for, in the order the states outrank each other.
///
/// A chosen toggle wins over a hover, because the accent fill already says it is on; a disabled
/// button wins over everything a pointer could do to it.
fn widget_tone(ui: &Ui, response: &Response, enabled: bool, selected: bool) -> IconTone {
    if selected {
        IconTone::Selected
    } else if !enabled || !ui.is_enabled() {
        IconTone::Disabled
    } else if response.is_pointer_button_down_on() {
        IconTone::Press
    } else if response.hovered() {
        IconTone::Hover
    } else {
        IconTone::Rest
    }
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
/// Two paths share one cache: a role raster, keyed by the variant and whether the ink is lightened,
/// whose pixels are the [`RoleRamp`] inks the master's literals name; and the one-ink raster, keyed
/// by the tint, whose pixels are the master's alpha multiplied by the widget's colour. A theme or
/// mode change therefore re-rasterises once per `(uri, px)` and a widget state only picks another
/// texture: a disabled icon reuses the resting role raster and the paint multiplies its alpha.
fn icon_texture(
    ctx: &egui::Context,
    icon: Icon,
    px: u32,
    color: Color32,
    dark: bool,
    roles: bool,
    tone: IconTone,
) -> Option<TextureHandle> {
    let lightened = matches!(tone, IconTone::Hover | IconTone::Press);
    let id = if roles {
        egui::Id::unique(("furnicraft.icon.role", icon.uri(), px, dark, lightened))
    } else {
        egui::Id::unique(("furnicraft.icon.ink", icon.uri(), px, color.to_array()))
    };
    if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = if roles {
        let svg = role_svg(icon, dark, lightened)?;
        raster(svg.as_bytes(), px)?
    } else {
        tint(raster(icon.asset(), px)?, color)
    };
    let texture = ctx.load_texture(icon.uri(), image, TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Rasterises an SVG master at `px` pixels square.
fn raster(asset: &[u8], px: u32) -> Option<egui::ColorImage> {
    egui_extras::image::load_svg_bytes_with_size(
        asset,
        egui::load::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: false },
        &Default::default(),
    )
    .ok()
}

/// The master with every role literal replaced by the ink of the active variant.
///
/// `lightened` is the one emphasis step a hover or a press asks for; every other state keeps the
/// resting ink and is applied at paint time. The substitution changes no attribute but the colour,
/// so the alpha silhouette is the drawing itself: dark and light differ in hue, never in shape. A
/// file that is not UTF-8 cannot carry a literal and falls back to the one-ink path by returning
/// `None`.
fn role_svg(icon: Icon, dark: bool, lightened: bool) -> Option<String> {
    let svg = std::str::from_utf8(icon.asset()).ok()?;
    let ramp = RoleRamp::for_variant(dark);
    let mut out = svg.to_owned();
    for family in Family::ALL {
        let ink = ramp.ink(family);
        let toned = if lightened { crate::theme::mix(ink, hover_target(dark), HOVER_STEP) } else { ink };
        out = out.replace(family.master_hex(), &hex(toned));
    }
    Some(out)
}

/// The share of the way to the emphasis target a hover or a press moves a role ink, in percent.
const HOVER_STEP: u32 = 10;

/// The colour a hover or a press moves a role ink towards: white on a dark variant, black on paper.
///
/// The step goes away from the surface, so the glyph gains contrast in both variants instead of
/// fading into the light one; it is one tone ladder, not a second ink.
const fn hover_target(dark: bool) -> Color32 {
    if dark { Color32::WHITE } else { Color32::BLACK }
}

/// The `#RRGGBB` literal of an opaque ink, the form a master carries.
fn hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
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

    /// A minimal valid role-ink drawing, used where the real files are still placeholders.
    const SAMPLE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" fill="#B8C2CE"/></svg>"##;

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
        Icon::new(Theme::Actions, Family::View, "confirm", "bytes://icons/actions/confirm.svg", &[])
    }

    /// An icon with a real drawing, for the rasteriser tests.
    fn drawn() -> Icon {
        Icon::new(Theme::Actions, Family::View, "confirm", "bytes://icons/actions/confirm.svg", SAMPLE_SVG)
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
        assert_eq!(seen.len(), 97);
    }

    #[test]
    fn the_readme_theme_table_lists_exactly_the_declared_icons() {
        // The table in `assets/README.md` is the contract a drawing session reads before it adds a
        // file; a missing row or a stale name sends it to a file the code does not embed.
        let readme = include_str!("../../../assets/README.md");
        for theme in Theme::ALL {
            let prefix = format!("| `{}` |", theme.as_str());
            let row = readme
                .lines()
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("{:?} has no row in the theme table", theme));
            let documented: HashSet<&str> = row[prefix.len()..].split('`').skip(1).step_by(2).collect();
            let declared: HashSet<&str> = theme.icons().iter().map(|icon| icon.name()).collect();
            assert_eq!(documented, declared, "the {:?} row and its `ALL` list disagree", theme);
        }
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
    fn every_drawn_asset_carries_its_family_ink_and_nothing_else() {
        // The role table is closed: a master may use one, two or three of its six literals, never a
        // colour of its own, and its family's literal is always one of them (U70).
        let allowed: HashSet<&str> = Family::ALL.iter().map(|family| family.master_hex()).collect();
        for theme in Theme::ALL {
            for icon in theme.icons() {
                if !icon.is_drawn() {
                    continue;
                }
                let svg = std::str::from_utf8(icon.asset()).expect("an icon asset is UTF-8");
                let name = format!("{}/{}", icon.theme().as_str(), icon.name());
                let mut inks = HashSet::new();
                let mut at = 0;
                while let Some(found) = svg[at..].find('#') {
                    let start = at + found;
                    let rest = &svg[start..];
                    let literal = rest.get(..7).expect("a colour literal has six digits");
                    assert!(
                        literal[1..].bytes().all(|byte| byte.is_ascii_hexdigit()),
                        "{name} carries `{literal}`, which is not a six-digit hex colour"
                    );
                    inks.insert(literal.to_owned());
                    at = start + 7;
                }
                assert!(inks.len() <= 3, "{name} carries {} role inks, the table allows three", inks.len());
                for ink in &inks {
                    assert!(allowed.contains(ink.as_str()), "{name} carries `{ink}`, outside the role table");
                }
                assert!(
                    inks.contains(icon.family().master_hex()),
                    "{name} is tagged {:?} but does not carry its ink {}",
                    icon.family(),
                    icon.family().master_hex()
                );
            }
        }
    }

    #[test]
    fn the_dark_ramp_is_the_master_table() {
        for family in Family::ALL {
            assert_eq!(hex(RoleRamp::DARK.ink(family)), family.master_hex(), "{family:?} disagrees with the table");
        }
    }

    #[test]
    fn every_role_ink_clears_three_to_one_on_panel_and_ribbon() {
        // `plan/notes/U70.md`: the hue is decoration only where the glyph still reads. The draw ink
        // is the weakest on a light panel and is the one a change to the table breaks first.
        for preset in crate::theme::ThemePreset::ALL {
            for dark in [true, false] {
                let palette = preset.palette(dark);
                let ramp = RoleRamp::for_variant(dark);
                for family in Family::ALL {
                    let ink = ramp.ink(family);
                    for (surface, fill) in [("panel", palette.panel), ("ribbon", palette.surface)] {
                        let ratio = crate::theme::contrast_ratio(ink, fill);
                        assert!(ratio >= 3.0, "{family:?} on {preset:?} {surface} is {ratio:.2}:1, below 3:1");
                    }
                }
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
    fn every_chrome_icon_is_drawn() {
        // U46: the header, the scene tree, the panel fold and the viewport cluster expect a glyph.
        // A zero-byte file there is a text fallback on a surface that should not have one, so the
        // three chrome themes are complete. `primitives/cone` stays a placeholder until the cone
        // primitive lands (`plan/debt.md`).
        for theme in [Theme::View, Theme::Actions, Theme::Window] {
            for icon in theme.icons() {
                assert!(
                    icon.is_drawn(),
                    "{}/{} is still a placeholder on a chrome surface",
                    icon.theme().as_str(),
                    icon.name()
                );
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

    /// Runs one icon button under one preset and variant and returns the uploaded colours.
    fn uploaded_with(
        preset: crate::theme::ThemePreset,
        dark: bool,
        pixels_per_point: f32,
        draw: impl FnMut(&mut Ui),
    ) -> Vec<egui::ColorImage> {
        let ctx = egui::Context::default();
        let preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light };
        crate::theme::install_with(&ctx, preference, preset, None);
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

    #[test]
    fn the_role_ink_reaches_the_icon_pixels_for_every_preset() {
        let px = TOOL_ICON_POINT_SIZE as usize;
        for preset in crate::theme::ThemePreset::ALL {
            for dark in [true, false] {
                let images = uploaded_with(preset, dark, 1.0, |ui| {
                    tool_button(ui, drawn(), "Confirm", None, true, false);
                });
                let rasters = icon_rasters(&images, px);
                assert_eq!(rasters.len(), 1, "{preset:?} {dark}: one icon uploads one texture");
                let ink = RoleRamp::for_variant(dark).ink(drawn().family());
                assert!(
                    rasters[0].pixels.contains(&ink),
                    "{preset:?} {dark}: the raster is not the family ink {ink:?}"
                );
            }
        }
    }

    #[test]
    fn a_theme_change_keeps_the_silhouette() {
        let px = TOOL_ICON_POINT_SIZE as usize;
        let dark = uploaded_with(crate::theme::ThemePreset::InkAzure, true, 1.0, |ui| {
            tool_button(ui, drawn(), "Confirm", None, true, false);
        });
        let light = uploaded_with(crate::theme::ThemePreset::InkAzure, false, 1.0, |ui| {
            tool_button(ui, drawn(), "Confirm", None, true, false);
        });
        let dark = icon_rasters(&dark, px);
        let light = icon_rasters(&light, px);
        assert_eq!(dark.len(), 1);
        assert_eq!(light.len(), 1);
        let dark_alpha: Vec<u8> = dark[0].pixels.iter().map(|pixel| pixel.a()).collect();
        let light_alpha: Vec<u8> = light[0].pixels.iter().map(|pixel| pixel.a()).collect();
        assert_eq!(dark_alpha, light_alpha, "a theme change must not change the silhouette");
    }

    /// One frame of a 16 pt icon button under one preset and mode, with the theme's resting ink.
    fn mono_frame(
        preset: crate::theme::ThemePreset,
        dark: bool,
        mode: crate::settings::IconColor,
    ) -> (Vec<egui::ColorImage>, Color32) {
        let ctx = egui::Context::default();
        let preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light };
        crate::theme::install_with(&ctx, preference, preset, None);
        ctx.set_pixels_per_point(1.0);
        crate::set_icon_color(&ctx, mode);
        let ink = preset.palette(dark).text;
        let output = ctx.run_ui(Default::default(), |ui| {
            icon_button(ui, drawn(), "Confirm");
        });
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
        (images, ink)
    }

    #[test]
    fn the_monochrome_fallback_equals_the_tint_path_at_sixteen_pixels() {
        // U70: below the 20 px threshold, and in `Never` at any size, the raster is exactly the
        // master's alpha multiplied by the widget's colour — the path U27 shipped.
        let px = ICON_POINT_SIZE as usize;
        for (mode, preset) in [
            (crate::settings::IconColor::Auto, crate::theme::ThemePreset::GraphiteCopper),
            (crate::settings::IconColor::Never, crate::theme::ThemePreset::SlateMint),
        ] {
            let (images, ink) = mono_frame(preset, true, mode);
            let rasters = icon_rasters(&images, px);
            assert_eq!(rasters.len(), 1, "{mode:?} at {px} px uploads one texture");
            let expected = tint(
                raster(SAMPLE_SVG, ICON_POINT_SIZE as u32).expect("the sample rasterises"),
                preset.palette(true).text,
            );
            assert_eq!(ink, preset.palette(true).text);
            assert_eq!(rasters[0].pixels, expected.pixels, "{mode:?} is not the tint path");
        }
    }

    #[test]
    fn a_mode_switch_rasterises_once() {
        let ctx = egui::Context::default();
        crate::theme::install_with(&ctx, egui::ThemePreference::Dark, crate::theme::ThemePreset::GraphiteCopper, None);
        ctx.set_pixels_per_point(1.0);
        let frame = |ctx: &egui::Context| {
            let output = ctx.run_ui(Default::default(), |ui| {
                tool_button(ui, drawn(), "Confirm", None, true, false);
            });
            let images: Vec<egui::ColorImage> = output
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
        };
        let first = frame(&ctx);
        assert_eq!(icon_rasters(&first, TOOL_ICON_POINT_SIZE as usize).len(), 1, "the role raster uploads once");
        let second = frame(&ctx);
        assert!(icon_rasters(&second, TOOL_ICON_POINT_SIZE as usize).is_empty(), "a second frame reuses the cache");
        crate::set_icon_color(&ctx, crate::settings::IconColor::Never);
        let third = frame(&ctx);
        assert_eq!(
            icon_rasters(&third, TOOL_ICON_POINT_SIZE as usize).len(),
            1,
            "a mode switch uploads the one-ink raster once"
        );
        let fourth = frame(&ctx);
        assert!(icon_rasters(&fourth, TOOL_ICON_POINT_SIZE as usize).is_empty(), "the new mode is cached too");
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

    /// The number of filled rectangles in a frame, the flat button's wash included.
    fn filled_rects(shapes: &[ClippedShape]) -> usize {
        fn walk(shape: &Shape, out: &mut usize) {
            match shape {
                Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => *out += 1,
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn a_flat_button_is_a_glyph_with_no_plinth() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let frame = |pointer: egui::Pos2| {
            let mut rect = egui::Rect::NOTHING;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                rect = flat_icon_button(ui, drawn(), 22.0, 20.0, "Projection", true).rect;
            });
            let fills = filled_rects(&output.shapes);
            output.drop_without_applying_deltas();
            (rect, fills)
        };

        // The first frame registers the cell with the pointer far away, so nothing is painted twice.
        let (rect, at_rest) = frame(egui::pos2(screen.right() - 1.0, screen.bottom() - 1.0));
        assert_eq!(at_rest, 0, "the flat button paints no cell at rest");
        // A second frame under the pointer adds the hover wash, and only that.
        let (_, hovered) = frame(rect.center());
        assert_eq!(hovered, 1, "one rounded wash under the pointer");
    }

    #[test]
    fn a_flat_placeholder_draws_its_label() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let output = ctx.run_ui(Default::default(), |ui| {
            flat_icon_button(ui, placeholder(), 22.0, 20.0, "Projection", true);
        });
        let has_label = drawn_text(&output.shapes).iter().any(|text| text == "Projection");
        output.drop_without_applying_deltas();
        assert!(has_label);
    }

    #[test]
    fn a_flat_button_at_a_rect_draws_its_label_when_the_asset_is_empty() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::splat(22.0));
        let output = ctx.run_ui(Default::default(), |ui| {
            flat_icon_button_at(ui, rect, egui::Id::unique("flat_probe"), placeholder(), 20.0, "Projection");
        });
        let has_label = drawn_text(&output.shapes).iter().any(|text| text == "Projection");
        output.drop_without_applying_deltas();
        assert!(has_label);
    }
}
