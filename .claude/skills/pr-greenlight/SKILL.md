---
name: pr-greenlight
description: "Agent Loop that runs local pre-flight CI checks, auto-fixes issues, creates/pushes the PR, monitors CI, and loops until all checks pass. Replaces the manual chain of preflight → create-pr → babysit-pr."
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(cargo:*), Bash(bun:*), Bash(grep:*), Bash(rg:*), Bash(jq:*), Bash(diff:*), Bash(cat:*), Bash(rm:*), Bash(date:*), Bash(wc:*), Bash(find:*), Bash(node:*), Read, Edit, Write, AskUserQuestion, ScheduleWakeup, Agent
---

# pr-greenlight

## Purpose

A single Agent Loop that takes uncommitted or committed work on a feature branch all the way to a green PR. It replaces the manual chain of:

1. (forgotten) local checks → push → CI fails → new session to fix → repeat
2. `/create-pr` → wait → `/babysit-pr` → wait → fix → push → wait

The loop runs: **Pre-flight → Auto-fix → Push/PR → Monitor CI → Fix CI → Loop until green**.

## When to trigger

- `/pr-greenlight` — full loop: preflight + create PR + babysit
- `/pr-greenlight preflight` or `/preflight` — only run local pre-flight checks, no push
- User says "帮我把这个 PR 搞绿", "run preflight", "pre-flight checks", "搞定 PR"

## State file

`/tmp/claude-pr-greenlight-state.json`:

```json
{
  "phase": "preflight|pushed|monitoring",
  "pr_number": null,
  "round": 0,
  "max_rounds": 4,
  "preflight_passed": false,
  "checks_run": [],
  "fixes_applied": [],
  "processed_comment_ids": [],
  "started_at": "ISO timestamp"
}
```

Lock file: `/tmp/claude-pr-greenlight.lock`

## Phase 1 — Perceive: Analyze what changed

```bash
touch /tmp/claude-pr-greenlight.lock
```

Run in parallel:

```bash
git rev-parse --abbrev-ref HEAD
git status --short
git diff main...HEAD --stat
git diff main...HEAD --name-only
git log main..HEAD --format="%h %s"
```

From the file list, classify what changed into categories:

| Category | Detection pattern |
|----------|-------------------|
| `rust` | `*.rs`, `Cargo.toml`, `Cargo.lock` |
| `frontend` | `src/**/*.{ts,tsx,js,jsx,css}`, `package.json`, `bun.lockb` |
| `api-endpoints` | `*.rs` files containing `#[utoipa::path]` or `#[tauri::command]` changes |
| `tauri-ipc` | changes in `src-tauri/src/`, DTO structs used by Tauri commands |
| `openapi` | `schema/openapi.json`, files with `#[utoipa::path]` |
| `docs-site` | `docs-site/**` |
| `markdown` | `*.md` (outside docs-site) |
| `generated` | `src/api/generated/**`, `src/lib/ipc-bindings.generated.ts` |

Store the categories in state as `change_categories`.

**Refuse conditions** (same as create-pr):
- On `main` → refuse
- `git log main..HEAD` empty → refuse (nothing to PR)
- `gh auth status` fails → ask user to auth

## Phase 2 — Reason & Act: Pre-flight checks

Run ONLY the checks relevant to `change_categories`. Each check follows: run → detect failure → auto-fix → re-run → confirm.

### 2a — Format (always run)

```bash
# Rust format (if rust changed)
cargo fmt --all --check 2>&1
# If fails:
cargo fmt --all
git add -u '*.rs'

# Frontend format (if frontend/markdown/docs changed)
bun run format 2>&1
# Prettier auto-fixes; stage changed files
git add -u
```

### 2b — Code generation drift (the #1 CI failure cause)

Only if `rust` or `api-endpoints` changed:

```bash
# 1. OpenAPI schema
bun run gen:openapi 2>&1
git diff --exit-code schema/openapi.json
# If diff: stage it
git add schema/openapi.json

# 2. IPC bindings (if tauri-ipc changed)
cargo test -p uc-tauri --test specta_export 2>&1
git diff --exit-code src/lib/ipc-bindings.generated.ts
# If diff: stage it
git add src/lib/ipc-bindings.generated.ts

# 3. API client (if openapi.json changed in step 1 or was already changed)
bun run gen:client 2>&1
git diff --exit-code src/api/generated/
# If diff: stage it
git add src/api/generated/
```

