// SPDX-License-Identifier: GPL-3.0-or-later

//! Resumable, verified downloads of an asset version.
//!
//! Layout under the cache folder:
//!
//! ```text
//! <cache_dir>/<asset id>/<version id>/
//!     chunks/<GUID>.chunk    downloaded chunk files, verified before renaming
//!     files.partial/         files being assembled
//!     files/                 the finished asset
//!     complete               marker: files/ is finished (chunks/ is removed)
//! ```

mod assemble;
pub(crate) mod chunk;
pub(crate) mod plan;

use std::path::{Path, PathBuf};

use jobs::Reporter;

use crate::asset::AssetKind;
use crate::error::{Error, Result};
use crate::install::detect_kind;
use crate::source::BoxFuture;
use plan::{ChunkSpec, Plan};

/// A downloaded asset version, ready for [`crate::install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedAsset {
    /// The folder with the asset files (`…/files`).
    pub path: PathBuf,
    /// The declared kind, or the one recognised from the files if Fab did not
    /// declare it.
    pub kind: AssetKind,
}

/// Folders of one asset version in the cache.
#[derive(Debug, Clone)]
pub(crate) struct VersionDirs {
    root: PathBuf,
}

impl VersionDirs {
    pub fn new(cache_dir: &Path, asset_id: &str, version_id: &str) -> Self {
        Self {
            root: cache_dir
                .join(file_name(asset_id))
                .join(file_name(version_id)),
        }
    }

    pub fn chunks(&self) -> PathBuf {
        self.root.join("chunks")
    }

    pub fn files(&self) -> PathBuf {
        self.root.join("files")
    }

    pub fn partial_files(&self) -> PathBuf {
        self.root.join("files.partial")
    }

    fn marker(&self) -> PathBuf {
        self.root.join("complete")
    }
}

/// Makes an id usable as a single path component.
fn file_name(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

pub(crate) fn chunk_path(chunks_dir: &Path, guid: &str) -> PathBuf {
    chunks_dir.join(format!("{}.chunk", file_name(guid)))
}

/// Downloads raw bytes; the real one uses `reqwest`, tests use a fake.
pub(crate) trait Fetch: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<u8>>>;
}

/// [`Fetch`] over the shared HTTP client.
pub(crate) struct HttpFetch(pub reqwest::Client);

impl Fetch for HttpFetch {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let response = self
                .0
                .get(url)
                .send()
                .await
                .map_err(|e| Error::network("download", e))?;
            let status = response.status();
            if !status.is_success() {
                return Err(Error::Http {
                    what: "download".to_owned(),
                    status: status.as_u16(),
                });
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::network("download", e))?;
            Ok(bytes.to_vec())
        })
    }
}

/// The finished download, if a previous run completed it.
pub(crate) async fn completed(dirs: &VersionDirs, kind: AssetKind) -> Option<DownloadedAsset> {
    let files = dirs.files();
    let done = tokio::fs::try_exists(dirs.marker()).await.unwrap_or(false)
        && tokio::fs::try_exists(&files).await.unwrap_or(false);
    done.then(|| DownloadedAsset {
        kind: resolve_kind(kind, &files),
        path: files,
    })
}

fn resolve_kind(declared: AssetKind, files: &Path) -> AssetKind {
    match declared {
        AssetKind::Unknown => detect_kind(files),
        known => known,
    }
}

/// Downloads the missing chunks of `plan`, assembles the files and marks the
/// version complete.
pub(crate) async fn run(
    plan: Plan,
    dirs: VersionDirs,
    fetch: &dyn Fetch,
    reporter: &Reporter,
    kind: AssetKind,
) -> Result<DownloadedAsset> {
    let chunks_dir = dirs.chunks();
    tokio::fs::create_dir_all(&chunks_dir)
        .await
        .map_err(|e| Error::io("create folder", &chunks_dir, e))?;

    reporter.stage("Downloading");
    let total = plan.total_bytes();
    let count = plan.chunks.len().max(1);
    let mut done_bytes = 0u64;
    for (index, spec) in plan.chunks.iter().enumerate() {
        reporter.checkpoint().await?;
        let path = chunk_path(&chunks_dir, &spec.guid);
        if is_valid_on_disk(&path, spec).await {
            tracing::trace!(guid = %spec.guid, "chunk already downloaded");
        } else {
            download_chunk(fetch, spec, &path).await?;
        }
        done_bytes += spec.size;
        #[allow(clippy::cast_precision_loss, reason = "progress only")]
        let fraction = if total > 0 {
            done_bytes as f32 / total as f32
        } else {
            (index + 1) as f32 / count as f32
        };
        reporter.fraction(Some(fraction));
    }

    reporter.stage("Assembling");
    let assemble_reporter = reporter.clone();
    let assemble_dirs = dirs.clone();
    tokio::task::spawn_blocking(move || {
        assemble::assemble(&plan, &assemble_dirs, &assemble_reporter)
    })
    .await
    .map_err(|_| Error::api_changed("assembling the files failed unexpectedly"))??;

    let marker = dirs.marker();
    tokio::fs::write(&marker, b"")
        .await
        .map_err(|e| Error::io("write", &marker, e))?;
    if let Err(error) = tokio::fs::remove_dir_all(&chunks_dir).await {
        tracing::warn!(path = %chunks_dir.display(), %error, "cannot remove downloaded chunks");
    }
    let files = dirs.files();
    tracing::info!(path = %files.display(), "asset downloaded");
    Ok(DownloadedAsset {
        kind: resolve_kind(kind, &files),
        path: files,
    })
}

async fn is_valid_on_disk(path: &Path, spec: &ChunkSpec) -> bool {
    let path = path.to_owned();
    let spec = spec.clone();
    tokio::task::spawn_blocking(move || {
        std::fs::read(&path)
            .ok()
            .and_then(|raw| chunk::decode(raw, &spec))
            .is_some()
    })
    .await
    .unwrap_or(false)
}

/// Downloads and verifies a chunk (one retry), then writes it atomically.
async fn download_chunk(fetch: &dyn Fetch, spec: &ChunkSpec, path: &Path) -> Result<()> {
    for attempt in 1..=2 {
        let raw = fetch.fetch(&spec.url).await?;
        let check = spec.clone();
        let (is_valid, raw) = tokio::task::spawn_blocking(move || {
            let is_valid = chunk::decode(raw.clone(), &check).is_some();
            (is_valid, raw)
        })
        .await
        .map_err(|_| Error::ChunkCorrupt {
            guid: spec.guid.clone(),
        })?;
        if is_valid {
            let partial = path.with_extension("chunk.part");
            tokio::fs::write(&partial, &raw)
                .await
                .map_err(|e| Error::io("write", &partial, e))?;
            return tokio::fs::rename(&partial, path)
                .await
                .map_err(|e| Error::io("rename", &partial, e));
        }
        tracing::warn!(guid = %spec.guid, attempt, "downloaded chunk failed verification");
    }
    Err(Error::ChunkCorrupt {
        guid: spec.guid.clone(),
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
