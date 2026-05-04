---
status: partial
phase: 094-backend-network-allow-relay-fallback
source: [094-VERIFICATION.md]
started: 2026-05-04T13:30:00Z
updated: 2026-05-04T14:20:00Z
---

## Current Test

[2/2 tests config-translation portion captured; endpoint addrs verification pending — daemon does not currently log `endpoint.addr().addrs`]

## Tests

### 1. settings.json 手工添加 `network.allow_relay_fallback: false` 后重启 daemon

expected:

- 启动日志（target=`settings.network`）可见 `disable_relays = true` 字段
- 同 daemon 进程内 `IrohNodeBuilder::bind` 实际以 `RelayMode::Disabled` 模式 bind
- 此时 `endpoint.addr().addrs` 不含 `TransportAddr::Relay` 项（等价 Tier B 自动断言 `relay_disabled_publishes_no_relay_addrs`）

result: partial — log evidence captured, endpoint addrs verification pending

evidence:

```
2026-05-04 12:15:46.991  INFO settings.network: crates/uc-bootstrap/src/builders.rs:209: applying network.allow_relay_fallback=false → disable_relays=true allow_relay_fallback=false disable_relays=true
```

- ✓ tracing 日志确认 `network_policy.rs:43` 唯一取反点行为：`allow_relay_fallback=false → disable_relays=true`
- ✓ `builders.rs:209` 装配点 tracing::info! 字段值取自 `iroh_config.disable_relays`（Pattern A — 不在装配点内联反转）
- ⏳ daemon 当前没有日志 endpoint.addr().addrs 内容（tracing 未覆盖此点）；endpoint 不含 Relay 候选的断言由 Plan 06 Tier B integration test `relay_disabled_publishes_no_relay_addrs` 在 CI 中自动覆盖

why_human: ROADMAP success criterion #1 显式要求 daemon 启动后端到端验证；Plan 06 的 Tier B integration test 与 Plan 05 的 truth-table 单测分别覆盖了 endpoint 行为与配置翻译，但端到端 "settings.json → 重启 daemon → 日志 + endpoint 行为联合可观察" 链路只能由 human 在真实 daemon 启动场景中验证（Tier C 手工抓包/日志查看）。

### 2. 反向用例：`allow_relay_fallback: true` 或缺 `network` 段时启动 daemon

expected:

- endpoint 仍可观察到 Relay 候选地址
- 启动日志 `disable_relays = false`
- `endpoint.addr().addrs` 含 `TransportAddr::Relay` 候选

result: partial — log evidence captured, endpoint addrs verification pending

evidence:

```
2026-05-04 12:11:46.924  INFO settings.network: crates/uc-bootstrap/src/builders.rs:209: applying network.allow_relay_fallback=true → disable_relays=false allow_relay_fallback=true disable_relays=false
```

- ✓ tracing 日志确认默认值/显式 true 的行为：`allow_relay_fallback=true → disable_relays=false`
- ⏳ daemon 当前没有日志 endpoint.addr().addrs Relay 候选；CI 环境 / Relay mesh 连通性不可靠（PATTERNS.md §11 critical finding 3 已锁定），Plan 06 Tier B 的 `relay_default_binds_without_panic` 也仅断 "bind 不 panic" 弱不等式

why_human: ROADMAP success criterion #1 同条款；CI 环境 / Relay mesh 连通性不可靠（PATTERNS.md §11 critical finding 3 已锁定），所以 Plan 06 Tier B 的 `relay_default_binds_without_panic` 仅断 "bind 不 panic" 弱不等式，不强断 Relay 候选必须存在 — 真实存在性必须由 human 在公网可达的环境中验证。

## Summary

total: 2
passed: 0
issues: 0
pending: 0
partial: 2
skipped: 0
blocked: 0

## Gaps

### G-094-UAT-01：daemon 缺 endpoint.addr().addrs 启动日志

severity: low
type: observability gap
status: open

**症状：** Phase 94 success criterion #1 / #2 期望 human 在真实 daemon 启动后能验证 `endpoint.addr().addrs` 含/不含 `TransportAddr::Relay` 候选。当前 daemon tracing 未覆盖此点，human 只能通过 Tier B integration test 间接信任（不是端到端独立观察）。

**配置翻译链路（builders.rs:209 tracing）已 captured 双向证据，可信任。**

**潜在补救（不在 Phase 94 范围）：**

- 在 `uc-bootstrap::build_space_setup_assembly` bind 完成后追加一条 tracing::debug! 打 `endpoint.addr().addrs` 摘要（addrs.len + 是否含 Relay）
- 或在 daemon HTTP `/health` / `/peers/self` 端点暴露 endpoint addrs（前端可看，无需开 trace）

**记到 todos / Phase 96**：连接通道指示器本身需要 `ConnectionChannelPort` + `IrohConnectionChannelAdapter`（ROADMAP Phase 96 描述），届时 endpoint addrs 自然可观察。这条 gap 等到 Phase 96 自然偿还，不需要 Phase 94 单独修。
