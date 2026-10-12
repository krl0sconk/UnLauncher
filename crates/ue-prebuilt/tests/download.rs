// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the resumable engine download, against a local fake server.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jobs::{Control, Reporter};
use tempfile::{TempDir, tempdir};
use ue_prebuilt::{DOWNLOADING_STAGE, Error, download_file};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BODY: &[u8] = b"0123456789abcdefghij";
/// Size of [`BODY`], as passed to `download_file`.
const LEN: Option<u64> = Some(BODY.len() as u64);
const FILE: &str = "Linux_Unreal_Engine_5.7.3.zip";

struct Setup {
    server: MockServer,
    _dir: TempDir,
    dest: PathBuf,
    part: PathBuf,
}

impl Setup {
    async fn new() -> Self {
        let dir = tempdir().unwrap();
        let dest = dir.path().join(FILE);
        let part = dir.path().join(format!("{FILE}.part"));
        Self {
            server: MockServer::start().await,
            _dir: dir,
            dest,
            part,
        }
    }

    fn url(&self) -> String {
        format!("{}/blob/{FILE}?X-Amz-Signature=secret", self.server.uri())
    }

    async fn download(&self, expected: Option<u64>, reporter: &Reporter) -> Result<PathBuf, Error> {
        let http = reqwest::Client::new();
        download_file(&http, &self.url(), &self.dest, expected, reporter).await
    }
}

fn full_body() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_bytes(BODY)
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[tokio::test]
async fn downloads_the_whole_file() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path(format!("/blob/{FILE}")))
        .respond_with(full_body())
        .expect(1)
        .mount(&s.server)
        .await;
    let (reporter, handle) = jobs::channel();

    let out = s.download(LEN, &reporter).await.unwrap();

    assert_eq!(out, s.dest);
    assert_eq!(read(&s.dest), BODY);
    assert!(!s.part.exists());
    let progress = handle.progress();
    assert_eq!(progress.stage, DOWNLOADING_STAGE);
    assert_eq!(progress.fraction, Some(1.0));
    assert_eq!(progress.message.as_deref(), Some(FILE));
}

#[tokio::test]
async fn downloads_without_a_known_size() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .respond_with(full_body())
        .mount(&s.server)
        .await;

    s.download(None, &Reporter::detached()).await.unwrap();

    assert_eq!(read(&s.dest), BODY);
}

#[tokio::test]
async fn resumes_with_range_when_the_server_answers_206() {
    let s = Setup::new().await;
    fs::write(&s.part, &BODY[..4]).unwrap();
    Mock::given(method("GET"))
        .and(header("range", "bytes=4-"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", format!("bytes 4-19/{}", BODY.len()))
                .set_body_bytes(&BODY[4..]),
        )
        .expect(1)
        .mount(&s.server)
        .await;

    s.download(None, &Reporter::detached()).await.unwrap();

    assert_eq!(read(&s.dest), BODY);
    assert!(!s.part.exists());
}

#[tokio::test]
async fn restarts_cleanly_when_the_server_ignores_range() {
    let s = Setup::new().await;
    fs::write(&s.part, b"XXXX").unwrap();
    Mock::given(method("GET"))
        .respond_with(full_body())
        .expect(1)
        .mount(&s.server)
        .await;

    s.download(LEN, &Reporter::detached()).await.unwrap();

    assert_eq!(read(&s.dest), BODY);
}

#[tokio::test]
async fn restarts_when_the_range_is_not_satisfiable() {
    let s = Setup::new().await;
    fs::write(&s.part, b"XXXX").unwrap();
    Mock::given(method("GET"))
        .and(header("range", "bytes=4-"))
        .respond_with(ResponseTemplate::new(416))
        .expect(1)
        .mount(&s.server)
        .await;
    Mock::given(method("GET"))
        .respond_with(full_body())
        .mount(&s.server)
        .await;

    s.download(None, &Reporter::detached()).await.unwrap();

    assert_eq!(read(&s.dest), BODY);
}

#[tokio::test]
async fn short_download_is_a_size_mismatch_and_keeps_the_part() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .respond_with(full_body())
        .mount(&s.server)
        .await;

    let err = s
        .download(Some(100), &Reporter::detached())
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            Error::SizeMismatch {
                expected: 100,
                actual: 20,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!s.dest.exists());
    assert_eq!(read(&s.part), BODY);
}

