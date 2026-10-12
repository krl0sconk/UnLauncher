// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting `unlauncher-web` and talking to it.

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Secret;
use system::{HostCommand, Sandbox};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};
use url::Url;

use crate::error::{Error, Result};
use crate::protocol::{Command, Event, Link, decode_event, encode_command};

/// Name of the web window's binary, installed next to `unlauncher`.
pub const PROGRAM: &str = "unlauncher-web";

/// How long the window may take to start.
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// How long [`WebBridge::fetch_json`] waits for a new page before retrying.
pub const FETCH_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// How long [`WebBridge::close`] waits for the window to exit before killing
/// it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Lines of the window's stderr kept to explain a failed start.
const STDERR_TAIL: usize = 20;

/// Sends commands to a running window. Cheap to clone; usable from another
/// task while the owner of the [`WebBridge`] waits for events.
#[derive(Debug, Clone)]
pub struct BridgeSender {
    tx: mpsc::UnboundedSender<Command>,
}

impl BridgeSender {
    /// Queues `command` for the window.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn send(&self, command: Command) -> Result<()> {
        self.tx.send(command).map_err(|_| Error::Closed)
    }

    /// Shows the window and loads `url`.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn open(&self, url: &str) -> Result<()> {
        self.send(Command::Open {
            url: url.to_owned(),
        })
    }

    /// Loads `url` without showing the window.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn load(&self, url: &str) -> Result<()> {
        self.send(Command::Load {
            url: url.to_owned(),
        })
    }

    /// Asks the window to close.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is already gone.
    pub fn close(&self) -> Result<()> {
        self.send(Command::Close)
    }
}

/// A running `unlauncher-web` window.
///
/// Dropping it kills the window process.
#[derive(Debug)]
pub struct WebBridge {
    child: Child,
    sender: BridgeSender,
    events: mpsc::UnboundedReceiver<Event>,
    /// The page that finished loading last.
    loaded: Option<Url>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_task: Option<JoinHandle<()>>,
}

impl WebBridge {
    /// Starts `unlauncher-web` (found next to the running executable) with its
    /// cookies in `data_dir`, and waits until it is ready.
    ///
    /// # Errors
    ///
    /// - [`Error::Spawn`] if `unlauncher-web` is not installed.
    /// - [`Error::Io`] if it cannot be started.
    /// - [`Error::NotStarted`] if it exits before it is ready (for example,
    ///   WebKitGTK is missing).
    /// - [`Error::Timeout`] if it is not ready within [`READY_TIMEOUT`].
    pub async fn spawn(data_dir: &Path) -> Result<Self> {
        Self::spawn_with(HostCommand::bundled(PROGRAM)?, data_dir).await
    }

    /// [`WebBridge::spawn`] with an explicit command, for tests.
    ///
    /// # Errors
    ///
    /// As [`WebBridge::spawn`].
    pub async fn spawn_with(command: HostCommand, data_dir: &Path) -> Result<Self> {
        let mut std_command = command
            .arg("--data-dir")
            .arg(data_dir)
            .to_command(Sandbox::detect());
        std_command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut command = tokio::process::Command::from(std_command);
        command.kill_on_drop(true);
        let mut child = command.spawn().map_err(|source| Error::Io {
            action: "start",
            source,
        })?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(Error::Io {
                action: "start",
                source: io::Error::other("missing pipes"),
            });
        };

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        tokio::spawn(write_commands(stdin, command_rx));
        let (event_tx, events) = mpsc::unbounded_channel();
        tokio::spawn(read_events(stdout, event_tx));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_task = tokio::spawn(read_stderr(stderr, Arc::clone(&stderr_tail)));

