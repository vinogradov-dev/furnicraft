//! Where the workspace remembers itself: the platform config directory and an atomic write.
//!
//! Settings are workspace state, not scene data, so they live in one JSON file under the operating
//! system's config directory instead of inside the `.fcs`. The writer follows the same shape as
//! `furnicraft-project`: a temporary sibling is flushed, synced and renamed over the target, so a
//! crash during a save cannot leave a half-written file and the next run still reads the previous
//! one.
//!
//! A missing, unreadable or unknown-version file is not a failure the user has to solve: [`load`]
//! returns the defaults together with a one-line reason the shell shows in the status bar.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use furnicraft_text::{Lang, Message};
use furnicraft_ui::settings::Settings;

/// The name of the settings file inside the application's config directory.
const FILE_NAME: &str = "settings.json";

/// A settings location, resolved once when the application starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsStore {
    path: Option<PathBuf>,
}

/// Why the workspace is running on its defaults, as a message that waits for a language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Notice {
    /// The platform named no config directory.
    NoLocation,
    /// The file does not exist yet; the first run is not an error.
    NoFile,
    /// The file exists but could not be read; the reason is the detail.
    ReadFailed(String),
    /// The file was read but this build cannot use it.
    Unusable(String),
    /// The file was read, but one value in it is not one this build knows.
    UnknownPalette(String),
}

impl Notice {
    /// The sentence in `lang`.
    pub(crate) fn text(&self, lang: Lang) -> String {
        match self {
            Self::NoLocation => Message::NoticeNoSettingsLocation.text(lang).to_owned(),
            Self::NoFile => Message::NoticeNoSavedSettings.text(lang).to_owned(),
            Self::ReadFailed(detail) => Message::NoticeReadFailed.with(lang, [detail]),
            Self::Unusable(detail) => Message::NoticeUnusable.with(lang, [detail]),
            Self::UnknownPalette(name) => {
                Message::NoticeUnknownPalette.with(lang, [name, Message::PaletteGraphiteCopper.text(lang)])
            }
        }
    }
}

/// The settings read from disk and the reason the defaults were used, if they were.
#[derive(Debug)]
pub(crate) struct Loaded {
    pub(crate) settings: Settings,
    pub(crate) notice: Option<Notice>,
}

impl SettingsStore {
    /// The store under the operating system's config directory.
    ///
    /// The path is `None` only when the environment names no home directory at all; the application
    /// then works with in-memory defaults for this run.
    pub(crate) fn for_workspace() -> Self {
        Self { path: config_dir().map(|directory| directory.join(FILE_NAME)) }
    }

    /// A store at an explicit path, for tests and for a future command-line override.
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// The file this store reads and writes.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Reads the settings, or returns defaults with the reason they were used.
    ///
    /// A first run has no file yet; that is the ordinary case, not an error, and the notice simply
    /// says the defaults are in use.
    pub(crate) fn load(&self) -> Loaded {
        let Some(path) = self.path() else {
            return Loaded { settings: Settings::default(), notice: Some(Notice::NoLocation) };
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Loaded { settings: Settings::default(), notice: Some(Notice::NoFile) };
            }
            Err(error) => {
                return Loaded { settings: Settings::default(), notice: Some(Notice::ReadFailed(reason(&error))) };
            }
        };
        match Settings::read_json(&text) {
            Ok((settings, repairs)) => Loaded { settings, notice: repairs.into_iter().next().map(notice) },
            Err(error) => Loaded { settings: Settings::default(), notice: Some(Notice::Unusable(error.to_string())) },
        }
    }

    /// Writes the settings atomically, creating the config directory if it is missing.
    pub(crate) fn save(&self, settings: &Settings) -> Result<(), String> {
        let Some(path) = self.path() else {
            return Ok(());
        };
        let text = settings.to_json().map_err(|error| error.to_string())?;
        write_atomically(path, text.as_bytes())
            .map_err(|error| format!("Could not save the workspace settings: {error}"))
    }
}

/// The sentence for a value the loader had to replace.
fn notice(repair: furnicraft_ui::settings::Repair) -> Notice {
    match repair {
        furnicraft_ui::settings::Repair::UnknownPalette(name) => Notice::UnknownPalette(name),
    }
}

/// The application's config directory, following each platform's convention.
fn config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(PathBuf::from).map(|appdata| appdata.join("FurniCraft"));
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support").join("FurniCraft"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from).map(|home| home.join(".config")))
        .map(|directory| directory.join("furnicraft"))
}

/// Writes `bytes` to a temporary sibling, flushes it and renames it over `target`.
fn write_atomically(target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(directory) = target.parent() {
        fs::create_dir_all(directory)?;
    }
    let temp = temp_path(target)?;
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The sibling path a save writes before renaming: `<name>.tmp`.
fn temp_path(target: &Path) -> io::Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the settings path has no file name"))?;
    let mut temp = name.to_os_string();
    temp.push(".tmp");
    Ok(target.with_file_name(temp))
}

