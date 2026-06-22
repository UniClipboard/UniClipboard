---
name: error-diagnose-fix
description: "Agent Loop for build/compilation errors: run the build yourself, collect ALL errors at once, find root causes via dependency analysis, fix in order, revert failed hypotheses, loop until green. Eliminates the user-paste-error → agent-guess → still-broken ping-pong."
user-invocable: true
allowed-tools: Bash(cargo:*), Bash(bun:*), Bash(npm:*), Bash(npx:*), Bash(node:*), Bash(git:*), Bash(grep:*), Bash(rg:*), Bash(jq:*), Bash(cat:*), Bash(find:*), Bash(diff:*), Bash(rm:*), Bash(wc:*), Bash(eas:*), Bash(pod:*), Bash(xcodebuild:*), Read, Edit, Write, AskUserQuestion, Agent, mcp__context7__resolve-library-id, mcp__context7__query-docs
---

# error-diagnose-fix

## Purpose

An Agent Loop that takes ownership of build/compilation errors instead of waiting for the user to paste them one by one. The agent runs the build command itself, collects all errors at once, analyzes their dependency relationships, fixes root causes first, and loops until the build succeeds.

This eliminates the ping-pong pattern observed in sessions:
```
User: [pastes error]
Agent: [fixes one thing]
User: [pastes next error]
Agent: [fixes, breaks something else]
... 5-10 rounds later ...
```

Replaced by:
```
Agent: [runs build, sees 7 errors, identifies 2 root causes, fixes both, re-runs, 0 errors]
```

## When to trigger

- `/error-diagnose-fix` or `/edf` — start the loop
- `/error-diagnose-fix <command>` — specify the build command explicitly
- User pastes a build error and says "fix this", "搞定这个报错", "编译不过"
- User says "build is broken", "构建失败", "编译错误"
- Framework upgrade just broke the build (expo upgrade, dependency bump, etc.)
- Multiple cascading errors after a refactor

## When NOT to use

- Runtime bugs (wrong behavior, not build failure) → use `systematic-debugging`
- Cross-device sync issues → use `dual-side-debug`
- CI-specific failures (works locally) → use `pr-greenlight` or `babysit-pr`
- Single obvious typo the user already knows about → just fix it directly

## State file

`/tmp/claude-edf-state.json`:

```json
{
  "build_command": "cargo check --workspace",
  "build_system": "cargo",
  "round": 0,
  "max_rounds": 6,
  "started_at": "ISO timestamp",
  "error_snapshots": [
    {
      "round": 0,
      "error_count": 7,
      "error_signatures": ["E0308@uc-core/src/lib.rs:42", "E0599@..."],
      "root_causes_identified": ["type mismatch in ContentHash refactor"],
      "fixes_applied": [],
      "hypothesis": null
    }
  ],
  "reverted_hypotheses": [],
  "resolved_errors": []
}
```

## Phase 1 — Perceive: Detect build system and run build

### 1a — Identify build system and command

If the user provided a command, use it. Otherwise, auto-detect:

| Signal | Build command | Build system |
|--------|---------------|--------------|
| `Cargo.toml` at workspace root | `cargo check --workspace 2>&1` | `cargo` |
| `package.json` with `build` script | `bun run build 2>&1` | `bun` |
| `expo` in dependencies | `npx expo prebuild --clean 2>&1` or `npx expo run:ios 2>&1` | `expo` |
| Both Rust and frontend changed | Run both sequentially | `mixed` |

For this repo specifically, common commands are:
- Rust: `cargo check --workspace --locked`
- Frontend: `bun run build` (tsc + vite)
- Full: both of the above
- Expo (android project): `npx expo run:android` / `npx expo run:ios`

If unclear, ask the user which build to fix.

### 1b — Run the build and capture ALL output

```bash
# Example for cargo
cargo check --workspace --locked 2>&1 | tee /tmp/claude-edf-build-output.txt
echo "EXIT_CODE=$?"
```

**Critical:** capture the FULL output, not just `tail`. Errors at the top of the output often contain the root cause, while the bottom has cascading failures.

### 1c — Parse errors into a structured list

For each build system, parse errors into a uniform structure:

**Cargo errors:**
```
{
  "code": "E0308",           // error code
  "message": "mismatched types",
  "file": "crates/uc-core/src/active_clipboard.rs",
  "line": 42,
  "span": "expected `String`, found `&str`",
  "suggestion": "try using `.to_string()`",   // if rustc suggests
  "note": "..."              // additional context from rustc
}
```

**TypeScript errors:**
```
{
  "code": "TS2322",
  "message": "Type 'X' is not assignable to type 'Y'",
  "file": "src/components/Foo.tsx",
  "line": 15
}
```

