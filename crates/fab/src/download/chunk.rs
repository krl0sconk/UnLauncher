// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoding and verifying Epic chunk files (`*.chunk`).
//!
//! `egs-api` parses the chunk header and inflates the data; this module adds
//! the bounds checks it lacks and the verification it does not do.

use egs_api::api::types::chunk::Chunk;

use super::plan::ChunkSpec;

/// `FRollingHash` polynomial (CRC-64/ECMA reflected), as in Legendary's
/// `rolling_hash.py`.
const POLY: u64 = 0xC96C_5795_D787_0F42;

static TABLE: [u64; 256] = table();

#[allow(clippy::cast_possible_truncation, reason = "index < 256")]
const fn table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u64;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ POLY
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

/// Epic's rolling hash over a whole chunk (the value in the manifest's chunk
/// hash list and in the chunk file name).
pub(crate) fn rolling_hash(data: &[u8]) -> u64 {
    data.iter().fold(0u64, |hash, &byte| {
        hash.rotate_left(1) ^ TABLE[usize::from(byte)]
    })
}

/// Header sizes by chunk header version: v1 has 41 bytes, v2 adds a SHA-1 and
/// its hash type (62), v3 the uncompressed size (66).
fn min_header_len(raw: &[u8]) -> Option<usize> {
    let version = u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?);
    Some(match version {
        0 | 1 => 41,
        2 => 62,
        _ => 66,
    })
}

/// Decodes a downloaded chunk file and verifies it against the manifest.
///
/// Returns the uncompressed data, or `None` if the file is truncated, cannot
/// be inflated, has the wrong size or its rolling hash does not match. CPU
/// bound: call from a blocking thread.
pub(crate) fn decode(raw: Vec<u8>, spec: &ChunkSpec) -> Option<Vec<u8>> {
    // egs-api indexes the buffer without bounds checks.
    if raw.len() < min_header_len(&raw)? {
        return None;
    }
    let chunk = Chunk::from_vec(raw)?;
    if let Some(size) = chunk.uncompressed_size
        && usize::try_from(size).ok()? != chunk.data.len()
    {
        return None;
    }
    // hash_type: 1 = rolling, 2 = SHA-1, 3 = both; absent in v1 (rolling).
    let has_rolling = chunk.hash_type.is_none_or(|kind| kind & 1 != 0);
    if has_rolling && rolling_hash(&chunk.data) != spec.hash {
        return None;
    }
    if !has_rolling {
        // SHA-1 only: inflating succeeded and the size matched; UnLauncher
        // has no SHA-1 implementation to check further.
        tracing::debug!(guid = %spec.guid, "chunk has no rolling hash; size checked only");
    }
    Some(chunk.data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds an uncompressed version-1 chunk file around `data`.
    pub(crate) fn chunk_file(data: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&0xB1FE_3AA2_u32.to_le_bytes()); // magic
        raw.extend_from_slice(&1u32.to_le_bytes()); // header version
        raw.extend_from_slice(&41u32.to_le_bytes()); // header size
        raw.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
        raw.extend_from_slice(&[0u8; 16]); // guid
        raw.extend_from_slice(&rolling_hash(data).to_le_bytes());
        raw.push(0); // stored, not compressed
        raw.extend_from_slice(data);
        raw
    }

    pub(crate) fn spec(guid: &str, data: &[u8]) -> ChunkSpec {
        ChunkSpec {
            guid: guid.to_owned(),
            hash: rolling_hash(data),
            size: u64::try_from(chunk_file(data).len()).unwrap_or(0),
            url: format!("https://cdn.example.invalid/{guid}.chunk"),
        }
    }

    #[test]
    fn rolling_hash_matches_reference() {
        // Values computed with Legendary's algorithm (rolling_hash.get_hash).
        assert_eq!(rolling_hash(b""), 0);
        assert_eq!(rolling_hash(&[0]), 0);
        assert_eq!(rolling_hash(&[1]), 0xB32E_4CBE_03A7_5F6F);
        assert_eq!(rolling_hash(b"UnLauncher"), 0x681D_401C_7FCC_CAB4);
    }

    #[test]
    fn decodes_valid_chunk() {
        let data = b"hello chunk".to_vec();
        assert_eq!(decode(chunk_file(&data), &spec("a", &data)), Some(data));
    }

    #[test]
    fn rejects_wrong_hash_and_truncation() {
        let data = b"hello chunk".to_vec();
        let other = spec("a", b"something else");
        assert_eq!(decode(chunk_file(&data), &other), None);
        let raw = chunk_file(&data);
        assert_eq!(decode(raw[..20].to_vec(), &spec("a", &data)), None);
        assert_eq!(decode(Vec::new(), &spec("a", &data)), None);
        let mut bad_magic = raw;
        bad_magic[0] = 0;
        assert_eq!(decode(bad_magic, &spec("a", &data)), None);
    }
}
