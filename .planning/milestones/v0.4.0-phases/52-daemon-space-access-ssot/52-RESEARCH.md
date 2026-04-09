# Phase 52: Daemon as Single Source of Truth for Space Access State - Research

**Researched:** 2026-03-23
**Domain:** Rust/Tauri — daemon WebSocket event pipeline, GUI orchestrator removal, realtime state sync
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** 完全移除 GUI 端的 `SpaceAccessOrchestrator`。GUI 进程不再持有任何 space access 状态副本。所有状态由 daemon 单独管理。
- **D-02:** 前端通过 WebSocket 事件推送获取 daemon space access 状态变更。初始化时通过 daemon HTTP API 查询一次当前状态作为初始快照。
- **D-03:** 复用现有 `DaemonWsBridge` 路径将 daemon WebSocket 事件翻译为 Tauri 前端事件。
- **D-04:** 不做 space access 状态持久化。Space access 是短暂交互式流程，daemon 重启后对端会因超时/连接断开感知失败。
- **D-05:** Daemon 重启后 space access state 直接重置为 `SpaceAccessState::Idle`（现有行为保持不变）。
- **D-06:** 新增 `space_access.state_changed` WebSocket 事件类型，携带完整 `SpaceAccessState` 快照。与 `peers.changed` 事件模式一致——推送完整快照而非增量 diff。
- **D-07:** 通过现有 DaemonWsBridge 路径转发，前端通过 Tauri event listener 消费。

### Claude's Discretion

- daemon HTTP API 中查询当前 space access state 的具体 endpoint 设计
- `space_access.state_changed` 事件中除 state 外是否携带额外 metadata（如 session_id）
- GUI 端移除 orchestrator 后的代码清理范围和编译适配
- DaemonWsBridge 中事件翻译的具体实现

### Deferred Ideas (OUT OF SCOPE)

- Space access 状态持久化（短暂交互流程，重启重置即可）
- Space access 流程超时恢复（daemon 重启后对进行中流程的主动通知/清理，当前依赖对端超时即可）
  </user_constraints>

---

## Summary

Phase 52 重设计 daemon ↔ GUI 之间的 space access 状态同步机制，将 daemon 确立为唯一状态源。当前 GUI 进程持有自己的 `SpaceAccessOrchestrator` 实例，导致状态存在双源问题。本阶段移除 GUI 端 orchestrator，增加 daemon → GUI 的 WebSocket 推送通道，GUI 初始化时通过 HTTP 拉取快照，后续由事件驱动。

本阶段的技术工作可分为三层：（1）daemon 侧：在 `DaemonApiEventEmitter` / `DaemonPairingHost` 中在 `SpaceAccessState` 变更时广播 `space_access.state_changed` WS 事件；（2）bridge 侧：在 `daemon_ws_bridge.rs` 的 `map_daemon_ws_event` 函数中增加对新事件的翻译；（3）GUI/前端侧：移除 `GuiBootstrapContext.space_access_orchestrator` 字段，新增 HTTP 查询 endpoint，前端订阅 `RealtimeEvent::SpaceAccessStateChanged`。

`peers.changed` / `setup.state_changed` 是完整的参考模式，本阶段的实现应与之对齐，保持代码一致性。

**Primary recommendation:** 严格复用 `peers.changed` 完整快照推送模式，在 daemon 侧的 `SpaceAccessOrchestrator::dispatch` 后通过 `event_tx` 广播 `space_access.state_changed`，GUI 端对 orchestrator 的所有引用替换为 daemon HTTP 查询 + WS 事件订阅。

---

## Standard Stack

本阶段是纯架构重构，不引入新依赖。使用现有栈：

### Core（现有，直接复用）

