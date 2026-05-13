use crate::config::AppConfig;
use crate::secrets;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum LlmPart {
    Text(String),
    Image {
        mime: String,
        base64_data: String,
    },
}

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: String,
    pub parts: Vec<LlmPart>,
}

impl LlmMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            parts: vec![LlmPart::Text(content.into())],
        }
    }

    /// User message with optional base64-encoded images `(mime, base64)`.
    pub fn vision_user(user_text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut parts = vec![LlmPart::Text(user_text.into())];
        for (mime, base64_data) in images {
            parts.push(LlmPart::Image { mime, base64_data });
        }
        Self {
            role: "user".into(),
            parts,
        }
    }

    /// Text-only projection (drops images) for streaming chat paths.
    pub fn collapse_text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| {
                if let LlmPart::Text(t) = p {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn model_id_for_provider(cfg: &AppConfig, provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "openai" => cfg.openai_model.trim().to_string(),
        "anthropic" => cfg.anthropic_model.trim().to_string(),
        "gemini" => cfg.gemini_model.trim().to_string(),
        "compatible" => cfg.compatible_model.trim().to_string(),
        "ollama" => cfg.ollama_model.trim().to_string(),
        _ => String::new(),
    }
}

/// Conservative allow-list for multimodal ingest (diagram screenshots).
pub fn provider_supports_vision(provider: &str, model: &str) -> bool {
    let p = provider.trim().to_lowercase();
    let m = model.trim().to_lowercase();
    match p.as_str() {
        "openai" | "compatible" => {
            m.contains("gpt-4o")
                || m.contains("gpt-4.1")
                || m.contains("gpt-5")
                || m.starts_with("o3")
                || m.starts_with("o4")
                || m.contains("vision")
        }
        "anthropic" => {
            m.contains("claude-3-5-sonnet")
                || m.contains("claude-3-5-haiku")
                || m.contains("claude-3-7")
                || m.contains("claude-sonnet-4")
                || m.contains("claude-opus-4")
                || m.contains("claude-haiku-4")
        }
        "gemini" => {
            m.starts_with("gemini-1.5")
                || m.starts_with("gemini-2")
                || m.starts_with("gemini-3")
                || m.starts_with("gemini")
        }
        "ollama" => {
            m.contains("llava")
                || m.contains("bakllava")
                || m.contains("llama3.2-vision")
                || m.contains("vision")
        }
        _ => false,
    }
}

fn openai_chat_content(parts: &[LlmPart]) -> serde_json::Value {
    if parts.len() == 1 {
        if let LlmPart::Text(t) = &parts[0] {
            return json!(t);
        }
    }
    let blocks: Vec<serde_json::Value> = parts
        .iter()
        .filter_map(|p| match p {
            LlmPart::Text(t) => Some(json!({"type": "text", "text": t})),
            LlmPart::Image { mime, base64_data } => Some(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{};base64,{}", mime, base64_data)}
            })),
        })
        .collect();
    json!(blocks)
}

fn openai_messages_json(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            json!({
                "role": m.role,
                "content": openai_chat_content(&m.parts),
            })
        })
        .collect()
}

fn ollama_message_json(m: &LlmMessage) -> serde_json::Value {
    let text = m.collapse_text();
    let images: Vec<&str> = m
        .parts
        .iter()
        .filter_map(|p| {
            if let LlmPart::Image { base64_data, .. } = p {
                Some(base64_data.as_str())
            } else {
                None
            }
        })
        .collect();
    if images.is_empty() {
        json!({"role": m.role, "content": text})
    } else {
        json!({"role": m.role, "content": text, "images": images})
    }
}

fn anthropic_message_content(m: &LlmMessage) -> serde_json::Value {
    if m.parts.len() == 1 {
        if let LlmPart::Text(t) = &m.parts[0] {
            return json!(t);
        }
    }
    let blocks: Vec<serde_json::Value> = m
        .parts
        .iter()
        .filter_map(|p| match p {
            LlmPart::Text(t) => Some(json!({"type": "text", "text": t})),
            LlmPart::Image { mime, base64_data } => Some(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime,
                    "data": base64_data
                }
            })),
        })
        .collect();
    json!(blocks)
}

