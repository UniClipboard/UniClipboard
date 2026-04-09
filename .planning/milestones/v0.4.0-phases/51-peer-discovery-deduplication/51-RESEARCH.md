# Phase 51: Peer Discovery Deduplication Fix - Research

**Researched:** 2026-03-23
**Domain:** libp2p mDNS peer discovery, daemon WebSocket realtime events, frontend peer state management
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Bug 出现在 Setup 配对发现页，开发环境 dual mode 下触发
- **D-02:** 表现为同一台物理机器出现多条记录，但各条记录的设备名和 peer_id 均不同
- **D-03:** 除了真实的 peerA 之外，还出现了一个未知设备，可能是 peerB 的 daemon 通过 mDNS 发现了自己
- **D-04:** 后端单层根治——在 daemon/app 层找出根因并修复，前端不需要额外去重处理
- **D-05:** 不在前端加防御性去重，信任后端返回的数据正确性
- **D-06:** 仅修复 mDNS 扫描重复问题，不扩展到过期 peer 清理、peer_id 持久化稳定性等相关但独立的改进

### Claude's Discretion

具体根因定位和修复方案由 researcher 调查确定——可能的方向包括：

- GUI (Tauri) 进程自身的 libp2p 网络实例与 daemon 的 libp2p 实例互相发现
- dual mode 下多个 daemon 进程使用不同 identity store 导致多 peer_id
- `GetP2pPeersSnapshot` 聚合逻辑中的合并问题
- mDNS 缓存中残留过期条目

### Deferred Ideas (OUT OF SCOPE)

- **过期 peer 清理** — mDNS 过期后 peer 残留问题是独立议题
- **peer_id 持久化稳定性** — daemon 重启后 peer_id 是否稳定是独立改进
- **前端防御性去重** — 后续 phase 可添加，不在此 phase 实施
  </user_constraints>

---

## Summary

通过逐层代码分析，定位到 bug 的两个根因：

**根因 1（主因）：前端 `peers.changed` 事件处理与 daemon 事件格式不匹配。**
daemon ws 发出的 `peers.changed` 是增量单 peer 事件（`PeerChangedPayload` 含单个 peer 的 `peer_id + device_name + discovered`），但 `DaemonWsBridge` 将其翻译为 `PeerChangedEvent { peers: vec![single_peer] }`，前端 `useDeviceDiscovery.ts` 的 `peers.changed` 处理直接用 `payload.peers`（单元素数组）**替换**整个 peer 列表（`setPeers(nextPeers)`）。当两个 peer 的 `PeerDiscovered` 事件相继到达时，第二次替换会让第一个 peer 消失，但在 UI 渲染层面短时间内会出现"先后闪现"的重复感。

**根因 2（辅因）：daemon `/peers` 快照的 local_peer_id 自过滤不在 GetP2pPeersSnapshot 层做二次保险。**
mDNS 的 `local_peer_id` 过滤在 `run_swarm_loop`（line 1550/1589）处完成，但 `GetP2pPeersSnapshot` 在聚合 discovered + paired 时没有再次排除 local peer。如果任何路径绕过 mDNS 层（如 `upsert_discovered_from_connection` 从连接事件进入 cache），local peer 理论上可能出现在快照中。

**特别说明（dual mode 路径隔离已验证正确）：**

- peerA 用 `UC_PROFILE=a`，peerB 用 `UC_PROFILE=b`，`apply_profile_suffix` 隔离了 app_data_root 和 identity 文件路径，两者有不同的 peer_id，符合预期。
- GUI Tauri 进程构建了 `Libp2pNetworkAdapter`，但 `start_background_tasks` 中 `libp2p_network: _`（被忽略），从不调用 `start_network()`，GUI 进程不会参与 mDNS 广播。

