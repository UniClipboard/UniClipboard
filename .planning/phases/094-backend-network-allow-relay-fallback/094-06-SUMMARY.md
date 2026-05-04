---
phase: 094-backend-network-allow-relay-fallback
plan: 06
subsystem: uc-infra / iroh
tags:
  - uc-infra
  - iroh
  - lan-only
  - integration-test
  - pitfall-3-defense
requirements:
  - NETSET-03
provides:
  - "uc-infra::IrohNodeBuilder::bind 加 OnceCell BIND_LOCK 进程级单次守护（Pitfall 3 结构性防御）"
  - "Tier B 自动化（D-C1 两组用例 — Disabled 强不等式 / Default 弱不等式）"
  - "uc-infra cargo feature `test-util` — 下游 e2e 显式启用以 elide BIND_LOCK 守护"
requires:
  - "iroh 0.98 Endpoint::builder().relay_mode().bind() API（已在项目 lockfile）"
  - "std::sync::OnceLock（stdlib，无新依赖）"
affects:
  - "src-tauri/crates/uc-infra/src/network/iroh/node.rs（加守护）"
  - "src-tauri/crates/uc-infra/Cargo.toml（加 [features].test-util = []）"
  - "src-tauri/crates/uc-bootstrap/Cargo.toml（dev-deps 启用 uc-infra/test-util）"
key-files:
  created:
    - "src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs"
  modified:
    - "src-tauri/crates/uc-infra/src/network/iroh/node.rs"
    - "src-tauri/crates/uc-infra/Cargo.toml"
    - "src-tauri/crates/uc-bootstrap/Cargo.toml"
decisions:
  - "选分支 B（cargo feature 修订版）—— 用 #[cfg(not(any(test, feature = \"test-util\")))] 替代 plan 锁定的纯 #[cfg(not(test))]"
  - "下游 crate 通过 dev-deps 启用 uc-infra/test-util feature elide 守护（替代 #[cfg(test)] 的不可传递性）"
  - "Default 用例只断 'no panic' 弱不等式（D-C1 + PATTERNS.md §11 critical finding 3）"
metrics:
  duration: "~10 分钟"
  task-count: 2
  file-count: 4
  loc-added: ~100
  commits: 2
threat-flags: []
---

# Phase 94 Plan 06: OnceCell BIND_LOCK + LAN-only RelayMode integration tests Summary

One-liner: `IrohNodeBuilder::bind` 加 std::sync::OnceLock 单进程守护 + Tier B integration test 自动断言 RelayMode::Disabled 路径下 endpoint 不发布 Relay 候选地址（NETSET-03）。

## Tasks Completed

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | IrohNodeBuilder::bind OnceCell BIND_LOCK 守护（Pitfall 3 结构性防御） | `faee71f7` | uc-infra/src/network/iroh/node.rs, uc-infra/Cargo.toml, uc-bootstrap/Cargo.toml |
| 2 | uc-infra/tests/lan_only_relay_mode.rs（D-C1 两组用例 Tier B 自动化） | `97dd38f7` | uc-infra/tests/lan_only_relay_mode.rs |

## What Was Built

### Task 1: BIND_LOCK 进程级单次 bind 守护

**文件改动：**
- `uc-infra/src/network/iroh/node.rs`（+25 lines）：
  - 顶部 `use std::sync::OnceLock;`（条件 import — `#[cfg(not(any(test, feature = "test-util")))]`）
  - `IrohNodeBuilder` 类型上方 `static BIND_LOCK: OnceLock<()> = OnceLock::new();`（同条件守护）
  - `bind()` 方法体起始 `BIND_LOCK.set(()).expect("...Pitfall 3...")` —— 第二次 set 必 panic
- `uc-infra/Cargo.toml`（+9 lines）：
  - `[features]` 新增 `test-util = []`（默认空，production 严禁开启）
- `uc-bootstrap/Cargo.toml`（+5 lines）：
  - `[dev-dependencies]` 新增 `uc-infra = { path = "../uc-infra", features = ["test-util"] }` 让 e2e 编译时 elide 守护

