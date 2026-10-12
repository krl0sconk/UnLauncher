// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `.uproject` parsing, editing, discovery and engine resolution.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fmt::Write as _;
use std::fs;

use common::Overwrite;
use support::{fake_engine, fake_project, fixture, read_fixture};
use tempfile::tempdir;
use ue_core::{
    EngineAssociation, EngineId, EngineRegistry, Error, Project, discover_projects, resolve_engine,
};

fn id(s: &str) -> EngineId {
    EngineId::new(s).unwrap()
}

#[test]
fn loads_version_association() {
    let project = Project::load(&fixture("uproject/version.uproject")).unwrap();
    assert_eq!(project.name, "version");
    assert_eq!(
        project.association,
        EngineAssociation::Version { major: 5, minor: 7 }
    );
}

#[test]
fn loads_guid_association() {
    let project = Project::load(&fixture("uproject/guid.uproject")).unwrap();
    assert_eq!(
        project.association,
        EngineAssociation::Id(id("{8A9F2E21-4B6C-4D1E-9F3A-7C2B1D0E5F64}"))
    );
}

#[test]
fn empty_or_missing_association_is_none() {
    for name in ["empty_association", "missing_association"] {
        let project = Project::load(&fixture(&format!("uproject/{name}.uproject"))).unwrap();
        assert_eq!(project.association, EngineAssociation::None, "{name}");
    }
}

#[test]
fn rejects_malformed_projects() {
    for name in ["malformed", "not_object"] {
        let err = Project::load(&fixture(&format!("uproject/{name}.uproject"))).unwrap_err();
        assert!(matches!(err, Error::UProjectMalformed { .. }), "{name}");
    }
}

#[test]
fn rejects_non_string_association() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("Bad.uproject");
    fs::write(&path, "{\"EngineAssociation\": 5.7}").unwrap();
    assert!(matches!(
        Project::load(&path).unwrap_err(),
        Error::UProjectMalformed { .. }
    ));
}

#[test]
fn association_displays_like_unreal_writes_it() {
    assert_eq!(
        EngineAssociation::Version { major: 5, minor: 7 }.to_string(),
        "5.7"
    );
    assert_eq!(EngineAssociation::Id(id("{ABC}")).to_string(), "{ABC}");
    assert_eq!(EngineAssociation::None.to_string(), "");
}

#[test]
fn saving_unchanged_unreal_file_is_byte_identical() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("kart.uproject");
    let original = read_fixture("uproject/version.uproject");
    fs::write(&path, &original).unwrap();

    let mut project = Project::load(&path).unwrap();
    project.set_association(EngineAssociation::Version { major: 5, minor: 7 });
    project.save(Overwrite::Yes).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn set_association_preserves_unknown_fields_and_order() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("Game.uproject");
    fs::write(&path, read_fixture("uproject/unknown_fields.uproject")).unwrap();

    let mut project = Project::load(&path).unwrap();
    project.set_association(EngineAssociation::Id(id("{GUID-1}")));
    let backup = project.save(Overwrite::Yes).unwrap().unwrap();
    assert_eq!(backup, dir.path().join("Game.uproject.bak"));

    let before: serde_json::Value =
        serde_json::from_str(&read_fixture("uproject/unknown_fields.uproject")).unwrap();
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut expected = before.clone();
    expected["EngineAssociation"] = "{GUID-1}".into();
    assert_eq!(after, expected);

    let keys: Vec<_> = after.as_object().unwrap().keys().cloned().collect();
    let original_keys: Vec<_> = before.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, original_keys);
    assert_eq!(
        Project::load(&path).unwrap().association,
        EngineAssociation::Id(id("{GUID-1}"))
    );
}

#[test]
fn set_association_adds_missing_field() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("Game.uproject");
    fs::write(&path, read_fixture("uproject/missing_association.uproject")).unwrap();
    let mut project = Project::load(&path).unwrap();
    project.set_association(EngineAssociation::Version { major: 5, minor: 6 });
    project.save(Overwrite::Yes).unwrap();
    assert_eq!(
        Project::load(&path).unwrap().association,
        EngineAssociation::Version { major: 5, minor: 6 }
    );
}

