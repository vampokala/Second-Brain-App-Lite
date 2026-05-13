//! Chat context pack + persistence helpers.

use crate::config::{normalize_llm_provider, resolved_triple, AppConfig};
use crate::llm::{stream_chat, LlmMessage};
use crate::paths::user_data_dir;
use crate::personas;
use crate::retrieval::{self, RetrievalHit};
use crate::secrets;
use crate::sessions::{ChatMessage, SessionFile};
use crate::web_compile;
use crate::wiki;
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRetrievalMeta {
    pub wiki_sources_only: bool,
    pub include_web_search: bool,
    pub hit_count: usize,
    pub max_score: f64,
    pub brave_key_configured: bool,
    pub web_pages_fetched: u32,
    pub persona_display: String,
    pub persona_addon_applied: bool,
}

pub fn memory_path() -> Result<PathBuf> {
    Ok(user_data_dir()?.join("context").join("memory.md"))
}

pub fn load_memory_excerpt(max: usize) -> String {
    let Ok(p) = memory_path() else {
        return String::new();
    };
    if !p.exists() {
        return String::new();
    }
    wiki::read_optional(&p, max)
}

pub async fn stream_session_chat<FMeta, FDelta>(
    cfg: &AppConfig,
    session: &SessionFile,
    user_message: String,
    wiki_sources_only: bool,
    include_web_search: bool,
    on_meta: FMeta,
    on_delta: FDelta,
) -> Result<()>
where
    FMeta: FnOnce(ChatRetrievalMeta),
    FDelta: FnMut(String),
{
    let (raw_dir, wiki_dir, schema_dir) = resolved_triple(cfg)?;
    let wiki_dir = wiki_dir.canonicalize().unwrap_or(wiki_dir);
    let schema_dir = schema_dir.canonicalize().unwrap_or(schema_dir);

    let _ = raw_dir;

    let (sp_claude, sp_llm) = wiki::schema_paths(&schema_dir);
    let claude = wiki::read_optional(&sp_claude, 500_000);
    let llm_wiki = wiki::read_optional(&sp_llm, 500_000);

    let index_excerpt = wiki::read_optional(&wiki_dir.join("index.md"), 14_000);
    let glossary_excerpt = wiki::read_optional(&wiki_dir.join("glossary.md"), 6000);
    let memory = load_memory_excerpt(4000);

    let track_filter = cfg
        .retrieval_track_filter
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let hits: Vec<RetrievalHit> = if wiki_sources_only {
        retrieval::search(&wiki_dir, &user_message, 8, track_filter)
    } else {
        vec![]
    };

    let mut retrieved = String::new();
    for h in &hits {
        retrieved.push_str(&format!(
            "### {}\n(score {:.2})\n{}\n\n",
            h.path, h.score, h.excerpt
        ));
    }

    let sources_list: String = hits
        .iter()
        .map(|h| format!("- {}", h.path))
        .collect::<Vec<_>>()
        .join("\n");

    let hit_count = hits.len();
    let max_score = hits.first().map(|h| h.score).unwrap_or(0.0);

    let mut web_block = String::new();
    let mut web_pages_fetched: u32 = 0;
    if include_web_search {
        let pack = web_compile::brave_search_and_fetch_context(cfg, user_message.trim()).await?;
        web_pages_fetched = pack.pages_fetched;
        web_block = format!(
            r###"### Web search results (fetched pages; cite by URL)

{}

"###,
            pack.context_block
        );
    }

    let brave_key_configured = secrets::brave_search_key_configured();
    on_meta(ChatRetrievalMeta {
        wiki_sources_only,
        include_web_search,
        hit_count,
        max_score,
        brave_key_configured,
        web_pages_fetched,
        persona_display: personas::persona_display(cfg),
        persona_addon_applied: personas::persona_addon_applied(cfg),
    });

    let persona_block = personas::build_persona_system_section(cfg);

    let mode_instructions = mode_block(
        wiki_sources_only,
        include_web_search,
        hit_count,
        !web_block.is_empty(),
    );

    let wiki_section = if wiki_sources_only {
        format!(
            r###"### Retrieved wiki excerpts (ground wiki-specific claims here)
{}

"###,
            if retrieved.trim().is_empty() {
                "(No matching wiki pages were retrieved for this query.)\n".to_string()
            } else {
                retrieved.clone()
            }
        )
    } else {
        String::new()
    };

    let web_section = if include_web_search {
        web_block.clone()
    } else {
        String::new()
    };

    let system = format!(
        r###"{persona_block}{mode_instructions}

After your answer, end with a short section:

### Sources used
{sources_list}

### Schema — CLAUDE.md (excerpt / full)
{claude}

### Pattern — llm-wiki.md (excerpt / full)
{llm_wiki}

### wiki/index excerpt
{index_excerpt}

### wiki/glossary excerpt
{glossary_excerpt}

### Rolling memory
{memory}

{wiki_section}{web_section}"###,
        persona_block = persona_block,
        mode_instructions = mode_instructions,
        sources_list = if wiki_sources_only {
            sources_list
        } else {
            "(Wiki retrieval disabled for this message.)".into()
        },
        claude = claude,
        llm_wiki = llm_wiki,
        index_excerpt = index_excerpt,
        glossary_excerpt = glossary_excerpt,
        memory = memory,
        wiki_section = wiki_section,
        web_section = web_section,
    );

    let mut messages: Vec<LlmMessage> = vec![LlmMessage::text("system", system)];

    let tail: Vec<_> = session.messages.iter().rev().take(20).rev().cloned().collect();
    for m in tail {
        messages.push(LlmMessage::text(m.role, m.content));
    }

    let provider = normalize_llm_provider(&cfg.default_provider);
    stream_chat(&provider, cfg, &messages, on_delta).await?;

    Ok(())
}

