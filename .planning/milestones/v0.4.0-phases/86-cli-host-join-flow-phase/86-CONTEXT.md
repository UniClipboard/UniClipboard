# Phase 86: cli-host-join-flow-phase - Context

**Gathered:** 2026-04-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Refactor CLI setup flow (run_pair / run_connect) to:

1. Centralize remote state string parsing into typed `ParsedSetupState`
2. Introduce lightweight `HostCliPhase` / `JoinCliPhase` enums as CLI-layer phase interpretation
3. Restructure the main loop as "poll → parse → derive phase → execute action"
4. Fix existing broken code (if/else, double if let, empty branches, variable shadowing)

After this phase, the CLI layer does only three things:

- Poll daemon setup state
- Parse remote state into typed `ParsedSetupState`
- Decide what to display/send based on current CLI phase

</domain>

<decisions>
## Implementation Decisions

### Phase 0 Scope — "Stop the bleeding"

- **D-01:** `run_pair` 的 if/else if 结构修复：Lines 202-218 的双重否定条件清空 submitted session 的逻辑，以及 lines 278-313 的连续 if/else if 分支
- **D-02:** 修双重 `if let Err(...)` 拼接错误（如果存在）
- **D-03:** 去掉空分支
- **D-04:** 去掉重复变量遮蔽
- **D-05:** 加 `state_signature`（Debug fmt 或 Hash 实现）用于"仅在状态变化时打印 debug log"——否则删除
- **D-06:** Helper 函数（`setup_state_variant`、`setup_state_error_code`、`setup_state_short_code`、`format_selected_peer_label`）在 Phase 1 中**直接删除**，全部迁移到新 `parse_setup_state()` 后不再保留

### Phase 1 — Typed Remote State Parsing

- **D-07:** `setup_cli_state` 模块放在 **`uc-daemon-client`** crate（不是 uc-cli 或 uc-core）；解析逻辑离 DTO 近，uc-cli 引用 client 的解析结果
- **D-08:** 保持 `SetupHint` 和 `SetupVariant` 为**两个独立 enum**（不合并）：
  - `SetupHint` 来自 `next_step_hint` 字段（`Idle`、`Completed`、`HostConfirmPeer`、`JoinSelectPeer`、`JoinEnterPassphrase`、`Unknown(String)`）
  - `SetupVariant` 来自 `state` 字段（`Idle`、`JoinSpaceConfirmPeer`、`JoinSpaceInputPassphrase`、`Completed`、`Unknown(String)`）
  - `ParsedSetupState` 同时包含两者
- **D-09:** 提供入口函数 `parse_setup_state(dto: &SetupStateResponseDto) -> ParsedSetupState`
- **D-10:** 旧 helper（`setup_state_variant`、`setup_state_error_code`、`setup_state_short_code`、`format_selected_peer_label`）在 Phase 1 中**直接删除**，不做 deprecated 保留

### Phase 2 — Lightweight CLI Phase Enums

- **D-11:** `HostCliPhase` enum：
  ```rust
  enum HostCliPhase {
      WaitingJoinRequest,
      NeedDecision { session_id: String },
      NeedVerification { session_id: String },
      WaitingBackendCompletion,
      Completed,
      Canceled,
  }
  ```
- **D-12:** `JoinCliPhase` enum：
  ```rust
  enum JoinCliPhase {
      SelectingPeer,
      WaitingPeerDiscovery,
      WaitingHostResponse,
      NeedPeerConfirmation { session_id: String },
      NeedPassphrase,
      WaitingBackendCompletion,
      Completed,
      Canceled,
  }
  ```
- **D-13:** `session_id` **放在 phase variant 里**（`NeedDecision { session_id }`），不分离到 `HostCliSession`；phase 切换时自然丢弃旧的 session_id
- **D-14:** 提供纯函数 `derive_host_phase(parsed: &ParsedSetupState, current: &HostCliPhase) -> HostCliPhase` 和 `derive_join_phase(parsed: &ParsedSetupState, current: &JoinCliPhase) -> JoinCliPhase`
- **D-15:** `last_submitted_decision_session` / `last_submitted_verification_session` 等去重字段**不再承担 phase 职责**，仅用于幂等去重；phase 决定是否 prompt

### Phase 3 — Phase-Driven Loop

- **D-16:** `HostCliSession` 结构：

  ```rust
  struct HostCliSession {
      phase: HostCliPhase,
      pairing_presence_enabled: bool,
      last_lease_refresh: Instant,
      spinner: Option<ProgressBar>, // ProgressBar is Clone
  }
  ```

  `JoinCliSession` 类似结构

- **D-17:** `on_phase_changed` 回调**仅处理 UI 状态变化**（打印阶段切换日志、清理 spinner），不涉及业务逻辑；match arm 负责 prompt 和 action

