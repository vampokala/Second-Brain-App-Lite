#!/usr/bin/env bash

# Always run from the directory that contains this script (works from Finder, symlinks, etc.)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

# Prune stale Cargo/npm log-like files under src-tauri/target (see scripts/cleanup-old-target-logs.*).
bash "$SCRIPT_DIR/scripts/cleanup-old-target-logs.sh"

BROWSER_URL="http://localhost:1420"

# ── Helpers ──────────────────────────────────────────────────────────────────

open_browser() {
  sleep 2
  if command -v open &>/dev/null; then
    open "$BROWSER_URL"          # macOS
  elif command -v xdg-open &>/dev/null; then
    xdg-open "$BROWSER_URL"      # Linux
  fi
}

run_browser_mode() {
  echo ""
  echo "Launching in browser mode → $BROWSER_URL"
  echo "Press Ctrl+C to stop."
  echo ""
  open_browser &
  npm run dev
  exit $?
}

# After Tauri exits non-zero: only fall back to browser for likely runtime / firewall / WebView issues.
should_fallback_after_tauri() {
  local code=$1
  case $code in
    0) return 1 ;;
    101) return 1 ;;   # cargo / rustc compile error — fix code, do not mask with browser
    130|143) return 1 ;; # user interrupt
  esac
  return 0
}

# ── Dependencies (npm + optional Rust prefetch) ───────────────────────────────

if ! bash "$SCRIPT_DIR/scripts/install-deps.sh" --allow-partial-npm; then
  exit 1
fi

# ── Tauri desktop mode (requires Rust / cargo) ────────────────────────────────

if command -v cargo &>/dev/null; then
  echo "$(cargo --version) detected."
  echo "Starting Second Brain Lite (Tauri desktop mode)..."
  echo ""

  npm run tauri:dev
  TAURI_EXIT=$?

  if [ "$TAURI_EXIT" -eq 0 ]; then
    exit 0
  fi

  if ! should_fallback_after_tauri "$TAURI_EXIT"; then
    if [ "$TAURI_EXIT" -eq 101 ]; then
      echo ""
      echo "Rust build failed (exit 101). Fix the errors above."
    fi
    exit "$TAURI_EXIT"
  fi

  echo ""
  echo "WARNING: Tauri exited with code $TAURI_EXIT."
  echo "  Falling back to browser mode (often caused by firewall, WebView, or localhost)."
  run_browser_mode

else
  echo ""
  echo "Rust / cargo not found — Tauri desktop mode is unavailable."
  echo "  Install Rust at https://rustup.rs for the full desktop experience."
  echo ""
  echo "Starting browser mode..."
  run_browser_mode
fi
