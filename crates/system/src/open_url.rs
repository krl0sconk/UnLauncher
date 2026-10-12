// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening web pages in the user's browser.

use crate::error::{Error, Result};
use crate::host_command::HostCommand;
use crate::sandbox::Sandbox;

/// Opens `url` (http or https only) in the default browser.
///
/// Runs `xdg-open` directly, also inside Flatpak: there `xdg-open` is the
/// sandbox's own helper, which goes through the `OpenURI` portal, so no host
/// access is needed.
///
/// # Errors
///
/// [`Error::UnsupportedUrl`] for other schemes, and [`Error::SpawnFailed`] if
/// `xdg-open` cannot be started.
pub fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::UnsupportedUrl {
            url: url.to_owned(),
        });
    }
    HostCommand::new("xdg-open")
        .arg(url)
        .spawn_detached_in(Sandbox::Host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_web_urls() {
        assert!(matches!(
            open_url("file:///etc/passwd"),
            Err(Error::UnsupportedUrl { .. })
        ));
    }
}
