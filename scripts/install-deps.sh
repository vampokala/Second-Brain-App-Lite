#!/usr/bin/env bash
# Install JavaScript (npm) and optionally prefetch Rust crates for Second Brain Lite.
#
# Usage:
#   ./scripts/install-deps.sh              # npm install + cargo fetch (if cargo exists)
#   ./scripts/install-deps.sh --npm-only   # only npm
#   ./scripts/install-deps.sh --rust-only  # only cargo fetch (requires cargo)
#   ./scripts/install-deps.sh --allow-partial-npm
#       If npm fails after retries but node_modules exists, exit 0 (for run-wiki).
#
# Env: SB_LITE_NPM_RETRIES (default 3)
#
# Python is not required for this project (no Python tooling in the repo).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT" || exit 1

DO_NPM=1
DO_RUST=1
ALLOW_PARTIAL_NPM=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --npm-only) DO_NPM=1; DO_RUST=0; shift ;;
    --rust-only) DO_NPM=0; DO_RUST=1; shift ;;
    --allow-partial-npm) ALLOW_PARTIAL_NPM=1; shift ;;
    -h|--help)
      cat <<'EOF'
Usage: scripts/install-deps.sh [options]

  (default)           npm install + cargo fetch (if cargo is on PATH)
  --npm-only          Only npm install
  --rust-only         Only cargo fetch (needs cargo and src-tauri/Cargo.toml)
  --allow-partial-npm If npm fails after retries but node_modules exists, exit 0

Environment:
  SB_LITE_NPM_RETRIES   Retry count for npm (default: 3)

Note: Python is not required (Node.js + Rust/Tauri only).
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      echo "Use --npm-only, --rust-only, or --allow-partial-npm" >&2
      exit 1
      ;;
  esac
done

NPM_RETRIES="${SB_LITE_NPM_RETRIES:-3}"

npm_install_with_retries() {
  local attempt=1
  while [[ "$attempt" -le "$NPM_RETRIES" ]]; do
    echo "Installing npm packages (attempt $attempt/$NPM_RETRIES)..."
    if npm install --no-fund --no-audit; then
      return 0
    fi
    if [[ "$attempt" -lt "$NPM_RETRIES" ]]; then
      echo "  Install failed — retrying in 3s..."
      sleep 3
    fi
    attempt=$((attempt + 1))
  done
  return 1
}

prefetch_rust_libs() {
  if [[ ! -f "src-tauri/Cargo.toml" ]]; then
    return 0
  fi
  if ! command -v cargo &>/dev/null; then
    echo "Skipping Rust prefetch: cargo not on PATH."
    return 0
  fi
  echo "Prefetching Rust crates (cargo fetch)..."
  if ! cargo fetch --manifest-path "src-tauri/Cargo.toml"; then
    echo "WARNING: cargo fetch failed (network, firewall, or proxy)." >&2
    echo "  Run this script again or let the next Tauri build download crates." >&2
    return 1
  fi
  return 0
}

if [[ "$DO_NPM" -eq 1 ]]; then
  if ! command -v node &>/dev/null; then
    echo "ERROR: Node.js is not installed." >&2
    echo "  https://nodejs.org (LTS recommended)" >&2
    exit 1
  fi
  if ! command -v npm &>/dev/null; then
    echo "ERROR: npm is not available." >&2
    exit 1
  fi
  echo "Node $(node -v) detected."

  if ! npm_install_with_retries; then
    echo "" >&2
    echo "ERROR: npm install failed after $NPM_RETRIES attempts." >&2
    if [[ -d "node_modules" && "$ALLOW_PARTIAL_NPM" -eq 1 ]]; then
      echo "  Continuing with existing node_modules (--allow-partial-npm)." >&2
    elif [[ -d "node_modules" ]]; then
      echo "  node_modules exists but install failed. Re-run with network access or use --allow-partial-npm from run-wiki." >&2
      exit 1
    else
      echo "  Check network, proxy, or firewall (registry.npmjs.org)." >&2
      exit 1
    fi
  fi
fi

if [[ "$DO_RUST" -eq 1 ]]; then
  prefetch_rust_libs || true
fi

echo "Dependency install step finished."
