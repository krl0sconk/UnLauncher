// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::path::PathBuf;

use common::UserFacing;

/// Errors returned by `system`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `$HOME` is not set, so user paths cannot be found.
    #[error("the HOME environment variable is not set")]
    HomeNotSet,

    /// Only web pages can be opened.
    #[error("refusing to open {url}: only http and https links are supported")]
    UnsupportedUrl {
        /// The rejected URL.
        url: String,
    },

    /// A process could not be started.
    #[error("failed to start {program}")]
    SpawnFailed {
        /// The program that was started (as given, before `flatpak-spawn`).
        program: String,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A process did not finish in time and was stopped.
    #[error("{program} did not finish in time")]
    TimedOut {
        /// The program that was started (as given, before `flatpak-spawn`).
        program: String,
    },

    /// One of UnLauncher's own programs is not installed next to it.
    #[error("{program} is missing (looked for {})", path.display())]
    BundledProgramMissing {
        /// The program's name.
        program: String,
        /// Where it was expected.
        path: PathBuf,
    },

    /// A shortcut with the same desktop file id already exists and
    /// [`common::Overwrite::No`] was given.
    #[error("a shortcut already exists at {}", path.display())]
    ShortcutExists {
        /// The existing desktop file.
        path: PathBuf,
    },

    /// A shortcut has no command to run.
    #[error("the shortcut \"{name}\" has no command to run")]
    ShortcutWithoutCommand {
        /// The shortcut's display name.
        name: String,
    },

    /// The user cancelled the desktop's "add to applications" dialog.
    #[error("adding the shortcut was cancelled")]
    ShortcutCancelled,

    /// The `org.freedesktop.portal.DynamicLauncher` portal failed or is
    /// missing.
    #[error("the desktop launcher portal failed: {message}")]
    Portal {
        /// What the portal (or the D-Bus connection to it) reported.
        message: String,
    },

    /// A desktop notification could not be shown (no session bus,
    /// notification service or portal). Not fatal: log it and carry on.
    #[error("the desktop notification could not be shown: {message}")]
    Notification {
        /// What D-Bus, the portal or the notification server reported.
        message: String,
    },

    /// A file could not be written.
    #[error("failed to write {}", path.display())]
    WriteFile {
        /// The file being written.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// A file could not be removed.
    #[error("failed to remove {}", path.display())]
    RemoveFile {
        /// The file being removed.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Result type of `system`.
pub type Result<T> = std::result::Result<T, Error>;

impl UserFacing for Error {
    fn summary(&self) -> String {
        self.to_string()
    }

    fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::HomeNotSet => "Start UnLauncher from a normal user session.",
            Self::SpawnFailed { .. } => {
                "Check that the program is installed on your system and is executable."
            }
            Self::TimedOut { .. } => {
                "The program may be waiting for input or stuck; try again, or run it in a \
                 terminal to see what it asks."
            }
            Self::BundledProgramMissing { .. } => {
                "UnLauncher is not installed completely. Reinstall it, or during \
                 development build the whole workspace with `cargo build --workspace`."
            }
            Self::ShortcutExists { .. } => {
                "Confirm that you want to replace the existing shortcut, or remove it first."
            }
            Self::UnsupportedUrl { .. } | Self::ShortcutWithoutCommand { .. } => {
                "This is a bug in UnLauncher; please report it."
            }
            Self::ShortcutCancelled => return None,
            Self::Portal { .. } => {
                "Make sure xdg-desktop-portal and a portal backend for your desktop \
                 (for example xdg-desktop-portal-gnome or xdg-desktop-portal-kde) are \
                 installed and up to date."
            }
            Self::Notification { .. } => {
                "Notifications need a running notification service (and, inside Flatpak, \
                 xdg-desktop-portal). Check that your desktop shows notifications from \
                 other apps."
            }
            Self::WriteFile { .. } | Self::RemoveFile { .. } => {
                "Check that the folder exists, that you have permission to change it \
                 and that the disk is not full."
            }
        };
        Some(hint.to_owned())
    }
}
