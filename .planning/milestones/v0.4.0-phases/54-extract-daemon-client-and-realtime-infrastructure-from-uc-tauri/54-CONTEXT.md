# Phase 54: Extract daemon client and realtime infrastructure from uc-tauri - Context

**Gathered:** 2026-03-23
**Status:** Ready for planning

<domain>
## Phase Boundary

将 `uc-tauri` 中与 Tauri 无关的 daemon 通信层提取到新建的 `uc-daemon-client` crate。包括：

- **HTTP 客户端**：`daemon_client/` 全部 4 个文件（pairing、query、setup、authorized_daemon_request 工具函数）
- **WebSocket 桥接**：`bootstrap/daemon_ws_bridge.rs`（852 行，tokio-tungstenite，零 Tauri 依赖）
- **Realtime 运行时**：`bootstrap/realtime_runtime.rs`（285 行，启动 WS 消费者任务，零 Tauri 依赖）
- **连接状态**：`DaemonConnectionState`（从 `bootstrap/runtime.rs` 中提取）

**In scope:**

- 新建 `uc-daemon-client` workspace crate
- 搬移上述文件到新 crate，不留 re-export stub
- 更新所有调用方的 import 路径（uc-tauri commands、main.rs、tests）
- 更新 Cargo workspace 配置
- `uc-tauri` 直接依赖 `uc-daemon-client`
- 重命名 `TauriDaemonPairingClient` → `DaemonPairingClient` 等（去掉 Tauri 前缀）

**Out of scope:**

- daemon_lifecycle.rs 提取（Phase 55）
- setup_pairing_bridge.rs 提取（Phase 55）
- wiring.rs / run.rs 的部分提取（第四优先级，未排期）
- EventHub 泛化（Issue #316，等第二个场景触发）

</domain>

<decisions>
## Implementation Decisions

### 新 crate 归属

- **D-01:** 新建 `uc-daemon-client` crate，位于 `src-tauri/crates/uc-daemon-client/`
- **D-02:** 职责单一——"与 daemon 通信"（HTTP + WebSocket）
- **D-03:** 不放进 uc-daemon（daemon 是服务端，不应包含客户端代码）
- **D-04:** 不放进 uc-bootstrap（uc-bootstrap 职责是"组装依赖"，不是"运行时通信"）

### Crate 结构

- **D-05:** 目录结构如下：
  ```
  src-tauri/crates/uc-daemon-client/
  ├── Cargo.toml
  └── src/
      ├── lib.rs
      ├── connection.rs     ← DaemonConnectionState + helpers
      ├── http/
      │   ├── mod.rs           ← authorized_daemon_request
      │   ├── pairing.rs       ← DaemonPairingClient（重命名去 Tauri 前缀）
      │   ├── query.rs         ← DaemonQueryClient
      │   └── setup.rs         ← DaemonSetupClient
      ├── ws_bridge.rs      ← DaemonWsBridge
      └── realtime.rs       ← start_realtime_runtime
  ```
- **D-06:** 依赖：`uc-core`, `uc-app`, `uc-daemon`（for DaemonConnectionInfo）, `reqwest`, `tokio-tungstenite`, `tokio`, `futures-util`, `async-trait`, `serde`, `serde_json`, `anyhow`, `tracing`

### DaemonConnectionState 归属

- **D-07:** `DaemonConnectionState` 搬到 `uc-daemon-client/src/connection.rs`
- **D-08:** `uc-daemon-client` 依赖 `uc-daemon`（lib）获取 `DaemonConnectionInfo` 类型
- **D-09:** 依赖链：`uc-tauri → uc-daemon-client → uc-daemon (lib)`

### 迁移策略

- **D-10:** 一次性完成，不留 re-export stub
- **D-11:** 所有调用方（commands、main.rs、tests）在同一个 phase 内更新 import 路径
- **D-12:** 每个逻辑步骤一个 atomic commit（创建 crate → 搬移代码 → 更新调用方 → 清理）

### 依赖方向

