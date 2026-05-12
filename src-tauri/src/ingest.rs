//! Lite ingest: new/changed documents under raw → wiki/sources + index + log.

use crate::atomic;
use crate::config::{normalize_llm_provider, resolved_triple, AppConfig};
use crate::llm::{complete_chat, ChatCompleteOptions, LlmMessage};
use base64::Engine as _;
use crate::manifest::{sha256_bytes, ManifestEntry};
use crate::wiki;
use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestProgressPayload {
    pub phase: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
}

#[derive(Debug, Deserialize)]
/// Matches ingest prompt keys (snake_case). Aliases accept models that emit camelCase instead.
#[serde(rename_all = "snake_case")]
pub struct IngestLlmJson {
    #[serde(default)]
    pub slug: String,
    pub title: String,
    #[serde(alias = "oneLineSummary")]
    pub one_line_summary: String,
    /// Wiki body when short; large bodies should use `body_markdown_b64` instead.
    #[serde(default, alias = "bodyMarkdown")]
    pub body_markdown: String,
    /// Base64 (standard alphabet) of UTF-8 markdown body — avoids invalid JSON from code fences/quotes.
    #[serde(default, alias = "bodyMarkdownB64")]
    pub body_markdown_b64: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, alias = "glossaryPatch")]
    pub glossary_patch: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIngestResult {
    pub relative_raw_path: String,
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInference {
    pub track_id: Option<String>,
    pub confidence: f32,
    pub reason: String,
}

fn strip_json_fence(s: &str) -> String {
    let s = s.trim();
    let Some(pos) = s.find("```") else {
        return s.to_string();
    };
    let mut inner = &s[pos + 3..];
    inner = inner.trim_start();
    if inner.starts_with("json") {
        inner = &inner["json".len()..];
    } else if inner.starts_with("JSON") {
        inner = &inner["JSON".len()..];
    }
    inner = inner.trim_start_matches(|c| c == '\n' || c == '\r');
    if let Some(end) = inner.find("```") {
        inner[..end].trim().to_string()
    } else {
        inner.trim().to_string()
    }
}

/// Find the first `{ ... }` slice with brace depth, respecting JSON string escapes.
fn extract_balanced_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let rest = &s[start..];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in rest.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = i + ch.len_utf8();
                        return Some(rest[..end].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn parse_ingest_json(raw: &str) -> Result<IngestLlmJson> {
    let trimmed = raw.trim();
    let fenced = strip_json_fence(trimmed);

    let mut last_err: Option<serde_json::Error> = None;

    for candidate in [fenced.as_str(), trimmed] {
        match serde_json::from_str::<IngestLlmJson>(candidate) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
        if let Some(obj) = extract_balanced_json_object(candidate) {
            match serde_json::from_str::<IngestLlmJson>(&obj) {
                Ok(v) => return Ok(v),
                Err(e) => last_err = Some(e),
            }
        }
    }

    match last_err {
        Some(e) => Err(e).context("parse ingest JSON from model"),
        None => Err(anyhow::anyhow!(
            "parse ingest JSON from model: no JSON object found"
        )),
    }
}

fn resolve_ingest_body(parsed: &IngestLlmJson) -> Result<String> {
    if let Some(ref b64) = parsed.body_markdown_b64 {
        let compact: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
        if !compact.is_empty() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(compact.as_bytes())
                .context("decode body_markdown_b64")?;
            return String::from_utf8(bytes).context("body_markdown_b64 is not valid UTF-8");
        }
    }
    if !parsed.body_markdown.trim().is_empty() {
        return Ok(parsed.body_markdown.clone());
    }
    anyhow::bail!("model returned empty body: set body_markdown_b64 (preferred) or body_markdown");
}

fn kebab_slug(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "note".into())
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else {
                '-'
            }
        })
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

