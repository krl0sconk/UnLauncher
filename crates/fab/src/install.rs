// SPDX-License-Identifier: GPL-3.0-or-later

//! Copying a downloaded asset into a project or an engine.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use common::Overwrite;

use crate::asset::AssetKind;
use crate::download::DownloadedAsset;
use crate::error::{Error, Result};

/// How deep to look for `.uplugin` / `.uproject` files in a download.
const SEARCH_DEPTH: usize = 3;

/// Where to install an asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallTarget {
    /// A project, given by its `.uproject` file.
    Project(PathBuf),
    /// An engine, given by its root folder (the one containing `Engine/`).
    Engine(PathBuf),
    /// A new project folder inside `parent`, for complete-project assets.
    NewProject {
        /// Folder that receives the project folder.
        parent: PathBuf,
    },
}

/// What the downloaded files turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Layout {
    /// A folder with `<Name>.uplugin`.
    Plugin { root: PathBuf, name: OsString },
    /// A `Content/` folder.
    Content { content: PathBuf },
    /// A folder with a `.uproject`.
    Project { root: PathBuf, uproject: PathBuf },
}

/// Installs a downloaded asset and returns the folder it was copied to.
///
/// The destination comes from the downloaded files, not from the declared
/// kind:
///
/// - `<Name>.uplugin` → `<project>/Plugins/<Name>/` or
///   `<engine>/Engine/Plugins/Marketplace/<Name>/`.
/// - `Content/` → its entries go to `<project>/Content/`, keeping their names
///   so `/Game/<Pack>/…` references stay valid.
/// - `<Name>.uproject` (a complete project) → only with
///   [`InstallTarget::NewProject`]: the folder is copied to `<parent>/<Name>/`
///   and the new `.uproject` is returned. The caller sets the engine.
///
/// Copies into a hidden sibling and renames it into place. Blocking: call it
/// from `spawn_blocking`. `title` is only used in logs.
///
/// # Errors
///
/// - [`Error::AlreadyExists`]: a destination exists and `overwrite` is
///   [`Overwrite::No`]; nothing is copied.
/// - [`Error::ContentIntoEngine`], [`Error::ProjectAsset`],
///   [`Error::NotAProject`]: the asset does not fit the target.
/// - [`Error::UnknownLayout`], [`Error::TargetNotFound`], [`Error::Io`].
pub fn install(
    asset: &DownloadedAsset,
    title: &str,
    target: &InstallTarget,
    overwrite: Overwrite,
) -> Result<PathBuf> {
    let layout = detect_layout(&asset.path)?;
    if let InstallTarget::NewProject { parent } = target {
        let Layout::Project { root, uproject } = layout else {
            return Err(Error::NotAProject {
                path: asset.path.clone(),
            });
        };
        let installed = install_project(&root, &uproject, parent, overwrite)?;
        tracing::info!(%title, path = %installed.display(), "project created from asset");
        return Ok(installed);
    }
    let installed = match layout {
        Layout::Project { .. } => {
            return Err(Error::ProjectAsset {
                path: asset.path.clone(),
            });
        }
        Layout::Plugin { root, name } => {
            let plugins = match target {
                InstallTarget::NewProject { .. } => {
                    return Err(Error::NotAProject {
                        path: asset.path.clone(),
                    });
                }
                InstallTarget::Project(uproject) => project_dir(uproject)?.join("Plugins"),
                InstallTarget::Engine(engine) => engine_dir(engine)?
                    .join("Engine")
                    .join("Plugins")
                    .join("Marketplace"),
            };
            let destination = plugins.join(&name);
            check_free(&destination, overwrite)?;
            copy_into_place(&root, &destination)?;
            destination
        }
        Layout::Content { content } => {
            let uproject = match target {
                InstallTarget::Project(uproject) => uproject,
                InstallTarget::Engine(_) => return Err(Error::ContentIntoEngine),
                InstallTarget::NewProject { .. } => {
                    return Err(Error::NotAProject {
                        path: asset.path.clone(),
                    });
                }
            };
            let project_content = project_dir(uproject)?.join("Content");
            let entries = list_dir(&content)?;
            for entry in &entries {
                check_free(&project_content.join(entry), overwrite)?;
            }
            for entry in &entries {
                copy_into_place(&content.join(entry), &project_content.join(entry))?;
            }
            match entries.as_slice() {
                [single] if content.join(single).is_dir() => project_content.join(single),
                _ => project_content,
            }
        }
    };
    tracing::info!(%title, path = %installed.display(), "asset installed");
    Ok(installed)
}

/// Recognises the kind of a downloaded asset from its files.
pub(crate) fn detect_kind(files: &Path) -> AssetKind {
    match detect_layout(files) {
        Ok(Layout::Plugin { .. }) => AssetKind::Plugin,
        Ok(Layout::Content { .. }) => AssetKind::Content,
        Ok(Layout::Project { .. }) => AssetKind::Project,
        Err(_) => AssetKind::Unknown,
    }
}