#[tokio::test]
async fn longer_download_is_a_size_mismatch_and_drops_the_part() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .respond_with(full_body())
        .mount(&s.server)
        .await;

    let err = s
        .download(Some(5), &Reporter::detached())
        .await
        .unwrap_err();

    assert!(matches!(err, Error::SizeMismatch { .. }), "{err:?}");
    assert!(!s.dest.exists());
    assert!(!s.part.exists());
}

#[tokio::test]
async fn cancel_keeps_the_part_file() {
    let s = Setup::new().await;
    fs::write(&s.part, &BODY[..4]).unwrap();
    Mock::given(method("GET"))
        .respond_with(full_body())
        .expect(0)
        .mount(&s.server)
        .await;
    let (reporter, handle) = jobs::channel();
    handle.send(Control::Cancel);

    let err = s.download(LEN, &reporter).await.unwrap_err();

    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert_eq!(read(&s.part), &BODY[..4]);
    assert!(!s.dest.exists());
}

#[tokio::test]
async fn pause_waits_and_resume_finishes() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .respond_with(full_body())
        .mount(&s.server)
        .await;
    let (reporter, handle) = jobs::channel();
    handle.send(Control::Pause);
    let http = reqwest::Client::new();
    let (url, dest) = (s.url(), s.dest.clone());
    let task = tokio::spawn(async move { download_file(&http, &url, &dest, LEN, &reporter).await });

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!task.is_finished());
    assert!(!s.dest.exists());
    handle.send(Control::Run);

    task.await.unwrap().unwrap();
    assert_eq!(read(&s.dest), BODY);
}

#[tokio::test]
async fn never_overwrites_an_existing_destination() {
    let s = Setup::new().await;
    fs::write(&s.dest, b"mine").unwrap();
    Mock::given(method("GET"))
        .respond_with(full_body())
        .expect(0)
        .mount(&s.server)
        .await;

    let err = s.download(LEN, &Reporter::detached()).await.unwrap_err();

    assert!(matches!(err, Error::DestinationExists { .. }), "{err:?}");
    assert_eq!(read(&s.dest), b"mine");
}

#[tokio::test]
async fn complete_part_is_finished_without_a_request() {
    let s = Setup::new().await;
    fs::write(&s.part, BODY).unwrap();
    Mock::given(method("GET"))
        .respond_with(full_body())
        .expect(0)
        .mount(&s.server)
        .await;

    s.download(LEN, &Reporter::detached()).await.unwrap();

    assert_eq!(read(&s.dest), BODY);
}

#[tokio::test]
async fn http_errors_keep_the_part_and_hide_the_url() {
    let s = Setup::new().await;
    fs::write(&s.part, &BODY[..4]).unwrap();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&s.server)
        .await;

    let err = s.download(LEN, &Reporter::detached()).await.unwrap_err();

    assert!(
        matches!(err, Error::HttpStatus { status: 403, .. }),
        "{err:?}"
    );
    assert!(!format!("{err} {err:?}").contains("secret"));
    assert_eq!(read(&s.part), &BODY[..4]);
}

#[tokio::test]
async fn network_errors_hide_the_signed_url() {
    let s = Setup::new().await;
    let http = reqwest::Client::new();
    // Nothing listens on port 9 of localhost.
    let url = "http://127.0.0.1:9/blob?X-Amz-Signature=secret";

    let err = download_file(&http, url, &s.dest, LEN, &Reporter::detached())
        .await
        .unwrap_err();

    assert!(matches!(err, Error::Download { .. }), "{err:?}");
    let mut text = format!("{err:?}");
    let mut source = std::error::Error::source(&err);
    while let Some(inner) = source {
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    assert!(!text.contains("secret"), "{text}");
}

#[tokio::test]
async fn refuses_a_file_bigger_than_the_free_space() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path(format!("/blob/{FILE}")))
        .respond_with(full_body())
        .mount(&s.server)
        .await;
    let (reporter, _handle) = jobs::channel();

    // No disk has an exabyte free.
    let error = s.download(Some(1 << 60), &reporter).await.unwrap_err();

    assert!(matches!(error, Error::NotEnoughSpace { .. }), "{error:?}");
    assert!(!s.dest.exists());
    assert_eq!(fs::metadata(&s.part).map_or(0, |m| m.len()), 0);
}