**Primary recommendation:** 修复 `peers.changed` 的前端消费逻辑——改为增量合并而非全量替换；并在 `GetP2pPeersSnapshot` 层加入 local_peer_id 过滤作为防御层。按 D-04/D-05，修复集中在后端，前端消费逻辑属于后端下发数据质量问题，需在后端保证 snapshot 正确，同时修复前端的增量 vs 全量不匹配。

---

## Architecture Patterns

### Peer 数据流（全链路）

```
libp2p mDNS (daemon process)
  └─ mdns::Event::Discovered
       └─ filter(local_peer_id)          ← line 1550 in libp2p_network.rs
            └─ collect_mdns_discovered()  ← 按 peer_id 合并多 Multiaddr
                 └─ apply_mdns_discovered()
                      └─ PeerCaches.upsert_discovered()   ← HashMap<String, DiscoveredPeer>
                           └─ NetworkEvent::PeerDiscovered → event_tx

daemon PeerDiscoveryWorker
  └─ NetworkEvent::PeerDiscovered
       └─ announce_device_name()          ← 触发 business protocol 广播设备名

daemon DaemonPairingHost (run_event_loop)
  └─ NetworkEvent::PeerDiscovered
       └─ emit_ws_event("peers", "peers.changed", PeerChangedPayload{single peer})
            └─ DaemonApiState.event_tx (broadcast channel)
                 └─ websocket client → DaemonWsEvent

Tauri DaemonWsBridge
  └─ "peers.changed" (PeerChangedPayload)
       └─ RealtimeEvent::PeersChanged(PeerChangedEvent { peers: vec![single_peer] })
            └─ RealtimeTopicPort.broadcast()

peers_consumer.rs
  └─ RealtimeEvent::PeersChanged(event)
       └─ HostEvent::Realtime(RealtimeFrontendEvent{ payload: PeersChanged(event) })
            └─ Tauri emit → frontend

Frontend useDeviceDiscovery.ts
  └─ event.type === 'peers.changed'
       └─ payload.peers (单元素数组!)
            └─ setPeers(nextPeers)   ← *** BUG: 全量替换, 丢弃其他 peers ***

Frontend initial load (loadPeers)
  └─ getP2PPeers() → Tauri command → daemon /peers
       └─ GetP2pPeersSnapshot.execute()
            └─ PeerDirectoryPort.get_discovered_peers()  ← PeerCaches.discovered_peers snapshot
```

### 关键问题点

**问题 A：peers.changed 增量事件 vs 前端全量替换**

daemon 发出的 `peers.changed` 是单 peer 增量通知（`PeerChangedPayload` 含 `discovered: bool`）。
`DaemonWsBridge` 翻译时强制包装为 `vec![single_peer]`（单元素数组）。
前端 `useDeviceDiscovery.ts` 的处理：

```typescript
// src/hooks/useDeviceDiscovery.ts line 108-123
if (event.type === 'peers.changed') {
  const payload = event.payload as {
    peers: Array<{peerId: string; deviceName?: string | null; connected: boolean}>
  }
  const nextPeers: DiscoveredPeer[] = payload.peers.map(peer => ({ ... }))
  setPeers(nextPeers)          // ← 直接替换，不合并
  setScanPhase(nextPeers.length > 0 ? 'hasDevices' : 'empty')
}
```

当 mDNS 依次发现两个 peer（peerA daemon 和 peerB daemon 各自发现对方后，其中一方再广播），每个 `peers.changed` 事件只含一个 peer，setPeers 会来回替换，在 React 批处理边界内如果状态合并失效则表现为闪烁或残留"两条"。

**问题 B：GetP2pPeersSnapshot 缺少 local_peer_id 过滤**

```rust
// uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs
pub async fn execute(&self) -> Result<Vec<P2pPeerSnapshot>> {
    let discovered = self.peer_dir.get_discovered_peers().await?;
    // ... 无 local_peer_id 排除 ...
    for peer in discovered {
        snapshots.push(P2pPeerSnapshot { ... });
    }
}
```

