// SPDX-License-Identifier: GPL-3.0-or-later

//! What UnLauncher knows about engines that `Install.ini` cannot store.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::EngineKind;
use crate::error::{Error, Result};
use crate::file_io;

/// First lines of the file, so a user who opens it knows who owns it.
const HEADER: &str = "# Engine metadata kept by UnLauncher. Install.ini cannot store it.\n\
                      # Entries are keyed by absolute engine root path.\n\n";

/// One `[[engines]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    #[serde(flatten)]
    kind: EngineKind,
}

/// The file as read from disk.
#[derive(Debug, Default, Deserialize)]
struct Document {
    #[serde(default)]
    engines: Vec<toml::Table>,
}

/// The file as written to disk.
#[derive(Serialize)]
struct OutDocument<'a> {
    engines: Vec<OutEntry<'a>>,
}

/// An entry to write: understood, or kept as read.
#[derive(Serialize)]
#[serde(untagged)]
enum OutEntry<'a> {
    Known(&'a Entry),
    Raw(&'a toml::Table),
}

/// How each engine was installed (its [`EngineKind`]), keyed by absolute
/// engine root path.
///
/// `Install.ini` only stores `<id>=<path>`, so UnLauncher keeps this in a TOML
/// file of its own (the app uses `system::paths::app_data_dir()/engines.toml`):
///
/// ```toml
/// [[engines]]
/// path = "/home/me/UE/UE_5.7"
/// kind = "prebuilt"
///
/// [[engines]]
/// path = "/home/me/UE/source"
/// kind = "source"
/// remote = "https://github.com/EpicGames/UnrealEngine.git"
/// git_ref = "5.7"
/// ```
///
/// Engines without an entry are [`EngineKind::Unknown`]. Entries this version
/// does not understand (for example a kind added by a newer UnLauncher) are
/// skipped with a warning and written back unchanged by [`EngineMetadata::save`].
#[derive(Debug, Clone)]
pub struct EngineMetadata {
    path: PathBuf,
    entries: Vec<Entry>,
    unparsed: Vec<toml::Table>,
}

impl EngineMetadata {
    /// Loads the metadata file at `path`. A missing file is empty metadata.
    ///
    /// # Errors
    ///
    /// [`Error::ReadFile`] if the file exists but cannot be read and
    /// [`Error::MetadataMalformed`] if it is not valid TOML.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(Error::ReadFile {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let document: Document =
            toml::from_str(&text).map_err(|source| Error::MetadataMalformed {
                path: path.to_path_buf(),
                source,
            })?;
        let mut entries = Vec::new();
        let mut unparsed = Vec::new();
        for table in document.engines {
            match Entry::deserialize(toml::Value::Table(table.clone())) {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "keeping unrecognised engine metadata entry");
                    unparsed.push(table);
                }
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            entries,
            unparsed,
        })
    }

    /// Path of the metadata file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How the engine at `engine_root` was installed, or
    /// [`EngineKind::Unknown`] if there is no entry for it.
    ///
    /// Paths are compared as absolute paths, and also after resolving symlinks
    /// when both exist.
    #[must_use]
    pub fn kind(&self, engine_root: &Path) -> EngineKind {
        self.position(engine_root)
            .map_or(EngineKind::Unknown, |index| {
                self.entries[index].kind.clone()
            })
    }

    /// Records how the engine at `engine_root` was installed, replacing any
    /// previous entry. Setting [`EngineKind::Unknown`] removes the entry.
    ///
    /// Only changes memory; call [`EngineMetadata::save`] to write it.
    pub fn set_kind(&mut self, engine_root: &Path, kind: EngineKind) {
        if kind == EngineKind::Unknown {
            self.remove(engine_root);
            return;
        }
        match self.position(engine_root) {
            Some(index) => self.entries[index].kind = kind,
            None => self.entries.push(Entry {
                path: absolute(engine_root),
                kind,
            }),
        }
    }

    /// Forgets the engine at `engine_root`. Does nothing if there is no entry.
    ///
    /// Only changes memory; call [`EngineMetadata::save`] to write it.
    pub fn remove(&mut self, engine_root: &Path) {
        if let Some(index) = self.position(engine_root) {
            self.entries.remove(index);
        }
    }

    /// Writes the metadata to its file atomically, creating its directories if
    /// needed.
    ///
    /// The file belongs to UnLauncher, not to the user or Unreal, so it is
    /// replaced without an `Overwrite` confirmation and without a backup.
    ///
    /// # Errors
    ///
    /// [`Error::MetadataUnserializable`] if an engine path is not valid UTF-8
    /// and [`Error::WriteFile`] if writing fails.
    pub fn save(&self) -> Result<()> {
        let engines = self
            .entries
            .iter()
            .map(OutEntry::Known)
            .chain(self.unparsed.iter().map(OutEntry::Raw))
            .collect();
        let body = toml::to_string(&OutDocument { engines }).map_err(|source| {
            Error::MetadataUnserializable {
                path: self.path.clone(),
                source,
            }
        })?;
        file_io::write_without_backup(&self.path, format!("{HEADER}{body}").as_bytes())
    }

    fn position(&self, engine_root: &Path) -> Option<usize> {
        let wanted = absolute(engine_root);
        self.entries
            .iter()
            .position(|e| e.path == wanted)
            .or_else(|| {
                let wanted = fs::canonicalize(&wanted).ok()?;
                self.entries
                    .iter()
                    .position(|e| fs::canonicalize(&e.path).is_ok_and(|p| p == wanted))
            })
    }
}

/// `path` as an absolute path, or unchanged if the current directory is unknown.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