fn gemini_parts(m: &LlmMessage) -> Vec<serde_json::Value> {
    m.parts
        .iter()
        .map(|p| match p {
            LlmPart::Text(t) => json!({"text": t}),
            LlmPart::Image { mime, base64_data } => json!({
                "inline_data": {"mime_type": mime, "data": base64_data}
            }),
        })
        .collect()
}

/// Options for non-streaming chat completion.
#[derive(Debug, Clone, Default)]
pub struct ChatCompleteOptions {
    /// When true, providers that support it constrain the assistant message to JSON (used for ingest).
    pub json_response: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceMsg {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMsg,
}

#[derive(Debug, Deserialize)]
struct OpenAiResp {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicResp {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct OllamaMsg {
    message: OllamaInner,
}

#[derive(Debug, Deserialize)]
struct OllamaInner {
    content: String,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("http client")
}

/// Keychain access can fail or deadlock if invoked directly from some async contexts; keep it on the blocking pool.
async fn blocking_secret<R: Send + 'static>(
    fetch: impl FnOnce() -> Result<R> + Send + 'static,
) -> Result<R> {
    tokio::task::spawn_blocking(fetch)
        .await
        .map_err(|e| anyhow!("API key resolve task failed: {}", e))?
}

pub async fn complete_chat(
    provider: &str,
    cfg: &AppConfig,
    messages: &[LlmMessage],
    opts: ChatCompleteOptions,
) -> Result<String> {
    match provider {
        "ollama" => ollama_complete(cfg, messages, false, opts.json_response).await,
        "openai" => openai_complete(cfg, messages, false, opts.json_response).await,
        "anthropic" => anthropic_complete(cfg, messages, opts.json_response).await,
        "compatible" => openai_compatible_complete(cfg, messages, false, opts.json_response).await,
        "gemini" => gemini_complete(cfg, messages, opts.json_response).await,
        _ => Err(anyhow!("unknown provider {}", provider)),
    }
}

/// JSON Schema for Lite wiki ingest (shared by OpenAI structured outputs + Anthropic `output_config`).
fn lite_ingest_json_schema_core() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "slug": {"type": "string"},
            "title": {"type": "string"},
            "one_line_summary": {"type": "string"},
            "body_markdown_b64": {"type": "string"},
            "body_markdown": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "glossary_patch": {"type": "string"}
        },
        "required": [
            "slug",
            "title",
            "one_line_summary",
            "body_markdown_b64",
            "body_markdown",
            "tags",
            "glossary_patch"
        ],
        "additionalProperties": false
    })
}

fn openai_ingest_structured_response_format() -> serde_json::Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "lite_ingest_wiki",
            "strict": true,
            "schema": lite_ingest_json_schema_core()
        }
    })
}

fn anthropic_ingest_output_config() -> serde_json::Value {
    json!({
        "format": {
            "type": "json_schema",
            "schema": lite_ingest_json_schema_core()
        }
    })
}

async fn ollama_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    stream: bool,
    json_response: bool,
) -> Result<String> {
    let url = format!(
        "{}/api/chat",
        cfg.ollama_base_url.trim_end_matches('/')
    );
    let mut body = json!({
      "model": cfg.ollama_model,
      "messages": messages.iter().map(|m| ollama_message_json(m)).collect::<Vec<_>>(),
      "stream": stream,
    });
    if json_response {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("format".into(), json!("json"));
        }
    }
    let c = client()?;
    let res = c.post(&url).json(&body).send().await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("ollama error: {}", t));
    }
    let v: OllamaMsg = res.json().await?;
    Ok(v.message.content)
}