- **D-18:** Phase-driven loop 中 **action 失败立即 abort**（返回 `EXIT_ERROR`），不 retry、不 spinner 提示；match arm 里的 prompt 失败由用户取消处理

- **D-19:** Loop 结构：
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
          // ...
          HostCliPhase::Completed => return EXIT_SUCCESS,
          HostCliPhase::Canceled => return EXIT_SUCCESS,
      }
      sleep(POLL_INTERVAL).await;
  }
  ```

### Module Structure

- **D-20:** 文件划分（在 `uc-daemon-client` 的 `src/` 下新增 `setup/`）：
  ```
  uc-daemon-client/src/setup/
    mod.rs
    parsed_state.rs    // ParsedSetupState / parse_setup_state()
  ```
  `uc-cli/src/commands/setup/` 下：
  ```
  setup/
    mod.rs            // run_interactive / run_new_space / run_status / run_reset
    host_flow.rs      // HostCliPhase / HostCliSession / run_pair
    join_flow.rs      // JoinCliPhase / JoinCliSession / run_connect
    prompt.rs         // 通用 prompt helper
  ```

### Deferred Ideas

- `prompt_host_verification` 和 `prompt_join_peer_confirmation` 合并为 `prompt_peer_trust_confirmation` — 两个函数逻辑几乎一样，可抽成一个带 title 参数的通用函数；暂推迟到 Phase 86 执行过程中如果发现容易处理时一并做

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Core Files (existing, will be modified)

- `src-tauri/crates/uc-cli/src/commands/setup.rs` — Main refactoring target; contains run_pair, run_connect, and existing helpers
- `src-tauri/crates/uc-cli/src/ui.rs` — UI helpers (spinner, prompts, verification code display)
- `src-tauri/crates/uc-daemon-client/src/lib.rs` — uc-daemon-client crate root; new setup/ module will be added here
- `src-tauri/crates/uc-daemon/src/api/dto/setup.rs` — `SetupStateResponseDto` definition (source of the wire protocol)

### Protocol Strings (already centralized)

- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — String constants for wire protocol; existing constants already used for setup state variants/hints

### Related Prior Phases

- `46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri/46-CONTEXT.md` — daemon-side pairing host architecture
- `67-setup-filter/67-CONTEXT.md` — setup completion emitter pattern

### No external specs — requirements fully captured in decisions above

</canonical_refs>

<codebase_context>

## Existing Code Insights

### Reusable Assets

- `indicatif::ProgressBar` — Clone, so `Option<ProgressBar>` in session structs works with simple ownership transfer
- Existing `ui::spinner()`, `ui::confirm()`, `ui::password()` — already established prompt patterns
- `DaemonClientContext::from_env()` — existing daemon connection pattern

### Established Patterns

- Phase-driven loop with `tokio::time::sleep(POLL_INTERVAL)` polling — already established
- `setup_client.get_setup_state().await` — existing polling pattern
- `uc_bootstrap::build_cli_runtime()` — existing CLI runtime construction

### Integration Points

- `uc-daemon-client` → `uc-cli`: CLI calls `DaemonClientContext::from_env()` then `.setup_client()`
- New `setup/` module in `uc-daemon-client` consumed by `uc-cli` via `use uc_daemon_client::setup::ParsedSetupState`
- `HostCliPhase` / `JoinCliPhase` defined in `uc-cli` but derived by `derive_*_phase()` functions that accept `ParsedSetupState` from daemon-client

### Breaking Changes from this Phase

- `setup_state_variant()` / `setup_state_error_code()` / `setup_state_short_code()` / `format_selected_peer_label()` — deleted, all callers migrated to `parse_setup_state()`

</codebase_context>

<specifics>
## Specific Ideas

- `state_signature` in Phase 0: add a `Debug` impl or simple hash for `SetupStateResponseDto` to detect state changes; only print debug log when signature changes, not every poll
- `prompt_peer_trust_confirmation` merge: `fn prompt_peer_trust_confirmation(peer_label: &str, short_code: Option<&str>, title: &str) -> Result<bool, String>` — handles both host verification and join peer confirmation with title parameter
- Phase 0 debug log: after fix, `run_pair` should print a single debug line when `state_signature` changes (not every iteration)

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)

None — no todos matched this phase scope.

### Ideas Mentioned During Discussion

- **Merge `prompt_host_verification` + `prompt_join_peer_confirmation`** — noted as easy to do during Phase 86 execution if time permits; not a separate phase
- **state_signature 用途澄清** — 用于"仅在状态变化时打印 debug log"；如果这个功能没有价值则整个删掉，不在 Phase 0 的范围内

</deferred>

---

_Phase: 86-cli-host-join-flow-phase_
_Context gathered: 2026-04-03_
