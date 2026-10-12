// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end flows: clone and build a new engine, or update and rebuild one,
//! and register the result.

use std::path::{Path, PathBuf};

use common::Secret;
use jobs::Reporter;
use system::Sandbox;
use ue_core::{Engine, EngineId, EngineKind, EngineMetadata, EngineRegistry};

use crate::error::{Error, Result};
use crate::pipeline::{BuildOptions, BuildReport, run_build};
use crate::source::{UpdateOutcome, clone_shallow, update};

/// What to build from source.
#[derive(Debug, Clone)]
pub struct SourceInstall {
    /// Git remote, normally [`UE_REMOTE`](crate::UE_REMOTE).
    pub remote: String,
    /// Branch or tag, e.g. `5.7` or `5.7.4-release`.
    pub git_ref: String,
    /// Folder for the source tree, which becomes the engine root.
    pub destination: PathBuf,
    /// Parallel compile jobs; `None` derives them from cores and RAM.
    pub jobs: Option<usize>,
    /// Where the full build log is written.
    pub log_path: PathBuf,
}

/// Clones the source (unless a previous attempt already did) and builds it.
///
/// If `destination` already holds a git checkout, cloning is skipped, so an
/// interrupted install can be retried without downloading again. Register the
/// result with [`register_built`].
///
/// # Errors
///
/// The errors of [`clone_shallow`] and [`run_build`].
pub async fn install_from_source(
    request: &SourceInstall,
    token: &Secret,
    sandbox: Sandbox,
    reporter: &Reporter,
) -> Result<BuildReport> {
    if is_checkout(&request.destination) {
        tracing::info!(path = %request.destination.display(), "source already cloned, building it");
    } else {
        clone_shallow(
            &request.remote,
            &request.git_ref,
            &request.destination,
            token,
            reporter,
        )
        .await?;
    }
    let mut options = BuildOptions::new(&request.destination, &request.log_path);
    options.jobs = request.jobs;
    run_build(&options, sandbox, reporter).await
}

/// Fetches `git_ref` into an engine built from source and rebuilds what
/// changed. Refuses if the checkout has local changes.
///
/// # Errors
///
/// The errors of [`update`] and [`run_build`].
pub async fn update_from_source(
    engine_root: &Path,
    git_ref: &str,
    token: &Secret,
    options: &BuildOptions,
    sandbox: Sandbox,
    reporter: &Reporter,
) -> Result<(UpdateOutcome, BuildReport)> {
    let outcome = update(engine_root, git_ref, token, reporter).await?;
    let report = run_build(options, sandbox, reporter).await?;
    Ok((outcome, report))
}

/// Registers a built engine (in memory) as [`EngineKind::Source`]. The caller
/// saves the registry (after the user's confirmation) and the metadata.
///
/// # Errors
///
/// The errors of [`EngineRegistry::register`]. If the engine is already
/// registered (an update), only the metadata is refreshed.
pub fn register_built(
    registry: &mut EngineRegistry,
    metadata: &mut EngineMetadata,
    engine_root: &Path,
    remote: &str,
    git_ref: &str,
    id: Option<EngineId>,
) -> Result<Engine> {
    let kind = EngineKind::Source {
        remote: remote.to_owned(),
        git_ref: git_ref.to_owned(),
    };
    metadata.set_kind(engine_root, kind.clone());
    let registered = registry
        .engines()
        .iter()
        .find(|e| same_dir(&e.path, engine_root))
        .cloned();
    let mut engine = match registered {
        Some(engine) => engine,
        None => registry
            .register(engine_root, id)
            .map_err(|source| Error::Register(Box::new(source)))?
            .clone(),
    };
    engine.kind = kind;
    Ok(engine)
}

fn is_checkout(path: &Path) -> bool {
    git2::Repository::open(path).is_ok_and(|repo| repo.head().is_ok())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
