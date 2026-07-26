#!/bin/sh
# PostToolUse hook: inspect an executed shell command and emit project hints.

input=$(cat)
command -v jq >/dev/null 2>&1 || exit 0

cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // .tool_input.cmd // empty')
cwd=$(printf '%s' "$input" | jq -r '.cwd // empty')
[ -n "$cmd" ] || exit 0
[ -n "$cwd" ] || cwd=$(pwd)

emit_hint() {
  jq -n --arg message "$1" '{systemMessage: $message}'
}

if printf '%s' "$cmd" | grep -qE '(git mv|mv|rename).*(crates/|apps/)'; then
  emit_hint '[hook] A crate file moved. Consider using $refresh-agents to update repository navigation.'
  exit 0
fi

if [ ! -f /tmp/codex-babysit-pr.lock ] && printf '%s' "$cmd" | grep -qE 'git push|gh pr create'; then
  branch=$(git -C "$cwd" rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$branch" ] && [ "$branch" != "main" ] && [ "$branch" != "master" ]; then
    emit_hint "[babysit-pr:auto] Push or PR creation detected on branch ${branch}. Use \$babysit-pr to monitor CI and reviews."
  fi
fi