**为何最终选 cargo feature 而非 plan 锁定的纯 #[cfg(not(test))]：**

Plan 锁定方案是 `#[cfg(not(test))]` 守护，假设 test build 下 elided。**实测发现这违反 Rust cfg 传递性规则**：`#[cfg(test)]` 只对**正在 `cargo test` 的 crate** 生效，不传递到下游依赖。`uc-bootstrap/tests/slice*_e2e.rs` 编译时使用的是 `uc-infra` 的 production build —— 原始 `#[cfg(not(test))]` 守护激活 → 5 个 e2e binary panic（已实测）。

修复（Rule 1 — 修复 plan 锁定方案的潜在 bug）：把守护改成 `#[cfg(not(any(test, feature = "test-util")))]`，并在 uc-infra 加 `test-util` feature；下游 crate 在 dev-dependencies 中启用 feature 即可 elide 守护。这是 Rust 标准工程模式，编译期决定，**不**引入运行时分支或运行时热切换面，与 Pitfall 3 防御目标完全兼容。

### Task 2: lan_only_relay_mode.rs integration tests

**新建：** `uc-infra/tests/lan_only_relay_mode.rs`（91 lines）

测试用例：
1. **`relay_disabled_publishes_no_relay_addrs`**（强不等式）：用 `RelayMode::Disabled` bind 后 `endpoint.addr().addrs.iter().any(|a| matches!(a, TransportAddr::Relay(_)))` 必须 == `false`。这是 LAN-only Mode 在 endpoint 层面的可观察事实。
2. **`relay_default_binds_without_panic`**（弱不等式）：`RelayMode::Default` bind 不 panic、不抛错；显式探一下 `endpoint.addr().addrs` 字段确认 endpoint 状态可读，但**不**对内容做断言。

**Helper 设计：**
- `bind_with_relay_mode(mode)` —— 沿用 `iroh_presence_probe.rs:17-29` loopback bind 模式
- `wait_for_addrs(endpoint)` —— 与 `iroh_presence_probe.rs:34-42` 同重试策略，但**不 panic**（Default 用例即使没枚举到候选也不影响断言通过）
- 直接用 iroh API（不复用 `IrohNodeConfig` production stub）— D-C1 / PATTERNS.md §11 共同确认

## Decisions Made

### D1: 分支 B（cargo feature 修订版）替代纯 cfg(test)

| Aspect | 锁定方案（plan） | 实施方案（修订） |
|--------|------------------|------------------|
| 守护条件 | `#[cfg(not(test))]` | `#[cfg(not(any(test, feature = "test-util")))]` |
| 下游 e2e 兼容 | ❌ 失败（cfg 不传递） | ✅ 通过（dev-deps 启用 feature） |
| Production 守护语义 | 不变（默认 build 激活） | **不变**（默认 build 激活，无 `test-util` feature 时） |
| 运行时热切换面 | 无 | 无（feature 是编译期决定） |
| Pitfall 3 强度 | 同等 | 同等 |

### D2: Default 用例只断 "no panic" 弱不等式

D-C1 锁定 + PATTERNS.md §11 critical finding 3 共同要求 —— `RelayMode::Default` 不一定立刻发布 Relay 地址（取决于 iroh 与公网 relay mesh 的连通性，CI 可能没有公网或被 firewall 限制），所以反向断言**只**断"bind 不 panic"。具体 Relay 候选行为留给 Tier C 手工抓包验证。

## CI 盲点声明（must_haves.truths 修订版）

**Production BIND_LOCK 行为不被任何自动化测试覆盖：**
- `cargo test -p uc-infra --lib` 走 `cfg(test)` → 守护 elided
- `cargo test -p uc-infra --test lan_only_relay_mode` 同上
- `cargo test -p uc-bootstrap --test slice*_e2e` 启用 `uc-infra/test-util` feature → 守护 elided
- 启用任何带 `test-util` feature 的下游测试 → 守护 elided

**补偿措施：**
1. **uc-bootstrap 单 entrypoint 保证 production single-bind：**
   - `builders.rs:178`（GUI runtime）—— 单一调用点
   - `non_gui_runtime.rs:280`（CLI/daemon runtime）—— 单一调用点
   - 这两个调用点由 plan 05 装配体系约束，不会同进程同时活跃
