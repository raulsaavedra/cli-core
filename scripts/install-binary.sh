#!/usr/bin/env bash
set -euo pipefail

install_binary() {
  local src="$1"
  local dest="$2"

  mkdir -p "$(dirname "$dest")"
  cp "$src" "$dest"
  chmod 0755 "$dest"

  if command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$dest" 2>/dev/null || true
  fi

  if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$dest" >/dev/null 2>&1 || true
  fi
}
