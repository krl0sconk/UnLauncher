// SPDX-License-Identifier: GPL-3.0-or-later

//! Memory figures from `/proc/meminfo`.

use std::path::Path;

use crate::error::{Error, Result};

/// The default location of the kernel's memory report.
pub const PROC_MEMINFO: &str = "/proc/meminfo";

/// Memory figures from `/proc/meminfo`, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    /// Physical RAM (`MemTotal`).
    pub total_bytes: u64,
    /// RAM available to new processes without swapping (`MemAvailable`).
    pub available_bytes: u64,
}

impl MemInfo {
    /// Parses the text of `/proc/meminfo`. Returns `None` if `MemTotal` or
    /// `MemAvailable` is missing or malformed.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let field = |name: &str| -> Option<u64> {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix(name)?.strip_prefix(':')?;
                let mut parts = rest.split_whitespace();
                let value: u64 = parts.next()?.parse().ok()?;
                match parts.next() {
                    Some("kB") => value.checked_mul(1024),
                    None => Some(value),
                    Some(_) => None,
                }
            })
        };
        Some(Self {
            total_bytes: field("MemTotal")?,
            available_bytes: field("MemAvailable")?,
        })
    }

    /// Reads and parses `path` (normally [`PROC_MEMINFO`]).
    ///
    /// # Errors
    ///
    /// [`Error::ReadFile`] if the file cannot be read or has no `MemTotal` /
    /// `MemAvailable` line.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text).ok_or_else(|| Error::ReadFile {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "MemTotal or MemAvailable not found",
            ),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FIXTURE: &str = "MemTotal:       65756016 kB\n\
                           MemFree:         1234567 kB\n\
                           MemAvailable:   40000000 kB\n\
                           Buffers:          123456 kB\n";

    #[test]
    fn parses_total_and_available() {
        let info = MemInfo::parse(FIXTURE).unwrap();
        assert_eq!(info.total_bytes, 65_756_016 * 1024);
        assert_eq!(info.available_bytes, 40_000_000 * 1024);
    }

    #[test]
    fn missing_field_is_none() {
        assert_eq!(MemInfo::parse("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn reads_from_injected_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meminfo");
        std::fs::write(&path, FIXTURE).unwrap();
        assert_eq!(
            MemInfo::read(&path).unwrap().available_bytes,
            40_000_000 * 1024
        );
        assert!(MemInfo::read(&dir.path().join("missing")).is_err());
    }
}
