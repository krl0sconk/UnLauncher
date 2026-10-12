// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop notifications, e.g. "Unreal Engine 5.6 finished building".
//!
//! Inside Flatpak they go through the `org.freedesktop.portal.Notification`
//! portal; on the host, straight to `org.freedesktop.Notifications` with
//! `notify-rust`. Both need a D-Bus session bus, so only the construction of
//! the notifications is unit-tested.

use std::sync::atomic::{AtomicU64, Ordering};

use ashpd::desktop::notification::{Notification as PortalNotification, NotificationProxy};

use crate::APP_ID;
use crate::blocking::block_on;
use crate::error::{Error, Result};
use crate::sandbox::Sandbox;

/// Application name shown by notification servers on the host.
const APP_NAME: &str = "UnLauncher";

/// How long a notification may take to reach D-Bus.
const NOTIFY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Counter that makes portal notification ids unique within the process (the
/// portal replaces a notification that reuses an id).
static NEXT_PORTAL_ID: AtomicU64 = AtomicU64::new(1);

/// Shows a desktop notification with `title` and `body`.
///
/// - [`Sandbox::Flatpak`]: through the XDG Notification portal.
/// - [`Sandbox::Host`]: through the desktop's notification server.
///
/// Blocks until the notification has been handed to D-Bus (not until it is
/// dismissed). Safe to call from any thread, including async runtime threads.
///
/// A notification is a courtesy: callers should log a failure and carry on.
///
/// # Errors
///
/// [`Error::Notification`] if there is no session bus, no notification
/// service or portal.
pub fn notify(sandbox: Sandbox, title: &str, body: &str) -> Result<()> {
    match sandbox {
        Sandbox::Host => {
            let notification = host_notification(title, body);
            block_on(
                async move {
                    notification
                        .show_async()
                        .await
                        .map(drop)
                        .map_err(|error| notification_error(error.to_string()))
                },
                NOTIFY_TIMEOUT,
                notification_error,
            )?;
        }
        Sandbox::Flatpak => {
            let id = portal_id();
            let notification = portal_notification(title, body);
            block_on(
                async move {
                    // A connection of our own: see `crate::blocking::block_on`.
                    let connection = ashpd::zbus::Connection::session()
                        .await
                        .map_err(|error| notification_error(error.to_string()))?;
                    let proxy = NotificationProxy::with_connection(connection)
                        .await
                        .map_err(|error| notification_error(error.to_string()))?;
                    proxy
                        .add_notification(&id, notification)
                        .await
                        .map_err(|error| notification_error(error.to_string()))
                },
                NOTIFY_TIMEOUT,
                notification_error,
            )?;
        }
    }
    tracing::debug!(%title, "notification shown");
    Ok(())
}

/// The notification sent on the host.
fn host_notification(title: &str, body: &str) -> notify_rust::Notification {
    let mut notification = notify_rust::Notification::new();
    notification
        .appname(APP_NAME)
        .summary(title)
        .body(body)
        .icon(APP_ID);
    notification
}

/// The notification sent through the portal (it uses the app's own icon).
fn portal_notification(title: &str, body: &str) -> PortalNotification {
    PortalNotification::new(title).body(body)
}

/// A new portal notification id.
fn portal_id() -> String {
    let n = NEXT_PORTAL_ID.fetch_add(1, Ordering::Relaxed);
    format!("unlauncher-{n}")
}

/// [`Error::Notification`] with `message`.
fn notification_error(message: String) -> Error {
    Error::Notification { message }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn host_notification_has_title_body_and_app() {
        let n = host_notification("Build finished", "Unreal Engine 5.6 is ready");
        assert_eq!(n.summary, "Build finished");
        assert_eq!(n.body, "Unreal Engine 5.6 is ready");
        assert_eq!(n.appname, APP_NAME);
        assert_eq!(n.icon, APP_ID);
    }

    #[test]
    fn portal_notification_has_title_and_body() {
        let debug = format!("{:?}", portal_notification("Build failed", "See the log"));
        assert!(debug.contains("Build failed"), "{debug}");
        assert!(debug.contains("See the log"), "{debug}");
    }

    #[test]
    fn portal_ids_are_unique() {
        assert_ne!(portal_id(), portal_id());
    }
}
