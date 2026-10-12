// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `EngineRegistry`: loading, registering and saving `Install.ini`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;
use std::path::Path;

use common::Overwrite;
use support::{fake_engine, read_fixture};
use tempfile::tempdir;
use ue_core::{EngineId, EngineRegistry, Error};

fn write_ini(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("Install.ini");
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn missing_file_loads_as_empty_registry() {
    let dir = tempdir().unwrap();
    let registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    assert_eq!(registry.engines().len(), 0);
    assert_eq!(registry.invalid_entries().len(), 0);
}

#[test]
fn separates_valid_and_invalid_entries() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let ini = write_ini(
        dir.path(),
        &format!(
            "[Installations]\nUE_5.7={}\nUE_5.1=/does/not/exist\n",
            engine.display()
        ),
    );
    let registry = EngineRegistry::load(&ini).unwrap();
    assert_eq!(registry.engines().len(), 1);
    assert_eq!(registry.engines()[0].id.as_str(), "UE_5.7");
    assert_eq!(registry.invalid_entries().len(), 1);
    assert_eq!(registry.invalid_entries()[0].id.as_str(), "UE_5.1");
}

#[test]
fn register_uses_major_minor_id_by_default() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    let id = registry.register(&engine, None).unwrap().id.clone();
    assert_eq!(id.as_str(), "UE_5.7");
}

#[test]
fn register_generates_guid_when_default_id_is_taken() {
    let dir = tempdir().unwrap();
    let a = fake_engine(&dir.path().join("a"), 5, 7, 4);
    let b = fake_engine(&dir.path().join("b"), 5, 7, 3);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    registry.register(&a, None).unwrap();
    let id = registry.register(&b, None).unwrap().id.clone();
    let id = id.as_str();
    assert!(id.starts_with('{') && id.ends_with('}'), "{id}");
    assert_eq!(id.len(), 38, "{id}");
    assert_eq!(id, id.to_uppercase());
}

#[test]
fn register_with_explicit_id() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    let id = EngineId::new("MyFork").unwrap();
    assert_eq!(registry.register(&engine, Some(id.clone())).unwrap().id, id);
}

#[test]
fn register_rejects_taken_explicit_id() {
    let dir = tempdir().unwrap();
    let a = fake_engine(&dir.path().join("a"), 5, 7, 4);
    let b = fake_engine(&dir.path().join("b"), 5, 6, 0);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    registry.register(&a, None).unwrap();
    let err = registry
        .register(&b, Some(EngineId::new("UE_5.7").unwrap()))
        .unwrap_err();
    assert!(matches!(err, Error::EngineIdTaken { .. }));
}

#[test]
fn register_rejects_path_already_registered() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    registry.register(&engine, None).unwrap();
    let err = registry
        .register(&engine, Some(EngineId::new("Other").unwrap()))
        .unwrap_err();
    assert!(matches!(err, Error::EngineAlreadyRegistered { id, .. } if id.as_str() == "UE_5.7"));
}

#[test]
fn register_rejects_non_engine_directory() {
    let dir = tempdir().unwrap();
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    let err = registry.register(dir.path(), None).unwrap_err();
    assert!(matches!(err, Error::NotAnEngine { .. }));
}

#[test]
fn engine_id_rejects_characters_that_break_install_ini() {
    for bad in ["", "  ", "a=b", "a\nb", "[x]", ";comment", "#x"] {
        assert!(
            matches!(EngineId::new(bad), Err(Error::InvalidEngineId { .. })),
            "{bad:?}"
        );
    }
}

#[test]
fn unregister_removes_valid_and_invalid_entries() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE_5.7"), 5, 7, 4);
    let ini = write_ini(
        dir.path(),
        &format!(
            "[Installations]\nUE_5.7={}\nUE_5.1=/gone\n",
            engine.display()
        ),
    );
    let mut registry = EngineRegistry::load(&ini).unwrap();
    assert_eq!(
        registry
            .unregister(&EngineId::new("UE_5.7").unwrap())
            .unwrap(),
        engine
    );
    assert_eq!(
        registry
            .unregister(&EngineId::new("UE_5.1").unwrap())
            .unwrap(),
        Path::new("/gone")
    );
    assert_eq!(registry.engines().len(), 0);
    assert_eq!(registry.invalid_entries().len(), 0);
}

#[test]
fn unregister_unknown_id_fails() {
    let dir = tempdir().unwrap();
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    let err = registry
        .unregister(&EngineId::new("UE_9.9").unwrap())
        .unwrap_err();
    assert!(matches!(err, Error::EngineNotRegistered { .. }));
}

#[test]
fn save_refuses_to_overwrite_without_confirmation() {
    let dir = tempdir().unwrap();
    let original = read_fixture("install_ini/with_extras.ini");
    let ini = write_ini(dir.path(), &original);
    let engine = fake_engine(&dir.path().join("UE"), 5, 8, 0);
    let mut registry = EngineRegistry::load(&ini).unwrap();
    registry.register(&engine, None).unwrap();

    let err = registry.save(Overwrite::No).unwrap_err();
    assert!(matches!(err, Error::OverwriteNotConfirmed { .. }));
    assert_eq!(fs::read_to_string(&ini).unwrap(), original);
    assert!(!dir.path().join("Install.ini.bak").exists());
}

#[test]
fn save_creates_missing_file_and_parent_directories() {
    let dir = tempdir().unwrap();
    let ini = dir.path().join("config/Epic/UnrealEngine/Install.ini");
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    let mut registry = EngineRegistry::load(&ini).unwrap();
    registry.register(&engine, None).unwrap();

    assert_eq!(registry.save(Overwrite::No).unwrap(), None);
    let expected = format!("[Installations]\nUE_5.7={}\n", engine.display());
    assert_eq!(fs::read_to_string(&ini).unwrap(), expected);
}

#[test]
fn round_trip_preserves_unknown_content_and_backs_up() {
    let dir = tempdir().unwrap();
    let original = read_fixture("install_ini/with_extras.ini");
    let ini = write_ini(dir.path(), &original);
    let engine = fake_engine(&dir.path().join("UE"), 5, 8, 0);

    // Register and save: every original line survives and a backup is made.
    let mut registry = EngineRegistry::load(&ini).unwrap();
    registry.register(&engine, None).unwrap();
    let backup = registry.save(Overwrite::Yes).unwrap().unwrap();
    assert_eq!(backup, dir.path().join("Install.ini.bak"));
    assert_eq!(fs::read_to_string(&backup).unwrap(), original);
    let saved = fs::read_to_string(&ini).unwrap();
    for line in original.lines() {
        assert!(saved.lines().any(|l| l == line), "lost line {line:?}");
    }

    // Reload sees the new engine.
    let mut registry = EngineRegistry::load(&ini).unwrap();
    assert!(registry.engines().iter().any(|e| e.id.as_str() == "UE_5.8"));

    // Unregister and save again: the file is byte-identical to the original.
    registry
        .unregister(&EngineId::new("UE_5.8").unwrap())
        .unwrap();
    registry.save(Overwrite::Yes).unwrap();
    assert_eq!(fs::read_to_string(&ini).unwrap(), original);
}

#[test]
fn save_without_changes_is_byte_identical() {
    let dir = tempdir().unwrap();
    let original = read_fixture("install_ini/crlf.ini");
    let ini = write_ini(dir.path(), &original);
    EngineRegistry::load(&ini)
        .unwrap()
        .save(Overwrite::Yes)
        .unwrap();
    assert_eq!(fs::read_to_string(&ini).unwrap(), original);
}
