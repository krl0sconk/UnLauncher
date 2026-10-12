// SPDX-License-Identifier: GPL-3.0-or-later

//! How many compile jobs to run at once.

use std::path::Path;

use crate::error::Result;
use crate::meminfo::MemInfo;

/// RAM budgeted per parallel compile job.
pub const RAM_PER_JOB_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// `min(cores, available_ram / 8 GiB)`, at least 1.
#[must_use]
pub fn parallel_jobs(cores: usize, available_ram_bytes: u64) -> usize {
    let by_ram = usize::try_from(available_ram_bytes / RAM_PER_JOB_BYTES).unwrap_or(usize::MAX);
    cores.min(by_ram).max(1)
}

/// [`parallel_jobs`] for this machine: the cores UnLauncher may use
/// ([`std::thread::available_parallelism`]) and `MemAvailable` from
/// `meminfo_path` (normally [`crate::PROC_MEMINFO`]).
///
/// The user's override, if any, is applied by the caller
/// ([`crate::BuildOptions::jobs`]).
///
/// # Errors
///
/// [`crate::Error::ReadFile`] if `meminfo_path` cannot be read or parsed.
pub fn detect_parallel_jobs(meminfo_path: &Path) -> Result<usize> {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mem = MemInfo::read(meminfo_path)?;
    Ok(parallel_jobs(cores, mem.available_bytes))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn limited_by_ram() {
        assert_eq!(parallel_jobs(32, 64 * GIB), 8);
    }

    #[test]
    fn limited_by_cores() {
        assert_eq!(parallel_jobs(4, 128 * GIB), 4);
    }

    #[test]
    fn never_zero() {
        assert_eq!(parallel_jobs(16, 2 * GIB), 1);
        assert_eq!(parallel_jobs(0, 0), 1);
    }

    #[test]
    fn detects_from_meminfo_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meminfo");
        std::fs::write(&path, "MemTotal: 1000 kB\nMemAvailable: 1000 kB\n").unwrap();
        assert_eq!(detect_parallel_jobs(&path).unwrap(), 1);
    }
}
