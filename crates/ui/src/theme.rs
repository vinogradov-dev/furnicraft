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
use furnicraft_scene::SceneColors;
use furnicraft_text::Message;
use serde::{Deserialize, Serialize};

/// The darkest chrome: the window frame around the panels.
pub const CHROME: Color32 = Color32::from_rgb(0x14, 0x17, 0x1b);

/// A raised surface: the header, the ribbon body and the panel headers.
pub const SURFACE: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x24);

/// The panel background, a shade above [`SURFACE`] where content sits.
pub const PANEL: Color32 = Color32::from_rgb(0x22, 0x27, 0x2e);

/// A control's resting fill.
pub const CONTROL: Color32 = Color32::from_rgb(0x2a, 0x30, 0x37);

/// A hovered control: [`CONTROL`] one step towards [`TEXT`].
///
/// Hover, press, the hairline and the disabled ink are not palette data of their own: they are one
/// step away from a base token, so six ramps stay eleven values each instead of seventeen.
pub const CONTROL_HOVER: Color32 = mix(CONTROL, TEXT, 6);

/// A pressed control: [`CONTROL`] sunk towards [`CHROME`].
pub const CONTROL_ACTIVE: Color32 = mix(CONTROL, CHROME, 55);

/// The translucent fill of a floating cluster over the viewport: [`SURFACE`] at about 88 %.
///
/// A floating cluster sits over the drawn scene, so it has to keep the scene readable behind it
/// without losing the contrast of its own controls. The alpha is premultiplied, which is what
/// `egui` expects from a `Color32`.
pub const FLOATING: Color32 = Color32::from_rgba_premultiplied(0x1e, 0x21, 0x26, 224);

/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xdd, 0xe3);

/// Secondary text: hints, units, group captions.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9f);

/// Text that cannot be acted on: [`TEXT_DIM`] folded into the panel it sits on.
pub const TEXT_DISABLED: Color32 = mix(TEXT_DIM, PANEL, 45);

/// A hairline between sections: [`CONTROL`] one step towards [`TEXT`].
pub const LINE: Color32 = mix(CONTROL, TEXT, 8);

/// The smallest contrast the accent may have against the surface it is drawn on.
pub const ACCENT_CONTRAST: f32 = 3.0;

/// The action accent: the armed tool, the active tab, the selection.
pub const ACCENT: Color32 = Color32::from_rgb(0xe0, 0xa2, 0x4a);

/// Accent as a background, once it has to carry text or an icon.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x8a, 0x5a, 0x1f);

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
/// One icon cell: a [`RIBBON_ICON_POINT_SIZE`] glyph with the air it needs around it. The row carries
/// the glyph only — the tools are named by their hints and by U67's list, and the group's caption
/// sits in the band under the row — so it is short enough for the ribbon to float over the scene
/// without taking a usable strip of it (U68, `plan/notes/U71.md`).
pub const RIBBON_ROW_HEIGHT: f32 = 30.0;

/// The width of one ribbon cell, in points.
///
/// Every tool of every group takes the same cell, so a row is one pitch and a tool never sizes itself
/// from its name (ADR 0067, revised by ADR 0091): 34 points hold the 22-point glyph with six points of
/// air on each side. A tool with more than one method keeps this cell as its main face and adds
/// [`crate::icons::RIBBON_CARET_WIDTH`] points beside it for its list, so the glyph is never squeezed
/// (X36).
pub const RIBBON_CELL_WIDTH: f32 = 34.0;

/// The narrowest a ribbon tab may be, so a short name does not become a stamp.
pub const RIBBON_TAB_MIN_WIDTH: f32 = 64.0;

/// The height of the ribbon's tab row, in points.
pub const RIBBON_TAB_HEIGHT: f32 = 28.0;

/// The height of the caption band under a ribbon group, in points.
///
/// The band is the strip under the tool row and carries the 9.5 pt group name in capitals; at 16
/// points the capitals keep at least three points of air above and below, so the name touches
/// neither the tool row nor the closing rule (U75). It is type-derived, so it is not a multiple of
/// [`STEP`]: a caption is a line of small type, not a control.
pub const RIBBON_CAPTION_HEIGHT: f32 = 16.0;

/// The thickness of the hairline that closes a chrome surface, in points.
///
/// The ribbon draws one under its tab row and one under its caption band, which is what makes
/// [`super::toolbars::height`] the 29 and 76 points of the approved metrics.
pub const RIBBON_RULE: f32 = 1.0;

/// The rest width of a guide the program draws for the person, in points.
///
/// A guide is screen paint over the model — the square of a work cursor, the bracket of a group or a
/// product that is open — so its widths are a rule in points and never a world size: a guide that
/// grew with the zoom would read as geometry (ADR 0111).
pub const HELPER_STROKE: f32 = 1.5;

/// The width of a guide that crosses the whole viewport: the optional work crosshair.
///
/// A point, the weight of a hairline, because a line that spans the view is long enough to read at
/// the quietest weight; a heavier crosshair covers the form it is there to help measure.
pub const HELPER_CROSSHAIR_STROKE: f32 = 1.0;

/// The width of the bracket of the level that is open, in points.
///
/// Half a point over [`HELPER_STROKE`]: opening a level is what decides what the next click does, so
/// the container that owns the viewport says so at a glance instead of at a squint.
pub const HELPER_OPEN_STROKE: f32 = 2.0;

