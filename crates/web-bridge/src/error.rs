// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;

use common::UserFacing;

/// Errors returned by `web-bridge`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `unlauncher-web` is not installed next to UnLauncher.
    #[error(transparent)]
    Spawn(#[from] system::Error),

    /// The process could not be started or talked to.
    #[error("cannot {action} the web window")]
    Io {
        /// What was being done (`"start"`, `"send a command to"`).
        action: &'static str,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// The window exited before it was ready, usually because WebKitGTK is
    /// missing outside Flatpak.
    #[error("the web window could not start{}", detail.as_ref().map(|d| format!(": {d}")).unwrap_or_default())]
    NotStarted {
        /// The last line the window wrote on stderr, if any.
        detail: Option<String>,
    },

    /// The user closed the window (or it exited) before finishing.
    #[error("the web window was closed")]
    Closed,

    /// Waiting took too long.
    #[error("timed out waiting for {what}")]
    Timeout {
        /// What was awaited.
        what: &'static str,
    },

    /// The page failed to do what was asked.
    #[error("the web page failed: {message}")]
    Page {
        /// What the window reported. Never contains secrets.
        message: String,
    },

    /// The page answered, but not with JSON.
    #[error("the web page answered HTTP {status} without the expected data")]
    NotJson {
        /// The last HTTP status seen.
        status: u16,
    },

    /// The window's data folder could not be removed.
    #[error("cannot remove the web window's data in {}", path.display())]
    ClearData {
        /// The folder.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Result type of `web-bridge`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::Spawn(error) => return error.hint(),
            Self::Io { .. } | Self::NotStarted { .. } => {
                "The built-in web window needs WebKitGTK 4.1. Outside Flatpak, install it \
                 (Ubuntu/Debian: libwebkit2gtk-4.1-0, Fedora: webkit2gtk4.1, Arch: \
                 webkit2gtk-4.1) or use the system browser instead."
            }
            Self::Closed => return None,
            Self::Timeout { .. } | Self::NotJson { .. } => {
                "Make sure you are signed in to your Epic account in the window and try \
                 again, or use the system browser instead."
            }
            Self::Page { .. } => "Try again, or use the system browser instead.",
            Self::ClearData { .. } => {
                "Check that you have permission to change UnLauncher's data folder."
            }
        };
        Some(hint.to_owned())
    }
}
