// SPDX-License-Identifier: GPL-3.0-or-later

//! System paths. Other crates get paths from here instead of hardcoding them.
//!
//! Every function has a variant that takes `home` (and the relevant XDG
//! variable) explicitly, for tests.

use std::env;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// `Install.ini` relative to the home directory.
const INSTALL_INI: &str = ".config/Epic/UnrealEngine/Install.ini";

/// Name of UnLauncher's own folders under the XDG config and data dirs.
const APP_DIR_NAME: &str = "unlauncher";

/// Desktop files of the user's applications, relative to the home directory.
const APPLICATIONS: &str = ".local/share/applications";

/// The XDG user directories file, relative to the home directory.
const USER_DIRS: &str = ".config/user-dirs.dirs";

/// The user's home directory, from `$HOME`.
///
/// Inside Flatpak, `$HOME` is still the real home, so this points to the
/// host's files.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or(Error::HomeNotSet)
}

/// Path of Unreal's `Install.ini`: `~/.config/Epic/UnrealEngine/Install.ini`.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn install_ini_path() -> Result<PathBuf> {
    home_dir().map(|home| install_ini_path_in(&home))
}

/// [`install_ini_path`] for an explicit home directory.
///
/// Deliberately ignores `$XDG_CONFIG_HOME`: Unreal always uses `~/.config`,
/// and inside Flatpak `$XDG_CONFIG_HOME` points to the sandbox's own config.
#[must_use]
pub fn install_ini_path_in(home: &Path) -> PathBuf {
    home.join(INSTALL_INI)
}

/// Unreal's own configuration folder on the host,
/// `~/.config/Epic/UnrealEngine` (shared caches live in its `Common/`).
/// Like [`install_ini_path_in`], it ignores `$XDG_CONFIG_HOME` on purpose.
#[must_use]
pub fn unreal_config_dir_in(home: &Path) -> PathBuf {
    home.join(".config/Epic/UnrealEngine")
}

/// Directories where engines are usually installed: the home directory and
/// `/opt`.
#[must_use]
pub fn default_engine_search_paths(home: &Path) -> Vec<PathBuf> {
    vec![home.to_path_buf(), PathBuf::from("/opt")]
}

/// Directories where Unreal creates projects by default:
/// `~/Documents/Unreal Projects` and `~/Unreal Projects`.
#[must_use]
pub fn default_project_search_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Documents/Unreal Projects"),
        home.join("Unreal Projects"),
    ]
}

/// UnLauncher's own configuration folder: `$XDG_CONFIG_HOME/unlauncher`, or
/// `~/.config/unlauncher` when `$XDG_CONFIG_HOME` is unset.
///
/// Unlike [`install_ini_path`], this honours `$XDG_CONFIG_HOME` on purpose:
/// inside Flatpak it points to the sandbox's own config
/// (`~/.var/app/<app-id>/config`), which is exactly where UnLauncher's
/// settings belong.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn app_config_dir() -> Result<PathBuf> {
    let xdg = xdg_var("XDG_CONFIG_HOME");
    home_dir().map(|home| app_config_dir_in(&home, xdg.as_deref()))
}

/// [`app_config_dir`] for an explicit home and `$XDG_CONFIG_HOME`.
///
/// A relative or empty `xdg_config_home` is ignored, as the XDG Base
/// Directory spec requires.
#[must_use]
pub fn app_config_dir_in(home: &Path, xdg_config_home: Option<&Path>) -> PathBuf {
    xdg_or(home, xdg_config_home, ".config").join(APP_DIR_NAME)
}

/// UnLauncher's own data folder: `$XDG_DATA_HOME/unlauncher`, or
/// `~/.local/share/unlauncher` when `$XDG_DATA_HOME` is unset.
///
/// Like [`app_config_dir`], inside Flatpak this is the sandbox's own data
/// folder, which is what UnLauncher wants for its own data.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn app_data_dir() -> Result<PathBuf> {
    let xdg = xdg_var("XDG_DATA_HOME");
    home_dir().map(|home| app_data_dir_in(&home, xdg.as_deref()))
}

