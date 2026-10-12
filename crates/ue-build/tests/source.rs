// SPDX-License-Identifier: GPL-3.0-or-later

//! Clone and update against local repositories (no network).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use common::Secret;
use git2::{Repository, Signature};
use jobs::{Control, Reporter};
use ue_build::{Error, clone_shallow, update};

/// Writes `files` and commits them on HEAD of `repo`.
fn commit(repo: &Repository, files: &[(&str, &str)], message: &str) -> git2::Oid {
    let root = repo.workdir().unwrap();
    let mut index = repo.index().unwrap();
    for (name, content) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        index.add_path(Path::new(name)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("Test", "test@example.com").unwrap();
    let parents: Vec<_> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parents: Vec<_> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .unwrap()
}

/// An "upstream" repository on branch `release` with an annotated tag `5.7.0`.
fn upstream(dir: &Path) -> (Repository, String) {
    let repo = Repository::init(dir).unwrap();
    repo.set_head("refs/heads/release").unwrap();
    let first = commit(
        &repo,
        &[("Setup.sh", "echo setup\n"), ("Engine/a.txt", "1\n")],
        "first",
    );
    let sig = Signature::now("Test", "test@example.com").unwrap();
    repo.tag(
        "5.7.0",
        &repo.find_object(first, None).unwrap(),
        &sig,
        "5.7.0",
        false,
    )
    .unwrap();
    let url = format!("file://{}", dir.display());
    (repo, url)
}

fn token() -> Secret {
    Secret::new("unused-for-file-remotes")
}

#[tokio::test]
async fn clones_a_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    clone_shallow(&url, "release", &dest, &token(), &Reporter::detached())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dest.join("Engine/a.txt")).unwrap(),
        "1\n"
    );
    let repo = Repository::open(&dest).unwrap();
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "release");
    // The remote URL never contains the token.
    let config = std::fs::read_to_string(dest.join(".git/config")).unwrap();
    assert!(!config.contains("unused-for-file-remotes"));
}

#[tokio::test]
async fn clones_a_tag_detached() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    clone_shallow(&url, "5.7.0", &dest, &token(), &Reporter::detached())
        .await
        .unwrap();
    let repo = Repository::open(&dest).unwrap();
    assert!(repo.head_detached().unwrap());
    assert!(dest.join("Setup.sh").is_file());
}

#[tokio::test]
async fn unknown_ref_fails_and_cleans_up() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    let err = clone_shallow(&url, "nope", &dest, &token(), &Reporter::detached())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::RefNotFound { .. }), "{err:?}");
    assert!(!dest.exists());
}

#[tokio::test]
async fn failed_clone_into_existing_empty_dir_keeps_the_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    std::fs::create_dir(&dest).unwrap();
    clone_shallow(&url, "nope", &dest, &token(), &Reporter::detached())
        .await
        .unwrap_err();
    assert!(dest.is_dir());
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
}

#[tokio::test]
async fn refuses_non_empty_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("mine.txt"), "user data").unwrap();
    let err = clone_shallow(&url, "release", &dest, &token(), &Reporter::detached())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::DestinationNotEmpty { .. }));
    assert_eq!(
        std::fs::read_to_string(dest.join("mine.txt")).unwrap(),
        "user data"
    );
}

#[tokio::test]
async fn cancelled_clone_leaves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (_up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    let (reporter, handle) = jobs::channel();
    handle.send(Control::Cancel);
    let err = clone_shallow(&url, "release", &dest, &token(), &reporter)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(!dest.exists());
}

#[tokio::test]
async fn update_fetches_new_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let (up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    clone_shallow(&url, "release", &dest, &token(), &Reporter::detached())
        .await
        .unwrap();

    let same = update(&dest, "release", &token(), &Reporter::detached())
        .await
        .unwrap();
    assert!(same.is_up_to_date());

    let second = commit(&up, &[("Engine/a.txt", "2\n")], "second");
    let outcome = update(&dest, "release", &token(), &Reporter::detached())
        .await
        .unwrap();
    assert_eq!(outcome.old_commit, same.new_commit);
    assert_eq!(outcome.new_commit, second.to_string());
    assert_eq!(
        std::fs::read_to_string(dest.join("Engine/a.txt")).unwrap(),
        "2\n"
    );
    let repo = Repository::open(&dest).unwrap();
    assert_eq!(repo.head().unwrap().target(), Some(second));
}

#[tokio::test]
async fn update_refuses_local_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (up, url) = upstream(&tmp.path().join("up"));
    let dest = tmp.path().join("engine");
    clone_shallow(&url, "release", &dest, &token(), &Reporter::detached())
        .await
        .unwrap();
    commit(&up, &[("Engine/a.txt", "2\n")], "second");
    std::fs::write(dest.join("Engine/a.txt"), "my change\n").unwrap();
    // Untracked files (build outputs) do not block updates by themselves.
    std::fs::write(dest.join("untracked.txt"), "x").unwrap();

    let err = update(&dest, "release", &token(), &Reporter::detached())
        .await
        .unwrap_err();
    match err {
        Error::LocalChanges { files, .. } => {
            assert_eq!(files, vec![std::path::PathBuf::from("Engine/a.txt")]);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(dest.join("Engine/a.txt")).unwrap(),
        "my change\n"
    );
}

#[tokio::test]
async fn update_of_non_repo_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let err = update(tmp.path(), "release", &token(), &Reporter::detached())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotARepository { .. }));
}
