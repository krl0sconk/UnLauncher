// SPDX-License-Identifier: GPL-3.0-or-later

//! Installing downloaded Fab assets into fake project and engine trees.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use common::Overwrite;
use fab::{AssetKind, DownloadedAsset, Error, InstallTarget, install};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A fake download with the given files.
fn download(root: &Path, kind: AssetKind, files: &[&str]) -> DownloadedAsset {
    let path = root.join("cache/asset/v1/files");
    fs::create_dir_all(&path).unwrap();
    for file in files {
        write(&path.join(file), file);
    }
    DownloadedAsset { path, kind }
}

/// A fake project; returns its `.uproject`.
fn project(root: &Path) -> PathBuf {
    let uproject = root.join("MyGame/MyGame.uproject");
    write(&uproject, "{}");
    uproject
}

/// A fake engine; returns its root.
fn engine(root: &Path) -> PathBuf {
    let engine = root.join("UE_5.4");
    fs::create_dir_all(engine.join("Engine/Binaries")).unwrap();
    engine
}

fn plugin_download(tmp: &TempDir) -> DownloadedAsset {
    download(
        tmp.path(),
        AssetKind::Plugin,
        &[
            "Handy/Handy.uplugin",
            "Handy/Source/Handy.cpp",
            "Handy/Content/Icon.uasset",
        ],
    )
}

