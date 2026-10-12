// SPDX-License-Identifier: GPL-3.0-or-later

//! Creating a project by copying a Blueprint template.
//!
//! This follows what the Unreal Editor does for Blueprint templates
//! (`GameProjectUtils::CreateProjectFromTemplate`):
//!
//! - the template folder is copied except for build output (`Binaries`,
//!   `Intermediate`, `Saved`, `DerivedDataCache`), `Config/TemplateDefs.ini`
//!   and whatever its `FoldersToIgnore`/`FilesToIgnore` list (e.g. `Media`);
//! - the shared content packs it lists (`SharedContentPacks`) are copied from
//!   `Templates/TemplateResources` into `Content/<pack folder>`; template
//!   variants are not applied;
//! - the `.uproject` is renamed after the project and associated to the engine;
//! - Config files are updated for the new name (see [`super::config`]).
//!
//! C++ templates also need their modules, targets and source files renamed and
//! are refused for now.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::ProjectTemplate;
use super::config::rewrite_config;
use super::defs::{TEMPLATE_DEFS_PATH, TemplateDefs};
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::file_io;
use crate::project::{EngineAssociation, Project};

/// Top-level template folders that are never copied: build output and caches.
const EXCLUDED_DIRS: [&str; 4] = ["Binaries", "Intermediate", "Saved", "DerivedDataCache"];

/// Longest project name Unreal accepts.
const MAX_PROJECT_NAME_LEN: usize = 20;

/// Where shared content packs live, relative to the engine root.
const CONTENT_PACKS_DIR: &str = "Templates/TemplateResources";

/// Detail level Unreal uses for content packs by default ("Maximum" quality).
const DEFAULT_DETAIL_LEVEL: &str = "High";

const BOM: char = '\u{feff}';

/// Creates the project `name` in `parent_dir/name` from a Blueprint `template`
/// and associates it with `engine`.
///
/// The project is assembled in a hidden temporary folder inside `parent_dir`
/// and moved into place at the end, so a failure never leaves a partial
/// project behind. `parent_dir` is created if missing. The `.uproject` is
/// renamed to `<name>.uproject`, Config files that mention the template name
/// are updated and a new `ProjectID` is written to `DefaultGame.ini`.
///
/// # Errors
///
/// - [`Error::InvalidProjectName`] unless `name` is 1–20 ASCII letters, digits
///   or underscores starting with a letter, as Unreal requires;
/// - [`Error::CodeTemplateUnsupported`] if the template has C++ code;
/// - [`Error::ProjectExists`] if `parent_dir/name` already exists (nothing is
///   ever overwritten);
/// - [`Error::TemplateMalformed`] if a content pack the template needs is
///   missing or invalid;
/// - [`Error::ReadFile`], [`Error::WriteFile`] or [`Error::UProjectMalformed`]
///   if copying or editing the files fails.
pub fn create_project(
    template: &ProjectTemplate,
    engine: &Engine,
    parent_dir: &Path,
    name: &str,
) -> Result<Project> {
    validate_project_name(name)?;
    if template.has_code {
        return Err(Error::CodeTemplateUnsupported {
            template: template.id.clone(),
        });
    }
    let defs = TemplateDefs::load(&template.path)?;

    fs::create_dir_all(parent_dir).map_err(write_err(parent_dir))?;
    let destination = parent_dir.join(name);
    // Claim the destination atomically: creating the folder fails if anything
    // with that name exists. The empty folder is replaced at the end.
    match fs::create_dir(&destination) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(Error::ProjectExists { path: destination });
        }
        Err(source) => {
            return Err(Error::WriteFile {
                path: destination,
                source,
            });
        }
    }

    match assemble(template, &defs, engine, parent_dir, &destination, name) {
        Ok(uproject) => {
            tracing::info!(
                project = %uproject.display(),
                template = %template.id,
                engine_id = %engine.id,
                "project created from template"
            );
            Project::load(&uproject)
        }
        Err(error) => {
            // Only removes the placeholder if it is still empty.
            if let Err(cleanup) = fs::remove_dir(&destination) {
                tracing::warn!(path = %destination.display(), error = %cleanup, "cannot remove placeholder folder");
            }
            Err(error)
        }
    }
}

