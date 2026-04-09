# Phase 86: CLI Host/Join Flow Phase Refactor - Research

**Researched:** 2026-04-03
**Domain:** CLI setup flow refactoring (Rust async/CLI, daemon HTTP client)
**Confidence:** HIGH

## Summary

This phase refactors the CLI setup flow to centralize remote state parsing and introduce lightweight CLI-phase enums. The key insight is that `SetupStateResponseDto` contains two orthogonal pieces of information: `next_step_hint` (what the CLI should do next) and `state` (the backend pairing session variant). The refactor extracts these into typed enums (`SetupHint`, `SetupVariant`) composing `ParsedSetupState`, then uses `HostCliPhase`/`JoinCliPhase` enums to drive the CLI action loop.

**Primary recommendation:** Follow the Phase 0/1/2/3 incremental approach strictly. Phase 0 fixes only the broken if/else logic without introducing new types, establishing a baseline before the typed-state migration.

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** `run_pair` if/else if 结构修复：Lines 202-218 的双重否定条件清空 submitted session 的逻辑，以及 lines 278-313 的连续 if/else if 分支
- **D-02:** 修双重 `if let Err(...)` 拼接错误（如果存在）
- **D-03:** 去掉空分支
- **D-04:** 去掉重复变量遮蔽
- **D-05:** 加 `state_signature`（Debug fmt 或 Hash 实现）用于"仅在状态变化时打印 debug log"——否则删除
- **D-06:** Helper 函数（`setup_state_variant`、`setup_state_error_code`、`setup_state_short_code`、`format_selected_peer_label`）在 Phase 1 中**直接删除**
- **D-07:** `setup_cli_state` 模块放在 **`uc-daemon-client`** crate
- **D-08:** `SetupHint` 和 `SetupVariant` 为**两个独立 enum**（不合并）
- **D-09:** 提供入口函数 `parse_setup_state(dto: &SetupStateResponseDto) -> ParsedSetupState`
- **D-10:** 旧 helper 在 Phase 1 中**直接删除**，不做 deprecated 保留
- **D-11:** `HostCliPhase` enum with variants: WaitingJoinRequest, NeedDecision { session_id }, NeedVerification { session_id }, WaitingBackendCompletion, Completed, Canceled
- **D-12:** `JoinCliPhase` enum with variants: SelectingPeer, WaitingPeerDiscovery, WaitingHostResponse, NeedPeerConfirmation { session_id }, NeedPassphrase, WaitingBackendCompletion, Completed, Canceled
- **D-13:** `session_id` **放在 phase variant 里**
- **D-14:** `derive_host_phase(parsed: &ParsedSetupState, current: &HostCliPhase) -> HostCliPhase` 纯函数
- **D-15:** 去重字段**不再承担 phase 职责**，仅用于幂等去重
- **D-16:** `HostCliSession` struct with `phase: HostCliPhase`, `pairing_presence_enabled`, `last_lease_refresh`, `spinner`
- **D-17:** `on_phase_changed` 回调**仅处理 UI 状态变化**
- **D-18:** action 失败立即 abort（返回 `EXIT_ERROR`）
- **D-19:** Loop structure: poll -> parse -> derive phase -> match on phase -> sleep
- **D-20:** 文件划分: `uc-daemon-client/src/setup/` + `uc-cli/src/commands/setup/` with host_flow.rs, join_flow.rs, prompt.rs

### Deferred Ideas (OUT OF SCOPE)

- `prompt_host_verification` 和 `prompt_join_peer_confirmation` 合并为 `prompt_peer_trust_confirmation` — 暂推迟

## Phase Requirements

| ID        | Description                                   | Research Support                                 |
| --------- | --------------------------------------------- | ------------------------------------------------ |
| REQ-86-01 | Phase 0: Fix broken if/else in run_pair       | Lines 202-218, 278-313 identified as needing fix |
| REQ-86-02 | Phase 1: ParsedSetupState in uc-daemon-client | D-07/D-08/D-09 confirmed                         |
| REQ-86-03 | Phase 2: HostCliPhase / JoinCliPhase enums    | D-11/D-12/D-13/D-14 confirmed                    |
| REQ-86-04 | Phase 3: Phase-driven loop                    | D-16/D-17/D-18/D-19 confirmed                    |

## Standard Stack

### Core

| Library                  | Version   | Purpose                                | Why Standard                  |
| ------------------------ | --------- | -------------------------------------- | ----------------------------- |
| `serde_json::Value`      | workspace | Dynamic JSON parsing for `state` field | Existing wire protocol        |
| `tokio::time::sleep`     | workspace | Async polling interval                 | Already used in existing code |
| `indicatif::ProgressBar` | workspace | CLI spinner                            | Already used in `ui.rs`       |

### Supporting

