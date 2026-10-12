// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine versions from `Engine/Build/Build.version`.

use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::file_io;

/// Location of `Build.version` relative to an engine root.
pub(crate) const BUILD_VERSION_PATH: &str = "Engine/Build/Build.version";

/// An Unreal Engine version (`major.minor.patch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EngineVersion {
    /// Major version (`5` in 5.7.4).
    pub major: u32,
    /// Minor version (`7` in 5.7.4).
    pub minor: u32,
    /// Patch version (`4` in 5.7.4).
    pub patch: u32,
}

impl EngineVersion {
    /// Creates a version from its parts.
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for EngineVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The contents of `Build.version` that UnLauncher uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVersion {
    /// Engine version.
    pub version: EngineVersion,
    /// Source branch, e.g. `++UE5+Release-5.7`.
    pub branch_name: Option<String>,
    /// Perforce changelist the build comes from.
    pub changelist: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawBuildVersion {
    major_version: u32,
    minor_version: u32,
    patch_version: u32,
    branch_name: Option<String>,
    changelist: Option<u64>,
}

/// Reads `Engine/Build/Build.version` of the engine at `engine_root`.
///
/// # Errors
///
/// [`Error::ReadFile`] if the file cannot be read and
/// [`Error::BuildVersionMalformed`] if it is not valid.
pub fn read_build_version(engine_root: &Path) -> Result<BuildVersion> {
    read_build_version_file(&engine_root.join(BUILD_VERSION_PATH))
}

/// Reads a `Build.version` file at an explicit path.
///
/// # Errors
///
/// [`Error::ReadFile`] if the file cannot be read and
/// [`Error::BuildVersionMalformed`] if it is not valid JSON with numeric
/// `MajorVersion`, `MinorVersion` and `PatchVersion`.
pub fn read_build_version_file(path: &Path) -> Result<BuildVersion> {
    let text = file_io::read_to_string(path)?;
    let raw: RawBuildVersion =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|source| {
            Error::BuildVersionMalformed {
                path: path.to_path_buf(),
                source,
            }
        })?;
    Ok(BuildVersion {
        version: EngineVersion::new(raw.major_version, raw.minor_version, raw.patch_version),
        branch_name: raw.branch_name.filter(|b| !b.is_empty()),
        changelist: raw.changelist,
    })
}