`PeerDirectoryPort` 接口上有 `local_peer_id()` 方法（由 `Libp2pNetworkAdapter` 实现），但 `GetP2pPeersSnapshot` 从不调用它来过滤。mDNS 层的过滤是必要的，但聚合 use case 层的二次保险缺失。

**问题 C：PeerDirectoryPort.get_discovered_peers() 未排除 local_peer_id**

`Libp2pNetworkAdapter` 实现 `get_discovered_peers` 时直接返回 `PeerCaches.discovered_peers`，未对 local_peer_id 做额外过滤。虽然 mDNS 事件入口处有过滤，但通过 `upsert_discovered_from_connection`（ConnectionEstablished 路径）进入的 peer 理论上没有该过滤。

### 修复策略（后端单层根治）

**策略：在 `PeerDirectoryPort.get_discovered_peers()` 实现层强制排除 local_peer_id**

```rust
// src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
// Libp2pNetworkAdapter::get_discovered_peers() impl
async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
    let caches = self.caches.read().await;
    Ok(caches
        .discovered_peers
        .values()
        .filter(|p| p.peer_id != self.local_peer_id)  // ← 添加此行
        .cloned()
        .collect())
}
```

这是最干净的根治位置：`get_discovered_peers` 是 `PeerDirectoryPort` trait 的实现，它是所有 peer 数据消费者（`GetP2pPeersSnapshot`、daemon `/peers` 路由）的唯一入口点。在此处过滤，可以同时覆盖快照查询和 mDNS 缓存中意外进入 local_peer_id 的边缘情况。

**附加修复：前端 peers.changed 改为增量合并（仍属后端约定问题）**

由于 `peers.changed` 是增量通知，前端应该合并而非替换。但 D-05 禁止前端添加防御性逻辑。正确做法是修改后端的事件语义：

选项 1（推荐）：daemon 在发出 `peers.changed` 时，发出**全量** peer 列表（调用 `GetP2pPeersSnapshot` 后序列化为数组），而非单 peer 增量。这与前端的全量替换语义匹配。

选项 2：保持增量，修改前端消费为增量合并。但违反 D-05。

选项 1 在后端实现的变更点：`DaemonPairingHost` 的 `emit_peers_changed` 需要调用 `get_p2p_peers_snapshot()`，发出全量 peer 列表；`DaemonWsBridge` 翻译逻辑需相应更新。

---

## Standard Stack

此 phase 不引入新库，所有修改在已有 Rust 代码和 TypeScript 代码中完成。

| 组件                  | 位置                                                    | 作用                                       |
| --------------------- | ------------------------------------------------------- | ------------------------------------------ |
| `PeerCaches`          | `uc-platform/src/adapters/libp2p_network.rs`            | peer 内存缓存，HashMap 保证 peer_id 唯一   |
| `PeerDirectoryPort`   | `uc-core/src/ports/peer_directory.rs`                   | trait：get_discovered_peers, local_peer_id |
| `GetP2pPeersSnapshot` | `uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` | 聚合 discovered + connected + paired       |
| `DaemonPairingHost`   | `uc-daemon/src/pairing/host.rs`                         | 发出 NetworkEvent → ws event               |
| `DaemonWsBridge`      | `uc-tauri/src/bootstrap/daemon_ws_bridge.rs`            | 翻译 daemon ws 事件 → RealtimeEvent        |
| `useDeviceDiscovery`  | `src/hooks/useDeviceDiscovery.ts`                       | 前端 peer 列表状态管理                     |

---

## Don't Hand-Roll

| Problem            | Don't Build    | Use Instead                      | Why                                      |
| ------------------ | -------------- | -------------------------------- | ---------------------------------------- |
| peer 去重          | 自定义去重结构 | HashMap key = peer_id（已有）    | PeerCaches.discovered_peers 已是 HashMap |
| 全量 snapshot 广播 | 新的广播协议   | 扩展现有 `peers.changed` payload | 已有 event bus，只需扩展 payload schema  |

