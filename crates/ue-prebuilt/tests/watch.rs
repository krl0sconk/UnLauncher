// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for watching the downloads folder, in a temporary folder.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use ue_prebuilt::{DownloadsWatcher, FoundEngineZip, watch_for_engine_zip};

const ZIP: &str = "Linux_Unreal_Engine_5.7.4.zip";

fn write(path: &Path, bytes: usize) {
    fs::write(path, vec![0u8; bytes]).unwrap();
}

/// A watcher that started a minute ago, so files written now count as new.
fn watcher(dir: &Path) -> DownloadsWatcher {
    DownloadsWatcher::new(dir, SystemTime::now() - Duration::from_mins(1))
}

#[test]
fn reports_a_zip_once_its_size_is_stable() {
    let dir = tempfile::tempdir().unwrap();
    let mut watcher = watcher(dir.path());
    let zip = dir.path().join(ZIP);
    write(&zip, 10);

    assert!(
        watcher.poll().is_empty(),
        "first sight only records the size"
    );
    write(&zip, 20);
    assert!(watcher.poll().is_empty(), "still growing");
    assert_eq!(
        watcher.poll(),
        [FoundEngineZip {
            path: zip.clone(),
            version: "5.7.4".to_owned()
        }]
    );
    assert!(watcher.poll().is_empty(), "reported only once");
}

#[test]
fn waits_while_the_browser_is_downloading() {
    let dir = tempfile::tempdir().unwrap();
    let mut watcher = watcher(dir.path());
    let zip = dir.path().join(ZIP);
    for suffix in [".part", ".crdownload"] {
        let partial = dir.path().join(format!("{ZIP}{suffix}"));
        write(&zip, 30);
        write(&partial, 5);
        assert!(watcher.poll().is_empty());
        assert!(
            watcher.poll().is_empty(),
            "never complete while {suffix} exists"
        );
        fs::remove_file(&partial).unwrap();
    }
    assert!(watcher.poll().is_empty());
    assert_eq!(watcher.poll().len(), 1);
}

#[test]
fn ignores_empty_placeholders_other_files_and_old_zips() {
    let dir = tempfile::tempdir().unwrap();
    // Firefox creates an empty file with the final name while downloading.
    write(&dir.path().join(ZIP), 0);
    write(&dir.path().join("Linux_Fab_5.7.zip"), 10);
    write(&dir.path().join("notes.txt"), 10);
    let old = dir.path().join("Linux_Unreal_Engine_5.6.1.zip");
    write(&old, 10);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_hours(1))
        .unwrap();

    let mut watcher = watcher(dir.path());
    assert!(watcher.poll().is_empty());
    assert!(watcher.poll().is_empty());
}

#[test]
fn a_missing_folder_gives_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut watcher = watcher(&dir.path().join("missing"));
    assert!(watcher.poll().is_empty());
    assert_eq!(watcher.dir(), dir.path().join("missing"));
}

#[tokio::test]
async fn watch_returns_the_first_complete_zip() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join(ZIP), 42);
    let found = watch_for_engine_zip(
        watcher(dir.path()),
        Duration::from_millis(10),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(found.version, "5.7.4");
}

#[tokio::test]
async fn watch_gives_up_after_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let found = watch_for_engine_zip(
        watcher(dir.path()),
        Duration::from_millis(10),
        Duration::from_millis(50),
    )
    .await;
    assert_eq!(found, None);
}
