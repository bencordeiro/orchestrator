#!/usr/bin/env bash
# Download, verify, and stage the pinned CLIProxyAPI sidecar for Linux/macOS.
#
# Mirrors scripts/download-cliproxy.ps1. Pin + checksums live in
# src-tauri/binaries/VERSION.txt — this script never picks a version itself.
#
# Stages two names, matching the Windows script:
#   orchestrator-cli-proxy-api            plain name, used by the dev-layout lookup
#   orchestrator-cli-proxy-api-<triple>   the name Tauri's externalBin requires
#
# The staged name is namespaced because the .deb installs externalBin into
# /usr/bin, where a bare `cli-proxy-api` would collide with a user's own
# CLIProxyAPI. The name inside the upstream archive is still `cli-proxy-api`
# (VERSION.txt `<platform>_binary`); this script renames it while staging.
#
# Usage:
#   scripts/download-cliproxy.sh            # auto-detect platform
#   scripts/download-cliproxy.sh linux_amd64
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="$repo_root/src-tauri/binaries"
version_file="$bin_dir/VERSION.txt"

[ -f "$version_file" ] && : || { echo "Missing VERSION.txt at $version_file" >&2; exit 1; }

# Read a key from VERSION.txt. Tolerates CRLF (the file has lived on Windows)
# and a stray BOM, so a checkout with mangled line endings still builds.
read_pin() {
  sed -e 's/\r$//' -e '1s/^\xEF\xBB\xBF//' "$version_file" \
    | grep -E "^$1=" | head -n1 | cut -d= -f2-
}

detect_platform() {
  local os arch
  case "$(uname -s)" in
    Linux)  os=linux ;;
    Darwin) os=darwin ;;
    *) echo "Unsupported OS '$(uname -s)' — use the PowerShell script on Windows" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64)  arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
  esac
  echo "${os}_${arch}"
}

platform="${1:-$(detect_platform)}"

url="$(read_pin "${platform}_url")"
expected="$(read_pin "${platform}_sha256")"
binary_name="$(read_pin "${platform}_binary")"
triple="$(read_pin "${platform}_triple")"
version="$(read_pin version)"

if [ -z "$url" ] || [ -z "$expected" ] || [ -z "$binary_name" ] || [ -z "$triple" ]; then
  echo "VERSION.txt has no complete pin for platform '$platform'." >&2
  echo "Expected keys: ${platform}_url, ${platform}_sha256, ${platform}_binary, ${platform}_triple" >&2
  exit 1
fi
expected="$(echo "$expected" | tr '[:upper:]' '[:lower:]')"

mkdir -p "$bin_dir"
archive="$bin_dir/$(basename "$url")"

sha_of() { sha256sum "$1" | cut -d' ' -f1; }

# Reuse a previously downloaded archive only if it still matches the pin.
if [ -f "$archive" ]; then
  if [ "$(sha_of "$archive")" = "$expected" ]; then
    echo "Reusing verified archive: $archive"
  else
    echo "Cached archive checksum mismatch; re-downloading..."
    rm -f "$archive"
  fi
fi

if [ ! -f "$archive" ]; then
  echo "Downloading $url"
  curl -sSL --fail --max-time 600 -o "$archive" "$url"
fi

actual="$(sha_of "$archive")"
if [ "$actual" != "$expected" ]; then
  echo "SHA256 mismatch for $archive" >&2
  echo "  got:      $actual" >&2
  echo "  expected: $expected" >&2
  # A bad download must never be left where a later run could trust it.
  rm -f "$archive"
  exit 1
fi
echo "Checksum OK: $actual"

extract_dir="$bin_dir/extract"
rm -rf "$extract_dir"
mkdir -p "$extract_dir"
tar -xzf "$archive" -C "$extract_dir"

# The binary may sit at the archive root or one directory down.
src_bin="$(find "$extract_dir" -type f -name "$binary_name" -print -quit)"
if [ -z "$src_bin" ]; then
  echo "'$binary_name' not found inside $archive" >&2
  exit 1
fi

# Namespaced staging name (see header). Keep in sync with SIDECAR_BIN in
# src-tauri/src/sidecar/config.rs and externalBin in tauri.conf.json.
staged_name="orchestrator-cli-proxy-api"
dest_plain="$bin_dir/$staged_name"
dest_triple="$bin_dir/${staged_name}-${triple}"

install -m 0755 "$src_bin" "$dest_plain"
install -m 0755 "$src_bin" "$dest_triple"
rm -rf "$extract_dir"

echo "Installed: $dest_plain"
echo "Staged for Tauri externalBin: $dest_triple"
echo "Pin: CLIProxyAPI v$version ($platform) — see $version_file"