**ESLint/Prettier:**
```
{
  "rule": "no-unused-vars",
  "message": "'x' is defined but never used",
  "file": "src/utils.ts",
  "line": 8,
  "fixable": true
}
```

Store the error list and count as `error_snapshots[round]`.

## Phase 2 — Reason: Analyze error dependencies

### 2a — Group errors by file and crate/package

Errors in the same file are often related. Errors in downstream crates that depend on an upstream crate with errors are likely cascading.

### 2b — Identify root cause vs cascade

**Root cause signals:**
- Error in a type definition, trait impl, or function signature → changes cascade to all callers
- Error in a `mod.rs` or re-export → cascades to all importers
- "cannot find" / "not defined" errors pointing to something recently renamed/moved
- Version mismatch in `Cargo.toml` / `package.json` → cascading API breakage

**Cascade signals:**
- Errors that reference a type/function from another file that also has errors
- "method not found" when the trait/struct definition has a separate error
- Many errors in different files all pointing to the same root type/function
- "unused import" errors appearing alongside "not found" errors (import was for something removed)

### 2c — Build a fix order

```
Root causes (fix first):
  1. [E0308] crates/uc-core/src/types.rs:42 — ContentHash type changed
     Cascading errors: 5 errors in 3 files that use ContentHash

  2. [E0432] crates/uc-core/src/lib.rs:8 — module `old_name` not found
     Cascading errors: 2 errors in files that import from old_name

Independent errors (fix after):
  3. [E0599] crates/uc-daemon/src/api.rs:100 — method not found (unrelated)
```

### 2d — For framework/dependency errors: check docs

If errors are caused by a version upgrade or unfamiliar API:

```
Use context7 MCP:
1. mcp__context7__resolve-library-id("expo-router") → get library ID
2. mcp__context7__query-docs(libraryId, "migration guide from v3 to v4")
```

This replaces guessing at breaking changes from training data.

## Phase 3 — Act: Fix root causes

### 3a — State the hypothesis

Before fixing, record in the state file:
```json
{
  "hypothesis": "ContentHash changed from String to [u8; 32], need to update all call sites",
  "files_to_change": ["crates/uc-core/src/dispatch.rs", "crates/uc-daemon/src/sync.rs"],
  "expected_outcome": "7 errors → ~2 errors (cascade eliminated, independent errors remain)"
}
```

### 3b — Create a revert point

```bash
git stash push -m "edf-round-N-checkpoint" --include-untracked 2>/dev/null
# Or if working tree is clean, just note the current HEAD
git rev-parse HEAD > /tmp/claude-edf-revert-point.txt
```

### 3c — Apply fixes

Fix root causes first, in the order from Phase 2c. Guidelines:

- **Type mismatch / API change:** Read the new type definition, update all call sites consistently
- **Missing module / function:** Check if renamed (git log, grep old name) or removed
- **Version breaking change:** Check migration guide (context7), apply required changes
- **Import errors:** Update import paths, re-export if needed

**After fixing each root cause**, do NOT re-run the full build yet. Fix all identified root causes first, then re-run once.

### 3d — Handle auto-fixable errors

Some errors have automatic fixes:

```bash
# Rust: apply compiler suggestions
# (rustc often suggests exact fix, apply them)

# ESLint: auto-fix
bun run lint:fix -- <specific files>

# Prettier: auto-format
bun run format
```

## Phase 4 — Observe: Re-run build and compare

### 4a — Re-run the same build command

```bash
cargo check --workspace --locked 2>&1 | tee /tmp/claude-edf-build-output.txt
echo "EXIT_CODE=$?"
```

### 4b — Compare error sets

Parse the new errors and compare with the previous round:

| Outcome | Meaning | Next action |
|---------|---------|-------------|
| **0 errors** | Build succeeds | **Success → cleanup** |
| **Fewer errors, different ones** | Root cause fixed, cascade eliminated, new errors revealed | Progress. Go to Phase 2 with remaining errors |
| **Same error count, same errors** | Fix didn't work | **Revert. Record failed hypothesis. Try different approach** |
| **More errors than before** | Fix introduced new problems | **Revert immediately.** |
| **Same count but different errors** | Partial progress, some regressions | Analyze carefully. May need partial revert |

### 4c — On regression: revert

```bash
# If fix made things worse or didn't help
git checkout -- .
# Or restore from stash
git stash pop
```

Record the failed hypothesis:
```json
{
  "hypothesis": "...",
  "result": "no improvement — same 7 errors",
  "reverted": true
}
```

Add to `reverted_hypotheses` in state. **Never retry a reverted hypothesis.**

### 4d — Progress check

If errors decreased, celebrate and continue:
```
Round 1: 7 errors → 2 errors (fixed ContentHash cascade)
Round 2: 2 errors → 0 errors ✓
```

## Phase 5 — Loop or escalate

### Loop condition

