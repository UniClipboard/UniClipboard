---
name: continue-task
description: "Resume work from a /wrap state file. Reads the structured active-task.json, presents a context summary, restores branch state, and lets you pick up exactly where the last session left off — no manual context rebuild."
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(grep:*), Bash(jq:*), Bash(cat:*), Bash(find:*), Read, Edit, AskUserQuestion
---

# continue-task

## Purpose

The counterpart to `/wrap`. When a previous session ended with `/wrap`, this skill reads the structured state file and rebuilds context in seconds instead of minutes.

**Before (observed pattern):**
```
User: "继续上次的工作"
Agent: [reads git log, guesses what was happening, asks 3 questions, reads 5 files] — 3-8 minutes
```

**After:**
```
User: /continue
Agent: [reads active-task.json + context.md, presents summary] — 10 seconds
  "上次你在 active-clipboard-state 分支做 PR8 (pull store-only LWW),
   已完成 PR1-7, 下一步是加双节点 e2e 测试。继续？"
```

## When to trigger

- `/continue` — resume from state file
- User says "继续上次的", "pick up where I left off", "resume the task"
- At the start of a new session when the user references prior work

## Workflow

### Step 1 — Find and read state file

```bash
PROJECT_KEY=$(pwd | sed 's|^/||; s|/|-|g')
STATE_DIR="$HOME/.claude/projects/-${PROJECT_KEY}"
STATE_FILE="${STATE_DIR}/active-task.json"
CONTEXT_FILE="${STATE_DIR}/active-task-context.md"
```

If the state file does not exist:
```
No active task found for this project.
Last /wrap state file not found at: <path>

Options:
  A) Check git log and branch state to reconstruct context
  B) Start fresh
```

If the state file exists, read both files.

### Step 2 — Validate state freshness

Check if the state is stale:

```bash
# Is the branch still current?
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
STATE_BRANCH=$(jq -r '.branch' "$STATE_FILE")

# Has the branch moved since state was written?
STATE_TIME=$(jq -r '.created_at' "$STATE_FILE")

# Is there a PR and what's its status?
PR_NUMBER=$(jq -r '.pr.number // empty' "$STATE_FILE")
if [ -n "$PR_NUMBER" ]; then
  gh pr view "$PR_NUMBER" --json state --jq '.state' 2>/dev/null
fi
```

Staleness conditions:
- State is more than 7 days old → warn, but still usable
- Branch was deleted or merged → state is stale, report and offer to clean up
- PR was closed/merged → task may be done, confirm with user
- Current branch differs from state branch → ask which to use

### Step 3 — Present context summary

Render a concise briefing:

```
📋 Active task: Implement ActiveClipboardState for cross-device LWW register
   Branch: active-clipboard-state
   PR: #1117 (open, 2 checks pending)
   Last wrapped: 2026-06-20 08:49

   ✅ Done:
     • PR1-PR6: core state machine, reconcile, sync
     • PR7: mobile cutover

   ➡️ Next:
     • PR8: pull store-only LWW starvation workaround
     • Add dual-node e2e test for PR8

   📁 Key context:
     • .planning/2026-06-19-issue-1017-active-clipboard.md
     • docs/architecture/active-clipboard-state.md
```

If there's an active debug state:

```
   🔍 Debug session in progress:
     Symptom: Windows restore → Mac sync delay ~3.4s
     
     Ruled out:
       ✗ #1017 logic bug (disproven: timing issue, not state bug)
       ✗ relay latency (disproven: direct conn established)
     
     Current hypothesis: iroh candidate address filtering not excluding Tailscale 100.x
     Next step: grep CIDR constants in node.rs, check filter function
```

### Step 4 — Offer actions

```
What would you like to do?
  A) Continue from where we left off (start with next steps)
  B) Review the changes made so far (git diff, PR status)
  C) Start a different task (archive this state)
  D) Read the full context document first
```

On choice A:
- If the branch differs from current, offer to switch: `git switch <state_branch>`
- Read the context references listed in the state file
- Begin working on the first item in `task.next`

On choice B:
```bash
git log main..HEAD --oneline
git diff main...HEAD --stat
gh pr view <number> --json statusCheckRollup,reviews 2>/dev/null
```

On choice C:
- Rename the state file to `archived-task-<date>.json`
- Start fresh

On choice D:
- Read and present the full `active-task-context.md`

### Step 5 — Load context references

For each file in `context_refs`, check if it exists and is relevant:

```bash
for ref in $(jq -r '.context_refs[]' "$STATE_FILE"); do
  if [ -f "$ref" ]; then
    echo "Reading: $ref"
    # Read the file to load context
  else
    echo "⚠️ Referenced file not found: $ref (may have been moved or deleted)"
  fi
done
```

### Step 6 — Clear state on completion

When the resumed task is completed (user confirms or `/wrap` is called again with a new task), the old state is naturally overwritten.

If the user explicitly says "this task is done" or "任务完成了":

```bash
# Archive the state
mv "$STATE_FILE" "${STATE_DIR}/archived-task-$(date +%Y%m%d-%H%M).json"
mv "$CONTEXT_FILE" "${STATE_DIR}/archived-task-$(date +%Y%m%d-%H%M)-context.md" 2>/dev/null
```

## Handling edge cases

### No state file, but user wants to continue

Fall back to git-based reconstruction:

```bash
git log --oneline -10
git status
git branch -v
gh pr list --author @me --json number,title,headRefName --jq '.[]' 2>/dev/null
```

Present what can be inferred and ask the user to fill in gaps.

### State file from a different branch

If the user switched branches between sessions:

```
⚠️ State file references branch 'active-clipboard-state',
   but you're currently on 'main'.

   A) Switch to active-clipboard-state and continue
   B) Stay on main, ignore the state file
   C) Show me what's in the state file first
```

### Multiple projects with state files

If the user works on multiple projects (uniclipboard, uc-website, uniclipboard-android), each has its own state file under its own project key. No conflict.

### Parallel branches in the same project (worktrees)

Each worktree has a different `pwd` and therefore a different project key. State files are naturally isolated.

## Interaction with other skills

| Skill | How it interacts |
|-------|-----------------|
| `/wrap` | Creates the state file that `/continue` reads |
| `/handoff` | Produces prose-only document; `/continue` cannot consume it (but the context.md from `/wrap` is similar) |
| `/resume` (built-in) | Resumes the Claude Code conversation itself; `/continue` restores task-level context which is a higher-level concept |
| `/pr-greenlight` | Can be the "next step" suggested by the state file |
| `/error-diagnose-fix` | Debug state section helps this skill avoid retrying ruled-out hypotheses |

## Anti-patterns

- Reading the state file but then ignoring it and asking the user "what are you working on?"
- Loading all context references even when the user only wants to do a quick commit
- Treating a stale state file (7+ days old) as current without warning
- Not checking if the branch still exists before suggesting to switch
- Spending 5 minutes "analyzing" the state when a 10-second summary suffices
