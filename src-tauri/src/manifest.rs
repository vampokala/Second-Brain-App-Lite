//! SHA-256 keyed ingest manifest in user data dir.

use crate::atomic;
use crate::paths::user_data_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub content_sha256: String,
    pub last_ingest_at: String,
    #[serde(default)]
    pub wiki_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestManifest {
    /// Keyed by relative path from raw root using forward slashes.
    pub entries: HashMap<String, ManifestEntry>,
}

fn manifest_path() -> Result<PathBuf> {
    Ok(user_data_dir()?.join("ingest-manifest.json"))
}

pub fn load_manifest() -> Result<IngestManifest> {
    let p = manifest_path()?;
    if !p.exists() {
        return Ok(IngestManifest::default());
    }
    let s = std::fs::read_to_string(&p)?;
    Ok(serde_json::from_str(&s).unwrap_or_default())
}

pub fn save_manifest(m: &IngestManifest) -> Result<()> {
    let p = manifest_path()?;
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
