#!/usr/bin/env bash
set -euo pipefail

# Publish a repository's own skills into the shared agent skills directory.
install_cli_skills() {
  local root="$1"
  local skills_dir="${AGENTS_SKILLS_DIR:-${HOME}/.agents/skills}"
  local skill

  for skill in "$root"/skills/*/; do
    [ -f "${skill}SKILL.md" ] || continue
    mkdir -p "$skills_dir"
    ln -sfn "${skill%/}" "${skills_dir}/$(basename "$skill")"
    echo "Installed skill ${skills_dir}/$(basename "$skill")"
  done
}

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
