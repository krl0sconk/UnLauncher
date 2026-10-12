// SPDX-License-Identifier: GPL-3.0-or-later

//! Streaming extraction of a planned archive into a folder.

use std::fs::{self, File, Permissions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use jobs::Reporter;
use zip::ZipArchive;

use super::plan::{EntryKind, Plan};
use crate::error::{Error, Result};

/// Bytes read from the archive per step; pause and cancel are checked after
/// each one, so a multi-gigabyte file stays responsive.
const CHUNK: usize = 1 << 20;

/// Minimum time between progress updates.
const THROTTLE: Duration = Duration::from_millis(100);

/// Longest symlink target accepted.
const MAX_LINK_TARGET: u64 = 4096;

/// Permission bits restored from the archive (no setuid, setgid or sticky).
const PERMISSION_BITS: u32 = 0o777;

/// Extracts every planned entry under `root`.
///
/// Regular files and folders come first; symlinks are created at the end, so
/// no file is ever written through a link from the archive. Folder modes are
/// applied last and always keep the owner's `rwx`, so the tree can be removed.
pub(crate) fn extract<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    zip_path: &Path,
    plan: &Plan,
    root: &Path,
    reporter: &Reporter,
) -> Result<()> {
    let mut meter = Meter::new(reporter, plan.total_bytes);
    let mut folder_modes = Vec::new();
    let mut links = Vec::new();
    let mut buffer = vec![0; CHUNK];

    for planned in &plan.entries {
        checkpoint(reporter)?;
        let target = root.join(&planned.relative);
        let read_err = |source| Error::ReadEntry {
            archive: zip_path.to_path_buf(),
            entry: planned.relative.display().to_string(),
            source,
        };
        match planned.kind {
            EntryKind::Dir => {
                fs::create_dir_all(&target).map_err(write_err(&target))?;
                if let Some(mode) = planned.mode {
                    folder_modes.push((target, mode));
                }
            }
            EntryKind::Symlink => {
                let mut link_target = String::new();
                archive
                    .by_index(planned.index)
                    .map_err(read_err)?
                    .take(MAX_LINK_TARGET)
                    .read_to_string(&mut link_target)
                    .map_err(|e| read_err(e.into()))?;
                links.push((planned.relative.clone(), PathBuf::from(link_target)));
                meter.add(planned.size, &planned.relative);
            }
            EntryKind::File => {
                meter.file(&planned.relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(write_err(parent))?;
                }
                let mut out = File::create(&target).map_err(write_err(&target))?;
                // Never write more than the index declared: the space check relies on it.
                let mut entry = archive
                    .by_index(planned.index)
                    .map_err(read_err)?
                    .take(planned.size);
                loop {
                    let read = entry.read(&mut buffer).map_err(|e| read_err(e.into()))?;
                    if read == 0 {
                        break;
                    }
                    out.write_all(&buffer[..read]).map_err(write_err(&target))?;
                    meter.add(read as u64, &planned.relative);
                    checkpoint(reporter)?;
                }
                if let Some(mode) = planned.mode {
                    out.set_permissions(Permissions::from_mode(mode & PERMISSION_BITS))
                        .map_err(write_err(&target))?;
                }
            }
        }
    }

    for (link, link_target) in links {
        checkpoint(reporter)?;
        create_link(root, &link, &link_target)?;
    }
    for (folder, mode) in folder_modes.into_iter().rev() {
        fs::set_permissions(
            &folder,
            Permissions::from_mode((mode & PERMISSION_BITS) | 0o700),
        )
        .map_err(write_err(&folder))?;
    }
    meter.finish();
    Ok(())
}

/// Creates the symlink `root/link -> link_target` if it cannot point outside
/// `root`; otherwise skips it with a warning.
fn create_link(root: &Path, link: &Path, link_target: &Path) -> Result<()> {
    if !link_stays_inside(link, link_target) {
        tracing::warn!(link = %link.display(), target = %link_target.display(), "skipping symlink that points outside the engine");
        return Ok(());
    }
    // The checks above assume the link's folders are real folders.
    let mut ancestor = root.to_path_buf();
    for component in link.parent().into_iter().flat_map(Path::components) {
        ancestor.push(component);
        let is_link = fs::symlink_metadata(&ancestor).is_ok_and(|m| m.file_type().is_symlink());
        if is_link {
            tracing::warn!(link = %link.display(), "skipping symlink inside a symlinked folder");
            return Ok(());
        }
    }
    let path = root.join(link);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(write_err(parent))?;
    }
    match std::os::unix::fs::symlink(link_target, &path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            tracing::warn!(link = %link.display(), "skipping symlink over an existing entry");
            Ok(())
        }
        Err(source) => Err(Error::WriteFile { path, source }),
    }
}

/// Whether `link_target`, relative to the folder of `link`, stays inside the
/// root. Only leading `..` are allowed, so the result does not depend on other
/// symlinks the target passes through.
pub(crate) fn link_stays_inside(link: &Path, link_target: &Path) -> bool {
    let mut depth = link
        .parent()
        .map_or(0, |parent| parent.components().count());
    let mut seen_name = false;
    for component in link_target.components() {
        match component {
            Component::Normal(_) => seen_name = true,
            Component::CurDir => {}
            Component::ParentDir if !seen_name => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    !link_target.as_os_str().is_empty()
}

fn checkpoint(reporter: &Reporter) -> Result<()> {
    reporter.checkpoint_blocking().map_err(|_| Error::Cancelled)
}

fn write_err(path: &Path) -> impl FnOnce(io::Error) -> Error + use<> {
    let path = path.to_path_buf();
    move |source| Error::WriteFile { path, source }
}

/// Throttled progress: fraction of bytes written and the current file.
struct Meter<'a> {
    reporter: &'a Reporter,
    total: u64,
    done: u64,
    last: Option<Instant>,
}

impl<'a> Meter<'a> {
    fn new(reporter: &'a Reporter, total: u64) -> Self {
        reporter.fraction(Some(0.0));
        Self {
            reporter,
            total,
            done: 0,
            last: None,
        }
    }

    fn file(&mut self, current: &Path) {
        self.add(0, current);
    }

    fn add(&mut self, bytes: u64, current: &Path) {
        self.done = self.done.saturating_add(bytes);
        if self.last.is_some_and(|last| last.elapsed() < THROTTLE) {
            return;
        }
        self.last = Some(Instant::now());
        self.reporter.fraction(Some(self.fraction()));
        self.reporter.message(current.display().to_string());
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // A progress bar.
    fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        (self.done as f64 / self.total as f64) as f32
    }

    fn finish(&self) {
        self.reporter.fraction(Some(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_targets_inside_the_root_are_accepted() {
        assert!(link_stays_inside(
            Path::new("Engine/Binaries/Linux/libfoo.so"),
            Path::new("libfoo.so.1")
        ));
        assert!(link_stays_inside(
            Path::new("Engine/a/link"),
            Path::new("../../Templates/x")
        ));
        assert!(link_stays_inside(Path::new("a/link"), Path::new("./b")));
    }

    #[test]
    fn link_targets_outside_the_root_are_rejected() {
        assert!(!link_stays_inside(Path::new("link"), Path::new("../x")));
        assert!(!link_stays_inside(
            Path::new("a/link"),
            Path::new("../../x")
        ));
        assert!(!link_stays_inside(
            Path::new("a/link"),
            Path::new("/etc/passwd")
        ));
        assert!(!link_stays_inside(
            Path::new("a/link"),
            Path::new("b/../..")
        ));
        assert!(!link_stays_inside(Path::new("a/link"), Path::new("")));
    }
}
