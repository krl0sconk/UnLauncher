// SPDX-License-Identifier: GPL-3.0-or-later

//! Importing a prebuilt engine zip that the user already downloaded.

mod extract;
mod plan;

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jobs::Reporter;
use ue_core::{
    Engine, EngineId, EngineKind, EngineMetadata, EngineRegistry, EngineVersion,
    read_build_version, validate_engine_dir,
};
use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::space::check_free_space;

/// Name of the progress stage reported while extracting.
pub const EXTRACTING_STAGE: &str = "Extracting";

/// An engine extracted by [`import_zip`], not registered yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEngine {
    /// Absolute engine root (the folder that contains `Engine/`).
    pub path: PathBuf,
    /// Version from `Build.version`.
    pub version: EngineVersion,
}

/// Extracts the prebuilt engine in `zip_path` into `destination`.
///
/// Blocking: call it from `spawn_blocking`. `destination` must not exist or be
/// an empty folder; nothing is overwritten. The archive may hold `Engine/` at
/// its root or inside one wrapping folder, which is stripped. Entries are
/// checked and the free space is compared with the uncompressed size before
/// anything is written. Files are streamed into a hidden folder next to
/// `destination` and renamed into place once the result is a valid engine;
/// Unix permissions are restored, and symlinks that point outside the engine
/// are skipped. Pause and cancel are honoured between files and inside large
/// ones. On any error, including cancellation, what was extracted is removed.
///
/// # Errors
///
/// - [`Error::DestinationNotEmpty`], [`Error::InvalidDestination`]: the
///   destination cannot be used.
/// - [`Error::ReadFile`], [`Error::InvalidArchive`], [`Error::ReadEntry`]: the
///   zip cannot be read; [`Error::UnsafeEntry`]: an entry would land outside
///   the destination.
/// - [`Error::NotEnoughSpace`], [`Error::WriteFile`], [`Error::Cancelled`].
/// - [`Error::NotAnEngineArchive`]: the zip does not contain an engine.
pub fn import_zip(
    zip_path: &Path,
    destination: &Path,
    reporter: &Reporter,
) -> Result<ImportedEngine> {
    let destination = std::path::absolute(destination).map_err(|source| Error::ReadFile {
        path: destination.to_path_buf(),
        source,
    })?;
    check_destination(&destination)?;
    let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
        return Err(Error::InvalidDestination { path: destination });
    };

    let file = File::open(zip_path).map_err(|source| Error::ReadFile {
        path: zip_path.to_path_buf(),
        source,
    })?;
    let mut archive =
        ZipArchive::new(BufReader::new(file)).map_err(|source| Error::InvalidArchive {
            path: zip_path.to_path_buf(),
            source,
        })?;
    let plan = plan::plan(&mut archive, zip_path)?;

    fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
        path: parent.to_path_buf(),
        source,
    })?;
    check_free_space(parent, plan.total_bytes)?;

    reporter.stage(EXTRACTING_STAGE);
    let staging = Staging::create(parent, name)?;
    tracing::info!(
        archive = %zip_path.display(),
        destination = %destination.display(),
        bytes = plan.total_bytes,
        "importing engine"
    );
    extract::extract(&mut archive, zip_path, &plan, staging.path(), reporter)?;

    let version = validate_engine_dir(staging.path())
        .and_then(|()| read_build_version(staging.path()))
        .map_err(|source| Error::NotAnEngineArchive {
            path: zip_path.to_path_buf(),
            source: Box::new(source),
        })?
        .version;

    staging.move_to(&destination)?;
    tracing::info!(path = %destination.display(), %version, "engine imported");
    Ok(ImportedEngine {
        path: destination,
        version,
    })
}

/// Registers an imported engine in `registry` and records it as
/// [`EngineKind::Prebuilt`] in `metadata`. Returns the registered engine.
///
/// Only changes memory. The caller saves both: `metadata.save()`, and
/// `registry.save(..)`, which needs the user's `Overwrite` confirmation since
/// `Install.ini` is a user file.
///
/// # Errors
///
/// [`Error::Engine`] with the errors of [`EngineRegistry::register`] (path
/// already registered, id taken, not an engine…).
pub fn register_imported(
    registry: &mut EngineRegistry,
    metadata: &mut EngineMetadata,
    engine: &ImportedEngine,
    id: Option<EngineId>,
) -> Result<Engine> {
    let mut registered = registry.register(&engine.path, id)?.clone();
    metadata.set_kind(&registered.path, EngineKind::Prebuilt);
    registry.apply_metadata(metadata);
    registered.kind = EngineKind::Prebuilt;
    Ok(registered)
}

/// Fails unless `destination` is missing or an empty folder.
fn check_destination(destination: &Path) -> Result<()> {
    let not_empty = || Error::DestinationNotEmpty {
        path: destination.to_path_buf(),
    };
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::ReadFile {
            path: destination.to_path_buf(),
            source,
        }),
        Ok(meta) if !meta.is_dir() => Err(not_empty()),
        Ok(_) => {
            let mut entries = fs::read_dir(destination).map_err(|source| Error::ReadFile {
                path: destination.to_path_buf(),
                source,
            })?;
            if entries.next().is_some() {
                Err(not_empty())
            } else {
                Ok(())
            }
        }
    }
}

/// A hidden folder next to the destination that is removed on drop unless it
/// was moved into place.
struct Staging {
    path: PathBuf,
    keep: bool,
}

impl Staging {
    fn create(parent: &Path, name: &std::ffi::OsStr) -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let mut attempt = 0u32;
        loop {
            let mut hidden = OsString::from(".");
            hidden.push(name);
            hidden.push(format!(
                ".importing-{}-{nanos:x}-{attempt}",
                std::process::id()
            ));
            let path = parent.join(hidden);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                    attempt += 1;
                }
                Err(source) => return Err(Error::WriteFile { path, source }),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Renames the folder to `destination`. `rename` replaces an empty folder
    /// and fails on a non-empty one, so nothing is overwritten.
    fn move_to(mut self, destination: &Path) -> Result<()> {
        match fs::rename(&self.path, destination) {
            Ok(()) => {
                self.keep = true;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::DirectoryNotEmpty
                        | io::ErrorKind::AlreadyExists
                        | io::ErrorKind::NotADirectory
                ) =>
            {
                Err(Error::DestinationNotEmpty {
                    path: destination.to_path_buf(),
                })
            }
            Err(source) => Err(Error::WriteFile {
                path: destination.to_path_buf(),
                source,
            }),
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.path) {
            tracing::warn!(path = %self.path.display(), %error, "failed to remove partial import");
        }
    }
}
