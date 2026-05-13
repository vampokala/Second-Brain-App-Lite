//! SHA-256 keyed ingest manifest in user data dir.

use crate::atomic;
use crate::paths::user_data_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub content_sha256: String,
    pub last_ingest_at: String,
    #[serde(default)]
    pub wiki_paths: Vec<String>,
    /// When set with `source_mtime_ms`, ingest can skip reading/hashing the raw file if both match disk.
    #[serde(default)]
    pub source_size: Option<u64>,
    /// `modified()` time as milliseconds since Unix epoch (best-effort; 0 if unavailable).
    #[serde(default)]
    pub source_mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestManifest {
    /// Keyed by relative path from raw root using forward slashes.
    pub entries: HashMap<String, ManifestEntry>,
}

fn manifest_path_for(raw_root: &Path) -> Result<PathBuf> {
    let key = sha256_bytes(raw_root.to_string_lossy().as_bytes());
    Ok(user_data_dir()?.join(format!("ingest-manifest-{}.json", &key[..16])))
}

pub fn load_manifest_for(raw_root: &Path) -> Result<IngestManifest> {
    let p = manifest_path_for(raw_root)?;
    if !p.exists() {
        return Ok(IngestManifest::default());
    }
    let s = std::fs::read_to_string(&p)?;
    Ok(serde_json::from_str(&s).unwrap_or_default())
}

pub fn save_manifest_for(raw_root: &Path, m: &IngestManifest) -> Result<()> {
    let p = manifest_path_for(raw_root)?;
    std::fs::create_dir_all(p.parent().unwrap())?;
    let json = serde_json::to_string_pretty(m)?;
    atomic::atomic_write(&p, json.as_bytes())
}

pub fn sha256_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// File size and modification time for incremental ingest (avoid re-reading unchanged raw files).
pub fn raw_file_stamp(path: &Path) -> std::io::Result<(u64, i64)> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();
    let ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    Ok((len, ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_file_stamp_matches_len() {
        let dir = std::env::temp_dir().join(format!(
            "sb-lite-stamp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.txt");
        std::fs::write(&p, b"hello").unwrap();
        let (sz, _ms) = raw_file_stamp(&p).unwrap();
        assert_eq!(sz, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