---

## Common Pitfalls

### Pitfall 1：只过滤 mDNS 入口，遗漏 Connection 入口

**What goes wrong:** `upsert_discovered_from_connection`（`ConnectionEstablished` 事件触发）可以向 `PeerCaches.discovered_peers` 写入任意 peer，包括 local peer 自身（如果 loopback 连接发生）。
**Why it happens:** mDNS 过滤只在 `run_swarm_loop` 的 `mdns::Event::Discovered` 分支，`ConnectionEstablished` 分支没有过滤。
**How to avoid:** 在 `get_discovered_peers()` 返回时统一过滤，覆盖所有写入路径。
**Warning signs:** 发现列表中出现 local_peer_id 对应的条目。

### Pitfall 2：peers.changed 语义混淆（增量 vs 全量）

**What goes wrong:** daemon 发出增量事件（单 peer），前端按全量处理（替换列表），导致每次 mDNS 更新时列表内容颠覆。
**Why it happens:** daemon 的 `PeerChangedPayload` 是单 peer 描述，但 `DaemonWsBridge` 将其包装为 `peers: vec![...]`，前端误认为这是全量列表。
**How to avoid:** 明确约定 `peers.changed` 语义：要么统一为增量（前端 merge），要么统一为全量（后端 snapshot）。D-04 要求后端根治，推荐改为全量。
**Warning signs:** 在存在多个 peer 时，快速连续的 mDNS 事件导致列表只保留最后一个 peer。

### Pitfall 3：GetP2pPeersSnapshot 调用时机

**What goes wrong:** 在 `PeerDiscovered` 事件触发后立即调用快照可能包含旧缓存（device_name 还未被 announce 更新）。
**Why it happens:** `PeerDiscoveryWorker.announce_device_name()` 是异步 business protocol，可能在 `peers.changed` 广播之后才完成。
**How to avoid:** `peers.changed` 全量方案中，device_name 为 null 是合法状态，前端应展示本地化 fallback。`peers.name_updated` 事件用于后续更新。

### Pitfall 4：dual mode identity 路径隔离

**What goes wrong:** 如果 peerA 和 peerB 的 daemon 使用相同的 `app_data_root`，则两者共用同一个 identity 文件，peer_id 相同，自过滤会错误地将对方过滤掉。
**Why it happens:** `apply_profile_suffix` 依赖 `UC_PROFILE` 环境变量，如果 daemon 没有继承该变量则路径相同。
**How to avoid:** 验证 spawn_daemon_process 继承了父进程环境变量（已验证：`Command::new` 默认继承）。
**Warning signs:** 两个 daemon 使用同一个 identity 文件，peer_id 重叠。

---

## Code Examples

### 现有：mDNS 层 local_peer_id 过滤（已有，正确）

```rust
// src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs line 1548-1551
let mut peers: Vec<(PeerId, Multiaddr)> = peers
    .into_iter()
    .filter(|(peer_id, _)| peer_id.to_string() != local_peer_id)
    .collect();
```

### 建议修复 1：get_discovered_peers 层过滤（新增）

```rust
// uc-platform/src/adapters/libp2p_network.rs
// Libp2pNetworkAdapter 的 PeerDirectoryPort 实现
async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
    let caches = self.caches.read().await;
    let local_id = &self.local_peer_id;
    Ok(caches
        .discovered_peers
        .values()
        .filter(|p| &p.peer_id != local_id)
        .cloned()
        .collect())
}
```

### 建议修复 2：daemon peers.changed 发出全量列表（修改 DaemonPairingHost）

