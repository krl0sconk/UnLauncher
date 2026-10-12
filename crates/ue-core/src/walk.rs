// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded directory walk shared by engine and project discovery.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// What to do after visiting a directory.
pub(crate) enum Visit {
    /// Look inside it.
    Descend,
    /// Do not look inside it.
    Skip,
}

/// Visits each search path and its subdirectories up to `max_depth` levels
/// below it (the search path itself is depth 0).
///
/// Hidden directories and symlinked subdirectories are not entered, missing or
/// unreadable directories are skipped, and a directory reachable from several
/// search paths is visited once.
pub(crate) fn walk_dirs(
    search_paths: &[PathBuf],
    max_depth: usize,
    mut visit: impl FnMut(&Path) -> Visit,
) {
    let mut seen = HashSet::new();
    for root in search_paths {
        if !root.is_dir() {
            tracing::debug!(path = %root.display(), "search path is not a directory");
            continue;
        }
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let key = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            if !seen.insert(key) {
                continue;
            }
            if matches!(visit(&dir), Visit::Skip) || depth >= max_depth {
                continue;
            }
            let mut children = subdirectories(&dir);
            // Reverse so the stack pops them in name order.
            children.sort_unstable_by(|a, b| b.cmp(a));
            stack.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }
}

fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(path = %dir.display(), %error, "cannot read directory");
            return Vec::new();
        }
    };
    entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .collect()
}