### 2c — Lint (scoped to changed files)

```bash
# Frontend lint — scope to changed files only (full repo has baseline debt)
CHANGED_TS=$(git diff main...HEAD --name-only --diff-filter=d -- '*.ts' '*.tsx' '*.js' '*.jsx' | head -50)
if [ -n "$CHANGED_TS" ]; then
  bun run lint --fix -- $CHANGED_TS 2>&1
  git add -u
fi
```

### 2d — Compilation & type check

```bash
# Rust check (if rust changed)
cargo check --workspace --locked 2>&1 | tail -40

# Frontend build = tsc + vite (if frontend changed)
bun run build 2>&1 | tail -40
```

If either fails, **do NOT auto-fix** — these are real errors. Report to user with the error output and ask how to proceed.

### 2e — Tests (if relevant code changed)

```bash
# Frontend tests (if frontend changed)
bun run test -- --run 2>&1 | tail -40

# Rust tests are slow — only run focused tests for changed crates
# Detect changed crates from file paths
CHANGED_CRATES=$(git diff main...HEAD --name-only -- 'crates/*/src' 'apps/*/src' | sed 's|.*/\(crates/[^/]*\)/.*|\1|;s|.*/\(apps/[^/]*\)/.*|\1|' | sort -u)
for crate_path in $CHANGED_CRATES; do
  crate_name=$(basename $crate_path)
  cargo test -p $crate_name --lib 2>&1 | tail -20
done
```

### 2f — Markdown lint-staged simulation (if markdown changed)

```bash
# Only for non-docs-site markdown
CHANGED_MD=$(git diff main...HEAD --name-only --diff-filter=d -- '*.md' ':!docs-site/**' | head -20)
if [ -n "$CHANGED_MD" ]; then
  for f in $CHANGED_MD; do
    node scripts/fix-md-cjk-emphasis.mjs "$f" 2>/dev/null
    npx autocorrect --fix "$f" 2>/dev/null
    npx prettier --write "$f" 2>/dev/null
  done
  git add -u '*.md'
fi
```

### 2g — Commit pre-flight fixes

If any files were staged by auto-fix steps:

```bash
git diff --cached --stat
```

If there are staged changes, commit them:

```bash
git commit -m "$(cat <<'EOF'
chore: pre-flight auto-fix (format, codegen, lint)
EOF
)"
```

Update state: `preflight_passed = true`.

### Pre-flight summary

Print a table:

```
Pre-flight results:
  ✓ cargo fmt
  ✓ prettier
  ✓ OpenAPI schema (regenerated, +12 -3)
  ✓ IPC bindings (no drift)
  ✓ API client (regenerated)
  ✓ ESLint (2 auto-fixed)
  ✓ cargo check
  ✓ tsc + vite build
  ✓ vitest (14 passed)
  ✓ cargo test uc-core (23 passed)
  — cargo test uc-daemon (skipped, no changes)
```

If the user only asked for `/pr-greenlight preflight` or `/preflight`, **stop here**. Clean up lock file and report results.

## Phase 3 — Push & Create PR

If pre-flight passed, proceed to push and create PR.

**Delegate to the `create-pr` skill logic**:

1. Branch name review (propose rename if needed)
2. docs-site impact scan
3. Push and `gh pr create`

Key points:
- Follow all `create-pr` conventions (title format, body format, branch name check)
- Record the PR number in state

After PR is created, immediately proceed to Phase 4.

## Phase 4 — Monitor CI (Agent Loop)

This is the core monitoring loop, similar to `babysit-pr` but integrated.

### 4a — Wait for CI to start

```bash
# Give CI 30s to register checks
sleep 30
gh pr checks --json name,state,conclusion 2>/dev/null
```

If no checks registered yet, schedule wakeup:
```
ScheduleWakeup(120s, "waiting for CI checks to register")
```

### 4b — Check status

```bash
gh pr checks --json name,state,conclusion,link 2>/dev/null
```

Classify:
- **All passed** → go to 4c (check for review comments before declaring victory)
- **Some pending** → schedule wakeup (180s)
- **Some failed** → proceed to 4d