2. **任何二次 bind 修改 PR 在 production build 立刻 panic：**
   - `cargo build -p uc-bootstrap --release`（不带 `test-util` feature）后启动 daemon 即触发
3. **CI 必须保证 daemon release build 不携带 `test-util` feature：**
   - production CI pipeline 严禁 `--features test-util` 或 `-F test-util`
   - 这是 reviewer checklist 的一项隐性要求（Phase 97 PR 模板补完）

## Pitfall 3 / Pitfall 8 防御自查

**Pitfall 3（运行时热切换诱惑）：**
- ✅ `pub static BIND_LOCK | pub fn reset_bind_lock` 全工程 0 命中（私有 + 无后门）
- ✅ `BIND_LOCK | OnceLock` 在 node.rs 命中 3 处（import + static + bind() 守护）
- ✅ 第二次 `BIND_LOCK.set(())` panic message 包含 "Pitfall 3" + "runtime hot-swap of LAN-only Mode is explicitly out of scope" + 引用 PITFALLS.md
- ✅ 不依赖 `tokio::sync::OnceCell`（异步版）— 用 std `OnceLock` 同步语义足够
- ✅ 不允许 `Mutex<bool>`/`AtomicBool` 替代（前者允许 reset，OnceLock 是 single-shot 不可逆）

**Pitfall 8（测试覆盖分层）：**
- ✅ Tier A（unit truth-table）—— 留给 plan 05 task 1 覆盖（`uc-bootstrap::network_policy::tests`）
- ✅ Tier B（integration — 本 plan）—— 2 个 case 自动化覆盖 D-C1
- ✅ Tier C（manual 抓包）—— 留给手工流程（D-C1 锁定，roadmap 验收 #1）

**取反点不变量（Pattern A 唯一取反点铁律）：**
- ✅ 全工程 `disable_relays = ! | disable_relays: !` 0 命中（plan 05 唯一取反点尚未引入，符合预期）
- ✅ 本 plan **不**新增任何反向写法

## Verification

| 命令 | 退出码 | 结果 |
|------|-------|------|
| `cargo check -p uc-infra` | 0 | Finished `dev` profile |
| `cargo build -p uc-infra` | 0 | Finished `dev` profile（production cfg, no `test-util` feature） |
| `cargo build -p uc-infra --tests` | 0 | Finished |
| `cargo test -p uc-infra --lib network::iroh::node` | 0 | **5 passed**（含 `bind_is_idempotent_across_builds_for_same_store` 一个 binary 内 2 次 bind） |
| `cargo test -p uc-infra --test lan_only_relay_mode` | 0 | **2 passed** in 0.08s |
| `cargo test -p uc-bootstrap --test slice2_phase2_clipboard_e2e` | 0 | **2 passed** |
| `cargo test -p uc-bootstrap --test slice1_handshake_e2e` | 0 | **1 passed** |
| `cargo test -p uc-bootstrap --test slice2_phase1_presence_e2e` | 0 | **2 passed** |

**E2e binary pass 矩阵（OnceCell 守护后）：**

| Binary | 同 binary 内 bind 次数 | 修复方式 | 状态 |
|--------|----------------------|----------|------|
| uc-infra/src/.../node.rs::tests | 5 个 #[tokio::test] 各 1 次 + idempotent 1 次 = 6 次 | `cfg(test)` elide | ✅ pass |
| uc-bootstrap/tests/slice1_handshake_e2e.rs | sponsor + joiner 双 endpoint = 2 次 | dev-deps `test-util` feature elide | ✅ pass |
| uc-bootstrap/tests/slice2_phase1_presence_e2e.rs | sponsor + joiner = 2 次（× 2 个 test = 4 次/binary） | 同上 | ✅ pass |
| uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs | sponsor + joiner = 2 次（× 2 个 test = 4 次/binary） | 同上 | ✅ pass |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Plan 锁定方案 `#[cfg(not(test))]` 不能 elide 下游 e2e 守护**

