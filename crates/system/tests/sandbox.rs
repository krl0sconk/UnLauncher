// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sandbox detection.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use system::Sandbox;
use tempfile::tempdir;

#[test]
fn detects_flatpak_when_marker_exists() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join(".flatpak-info");
    fs::write(
        &marker,
        "[Application]\nname=io.github.krl0sconk.UnLauncher\n",
    )
    .unwrap();
    assert_eq!(Sandbox::detect_at(&marker), Sandbox::Flatpak);
    assert!(Sandbox::detect_at(&marker).is_flatpak());
}

#[test]
fn detects_host_without_marker() {
    let dir = tempdir().unwrap();
    assert_eq!(
        Sandbox::detect_at(&dir.path().join(".flatpak-info")),
        Sandbox::Host
    );
}

#[test]
fn a_directory_is_not_the_marker() {
    let dir = tempdir().unwrap();
    assert_eq!(Sandbox::detect_at(dir.path()), Sandbox::Host);
}
