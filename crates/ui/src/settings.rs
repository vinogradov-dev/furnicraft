//! The workspace settings and their window.
//!
//! Settings are a view of the application, never a document surface: a change applies at once and
//! never reaches the `.fcs`, and a scene opened on another machine carries none of it. The window is
//! the only editor of [`Settings`]; persistence belongs to the shell, which owns the config path and
//! the atomic write (ADR 0068).
//!
//! The data is versioned and repaired on read. A missing, truncated or older file is not an error
//! the user has to solve: the loader falls back to the defaults and reports the reason in the status
//! bar. A value outside its range is clamped, so a hand-edited file cannot push the UI scale past
//! what the control offers.

use std::path::{Path, PathBuf};

use egui::{Align, Context, Id, Layout, Modal, Ui};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use furnicraft_text::{Lang, Message};

use crate::display::{OpeningView, ShadingStyle, shading_label};
use crate::scrub::Scrub;
use crate::{theme, ui_scale};

/// The settings schema this build writes and reads.
pub const SETTINGS_VERSION: u32 = 1;

/// The smallest grid step the settings offer, in millimetres.
pub const MIN_GRID_STEP_MM: u32 = 1;

/// The largest grid step the settings offer, in millimetres.
pub const MAX_GRID_STEP_MM: u32 = 10_000;

/// The smallest hand-turn step of the roll ring, in degrees.
pub const MIN_ROLL_STEP_DEGREES: u32 = 1;

/// The largest hand-turn step of the roll ring, in degrees.
pub const MAX_ROLL_STEP_DEGREES: u32 = 15;

/// How many recent files the workspace remembers.
pub const MAX_RECENT: usize = 10;

/// The theme preference offered by the settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Light text on a dark surface.
    Dark,
    /// Dark text on a light surface.
    Light,
    /// Follow the operating system.
    #[default]
    System,
}

impl ThemeChoice {
    /// Every choice in the order the window lists them.
    pub const ALL: [Self; 3] = [Self::Dark, Self::Light, Self::System];

    /// The user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::Dark => Message::ThemeDark,
            Self::Light => Message::ThemeLight,
            Self::System => Message::ThemeSystem,
        }
    }

    /// The `egui` preference that activates the choice.
    pub const fn preference(self) -> egui::ThemePreference {
        match self {
            Self::Dark => egui::ThemePreference::Dark,
            Self::Light => egui::ThemePreference::Light,
            Self::System => egui::ThemePreference::System,
        }
    }
}

/// The language the interface speaks, as the workspace stores it.
///
/// A saved `.fcs` never carries this: the language belongs to the workspace, not to the scene, so
/// the same file opens in whatever language the person looking at it chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// English.
    #[default]
    English,
    /// Russian.
    Russian,
}

impl Language {
    /// Every language in the order the window lists them.
    pub const ALL: [Self; 2] = [Self::English, Self::Russian];

    /// The message-table language this choice selects.
    pub const fn lang(self) -> Lang {
        match self {
            Self::English => Lang::En,
            Self::Russian => Lang::Ru,
        }
    }

    /// The choice that selects `lang`.
    pub const fn from_lang(lang: Lang) -> Self {
        match lang {
            Lang::En => Self::English,
            Lang::Ru => Self::Russian,
        }
    }

    /// The language's own name, never translated: a user who cannot read the current language has
    /// to be able to find their own in the list.
    pub const fn label(self) -> Message {
        match self {
            Self::English => Message::LanguageEnglish,
            Self::Russian => Message::LanguageRussian,
        }
    }
}

/// The narrowest panel opacity the settings offer, in percent.
pub const MIN_PANEL_OPACITY: u32 = 55;

/// The widest panel opacity the settings offer, in percent.
pub const MAX_PANEL_OPACITY: u32 = 100;

/// The opacity a fresh workspace opens with, in percent.
pub const DEFAULT_PANEL_OPACITY: u32 = 92;

/// The raster width below which [`IconColor::Auto`] falls back to one ink, in pixels.
///
/// A pixel count, not a point size: at 16 px a two-tone fill reads as a muddy blob and the outline
/// loses its weight, while at 20 px the three tones separate. A UI scale below 1 therefore falls
/// back on its own (`plan/notes/U70.md`).
pub const ICON_COLOR_MIN_PX: u32 = 20;

/// How the icon pipeline colours a glyph.
///
/// The setting is read by the icon pass (U70): a semantic family gives an icon its hue, and the
/// automatic mode drops to one ink where the raster is too small to carry a colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconColor {
    /// A hue by semantic family, with a monochrome fallback below a 20 px raster.
    #[default]
    Auto,
    /// A hue by semantic family at every size.
    Always,
    /// One ink everywhere.
    Never,
}

impl IconColor {
    /// Every mode in the order the window lists them.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Always, Self::Never];