| Library                     | Version   | Purpose             | When to Use           |
| --------------------------- | --------- | ------------------- | --------------------- |
| `console::style`            | workspace | ANSI styling        | Already used in ui.rs |
| `dialoguer::Select/Confirm` | workspace | Interactive prompts | Already used in ui.rs |

### Alternatives Considered

No alternatives — existing project conventions followed exactly.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-daemon-client/src/
├── setup/
│   ├── mod.rs           // module re-exports
│   └── parsed_state.rs   // ParsedSetupState, SetupHint, SetupVariant, parse_setup_state()

src-tauri/crates/uc-cli/src/commands/setup/
├── mod.rs               // run_interactive, run_new_space, run_status, run_reset
├── host_flow.rs         // HostCliPhase, HostCliSession, run_pair
├── join_flow.rs         // JoinCliPhase, JoinCliSession, run_connect
└── prompt.rs            // prompt helpers (shared between flows)
```

### Pattern 1: Phase-Driven Loop (D-19)

**What:** Main loop structured as poll -> parse -> derive phase -> execute action
**When to use:** Both `run_pair` and `run_connect` after Phase 2
**Example:**

```rust
loop {
    let dto = fetch_setup_state(...).await?;
    let parsed = parse_setup_state(&dto);
    let next_phase = derive_host_phase(&parsed, &phase);
    if next_phase != phase {
        on_phase_changed(&phase, &next_phase, ...);
        phase = next_phase;
    }
    match &phase {
        HostCliPhase::WaitingJoinRequest => { ... }
        HostCliPhase::NeedDecision { session_id } => { ... }
        HostCliPhase::Completed => return EXIT_SUCCESS,
        HostCliPhase::Canceled => return EXIT_SUCCESS,
    }
    sleep(POLL_INTERVAL).await;
}
```

### Pattern 2: ParsedSetupState (D-08/D-09)

**What:** Centralized parsing of `SetupStateResponseDto` into typed enums
**When to use:** Before any state inspection in the CLI flows
**Example:**

```rust
// In uc-daemon-client/src/setup/parsed_state.rs

/// Hint derived from next_step_hint field (what CLI should do)
enum SetupHint {
    Idle,
    Completed,
    HostConfirmPeer,
    JoinSelectPeer,
    JoinEnterPassphrase,
    Unknown(String),
}

/// Variant derived from state field (backend session type)
enum SetupVariant {
    Idle,
    JoinSpaceConfirmPeer,
    JoinSpaceInputPassphrase,
    Completed,
    Unknown(String),
}

/// Combined parsed state
struct ParsedSetupState {
    hint: SetupHint,
    variant: SetupVariant,
    session_id: Option<String>,
    has_completed: bool,
    short_code: Option<String>,
    selected_peer_label: Option<String>,
}

pub fn parse_setup_state(dto: &SetupStateResponseDto) -> ParsedSetupState {
    // Extract hint from dto.next_step_hint
    // Extract variant from dto.state (Value::String or Value::Object)
    // Extract short_code from state payload if variant == JoinSpaceConfirmPeer
    // Build selected_peer_label from dto.selected_peer_id + dto.selected_peer_name
}
```

### Pattern 3: State Signature for Debug Logging (D-05)

**What:** `Debug` impl or `Hash` on `SetupStateResponseDto` for change detection
**When to use:** Only if debug logging is valuable, otherwise delete per D-05
**Example:**

```rust
// Option A: Debug impl (simpler, for formatted comparison)
impl Debug for SetupStateResponseDto {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupStateResponseDto")
            .field("hint", &self.next_step_hint)
            .field("variant", &setup_state_variant(&self.state))
            .field("session_id", &self.session_id)
            .field("has_completed", &self.has_completed)
            .finish()
    }
}

