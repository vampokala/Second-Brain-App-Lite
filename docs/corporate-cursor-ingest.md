# Corporate / Cursor-assisted ingest and local Cursor archive

This document describes workflows when **direct LLM provider APIs are blocked** but you may still use **models hosted through Cursor**, and when you want to reuse **past Cursor chats** stored on disk.

## Threat model and privacy

- **Lite does not call a model** during Cursor-assisted ingest: you copy a prompt pack into Cursor Chat yourself; only you decide what leaves the machine.
- **Local transcript reads** use normal desktop file APIs under your user account (same as any local app). Paths are under Cursor’s `workspaceStorage` (see below).
- **No `state.vscdb` parsing in MVP**: some Cursor builds may store threads only in SQLite; if list/preview is empty, use **Reveal in OS** and an external tool (e.g. your [llm-wiki](https://github.com/vampokala/llm-wiki) fork) — hybrid “Option C” from the product plan.

## Cursor-assisted ingest (paste loop)

1. Open **Ingest** and expand **Paste** (same content and **track** controls as normal paste ingest).
2. Turn on **Cursor-assisted ingest → Show workflow**.
3. Click **Save to raw & generate prompt pack**. This writes `raw/<track>/pastes/...md` and fills the **Prompt pack** text area.
4. Copy the prompt pack into **Cursor Chat** and run it against your sanctioned model.
5. Paste the model’s **single JSON object** into **Model response (JSON)**. The shape matches normal ingest: `slug`, `title`, `one_line_summary`, `body_markdown_b64` (preferred) or `body_markdown`, `tags`, optional `glossary_patch`.
6. Click **Validate JSON**, review the preview, then **Commit to wiki**.  
   - `wiki/log.md` records `Ingest mode: cursor-assisted` for these commits.  
   - Automated **Run full ingest** uses `Ingest mode: provider-ingest`.

## Import Cursor chat archive

1. Expand **Import Cursor chat archive**.
2. Optional filters: **Hash contains**, **Max age (days)**, **Vault path contains** (matched against `workspace.json` `folder` when present).
3. **Discover workspaces** → pick a **Workspace** → **List .jsonl transcripts**.
4. Select a file, **Preview excerpt**, optionally **Reveal in OS** or **Copy path** for external tools.

### Destinations (multi-select)

| Option | Effect |
|--------|--------|
| **Append excerpt to Cursor-assisted prompt pack** | Appends a markdown block to the prompt pack textarea (open Cursor-assisted to copy). Does not write `wiki/` by itself. |
| **Stage wiki page** | Saves excerpt markdown to `raw/`, builds stub ingest JSON, runs the **same commit path** as Cursor-assisted (`Ingest mode: cursor-archive` in the log). |
| **Import as Lite chat session** | Creates a new app chat session under app data (`sessions/*.json`) with one message per parsed line. |

## Transcript format (S0.1 / fixtures)

The MVP parser accepts **NDJSON**: one JSON object per line, with either:

- `role` + `content` (string), or  
- `content` as an array of `{ "text": "..." }` parts (OpenAI-style), or  
- A small set of `{ "type": "...", "message": { "content": "..." } }` style envelopes.

A **redacted minimal fixture** lives at `tests/fixtures/cursor/minimal.jsonl` in this repo. Cursor’s on-disk format can change between releases; if preview is empty, inspect a real file and extend the parser (or contribute a fixture).

## Upstream llm-wiki fork

If you maintain [vampokala/llm-wiki](https://github.com/vampokala/llm-wiki), apply the fixture/doc reconciliation described in [vendor-notes/llm-wiki-fork-cursor-fixture.md](../vendor-notes/llm-wiki-fork-cursor-fixture.md).

## Related files (code)

| Area | Path |
|------|------|
| Prompt pack + commit | `src-tauri/src/ingest.rs` (`build_cursor_assist_prompt_pack`, `commit_parsed_ingest_to_wiki`, `preview_ingest_commit`) |
| Archive discovery / parse | `src-tauri/src/cursor_archive.rs` |
| Tauri commands | `src-tauri/src/lib.rs` |
| UI | `src/views/Ingest/index.tsx` |