#[test]
fn save_requires_confirmation() {
    let dir = tempdir().unwrap();
    let path = fake_project(dir.path(), "Game", "5.7");
    let original = fs::read_to_string(&path).unwrap();
    let mut project = Project::load(&path).unwrap();
    project.set_association(EngineAssociation::None);
    assert!(matches!(
        project.save(Overwrite::No).unwrap_err(),
        Error::OverwriteNotConfirmed { .. }
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

fn registry_with(dir: &std::path::Path, entries: &[(&str, (u32, u32, u32))]) -> EngineRegistry {
    let mut text = String::from("[Installations]\n");
    for (key, (major, minor, patch)) in entries {
        let root = fake_engine(
            &dir.join(key.trim_matches(['{', '}'])),
            *major,
            *minor,
            *patch,
        );
        writeln!(text, "{key}={}", root.display()).unwrap();
    }
    let ini = dir.join("Install.ini");
    fs::write(&ini, text).unwrap();
    EngineRegistry::load(&ini).unwrap()
}

fn project_with(dir: &std::path::Path, association: &str) -> Project {
    Project::load(&fake_project(&dir.join("proj"), "Game", association)).unwrap()
}

#[test]
fn resolves_version_by_exact_id_first() {
    let dir = tempdir().unwrap();
    let registry = registry_with(dir.path(), &[("UE_5.7", (5, 7, 4)), ("5.7", (5, 7, 0))]);
    let engine = resolve_engine(&project_with(dir.path(), "5.7"), &registry).unwrap();
    assert_eq!(engine.id.as_str(), "5.7");
}

#[test]
fn resolves_version_through_ue_prefixed_id() {
    let dir = tempdir().unwrap();
    let registry = registry_with(dir.path(), &[("Fork", (5, 7, 1)), ("UE_5.7", (5, 7, 4))]);
    let engine = resolve_engine(&project_with(dir.path(), "5.7"), &registry).unwrap();
    assert_eq!(engine.id.as_str(), "UE_5.7");
}

#[test]
fn resolves_version_by_engine_version_as_fallback() {
    let dir = tempdir().unwrap();
    let registry = registry_with(dir.path(), &[("Fork", (5, 7, 1)), ("Other", (5, 6, 0))]);
    let engine = resolve_engine(&project_with(dir.path(), "5.7"), &registry).unwrap();
    assert_eq!(engine.id.as_str(), "Fork");
}

#[test]
fn resolves_guid_ignoring_case() {
    let dir = tempdir().unwrap();
    let registry = registry_with(dir.path(), &[("{AB12CD34-0000}", (5, 7, 4))]);
    let engine = resolve_engine(&project_with(dir.path(), "{ab12cd34-0000}"), &registry).unwrap();
    assert_eq!(engine.id.as_str(), "{AB12CD34-0000}");
}

#[test]
fn unresolved_associations_return_none() {
    let dir = tempdir().unwrap();
    let registry = registry_with(dir.path(), &[("UE_5.7", (5, 7, 4))]);
    for association in ["", "5.3", "{MISSING}"] {
        assert!(
            resolve_engine(&project_with(dir.path(), association), &registry).is_none(),
            "{association:?}"
        );
    }
}

#[test]
fn association_for_engine_prefers_version_for_ue_ids() {
    let dir = tempdir().unwrap();
    let registry = registry_with(
        dir.path(),
        &[("UE_5.7", (5, 7, 4)), ("{GUID-2}", (5, 7, 4))],
    );
    let by_id = |s: &str| {
        registry
            .engines()
            .iter()
            .find(|e| e.id.as_str() == s)
            .unwrap()
    };
    assert_eq!(
        EngineAssociation::for_engine(by_id("UE_5.7")),
        EngineAssociation::Version { major: 5, minor: 7 }
    );
    assert_eq!(
        EngineAssociation::for_engine(by_id("{GUID-2}")),
        EngineAssociation::Id(id("{GUID-2}"))
    );
}

#[test]
fn discovers_projects_without_entering_projects_or_engines() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    let a = fake_project(&base.join("Alpha"), "Alpha", "5.7");
    let b = fake_project(&base.join("work/Beta"), "Beta", "5.6");
    // A .uproject inside a project's plugin folder is not a separate project.
    fake_project(&base.join("Alpha/Plugins/Sample"), "Sample", "5.7");
    // Engine templates are not user projects.
    let engine = fake_engine(&base.join("UE_5.7"), 5, 7, 4);
    fake_project(&engine.join("Templates/TP_Blank"), "TP_Blank", "");
    // Too deep.
    fake_project(&base.join("a/b/c/d/Deep"), "Deep", "5.7");
    // Broken project files are reported, not skipped silently.
    fs::create_dir_all(base.join("Broken")).unwrap();
    fs::write(base.join("Broken/Broken.uproject"), "{").unwrap();

    let results = discover_projects(&[base.to_path_buf()], 3);
    let mut found: Vec<_> = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .map(|p| p.path.clone())
        .collect();
    found.sort();
    assert_eq!(found, vec![a, b]);
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
}
