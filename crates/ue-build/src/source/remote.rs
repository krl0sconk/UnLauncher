// SPDX-License-Identifier: GPL-3.0-or-later

//! Talking to the remote: credentials, progress, ref lookup, shallow fetch and
//! checkout. Everything here is blocking (git2).

use git2::build::CheckoutBuilder;
use git2::{
    AutotagOption, Cred, CredentialType, Direction, FetchOptions, Oid, RemoteCallbacks, Repository,
};
use jobs::Reporter;

use common::Secret;

use crate::error::{Error, Result};

/// Name of the remote UnLauncher creates.
pub(crate) const ORIGIN: &str = "origin";

/// Usernames GitHub accepts with a token as password. `x-access-token` is the
/// documented one; `git` is a common fallback for other hosts.
const TOKEN_USERNAMES: [&str; 2] = ["x-access-token", "git"];

/// A branch or tag on the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoteRef {
    Branch(String),
    Tag(String),
}

impl RemoteRef {
    /// The refspec that fetches only this ref.
    fn refspec(&self) -> String {
        match self {
            Self::Branch(name) => format!("+refs/heads/{name}:refs/remotes/{ORIGIN}/{name}"),
            Self::Tag(name) => format!("+refs/tags/{name}:refs/tags/{name}"),
        }
    }

    /// The local ref the fetch writes.
    fn local_ref(&self) -> String {
        match self {
            Self::Branch(name) => format!("refs/remotes/{ORIGIN}/{name}"),
            Self::Tag(name) => format!("refs/tags/{name}"),
        }
    }
}

/// Callbacks with token credentials and progress/cancel through `reporter`.
pub(crate) fn callbacks<'a>(token: &'a Secret, reporter: &'a Reporter) -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    let mut attempt = 0usize;
    callbacks.credentials(move |_url, _username, allowed| {
        if !allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
            return Err(git2::Error::from_str(
                "only token authentication is supported",
            ));
        }
        // libgit2 calls this again after a rejection; give up after trying
        // each username once.
        let username = TOKEN_USERNAMES.get(attempt).ok_or_else(|| {
            git2::Error::new(
                git2::ErrorCode::Auth,
                git2::ErrorClass::Http,
                "the token was rejected",
            )
        })?;
        attempt += 1;
        Cred::userpass_plaintext(username, token.expose())
    });
    callbacks.transfer_progress(move |stats| {
        let total = stats.total_objects();
        if total > 0 {
            #[allow(clippy::cast_precision_loss)]
            let fraction = stats.received_objects() as f32 / total as f32;
            reporter.fraction(Some(fraction));
            #[allow(clippy::cast_precision_loss)]
            let mib = stats.received_bytes() as f64 / (1024.0 * 1024.0);
            reporter.message(format!(
                "Receiving objects: {}/{} ({mib:.1} MiB)",
                stats.received_objects(),
                total
            ));
        }
        !reporter.is_cancelled()
    });
    callbacks
}

/// Maps a git2 error, recognising cancellation and rejected credentials.
pub(crate) fn git_error(
    error: &git2::Error,
    action: &str,
    remote: &str,
    reporter: &Reporter,
) -> Error {
    if reporter.is_cancelled() {
        return Error::Cancelled;
    }
    if error.code() == git2::ErrorCode::Auth {
        return Error::AuthenticationFailed {
            remote: remote.to_owned(),
        };
    }
    Error::Git {
        action: action.to_owned(),
        message: error.message().to_owned(),
    }
}

/// Whether `url` uses libgit2's local transport (a path or `file://`).
fn is_local(url: &str) -> bool {
    url.starts_with("file://") || url.starts_with('/') || url.starts_with('.')
}

/// Finds whether `git_ref` is a branch or a tag on the remote (branches win,
/// like `git clone --branch`).
pub(crate) fn resolve_ref(
    repo: &Repository,
    url: &str,
    git_ref: &str,
    token: &Secret,
    reporter: &Reporter,
) -> Result<RemoteRef> {
    let action = format!("connect to {url}");
    let mut remote = repo
        .find_remote(ORIGIN)
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    let connection = remote
        .connect_auth(Direction::Fetch, Some(callbacks(token, reporter)), None)
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    let heads = connection
        .list()
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    let branch = format!("refs/heads/{git_ref}");
    let tag = format!("refs/tags/{git_ref}");
    let names: Vec<&str> = heads.iter().map(git2::RemoteHead::name).collect();
    if names.contains(&branch.as_str()) {
        Ok(RemoteRef::Branch(git_ref.to_owned()))
    } else if names.contains(&tag.as_str()) {
        Ok(RemoteRef::Tag(git_ref.to_owned()))
    } else {
        Err(Error::RefNotFound {
            remote: url.to_owned(),
            git_ref: git_ref.to_owned(),
        })
    }
}

/// Fetches only `target` with depth 1 and returns the commit it points to.
pub(crate) fn fetch_shallow(
    repo: &Repository,
    url: &str,
    target: &RemoteRef,
    token: &Secret,
    reporter: &Reporter,
) -> Result<Oid> {
    let action = format!("fetch from {url}");
    let mut remote = repo
        .find_remote(ORIGIN)
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    let mut options = FetchOptions::new();
    options
        .remote_callbacks(callbacks(token, reporter))
        .download_tags(AutotagOption::None);
    if is_local(url) {
        // libgit2's local transport cannot fetch shallow; a local copy is
        // cheap anyway. The real remote is always HTTPS.
        tracing::debug!(%url, "local remote: fetching full history");
    } else {
        options.depth(1);
    }
    remote
        .fetch(&[target.refspec()], Some(&mut options), None)
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    let commit = repo
        .revparse_single(&target.local_ref())
        .and_then(|object| object.peel_to_commit())
        .map_err(|e| git_error(&e, &action, url, reporter))?;
    Ok(commit.id())
}

/// Checks out `commit` and points HEAD at it: a local branch tracking the
/// remote one for branches, a detached HEAD for tags.
///
/// `force` is only used on a fresh clone; updates use a safe checkout, which
/// never overwrites modified files.
pub(crate) fn checkout(
    repo: &Repository,
    target: &RemoteRef,
    commit: Oid,
    force: bool,
    reporter: &Reporter,
) -> Result<()> {
    let action = "check out the source";
    let map = |e: git2::Error| Error::Git {
        action: action.to_owned(),
        message: e.message().to_owned(),
    };
    let object = repo.find_object(commit, None).map_err(map)?;
    let mut builder = CheckoutBuilder::new();
    if force {
        builder.force();
    } else {
        builder.safe();
    }
    let progress = reporter.clone();
    builder.progress(move |_path, done, total| {
        if total > 0 {
            #[allow(clippy::cast_precision_loss)]
            progress.fraction(Some(done as f32 / total as f32));
        }
    });
    repo.checkout_tree(&object, Some(&mut builder))
        .map_err(map)?;
    match target {
        RemoteRef::Branch(name) => {
            // `Repository::branch` refuses to move the checked-out branch, so
            // write the ref directly.
            let refname = format!("refs/heads/{name}");
            repo.reference(&refname, commit, true, "UnLauncher: check out")
                .map_err(map)?;
            let mut branch = repo
                .find_branch(name, git2::BranchType::Local)
                .map_err(map)?;
            branch
                .set_upstream(Some(&format!("{ORIGIN}/{name}")))
                .map_err(map)?;
            repo.set_head(&refname).map_err(map)?;
        }
        RemoteRef::Tag(_) => repo.set_head_detached(commit).map_err(map)?,
    }
    Ok(())
}
