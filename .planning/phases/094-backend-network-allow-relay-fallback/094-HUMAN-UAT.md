---
status: partial
phase: 094-backend-network-allow-relay-fallback
source: [094-VERIFICATION.md]
started: 2026-05-04T13:30:00Z
updated: 2026-05-04T13:30:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. settings.json 手工添加 `network.allow_relay_fallback: false` 后重启 daemon

expected:
- 启动日志（target=`settings.network`）可见 `disable_relays = true` 字段
- 同 daemon 进程内 `IrohNodeBuilder::bind` 实际以 `RelayMode::Disabled` 模式 bind
- 此时 `endpoint.addr().addrs` 不含 `TransportAddr::Relay` 项（等价 Tier B 自动断言 `relay_disabled_publishes_no_relay_addrs`）

result: [pending]

why_human: ROADMAP success criterion #1 显式要求 daemon 启动后端到端验证；Plan 06 的 Tier B integration test 与 Plan 05 的 truth-table 单测分别覆盖了 endpoint 行为与配置翻译，但端到端 "settings.json → 重启 daemon → 日志 + endpoint 行为联合可观察" 链路只能由 human 在真实 daemon 启动场景中验证（Tier C 手工抓包/日志查看）。

### 2. 反向用例：`allow_relay_fallback: true` 或缺 `network` 段时启动 daemon

expected:
- endpoint 仍可观察到 Relay 候选地址
- 启动日志 `disable_relays = false`
- `endpoint.addr().addrs` 含 `TransportAddr::Relay` 候选

result: [pending]

why_human: ROADMAP success criterion #1 同条款；CI 环境 / Relay mesh 连通性不可靠（PATTERNS.md §11 critical finding 3 已锁定），所以 Plan 06 Tier B 的 `relay_default_binds_without_panic` 仅断 "bind 不 panic" 弱不等式，不强断 Relay 候选必须存在 — 真实存在性必须由 human 在公网可达的环境中验证。

## Summary

total: 2
passed: 0
issues: 0
pending: 2
skipped: 0
blocked: 0

## Gaps