pub fn sanitize_track_id(input: &str) -> String {
    input
        .trim()
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

fn track_from_rel(rel: &str) -> Option<String> {
    let mut parts = rel.split('/');
    let head = parts.next()?.trim();
    if head.is_empty() {
        return None;
    }
    let t = sanitize_track_id(head);
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn ensure_track_tag(tags: &[String], track: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = tags.iter().filter(|t| !t.trim().is_empty()).cloned().collect();
    if let Some(t) = track {
        let marker = format!("track:{t}");
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&marker)) {
            out.push(marker);
        }
    }
    out
}

pub fn list_tracks(cfg: &AppConfig) -> Result<Vec<String>> {
    let (raw_dir, _, _) = resolved_triple(cfg)?;
    let raw_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let mut tracks = vec![];
    let Ok(entries) = std::fs::read_dir(&raw_dir) else {
        return Ok(tracks);
    };
    for ent in entries.flatten() {
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let tid = sanitize_track_id(&name);
        if tid.is_empty() {
            continue;
        }
        tracks.push(tid);
    }
    tracks.sort();
    tracks.dedup();
    Ok(tracks)
}

pub fn infer_track_for_content_detailed(
    cfg: &AppConfig,
    content: &str,
    hint: Option<&str>,
) -> Result<TrackInference> {
    let tracks = list_tracks(cfg)?;
    if tracks.is_empty() {
        return Ok(TrackInference {
            track_id: None,
            confidence: 0.0,
            reason: "No tracks found under raw/".into(),
        });
    }
    let hay = format!("{} {}", hint.unwrap_or_default(), content).to_lowercase();
    let mut best: Option<(String, usize)> = None;
    let mut second_best = 0usize;
    for t in tracks {
        let mut score = 0usize;
        for token in t.split('-').filter(|x| x.len() > 2) {
            score += hay.matches(token).count();
        }
        if hay.contains(&t) {
            score += 3;
        }
        if score == 0 {
            continue;
        }
        match &best {
            None => best = Some((t, score)),
            Some((_, s)) if score > *s => {
                second_best = *s;
                best = Some((t, score));
            }
            Some((_, s)) if score > second_best && score <= *s => {
                second_best = score;
            }
            _ => {}
        }
    }
    let Some((track, top_score)) = best else {
        return Ok(TrackInference {
            track_id: None,
            confidence: 0.0,
            reason: "No confident token matches found".into(),
        });
    };
    let confidence = if top_score == 0 {
        0.0
    } else {
        (top_score as f32 / (top_score + second_best + 1) as f32).min(0.99)
    };
    Ok(TrackInference {
        track_id: Some(track),
        confidence,
        reason: format!("topScore={top_score}, runnerUp={second_best}"),
    })
}

fn build_frontmatter(
    title: &str,
    raw_rel: &str,
    tags: &[String],
    track_id: Option<&str>,
    today: &str,
) -> Result<String> {
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::String("title".into()),
        serde_yaml::Value::String(title.into()),
    );
    root.insert(
        serde_yaml::Value::String("type".into()),
        serde_yaml::Value::String("source".into()),
    );
    root.insert(
        serde_yaml::Value::String("created".into()),
        serde_yaml::Value::String(today.into()),
    );
    root.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(today.into()),
    );
    let mut sources = serde_yaml::Sequence::new();
    sources.push(serde_yaml::Value::String(format!("raw/{}", raw_rel)));
    root.insert(
        serde_yaml::Value::String("sources".into()),
        serde_yaml::Value::Sequence(sources),
    );
    let mut tag_seq = serde_yaml::Sequence::new();
    for t in ensure_track_tag(tags, track_id).iter() {
        tag_seq.push(serde_yaml::Value::String(t.clone()));
    }
    root.insert(
        serde_yaml::Value::String("tags".into()),
        serde_yaml::Value::Sequence(tag_seq),
    );
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(root))?;
    Ok(format!("---\n{}---\n\n", yaml))
}