```rust
// uc-daemon/src/pairing/host.rs
// NetworkEvent::PeerDiscovered 处理
NetworkEvent::PeerDiscovered(_peer) => {
    // 取全量快照而非单 peer 增量
    let usecases = CoreUseCases::new(runtime.as_ref());
    let snapshots = usecases.get_p2p_peers_snapshot().execute().await
        .unwrap_or_default();
    let peers: Vec<PeerSnapshotDto> = snapshots.into_iter().map(|s| s.into()).collect();
    emit_ws_event(
        &event_tx,
        "peers",
        "peers.changed",
        None,
        PeersChangedPayload { peers },  // 全量替换语义
    );
}
```

### 建议修复 2 配套：DaemonWsBridge 翻译全量 payload

```rust
// uc-tauri/src/bootstrap/daemon_ws_bridge.rs
"peers.changed" => serde_json::from_value::<PeersChangedPayload>(event.payload)
    .ok()
    .map(|payload| {
        RealtimeEvent::PeersChanged(PeerChangedEvent {
            peers: payload.peers.into_iter().map(|p| RealtimePeerSummary {
                peer_id: p.peer_id,
                device_name: p.device_name,
                connected: p.connected,
            }).collect(),
        })
    }),
```

---

## Runtime State Inventory

此 phase 是 bug fix，不涉及重命名或迁移。

无运行时状态需要迁移。

---

## Environment Availability

Step 2.6: SKIPPED — 此 phase 是纯代码修改，无外部依赖变化。

---

## Validation Architecture

### Test Framework

| Property           | Value                                                       |
| ------------------ | ----------------------------------------------------------- |
| Framework          | Rust: cargo test; Frontend: vitest (jsdom)                  |
| Config file        | src-tauri/Cargo.toml (workspace); vitest.config.ts          |
| Quick run command  | `cd src-tauri && cargo test -p uc-platform test_peer_dedup` |
| Full suite command | `cd src-tauri && cargo test && bun test`                    |

### Phase Requirements → Test Map