/// The height of the viewport bar at the bottom of the window, in points.
pub const STATUS_HEIGHT: f32 = 28.0;

/// The height of a side panel's header row, in points.
///
/// One control tall, like a header chip, so the title and the fold chevron sit on the same line the
/// header uses for its own buttons.
pub const PANEL_HEADER_HEIGHT: f32 = 26.0;

/// The width of a collapsed side panel's strip, in points.
///
/// Wide enough for a 14 pt glyph over a turned title; the floating panel and its fold animation
/// both start and end here (U66).
pub const COLLAPSED_PANEL_WIDTH: f32 = 34.0;

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
    /// The override is one colour for both variants, so it is first pushed away from the surface
    /// until it reads on it (a bright pick stays as it is on slate and is folded dark for a light
    /// variant), then the filled accent is derived from the result, so a pale accent still carries
    /// the body text it holds.
    pub fn with_accent(self, accent: Color32) -> Self {
        let accent = self.readable(accent);
        let accent_fill = if self.dark { mix(accent, Color32::BLACK, 34) } else { mix(accent, self.panel, 75) };
        Self { accent, accent_fill, ..self }
    }

    /// `accent` moved away from this palette's surface until it has [`ACCENT_CONTRAST`].
    ///
    /// A pick that already reads is kept exactly as chosen; one that does not is folded towards
    /// white on a dark palette and towards black on a light one, in 5 % steps.
    fn readable(self, accent: Color32) -> Color32 {
        let away = if self.dark { Color32::WHITE } else { Color32::BLACK };
        (0..=20)
            .map(|step| mix(accent, away, step * 5))
            .find(|candidate| contrast_ratio(*candidate, self.surface) >= ACCENT_CONTRAST)
            .unwrap_or(away)
    }

    /// The ink of a guide: a bracket, a work cursor, a crosshair the program draws.
    ///
    /// The guide is derived, not chosen, so one rule serves every preset and the scene can carry the
    /// same value as a constant (ADR 0111).
    pub fn helper(self) -> Color32 {
        helper_ink(self.warn, self.dark)
    }
}

/// The base tokens of one variant of one preset.
///
/// A ramp names only what a designer chooses; the hover, press, line and disabled tokens are mixed
/// from these, so a preset is eleven values per variant and a new token costs one field here and one
/// per ramp. `plan/notes/U71.md` holds the measured tables these ramps come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ramp {
    /// The window frame behind the panels.
    pub chrome: Color32,
    /// The header, the ribbon body and the panel headers.
    pub surface: Color32,
    /// The panel background where content sits.
    pub panel: Color32,
    /// A control at rest.
    pub control: Color32,
    /// A translucent cluster over the viewport, alpha included.
    pub floating: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text.
    pub text_dim: Color32,
    /// The action accent.
    pub accent: Color32,
    /// The accent used as a filled background.
    pub accent_fill: Color32,
    /// A destructive action's text.
    pub danger: Color32,
    /// A warning text.
    pub warn: Color32,
}

impl Ramp {
    /// The full palette this ramp paints for `dark`.
    pub const fn palette(self, dark: bool) -> Palette {
        let (hover, active, line, disabled) = if dark {
            (
                mix(self.control, self.text, 6),
                mix(self.control, self.chrome, 55),
                mix(self.control, self.text, 8),
                mix(self.text_dim, self.panel, 45),
            )
        } else {
            (
                mix(self.control, self.text, 5),
                mix(self.control, self.text, 10),
                mix(self.control, self.text, 12),
                mix(self.text_dim, self.panel, 30),
            )
        };
        Palette {
            dark,
            chrome: self.chrome,
            surface: self.surface,
            panel: self.panel,
            control: self.control,
            control_hover: hover,
            control_active: active,
            floating: self.floating,
            text: self.text,
            text_dim: self.text_dim,
            text_disabled: disabled,
            line,
            accent: self.accent,
            accent_fill: self.accent_fill,
            danger: self.danger,
            warn: self.warn,
        }
    }
}

/// P1 graphite·copper, dark: the values the consts above name.
const GRAPHITE_COPPER_DARK: Ramp = Ramp {
    chrome: CHROME,
    surface: SURFACE,
    panel: PANEL,
    control: CONTROL,
    floating: FLOATING,
    text: TEXT,
    text_dim: TEXT_DIM,
    accent: ACCENT,
    accent_fill: ACCENT_FILL,
    danger: DANGER,
    warn: WARN,
};

/// P1 graphite·copper, light: the same rhythm on paper.
const GRAPHITE_COPPER_LIGHT: Ramp = Ramp {
    chrome: Color32::from_rgb(0xe7, 0xe9, 0xec),
    surface: Color32::from_rgb(0xf0, 0xf2, 0xf4),
    panel: Color32::from_rgb(0xf7, 0xf8, 0xf9),
    control: Color32::from_rgb(0xe4, 0xe7, 0xeb),
    floating: Color32::from_rgba_premultiplied(0xd4, 0xd6, 0xd8, 224),
    text: Color32::from_rgb(0x23, 0x27, 0x2d),
    text_dim: Color32::from_rgb(0x5f, 0x6a, 0x75),
    accent: Color32::from_rgb(0xa6, 0x62, 0x0f),
    accent_fill: Color32::from_rgb(0xf0, 0xd3, 0xa8),
    danger: Color32::from_rgb(0xb2, 0x3b, 0x3b),
    warn: Color32::from_rgb(0x8a, 0x5a, 0x0f),
};

