// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the asset files from verified chunks.

use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use jobs::Reporter;

use super::chunk::decode;
use super::plan::{ChunkSpec, Plan};
use super::{VersionDirs, chunk_path};
use crate::error::{Error, Result};

/// Decoded chunks kept in memory while assembling (chunks are ~1 MiB).
const CACHE_CHUNKS: usize = 64;

/// Writes every file of `plan` into a temporary folder next to
/// `dirs.files()`, then renames it into place. Blocking.
pub(crate) fn assemble(plan: &Plan, dirs: &VersionDirs, reporter: &Reporter) -> Result<()> {
    let partial = dirs.partial_files();
    let files = dirs.files();
    remove_dir_if_exists(&partial)?;
    fs::create_dir_all(&partial).map_err(|e| Error::io("create folder", &partial, e))?;

    let specs: HashMap<&str, &ChunkSpec> = plan
        .chunks
        .iter()
        .map(|chunk| (chunk.guid.as_str(), chunk))
        .collect();
    let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
    let total = plan.files.len().max(1);

    for (index, file) in plan.files.iter().enumerate() {
        reporter.checkpoint_blocking()?;
        let target = partial.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io("create folder", parent, e))?;
        }
        let handle = fs::File::create(&target).map_err(|e| Error::io("write", &target, e))?;
        let mut out = BufWriter::new(handle);
        for part in &file.parts {
            if !cache.contains_key(&part.guid) {
                let spec = specs
                    .get(part.guid.as_str())
                    .ok_or_else(|| Error::api_changed("file part without chunk"))?;
                if cache.len() >= CACHE_CHUNKS {
                    cache.clear();
                }
                cache.insert(part.guid.clone(), load_chunk(dirs, spec)?);
            }
            let data = cache
                .get(&part.guid)
                .and_then(|data| data.get(part.offset..part.offset.checked_add(part.size)?))
                .ok_or_else(|| Error::api_changed("file part outside its chunk"))?;
            out.write_all(data)
                .map_err(|e| Error::io("write", &target, e))?;
        }
        out.flush().map_err(|e| Error::io("write", &target, e))?;
        #[allow(clippy::cast_precision_loss, reason = "progress only")]
        reporter.fraction(Some((index + 1) as f32 / total as f32));
    }

    remove_dir_if_exists(&files)?;
    fs::rename(&partial, &files).map_err(|e| Error::io("rename", &partial, e))
}

fn load_chunk(dirs: &VersionDirs, spec: &ChunkSpec) -> Result<Vec<u8>> {
    let path = chunk_path(&dirs.chunks(), &spec.guid);
    let raw = fs::read(&path).map_err(|e| Error::io("read", &path, e))?;
    decode(raw, spec).ok_or_else(|| Error::ChunkCorrupt {
        guid: spec.guid.clone(),
    })
}

pub(crate) fn remove_dir_if_exists(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io("remove", path, e)),
    }
}
