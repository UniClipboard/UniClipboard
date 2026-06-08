---
name: babysit-pr
description: Automatically monitor CI checks and review comments (coderabbitai, github-actions) on the current PR. Analyze findings, fix issues, push, and loop until all checks pass or max rounds reached.
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(grep:*), Bash(rg:*), Bash(jq:*), Bash(cat:*), Bash(rm:*), Bash(date:*), Read, Edit, Write, ScheduleWakeup, Agent
---

# babysit-pr

## Purpose

After a push or PR creation, CI checks and automated reviewers (CodeRabbit, GitHub Actions) produce status checks and review comments. This skill monitors those results, analyzes review comments and check failures, fixes legitimate issues, pushes, and loops — up to **3 rounds**.

## State file

`/tmp/claude-babysit-pr-state.json` tracks progress across wakeup cycles:

```json
{
  "pr_number": 123,
  "round": 0,
  "max_rounds": 3,
  "processed_comment_ids": [],
  "started_at": "2026-06-08T10:00:00Z"
}
```

- `round` increments each time you **push a fix**. Checking status without pushing does NOT count as a round.
- Lock file: `/tmp/claude-babysit-pr.lock` — prevents the PostToolUse hook from re-triggering while this skill is active.

## Workflow

### Step 0 — Initialize or resume

```bash
touch /tmp/claude-babysit-pr.lock
```

Check if state file exists:
- **No state file**: Create one. Get PR number via `gh pr view --json number --jq .number`. Set round=0, started_at=now, processed_comment_ids=[].
- **State file exists**: Read it. If round >= max_rounds → go to **Cleanup (max rounds)**. If started_at is more than 90 minutes ago → go to **Cleanup (timeout)**.

If no PR exists for the current branch, output "No PR found for current branch" and clean up.

### Step 1 — Check CI status

```bash
gh pr checks --json name,state,conclusion,link 2>/dev/null
```

Also fetch the overall PR status:
```bash
gh pr view --json statusCheckRollup --jq '.statusCheckRollup[] | {context: .context, state: .state, description: .description, targetUrl: .targetUrl}'
```

Classify:
- **All checks passed (and no new unprocessed review comments)** → go to **Cleanup (success)**
- **Some checks still PENDING** → schedule wakeup (180s), do NOT increment round
- **Some checks FAILED** → proceed to Step 2

### Step 2 — Gather failures and review comments

#### 2a — Failed checks

From the checks output in Step 1, collect all checks with `conclusion == "failure"` or `state == "FAILURE"`. For each failed check:
- Note the check name, failure link/URL
- If the failure link points to a GitHub Actions run, fetch the log:
  ```bash
  gh run view <run_id> --log-failed 2>/dev/null | tail -100
  ```

#### 2b — Review comments from coderabbitai and github-actions

```bash
# Inline review comments
gh api repos/UniClipboard/UniClipboard/pulls/<PR_NUMBER>/comments \
  --jq '[.[] | select(.user.login == "coderabbitai" or .user.login == "github-actions[bot]" or .user.login == "github-actions") | {id, user: .user.login, body, path, line, diff_hunk, created_at}]'

# Top-level review bodies
gh api repos/UniClipboard/UniClipboard/pulls/<PR_NUMBER>/reviews \
  --jq '[.[] | select(.user.login == "coderabbitai" or .user.login == "github-actions[bot]" or .user.login == "github-actions") | {id, user: .user.login, body, state}]'
```

Filter out IDs already in `processed_comment_ids`.

### Step 3 — Analyze and fix

For each issue (failed check or new review comment):

1. **Understand the problem**: Read the relevant file(s) and context.
2. **Evaluate legitimacy**:
   - CI check failures: always legitimate — must fix.
   - CodeRabbit comments: evaluate whether the suggestion improves correctness, security, or performance. **Skip pure style nits, subjective preferences, and false positives.** When skipping, note the reason briefly.
   - GitHub Actions bot comments: usually legitimate (lint errors, type errors, test failures) — fix them.
3. **Fix**: Edit the relevant files. Be surgical — only change what's needed.
4. **Record**: Add the comment/check ID to `processed_comment_ids`.

If the fix touches Rust code, run a quick validation:
```bash
cargo check -p <affected_crate> 2>&1 | tail -20
```

If the fix touches TypeScript/frontend code:
```bash
cd src-tauri && pnpm type-check 2>&1 | tail -20
```

### Step 4 — Commit and push

Only if fixes were made:

```bash
git add <specific fixed files>
git commit -m "$(cat <<'EOF'
fix: address CI/review feedback (babysit-pr round N)
EOF
)"
git push
```

Replace N with the actual round number. Increment `round` in the state file.

**If no fixes were needed** (all comments were false positives / already processed, but checks are still failing for unknown reasons):
- Do NOT push an empty commit.
- Log the situation and schedule one more wakeup. If the next check also shows nothing fixable, stop and report to the user.

### Step 5 — Schedule next check or finish

After pushing:
```
ScheduleWakeup(delaySeconds: 180, reason: "waiting for CI after babysit-pr round N", prompt: "/babysit-pr")
```

Use 180s to stay within the prompt cache window while giving CI time to start.

If no push was made but checks are still pending:
```
ScheduleWakeup(delaySeconds: 270, reason: "CI still pending, no fixes needed yet", prompt: "/babysit-pr")
```

### Cleanup

**On success (all checks pass)**:
```bash
rm -f /tmp/claude-babysit-pr.lock /tmp/claude-babysit-pr-state.json
```
Output: "All CI checks passed. babysit-pr complete after N round(s)."

**On max rounds reached**:
```bash
rm -f /tmp/claude-babysit-pr.lock /tmp/claude-babysit-pr-state.json
```
Output: "Reached max rounds (3). Some checks may still be failing — manual review needed. PR: <url>"

**On timeout (90 min)**:
```bash
rm -f /tmp/claude-babysit-pr.lock /tmp/claude-babysit-pr-state.json
```
Output: "babysit-pr timed out after 90 minutes. Check PR status manually."

**On no PR found**:
```bash
rm -f /tmp/claude-babysit-pr.lock /tmp/claude-babysit-pr-state.json
```

## Safety guardrails

- Max **3 rounds** of fix-push cycles (round only increments on push)
- **90-minute hard timeout** from first invocation
- **Lock file** prevents recursive hook triggering
- **Comment dedup** via processed_comment_ids
- Never `--force` push
- Never amend existing commits
- Skip CodeRabbit style nits — only fix correctness/security/CI issues
- If `cargo check` or type-check fails after your fix, revert the fix and note it — don't push broken code
- Each commit only includes files directly related to the fix

## Skipped comment handling

When you decide to skip a CodeRabbit comment (false positive, style nit, etc.), briefly note:
```
Skipped: [comment summary] — reason: [false positive / style nit / not applicable]
```
This helps the user understand what was deliberately not addressed.

## Anti-patterns

- Blindly applying every CodeRabbit suggestion without evaluating it
- Pushing empty commits or "no-op" changes
- Running indefinitely when CI is stuck on an infrastructure issue (not a code problem)
- Modifying files unrelated to the review feedback
- Ignoring CI failures and only processing review comments (or vice versa)
