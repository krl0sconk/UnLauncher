// SPDX-License-Identifier: GPL-3.0-or-later

//! `unlauncher-web`: UnLauncher's embedded Epic web window.
//!
//! A `wry` + `tao` window with no business logic. It reads one JSON command
//! per line on stdin, runs it, and writes one JSON event per line on stdout
//! (see `web_bridge::protocol`). The main interface starts it through
//! `web_bridge::WebBridge`; it can also be driven by hand:
//!
//! ```text
//! unlauncher-web --data-dir /tmp/unlauncher-web
//! {"type":"open","url":"https://www.unrealengine.com/linux"}
//! ```
//!
//! The authorization code only ever leaves in an `auth_code` event; URLs in
//! `navigated` events have no query or fragment.

mod page;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tao::event::{Event as WindowEvent, WindowEvent as WinEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Window, WindowBuilder};
use web_bridge::protocol::{Command, Event, Link, decode_command, encode_event};
use web_bridge::{PageKind, classify_url, is_allowed_navigation};
use wry::{PageLoadEvent, WebContext, WebView, WebViewBuilder};

use crate::page::{CodePage, PageMessage};

const TITLE: &str = "UnLauncher — Epic Games";
const USAGE: &str = "usage: unlauncher-web --data-dir <folder>";

/// Events delivered to the window's event loop from other threads and from
/// the web view's callbacks.
#[derive(Debug)]
enum UserEvent {
    Command(Command),
    StdinClosed,
    Ipc(String),
    PageLoaded(String),
    Popup(String),
}

