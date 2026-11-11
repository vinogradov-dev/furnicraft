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
//! what the slider offers.

use std::path::{Path, PathBuf};

use egui::{Align, Context, Id, Layout, Modal, Ui};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use furnicraft_text::{Lang, Message};

use crate::display::{OpeningView, ShadingStyle, shading_label};
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

/// The *Appearance* section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// The theme variant, or the operating system's preference.
    pub theme: ThemeChoice,
    /// The language the interface speaks.
    pub language: Language,
    /// The action accent, as sRGB bytes.
    pub accent: [u8; 3],
    /// The UI scale, in percent.
    pub ui_scale: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            language: Language::default(),
            accent: [theme::ACCENT.r(), theme::ACCENT.g(), theme::ACCENT.b()],
            ui_scale: ui_scale::DEFAULT_PERCENT,
        }
    }
}

impl Appearance {
    /// The accent as a colour.
    pub fn accent_color(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.accent[0], self.accent[1], self.accent[2])
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

/// The *Ribbon* section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RibbonSettings {
    /// The opacity of the ribbon background, from 0 to 1.
    pub opacity: f32,
}

impl Default for RibbonSettings {
    fn default() -> Self {
        Self { opacity: 1.0 }
    }
}

/// Everything the workspace remembers between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The schema version written by this build.
    pub version: u32,
    /// Theme, accent and UI scale.
    pub appearance: Appearance,
    /// Default display style, grid, edges and opening view.
    pub viewport: ViewportSettings,
    /// Ribbon background opacity.
    pub ribbon: RibbonSettings,
    /// Recently opened scenes, newest first.
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            appearance: Appearance::default(),
            viewport: ViewportSettings::default(),
            ribbon: RibbonSettings::default(),
            recent: Vec::new(),
        }
    }
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
        #[derive(Deserialize)]
        struct Probe {
            #[serde(default)]
            version: u32,
        }
        let probe: Probe = serde_json::from_str(text)?;
        if probe.version != SETTINGS_VERSION {
            return Err(SettingsError::Version { found: probe.version, expected: SETTINGS_VERSION });
        }
        let mut settings: Self = serde_json::from_str(text)?;
        settings.version = SETTINGS_VERSION;
        settings.sanitize();
        Ok(settings)
    }

    /// Serialises the workspace as pretty JSON.
    pub fn to_json(&self) -> Result<String, SettingsError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Clamps every ranged value and keeps the recent list bounded.
    pub fn sanitize(&mut self) {
        self.version = SETTINGS_VERSION;
        self.appearance.ui_scale = self.appearance.ui_scale.clamp(ui_scale::MIN_PERCENT, ui_scale::MAX_PERCENT);
        self.viewport.grid_step_mm = self.viewport.grid_step_mm.clamp(MIN_GRID_STEP_MM, MAX_GRID_STEP_MM);
        self.viewport.roll_step_degrees =
            self.viewport.roll_step_degrees.clamp(MIN_ROLL_STEP_DEGREES, MAX_ROLL_STEP_DEGREES);
        self.ribbon.opacity = if self.ribbon.opacity.is_finite() {
            self.ribbon.opacity.clamp(0.0, 1.0)
        } else {
            RibbonSettings::default().opacity
        };
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

    /// The theme as the value `theme::install_with` takes.
    pub const fn install_args(&self) -> (egui::ThemePreference, [u8; 3]) {
        (self.appearance.theme.preference(), self.appearance.accent)
    }
}

/// Which section of the settings window is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    /// Theme, accent and UI scale.
    #[default]
    Appearance,
    /// Display style, grid, edges and opening view.
    Viewport,
    /// Ribbon background opacity.
    Ribbon,
    /// The shortcut table.
    Shortcuts,
}

impl SettingsSection {
    /// Every section in the order the window lists them.
    pub const ALL: [Self; 4] = [Self::Appearance, Self::Viewport, Self::Ribbon, Self::Shortcuts];