/// Builds the project in a temporary folder and moves it to `destination`.
/// Returns the path of the final `.uproject`.
fn assemble(
    template: &ProjectTemplate,
    defs: &TemplateDefs,
    engine: &Engine,
    parent_dir: &Path,
    destination: &Path,
    name: &str,
) -> Result<PathBuf> {
    let malformed = |reason: &str| Error::TemplateMalformed {
        path: template.uproject.clone(),
        reason: reason.to_owned(),
    };
    let template_name = template
        .uproject
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| malformed("the .uproject file name is not valid UTF-8"))?;
    let uproject_file = template
        .uproject
        .file_name()
        .and_then(|file| file.to_str())
        .ok_or_else(|| malformed("the .uproject file name is not valid UTF-8"))?;

    // Dropping `staging` before `keep` removes it, so every early return cleans up.
    let staging = tempfile::Builder::new()
        .prefix(&format!(".{name}-"))
        .suffix(".unlauncher-tmp")
        .tempdir_in(parent_dir)
        .map_err(write_err(parent_dir))?;
    let root = staging.path();

    let ignored_dirs = defs.ignored_folders(template_name);
    let mut ignored_files = defs.ignored_files(template_name);
    ignored_files.push(TEMPLATE_DEFS_PATH.to_owned());
    let skip = |relative: &str, is_dir: bool| {
        if is_dir {
            (!relative.contains('/')
                && EXCLUDED_DIRS
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(relative)))
                || ignored_dirs
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(relative))
        } else {
            relative != uproject_file
                && ignored_files
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(relative))
        }
    };
    copy_tree(&template.path, root, "", &skip)?;
    insert_content_packs(&engine.path, defs, root)?;

    let uproject = root.join(format!("{name}.uproject"));
    let copied = root.join(uproject_file);
    if copied != uproject {
        fs::rename(&copied, &uproject).map_err(write_err(&uproject))?;
    }
    rewrite_config(&root.join("Config"), template_name, name)?;

    let mut project = Project::load(&uproject)?;
    project.set_association(EngineAssociation::for_engine(engine));
    project.save_new()?;

    // The temporary folder is private (0700); give the project the
    // permissions a normal new folder gets, like the placeholder.
    let permissions = fs::metadata(destination)
        .map_err(read_err(destination))?
        .permissions();
    fs::set_permissions(root, permissions).map_err(write_err(root))?;

    let staging = staging.keep();
    // Renaming over the empty placeholder folder replaces it atomically.
    if let Err(source) = fs::rename(&staging, destination) {
        if let Err(cleanup) = fs::remove_dir_all(&staging) {
            tracing::warn!(path = %staging.display(), error = %cleanup, "cannot remove temporary project folder");
        }
        return Err(Error::WriteFile {
            path: destination.to_path_buf(),
            source,
        });
    }
    Ok(destination.join(format!("{name}.uproject")))
}

/// Checks `name` against Unreal's rules for project names.
fn validate_project_name(name: &str) -> Result<()> {
    let reason = if name.is_empty() {
        "it is empty"
    } else if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        "only ASCII letters, digits and underscores are allowed"
    } else if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        "it must start with a letter"
    } else if name.len() > MAX_PROJECT_NAME_LEN {
        "it is longer than 20 characters"
    } else {
        return Ok(());
    };
    Err(Error::InvalidProjectName {
        name: name.to_owned(),
        reason,
    })
}

/// Recursively copies `source` into `target`, skipping entries for which
/// `skip(relative_path, is_dir)` is true. Relative paths use `/` and start at
/// `relative`. Copied files are made writable by their owner (engine templates
/// are often read-only); symlinked folders are not followed.
fn copy_tree(
    source: &Path,
    target: &Path,
    relative: &str,
    skip: &dyn Fn(&str, bool) -> bool,
) -> Result<()> {
    fs::create_dir_all(target).map_err(write_err(target))?;
    let entries = fs::read_dir(source).map_err(read_err(source))?;
    for entry in entries {
        let entry = entry.map_err(read_err(source))?;
        let from = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let child = if relative.is_empty() {
            name.into_owned()
        } else {
            format!("{relative}/{name}")
        };
        let metadata = fs::metadata(&from).map_err(read_err(&from))?;
        let to = target.join(&file_name);
        if metadata.is_dir() {
            if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                tracing::warn!(path = %from.display(), "not following symlinked folder");
                continue;
            }
            if !skip(&child, true) {
                copy_tree(&from, &to, &child, skip)?;
            }
        } else if metadata.is_file() && !skip(&child, false) {
            fs::copy(&from, &to).map_err(write_err(&to))?;
            let mut permissions = fs::metadata(&to).map_err(read_err(&to))?.permissions();
            let mode = permissions.mode();
            if mode & 0o200 == 0 {
                permissions.set_mode(mode | 0o200);
                fs::set_permissions(&to, permissions).map_err(write_err(&to))?;
            }
        }
    }
    Ok(())
}

