mod atomic;
mod chat;
mod config;
mod extract;
mod ingest;
mod llm;
mod manifest;
mod paths;
mod retrieval;
mod secrets;
mod sessions;
mod wiki;

use config::{load_config, normalize_llm_provider, save_config, resolved_triple, AppConfig};
use ingest::{run_ingest, FileIngestResult, IngestProgressPayload};
use serde::Deserialize;
use sessions::{delete_session, list_sessions, load_session, save_session, SessionFile};
use std::path::PathBuf;
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamPayload {
    pub session_id: String,
    pub user_message: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaStatus {
    pub claude_md: bool,
    pub llm_wiki_md: bool,
}

#[tauri::command]
fn get_platform_os() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn get_app_data_dir() -> Result<String, String> {
    paths::user_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
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

#[tauri::command]
async fn run_ingest_cmd(
    app: tauri::AppHandle,
    cfg: AppConfig,
    full_tier: bool,
) -> Result<Vec<FileIngestResult>, String> {
    run_ingest(&cfg, full_tier, |progress| {
        let _ = app.emit("ingest-progress", progress);
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestPastePayload {
    pub content: String,
    #[serde(default)]
    pub file_stem: Option<String>,
}

/// Saves pasted text as `raw/pastes/<name>.md` then runs the normal ingest pass.
#[tauri::command]
async fn ingest_pasted_text_cmd(
    app: tauri::AppHandle,
    cfg: AppConfig,
    full_tier: bool,
    payload: IngestPastePayload,
) -> Result<Vec<FileIngestResult>, String> {
    let rel = ingest::save_paste_to_raw(&cfg, &payload.content, payload.file_stem.as_deref())
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
    run_ingest(&cfg, full_tier, |progress| {
        let _ = app.emit("ingest-progress", progress);
    })
    .await
    .map_err(|e| e.to_string())
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

    let mut assistant_buf = String::new();
    let stream_result = chat::stream_session_chat(&cfg, &sess, payload.user_message.clone(), |delta| {
        assistant_buf.push_str(&delta);
        let _ = app.emit("chat-token", delta);
    })
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
        llm::LlmMessage {
            role: "system".into(),
            content: "You maintain a short personal memory file for the user's wiki assistant.".into(),
        },
        llm::LlmMessage {
            role: "user".into(),
            content: prompt_user,
        },
    ];

    let provider = normalize_llm_provider(&cfg.default_provider);
    let out = llm::complete_chat(&provider, &cfg, &messages)
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
        llm::LlmMessage {
            role: "system".into(),
            content: "You maintain a short personal memory file for the user's wiki assistant.".into(),
        },
        llm::LlmMessage {
            role: "user".into(),
            content: prompt_user,
        },
    ];

    let provider = normalize_llm_provider(&cfg.default_provider);
    let out = llm::complete_chat(&provider, &cfg, &messages)
        .await
        .map_err(|e| e.to_string())?;

    let mp = chat::memory_path().map_err(|e| e.to_string())?;
    if let Some(parent) = mp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic::atomic_write(&mp, out.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            get_app_data_dir,
            load_app_config,
            save_app_config,
            pick_vault_folder,
            setup_vault_paths,
            read_schema_status,
            copy_schema_templates,
            save_api_secret,
            api_secret_hint,
            fetch_ollama_models,
            run_ingest_cmd,
            ingest_pasted_text_cmd,
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