#[test]
fn installs_plugin_into_project() {
    let tmp = TempDir::new().unwrap();
    let asset = plugin_download(&tmp);
    let uproject = project(tmp.path());

    let installed = install(
        &asset,
        "Handy",
        &InstallTarget::Project(uproject),
        Overwrite::No,
    )
    .unwrap();

    assert_eq!(installed, tmp.path().join("MyGame/Plugins/Handy"));
    assert!(installed.join("Handy.uplugin").is_file());
    assert!(installed.join("Source/Handy.cpp").is_file());
    let leftovers: Vec<_> = fs::read_dir(tmp.path().join("MyGame/Plugins"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, vec!["Handy"]);
}

#[test]
fn installs_plugin_at_download_root_into_engine() {
    let tmp = TempDir::new().unwrap();
    let asset = download(
        tmp.path(),
        AssetKind::Unknown,
        &["Handy.uplugin", "Source/Handy.cpp"],
    );
    let engine = engine(tmp.path());

    let installed = install(
        &asset,
        "Handy",
        &InstallTarget::Engine(engine.clone()),
        Overwrite::No,
    )
    .unwrap();

    assert_eq!(installed, engine.join("Engine/Plugins/Marketplace/Handy"));
    assert!(installed.join("Handy.uplugin").is_file());
}

#[test]
fn installs_content_pack_keeping_its_folder_name() {
    let tmp = TempDir::new().unwrap();
    let asset = download(
        tmp.path(),
        AssetKind::Content,
        &["Content/Rocks/Rock.uasset", "Content/Rocks/Maps/Demo.umap"],
    );
    let uproject = project(tmp.path());

    let installed = install(
        &asset,
        "Rocks Pack",
        &InstallTarget::Project(uproject),
        Overwrite::No,
    )
    .unwrap();

    assert_eq!(installed, tmp.path().join("MyGame/Content/Rocks"));
    assert!(installed.join("Maps/Demo.umap").is_file());
}

#[test]
fn installs_content_with_several_entries_into_content() {
    let tmp = TempDir::new().unwrap();
    let asset = download(
        tmp.path(),
        AssetKind::Content,
        &[
            "Pack/Content/Rocks/Rock.uasset",
            "Pack/Content/Trees/Tree.uasset",
        ],
    );
    let uproject = project(tmp.path());

    let installed = install(
        &asset,
        "Nature",
        &InstallTarget::Project(uproject),
        Overwrite::No,
    )
    .unwrap();

    assert_eq!(installed, tmp.path().join("MyGame/Content"));
    assert!(installed.join("Rocks/Rock.uasset").is_file());
    assert!(installed.join("Trees/Tree.uasset").is_file());
}

#[test]
fn refuses_to_overwrite_without_permission() {
    let tmp = TempDir::new().unwrap();
    let asset = plugin_download(&tmp);
    let uproject = project(tmp.path());
    let existing = tmp.path().join("MyGame/Plugins/Handy/Mine.txt");
    write(&existing, "user work");
    let target = InstallTarget::Project(uproject);

    let error = install(&asset, "Handy", &target, Overwrite::No).unwrap_err();

    assert!(matches!(error, Error::AlreadyExists { .. }));
    assert_eq!(fs::read_to_string(&existing).unwrap(), "user work");

    let installed = install(&asset, "Handy", &target, Overwrite::Yes).unwrap();
    assert!(!existing.exists());
    assert!(installed.join("Handy.uplugin").is_file());
}

#[test]
fn refuses_content_into_engine() {
    let tmp = TempDir::new().unwrap();
    let asset = download(
        tmp.path(),
        AssetKind::Content,
        &["Content/Rocks/Rock.uasset"],
    );
    let engine = engine(tmp.path());

    let error = install(
        &asset,
        "Rocks",
        &InstallTarget::Engine(engine.clone()),
        Overwrite::Yes,
    )
    .unwrap_err();

    assert!(matches!(error, Error::ContentIntoEngine));
    assert!(!engine.join("Engine/Content").exists());
}

#[test]
fn refuses_complete_projects_and_unknown_layouts() {
    let tmp = TempDir::new().unwrap();
    let uproject = project(tmp.path());
    let target = InstallTarget::Project(uproject);

    let sample = download(tmp.path(), AssetKind::Project, &["Sample/Sample.uproject"]);
    assert!(matches!(
        install(&sample, "Sample", &target, Overwrite::No),
        Err(Error::ProjectAsset { .. })
    ));

    let other = TempDir::new().unwrap();
    let odd = download(other.path(), AssetKind::Unknown, &["readme.txt"]);
    assert!(matches!(
        install(&odd, "Odd", &target, Overwrite::No),
        Err(Error::UnknownLayout { .. })
    ));
}

#[test]
fn rejects_missing_targets() {
    let tmp = TempDir::new().unwrap();
    let asset = plugin_download(&tmp);
    let not_engine = tmp.path().join("nothing");
    assert!(matches!(
        install(
            &asset,
            "Handy",
            &InstallTarget::Engine(not_engine),
            Overwrite::No
        ),
        Err(Error::TargetNotFound { .. })
    ));
    let not_project = tmp.path().join("Missing.uproject");
    assert!(matches!(
        install(
            &asset,
            "Handy",
            &InstallTarget::Project(not_project),
            Overwrite::No
        ),
        Err(Error::TargetNotFound { .. })
    ));
}

#[test]
fn creates_a_new_project_from_a_complete_project() {
    let tmp = TempDir::new().unwrap();
    let asset = download(
        tmp.path(),
        AssetKind::Project,
        &[
            "CitySample/CitySample.uproject",
            "CitySample/Content/Maps/Big.umap",
            "CitySample/Config/DefaultEngine.ini",
        ],
    );
    let parent = tmp.path().join("Unreal Projects");
    let target = InstallTarget::NewProject {
        parent: parent.clone(),
    };

    let uproject = install(&asset, "City Sample", &target, Overwrite::No).unwrap();
    assert_eq!(uproject, parent.join("CitySample/CitySample.uproject"));
    assert!(uproject.is_file());
    assert!(parent.join("CitySample/Content/Maps/Big.umap").is_file());

    // Never replaces an existing project folder.
    assert!(matches!(
        install(&asset, "City Sample", &target, Overwrite::No),
        Err(Error::AlreadyExists { .. })
    ));
}

#[test]
fn new_project_target_needs_a_complete_project() {
    let tmp = TempDir::new().unwrap();
    let asset = plugin_download(&tmp);
    let target = InstallTarget::NewProject {
        parent: tmp.path().join("Unreal Projects"),
    };
    assert!(matches!(
        install(&asset, "Handy", &target, Overwrite::No),
        Err(Error::NotAProject { .. })
    ));
    assert!(!tmp.path().join("Unreal Projects").exists());
}
