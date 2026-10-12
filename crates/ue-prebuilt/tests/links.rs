// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the engine downloads from the links of `www.unrealengine.com/linux`
//! (what the web window reads from the page). The fixture has the same shape
//! as the real links, with fake signatures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ue_prebuilt::engines_from_links;

fn fixture() -> Vec<String> {
    serde_json::from_str(include_str!("fixtures/linux_page_links.json")).unwrap()
}

#[test]
fn finds_engine_zips_newest_first() {
    let links = fixture();
    let engines = engines_from_links(links.iter().map(String::as_str));
    let versions: Vec<_> = engines.iter().map(|e| e.version.as_str()).collect();
    assert_eq!(versions, ["5.10.0", "5.8.3", "5.7.4"]);
    assert_eq!(engines[1].file_name, "Linux_Unreal_Engine_5.8.3.zip");
    assert_eq!(engines[1].size, None);
    // The signed link is kept whole: its signature covers every parameter.
    assert!(engines[1].url().contains("fakesiga1b2c3"));
    assert!(engines[1].url().contains("response-content-disposition"));
}

#[test]
fn keeps_the_first_link_of_a_version() {
    let links = fixture();
    let engines = engines_from_links(links.iter().map(String::as_str));
    let v574: Vec<_> = engines.iter().filter(|e| e.version == "5.7.4").collect();
    assert_eq!(v574.len(), 1);
    assert!(v574[0].url().contains("fakesigd4e5f6"));
}

#[test]
fn reads_a_plain_filename_too() {
    let url = "https://x.invalid/b?X-Amz-Signature=s&response-content-disposition=attachment%3B%20filename%3D%22Linux_Unreal_Engine_5.6.1.zip%22";
    let engines = engines_from_links([url]);
    assert_eq!(engines.len(), 1);
    assert_eq!(engines[0].version, "5.6.1");
}

#[test]
fn nothing_from_a_page_without_engine_links() {
    assert!(
        engines_from_links(["https://www.unrealengine.com/en-US/eula", "not a url"]).is_empty()
    );
}

#[test]
fn debug_hides_the_signed_urls() {
    let links = fixture();
    let engines = engines_from_links(links.iter().map(String::as_str));
    assert!(!format!("{engines:?}").contains("fakesig"));
}

fn link_on(host_and_path: &str) -> String {
    format!(
        "{host_and_path}?response-content-disposition=attachment%3B%20filename%3D%22Linux_Unreal_Engine_5.8.3.zip%22"
    )
}

#[test]
fn only_https_links_on_epic_hosts_pass_the_check() {
    let allowed = [
        "https://ucs-blob-store.s3-accelerate.amazonaws.com/blobs/a",
        "https://cdn.unrealengine.com/a.zip",
        "https://downloads.epicgames.com/a.zip",
        "https://HOST.EpicGamesCDN.com/a.zip",
    ];
    let blocked = [
        "http://ucs-blob-store.s3-accelerate.amazonaws.com/a",
        "https://evil.net/a.zip",
        "https://epicgames.com.evil.net/a.zip",
        "https://evil.net/epicgames.com/a.zip",
        "https://notepicgames.com/a.zip",
        "https://amazonaws.com/a.zip",
        "https://other-bucket.s3-accelerate.amazonaws.com/a",
        "file:///etc/passwd",
    ];
    for url in allowed {
        let engines = engines_from_links([link_on(url).as_str()]);
        assert!(engines[0].check_url().is_ok(), "{url}");
    }
    for url in blocked {
        let engines = engines_from_links([link_on(url).as_str()]);
        let error = engines[0].check_url().unwrap_err();
        assert!(
            matches!(error, ue_prebuilt::Error::UntrustedDownloadUrl { .. }),
            "{url}"
        );
        assert!(!error.to_string().contains("response-content"), "{url}");
    }
}

#[tokio::test]
async fn an_untrusted_link_is_not_requested() {
    let engines = engines_from_links([link_on("http://127.0.0.1:9/a.zip").as_str()]);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("Linux_Unreal_Engine_5.8.3.zip");
    let result = ue_prebuilt::download_engine(
        &reqwest::Client::new(),
        &engines[0],
        &dest,
        &jobs::Reporter::detached(),
    )
    .await;
    assert!(matches!(
        result,
        Err(ue_prebuilt::Error::UntrustedDownloadUrl { .. })
    ));
    assert!(!dest.exists());
}
