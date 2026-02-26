# PeerB Passive Clipboard Integration Mode

## TL;DR

> **Summary**: 为本地双实例（peerA/peerB）开发测试引入 `UC_CLIPBOARD_MODE=full|passive`。`passive` 下不启动 OS clipboard watcher、禁止 OS clipboard read/write，但仍能接收远端剪贴板消息并**直接持久化 + 刷新 UI**。
> **Deliverables**:
>
> - `UC_CLIPBOARD_MODE` 环境变量驱动的两种模式（默认 `full`）
> - `passive` 入站同步：不写 OS clipboard、仍落库并触发 `clipboard://event`
> - `passive` 下 `restore_clipboard_entry` / `sync_clipboard_items` 明确报错
> - 单元/集成测试覆盖关键约束（不触碰 OS ports、无重复持久化）
>   **Effort**: Medium
>   **Parallel**: YES - 2 waves
>   **Critical Path**: Mode/policy 类型 → usecase 改造（watcher/inbound/restore）→ bootstrap loop emit → 命令显式报错 → 测试与双实例验证

## Context

### Original Request

- 本地开发环境要跑两个实例（peerA、peerB）做 P2P 同步测试。
- peerA：保持现有行为（监听系统剪贴板变化、捕获、出站同步；入站可按现有流程写回系统剪贴板）。
- peerB：
  - **不得监听**系统剪贴板变化（不触发本地捕获/出站）
  - **仍需接收**远端剪贴板同步并持久化/展示
  - **不得写入**系统剪贴板（即使收到远端同步）
- 不要硬编码 `peerB`；要抽象可配置。

### Interview Summary

- 模式选择：环境变量 `UC_CLIPBOARD_MODE=full|passive`（默认 `full`）。
- `passive` 下：`restore_clipboard_entry` 与 `sync_clipboard_items` 必须返回显式错误（不是 silent no-op）。
- `passive` 目标明确为“完全不与 OS clipboard 交互（watch/read/write 都禁用）”。

### Repo Findings (grounded)

- 多实例脚本已存在：`package.json` 使用 `UC_PROFILE=a|b` 隔离 DB/settings 路径（`tauri:dev:peerA`, `tauri:dev:peerB`, `tauri:dev:dual`）。
- watcher 启动链路：
  - `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs:158` `AppLifecycleCoordinator::ensure_ready()` 总会调用 `StartClipboardWatcher::execute()`。
  - `src-tauri/crates/uc-app/src/usecases/start_clipboard_watcher.rs:52` 调用 `WatcherControlPort::start_watcher()`。
- 入站同步现状（问题根源）：
  - `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs:43`：解密 → 读 OS clipboard 做 dedupe → `set_next_origin(RemotePush)` → `SystemClipboardPort::write_snapshot()`。
  - DB 持久化与 `clipboard://event` 刷新依赖 watcher 回调（`src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:859` 的 `on_clipboard_changed`）。
  - `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:1318` 的 `run_clipboard_receive_loop` 成功时不 emit `clipboard://event`。
- 命令层差异：
  - `src-tauri/crates/uc-tauri/src/commands/clipboard.rs:256` `sync_clipboard_items` 当前无论成功失败都 `Ok(true)`；需求要求 passive 必须显式 Err。

### Metis Review (gaps addressed)

- Guardrails：full 模式行为保持不变；passive 模式严禁触碰 OS clipboard ports。
- 去重：不再读 OS snapshot；最小正确性用 `ClipboardMessage.id` 做 in-memory TTL/LRU 去重（并在计划里留可逆 TODO 以支持未来 DB 幂等）。
- 事件 payload：复用现有 `crate::events::forward_clipboard_event` 与 `ClipboardEvent::NewContent`，不改变前端既有 `entry_id` 字段契约。

## Work Objectives

### Core Objective

引入可配置 clipboard 集成模式，使 peerB 在同机双实例测试下不再因共享 OS clipboard 而产生本地捕获/出站，同时仍能展示远端入站内容。

### Deliverables

- `UC_CLIPBOARD_MODE` 两种模式：`full`（默认）、`passive`。
- passive：
  - watcher 不启动
  - 入站消息直接持久化（使用 `CaptureClipboardUseCase::execute_with_origin(..., RemotePush)`）
  - 成功后显式 emit `clipboard://event` 触发 UI reload
  - `restore_clipboard_entry` / `sync_clipboard_items` 返回 Err（用户可见）

