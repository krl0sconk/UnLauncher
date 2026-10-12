// SPDX-License-Identifier: GPL-3.0-or-later

//! Talking to `unlauncher-web`, the embedded Epic web window.
//!
//! The window runs in its own process so the main interface never loads
//! WebKit. It reads one JSON [`Command`] per line on stdin and writes one JSON
//! [`Event`] per line on stdout.
//!
//! - [`protocol`]: the messages and their encoding.
//! - [`WebBridge`]: starts the window through
//!   [`system::HostCommand::bundled`] and talks to it: open a page, run a
//!   `fetch` inside it, wait for Epic's authorization code.
//! - [`classify_url`]: what kind of page the window is on (Epic login, the
//!   authorization-code page, a third-party login such as Google).
//! - [`is_allowed_navigation`]: where the window may go.
//! - [`clear_data`]: removes the window's cookies without starting it.

mod classify;
mod client;
mod error;
mod navigation;
pub mod protocol;

pub use classify::{PageKind, Provider, classify_url};
pub use client::{
    BridgeSender, FETCH_RETRY_INTERVAL, PROGRAM, READY_TIMEOUT, WebBridge, clear_data,
};
pub use error::{Error, Result};
pub use navigation::{ALLOWED_DOMAINS, is_allowed_navigation};
pub use protocol::{Command, Event, Link};
