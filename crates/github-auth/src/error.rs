// SPDX-License-Identifier: GPL-3.0-or-later

use common::UserFacing;

/// Errors returned by `github-auth`. None of them contains a token.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// GitHub could not be reached.
    #[error("could not reach GitHub")]
    Http(#[source] reqwest::Error),

    /// GitHub answered with an HTTP status this crate does not expect.
    #[error("GitHub answered HTTP {status} to {what}")]
    UnexpectedStatus {
        /// What was asked.
        what: &'static str,
        /// HTTP status.
        status: u16,
    },

    /// GitHub's answer did not have the expected shape.
    #[error("GitHub's answer to {what} has an unexpected format")]
    UnexpectedAnswer {
        /// What was asked.
        what: &'static str,
    },

    /// The device code expired before the user authorized UnLauncher.
    #[error("the GitHub code expired before it was used")]
    Expired,

    /// The user declined to authorize UnLauncher on GitHub.
    #[error("the GitHub authorization was declined")]
    Denied,

    /// Signing in with the GitHub CLI failed.
    #[error("the GitHub CLI could not sign in: {message}")]
    GhLogin {
        /// What `gh` said last (never a token).
        message: String,
    },

    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,

    /// GitHub stopped the Device Flow with another error.
    #[error("GitHub stopped the sign-in: {code}")]
    DeviceFlow {
        /// GitHub's error code, e.g. `unsupported_grant_type`.
        code: String,
    },
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // reqwest errors carry the URL, never the request body or headers.
        Self::Http(error.without_url())
    }
}

/// Result type of `github-auth`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::Http(_) => "Check your internet connection and try again.",
            Self::UnexpectedStatus { .. } | Self::UnexpectedAnswer { .. } => {
                "GitHub may be having problems; try again later. If it persists, update \
                 UnLauncher."
            }
            Self::Expired => "Start “Connect with GitHub” again and enter the new code soon.",
            Self::Denied => {
                "Start “Connect with GitHub” again and choose Authorize on GitHub, or use a \
                 personal token under Advanced options."
            }
            Self::GhLogin { .. } => {
                "Check that the GitHub CLI works (`gh auth status` in a terminal), or use a \
                 personal token under Advanced options."
            }
            Self::Cancelled => return None,
            Self::DeviceFlow { .. } => {
                "Try again later, or use a personal token under Advanced options."
            }
        };
        Some(hint.to_owned())
    }
}
