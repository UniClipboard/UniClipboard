#!/bin/sh
# PostToolUse hook (Edit|Write): on the first edit of a Rust / frontend file in a
# session, remind the agent to read the matching docs/agent/*.md rules file.
# Reminds at most once per session per area, via a flag file keyed by session_id.

input=$(cat)

if command -v jq >/dev/null 2>&1; then
  file_path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')
  session_id=$(printf '%s' "$input" | jq -r '.session_id // empty')
else
  file_path=$(printf '%s' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
  session_id=$(printf '%s' "$input" | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
fi

[ -n "$file_path" ] || exit 0
[ -n "$session_id" ] || session_id="nosession"

project_dir=${CLAUDE_PROJECT_DIR:-$(pwd)}
rel=${file_path#"$project_dir"/}

# Note: in `case`, * also matches `/`, so these cover nested paths.
case "$rel" in
  crates/*.rs | apps/*.rs | src-tauri/*.rs)
    kind=rust
    msg='[hook] First Rust edit this session — read docs/agent/rust-tauri-rules.md if you have not (plus docs/agent/architecture-rules.md when crate boundaries, ports, or DTO conversions are involved).'
    ;;
  src/*.ts | src/*.tsx | src/*.css)
    kind=frontend
    msg='[hook] First frontend edit this session — read docs/agent/frontend-ui-rules.md if you have not.'
    ;;
  *)
    exit 0
    ;;
esac

flag="${TMPDIR:-/tmp}/claude-guide-hint-${session_id}-${kind}"
[ -f "$flag" ] && exit 0
touch "$flag" 2>/dev/null
echo "$msg"