- **D-13:** `uc-tauri` 直接在 Cargo.toml 中依赖 `uc-daemon-client`
- **D-14:** commands 直接 `use uc_daemon_client::http::DaemonPairingClient` 等

### Claude's Discretion

- 具体的 module 内部组织细节（如 ws_bridge 内部的子模块拆分）
- test 文件的归属（跟随源码搬移还是保留在 uc-tauri tests/）
- DaemonWsBridgeConfig 是否跟随 ws_bridge 搬移

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 提取对象（源文件）

- `src-tauri/crates/uc-tauri/src/daemon_client/mod.rs` — authorized_daemon_request 工具函数
- `src-tauri/crates/uc-tauri/src/daemon_client/pairing.rs` — TauriDaemonPairingClient（316 行）
- `src-tauri/crates/uc-tauri/src/daemon_client/query.rs` — TauriDaemonQueryClient（62 行）
- `src-tauri/crates/uc-tauri/src/daemon_client/setup.rs` — TauriDaemonSetupClient（225 行）
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` — DaemonWsBridge（852 行）
- `src-tauri/crates/uc-tauri/src/bootstrap/realtime_runtime.rs` — start_realtime_runtime + install_daemon_setup_pairing_facade（285 行）
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` §DaemonConnectionState — 连接状态类型定义

### 关键调用方（需要更新 import 的文件）

- `src-tauri/src/main.rs` — DaemonConnectionState 创建、realtime_runtime 调用
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` — TauriDaemonPairingClient 使用
- `src-tauri/crates/uc-tauri/src/commands/setup.rs` — TauriDaemonSetupClient 使用
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` — DaemonConnectionState 使用
- `src-tauri/crates/uc-tauri/src/bootstrap/setup_pairing_bridge.rs` — DaemonConnectionState + TauriDaemonPairingClient
- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` — DaemonWsBridge 测试
- `src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs` — DaemonConnectionState 测试
- `src-tauri/crates/uc-tauri/tests/daemon_command_shell.rs` — DaemonConnectionState 测试

### 先例 phases

- `.planning/phases/40-uc-bootstrap-crate/40-CONTEXT.md` — crate 提取模式先例
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — 类型搬移模式先例

### DaemonConnectionInfo 定义

- `src-tauri/crates/uc-daemon/src/api/auth.rs` — DaemonConnectionInfo 结构体定义

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- Phase 40 的 re-export stub 模式已验证，但本次选择一次性完成
- `uc-daemon` 已有 `[lib]` section，支持被其他 crate 依赖

### Established Patterns

- Cargo workspace crate 创建模式：参考 `uc-bootstrap`、`uc-daemon` 的 Cargo.toml 格式
- `DaemonConnectionState` 使用 `Arc<RwLock<Option<DaemonConnectionInfo>>>` 模式，支持运行时动态更新 daemon URL/token

### Integration Points

- `uc-tauri/Cargo.toml` 需要新增 `uc-daemon-client` 依赖，可能移除 `reqwest` 和 `tokio-tungstenite` 直接依赖（如果只有 daemon_client 使用这些）
- `src-tauri/Cargo.toml`（workspace）需要注册新 crate member
- `uc-daemon-client` 需要依赖 `uc-daemon`（lib）获取 `DaemonConnectionInfo` 类型

</code_context>

<specifics>
## Specific Ideas

- 重命名去掉 "Tauri" 前缀：`TauriDaemonPairingClient` → `DaemonPairingClient`，`TauriDaemonQueryClient` → `DaemonQueryClient`，`TauriDaemonSetupClient` → `DaemonSetupClient`
- 最终目标是让 uc-tauri 仅作为 GUI 运行时层，此 phase 是系列提取的第一步

</specifics>

<deferred>
## Deferred Ideas

- EventHub 泛化（Issue #316）——等第二个需要 Hub 的场景出现时触发
- daemon_lifecycle.rs 提取 → Phase 55
- setup_pairing_bridge.rs 提取 → Phase 55
- wiring.rs / run.rs / file_transfer_wiring.rs 部分提取——第四优先级，未排期

</deferred>

---

_Phase: 54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri_
_Context gathered: 2026-03-23_
