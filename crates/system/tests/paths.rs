// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for system paths, always with an explicit fake home.

use std::path::{Path, PathBuf};

use system::paths;

#[test]
fn install_ini_is_under_dot_config_epic() {
    assert_eq!(
        paths::install_ini_path_in(Path::new("/home/me")),
        Path::new("/home/me/.config/Epic/UnrealEngine/Install.ini")
    );
}

#[test]
fn default_search_paths() {
    let home = Path::new("/home/me");
    assert_eq!(
        paths::default_engine_search_paths(home),
        [PathBuf::from("/home/me"), PathBuf::from("/opt")]
    );
    assert_eq!(
        paths::default_project_search_paths(home),
        [
            PathBuf::from("/home/me/Documents/Unreal Projects"),
            PathBuf::from("/home/me/Unreal Projects"),
        ]
    );
}

#[test]
fn app_config_dir_prefers_xdg_config_home() {
    let home = Path::new("/home/me");
    assert_eq!(
        paths::app_config_dir_in(home, Some(Path::new("/home/me/.var/app/x/config"))),
        Path::new("/home/me/.var/app/x/config/unlauncher")
    );
    assert_eq!(
        paths::app_config_dir_in(home, None),
        Path::new("/home/me/.config/unlauncher")
    );
}

#[test]
fn app_data_dir_prefers_xdg_data_home() {
    let home = Path::new("/home/me");
    assert_eq!(
        paths::app_data_dir_in(home, Some(Path::new("/data"))),
        Path::new("/data/unlauncher")
    );
    assert_eq!(
        paths::app_data_dir_in(home, None),
        Path::new("/home/me/.local/share/unlauncher")
    );
}

#[test]
fn app_cache_dir_prefers_xdg_cache_home() {
    let home = Path::new("/home/me");
    assert_eq!(
        paths::app_cache_dir_in(home, Some(Path::new("/cache"))),
        Path::new("/cache/unlauncher")
    );
    assert_eq!(
        paths::app_cache_dir_in(home, None),
        Path::new("/home/me/.cache/unlauncher")
    );
    assert_eq!(
        paths::app_cache_dir_in(home, Some(Path::new("relative"))),
        Path::new("/home/me/.cache/unlauncher")
    );
}

#[test]
fn relative_or_empty_xdg_dirs_are_ignored() {
    let home = Path::new("/home/me");
    assert_eq!(
        paths::app_config_dir_in(home, Some(Path::new("relative/config"))),
        Path::new("/home/me/.config/unlauncher")
    );
    assert_eq!(
        paths::app_data_dir_in(home, Some(Path::new(""))),
        Path::new("/home/me/.local/share/unlauncher")
    );
}

#[test]
fn applications_dir_is_on_the_host() {
    assert_eq!(
        paths::applications_dir_in(Path::new("/home/me")),
        Path::new("/home/me/.local/share/applications")
    );
}

#[test]
fn unreal_config_dir_contains_install_ini() {
    let home = Path::new("/home/me");
    assert!(paths::install_ini_path_in(home).starts_with(paths::unreal_config_dir_in(home)));
}

#[test]
fn downloads_dir_from_user_dirs() {
    let home = Path::new("/home/me");
    let user_dirs = "# This file is written by xdg-user-dirs-update\n\
                     XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n\
                     XDG_DOWNLOAD_DIR=\"$HOME/Descargas\"\n";
    assert_eq!(
        paths::downloads_dir_in(home, Some(user_dirs)),
        Path::new("/home/me/Descargas")
    );
    assert_eq!(
        paths::downloads_dir_in(home, Some("XDG_DOWNLOAD_DIR=\"/data/dl\"\n")),
        Path::new("/data/dl")
    );
}

#[test]
fn downloads_dir_falls_back_to_downloads() {
    let home = Path::new("/home/me");
    let expected = Path::new("/home/me/Downloads");
    assert_eq!(paths::downloads_dir_in(home, None), expected);
    assert_eq!(paths::downloads_dir_in(home, Some("")), expected);
    assert_eq!(
        paths::downloads_dir_in(home, Some("#XDG_DOWNLOAD_DIR=\"$HOME/x\"\n")),
        expected
    );
    // `$HOME` alone is how xdg-user-dirs disables a folder.
    assert_eq!(
        paths::downloads_dir_in(home, Some("XDG_DOWNLOAD_DIR=\"$HOME/\"\n")),
        expected
    );
    assert_eq!(
        paths::downloads_dir_in(home, Some("XDG_DOWNLOAD_DIR=\"relative\"\n")),
        expected
    );
}
