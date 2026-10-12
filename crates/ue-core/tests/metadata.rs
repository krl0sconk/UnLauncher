// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `EngineMetadata`: how each engine was installed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;

use support::fake_engine;
use tempfile::tempdir;
use ue_core::{EngineKind, EngineMetadata, EngineRegistry, Error};

fn source_kind() -> EngineKind {
    EngineKind::Source {
        remote: "https://github.com/EpicGames/UnrealEngine.git".into(),
        git_ref: "5.7".into(),
    }
}

#[test]
fn missing_file_is_empty_metadata() {
    let dir = tempdir().unwrap();
    let metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();
    assert_eq!(metadata.kind(dir.path()), EngineKind::Unknown);
}

#[test]
fn round_trips_every_kind() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data/unlauncher/engines.toml");
    let prebuilt = dir.path().join("UE_5.7");
    let source = dir.path().join("UE_src");

    let mut metadata = EngineMetadata::load(&file).unwrap();
    metadata.set_kind(&prebuilt, EngineKind::Prebuilt);
    metadata.set_kind(&source, source_kind());
    metadata.save().unwrap();

    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("kind = \"prebuilt\""), "{text}");
    assert!(text.contains("kind = \"source\""), "{text}");
    assert!(text.contains("git_ref = \"5.7\""), "{text}");

    let metadata = EngineMetadata::load(&file).unwrap();
    assert_eq!(metadata.kind(&prebuilt), EngineKind::Prebuilt);
    assert_eq!(metadata.kind(&source), source_kind());
    assert_eq!(
        metadata.kind(&dir.path().join("other")),
        EngineKind::Unknown
    );
}

#[test]
fn set_kind_replaces_and_unknown_removes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("engines.toml");
    let engine = dir.path().join("UE");
    let mut metadata = EngineMetadata::load(&file).unwrap();
    metadata.set_kind(&engine, EngineKind::Prebuilt);
    metadata.set_kind(&engine, source_kind());
    assert_eq!(metadata.kind(&engine), source_kind());
    metadata.save().unwrap();
    assert_eq!(
        fs::read_to_string(&file)
            .unwrap()
            .matches("[[engines]]")
            .count(),
        1
    );

    metadata.set_kind(&engine, EngineKind::Unknown);
    assert_eq!(metadata.kind(&engine), EngineKind::Unknown);
    metadata.save().unwrap();
    assert!(!fs::read_to_string(&file).unwrap().contains("[[engines]]"));
}

#[test]
fn remove_forgets_an_engine() {
    let dir = tempdir().unwrap();
    let engine = dir.path().join("UE");
    let mut metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();
    metadata.set_kind(&engine, EngineKind::Prebuilt);
    metadata.remove(&engine);
    metadata.remove(&engine);
    assert_eq!(metadata.kind(&engine), EngineKind::Unknown);
}

#[test]
fn matches_paths_through_symlinks() {
    let dir = tempdir().unwrap();
    let real = fake_engine(&dir.path().join("real"), 5, 7, 4);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();
    metadata.set_kind(&real, EngineKind::Prebuilt);
    assert_eq!(metadata.kind(&link), EngineKind::Prebuilt);
}

#[test]
fn keeps_entries_it_does_not_understand() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("engines.toml");
    fs::write(
        &file,
        "[[engines]]\npath = \"/opt/UE\"\nkind = \"from_the_future\"\nflavour = \"x\"\n",
    )
    .unwrap();
    let mut metadata = EngineMetadata::load(&file).unwrap();
    assert_eq!(
        metadata.kind(std::path::Path::new("/opt/UE")),
        EngineKind::Unknown
    );
    metadata.set_kind(&dir.path().join("UE"), EngineKind::Prebuilt);
    metadata.save().unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("from_the_future"), "{text}");
    assert!(text.contains("flavour"), "{text}");
}

#[test]
fn malformed_file_is_an_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("engines.toml");
    fs::write(&file, "[[engines]\n").unwrap();
    let err = EngineMetadata::load(&file).unwrap_err();
    assert!(matches!(err, Error::MetadataMalformed { .. }), "{err:?}");
}

#[test]
fn registry_applies_metadata() {
    let dir = tempdir().unwrap();
    let a = fake_engine(&dir.path().join("a"), 5, 7, 4);
    let b = fake_engine(&dir.path().join("b"), 5, 6, 1);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    registry.register(&a, None).unwrap();
    registry.register(&b, None).unwrap();

    let mut metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();
    metadata.set_kind(&a, source_kind());
    registry.apply_metadata(&metadata);

    assert_eq!(registry.find("UE_5.7").unwrap().kind, source_kind());
    assert_eq!(registry.find("UE_5.6").unwrap().kind, EngineKind::Unknown);
}
