//! Non-secret app configuration persisted as JSON.

use crate::paths::{resolve_dirs_from_env, user_data_dir};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub os_hint: String,
    #[serde(default)]
    pub raw_dir: Option<String>,
    #[serde(default)]
    pub wiki_dir: Option<String>,
    #[serde(default)]
    pub schema_dir: Option<String>,
    #[serde(default)]
    pub vault_root: Option<String>,
    /// openai | anthropic | ollama | compatible | gemini
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default)]
    pub ollama_enabled: bool,
    #[serde(default = "default_ollama_url")]
    pub ollama_base_url: String,
    #[serde(default)]
    pub ollama_model: String,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    #[serde(default = "default_anthropic_model")]
    pub anthropic_model: String,
    #[serde(default)]
    pub compatible_base_url: String,
    #[serde(default)]
    pub compatible_model: String,
    #[serde(default = "default_gemini_base_url")]
    pub gemini_base_url: String,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub retrieval_track_filter: Option<String>,
}

fn default_provider() -> String {
    "ollama".into()
}

/// Trim, lowercase, and map common aliases so chat/ingest match LLM routing (`llm.rs`).
pub fn normalize_llm_provider(raw: &str) -> String {
    let p = raw.trim().to_lowercase();
    match p.as_str() {
        "" => default_provider(),
        "claude" => "anthropic".into(),
        "google" => "gemini".into(),
        other => other.into(),
    }
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".into()
}

fn default_openai_model() -> String {
    // GPT-5.4 mini: Chat Completions–compatible; strong price/performance (OpenAI docs, 2026).
    "gpt-5.4-mini".into()
}

fn default_anthropic_model() -> String {
    // Claude Sonnet 4.6: current balanced API model (Anthropic docs; 3.x aliases retired).
    "claude-sonnet-4-6".into()
}

fn default_gemini_base_url() -> String {
    "https://generativelanguage.googleapis.com/v1beta".into()
}

fn default_gemini_model() -> String {
    // Align with sibling app defaults (Gemini API generateContent model id).
    "gemini-3.1-flash-lite".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            os_hint: "auto".into(),
            raw_dir: None,
            wiki_dir: None,
            schema_dir: None,
            vault_root: None,
            default_provider: default_provider(),
            ollama_enabled: true,
            ollama_base_url: default_ollama_url(),
            ollama_model: String::new(),
            openai_model: default_openai_model(),
            anthropic_model: default_anthropic_model(),
            compatible_base_url: String::new(),
            compatible_model: String::new(),
            gemini_base_url: default_gemini_base_url(),
            gemini_model: default_gemini_model(),
            theme: "system".into(),
            retrieval_track_filter: None,
        }
    }
}

fn config_path() -> Result<PathBuf> {
    let d = user_data_dir()?;
    Ok(d.join("config.json"))
}

pub fn load_config() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let s = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut c: AppConfig = serde_json::from_str(&s).unwrap_or_default();

    // Env overrides paths when set
    if let Some((raw, wiki, schema)) = resolve_dirs_from_env() {
        c.raw_dir = Some(raw.to_string_lossy().into_owned());
        c.wiki_dir = Some(wiki.to_string_lossy().into_owned());
        c.schema_dir = Some(schema.to_string_lossy().into_owned());
    }

    Ok(c)
}

pub fn save_config(cfg: &AppConfig) -> Result<()> {
    let dir = user_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = config_path()?;
    let s = serde_json::to_string_pretty(cfg)?;
    crate::atomic::atomic_write(&path, s.as_bytes())?;
    Ok(())
}

/// Resolved wiki/raw/schema paths from config (must exist for ingest/chat).
pub fn resolved_triple(cfg: &AppConfig) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let raw = cfg
        .raw_dir
        .clone()
        .filter(|s| !s.is_empty())
        .context("raw_dir not configured")?;
    let wiki = cfg
        .wiki_dir
        .clone()
        .filter(|s| !s.is_empty())
        .context("wiki_dir not configured")?;
    let schema = cfg
        .schema_dir
        .clone()
        .filter(|s| !s.is_empty())
        .context("schema_dir not configured")?;
    Ok((PathBuf::from(raw), PathBuf::from(wiki), PathBuf::from(schema)))
}