/// [`app_data_dir`] for an explicit home and `$XDG_DATA_HOME`.
///
/// A relative or empty `xdg_data_home` is ignored, as the XDG Base Directory
/// spec requires.
#[must_use]
pub fn app_data_dir_in(home: &Path, xdg_data_home: Option<&Path>) -> PathBuf {
    xdg_or(home, xdg_data_home, ".local/share").join(APP_DIR_NAME)
}

/// UnLauncher's cache folder: `$XDG_CACHE_HOME/unlauncher`, or
/// `~/.cache/unlauncher` when `$XDG_CACHE_HOME` is unset. Things that can be
/// downloaded again (Fab thumbnails) live here.
///
/// Like [`app_data_dir`], inside Flatpak this is the sandbox's own folder.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn app_cache_dir() -> Result<PathBuf> {
    let xdg = xdg_var("XDG_CACHE_HOME");
    home_dir().map(|home| app_cache_dir_in(&home, xdg.as_deref()))
}

/// [`app_cache_dir`] for an explicit home and `$XDG_CACHE_HOME`.
///
/// A relative or empty `xdg_cache_home` is ignored, as the XDG Base Directory
/// spec requires.
#[must_use]
pub fn app_cache_dir_in(home: &Path, xdg_cache_home: Option<&Path>) -> PathBuf {
    xdg_or(home, xdg_cache_home, ".cache").join(APP_DIR_NAME)
}

/// The host's folder for the user's desktop files:
/// `~/.local/share/applications`.
///
/// Deliberately ignores `$XDG_DATA_HOME`, which inside Flatpak points to the
/// sandbox; desktop shortcuts must land on the host. Inside Flatpak,
/// shortcuts go through the portal instead (see [`crate::install_shortcut`]).
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn applications_dir() -> Result<PathBuf> {
    home_dir().map(|home| applications_dir_in(&home))
}

/// [`applications_dir`] for an explicit home directory.
#[must_use]
pub fn applications_dir_in(home: &Path) -> PathBuf {
    home.join(APPLICATIONS)
}

/// The user's downloads folder on the host: `XDG_DOWNLOAD_DIR` from
/// `~/.config/user-dirs.dirs`, or `~/Downloads`.
///
/// Reads the host's file even inside Flatpak (where `$XDG_CONFIG_HOME` points
/// to the sandbox), because the browser saves downloads on the host.
///
/// # Errors
///
/// [`Error::HomeNotSet`] if `$HOME` is unset or empty.
pub fn downloads_dir() -> Result<PathBuf> {
    let home = home_dir()?;
    let user_dirs = std::fs::read_to_string(home.join(USER_DIRS)).ok();
    Ok(downloads_dir_in(&home, user_dirs.as_deref()))
}

/// [`downloads_dir`] for an explicit home and contents of `user-dirs.dirs`.
///
/// Understands `XDG_DOWNLOAD_DIR="$HOME/Downloads"` and absolute paths.
/// A value that is just `$HOME` (how `xdg-user-dirs` disables a folder), a
/// relative path or a missing entry gives `~/Downloads`.
#[must_use]
pub fn downloads_dir_in(home: &Path, user_dirs: Option<&str>) -> PathBuf {
    let fallback = home.join("Downloads");
    let Some(value) = user_dirs.and_then(|text| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
            .find_map(|line| line.strip_prefix("XDG_DOWNLOAD_DIR="))
    }) else {
        return fallback;
    };
    let value = value.trim().trim_matches('"');
    let path = if let Some(rest) = value.strip_prefix("$HOME") {
        home.join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() || path == home {
        return fallback;
    }
    path
}

/// Reads an XDG variable from the environment.
fn xdg_var(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

/// `xdg` if it is a usable absolute path, else `home/fallback`.
fn xdg_or(home: &Path, xdg: Option<&Path>, fallback: &str) -> PathBuf {
    match xdg {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => home.join(fallback),
    }
}
