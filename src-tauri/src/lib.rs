mod atomic;
mod chat;
mod config;
mod cursor_archive;
mod extract;
mod extract_diagrams;
mod extract_image;
mod extract_notebook;
mod extract_pptx;
mod extract_tabular;
mod ingest;
mod llm;
mod manifest;
mod paths;
mod personas;
mod retrieval;
mod secrets;
mod sessions;
mod web_compile;
mod wiki;

use config::{load_config, normalize_llm_provider, save_config, resolved_triple, AppConfig};
use cursor_archive::{
    filter_workspaces, parse_excerpt, reveal_path_in_os, ChatExcerpt, CursorTranscriptFile,
    CursorWorkspaceEntry, WorkspaceFilter,
};
use ingest::{
    commit_parsed_ingest_to_wiki, preview_ingest_commit, run_ingest, build_cursor_assist_prompt_pack,
    FileIngestResult, IngestCommitPreview, IngestProgressPayload, TrackInference,
};
use serde::{Deserialize, Serialize};
use sessions::{delete_session, list_sessions, load_session, save_session, SessionFile};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[derive(Default)]
struct IngestRunState {
    cancel_flag: Mutex<Option<Arc<AtomicBool>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamPayload {
    pub session_id: String,
    pub user_message: String,
    #[serde(default = "default_true")]
    pub wiki_sources_only: bool,
    #[serde(default)]
    pub include_web_search: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaStatus {
    pub claude_md: bool,
    pub llm_wiki_md: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestUiHints {
    pub supported_extensions: Vec<String>,
    pub vision_capable: bool,
    pub active_provider: String,
    pub active_model_id: String,
}

#[tauri::command]
fn get_ingest_ui_hints(cfg: AppConfig) -> Result<IngestUiHints, String> {
    let provider = normalize_llm_provider(&cfg.default_provider);
    let mid = llm::model_id_for_provider(&cfg, &provider);
    Ok(IngestUiHints {
        supported_extensions: crate::extract::SUPPORTED_EXTENSIONS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        vision_capable: llm::provider_supports_vision(&provider, &mid),
        active_provider: provider,
        active_model_id: mid,
    })
}

#[tauri::command]
fn get_platform_os() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn load_app_config() -> Result<AppConfig, String> {
    load_config().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_app_config(cfg: AppConfig) -> Result<(), String> {
    save_config(&cfg).map_err(|e| e.to_string())
}

/// Native folder picker on the Rust side (recommended on macOS vs `plugin:dialog|open` from JS).
#[tauri::command]
async fn pick_vault_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let mut d = app.dialog().file().set_title("Choose vault root folder");
    if let Some(win) = app.get_webview_window("main") {
        d = d.set_parent(&win);
    }
    Ok(d.blocking_pick_folder().map(|p| p.to_string()))
}

/// Creates `raw/` and `wiki/` under `vault_root`, sets paths on config, bootstraps wiki layout.
#[tauri::command]
fn setup_vault_paths(mut cfg: AppConfig, vault_root: String) -> Result<AppConfig, String> {
    let root = PathBuf::from(vault_root.trim());
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let raw = root.join("raw");
    let wiki = root.join("wiki");
    std::fs::create_dir_all(&raw).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&wiki).map_err(|e| e.to_string())?;
    cfg.vault_root = Some(root.to_string_lossy().into_owned());
    cfg.raw_dir = Some(raw.to_string_lossy().into_owned());
    cfg.wiki_dir = Some(wiki.to_string_lossy().into_owned());
    cfg.schema_dir = Some(root.to_string_lossy().into_owned());
    save_config(&cfg).map_err(|e| e.to_string())?;
    wiki::ensure_wiki_layout(&wiki).map_err(|e| e.to_string())?;
    Ok(cfg)
}

#[tauri::command]
fn read_schema_status(schema_dir: String) -> Result<SchemaStatus, String> {
    let root = PathBuf::from(schema_dir);
    let (c, l) = wiki::schema_paths(&root);
    Ok(SchemaStatus {
        claude_md: c.exists(),
        llm_wiki_md: l.exists(),
    })
}

#[tauri::command]
fn copy_schema_templates(schema_dir: String) -> Result<(), String> {
    let root = PathBuf::from(schema_dir);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let bundled = wiki::bundled_resources_dir();
    let src_claude = bundled.join("CLAUDE.md");
    let src_llm = bundled.join("llm-wiki.md");
    let dst_claude = root.join("CLAUDE.md");
    let dst_llm = root.join("llm-wiki.md");
    if !dst_claude.exists() && src_claude.exists() {
        std::fs::copy(&src_claude, &dst_claude).map_err(|e| e.to_string())?;
    }
    if !dst_llm.exists() && src_llm.exists() {
        std::fs::copy(&src_llm, &dst_llm).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn save_api_secret(provider: String, secret: String) -> Result<(), String> {
    let account = match provider.as_str() {
        "openai" => "openai_api_key",
        "anthropic" => "anthropic_api_key",
        "compatible" => "compatible_api_key",
        "gemini" => "gemini_api_key",
        "brave" => "brave_search_api_key",
        _ => return Err("unknown provider".into()),
    };
    secrets::set_secret(account, &secret).map_err(|e| e.to_string())
}

#[tauri::command]
fn api_secret_hint(provider: String) -> Result<Option<String>, String> {
    let account = match provider.as_str() {
        "openai" => "openai_api_key",
        "anthropic" => "anthropic_api_key",
        "compatible" => "compatible_api_key",
        "gemini" => "gemini_api_key",
        "brave" => "brave_search_api_key",
        _ => return Err("unknown provider".into()),
    };
    secrets::masked_hint(account).map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_ollama_models(base_url: String) -> Result<Vec<String>, String> {
    let url = format!(
        "{}/api/tags",
        base_url.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Ok(vec![]);
    }
    #[derive(Deserialize)]
    struct Tag {
        name: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        models: Vec<Tag>,
    }
    let j: Resp = res.json().await.map_err(|e| e.to_string())?;
    Ok(j.models.into_iter().map(|m| m.name).collect())
}

fn openai_model_list_filter(id: &str) -> bool {
    let l = id.to_lowercase();
    if l.contains("embedding")
        || l.contains("moderation")
        || l.contains("tts")
        || l.contains("whisper")
        || l.contains("dall-e")
        || l.contains("davinci")
        || l.contains("realtime")
        || l.contains("transcribe")
        || l.contains("omni-moderation")
    {
        return false;
    }
    true
}

#[tauri::command]
async fn fetch_openai_models() -> Result<Vec<String>, String> {
    let key = secrets::resolve_openai_key().map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", key.trim()))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "OpenAI /v1/models failed ({}): {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }
    #[derive(Deserialize)]
    struct OpenAiModel {
        id: String,
    }
    #[derive(Deserialize)]
    struct OpenAiModelsResp {
        data: Vec<OpenAiModel>,
    }
    let j: OpenAiModelsResp = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = j
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| openai_model_list_filter(id))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[tauri::command]
async fn fetch_anthropic_models() -> Result<Vec<String>, String> {
    let key = secrets::resolve_anthropic_key().map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", key.trim())
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Anthropic /v1/models failed ({}): {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }
    #[derive(Deserialize)]
    struct AnthropicModel {
        id: String,
    }
    #[derive(Deserialize)]
    struct AnthropicModelsResp {
        data: Vec<AnthropicModel>,
    }
    let j: AnthropicModelsResp = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = j.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[tauri::command]
async fn fetch_gemini_models(gemini_base_url: String) -> Result<Vec<String>, String> {
    let key = secrets::resolve_gemini_key().map_err(|e| e.to_string())?;
    let base = gemini_base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("Gemini API base URL is empty — set “Gemini API base URL” first.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(format!("{}/models", base))
        .query(&[("key", key.trim())])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Gemini list models failed ({}): {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GeminiModelEntry {
        name: String,
        supported_generation_methods: Option<Vec<String>>,
    }
    #[derive(Deserialize)]
    struct GeminiModelsResp {
        models: Option<Vec<GeminiModelEntry>>,
    }
    let j: GeminiModelsResp = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = Vec::new();
    for m in j.models.unwrap_or_default() {
        let supports = m
            .supported_generation_methods
            .as_ref()
            .map(|v| v.iter().any(|x| x == "generateContent"))
            .unwrap_or(true);
        if !supports {
            continue;
        }
        let tail = m.name.strip_prefix("models/").unwrap_or(&m.name);
        if !tail.is_empty() {
            ids.push(tail.to_string());
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[tauri::command]
async fn run_ingest_cmd(
    app: tauri::AppHandle,
    ingest_state: tauri::State<'_, IngestRunState>,
    cfg: AppConfig,
    full_tier: bool,
    track_id: Option<String>,
) -> Result<Vec<FileIngestResult>, String> {
    let track = track_id
        .as_deref()
        .map(ingest::sanitize_track_id)
        .filter(|s| !s.is_empty());
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut slot = ingest_state
            .cancel_flag
            .lock()
            .map_err(|_| "failed to lock ingest state".to_string())?;
        *slot = Some(cancel_flag.clone());
    }
    let out = run_ingest(&cfg, full_tier, track.as_deref(), |progress| {
        let _ = app.emit("ingest-progress", progress);
    }, || cancel_flag.load(Ordering::Relaxed))
    .await
    .map_err(|e| e.to_string());
    if let Ok(mut slot) = ingest_state.cancel_flag.lock() {
        *slot = None;
    }
    out
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestPastePayload {
    pub content: String,
    #[serde(default)]
    pub file_stem: Option<String>,
    #[serde(default)]
    pub track_id: Option<String>,
    #[serde(default)]
    pub auto_detect_track: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestUrlPayload {
    pub url: String,
    #[serde(default)]
    pub file_stem: Option<String>,
    #[serde(default)]
    pub track_id: Option<String>,
    #[serde(default)]
    pub auto_detect_track: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferTrackPayload {
    pub content: String,
    #[serde(default)]
    pub hint: Option<String>,
}

/// Saves pasted text as `raw/pastes/<name>.md` then runs the normal ingest pass.
#[tauri::command]
async fn ingest_pasted_text_cmd(
    app: tauri::AppHandle,
    ingest_state: tauri::State<'_, IngestRunState>,
    cfg: AppConfig,
    full_tier: bool,
    payload: IngestPastePayload,
) -> Result<Vec<FileIngestResult>, String> {
    let inferred = if let Some(t) = payload.track_id.as_deref() {
        TrackInference {
            track_id: Some(ingest::sanitize_track_id(t)),
            confidence: 1.0,
            reason: "User selected track".into(),
        }
    } else if payload.auto_detect_track {
        ingest::infer_track_for_content_detailed(&cfg, &payload.content, None).map_err(|e| e.to_string())?
    } else {
        TrackInference {
            track_id: None,
            confidence: 0.0,
            reason: "No track selected".into(),
        }
    };
    let chosen_track = inferred.track_id.clone();
    let route_msg = if let Some(t) = chosen_track.as_deref() {
        format!(
            "Routing content to track '{}' (confidence {:.0}%, {}).",
            t,
            inferred.confidence * 100.0,
            inferred.reason
        )
    } else {
        "No track resolved; using inbox fallback.".to_string()
    };
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "route".into(),
            message: route_msg,
            current: None,
            total: None,
            relative_path: None,
        },
    );
    let rel = ingest::save_paste_to_raw(
        &cfg,
        &payload.content,
        payload.file_stem.as_deref(),
        chosen_track.as_deref(),
    )
        .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "paste".into(),
            message: format!("Saved raw/{rel}; running ingest…"),
            current: None,
            total: None,
            relative_path: Some(rel),
        },
    );
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut slot = ingest_state
            .cancel_flag
            .lock()
            .map_err(|_| "failed to lock ingest state".to_string())?;
        *slot = Some(cancel_flag.clone());
    }
    let out = run_ingest(&cfg, full_tier, None, |progress| {
        let _ = app.emit("ingest-progress", progress);
    }, || cancel_flag.load(Ordering::Relaxed))
    .await
    .map_err(|e| e.to_string());
    if let Ok(mut slot) = ingest_state.cancel_flag.lock() {
        *slot = None;
    }
    out
}

#[tauri::command]
async fn ingest_url_cmd(
    app: tauri::AppHandle,
    ingest_state: tauri::State<'_, IngestRunState>,
    cfg: AppConfig,
    full_tier: bool,
    payload: IngestUrlPayload,
) -> Result<Vec<FileIngestResult>, String> {
    if payload.url.trim().is_empty() {
        return Err("url is required".into());
    }
    let inferred = if let Some(t) = payload.track_id.as_deref() {
        TrackInference {
            track_id: Some(ingest::sanitize_track_id(t)),
            confidence: 1.0,
            reason: "User selected track".into(),
        }
    } else if payload.auto_detect_track {
        ingest::infer_track_for_content_detailed(&cfg, &payload.url, Some(&payload.url)).map_err(|e| e.to_string())?
    } else {
        TrackInference {
            track_id: None,
            confidence: 0.0,
            reason: "No track selected".into(),
        }
    };
    let chosen_track = inferred.track_id.clone();
    let route_msg = if let Some(t) = chosen_track.as_deref() {
        format!(
            "Routing URL to track '{}' (confidence {:.0}%, {}).",
            t,
            inferred.confidence * 100.0,
            inferred.reason
        )
    } else {
        "No track resolved; using inbox fallback.".to_string()
    };
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "route".into(),
            message: route_msg,
            current: None,
            total: None,
            relative_path: None,
        },
    );
    let rel = ingest::save_url_to_raw(
        &cfg,
        payload.url.trim(),
        payload.file_stem.as_deref(),
        chosen_track.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "url".into(),
            message: format!("Saved raw/{rel}; running ingest…"),
            current: None,
            total: None,
            relative_path: Some(rel),
        },
    );
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut slot = ingest_state
            .cancel_flag
            .lock()
            .map_err(|_| "failed to lock ingest state".to_string())?;
        *slot = Some(cancel_flag.clone());
    }
    let out = run_ingest(&cfg, full_tier, None, |progress| {
        let _ = app.emit("ingest-progress", progress);
    }, || cancel_flag.load(Ordering::Relaxed))
    .await
    .map_err(|e| e.to_string());
    if let Ok(mut slot) = ingest_state.cancel_flag.lock() {
        *slot = None;
    }
    out
}

#[tauri::command]
fn cancel_ingest_cmd(ingest_state: tauri::State<'_, IngestRunState>) -> Result<(), String> {
    let slot = ingest_state
        .cancel_flag
        .lock()
        .map_err(|_| "failed to lock ingest state".to_string())?;
    if let Some(flag) = slot.as_ref() {
        flag.store(true, Ordering::Relaxed);
        Ok(())
    } else {
        Err("No ingest is currently running.".into())
    }
}

#[tauri::command]
fn list_tracks_cmd(cfg: AppConfig) -> Result<Vec<String>, String> {
    ingest::list_tracks(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
fn infer_track_cmd(cfg: AppConfig, payload: InferTrackPayload) -> Result<TrackInference, String> {
    ingest::infer_track_for_content_detailed(&cfg, &payload.content, payload.hint.as_deref())
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCursorAssistResponse {
    pub raw_rel: String,
    pub prompt_pack: String,
}

/// Save pasted content to `raw/`, then return the prompt pack for Cursor Chat (no LLM in Lite).
#[tauri::command]
fn prepare_cursor_assisted_ingest(
    cfg: AppConfig,
    full_tier: bool,
    payload: IngestPastePayload,
) -> Result<PrepareCursorAssistResponse, String> {
    let inferred = if let Some(t) = payload.track_id.as_deref() {
        TrackInference {
            track_id: Some(ingest::sanitize_track_id(t)),
            confidence: 1.0,
            reason: "User selected track".into(),
        }
    } else if payload.auto_detect_track {
        ingest::infer_track_for_content_detailed(&cfg, &payload.content, None)
            .map_err(|e| e.to_string())?
    } else {
        TrackInference {
            track_id: None,
            confidence: 0.0,
            reason: "No track selected".into(),
        }
    };
    let chosen_track = inferred.track_id.clone();
    let rel = ingest::save_paste_to_raw(
        &cfg,
        &payload.content,
        payload.file_stem.as_deref(),
        chosen_track.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    let pack = build_cursor_assist_prompt_pack(&cfg, &rel, full_tier).map_err(|e| e.to_string())?;
    Ok(PrepareCursorAssistResponse {
        raw_rel: rel,
        prompt_pack: pack,
    })
}

#[tauri::command]
fn preview_cursor_assisted_commit_cmd(
    cfg: AppConfig,
    raw_rel: String,
    pasted_model_json: String,
) -> Result<IngestCommitPreview, String> {
    preview_ingest_commit(&cfg, &raw_rel, &pasted_model_json).map_err(|e| e.to_string())
}

#[tauri::command]
fn commit_cursor_assisted_ingest_cmd(
    app: tauri::AppHandle,
    cfg: AppConfig,
    raw_rel: String,
    pasted_model_json: String,
) -> Result<FileIngestResult, String> {
    let preview = preview_ingest_commit(&cfg, &raw_rel, &pasted_model_json).map_err(|e| e.to_string())?;
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "commit".into(),
            message: format!(
                "Writing wiki/{} from Cursor-assisted JSON",
                preview.wiki_source_rel
            ),
            current: None,
            total: None,
            relative_path: Some(raw_rel.clone()),
        },
    );
    commit_parsed_ingest_to_wiki(&cfg, &raw_rel, &pasted_model_json, "cursor-assisted")
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorArchiveDiscoverQuery {
    #[serde(default)]
    pub hash_contains: Option<String>,
    #[serde(default)]
    pub max_age_days: Option<u32>,
    #[serde(default)]
    pub vault_path_contains: Option<String>,
}

#[tauri::command]
fn cursor_archive_discover_cmd(
    query: Option<CursorArchiveDiscoverQuery>,
) -> Result<Vec<CursorWorkspaceEntry>, String> {
    let entries = cursor_archive::discover_workspaces().map_err(|e| e.to_string())?;
    let filtered = if let Some(q) = query {
        filter_workspaces(
            entries,
            &WorkspaceFilter {
                hash_contains: q.hash_contains,
                max_age_days: q.max_age_days,
                vault_path_contains: q.vault_path_contains,
            },
        )
    } else {
        entries
    };
    Ok(filtered)
}

#[tauri::command]
fn cursor_archive_list_cmd(workspace_abs: String) -> Result<Vec<CursorTranscriptFile>, String> {
    cursor_archive::list_transcripts(&workspace_abs).map_err(|e| e.to_string())
}

#[tauri::command]
fn cursor_archive_preview_cmd(path: String, max_chars: usize) -> Result<ChatExcerpt, String> {
    parse_excerpt(&path, max_chars).map_err(|e| e.to_string())
}

#[tauri::command]
fn cursor_archive_reveal_cmd(path: String) -> Result<(), String> {
    reveal_path_in_os(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn cursor_archive_commit_excerpt_wiki_cmd(
    app: tauri::AppHandle,
    cfg: AppConfig,
    transcript_path: String,
    max_chars: usize,
    track_id: Option<String>,
    title_stem: Option<String>,
) -> Result<FileIngestResult, String> {
    let ex = parse_excerpt(&transcript_path, max_chars).map_err(|e| e.to_string())?;
    let md = cursor_archive::render_excerpt_markdown(&ex);
    let stem = title_stem
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("cursor-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    let track = track_id
        .as_deref()
        .map(ingest::sanitize_track_id)
        .filter(|s| !s.is_empty());
    let raw_rel =
        ingest::save_paste_to_raw(&cfg, &md, Some(&stem), track.as_deref()).map_err(|e| e.to_string())?;
    let slug = std::path::Path::new(&raw_rel)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "cursor-import".into());
    let title = ex.title.clone();
    let tags = vec!["cursor-archive".to_string(), "source".to_string()];
    let json = ingest::stub_ingest_json_for_markdown_body(
        &slug,
        &title,
        "Imported Cursor transcript excerpt (local archive)",
        &md,
        &tags,
    );
    let _ = app.emit(
        "ingest-progress",
        IngestProgressPayload {
            phase: "cursor-archive".into(),
            message: format!("Committing wiki from excerpt → raw/{raw_rel}"),
            current: None,
            total: None,
            relative_path: Some(raw_rel.clone()),
        },
    );
    commit_parsed_ingest_to_wiki(&cfg, &raw_rel, &json, "cursor-archive").map_err(|e| e.to_string())
}

#[tauri::command]
fn cursor_archive_import_excerpt_session_cmd(
    transcript_path: String,
    max_chars: usize,
) -> Result<SessionFile, String> {
    let ex = parse_excerpt(&transcript_path, max_chars).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    let messages: Vec<sessions::ChatMessage> = ex
        .messages
        .into_iter()
        .map(|m| sessions::ChatMessage {
            role: m.role,
            content: m.text,
            ts: now.clone(),
        })
        .collect();
    let sess = SessionFile {
        id: id.clone(),
        title: ex.title,
        created: now.clone(),
        updated: now,
        messages,
    };
    save_session(&sess).map_err(|e| e.to_string())?;
    Ok(sess)
}

#[tauri::command]
fn list_chat_sessions() -> Result<Vec<SessionFile>, String> {
    list_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
fn load_chat_session(id: String) -> Result<SessionFile, String> {
    load_session(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_chat_session(sess: SessionFile) -> Result<(), String> {
    save_session(&sess).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_chat_session(id: String) -> Result<(), String> {
    delete_session(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn new_chat_session() -> Result<SessionFile, String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let sess = SessionFile {
        id: id.clone(),
        title: format!("Chat {}", &id[..8]),
        created: now.clone(),
        updated: now,
        messages: vec![],
    };
    save_session(&sess).map_err(|e| e.to_string())?;
    Ok(sess)
}

#[tauri::command]
async fn chat_stream_cmd(
    app: tauri::AppHandle,
    cfg: AppConfig,
    payload: ChatStreamPayload,
) -> Result<(), String> {
    let mut sess = load_session(&payload.session_id).map_err(|e| e.to_string())?;
    chat::push_message(&mut sess, "user", payload.user_message.clone());
    save_session(&sess).map_err(|e| e.to_string())?;

    let wiki_sources_only = payload.wiki_sources_only;
    let include_web_search = payload.include_web_search;

    if include_web_search && !secrets::brave_search_key_configured() {
        let _ = app.emit(
            "chat-token",
            "\n\n__ERROR__Web search is on but no Brave Search API key is set — add it in Settings → API Keys."
                .to_string(),
        );
        return Err("Brave Search API key not set".into());
    }

    let mut assistant_buf = String::new();
    let stream_result = chat::stream_session_chat(
        &cfg,
        &sess,
        payload.user_message.clone(),
        wiki_sources_only,
        include_web_search,
        |meta| {
            let _ = app.emit("chat-retrieval-meta", meta);
        },
        |delta| {
            assistant_buf.push_str(&delta);
            let _ = app.emit("chat-token", delta);
        },
    )
    .await;

    if let Err(e) = stream_result {
        let _ = app.emit(
            "chat-token",
            format!("\n\n__ERROR__{}", e),
        );
        return Err(e.to_string());
    }

    chat::push_message(&mut sess, "assistant", assistant_buf);
    save_session(&sess).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWikiArgs {
    pub title: String,
    pub body_markdown: String,
}

#[tauri::command]
fn save_answer_to_wiki(cfg: AppConfig, args: SaveWikiArgs) -> Result<String, String> {
    let (_, wiki_dir, _) = resolved_triple(&cfg).map_err(|e| e.to_string())?;
    let wiki_dir = wiki_dir.canonicalize().unwrap_or(wiki_dir);
    wiki::ensure_wiki_layout(&wiki_dir).map_err(|e| e.to_string())?;

    let slug = ingest_slug_from_title(&args.title);
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("title".into()),
        serde_yaml::Value::String(args.title.clone()),
    );
    root.insert(
        serde_yaml::Value::String("type".into()),
        serde_yaml::Value::String("analysis".into()),
    );
    root.insert(
        serde_yaml::Value::String("created".into()),
        serde_yaml::Value::String(today.clone()),
    );
    root.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(today.clone()),
    );
    root.insert(
        serde_yaml::Value::String("sources".into()),
        serde_yaml::Value::Sequence(serde_yaml::Sequence::new()),
    );
    let mut tags = serde_yaml::Sequence::new();
    tags.push(serde_yaml::Value::String("chat-export".into()));
    root.insert(
        serde_yaml::Value::String("tags".into()),
        serde_yaml::Value::Sequence(tags),
    );
    let fm = serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).map_err(|e| e.to_string())?;
    let page = format!("---\n{}---\n\n{}", fm, args.body_markdown);

    let analyses = wiki_dir.join("analyses");
    std::fs::create_dir_all(&analyses).map_err(|e| e.to_string())?;
    let path = analyses.join(format!("{}.md", slug));
    atomic::atomic_write(&path, page.as_bytes()).map_err(|e| e.to_string())?;

    let index_path = wiki_dir.join("index.md");
    let mut idx = std::fs::read_to_string(&index_path).unwrap_or_default();
    let line = format!(
        "- [[analyses/{}]] — {} | analysis | {}",
        slug,
        args.title,
        today
    );
    let anchor = format!("[[analyses/{}]]", slug);
    if !idx.contains(&anchor) {
        let needle = "## Analyses";
        if let Some(pos) = idx.find(needle) {
            let after = pos + needle.len();
            let head = &idx[..after];
            let tail = &idx[after..];
            let stripped = tail
                .trim_start_matches('\n')
                .trim_start_matches("*(Empty)*")
                .trim_start_matches('\n');
            idx = format!("{}\n\n{}\n\n{}", head, line, stripped);
        } else {
            idx.push_str("\n\n## Analyses\n\n");
            idx.push_str(&line);
            idx.push('\n');
        }
        atomic::atomic_write(&index_path, idx.as_bytes()).map_err(|e| e.to_string())?;
    }

    let log_path = wiki_dir.join("log.md");
    let mut log = std::fs::read_to_string(&log_path).unwrap_or_default();
    log.push_str(&format!(
        "\n## [{}] query | filed chat answer\nOutput filed: yes — analyses/{}.md\n\n",
        today, slug
    ));
    atomic::atomic_write(&log_path, log.as_bytes()).map_err(|e| e.to_string())?;

    Ok(format!("analyses/{}.md", slug))
}

fn ingest_slug_from_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .fold(String::new(), |mut acc, c| {
            if acc.ends_with('-') && c == '-' {
                acc
            } else {
                acc.push(c);
                acc
            }
        })
}

#[tauri::command]
async fn update_memory_roll_up(cfg: AppConfig, session_id: String) -> Result<(), String> {
    let sess = load_session(&session_id).map_err(|e| e.to_string())?;
    let tail: String = sess
        .messages
        .iter()
        .rev()
        .take(24)
        .rev()
        .map(|m| format!("{}: {}\n", m.role, m.content))
        .collect();

    let prev = chat::load_memory_excerpt(8000);

    let prompt_user = format!(
        "Previous rolling memory:\n{}\n\nRecent conversation:\n{}\n\nWrite an updated concise rolling memory (markdown, <= 120 lines) capturing stable facts, goals, and entities.",
        prev, tail
    );

    let messages = vec![
        llm::LlmMessage::text(
            "system",
            "You maintain a short personal memory file for the user's wiki assistant.",
        ),
        llm::LlmMessage::text("user", prompt_user),
    ];

    let provider = normalize_llm_provider(&cfg.default_provider);
    let out = llm::complete_chat(&provider, &cfg, &messages, Default::default())
        .await
        .map_err(|e| e.to_string())?;

    let mp = chat::memory_path().map_err(|e| e.to_string())?;
    if let Some(parent) = mp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic::atomic_write(&mp, out.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Roll arbitrary text (e.g. pasted content from Ingest) into the rolling memory file.
/// Unlike `update_memory_roll_up` this does not require an active chat session.
#[tauri::command]
async fn rollup_content_to_memory(cfg: AppConfig, content: String) -> Result<(), String> {
    let prev = chat::load_memory_excerpt(8000);

    let prompt_user = format!(
        "Previous rolling memory:\n{}\n\nNew content to integrate:\n{}\n\nWrite an updated concise rolling memory (markdown, <= 120 lines) capturing stable facts, goals, and entities.",
        prev, content
    );

    let messages = vec![
        llm::LlmMessage::text(
            "system",
            "You maintain a short personal memory file for the user's wiki assistant.",
        ),
        llm::LlmMessage::text("user", prompt_user),
    ];

    let provider = normalize_llm_provider(&cfg.default_provider);
    let out = llm::complete_chat(&provider, &cfg, &messages, Default::default())
        .await
        .map_err(|e| e.to_string())?;

    let mp = chat::memory_path().map_err(|e| e.to_string())?;
    if let Some(parent) = mp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic::atomic_write(&mp, out.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_chat_personas() -> Vec<personas::PersonaMeta> {
    personas::all_personas()
}

#[tauri::command]
fn list_student_grade_options() -> Vec<personas::GradeOption> {
    personas::student_grade_options()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(IngestRunState::default())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_platform_os,
            get_ingest_ui_hints,
            load_app_config,
            save_app_config,
            list_chat_personas,
            list_student_grade_options,
            pick_vault_folder,
            setup_vault_paths,
            read_schema_status,
            copy_schema_templates,
            save_api_secret,
            api_secret_hint,
            fetch_ollama_models,
            fetch_openai_models,
            fetch_anthropic_models,
            fetch_gemini_models,
            run_ingest_cmd,
            ingest_pasted_text_cmd,
            ingest_url_cmd,
            cancel_ingest_cmd,
            list_tracks_cmd,
            infer_track_cmd,
            prepare_cursor_assisted_ingest,
            preview_cursor_assisted_commit_cmd,
            commit_cursor_assisted_ingest_cmd,
            cursor_archive_discover_cmd,
            cursor_archive_list_cmd,
            cursor_archive_preview_cmd,
            cursor_archive_reveal_cmd,
            cursor_archive_commit_excerpt_wiki_cmd,
            cursor_archive_import_excerpt_session_cmd,
            list_chat_sessions,
            load_chat_session,
            save_chat_session,
            delete_chat_session,
            new_chat_session,
            chat_stream_cmd,
            save_answer_to_wiki,
            update_memory_roll_up,
            rollup_content_to_memory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
