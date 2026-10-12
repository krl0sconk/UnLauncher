// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the new-version notice.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ue_core::EngineVersion;
use ue_prebuilt::newer_than_installed;

fn list(versions: &[&str]) -> Vec<String> {
    versions.iter().map(|v| (*v).to_owned()).collect()
}

#[test]
fn offers_a_newer_minor_version() {
    let versions = list(&["5.5", "5.6", "5.7", "5.8"]);
    let installed = [EngineVersion::new(5, 7, 4), EngineVersion::new(5, 6, 1)];
    assert_eq!(
        newer_than_installed(&versions, &installed),
        Some("5.8".to_owned())
    );
}

#[test]
fn nothing_when_the_newest_is_installed() {
    let versions = list(&["5.6", "5.7"]);
    assert_eq!(
        newer_than_installed(&versions, &[EngineVersion::new(5, 7, 0)]),
        None
    );
    // A newer patch of the same minor is not a new version for Fab.
    assert_eq!(
        newer_than_installed(&versions, &[EngineVersion::new(5, 7, 4)]),
        None
    );
}

#[test]
fn nothing_when_an_installed_engine_is_newer() {
    let versions = list(&["5.6", "5.7"]);
    assert_eq!(
        newer_than_installed(&versions, &[EngineVersion::new(5, 8, 0)]),
        None
    );
}

#[test]
fn offers_the_newest_when_nothing_is_installed() {
    assert_eq!(
        newer_than_installed(&list(&["5.7", "5.10", "5.9"]), &[]),
        Some("5.10".to_owned())
    );
}

#[test]
fn accepts_prefixed_names_and_skips_junk() {
    let versions = list(&["UE_5.6", "UE 5.8", "4.27", "latest", "", "5", "UE_5.7.4"]);
    assert_eq!(
        newer_than_installed(&versions, &[EngineVersion::new(5, 7, 4)]),
        Some("5.8".to_owned())
    );
}

#[test]
fn nothing_from_an_empty_or_unreadable_list() {
    assert_eq!(newer_than_installed(&[], &[]), None);
    assert_eq!(newer_than_installed(&list(&["latest"]), &[]), None);
}