        let mut bridge = Self {
            child,
            sender: BridgeSender { tx: command_tx },
            events,
            loaded: None,
            stderr_tail,
            stderr_task: Some(stderr_task),
        };
        bridge.wait_ready().await?;
        tracing::debug!("web window ready");
        Ok(bridge)
    }

    async fn wait_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            match timeout_at(deadline, self.next_event()).await {
                Err(_) => {
                    return Err(Error::Timeout {
                        what: "the web window to start",
                    });
                }
                Ok(None) => return Err(self.not_started().await),
                Ok(Some(Event::Ready)) => return Ok(()),
                Ok(Some(_)) => {}
            }
        }
    }

    /// The error for a window that exited before being ready, with the last
    /// line it wrote on stderr (a missing library, for example).
    async fn not_started(&mut self) -> Error {
        if let Some(task) = self.stderr_task.take() {
            let _ = timeout(Duration::from_secs(1), task).await;
        }
        let detail = self
            .stderr_tail
            .lock()
            .ok()
            .and_then(|tail| tail.back().cloned());
        tracing::warn!(
            detail = detail.as_deref().unwrap_or(""),
            "web window failed to start"
        );
        Error::NotStarted { detail }
    }

    /// A handle to send commands from elsewhere.
    #[must_use]
    pub fn sender(&self) -> BridgeSender {
        self.sender.clone()
    }

    /// Shows the window and loads `url`.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn open(&self, url: &str) -> Result<()> {
        self.sender.open(url)
    }

    /// Loads `url` without showing the window.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn load(&self, url: &str) -> Result<()> {
        self.sender.load(url)
    }

    /// Asks the window to delete its cookies and browsing data.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window is gone.
    pub fn clear_data(&self) -> Result<()> {
        self.sender.send(Command::ClearData)
    }

    /// The next event from the window, or `None` once it has exited.
    pub async fn next_event(&mut self) -> Option<Event> {
        let event = self.events.recv().await?;
        if let Event::Navigated { url } = &event {
            self.loaded = Url::parse(url).ok();
        }
        Some(event)
    }

    /// The page that finished loading last, if any.
    #[must_use]
    pub fn loaded_page(&self) -> Option<&Url> {
        self.loaded.as_ref()
    }

    /// The next event before `deadline`; [`Event::Closed`] and the end of the
    /// process are errors.
    async fn next_before(&mut self, deadline: Instant, what: &'static str) -> Result<Event> {
        match timeout_at(deadline, self.next_event()).await {
            Err(_) => Err(Error::Timeout { what }),
            Ok(None | Some(Event::Closed)) => Err(Error::Closed),
            Ok(Some(event)) => Ok(event),
        }
    }

    fn is_loaded(&self, host: &str) -> bool {
        self.loaded
            .as_ref()
            .and_then(Url::host_str)
            .is_some_and(|h| h.eq_ignore_ascii_case(host))
    }

    /// Runs `fetch(url)` inside the page once a page of `host` has loaded, and
    /// returns the body when it is HTTP 200 and valid JSON.
    ///
    /// Until then (Cloudflare's check, a web session that is not signed in
    /// yet) it waits for the next page load, or [`FETCH_RETRY_INTERVAL`], and
    /// tries again. The window stays visible so the user can sign in.
    ///
    /// # Errors
    ///
    /// - [`Error::Closed`] if the user closes the window.
    /// - [`Error::NotJson`] if the time runs out after the page answered
    ///   without JSON, [`Error::Timeout`] if it never answered.
    pub async fn fetch_json(&mut self, url: &str, host: &str, limit: Duration) -> Result<String> {
        const WHAT: &str = "the web page";
        let deadline = Instant::now() + limit;
        let mut last_status = None;
        loop {
            while !self.is_loaded(host) {
                if let Err(error) = self.next_before(deadline, WHAT).await {
                    return Err(timeout_status(error, last_status));
                }
            }
            self.sender.send(Command::Fetch {
                url: url.to_owned(),
            })?;
            let reloaded = loop {
                match self.next_before(deadline, WHAT).await {
                    Err(error) => return Err(timeout_status(error, last_status)),
                    Ok(Event::FetchResult { status, body }) => {
                        if status == 200
                            && serde_json::from_str::<serde::de::IgnoredAny>(&body).is_ok()
                        {
                            return Ok(body);
                        }
                        tracing::debug!(status, "page fetch not ready yet");
                        last_status = Some(status);
                        break false;
                    }
                    Ok(Event::Error { message }) => {
                        tracing::debug!(%message, "page fetch failed");
                        break false;
                    }
                    // The page changed under the fetch: try on the new one.
                    Ok(Event::Navigated { .. }) => break true,
                    Ok(_) => {}
                }
            };
            if reloaded {
                continue;
            }
            let wait_until = deadline.min(Instant::now() + FETCH_RETRY_INTERVAL);
            loop {
                match self.next_before(wait_until, WHAT).await {
                    Ok(Event::Navigated { .. }) => break,
                    Ok(_) => {}
                    Err(Error::Timeout { .. }) if Instant::now() < deadline => break,
                    Err(error) => return Err(timeout_status(error, last_status)),
                }
            }
        }
    }

    /// Waits until a page of `host` has loaded, then reads its links.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window closes, [`Error::Timeout`] after
    /// `limit`.
    pub async fn page_links(&mut self, host: &str, limit: Duration) -> Result<Vec<Link>> {
        const WHAT: &str = "the page's links";
        let deadline = Instant::now() + limit;
        loop {
            while !self.is_loaded(host) {
                self.next_before(deadline, WHAT).await?;
            }
            self.sender.send(Command::Links)?;
            loop {
                match self.next_before(deadline, WHAT).await? {
                    Event::Links { links } => return Ok(links),
                    // The page changed under the request: ask the new one.
                    Event::Navigated { .. } => break,
                    Event::Error { message } => {
                        tracing::debug!(%message, "reading the page's links failed");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Waits for the next page load (for example, after the user signs in on
    /// the page) and returns its URL.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the window closes first, [`Error::Timeout`] after
    /// `limit`.
    pub async fn wait_navigation(&mut self, limit: Duration) -> Result<Url> {
        let deadline = Instant::now() + limit;
        loop {
            if let Event::Navigated { url } = self.next_before(deadline, "the next page").await?
                && let Ok(url) = Url::parse(&url)
            {
                return Ok(url);
            }
        }
    }

    /// Waits until the window reads Epic's authorization-code page.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the user closes the window first.
    pub async fn wait_auth_code(&mut self) -> Result<Secret> {
        loop {
            match self.next_event().await {
                None | Some(Event::Closed) => return Err(Error::Closed),
                Some(Event::AuthCode { code }) => return Ok(code),
                Some(Event::Error { message }) => {
                    tracing::debug!(%message, "web window error while signing in");
                }
                Some(_) => {}
            }
        }
    }

    /// Closes the window and waits for it to exit (killing it after a few
    /// seconds).
    pub async fn close(mut self) {
        let _ = self.sender.close();
        if timeout(CLOSE_TIMEOUT, self.child.wait()).await.is_err() {
            tracing::warn!("web window did not exit; killing it");
            let _ = self.child.kill().await;
        }
    }
}

/// A timeout after the page answered without JSON is reported as such.
fn timeout_status(error: Error, last_status: Option<u16>) -> Error {
    match (error, last_status) {
        (Error::Timeout { .. }, Some(status)) => Error::NotJson { status },
        (error, _) => error,
    }
}

async fn write_commands(mut stdin: ChildStdin, mut commands: mpsc::UnboundedReceiver<Command>) {
    while let Some(command) = commands.recv().await {
        let mut line = encode_command(&command);
        line.push('\n');
        if let Err(error) = stdin.write_all(line.as_bytes()).await {
            tracing::debug!(%error, "web window stdin closed");
            return;
        }
        let _ = stdin.flush().await;
    }
}

async fn read_events(stdout: tokio::process::ChildStdout, events: mpsc::UnboundedSender<Event>) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(event) = decode_event(&line) {
            if events.send(event).is_err() {
                return;
            }
        } else {
            // Never log the line itself: it could hold a code.
            tracing::warn!(
                len = line.len(),
                "ignoring an unknown message from the web window"
            );
        }
    }
}

async fn read_stderr(stderr: tokio::process::ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(mut tail) = tail.lock() {
            if tail.len() == STDERR_TAIL {
                tail.pop_front();
            }
            tail.push_back(line.chars().take(300).collect());
        }
    }
}

/// Deletes the web window's data folder (cookies, cache), for example after
/// signing out, without starting the window. A missing folder is fine.
///
/// The folder belongs to UnLauncher (`app_data_dir()/webview`), so no
/// confirmation is needed.
///
/// # Errors
///
/// [`Error::ClearData`] if the folder exists and cannot be removed.
pub fn clear_data(data_dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(data_dir) {
        Ok(()) => {
            tracing::info!(path = %data_dir.display(), "web window data removed");
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::ClearData {
            path: data_dir.to_path_buf(),
            source,
        }),
    }
}
