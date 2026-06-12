---
name: e2e-test-thinker
description: "Analyze the current branch's diff against main and determine which changes are testable via CLI-based end-to-end tests. Outputs a concrete test plan with runnable test case sketches using the uc-e2e-tests harness. Use when the user wants to think about, plan, or evaluate e2e test coverage for their current work."
---

# E2E Test Thinker

Analyze the current branch's changes and determine whether — and how — they can be covered by CLI-based end-to-end tests using the `uc-e2e-tests` harness.

## When to Use

- After completing a feature or bug fix, before opening a PR
- When the user asks "should I write e2e tests for this?" or "can this be tested via CLI?"
- When reviewing a branch for test coverage gaps
- Triggered by: "e2e test", "端到端测试", "think about e2e", "e2e coverage", "CLI test plan"

## Background: The E2E Harness

The harness lives at `tests/e2e/` and provides three building blocks:

| Component | Purpose |
|-----------|---------|
| `TestProfile` | UUID-based profile isolation — each test gets its own data dir, socket, and identity |
| `TestDaemon` | Spawn `uniclipd` with a test profile, wait for `/health` to return 200, kill on drop |
| `TestCli` | Run `uniclip` subcommands against a test profile; `run_ok()`, `run_capture()` helpers |

**Test pattern:** every test is `#[tokio::test] #[ignore]` and runs with `cargo test -p uc-e2e-tests -- --ignored`. Tests are black-box: they only interact via the CLI binary and HTTP health endpoint.

**Existing test files** (for reference and dedup):

| File | Covers |
|------|--------|
| `daemon_lifecycle.rs` | start, health, kill |
| `single_node.rs` | init, status, send, search, devices, members (+ JSON variants) |
| `error_cases.rs` | double-init, empty passphrase, commands before init |
| `stop_and_restart.rs` | stop, start, foreground, already-running, setup gate |
| `dual_node.rs` | invite + join pairing (infrastructure only) |
| `clipboard_history.rs` | history list/search |
| `clipboard_sync.rs` | cross-node clipboard sync |
| `file_transfer.rs` | file send/receive |
| `watch_events.rs` | event stream / watch mode |
| `daemon_api.rs` | direct HTTP API calls |
| `mobile_sync.rs` | mobile sync gateway |

## Procedure

### Phase 1: Gather the Diff

Run these commands to understand what changed on the current branch:

```bash
# What files changed?
git diff main...HEAD --stat

# Full diff for analysis
git diff main...HEAD

# Commit messages for intent
git log main..HEAD --oneline
```

If the branch IS main (no divergence), fall back to `git diff HEAD~5..HEAD` or ask the user which range to analyze.

### Phase 2: Classify Each Change

For every changed file/module, classify it into one of these buckets:

| Bucket | CLI-Testable? | Example |
|--------|--------------|---------|
| **CLI command added/modified** | ✅ Yes — primary target | New subcommand, changed output format, new flag |
| **Daemon API endpoint added/modified** | ✅ Yes — via CLI or direct HTTP | New route, changed response shape |
| **Core business logic change** | ⚠️ Maybe — only if it surfaces through CLI output or behavior | Encryption change that affects `init` flow |
| **GUI-only change** | ❌ No — needs browser/Tauri test | React component, Tauri command handler |
| **Build/CI/docs change** | ❌ No | Cargo.toml deps, CI yaml, markdown |
| **Internal refactor (same behavior)** | ⚠️ Regression only — existing tests should still pass | Renamed internal module, changed data structure |

### Phase 3: Check for Existing Coverage

For each ✅/⚠️ item, grep existing test files to see if it's already covered:

```bash
grep -r "relevant_command_or_keyword" tests/e2e/tests/
```

Report what's covered and what's not.

### Phase 4: Design Test Cases

For each uncovered testable change, produce a test case sketch following these rules:

**Structure rules:**
1. One `#[tokio::test] #[ignore] async fn test_<descriptive_name>()` per scenario
2. Use `TestProfile::new("unique-slug")` for isolation
3. Use `TestDaemon::start(profile).await` to spawn daemon
4. Use `TestCli::new(&daemon.profile)` for CLI commands
5. Always init before testing business commands (unless testing pre-init behavior)
6. Use `run_capture()` for assertions on exit code, stdout, stderr
7. Use `--json` flag when testing structured output
8. Parse JSON with `serde_json::from_str::<serde_json::Value>()` for schema assertions

**Assertion patterns:**
- Exit code: `assert!(output.success())` or `assert!(!output.success())`
- JSON field: `json.get("field").and_then(|v| v.as_str())`
- Contains: `combined.contains("expected")`  
- Valid JSON: `serde_json::from_str(output.stdout.trim()).is_ok()`

**Decide which test file the new case belongs in:**
- Fits an existing file's theme → add to that file
- New feature area → propose a new file name

### Phase 5: Output the Plan

Present the results as:

```
## E2E Test Analysis for branch `<branch-name>`

### Changes Summary
- <one-line per changed area>

### Not Testable via CLI
- <item>: <reason>

### Already Covered
- <item>: covered by `<test_file>::<test_fn>`

### Proposed New Tests

#### 1. `test_<name>` → `<target_file>.rs`

**What it tests:** <one sentence>

**Sketch:**
```rust
#[tokio::test]
#[ignore]
async fn test_<name>() {
    // ... concrete test code ...
}
```

**Confidence:** High / Medium / Low
**Why:** <why this confidence level>
```

### Phase 6: Ask Before Writing

After presenting the plan, ask the user:
1. Which tests to actually write (all / subset / none)
2. Whether to create new test files or append to existing ones
3. Any edge cases they want added

Do NOT write test code until the user confirms.

## Decision Criteria: When NOT to Propose E2E Tests

Skip proposing e2e tests when:
- The change is purely GUI (React/Tauri commands) with no CLI surface
- The change is a docs/CI/build-only change
- The change is an internal refactor where existing tests provide regression coverage
- The CLI binary doesn't expose the changed behavior (e.g., internal daemon-to-daemon protocol change with no CLI observability)

In these cases, explain WHY and suggest the appropriate test layer (unit test, integration test, manual test, GUI test).

## Common Pitfalls

- **Don't test daemon internals** — e2e is black-box. If you need to assert internal state, that's a unit/integration test.
- **Don't parse human-readable CLI output with exact string matching** — use `--json` mode and parse structured output.
- **Don't forget profile isolation** — every test MUST use a unique `TestProfile`. Shared profiles cause flaky parallel test runs.
- **Don't assume network** — single-node tests don't need network. Dual-node tests use localhost loopback only.
- **Don't test timing** — avoid `sleep`-based assertions. Poll with a deadline loop if you need to wait for async behavior.
