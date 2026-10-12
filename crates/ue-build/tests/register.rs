// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for registering a built engine.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use tempfile::tempdir;
use ue_build::register_built;
use ue_core::{EngineKind, EngineMetadata, EngineRegistry};

fn fake_engine(root: &Path) {
    fs::create_dir_all(root.join("Engine/Binaries/Linux")).unwrap();
    fs::write(root.join("Engine/Binaries/Linux/UnrealEditor"), b"").unwrap();
    fs::create_dir_all(root.join("Engine/Build")).unwrap();
    fs::write(
        root.join("Engine/Build/Build.version"),
        r#"{"MajorVersion":5,"MinorVersion":7,"PatchVersion":4}"#,
    )
    .unwrap();
}

#[test]
fn registers_new_engine_as_source() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("UnrealEngine");
    fake_engine(&root);
    let mut registry = EngineRegistry::load(&dir.path().join("Install.ini")).unwrap();
    let mut metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();

    let engine = register_built(
        &mut registry,
        &mut metadata,
        &root,
        "https://x/ue.git",
        "5.7",
        None,
    )
    .unwrap();
    let source = EngineKind::Source {
        remote: "https://x/ue.git".into(),
        git_ref: "5.7".into(),
    };
    assert_eq!(engine.id.as_str(), "UE_5.7");
    assert_eq!(engine.kind, source);
    assert_eq!(metadata.kind(&root), source);
}

#[test]
fn already_registered_engine_only_updates_metadata() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("UnrealEngine");
    fake_engine(&root);
    let ini = dir.path().join("Install.ini");
    fs::write(&ini, format!("[Installations]\nMine={}\n", root.display())).unwrap();
    let mut registry = EngineRegistry::load(&ini).unwrap();
    let mut metadata = EngineMetadata::load(&dir.path().join("engines.toml")).unwrap();

    let engine = register_built(&mut registry, &mut metadata, &root, "r", "5.8", None).unwrap();
    assert_eq!(engine.id.as_str(), "Mine");
    assert_eq!(registry.engines().len(), 1);
}