/// A short reason for the status bar that does not depend on the platform's error wording alone.
fn reason(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        io::ErrorKind::InvalidData => "the file is not text".to_owned(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::path::PathBuf;

    use furnicraft_testkit::scratch_dir;
    use furnicraft_ui::settings::{SETTINGS_VERSION, ThemeChoice};

    use super::*;

    /// A scratch directory this test owns.
    fn scratch(name: &str) -> PathBuf {
        scratch_dir(&format!("settings-{name}"))
    }

    fn custom() -> Settings {
        let mut settings = Settings::default();
        settings.appearance.theme = ThemeChoice::Light;
        settings.appearance.accent = Some([0x11, 0x99, 0x44]);
        settings.appearance.ui_scale = 125;
        settings.viewport.grid_step_mm = 25;
        settings.appearance.panel_opacity = 70;
        settings.recent.push(PathBuf::from("/tmp/cabinet.fcs"));
        settings
    }

    #[test]
    fn a_missing_file_yields_defaults_with_a_reason() {
        let store = SettingsStore::at(scratch("missing").join(FILE_NAME));
        let loaded = store.load();
        assert_eq!(loaded.settings, Settings::default());
        assert!(
            loaded.notice.as_ref().map(|notice| notice.text(Lang::En)).unwrap_or_default().contains("defaults"),
            "a first run explains that the defaults are in use: {loaded:?}"
        );
    }

    #[test]
    fn a_saved_file_round_trips_through_the_store() {
        let directory = scratch("round-trip");
        let store = SettingsStore::at(directory.join(FILE_NAME));
        let settings = custom();
        store.save(&settings).expect("save");

        let loaded = store.load();
        assert_eq!(loaded.settings, settings);
        assert_eq!(loaded.notice, None);
        assert!(!directory.join(format!("{FILE_NAME}.tmp")).exists(), "the temporary file is renamed away");
    }

    #[test]
    fn a_truncated_file_yields_defaults_with_the_reason() {
        let directory = scratch("truncated");
        let path = directory.join(FILE_NAME);
        fs::write(&path, "{\"version\": 1, \"appearance\": {").expect("write");
        let loaded = SettingsStore::at(path).load();
        assert_eq!(loaded.settings, Settings::default());
        assert!(
            loaded.notice.as_ref().map(|notice| notice.text(Lang::En)).unwrap_or_default().contains("using defaults"),
            "{loaded:?}"
        );
    }

    #[test]
    fn a_file_from_another_version_yields_defaults_with_the_reason() {
        let directory = scratch("version");
        let path = directory.join(FILE_NAME);
        let mut settings = custom();
        settings.version = SETTINGS_VERSION + 1;
        fs::write(&path, settings.to_json().expect("serialise")).expect("write");
        let loaded = SettingsStore::at(path).load();
        assert_eq!(loaded.settings, Settings::default());
        assert!(
            loaded.notice.as_ref().map(|notice| notice.text(Lang::En)).unwrap_or_default().contains("version"),
            "the reason names the version mismatch: {loaded:?}"
        );
    }

    #[test]
    fn an_unknown_palette_loads_the_default_look_with_one_notice() {
        let directory = scratch("palette");
        let path = directory.join(FILE_NAME);
        fs::write(&path, r#"{"version":1,"appearance":{"palette":"p3_graphite","ui_scale":125}}"#).expect("write");
        let loaded = SettingsStore::at(path).load();
        assert_eq!(loaded.settings.appearance.palette, furnicraft_ui::theme::ThemePreset::GraphiteCopper);
        assert_eq!(loaded.settings.appearance.ui_scale, 125, "the rest of the file is kept");
        let notice = loaded.notice.expect("one notice, not a silent replacement");
        assert_eq!(notice, Notice::UnknownPalette("p3_graphite".to_owned()));
        let text = notice.text(Lang::En);
        assert!(text.contains("p3_graphite"), "the notice names the value it replaced: {text}");
    }

    #[test]
    fn an_out_of_range_value_is_clamped_by_the_loader() {
        let directory = scratch("clamped");
        let path = directory.join(FILE_NAME);
        fs::write(&path, r#"{"version":1,"appearance":{"ui_scale":900}}"#).expect("write");
        let loaded = SettingsStore::at(path).load();
        assert_eq!(loaded.settings.appearance.ui_scale, furnicraft_ui::ui_scale::MAX_PERCENT);
        assert_eq!(loaded.notice, None);
    }

    #[test]
    fn a_failed_save_leaves_no_temporary_file() {
        let directory = scratch("failed");
        // A directory where the file should be makes the rename fail after the temporary is written.
        let path = directory.join(FILE_NAME);
        fs::create_dir_all(&path).expect("directory in place of the file");
        let store = SettingsStore::at(path);
        assert!(store.save(&Settings::default()).is_err());
        assert!(!directory.join(format!("{FILE_NAME}.tmp")).exists());
    }
}
