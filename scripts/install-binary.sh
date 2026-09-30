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

# Install a binary by renaming a prepared copy over the destination, so processes still
# running the previous binary keep a valid code signature.
install_binary() {
  local src="$1"
  local dest="$2"
  local staged

  mkdir -p "$(dirname "$dest")"
  staged="$(mktemp "${dest}.XXXXXX")"
  cp "$src" "$staged"
  chmod 0755 "$staged"

  if command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$staged" 2>/dev/null || true
  fi

  if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$staged" >/dev/null 2>&1 || true
  fi

  mv -f "$staged" "$dest"
}
