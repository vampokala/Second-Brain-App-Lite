#!/usr/bin/env bash

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

# ── Node.js check ─────────────────────────────────────────────────────────────

if ! command -v node &>/dev/null; then
  echo "ERROR: Node.js is not installed."
  echo "  Download it from https://nodejs.org (LTS version recommended)."
  exit 1
fi
echo "Node $(node -v) detected."

# ── npm install ───────────────────────────────────────────────────────────────

if [ ! -d "node_modules" ] || [ "package-lock.json" -nt "node_modules/.package-lock.json" ]; then
  echo "Installing npm packages..."
  if ! npm install; then
    echo ""
    echo "ERROR: npm install failed."
    echo "  Check your network connection or proxy settings and try again."
    exit 1
  fi
else
  echo "Dependencies up to date, skipping npm install."
fi

# ── Tauri desktop mode (requires Rust / cargo) ────────────────────────────────

if command -v cargo &>/dev/null; then
  echo "$(cargo --version) detected."
  echo "Starting Second Brain Lite (Tauri desktop mode)..."
  echo ""

  npm run tauri:dev
  TAURI_EXIT=$?

  # Exit 0 = user closed the window normally — don't fall back
  if [ $TAURI_EXIT -eq 0 ]; then
    exit 0
  fi

  echo ""
  echo "WARNING: Tauri exited with code $TAURI_EXIT."
  echo "  This can happen due to a firewall blocking localhost, a missing"
  echo "  WebView runtime, or a network error while downloading Rust crates."
  echo ""
  echo "Falling back to browser mode..."
  run_browser_mode

else
  echo ""
  echo "Rust / cargo not found — Tauri desktop mode is unavailable."
  echo "  Install Rust at https://rustup.rs for the full desktop experience."
  echo ""
  echo "Falling back to browser mode..."
  run_browser_mode
fi