// Option B: Hash (for Eq-based comparison)
impl Hash for SetupStateResponseDto { ... }
```

### Anti-Patterns to Avoid

- **Variable shadowing in loop:** `let state: SetupStateResponseDto = ...` shadows outer `initial_state`. Acceptable for loop variable reuse but must not shadow across prompt branches.
- **Double negative conditions:** `if !matches!(...) || !matches!(...)` — Phase 0 fix target
- **Empty branches:** `else if state.next_step_hint == "completed" {}` — Phase 0 fix target
- **if/else if chain without final else:** Phases 278-313 in `run_pair` — Phase 0 fix target

## Don't Hand-Roll

| Problem                | Don't Build                  | Use Instead                                   | Why                                                            |
| ---------------------- | ---------------------------- | --------------------------------------------- | -------------------------------------------------------------- |
| JSON state parsing     | ad-hoc Value matching in CLI | `parse_setup_state()` in daemon-client        | `SetupHint`/`SetupVariant` enums centralize protocol knowledge |
| Phase derivation logic | inline match on raw strings  | `derive_host_phase()` / `derive_join_phase()` | Pure functions, testable independently                         |

## Common Pitfalls

### Pitfall 1: Phase 0 vs Phase 1 Boundary Blur

**What goes wrong:** Developer tries to "clean up" old helpers while fixing Phase 0, breaking the incremental approach.
**Why it happens:** D-06 says delete helpers in Phase 1, but D-05's state_signature might seem to require the new types.
**How to avoid:** Phase 0 is ONLY bug fixes to existing if/else structure. Phase 1 adds ParsedSetupState. Do not mix.
**Warning signs:** `parse_setup_state` appears in Phase 0 code.

### Pitfall 2: session_id Lifetime in Phase Derivation

**What goes wrong:** `session_id` extracted in one loop iteration is used after phase transition, causing stale session*id in prompts.
**Why it happens:** D-13 puts session_id in phase variant, but implementation still uses outer variable.
**How to avoid:** `derive*\*\_phase()`functions receive`current: &HostCliPhase`to access session_id from current phase if needed.
**Warning signs:**`submitted_host_decision_session` variable still used after Phase 3.

### Pitfall 3: Broken Merge of Helper Functions

**What goes wrong:** `prompt_host_verification` and `prompt_join_peer_confirmation` have slightly different signatures — merge is not trivial.
**Why it happens:** The two functions take `state: &SetupStateResponseDto` but peer label formatting differs.
**How to avoid:** Keep separate per D-14 discretion — deferred `prompt_peer_trust_confirmation` is a separate concern.

## Code Examples

### Existing Broken Structure (Phase 0 Target)

From `setup.rs` lines 202-218:

```rust
// PROBLEM: Double negative clearing logic
if !matches!(state.next_step_hint.as_str(), "host-confirm-peer")
    || matches!(
        setup_state_variant(&state.state),
        Some("JoinSpaceConfirmPeer")
    )
{
    submitted_host_decision_session = None;
}

if state.next_step_hint != "host-confirm-peer"
    || !matches!(
        setup_state_variant(&state.state),
        Some("JoinSpaceConfirmPeer")
    )
{
    submitted_host_verification_session = None;
}
```

### ParsedSetupState Entry Point (Phase 1 Target)

```rust
// uc-daemon-client/src/setup/parsed_state.rs
pub fn parse_setup_state(dto: &SetupStateResponseDto) -> ParsedSetupState {
    let hint = match dto.next_step_hint.as_str() {
        "idle" => SetupHint::Idle,
        "completed" => SetupHint::Completed,
        "host-confirm-peer" => SetupHint::HostConfirmPeer,
        "join-select-peer" => SetupHint::JoinSelectPeer,
        "join-enter-passphrase" => SetupHint::JoinEnterPassphrase,
        other => SetupHint::Unknown(other.to_string()),
    };

    let variant = match setup_state_variant(&dto.state) {
        Some("Idle") => SetupVariant::Idle,
        Some("JoinSpaceConfirmPeer") => SetupVariant::JoinSpaceConfirmPeer,
        Some("JoinSpaceInputPassphrase") => SetupVariant::JoinSpaceInputPassphrase,
        Some("Completed") => SetupVariant::Completed,
        Some(s) => SetupVariant::Unknown(s.to_string()),
        None => SetupVariant::Unknown("<none>".to_string()),
    };

    let short_code = extract_short_code(&dto.state, &variant);
    let selected_peer_label = format_selected_peer_label(dto);

    ParsedSetupState { hint, variant, session_id: dto.session_id.clone(), has_completed: dto.has_completed, short_code, selected_peer_label }
}
```

### derive_host_phase Function (Phase 2 Target)

```rust
// uc-cli/src/commands/setup/host_flow.rs
pub fn derive_host_phase(parsed: &ParsedSetupState, current: &HostCliPhase) -> HostCliPhase {
    use HostCliPhase::*;
    use SetupHint::*;

    match &parsed.hint {
        Idle if matches!(current, WaitingBackendCompletion { .. }) => WaitingBackendCompletion,
        Idle => Canceled,
        Completed => Completed,
        HostConfirmPeer => {
            if matches!(parsed.variant, SetupVariant::JoinSpaceConfirmPeer) {
                NeedVerification { session_id: parsed.session_id.clone().unwrap_or_default() }
            } else {
                NeedDecision { session_id: parsed.session_id.clone().unwrap_or_default() }
            }
        }
        _ => current.clone(),
    }
}
```

## State of the Art

| Old Approach                                | Current Approach                         | When Changed | Impact                                           |
| ------------------------------------------- | ---------------------------------------- | ------------ | ------------------------------------------------ |
| Inline string matching on `next_step_hint`  | `SetupHint` enum with exhaustive match   | Phase 86     | Compile-time exhaustiveness checking             |
| State as untyped `Value`                    | `SetupVariant` enum extracted from Value | Phase 86     | Clear protocol variant coverage                  |
| if/else if chain with implicit fall-through | Phase-driven match on typed enums        | Phase 86     | Each phase is explicit, no implicit fall-through |

## Open Questions

1. **state_signature implementation choice**
   - What we know: D-05 mentions Debug fmt or Hash for "only print debug when state changes"
   - What's unclear: Whether the actual debug logging value justifies the implementation cost
   - Recommendation: Implement Debug on SetupStateResponseDto (minimal) and let Phase 0 use it to add one `debug!` line per state change in run_pair

2. **short_code extraction from state Value**
   - What we know: `setup_state_short_code` extracts from `map.get("JoinSpaceConfirmPeer")?.get("short_code")`
   - What's unclear: Whether other variants can have short_code
   - Recommendation: Only extract for `JoinSpaceConfirmPeer` variant, return None for others

3. **On phase_changed callback scope**
   - What we know: D-17 says "only UI state changes, no business logic"
   - What's unclear: What counts as UI state vs business logic — `disable_host_pairing_presence` is called in multiple places
   - Recommendation: `on_phase_changed` prints debug log and manages spinner; actual `register_gui_participant(false, ...)` stays in match arms

## Environment Availability

| Dependency | Required By     | Available       | Version   | Fallback |
| ---------- | --------------- | --------------- | --------- | -------- |
| Rust 1.75+ | All Rust crates | Yes (workspace) | workspace | —        |
| tokio      | Async runtime   | Yes (workspace) | workspace | —        |
| serde_json | Value parsing   | Yes (workspace) | workspace | —        |
| indicatif  | ProgressBar     | Yes (workspace) | workspace | —        |

**Missing dependencies with no fallback:** None

## Validation Architecture

### Test Framework

| Property           | Value                                                            |
| ------------------ | ---------------------------------------------------------------- |
| Framework          | Rust built-in `#[test]` + `#[cfg(test)]`                         |
| Config file        | None — inline in source files                                    |
| Quick run command  | `cd src-tauri && cargo test -p uc-cli --lib -- --test-threads=1` |
| Full suite command | `cd src-tauri && cargo test -p uc-cli`                           |

