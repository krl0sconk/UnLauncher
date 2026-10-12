// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux and Flatpak integration.
//!
//! - [`Sandbox`]: whether UnLauncher runs inside Flatpak.
//! - [`HostCommand`]: the only way to start external processes; it goes
//!   through `flatpak-spawn --host` when sandboxed.
//! - [`paths`]: system paths (home, `Install.ini`, default search paths,
//!   UnLauncher's own config and data folders).
//! - [`DesktopShortcut`]: per-engine desktop shortcuts, written directly on
//!   the host or through the `DynamicLauncher` portal inside Flatpak.
//! - [`notify`]: desktop notifications, through the Notification portal
//!   inside Flatpak.

mod blocking;
mod error;
mod host_command;
mod notifications;
mod open_url;
pub mod paths;
mod sandbox;
mod shortcuts;

pub use error::{Error, Result};
pub use host_command::HostCommand;
pub use notifications::notify;
pub use open_url::open_url;
pub use sandbox::Sandbox;
pub use shortcuts::{
    DesktopShortcut, desktop_file_id, install_shortcut, remove_shortcut, render_desktop_entry,
};

/// UnLauncher's application id (Flatpak app id, D-Bus name and desktop file
/// name).
pub const APP_ID: &str = "io.github.krl0sconk.UnLauncher";
