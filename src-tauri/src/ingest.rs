//! Lite ingest: new/changed documents under raw → wiki/sources + index + log.

use crate::atomic;
use crate::config::{normalize_llm_provider, resolved_triple, AppConfig};
use crate::llm::{complete_chat, LlmMessage};
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
    #[serde(alias = "bodyMarkdown")]
    pub body_markdown: String,
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

fn build_frontmatter(
    title: &str,
    raw_rel: &str,
    tags: &[String],
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
    for t in tags {
        tag_seq.push(serde_yaml::Value::String(t.clone()));
    }
    root.insert(
        serde_yaml::Value::String("tags".into()),
        serde_yaml::Value::Sequence(tag_seq),
    );
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(root))?;
    Ok(format!("---\n{}---\n\n", yaml))
}

fn upsert_index_sources(index_content: &str, slug: &str, summary: &str, today: &str) -> String {
    let line = format!(
        "- [[sources/{}]] — {} | source | {}",
        slug, summary, today
    );
    let anchor = format!("[[sources/{}]]", slug);
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

fn append_log(wiki_dir: &Path, title: &str, slug: &str, today: &str) -> Result<()> {
    let log_path = wiki_dir.join("log.md");
    let entry = format!(
        "\n## [{}] ingest | {}\n\nPages created: wiki/sources/{}.md\nPages updated: wiki/index.md, wiki/log.md\nKey additions: lite ingest summary page\n\n",
        today, title, slug
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
pub fn save_paste_to_raw(cfg: &AppConfig, content: &str, stem_opt: Option<&str>) -> Result<String> {
    if content.len() > MAX_PASTE_BYTES {
        anyhow::bail!("paste exceeds {} KiB", MAX_PASTE_BYTES / 1024);
    }
    let (raw_dir, _, _) = resolved_triple(cfg)?;
    let raw_dir = raw_dir.canonicalize().unwrap_or(raw_dir);
    let paste_dir = raw_dir.join("pastes");
    std::fs::create_dir_all(&paste_dir).context("create raw/pastes")?;

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

    let rel = format!("pastes/{candidate}.md");
    atomic::atomic_write(&path, content.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(rel)
}

pub async fn run_ingest<F>(cfg: &AppConfig, full_tier: bool, mut on_progress: F) -> Result<Vec<FileIngestResult>>
where
    F: FnMut(IngestProgressPayload) + Send,
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
    let mut manifest = crate::manifest::load_manifest()?;
    let mut results = vec![];

    let mut raw_paths: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(&raw_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !crate::extract::is_supported_raw_file(path) {
            continue;
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

    for (idx, path) in raw_paths.iter().enumerate() {
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

        let tier_hint = if full_tier {
            "Full tier: also propose glossary_patch with any important new terms (markdown bullets)."
        } else {
            "Lite tier: glossary_patch only if a few critical terms are obvious."
        };

        let sys = format!(
            "{}\n\nYou output ONLY valid JSON (no markdown fences). Keys: slug (kebab-case filename stem), title, one_line_summary (short), body_markdown (markdown body WITHOUT YAML frontmatter), tags (string array), glossary_patch (string or empty).\n\n{}",
            tier_hint,
            "Integrate with existing wiki style per schema."
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

        let parsed = match complete_chat(&provider, cfg, &messages).await {
            Ok(raw) => match parse_ingest_json(&raw) {
                Ok(p) => p,
                Err(e) => {
                    let detail = format!("LLM JSON: {}", e);
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

        let page = format!("{}{}", fm, parsed.body_markdown);
        let sources_dir = wiki_dir.join("sources");
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
        let new_index = upsert_index_sources(
            &index_body,
            &slug,
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

        if let Err(e) = append_log(&wiki_dir, &parsed.title, &slug, &today) {
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
                wiki_paths: vec![format!("sources/{}.md", slug)],
            },
        );

        let wrote = format!("sources/{}.md", slug);
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

    crate::manifest::save_manifest(&manifest)?;

    let ok_n = results.iter().filter(|r| r.status == "ok").count();
    let err_n = results.iter().filter(|r| r.status == "error").count();
    let skip_n = results.iter().filter(|r| r.status == "skipped").count();
    on_progress(IngestProgressPayload {
        phase: "complete".into(),
        message: format!(
            "Finished — ok: {}, skipped: {}, errors: {}; manifest saved",
            ok_n, skip_n, err_n
        ),
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
}
