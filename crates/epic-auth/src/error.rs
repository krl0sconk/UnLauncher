// SPDX-License-Identifier: GPL-3.0-or-later

use common::UserFacing;

/// Errors returned by `epic-auth`.
///
/// No variant ever carries a token or an authorization code.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The HTTP client could not be built (TLS backend unavailable).
    #[error("cannot set up the HTTP client")]
    HttpClient(#[source] reqwest::Error),

    /// The system keyring could not be used.
    #[error("cannot access the system keyring: {message}")]
    Keyring {
        /// What the keyring reported. Never contains the secret.
        message: String,
    },

    /// The pasted text is neither an authorization code nor Epic's JSON page.
    #[error("the pasted text is not an Epic authorization code")]
    InvalidAuthorizationCode,

    /// Epic's page says `"authorizationCode": null`: the browser was not
    /// signed in to Epic when the page was opened.
    #[error("Epic did not return an authorization code: you are not signed in on the Epic website")]
    NotSignedIn,

    /// Epic rejected the authorization code or the stored refresh token.
    ///
    /// Epic's own message is not kept: it can quote the rejected token.
    #[error("Epic rejected the sign-in")]
    LoginRejected,

    /// The session can no longer be refreshed; the user must sign in again.
    #[error("the Epic session expired")]
    SessionExpired,

    /// A request to Epic failed before an answer arrived.
    #[error("cannot reach Epic Games ({what})")]
    Network {
        /// What was being requested.
        what: &'static str,
        /// The transport error, with its URL removed.
        #[source]
        source: reqwest::Error,
    },

    /// Epic answered with a server error.
    #[error("Epic Games is not available right now ({what})")]
    EpicUnavailable {
        /// What was being requested.
        what: &'static str,
    },

    /// Epic answered something UnLauncher does not understand.
    #[error("Epic's response changed and UnLauncher cannot understand it: {what}")]
    ApiChanged {
        /// What did not match the expectations.
        what: String,
    },
}

/// Result type of `epic-auth`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        Some(
            match self {
                Self::HttpClient(_) => "Reinstall UnLauncher; this is a packaging problem.",
                Self::Keyring { .. } => {
                    "Make sure a Secret Service keyring (GNOME Keyring or KWallet) is installed and unlocked."
                }
                Self::InvalidAuthorizationCode => {
                    "Copy the 32-character authorizationCode from the Epic page (or the whole page) and paste it again."
                }
                Self::NotSignedIn => {
                    "Sign in on the Epic website in your browser, then open the login link again and copy the new code."
                }
                Self::LoginRejected => {
                    "Authorization codes expire after a few minutes and work once: open the login link again and paste the new code."
                }
                Self::SessionExpired => "Sign in to Epic again.",
                Self::Network { .. } => "Check your internet connection and try again.",
                Self::EpicUnavailable { .. } => "Try again in a few minutes.",
                Self::ApiChanged { .. } => {
                    "Epic changed its service; update UnLauncher to the latest version."
                }
            }
            .to_owned(),
        )
    }
}

/// Converts an `egs-api` error, keeping tokens and URLs out of the message.
pub(crate) fn from_egs(what: &'static str, error: egs_api::api::error::EpicAPIError) -> Error {
    use egs_api::api::error::EpicAPIError as E;
    match error {
        // Epic's message can quote the rejected token: drop it.
        E::InvalidCredentials | E::APIError(_) => Error::LoginRejected,
        E::NetworkError(source) => Error::Network {
            what,
            source: source.without_url(),
        },
        E::Server => Error::EpicUnavailable { what },
        E::HttpError { status, .. } if status.is_server_error() => Error::EpicUnavailable { what },
        E::HttpError { status, .. }
            if status == reqwest::StatusCode::BAD_REQUEST
                || status == reqwest::StatusCode::UNAUTHORIZED =>
        {
            Error::LoginRejected
        }
        E::HttpError { status, .. } => Error::ApiChanged {
            what: format!("{what}: unexpected HTTP {status}"),
        },
        E::DeserializationError(_) | E::InvalidParams | E::FabTimeout => Error::ApiChanged {
            what: format!("{what}: unexpected response"),
        },
    }
}