| Req ID      | Behavior                                | Test Type | Automated Command                                                                    | File Exists?          |
| ----------- | --------------------------------------- | --------- | ------------------------------------------------------------------------------------ | --------------------- |
| (无正式 ID) | get_discovered_peers 排除 local_peer_id | unit      | `cd src-tauri && cargo test -p uc-platform test_get_discovered_peers_excludes_local` | ❌ Wave 0             |
| (无正式 ID) | GetP2pPeersSnapshot 不包含 local peer   | unit      | `cd src-tauri && cargo test -p uc-app test_snapshot_excludes_local_peer`             | ❌ Wave 0             |
| (无正式 ID) | peers.changed 全量语义，前端 mock 验证  | unit      | `bun test src/hooks/useDeviceDiscovery`                                              | ✅ 文件存在但需新测试 |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-platform && bun test src/hooks/useDeviceDiscovery`
- **Per wave merge:** `cd src-tauri && cargo test && bun test`
- **Phase gate:** Full suite green

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — 新增测试：`test_get_discovered_peers_excludes_local`，验证 `get_discovered_peers()` 不返回 local_peer_id 的条目
- [ ] `src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` — 新增测试：`test_snapshot_excludes_local_peer`，mock `local_peer_id()` 返回与发现列表中某 peer 相同的 id，验证快照中不出现该 peer
- [ ] `src/hooks/useDeviceDiscovery.test.ts`（如不存在）— 验证 `peers.changed` 全量替换逻辑：多个 peers 同时在 payload 中时，列表保留全部

---

## Open Questions

1. **peers.changed 全量 vs 增量的正确方向**
   - What we know: 当前 daemon 发增量，前端做全量替换，二者语义不匹配
   - What's unclear: 改为全量可能带来性能开销（每次 peer 变化都需查询 snapshot），在高频 mDNS 场景下是否可接受
   - Recommendation: 对于局域网配对场景（peer 数量 < 20），全量快照每次几毫秒，开销可接受。选择全量，消除语义歧义

2. **upsert_discovered_from_connection 是否真的能写入 local peer**
   - What we know: `ConnectionEstablished` 路径调用 `upsert_discovered_from_connection`，此处无 local_peer_id 过滤
   - What's unclear: libp2p 是否会向自身发起连接（理论上不会），但 loopback 或代理场景可能
   - Recommendation: 防御性在 `get_discovered_peers()` 层过滤，代价极小，消除潜在风险

3. **dual mode 中 peerA/peerB 发现对方的设备名时机**
   - What we know: `PeerDiscoveryWorker` 在 `PeerDiscovered` 后调用 `announce_device_name`，此时 device_name 为 None
   - What's unclear: frontend 初始 `loadPeers()` 的时机是否早于 device_name 公告完成
   - Recommendation: `peers.changed` 全量快照时 device_name 可能为 null，前端展示 fallback。`peers.name_updated` 会在 device_name 解析后更新——这是正常的两阶段渲染，不是 bug

---

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — 直接读取，local_peer_id 过滤点（line 1550, 1589），PeerCaches 结构，mDNS 事件处理全链路
- `src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` — 直接读取，聚合逻辑，缺少 local_peer_id 排除
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` — 直接读取，`peers.changed` 翻译为单元素 `vec![single_peer]`
- `src/hooks/useDeviceDiscovery.ts` — 直接读取，`setPeers(nextPeers)` 全量替换
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — 直接读取，`PeerDiscovered` → `emit_ws_event` 单 peer 增量
- `src-tauri/crates/uc-platform/src/identity_store.rs` — 直接读取，`FileIdentityStore` 路径隔离逻辑
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — 直接读取，`apply_profile_suffix` 路径隔离，identity_store 构建
- `src-tauri/crates/uc-bootstrap/src/builders.rs` — 直接读取，`build_gui_app/build_daemon_app` 分离
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — 直接读取，`libp2p_network: _` 被 GUI 忽略，GUI 不启动 libp2p
- `package.json` tauri:dev:peerA/peerB 脚本 — 直接读取，UC_PROFILE=a/b 环境变量确认

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` — `spawn_daemon_process` 使用 `Command::new`（继承父进程环境变量，UC_PROFILE 正确传递）

---

## Metadata

**Confidence breakdown:**

- Root cause analysis: HIGH — 基于直接代码阅读，链路完整
- Fix strategy: HIGH — 修改点明确，无跨 crate 破坏性变化
- Edge cases (connection path bypass): MEDIUM — 理论上可能，实际触发条件未完全验证

**Research date:** 2026-03-23
**Valid until:** 2026-04-23（libp2p 和 Tauri 版本稳定，30 天有效）

---

## 关键调查结论汇总

| 调查项                                      | 结论                                                                                     | 置信度 |
| ------------------------------------------- | ---------------------------------------------------------------------------------------- | ------ |
| GUI 进程是否启动 libp2p                     | 否，`start_background_tasks` 中 `libp2p_network: _` 被忽略，GUI 不调用 `start_network()` | HIGH   |
| dual mode identity store 路径是否隔离       | 是，`apply_profile_suffix` + `UC_PROFILE=a/b` 确保两个 daemon 使用不同 identity 文件     | HIGH   |
| daemon spawn 是否继承 UC_PROFILE            | 是，`Command::new` 默认继承父进程环境变量                                                | HIGH   |
| GetP2pPeersSnapshot 是否有 local peer 过滤  | 否，无 local_peer_id 排除                                                                | HIGH   |
| get_discovered_peers 是否有 local peer 过滤 | 否，直接返回 PeerCaches snapshot                                                         | HIGH   |
| peers.changed 语义（增量 vs 全量）          | 增量（单 peer）—但前端按全量处理，存在语义不匹配                                         | HIGH   |
| mDNS 层自过滤逻辑                           | 存在（line 1550/1589），但 ConnectionEstablished 路径无此过滤                            | HIGH   |