    /// Whether a raster of `px` pixels carries the role inks rather than one tint.
    pub const fn uses_roles(self, px: u32) -> bool {
        match self {
            Self::Auto => px >= ICON_COLOR_MIN_PX,
            Self::Always => true,
            Self::Never => false,
        }
    }

    /// The user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::Auto => Message::IconColorAuto,
            Self::Always => Message::IconColorAlways,
            Self::Never => Message::IconColorNever,
        }
    }
}

/// The *Appearance* section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// The theme variant, or the operating system's preference.
    pub theme: ThemeChoice,
    /// The language the interface speaks.
    pub language: Language,
    /// The approved look the whole window is painted with.
    pub palette: theme::ThemePreset,
    /// The accent the user picked; `None` draws the accent the palette names.
    pub accent: Option<[u8; 3]>,
    /// The opacity of the chrome drawn over the scene, in percent.
    pub panel_opacity: u32,
    /// How the icon pipeline colours a glyph.
    pub icon_color: IconColor,
    /// The UI scale, in percent.
    pub ui_scale: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: Language::default(),
            palette: theme::ThemePreset::default(),
            accent: None,
            panel_opacity: DEFAULT_PANEL_OPACITY,
            icon_color: IconColor::default(),
            ui_scale: ui_scale::DEFAULT_PERCENT,
        }
    }
}

impl Appearance {
    /// The accent override as a colour, if the user picked one.
    pub fn accent_color(&self) -> Option<egui::Color32> {
        self.accent.map(|[r, g, b]| egui::Color32::from_rgb(r, g, b))
    }

    /// The accent the window draws with in the `dark` variant, override included.
    pub fn drawn_accent(&self, dark: bool) -> egui::Color32 {
        self.accent_color().unwrap_or_else(|| self.palette.palette(dark).accent)
    }
}

/// The *Viewport* section: what a new viewport opens with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportSettings {
    /// The display style a viewport opens with.
    pub style: ShadingStyle,
    /// Whether the floor grid is drawn.
    pub grid: bool,
    /// The minor grid spacing, in millimetres.
    pub grid_step_mm: u32,
    /// Whether the edge pass draws every triangle edge, diagonals included.
    pub all_edges: bool,
    /// The direction a viewport opens from.
    pub opening_view: OpeningView,
    /// The step of a hand turn on the corner widget's roll ring, in degrees.
    pub roll_step_degrees: u32,
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            style: ShadingStyle::default(),
            grid: true,
            grid_step_mm: 100,
            all_edges: false,
            opening_view: OpeningView::default(),
            roll_step_degrees: 1,
        }
    }
}

/// The ribbon's own state, remembered between runs.
///
/// Only the fold is persisted: which tab is open and what the numeric fields hold are session view
/// state, and U67's pinned variant will join this section when the list lands. A file written before
/// the section existed opens the body, which is the state the workspace has always started in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RibbonSettings {
    /// Whether the ribbon's tool body is drawn under its tab row.
    pub open: bool,
}

impl Default for RibbonSettings {
    fn default() -> Self {
        Self { open: true }
    }
}

/// Everything the workspace remembers between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The schema version written by this build.
    pub version: u32,
    /// Theme, palette, accent and UI scale.
    pub appearance: Appearance,
    /// Default display style, grid, edges and opening view.
    pub viewport: ViewportSettings,
    /// Recently opened scenes, newest first.
    pub recent: Vec<PathBuf>,
    /// The fold of the ribbon.
    pub ribbon: RibbonSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            appearance: Appearance::default(),
            viewport: ViewportSettings::default(),
            recent: Vec::new(),
            ribbon: RibbonSettings::default(),
        }
    }
}

/// A value the settings file carried that this build replaced with its default.
///
/// A file that loads with one unknown value is repaired rather than rejected: a palette name written
/// by a newer build must not cost the user their recent files and their viewport defaults, and the
/// shell reports the repair in the status bar instead of silently reinterpreting it (`AGENTS.md`
/// §4.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repair {
    /// The palette name is not one this build knows; the default preset is in use.
    UnknownPalette(String),
}

