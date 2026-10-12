// SPDX-License-Identifier: GPL-3.0-or-later

//! Host tools needed to build (`git`, `bash`).

use std::process::Stdio;
use std::time::Duration;

use system::{HostCommand, Sandbox};

use super::{CheckResult, CheckStatus};

const TIMEOUT: Duration = Duration::from_secs(15);

/// Runs `<tool> --version` on the host and reports its first output line.
pub(super) async fn check_tool(tool: &str, sandbox: Sandbox) -> CheckResult {
    let name = format!("Tool: {tool}");
    let mut command =
        tokio::process::Command::from(HostCommand::new(tool).arg("--version").to_command(sandbox));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let missing = |detail: String| {
        CheckResult::new(
            &name,
            CheckStatus::Fail,
            format!("{tool} was not found or did not run: {detail}"),
            Some(format!(
                "Install {tool} with your distribution's package manager \
                 (for example `sudo apt install {tool}`, `sudo dnf install {tool}` or \
                 `sudo pacman -S {tool}`)."
            )),
        )
    };
    match tokio::time::timeout(TIMEOUT, command.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let version = stdout.lines().next().unwrap_or("").trim().to_owned();
            CheckResult::new(&name, CheckStatus::Ok, version, None)
        }
        Ok(Ok(output)) => missing(format!("exited with {}", output.status)),
        Ok(Err(error)) => missing(error.to_string()),
        Err(_) => missing("timed out".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finds_bash() {
        let r = check_tool("bash", Sandbox::Host).await;
        assert_eq!(r.status, CheckStatus::Ok, "{r:?}");
        assert!(r.message.contains("bash"));
    }

    #[tokio::test]
    async fn missing_tool_fails_with_hint() {
        let r = check_tool("unlauncher-no-such-tool", Sandbox::Host).await;
        assert_eq!(r.status, CheckStatus::Fail);
        assert!(r.hint.is_some());
    }
}
