// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading an archive's index before extracting anything: safe paths, layout
//! and total size.

use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};

use zip::ZipArchive;

use crate::error::{Error, Result};

/// Top-level folders of an engine release; if one of them is at the root of
/// the archive, there is no wrapping folder to strip.
const ENGINE_TOP_LEVEL: [&str; 3] = ["Engine", "Templates", "FeaturePacks"];

/// What an archive entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Dir,
    File,
    Symlink,
}

/// One entry to extract.
#[derive(Debug, Clone)]
pub(crate) struct PlannedEntry {
    /// Index in the archive.
    pub index: usize,
    /// Path relative to the engine root, after stripping any wrapping folder.
    pub relative: PathBuf,
    pub kind: EntryKind,
    /// Unix mode bits stored in the archive, if any.
    pub mode: Option<u32>,
    /// Uncompressed size.
    pub size: u64,
}

/// Everything to extract, in archive order.
#[derive(Debug)]
pub(crate) struct Plan {
    pub entries: Vec<PlannedEntry>,
    /// Sum of the uncompressed sizes of files and symlinks.
    pub total_bytes: u64,
}

/// Reads the archive index. Rejects the whole archive if any entry has an
/// unsafe path, and strips a single folder that wraps everything (Epic's zips
/// sometimes put the engine inside `Linux_Unreal_Engine_<version>/`).
pub(crate) fn plan<R: Read + Seek>(archive: &mut ZipArchive<R>, zip_path: &Path) -> Result<Plan> {
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|source| Error::InvalidArchive {
                path: zip_path.to_path_buf(),
                source,
            })?;
        let relative = entry.enclosed_name().ok_or_else(|| Error::UnsafeEntry {
            archive: zip_path.to_path_buf(),
            entry: String::from_utf8_lossy(entry.name_raw()).into_owned(),
        })?;
        let kind = if entry.is_symlink() {
            EntryKind::Symlink
        } else if entry.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        entries.push(PlannedEntry {
            index,
            relative,
            kind,
            mode: entry.unix_mode(),
            size: entry.size(),
        });
    }

    if let Some(wrapper) = wrapping_folder(&entries) {
        tracing::debug!(wrapper = %wrapper.display(), "stripping wrapping folder");
        for entry in &mut entries {
            if let Ok(rest) = entry.relative.strip_prefix(&wrapper) {
                entry.relative = rest.to_path_buf();
            }
        }
    }
    entries.retain(|e| e.relative.components().next().is_some());

    let total_bytes = entries
        .iter()
        .filter(|e| e.kind != EntryKind::Dir)
        .map(|e| e.size)
        .fold(0u64, u64::saturating_add);
    Ok(Plan {
        entries,
        total_bytes,
    })
}

/// The single top-level folder that contains every entry, unless it is one
/// of the engine's own top-level folders.
fn wrapping_folder(entries: &[PlannedEntry]) -> Option<PathBuf> {
    let mut first: Option<&OsStr> = None;
    let mut has_nested = false;
    for entry in entries {
        let mut components = entry.relative.components();
        let Some(Component::Normal(head)) = components.next() else {
            continue;
        };
        if components.next().is_some() {
            has_nested = true;
        } else if entry.kind != EntryKind::Dir {
            // A file at the root: nothing wraps the archive.
            return None;
        }
        match first {
            None => first = Some(head),
            Some(seen) if seen != head => return None,
            Some(_) => {}
        }
    }
    let first = first?;
    if !has_nested
        || ENGINE_TOP_LEVEL
            .iter()
            .any(|name| OsStr::new(name) == first)
    {
        return None;
    }
    Some(PathBuf::from(first))
}