/// P2 ink·azure, dark.
const INK_AZURE_DARK: Ramp = Ramp {
    chrome: Color32::from_rgb(0x10, 0x13, 0x19),
    surface: Color32::from_rgb(0x17, 0x1b, 0x22),
    panel: Color32::from_rgb(0x1e, 0x23, 0x2b),
    control: Color32::from_rgb(0x27, 0x2e, 0x38),
    floating: Color32::from_rgba_premultiplied(0x1a, 0x1f, 0x27, 224),
    text: Color32::from_rgb(0xd7, 0xde, 0xe8),
    text_dim: Color32::from_rgb(0x87, 0x92, 0xa1),
    accent: Color32::from_rgb(0x4a, 0x9e, 0xff),
    accent_fill: Color32::from_rgb(0x21, 0x5f, 0x9e),
    danger: DANGER,
    warn: WARN,
};

/// P2 ink·azure, light.
const INK_AZURE_LIGHT: Ramp = Ramp {
    chrome: Color32::from_rgb(0xe7, 0xed, 0xf4),
    surface: Color32::from_rgb(0xf0, 0xf4, 0xf9),
    panel: Color32::from_rgb(0xf7, 0xf9, 0xfc),
    control: Color32::from_rgb(0xe2, 0xe9, 0xf2),
    floating: Color32::from_rgba_premultiplied(0xd4, 0xd7, 0xda, 224),
    text: Color32::from_rgb(0x1d, 0x25, 0x30),
    text_dim: Color32::from_rgb(0x58, 0x65, 0x76),
    accent: Color32::from_rgb(0x1f, 0x6f, 0xd0),
    accent_fill: Color32::from_rgb(0xcf, 0xe0, 0xf7),
    danger: Color32::from_rgb(0xb2, 0x3b, 0x3b),
    warn: Color32::from_rgb(0x8a, 0x5a, 0x0f),
};

/// P4 slate·mint, dark.
const SLATE_MINT_DARK: Ramp = Ramp {
    chrome: Color32::from_rgb(0x11, 0x17, 0x15),
    surface: Color32::from_rgb(0x18, 0x20, 0x1d),
    panel: Color32::from_rgb(0x22, 0x2b, 0x27),
    control: Color32::from_rgb(0x2a, 0x35, 0x30),
    floating: Color32::from_rgba_premultiplied(0x1b, 0x24, 0x20, 224),
    text: Color32::from_rgb(0xd9, 0xe3, 0xdd),
    text_dim: Color32::from_rgb(0x8a, 0x9a, 0x92),
    accent: Color32::from_rgb(0x4f, 0xd1, 0xa5),
    accent_fill: Color32::from_rgb(0x1e, 0x6e, 0x55),
    danger: DANGER,
    warn: WARN,
};

/// P4 slate·mint, light.
const SLATE_MINT_LIGHT: Ramp = Ramp {
    chrome: Color32::from_rgb(0xe8, 0xee, 0xea),
    surface: Color32::from_rgb(0xf1, 0xf6, 0xf3),
    panel: Color32::from_rgb(0xf8, 0xfb, 0xf9),
    control: Color32::from_rgb(0xe3, 0xeb, 0xe7),
    floating: Color32::from_rgba_premultiplied(0xd5, 0xd9, 0xd7, 224),
    text: Color32::from_rgb(0x1e, 0x2a, 0x25),
    text_dim: Color32::from_rgb(0x57, 0x6b, 0x62),
    accent: Color32::from_rgb(0x17, 0x79, 0x5c),
    accent_fill: Color32::from_rgb(0xcd, 0xeb, 0xdd),
    danger: Color32::from_rgb(0xb2, 0x3b, 0x3b),
    warn: Color32::from_rgb(0x8a, 0x5a, 0x0f),
};

/// The dark palette the editor opens with: P1 graphite·copper.
pub const DARK: Palette = GRAPHITE_COPPER_DARK.palette(true);

/// The light palette of the default preset: P1 graphite·copper on paper.
pub const LIGHT: Palette = GRAPHITE_COPPER_LIGHT.palette(false);

/// One of the three looks the owner approved (`plan/notes/U71.md`).
///
/// A preset is data, not a code path: each variant is a [`Ramp`], and every other colour of the
/// palette is mixed from it. The stored name is the settings file's, so renaming a variant is a
/// migration rather than a refactor; P3 was designed and is deliberately not shipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    /// P1 graphite·copper: a warm action accent on neutral slate. The default.
    #[default]
    GraphiteCopper,
    /// P2 ink·azure: a blue action accent on ink.
    InkAzure,
    /// P4 slate·mint: a mint action accent on a green-grey slate.
    SlateMint,
}

impl ThemePreset {
    /// Every preset, in the order the Appearance page draws them.
    pub const ALL: [Self; 3] = [Self::GraphiteCopper, Self::InkAzure, Self::SlateMint];