async fn openai_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    stream: bool,
    json_response: bool,
) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_openai_key()).await?;
    let url = "https://api.openai.com/v1/chat/completions";
    let model_id = openai_chat_model_for_api(&cfg.openai_model);
    let messages_json = openai_messages_json(messages);
    let mut body = json!({
      "model": model_id,
      "messages": messages_json,
      "stream": stream,
    });
    if !openai_chat_omits_temperature_parameter(&model_id) {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("temperature".into(), json!(0.2));
        }
    }
    if json_response {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "response_format".into(),
                openai_ingest_structured_response_format(),
            );
        }
    }

    let c = client()?;
    let mut rate_retries = 0u32;
    let mut body_fix_retries = 0u8;

    loop {
        let res = c
            .post(url)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await?;

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let t = res.text().await.unwrap_or_default();
            rate_retries += 1;
            if rate_retries > 16 {
                return Err(anyhow!("openai error (rate limited): {}", t));
            }
            let wait = openai_retry_after_ms_from_error_body(&t)
                .unwrap_or(1500u64.saturating_mul(rate_retries as u64))
                .saturating_add(150)
                .min(120_000);
            tokio::time::sleep(Duration::from_millis(wait)).await;
            continue;
        }

        if res.status().is_success() {
            let v: OpenAiResp = res.json().await?;
            return Ok(v.choices
                .get(0)
                .and_then(|c| c.message.content.clone())
                .unwrap_or_default());
        }

        let status = res.status();
        let t = res.text().await.unwrap_or_default();
        let t_lower = t.to_ascii_lowercase();

        if status == reqwest::StatusCode::BAD_REQUEST
            && body.get("temperature").is_some()
            && t_lower.contains("temperature")
            && (t_lower.contains("unsupported") || t_lower.contains("unsupported_value"))
            && body_fix_retries < 4
        {
            if let Some(obj) = body.as_object_mut() {
                obj.remove("temperature");
            }
            body_fix_retries += 1;
            continue;
        }

        if json_response
            && status == reqwest::StatusCode::BAD_REQUEST
            && !openai_response_format_is_plain_json_object(&body)
            && body_fix_retries < 8
        {
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "response_format".into(),
                    json!({ "type": "json_object" }),
                );
            }
            body_fix_retries += 1;
            continue;
        }

        return Err(anyhow!("openai error: {}", t));
    }
}

fn openai_response_format_is_plain_json_object(body: &serde_json::Value) -> bool {
    body.get("response_format")
        .and_then(|rf| rf.get("type"))
        .and_then(|t| t.as_str())
        == Some("json_object")
}

async fn openai_compatible_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    stream: bool,
    json_response: bool,
) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_compatible_key()).await?;
    let base = cfg.compatible_base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base);
    let model_id = openai_chat_model_for_api(&cfg.compatible_model);
    let messages_json = openai_messages_json(messages);
    let mut body = json!({
      "model": model_id,
      "messages": messages_json,
      "stream": stream,
    });
    if !openai_chat_omits_temperature_parameter(&model_id) {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("temperature".into(), json!(0.2));
        }
    }
    if json_response {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "response_format".into(),
                openai_ingest_structured_response_format(),
            );
        }
    }
    let c = client()?;
    let mut rate_retries = 0u32;
    let mut body_fix_retries = 0u8;

    loop {
        let res = c.post(&url).bearer_auth(&key).json(&body).send().await?;

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let t = res.text().await.unwrap_or_default();
            rate_retries += 1;
            if rate_retries > 16 {
                return Err(anyhow!("compatible openai error (rate limited): {}", t));
            }
            let wait = openai_retry_after_ms_from_error_body(&t)
                .unwrap_or(1500u64.saturating_mul(rate_retries as u64))
                .saturating_add(150)
                .min(120_000);
            tokio::time::sleep(Duration::from_millis(wait)).await;
            continue;
        }

        if res.status().is_success() {
            let v: OpenAiResp = res.json().await?;
            return Ok(v.choices
                .get(0)
                .and_then(|c| c.message.content.clone())
                .unwrap_or_default());
        }

        let status = res.status();
        let t = res.text().await.unwrap_or_default();
        let t_lower = t.to_ascii_lowercase();

        if status == reqwest::StatusCode::BAD_REQUEST
            && body.get("temperature").is_some()
            && t_lower.contains("temperature")
            && (t_lower.contains("unsupported") || t_lower.contains("unsupported_value"))
            && body_fix_retries < 4
        {
            if let Some(obj) = body.as_object_mut() {
                obj.remove("temperature");
            }
            body_fix_retries += 1;
            continue;
        }

        if json_response
            && status == reqwest::StatusCode::BAD_REQUEST
            && !openai_response_format_is_plain_json_object(&body)
            && body_fix_retries < 8
        {
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "response_format".into(),
                    json!({ "type": "json_object" }),
                );
            }
            body_fix_retries += 1;
            continue;
        }

        return Err(anyhow!("compatible openai error: {}", t));
    }
}