    /// The user-visible name.
    pub const fn label(self) -> Message {
        match self {
            Self::Appearance => Message::SectionAppearance,
            Self::Viewport => Message::SectionViewport,
            Self::Ribbon => Message::SectionRibbon,
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
                    SettingsSection::Ribbon => ribbon_ui(ui, settings, lang),
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

/// The *Appearance* page: theme, language, accent and UI scale.
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
    ui.label(egui::RichText::new(Message::Language.text(lang)).color(theme::text_dim(ui)));
    ui.horizontal(|ui| {
        for choice in Language::ALL {
            changed |= ui.radio_value(&mut settings.appearance.language, choice, choice.label().text(lang)).changed();
        }
    });
    ui.add_space(theme::STEP);
    ui.horizontal(|ui| {
        ui.label(Message::Accent.text(lang));
        changed |= ui.color_edit_button_srgb(&mut settings.appearance.accent).changed();
    });
    ui.add_space(theme::STEP);
    // The control edits `egui`'s zoom factor directly; the stored percentage follows the live value
    // so the next launch opens at the scale the user sees, not the one the slider started with.
    if ui_scale::show(ui).changed() {
        let live = ui_scale::percent(ui.ctx().zoom_factor());
        changed |= settings.appearance.ui_scale != live;
        settings.appearance.ui_scale = live;
    }
    ui.label(egui::RichText::new(Message::ScaleHint.text(lang)).small().color(theme::text_dim(ui)));
    changed
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
                egui::DragValue::new(&mut settings.viewport.roll_step_degrees)
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
            egui::DragValue::new(&mut settings.viewport.grid_step_mm)
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

/// The *Ribbon* page: one opacity slider.
fn ribbon_ui(ui: &mut Ui, settings: &mut Settings, lang: Lang) -> bool {
    ui.heading(Message::SectionRibbon.text(lang));
    let mut percent = settings.ribbon.opacity * 100.0;
    let response =
        ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix(" %").text(Message::BackgroundOpacity.text(lang)));
    if response.changed() {
        settings.ribbon.opacity = (percent / 100.0).clamp(0.0, 1.0);
    }
    ui.label(egui::RichText::new(Message::RibbonOpacityHint.text(lang)).small().color(theme::text_dim(ui)));
    response.changed()
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
                accent: [0x11, 0x99, 0x44],
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
            ribbon: RibbonSettings { opacity: 0.6 },
            recent: vec![PathBuf::from("/tmp/cabinet.fcs"), PathBuf::from("/tmp/shelf.fcs")],
        }
    }

    #[test]
    fn a_round_trip_through_json_restores_every_field() {
        let settings = custom();
        let json = settings.to_json().expect("serialise");
        let restored = Settings::from_json(&json).expect("parse");
        assert_eq!(restored, settings);
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
        let json = r#"{"version":1,"appearance":{"ui_scale":900},"viewport":{"grid_step_mm":0,"roll_step_degrees":0},"ribbon":{"opacity":7.0}}"#;
        let settings = Settings::from_json(json).expect("parse and repair");
        assert_eq!(settings.appearance.ui_scale, ui_scale::MAX_PERCENT);
        assert_eq!(settings.viewport.grid_step_mm, MIN_GRID_STEP_MM);
        assert_eq!(settings.viewport.roll_step_degrees, MIN_ROLL_STEP_DEGREES);
        assert_eq!(settings.ribbon.opacity, 1.0);
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
    fn a_negative_or_non_finite_opacity_falls_back_to_the_default() {
        let json = r#"{"version":1,"ribbon":{"opacity":-2.0}}"#;
        let settings = Settings::from_json(json).expect("parse and repair");
        assert_eq!(settings.ribbon.opacity, 0.0, "a negative opacity clamps to invisible");

        let mut settings = Settings { ribbon: RibbonSettings { opacity: f32::NAN }, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.ribbon.opacity, RibbonSettings::default().opacity);
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
        for key in ["ui_scale", "grid_step_mm", "roll_step_degrees", "opacity", "accent", "theme", "recent", "language"]
        {
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
