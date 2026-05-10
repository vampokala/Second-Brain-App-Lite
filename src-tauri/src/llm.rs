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

pub async fn complete_chat(provider: &str, cfg: &AppConfig, messages: &[LlmMessage]) -> Result<String> {
    match provider {
        "ollama" => ollama_complete(cfg, messages, false).await,
        "openai" => openai_complete(cfg, messages, false).await,
        "anthropic" => anthropic_complete(cfg, messages).await,
        "compatible" => openai_compatible_complete(cfg, messages, false).await,
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
    let key = secrets::openai_key().context("OPENAI_API_KEY / saved OpenAI key missing")?;
    let url = "https://api.openai.com/v1/chat/completions";
    let body = json!({
      "model": cfg.openai_model,
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
    let key = secrets::compatible_key().context("compatible API key missing")?;
    let base = cfg.compatible_base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base);
    let body = json!({
      "model": cfg.compatible_model,
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

async fn anthropic_complete(cfg: &AppConfig, messages: &[LlmMessage]) -> Result<String> {
    let key = secrets::anthropic_key().context("ANTHROPIC_API_KEY / saved key missing")?;
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
    let body = json!({
      "model": cfg.anthropic_model,
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
        secrets::openai_key().context("OpenAI key missing")?
    } else {
        secrets::compatible_key().context("Compatible API key missing")?
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
        cfg.openai_model.clone()
    } else {
        cfg.compatible_model.clone()
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
