---
status: passed
phase: 094-backend-network-allow-relay-fallback
source: [094-VERIFICATION.md]
started: 2026-05-04T13:30:00Z
updated: 2026-05-04T14:30:00Z
completed: 2026-05-04T14:30:00Z
---

## Current Test

[2/2 PASSED — real daemon dual-direction evidence captured from production log]

## Tests

### 1. settings.json 手工添加 `network.allow_relay_fallback: false` 后重启 daemon

expected:

- 启动日志（target=`settings.network`）可见 `disable_relays = true` 字段
- 同 daemon 进程内 `IrohNodeBuilder::bind` 实际以 `RelayMode::Disabled` 模式 bind
- 此时 `endpoint.addr().addrs` 不含 `TransportAddr::Relay` 项

result: **passed** ✅

evidence:

```
2026-05-04T12:15:46.991Z INFO settings.network: applying network.allow_relay_fallback=false → disable_relays=true (builders.rs:209)
2026-05-04T12:15:47.003Z DEBUG iroh::_events::direct_addrs: addrs={DirectAddr 192.168.31.72:52692 Local, DirectAddr 198.18.0.1:52692 Local}
```

- ✓ 配置翻译链路：`network_policy.rs:43` 唯一取反点 + `builders.rs:209` tracing::info! 双确认
- ✓ direct_addrs 仅含 LAN/Local 项，**不含**公网 Qad（QUIC Address Discovery）IP（对比 12:11 反向用例的 `180.164.125.95:59099 Qad`）
- ✓ **关键证据**：12:15 重启之后整段日志**零条** `home is now relay` INFO — 对比 12:11 重启时 `iroh::socket::transports::relay::actor` 在 12:11:13.031 + 12:11:38.721 各注册一次 home relay。这直接证明 `RelayMode::Disabled` 应用到了 endpoint，未尝试注册 home relay
- ✓ 12:16:05+ 与外网 peer `bcb58fce2a` 死循环重试连接（NodeAddr 仅含 relay_url + ip_addresses=[]）持续到 12:16:58 — 我方 LAN-only 拒绝 relay path → 直连不可达 → 上层重试。这正是 NETSET-03 期望的 LAN-only 目标行为

### 2. 反向用例：`allow_relay_fallback: true` 时启动 daemon

expected:

- endpoint 仍可观察到 Relay 候选地址
- 启动日志 `disable_relays = false`
- `endpoint.addr().addrs` 含 `TransportAddr::Relay` 候选

result: **passed** ✅

evidence:

```
2026-05-04T12:11:46.924Z INFO settings.network: applying network.allow_relay_fallback=true → disable_relays=false (builders.rs:209)
2026-05-04T12:11:13.031Z INFO iroh::socket::transports::relay::actor: home is now relay https://aps1-1.relay.n0.iroh-canary.iroh.link/
2026-05-04T12:11:38.721Z INFO iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link/
2026-05-04T12:11:47.992Z DEBUG iroh::_events::direct_addrs: addrs={DirectAddr 180.164.125.95:59099 Qad, DirectAddr 192.168.31.72:49621 Local, ...}
```

- ✓ 配置翻译链路：`builders.rs:209` 反向 log 确认
- ✓ **关键证据**：endpoint 注册了 home relay（`aps1-1` / `euc1-1` 两个候选） — 直接证明 `RelayMode::Default` 应用到了 endpoint，relay actor 启动正常
- ✓ direct_addrs 含公网 Qad IP（`180.164.125.95:59099`），表明 NAT 穿透 + 公网通告能力都在线

## Summary

total: 2
passed: 2
issues: 0
pending: 0
partial: 0
skipped: 0
blocked: 0

## Gaps

### G-094-UAT-01：daemon 缺 `endpoint.addr().addrs` 显式日志

severity: low → **resolved (indirect evidence sufficient)**
type: observability gap
status: closed (2026-05-04T14:30:00Z)

**症状：** Phase 94 success criterion #1 / #2 期望 human 在真实 daemon 启动后能验证 `endpoint.addr().addrs` 含/不含 `TransportAddr::Relay` 候选。当前 daemon tracing 未直接日志 `addr().addrs` 内容。

**resolution：** 间接证据已 sufficient — `iroh::socket::transports::relay::actor` 的 `home is now relay <url>` INFO log 在 RelayMode::Default 时直接出现、在 RelayMode::Disabled 时**完全缺席**，结合 `iroh::_events::direct_addrs` 的 LAN/Local-only addrs 输出，已端到端覆盖 endpoint 的 RelayMode 行为差异。Phase 96（连接通道指示器）会进一步暴露 `ConnectionChannelPort` 给前端，届时 endpoint addrs 会更直观。本 gap 标 closed，不再需要补 ad-hoc log。
