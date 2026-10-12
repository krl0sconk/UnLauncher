// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for reading `Engine/Build/Build.version`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use support::fixture;
use ue_core::{EngineVersion, Error, read_build_version_file};

#[test]
fn reads_real_build_version() {
    let build = read_build_version_file(&fixture("build_version/ue_5_7_4.version")).unwrap();
    assert_eq!(build.version, EngineVersion::new(5, 7, 4));
    assert_eq!(build.branch_name.as_deref(), Some("++UE5+Release-5.7"));
    assert_eq!(build.changelist, Some(51_494_982));
}

#[test]
fn reads_build_version_without_optional_fields() {
    let build = read_build_version_file(&fixture("build_version/minimal.version")).unwrap();
    assert_eq!(build.version, EngineVersion::new(5, 3, 0));
    assert_eq!(build.branch_name, None);
    assert_eq!(build.changelist, None);
}

#[test]
fn rejects_malformed_json() {
    let path = fixture("build_version/malformed.version");
    let err = read_build_version_file(&path).unwrap_err();
    assert!(matches!(err, Error::BuildVersionMalformed { path: p, .. } if p == path));
}

#[test]
fn rejects_wrong_field_types() {
    let err = read_build_version_file(&fixture("build_version/wrong_type.version")).unwrap_err();
    assert!(matches!(err, Error::BuildVersionMalformed { .. }));
}

#[test]
fn missing_file_reports_its_path() {
    let path = fixture("build_version/does_not_exist.version");
    let err = read_build_version_file(&path).unwrap_err();
    assert!(matches!(err, Error::ReadFile { path: p, .. } if p == path));
}

#[test]
fn version_displays_as_dotted_triple() {
    assert_eq!(EngineVersion::new(5, 7, 4).to_string(), "5.7.4");
}
