// SPDX-License-Identifier: GPL-3.0-or-later

//! Launcher settings, stored as TOML in the app's own config directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// File name inside `system::paths::app_config_dir()`.
pub const FILE_NAME: &str = "settings.toml";

/// Which theme the interface uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the system's light or dark setting.
    #[default]
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

/// Persistent launcher settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Whether the first-run detection has been shown.
    pub first_run_done: bool,
    /// Extra folders where projects are searched, besides the defaults.
    pub project_folders: Vec<PathBuf>,
    /// When Fab's list of engine versions was last read (seconds since the
    /// Unix epoch).
    pub last_update_check: Option<u64>,
    /// Engine versions Fab listed then, for the new-version notice.
    pub fab_versions: Vec<String>,
    /// Light, dark or the system's.
    pub theme: ThemeChoice,
    /// The engine launched last (its id), which gets the amber Launch button.
    pub last_engine: Option<String>,
}

/// How often Fab's list of engine versions is read.
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

impl Settings {
    /// Whether Fab's list of engine versions should be read again at `now`
    /// (seconds since the Unix epoch): at most once a day.
    pub fn update_check_due(&self, now: u64) -> bool {
        self.last_update_check
            .is_none_or(|last| now.saturating_sub(last) >= UPDATE_CHECK_INTERVAL_SECS || last > now)
    }
}

impl Settings {
    /// Loads settings from `path`. A missing file gives the defaults; a
    /// damaged one is reported and replaced by the defaults in memory.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
                tracing::warn!(path = %path.display(), %error, "ignoring invalid settings file");
                Self::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "cannot read settings file");
                Self::default()
            }
        }
    }

    /// Writes settings to `path`, creating its directory. This file belongs to
    /// UnLauncher, so it is replaced without asking.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(&dir.path().join("none.toml")),
            Settings::default()
        );
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/settings.toml");
        let settings = Settings {
            first_run_done: true,
            project_folders: vec![PathBuf::from("/data/My Projects")],
            last_update_check: Some(1_791_000_000),
            fab_versions: vec!["5.7".to_owned(), "5.8".to_owned()],
            theme: ThemeChoice::Light,
            last_engine: Some("UE_5.7".to_owned()),
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn update_check_runs_at_most_once_a_day() {
        let mut settings = Settings::default();
        assert!(settings.update_check_due(1_000_000));
        settings.last_update_check = Some(1_000_000);
        assert!(!settings.update_check_due(1_000_000 + UPDATE_CHECK_INTERVAL_SECS - 1));
        assert!(settings.update_check_due(1_000_000 + UPDATE_CHECK_INTERVAL_SECS));
        // A clock that went back does not block the check forever.
        assert!(settings.update_check_due(999_000));
    }

    #[test]
    fn old_files_without_update_fields_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "first_run_done = true\n").unwrap();
        let settings = Settings::load(&path);
        assert!(settings.first_run_done);
        assert_eq!(settings.last_update_check, None);
        assert!(settings.fab_versions.is_empty());
    }

    #[test]
    fn theme_defaults_to_the_system_and_reads_lowercase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "first_run_done = true\n").unwrap();
        assert_eq!(Settings::load(&path).theme, ThemeChoice::System);
        fs::write(&path, "theme = \"dark\"\n").unwrap();
        assert_eq!(Settings::load(&path).theme, ThemeChoice::Dark);
    }

    #[test]
    fn invalid_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "first_run_done = 'not a bool'").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