fn main() -> ExitCode {
    let Some(data_dir) = parse_args(std::env::args().skip(1)) else {
        let _ = writeln!(std::io::stderr(), "{USAGE}");
        return ExitCode::from(2);
    };
    if let Err(error) = prepare_data_dir(&data_dir) {
        let _ = writeln!(
            std::io::stderr(),
            "unlauncher-web: cannot create {}: {error}",
            data_dir.display()
        );
        return ExitCode::FAILURE;
    }
    match run(data_dir) {
        Ok(never) => match never {},
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "unlauncher-web: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `--data-dir <folder>`, the only argument.
fn parse_args(mut args: impl Iterator<Item = String>) -> Option<PathBuf> {
    match (args.next().as_deref(), args.next(), args.next()) {
        (Some("--data-dir"), Some(dir), None) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => None,
    }
}

/// Creates the data folder, readable only by the user (cookies live there).
fn prepare_data_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Empties the data folder without removing it (WebKit keeps it open).
fn empty_data_dir(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Writes one event line. Exits when the interface is gone.
fn emit(event: &Event) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", encode_event(event))
        .and_then(|()| out.flush())
        .is_err()
    {
        std::process::exit(0);
    }
}

fn emit_error(message: impl Into<String>) {
    emit(&Event::Error {
        message: message.into(),
    });
}

/// Reads commands from stdin until it closes.
fn read_stdin(proxy: &EventLoopProxy<UserEvent>) {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match decode_command(&line) {
            Some(command) => {
                if proxy.send_event(UserEvent::Command(command)).is_err() {
                    return;
                }
            }
            None => emit_error("unknown command"),
        }
    }
    let _ = proxy.send_event(UserEvent::StdinClosed);
}

/// The window, its web view and what the event loop remembers between events.
struct State {
    window: Window,
    webview: WebView,
    data_dir: PathBuf,
    nonce: String,
    /// The page that finished loading last, as classified.
    page: PageKind,
    fetch_pending: bool,
    links_pending: bool,
}

impl State {
    fn command(&mut self, command: Command) -> bool {
        match command {
            Command::Open { url } => {
                self.window.set_visible(true);
                self.window.set_focus();
                if let Err(error) = self.webview.load_url(&url) {
                    emit_error(format!("cannot open the page: {error}"));
                }
            }
            Command::Load { url } => {
                if let Err(error) = self.webview.load_url(&url) {
                    emit_error(format!("cannot open the page: {error}"));
                }
            }
            Command::Fetch { url } => {
                self.fetch_pending = true;
                if let Err(error) = self
                    .webview
                    .evaluate_script(&page::fetch_script(&self.nonce, &url))
                {
                    self.fetch_pending = false;
                    emit_error(format!("cannot run the request: {error}"));
                }
            }
            Command::Links => {
                self.links_pending = true;
                if let Err(error) = self
                    .webview
                    .evaluate_script(&page::links_script(&self.nonce))
                {
                    self.links_pending = false;
                    emit_error(format!("cannot read the page's links: {error}"));
                }
            }
            Command::ClearData => {
                if let Err(error) = self.webview.clear_all_browsing_data() {
                    emit_error(format!("cannot clear browsing data: {error}"));
                }
                if let Err(error) = empty_data_dir(&self.data_dir) {
                    emit_error(format!("cannot remove the window's data: {error}"));
                }
            }
            Command::Close => return false,
        }
        true
    }

    fn page_loaded(&mut self, url: &str) {
        let Some(url) = page::public_url(url) else {
            return;
        };
        self.page = classify_url(&url);
        // A page change cancels requests running in the previous page.
        self.fetch_pending = false;
        self.links_pending = false;
        emit(&Event::Navigated {
            url: url.to_string(),
        });
        if self.page == PageKind::AuthCodePage
            && let Err(error) = self
                .webview
                .evaluate_script(&page::page_text_script(&self.nonce))
        {
            emit_error(format!("cannot read the code page: {error}"));
        }
    }

    fn ipc(&mut self, body: &str) {
        match page::parse_message(body, &self.nonce) {
            Some(PageMessage::FetchResult { status, body }) if self.fetch_pending => {
                self.fetch_pending = false;
                emit(&Event::FetchResult { status, body });
            }
            Some(PageMessage::FetchError { message }) if self.fetch_pending => {
                self.fetch_pending = false;
                emit_error(format!("request failed: {message}"));
            }
            Some(PageMessage::Links(links)) if self.links_pending => {
                self.links_pending = false;
                emit(&Event::Links {
                    links: links
                        .into_iter()
                        .map(|(text, url)| Link {
                            text,
                            url: common::Secret::new(url),
                        })
                        .collect(),
                });
            }
            Some(PageMessage::PageText { text }) if self.page == PageKind::AuthCodePage => {
                match page::read_code_page(&text) {
                    CodePage::Code(code) => emit(&Event::AuthCode {
                        code: common::Secret::new(code),
                    }),
                    CodePage::NotSignedIn => {
                        emit_error("Epic shows no authorization code: not signed in");
                    }
                    CodePage::Unreadable => emit_error("the code page has an unexpected format"),
                }
            }
            // Stale, spoofed or unexpected: ignore without logging its content.
            _ => {}
        }
    }
}

fn run(data_dir: PathBuf) -> Result<std::convert::Infallible, String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let stdin_proxy = proxy.clone();
    std::thread::Builder::new()
        .name("unlauncher-web-stdin".to_owned())
        .spawn(move || read_stdin(&stdin_proxy))
        .map_err(|error| format!("cannot read commands: {error}"))?;

    let window = WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(tao::dpi::LogicalSize::new(1000.0, 780.0))
        .with_visible(false)
        .build(&event_loop)
        .map_err(|error| format!("cannot create the window: {error}"))?;

    let mut context = WebContext::new(Some(data_dir.clone()));
    let ipc_proxy = proxy.clone();
    let popup_proxy = proxy.clone();
    let navigation_proxy = proxy.clone();
    let load_proxy = proxy;
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url("about:blank")
        // The developer tools only in debug builds.
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |request| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(request.body().clone()));
        })
        // Never open other windows: report them (Epic's Google and Apple
        // buttons use popups) and let the interface decide.
        .with_new_window_req_handler(move |url, _features| {
            let _ = popup_proxy.send_event(UserEvent::Popup(url));
            wry::NewWindowResponse::Deny
        })
        // Only Epic's own sites: anything else is refused and
        // reported like a popup, so the interface can offer the browser.
        .with_navigation_handler(move |url| {
            if is_allowed_navigation(&url) {
                return true;
            }
            let _ = navigation_proxy.send_event(UserEvent::Popup(url));
            false
        })
        .with_on_page_load_handler(move |event, url| {
            if matches!(event, PageLoadEvent::Finished) {
                let _ = load_proxy.send_event(UserEvent::PageLoaded(url));
            }
        });
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let container = window
            .default_vbox()
            .ok_or_else(|| "the window has no GTK container".to_owned())?;
        builder
            .build_gtk(container)
            .map_err(|error| format!("cannot create the web view: {error}"))?
    };

    let mut state = State {
        window,
        webview,
        data_dir,
        nonce: page::new_nonce(),
        page: PageKind::Other,
        fetch_pending: false,
        links_pending: false,
    };
    emit(&Event::Ready);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let keep_running = match event {
            WindowEvent::UserEvent(UserEvent::Command(command)) => state.command(command),
            WindowEvent::UserEvent(UserEvent::StdinClosed)
            | WindowEvent::WindowEvent {
                event: WinEvent::CloseRequested,
                ..
            } => false,
            WindowEvent::UserEvent(UserEvent::PageLoaded(url)) => {
                state.page_loaded(&url);
                true
            }
            WindowEvent::UserEvent(UserEvent::Popup(url)) => {
                if let Some(url) = page::public_url(&url) {
                    emit(&Event::PopupBlocked {
                        url: url.to_string(),
                    });
                }
                true
            }
            WindowEvent::UserEvent(UserEvent::Ipc(body)) => {
                state.ipc(&body);
                true
            }
            _ => true,
        };
        if !keep_running {
            emit(&Event::Closed);
            *control_flow = ControlFlow::Exit;
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Option<PathBuf> {
        parse_args(list.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_the_data_dir() {
        assert_eq!(
            args(&["--data-dir", "/tmp/w"]),
            Some(PathBuf::from("/tmp/w"))
        );
        assert_eq!(args(&[]), None);
        assert_eq!(args(&["--data-dir"]), None);
        assert_eq!(args(&["--data-dir", ""]), None);
        assert_eq!(args(&["--data-dir", "/a", "extra"]), None);
        assert_eq!(args(&["--other", "/a"]), None);
    }

    #[test]
    fn data_dir_is_private_and_can_be_emptied() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("unlauncher/webview");
        prepare_data_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        std::fs::create_dir_all(dir.join("cookies")).unwrap();
        std::fs::write(dir.join("cookies/db.sqlite"), "x").unwrap();
        std::fs::write(dir.join("cache"), "x").unwrap();
        empty_data_dir(&dir).unwrap();
        assert!(dir.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}
