// SPDX-License-Identifier: GPL-3.0-or-later

//! Listing and downloading Epic's prebuilt Linux engine releases.
//!
//! The Linux page (<https://www.unrealengine.com/linux>) sits behind
//! Cloudflare and needs Epic's web session. Signed in, it links every release
//! to a signed S3 URL; the interface reads those links from inside the
//! built-in web window (`web-bridge`) and hands them to
//! [`engines_from_links`]. [`engines_from_blobs_json`] reads the JSON API the
//! page used before (`/api/blobs/linux`, which now answers "User not
//! authenticated" even with a session).

mod file;

use std::cmp::Ordering;
use std::fmt;

use egs_api::api::types::engine_blob::{EngineBlob, EngineBlobsResponse};

use crate::error::{Error, Result};

pub use file::{DOWNLOADING_STAGE, download_file};

/// The exact host of Epic's signed engine downloads (an S3 bucket).
const BLOB_STORE_HOST: &str = "ucs-blob-store.s3-accelerate.amazonaws.com";
/// Domains whose subdomains may also serve an engine (Epic's own CDN names).
const TRUSTED_DOMAINS: [&str; 3] = ["epicgames.com", "unrealengine.com", "epicgamescdn.com"];

/// File name prefix of the engine zips (other blobs are Fab/Bridge plugins).
const ENGINE_PREFIX: &str = "linux_unreal_engine_";
/// File name suffix of the engine zips.
const ZIP_SUFFIX: &str = ".zip";

/// A prebuilt engine release offered for download on Epic's Linux page.
///
/// The download URL is presigned and expires after a while: use it soon
/// after listing, and never log it. [`fmt::Debug`] hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct EngineDownload {
    /// Engine version taken from the file name, e.g. `"5.7.3"`.
    pub version: String,
    /// File name, e.g. `"Linux_Unreal_Engine_5.7.3.zip"`.
    pub file_name: String,
    /// Size in bytes, if Epic reported it.
    pub size: Option<u64>,
    /// Upload date as Epic reports it (ISO 8601), if any.
    pub created_at: Option<String>,
    url: String,
}

impl EngineDownload {
    /// The presigned download URL. Treat it as a secret: do not log or store it.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Checks that the download URL is `https` and points to a host Epic
    /// serves engines from: its blob store, or a subdomain of Epic's own
    /// domains. The links come from a web page, so nothing else is fetched.
    ///
    /// # Errors
    ///
    /// [`Error::UntrustedDownloadUrl`] otherwise. It names the host only,
    /// never the (signed) URL.
    pub fn check_url(&self) -> Result<()> {
        check_download_url(&self.url, &self.file_name)
    }

    /// Converts one of Epic's blobs, or `None` if it is not a Linux engine zip
    /// (for example a Fab or Bridge plugin).
    fn from_blob(blob: EngineBlob) -> Option<Self> {
        let version = engine_zip_version(&blob.name)?;
        if blob.url.is_empty() {
            return None;
        }
        Some(Self {
            version,
            file_name: blob.name,
            size: (blob.size > 0).then_some(blob.size),
            created_at: (!blob.created_at.is_empty()).then_some(blob.created_at),
            url: blob.url,
        })
    }
}

impl fmt::Debug for EngineDownload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineDownload")
            .field("version", &self.version)
            .field("file_name", &self.file_name)
            .field("size", &self.size)
            .field("created_at", &self.created_at)
            .field("url", &"[redacted]")
            .finish()
    }
}

/// Whether `url` is an `https` URL on a host engines are downloaded from.
fn check_download_url(url: &str, file_name: &str) -> Result<()> {
    let untrusted = |host: String| Error::UntrustedDownloadUrl {
        file_name: file_name.to_owned(),
        host,
    };
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return Err(untrusted("(invalid URL)".to_owned()));
    };
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let trusted = parsed.scheme() == "https"
        && (host == BLOB_STORE_HOST
            || TRUSTED_DOMAINS.iter().any(|domain| {
                host.strip_suffix(domain)
                    .is_some_and(|rest| rest.ends_with('.'))
            }));
    if trusted {
        Ok(())
    } else {
        Err(untrusted(host))
    }
}

/// Downloads `engine` to `dest` like [`download_file`], after checking its URL
/// with [`EngineDownload::check_url`]. Nothing is requested if the check fails.
///
/// # Errors
///
/// [`Error::UntrustedDownloadUrl`], plus everything [`download_file`] returns.
pub async fn download_engine(
    http: &reqwest::Client,
    engine: &EngineDownload,
    dest: &std::path::Path,
    reporter: &jobs::Reporter,
) -> Result<std::path::PathBuf> {
    engine.check_url()?;
    download_file(http, &engine.url, dest, engine.size, reporter).await
}

/// The page path, relative to `www.unrealengine.com`, that lists the Linux
/// downloads.
pub const BLOBS_PATH: &str = "/api/blobs/linux";

