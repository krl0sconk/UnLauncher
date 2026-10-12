// SPDX-License-Identifier: GPL-3.0-or-later

//! Signing in with a fake GitHub CLI.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use github_auth::{Error, gh_installed, gh_login};
use system::Sandbox;
use tempfile::TempDir;

fn program(dir: &TempDir, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.path().join("gh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Runs `gh_login`, retrying while the script is "busy": another test thread
/// may fork while it is written (rust-lang/rust#114554).
fn login(gh: &Path, cancel: &AtomicBool) -> (Result<(), Error>, Vec<String>) {
    for _ in 0..50 {
        let mut codes = Vec::new();
        let result = gh_login(Sandbox::Host, gh, cancel, |code| {
            codes.push(code.to_owned());
        });
        match &result {
            Err(Error::GhLogin { message }) if message.contains("busy") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => return (result, codes),
        }
    }
    panic!("the fake gh stayed busy");
}

/// Like `gh auth login --web`: prints the code, waits for Enter.
const GH_OK: &str = r#"[ "$1 $2 $7" = "auth login --web" ] || { echo "bad args: $*" >&2; exit 9; }
[ "$GH_BROWSER" = "true" ] || exit 8
echo '! First copy your one-time code: 1A2B-3C4D' >&2
echo 'Press Enter to open https://github.com/login/device in your browser... ' >&2
read line
echo '✓ Authentication complete.' >&2
echo '✓ Logged in as octo' >&2"#;

#[test]
fn reports_the_code_and_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let gh = program(&dir, GH_OK);
    let (result, codes) = login(&gh, &AtomicBool::new(false));
    result.unwrap();
    assert_eq!(codes, ["1A2B-3C4D"]);
}

#[test]
fn reports_a_failure_with_gh_s_last_words() {
    let dir = tempfile::tempdir().unwrap();
    let gh = program(&dir, "echo 'error connecting to github.com' >&2; exit 1");
    let (result, _) = login(&gh, &AtomicBool::new(false));
    assert!(
        matches!(&result, Err(Error::GhLogin { message }) if message.contains("error connecting")),
        "{result:?}"
    );
}

#[test]
fn can_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let gh = program(&dir, "echo 'code: AAAA-BBBB' >&2; sleep 30");
    let (result, _) = login(&gh, &AtomicBool::new(true));
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
}

#[test]
fn a_missing_gh_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-gh");
    assert!(!gh_installed(Sandbox::Host, &missing));
    let result = gh_login(Sandbox::Host, &missing, &AtomicBool::new(false), |_| {});
    assert!(matches!(result, Err(Error::GhLogin { .. })));
}
