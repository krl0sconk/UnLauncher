// SPDX-License-Identifier: GPL-3.0-or-later

//! The GitHub CLI (`gh`): its token, and signing in with it.
//!
//! The EpicGames organization blocks OAuth Apps it has not approved, such as
//! UnLauncher's, but approves the GitHub CLI's. Signing in with `gh` (guided
//! by UnLauncher, without a terminal) gives a token that can read the engine
//! source.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::Secret;
use system::{HostCommand, Sandbox};

use crate::error::{Error, Result};

/// How long `gh --version` and `gh auth token` may take.
const QUICK: Duration = Duration::from_secs(15);

/// How long the sign-in may take (GitHub's device codes last 15 minutes).
const LOGIN_TIMEOUT: Duration = Duration::from_mins(15);

/// Whether the host has a working `gh`.
#[must_use]
pub fn gh_installed(sandbox: Sandbox, gh: &Path) -> bool {
    HostCommand::new(gh)
        .arg("--version")
        .output_in(sandbox, b"", QUICK)
        .is_ok_and(|output| output.status.success())
}

/// The token `gh` is signed in with for github.com, if any.
#[must_use]
pub fn gh_token(sandbox: Sandbox, gh: &Path) -> Option<Secret> {
    let output = HostCommand::new(gh)
        .args(["auth", "token", "--hostname", "github.com"])
        .env("GH_PROMPT_DISABLED", "1")
        .output_in(sandbox, b"", QUICK)
        .map_err(|error| tracing::debug!(%error, "gh not usable"))
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();
    (!token.is_empty() && !token.contains(char::is_whitespace)).then(|| Secret::new(token))
}

/// Signs `gh` in to github.com in the browser (`gh auth login --web`),
/// without a terminal.
///
/// `on_code` receives the one-time code `gh` prints, for the interface to
/// show (and copy); the user types it on `https://github.com/login/device`.
/// `gh` does not open the browser itself (the interface does). Blocks until
/// `gh` finishes, `cancel` becomes true or 15 minutes pass; call it with
/// `spawn_blocking` from async code. Afterwards [`gh_token`] gives the token.
///
/// # Errors
///
/// [`Error::GhLogin`] if `gh` cannot be started or fails,
/// [`Error::Cancelled`] if `cancel` was set, [`Error::Expired`] after 15
/// minutes.
pub fn gh_login(
    sandbox: Sandbox,
    gh: &Path,
    cancel: &AtomicBool,
    mut on_code: impl FnMut(&str),
) -> Result<()> {
    let mut command = HostCommand::new(gh)
        .args([
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--git-protocol",
            "https",
            "--web",
        ])
        // A no-op browser: UnLauncher opens the page itself.
        .env("GH_BROWSER", "true")
        .to_command(sandbox);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::GhLogin {
            message: format!("cannot start the GitHub CLI: {error}"),
        })?;

    // gh prints the code on stderr (or stdout): read both on threads.
    let (lines_tx, lines) = mpsc::channel::<String>();
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = lines_tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(pipe)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(lines_tx);
    let mut stdin = child.stdin.take();

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut last_line = String::new();
    let mut code_seen = false;
    let status = loop {
        while let Ok(line) = lines.try_recv() {
            if !code_seen && let Some(code) = one_time_code(&line) {
                code_seen = true;
                tracing::info!("GitHub CLI shows a one-time code");
                on_code(code);
                // "Press Enter to open github.com in your browser...".
                if let Some(stdin) = stdin.as_mut() {
                    let _ = stdin.write_all(b"\n");
                    let _ = stdin.flush();
                }
            }
            if !line.trim().is_empty() {
                last_line = line.trim().chars().take(200).collect();
            }
        }
        if let Some(status) = child.try_wait().map_err(|error| Error::GhLogin {
            message: error.to_string(),
        })? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.load(Ordering::Relaxed) {
                Error::Cancelled
            } else {
                Error::Expired
            });
        }
        thread::sleep(Duration::from_millis(50));
    };
    drop(stdin);
    // The reader threads may still hold gh's last words: wait for the pipes to
    // close (bounded, in case a grandchild keeps them open).
    let drain_until = Instant::now() + Duration::from_secs(2);
    while let Ok(line) = lines.recv_timeout(drain_until.saturating_duration_since(Instant::now())) {
        if !line.trim().is_empty() {
            last_line = line.trim().chars().take(200).collect();
        }
    }
    if status.success() {
        tracing::info!("GitHub CLI signed in");
        Ok(())
    } else {
        Err(Error::GhLogin { message: last_line })
    }
}

/// The `XXXX-XXXX` code in a line of `gh`'s output, e.g.
/// `! First copy your one-time code: 1A2B-3C4D`.
fn one_time_code(line: &str) -> Option<&str> {
    if !line.to_ascii_lowercase().contains("code") {
        return None;
    }
    line.split(|c: char| c.is_whitespace() || c == ':')
        .find(|word| {
            let bytes = word.as_bytes();
            bytes.len() == 9
                && bytes[4] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 4 || b.is_ascii_alphanumeric())
        })
}

#[cfg(test)]
mod tests {
    use super::one_time_code;

    #[test]
    fn finds_the_code() {
        assert_eq!(
            one_time_code("! First copy your one-time code: 1A2B-3C4D"),
            Some("1A2B-3C4D")
        );
        assert_eq!(
            one_time_code("Press Enter to open github.com in your browser..."),
            None
        );
        assert_eq!(one_time_code("code: 12345-678"), None);
    }
}