/// Maps retired Chat Completions `-latest` pointers and noisy aliases to stable OpenAI model ids.
fn openai_chat_model_for_api(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "gpt-4o-latest" | "chatgpt-4o-latest" => "gpt-4o".into(),
        "gpt-4o-mini-latest" => "gpt-4o-mini".into(),
        "gpt-4-turbo-latest" => "gpt-4-turbo".into(),
        "gpt-4-latest" => "gpt-4-turbo".into(),
        "gpt-3.5-turbo-latest" => "gpt-3.5-turbo".into(),
        "gpt-5-chat-latest" | "gpt-5-latest" => "gpt-5.4-mini".into(),
        "o1-latest" => "o1".into(),
        "o3-mini-latest" => "o3-mini".into(),
        "o4-mini-latest" => "o4-mini".into(),
        _ => {
            if lower.ends_with("-latest") {
                let stem = lower.trim_end_matches("-latest");
                if stem.starts_with("gpt-")
                    || stem.starts_with("o1")
                    || stem.starts_with("o3")
                    || stem.starts_with("o4")
                {
                    return stem.to_string();
                }
            }
            trimmed.to_string()
        }
    }
}

/// GPT-5 family and several reasoning-style Chat Completions models reject custom `temperature`
/// (OpenAI returns `unsupported_value` unless you omit the field and use the API default).
fn openai_chat_omits_temperature_parameter(model_id: &str) -> bool {
    let m = model_id.trim().to_ascii_lowercase();
    if m.contains("gpt-5") {
        return true;
    }
    m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4")
}

/// Parse "Please try again in 645ms" from OpenAI TPM / RPM error bodies.
fn openai_retry_after_ms_from_error_body(text: &str) -> Option<u64> {
    let re = Regex::new(r"(?i)try again in\s+(\d+)\s*ms").ok()?;
    re.captures(text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u64>().ok())
}

/// Maps retired Claude 3 / `-latest` aliases to current Messages API model ids (see Anthropic model docs).
fn anthropic_model_for_api(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "claude-3-5-haiku-latest"
        | "claude-3-5-haiku-20241022"
        | "claude-3-haiku-20240307"
        | "claude-3-haiku-latest" => "claude-haiku-4-5".into(),
        "claude-3-5-sonnet-latest"
        | "claude-3-5-sonnet-20240620"
        | "claude-3-5-sonnet-20241022"
        | "claude-3-sonnet-latest"
        | "claude-3-sonnet-20240229" => "claude-sonnet-4-6".into(),
        "claude-3-opus-latest" | "claude-3-opus-20240229" => "claude-opus-4-7".into(),
        _ => trimmed.to_string(),
    }
}