| 组件                                         | 版本/位置                                          | 用途                                                  |
| -------------------------------------------- | -------------------------------------------------- | ----------------------------------------------------- |
| `SpaceAccessState`                           | `uc-core/src/security/space_access/state.rs`       | 已 derive `Serialize/Deserialize`，可直接用于事件传输 |
| `SpaceAccessOrchestrator`                    | `uc-app/src/usecases/space_access/orchestrator.rs` | daemon 侧保留，GUI 侧移除                             |
| `DaemonApiEventEmitter`                      | `uc-daemon/src/api/event_emitter.rs`               | 现有事件广播机制，需扩展                              |
| `DaemonWsBridge`                             | `uc-tauri/src/bootstrap/daemon_ws_bridge.rs`       | 现有翻译路径，需增加新事件处理                        |
| `DaemonQueryService`                         | `uc-daemon/src/api/query.rs`                       | HTTP 查询服务，需扩展新 endpoint                      |
| `RealtimeTopic` / `RealtimeEvent`            | `uc-core/src/ports/realtime.rs`                    | 需新增 `SpaceAccess` topic 和事件类型                 |
| `DaemonApiState`                             | `uc-daemon/src/api/server.rs`                      | 需扩展 `space_access_orchestrator` 字段               |
| broadcast channel (`tokio::sync::broadcast`) | Tokio                                              | 现有 `event_tx: broadcast::Sender<DaemonWsEvent>`     |

---

## Architecture Patterns

### 已验证的参考模式：peers.changed 完整快照

`peers.changed` 事件完整展示了本阶段需要复用的模式：

**daemon 侧广播（`DaemonPairingHost` 中触发）：**

```rust
// Source: src-tauri/crates/uc-daemon/src/api/event_emitter.rs
let _ = self.event_tx.send(DaemonWsEvent {
    topic: "space-access".to_string(),
    event_type: "space_access.state_changed".to_string(),
    session_id: None,
    ts: chrono::Utc::now().timestamp_millis(),
    payload: serde_json::to_value(SpaceAccessStateChangedPayload {
        state: new_state,
    }).unwrap_or_default(),
});
```

**WebSocket topic 注册（`ws.rs`）：**

```rust
// 新增 topic 常量
const TOPIC_SPACE_ACCESS: &str = "space-access";
const SPACE_ACCESS_SNAPSHOT_EVENT: &str = "space_access.snapshot";
const SPACE_ACCESS_STATE_CHANGED_EVENT: &str = "space_access.state_changed";

// 在 is_supported_topic 中新增
TOPIC_SPACE_ACCESS

// 在 build_snapshot_event 中新增
TOPIC_SPACE_ACCESS => snapshot_event(
    TOPIC_SPACE_ACCESS,
    SPACE_ACCESS_SNAPSHOT_EVENT,
    None,
    state.query_service.space_access_state().await?,
)
.map(Some),
```

**bridge 翻译（`daemon_ws_bridge.rs` `map_daemon_ws_event`）：**

```rust
"space_access.state_changed" => {
    serde_json::from_value::<SpaceAccessStateChangedPayload>(event.payload)
        .ok()
        .map(|payload| RealtimeEvent::SpaceAccessStateChanged(SpaceAccessStateChangedEvent {
            state: payload.state,
        }))
}
```

**`RealtimeTopic` 扩展（`uc-core/src/ports/realtime.rs`）：**

```rust
pub enum RealtimeTopic {
    Pairing,
    Peers,
    PairedDevices,
    Setup,
    SpaceAccess,  // 新增
}
```

**`RealtimeEvent` 扩展：**

```rust
pub enum RealtimeEvent {
    // ... 现有 ...
    SpaceAccessStateChanged(SpaceAccessStateChangedEvent),  // 新增
}

pub struct SpaceAccessStateChangedEvent {
    pub state: SpaceAccessState,  // 完整快照
}
```

### HTTP 查询 Endpoint 模式

参考 `/status`、`/peers` 的 GET 模式，在 `routes.rs` 新增：

```rust
.route("/space-access/state", get(space_access_state))
```

Handler 通过 `DaemonApiState.space_access_orchestrator` 查询：

```rust
async fn space_access_state(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let orchestrator = match state.space_access_orchestrator() {
        Some(o) => o,
        None => return internal_error(anyhow::anyhow!("space access not available")).into_response(),
    };
    Json(SpaceAccessStateResponse {
        state: orchestrator.get_state().await,
    }).into_response()
}
```

