// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub credentials the computer already has: the GitHub CLI (`gh`) and
//! git's credential helpers. Read without any prompt; their values are never
//! logged.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use common::Secret;
use system::{HostCommand, Sandbox};

/// How long each program may take (a credential helper may unlock a keyring).
const TIMEOUT: Duration = Duration::from_secs(15);

/// Where an existing credential comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingSource {
    /// `gh auth token`.
    GhCli,
    /// `git credential fill` for `https://github.com`.
    GitCredential,
}

impl ExistingSource {
    /// A name for the interface.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::GhCli => "GitHub CLI",
            Self::GitCredential => "git",
        }
    }
}

/// A GitHub token found on the computer.
#[derive(Clone)]
pub struct ExistingCredential {
    /// Where it was found.
    pub source: ExistingSource,
    /// The token.
    pub token: Secret,
}

impl fmt::Debug for ExistingCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExistingCredential")
            .field("source", &self.source)
            .field("token", &self.token)
            .finish()
    }
}

/// Looks for GitHub credentials with the host's `gh` and `git`, in that
/// order. Missing programs, no credentials or a failure just give fewer
/// results. Blocking: call it with `spawn_blocking` from async code.
#[must_use]
pub fn detect_existing(sandbox: Sandbox) -> Vec<ExistingCredential> {
    detect_with(sandbox, &PathBuf::from("gh"), &PathBuf::from("git"))
}

/// [`detect_existing`] with explicit programs (tests use fakes).
#[must_use]
pub fn detect_with(
    sandbox: Sandbox,
    gh: &std::path::Path,
    git: &std::path::Path,
) -> Vec<ExistingCredential> {
    let mut found = Vec::new();
    if let Some(token) = crate::gh::gh_token(sandbox, gh) {
        found.push(ExistingCredential {
            source: ExistingSource::GhCli,
            token,
        });
    }
    if let Some(token) = from_git(sandbox, git)
        && !found.iter().any(|c| c.token == token)
    {
        found.push(ExistingCredential {
            source: ExistingSource::GitCredential,
            token,
        });
    }
    tracing::debug!(count = found.len(), "existing GitHub credentials found");
    found
}

/// `git credential fill` for `https://github.com`, never prompting.
fn from_git(sandbox: Sandbox, git: &std::path::Path) -> Option<Secret> {
    let output = HostCommand::new(git)
        .args([
            "-c",
            "core.askPass=",
            "-c",
            "credential.interactive=never",
            "credential",
            "fill",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("GCM_INTERACTIVE", "never")
        .output_in(sandbox, b"protocol=https\nhost=github.com\n\n", TIMEOUT)
        .map_err(|error| tracing::debug!(%error, "git credential not usable"))
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let password = text
        .lines()
        .find_map(|line| line.strip_prefix("password="))?
        .trim();
    (!password.is_empty()).then(|| Secret::new(password))
}
