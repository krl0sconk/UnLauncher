// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for engine directory validation and discovery.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;

use support::{fake_engine, fake_engine_with_editor};
use tempfile::tempdir;
use ue_core::{
    Engine, EngineId, EngineKind, EngineVersion, Error, discover_engines, editor_executable,
    validate_engine_dir,
};

#[test]
fn validates_an_engine_directory() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    validate_engine_dir(&root).unwrap();
}

#[test]
fn accepts_ue4_editor_binary() {
    let dir = tempdir().unwrap();
    let root = fake_engine_with_editor(&dir.path().join("UE_4.27"), "UE4Editor", 4, 27, 2);
    validate_engine_dir(&root).unwrap();
}

#[test]
fn rejects_directory_without_editor() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("broken"), 5, 7, 4);
    fs::remove_file(root.join("Engine/Binaries/Linux/UnrealEditor")).unwrap();
    let err = validate_engine_dir(&root).unwrap_err();
    assert!(matches!(err, Error::NotAnEngine { path, .. } if path == root));
}

#[test]
fn rejects_missing_directory() {
    let dir = tempdir().unwrap();
    let err = validate_engine_dir(&dir.path().join("nope")).unwrap_err();
    assert!(matches!(err, Error::NotAnEngine { .. }));
}

#[test]
fn loads_engine_with_unknown_kind() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let engine = Engine::load(EngineId::new("UE_5.7").unwrap(), &root).unwrap();
    assert_eq!(engine.id.as_str(), "UE_5.7");
    assert_eq!(engine.version, EngineVersion::new(5, 7, 4));
    assert_eq!(engine.path, root);
    assert_eq!(engine.kind, EngineKind::Unknown);
}

#[test]
fn discovers_engines_up_to_max_depth() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let shallow = fake_engine(&base.join("UE_5.7"), 5, 7, 4);
    let nested = fake_engine(&base.join("engines/epic/UE_5.6"), 5, 6, 1);
    fake_engine(&base.join("a/b/c/d/UE_5.5"), 5, 5, 0);
    fs::create_dir_all(base.join("not_an_engine/Engine")).unwrap();

    let found = discover_engines(&[base.to_path_buf()], 3);
    let paths: Vec<_> = found.iter().map(|e| e.path.clone()).collect();
    assert_eq!(paths, vec![shallow, nested]);
    assert_eq!(found[0].version, EngineVersion::new(5, 7, 4));
}

#[test]
fn search_path_itself_can_be_an_engine() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let found = discover_engines(std::slice::from_ref(&root), 0);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, root);
}

#[test]
fn does_not_descend_into_engines() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    fake_engine(&root.join("Engine/Plugins/Inner"), 5, 7, 4);
    assert_eq!(discover_engines(&[dir.path().to_path_buf()], 5).len(), 1);
}

#[test]
fn ignores_missing_and_overlapping_search_paths() {
    let dir = tempdir().unwrap();
    fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let found = discover_engines(
        &[
            dir.path().join("missing"),
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
        ],
        2,
    );
    assert_eq!(found.len(), 1);
}

#[test]
fn editor_executable_prefers_unreal_editor() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    assert_eq!(
        editor_executable(&root).unwrap(),
        root.join("Engine/Binaries/Linux/UnrealEditor")
    );
    let engine = Engine::load(EngineId::new("UE_5.7").unwrap(), &root).unwrap();
    assert_eq!(
        engine.editor_path().unwrap(),
        root.join("Engine/Binaries/Linux/UnrealEditor")
    );
}

#[test]
fn editor_executable_finds_ue4_editor() {
    let dir = tempdir().unwrap();
    let root = fake_engine_with_editor(&dir.path().join("UE_4.27"), "UE4Editor", 4, 27, 2);
    assert_eq!(
        editor_executable(&root).unwrap(),
        root.join("Engine/Binaries/Linux/UE4Editor")
    );
}

#[test]
fn editor_executable_fails_without_editor() {
    let dir = tempdir().unwrap();
    let root = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let engine = Engine::load(EngineId::new("UE_5.7").unwrap(), &root).unwrap();
    fs::remove_file(root.join("Engine/Binaries/Linux/UnrealEditor")).unwrap();
    assert!(matches!(
        editor_executable(&root).unwrap_err(),
        Error::NotAnEngine { path, .. } if path == root
    ));
    assert!(matches!(
        engine.editor_path().unwrap_err(),
        Error::NotAnEngine { .. }
    ));
}