### 事件触发时机

**关键决策：何时广播 `space_access.state_changed`**

`SpaceAccessOrchestrator::dispatch()` 内部更新状态后，需要通过 event_tx 广播。最干净的方式是在 `DaemonPairingHost` 中包装对 orchestrator 的调用，在每次 dispatch 返回后广播：

```rust
// src-tauri/crates/uc-daemon/src/pairing/host.rs
// 在每次调用 orchestrator.dispatch() 后：
let new_state = self.space_access_orchestrator.dispatch(...).await?;
self.broadcast_space_access_state(new_state).await;

fn broadcast_space_access_state(&self, state: SpaceAccessState) {
    let payload = SpaceAccessStateChangedPayload { state };
    let _ = self.event_tx.send(DaemonWsEvent {
        topic: "space-access".to_string(),
        event_type: "space_access.state_changed".to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload: serde_json::to_value(payload).unwrap_or_default(),
    });
}
```

### Recommended Project Structure（变更涉及文件）

```
src-tauri/
├── crates/uc-core/src/ports/realtime.rs          # 新增 SpaceAccess topic/event
├── crates/uc-daemon/src/api/
│   ├── types.rs                                   # 新增 SpaceAccessStateChangedPayload, SpaceAccessStateResponse
│   ├── routes.rs                                  # 新增 /space-access/state endpoint
│   ├── ws.rs                                      # 新增 TOPIC_SPACE_ACCESS 支持
│   ├── query.rs                                   # 新增 space_access_state() 方法
│   └── server.rs                                  # 新增 space_access_orchestrator 字段
├── crates/uc-daemon/src/pairing/host.rs           # 在 dispatch 后广播 state_changed
├── crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs  # 新增事件翻译
├── crates/uc-bootstrap/src/
│   ├── builders.rs                                # 移除 GuiBootstrapContext.space_access_orchestrator
│   └── assembly.rs                                # 清理 GUI 路径的 orchestrator 创建
src-tauri/src/main.rs                              # 移除 space_access_orchestrator 绑定和传入
src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs  # 移除 space_access_completion 后台任务
```

### Anti-Patterns to Avoid

- **不要在 GUI 侧保留任何 SpaceAccessOrchestrator 实例**：哪怕是只读查询也不行，应通过 daemon HTTP endpoint。
- **不要在 DaemonApiEventEmitter 中直接引用 orchestrator**：emitter 只负责序列化广播，状态由 PairingHost 在 dispatch 后传入。
- **不要用增量 diff 模式**：参考 D-06，始终推送完整状态快照，简化前端状态管理。

---

## Don't Hand-Roll

| Problem       | Don't Build            | Use Instead                                    | Why                                     |
| ------------- | ---------------------- | ---------------------------------------------- | --------------------------------------- |
| WS 事件广播   | 新的 broadcast channel | 复用 `DaemonApiState.event_tx`                 | 已有线程安全 broadcast sender，全局共享 |
| WS 事件翻译   | 新的翻译层             | 在 `map_daemon_ws_event` 中加 match arm        | 现有函数专门负责此工作                  |
| HTTP 状态查询 | 新的 HTTP 客户端或传输 | 扩展 `DaemonQueryService` + `routes.rs`        | 现有模式直接可扩展                      |
| 前端状态订阅  | 新的实时连接           | 扩展 `DaemonWsBridge.subscribe()` 订阅新 topic | bridge 已实现 topic 过滤                |

**Key insight:** 本阶段的所有基础设施已经存在。工作量集中在：（1）接通新的事件类型到现有管道；（2）从 GUI 路径中移除不再需要的引用。

---

## Common Pitfalls

### Pitfall 1: DaemonApiState 中缺少 space_access_orchestrator 字段