### Phase Requirements to Test Map

| Req ID    | Behavior                                                  | Test Type | Automated Command                                             | File Exists            |
| --------- | --------------------------------------------------------- | --------- | ------------------------------------------------------------- | ---------------------- |
| REQ-86-01 | setup_state_variant parses string and object variants     | unit      | `cargo test -p uc-cli --lib setup::tests -- --test-threads=1` | Yes (setup.rs:844-972) |
| REQ-86-01 | setup_state_error_code extracts error from payload        | unit      | same                                                          | Yes                    |
| REQ-86-01 | filter_joinable_peers removes paired peers                | unit      | same                                                          | Yes                    |
| REQ-86-02 | parse_setup_state produces correct SetupHint/SetupVariant | unit      | `cargo test -p uc-daemon-client --lib setup::`                | No (new file)          |
| REQ-86-03 | derive_host_phase produces correct phase transitions      | unit      | `cargo test -p uc-cli --lib host_flow::`                      | No (new file)          |
| REQ-86-03 | derive_join_phase produces correct phase transitions      | unit      | `cargo test -p uc-cli --lib join_flow::`                      | No (new file)          |

### Sampling Rate

- **Per task commit:** `cargo test -p uc-cli --lib -- --test-threads=1 2>&1 | tail -20`
- **Per wave merge:** `cargo test -p uc-cli -p uc-daemon-client`
- **Phase gate:** All tests green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `uc-daemon-client/src/setup/parsed_state.rs` — Unit tests for `parse_setup_state`
- [ ] `uc-daemon-client/src/setup/mod.rs` — Module definition
- [ ] `uc-cli/src/commands/setup/host_flow.rs` — Unit tests for `derive_host_phase`
- [ ] `uc-cli/src/commands/setup/join_flow.rs` — Unit tests for `derive_join_phase`
- [ ] Framework install: `cargo test -p uc-daemon-client -p uc-cli` (runs from workspace root)

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-cli/src/commands/setup.rs` — Main refactoring target
- `src-tauri/crates/uc-daemon/src/api/dto/setup.rs` — SetupStateResponseDto definition
- `src-tauri/crates/uc-daemon-client/src/lib.rs` — daemon-client crate structure
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — Protocol constants

### Secondary (MEDIUM confidence)

- None — all primary sources verified

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — existing workspace dependencies only
- Architecture: HIGH — decisions fully specified in D-01 through D-20
- Pitfalls: HIGH — known anti-patterns explicitly called out in D-01 through D-05

**Research date:** 2026-04-03
**Valid until:** 2026-05-03 (30 days — stable phase with locked decisions)