fn detect_layout(files: &Path) -> Result<Layout> {
    if let Some(uproject) = find_file(files, "uproject", SEARCH_DEPTH)
        && let Some(root) = uproject.parent()
    {
        return Ok(Layout::Project {
            root: root.to_owned(),
            uproject: uproject.clone(),
        });
    }
    if let Some(uplugin) = find_file(files, "uplugin", SEARCH_DEPTH)
        && let (Some(root), Some(name)) = (uplugin.parent(), uplugin.file_stem())
    {
        return Ok(Layout::Plugin {
            root: root.to_owned(),
            name: name.to_owned(),
        });
    }
    let mut candidates = vec![files.join("Content")];
    if let [single] = list_dir(files)?.as_slice() {
        candidates.push(files.join(single).join("Content"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.is_dir())
        .map(|content| Layout::Content { content })
        .ok_or_else(|| Error::UnknownLayout {
            path: files.to_owned(),
        })
}

/// Breadth-first search for a file with `extension`, shallowest first.
fn find_file(root: &Path, extension: &str, depth: usize) -> Option<PathBuf> {
    let mut level = vec![root.to_owned()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for dir in level {
            let Ok(names) = list_dir(&dir) else { continue };
            for name in names {
                let path = dir.join(name);
                if path.is_dir() {
                    next.push(path);
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
                {
                    return Some(path);
                }
            }
        }
        level = next;
    }
    None
}

/// Copies a downloaded project folder to `<parent>/<Name>/` and returns the
/// new `.uproject` path.
fn install_project(
    root: &Path,
    uproject: &Path,
    parent: &Path,
    overwrite: Overwrite,
) -> Result<PathBuf> {
    let (Some(stem), Some(file_name)) = (uproject.file_stem(), uproject.file_name()) else {
        return Err(Error::UnknownLayout {
            path: root.to_owned(),
        });
    };
    let destination = parent.join(stem);
    check_free(&destination, overwrite)?;
    copy_into_place(root, &destination)?;
    Ok(destination.join(file_name))
}

/// Entry names of a folder, sorted.
fn list_dir(dir: &Path) -> Result<Vec<OsString>> {
    let mut names = fs::read_dir(dir)
        .map_err(|e| Error::io("read folder", dir, e))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| Error::io("read folder", dir, e))?;
    names.sort();
    Ok(names)
}

fn project_dir(uproject: &Path) -> Result<&Path> {
    let is_uproject = uproject
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"));
    match uproject.parent() {
        Some(dir) if is_uproject && uproject.is_file() => Ok(dir),
        _ => Err(Error::TargetNotFound {
            path: uproject.to_owned(),
        }),
    }
}

fn engine_dir(engine: &Path) -> Result<&Path> {
    if engine.join("Engine").is_dir() {
        Ok(engine)
    } else {
        Err(Error::TargetNotFound {
            path: engine.to_owned(),
        })
    }
}

fn check_free(destination: &Path, overwrite: Overwrite) -> Result<()> {
    if overwrite == Overwrite::No && destination.symlink_metadata().is_ok() {
        return Err(Error::AlreadyExists {
            path: destination.to_owned(),
        });
    }
    Ok(())
}

/// Copies `source` (file or folder) to a hidden sibling of `destination`,
/// then swaps it in, replacing what was there (already allowed by the caller).
fn copy_into_place(source: &Path, destination: &Path) -> Result<()> {
    let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
        return Err(Error::TargetNotFound {
            path: destination.to_owned(),
        });
    };
    fs::create_dir_all(parent).map_err(|e| Error::io("create folder", parent, e))?;
    let hidden = |suffix: &str| {
        let mut hidden = OsString::from(".");
        hidden.push(name);
        hidden.push(suffix);
        parent.join(hidden)
    };
    let temporary = hidden(".unlauncher-tmp");
    let previous = hidden(".unlauncher-old");
    remove_any(&temporary)?;
    if let Err(error) = copy_tree(source, &temporary) {
        let _ = remove_any(&temporary);
        return Err(error);
    }
    let had_previous = destination.symlink_metadata().is_ok();
    if had_previous {
        remove_any(&previous)?;
        fs::rename(destination, &previous).map_err(|e| Error::io("rename", destination, e))?;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        if had_previous {
            let _ = fs::rename(&previous, destination);
        }
        let _ = remove_any(&temporary);
        return Err(Error::io("rename", &temporary, error));
    }
    if had_previous && let Err(error) = remove_any(&previous) {
        tracing::warn!(path = %previous.display(), %error, "cannot remove the replaced copy");
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let kind = source
        .symlink_metadata()
        .map_err(|e| Error::io("read", source, e))?
        .file_type();
    if kind.is_dir() {
        fs::create_dir(destination).map_err(|e| Error::io("create folder", destination, e))?;
        for name in list_dir(source)? {
            copy_tree(&source.join(&name), &destination.join(&name))?;
        }
    } else if kind.is_file() {
        fs::copy(source, destination).map_err(|e| Error::io("copy", source, e))?;
    } else {
        tracing::warn!(path = %source.display(), "skipping a symlink or special file");
    }
    Ok(())
}

fn remove_any(path: &Path) -> Result<()> {
    let result = match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(_) => return Ok(()),
    };
    result.map_err(|e| Error::io("remove", path, e))
}
