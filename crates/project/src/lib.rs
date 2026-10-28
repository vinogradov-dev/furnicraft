//! The scene file: an envelope around the document, and its atomic writer.
//!
//! A saved scene is UTF-8 JSON with the extension `.fcs`. The envelope owns the format identity
//! and its version, carries the serialized [`Project`] as its only payload, and preserves
//! top-level sections it does not understand so an older build never destroys a newer build's
//! data. The document keeps its own schema version and migrations in [`furnicraft_model`].
//!
//! The crate is deliberately below `engine` and `ui`: saving and loading are file operations on
//! data, and a headless job or the assistant needs them without a window. The contract is
//! specified in `docs/domain/scene-file.md`.
//!
//! What the file never stores — order data, the material catalogue, derived meshes, diagnostics,
//! undo history, selection and viewport state — is enforced by the document model, not by this
//! crate: the envelope can only serialize a [`Project`].
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use furnicraft_model::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The value of the envelope's `format` field.
pub const FORMAT: &str = "furnicraft.scene";

/// The envelope version this build writes and understands.
pub const FORMAT_VERSION: u32 = 1;

/// Largest scene file this build reads, so a hostile file cannot force an unbounded allocation.
pub const MAX_SCENE_BYTES: u64 = 1 << 30;

/// A scene file that cannot be read, parsed or written.
#[derive(Debug, Error)]
pub enum SceneFileError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The path that was read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The file is larger than this build accepts.
    #[error("{path} is {bytes} bytes; the limit is {max}")]
    TooLarge {
        /// The path that was read.
        path: PathBuf,
        /// The file size on disk.
        bytes: u64,
        /// The supported maximum.
        max: u64,
    },
    /// The file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The path that was written.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The file is not valid JSON for the envelope or the document.
    #[error("invalid scene JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The envelope has no `format` field.
    #[error("not a scene file: the `format` field is missing")]
    MissingFormat,
    /// The envelope names another format.
    #[error("not a scene file: format `{found}` is not `{expected}`")]
    WrongFormat {
        /// The value found in the file.
        found: String,
        /// The value this build writes.
        expected: &'static str,
    },
    /// The envelope was written by a newer or otherwise unsupported build.
    #[error("unsupported scene format version {found}; this build supports {supported}")]
    UnsupportedFormatVersion {
        /// The version found in the file.
        found: u32,
        /// The version this build writes.
        supported: u32,
    },
}

/// A loaded scene: the document plus any top-level sections this build does not understand.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFile {
    document: Project,
    extra: BTreeMap<String, Value>,
}

/// The serialized envelope; only reading is derived, writing is explicit to borrow the parts.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    format: Option<String>,
    format_version: u32,
    document: Project,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// The serialized envelope, borrowed from a [`SceneFile`].
#[derive(Serialize)]
struct EnvelopeRef<'a> {
    format: &'static str,
    format_version: u32,
    document: &'a Project,
    #[serde(flatten)]
    extra: &'a BTreeMap<String, Value>,
}

impl SceneFile {
    /// Wraps a document that is about to be saved. No unknown sections are carried.
    pub fn new(document: Project) -> Self {
        Self { document, extra: BTreeMap::new() }
    }

    /// Carries top-level sections from a file that was loaded earlier, so a save of that scene
    /// gives them back unchanged.
    pub fn with_extra(mut self, extra: BTreeMap<String, Value>) -> Self {
        self.extra = extra;
        self
    }

    /// Parses and checks a scene file's text.
    ///
    /// The document is migrated and validated by its own deserializer; this function checks the
    /// envelope: the format identity, the framing version and the preserved sections.
    pub fn parse(text: &str) -> Result<Self, SceneFileError> {
        let envelope: Envelope = serde_json::from_str(text)?;
        let Some(format) = envelope.format else {
            return Err(SceneFileError::MissingFormat);
        };
        if format != FORMAT {
            return Err(SceneFileError::WrongFormat { found: format, expected: FORMAT });
        }
        if envelope.format_version != FORMAT_VERSION {
            return Err(SceneFileError::UnsupportedFormatVersion {
                found: envelope.format_version,
                supported: FORMAT_VERSION,
            });
        }
        Ok(Self { document: envelope.document, extra: envelope.extra })
    }

    /// Reads and parses a scene file from disk.
    pub fn read(path: &Path) -> Result<Self, SceneFileError> {
        let metadata = fs::metadata(path).map_err(|source| SceneFileError::Read { path: path.to_owned(), source })?;
        if metadata.len() > MAX_SCENE_BYTES {
            return Err(SceneFileError::TooLarge {
                path: path.to_owned(),
                bytes: metadata.len(),
                max: MAX_SCENE_BYTES,
            });
        }
        let text = fs::read_to_string(path).map_err(|source| SceneFileError::Read { path: path.to_owned(), source })?;
        Self::parse(&text)
    }

    /// The document carried by the file.
    pub const fn document(&self) -> &Project {
        &self.document
    }

    /// Consumes the scene and returns its document.
    pub fn into_document(self) -> Project {
        self.document
    }

    /// Top-level sections this build does not understand, preserved for a newer build.
    pub const fn extra(&self) -> &BTreeMap<String, Value> {
        &self.extra
    }

    /// Serializes the envelope as pretty JSON with a trailing newline.
    pub fn to_json(&self) -> Result<String, SceneFileError> {
        let envelope = EnvelopeRef {
            format: FORMAT,
            format_version: FORMAT_VERSION,
            document: &self.document,
            extra: &self.extra,
        };
        let mut text = serde_json::to_string_pretty(&envelope)?;
        text.push('\n');
        Ok(text)
    }

    /// Writes the scene atomically: a temporary sibling is flushed and renamed over the target.
    ///
    /// A failure before the rename leaves the previous file untouched and removes the temporary
    /// file; the target is never truncated before the new content exists.
    pub fn write(&self, path: &Path) -> Result<(), SceneFileError> {
        let text = self.to_json()?;
        let temp = temp_path(path)?;
        write_and_rename(&temp, path, text.as_bytes())
    }
}

/// The sibling path a save writes before renaming: `<name>.fcs.tmp`.
fn temp_path(path: &Path) -> Result<PathBuf, SceneFileError> {
    let Some(name) = path.file_name() else {
        return Err(SceneFileError::Write {
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "the scene path has no file name"),
        });
    };
    let mut temp = name.to_os_string();
    temp.push(".tmp");
    Ok(path.with_file_name(temp))
}

/// Writes `bytes` to `temp`, syncs it and renames it over `target`.
fn write_and_rename(temp: &Path, target: &Path, bytes: &[u8]) -> Result<(), SceneFileError> {
    let mut file = File::create(temp).map_err(|source| write_failure(temp, source))?;
    if let Err(source) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(temp);
        return Err(write_failure(temp, source));
    }
    drop(file);
    if let Err(source) = fs::rename(temp, target) {
        let _ = fs::remove_file(temp);
        return Err(write_failure(target, source));
    }
    Ok(())
}

fn write_failure(path: &Path, source: io::Error) -> SceneFileError {
    SceneFileError::Write { path: path.to_owned(), source }
}
