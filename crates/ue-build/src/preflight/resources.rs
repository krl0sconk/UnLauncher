// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk and RAM checks.

use std::path::{Path, PathBuf};

use super::{CheckResult, CheckStatus};
use crate::meminfo::MemInfo;

const GB: u64 = 1_000_000_000;

// Thresholds are decimal GB. They are estimates: calibrate them with a real
// build.

/// Warn when the destination has less free space than this (300 GB).
pub const DISK_WARN_BYTES: u64 = 300 * GB;
/// Fail when the destination has less free space than this (150 GB).
pub const DISK_FAIL_BYTES: u64 = 150 * GB;
/// Warn when the machine has less RAM than this (32 GB, Epic's Linux
/// recommendation). Decimal GB so a "32 GB" machine, whose `MemTotal` is a bit
/// under 32 GiB, still passes.
pub const RAM_WARN_BYTES: u64 = 32 * GB;

const DISK: &str = "Disk space";
const RAM: &str = "Memory";

/// Status for `free_bytes` of free disk at the destination.
#[must_use]
pub fn disk_status(free_bytes: u64) -> CheckStatus {
    if free_bytes < DISK_FAIL_BYTES {
        CheckStatus::Fail
    } else if free_bytes < DISK_WARN_BYTES {
        CheckStatus::Warn
    } else {
        CheckStatus::Ok
    }
}

/// Status for `total_bytes` of physical RAM.
#[must_use]
pub fn ram_status(total_bytes: u64) -> CheckStatus {
    if total_bytes < RAM_WARN_BYTES {
        CheckStatus::Warn
    } else {
        CheckStatus::Ok
    }
}

/// The closest ancestor of `path` (or `path` itself) that exists.
#[must_use]
pub fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| !p.as_os_str().is_empty() && p.exists())
        .map(Path::to_path_buf)
}

#[allow(clippy::cast_precision_loss)]
fn gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / GB as f64)
}

fn free_bytes(path: &Path) -> std::io::Result<u64> {
    let stat = rustix::fs::statvfs(path)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

pub(super) fn disk_result(destination: &Path, free: u64) -> CheckResult {
    let status = disk_status(free);
    let message = format!("{} free at {}", gb(free), destination.display());
    let hint = match status {
        CheckStatus::Ok => None,
        CheckStatus::Warn => Some(format!(
            "A full editor build can use around {}. Free up space or choose another disk.",
            gb(DISK_WARN_BYTES)
        )),
        CheckStatus::Fail => Some(format!(
            "At least {} free is needed. Free up space or choose another disk.",
            gb(DISK_FAIL_BYTES)
        )),
    };
    CheckResult::new(DISK, status, message, hint)
}

pub(super) async fn check_disk(destination: &Path) -> CheckResult {
    let Some(existing) = nearest_existing_ancestor(destination) else {
        return CheckResult::new(
            DISK,
            CheckStatus::Fail,
            format!("{} is not on any existing folder", destination.display()),
            Some("Choose an absolute destination folder.".to_owned()),
        );
    };
    let probe = existing.clone();
    match tokio::task::spawn_blocking(move || free_bytes(&probe)).await {
        Ok(Ok(free)) => disk_result(destination, free),
        Ok(Err(error)) => CheckResult::new(
            DISK,
            CheckStatus::Warn,
            format!(
                "could not measure free space at {}: {error}",
                existing.display()
            ),
            Some("Check that the destination folder is accessible.".to_owned()),
        ),
        Err(error) => CheckResult::new(
            DISK,
            CheckStatus::Warn,
            format!("could not measure free space: {error}"),
            None,
        ),
    }
}

pub(super) fn ram_result(info: MemInfo) -> CheckResult {
    let status = ram_status(info.total_bytes);
    let message = format!("{} of RAM", gb(info.total_bytes));
    let hint = (status != CheckStatus::Ok).then(|| {
        format!(
            "Epic recommends {} for building on Linux. The build will run fewer jobs in \
             parallel and take longer; adding swap helps avoid out-of-memory failures.",
            gb(RAM_WARN_BYTES)
        )
    });
    CheckResult::new(RAM, status, message, hint)
}

pub(super) async fn check_ram(meminfo_path: &Path) -> CheckResult {
    let parsed = tokio::fs::read_to_string(meminfo_path)
        .await
        .ok()
        .and_then(|text| MemInfo::parse(&text));
    match parsed {
        Some(info) => ram_result(info),
        None => CheckResult::new(
            RAM,
            CheckStatus::Warn,
            format!("could not read {}", meminfo_path.display()),
            None,
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn disk_thresholds() {
        assert_eq!(disk_status(149 * GB), CheckStatus::Fail);
        assert_eq!(disk_status(150 * GB), CheckStatus::Warn);
        assert_eq!(disk_status(299 * GB), CheckStatus::Warn);
        assert_eq!(disk_status(300 * GB), CheckStatus::Ok);
    }

    #[test]
    fn ram_threshold_accepts_a_32_gib_machine() {
        // A 32 GiB machine typically reports ~31.3 GiB of MemTotal.
        assert_eq!(ram_status(32_800_000_000), CheckStatus::Ok);
        assert_eq!(ram_status(16 * 1024 * 1024 * 1024), CheckStatus::Warn);
    }

    #[test]
    fn finds_nearest_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a/b/c");
        assert_eq!(nearest_existing_ancestor(&target).unwrap(), dir.path());
    }

    #[test]
    fn disk_result_has_hint_when_low() {
        let r = disk_result(Path::new("/x"), 100 * GB);
        assert_eq!(r.status, CheckStatus::Fail);
        assert!(r.hint.is_some());
    }

    #[tokio::test]
    async fn checks_disk_of_missing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let r = check_disk(&dir.path().join("not/yet/created")).await;
        assert!(r.message.contains("free at"), "{r:?}");
    }

    #[tokio::test]
    async fn ram_from_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meminfo");
        std::fs::write(&path, "MemTotal: 16000000 kB\nMemAvailable: 8000000 kB\n").unwrap();
        assert_eq!(check_ram(&path).await.status, CheckStatus::Warn);
        assert_eq!(
            check_ram(&dir.path().join("nope")).await.status,
            CheckStatus::Warn
        );
    }
}