fn gemini_extract_answer(data: &serde_json::Value) -> String {
    data.pointer("/candidates/0/content/parts/0/text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

/// Maps legacy Gemini 1.x/2.x and `-latest` shortcuts to current `generateContent` model ids.
fn gemini_model_for_api(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "gemini-1.5-flash-latest"
        | "gemini-1.5-flash"
        | "gemini-1.5-flash-8b"
        | "gemini-2.0-flash-latest"
        | "gemini-2.0-flash"
        | "gemini-2.0-flash-exp"
        | "gemini-flash-latest"
        | "gemini-flash" => "gemini-3.1-flash-lite".into(),
        "gemini-1.5-pro-latest"
        | "gemini-1.5-pro"
        | "gemini-pro-latest"
        | "gemini-pro"
        | "gemini-2.0-pro-latest"
        | "gemini-2.5-flash-latest"
        | "gemini-2.5-pro-latest"
        | "gemini-2.5-flash"
        | "gemini-2.5-pro" => "gemini-3.1-pro-preview".into(),
        _ => {
            if lower.ends_with("-latest") && lower.starts_with("gemini-") {
                return lower.trim_end_matches("-latest").to_string();
            }
            trimmed.to_string()
        }
    }
}

async fn gemini_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    json_response: bool,
) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_gemini_key()).await?;
    let base = cfg.gemini_base_url.trim_end_matches('/');
    let model = gemini_model_for_api(&cfg.gemini_model);
    let url = format!("{}/models/{}:generateContent", base, model);

    let mut system_chunks: Vec<String> = Vec::new();
    let mut contents: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        if m.role == "system" {
            system_chunks.push(m.collapse_text());
            continue;
        }
        let role = if m.role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": role,
            "parts": gemini_parts(m),
        }));
    }

    let mut body_map = serde_json::Map::new();
    body_map.insert("contents".into(), json!(contents));
    let sys = system_chunks.join("\n").trim().to_string();
    if !sys.is_empty() {
        body_map.insert(
            "systemInstruction".into(),
            json!({ "parts": [{ "text": sys }] }),
        );
    }
    if json_response {
        body_map.insert(
            "generationConfig".into(),
            json!({ "responseMimeType": "application/json" }),
        );
    }
    let body = serde_json::Value::Object(body_map);

    let c = client()?;
    let res = c
        .post(url)
        .query(&[("key", key.as_str())])
        .json(&body)
        .send()
        .await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("gemini error: {}", t));
    }
    let data: serde_json::Value = res.json().await?;
    Ok(gemini_extract_answer(&data))
}

async fn anthropic_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    json_response: bool,
) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_anthropic_key()).await?;
    let mut system = String::new();
    let mut anth_msgs = vec![];
    for m in messages {
        if m.role == "system" {
            system.push_str(&m.collapse_text());
            system.push('\n');
        } else {
            anth_msgs.push(json!({
              "role": if m.role == "assistant" { "assistant" } else { "user" },
              "content": anthropic_message_content(m),
            }));
        }
    }
    let url = "https://api.anthropic.com/v1/messages";
    let model = anthropic_model_for_api(&cfg.anthropic_model);
    let max_tokens = if json_response { 32_768 } else { 8192 };
    let mut body = json!({
      "model": model,
      "max_tokens": max_tokens,
      "system": system.trim(),
      "messages": anth_msgs,
    });
    if json_response {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("output_config".into(), anthropic_ingest_output_config());
        }
    }
    let c = client()?;
    let mut res = c
        .post(url)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;
    if !res.status().is_success() {
        let status = res.status();
        let t = res.text().await.unwrap_or_default();
        if json_response && status == reqwest::StatusCode::BAD_REQUEST {
            body = json!({
              "model": model,
              "max_tokens": max_tokens,
              "system": system.trim(),
              "messages": anth_msgs,
            });
            res = c
                .post(url)
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await?;
            if !res.status().is_success() {
                let t2 = res.text().await.unwrap_or_default();
                return Err(anyhow!("anthropic error: {}", t2));
            }
        } else {
            return Err(anyhow!("anthropic error: {}", t));
        }
    }
    let v: AnthropicResp = res.json().await?;
    let text: String = v
        .content
        .into_iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(text)
}

