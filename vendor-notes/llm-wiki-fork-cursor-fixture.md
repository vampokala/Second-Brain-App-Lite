# Upstream reconciliation: `vampokala/llm-wiki` Cursor fixture

The upstream doc [docs/adapters/cursor.md](https://github.com/vampokala/llm-wiki/blob/master/docs/adapters/cursor.md) references `tests/fixtures/cursor/minimal.jsonl`, but that path was missing on master.

**Apply to the fork** (copy-paste):

1. Add directory `tests/fixtures/cursor/`.
2. Copy `tests/fixtures/cursor/minimal.jsonl` from **Second-Brain-Lite** into `llm-wiki` at the same relative path, **or** update `docs/adapters/cursor.md` to remove the broken path and describe the NDJSON contract inline.

The fixture uses one JSON object per line with `role` and `content` (string), which Second-Brain-Lite’s `cursor_archive` parser treats as the MVP contract.
