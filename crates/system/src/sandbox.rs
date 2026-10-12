// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

/// File that Flatpak mounts inside every sandbox.
const FLATPAK_INFO: &str = "/.flatpak-info";

/// Where UnLauncher is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sandbox {
    /// Directly on the host system.
    Host,
    /// Inside a Flatpak sandbox: host processes need `flatpak-spawn --host`.
    Flatpak,
}

impl Sandbox {
    /// Detects the sandbox by checking for `/.flatpak-info`.
    #[must_use]
    pub fn detect() -> Self {
        Self::detect_at(Path::new(FLATPAK_INFO))
    }

    /// Like [`Sandbox::detect`], with the marker file path injected for tests.
    #[must_use]
    pub fn detect_at(flatpak_info: &Path) -> Self {
        if flatpak_info.is_file() {
            Self::Flatpak
        } else {
            Self::Host
        }
    }

    /// Whether this is [`Sandbox::Flatpak`].
    #[must_use]
    pub fn is_flatpak(self) -> bool {
        self == Self::Flatpak
    }
}
