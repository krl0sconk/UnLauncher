// SPDX-License-Identifier: GPL-3.0-or-later

//! "Get more on Fab": fab.com in the built-in web window, with the session
//! the window keeps. When the user closes it, the library is read again.

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;

/// The state of the fab.com window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FabWindow {
    /// Not open.
    Closed,
    /// Starting.
    Opening,
    /// Open: the user is on Fab.
    Open,
    /// The window cannot start: Fab opened in the browser instead.
    InBrowser,
}

/// Progress of the fab.com window.
#[derive(Debug)]
pub enum FabWindowEvent {
    /// The window is showing Fab.
    Opened,
    /// No window: Fab was opened in the system browser.
    Browser,
    /// The user closed the window.
    Closed,
}

impl LauncherApp {
    /// Opens fab.com in the web window, or in the system browser when the
    /// window cannot start.
    pub(super) fn get_more_on_fab(&mut self) {
        if matches!(
            self.library.fab_window,
            FabWindow::Opening | FabWindow::Open
        ) {
            return;
        }
        self.library.fab_window = FabWindow::Opening;
        let webview_dir = self.env.webview_dir.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let report = move |event| {
            let _ = tx.send(Event::FabWindow(event));
            ctx.request_repaint();
        };
        self.spawn_async(async move {
            let last = fab_window(&webview_dir, &report).await;
            Event::FabWindow(last)
        });
    }

    pub(super) fn fab_window_event(&mut self, event: &FabWindowEvent) {
        match event {
            FabWindowEvent::Opened => self.library.fab_window = FabWindow::Open,
            FabWindowEvent::Browser => self.library.fab_window = FabWindow::InBrowser,
            FabWindowEvent::Closed => {
                self.library.fab_window = FabWindow::Closed;
                // The user may have claimed or bought something.
                self.handle_epic(Intent::RefreshLibrary);
            }
        }
    }
}

/// Shows fab.com in the window until the user closes it. Returns how it
/// ended; `report` is told when the window is open.
async fn fab_window(
    webview_dir: &std::path::Path,
    report: &(impl Fn(FabWindowEvent) + Send + Sync),
) -> FabWindowEvent {
    let mut window = match web_bridge::WebBridge::spawn(webview_dir).await {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!(%error, "the web window cannot start; opening Fab in the browser");
            return open_in_browser().await;
        }
    };
    if window.open(crate::text::FAB_URL).is_err() {
        return open_in_browser().await;
    }
    report(FabWindowEvent::Opened);
    loop {
        match window.next_event().await {
            None | Some(web_bridge::Event::Closed) => break,
            // Logins with Google, Apple and the like do not work inside an
            // embedded window: they go to the browser.
            Some(web_bridge::Event::PopupBlocked { url }) => {
                let opened = tokio::task::spawn_blocking(move || system::open_url(&url)).await;
                if let Ok(Err(error)) = opened {
                    tracing::warn!(%error, "cannot open a Fab popup in the browser");
                }
            }
            Some(web_bridge::Event::Error { message }) => {
                tracing::warn!(%message, "the Fab window reported an error");
            }
            Some(_) => {}
        }
    }
    window.close().await;
    FabWindowEvent::Closed
}

/// Fab in the system browser, for when there is no window.
async fn open_in_browser() -> FabWindowEvent {
    let opened = tokio::task::spawn_blocking(|| system::open_url(crate::text::FAB_URL)).await;
    if let Ok(Err(error)) = opened {
        tracing::warn!(%error, "cannot open Fab in the browser");
    }
    FabWindowEvent::Browser
}
