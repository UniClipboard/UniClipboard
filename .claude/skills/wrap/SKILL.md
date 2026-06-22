---
name: wrap
description: "Session wrap-up: commit pending changes, push, write structured state for the next session to resume from. Eliminates 'glue sessions' that exist only to commit/push, and preserves debug context across sessions."
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(grep:*), Bash(jq:*), Bash(cat:*), Bash(rm:*), Bash(date:*), Bash(wc:*), Read, Edit, Write, AskUserQuestion
---

# wrap

## Purpose

A session-ending skill that wraps up the current work into a clean state for the next session. It replaces the pattern of opening a new "glue session" just to commit and push, and it replaces ad-hoc `/handoff` prose with a structured, machine-readable state file that `/continue` can consume.

**Before (observed in 29 of 50 recent sessions):**
```
Session N: [does the work] → ends without committing
Session N+1: "commit and push" → 1 prompt, done   ← wasted session
Session N+2: "继续" → spends 3-8 min rebuilding context
```

**After:**
```
Session N: [does the work] → /wrap → committed, pushed, state saved
Session N+1: /continue → full context in 10 seconds
```

## When to trigger

- `/wrap` — full wrap-up: commit + push + state file
- `/wrap --no-push` — commit + state file, skip push
- `/wrap --no-commit` — state file only (for when changes aren't ready to commit)
- User says "收工", "wrap up", "结束今天的工作", "保存进度"
- End of a long session where context will be lost

## State file location

```bash
PROJECT_KEY=$(pwd | sed 's|^/||; s|/|-|g')
STATE_FILE="$HOME/.claude/projects/-${PROJECT_KEY}/active-task.json"
```

This places the state file alongside Claude Code's own session data for the project. It is NOT tracked in git.

## Workflow

### Step 1 — Take stock (parallel)

```bash
git status --short
git rev-parse --abbrev-ref HEAD
git log --oneline -5
git diff --stat                        # unstaged changes
git diff --cached --stat               # staged changes
gh pr list --head $(git rev-parse --abbrev-ref HEAD) --json number,url,title --jq '.[0]' 2>/dev/null
```

### Step 2 — Commit pending changes

Skip if `--no-commit` or if working tree is clean.

If there are changes:

1. Show the user what will be committed:
   ```
   Uncommitted changes:
     M  crates/uc-core/src/types.rs
     M  crates/uc-daemon/src/api.rs
     A  crates/uc-core/src/new_module.rs
   ```

2. Ask the user:
   ```
   Commit these changes?
   A) Yes, commit all
   B) Let me pick which files
   C) Skip commit (just save state)
   ```

3. On commit, draft a message from the diff context:
   ```bash
   git add <files>
   git commit -m "$(cat <<'EOF'
   <type>(<scope>): <message>
   EOF
   )"
   ```

### Step 3 — Push

Skip if `--no-push` or if no remote tracking branch.

```bash
git push
```

If a PR exists for this branch, note the PR URL in the state file.

### Step 4 — Write state file

Build a structured state file. The state file has two parts:
- **JSON** (`active-task.json`) — machine-readable, consumed by `/continue`
- **Markdown** (`active-task-context.md`) — human-readable handoff detail

#### 4a — active-task.json

```json
{
  "version": 1,
  "created_at": "2026-06-21T14:30:00Z",
  "project": "/Users/mark/MyProjects/uniclipboard",
  "branch": "active-clipboard-state",
  "pr": {
    "number": 1117,
    "url": "https://github.com/UniClipboard/UniClipboard/pulls/1117"
  },
  "task": {
    "summary": "Implement ActiveClipboardState for cross-device LWW register",
    "status": "in_progress",
    "done": [
      "PR1-PR6: core state machine, reconcile, sync",
      "PR7: mobile cutover"
    ],
    "next": [
      "PR8: pull store-only LWW starvation workaround",
      "Add dual-node e2e test for PR8"
    ],
    "blocked_on": null
  },
  "debug": {
    "active": false,
    "symptom": null,
    "hypotheses_tried": [],
    "hypotheses_ruled_out": [],
    "evidence": [],
    "current_hypothesis": null
  },
  "pending_actions": {
    "needs_push": false,
    "needs_pr": false,
    "needs_rebase": false,
    "uncommitted_files": []
  },
  "context_refs": [
    ".planning/2026-06-19-issue-1017-active-clipboard.md",
    "docs/architecture/active-clipboard-state.md"
  ]
}
```

#### 4b — active-task-context.md

This replaces the freeform `/handoff` document. Written to the same directory.

```markdown
# Active Task: <summary>

**Branch:** `<branch>`
**PR:** #<number> (<url>)
**Last updated:** <timestamp>

## What's done
<bulleted list from task.done>

## What's next
<bulleted list from task.next>

## Key decisions made
<non-obvious decisions from this session that the next agent needs to know>

## Debug state (if active)
### Symptom
<what's broken>

### Hypotheses tried
| # | Hypothesis | Result | Evidence |
|---|-----------|--------|----------|
| 1 | ... | Ruled out | ... |
| 2 | ... | Partially confirmed | ... |

### Current hypothesis
<what to test next>

## Files changed this session
<git diff --stat output>

## Suggested skills
<relevant skills for the next session>
```

### Step 5 — Report

```
✅ Session wrapped.
  Branch: active-clipboard-state
  Committed: 3 files (feat(core): add ActiveClipboardState reconcile)
  Pushed: ✓
  PR: #1117
  State saved: ~/.claude/projects/-Users-mark-.../active-task.json
  
  Next session: /continue to pick up where you left off.
```

## Populating the state file

### Task information

Derive from the conversation context:
- `summary`: One line describing the overall task (not just the last thing done)
- `done`: What was accomplished in this session AND prior sessions (cumulative)
- `next`: Specific next steps, not vague ("implement X" not "continue working")
- `blocked_on`: If waiting on something external (CI, user decision, another PR)

### Debug state

Only populate if the session involved debugging:
- `hypotheses_tried`: Each hypothesis with its result and evidence
- `hypotheses_ruled_out`: Explicitly mark what was disproven (prevents next session from retrying)
- `evidence`: Key observations (log snippets, test results, measurements)
- `current_hypothesis`: What to test next

### Context references

List files that the next session should read for context:
- Planning docs in `.planning/`
- Architecture docs in `docs/`
- Relevant issue URLs
- Do NOT duplicate content from these files — just reference them

## Interaction with existing /handoff

- `/wrap` supersedes `/handoff` for the common case (end of session)
- `/handoff` remains available for when you want a standalone document without committing/pushing
- If `/wrap` is called, it creates both the structured state AND the markdown context (no need for separate `/handoff`)

## Cleanup

The state file is overwritten by each `/wrap` call. `/continue` clears it after successful resume. Old state files are not cleaned up automatically — they're small (< 5KB) and serve as history.

## Safety guardrails

- Never force-push
- Never amend already-pushed commits
- Ask before committing if there are unstaged changes (user might not want all of them)
- Redact passwords, API keys, and PII from the state file and context document
- Do NOT write the state file into the git-tracked repo
- Do NOT commit the state file
- If the working tree has merge conflicts, warn and skip commit

## Anti-patterns

- Writing a 500-line handoff document that duplicates the planning doc
- Using vague next steps ("continue the work" — useless for /continue)
- Including raw log output in the context file (summarize, don't paste)
- Forgetting to populate debug state when the session was a debugging session
- Overwriting state without checking if there's important state from a parallel session on a different branch
