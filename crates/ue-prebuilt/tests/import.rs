// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for importing a prebuilt engine zip.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::UserFacing;
use jobs::{Control, Reporter};
use tempfile::{TempDir, tempdir};
use ue_core::{EngineKind, EngineMetadata, EngineRegistry, EngineVersion};
use ue_prebuilt::{EXTRACTING_STAGE, Error, import_zip, register_imported};
use zip::write::SimpleFileOptions;

/// One entry of a test archive.
enum Item {
    Dir,
    File(Vec<u8>, u32),
    Link(&'static str),
}

const BUILD_VERSION: &str = "{\n\t\"MajorVersion\": 5,\n\t\"MinorVersion\": 7,\n\t\"PatchVersion\": 4,\n\t\"Changelist\": 0,\n\t\"BranchName\": \"++UE5+Release-5.7\"\n}\n";

/// Size of the big file in the fake engine: several extraction chunks.
const BIG: usize = 3 * 1024 * 1024 + 17;

/// A small engine tree as Epic ships it, under `prefix` (`""` or `"Wrapper/"`).
fn engine_items(prefix: &str) -> Vec<(String, Item)> {
    let p = |s: &str| format!("{prefix}{s}");
    let mut items = Vec::new();
    if !prefix.is_empty() {
        items.push((p(""), Item::Dir));
    }
    items.extend([
        (p("Engine/"), Item::Dir),
        (p("Engine/Binaries/Linux/"), Item::Dir),
        (
            p("Engine/Binaries/Linux/UnrealEditor"),
            Item::File(b"#!/bin/sh\n".to_vec(), 0o755),
        ),
        (
            p("Engine/Binaries/Linux/libfoo.so.1"),
            Item::File(vec![7; BIG], 0o644),
        ),
        (
            p("Engine/Binaries/Linux/libfoo.so"),
            Item::Link("libfoo.so.1"),
        ),
        (p("Engine/Escape"), Item::Link("../../outside")),
        (
            p("Engine/Build/Build.version"),
            Item::File(BUILD_VERSION.as_bytes().to_vec(), 0o644),
        ),
        (
            p("Templates/TP_Blank/TP_Blank.uproject"),
            Item::File(b"{}".to_vec(), 0o644),
        ),
        (p("FeaturePacks/"), Item::Dir),
    ]);
    items
}

fn write_zip(path: &Path, items: &[(String, Item)]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, item) in items {
        match item {
            Item::Dir => {
                zip.add_directory(name.as_str(), SimpleFileOptions::default())
                    .unwrap();
            }
            Item::File(data, mode) => {
                zip.start_file(
                    name.as_str(),
                    SimpleFileOptions::default().unix_permissions(*mode),
                )
                .unwrap();
                zip.write_all(data).unwrap();
            }
            Item::Link(target) => {
                zip.add_symlink(name.as_str(), *target, SimpleFileOptions::default())
                    .unwrap();
            }
        }
    }
    zip.finish().unwrap();
}

/// A zip in its own temp folder, and an empty temp folder for the destination.
struct Fixture {
    _zip_dir: TempDir,
    zip: PathBuf,
    out: TempDir,
}

fn fixture(items: &[(String, Item)]) -> Fixture {
    let zip_dir = tempdir().unwrap();
    let zip = zip_dir.path().join("engine.zip");
    write_zip(&zip, items);
    Fixture {
        _zip_dir: zip_dir,
        zip,
        out: tempdir().unwrap(),
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Fails if `dir` has anything in it (a partial import or its hidden staging folder).
fn assert_left_nothing(dir: &Path) {
    let names = names_in(dir);
    assert_eq!(names.len(), 0, "left behind: {names:?}");
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn assert_engine_tree(root: &Path) {
    let editor = root.join("Engine/Binaries/Linux/UnrealEditor");
    assert_eq!(mode(&editor), 0o755);
    assert_eq!(mode(&root.join("Engine/Build/Build.version")), 0o644);
    assert_eq!(
        fs::read(root.join("Engine/Binaries/Linux/libfoo.so.1"))
            .unwrap()
            .len(),
        BIG
    );
    assert!(root.join("Templates/TP_Blank/TP_Blank.uproject").is_file());
    assert!(root.join("FeaturePacks").is_dir());
}

#[test]
fn imports_engine_at_archive_root() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE_5.7");
    let (reporter, handle) = jobs::channel();

    let engine = import_zip(&f.zip, &destination, &reporter).unwrap();

    assert_eq!(engine.path, destination);
    assert_eq!(engine.version, EngineVersion::new(5, 7, 4));
    assert_engine_tree(&destination);
    let progress = handle.progress();
    assert_eq!(progress.stage, EXTRACTING_STAGE);
    assert_eq!(progress.fraction, Some(1.0));
    assert_eq!(names_in(f.out.path()), ["UE_5.7"]);
}

#[test]
fn strips_a_single_wrapping_folder() {
    let f = fixture(&engine_items("Linux_Unreal_Engine_5.7.4/"));
    let destination = f.out.path().join("UE");
    import_zip(&f.zip, &destination, &Reporter::detached()).unwrap();
    assert_engine_tree(&destination);
    assert!(!destination.join("Linux_Unreal_Engine_5.7.4").exists());
}

#[test]
fn creates_inner_symlinks_and_skips_escaping_ones() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    import_zip(&f.zip, &destination, &Reporter::detached()).unwrap();

    let link = destination.join("Engine/Binaries/Linux/libfoo.so");
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("libfoo.so.1"));
    assert!(fs::symlink_metadata(destination.join("Engine/Escape")).is_err());
}

