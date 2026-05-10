//! Bootstrap wiki folders and read schema / wiki slices.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn ensure_wiki_layout(wiki_dir: &Path) -> Result<()> {
    for sub in [
        "sources",
        "features",
        "products",
        "personas",
        "concepts",
        "style",
        "analyses",
    ] {
        std::fs::create_dir_all(wiki_dir.join(sub))?;
    }

    let index = wiki_dir.join("index.md");
    if !index.exists() {
        let body = r#"# Wiki Index

Master catalog of all pages.

---

## Sources

*(No sources ingested yet — use Second Brain Lite **Ingest**.)*

---

## Analyses

*(Empty)*

---

## Core Files

| Page | Summary |
|------|---------|
| [[glossary]] | Terminology |
| [[overview]] | Overview |

"#;
        crate::atomic::atomic_write(&index, body.as_bytes())?;
    }

    let glossary = wiki_dir.join("glossary.md");
    if !glossary.exists() {
        crate::atomic::atomic_write(
            &glossary,
            b"# Glossary\n\nTerms appear here as you ingest sources.\n",
        )?;
    }

    let overview = wiki_dir.join("overview.md");
    if !overview.exists() {
        crate::atomic::atomic_write(
            &overview,
            b"# Overview\n\nHigh-level synthesis of your knowledge base.\n",
        )?;
    }

    let log = wiki_dir.join("log.md");
    if !log.exists() {
        crate::atomic::atomic_write(
            &log,
            b"# Wiki log\n\nAppend-only activity.\n\n",
        )?;
    }

    Ok(())
}

pub fn read_optional(path: &Path, max_chars: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) if s.len() <= max_chars => s,
        Ok(s) => s.chars().take(max_chars).collect::<String>() + "\n…\n",
        Err(_) => String::new(),
    }
}

pub fn schema_paths(schema_dir: &Path) -> (PathBuf, PathBuf) {
    (
        schema_dir.join("CLAUDE.md"),
        schema_dir.join("llm-wiki.md"),
    )
}

pub fn bundled_resources_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
}