fn upsert_index_sources(index_content: &str, source_rel: &str, summary: &str, today: &str) -> String {
    let line = format!(
        "- [[sources/{}]] — {} | source | {}",
        source_rel, summary, today
    );
    let anchor = format!("[[sources/{}]]", source_rel);
    if index_content.contains(&anchor) {
        return index_content.to_string();
    }
    let re =
        Regex::new(r"\*\([Nn]o sources[^\)]*\)\*").expect("regex");
    let stripped_placeholders = re.replace_all(index_content, "").to_string();

    let needle = "## Sources";
    if let Some(idx) = stripped_placeholders.find(needle) {
        let after = idx + needle.len();
        let head = &stripped_placeholders[..after];
        let tail = stripped_placeholders[after..].trim_start();
        let insert = format!("\n\n{}\n\n", line);
        return format!("{}{}{}", head, insert, tail);
    }

    format!(
        "{}\n\n## Sources\n\n{}\n",
        stripped_placeholders.trim_end(),
        line
    )
}

fn append_log(wiki_dir: &Path, title: &str, source_rel: &str, today: &str) -> Result<()> {
    let log_path = wiki_dir.join("log.md");
    let entry = format!(
        "\n## [{}] ingest | {}\n\nPages created: wiki/sources/{}.md\nPages updated: wiki/index.md, wiki/log.md\nKey additions: lite ingest summary page\n\n",
        today, title, source_rel
    );
    let mut prev = std::fs::read_to_string(&log_path).unwrap_or_default();
    prev.push_str(&entry);
    atomic::atomic_write(&log_path, prev.as_bytes())
}

fn maybe_patch_glossary(wiki_dir: &Path, patch: Option<&str>) -> Result<()> {
    let Some(p) = patch.filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let gp = wiki_dir.join("glossary.md");
    let mut body = std::fs::read_to_string(&gp).unwrap_or_default();
    body.push_str("\n\n");
    body.push_str(p.trim());
    body.push('\n');
    atomic::atomic_write(&gp, body.as_bytes())
}

/// Maximum UTF-8 size for a single pasted ingest payload.
const MAX_PASTE_BYTES: usize = 512 * 1024;

