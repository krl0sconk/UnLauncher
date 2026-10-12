// SPDX-License-Identifier: GPL-3.0-or-later

//! Unreal Engine installations and projects.
//!
//! This crate knows which engines and projects exist and how they relate. It
//! only reads and writes local files: it never touches the network or starts
//! processes. Every function that touches disk takes the path as a parameter,
//! so callers decide where `Install.ini` lives (see `system::paths`).
//!
//! - [`EngineRegistry`]: the engines listed in `Install.ini`.
//! - [`InstallIni`]: lossless editing of `Install.ini` text.
//! - [`EngineMetadata`]: how each engine was installed ([`EngineKind`]), which `Install.ini` cannot store.
//! - [`Engine`], [`validate_engine_dir`], [`editor_executable`], [`discover_engines`]: engine folders.
//! - [`read_build_version`]: the version in `Engine/Build/Build.version`.
//! - [`Project`], [`EngineAssociation`], [`resolve_engine`], [`discover_projects`]: `.uproject` files.
//! - [`project_log_path`], [`read_crash_cause`]: the editor's log and why it crashed.
//! - [`ProjectTemplate`], [`list_templates`], [`create_project`]: new projects from engine templates.
//! - [`project_thumbnail`]: the picture of a project (templates carry theirs in `ProjectTemplate::thumbnail`).

mod editor_log;
mod engine;
mod error;
mod file_io;
mod install_ini;
mod maintenance;
mod metadata;
mod project;
mod registry;
mod template;
mod thumbnail;
mod version;
mod walk;

pub use editor_log::{
    CrashCause, CrashKind, diagnose_crash, engine_log_path, project_log_path, read_crash_cause,
};
pub use engine::{
    DiscoveredEngine, Engine, EngineId, EngineKind, discover_engines, editor_executable,
    validate_engine_dir,
};
pub use error::{Error, Result};
pub use install_ini::InstallIni;
pub use maintenance::{Cache, caches, clean_cache, delete_project, dir_size, uninstall_engine};
pub use metadata::EngineMetadata;
pub use project::{EngineAssociation, Project, discover_projects, resolve_engine};
pub use registry::{EngineRegistry, InvalidEntry};
pub use template::{ProjectTemplate, create_project, list_templates};
pub use thumbnail::project_thumbnail;
pub use version::{BuildVersion, EngineVersion, read_build_version, read_build_version_file};