/// Why a settings file could not be used.
#[derive(Debug, Error)]
pub enum SettingsError {
    /// The file was written by another schema version.
    #[error("settings are version {found}, this build reads version {expected}")]
    Version {
        /// The version found in the file.
        found: u32,
        /// The version this build writes.
        expected: u32,
    },
    /// The file is not valid JSON or does not match the schema.
    #[error("settings are not readable: {0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    /// Parses a settings file, rejecting a schema version this build does not know.
    ///
    /// The version is probed before the full schema is read, because `#[serde(default)]` would
    /// otherwise turn a missing version into the current one. Every value is clamped afterwards, so
    /// a hand-edited file still yields a usable workspace.
    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        Self::read_json(text).map(|(settings, _)| settings)
    }

    /// The same read, reporting the values it had to replace with their defaults.
    ///
    /// An unknown value is a report, not a rejection: the rest of the file — the recent list, the
    /// viewport defaults, the scale — still describes the workspace the user left.
    pub fn read_json(text: &str) -> Result<(Self, Vec<Repair>), SettingsError> {
        #[derive(Deserialize)]
        struct Probe {
            #[serde(default)]
            version: u32,
        }
        let probe: Probe = serde_json::from_str(text)?;
        if probe.version != SETTINGS_VERSION {
            return Err(SettingsError::Version { found: probe.version, expected: SETTINGS_VERSION });
        }
        let mut value: serde_json::Value = serde_json::from_str(text)?;
        let mut repairs = Vec::new();
        if let Some(repair) = repair_palette(&mut value) {
            repairs.push(repair);
        }
        migrate_accent(&mut value);
        let mut settings: Self = serde_json::from_value(value)?;
        settings.version = SETTINGS_VERSION;
        settings.sanitize();
        Ok((settings, repairs))
    }

    /// Serialises the workspace as pretty JSON.
    pub fn to_json(&self) -> Result<String, SettingsError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Clamps every ranged value and keeps the recent list bounded.
    pub fn sanitize(&mut self) {
        self.version = SETTINGS_VERSION;
        self.appearance.ui_scale = self.appearance.ui_scale.clamp(ui_scale::MIN_PERCENT, ui_scale::MAX_PERCENT);
        self.appearance.panel_opacity = self.appearance.panel_opacity.clamp(MIN_PANEL_OPACITY, MAX_PANEL_OPACITY);
        self.viewport.grid_step_mm = self.viewport.grid_step_mm.clamp(MIN_GRID_STEP_MM, MAX_GRID_STEP_MM);
        self.viewport.roll_step_degrees =
            self.viewport.roll_step_degrees.clamp(MIN_ROLL_STEP_DEGREES, MAX_ROLL_STEP_DEGREES);
        let mut seen = Vec::new();
        self.recent.retain(|path| {
            let fresh = !seen.contains(path);
            if fresh {
                seen.push(path.clone());
            }
            fresh
        });
        self.recent.truncate(MAX_RECENT);
    }

    /// Moves `path` to the front of the recent list.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|candidate| candidate != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// The theme, palette and accent as the values `theme::install_with` takes.
    pub const fn install_args(&self) -> (egui::ThemePreference, theme::ThemePreset, Option<[u8; 3]>) {
        (self.appearance.theme.preference(), self.appearance.palette, self.appearance.accent)
    }
}

/// Replaces a palette name this build does not know with the default and reports it.
///
/// The stored name is read out of the JSON tree before the typed read, so one unknown name costs
/// one value and not the whole file. A value that is not a string at all is left for `serde` to
/// reject: that is a corrupt file, not an older one.
fn repair_palette(value: &mut serde_json::Value) -> Option<Repair> {
    let slot = value.get_mut("appearance")?.get_mut("palette")?;
    let serde_json::Value::String(name) = slot else {
        return None;
    };
    if theme::ThemePreset::from_stored_name(name).is_some() {
        return None;
    }
    let unknown = std::mem::take(name);
    *slot = serde_json::Value::String(theme::ThemePreset::default().stored_name().to_owned());
    Some(Repair::UnknownPalette(unknown))
}

/// Drops the accent a workspace written before U71 carried.
///
/// Before the presets there was one palette, so every file stored its accent whether or not the user
/// picked it; keeping that value would paint P1 graphite·copper in the old blue. A file without a
/// palette therefore starts on the accent its palette names, while an accent written by this build
/// is an override and is kept.
fn migrate_accent(value: &mut serde_json::Value) {
    let Some(appearance) = value.get_mut("appearance").and_then(serde_json::Value::as_object_mut) else {
        return;
    };
    if appearance.contains_key("palette") {
        return;
    }
    if appearance.contains_key("accent") {
        appearance.insert("accent".to_owned(), serde_json::Value::Null);
    }
}

/// Which section of the settings window is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    /// Theme, palette, accent and UI scale.
    #[default]
    Appearance,
    /// Display style, grid, edges and opening view.
    Viewport,
    /// The shortcut table.
    Shortcuts,
}

impl SettingsSection {
    /// Every section in the order the window lists them.
    pub const ALL: [Self; 3] = [Self::Appearance, Self::Viewport, Self::Shortcuts];

    /// The user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::Appearance => Message::SectionAppearance,
            Self::Viewport => Message::SectionViewport,
            Self::Shortcuts => Message::SectionShortcuts,
        }
    }
}

/// What the settings window reports back to the shell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettingsOutcome {
    /// A value changed and has to be applied and persisted.
    pub changed: bool,
    /// The window asked to close.
    pub close: bool,
}

