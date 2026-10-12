// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop shortcuts that launch an engine through UnLauncher.
//!
//! On the host they are plain files in `~/.local/share/applications`. Inside
//! Flatpak the sandbox cannot write there, so they go through the
//! `org.freedesktop.portal.DynamicLauncher` portal, which asks the user for
//! confirmation and requires desktop file ids prefixed with the app id.

mod desktop_entry;
mod portal;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use common::Overwrite;

use crate::APP_ID;
use crate::error::{Error, Result};
use crate::sandbox::Sandbox;

pub use desktop_entry::render_desktop_entry;

/// Where the shortcut icons go, relative to the parent of the applications
/// folder (`~/.local/share`).
const HOST_ICON_DIR: &str = "icons/hicolor/256x256/apps";

/// A desktop shortcut (menu entry) that launches an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopShortcut {
    /// Unique suffix of the desktop file id, usually the engine id. Characters
    /// outside `[A-Za-z0-9_]` are replaced with `_` by [`desktop_file_id`].
    pub id: String,
    /// Name shown in the application menu.
    pub name: String,
    /// Optional tooltip text.
    pub comment: Option<String>,
    /// Command to run, as an argument list.
    ///
    /// Pass UnLauncher's own command, for example
    /// `["unlauncher", "--launch-engine", "<id>"]`. Inside Flatpak the portal
    /// rewrites `Exec` to run that command inside UnLauncher's sandbox
    /// (`flatpak run --command=unlauncher <app-id> …`), so the program must be
    /// a binary of the app, not a host path.
    pub exec: Vec<String>,
    /// The icon as PNG bytes (256×256 recommended; the portal accepts at most
    /// 512×512).
    pub icon_png: Vec<u8>,
}

/// Desktop file id (without `.desktop`) of the shortcut `shortcut_id`:
/// `io.github.krl0sconk.UnLauncher.engine_<id>`, with every character of the
/// id outside `[A-Za-z0-9_]` replaced with `_`.
///
/// The app id prefix is required by the portal and keeps UnLauncher's files
/// apart from everything else on the host.
#[must_use]
pub fn desktop_file_id(shortcut_id: &str) -> String {
    let sanitized: String = shortcut_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{APP_ID}.engine_{sanitized}")
}

/// Installs `shortcut` in the application menu.
///
/// - [`Sandbox::Host`]: writes `<applications_dir>/<id>.desktop` atomically
///   and the icon under `<applications_dir>/../icons/hicolor/256x256/apps/`.
///   The icon goes first, so the desktop file never points to a missing one.
/// - [`Sandbox::Flatpak`]: uses the `DynamicLauncher` portal, which shows a
///   confirmation dialog; `applications_dir` is ignored. Blocks until the user
///   answers: call it from `spawn_blocking`.
///
/// # Errors
///
/// - [`Error::ShortcutWithoutCommand`]: `shortcut.exec` is empty.
/// - [`Error::ShortcutExists`]: it exists and `overwrite` is [`Overwrite::No`].
/// - [`Error::WriteFile`] (host); [`Error::ShortcutCancelled`], [`Error::Portal`]
///   (Flatpak).
pub fn install_shortcut(
    sandbox: Sandbox,
    shortcut: &DesktopShortcut,
    applications_dir: &Path,
    overwrite: Overwrite,
) -> Result<()> {
    if shortcut.exec.is_empty() {
        return Err(Error::ShortcutWithoutCommand {
            name: shortcut.name.clone(),
        });
    }
    let id = desktop_file_id(&shortcut.id);
    let file_name = format!("{id}.desktop");
    match sandbox {
        Sandbox::Host => install_on_host(shortcut, &id, &file_name, applications_dir, overwrite),
        Sandbox::Flatpak => {
            // The portal replaces Name and Icon with the ones confirmed in
            // its dialog, so the icon value here is only a placeholder.
            let entry = render_desktop_entry(shortcut, &id);
            portal::install(
                &file_name,
                &shortcut.name,
                &shortcut.icon_png,
                &entry,
                overwrite,
            )
        }
    }
}

/// Removes the shortcut `shortcut_id` from the application menu.
///
/// On the host, removes the desktop file and its icon from the paths
/// described in [`install_shortcut`]; files that are already gone are not an
/// error. Inside Flatpak, asks the portal to remove it.
///
/// # Errors
///
/// - [`Error::RemoveFile`] if a file exists but cannot be removed (host).
/// - [`Error::Portal`] if the portal is missing or fails, including when the
///   shortcut does not exist (Flatpak).
pub fn remove_shortcut(sandbox: Sandbox, shortcut_id: &str, applications_dir: &Path) -> Result<()> {
    let id = desktop_file_id(shortcut_id);
    let file_name = format!("{id}.desktop");
    match sandbox {
        Sandbox::Host => {
            remove_if_exists(&applications_dir.join(&file_name))?;
            remove_if_exists(&host_icon_path(applications_dir, &id))?;
            tracing::info!(desktop_file = %file_name, "shortcut removed");
            Ok(())
        }
        Sandbox::Flatpak => portal::uninstall(&file_name),
    }
}

/// Host install; see [`install_shortcut`].
fn install_on_host(
    shortcut: &DesktopShortcut,
    id: &str,
    file_name: &str,
    applications_dir: &Path,
    overwrite: Overwrite,
) -> Result<()> {
    let desktop_path = applications_dir.join(file_name);
    if overwrite == Overwrite::No && desktop_path.symlink_metadata().is_ok() {
        return Err(Error::ShortcutExists { path: desktop_path });
    }
    let icon_path = host_icon_path(applications_dir, id);
    write_atomic(&icon_path, &shortcut.icon_png)?;
    // `Icon` must be text; a non-UTF-8 path falls back to the theme name,
    // which still resolves through the hicolor folder.
    let icon = icon_path.to_str().unwrap_or(id);
    let entry = render_desktop_entry(shortcut, icon);
    write_atomic(&desktop_path, entry.as_bytes())?;
    tracing::info!(path = %desktop_path.display(), "shortcut installed");
    Ok(())
}

/// Path of a shortcut's icon on the host.
fn host_icon_path(applications_dir: &Path, id: &str) -> PathBuf {
    applications_dir
        .parent()
        .unwrap_or(applications_dir)
        .join(HOST_ICON_DIR)
        .join(format!("{id}.png"))
}

/// Writes `contents` to a temporary file next to `path` and renames it over
/// `path`, creating the parent folders.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let write_error = |source| Error::WriteFile {
        path: path.to_path_buf(),
        source,
    };
    let parent = path
        .parent()
        .ok_or_else(|| write_error(io::Error::from(io::ErrorKind::InvalidInput)))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| write_error(io::Error::from(io::ErrorKind::InvalidInput)))?;
    fs::create_dir_all(parent).map_err(write_error)?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = parent.join(tmp_name);
    let result = fs::write(&tmp, contents).and_then(|()| fs::rename(&tmp, path));
    if let Err(source) = result {
        let _ = fs::remove_file(&tmp);
        return Err(write_error(source));
    }
    Ok(())
}

/// Removes `path`, treating "not found" as success.
fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::RemoveFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}
