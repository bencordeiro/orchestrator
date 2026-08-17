#!/usr/bin/env bash
# One-command Linux release build for Orchestrator.
# Runs tests, downloads/verifies the CLIProxyAPI sidecar, builds UI + Tauri
# deb/AppImage, optional smoke launch of the packaged binary.
#
# Mirrors scripts/release.ps1. Keep behaviour in sync across the two.
#
# Usage (from anywhere):
#   scripts/release.sh
#   scripts/release.sh --skip-tests
#   scripts/release.sh --skip-smoke --skip-download
set -euo pipefail

skip_tests=0
skip_smoke=0
skip_download=0
for arg in "$@"; do
  case "$arg" in
    --skip-tests)    skip_tests=1 ;;
    --skip-smoke)    skip_smoke=1 ;;
    --skip-download) skip_download=1 ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cyan()   { printf '\033[36m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }

# rustup installs to ~/.cargo/bin, which is not on PATH for non-login shells.
export PATH="$HOME/.cargo/bin:$PATH"

need() { command -v "$1" >/dev/null 2>&1 || { red "Required command not found on PATH: $1"; exit 1; }; }
need cargo
need npm

cyan "=== Orchestrator release build (Linux) ==="
echo "Repo: $repo_root"

if [ "$skip_tests" -eq 0 ]; then
  echo; cyan "=== cargo test (core) ==="
  cargo test
  echo; cyan "=== cargo test (src-tauri) ==="
  ( cd src-tauri && cargo test )
else
  yellow "Skipping tests (--skip-tests)"
fi

if [ "$skip_download" -eq 0 ]; then
  echo; cyan "=== download + verify CLIProxyAPI sidecar ==="
  "$repo_root/scripts/download-cliproxy.sh"
else
  yellow "Skipping sidecar download (--skip-download)"
fi

# Resolve the exact externalBin name Tauri expects for this host triple, rather
# than hardcoding it — keeps aarch64 and cross-builds honest.
host_triple="$(rustc -vV | awk '/^host: /{print $2}')"
staged_sidecar="src-tauri/binaries/orchestrator-cli-proxy-api-${host_triple}"
if [ ! -f "$staged_sidecar" ]; then
  red "Missing sidecar for bundling: $staged_sidecar"
  red "Run scripts/download-cliproxy.sh"
  exit 1
fi
[ -x "$staged_sidecar" ] || chmod +x "$staged_sidecar"

echo; cyan "=== npm install + UI build ==="
(
  cd ui
  if [ -f package-lock.json ]; then
    npm ci || npm install
  else
    npm install
  fi
  npm run build
)

echo; cyan "=== Tauri bundle (deb + AppImage) ==="
# Load the updater signing key the same way release.ps1 does. A password-
# protected key with no password supplied makes the CLI prompt and hang an
# unattended build, so only load it when the password is present.
default_key="$HOME/.tauri/orchestrator/orchestrator.key"
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ] && [ -f "${TAURI_SIGNING_PRIVATE_KEY_PATH}" ]; then
    TAURI_SIGNING_PRIVATE_KEY="$(tr -d '\n' < "$TAURI_SIGNING_PRIVATE_KEY_PATH")"
    export TAURI_SIGNING_PRIVATE_KEY
  elif [ -f "$default_key" ] && [ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]; then
    TAURI_SIGNING_PRIVATE_KEY="$(tr -d '\n' < "$default_key")"
    export TAURI_SIGNING_PRIVATE_KEY
    echo "Loaded updater private key from $default_key"
  else
    yellow "Note: no updater private key - bundles still build; updater .sig may be skipped."
    yellow "See docs/UPDATER.md"
  fi
fi

# Only AppImage supports the Tauri updater on Linux; .deb never does.
# Without a signing key, disable updater artifacts outright — otherwise the CLI
# errors ("public key found, but no private key") *after* bundling.
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  npx --yes @tauri-apps/cli build --bundles deb,appimage
else
  yellow "No signing key - building without updater artifacts."
  npx --yes @tauri-apps/cli build --bundles deb,appimage \
    --config '{"bundle":{"createUpdaterArtifacts":false}}'
fi

bundle_dir="$repo_root/src-tauri/target/release/bundle"
app_bin="$repo_root/src-tauri/target/release/orchestrator-app"

echo; cyan "=== Bundle outputs ==="
if [ -d "$bundle_dir" ]; then
  find "$bundle_dir" -type f \( -name '*.deb' -o -name '*.AppImage' -o -name '*.sig' \) \
    -exec ls -lh {} + | awk '{print "  " $NF "  (" $5 ")"}'
else
  red "Bundle dir not found: $bundle_dir"
fi

if [ "$skip_smoke" -eq 0 ]; then
  echo; cyan "=== Smoke: launch release binary, hit /health ==="
  if [ ! -x "$app_bin" ]; then
    yellow "Release app binary missing at $app_bin - smoke skipped"
  elif ss -ltn 2>/dev/null | grep -q ':7420 '; then
    # NEVER kill the port owner: it is likely the user's live app, possibly
    # mid-delegation (in-memory background jobs die with it).
    yellow "Port 7420 already in use (running Orchestrator instance) - smoke skipped to avoid killing it"
  else
    "$app_bin" >/dev/null 2>&1 &
    smoke_pid=$!
    ok=0
    for _ in $(seq 1 30); do
      sleep 0.5
      if body="$(curl -fsS --max-time 2 http://127.0.0.1:7420/health 2>/dev/null)"; then
        echo "HEALTH OK: $body"
        ok=1
        break
      fi
      if ! kill -0 "$smoke_pid" 2>/dev/null; then
        red "Process exited early"
        break
      fi
    done
    if kill -0 "$smoke_pid" 2>/dev/null; then
      kill "$smoke_pid" 2>/dev/null || true
      wait "$smoke_pid" 2>/dev/null || true
    fi
    [ "$ok" -eq 1 ] || yellow "Smoke health check failed - verify manually if needed"
  fi
else
  yellow "Skipping smoke (--skip-smoke)"
fi

echo; green "=== Done ==="
deb="$(find "$bundle_dir/deb" -name '*.deb' -type f 2>/dev/null | head -n1 || true)"
appimage="$(find "$bundle_dir/appimage" -name '*.AppImage' -type f 2>/dev/null | head -n1 || true)"
[ -n "$deb" ]      && echo "deb package:    $deb"
[ -n "$appimage" ] && echo "AppImage:       $appimage"
echo "App binary:     $app_bin"