/// Draws the Settings modal and returns whether a value changed or the window closed.
///
/// The window never writes a file: the shell applies the change at once and owns the atomic save.
/// Escape and a click on the backdrop close it, like every other modal of the application.
pub fn show(ctx: &Context, settings: &mut Settings, section: &mut SettingsSection) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::default();
    let lang = settings.appearance.language.lang();
    let response = Modal::new(Id::unique("settings_window")).show(ctx, |ui| {
        ui.set_width(640.0);
        ui.horizontal(|ui| {
            ui.heading(crate::text(ui.ctx(), Message::SettingsTitle));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(crate::text(ui.ctx(), Message::Done)).clicked() {
                    outcome.close = true;
                }
            });
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                for entry in SettingsSection::ALL {
                    ui.selectable_value(section, entry, entry.label().text(lang));
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(430.0);
                ui.set_min_height(320.0);
                outcome.changed |= match *section {
                    SettingsSection::Appearance => appearance_ui(ui, settings, lang),
                    SettingsSection::Viewport => viewport_ui(ui, settings, lang),
                    SettingsSection::Shortcuts => shortcuts_ui(ui, lang),
                };
            });
        });
    });
    if response.should_close() {
        outcome.close = true;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        outcome.close = true;
    }
    outcome
}

/// The *Appearance* page: theme, palette, accent, opacity, icons, language and scale.
fn appearance_ui(ui: &mut Ui, settings: &mut Settings, lang: Lang) -> bool {
    let mut changed = false;
    ui.heading(Message::SectionAppearance.text(lang));
    ui.label(egui::RichText::new(Message::Theme.text(lang)).color(theme::text_dim(ui)));
    ui.horizontal(|ui| {
        for choice in ThemeChoice::ALL {
            changed |= ui.radio_value(&mut settings.appearance.theme, choice, choice.label().text(lang)).changed();
        }
    });
    ui.add_space(theme::STEP);
    ui.label(egui::RichText::new(Message::Palette.text(lang)).color(theme::text_dim(ui)));
    ui.horizontal(|ui| {
        for preset in theme::ThemePreset::ALL {
            changed |= preset_card(ui, settings, preset, lang);
        }
    });
    ui.label(egui::RichText::new(Message::PaletteHint.text(lang)).small().color(theme::text_dim(ui)));
    ui.add_space(theme::STEP);
    ui.horizontal(|ui| {
        ui.label(Message::Accent.text(lang));
        // The button shows the colour the window actually draws with, so a palette's own accent is
        // the starting point rather than a placeholder; a pick replaces it for both variants.
        let drawn = settings.appearance.drawn_accent(ui.visuals().dark_mode).to_array();
        let mut bytes = [drawn[0], drawn[1], drawn[2]];
        // The colour well sizes itself from `interact_size`, which this theme keeps at zero width so
        // a text button is as wide as its caption; a well has to be told it is a square.
        let picked = ui
            .scope(|ui| {
                ui.spacing_mut().interact_size = egui::vec2(theme::HEADER_CONTROL_HEIGHT, theme::HEADER_CONTROL_HEIGHT);
                ui.color_edit_button_srgb(&mut bytes)
            })
            .inner;
        if picked.changed() {
            settings.appearance.accent = Some(bytes);
            changed = true;
        }
    })
    .response
    .on_hover_text(Message::AccentHint.text(lang));
    ui.add_space(theme::STEP);
    ui.horizontal(|ui| {
        ui.label(Message::PanelOpacity.text(lang));
        changed |= ui
            .add(
                egui::Slider::new(&mut settings.appearance.panel_opacity, MIN_PANEL_OPACITY..=MAX_PANEL_OPACITY)
                    .suffix(" %"),
            )
            .changed();
    });
    ui.label(egui::RichText::new(Message::PanelOpacityHint.text(lang)).small().color(theme::text_dim(ui)));
    ui.add_space(theme::STEP);
    ui.horizontal(|ui| {
        ui.label(Message::IconColor.text(lang));
        for mode in IconColor::ALL {
            changed |= ui.radio_value(&mut settings.appearance.icon_color, mode, mode.label().text(lang)).changed();
        }
    });
    ui.label(egui::RichText::new(Message::IconColorHint.text(lang)).small().color(theme::text_dim(ui)));
    ui.add_space(theme::STEP);
    ui.label(egui::RichText::new(Message::Language.text(lang)).color(theme::text_dim(ui)));
    ui.horizontal(|ui| {
        for choice in Language::ALL {
            changed |= ui.radio_value(&mut settings.appearance.language, choice, choice.label().text(lang)).changed();
        }
    });
    ui.add_space(theme::STEP);
    // The control edits `egui`'s zoom factor and writes back the percentage it applied; the stored
    // scale follows the live value, so the next launch opens at the scale the user sees.
    changed |= ui_scale::show(ui, &mut settings.appearance.ui_scale).changed();
    ui.label(egui::RichText::new(Message::ScaleHint.text(lang)).small().color(theme::text_dim(ui)));
    ui.add_space(theme::STEP * 2.0);
    // Reset is the way back from a colour picked by hand; it keeps the language and the scale, which
    // are not part of the look and which the user tuned for their own screen.
    if ui.button(Message::ResetAppearance.text(lang)).on_hover_text(Message::ResetAppearanceHint.text(lang)).clicked() {
        settings.appearance = Appearance {
            language: settings.appearance.language,
            ui_scale: settings.appearance.ui_scale,
            ..Appearance::default()
        };
        changed = true;
    }
    changed
}

