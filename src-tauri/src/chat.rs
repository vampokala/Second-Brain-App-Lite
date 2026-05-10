//! Chat context pack + persistence helpers.

use crate::config::{resolved_triple, AppConfig};
use crate::llm::{stream_chat, LlmMessage};
use crate::paths::user_data_dir;
use crate::retrieval;
use crate::sessions::{ChatMessage, SessionFile};
use crate::wiki;
use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;

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

pub async fn stream_session_chat(
    cfg: &AppConfig,
    session: &SessionFile,
    user_message: String,
    on_delta: impl FnMut(String),
) -> Result<()> {
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

    let hits = retrieval::search(&wiki_dir, &user_message, 8);
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

    let system = format!(
        r#"You are the wiki maintainer (see CLAUDE.md + llm-wiki.md pattern below).
Answer using the retrieved wiki excerpts and index/glossary context.
After your answer, end with a short section:

### Sources used
{}

### Schema — CLAUDE.md (excerpt / full)
{}

### Pattern — llm-wiki.md (excerpt / full)
{}

### wiki/index excerpt
{}

### wiki/glossary excerpt
{}

### Rolling memory
{}

### Retrieved wiki excerpts
{}
"#,
        sources_list, claude, llm_wiki, index_excerpt, glossary_excerpt, memory, retrieved
    );

    let mut messages: Vec<LlmMessage> = vec![LlmMessage {
        role: "system".into(),
        content: system,
    }];

    let tail: Vec<_> = session.messages.iter().rev().take(20).rev().cloned().collect();
    for m in tail {
        messages.push(LlmMessage {
            role: m.role,
            content: m.content,
        });
    }

    let provider = cfg.default_provider.as_str();
    stream_chat(provider, cfg, &messages, on_delta).await?;

    Ok(())
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
