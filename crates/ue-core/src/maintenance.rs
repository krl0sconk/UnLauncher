// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk usage, cache cleaning and uninstalling engines.
//!
//! Only caches Unreal regenerates on its own are cleaned: the engine's local
//! Derived Data Cache and the shared caches under
//! `~/.config/Epic/UnrealEngine/Common`. `Engine/Intermediate` is never
//! touched: prebuilt engines need it to build C++ projects.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use common::Delete;

use crate::engine::{EngineId, validate_engine_dir};
use crate::error::{Error, Result};
use crate::metadata::EngineMetadata;
use crate::registry::EngineRegistry;

/// A cache folder that can be emptied safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    /// Short English name for the interface.
    pub name: &'static str,
    /// The folder.
    pub path: PathBuf,
    /// Bytes used (0 if missing).
    pub size: u64,
    /// Whether it is shared by every engine and project.
    pub shared: bool,
}

/// Total size of `path` in bytes, without following symlinks. Missing or
/// unreadable entries count as 0.
#[must_use]
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// The caches of the engine at `engine_root`, plus the shared caches under
/// `unreal_config_dir` (`~/.config/Epic/UnrealEngine`, from `system::paths`).
#[must_use]
pub fn caches(engine_root: &Path, unreal_config_dir: &Path) -> Vec<Cache> {
    let common = unreal_config_dir.join("Common");
    [
        (
            "Engine Derived Data Cache",
            engine_root.join("Engine/DerivedDataCache"),
            false,
        ),
        (
            "Shared Derived Data Cache",
            common.join("DerivedDataCache"),
            true,
        ),
        ("Shared Zen cache", common.join("Zen"), true),
    ]
    .into_iter()
    .map(|(name, path, shared)| Cache {
        name,
        size: dir_size(&path),
        path,
        shared,
    })
    .collect()
}

/// Empties a cache folder, keeping the folder itself. Returns the bytes freed.
///
/// Close every Unreal Editor first: the editor (and the Zen server) write to
/// these folders while running.
///
/// # Errors
///
/// [`Error::DeleteNotConfirmed`] without [`Delete::Yes`], and
/// [`Error::DeleteFile`] if something cannot be removed.
pub fn clean_cache(cache: &Path, delete: Delete) -> Result<u64> {
    if delete == Delete::No {
        return Err(Error::DeleteNotConfirmed {
            path: cache.to_path_buf(),
        });
    }
    let size = dir_size(cache);
    let entries = match fs::read_dir(cache) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(source) => {
            return Err(Error::DeleteFile {
                path: cache.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let removed = if is_dir {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        removed.map_err(|source| Error::DeleteFile { path, source })?;
    }
    tracing::info!(path = %cache.display(), bytes = size, "cache cleaned");
    Ok(size)
}

/// Deletes a project: the folder that contains `uproject`. Returns that folder.
///
/// Refuses when the folder is not clearly that project's own folder: it holds
/// more than one `.uproject`, or it is an engine. In that case nothing is
/// deleted.
///
/// # Errors
///
/// [`Error::DeleteNotConfirmed`] without [`Delete::Yes`],
/// [`Error::UProjectMalformed`] if `uproject` is not a `.uproject` file,
/// [`Error::NotOnlyThisProject`] when the folder holds more than the project,
/// and [`Error::DeleteFile`].
pub fn delete_project(uproject: &Path, delete: Delete) -> Result<PathBuf> {
    let is_uproject = uproject
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"));
    let folder = match uproject.parent() {
        Some(folder) if is_uproject && uproject.is_file() && !folder.as_os_str().is_empty() => {
            folder.to_path_buf()
        }
        _ => {
            return Err(Error::UProjectMalformed {
                path: uproject.to_path_buf(),
                reason: "not an existing .uproject file".to_owned(),
            });
        }
    };
    if delete == Delete::No {
        return Err(Error::DeleteNotConfirmed { path: folder });
    }
    if crate::project::uproject_files(&folder).len() != 1 || validate_engine_dir(&folder).is_ok() {
        return Err(Error::NotOnlyThisProject { path: folder });
    }
    fs::remove_dir_all(&folder).map_err(|source| Error::DeleteFile {
        path: folder.clone(),
        source,
    })?;
    tracing::info!(path = %folder.display(), "project deleted");
    Ok(folder)
}

/// Unregisters the engine `id` and deletes its folder. Saves `Install.ini`
/// (with a backup) and the metadata. Returns the deleted folder.
///
/// The folder is only deleted if it is still a valid engine, so a wrong entry
/// can never delete an unrelated folder; otherwise only the entry is removed.
///
/// # Errors
///
/// [`Error::DeleteNotConfirmed`] without [`Delete::Yes`],
/// [`Error::EngineNotRegistered`], [`Error::DeleteFile`] and the errors of
/// saving the registry or metadata.
pub fn uninstall_engine(
    registry: &mut EngineRegistry,
    metadata: &mut EngineMetadata,
    id: &EngineId,
    delete: Delete,
) -> Result<PathBuf> {
    let path = registry
        .engines()
        .iter()
        .map(|e| (&e.id, &e.path))
        .chain(registry.invalid_entries().iter().map(|e| (&e.id, &e.path)))
        .find(|(other, _)| *other == id)
        .map(|(_, path)| path.clone())
        .ok_or_else(|| Error::EngineNotRegistered { id: id.clone() })?;
    if delete == Delete::No {
        return Err(Error::DeleteNotConfirmed { path });
    }
    let is_engine = validate_engine_dir(&path).is_ok();
    registry.unregister(id)?;
    registry.save(common::Overwrite::Yes)?;
    metadata.remove(&path);
    metadata.save()?;
    if is_engine {
        fs::remove_dir_all(&path).map_err(|source| Error::DeleteFile {
            path: path.clone(),
            source,
        })?;
        tracing::info!(engine_id = %id, path = %path.display(), "engine uninstalled");
    }
    Ok(path)
}