/// One palette card: the sweeps of the preset, its name, and a border that marks the active one.
///
/// The card is painted in the preset's own colours for the variant the window draws, so the two
/// cards that are not active still show what choosing them would do. Picking one drops the accent
/// override, because the accent on the card is part of the look it names.
fn preset_card(ui: &mut Ui, settings: &mut Settings, preset: theme::ThemePreset, lang: Lang) -> bool {
    let ramp = preset.palette(ui.visuals().dark_mode);
    let selected = settings.appearance.palette == preset;
    let stroke =
        egui::Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { theme::accent(ui) } else { theme::line(ui) });
    let card = egui::Frame::new()
        .fill(ramp.panel)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_min_width(96.0);
            ui.horizontal(|ui| {
                for fill in [ramp.chrome, ramp.control, ramp.text, ramp.accent] {
                    swatch(ui, fill);
                }
            });
            ui.label(egui::RichText::new(preset.label().text(lang)).small().color(ramp.text));
        });
    if !card.response.interact(egui::Sense::click()).clicked() {
        return false;
    }
    if selected {
        return false;
    }
    settings.appearance.palette = preset;
    settings.appearance.accent = None;
    true
}

/// A small rectangle of one colour, so a card shows the palette it names.
fn swatch(ui: &mut Ui, fill: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(2), fill);
}

