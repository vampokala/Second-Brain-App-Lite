//! Resolve configured dirs with env overrides.

use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn resolve_dirs_from_env() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let vault = std::env::var("SECOND_BRAIN_VAULT_ROOT").ok().map(PathBuf::from);
    let raw = std::env::var("SECOND_BRAIN_RAW_DIR").ok().map(PathBuf::from);
    let wiki = std::env::var("SECOND_BRAIN_WIKI_DIR").ok().map(PathBuf::from);
    let schema = std::env::var("SECOND_BRAIN_SCHEMA_DIR").ok().map(PathBuf::from);

    if raw.is_some() && wiki.is_some() && schema.is_some() {
        return Some((raw.unwrap(), wiki.unwrap(), schema.unwrap()));
    }

    if let Some(root) = vault {
        let r = root.join("raw");
        let w = root.join("wiki");
        let s = root.clone();
        return Some((r, w, s));
    }

    None
}

pub fn user_data_dir() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|p| p.join("SecondBrainLite"))
        .context("no local data dir")
}
