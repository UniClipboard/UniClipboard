#!/bin/sh
# PostToolUse hook: remind Codex about area-specific guidance after the first edit.

input=$(cat)
command -v jq >/dev/null 2>&1 || exit 0

payload=$(printf '%s' "$input" | jq -r '
  .tool_input
  | if type == "object" then (.file_path // .patch // .input // "")
    elif type == "string" then .
    else ""
    end
')
session_id=$(printf '%s' "$input" | jq -r '.session_id // "nosession"')
[ -n "$payload" ] || exit 0

kind=
message=
if printf '%s\n' "$payload" | grep -qE '(^|[/ :])(crates|apps|src-tauri)/[^[:space:]]+\.rs([[:space:]]|$)'; then
  kind=rust
  message='[hook] First Rust edit this session. Read docs/agent/rust-tauri-rules.md if needed, plus docs/agent/architecture-rules.md for boundary changes.'
elif printf '%s\n' "$payload" | grep -qE '(^|[/ :])src/[^[:space:]]+\.(ts|tsx|css)([[:space:]]|$)'; then
  kind=frontend
  message='[hook] First frontend edit this session. Read docs/agent/frontend-ui-rules.md if needed.'
else
  exit 0
fi

flag="${TMPDIR:-/tmp}/codex-guide-hint-${session_id}-${kind}"
[ -f "$flag" ] && exit 0
touch "$flag" 2>/dev/null || exit 0
jq -n --arg message "$message" '{systemMessage: $message}'