    /// The user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::GraphiteCopper => Message::PaletteGraphiteCopper,
            Self::InkAzure => Message::PaletteInkAzure,
            Self::SlateMint => Message::PaletteSlateMint,
        }
    }

    /// The name the settings file stores.
    pub const fn stored_name(self) -> &'static str {
        match self {
            Self::GraphiteCopper => "graphite_copper",
            Self::InkAzure => "ink_azure",
            Self::SlateMint => "slate_mint",
        }
    }

    /// The preset a stored name means, or `None` for a name this build does not know.
    pub fn from_stored_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.stored_name() == name)
    }

    /// This preset's dark palette.
    pub const fn dark(self) -> Palette {
        match self {
            Self::GraphiteCopper => GRAPHITE_COPPER_DARK.palette(true),
            Self::InkAzure => INK_AZURE_DARK.palette(true),
            Self::SlateMint => SLATE_MINT_DARK.palette(true),
        }
    }

    /// This preset's light palette.
    pub const fn light(self) -> Palette {
        match self {
            Self::GraphiteCopper => GRAPHITE_COPPER_LIGHT.palette(false),
            Self::InkAzure => INK_AZURE_LIGHT.palette(false),
            Self::SlateMint => SLATE_MINT_LIGHT.palette(false),
        }
    }

    /// This preset's palette for `dark`.
    ///
    /// The flag is the same one `egui` stores on its [`Visuals`], so a caller that has the variant
    /// never has to name a [`Palette`] itself.
    pub const fn palette(self, dark: bool) -> Palette {
        if dark { self.dark() } else { self.light() }
    }
}

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

/// The ribbon's own shade: the surface lifted one step toward the panel.
///
/// The approved presets give the ribbon a value of its own, and it sits one step above `surface` in
/// both variants (lighter on paper and on graphite). Deriving it keeps `Palette` at the tokens `egui`
/// can carry; a fifth colour would need a second store for no visible gain
/// (`plan/notes/U71.md`, `plan/notes/U68.md`).
pub fn ribbon(ui: &egui::Ui) -> Color32 {
    mix(surface(ui), panel(ui), 40)
}

/// The darker strip a ribbon group is captioned in: the list and the band under the tool row.
pub fn band(ui: &egui::Ui) -> Color32 {
    chrome(ui)
}

/// The fill of a ribbon group's caption band: the control tone one quiet step toward [`band`].
///
/// The chrome itself is the darkest surface of the window, and a strip of it under every group read
/// as a hole cut through the ribbon. Mixing the control tone a third of the way to the chrome keeps
/// the band below the buttons it captions without taking it out of the strip (U75).
pub fn caption_band(ui: &egui::Ui) -> Color32 {
    mix(control(ui), band(ui), 30)
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

/// The ink of a guide the active theme draws: a bracket, a work cursor, a crosshair.
///
/// A guide is the program's own help over the model, so it is one warm amber that never takes the
/// accent of an action, the danger of a refusal or the cyan of a selection (ADR 0111, U83).
pub fn helper(ui: &egui::Ui) -> Color32 {
    helper_ink(warn(ui), ui.visuals().dark_mode)
}

/// How far [`helper`] is lifted away from the warning amber, in percent.
///
/// A guide is not a warning and must not be read as one: lifted a quarter of the way off the
/// surface it has a value of its own — brighter than a warning on slate, deeper on paper — and it
/// keeps the warm hue that says the program drew it.
const HELPER_LIFT: u32 = 25;

/// `warn` lifted [`HELPER_LIFT`] percent away from the surface, which is the guide ink.
///
/// The lift follows the variant rather than the preset — white on a dark palette, black on a light
/// one, the direction [`Palette::readable`] pushes an accent — so the ink is the same in all three
/// presets and the scene can carry it as a constant (`SceneColors::helper`). A test pins the
/// contrast on the surface, the panel and the workspace of every variant.
fn helper_ink(warn: Color32, dark: bool) -> Color32 {
    mix(warn, if dark { Color32::WHITE } else { Color32::BLACK }, HELPER_LIFT)
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

/// Installs both variants of `preset` and the theme preference on `ctx`.
///
/// The active variant is `egui`'s business: it keeps the two styles apart and swaps them when the
/// preference changes, so the settings window only has to call this once per change. `accent` is the
/// user's override, or `None` to let each variant draw the accent its ramp names; an override is one
/// colour for both variants, so [`Palette::with_accent`] keeps it readable on each surface.
pub fn install_with(
    ctx: &egui::Context,
    preference: egui::ThemePreference,
    preset: ThemePreset,
    accent: Option<[u8; 3]>,
) {
    let (mut dark, mut light) = (preset.dark(), preset.light());
    if let Some(accent) = accent {
        let accent = Color32::from_rgb(accent[0], accent[1], accent[2]);
        dark = dark.with_accent(accent);
        light = light.with_accent(accent);
    }
    ctx.style_mut_of(egui::Theme::Dark, |style| apply_with(style, dark));
    ctx.style_mut_of(egui::Theme::Light, |style| apply_with(style, light));
    ctx.set_theme(preference);
}

/// The viewport colours of the active variant, for the `wgpu` frame.
///
/// The viewport is not an `egui` surface, so it cannot read the chrome palette at draw time; this is
/// the one bridge from the active [`egui::Theme`] to the scene's non-material colours. The shell
/// reads it once per frame, so a preference switch, including *System* following the OS, reaches the
/// background, the grid and the unlit ink on the next frame (X8).
///
/// The palette's `dark` flag is the same bit `egui` switches on, so the two can never disagree about
/// which variant is active.
pub fn scene_colors(theme: egui::Theme) -> SceneColors {
    match theme {
        egui::Theme::Dark => SceneColors::DARK,
        egui::Theme::Light => SceneColors::LIGHT,
    }
}

/// `a` moved `percent` of the way to `b`, channel by channel.
///
/// Integer arithmetic on purpose: a ramp is a constant, so a palette must never depend on the
/// rounding of a float at start-up, and `percent` above 100 is the same as 100.
pub const fn mix(a: Color32, b: Color32, percent: u32) -> Color32 {
    Color32::from_rgb(
        mix_channel(a.r(), b.r(), percent),
        mix_channel(a.g(), b.g(), percent),
        mix_channel(a.b(), b.b(), percent),
    )
}

/// One channel of [`mix`], rounded to the nearest step.
const fn mix_channel(a: u8, b: u8, percent: u32) -> u8 {
    let percent = if percent > 100 { 100 } else { percent };
    ((a as u32 * (100 - percent) + b as u32 * percent + 50) / 100) as u8
}

/// An opaque `fill` drawn at `percent` opacity.
///
/// `Color32` is premultiplied, so the alpha alone is scaled and the colour is rebuilt: a translucent
/// surface is the same colour with a smaller alpha, not a darker one. The input has to be opaque —
/// every chrome surface is.
/// The neutral veil a modal lays over the window.
///
/// It is black at about 63 %, the same in both variants: the veil dims what is behind it, it is not
/// a palette entry, so it has no light-theme value of its own (U80).
pub fn scrim() -> Color32 {
    Color32::from_black_alpha(160)
}

pub fn at_opacity(fill: Color32, percent: u32) -> Color32 {
    let alpha = (u16::from(fill.a()) * u16::from(percent.min(100) as u8) / 100) as u8;
    Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha)
}

/// The WCAG contrast ratio between two opaque colours, from 1 (equal) to 21.
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (first, second) = (relative_luminance(a), relative_luminance(b));
    let (bright, dark) = if first > second { (first, second) } else { (second, first) };
    (bright + 0.05) / (dark + 0.05)
}