- **Found during:** Task 1 验证阶段（`cargo test -p uc-bootstrap --test slice*_e2e` 全部 panic）
- **Issue:** `#[cfg(test)]` 只对正在 `cargo test` 的 crate 生效，不传递到下游依赖；uc-bootstrap e2e 编译时使用 uc-infra production build，原始守护激活 → 5 个 e2e binary panic
- **Fix:** 把守护改成 `#[cfg(not(any(test, feature = "test-util")))]`；新增 `uc-infra/Cargo.toml [features].test-util = []`；新增 `uc-bootstrap/Cargo.toml dev-deps` 启用该 feature
- **影响：** plan must_haves.truths 中"#[cfg(not(test))]" 描述需读为"修订版双契约（cfg + cargo feature）"；CI 盲点描述更新为"含 test-util feature 的下游测试 elide"
- **Pitfall 3 防御强度：** **不变**（cargo feature 编译期决定，不引入运行时分支；production CI 不携带 `test-util` 即同等保护）
- **Files modified:** uc-infra/Cargo.toml, uc-bootstrap/Cargo.toml（节省一处 `node.rs` 守护条件改动）
- **Commit:** `faee71f7`

**2. [Rule 2 - Critical Functionality] CI 盲点必须显式声明在 SUMMARY.md（避免藏在 STRIDE T-094.06-06）**

- **Found during:** SUMMARY 编写阶段
- **Issue:** Plan 06 must_haves.truths 第三条要求"CI 盲点声明"，且 STRIDE T-094.06-06 也提到此盲点；但 SUMMARY 必须再次显式上浮，让 reviewer 在审 SUMMARY 时第一眼看到
- **Fix:** SUMMARY 加独立"CI 盲点声明"段，列举所有不被覆盖的路径 + 三条补偿措施
- **Commit:** N/A（这是文档加强，不在 task commit 中）

## Known Stubs

无 — 本 plan 未引入任何 stub/placeholder。`endpoint.close()`、`Endpoint::builder()`、`TransportAddr::Relay` 都是 iroh 0.98 已发布 API。

## Self-Check: PASSED

- ✅ 文件存在：`src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs` FOUND
- ✅ Commit `faee71f7` FOUND
- ✅ Commit `97dd38f7` FOUND
- ✅ 5 处 acceptance criteria grep 全过：
  - `BIND_LOCK | OnceLock` 命中 3 处（≥ 2 ok）
  - `pub static BIND_LOCK | pub fn reset_bind_lock` 全工程 0 命中
  - `TransportAddr::Relay` 在 lan_only_relay_mode.rs 命中 3 处（注释 + matches!）
  - `IrohNodeConfig` 在 lan_only_relay_mode.rs 命中 1 处（仅 doc comment）
  - `must contain.*Relay | MUST contain Relay` 在 lan_only_relay_mode.rs 命中 0 处（弱不等式正确）
  - `disable_relays = ! | disable_relays: !` 全工程 0 命中（plan 05 之前预期值）
- ✅ 4 个 cargo test command 全过：node.rs unit + lan_only_relay_mode + 3 个 slice e2e
- ✅ Production build（不带 `test-util`）`cargo build -p uc-infra` ok 无警告

## Outputs for Phase Verification

- 自动断言信号：`endpoint bind 时 RelayMode::Disabled → addr().addrs 不含 Relay` 已由 `relay_disabled_publishes_no_relay_addrs` 自动覆盖（NETSET-03 success criterion #1）
- Pitfall 3 结构性防御：`BIND_LOCK` OnceCell 在 production build 激活，任何同进程二次 bind 即 panic
- Pitfall 8 测试分层：Tier B 自动化就位，Tier C 抓包仍是手工流程（D-C1）

## Files Touched

```
.planning/phases/094-backend-network-allow-relay-fallback/094-06-SUMMARY.md  (NEW)
src-tauri/crates/uc-bootstrap/Cargo.toml                                     (modified)
src-tauri/crates/uc-infra/Cargo.toml                                         (modified)
src-tauri/crates/uc-infra/src/network/iroh/node.rs                           (modified, +25)
src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs                       (NEW, +91)
```