**What goes wrong:** HTTP endpoint 和 WS snapshot 无法访问 orchestrator 实例。
**Why it happens:** `DaemonApiState` 目前只有 `pairing_host` 和 `setup_orchestrator`，没有 space access 字段。
**How to avoid:** 参考 `with_setup()` 方法，在 `DaemonApiState` 增加 `space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>` 字段和 `with_space_access()` builder 方法，在 `DaemonApp::run()` 中传入。
**Warning signs:** 编译器报错 `no field space_access_orchestrator on DaemonApiState`。

### Pitfall 2: 移除 GUI 端 orchestrator 后的编译错误链

**What goes wrong:** `GuiBootstrapContext.space_access_orchestrator` 字段被多处引用：

- `src-tauri/src/main.rs:327` (destructuring)
- `src-tauri/src/main.rs:593` (传入 `start_background_tasks`)
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:106,238` (函数签名和内部使用)
- `src-tauri/crates/uc-bootstrap/src/assembly.rs:885,895,913,980,1044,1070,1194`
- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs:539,638` (测试 fixture)

**Why it happens:** 字段在多层依赖中流通，移除一处会导致级联编译错误。
**How to avoid:** 自上而下逐层移除：先 `wiring.rs` 中的 `space_access_completion` 后台任务 → 再 `builders.rs` 中的字段 → 再 `assembly.rs` 中的创建逻辑 → 最后 `main.rs` 的 destructuring。
**Warning signs:** `cargo check` 输出多个 `cannot find value space_access_orchestrator` 错误。

### Pitfall 3: WS 事件 topic 名称不一致

**What goes wrong:** `DaemonApiEventEmitter` 广播时用的 topic 与 `ws.rs` 中注册的 `is_supported_topic` 不一致，导致事件被过滤丢弃。
**Why it happens:** 两处各自定义 topic 字符串常量，容易拼写不一致。
**How to avoid:** 在 `uc-daemon/src/api/ws.rs` 中定义常量，`event_emitter.rs` 和 `query.rs` 中用相同值（或从 ws.rs re-export）。topic 名称建议使用 `"space-access"`（与 `"pairing"`, `"peers"` 的命名风格一致）。
**Warning signs:** 订阅后没有收到任何 WS 事件，但 DaemonApiEventEmitter 的日志显示事件已发送。

### Pitfall 4: RealtimeTopic 枚举扩展遗漏 match arm

**What goes wrong:** 在 `daemon_ws_bridge.rs` 的 `topic_name()` 或 `event_topic()` 函数中，`RealtimeTopic::SpaceAccess` 未处理，导致 `unreachable!` panic 或编译警告。
**Why it happens:** Rust 的 match 穷尽性检查在开启 `#[warn(non_exhaustive_patterns)]` 时才提示，有时被忽略。
**How to avoid:** 添加新 topic 后立即检查所有 match `RealtimeTopic` 的位置（共 2 处：`topic_name` 和 `event_topic`）。
**Warning signs:** `cargo check` 报 `non-exhaustive patterns` 警告或 `match arm not covered`。

### Pitfall 5: 前端初始化时序问题

**What goes wrong:** 前端 WS 订阅完成前，GUI 初始化的 HTTP 拉取已经返回，之后 daemon 状态发生变化但前端尚未处于监听状态，导致状态漂移。
**Why it happens:** D-02 要求"初始化时通过 HTTP 查询一次当前状态"，但如果先 HTTP 后订阅，中间有竞争窗口。
**How to avoid:** 参考 `setupRealtimeStore.ts` 中的模式——先建立 WS 订阅（`onSpaceAccessStateChanged`），再进行 HTTP 初始化查询并用结果更新本地状态（订阅的初始快照事件会覆盖初始化时可能遗漏的变更）。实际上 WS 的 subscribe 握手本身会触发 snapshot 事件，可以作为初始状态的来源，无需额外 HTTP 调用。
**Warning signs:** 偶发性前端 space access UI 停留在过期状态。

---

## Code Examples

### 1. SpaceAccessStateChangedPayload DTO（新增到 types.rs）

