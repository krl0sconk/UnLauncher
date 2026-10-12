// SPDX-License-Identifier: GPL-3.0-or-later

//! Shortcuts through `org.freedesktop.portal.DynamicLauncher`, used inside
//! Flatpak. Kept thin: it cannot be tested without a running portal.

use std::future::Future;
use std::path::PathBuf;

use ashpd::desktop::Icon;
use ashpd::desktop::ResponseError;
use ashpd::desktop::dynamic_launcher::{
    DynamicLauncherProxy, InstallOptions, LauncherType, PrepareInstallOptions, UninstallOptions,
};
use common::Overwrite;

use crate::error::{Error, Result};

/// Asks the portal to install `entry` as `file_name`, showing the desktop's
/// confirmation dialog with `name` and `icon_png`.
pub(super) fn install(
    file_name: &str,
    name: &str,
    icon_png: &[u8],
    entry: &str,
    overwrite: Overwrite,
) -> Result<()> {
    block_on(async {
        let proxy = proxy().await?;
        if overwrite == Overwrite::No && proxy.desktop_entry(file_name).await.is_ok() {
            return Err(Error::ShortcutExists {
                path: PathBuf::from(file_name),
            });
        }
        let options = PrepareInstallOptions::default()
            .set_launcher_type(LauncherType::Application)
            .set_modal(true);
        let response = proxy
            .prepare_install(None, name, Icon::Bytes(icon_png.to_vec()), options)
            .await
            .map_err(portal_error)?
            .response()
            .map_err(portal_error)?;
        proxy
            .install(
                response.token(),
                file_name,
                entry,
                InstallOptions::default(),
            )
            .await
            .map_err(portal_error)?;
        tracing::info!(
            desktop_file = file_name,
            "shortcut installed through the portal"
        );
        Ok(())
    })
}

/// Asks the portal to remove the launcher `file_name`.
pub(super) fn uninstall(file_name: &str) -> Result<()> {
    block_on(async {
        let proxy = proxy().await?;
        proxy
            .uninstall(file_name, UninstallOptions::default())
            .await
            .map_err(portal_error)?;
        tracing::info!(
            desktop_file = file_name,
            "shortcut removed through the portal"
        );
        Ok(())
    })
}

/// The portal shows a dialog that waits for the user.
const PORTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(5);

/// Runs `future` on a private runtime; see [`crate::blocking::block_on`].
fn block_on<T: Send>(future: impl Future<Output = Result<T>> + Send) -> Result<T> {
    crate::blocking::block_on(future, PORTAL_TIMEOUT, |message| Error::Portal { message })
}

/// The `DynamicLauncher` portal on a connection of our own (see
/// [`crate::blocking::block_on`]).
async fn proxy() -> Result<DynamicLauncherProxy> {
    let connection = ashpd::zbus::Connection::session()
        .await
        .map_err(|error| Error::Portal {
            message: error.to_string(),
        })?;
    DynamicLauncherProxy::with_connection(connection)
        .await
        .map_err(portal_error)
}

/// Maps an `ashpd` error to ours.
fn portal_error(error: ashpd::Error) -> Error {
    match error {
        ashpd::Error::Response(ResponseError::Cancelled) => Error::ShortcutCancelled,
        other => Error::Portal {
            message: other.to_string(),
        },
    }
}