### 4c — Check for bot review comments

Same as babysit-pr Step 2b — check all three GitHub comment endpoints:

```bash
BOT_FILTER='select(.user.login | test("coderabbitai|github-actions|codecov"))'

# Inline review comments
gh api repos/UniClipboard/UniClipboard/pulls/<PR>/comments \
  --jq "[.[] | ${BOT_FILTER} | {id, user: .user.login, body, path, line}]"

# Review bodies
gh api repos/UniClipboard/UniClipboard/pulls/<PR>/reviews \
  --jq "[.[] | ${BOT_FILTER} | {id, user: .user.login, body, state}]"

# Issue comments (CodeRabbit walkthrough lives here)
gh api repos/UniClipboard/UniClipboard/issues/<PR>/comments \
  --jq "[.[] | ${BOT_FILTER} | {id, user: .user.login, body: (.body | .[0:2000])}]"
```

Filter out `processed_comment_ids`. If zero unprocessed actionable comments AND all checks pass → **Success. Clean up.**

### 4d — Fix failures

For each failed check or new review comment:

1. **Diagnose**: Fetch logs (`gh run view <run_id> --log-failed | tail -100`), read relevant code
2. **Classify**:
   - Format/lint failure → auto-fix (same as pre-flight 2a-2c)
   - Code generation drift → re-run gen commands (same as 2b)
   - Compilation error → fix the code
   - Test failure → fix the code
   - CodeRabbit comment → evaluate: must-fix (correctness/security) vs nit (skip)
3. **Fix**: Edit files surgically
4. **Validate locally**: Run the same check that failed to confirm fix works
5. **Commit and push**:

```bash
git add <specific files>
git commit -m "$(cat <<'EOF'
fix: address CI feedback (pr-greenlight round N)
EOF
)"
git push
```

Increment `round` in state. If `round >= max_rounds` → clean up with warning.

### 4e — Loop back

```
ScheduleWakeup(180s, "CI running after pr-greenlight round N fix")
```

Re-enter at Phase 4b.

## Cleanup

**On success:**
```bash
rm -f /tmp/claude-pr-greenlight.lock /tmp/claude-pr-greenlight-state.json
```
```
✅ PR #<N> is green. All CI checks passed after <round> round(s).
   URL: <pr_url>
   Pre-flight caught: <list of issues auto-fixed before push>
   CI rounds: <N>
```

**On max rounds (4):**
```bash
rm -f /tmp/claude-pr-greenlight.lock /tmp/claude-pr-greenlight-state.json
```
```
⚠️ PR #<N> still has failures after 4 rounds. Manual review needed.
   Remaining failures: <list>
   URL: <pr_url>
```

**On timeout (90 min):**
```bash
rm -f /tmp/claude-pr-greenlight.lock /tmp/claude-pr-greenlight-state.json
```

## Safety guardrails

- Max **4 rounds** of CI fix cycles (round only increments on push)
- **90-minute hard timeout**
- **Lock file** prevents re-entrant triggering
- Never `--force` push
- Never amend existing commits
- If a fix breaks something else (cargo check / tsc fails after edit), **revert** the fix and report
- Skip CodeRabbit style nits — only fix correctness/security/performance issues
- Pre-flight auto-fixes go in a dedicated commit, not mixed with feature work
- Scoped lint: only lint changed files (repo has baseline ESLint debt)
- `processed_comment_ids` prevents duplicate processing

## Interaction with existing skills

- **Supersedes** manual `/create-pr` + `/babysit-pr` chain for the common case
- `/create-pr` and `/babysit-pr` remain available for standalone use
- If a PR already exists for the branch, skip Phase 3 (push only, no `gh pr create`)
- If user runs `/pr-greenlight preflight`, only Phase 1-2 execute (no push, no PR)

## Anti-patterns

- Running all checks regardless of what changed (wastes time on unrelated failures)
- Auto-fixing compilation errors without understanding them
- Pushing broken code because "CI will catch it"
- Running `bun run lint` on entire repo (33 baseline errors will confuse the loop)
- Treating `cargo fmt` failures as real bugs (they're always auto-fixable)
- Skipping the local validation step after fixing a CI failure (leads to ping-pong)
- Processing old/already-addressed CodeRabbit comments
- Applying every CodeRabbit nit blindly
