// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for disk usage, cache cleaning and uninstalling.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;

use common::Delete;
use support::fake_engine;
use tempfile::tempdir;
use ue_core::{
    EngineId, EngineMetadata, EngineRegistry, Error, caches, clean_cache, delete_project, dir_size,
    uninstall_engine,
};

#[test]
fn dir_size_sums_files_without_following_symlinks() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("a/b")).unwrap();
    fs::write(dir.path().join("a/one"), vec![0u8; 10]).unwrap();
    fs::write(dir.path().join("a/b/two"), vec![0u8; 5]).unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("big"), vec![0u8; 1000]).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("a/link")).unwrap();
    assert_eq!(dir_size(&dir.path().join("a")), 15);
    assert_eq!(dir_size(&dir.path().join("missing")), 0);
}

#[test]
fn lists_engine_and_shared_caches() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    fs::create_dir_all(engine.join("Engine/DerivedDataCache")).unwrap();
    fs::write(engine.join("Engine/DerivedDataCache/x.udd"), vec![0u8; 7]).unwrap();
    let found = caches(&engine, &dir.path().join("config"));
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].size, 7);
    assert!(!found[0].shared);
    assert!(found[1].shared && found[2].shared);
}

#[test]
fn clean_requires_confirmation_and_keeps_the_folder() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("DerivedDataCache");
    fs::create_dir_all(cache.join("sub")).unwrap();
    fs::write(cache.join("sub/a"), vec![0u8; 4]).unwrap();
    fs::write(cache.join("b"), vec![0u8; 6]).unwrap();

    assert!(matches!(
        clean_cache(&cache, Delete::No),
        Err(Error::DeleteNotConfirmed { .. })
    ));
    assert_eq!(dir_size(&cache), 10);
    assert_eq!(clean_cache(&cache, Delete::Yes).unwrap(), 10);
    assert!(cache.is_dir());
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    assert_eq!(
        clean_cache(&dir.path().join("missing"), Delete::Yes).unwrap(),
        0
    );
}

fn setup(dir: &std::path::Path, engine_path: &str) -> (EngineRegistry, EngineMetadata) {
    let ini = dir.join("Install.ini");
    fs::write(&ini, format!("[Installations]\nUE_5.7={engine_path}\n")).unwrap();
    (
        EngineRegistry::load(&ini).unwrap(),
        EngineMetadata::load(&dir.join("engines.toml")).unwrap(),
    )
}

#[test]
fn uninstall_deletes_engine_and_entry() {
    let dir = tempdir().unwrap();
    let engine = fake_engine(&dir.path().join("UE"), 5, 7, 4);
    let (mut registry, mut metadata) = setup(dir.path(), &engine.display().to_string());
    let id = EngineId::new("UE_5.7").unwrap();

    assert!(matches!(
        uninstall_engine(&mut registry, &mut metadata, &id, Delete::No),
        Err(Error::DeleteNotConfirmed { .. })
    ));
    assert!(engine.exists());

    uninstall_engine(&mut registry, &mut metadata, &id, Delete::Yes).unwrap();
    assert!(!engine.exists());
    let reloaded = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    assert!(reloaded.engines().is_empty() && reloaded.invalid_entries().is_empty());
    assert!(dir.path().join("Install.ini.bak").exists());
}

#[test]
fn uninstall_never_deletes_a_folder_that_is_not_an_engine() {
    let dir = tempdir().unwrap();
    let unrelated = dir.path().join("Documents");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("thesis.odt"), b"important").unwrap();
    let (mut registry, mut metadata) = setup(dir.path(), &unrelated.display().to_string());

    uninstall_engine(
        &mut registry,
        &mut metadata,
        &EngineId::new("UE_5.7").unwrap(),
        Delete::Yes,
    )
    .unwrap();
    assert!(unrelated.join("thesis.odt").exists());
}

#[test]
fn deletes_a_project_folder_after_confirmation() {
    let dir = tempdir().unwrap();
    let uproject = support::fake_project(&dir.path().join("Game"), "Game", "5.7");
    fs::create_dir_all(dir.path().join("Game/Content")).unwrap();

    assert!(matches!(
        delete_project(&uproject, Delete::No),
        Err(Error::DeleteNotConfirmed { .. })
    ));
    assert!(uproject.exists());
    assert_eq!(
        delete_project(&uproject, Delete::Yes).unwrap(),
        dir.path().join("Game")
    );
    assert!(!dir.path().join("Game").exists());
    assert!(dir.path().exists());
}

#[test]
fn never_deletes_a_folder_shared_by_several_projects() {
    let dir = tempdir().unwrap();
    let shared = dir.path().join("Projects");
    let a = support::fake_project(&shared, "A", "5.7");
    support::fake_project(&shared, "B", "5.7");
    assert!(matches!(
        delete_project(&a, Delete::Yes),
        Err(Error::NotOnlyThisProject { .. })
    ));
    assert!(a.exists());
}

#[test]
fn rejects_paths_that_are_not_projects() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), b"x").unwrap();
    assert!(matches!(
        delete_project(&dir.path().join("notes.txt"), Delete::Yes),
        Err(Error::UProjectMalformed { .. })
    ));
    assert!(dir.path().join("notes.txt").exists());
}
