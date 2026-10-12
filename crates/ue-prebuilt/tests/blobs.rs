// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the list of Linux engine downloads that the web window fetches
//! from `www.unrealengine.com/api/blobs/linux`. The fixture has fake signed
//! URLs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ue_prebuilt::{Error, engines_from_blobs_json};

const FIXTURE: &str = include_str!("fixtures/blobs_linux.json");

#[test]
fn keeps_engine_zips_newest_first() {
    let engines = engines_from_blobs_json(FIXTURE).unwrap();
    let versions: Vec<_> = engines.iter().map(|e| e.version.as_str()).collect();
    assert_eq!(versions, ["5.7.4", "5.6.1"]);
    let newest = &engines[0];
    assert_eq!(newest.file_name, "Linux_Unreal_Engine_5.7.4.zip");
    assert_eq!(newest.size, Some(33_812_345_678));
    assert_eq!(
        newest.created_at.as_deref(),
        Some("2026-03-10T12:00:00.000Z")
    );
    assert!(newest.url().contains("fakesignature2"));
}

#[test]
fn debug_hides_the_signed_urls() {
    let engines = engines_from_blobs_json(FIXTURE).unwrap();
    let debug = format!("{engines:?}");
    assert!(!debug.contains("fakesignature"), "{debug}");
}

#[test]
fn an_empty_list_is_fine() {
    assert!(
        engines_from_blobs_json(r#"{"blobs":[]}"#)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_other_answers_without_quoting_them() {
    for body in [
        "<html>Just a moment...</html>",
        r#"{"errorCode":"errors.com.epicgames.unauthorized","url":"https://x/?X-Amz-Signature=secret"}"#,
        "[1, 2]",
    ] {
        let error = engines_from_blobs_json(body).unwrap_err();
        assert!(matches!(error, Error::UnexpectedBlobs), "{error:?}");
        assert!(!error.to_string().contains("secret"));
    }
}