```
while error_count > 0 AND round < max_rounds AND no_stuck_condition:
    Phase 2 → Phase 3 → Phase 4
    round++
```

### Stuck detection

You are stuck if:
- Same error count for 2 consecutive rounds after different fix attempts
- 3+ hypotheses reverted without progress
- A single error persists across 3+ rounds despite different fix approaches

### On stuck: escalate to user

```
⚠️ Stuck after 3 rounds. The build still has 2 errors:

  1. [E0308] crates/uc-core/src/types.rs:42
     Tried: type conversion (reverted), trait impl (reverted)
     
  2. [TS2322] src/components/Foo.tsx:15
     Tried: type assertion (reverted)

Hypotheses exhausted. Would you like to:
  A) Give me a hint about the intended design
  B) Let me try a broader approach (refactor the affected area)
  C) Stop here — you'll fix these manually
```

### On max rounds (6): report and stop

```bash
rm -f /tmp/claude-edf-state.json
```

```
Build fix attempted for 6 rounds. Progress:
  Round 0: 12 errors
  Round 1: 12 → 5 errors (fixed import paths)
  Round 2: 5 → 3 errors (fixed type mismatches)
  Round 3: 3 → 2 errors (fixed missing trait impl)
  Round 4-5: stuck on 2 errors (2 hypotheses reverted)

Remaining errors:
  [details]

Failed hypotheses (do NOT retry):
  - "add Default impl for FooBar" — reverted, caused 4 new errors
  - "convert FooBar to enum" — reverted, type mismatch cascade
```

## Success cleanup

```bash
rm -f /tmp/claude-edf-state.json /tmp/claude-edf-build-output.txt /tmp/claude-edf-revert-point.txt
```

```
✅ Build succeeded after N round(s).

Fix summary:
  Round 1: Fixed ContentHash type across 3 files (5 cascade errors eliminated)
  Round 2: Updated import paths in uc-daemon (2 errors)
  
Total: 7 errors → 0 errors in 2 rounds.
```

Do NOT auto-commit. The user decides when and how to commit the fixes.

## Special scenarios

### Framework upgrade (Expo, React, dependency bump)

When errors come from a version upgrade:

1. **First**, use context7 to get the migration guide:
   ```
   resolve-library-id("expo") → query-docs(id, "upgrading from SDK 55 to 56")
   ```
2. **Second**, check `CHANGELOG` or release notes of the upgraded package
3. **Third**, look for a codemod or automated migration tool:
   ```bash
   npx expo-doctor     # Expo-specific
   npx @next/codemod   # Next.js-specific
   ```
4. Apply migration steps systematically, not one error at a time

### Cargo workspace: cross-crate cascades

When errors cascade across workspace crates:

1. Check `cargo tree -p <failing-crate>` to understand dependency direction
2. Fix errors in the most-upstream crate first
3. After fixing upstream, many downstream errors will disappear automatically
4. Only then address remaining downstream errors

### Mixed Rust + Frontend errors

When both `cargo check` and `bun run build` fail:

1. Fix Rust errors first (they often regenerate TypeScript bindings)
2. After Rust fixes, re-run code generation:
   ```bash
   bun run gen:openapi
   cargo test -p uc-tauri --test specta_export
   bun run gen:client
   ```
3. Then fix remaining TypeScript errors (many will have been resolved by codegen)

## Safety guardrails

- Max **6 rounds** (more than pr-greenlight because build fixes can be iterative)
- Always create a revert point before applying fixes
- **Revert immediately** if error count increases
- Never retry a reverted hypothesis — record and move on
- Do NOT auto-commit fixes (user decides)
- If the same error persists for 3 rounds, escalate to user
- For ambiguous design decisions (e.g., "should this be an enum or a struct?"), ask the user
- Track all hypotheses in state file for cross-session continuity

## Relationship to other skills

| Skill | When to use instead |
|-------|---------------------|
| `systematic-debugging` | Runtime bugs, wrong behavior (not build failure) |
| `pr-greenlight` | Build passes locally but CI fails |
| `babysit-pr` | PR is already open, CI is failing |
| `diagnosing-bugs` | Functional bugs, not compilation errors |

This skill can be invoked **by** `pr-greenlight` Phase 2d when `cargo check` or `bun run build` fails during pre-flight.

## Anti-patterns

- Waiting for user to paste errors instead of running the build command yourself
- Fixing errors one at a time without analyzing cascades
- Guessing at version compatibility instead of checking context7 docs
- Piling fix upon fix without re-running the build to check progress
- Reverting a fix that made *different* errors (that's progress, not regression)
- Using `--force` or `#[allow(...)]` to silence errors instead of fixing them
- Modifying test expectations to match broken behavior
- Fixing errors in generated files (fix the generator input, then regenerate)
- Attempting more than 2 fixes for the same error without asking the user
