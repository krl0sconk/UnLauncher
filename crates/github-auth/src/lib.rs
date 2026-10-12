// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub access to the private `EpicGames/UnrealEngine` repository without
//! asking the user to create or paste a token.
//!
//! - [`DeviceFlow`]: OAuth Device Flow with UnLauncher's OAuth App
//!   ([`CLIENT_ID`], no client secret): the user types a short code on
//!   `github.com/login/device` and UnLauncher receives a token.
//! - [`detect_existing`]: credentials the computer already has (`gh`, git's
//!   credential helpers), read without any prompt.
//! - [`gh_login`]: signs the GitHub CLI in through the browser, guided by
//!   UnLauncher; EpicGames approves the CLI's OAuth App, unlike UnLauncher's.
//! - [`check_access`]: whether a token can read the repository, and if not,
//!   what is missing ([`Access`]).
//!
//! Tokens are always [`common::Secret`]s and are never logged. This crate does
//! not store them: callers keep them in the system keyring.

mod access;
mod device;
mod error;
mod existing;
mod gh;

pub use access::{Access, GITHUB_API_BASE, UE_ORG, UE_REPO, check_access};
pub use device::{CLIENT_ID, DeviceCode, DeviceFlow, GITHUB_WEB_BASE, SCOPE};
pub use error::{Error, Result};
pub use existing::{ExistingCredential, ExistingSource, detect_existing, detect_with};
pub use gh::{gh_installed, gh_login, gh_token};
