// SPDX-License-Identifier: GPL-3.0-or-later

//! Getting the engine source with git: shallow clone and update.
//!
//! git2 is blocking, so the public functions run it on a blocking thread
//! (`spawn_blocking`). Cancellation is checked in git's transfer callback;
//! pausing is only honoured between steps (a paused transfer would time out
//! on the server).

mod remote;
mod update;

use std::path::{Path, PathBuf};

use common::Secret;
use git2::Repository;
use jobs::Reporter;

pub use update::{UpdateOutcome, update};

use crate::error::{Error, Result};

/// The engine repository on GitHub.
pub const UE_REMOTE: &str = "https://github.com/EpicGames/UnrealEngine.git";

/// Runs blocking git work on tokio's blocking pool.
pub(crate) async fn blocking<T, F>(work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::Internal {
            message: format!("git worker failed: {e}"),
        })?
}

/// Clones `git_ref` (a branch or a tag) of `remote` into `dest` with depth 1
/// (full history for local `file://` remotes, which libgit2 cannot fetch
/// shallow).
///
/// Only that ref is fetched, without other tags. `token` is sent as the HTTPS
/// password (username `x-access-token`, then `git`); it is never written to
/// the repository config. Progress goes to `reporter` as stage "Cloning"
/// then "Checking out".
///
/// `dest` must not exist or be an empty folder. On failure or cancellation
/// everything this function created in `dest` is removed.
///
/// # Errors
///
/// - [`Error::DestinationNotEmpty`] if `dest` has files.
/// - [`Error::RefNotFound`] if the remote has no such branch or tag.
/// - [`Error::AuthenticationFailed`] if the token is rejected.
/// - [`Error::Cancelled`] if `reporter` was cancelled.
/// - [`Error::Git`], [`Error::WriteFile`] for other failures.
pub async fn clone_shallow(
    remote: &str,
    git_ref: &str,
    dest: &Path,
    token: &Secret,
    reporter: &Reporter,
) -> Result<()> {
    reporter.checkpoint().await?;
    let (remote, git_ref, dest, token, reporter) = (
        remote.to_owned(),
        git_ref.to_owned(),
        dest.to_path_buf(),
        token.clone(),
        reporter.clone(),
    );
    blocking(move || clone_blocking(&remote, &git_ref, &dest, &token, &reporter)).await
}

fn clone_blocking(
    url: &str,
    git_ref: &str,
    dest: &Path,
    token: &Secret,
    reporter: &Reporter,
) -> Result<()> {
    let created = prepare_destination(dest)?;
    let result = clone_into(url, git_ref, dest, token, reporter);
    if result.is_err() {
        cleanup(dest, created);
    }
    result
}

fn clone_into(
    url: &str,
    git_ref: &str,
    dest: &Path,
    token: &Secret,
    reporter: &Reporter,
) -> Result<()> {
    reporter.stage("Cloning");
    let repo = Repository::init(dest).map_err(|e| Error::Git {
        action: format!("create a repository in {}", dest.display()),
        message: e.message().to_owned(),
    })?;
    repo.remote(remote::ORIGIN, url).map_err(|e| Error::Git {
        action: format!("add remote {url}"),
        message: e.message().to_owned(),
    })?;
    let target = remote::resolve_ref(&repo, url, git_ref, token, reporter)?;
    reporter.checkpoint_blocking()?;
    let commit = remote::fetch_shallow(&repo, url, &target, token, reporter)?;
    reporter.checkpoint_blocking()?;
    reporter.stage("Checking out");
    remote::checkout(&repo, &target, commit, true, reporter)?;
    tracing::info!(%url, %git_ref, %commit, dest = %dest.display(), "engine source cloned");
    Ok(())
}

/// Checks `dest` and creates it if needed. Returns whether it was created.
fn prepare_destination(dest: &Path) -> Result<bool> {
    match std::fs::read_dir(dest) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                Err(Error::DestinationNotEmpty {
                    path: dest.to_path_buf(),
                })
            } else {
                Ok(false)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dest).map_err(|source| Error::WriteFile {
                path: dest.to_path_buf(),
                source,
            })?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => {
            Err(Error::DestinationNotEmpty {
                path: dest.to_path_buf(),
            })
        }
        Err(source) => Err(Error::ReadFile {
            path: dest.to_path_buf(),
            source,
        }),
    }
}

/// Removes what a failed clone wrote. `dest` was empty or missing before, so
/// everything in it is ours.
fn cleanup(dest: &Path, created: bool) {
    let result = if created {
        std::fs::remove_dir_all(dest)
    } else {
        std::fs::read_dir(dest).and_then(|entries| {
            entries.flatten().try_for_each(|entry| {
                let path: PathBuf = entry.path();
                if entry.file_type()?.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                }
            })
        })
    };
    if let Err(error) = result {
        tracing::warn!(%error, dest = %dest.display(), "failed to clean up after a failed clone");
    }
}