### Definition of Done (verifiable)

- `cd src-tauri && cargo test -p uc-app` 通过。
- `cd src-tauri && cargo test -p uc-tauri` 通过。
- `cd src-tauri && cargo test --workspace` 通过。
- 被动模式关键断言（自动化测试）：
  - 不调用 `WatcherControlPort::start_watcher()`
  - 入站成功不调用 `SystemClipboardPort::read_snapshot/write_snapshot`
  - 入站成功返回 outcome 并触发一次 `clipboard://event`
  - `sync_clipboard_items` 在 passive 下返回 `Err(...)`

### Must Have

- 零 peer 名称硬编码（不基于 `UC_PROFILE`/peerId 分支）。
- full 模式保持现有行为（入站仍写 OS clipboard + watcher 持久化 + emit + outbound skip RemotePush）。

### Must NOT Have (guardrails)

- 不要在 `uc-platform`/adapter 中引入业务决策。
- 不要修改网络协议/消息结构（`ClipboardMessage` 不变）。
- 不要改变 `clipboard://event` payload 字段名（保持 `entry_id` 形状）。
- 不要引入新三方 crate 依赖来做 LRU（用 std + tokio 即可）。

## Verification Strategy

- Test decision: tests-after（先改代码再补齐/更新测试），但每个任务必须带自动化断言。
- QA policy: 每个任务包含“可执行验证”（`cargo test ...` / 事件监听断言 / 双实例脚本验证）。
- Evidence: `.sisyphus/evidence/task-{N}-{slug}.text`（命令输出/关键日志摘录）。

## Execution Strategy

### Parallel Execution Waves

Wave 1（基础定义 + 注入点，互不阻塞）

- Task 1: `uc-app` 增加 `ClipboardIntegrationMode` 类型
- Task 2: `uc-tauri` 增加 `UC_CLIPBOARD_MODE` 解析 helper
- Task 3: `package.json` 脚本增加 `UC_CLIPBOARD_MODE`

Wave 2（依赖 Wave 1 的行为改造）

- Task 4: watcher 启动 gating（StartClipboardWatcher）
- Task 5: restore gating（RestoreClipboardSelectionUseCase）
- Task 6: inbound passive 持久化 + outcome + in-memory dedupe（SyncInboundClipboardUseCase）
- Task 7: receive loop 成功 emit（uc-tauri wiring）
- Task 8: runtime UseCases accessor 适配新构造函数
- Task 9: `sync_clipboard_items` passive 显式报错

### Dependency Matrix (full)

- 1 → 4,5,6,7,8,9
- 2 → 7,8,9
- 3 → (独立，最终 QA 使用)
- 4 → 8
- 5 → 8
- 6 → 7,8

### Agent Dispatch Summary

- Wave 1: 3 tasks（quick/unspecified-low）
- Wave 2: 6 tasks（unspecified-high）

## TODOs

- [x] 1. 添加 `ClipboardIntegrationMode`（uc-app 纯数据）

  **What to do**:
  - 新增 `src-tauri/crates/uc-app/src/usecases/clipboard/integration_mode.rs`：
    - `pub enum ClipboardIntegrationMode { Full, Passive }`
    - 提供映射方法（决策已定：Passive 禁止 watch/read/write）：
      - `observe_os_clipboard()`
      - `allow_os_read()`
      - `allow_os_write()`
  - 更新 `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs` 增加 `pub mod integration_mode;` 并 `pub use ...` 方便引用。

  **Must NOT do**:
  - 不要在此处读取 env / settings。

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: 新增小模块 + 导出
  - Skills: [`executing-plans`] — keep diff minimal

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 4,5,6,8 | Blocked By: none

  **References**:
  - Clipboard module root: `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs:1`

  **Acceptance Criteria**:
  - [ ] `cd src-tauri && cargo check -p uc-app` 通过

  **QA Scenarios**:

  ```text
  Scenario: Compilation sanity
    Tool: Bash
    Steps: cd src-tauri && cargo check -p uc-app
    Expected: exit code 0
    Evidence: .sisyphus/evidence/task-1-mode-type.text

  Scenario: No env coupling
    Tool: Grep
    Steps: search new module for "env::var" usage
    Expected: no matches
    Evidence: .sisyphus/evidence/task-1-no-env.text
  ```

  **Commit**: YES | Message: `arch: add clipboard integration mode type` | Files: `src-tauri/crates/uc-app/src/usecases/clipboard/integration_mode.rs`, `src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs`

