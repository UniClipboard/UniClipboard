# Phase 51: Peer Discovery Deduplication Fix - Context

**Gathered:** 2026-03-23
**Status:** Ready for planning

<domain>
## Phase Boundary

修复 mDNS 扫描时同一物理设备出现多次的问题。仅修复去重 bug，不扩展到过期 peer 清理或 peer_id 持久化稳定性。

</domain>

<decisions>
## Implementation Decisions

### Bug 复现条件

- **D-01:** Bug 出现在 Setup 配对发现页，开发环境 dual mode 下触发
- **D-02:** 表现为同一台物理机器出现多条记录，但各条记录的设备名和 peer_id 均不同
- **D-03:** 除了真实的 peerA 之外，还出现了一个未知设备，可能是 peerB 的 daemon 通过 mDNS 发现了自己

### 去重策略

- **D-04:** 后端单层根治——在 daemon/app 层找出根因并修复，前端不需要额外去重处理
- **D-05:** 不在前端加防御性去重，信任后端返回的数据正确性

### Scope

- **D-06:** 仅修复 mDNS 扫描重复问题，不扩展到过期 peer 清理、peer_id 持久化稳定性等相关但独立的改进

### Claude's Discretion

- 具体根因定位和修复方案由 researcher 调查确定——可能的方向包括：
  - GUI (Tauri) 进程自身的 libp2p 网络实例与 daemon 的 libp2p 实例互相发现
  - dual mode 下多个 daemon 进程使用不同 identity store 导致多 peer_id
  - `GetP2pPeersSnapshot` 聚合逻辑中的合并问题
  - mDNS 缓存中残留过期条目

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Peer Discovery Infrastructure

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — PeerCaches HashMap 结构、mDNS 事件处理、local_peer_id 自过滤逻辑、regression tests（staleness invariants）
- `src-tauri/crates/uc-core/src/ports/peer_directory.rs` — PeerDirectoryPort trait（get_discovered_peers, get_connected_peers）
- `src-tauri/crates/uc-core/src/ports/discovery.rs` — DiscoveryPort trait
- `src-tauri/crates/uc-core/src/network/events.rs` — DiscoveredPeer struct、NetworkEvent enum

### Peer Snapshot Aggregation

- `src-tauri/crates/uc-app/src/usecases/pairing/get_p2p_peers_snapshot.rs` — GetP2pPeersSnapshot use case，合并 discovered + connected + paired 数据源
- `src-tauri/crates/uc-daemon/src/api/query.rs` — daemon HTTP query endpoint，调用 GetP2pPeersSnapshot

### Realtime Event Path

- `src-tauri/crates/uc-app/src/realtime/peers_consumer.rs` — peers.changed 事件映射和转发
- `src-tauri/crates/uc-daemon/src/api/ws.rs` — daemon WebSocket 事件广播
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` — DaemonWsBridge peers.changed 翻译

### Frontend

- `src/hooks/useDeviceDiscovery.ts` — 前端 peer 列表状态管理，peers.changed 事件消费

### Identity & Bootstrap

- `src-tauri/crates/uc-platform/src/identity_store.rs` — FileIdentityStore、load_or_create_identity
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — wire_dependencies 中 identity_store 注入逻辑
- `src-tauri/crates/uc-daemon/src/workers/peer_discovery.rs` — daemon PeerDiscoveryWorker

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `PeerCaches.discovered_peers: HashMap<String, DiscoveredPeer>` — 已按 peer_id 去重的后端缓存
- `GetP2pPeersSnapshot` — 统一的 peer 聚合 use case，GUI 和 CLI 共用
- libp2p mDNS 层的 `local_peer_id` 过滤 — 已有自过滤逻辑（line 1550, 1589）
- `list_sendable_peers_excludes_local_peer_id` — 已有的 regression test

### Established Patterns

- 后端 HashMap 缓存保证单 peer_id 唯一性
- `peers.changed` realtime event 是 peer 列表变更的唯一前端通道
- mDNS discovered/expired 事件对通过 `apply_mdns_discovered` / `apply_mdns_expired` 管理 peer 生命周期

### Integration Points

- GUI (Tauri) 和 daemon 各自通过 `wire_dependencies` → `FileIdentityStore` 构建独立的 libp2p 网络实例
- `PeerDiscoveryWorker` 在 daemon 中处理 mDNS 事件并广播 device name
- `DaemonWsBridge` 将 daemon 的 `peers.changed` 翻译为前端事件

### Key Investigation Areas

- GUI 进程是否仍独立启动 libp2p（可能与 daemon 的 libp2p 互相发现）
- dual mode 下 identity store 路径隔离是否正确
- `GetP2pPeersSnapshot` 合并多数据源时是否可能引入重复

</code_context>

<specifics>
## Specific Ideas

- Bug 在开发环境 dual mode 下最容易复现（peerA/peerB 双终端）
- 用户主要关注 Setup 配对发现页的体验——扫描到的设备列表应准确反映局域网内的真实设备
- 未知设备可能是 daemon 自发现——需要调查 GUI + daemon 双 libp2p 实例共存的影响

</specifics>

<deferred>
## Deferred Ideas

- **过期 peer 清理** — mDNS 过期后 peer 残留问题是独立议题，不在此 phase 范围内
- **peer_id 持久化稳定性** — daemon 重启后 peer_id 是否稳定取决于 identity store 持久化，是独立改进
- **前端防御性去重** — 虽然不在此 phase 实施，如果后端根治后仍有边缘情况，可在未来 phase 添加

</deferred>

---

_Phase: 51-peer-discovery-deduplication_
_Context gathered: 2026-03-23_
