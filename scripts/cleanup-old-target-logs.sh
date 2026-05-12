#!/usr/bin/env bash
# Prune build/debug log-like files older than N days under src-tauri/target and root npm-debug logs.
# Removes: *.log, Cargo build stderr, build-script output under debug|release/build, npm-debug.log*.
# Set RUN_WIKI_SKIP_TARGET_CLEANUP=1 to disable. Override age with RUN_WIKI_CLEAN_TARGET_DAYS (default 2).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "${RUN_WIKI_SKIP_TARGET_CLEANUP:-}" == "1" ]]; then
  exit 0
fi

days="${RUN_WIKI_CLEAN_TARGET_DAYS:-2}"
target_dir="$REPO_ROOT/src-tauri/target"
removed=0

if [[ -d "$target_dir" ]]; then
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    rm -f -- "$f" && removed=$((removed + 1))
  done < <(find "$target_dir" -type f \( -name '*.log' -o -name 'stderr' -o \( -name output \( -path '*/debug/build/*' -o -path '*/release/build/*' \) \) \) -mtime +"$days" 2>/dev/null)
fi

while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  rm -f -- "$f" && removed=$((removed + 1))
done < <(find "$REPO_ROOT" -maxdepth 1 -type f -name 'npm-debug.log*' -mtime +"$days" 2>/dev/null)

if [[ "$removed" -gt 0 ]]; then
  echo "Cleaned $removed stale log file(s) (older than ${days}d) under target/ and repo root."
fi