```rust
// Source: 参照 SetupStateChangedPayload 模式 (src-tauri/crates/uc-daemon/src/api/types.rs:108)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceAccessStateChangedPayload {
    pub state: SpaceAccessState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceAccessStateResponse {
    pub state: SpaceAccessState,
}
```

### 2. RealtimeEvent 扩展（新增到 realtime.rs）

```rust
// Source: 参照 SetupStateChangedEvent 模式 (src-tauri/crates/uc-core/src/ports/realtime.rs:82)
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceAccessStateChangedEvent {
    pub state: SpaceAccessState,
}

pub enum RealtimeEvent {
    // ... 现有 ...
    SpaceAccessStateChanged(SpaceAccessStateChangedEvent),
}

pub enum RealtimeTopic {
    // ... 现有 ...
    SpaceAccess,
}
```

### 3. DaemonWsBridge event_topic / topic_name 扩展

```rust
// Source: daemon_ws_bridge.rs (lines 726-749)
fn event_topic(event: &RealtimeEvent) -> RealtimeTopic {
    match event {
        // ... 现有 ...
        RealtimeEvent::SpaceAccessStateChanged(_) => RealtimeTopic::SpaceAccess,
    }
}

fn topic_name(topic: &RealtimeTopic) -> &'static str {
    match topic {
        // ... 现有 ...
        RealtimeTopic::SpaceAccess => "space-access",
    }
}
```

### 4. 移除 GUI 端 space_access_completion 后台任务（wiring.rs）

```rust
// 删除以下代码块（lines 237-262 in wiring.rs）:
// // --- Space access completion loop ---
// let completion_orchestrator = space_access_orchestrator.clone();
// registry
//     .spawn("space_access_completion", |token| async move { ... })
//     .await;
```

### 5. 前端订阅模式（参考 setupRealtimeStore.ts 模式）

```typescript
// 新增到 src/api/setup.ts 或新文件 src/api/spaceAccess.ts
export async function onSpaceAccessStateChanged(
  callback: (state: SpaceAccessState) => void
): Promise<() => void> {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== 'space-access' || event.type !== 'space_access.stateChanged') {
      return
    }
    callback((event.payload as { state: SpaceAccessState }).state)
  })
}
```

---

## State of the Art

| Old Approach                                          | Current Approach                                  | Impact                                     |
| ----------------------------------------------------- | ------------------------------------------------- | ------------------------------------------ |
| GUI 持有独立的 SpaceAccessOrchestrator 实例           | daemon 作为唯一状态源，GUI 通过 WS 获取           | 消除双源不一致，GUI 重启后自动恢复正确状态 |
| GUI 通过本地 orchestrator 查询状态                    | GUI 通过 daemon HTTP GET /space-access/state 查询 | 状态查询路径统一，不受 GUI 本地状态影响    |
| Space access completion 通过 GUI-local event bus 传递 | Daemon 广播 `space_access.state_changed` WS 事件  | 事件不再依赖 GUI 进程存活                  |

---

## Open Questions

1. **`space_access.state_changed` 事件是否需要携带 session_id？**
   - What we know: D-06 说"除 state 外是否携带额外 metadata（如 session_id）"属于 Claude's Discretion
   - What's unclear: `SpaceAccessState` enum 的 `WaitingOffer`、`WaitingUserPassphrase` 等变体已内含 `pairing_session_id`，所以 event level 的 session_id 是否冗余？
   - Recommendation: 不需要在 event 顶层携带 session_id，`SpaceAccessState` 本身已有足够信息。保持 payload 简洁：`{ state: SpaceAccessState }`。

2. **前端 space access 状态消费的具体位置？**
   - What we know: `src/api/setup.ts` 已有 `onSpaceAccessCompleted` 消费 `setup.spaceAccessCompleted` 事件；`setupRealtimeStore.ts` 消费它并调用 `handleSpaceAccessCompleted()`
   - What's unclear: 新的 `space_access.state_changed` 事件是否需要前端专门的 store，或者可以直接集成到现有 `setupRealtimeStore.ts`
   - Recommendation: 根据 D-01，GUI 不再持有 space access 状态，所以如果现有前端代码只是在 `handleSpaceAccessCompleted` 时转发给 setup orchestrator，则前端可能不需要维护独立的 space access state store——只需通过新事件触发现有的 `handleSpaceAccessCompleted` 调用逻辑即可。这属于实现时需要确认的范围。

