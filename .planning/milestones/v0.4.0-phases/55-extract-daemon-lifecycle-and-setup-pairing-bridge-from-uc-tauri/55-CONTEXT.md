# Phase 55: extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri - Context

**Gathered:** 2026-03-24 (discuss mode)
**Status:** Ready for planning

<domain>
## Phase Boundary

将 `uc-tauri` 中与 Tauri 无关的 daemon 生命周期管理和 setup pairing facade 相关代码提取到 `uc-daemon-client`。

本阶段交付：

- **`daemon_lifecycle.rs`**（340行）：GUI 拥有的 daemon 进程生命周期管理。零 Tauri 依赖。迁移到 `uc-daemon-client/src/daemon_lifecycle.rs`。
- **`setup_pairing_bridge.rs`**（140行）：死代码，Phase 54 已将调用方迁移到 `uc-daemon-client`，直接删除。
- 更新所有调用方的 import 路径。

本阶段不包括：

- 创建新 crate（复用 uc-daemon-client）
- 修改 daemon 业务逻辑（仅移动代码）
- 修改 Phase 56 的 refactor 工作

</domain>

<decisions>
## Implementation Decisions

### Crate 归属

- **D-01:** `daemon_lifecycle.rs` 迁移到 `uc-daemon-client/src/daemon_lifecycle.rs`（与 `DaemonConnectionState` 同 crate）
- **D-02:** `setup_pairing_bridge.rs` 从 `uc-tauri/bootstrap/` 删除（死代码）
- **D-03:** 不创建新 crate，复用现有 `uc-daemon-client`

### terminate_local_daemon_pid 处理

- **D-04:** `terminate_local_daemon_pid()` 函数从 `uc-tauri/bootstrap/run.rs` 移到 `uc-daemon-client/src/daemon_lifecycle.rs`
- **D-05:** `daemon_lifecycle.rs` 在迁移后完全自包含，无跨模块依赖
- **D-06:** `uc-tauri/bootstrap/run.rs` 改为从 `uc-daemon-client` 重新导入 `terminate_local_daemon_pid`

### 迁移策略

- **D-07:** 每步一提交（每逻辑步骤一个 atomic commit）
- **D-08:** 不留 re-export stub（Phase 54 已验证一次性完成可行）
- **D-09:** 删除 dead re-export：`uc-tauri/bootstrap/mod.rs` 中 `setup_pairing_bridge` 相关行

### 测试迁移

- **D-10:** `daemon_lifecycle.rs` 内的 3 个 `#[cfg(test)]` 单元测试随模块迁移到 `uc-daemon-client`
- **D-11:** `uc-tauri/tests/daemon_exit_cleanup.rs` 和 `uc-tauri/tests/daemon_bootstrap_contract.rs` 更新 import：`uc_tauri::bootstrap` → `uc_daemon_client::daemon_lifecycle`

### Claude's Discretion

- `daemon_lifecycle.rs` 内的 `#[cfg(test)]` 内联测试的具体测试桩模式（`spawn_test_child()`）是否需要适配 uc-daemon-client 的测试环境
- uc-daemon-client 模块内部是否需要子模块拆分（daemon_lifecycle + terminate_local_daemon_pid）

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 提取对象（源文件）

- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_lifecycle.rs` — 340行，零 Tauri 依赖
- `src-tauri/crates/uc-tauri/src/bootstrap/setup_pairing_bridge.rs` — 140行，死代码，待删除
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` §terminate_local_daemon_pid — 跨平台进程终止函数，待迁移（line 590-627）

### 目标 crate 结构

- `src-tauri/crates/uc-daemon-client/src/` — daemon_lifecycle.rs 新增落点
- `src-tauri/crates/uc-daemon-client/Cargo.toml` — 确认无 Tauri 依赖

### 调用方（需要更新 import）

