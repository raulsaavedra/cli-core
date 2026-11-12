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
signing_identity() {
  if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    echo "$CODESIGN_IDENTITY"
    return
  fi
  security find-identity -v -p codesigning 2>/dev/null | awk '/"Apple Development: / { print $2; exit }'
}

sign_binary() {
  local path="$1"
  local identifier="$2"
  local identity

  if [ -z "$identifier" ]; then
    codesign --force --sign - "$path" >/dev/null 2>&1 || true
    return
  fi
  identity="$(signing_identity)"
  if [ -n "$identity" ] && codesign --force --sign "$identity" --identifier "$identifier" "$path" >/dev/null 2>&1; then
    return
  fi
  codesign --force --sign - --identifier "$identifier" "$path" >/dev/null 2>&1 || true
  echo "Signed ${identifier} ad hoc: no Apple Development identity could sign it, so macOS asks for folder access again after each install" >&2
}

install_binary() {
  local src="$1"
  local dest="$2"
  local identifier="${3:-}"
  local staged

  mkdir -p "$(dirname "$dest")"
  staged="$(mktemp "${dest}.XXXXXX")"
  cp "$src" "$staged"
  chmod 0755 "$staged"

  if command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$staged" 2>/dev/null || true
  fi

  if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
    sign_binary "$staged" "$identifier"
  fi

  mv -f "$staged" "$dest"
}