/// The *Viewport* page: the defaults a new viewport opens with.
fn viewport_ui(ui: &mut Ui, settings: &mut Settings, lang: Lang) -> bool {
    let mut changed = false;
    ui.heading(Message::SectionViewport.text(lang));
    ui.horizontal(|ui| {
        ui.label(Message::DisplayStyle.text(lang));
        egui::ComboBox::from_id_salt("settings_style")
            .selected_text(shading_label(settings.viewport.style, lang))
            .show_ui(ui, |ui| {
                for style in ShadingStyle::ALL {
                    changed |=
                        ui.selectable_value(&mut settings.viewport.style, style, shading_label(style, lang)).changed();
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label(Message::OpeningView.text(lang));
        egui::ComboBox::from_id_salt("settings_opening_view")
            .selected_text(settings.viewport.opening_view.label(lang))
            .show_ui(ui, |ui| {
                for view in OpeningView::ALL {
                    changed |=
                        ui.selectable_value(&mut settings.viewport.opening_view, view, view.label(lang)).changed();
                }
            });
    });
    ui.add_space(theme::STEP);
    ui.horizontal(|ui| {
        ui.label(Message::RollRingStep.text(lang));
        changed |= ui
            .add(
                Scrub::new(&mut settings.viewport.roll_step_degrees)
                    .range(MIN_ROLL_STEP_DEGREES..=MAX_ROLL_STEP_DEGREES)
                    .suffix(" °")
                    .speed(0.2),
            )
            .on_hover_text(Message::RollRingStepHint.text(lang))
            .changed();
    });
    changed |= ui.checkbox(&mut settings.viewport.grid, Message::ShowFloorGrid.text(lang)).changed();
    changed |= ui
        .add(
            Scrub::new(&mut settings.viewport.grid_step_mm)
                .range(MIN_GRID_STEP_MM..=MAX_GRID_STEP_MM)
                .suffix(" mm")
                .speed(5.0),
        )
        .changed();
    changed |= ui
        .checkbox(&mut settings.viewport.all_edges, Message::ShowEveryTriangleEdge.text(lang))
        .on_hover_text(Message::AllEdgesHint.text(lang))
        .changed();
    ui.label(egui::RichText::new(Message::ViewportDefaultsHint.text(lang)).small().color(theme::text_dim(ui)));
    changed
}

/// The *Shortcuts* page: the keys the shell already listens for.
fn shortcuts_ui(ui: &mut Ui, lang: Lang) -> bool {
    ui.heading(Message::SectionShortcuts.text(lang));
    ui.label(egui::RichText::new(Message::ShortcutsHint.text(lang)).small().color(theme::text_dim(ui)));
    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
        egui::Grid::new("settings_shortcuts").num_columns(2).striped(true).show(ui, |ui| {
            for (key, action) in shortcut_table(lang) {
                ui.label(egui::RichText::new(key).strong());
                ui.label(action);
                ui.end_row();
            }
        });
    });
    false
}

/// The shortcut table, built from the one source that also labels the tools.
///
/// A key shown here is a key the shell listens for; the drawing tools come from the same table the
/// ribbon hints use, so a renamed shortcut cannot leave this page stale.
pub fn shortcut_table(lang: Lang) -> Vec<(String, String)> {
    let mut rows: Vec<(&str, Message)> = vec![
        ("Ctrl+N", Message::ShortcutNewScene),
        ("Ctrl+W", Message::ShortcutCloseTab),
        ("Ctrl+S", Message::ShortcutSave),
        ("Ctrl+Z", Message::ShortcutUndo),
        ("Ctrl+Y", Message::ShortcutRedo),
        ("Ctrl+K", Message::ShortcutPalette),
        ("F", Message::ShortcutFrameAll),
        ("O", Message::ShortcutProjection),
        ("Alt+W", Message::ShortcutExpand),
        ("G", Message::ShortcutGrid),
        ("Esc", Message::ShortcutCancel),
        ("MMB", Message::ShortcutOrbit),
        ("Shift+MMB", Message::ShortcutPan),
        ("Ctrl+MMB", Message::ShortcutZoom),
    ];
    let mut table = Vec::with_capacity(rows.len() + crate::toolbars::CONTOUR_TOOLS.len());
    for (key, action) in rows.drain(..) {
        table.push((key.to_owned(), action.text(lang).to_owned()));
    }
    for tool in crate::toolbars::CONTOUR_TOOLS {
        if let Some(key) = crate::toolbars::shortcut_label(tool) {
            table.push((key.to_owned(), tool.label().text(lang).to_owned()));
        }
    }
    table
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{Container, Id, Placement, Project};

    use super::*;

    fn custom() -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            appearance: Appearance {
                theme: ThemeChoice::Light,
                language: Language::Russian,
                palette: theme::ThemePreset::InkAzure,
                accent: Some([0x11, 0x99, 0x44]),
                panel_opacity: 70,
                icon_color: IconColor::Never,
                ui_scale: 125,
            },
            viewport: ViewportSettings {
                style: ShadingStyle::Wireframe,
                grid: false,
                grid_step_mm: 25,
                all_edges: true,
                opening_view: OpeningView::Top,
                roll_step_degrees: 5,
            },
            recent: vec![PathBuf::from("/tmp/cabinet.fcs"), PathBuf::from("/tmp/shelf.fcs")],
            ribbon: RibbonSettings { open: false },
        }
    }

    #[test]
    fn a_round_trip_through_json_restores_every_field() {
        let settings = custom();
        let json = settings.to_json().expect("serialise");
        let restored = Settings::from_json(&json).expect("parse");
        assert_eq!(restored, settings);
        assert_eq!(restored.appearance.palette, theme::ThemePreset::InkAzure);
        assert_eq!(restored.appearance.accent, Some([0x11, 0x99, 0x44]));
        assert_eq!(restored.appearance.panel_opacity, 70);
        assert_eq!(restored.appearance.icon_color, IconColor::Never);
    }

    #[test]
    fn a_truncated_file_is_an_error_the_loader_can_report() {
        let json = custom().to_json().expect("serialise");
        let truncated = &json[..json.len() / 2];
        assert!(Settings::from_json(truncated).is_err());
    }

    #[test]
    fn an_unknown_or_missing_version_is_refused() {
        let json = custom().to_json().expect("serialise");
        let newer = json.replace("\"version\": 1", "\"version\": 99");
        assert!(matches!(Settings::from_json(&newer), Err(SettingsError::Version { found: 99, .. })));
        let older = json.replace("\"version\": 1", "\"version\": 0");
        assert!(matches!(Settings::from_json(&older), Err(SettingsError::Version { found: 0, .. })));
    }

    #[test]
    fn a_value_outside_its_range_is_clamped_on_read() {
        let json = r#"{"version":1,"appearance":{"ui_scale":900,"panel_opacity":5},"viewport":{"grid_step_mm":0,"roll_step_degrees":0}}"#;
        let settings = Settings::from_json(json).expect("parse and repair");
        assert_eq!(settings.appearance.ui_scale, ui_scale::MAX_PERCENT);
        assert_eq!(settings.appearance.panel_opacity, MIN_PANEL_OPACITY);
        assert_eq!(settings.viewport.grid_step_mm, MIN_GRID_STEP_MM);
        assert_eq!(settings.viewport.roll_step_degrees, MIN_ROLL_STEP_DEGREES);
    }

    #[test]
    fn the_panel_opacity_is_a_percentage_inside_its_range() {
        assert_eq!(DEFAULT_PANEL_OPACITY, 92, "the owner's default leaves the scene a little visible");
        const { assert!(MIN_PANEL_OPACITY < DEFAULT_PANEL_OPACITY) };
        const { assert!(DEFAULT_PANEL_OPACITY < MAX_PANEL_OPACITY) };
        assert_eq!(Appearance::default().panel_opacity, DEFAULT_PANEL_OPACITY);

        let json = r#"{"version":1,"appearance":{"panel_opacity":100}}"#;
        assert_eq!(Settings::from_json(json).expect("parse").appearance.panel_opacity, MAX_PANEL_OPACITY);
        let json = r#"{"version":1,"appearance":{"panel_opacity":0}}"#;
        assert_eq!(Settings::from_json(json).expect("parse").appearance.panel_opacity, MIN_PANEL_OPACITY);
    }

    #[test]
    fn the_ribbon_fold_round_trips_through_the_workspace_file() {
        let mut settings = Settings::default();
        assert!(settings.ribbon.open, "a fresh workspace draws the ribbon body");
        settings.ribbon.open = false;
        let json = settings.to_json().expect("serialise");
        let read = Settings::from_json(&json).expect("parse");
        assert_eq!(read, settings, "the workspace file keeps the fold");
        assert!(!read.ribbon.open);

        // A file written before the section existed opens the body, which is what every build before
        // U68 did, and a file that still carries U71's `ribbon` object is read as one too.
        let json = r#"{"version":1,"appearance":{"theme":"dark"}}"#;
        assert!(Settings::from_json(json).expect("load").ribbon.open);
        let json = r#"{"version":1,"ribbon":{"opacity":0.51}}"#;
        assert!(Settings::from_json(json).expect("load").ribbon.open);
    }

    #[test]
    fn a_file_from_before_the_presets_starts_on_the_palette_accent() {
        let json = r#"{"version":1,"appearance":{"theme":"dark","accent":[47,159,212],"ui_scale":100},"ribbon":{"opacity":0.51}}"#;
        let (settings, repairs) = Settings::read_json(json).expect("a pre-U71 file loads");
        assert_eq!(settings.appearance.palette, theme::ThemePreset::GraphiteCopper);
        assert_eq!(settings.appearance.accent, None, "the old accent belonged to the old palette");
        assert_eq!(settings.appearance.panel_opacity, DEFAULT_PANEL_OPACITY);
        assert_eq!(settings.appearance.ui_scale, 100);
        assert!(repairs.is_empty());

        let json = r#"{"version":1,"appearance":{"palette":"slate_mint","accent":[17,153,68]}}"#;
        let settings = Settings::from_json(json).expect("load");
        assert_eq!(settings.appearance.accent, Some([17, 153, 68]), "an accent this build wrote is an override");
    }

    #[test]
    fn a_file_without_a_palette_opens_on_the_default_look() {
        let json = r#"{"version":1,"appearance":{"theme":"dark"}}"#;
        let (settings, repairs) = Settings::read_json(json).expect("parse");
        assert_eq!(settings.appearance.palette, theme::ThemePreset::GraphiteCopper);
        assert_eq!(settings.appearance.accent, None, "no override: the palette's own accent is drawn");
        assert_eq!(settings.appearance.panel_opacity, DEFAULT_PANEL_OPACITY);
        assert!(repairs.is_empty(), "a missing field is not a repair: {repairs:?}");
    }

    #[test]
    fn an_unknown_palette_is_repaired_and_reported() {
        let json = r#"{"version":1,"appearance":{"palette":"p3_graphite","ui_scale":125},"recent":["/tmp/a.fcs"]}"#;
        let (settings, repairs) = Settings::read_json(json).expect("one unknown value is not a corrupt file");
        assert_eq!(settings.appearance.palette, theme::ThemePreset::GraphiteCopper, "P1 stands in");
        assert_eq!(settings.appearance.ui_scale, 125, "the rest of the file survives");
        assert_eq!(settings.recent, vec![PathBuf::from("/tmp/a.fcs")]);
        assert_eq!(repairs, vec![Repair::UnknownPalette("p3_graphite".to_owned())]);
        assert_eq!(
            Settings::from_json(json).expect("the plain read").appearance.palette,
            theme::ThemePreset::GraphiteCopper
        );
    }

    #[test]
    fn a_palette_that_is_not_a_name_is_still_a_corrupt_file() {
        let json = r#"{"version":1,"appearance":{"palette":7}}"#;
        assert!(Settings::from_json(json).is_err(), "a wrong type is corrupt, not older");
    }

    #[test]
    fn the_default_appearance_keeps_the_language_and_the_scale_of_a_reset() {
        // What the Reset button does: the look returns to the defaults, the words and the scale stay.
        let mut settings = custom();
        settings.appearance = Appearance {
            language: settings.appearance.language,
            ui_scale: settings.appearance.ui_scale,
            ..Appearance::default()
        };
        assert_eq!(settings.appearance.palette, theme::ThemePreset::GraphiteCopper);
        assert_eq!(settings.appearance.accent, None);
        assert_eq!(settings.appearance.panel_opacity, DEFAULT_PANEL_OPACITY);
        assert_eq!(settings.appearance.icon_color, IconColor::Auto);
        assert_eq!(settings.appearance.language, Language::Russian);
        assert_eq!(settings.appearance.ui_scale, 125);
    }

    #[test]
    fn the_icon_colour_mode_falls_back_below_twenty_pixels() {
        // The threshold is a pixel count, so a UI scale under 100 % falls back without a point-size
        // check somewhere else (U70).
        assert!(IconColor::Auto.uses_roles(ICON_COLOR_MIN_PX));
        assert!(!IconColor::Auto.uses_roles(ICON_COLOR_MIN_PX - 1));
        assert!(IconColor::Always.uses_roles(1), "always means always");
        assert!(!IconColor::Never.uses_roles(512), "never means never");
        assert_eq!(IconColor::default(), IconColor::Auto);
    }

    #[test]
    fn the_roll_step_defaults_to_one_and_is_clamped_to_the_ring_range() {
        assert_eq!(ViewportSettings::default().roll_step_degrees, 1);
        let json = r#"{"version":1,"viewport":{"roll_step_degrees":100}}"#;
        let settings = Settings::from_json(json).expect("parse and repair");
        assert_eq!(settings.viewport.roll_step_degrees, MAX_ROLL_STEP_DEGREES);

        let json = r#"{"version":1,"viewport":{"roll_step_degrees":7}}"#;
        let settings = Settings::from_json(json).expect("parse");
        assert_eq!(settings.viewport.roll_step_degrees, 7, "a value in range is kept");
    }

    #[test]
    fn the_recent_list_is_newest_first_and_bounded() {
        let mut settings = Settings::default();
        for index in 0..(MAX_RECENT + 4) {
            settings.remember(Path::new(&format!("/tmp/scene-{index}.fcs")));
        }
        assert_eq!(settings.recent.len(), MAX_RECENT);
        assert_eq!(settings.recent[0], PathBuf::from(format!("/tmp/scene-{}.fcs", MAX_RECENT + 3)));

        settings.remember(Path::new("/tmp/scene-5.fcs"));
        assert_eq!(settings.recent.len(), MAX_RECENT, "remembering an old path does not grow the list");
        assert_eq!(settings.recent[0], PathBuf::from("/tmp/scene-5.fcs"));
        let count = settings.recent.iter().filter(|path| path.ends_with("scene-5.fcs")).count();
        assert_eq!(count, 1, "an old path moves to the front instead of repeating");
    }

    #[test]
    fn settings_never_appear_in_a_serialised_document() {
        let root = Container::new(Id::new(1).expect("id"), "Scene", Placement::IDENTITY, Vec::new());
        let project = Project::new(root).expect("project");
        let document = serde_json::to_string(&project).expect("serialise");
        for key in [
            "ui_scale",
            "grid_step_mm",
            "roll_step_degrees",
            "panel_opacity",
            "accent",
            "palette",
            "theme",
            "recent",
            "language",
            "icon_color",
        ] {
            assert!(!document.contains(key), "`{key}` is workspace state and must stay out of the .fcs");
        }
    }

    #[test]
    fn the_shortcut_table_names_every_drawing_tool() {
        let rows = shortcut_table(Lang::En);
        for tool in crate::toolbars::CONTOUR_TOOLS {
            assert!(
                rows.iter().any(|(_, action)| action == tool.label().text(Lang::En)),
                "{} is missing from the shortcut page",
                tool.label().text(Lang::En)
            );
        }
    }

    #[test]
    fn a_scene_saved_in_russian_loads_in_english_with_its_names_unchanged() {
        let root = Container::new(Id::new(1).expect("id"), "Шкаф", Placement::IDENTITY, Vec::new());
        let project = Project::new(root).expect("project");
        let json = serde_json::to_string(&project).expect("serialise");
        let restored: Project = serde_json::from_str(&json).expect("load");
        assert_eq!(restored.root().name(), "Шкаф", "the language never renames a node");
        assert_eq!(Language::from_lang(Lang::En), Language::English);
    }

    #[test]
    fn a_settings_file_without_a_language_opens_in_english() {
        let json = r#"{"version":1,"appearance":{"ui_scale":100}}"#;
        let settings = Settings::from_json(json).expect("parse");
        assert_eq!(settings.appearance.language, Language::English);
        assert_eq!(settings.appearance.language.lang(), Lang::En);
    }
}