fn slugify_paste_stem(input: &str) -> String {
    input
        .trim()
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

/// Writes UTF-8 markdown under `raw/pastes/`. Returns path relative to `raw/` (e.g. `pastes/note.md`).
pub fn save_paste_to_raw(
    cfg: &AppConfig,
    content: &str,
    stem_opt: Option<&str>,
    track_opt: Option<&str>,
) -> Result<String> {
    if content.len() > MAX_PASTE_BYTES {
        anyhow::bail!("paste exceeds {} KiB", MAX_PASTE_BYTES / 1024);
    }
    let (raw_dir, _, _) = resolved_triple(cfg)?;
    let raw_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let track = track_opt
        .map(sanitize_track_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "inbox".into());
    let paste_dir = raw_dir.join(&track).join("pastes");
    std::fs::create_dir_all(&paste_dir).context("create raw/<track>/pastes")?;

    let mut stem = stem_opt
        .map(slugify_paste_stem)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("paste-{}", Utc::now().format("%Y%m%d-%H%M%S")));

    stem = stem.chars().take(120).collect::<String>().trim_matches('-').to_string();
    if stem.is_empty() {
        stem = format!("paste-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    }

    let stem_base = stem.clone();
    let mut candidate = stem_base.clone();
    let mut path = paste_dir.join(format!("{candidate}.md"));
    let mut n = 2u32;
    while path.exists() {
        candidate = format!("{stem_base}-{n}");
        path = paste_dir.join(format!("{candidate}.md"));
        n += 1;
        if n > 10_000 {
            anyhow::bail!("could not pick a unique paste filename");
        }
    }

    let rel = format!("{track}/pastes/{candidate}.md");
    atomic::atomic_write(&path, content.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(rel)
}

fn slug_from_url(url: &str) -> String {
    sanitize_track_id(url)
        .chars()
        .take(80)
        .collect::<String>()
}

/// Fetch a URL and return `(title, plain_text)` with HTML stripped.
/// **No SSRF policy** — only use with URLs you trust (e.g. user-pasted ingest). For Brave result URLs, validate first.
pub async fn fetch_url_plain_excerpt(
    url: &str,
    timeout: std::time::Duration,
    max_body_bytes: usize,
    max_chars: usize,
) -> Result<(String, String)> {
    let client = reqwest::Client::builder().timeout(timeout).build()?;
    let res = client.get(url).send().await.context("fetch url")?;
    if !res.status().is_success() {
        anyhow::bail!("fetch failed with status {}", res.status());
    }
    let body_bytes = res.bytes().await.context("read url body")?;
    let slice: &[u8] = if body_bytes.len() > max_body_bytes {
        &body_bytes[..max_body_bytes]
    } else {
        &body_bytes
    };
    let body = String::from_utf8_lossy(slice);
    let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex");
    let title = title_re
        .captures(&body)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Web Article".into());
    let cleaned = crate::extract::extract_plain_text(Path::new("page.html"), body.as_bytes())
        .unwrap_or_else(|_| body.into_owned());
    let excerpt: String = cleaned.chars().take(max_chars).collect();
    Ok((title, excerpt))
}

pub async fn save_url_to_raw(
    cfg: &AppConfig,
    url: &str,
    stem_opt: Option<&str>,
    track_opt: Option<&str>,
) -> Result<String> {
    let (title, cleaned) = fetch_url_plain_excerpt(
        url,
        std::time::Duration::from_secs(20),
        5_000_000,
        2_000_000,
    )
    .await
    .context("fetch url")?;

    let mut stem = stem_opt
        .map(slugify_paste_stem)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slug_from_url(url));
    if stem.is_empty() {
        stem = format!("web-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    }

    let content = format!(
        "# {}\n\nSource URL: {}\nFetched At: {}\n\n---\n\n{}",
        title,
        url,
        Utc::now().to_rfc3339(),
        cleaned
    );
    let track = track_opt
        .map(sanitize_track_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "inbox".into());
    let (raw_dir, _, _) = resolved_triple(cfg)?;
    let raw_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let web_dir = raw_dir.join(&track).join("web");
    std::fs::create_dir_all(&web_dir)?;
    let out = web_dir.join(format!("{stem}.md"));
    atomic::atomic_write(&out, content.as_bytes())?;
    Ok(format!("{track}/web/{stem}.md"))
}

pub async fn run_ingest<F, C>(
    cfg: &AppConfig,
    full_tier: bool,
    track_filter: Option<&str>,
    mut on_progress: F,
    should_cancel: C,
) -> Result<Vec<FileIngestResult>>
where
    F: FnMut(IngestProgressPayload) + Send,
    C: Fn() -> bool + Send,
{
    let (raw_dir, wiki_dir, schema_dir) = resolved_triple(cfg)?;
    let raw_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let wiki_dir = wiki_dir.canonicalize().unwrap_or(wiki_dir);
    let schema_dir = schema_dir.canonicalize().unwrap_or(schema_dir);

    wiki::ensure_wiki_layout(&wiki_dir)?;

    on_progress(IngestProgressPayload {
        phase: "prepare".into(),
        message: "Loading schema excerpts; scanning raw/ for documents…".into(),
        current: None,
        total: None,
        relative_path: None,
    });

    let claude = wiki::read_optional(&wiki::schema_paths(&schema_dir).0, 500_000);
    let llm_wiki = wiki::read_optional(&wiki::schema_paths(&schema_dir).1, 500_000);
    let index_excerpt = wiki::read_optional(&wiki_dir.join("index.md"), 12_000);
    let glossary_excerpt = wiki::read_optional(&wiki_dir.join("glossary.md"), 8000);

    let provider = normalize_llm_provider(&cfg.default_provider);
    let mut manifest = crate::manifest::load_manifest_for(&raw_dir)?;
    let mut results = vec![];

    let mut raw_paths: Vec<PathBuf> = Vec::new();
    let track_filter = track_filter.map(sanitize_track_id).filter(|s| !s.is_empty());
    for entry in WalkDir::new(&raw_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !crate::extract::is_supported_raw_file(path) {
            continue;
        }
        if let Some(tf) = track_filter.as_ref() {
            let rel = path
                .strip_prefix(&raw_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if !rel.starts_with(&format!("{}/", tf)) {
                continue;
            }
        }
        raw_paths.push(path.to_path_buf());
    }
    raw_paths.sort();
    let total_n = raw_paths.len() as u32;

    on_progress(IngestProgressPayload {
        phase: "start".into(),
        message: format!(
            "Found {} supported file(s) under raw/ ({})",
            total_n,
            crate::extract::SUPPORTED_EXTENSIONS.join(", ")
        ),
        current: None,
        total: Some(total_n),
        relative_path: None,
    });

    let mut cancelled = false;
    for (idx, path) in raw_paths.iter().enumerate() {
        if should_cancel() {
            cancelled = true;
            on_progress(IngestProgressPayload {
                phase: "cancelled".into(),
                message: "Ingest stopped by user request.".into(),
                current: Some(idx as u32),
                total: Some(total_n),
                relative_path: None,
            });
            results.push(FileIngestResult {
                relative_raw_path: "(ingest)".into(),
                status: "cancelled".into(),
                detail: Some("Stopped by user request".into()),
            });
            break;
        }

        let current_i = (idx + 1) as u32;
        let rel = path
            .strip_prefix(&raw_dir)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");

        on_progress(IngestProgressPayload {
            phase: "file".into(),
            message: "Inspecting file".into(),
            current: Some(current_i),
            total: Some(total_n),
            relative_path: Some(rel.clone()),
        });

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                let detail = format!("read: {}", e);
                on_progress(IngestProgressPayload {
                    phase: "error".into(),
                    message: detail.clone(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "error".into(),
                    detail: Some(detail),
                });
                continue;
            }
        };

        let hash = sha256_bytes(&bytes);
        if let Some(ent) = manifest.entries.get(&rel) {
            if ent.content_sha256 == hash {
                on_progress(IngestProgressPayload {
                    phase: "skipped".into(),
                    message: "Unchanged (hash matches manifest); skipping LLM".into(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "skipped".into(),
                    detail: Some("unchanged".into()),
                });
                continue;
            }
        }

        let text = match crate::extract::extract_plain_text(path, &bytes) {
            Ok(t) => t,
            Err(e) => {
                let detail = format!("extract text: {}", e);
                on_progress(IngestProgressPayload {
                    phase: "error".into(),
                    message: detail.clone(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "error".into(),
                    detail: Some(detail),
                });
                continue;
            }
        };
        if text.trim().is_empty() {
            let detail =
                "extracted text is empty (file may be scanned PDF, encrypted, or blank)".to_string();
            on_progress(IngestProgressPayload {
                phase: "error".into(),
                message: detail.clone(),
                current: Some(current_i),
                total: Some(total_n),
                relative_path: Some(rel.clone()),
            });
            results.push(FileIngestResult {
                relative_raw_path: rel.clone(),
                status: "error".into(),
                detail: Some(detail),
            });
            continue;
        }
        let fallback_slug = kebab_slug(path);
        let track_id = track_from_rel(&rel);

        let tier_hint = if full_tier {
            "Full tier: also propose glossary_patch with any important new terms (markdown bullets)."
        } else {
            "Lite tier: glossary_patch only if a few critical terms are obvious."
        };

        let track_hint = track_id
            .as_ref()
            .map(|t| format!("Track namespace: {t}. Keep this identity explicit in title/body and include tag track:{t}."))
            .unwrap_or_else(|| "Track namespace: unknown/inbox. Infer a concise domain tag when obvious.".into());
        let sys = format!(
            "{}\n\nYou output ONLY valid JSON (no markdown fences). Keys: slug (kebab-case filename stem), title, one_line_summary (short), body_markdown_b64 (REQUIRED: standard Base64 of UTF-8 for the wiki markdown body WITHOUT YAML frontmatter — use this for the full body so JSON stays valid), body_markdown (optional; only if body is tiny with no quotes or code fences), tags (string array), glossary_patch (string or empty).\n\n{}\n{}",
            tier_hint,
            "Integrate with existing wiki style per schema.",
            track_hint
        );

        let user = format!(
            "### CLAUDE.md (schema)\n{}\n\n### llm-wiki.md (pattern)\n{}\n\n### index excerpt\n{}\n\n### glossary excerpt\n{}\n\n### Raw path\nraw/{}\n\n(Plain text only for ingest: PDF/Word/HTML are extracted locally; Markdown files pass through as-is.)\n\n### Raw content\n{}",
            claude,
            llm_wiki,
            index_excerpt,
            glossary_excerpt,
            rel,
            text.chars().take(48_000).collect::<String>()
        );

        let messages = vec![
            LlmMessage {
                role: "system".into(),
                content: sys,
            },
            LlmMessage {
                role: "user".into(),
                content: user,
            },
        ];

        on_progress(IngestProgressPayload {
            phase: "llm".into(),
            message: "Calling model…".into(),
            current: Some(current_i),
            total: Some(total_n),
            relative_path: Some(rel.clone()),
        });

        let parsed = match complete_chat(
            &provider,
            cfg,
            &messages,
            ChatCompleteOptions {
                json_response: true,
            },
        )
        .await
        {
            Ok(raw) => match parse_ingest_json(&raw) {
                Ok(p) => p,
                Err(e) => {
                    let snippet: String = raw.chars().take(500).collect();
                    let detail = format!("LLM JSON: {}; model output (truncated): {}", e, snippet);
                    on_progress(IngestProgressPayload {
                        phase: "error".into(),
                        message: detail.clone(),
                        current: Some(current_i),
                        total: Some(total_n),
                        relative_path: Some(rel.clone()),
                    });
                    results.push(FileIngestResult {
                        relative_raw_path: rel.clone(),
                        status: "error".into(),
                        detail: Some(detail),
                    });
                    continue;
                }
            },
            Err(e) => {
                let detail = format!("LLM: {}", e);
                on_progress(IngestProgressPayload {
                    phase: "error".into(),
                    message: detail.clone(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "error".into(),
                    detail: Some(detail),
                });
                continue;
            }
        };

        let body_markdown = match resolve_ingest_body(&parsed) {
            Ok(b) => b,
            Err(e) => {
                let detail = format!("ingest body: {}", e);
                on_progress(IngestProgressPayload {
                    phase: "error".into(),
                    message: detail.clone(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "error".into(),
                    detail: Some(detail),
                });
                continue;
            }
        };

        let slug = if parsed.slug.trim().is_empty() {
            fallback_slug.clone()
        } else {
            parsed.slug.trim().replace('/', "-").replace('\\', "-")
        };

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let fm = match build_frontmatter(
            &parsed.title,
            &rel,
            &parsed.tags,
            track_id.as_deref(),
            &today,
        ) {
            Ok(fm) => fm,
            Err(e) => {
                let detail = format!("frontmatter: {}", e);
                on_progress(IngestProgressPayload {
                    phase: "error".into(),
                    message: detail.clone(),
                    current: Some(current_i),
                    total: Some(total_n),
                    relative_path: Some(rel.clone()),
                });
                results.push(FileIngestResult {
                    relative_raw_path: rel.clone(),
                    status: "error".into(),
                    detail: Some(detail),
                });
                continue;
            }
        };

        let page = format!("{}{}", fm, body_markdown);
        let track_folder = track_id.clone().unwrap_or_else(|| "inbox".into());
        let sources_dir = wiki_dir.join("sources").join(&track_folder);
        std::fs::create_dir_all(&sources_dir)?;
        let out_path = sources_dir.join(format!("{}.md", slug));
        if let Err(e) = atomic::atomic_write(&out_path, page.as_bytes()) {
            let detail = format!("write source: {}", e);
            on_progress(IngestProgressPayload {
                phase: "error".into(),
                message: detail.clone(),
                current: Some(current_i),
                total: Some(total_n),
                relative_path: Some(rel.clone()),
            });
            results.push(FileIngestResult {
                relative_raw_path: rel.clone(),
                status: "error".into(),
                detail: Some(detail),
            });
            continue;
        }

        let index_path = wiki_dir.join("index.md");
        let index_body = std::fs::read_to_string(&index_path).unwrap_or_default();
        let source_rel = format!("{}/{}", track_folder, slug);
        let new_index = upsert_index_sources(
            &index_body,
            &source_rel,
            &parsed.one_line_summary,
            &today,
        );
        if let Err(e) = atomic::atomic_write(&index_path, new_index.as_bytes()) {
            let detail = format!("write index: {}", e);
            on_progress(IngestProgressPayload {
                phase: "error".into(),
                message: detail.clone(),
                current: Some(current_i),
                total: Some(total_n),
                relative_path: Some(rel.clone()),
            });
            results.push(FileIngestResult {
                relative_raw_path: rel.clone(),
                status: "error".into(),
                detail: Some(detail),
            });
            continue;
        }

        if let Err(e) = append_log(&wiki_dir, &parsed.title, &source_rel, &today) {
            let detail = format!("log: {}", e);
            on_progress(IngestProgressPayload {
                phase: "error".into(),
                message: detail.clone(),
                current: Some(current_i),
                total: Some(total_n),
                relative_path: Some(rel.clone()),
            });
            results.push(FileIngestResult {
                relative_raw_path: rel.clone(),
                status: "error".into(),
                detail: Some(detail),
            });
            continue;
        }

        if let Err(e) = maybe_patch_glossary(&wiki_dir, parsed.glossary_patch.as_deref()) {
            let detail = format!("glossary: {}", e);
            on_progress(IngestProgressPayload {
                phase: "error".into(),
                message: detail.clone(),
                current: Some(current_i),
                total: Some(total_n),
                relative_path: Some(rel.clone()),
            });
            results.push(FileIngestResult {
                relative_raw_path: rel.clone(),
                status: "error".into(),
                detail: Some(detail),
            });
            continue;
        }

        manifest.entries.insert(
            rel.clone(),
            ManifestEntry {
                content_sha256: hash,
                last_ingest_at: Utc::now().to_rfc3339(),
                wiki_paths: vec![format!("sources/{}/{}.md", track_folder, slug)],
            },
        );

        let wrote = format!("sources/{}/{}.md", track_folder, slug);
        on_progress(IngestProgressPayload {
            phase: "ok".into(),
            message: format!("Wrote wiki/{wrote}; appended wiki/log.md"),
            current: Some(current_i),
            total: Some(total_n),
            relative_path: Some(rel.clone()),
        });

        results.push(FileIngestResult {
            relative_raw_path: rel,
            status: "ok".into(),
            detail: Some(wrote),
        });
    }

    crate::manifest::save_manifest_for(&raw_dir, &manifest)?;

    let ok_n = results.iter().filter(|r| r.status == "ok").count();
    let err_n = results.iter().filter(|r| r.status == "error").count();
    let skip_n = results.iter().filter(|r| r.status == "skipped").count();
    on_progress(IngestProgressPayload {
        phase: if cancelled { "cancelled" } else { "complete" }.into(),
        message: if cancelled {
            format!(
                "Stopped — ok: {}, skipped: {}, errors: {}; partial manifest saved",
                ok_n, skip_n, err_n
            )
        } else {
            format!(
                "Finished — ok: {}, skipped: {}, errors: {}; manifest saved",
                ok_n, skip_n, err_n
            )
        },
        current: None,
        total: Some(total_n),
        relative_path: None,
    });

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ingest_prefers_body_markdown_b64() {
        let body = "## Title\n```yaml\nx: 1\n```\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
        let raw = format!(
            r#"{{"slug":"s","title":"T","one_line_summary":"S","body_markdown":"ignored","body_markdown_b64":"{}","tags":[]}}"#,
            b64
        );
        let v = parse_ingest_json(&raw).unwrap();
        assert_eq!(resolve_ingest_body(&v).unwrap(), body);
    }

    #[test]
    fn parse_ingest_accepts_snake_case_keys_from_prompt() {
        let raw = r###"{"slug":"ai-2027","title":"AI 2027","one_line_summary":"Brief","body_markdown":"## Hi","tags":["x"],"glossary_patch":null}"###;
        let v = parse_ingest_json(raw).unwrap();
        assert_eq!(v.slug, "ai-2027");
        assert_eq!(v.one_line_summary, "Brief");
    }

    #[test]
    fn parse_ingest_accepts_camel_case_aliases() {
        let raw = r##"{"title":"T","oneLineSummary":"S","bodyMarkdown":"B","tags":[]}"##;
        let v = parse_ingest_json(raw).unwrap();
        assert_eq!(v.slug, "");
        assert_eq!(v.one_line_summary, "S");
    }

    #[test]
    fn parse_ingest_strips_fence_and_preamble() {
        let raw = r##"Sure — here is JSON:
```json
{"title":"T","one_line_summary":"S","body_markdown":"Body"}
```
"##;
        let v = parse_ingest_json(raw).unwrap();
        assert_eq!(v.title, "T");
        assert_eq!(v.body_markdown, "Body");
    }

    #[test]
    fn sanitize_track_id_normalizes() {
        assert_eq!(sanitize_track_id(" Claims Team "), "claims-team");
        assert_eq!(sanitize_track_id("sales___ops"), "sales-ops");
    }

    #[test]
    fn upsert_index_sources_supports_namespaced_paths() {
        let idx = "## Sources\n\n";
        let out = upsert_index_sources(idx, "claims/meeting-notes", "Summary", "2026-05-11");
        assert!(out.contains("[[sources/claims/meeting-notes]]"));
    }

    #[test]
    fn infer_track_detailed_returns_confidence() {
        let suite_root = std::env::temp_dir().join(format!(
            "sb-lite-track-inf-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let raw_root = suite_root.join("raw");
        let wiki_root = suite_root.join("wiki");
        let schema_root = suite_root.join("schema");
        std::fs::create_dir_all(raw_root.join("claims-modernization")).unwrap();
        std::fs::create_dir_all(raw_root.join("sales-pipeline")).unwrap();
        std::fs::create_dir_all(&wiki_root).unwrap();
        std::fs::create_dir_all(&schema_root).unwrap();

        let mut cfg = AppConfig::default();
        cfg.raw_dir = Some(raw_root.to_string_lossy().to_string());
        cfg.wiki_dir = Some(wiki_root.to_string_lossy().to_string());
        cfg.schema_dir = Some(schema_root.to_string_lossy().to_string());

        let inf = infer_track_for_content_detailed(
            &cfg,
            "Today we discussed claims modernization milestones",
            None,
        )
        .unwrap();
        assert_eq!(inf.track_id.as_deref(), Some("claims-modernization"));
        assert!(inf.confidence > 0.1);
        let _ = std::fs::remove_dir_all(suite_root);
    }
}
