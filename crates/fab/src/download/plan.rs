// SPDX-License-Identifier: GPL-3.0-or-later

//! What to download and how to put files together, from a parsed manifest.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use egs_api::api::types::download_manifest::DownloadManifest;

use crate::error::{Error, Result};

/// A chunk to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkSpec {
    /// Chunk GUID, as written in the manifest.
    pub guid: String,
    /// Expected rolling hash of the uncompressed data.
    pub hash: u64,
    /// Size of the chunk file (for progress); 0 if unknown.
    pub size: u64,
    /// Where to download it.
    pub url: String,
}

/// A slice of a chunk that goes into a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartSpec {
    pub guid: String,
    pub offset: usize,
    pub size: usize,
}

/// A file to assemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSpec {
    /// Relative path, checked to stay inside the destination.
    pub path: PathBuf,
    pub parts: Vec<PartSpec>,
}

/// Everything needed to download and assemble one asset version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Unique chunks, sorted by GUID.
    pub chunks: Vec<ChunkSpec>,
    /// Files, sorted by path.
    pub files: Vec<FileSpec>,
}

impl Plan {
    /// Sum of the chunk file sizes.
    pub fn total_bytes(&self) -> u64 {
        self.chunks.iter().map(|chunk| chunk.size).sum()
    }
}

/// Builds the plan from a manifest whose `SourceURL` custom field is set.
///
/// # Errors
///
/// [`Error::UnsafePath`] if a file name leaves the destination,
/// [`Error::ApiChanged`] if chunks, hashes or links are missing.
pub(crate) fn plan_from_manifest(manifest: &DownloadManifest) -> Result<Plan> {
    let mut files = Vec::new();
    let mut chunks = BTreeMap::new();
    let linked = manifest.files();
    for listed in &manifest.file_manifest_list {
        let path = safe_relative_path(&listed.filename)?;
        let file = linked
            .get(&listed.filename)
            .ok_or_else(|| Error::api_changed("manifest file without chunk list"))?;
        // egs-api silently drops parts whose chunk has no download link.
        if file.file_chunk_parts.len() != listed.file_chunk_parts.len() {
            return Err(Error::api_changed("manifest chunk without download link"));
        }
        let mut parts = Vec::with_capacity(file.file_chunk_parts.len());
        for part in &file.file_chunk_parts {
            if !chunks.contains_key(&part.guid) {
                let url = part
                    .link
                    .as_ref()
                    .ok_or_else(|| Error::api_changed("manifest chunk without download link"))?;
                chunks.insert(
                    part.guid.clone(),
                    chunk_spec(manifest, &part.guid, url.as_str())?,
                );
            }
            parts.push(PartSpec {
                guid: part.guid.clone(),
                offset: to_usize(part.offset)?,
                size: to_usize(part.size)?,
            });
        }
        files.push(FileSpec { path, parts });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Plan {
        chunks: chunks.into_values().collect(),
        files,
    })
}

fn chunk_spec(manifest: &DownloadManifest, guid: &str, url: &str) -> Result<ChunkSpec> {
    let hash = manifest
        .chunk_hash_list
        .get(guid)
        .and_then(|hash| u64::try_from(*hash).ok())
        .ok_or_else(|| Error::api_changed("manifest chunk without hash"))?;
    let size = manifest
        .chunk_filesize_list
        .get(guid)
        .and_then(|size| u64::try_from(*size).ok())
        .unwrap_or(0);
    Ok(ChunkSpec {
        guid: guid.to_owned(),
        hash,
        size,
        url: url.to_owned(),
    })
}

fn to_usize(value: u128) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::api_changed("manifest chunk part too large"))
}

/// Turns a manifest file name (`/` or `\` separated) into a relative path that
/// cannot leave the destination folder.
pub(crate) fn safe_relative_path(name: &str) -> Result<PathBuf> {
    let unsafe_path = || Error::UnsafePath {
        path: name.to_owned(),
    };
    if name.starts_with(['/', '\\']) {
        return Err(unsafe_path());
    }
    let mut path = PathBuf::new();
    for part in name.split(['/', '\\']).filter(|part| !part.is_empty()) {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(normal)), None) if !part.contains(':') => path.push(normal),
            _ => return Err(unsafe_path()),
        }
    }
    if path.as_os_str().is_empty() {
        return Err(unsafe_path());
    }
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;

    use egs_api::api::types::download_manifest::{FileChunkPart, FileManifestList};

    use super::*;

    fn part(guid: &str, offset: u128, size: u128) -> FileChunkPart {
        FileChunkPart {
            guid: guid.to_owned(),
            link: None,
            offset,
            size,
        }
    }

    /// A manifest built by hand: two files sharing chunk `AAAA`.
    fn manifest() -> DownloadManifest {
        let mut manifest = DownloadManifest {
            manifest_file_version: 21,
            ..DownloadManifest::default()
        };
        manifest.file_manifest_list = vec![
            FileManifestList {
                filename: "Handy/Handy.uplugin".to_owned(),
                file_hash: String::new(),
                file_chunk_parts: vec![part("AAAA", 0, 4)],
            },
            FileManifestList {
                filename: "Handy/Content/Rock.uasset".to_owned(),
                file_hash: String::new(),
                file_chunk_parts: vec![part("AAAA", 4, 2), part("BBBB", 0, 3)],
            },
        ];
        manifest.chunk_hash_list =
            HashMap::from([("AAAA".to_owned(), 10), ("BBBB".to_owned(), 11)]);
        manifest.data_group_list = HashMap::from([("AAAA".to_owned(), 1), ("BBBB".to_owned(), 2)]);
        manifest.chunk_filesize_list =
            HashMap::from([("AAAA".to_owned(), 100), ("BBBB".to_owned(), 50)]);
        manifest.custom_fields = Some(HashMap::from([(
            "SourceURL".to_owned(),
            "https://cdn.example.invalid/Builds/x".to_owned(),
        )]));
        manifest
    }

    #[test]
    fn builds_plan_with_unique_chunks() {
        let plan = plan_from_manifest(&manifest()).unwrap();
        assert_eq!(plan.chunks.len(), 2);
        assert_eq!(plan.chunks[0].guid, "AAAA");
        assert_eq!(plan.chunks[0].hash, 10);
        assert!(plan.chunks[0].url.contains("ChunksV4/01/"));
        assert!(plan.chunks[0].url.ends_with("_AAAA.chunk"));
        assert_eq!(plan.total_bytes(), 150);
        assert_eq!(plan.files[0].path, Path::new("Handy/Content/Rock.uasset"));
        assert_eq!(plan.files[0].parts.len(), 2);
    }

    #[test]
    fn missing_source_url_is_an_api_change() {
        let mut manifest = manifest();
        manifest.custom_fields = None;
        assert!(matches!(
            plan_from_manifest(&manifest),
            Err(Error::ApiChanged { .. })
        ));
    }

    #[test]
    fn rejects_escaping_paths() {
        for name in [
            "../evil",
            "/abs",
            "\\abs",
            "a/../../b",
            "C:\\x",
            "a/./b",
            "",
        ] {
            assert!(safe_relative_path(name).is_err(), "{name}");
        }
        assert_eq!(
            safe_relative_path("Content\\Maps//Level.umap").unwrap(),
            Path::new("Content/Maps/Level.umap")
        );
    }
}
