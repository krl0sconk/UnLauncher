// SPDX-License-Identifier: GPL-3.0-or-later

//! `.uproject` files and their engine association.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use common::Overwrite;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::engine::{Engine, EngineId, validate_engine_dir};
use crate::error::{Error, Result};
use crate::file_io;
use crate::registry::EngineRegistry;
use crate::walk::{Visit, walk_dirs};

const ASSOCIATION_KEY: &str = "EngineAssociation";
const BOM: char = '\u{feff}';

/// The `EngineAssociation` field of a `.uproject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineAssociation {
    /// Empty or missing: Unreal asks which engine to use.
    None,
    /// An installed release, written as `"5.7"`.
    Version {
        /// Major version.
        major: u32,
        /// Minor version.
        minor: u32,
    },
    /// An `Install.ini` key, usually the GUID of a source build.
    Id(EngineId),
}

impl EngineAssociation {
    /// Interprets the string stored in a `.uproject`.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            return Self::None;
        }
        if let Some((major, minor)) = value.split_once('.')
            && let (Ok(major), Ok(minor)) = (major.parse(), minor.parse())
        {
            return Self::Version { major, minor };
        }
        Self::Id(EngineId::from_existing(value))
    }

    /// The association Unreal would write for `engine`: the version for
    /// `UE_<major>.<minor>` ids matching the engine version, the id otherwise.
    #[must_use]
    pub fn for_engine(engine: &Engine) -> Self {
        let (major, minor) = (engine.version.major, engine.version.minor);
        if engine.id.matches(&format!("UE_{major}.{minor}")) {
            Self::Version { major, minor }
        } else {
            Self::Id(engine.id.clone())
        }
    }
}

impl fmt::Display for EngineAssociation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => Ok(()),
            Self::Version { major, minor } => write!(f, "{major}.{minor}"),
            Self::Id(id) => write!(f, "{id}"),
        }
    }
}

/// An Unreal project, loaded from its `.uproject` file.
///
/// Fields UnLauncher does not use are kept, in their original order, and
/// written back unchanged by [`Project::save`].
#[derive(Debug, Clone)]
pub struct Project {
    /// Path to the `.uproject` file.
    pub path: PathBuf,
    /// Project name (the file name without extension).
    pub name: String,
    /// Engine the project uses.
    pub association: EngineAssociation,
    document: Map<String, Value>,
    has_bom: bool,
    crlf: bool,
    trailing_newline: bool,
}

impl Project {
    /// Loads a `.uproject` file.
    ///
    /// # Errors
    ///
    /// [`Error::ReadFile`] if it cannot be read and [`Error::UProjectMalformed`]
    /// if it is not a JSON object or `EngineAssociation` is not a string.
    pub fn load(path: &Path) -> Result<Self> {
        let text = file_io::read_to_string(path)?;
        let malformed = |reason: String| Error::UProjectMalformed {
            path: path.to_path_buf(),
            reason,
        };
        let body = text.strip_prefix(BOM).unwrap_or(&text);
        let document = match serde_json::from_str(body) {
            Ok(Value::Object(document)) => document,
            Ok(_) => return Err(malformed("the top level is not a JSON object".to_owned())),
            Err(error) => return Err(malformed(error.to_string())),
        };
        let association = match document.get(ASSOCIATION_KEY) {
            None => EngineAssociation::None,
            Some(Value::String(value)) => EngineAssociation::parse(value),
            Some(_) => return Err(malformed(format!("{ASSOCIATION_KEY} is not a string"))),
        };
        Ok(Self {
            path: path.to_path_buf(),
            name: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            association,
            document,
            has_bom: body.len() != text.len(),
            crlf: text.contains("\r\n"),
            trailing_newline: text.ends_with('\n'),
        })
    }

    /// Changes the engine association in memory. Call [`Project::save`] to
    /// write it.
    pub fn set_association(&mut self, association: EngineAssociation) {
        self.document.insert(
            ASSOCIATION_KEY.to_owned(),
            Value::String(association.to_string()),
        );
        self.association = association;
    }

    /// Writes the project back to its file, formatted like Unreal does (tab
    /// indentation), keeping unknown fields, the BOM, line endings and final
    /// newline of the original. Copies the original to `<file>.bak` first and
    /// returns that path.
    ///
    /// # Errors
    ///
    /// [`Error::OverwriteNotConfirmed`] if `overwrite` is [`Overwrite::No`]
    /// (the file always exists), and [`Error::WriteFile`] if writing fails.
    pub fn save(&self, overwrite: Overwrite) -> Result<Option<PathBuf>> {
        let text = self.render()?;
        let backup = file_io::write_with_backup(&self.path, text.as_bytes(), overwrite)?;
        tracing::info!(project = %self.path.display(), association = %self.association, "project saved");
        Ok(backup)
    }

    /// Writes the project like [`Project::save`] but without confirmation or
    /// backup. Only for a `.uproject` UnLauncher has just created itself.
    pub(crate) fn save_new(&self) -> Result<()> {
        let text = self.render()?;
        file_io::write_without_backup(&self.path, text.as_bytes())
    }

    /// The file contents, formatted like Unreal does.
    fn render(&self) -> Result<String> {
        let mut json = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
        let mut serializer = serde_json::Serializer::with_formatter(&mut json, formatter);
        self.document
            .serialize(&mut serializer)
            .map_err(|error| Error::UProjectMalformed {
                path: self.path.clone(),
                reason: error.to_string(),
            })?;
        let mut text = String::from_utf8_lossy(&json).into_owned();
        if self.trailing_newline {
            text.push('\n');
        }
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        if self.has_bom {
            text.insert(0, BOM);
        }
        Ok(text)
    }
}

/// Finds the registered engine a project uses, as Unreal would.
///
/// For a version such as `"5.7"`, tries the id `5.7`, then `UE_5.7`, then the
/// first engine whose version is 5.7. For an id, matches it ignoring ASCII
/// case. Returns `None` if the association is empty or nothing matches.
#[must_use]
pub fn resolve_engine<'a>(project: &Project, registry: &'a EngineRegistry) -> Option<&'a Engine> {
    match &project.association {
        EngineAssociation::None => None,
        EngineAssociation::Id(id) => registry.find(id.as_str()),
        EngineAssociation::Version { major, minor } => registry
            .find(&format!("{major}.{minor}"))
            .or_else(|| registry.find(&format!("UE_{major}.{minor}")))
            .or_else(|| {
                registry
                    .engines()
                    .iter()
                    .find(|e| e.version.major == *major && e.version.minor == *minor)
            }),
    }
}

/// Looks for `.uproject` files in `search_paths` and their subdirectories, up to
/// `max_depth` levels down.
///
/// Does not look inside a project folder (plugins are not projects) or an
/// engine (templates are not user projects). Each `.uproject` found yields its
/// loaded [`Project`] or the error that prevented loading it, sorted by path.
#[must_use]
pub fn discover_projects(search_paths: &[PathBuf], max_depth: usize) -> Vec<Result<Project>> {
    let mut files = Vec::new();
    walk_dirs(search_paths, max_depth, |dir| {
        if validate_engine_dir(dir).is_ok() {
            return Visit::Skip;
        }
        let projects = uproject_files(dir);
        if projects.is_empty() {
            Visit::Descend
        } else {
            files.extend(projects);
            Visit::Skip
        }
    });
    files.sort();
    files.iter().map(|path| Project::load(path)).collect()
}

/// The `.uproject` files directly inside `dir`.
pub(crate) fn uproject_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"))
                && path.is_file()
        })
        .collect()
}