---

## Environment Availability

Step 2.6: SKIPPED（纯代码重构，无外部工具依赖，Rust/Tauri 工具链已验证存在于项目中）

---

## Validation Architecture

### Test Framework

| Property           | Value                                                          |
| ------------------ | -------------------------------------------------------------- |
| Framework          | Rust cargo test + Vitest (TypeScript)                          |
| Config file        | `src-tauri/` (cargo test), `vitest.config.ts`                  |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon -- space_access 2>&1` |
| Full suite command | `cd src-tauri && cargo test && bun test`                       |

### Phase Requirements → Test Map

本阶段无显式 requirement ID 映射，但需覆盖以下行为：

| Behavior                                                    | Test Type | Automated Command                                               | Notes                                      |
| ----------------------------------------------------------- | --------- | --------------------------------------------------------------- | ------------------------------------------ |
| `space_access.state_changed` WS 事件在 daemon 侧被广播      | unit      | `cd src-tauri && cargo test -p uc-daemon -- space_access`       | 参照 `DaemonApiEventEmitter` 现有测试模式  |
| `DaemonWsBridge` 正确翻译 `space_access.state_changed` 事件 | unit      | `cd src-tauri && cargo test -p uc-tauri -- space_access`        | 参照 `daemon_ws_bridge.rs` 现有 peers 测试 |
| GUI 端 `space_access_orchestrator` 字段不再存在             | compile   | `cd src-tauri && cargo check`                                   | 编译不再引用相关字段即通过                 |
| `GET /space-access/state` 返回正确状态                      | unit      | `cd src-tauri && cargo test -p uc-daemon -- space_access_state` | 参照 setup HTTP 路由测试                   |

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/tests/` 中新增 `space_access_ws_event` 测试（覆盖 DaemonApiEventEmitter 广播行为）
- [ ] `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` 新增 `space_access_state_changed` 翻译测试（现有 fixture 需清理 `space_access_orchestrator` 字段）

---

## Sources

### Primary (HIGH confidence)

- 代码直读：`src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — DaemonApiEventEmitter 广播模式
- 代码直读：`src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` — map_daemon_ws_event 翻译模式，Subscriber 分发
- 代码直读：`src-tauri/crates/uc-daemon/src/api/ws.rs` — topic 注册、snapshot 构建、WS 握手
- 代码直读：`src-tauri/crates/uc-core/src/ports/realtime.rs` — RealtimeTopic/RealtimeEvent 定义
- 代码直读：`src-tauri/crates/uc-core/src/security/space_access/state.rs` — SpaceAccessState 已 derive Serialize/Deserialize
- 代码直读：`src-tauri/crates/uc-bootstrap/src/builders.rs` — GuiBootstrapContext.space_access_orchestrator 字段位置
- 代码直读：`src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:238` — space_access_completion 后台任务
- 代码直读：`src-tauri/src/main.rs:327,593` — main.rs 中的 orchestrator 引用点
- 代码直读：`src/store/setupRealtimeStore.ts` — 前端 setup realtime 订阅参考模式

### Secondary (MEDIUM confidence)

- 上下文文件 `52-CONTEXT.md` — 用户确认的决策和范围边界（HIGH）
- `STATE.md` 历史决策记录 — 相关 Phase 46.x 决策的架构背景

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — 完全基于代码直读，无需外部文档
- Architecture: HIGH — 参考模式（`peers.changed`, `setup.state_changed`）已在项目中验证
- Pitfalls: HIGH — 基于代码中的实际引用点分析，编译器会强制执行大部分约束

**Research date:** 2026-03-23
**Valid until:** 60 days（稳定架构，无外部依赖）
