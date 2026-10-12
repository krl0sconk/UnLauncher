// SPDX-License-Identifier: GPL-3.0-or-later

//! Pre-flight checks before cloning and building.

mod github;
mod resources;
mod tools;

use std::path::PathBuf;

use common::Secret;
use serde::{Deserialize, Serialize};
use system::Sandbox;

pub use github::{GITHUB_API_BASE, UE_REPO};
pub use resources::{
    DISK_FAIL_BYTES, DISK_WARN_BYTES, RAM_WARN_BYTES, disk_status, nearest_existing_ancestor,
    ram_status,
};

/// The outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CheckStatus {
    /// Fine.
    Ok,
    /// Building may work but is risky or slow.
    Warn,
    /// Building will not work until this is fixed.
    Fail,
}

/// The result of one pre-flight check, ready to show to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    /// Short name of the check, e.g. `"Disk space"`.
    pub name: String,
    /// The outcome.
    pub status: CheckStatus,
    /// What was found.
    pub message: String,
    /// What the user can do, when the status is not [`CheckStatus::Ok`].
    pub hint: Option<String>,
}

impl CheckResult {
    pub(crate) fn new(
        name: &str,
        status: CheckStatus,
        message: impl Into<String>,
        hint: Option<String>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            status,
            message: message.into(),
            hint,
        }
    }
}

/// What [`preflight`] checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightInput {
    /// Where the engine source will be cloned (it may not exist yet).
    pub destination: PathBuf,
    /// The kernel's memory report, normally [`crate::PROC_MEMINFO`].
    pub meminfo_path: PathBuf,
    /// Base URL of the GitHub API, normally [`GITHUB_API_BASE`] (tests inject
    /// a local server).
    pub github_api_base: String,
    /// Host programs that must be installed, checked with `<tool> --version`.
    pub tools: Vec<String>,
}

impl PreflightInput {
    /// The standard checks for cloning and building into `destination`.
    #[must_use]
    pub fn new(destination: impl Into<PathBuf>) -> Self {
        Self {
            destination: destination.into(),
            meminfo_path: PathBuf::from(crate::PROC_MEMINFO),
            github_api_base: GITHUB_API_BASE.to_owned(),
            tools: vec!["git".to_owned(), "bash".to_owned()],
        }
    }
}

/// Runs every pre-flight check: free disk at the destination, RAM, access to
/// `EpicGames/UnrealEngine` with `github_token`, and the host tools.
///
/// Never fails: problems are reported as [`CheckStatus::Fail`] results.
/// `http` is the shared client (with UnLauncher's `User-Agent`), which GitHub
/// requires. Results come in a fixed order: disk, RAM, GitHub, then one per
/// tool.
pub async fn preflight(
    input: &PreflightInput,
    sandbox: Sandbox,
    http: &reqwest::Client,
    github_token: Option<&Secret>,
) -> Vec<CheckResult> {
    let mut results = vec![
        resources::check_disk(&input.destination).await,
        resources::check_ram(&input.meminfo_path).await,
        github::check_github(http, &input.github_api_base, github_token).await,
    ];
    for tool in &input.tools {
        results.push(tools::check_tool(tool, sandbox).await);
    }
    for result in &results {
        tracing::debug!(check = %result.name, status = ?result.status, message = %result.message, "pre-flight check");
    }
    results
}

/// The worst status among `results` (`Ok` when empty).
#[must_use]
pub fn overall_status(results: &[CheckResult]) -> CheckStatus {
    results
        .iter()
        .map(|r| r.status)
        .max()
        .unwrap_or(CheckStatus::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_is_the_worst() {
        let r = |status| CheckResult::new("x", status, "", None);
        assert_eq!(overall_status(&[]), CheckStatus::Ok);
        assert_eq!(
            overall_status(&[r(CheckStatus::Ok), r(CheckStatus::Warn)]),
            CheckStatus::Warn
        );
        assert_eq!(
            overall_status(&[r(CheckStatus::Fail), r(CheckStatus::Warn)]),
            CheckStatus::Fail
        );
    }
}
