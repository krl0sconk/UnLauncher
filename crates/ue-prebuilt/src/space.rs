// SPDX-License-Identifier: GPL-3.0-or-later

//! Free disk space checks before writing tens of GB.

use std::path::Path;

use crate::error::{Error, Result};

/// Fails if the file system of `folder` has less than `needed` bytes free. If
/// the free space cannot be read, logs a warning and lets the caller go on.
pub(crate) fn check_free_space(folder: &Path, needed: u64) -> Result<()> {
    let stats = match rustix::fs::statvfs(folder) {
        Ok(stats) => stats,
        Err(error) => {
            tracing::warn!(path = %folder.display(), %error, "cannot read free space; not checking it");
            return Ok(());
        }
    };
    let available = stats.f_bavail.saturating_mul(stats.f_frsize);
    if available < needed {
        return Err(Error::NotEnoughSpace {
            path: folder.to_path_buf(),
            needed,
            available,
        });
    }
    Ok(())
}