#[test]
fn imports_into_an_existing_empty_folder() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    fs::create_dir(&destination).unwrap();
    import_zip(&f.zip, &destination, &Reporter::detached()).unwrap();
    assert_engine_tree(&destination);
    assert_eq!(names_in(f.out.path()), ["UE"]);
}

#[test]
fn refuses_a_destination_that_is_not_empty() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("mine.txt"), "keep me").unwrap();

    let err = import_zip(&f.zip, &destination, &Reporter::detached()).unwrap_err();

    assert!(matches!(err, Error::DestinationNotEmpty { .. }), "{err:?}");
    assert!(err.hint().is_some());
    assert_eq!(names_in(&destination), ["mine.txt"]);
    assert_eq!(names_in(f.out.path()), ["UE"]);
}

#[test]
fn refuses_a_destination_that_is_a_file() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    fs::write(&destination, "a file").unwrap();
    let err = import_zip(&f.zip, &destination, &Reporter::detached()).unwrap_err();
    assert!(matches!(err, Error::DestinationNotEmpty { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(&destination).unwrap(), "a file");
}

#[test]
fn rejects_entries_that_escape_and_writes_nothing() {
    let mut items = engine_items("");
    items.push(("../evil.txt".into(), Item::File(b"pwned".to_vec(), 0o644)));
    let f = fixture(&items);
    let destination = f.out.path().join("inner/UE");

    let err = import_zip(&f.zip, &destination, &Reporter::detached()).unwrap_err();

    assert!(
        matches!(&err, Error::UnsafeEntry { entry, .. } if entry == "../evil.txt"),
        "{err:?}"
    );
    assert_left_nothing(f.out.path());
}

#[test]
fn non_engine_zip_fails_and_leaves_nothing() {
    let f = fixture(&[
        ("MyProject/".into(), Item::Dir),
        (
            "MyProject/MyProject.uproject".into(),
            Item::File(b"{}".to_vec(), 0o644),
        ),
    ]);
    let destination = f.out.path().join("UE");

    let err = import_zip(&f.zip, &destination, &Reporter::detached()).unwrap_err();

    assert!(matches!(err, Error::NotAnEngineArchive { .. }), "{err:?}");
    assert!(err.hint().unwrap().contains("zip"));
    assert_left_nothing(f.out.path());
}

#[test]
fn not_a_zip_is_an_invalid_archive() {
    let out = tempdir().unwrap();
    let zip = out.path().join("engine.zip");
    fs::write(&zip, "not a zip").unwrap();
    let err = import_zip(&zip, &out.path().join("UE"), &Reporter::detached()).unwrap_err();
    assert!(matches!(err, Error::InvalidArchive { .. }), "{err:?}");
    assert_eq!(names_in(out.path()), ["engine.zip"]);
}

#[test]
fn cancelled_import_leaves_nothing() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    let (reporter, handle) = jobs::channel();
    handle.send(Control::Cancel);

    let err = import_zip(&f.zip, &destination, &reporter).unwrap_err();

    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert_left_nothing(f.out.path());
}

#[test]
fn cancel_while_paused_leaves_nothing() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    let (reporter, handle) = jobs::channel();
    handle.send(Control::Pause);

    let worker = {
        let zip = f.zip.clone();
        let destination = destination.clone();
        std::thread::spawn(move || import_zip(&zip, &destination, &reporter))
    };
    std::thread::sleep(Duration::from_millis(100));
    assert!(!worker.is_finished());
    assert!(!destination.exists());
    handle.send(Control::Cancel);

    let err = worker.join().unwrap().unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert_left_nothing(f.out.path());
}

#[test]
fn register_imported_records_a_prebuilt_engine() {
    let f = fixture(&engine_items(""));
    let destination = f.out.path().join("UE");
    let engine = import_zip(&f.zip, &destination, &Reporter::detached()).unwrap();

    let config = tempdir().unwrap();
    let mut registry = EngineRegistry::load(&config.path().join("Install.ini")).unwrap();
    let mut metadata = EngineMetadata::load(&config.path().join("engines.toml")).unwrap();
    let registered = register_imported(&mut registry, &mut metadata, &engine, None).unwrap();

    assert_eq!(registered.id.as_str(), "UE_5.7");
    assert_eq!(registered.kind, EngineKind::Prebuilt);
    assert_eq!(registry.find("UE_5.7").unwrap().kind, EngineKind::Prebuilt);
    assert_eq!(metadata.kind(&destination), EngineKind::Prebuilt);

    // Registering the same folder twice is a ue-core error.
    let err = register_imported(&mut registry, &mut metadata, &engine, None).unwrap_err();
    assert!(
        matches!(&err, Error::Engine(inner) if matches!(**inner, ue_core::Error::EngineAlreadyRegistered { .. })),
        "{err:?}"
    );
}
