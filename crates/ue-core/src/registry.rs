// SPDX-License-Identifier: GPL-3.0-or-later

//! The engines registered in `Install.ini`.

use std::fs;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use common::Overwrite;

use crate::engine::{Engine, EngineId};
use crate::error::{Error, Result};
use crate::file_io;
use crate::install_ini::InstallIni;
use crate::metadata::EngineMetadata;

/// An `Install.ini` entry that does not point to a usable engine.
#[derive(Debug)]
pub struct InvalidEntry {
    /// Key in `Install.ini`.
    pub id: EngineId,
    /// Path in `Install.ini`.
    pub path: PathBuf,
    /// Why the engine could not be loaded.
    pub error: Error,
}

/// The engines listed in an `Install.ini` file.
///
/// Changes are made in memory and written with [`EngineRegistry::save`], which
/// keeps everything in the file that UnLauncher does not manage.
#[derive(Debug)]
pub struct EngineRegistry {
    path: PathBuf,
    ini: InstallIni,
    engines: Vec<Engine>,
    invalid: Vec<InvalidEntry>,
}

impl EngineRegistry {
    /// Loads the registry from `ini_path`. A missing file is an empty registry.
    ///
    /// Entries whose path is not a valid engine are kept and reported by
    /// [`EngineRegistry::invalid_entries`]; they are never removed implicitly.
    ///
    /// # Errors
    ///
    /// [`Error::ReadFile`] if the file exists but cannot be read.
    pub fn load(ini_path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(ini_path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(Error::ReadFile {
                    path: ini_path.to_path_buf(),
                    source,
                });
            }
        };
        let ini = InstallIni::parse(&text);
        let mut registry = Self {
            path: ini_path.to_path_buf(),
            ini,
            engines: Vec::new(),
            invalid: Vec::new(),
        };
        for (key, value) in registry.ini.installations() {
            let id = EngineId::from_existing(&key);
            if registry.contains_id(key.as_str()) {
                tracing::warn!(engine_id = %id, "duplicate Install.ini entry ignored");
                continue;
            }
            let path = PathBuf::from(value);
            match Engine::load(id.clone(), &path) {
                Ok(engine) => registry.engines.push(engine),
                Err(error) => registry.invalid.push(InvalidEntry { id, path, error }),
            }
        }
        Ok(registry)
    }

    /// Path of the `Install.ini` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Registered engines that exist and are valid, in file order.
    #[must_use]
    pub fn engines(&self) -> &[Engine] {
        &self.engines
    }

    /// Registered entries that do not point to a valid engine.
    #[must_use]
    pub fn invalid_entries(&self) -> &[InvalidEntry] {
        &self.invalid
    }

    /// The valid engine registered with `id` (ignoring ASCII case).
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Engine> {
        self.engines.iter().find(|e| e.id.matches(id))
    }

    /// Registers the engine at `root`.
    ///
    /// Without an explicit `id`, uses `UE_<major>.<minor>`, or a new GUID if
    /// that id is taken. The path is stored as an absolute path.
    ///
    /// # Errors
    ///
    /// [`Error::NotAnEngine`] or a `Build.version` error if `root` is not a
    /// valid engine, [`Error::EngineAlreadyRegistered`] if it is already
    /// registered, [`Error::EngineIdTaken`] if `id` is in use and
    /// [`Error::NonUtf8Path`] if the path cannot be written to `Install.ini`.
    pub fn register(&mut self, root: &Path, id: Option<EngineId>) -> Result<&Engine> {
        let root = std::path::absolute(root).map_err(|source| Error::ReadFile {
            path: root.to_path_buf(),
            source,
        })?;
        // Validate before anything else, with a placeholder id.
        let mut engine = Engine::load(EngineId::from_existing("new"), &root)?;

        if let Some((existing, _)) = self.entries().find(|(_, path)| same_location(path, &root)) {
            return Err(Error::EngineAlreadyRegistered {
                id: existing.clone(),
                path: root,
            });
        }

        let id = match id {
            Some(id) => {
                if let Some((_, path)) =
                    self.entries().find(|(other, _)| other.matches(id.as_str()))
                {
                    return Err(Error::EngineIdTaken {
                        id,
                        path: path.to_path_buf(),
                    });
                }
                id
            }
            None => self.default_id(&engine),
        };

        let value = root
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path { path: root.clone() })?;
        self.ini.set(id.as_str(), value);
        tracing::info!(engine_id = %id, path = %root.display(), "engine registered");
        engine.id = id;
        self.engines.push(engine);
        let index = self.engines.len() - 1;
        Ok(&self.engines[index])
    }

    /// Removes the entry with `id`, valid or not, and returns its path. The
    /// engine files are not touched.
    ///
    /// # Errors
    ///
    /// [`Error::EngineNotRegistered`] if there is no entry with that id.
    pub fn unregister(&mut self, id: &EngineId) -> Result<PathBuf> {
        let path = if let Some(index) = self.engines.iter().position(|e| &e.id == id) {
            self.engines.remove(index).path
        } else if let Some(index) = self.invalid.iter().position(|e| &e.id == id) {
            self.invalid.remove(index).path
        } else {
            return Err(Error::EngineNotRegistered { id: id.clone() });
        };
        self.ini.remove(id.as_str());
        tracing::info!(engine_id = %id, path = %path.display(), "engine unregistered");
        Ok(path)
    }

    /// Sets the [`kind`](Engine::kind) of every valid engine from `metadata`;
    /// engines without an entry become [`EngineKind::Unknown`](crate::EngineKind::Unknown).
    pub fn apply_metadata(&mut self, metadata: &EngineMetadata) {
        for engine in &mut self.engines {
            engine.kind = metadata.kind(&engine.path);
        }
    }

    /// Writes the registry to its `Install.ini`.
    ///
    /// Creates the file and its directories if missing. An existing file is
    /// only replaced with [`Overwrite::Yes`], after copying it to
    /// `Install.ini.bak`; returns the backup path in that case.
    ///
    /// # Errors
    ///
    /// [`Error::OverwriteNotConfirmed`] if the file exists and `overwrite` is
    /// [`Overwrite::No`], and [`Error::WriteFile`] if writing fails.
    pub fn save(&self, overwrite: Overwrite) -> Result<Option<PathBuf>> {
        file_io::write_with_backup(&self.path, self.ini.render().as_bytes(), overwrite)
    }

    fn entries(&self) -> impl Iterator<Item = (&EngineId, &Path)> {
        self.engines
            .iter()
            .map(|e| (&e.id, e.path.as_path()))
            .chain(self.invalid.iter().map(|e| (&e.id, e.path.as_path())))
    }

    fn contains_id(&self, id: &str) -> bool {
        self.entries().any(|(other, _)| other.matches(id))
    }

    fn default_id(&self, engine: &Engine) -> EngineId {
        let named = format!("UE_{}.{}", engine.version.major, engine.version.minor);
        if !self.contains_id(&named) {
            return EngineId::from_existing(&named);
        }
        loop {
            let guid = random_guid();
            if !self.contains_id(&guid) {
                return EngineId::from_existing(&guid);
            }
        }
    }
}

/// Whether two paths name the same directory, resolving symlinks when both exist.
fn same_location(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A GUID in Unreal's `Install.ini` format: `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
///
/// Uses the standard library's randomly seeded hasher: unique enough for an
/// id, not meant to be unpredictable.
pub(crate) fn random_guid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let half = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    let (hi, lo) = (half(), half());
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        hi & 0xFFFF,
        lo >> 48,
        lo & 0xFFFF_FFFF_FFFF
    )
}
