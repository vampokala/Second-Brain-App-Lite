use crate::config::AppConfig;
use crate::secrets;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
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

pub async fn complete_chat(provider: &str, cfg: &AppConfig, messages: &[LlmMessage]) -> Result<String> {
    match provider {
        "ollama" => ollama_complete(cfg, messages, false).await,
        "openai" => openai_complete(cfg, messages, false).await,
        "anthropic" => anthropic_complete(cfg, messages).await,
        "compatible" => openai_compatible_complete(cfg, messages, false).await,
        "gemini" => gemini_complete(cfg, messages).await,
        _ => Err(anyhow!("unknown provider {}", provider)),
    }
}

async fn ollama_complete(cfg: &AppConfig, messages: &[LlmMessage], stream: bool) -> Result<String> {
    let url = format!(
        "{}/api/chat",
        cfg.ollama_base_url.trim_end_matches('/')
    );
    let body = json!({
      "model": cfg.ollama_model,
      "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
      "stream": stream,
    });
    let c = client()?;
    let res = c.post(&url).json(&body).send().await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("ollama error: {}", t));
    }
    let v: OllamaMsg = res.json().await?;
    Ok(v.message.content)
}

async fn openai_complete(cfg: &AppConfig, messages: &[LlmMessage], stream: bool) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_openai_key()).await?;
    let url = "https://api.openai.com/v1/chat/completions";
    let body = json!({
      "model": openai_chat_model_for_api(&cfg.openai_model),
      "messages": messages,
      "temperature": 0.2,
      "stream": stream,
    });
    let c = client()?;
    let res = c
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("openai error: {}", t));
    }
    let v: OpenAiResp = res.json().await?;
    Ok(v.choices
        .get(0)
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default())
}

async fn openai_compatible_complete(
    cfg: &AppConfig,
    messages: &[LlmMessage],
    stream: bool,
) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_compatible_key()).await?;
    let base = cfg.compatible_base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base);
    let body = json!({
      "model": openai_chat_model_for_api(&cfg.compatible_model),
      "messages": messages,
      "temperature": 0.2,
      "stream": stream,
    });
    let c = client()?;
    let res = c.post(&url).bearer_auth(key).json(&body).send().await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("compatible openai error: {}", t));
    }
    let v: OpenAiResp = res.json().await?;
    Ok(v.choices
        .get(0)
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default())
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

async fn gemini_complete(cfg: &AppConfig, messages: &[LlmMessage]) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_gemini_key()).await?;
    let base = cfg.gemini_base_url.trim_end_matches('/');
    let model = gemini_model_for_api(&cfg.gemini_model);
    let url = format!("{}/models/{}:generateContent", base, model);

    let mut system_chunks: Vec<String> = Vec::new();
    let mut contents: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        if m.role == "system" {
            system_chunks.push(m.content.clone());
            continue;
        }
        let role = if m.role == "assistant" { "model" } else { "user" };
        contents.push(json!({
            "role": role,
            "parts": [{"text": m.content}],
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

async fn anthropic_complete(cfg: &AppConfig, messages: &[LlmMessage]) -> Result<String> {
    let key = blocking_secret(|| secrets::resolve_anthropic_key()).await?;
    let mut system = String::new();
    let mut anth_msgs = vec![];
    for m in messages {
        if m.role == "system" {
            system.push_str(&m.content);
            system.push('\n');
        } else {
            anth_msgs.push(json!({
              "role": if m.role == "assistant" { "assistant" } else { "user" },
              "content": m.content,
            }));
        }
    }
    let url = "https://api.anthropic.com/v1/messages";
    let model = anthropic_model_for_api(&cfg.anthropic_model);
    let body = json!({
      "model": model,
      "max_tokens": 8192,
      "system": system.trim(),
      "messages": anth_msgs,
    });
    let c = client()?;
    let res = c
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;
    if !res.status().is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(anyhow!("anthropic error: {}", t));
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
            let full = gemini_complete(cfg, messages).await?;
            on_delta(full);
            Ok(())
        }
        "anthropic" => {
            let full = anthropic_complete(cfg, messages).await?;
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
      "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
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
    let body = json!({
      "model": model,
      "messages": messages,
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