/// Stream assistant tokens to callback (OpenAI-compatible SSE or Ollama NDJSON).
pub async fn stream_chat<F>(
    provider: &str,
    cfg: &AppConfig,
    messages: &[LlmMessage],
    mut on_delta: F,
) -> Result<()>
where
    F: FnMut(String),
{
    match provider {
        "ollama" => stream_ollama(cfg, messages, on_delta).await,
        "openai" => stream_openai_like(cfg, messages, true, &None, on_delta).await,
        "compatible" => stream_openai_like(cfg, messages, false, &Some(cfg.compatible_base_url.clone()), on_delta).await,
        "gemini" => {
            let full = gemini_complete(cfg, messages, false).await?;
            on_delta(full);
            Ok(())
        }
        "anthropic" => {
            let full = anthropic_complete(cfg, messages, false).await?;
            on_delta(full);
            Ok(())
        }
        _ => Err(anyhow!("unknown provider {}", provider)),
    }
}

async fn stream_ollama<F>(cfg: &AppConfig, messages: &[LlmMessage], mut on_delta: F) -> Result<()>
where
    F: FnMut(String),
{
    let url = format!(
        "{}/api/chat",
        cfg.ollama_base_url.trim_end_matches('/')
    );
    let body = json!({
      "model": cfg.ollama_model,
      "messages": messages.iter().map(|m| ollama_message_json(m)).collect::<Vec<_>>(),
      "stream": true,
    });
    let c = client()?;
    let res = c.post(&url).json(&body).send().await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("ollama stream error: {}", t));
    }
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(delta) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                    on_delta(delta.to_string());
                }
            }
        }
    }
    Ok(())
}

async fn stream_openai_like<F>(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    openai: bool,
    compatible_base: &Option<String>,
    mut on_delta: F,
) -> Result<()>
where
    F: FnMut(String),
{
    let key = if openai {
        blocking_secret(|| secrets::resolve_openai_key()).await?
    } else {
        blocking_secret(|| secrets::resolve_compatible_key()).await?
    };
    let url = if openai {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        format!(
            "{}/chat/completions",
            compatible_base.as_ref().unwrap().trim_end_matches('/')
        )
    };
    let model = if openai {
        openai_chat_model_for_api(&cfg.openai_model)
    } else {
        openai_chat_model_for_api(&cfg.compatible_model)
    };
    let messages_json = openai_messages_json(messages);
    let body = json!({
      "model": model,
      "messages": messages_json,
      "temperature": 0.3,
      "stream": true,
    });
    let c = client()?;
    let res = c.post(&url).bearer_auth(key).json(&body).send().await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("stream error: {}", t));
    }
    let mut stream = res.bytes_stream();
    let mut carry = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        carry.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = carry.find('\n') {
            let mut line = carry[..pos].trim().to_string();
            carry.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if line.starts_with("data:") {
                line = line.trim_start_matches("data:").trim().to_string();
            }
            if line == "[DONE]" {
                return Ok(());
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(delta) = v
                    .pointer("/choices/0/delta/content")
                    .and_then(|c| c.as_str())
                {
                    on_delta(delta.to_string());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod llm_vision_tests {
    use super::*;

    #[test]
    fn provider_supports_gpt4o() {
        assert!(provider_supports_vision("openai", "gpt-4o-mini"));
        assert!(!provider_supports_vision("openai", "gpt-3.5-turbo"));
    }

    #[test]
    fn provider_supports_claude_sonnet4() {
        assert!(provider_supports_vision("anthropic", "claude-sonnet-4-6"));
        assert!(!provider_supports_vision("anthropic", "claude-2.1"));
    }

    #[test]
    fn openai_message_includes_image_url() {
        let msgs = vec![
            LlmMessage::text("system", "sys"),
            LlmMessage::vision_user("look", vec![("image/png".into(), "AAA".into())]),
        ];
        let j = openai_messages_json(&msgs);
        let s = serde_json::to_string(&j).unwrap();
        assert!(s.contains("image_url"));
        assert!(s.contains("data:image/png;base64,AAA"));
    }

    #[test]
    fn ollama_message_includes_images_array() {
        let m = LlmMessage::vision_user("x", vec![("image/png".into(), "Ym9n".into())]);
        let v = ollama_message_json(&m);
        assert!(v.get("images").is_some());
    }
}
