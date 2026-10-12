// SPDX-License-Identifier: GPL-3.0-or-later

//! Updating an existing clone to the latest commit of a ref.

use std::path::{Path, PathBuf};

use common::Secret;
use git2::{Repository, Status, StatusOptions};
use jobs::Reporter;
use serde::{Deserialize, Serialize};

use super::{blocking, remote};
use crate::error::{Error, Result};

/// What [`update`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateOutcome {
    /// Commit checked out before the update (hex).
    pub old_commit: String,
    /// Commit checked out now (hex).
    pub new_commit: String,
}

impl UpdateOutcome {
    /// Whether nothing changed, so no rebuild is needed.
    #[must_use]
    pub fn is_up_to_date(&self) -> bool {
        self.old_commit == self.new_commit
    }
}

/// Fetches `git_ref` (branch or tag) with depth 1 from the repository's
/// `origin` and checks out its latest commit.
///
/// Never destroys work: if any tracked file is modified, staged or deleted it
/// refuses with [`Error::LocalChanges`] before touching anything, and the
/// checkout itself is "safe" (it never overwrites modified files). Untracked
/// and ignored files (build outputs) are left alone.
///
/// # Errors
///
/// - [`Error::NotARepository`] if `repo` is not a git repository.
/// - [`Error::LocalChanges`] if tracked files were changed.
/// - [`Error::RefNotFound`], [`Error::AuthenticationFailed`], [`Error::Git`]
///   as for [`crate::clone_shallow`].
/// - [`Error::Cancelled`] if `reporter` was cancelled.
pub async fn update(
    repo: &Path,
    git_ref: &str,
    token: &Secret,
    reporter: &Reporter,
) -> Result<UpdateOutcome> {
    reporter.checkpoint().await?;
    let (path, git_ref, token, reporter) = (
        repo.to_path_buf(),
        git_ref.to_owned(),
        token.clone(),
        reporter.clone(),
    );
    blocking(move || update_blocking(&path, &git_ref, &token, &reporter)).await
}

fn update_blocking(
    path: &Path,
    git_ref: &str,
    token: &Secret,
    reporter: &Reporter,
) -> Result<UpdateOutcome> {
    reporter.stage("Checking for local changes");
    let repo = Repository::open(path).map_err(|_| Error::NotARepository {
        path: path.to_path_buf(),
    })?;
    let files = local_changes(&repo)?;
    if !files.is_empty() {
        return Err(Error::LocalChanges {
            repo: path.to_path_buf(),
            files,
        });
    }
    let old = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map(|c| c.id())
        .map_err(|e| Error::Git {
            action: "read the current commit".to_owned(),
            message: e.message().to_owned(),
        })?;
    let url = repo
        .find_remote(remote::ORIGIN)
        .ok()
        .and_then(|r| r.url().ok().map(str::to_owned))
        .ok_or_else(|| Error::Git {
            action: "find the origin remote".to_owned(),
            message: "the repository has no origin remote".to_owned(),
        })?;
    reporter.checkpoint_blocking()?;
    reporter.stage("Fetching");
    let target = remote::resolve_ref(&repo, &url, git_ref, token, reporter)?;
    let new = remote::fetch_shallow(&repo, &url, &target, token, reporter)?;
    reporter.checkpoint_blocking()?;
    if new != old {
        reporter.stage("Checking out");
        remote::checkout(&repo, &target, new, false, reporter)?;
    }
    tracing::info!(repo = %path.display(), %git_ref, %old, %new, "engine source updated");
    Ok(UpdateOutcome {
        old_commit: old.to_string(),
        new_commit: new.to_string(),
    })
}

/// Tracked files that differ from HEAD (in the index or the worktree).
fn local_changes(repo: &Repository) -> Result<Vec<PathBuf>> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(false)
        .include_ignored(false)
        .exclude_submodules(true);
    let statuses = repo.statuses(Some(&mut options)).map_err(|e| Error::Git {
        action: "read the working tree status".to_owned(),
        message: e.message().to_owned(),
    })?;
    Ok(statuses
        .iter()
        .filter(|entry| {
            !matches!(
                entry.status(),
                Status::CURRENT | Status::IGNORED | Status::WT_NEW
            )
        })
        .map(|entry| PathBuf::from(String::from_utf8_lossy(entry.path_bytes()).into_owned()))
        .collect())
}