fn mode_block(
    wiki_sources_only: bool,
    include_web_search: bool,
    wiki_hit_count: usize,
    has_web_excerpts: bool,
) -> String {
    match (wiki_sources_only, include_web_search) {
        (true, false) => {
            let extra = if wiki_hit_count == 0 {
                "\nNo wiki excerpts were retrieved — say clearly that the vault did not surface matches; do not invent vault-specific facts.\n"
            } else {
                ""
            };
            format!(
                r#"You are the wiki maintainer (see CLAUDE.md + llm-wiki.md below).
Answer using the **Retrieved wiki excerpts** and index/glossary context. Ground vault-specific statements in those excerpts.{extra}"#
            )
        }
        (true, true) => format!(
            r#"You are the wiki maintainer (see CLAUDE.md + llm-wiki.md below).
You have **two** evidence types: (1) **Retrieved wiki excerpts** — ground vault-specific statements here; (2) **Web search results** — ground statements about the wider web here. Never attribute web-only facts to the wiki, or wiki-only facts to the web. Label provenance when it matters.
Wiki hits this turn: {wiki_hit_count}. Web pages fetched: {has}."#,
            has = if has_web_excerpts { "yes" } else { "none" }
        ),
        (false, true) => format!(
            r#"You are assisting in the wiki maintainer app. Wiki file retrieval is **off** for this message; use **Web search results** (and index/glossary for project vocabulary only). Do not claim content came from wiki files. Web pages fetched: {has}."#,
            has = if has_web_excerpts { "yes" } else { "none" }
        ),
        (false, false) => r#"You are assisting in the wiki maintainer app.
For this message the user disabled **wiki file retrieval** and **web search**. You still see schema/index/glossary/memory for tone and project vocabulary only — do **not** treat them as live evidence for factual claims about the outside world or the vault.
Do not pretend answers came from retrieved wiki pages or fetched web pages. You may help with general reasoning, drafting, or tasks that do not require vault-specific or live-web grounding."#
            .to_string(),
    }
}

pub fn push_message(sess: &mut SessionFile, role: &str, content: String) {
    let ts = Utc::now().to_rfc3339();
    sess.messages.push(ChatMessage {
        role: role.into(),
        content,
        ts,
    });
    sess.updated = Utc::now().to_rfc3339();
}
