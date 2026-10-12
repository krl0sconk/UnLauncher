// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading and safely writing user files.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use common::Overwrite;

use crate::error::{Error, Result};

/// Reads a whole file as UTF-8.
pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|source| Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })
}

/// Returns `<path>.bak`.
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".bak");
    PathBuf::from(name)
}

/// Writes `contents` to `path` atomically.
///
/// If the file exists, it is only replaced with [`Overwrite::Yes`], after
/// copying it to `<path>.bak`; the copy's path is returned. Symlinks are
/// followed, so the file they point to is updated rather than replaced. The new
/// content goes to a temporary file in the same directory that is then renamed
/// over the original, so a crash never leaves a half-written file.
pub(crate) fn write_with_backup(
    path: &Path,
    contents: &[u8],
    overwrite: Overwrite,
) -> Result<Option<PathBuf>> {
    let write_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source: io::Error| Error::WriteFile { path, source }
    };

    let exists = path.try_exists().map_err(|source| Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    let (target, backup) = if exists {
        if overwrite == Overwrite::No {
            return Err(Error::OverwriteNotConfirmed {
                path: path.to_path_buf(),
            });
        }
        let target = fs::canonicalize(path).map_err(|source| Error::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
        let backup = backup_path(path);
        fs::copy(&target, &backup).map_err(write_err(&backup))?;
        (target, Some(backup))
    } else {
        (path.to_path_buf(), None)
    };

    replace_atomically(&target, contents, exists)?;
    Ok(backup)
}

/// Writes `contents` to `path` atomically, replacing it if it exists, with no
/// confirmation and no backup.
///
/// Only for files UnLauncher has just created itself (for example inside a
/// project that is still being assembled) or that belong to UnLauncher (its
/// engine metadata), never for user files.
pub(crate) fn write_without_backup(path: &Path, contents: &[u8]) -> Result<()> {
    let exists = path.try_exists().map_err(|source| Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    replace_atomically(path, contents, exists)
}

/// Writes `contents` to a temporary file next to `target` and renames it over
/// `target`, keeping the permissions of the existing file when `exists`.
fn replace_atomically(target: &Path, contents: &[u8], exists: bool) -> Result<()> {
    let write_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source: io::Error| Error::WriteFile { path, source }
    };

    let parent = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).map_err(write_err(parent))?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(write_err(parent))?;
    tmp.write_all(contents).map_err(write_err(tmp.path()))?;
    if exists {
        let permissions = fs::metadata(target)
            .map_err(|source| Error::ReadFile {
                path: target.to_path_buf(),
                source,
            })?
            .permissions();
        tmp.as_file()
            .set_permissions(permissions)
            .map_err(write_err(tmp.path()))?;
    }
    tmp.as_file().sync_all().map_err(write_err(tmp.path()))?;
    tmp.persist(target)
        .map_err(|e| write_err(target)(e.error))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn backup_path_appends_bak() {
        assert_eq!(
            backup_path(Path::new("/a/Install.ini")),
            Path::new("/a/Install.ini.bak")
        );
    }

    #[test]
    fn writes_through_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.ini");
        let link = dir.path().join("link.ini");
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_with_backup(&link, b"new", Overwrite::Yes).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(fs::read_to_string(backup_path(&link)).unwrap(), "old");
    }
}
