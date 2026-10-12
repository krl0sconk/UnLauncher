// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

/// Errors returned by `fab`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Signing in or refreshing the Epic session failed.
    #[error(transparent)]
    Auth(#[from] epic_auth::Error),

    /// A request failed before an answer arrived.
    #[error("cannot reach Fab ({what})")]
    Network {
        /// What was being requested.
        what: String,
        /// The transport error, with its URL removed (signed URLs).
        #[source]
        source: reqwest::Error,
    },

    /// Fab or the CDN answered with an unexpected HTTP status.
    #[error("Fab answered HTTP {status} ({what})")]
    Http {
        /// What was being requested.
        what: String,
        /// The HTTP status code.
        status: u16,
    },

    /// Fab refused the manifest request (HTTP 403); it does so for a while
    /// after too many requests.
    #[error("Fab is refusing downloads right now")]
    FabBusy,

    /// Epic's answer does not match what UnLauncher expects.
    #[error("Fab's response changed and UnLauncher cannot understand it: {what}")]
    ApiChanged {
        /// What did not match.
        what: String,
    },

    /// Fab offers no download for this asset version.
    #[error("Fab offers no download for \"{title}\"")]
    NoDownload {
        /// Asset title.
        title: String,
    },

    /// The download manifest does not match the hash Fab announced.
    #[error("the download manifest is corrupt")]
    ManifestCorrupt,

    /// A downloaded chunk failed verification twice.
    #[error("downloaded data is corrupt (chunk {guid})")]
    ChunkCorrupt {
        /// Chunk GUID.
        guid: String,
    },

    /// The manifest names a file outside the download folder.
    #[error("the asset contains an unsafe path: {path}")]
    UnsafePath {
        /// The offending path, as written in the manifest.
        path: String,
    },

    /// A file operation failed.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being done ("write", "create folder"…).
        action: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },

    /// The task was cancelled.
    #[error("cancelled")]
    Cancelled,

    /// The install destination already exists and overwriting was not allowed.
    #[error("{} already exists", path.display())]
    AlreadyExists {
        /// The existing folder or file.
        path: PathBuf,
    },

    /// The install target is not a `.uproject` file or an engine folder.
    #[error("{} is not a project file or an engine folder", path.display())]
    TargetNotFound {
        /// The target given.
        path: PathBuf,
    },

    /// Content packs go into a project, not into an engine.
    #[error("content assets can only be installed into a project")]
    ContentIntoEngine,

    /// A new project was requested but the asset is not a complete project.
    #[error("{} is not a complete project", path.display())]
    NotAProject {
        /// The downloaded files.
        path: PathBuf,
    },

    /// Complete projects are not installed into another project or engine.
    #[error("this asset is a complete project")]
    ProjectAsset {
        /// Where the downloaded project is.
        path: PathBuf,
    },

    /// The downloaded files contain no `.uplugin`, `Content/` or `.uproject`.
    #[error("cannot recognise the downloaded asset in {}", path.display())]
    UnknownLayout {
        /// The downloaded files.
        path: PathBuf,
    },
}

/// Result type of `fab`.
pub type Result<T> = std::result::Result<T, Error>;

impl From<jobs::Cancelled> for Error {
    fn from(_: jobs::Cancelled) -> Self {
        Self::Cancelled
    }
}

impl Error {
    /// Builds an [`Error::Io`].
    pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }

    /// Builds an [`Error::Network`] without the (possibly signed) URL.
    pub(crate) fn network(what: impl Into<String>, source: reqwest::Error) -> Self {
        Self::Network {
            what: what.into(),
            source: source.without_url(),
        }
    }

    /// Builds an [`Error::ApiChanged`].
    pub(crate) fn api_changed(what: impl Into<String>) -> Self {
        Self::ApiChanged { what: what.into() }
    }
}

impl UserFacing for Error {
    fn summary(&self) -> String {
        match self {
            Self::Auth(error) => error.summary(),
            other => other.to_string(),
        }
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::Auth(error) => return error.hint(),
            Self::Network { .. } => "Check your internet connection and try again.",
            Self::Http { .. } | Self::FabBusy => "Try again in a few minutes.",
            Self::ApiChanged { .. } => {
                "Epic changed its service; update UnLauncher to the latest version."
            }
            Self::NoDownload { .. } => {
                "Check on fab.com that the asset is available for Unreal Engine."
            }
            Self::ManifestCorrupt | Self::ChunkCorrupt { .. } => {
                "Retry the download; already verified parts are kept."
            }
            Self::UnsafePath { .. } => "Report this asset; UnLauncher refused to write it.",
            Self::Io { .. } => "Check free disk space and folder permissions.",
            Self::Cancelled => return None,
            Self::AlreadyExists { .. } => "Remove or rename it, or confirm overwriting.",
            Self::TargetNotFound { .. } => {
                "Choose a .uproject file or the root folder of an installed engine."
            }
            Self::ContentIntoEngine => "Choose a project as the destination.",
            Self::NotAProject { .. } => "Install it into an existing project instead.",
            Self::ProjectAsset { path } => {
                return Some(format!(
                    "Open or copy the downloaded project from {}.",
                    path.display()
                ));
            }
            Self::UnknownLayout { .. } => {
                "Install it by hand from the downloaded folder, or report the asset."
            }
        };
        Some(hint.to_owned())
    }
}

/// Converts an `egs-api` error from a Fab call.
pub(crate) fn from_egs(what: &str, error: egs_api::api::error::EpicAPIError) -> Error {
    use egs_api::api::error::EpicAPIError as E;
    match error {
        E::FabTimeout => Error::FabBusy,
        E::NetworkError(source) => Error::network(what, source),
        E::InvalidCredentials => Error::Auth(epic_auth::Error::SessionExpired),
        E::HttpError { status, .. } if status == reqwest::StatusCode::UNAUTHORIZED => {
            Error::Auth(epic_auth::Error::SessionExpired)
        }
        E::HttpError { status, .. } => Error::Http {
            what: what.to_owned(),
            status: status.as_u16(),
        },
        E::Server => Error::Http {
            what: what.to_owned(),
            status: 500,
        },
        // Messages and bodies are dropped: they are not meant for users.
        E::APIError(_) | E::DeserializationError(_) | E::InvalidParams => {
            Error::api_changed(format!("{what}: unexpected response"))
        }
    }
}