- [x] 2. 添加 `UC_CLIPBOARD_MODE` 解析 helper（uc-tauri）

  **What to do**:
  - 新增 `src-tauri/crates/uc-tauri/src/bootstrap/clipboard_integration_mode.rs`：
    - `fn parse_clipboard_integration_mode(raw: Option<&str>) -> ClipboardIntegrationMode`
      - `Some("passive")`（大小写不敏感、允许空白）→ `Passive`
      - `Some("full")`/None/非法值 → `Full`
    - `pub fn resolve_clipboard_integration_mode() -> ClipboardIntegrationMode`（只读 env + 调 parse）
    - 非法值用 `tracing::warn!(...)` 记录但不 panic。
  - 在 `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` 里导出（如果多个模块要用）。
  - 单元测试仅测 `parse_*`（避免 env 并发）。

  **Must NOT do**:
  - 不要在 helper 中缓存全局可变状态（避免测试污染）。

  **Recommended Agent Profile**:
  - Category: `quick`
  - Skills: [`executing-plans`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 7,8,9 | Blocked By: 1

  **References**:
  - Env var convention example: `src-tauri/src/main.rs:337` (`UC_CONFIG_PATH`, `UC_PROFILE`)

  **Acceptance Criteria**:
  - [ ] `cd src-tauri && cargo test -p uc-tauri clipboard_integration_mode` 通过（按实际 test 名称调整）

  **QA Scenarios**:

  ```text
  Scenario: Parse full/passive
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-tauri parse_clipboard_integration_mode
    Expected: tests pass
    Evidence: .sisyphus/evidence/task-2-parse-tests.text

  Scenario: Default to full on invalid
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-tauri
    Expected: tests pass
    Evidence: .sisyphus/evidence/task-2-default-full.text
  ```

  **Commit**: YES | Message: `chore: add UC_CLIPBOARD_MODE resolver` | Files: `src-tauri/crates/uc-tauri/src/bootstrap/clipboard_integration_mode.rs`, `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs`

- [x] 3. 更新双实例 dev 脚本（peerA full / peerB passive）

  **What to do**:
  - 更新 `package.json`：
    - `tauri:dev:peerA` 增加 `UC_CLIPBOARD_MODE=full`（可选但推荐显式）
    - `tauri:dev:peerB` 增加 `UC_CLIPBOARD_MODE=passive`
  - 保持现有 `UC_PROFILE` 与 peerB 的 tauri flags 不变。

  **Must NOT do**:
  - 不要新增 `pkill`/脆弱 kill 逻辑（保持并发脚本稳定）。

  **Recommended Agent Profile**:
  - Category: `quick`
  - Skills: [`executing-plans`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: QA | Blocked By: none

  **References**:
  - Existing scripts: `package.json:11`

  **Acceptance Criteria**:
  - [ ] `bun run tauri:dev:dual` 能启动两个实例（手动 QA）

  **QA Scenarios**:

  ```text
  Scenario: Script sanity
    Tool: Bash
    Steps: bun run tauri:dev:dual
    Expected: both peer processes start; logs show UC_PROFILE a/b
    Evidence: .sisyphus/evidence/task-3-dual-start.text

  Scenario: Env present
    Tool: Bash
    Steps: print process env or add temporary log (no code change) is NOT allowed; instead rely on behavior tests in later tasks
    Expected: peerB behaves passive per later tasks
    Evidence: .sisyphus/evidence/task-3-env-behavior.text
  ```

  **Commit**: YES | Message: `chore: set UC_CLIPBOARD_MODE for dual dev` | Files: `package.json`

- [x] 4. passive 下禁止启动 OS watcher（StartClipboardWatcher gating）

  **What to do**:
  - 修改 `src-tauri/crates/uc-app/src/usecases/start_clipboard_watcher.rs`：
    - `StartClipboardWatcher` 增加字段 `mode: ClipboardIntegrationMode`
    - `new(...)`/`from_port(...)` 增加 `mode` 参数
    - `execute()`：若 `!mode.observe_os_clipboard()`，记录 `info!` 并 `return Ok(())`（不调用 port）
  - 更新调用点（最少 2 处）：
    - `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:631` 的 `usecases().start_clipboard_watcher()`
    - 任何测试/集成用例构造 `StartClipboardWatcher::new(...)`
  - 更新 `start_clipboard_watcher.rs` 内置单元测试：
    - 新增测试：passive 不触发 `MockWatcherControl.start_watcher`

  **Must NOT do**:
  - 不要在 usecase 里读取 env；mode 必须由调用方注入。

  **Recommended Agent Profile**:
  - Category: `unspecified-high` — touches runtime wiring + tests
  - Skills: [`executing-plans`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8 | Blocked By: 1,2

  **References**:
  - Current execute: `src-tauri/crates/uc-app/src/usecases/start_clipboard_watcher.rs:52`
  - Lifecycle always calls watcher: `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs:172`

  **Acceptance Criteria**:
  - [ ] `cd src-tauri && cargo test -p uc-app start_clipboard_watcher` 通过

  **QA Scenarios**:

  ```text
  Scenario: Passive skips start_watcher
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-app start_clipboard_watcher
    Expected: test asserts MockWatcherControl.was_started() == false
    Evidence: .sisyphus/evidence/task-4-passive-skip-watcher.text

  Scenario: Full still starts
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-app test_start_clipboard_watcher_succeeds
    Expected: test asserts started == true
    Evidence: .sisyphus/evidence/task-4-full-start-watcher.text
  ```

  **Commit**: YES | Message: `feat: gate clipboard watcher by integration mode` | Files: `src-tauri/crates/uc-app/src/usecases/start_clipboard_watcher.rs`, `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`

- [x] 5. passive 下禁止 restore 写 OS clipboard（RestoreClipboardSelectionUseCase gating）

  **What to do**:
  - 修改 `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs`：
    - struct 增加 `mode: ClipboardIntegrationMode`
    - `new(...)` 增加 `mode` 参数（建议放最后）
    - `restore_snapshot(...)`：若 `!mode.allow_os_write()`，直接 `return Err(anyhow::anyhow!("System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"))`
      - 注意：不要 `set_next_origin(LocalRestore, ...)`（因为不会写 OS）
  - 更新构造点：`src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:674` 的 `restore_clipboard_selection()` accessor。
  - 增加单元测试：passive 下 `SystemClipboardPort::write_snapshot` 计数为 0，且返回 Err。

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`executing-plans`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: none | Blocked By: 1,2

  **References**:
  - Current restore_snapshot writes OS: `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs:177`
  - Command expects error propagation: `src-tauri/crates/uc-tauri/src/commands/clipboard.rs:333`

  **Acceptance Criteria**:
  - [ ] `cd src-tauri && cargo test -p uc-app restore_snapshot` 通过

  **QA Scenarios**:

  ```text
  Scenario: Passive restore returns explicit error
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-app restore_snapshot
    Expected: test sees Err contains "writes disabled"; write_snapshot not called
    Evidence: .sisyphus/evidence/task-5-restore-disabled.text

  Scenario: Full restore unchanged
    Tool: Bash
    Steps: cd src-tauri && cargo test -p uc-app
    Expected: existing restore tests still pass
    Evidence: .sisyphus/evidence/task-5-restore-full-regression.text
  ```

  **Commit**: YES | Message: `feat: block clipboard restore in passive mode` | Files: `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs`, `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`

#HN|- [x] 6. passive 入站同步：不写 OS clipboard，直接持久化 + in-memory 去重（SyncInboundClipboardUseCase）

**What to do**:

- 修改 `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs`：
  - 新增 `mode: ClipboardIntegrationMode`
  - 新增用于 direct ingest 的 ports（用于构造 `CaptureClipboardUseCase`）：
    - `ClipboardEntryRepositoryPort`
    - `ClipboardEventWriterPort`
    - `SelectRepresentationPolicyPort`
    - `ClipboardRepresentationNormalizerPort`
    - `RepresentationCachePort`
    - `SpoolQueuePort`
  - `execute(...)` 返回值从 `Result<()>` 改为 `Result<SyncInboundOutcome>`：
    - `Noop`
    - `AppliedToSystemClipboard`
    - `Persisted { event_id: EventId }`
  - 行为分支：
    - `Full`：保持现有逻辑（OS read dedupe + set origin + write OS），成功返回 `AppliedToSystemClipboard`
    - `Passive`：
      - 不调用 `local_clipboard.read_snapshot/write_snapshot`
      - 不调用 `clipboard_change_origin.set_next_origin`
      - 去重：基于 `ClipboardMessage.id`（`max_entries=1024`, `ttl=10min`）
        - **实现细节（定案，避免执行者再做取舍）**：
          - 在 `SyncInboundClipboardUseCase` 内新增字段：
            - `recent_ids: tokio::sync::Mutex<std::collections::VecDeque<(String, std::time::Instant)>>`
            - 常量：`RECENT_ID_TTL: Duration = Duration::from_secs(600)`，`RECENT_ID_MAX: usize = 1024`
          - `passive` 分支进入时：
            1. `prune_expired(now)`：循环 pop_front，直到队首 `now - ts <= ttl`
            2. `is_duplicate`：遍历 `VecDeque` 比较 `id`，命中则返回 `SyncInboundOutcome::Noop`
            3. 仅在 **成功持久化** 后 `record(id, now)`：push_back；若超出 `RECENT_ID_MAX` 则 pop_front 直到满足
          - 复杂度 O(n) 但 n≤1024，足够且避免引入额外 crate / HashSet 同步复杂度。
      - 直接构造 `SystemClipboardSnapshot` 并执行 `CaptureClipboardUseCase::execute_with_origin(snapshot, ClipboardChangeOrigin::RemotePush)`
      - 成功返回 `Persisted { event_id }`
- 更新 `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs`：适配新的返回类型（忽略 outcome）。
- 更新 `sync_inbound.rs` 文件内现有单元测试：
  - full 模式测试适配新返回类型
  - 新增 passive 测试：断言不触碰 OS ports 且只持久化一次

**Must NOT do**:

- 不要在 passive 路径打印 plaintext 文本。

**Recommended Agent Profile**:

- Category: `unspecified-high` — touches core usecase + multiple tests
- Skills: [`systematic-debugging`, `executing-plans`]

**Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 7,8 | Blocked By: 1

**References**:

- Current inbound flow: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs:43`
- Capture usecase API: `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs:127`
- Full outbound skip for RemotePush: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs:73`

**Acceptance Criteria**:

- [ ] `cd src-tauri && cargo test -p uc-app sync_inbound` 通过
- [ ] 新增 passive 测试断言：`SystemClipboardPort::write_snapshot` 调用次数为 0

**QA Scenarios**:

```text
Scenario: Passive inbound persists without OS write
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-app sync_inbound::tests
  Expected: new test asserts Persisted outcome; no OS read/write; event/entry repo called
  Evidence: .sisyphus/evidence/task-6-passive-inbound.text

Scenario: Passive dedupe by message id
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-app -- sync_inbound_dedup
  Expected: applying same message twice yields one persistence call
  Evidence: .sisyphus/evidence/task-6-dedupe.text
```

**Commit**: YES | Message: `feat: persist inbound clipboard without OS in passive mode` | Files: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs`, `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs`

#YX|- [x] 7. passive 入站成功时 emit `clipboard://event`（bootstrap receive loop）

**What to do**:

- 修改 `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`：
  - 更新 `new_sync_inbound_clipboard_usecase(deps)`：传入 mode + 新 ports
  - `run_clipboard_receive_loop(...)`：
    - `Ok(outcome)` 时：若 outcome 是 `Persisted { event_id }` 且 `app_handle.is_some()`，调用：
      - `crate::events::forward_clipboard_event(app, ClipboardEvent::NewContent { entry_id: event_id.to_string(), preview: "New clipboard content".to_string() })`
    - 其他 outcome：不 emit（full 模式依赖 watcher emit）
- 增加一个小型纯函数 helper（可选但推荐）将 outcome → `Option<ClipboardEvent>`，便于单测。
- 增加 uc-tauri 单元测试：
  - 使用 `tauri::test::mock_app()` 监听 `clipboard://event`
  - 调用 helper / 或直接模拟 outcome 路径

**Recommended Agent Profile**:

- Category: `unspecified-high` — bootstrap 高风险文件
- Skills: [`systematic-debugging`, `executing-plans`]

**Parallelization**: Can Parallel: YES | Wave 2 | Blocks: final QA | Blocked By: 2,6

**References**:

- Receive loop today: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:1318`
- Clipboard event forwarder: `src-tauri/crates/uc-tauri/src/events/mod.rs:87`

**Acceptance Criteria**:

- [ ] `cd src-tauri && cargo test -p uc-tauri` 中新增测试断言收到 `clipboard://event`

**QA Scenarios**:

```text
Scenario: Persisted outcome emits clipboard event
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-tauri inbound_clipboard_emits
  Expected: test receives event payload with {"type":"NewContent","entry_id":...}
  Evidence: .sisyphus/evidence/task-7-emit.text

Scenario: Full mode does not double-emit
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-tauri
  Expected: no new event emissions on AppliedToSystemClipboard outcome
  Evidence: .sisyphus/evidence/task-7-no-double-emit.text
```

**Commit**: YES | Message: `feat: emit clipboard event for passive inbound apply` | Files: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`

#TM|- [x] 8. 更新 UseCases accessor 构造签名（uc-tauri runtime.rs）

**What to do**:

- 修改 `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`（UseCases impl）：
  - `start_clipboard_watcher()`：传入 `resolve_clipboard_integration_mode()`
  - `restore_clipboard_selection()`：传入 mode
  - `sync_inbound_clipboard()`：传入 mode + 新增 ports
- 确保 mode 解析 helper 被复用（不要重复实现 env 解析）。

**Recommended Agent Profile**:

- Category: `unspecified-high`
- Skills: [`executing-plans`]

**Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 4,5,6 compile | Blocked By: 2,4,5,6

**References**:

- Accessor sites: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:631`, `:674`, `:700`

**Acceptance Criteria**:

- [ ] `cd src-tauri && cargo check -p uc-tauri` 通过

**QA Scenarios**:

```text
Scenario: Compile uc-tauri
  Tool: Bash
  Steps: cd src-tauri && cargo check -p uc-tauri
  Expected: exit code 0
  Evidence: .sisyphus/evidence/task-8-compile.text

Scenario: Tests still build
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-tauri --no-run
  Expected: exit code 0
  Evidence: .sisyphus/evidence/task-8-test-build.text
```

**Commit**: YES | Message: `chore: thread clipboard integration mode through runtime accessors` | Files: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`

#WM|- [x] 9. `sync_clipboard_items` 在 passive 下显式返回 Err

**What to do**:

- 修改 `src-tauri/crates/uc-tauri/src/commands/clipboard.rs:256`：
  - 在命令执行体内 resolve mode：若 `Passive`，直接 `return Err("Clipboard sync disabled in passive mode (UC_CLIPBOARD_MODE=passive)".to_string())`
  - full 模式：保持现有行为（仍返回 `Ok(true)`；失败仅日志 warn）
- （可选）添加单元测试覆盖 passive 分支（若构造 runtime 成本过高，可只测 mode resolver + 一个小 helper）。

**Recommended Agent Profile**:

- Category: `unspecified-high`
- Skills: [`executing-plans`]

**Parallelization**: Can Parallel: YES | Wave 2 | Blocks: final QA | Blocked By: 2

**References**:

- Current command always Ok: `src-tauri/crates/uc-tauri/src/commands/clipboard.rs:267`
- Trace span pattern: `src-tauri/crates/uc-tauri/src/commands/clipboard.rs:260`

**Acceptance Criteria**:

- [ ] 在 `UC_CLIPBOARD_MODE=passive` 时，调用该命令返回 Err（用单测或集成测试证明）

**QA Scenarios**:

```text
Scenario: Passive returns explicit error
  Tool: Bash
  Steps: cd src-tauri && UC_CLIPBOARD_MODE=passive cargo test -p uc-tauri
  Expected: test asserts Err contains "disabled in passive mode"
  Evidence: .sisyphus/evidence/task-9-passive-error.text

Scenario: Full behavior unchanged
  Tool: Bash
  Steps: cd src-tauri && cargo test -p uc-tauri
  Expected: no regressions
  Evidence: .sisyphus/evidence/task-9-full-regression.text
```

**Commit**: YES | Message: `feat: return error for manual sync in passive mode` | Files: `src-tauri/crates/uc-tauri/src/commands/clipboard.rs`

## Final Verification Wave (4 parallel agents, ALL must APPROVE)

- [ ] F1. Plan Compliance Audit — oracle
- [ ] F2. Code Quality Review — unspecified-high
- [ ] F3. Real Manual QA — unspecified-high (dual instance)
- [ ] F4. Scope Fidelity Check — deep

## Commit Strategy

- 建议至少 4 个原子提交（按上面每个任务的 commit 建议）。
- 严格避免把“usecase 行为变化”与“脚本/文档”混在同一提交里。

## Success Criteria

- peerB（passive）不会启动 watcher，不会触碰 OS clipboard write/read，但能接收远端消息并在 UI 列表出现（通过 `clipboard://event` 驱动刷新）。
- peerA（full）行为保持不变。
