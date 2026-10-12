// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

use crate::known_errors::Diagnosis;

/// Errors returned by `ue-build`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file or directory could not be read.
    #[error("failed to read {}", path.display())]
    ReadFile {
        /// The file or directory that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A file or directory could not be written or removed.
    #[error("failed to write {}", path.display())]
    WriteFile {
        /// The file or directory that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// The clone destination exists and is not an empty directory.
    #[error("{} already exists and is not an empty folder", path.display())]
    DestinationNotEmpty {
        /// The destination folder.
        path: PathBuf,
    },

    /// A git operation failed.
    #[error("git failed to {action}: {message}")]
    Git {
        /// What was being done, e.g. `"fetch from https://github.com/…"`.
        action: String,
        /// The message reported by libgit2 (never contains the token).
        message: String,
    },

    /// The remote rejected the GitHub token.
    #[error("GitHub rejected the token for {remote}")]
    AuthenticationFailed {
        /// The remote URL.
        remote: String,
    },

    /// The remote has no branch or tag with that name.
    #[error("{remote} has no branch or tag named {git_ref:?}")]
    RefNotFound {
        /// The remote URL.
        remote: String,
        /// The branch or tag that was asked for.
        git_ref: String,
    },

    /// The built engine could not be registered.
    #[error("cannot register the built engine")]
    Register(#[source] Box<ue_core::Error>),

    /// A folder is not a git repository.
    #[error("{} is not a git repository", path.display())]
    NotARepository {
        /// The folder that was opened.
        path: PathBuf,
    },

    /// The engine source has local changes, so updating it could destroy work.
    #[error("{} has {} locally modified file(s)", repo.display(), files.len())]
    LocalChanges {
        /// The repository.
        repo: PathBuf,
        /// The modified files, relative to the repository root.
        files: Vec<PathBuf>,
    },

    /// A build script is missing from the engine source.
    #[error("{} not found", path.display())]
    MissingScript {
        /// The script that was expected.
        path: PathBuf,
    },

    /// A build step could not be started.
    #[error("failed to start {program}")]
    Spawn {
        /// The program that was started.
        program: String,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A build step exited with an error.
    #[error("{stage} failed{}", exit_code.map(|c| format!(" with exit code {c}")).unwrap_or_default())]
    BuildFailed {
        /// The stage that failed, e.g. "Setup.sh".
        stage: String,
        /// The exit code, or `None` if the process was killed by a signal.
        exit_code: Option<i32>,
        /// Known problems recognised in the output, most relevant first.
        diagnosis: Vec<Diagnosis>,
        /// The full build log.
        log_path: PathBuf,
    },

    /// The task was cancelled by the user.
    #[error("cancelled")]
    Cancelled,

    /// A background worker failed unexpectedly.
    #[error("internal error: {message}")]
    Internal {
        /// What went wrong.
        message: String,
    },
}

/// Result type of `ue-build`.
pub type Result<T> = std::result::Result<T, Error>;

impl From<jobs::Cancelled> for Error {
    fn from(_: jobs::Cancelled) -> Self {
        Self::Cancelled
    }
}

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::ReadFile { .. } => {
                "Check that the path exists and that you have permission to read it.".to_owned()
            }
            Self::WriteFile { .. } => "Check that you have permission to change the folder \
                 and that the disk is not full."
                .to_owned(),
            Self::DestinationNotEmpty { .. } => {
                "Choose a new or empty folder for the engine source.".to_owned()
            }
            Self::Git { .. } => "Check your internet connection and try again. If it keeps \
                 failing, see the log for the git error."
                .to_owned(),
            Self::AuthenticationFailed { .. } => {
                "Create a new GitHub token with read access to EpicGames/UnrealEngine and \
                 check that your GitHub account is linked to your Epic account \
                 (https://www.unrealengine.com/en-US/ue-on-github)."
                    .to_owned()
            }
            Self::RefNotFound { .. } => {
                "Check the spelling of the branch or tag (for example \"5.7.1-release\")."
                    .to_owned()
            }
            Self::Register(source) => return source.hint(),
            Self::NotARepository { .. } => {
                "Only engines cloned from git can be updated; download the source again.".to_owned()
            }
            Self::LocalChanges { .. } => "UnLauncher never discards your changes. Commit, \
                 stash or revert them with git, then update again."
                .to_owned(),
            Self::MissingScript { .. } => {
                "The engine source looks incomplete. Download it again.".to_owned()
            }
            Self::Spawn { .. } => {
                "Check that bash is installed and that the script is executable.".to_owned()
            }
            Self::BuildFailed {
                diagnosis,
                log_path,
                ..
            } => match diagnosis.first() {
                Some(d) => format!("{} {}", d.cause, d.solution),
                None => format!(
                    "No known cause was recognised. See the full log at {}.",
                    log_path.display()
                ),
            },
            Self::Cancelled => return None,
            Self::Internal { .. } => "This is a bug in UnLauncher; please report it.".to_owned(),
        };
        Some(hint)
    }
}