/// The relative luminance of an opaque colour, as WCAG defines it.
pub fn relative_luminance(color: Color32) -> f32 {
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// One sRGB channel, undoing the transfer curve.
fn linear(channel: u8) -> f32 {
    let channel = f32::from(channel) / 255.0;
    if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
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

    /// The spread of a colour's channels: how far it is from grey.
    fn spread(color: Color32) -> i32 {
        let (r, g, b) = (i32::from(color.r()), i32::from(color.g()), i32::from(color.b()));
        r.max(g).max(b) - r.min(g).min(b)
    }

    /// Every preset and variant, with the name a failure has to print.
    fn every_palette() -> Vec<(String, Palette)> {
        let mut palettes = Vec::new();
        for preset in ThemePreset::ALL {
            palettes.push((format!("{preset:?} dark"), preset.dark()));
            palettes.push((format!("{preset:?} light"), preset.light()));
        }
        palettes
    }

    #[test]
    fn every_palette_keeps_its_text_readable() {
        for (name, palette) in every_palette() {
            let ratio = |a: Color32, b: Color32| contrast_ratio(a, b);
            for (ink, fill, least, what) in [
                (palette.text, palette.panel, 4.5, "body text on a panel"),
                (palette.text, palette.surface, 4.5, "body text on a header"),
                (palette.text, palette.control, 4.5, "body text on a control"),
                (palette.text, palette.accent_fill, 3.0, "body text on the filled accent"),
                (palette.text_dim, palette.panel, 3.0, "dim text on a panel"),
                (palette.text_dim, palette.surface, 3.0, "dim text on a header"),
                (palette.accent, palette.surface, ACCENT_CONTRAST, "the accent on a header"),
                (palette.accent, palette.panel, ACCENT_CONTRAST, "the accent on a panel"),
                (palette.danger, palette.panel, 3.0, "a destructive label"),
                (palette.warn, palette.panel, 3.0, "a warning label"),
            ] {
                let measured = ratio(ink, fill);
                assert!(measured >= least, "{name}: {what} is {measured:.2}:1, under {least}:1");
            }
            // Disabled ink is quieter than dim ink, measured against the surface both sit on.
            assert!(
                ratio(palette.text_disabled, palette.panel) < ratio(palette.text_dim, palette.panel),
                "{name}: disabled text has to read as quieter than dim text"
            );
            // A hover and a press are visible against the control they replace.
            assert_ne!(palette.control_hover, palette.control, "{name}: hovering changes nothing");
            assert!(
                relative_luminance(palette.control_active) < relative_luminance(palette.control),
                "{name}: a pressed control sinks into the panel"
            );
            assert_ne!(palette.line, palette.panel, "{name}: a hairline has to be visible on a panel");
        }
    }

    /// The guide ink a frame of `preset` in one variant draws, read through the accessor.
    ///
    /// The accessor reads the installed [`egui::Visuals`], so this is the path a bracket takes: a
    /// palette-derived ink that a fallback to [`DARK`] would quietly break in the light variant.
    fn helper_ink_of(preset: ThemePreset, dark: bool) -> Color32 {
        let ctx = egui::Context::default();
        let preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light };
        install_with(&ctx, preference, preset, None);
        let mut ink = Color32::TRANSPARENT;
        let output = ctx.run_ui(Default::default(), |ui| ink = helper(ui));
        output.drop_without_applying_deltas();
        ink
    }

    /// An eight-bit colour as the scene stores it, so the two sides of one ink can be compared.
    fn ink_of(color: [f32; 4]) -> Color32 {
        let channel = |value: f32| (value * 255.0).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(channel(color[0]), channel(color[1]), channel(color[2]))
    }

    #[test]
    fn a_guide_reads_on_every_preset_and_never_as_an_action() {
        for preset in ThemePreset::ALL {
            for dark in [true, false] {
                let name = format!("{preset:?} {}", if dark { "dark" } else { "light" });
                let palette = preset.palette(dark);
                let ink = palette.helper();
                let colors = if dark { SceneColors::DARK } else { SceneColors::LIGHT };
                assert_eq!(ink, helper_ink_of(preset, dark), "{name}: the accessor draws the ink the ramp derives");
                assert_eq!(ink, ink_of(colors.helper), "{name}: the scene carries the same ink (ADR 0111)");
                // One ink that reads on the chrome and on the workspace, at the weight of a guide.
                for (fill, what) in [
                    (palette.surface, "the header"),
                    (palette.panel, "a panel"),
                    (ink_of(colors.background), "the workspace"),
                ] {
                    let ratio = contrast_ratio(ink, fill);
                    assert!(ratio >= 3.0, "{name}: the guide ink is {ratio:.2}:1 on {what}, under 3:1");
                }
                // A guide is not an action, a refusal or a selection.
                for (other, what) in [
                    (palette.accent, "the action accent"),
                    (palette.accent_fill, "the filled accent"),
                    (palette.danger, "the danger ink"),
                    (ink_of(furnicraft_scene::SELECTION_LINE_COLOR), "the selection ink"),
                ] {
                    assert_ne!(ink, other, "{name}: a guide is never {what}");
                }
                assert!(ink.r() > ink.g() && ink.g() > ink.b(), "{name}: a guide is a warm amber, not a tint");
                assert!(spread(ink) >= 90, "{name}: a guide has to read as a colour the program chose");
            }
        }
    }

    #[test]
    fn a_guide_width_is_a_point_rule_and_not_a_world_size() {
        // ADR 0111: three weights, one rule — the crosshair, the bracket at rest and the bracket of
        // the open level — so U88 and U89 draw a guide without inventing a thickness.
        assert_eq!(HELPER_CROSSHAIR_STROKE, 1.0, "a full-viewport crosshair is one hairline");
        assert_eq!(HELPER_STROKE, 1.5, "a bracket at rest is a point and a half");
        assert_eq!(HELPER_OPEN_STROKE, 2.0, "the bracket of the open level is two points");
        const { assert!(HELPER_CROSSHAIR_STROKE < HELPER_STROKE) };
        const { assert!(HELPER_STROKE < HELPER_OPEN_STROKE) };
        for width in [HELPER_CROSSHAIR_STROKE, HELPER_STROKE, HELPER_OPEN_STROKE] {
            assert_eq!(width % 0.5, 0.0, "a guide width lands on the half-point grid: {width}");
        }
    }

    /// The caption band and its neighbours, read inside one frame of `preset` in one variant.
    ///
    /// The accessors read the installed [`egui::Visuals`], so a test that names a preset has to run
    /// a frame instead of asking the ramp: that is the same path the ribbon takes.
    fn caption_fills(preset: ThemePreset, dark: bool) -> (Color32, Color32, Color32, Color32) {
        let ctx = egui::Context::default();
        let preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light };
        install_with(&ctx, preference, preset, None);
        let mut fills = (Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::TRANSPARENT);
        let output = ctx.run_ui(Default::default(), |ui| {
            fills = (control(ui), band(ui), caption_band(ui), text_dim(ui));
        });
        output.drop_without_applying_deltas();
        fills
    }

    #[test]
    fn the_caption_band_is_one_step_off_the_control_toward_the_chrome() {
        // U75: the band was the darkest chrome, which cut a hole under every ribbon group. It now
        // sits a third of the way from a control to the chrome, between the two in both variants.
        // On dark that means darker than a button and lighter than the chrome; on paper the chrome
        // is the lighter of the two, so the same mix reads the other way round — either way the band
        // is neither the button nor the hole.
        for preset in ThemePreset::ALL {
            for dark in [true, false] {
                let (control, band, caption, dim) = caption_fills(preset, dark);
                let (low, high) = if relative_luminance(control) < relative_luminance(band) {
                    (control, band)
                } else {
                    (band, control)
                };
                let measured = relative_luminance(caption);
                assert!(measured > relative_luminance(low), "{preset:?} {dark}: the band fell into the chrome");
                assert!(measured < relative_luminance(high), "{preset:?} {dark}: the band climbed onto a control");
                let ratio = contrast_ratio(dim, caption);
                assert!(ratio >= 3.0, "{preset:?} {dark}: the caption is {ratio:.2}:1 on its band, under 3:1");
            }
        }
    }

    #[test]
    fn a_preset_tints_the_chrome_but_never_colours_it() {
        for (name, palette) in every_palette() {
            for (what, color) in [
                ("chrome", palette.chrome),
                ("surface", palette.surface),
                ("panel", palette.panel),
                ("control", palette.control),
                ("control_hover", palette.control_hover),
                ("control_active", palette.control_active),
                ("line", palette.line),
                ("text", palette.text),
            ] {
                assert!(spread(color) <= 24, "{name}: {what} {color:?} is a colour, not a tint");
            }
            // The accent is the one saturated colour of the window: it carries the action.
            assert!(spread(palette.accent) >= 90, "{name}: the accent has to read as an action, not as grey");
            assert!(
                spread(palette.accent) > 4 * spread(palette.panel),
                "{name}: the accent has to outshine the chrome"
            );
        }
    }

    #[test]
    fn the_presets_are_three_distinct_looks() {
        for (index, first) in ThemePreset::ALL.iter().enumerate() {
            for second in &ThemePreset::ALL[index + 1..] {
                assert_ne!(first.dark(), second.dark(), "{first:?} and {second:?} draw the same dark window");
                assert_ne!(first.light(), second.light(), "{first:?} and {second:?} draw the same light window");
                assert_ne!(first.dark().accent, second.dark().accent, "{first:?} and {second:?} share an accent");
            }
        }
    }

    #[test]
    fn a_stored_preset_name_round_trips_through_the_settings_file() {
        for preset in ThemePreset::ALL {
            let json = serde_json::to_string(&preset).expect("serialise");
            assert_eq!(json, format!("\"{}\"", preset.stored_name()));
            assert_eq!(ThemePreset::from_stored_name(preset.stored_name()), Some(preset));
            let restored: ThemePreset = serde_json::from_str(&json).expect("read back");
            assert_eq!(restored, preset);
        }
        assert_eq!(ThemePreset::from_stored_name("p3_graphite"), None, "P3 is not shipped");
        assert_eq!(ThemePreset::default(), ThemePreset::GraphiteCopper, "a fresh workspace opens on P1");
    }

    #[test]
    fn an_accent_override_reads_on_both_variants() {
        // A pick that is comfortable on slate is far too bright for paper; the light variant folds it.
        let bright = Color32::from_rgb(0xe0, 0xa2, 0x4a);
        for preset in ThemePreset::ALL {
            for palette in [preset.dark(), preset.light()] {
                let overridden = palette.with_accent(bright);
                assert!(
                    contrast_ratio(overridden.accent, overridden.surface) >= ACCENT_CONTRAST,
                    "{preset:?}: the override vanishes into the surface"
                );
                assert!(
                    contrast_ratio(overridden.text, overridden.accent_fill) >= 3.0,
                    "{preset:?}: text on the derived accent fill has to read"
                );
            }
            assert_eq!(preset.dark().with_accent(bright).accent, bright, "a pick that reads is kept exactly");
            assert!(
                relative_luminance(preset.light().with_accent(bright).accent) < relative_luminance(bright),
                "the light variant folds a bright accent down"
            );
        }
    }

    #[test]
    fn installing_a_preset_leaves_no_default_colour_behind() {
        let ctx = egui::Context::default();
        for preset in ThemePreset::ALL {
            install_with(&ctx, egui::ThemePreference::Dark, preset, None);
            for (theme, palette) in [(egui::Theme::Dark, preset.dark()), (egui::Theme::Light, preset.light())] {
                let style = ctx.style_of(theme);
                assert_eq!(style.visuals.panel_fill, palette.panel, "{preset:?} {theme:?} keeps the palette");
                assert_eq!(style.visuals.override_text_color, Some(palette.text), "{preset:?} {theme:?} text");
                assert_eq!(style.visuals.hyperlink_color, palette.accent, "{preset:?} {theme:?} accent");
                assert_eq!(style.spacing.item_spacing.y, STEP, "{theme:?} keeps the rhythm");
                assert_eq!(style.text_styles.get(&TextStyle::Body).map(|font| font.size), Some(12.0));
            }
        }
        // A switch repaints the installed styles; no restart and no second install step.
        install_with(&ctx, egui::ThemePreference::Dark, ThemePreset::SlateMint, None);
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.panel_fill, ThemePreset::SlateMint.dark().panel);
        // The accent override reaches both variants and survives the switch between them.
        install_with(&ctx, egui::ThemePreference::System, ThemePreset::InkAzure, Some([0x11, 0x99, 0x44]));
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let accent = ctx.style_of(theme).visuals.hyperlink_color;
            assert!(spread(accent) > 4 * spread(ThemePreset::InkAzure.palette(theme == egui::Theme::Dark).panel));
        }
    }

    #[test]
    fn a_surface_at_an_opacity_keeps_its_colour() {
        let surface = SURFACE;
        let ninety_two = at_opacity(surface, 92);
        let [r, g, b, a] = ninety_two.to_srgba_unmultiplied();
        assert_eq!(a, 234, "92 % of an opaque fill");
        for (kept, original) in [(r, surface.r()), (g, surface.g()), (b, surface.b())] {
            assert!(
                i32::from(kept).abs_diff(i32::from(original)) <= 1,
                "the colour is kept and only the alpha moves: {kept} against {original}"
            );
        }
        assert_eq!(at_opacity(surface, 100), surface, "a full opacity is the fill itself");
        assert_eq!(at_opacity(surface, 140).a(), 255, "a value past the range clamps instead of wrapping");
        assert!(at_opacity(surface, 55).a() < ninety_two.a(), "a lower setting lets more of the scene through");
    }

    #[test]
    fn a_mix_moves_between_two_colours_and_clamps_its_weight() {
        let (black, white) = (Color32::BLACK, Color32::WHITE);
        assert_eq!(mix(black, white, 0), black);
        assert_eq!(mix(black, white, 100), white);
        assert_eq!(mix(black, white, 150), white, "a weight past 100 clamps");
        assert_eq!(mix(black, white, 50), Color32::from_rgb(128, 128, 128));
        assert!(relative_luminance(mix(black, white, 25)) < relative_luminance(mix(black, white, 75)));
        assert!((contrast_ratio(white, black) - 21.0).abs() < 0.01, "the WCAG scale runs from 1 to 21");
        assert_eq!(contrast_ratio(white, white), 1.0);
    }

    #[test]
    fn the_two_variants_paint_the_viewport_in_their_own_colours() {
        let dark = scene_colors(egui::Theme::Dark);
        let light = scene_colors(egui::Theme::Light);
        assert_eq!(dark, SceneColors::DARK, "the dark workspace draws the palette it always had");
        assert_eq!(light, SceneColors::LIGHT);
        assert!(dark.background[0] < light.background[0], "the light viewport clears to paper");
        assert!(light.grid.minor_color[0] < light.background[0], "the light grid is drawn in dark ink");
        assert!(dark.grid.minor_color[0] > dark.background[0], "the dark grid is drawn in light ink");
    }

    #[test]
    fn the_spacing_scale_is_one_step() {
        assert_eq!(STEP, 4.0, "the whole window is laid out on the 4-point grid");
        assert_eq!(HEADER_HEIGHT % STEP, 0.0, "the header sits on the grid");
        assert_eq!(HEADER_CONTROL_HEIGHT % STEP, 2.0, "a header control is half a step off, so it centres");
        const { assert!(HEADER_CONTROL_HEIGHT < HEADER_HEIGHT) };
        assert_eq!(STATUS_HEIGHT % STEP, 0.0, "the viewport bar sits on the grid");
        assert_eq!(RIBBON_CAPTION_HEIGHT, 16.0, "the caption band holds the capitals with air above and below");
        // The ribbon's metrics are the owner's approved ones (`plan/notes/U71.md`, revised by U75): a
        // 28 pt tab row, a 30 pt tool row, a 16 pt caption band, a 34 x 30 cell and a 22 pt glyph.
        assert_eq!(RIBBON_CELL_WIDTH % STEP, 2.0, "a cell is a step and a half wide, so the glyph breathes");
        const { assert!(RIBBON_CELL_WIDTH > crate::icons::RIBBON_ICON_POINT_SIZE) };
        const { assert!(RIBBON_CELL_WIDTH - crate::icons::RIBBON_ICON_POINT_SIZE >= 2.0 * STEP) };
        const { assert!(RIBBON_ROW_HEIGHT > crate::icons::RIBBON_ICON_POINT_SIZE) };
        assert_eq!(RIBBON_ROW_HEIGHT, 30.0, "the tool row is one 30 pt glyph cell, and U75 left it alone");
        // A tool with a method list keeps the whole cell and adds its caret beside it (X36), so the
        // split control is one cell wider than a cell and the glyph keeps its six points of air.
        let split = RIBBON_CELL_WIDTH + crate::icons::RIBBON_CARET_WIDTH;
        assert_eq!(split, 44.0, "a split cell is one cell and the caret beside it");
        assert_eq!(split % STEP, 0.0, "the caret column sits on the same grid as the cell");
        // The chrome rows keep one order: the header is the tallest because it carries the window
        // buttons, the tool row follows because it carries a glyph, then the tab row, the status bar
        // and the caption band.
        const { assert!(HEADER_HEIGHT > RIBBON_ROW_HEIGHT) };
        const { assert!(HEADER_HEIGHT > STATUS_HEIGHT) };
        const { assert!(RIBBON_ROW_HEIGHT > RIBBON_TAB_HEIGHT) };
        const { assert!(RIBBON_TAB_HEIGHT > RIBBON_CAPTION_HEIGHT) };
        const { assert!(RIBBON_CAPTION_HEIGHT < RIBBON_TAB_HEIGHT) };
        const { assert!(RIBBON_TAB_HEIGHT < RIBBON_ROW_HEIGHT) };
        // The two states of the approved ribbon: the tab row and its rule, plus the body when open.
        const FOLDED: f32 = RIBBON_TAB_HEIGHT + RIBBON_RULE;
        const OPEN: f32 = FOLDED + RIBBON_ROW_HEIGHT + RIBBON_CAPTION_HEIGHT + RIBBON_RULE;
        assert_eq!(FOLDED, 29.0, "the folded ribbon is its tab row and one hairline");
        assert_eq!(OPEN, 76.0, "the open ribbon is 76 points tall");
    }
}