/// Reads the answer of [`BLOBS_PATH`] (`{"blobs": [{name, createdAt, size,
/// url}]}`): the Linux engine zips, newest version first. Fab and Bridge
/// plugins are left out.
///
/// # Errors
///
/// [`Error::UnexpectedBlobs`] if the body does not have that shape (for
/// example Cloudflare's page or an error from Epic). The error never quotes
/// the body, which holds signed URLs.
pub fn engines_from_blobs_json(body: &str) -> Result<Vec<EngineDownload>> {
    let response: EngineBlobsResponse =
        serde_json::from_str(body).map_err(|_| Error::UnexpectedBlobs)?;
    let engines = engines_from_blobs(response.blobs);
    tracing::info!(count = engines.len(), "listed Linux engine downloads");
    Ok(engines)
}

/// The engine downloads among the links of `www.unrealengine.com/linux`,
/// newest version first.
///
/// The page links every release to a signed S3 URL whose
/// `response-content-disposition` parameter names the file
/// (`filename*=UTF-8''Linux_Unreal_Engine_5.8.3.zip`). Links to other files
/// (Fab, Bridge, documentation) are left out, and only the first link of a
/// version is kept. Sizes are unknown here; the download learns them from
/// the server.
pub fn engines_from_links<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<EngineDownload> {
    let mut engines: Vec<EngineDownload> = Vec::new();
    for url in urls {
        let Some(file_name) = disposition_file_name(url) else {
            continue;
        };
        let Some(version) = engine_zip_version(&file_name) else {
            continue;
        };
        if engines.iter().any(|e| e.version == version) {
            continue;
        }
        engines.push(EngineDownload {
            version,
            file_name,
            size: None,
            created_at: None,
            url: url.to_owned(),
        });
    }
    engines.sort_by(newest_first);
    tracing::info!(
        count = engines.len(),
        "found Linux engine downloads on the page"
    );
    engines
}

/// The file name in a signed URL's `response-content-disposition`
/// parameter: `filename*=UTF-8''…` (percent-encoded) or `filename="…"`.
fn disposition_file_name(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let disposition = url
        .query_pairs()
        .find(|(key, _)| key == "response-content-disposition")?
        .1
        .into_owned();
    let mut plain = None;
    for part in disposition.split(';').map(str::trim) {
        if let Some(encoded) = part.strip_prefix("filename*=") {
            let encoded = encoded.split_once("''").map_or(encoded, |(_, name)| name);
            return percent_decode(encoded);
        }
        if let Some(name) = part.strip_prefix("filename=") {
            plain = Some(name.trim_matches('"').to_owned());
        }
    }
    plain
}

/// Decodes `%XX` escapes; `None` if the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Keeps the Linux engine zips and sorts them newest version first.
fn engines_from_blobs(blobs: Vec<EngineBlob>) -> Vec<EngineDownload> {
    let mut engines: Vec<_> = blobs
        .into_iter()
        .filter_map(EngineDownload::from_blob)
        .collect();
    engines.sort_by(newest_first);
    engines
}

fn newest_first(a: &EngineDownload, b: &EngineDownload) -> Ordering {
    version_key(&b.version)
        .cmp(&version_key(&a.version))
        .then_with(|| b.created_at.cmp(&a.created_at))
}

/// The engine version in the name of one of Epic's Linux engine zips
/// (`Linux_Unreal_Engine_5.7.4.zip` → `5.7.4`), or `None` for any other file.
pub(crate) fn engine_zip_version(file_name: &str) -> Option<String> {
    let lower = file_name.to_ascii_lowercase();
    let version = lower
        .strip_prefix(ENGINE_PREFIX)?
        .strip_suffix(ZIP_SUFFIX)?;
    (!version_key(version).is_empty()).then(|| version.to_owned())
}

/// Numeric components of a version (`"5.7.3"` → `[5, 7, 3]`); empty if the
/// string is not a dotted number.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<Vec<u64>, _>>()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(name: &str, created_at: &str) -> EngineBlob {
        EngineBlob {
            name: name.to_owned(),
            created_at: created_at.to_owned(),
            size: 42,
            url: format!("https://example.invalid/{name}?X-Amz-Signature=secret"),
        }
    }

    #[test]
    fn keeps_engine_zips_newest_first() {
        let engines = engines_from_blobs(vec![
            blob("Linux_Unreal_Engine_5.6.1.zip", "2025-08-01T00:00:00Z"),
            blob("Linux_Fab_5.7.zip", "2026-01-01T00:00:00Z"),
            blob("Linux_Unreal_Engine_5.10.0.zip", "2026-09-01T00:00:00Z"),
            blob("Linux_Unreal_Engine_5.7.3.zip", "2026-01-15T00:00:00Z"),
            blob("Linux_Unreal_Engine_notes.zip", "2026-01-15T00:00:00Z"),
        ]);
        let versions: Vec<_> = engines.iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, ["5.10.0", "5.7.3", "5.6.1"]);
        assert_eq!(engines[1].file_name, "Linux_Unreal_Engine_5.7.3.zip");
        assert_eq!(engines[1].size, Some(42));
    }

    #[test]
    fn debug_hides_the_signed_url() {
        let engines = engines_from_blobs(vec![blob("Linux_Unreal_Engine_5.7.3.zip", "")]);
        let debug = format!("{:?}", engines[0]);
        assert!(!debug.contains("secret"), "{debug}");
        assert!(debug.contains("[redacted]"));
        assert!(engines[0].url().contains("secret"));
        assert_eq!(engines[0].created_at, None);
    }
}
