// SPDX-License-Identifier: GPL-3.0-or-later

//! Detecting existing credentials with fake `gh` and `git` programs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use github_auth::{ExistingSource, detect_with};
use system::Sandbox;
use tempfile::TempDir;

/// Writes an executable `name` running `body`. Retries are not needed here:
/// each test writes its scripts before any process starts in it, but other
/// test threads may fork meanwhile (rust-lang/rust#114554), so the scripts
/// are written once per test and then run.
fn program(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.path().join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Runs `detect_with` until no script is "busy" (see `program`).
fn detect(gh: &Path, git: &Path) -> Vec<github_auth::ExistingCredential> {
    let mut last = Vec::new();
    for _ in 0..20 {
        last = detect_with(Sandbox::Host, gh, git);
        if !last.is_empty() {
            return last;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    last
}

const GH_OK: &str = r#"[ "$1 $2 $3 $4" = "auth token --hostname github.com" ] || exit 9
echo gho_from_gh"#;

const GIT_OK: &str = r#"input=$(cat)
case "$input" in *"host=github.com"*) ;; *) exit 9 ;; esac
[ "$GIT_TERMINAL_PROMPT" = "0" ] || exit 8
printf 'protocol=https\nhost=github.com\nusername=octo\npassword=ghp_from_git\n'"#;

#[test]
fn finds_gh_then_git() {
    let dir = tempfile::tempdir().unwrap();
    let gh = program(&dir, "gh", GH_OK);
    let git = program(&dir, "git", GIT_OK);
    let found = detect(&gh, &git);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].source, ExistingSource::GhCli);
    assert_eq!(found[0].token.expose(), "gho_from_gh");
    assert_eq!(found[1].source, ExistingSource::GitCredential);
    assert_eq!(found[1].token.expose(), "ghp_from_git");
    let debug = format!("{found:?}");
    assert!(
        !debug.contains("gho_from_gh") && !debug.contains("ghp_from_git"),
        "{debug}"
    );
}

#[test]
fn skips_missing_or_failing_programs() {
    let dir = tempfile::tempdir().unwrap();
    let git = program(&dir, "git", GIT_OK);
    let found = detect(&dir.path().join("no-gh"), &git);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source, ExistingSource::GitCredential);

    let gh = program(&dir, "gh2", "echo 'not logged in' >&2; exit 1");
    let git = program(&dir, "git2", "cat >/dev/null; exit 128");
    assert!(detect_with(Sandbox::Host, &gh, &git).is_empty());
}

#[test]
fn the_same_token_is_offered_once() {
    let dir = tempfile::tempdir().unwrap();
    let gh = program(&dir, "gh", "echo same_token");
    let git = program(
        &dir,
        "git",
        "cat >/dev/null; printf 'password=same_token\\n'",
    );
    let found = detect(&gh, &git);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source, ExistingSource::GhCli);
}
