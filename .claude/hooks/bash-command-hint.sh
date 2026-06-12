#!/bin/sh
# PostToolUse hook (Bash): inspect the executed command text from stdin JSON
# and emit hints. Replaces the old inline hooks that relied on a non-existent
# $TOOL_INPUT env var (the documented interface is stdin JSON).

input=$(cat)

if command -v jq >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty')
else
  # Best-effort fallback; does not handle escaped quotes inside the command.
  cmd=$(printf '%s' "$input" | sed -n 's/.*"command"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/p' | head -n1)
fi

[ -n "$cmd" ] || exit 0

if printf '%s' "$cmd" | grep -qE '(git mv|mv|rename).*(crates/|apps/)'; then
  echo '[hook] Crate file moved — consider running /refresh-agents to update WHERE TO LOOK'
fi

if [ ! -f /tmp/claude-babysit-pr.lock ] && printf '%s' "$cmd" | grep -qE 'git push|gh pr create'; then
  branch=$(git -C "${CLAUDE_PROJECT_DIR:-.}" rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$branch" ] && [ "$branch" != "main" ] && [ "$branch" != "master" ]; then
    echo "[babysit-pr:auto] Push/PR-create detected on branch ${branch}. Auto-starting CI & review monitor (max 3 rounds). Invoke /babysit-pr now."
  fi
fi
