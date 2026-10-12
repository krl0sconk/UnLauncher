// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Mutex;

use super::chunk::tests::{chunk_file, spec};
use super::plan::{FileSpec, PartSpec};
use super::*;

const A: &[u8] = b"ABCDEF";
const B: &[u8] = b"xyz";

/// Serves chunk files from memory and records what was fetched.
#[derive(Default)]
struct FakeFetch {
    files: HashMap<String, Vec<u8>>,
    fetched: Mutex<Vec<String>>,
}

impl FakeFetch {
    fn fetched(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }
}

impl Fetch for FakeFetch {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            self.fetched.lock().unwrap().push(url.to_owned());
            self.files.get(url).cloned().ok_or(Error::Http {
                what: "download".to_owned(),
                status: 404,
            })
        })
    }
}

fn part(guid: &str, offset: usize, size: usize) -> PartSpec {
    PartSpec {
        guid: guid.to_owned(),
        offset,
        size,
    }
}

/// `Handy/Handy.uplugin` = "ABCD", `Handy/Content/Rock.uasset` = "EF" + "xyz".
fn plan() -> Plan {
    Plan {
        chunks: vec![spec("AAAA", A), spec("BBBB", B)],
        files: vec![
            FileSpec {
                path: "Handy/Content/Rock.uasset".into(),
                parts: vec![part("AAAA", 4, 2), part("BBBB", 0, 3)],
            },
            FileSpec {
                path: "Handy/Handy.uplugin".into(),
                parts: vec![part("AAAA", 0, 4)],
            },
        ],
    }
}

fn fetcher() -> FakeFetch {
    let plan = plan();
    FakeFetch {
        files: HashMap::from([
            (plan.chunks[0].url.clone(), chunk_file(A)),
            (plan.chunks[1].url.clone(), chunk_file(B)),
        ]),
        ..FakeFetch::default()
    }
}

#[tokio::test]
async fn downloads_and_assembles() {
    let cache = tempfile::tempdir().unwrap();
    let dirs = VersionDirs::new(cache.path(), "asset", "v1");
    let fetch = fetcher();
    let (reporter, handle) = jobs::channel();

    let done = run(plan(), dirs.clone(), &fetch, &reporter, AssetKind::Unknown)
        .await
        .unwrap();

    assert_eq!(done.path, cache.path().join("asset/v1/files"));
    assert_eq!(done.kind, AssetKind::Plugin);
    assert_eq!(
        std::fs::read(done.path.join("Handy/Handy.uplugin")).unwrap(),
        b"ABCD"
    );
    assert_eq!(
        std::fs::read(done.path.join("Handy/Content/Rock.uasset")).unwrap(),
        b"EFxyz"
    );
    assert_eq!(fetch.fetched().len(), 2);
    assert!(!dirs.chunks().exists());
    assert!(!dirs.partial_files().exists());
    assert_eq!(handle.progress().stage, "Assembling");
    assert_eq!(handle.progress().fraction, Some(1.0));
    assert_eq!(
        completed(&dirs, AssetKind::Plugin).await.unwrap().path,
        done.path
    );
}

#[tokio::test]
async fn resumes_with_verified_chunks_only() {
    let cache = tempfile::tempdir().unwrap();
    let dirs = VersionDirs::new(cache.path(), "asset", "v1");
    std::fs::create_dir_all(dirs.chunks()).unwrap();
    // A is complete; B was cut short by a crash.
    std::fs::write(chunk_path(&dirs.chunks(), "AAAA"), chunk_file(A)).unwrap();
    std::fs::write(chunk_path(&dirs.chunks(), "BBBB"), &chunk_file(B)[..30]).unwrap();
    let fetch = fetcher();

    run(
        plan(),
        dirs,
        &fetch,
        &jobs::Reporter::detached(),
        AssetKind::Plugin,
    )
    .await
    .unwrap();

    assert_eq!(fetch.fetched(), vec![plan().chunks[1].url.clone()]);
}

#[tokio::test]
async fn corrupt_download_fails_after_retry() {
    let cache = tempfile::tempdir().unwrap();
    let dirs = VersionDirs::new(cache.path(), "asset", "v1");
    let mut fetch = fetcher();
    let url = plan().chunks[1].url.clone();
    fetch.files.insert(url, chunk_file(b"not B"));

    let error = run(
        plan(),
        dirs.clone(),
        &fetch,
        &jobs::Reporter::detached(),
        AssetKind::Plugin,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, Error::ChunkCorrupt { ref guid } if guid == "BBBB"));
    assert_eq!(fetch.fetched().len(), 3);
    // The verified chunk stays for the next attempt; nothing was assembled.
    assert!(chunk_path(&dirs.chunks(), "AAAA").exists());
    assert!(!chunk_path(&dirs.chunks(), "BBBB").exists());
    assert!(completed(&dirs, AssetKind::Plugin).await.is_none());
}

#[tokio::test]
async fn cancel_stops_before_downloading() {
    let cache = tempfile::tempdir().unwrap();
    let dirs = VersionDirs::new(cache.path(), "asset", "v1");
    let fetch = fetcher();
    let (reporter, handle) = jobs::channel();
    handle.send(jobs::Control::Cancel);

    let error = run(plan(), dirs, &fetch, &reporter, AssetKind::Plugin)
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Cancelled));
    assert_eq!(fetch.fetched(), Vec::<String>::new());
}

#[test]
fn ids_become_safe_folder_names() {
    let dirs = VersionDirs::new(Path::new("/cache"), "../x", "a/b");
    assert_eq!(dirs.files(), Path::new("/cache/.._x/a_b/files"));
    let dirs = VersionDirs::new(Path::new("/cache"), "..", "");
    assert_eq!(dirs.files(), Path::new("/cache/_../_/files"));
}
