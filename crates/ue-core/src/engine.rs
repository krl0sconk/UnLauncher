// SPDX-License-Identifier: GPL-3.0-or-later

//! Engines: ids, validation and discovery.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::version::{EngineVersion, read_build_version};
use crate::walk::{Visit, walk_dirs};

/// Editor binaries, relative to an engine root, that mark a directory as an
/// engine: UE5 first, then UE4 (detected but not installed by UnLauncher).
const EDITOR_BINARIES: [&str; 2] = [
    "Engine/Binaries/Linux/UnrealEditor",
    "Engine/Binaries/Linux/UE4Editor",
];

/// The key of an engine in `Install.ini`: a GUID such as
/// `{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}` or a name such as `UE_5.7`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineId(String);

impl EngineId {
    /// Creates an id, checking that it can be stored as an `Install.ini` key.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidEngineId`] if the id is empty, has surrounding
    /// whitespace, contains `=` or a line break, or starts with `[`, `;` or `#`.
    pub fn new(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        let reason = if id.trim().is_empty() {
            Some("it is empty")
        } else if id.trim() != id {
            Some("it has leading or trailing whitespace")
        } else if id.contains(['=', '\n', '\r']) {
            Some("it contains '=' or a line break")
        } else if id.starts_with(['[', ';', '#']) {
            Some("it starts with '[', ';' or '#'")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(Error::InvalidEngineId { id, reason }),
            None => Ok(Self(id)),
        }
    }

    /// Wraps a key read from an existing file without validating it.
    pub(crate) fn from_existing(id: &str) -> Self {
        Self(id.to_owned())
    }

    /// The id as written in `Install.ini`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compares ids the way Unreal does: ignoring ASCII case.
    pub(crate) fn matches(&self, other: &str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How an engine was installed.
///
/// `Install.ini` does not record it; UnLauncher keeps it in
/// [`EngineMetadata`](crate::EngineMetadata). Serialized with a `kind` tag in
/// `snake_case` (`kind = "source"`, plus `remote` and `git_ref` for
/// [`EngineKind::Source`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineKind {
    /// Prebuilt release imported or downloaded by UnLauncher.
    Prebuilt,
    /// Built from source by UnLauncher.
    Source {
        /// Git remote it was cloned from.
        remote: String,
        /// Branch or tag that was built.
        git_ref: String,
    },
    /// Detected or registered, but without UnLauncher metadata.
    Unknown,
}

/// An Unreal Engine installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    /// Key in `Install.ini`.
    pub id: EngineId,
    /// Version from `Build.version`.
    pub version: EngineVersion,
    /// Engine root (the directory that contains `Engine/`).
    pub path: PathBuf,
    /// How it was installed.
    pub kind: EngineKind,
}

impl Engine {
    /// Loads the engine at `root`, giving it `id`. The kind is
    /// [`EngineKind::Unknown`], since `Install.ini` does not record it; see
    /// [`EngineRegistry::apply_metadata`](crate::EngineRegistry::apply_metadata).
    ///
    /// # Errors
    ///
    /// [`Error::NotAnEngine`] if `root` has no editor binary, and the errors of
    /// [`read_build_version`](crate::read_build_version) if `Build.version`
    /// cannot be read.
    pub fn load(id: EngineId, root: &Path) -> Result<Self> {
        validate_engine_dir(root)?;
        let build = read_build_version(root)?;
        Ok(Self {
            id,
            version: build.version,
            path: root.to_path_buf(),
            kind: EngineKind::Unknown,
        })
    }

    /// Path to this engine's editor executable; see [`editor_executable`].
    ///
    /// # Errors
    ///
    /// [`Error::NotAnEngine`] if the engine no longer has an editor binary.
    pub fn editor_path(&self) -> Result<PathBuf> {
        editor_executable(&self.path)
    }
}

/// Path to the editor executable of the engine at `engine_root`: the first that
/// exists of `Engine/Binaries/Linux/UnrealEditor` and, for UE4,
/// `Engine/Binaries/Linux/UE4Editor`.
///
/// # Errors
///
/// [`Error::NotAnEngine`] if neither exists.
pub fn editor_executable(engine_root: &Path) -> Result<PathBuf> {
    EDITOR_BINARIES
        .iter()
        .map(|bin| engine_root.join(bin))
        .find(|path| path.is_file())
        .ok_or_else(|| Error::NotAnEngine {
            path: engine_root.to_path_buf(),
            missing: engine_root.join(EDITOR_BINARIES[0]),
        })
}

/// Checks that `root` is an engine: it has `Engine/Binaries/Linux/UnrealEditor`
/// (or `UE4Editor` for UE4).
///
/// # Errors
///
/// [`Error::NotAnEngine`] if no editor binary exists.
pub fn validate_engine_dir(root: &Path) -> Result<()> {
    editor_executable(root).map(|_| ())
}

/// An engine found on disk, registered or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredEngine {
    /// Engine root.
    pub path: PathBuf,
    /// Version from `Build.version`.
    pub version: EngineVersion,
}

/// Looks for engines in `search_paths` and their subdirectories, up to
/// `max_depth` levels down. Does not look inside engines it finds.
///
/// Never fails: missing or unreadable directories are skipped, and engine
/// directories whose `Build.version` cannot be read are skipped with a warning.
/// Results are sorted by path.
#[must_use]
pub fn discover_engines(search_paths: &[PathBuf], max_depth: usize) -> Vec<DiscoveredEngine> {
    let mut found = Vec::new();
    walk_dirs(search_paths, max_depth, |dir| {
        if validate_engine_dir(dir).is_err() {
            return Visit::Descend;
        }
        match read_build_version(dir) {
            Ok(build) => found.push(DiscoveredEngine {
                path: dir.to_path_buf(),
                version: build.version,
            }),
            Err(error) => {
                tracing::warn!(path = %dir.display(), %error, "skipping engine without a valid version");
            }
        }
        Visit::Skip
    });
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}