- `src-tauri/src/main.rs` — `GuiOwnedDaemonState` 从 `uc_tauri::bootstrap` 迁移到 `uc_daemon_client::daemon_lifecycle`
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` — `terminate_local_daemon_pid` 重新导入，移除本地定义
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — 删除 `daemon_lifecycle` mod 声明和 re-export
- `src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs` — `GuiOwnedDaemonState`、`SpawnReason` import 更新
- `src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs` — `GuiOwnedDaemonState`、`SpawnReason` import 更新

### 先例 phases

- `.planning/phases/54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri/54-CONTEXT.md` — 提取模式先例
- `.planning/phases/46.3-gui-daemon/46.3-CONTEXT.md` — GuiOwnedDaemonState 原始设计背景
- `.planning/phases/46.6-daemon-tauri-tauri-daemon/46.6-CONTEXT.md` — daemon exit cleanup 决策背景

### 关键约束

- Phase 54 的 uc-daemon-client 依赖链：`uc-tauri → uc-daemon-client → uc-daemon`，无循环依赖
- `uc-daemon-client` 已有 `[lib]` section，支持被其他 crate 依赖
- uc-daemon-client 已有 `#[cfg(test)]` 模块模式（参考 `realtime.rs`）

</canonical_refs>

<codebase_context>

## Existing Code Insights

### Reusable Assets

- Phase 54 的一次性迁移模式（不留 re-export stub）已验证可行
- `uc-daemon-client/src/lib.rs` 已有 `pub mod connection; pub mod http; pub mod ws_bridge; pub mod realtime;` — 新增 `pub mod daemon_lifecycle;` 遵循现有模式
- `terminate_local_daemon_pid()` 是纯 Rust 函数，无 Tauri/reqwest/uc-\* 依赖，仅 `std::process::Command` + `std::io::Error`

### Established Patterns

- Crate 提取模式：Phase 40 (uc-bootstrap)、Phase 54 (uc-daemon-client) 建立了先例
- 每步一提交：Phase 54 验证了此模式在多步骤迁移中的可审查性
- 零 Tauri 依赖：daemon_lifecycle.rs 已满足，无需额外修改

### Integration Points

- `uc-tauri/src/main.rs` — `GuiOwnedDaemonState::default()` 创建点（line 332）和 `.manage()` 注册点（line 375）
- `uc-tauri/bootstrap/run.rs` — `terminate_local_daemon_pid` 定义处（line 590），被 `shutdown_owned_daemon` 调用
- `uc-tauri/bootstrap/mod.rs` — `daemon_lifecycle` mod 声明（line 8）和 re-export（line 24）待删除

</codebase_context>

<specifics>
## Specific Ideas

- `terminate_local_daemon_pid` 从 `run.rs` 移到 `daemon_lifecycle.rs` 后，`run.rs` 改为 `pub use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid`
- `setup_pairing_bridge.rs` 删除后，`bootstrap/mod.rs` 中对应的 `pub mod setup_pairing_bridge;` 和 `pub use setup_pairing_bridge::{build_setup_pairing_facade, DaemonBackedSetupPairingFacade};` 一并删除
- uc-daemon-client/src/lib.rs 新增：`pub mod daemon_lifecycle;`
- uc-daemon-client/src/lib.rs 新增 re-export：`pub use daemon_lifecycle::{GuiOwnedDaemonState, OwnedDaemonChild, SpawnReason, DaemonExitCleanupError};`

</specifics>

<deferred>
## Deferred Ideas

- **setup_pairing_bridge.rs 死代码成因复盘** — Phase 54 创建了 uc-daemon-client/realtime.rs 中的内联版本，但没有同步删除 uc-tauri 中的副本。此问题由 Phase 54 的两次独立 plan（P01/P02）引入，single PR chain 可避免
- **Todo #2026-03-21-fix-setup-pairing-confirmation-toast-missing** — 修复 setup 配对确认 toast 缺失（UI 层 bug，与架构无关）

### Reviewed Todos (not folded)

- `2026-03-21-fix-setup-pairing-confirmation-toast-missing` — 作用域是 UI 层（前端 setup 流程），Phase 55 是纯 Rust 架构提取，不涉及前端

</deferred>

---

_Phase: 55-extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri_
_Context gathered: 2026-03-24_