/// `FeaturePack/manifest.json` of a shared content pack.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PackManifest {
    additional_files: Option<AdditionalFiles>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AdditionalFiles {
    destination_files_folder: String,
    #[serde(default)]
    additional_files_list: Vec<String>,
}

/// Copies the shared content packs listed in `defs` from the engine into
/// `<project>/Content/<pack folder>`, as Unreal does when creating a project.
fn insert_content_packs(engine_root: &Path, defs: &TemplateDefs, project: &Path) -> Result<()> {
    for pack in &defs.shared_content_packs {
        let level = pack
            .detail_levels
            .iter()
            .find(|level| level.eq_ignore_ascii_case(DEFAULT_DETAIL_LEVEL))
            .or_else(|| pack.detail_levels.first())
            .map_or(DEFAULT_DETAIL_LEVEL, String::as_str);
        let manifest_path = engine_root
            .join(CONTENT_PACKS_DIR)
            .join(level)
            .join(&pack.mount_name)
            .join("FeaturePack/manifest.json");
        let malformed = |reason: String| Error::TemplateMalformed {
            path: manifest_path.clone(),
            reason,
        };
        if !is_plain_relative(Path::new(&pack.mount_name)) || !is_plain_relative(Path::new(level)) {
            return Err(malformed(format!(
                "invalid content pack name {:?}",
                pack.mount_name
            )));
        }
        let text = file_io::read_to_string(&manifest_path)?;
        let manifest: PackManifest = serde_json::from_str(text.trim_start_matches(BOM))
            .map_err(|error| malformed(error.to_string()))?;
        let files = manifest
            .additional_files
            .ok_or_else(|| malformed("it lists no files to add".to_owned()))?;
        if !is_plain_relative(Path::new(&files.destination_files_folder)) {
            return Err(malformed(format!(
                "invalid destination folder {:?}",
                files.destination_files_folder
            )));
        }
        let target = project
            .join("Content")
            .join(&files.destination_files_folder);
        for pattern in &files.additional_files_list {
            let source = pattern_dir(pattern)
                .map(|dir| engine_root.join(dir))
                .ok_or_else(|| malformed(format!("unsupported file pattern {pattern:?}")))?;
            if !source.is_dir() {
                return Err(malformed(format!("{} does not exist", source.display())));
            }
            copy_tree(&source, &target, "", &|_, _| false)?;
        }
    }
    Ok(())
}

/// The folder of a manifest file pattern such as
/// `Templates/TemplateResources/High/Input/Content/*.*`, relative to the engine.
fn pattern_dir(pattern: &str) -> Option<&Path> {
    let dir = ["/*.*", "/*.", "/*"]
        .iter()
        .find_map(|suffix| pattern.strip_suffix(suffix))?;
    let dir = Path::new(dir);
    is_plain_relative(dir).then_some(dir)
}

/// Whether `path` is non-empty, relative and has no `..` or `.` parts.
fn is_plain_relative(path: &Path) -> bool {
    path.components().next().is_some()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn read_err(path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::ReadFile { path, source }
}

fn write_err(path: &Path) -> impl FnOnce(io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error::WriteFile { path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_unreal_project_names() {
        for name in ["MyGame", "a", "Game_2", "A2345678901234567890"] {
            assert!(validate_project_name(name).is_ok(), "{name}");
        }
        for name in [
            "",
            "2Game",
            "_Game",
            "My Game",
            "Mi-Juego",
            "Juégo",
            "A23456789012345678901",
        ] {
            assert!(validate_project_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn pattern_dirs_stay_inside_the_engine() {
        assert_eq!(
            pattern_dir("Templates/TemplateResources/High/Input/Content/*.*"),
            Some(Path::new("Templates/TemplateResources/High/Input/Content"))
        );
        assert_eq!(pattern_dir("../outside/*.*"), None);
        assert_eq!(pattern_dir("/abs/*.*"), None);
        assert_eq!(pattern_dir("Templates/file.uasset"), None);
    }
}
