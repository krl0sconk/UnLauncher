// SPDX-License-Identifier: GPL-3.0-or-later

//! Fallback when the engine is downloaded in the system browser: watch the
//! downloads folder and notice when an engine zip has finished downloading.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::download::engine_zip_version;

/// Suffixes browsers add to a file while it downloads (Firefox, Chromium).
const PARTIAL_SUFFIXES: [&str; 2] = [".part", ".crdownload"];

/// An engine zip that finished downloading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEngineZip {
    /// The zip.
    pub path: PathBuf,
    /// Engine version from its name, e.g. `"5.7.4"`.
    pub version: String,
}

/// Watches a downloads folder for `Linux_Unreal_Engine_*.zip` files that
/// finished downloading after the watch started.
///
/// Call [`DownloadsWatcher::poll`] every few seconds. A zip counts as
/// complete when it has no `.part` or `.crdownload` file next to it, is not
/// empty, was modified after `started_at`, and kept the same size between
/// two polls. Each zip is reported once.
#[derive(Debug)]
pub struct DownloadsWatcher {
    dir: PathBuf,
    started_at: SystemTime,
    /// Size seen at the previous poll, for zips not reported yet.
    sizes: HashMap<PathBuf, u64>,
    reported: HashSet<PathBuf>,
}

impl DownloadsWatcher {
    /// Watches `dir` for zips modified after `started_at` (older ones were
    /// already there and are ignored).
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, started_at: SystemTime) -> Self {
        Self {
            dir: dir.into(),
            started_at,
            sizes: HashMap::new(),
            reported: HashSet::new(),
        }
    }

    /// The watched folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Looks at the folder once and returns the zips that are complete now.
    /// A folder that cannot be read gives nothing.
    pub fn poll(&mut self) -> Vec<FoundEngineZip> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::debug!(dir = %self.dir.display(), %error, "cannot read the downloads folder");
                return Vec::new();
            }
        };
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(version) = engine_zip_version(name) else {
                continue;
            };
            if self.reported.contains(&path) || self.is_partial(name) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let is_new = metadata.modified().is_ok_and(|m| m >= self.started_at);
            if !metadata.is_file() || metadata.len() == 0 || !is_new {
                continue;
            }
            seen.insert(path.clone());
            if self.sizes.get(&path) == Some(&metadata.len()) {
                self.sizes.remove(&path);
                self.reported.insert(path.clone());
                tracing::info!(path = %path.display(), %version, "engine zip downloaded");
                found.push(FoundEngineZip { path, version });
            } else {
                self.sizes.insert(path, metadata.len());
            }
        }
        // Forget zips that disappeared or became partial again.
        self.sizes.retain(|path, _| seen.contains(path));
        found
    }

    fn is_partial(&self, name: &str) -> bool {
        PARTIAL_SUFFIXES
            .iter()
            .any(|suffix| self.dir.join(format!("{name}{suffix}")).exists())
    }
}

/// Polls `watcher` every `interval` until an engine zip is complete, for at
/// most `limit`. Returns the first one found, or `None` if time runs out.
pub async fn watch_for_engine_zip(
    mut watcher: DownloadsWatcher,
    interval: Duration,
    limit: Duration,
) -> Option<FoundEngineZip> {
    let deadline = tokio::time::Instant::now() + limit;
    let mut ticks = tokio::time::interval(interval);
    loop {
        ticks.tick().await;
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        let (back, found) = tokio::task::spawn_blocking(move || {
            let found = watcher.poll();
            (watcher, found)
        })
        .await
        .ok()?;
        watcher = back;
        if let Some(zip) = found.into_iter().next() {
            return Some(zip);
        }
    }
}
