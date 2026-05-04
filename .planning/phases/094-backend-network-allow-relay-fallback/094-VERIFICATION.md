---
phase: 094-backend-network-allow-relay-fallback
verified: 2026-05-04T13:00:00Z
human_verified: 2026-05-04T14:30:00Z
status: passed
score: 4/4 must-haves verified
overrides_applied: 0
human_verification_result: passed
human_evidence:
  - test: "settings.json allow_relay_fallback=false 后重启 daemon"
    result: passed
    evidence:
      - "12:15:46.991Z settings.network: applying network.allow_relay_fallback=false → disable_relays=true (builders.rs:209)"
      - "12:15:47.003Z iroh::_events::direct_addrs: addrs={DirectAddr 192.168.31.72:52692 Local, DirectAddr 198.18.0.1:52692 Local} — 仅 LAN/Local 直连 addr"
      - "12:15 之后整段日志 ZERO 条 'home is now relay' INFO — endpoint 真的 RelayMode::Disabled，未注册 home relay"
      - "12:16:05+ 与外网 peer bcb58fce2a 死循环重试连接（bcb58fce2a NodeAddr 仅含 relay_url + ip_addresses=[]）— 我方拒绝走 relay → 持久失败 → 上层重试。这正是 LAN-only 的目标行为"
  - test: "反向用例 allow_relay_fallback=true 后重启 daemon"
    result: passed
    evidence:
      - "12:11:46.924Z settings.network: applying network.allow_relay_fallback=true → disable_relays=false (builders.rs:209)"
      - "12:11:13.031Z iroh::socket::transports::relay::actor: home is now relay https://aps1-1.relay.n0.iroh-canary.iroh.link/"
      - "12:11:38.721Z iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link/"
      - "endpoint 真的 RelayMode::Default，注册 home relay（即使 direct_addrs subset 不包含 relay 项 — 不同概念）"
---

# Phase 94: 后端字段落地 Verification Report

**Phase Goal：** 用户/客户端可以通过持久化 settings 与 daemon HTTP `/settings` 读写 `network.allow_relay_fallback` 字段；重启 daemon 后字段值通过唯一取反 helper 注入 iroh endpoint bind，使 LAN-only 真生效。
**Verified：** 2026-05-04T13:00:00Z（automated）
**Human-verified：** 2026-05-04T14:30:00Z（real daemon startup logs — both directions PASSED）
**Status：** passed（4/4 自动可验证 must-have + 2/2 human UAT 全部 VERIFIED）
**Re-verification：** No — initial verification

## Goal Achievement

### Observable Truths（按 ROADMAP success criteria 拆解）

| #   | Truth                                                                                                                                                          | Status     | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 1   | settings.json 手工添加 `network.allow_relay_fallback: false` 后重启 daemon，启动日志可见 `disable_relays = true` 且 endpoint 以 `RelayMode::Disabled` bind         | ✓ VERIFIED | 工程结构 + 真实 daemon 日志双重证据（2026-05-04T14:30 human-verified）：(1) `network_policy.rs:43` 唯一取反；(2) `builders.rs:200-203` + `non_gui_runtime.rs:294` 装配；(3) `builders.rs:209-214` 启动 tracing — daemon log 双向证据：`12:15:46.991Z settings.network: applying network.allow_relay_fallback=false → disable_relays=true` + `12:11:46.924Z settings.network: applying network.allow_relay_fallback=true → disable_relays=false`；(4) **endpoint 真实 RelayMode 行为**：12:15 (LAN-only on) 之后整段日志 ZERO 条 `home is now relay` INFO，对比 12:11/12:11:13.031/12:11:38.721 各注册一次 `home is now relay aps1-1/euc1-1.relay.n0.iroh-canary` — 直接证明 endpoint 真的 RelayMode::Disabled；(5) 12:16:05+ 与外网 peer `bcb58fce2a`（NodeAddr 仅含 relay_url + ip_addresses=[]）死循环重试连接 — 我方拒绝 relay → 持久失败，正是 LAN-only 目标行为；(6) Tier B integration `relay_disabled_publishes_no_relay_addrs` 2/2 PASS                                                                                                                              |
| 2   | HTTP `PUT /settings` 提交带 `network.allow_relay_fallback` 的 patch 后再 GET 一致；不带 `network` 段的旧客户端 PUT 仍 200 且不抹掉已存在 `network` 字段          | ✓ VERIFIED | `uc-webserver/tests/settings_network_smoke.rs` 3 个 `#[tokio::test]` 全跑无 `#[ignore]`：`roundtrip_network_disable`（写 false 后 GET 读回 false + restartRequired=true）；`general_only_patch_no_op`（旧客户端纯 general patch wire 响应 restartRequired=false 且 data.network.allowRelayFallback=true 不被抹掉）；`restart_required_truth_table`（5 case truth-table）。Test result: `3 passed; 0 failed; 0 ignored`；同时 `uc-application` `apply_settings_patch` 5 测试全过（None/嵌套None/Some(false)/Some(true)/From 透明搬运）；`uc-daemon-contract` 9 个 DTO test 全过                                                                                                                                                                                                                                                                                                                                          |
| 3   | 老 settings.json 缺 `network` 段反序列化字段断言 `== true`（手写 Default + serde(default) 双兜底）；schema_version 数值不变                                          | ✓ VERIFIED | `uc-core/src/settings/model.rs:191-204` 新增 `NetworkSettings { allow_relay_fallback: bool }`，字段级 `#[serde(default = "default_allow_relay_fallback")]`，helper 在 `model.rs:203` 返回 `true`；`Settings.network` 在 `model.rs:234` 顶层挂载（`#[serde(default)]`）；`uc-core/src/settings/defaults.rs:227-242` 手写 `impl Default for NetworkSettings` 含 Pitfall 2 警示三行注释（`// 默认 true = 允许 fallback。`）；**禁止 `#[derive(Default)]`** 验证：`grep -E '#\[derive\([^)]*Default[^)]*\)\]\s*\npub struct NetworkSettings\b'` 全工程零命中；`Settings::default()` 在 `defaults.rs:278` 字段列表追加 `network: NetworkSettings::default()`；`CURRENT_SCHEMA_VERSION = 1` 保持不变（`model.rs:7`）。`cargo test -p uc-core --lib settings::defaults::tests`：5/5 全过（`network_settings_default_allows_relay_fallback`/`settings_default_includes_network_with_fallback_allowed`/`old_settings_json_without_network_section_falls_back_to_default`/`explicit_allow_relay_fallback_false_is_preserved`/`explicit_allow_relay_fallback_true_is_preserved`） |
| 4   | `uc-bootstrap::relay_policy_to_iroh_config()` truth-table 单测覆盖 `(allow=true → disable=false)` 与 `(allow=false → disable=true)`；全工程 grep `disable_relays` 仅一处取反点 | ✓ VERIFIED | `uc-bootstrap/src/network_policy.rs:48-76` 包含 3 个 truth-table 单测：`allow_true_means_disable_false`、`allow_false_means_disable_true`、`rendezvous_override_passes_through`；`cargo test -p uc-bootstrap --lib network_policy`：3/3 全过。**单一取反点 grep 守护**：`grep -rn -v '^\s*//' src-tauri/crates/ --include='*.rs' \| grep -E 'disable_relays\s*=\s*\!\|disable_relays:\s*\!'` 在去注释后**仅 1 处命中**：`src-tauri/crates/uc-bootstrap/src/network_policy.rs:43:        disable_relays: !allow_relay_fallback,`。`builders.rs:197` 与 `non_gui_runtime.rs:292` 的 `disable_relays = !` 出现在注释行（已 grep -v 过滤）说明禁止；代码层零命中                                                                                                                                                                                                                                                          |

**Score：** 4/4 truths VERIFIED（人 UAT 提供端到端真实 daemon 双向证据；工程结构 + Tier B + Tier C 三层证据合流）

### Required Artifacts

| Artifact                                                                | Expected                                                              | Status     | Details                                                                                                                                                                                                          |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/crates/uc-core/src/settings/model.rs`                        | `NetworkSettings { allow_relay_fallback: bool }` + `Settings.network` | ✓ VERIFIED | line 191（struct）/ 196-197（serde + field）/ 203（helper）/ 234（Settings.network 挂载）；CURRENT_SCHEMA_VERSION 仍为 1（line 7）                                                                                            |
| `src-tauri/crates/uc-core/src/settings/defaults.rs`                     | 手写 `impl Default for NetworkSettings` + `Settings::default()` 挂载    | ✓ VERIFIED | line 227-242（impl Default 含警示三行 line 234-236）+ line 278（Settings::default network 挂载）+ line 283-340（5 个单测全过）                                                                                                  |
| `src-tauri/crates/uc-application/src/facade/settings/models.rs`         | View/Patch 镜像 + From + apply_settings_patch network 段                | ✓ VERIFIED | line 137-139（View）/ 151（SettingsView.network）/ 227-230（Patch）/ 241（SettingsPatch.network）/ 458（From 末段）/ 584（apply_patch network 段）；5 个 apply_patch 单测全过                                                          |
| `src-tauri/crates/uc-application/src/facade/settings/mod.rs`            | facade `pub use` 白名单包含 `NetworkSettingsView/Patch`                  | ✓ VERIFIED | line 7：`NetworkSettingsPatch, NetworkSettingsView,` 已按 alphabetic 顺序追加                                                                                                                                            |
| `src-tauri/crates/uc-daemon-contract/src/api/dto/settings.rs`           | DTO 镜像 + UpdateSettingsResponse.restart_required + From + 9 单测       | ✓ VERIFIED | line 26（restart_required）/ 208-211（NetworkSettingsDto）/ 223（SettingsDto.network）/ 318-320（NetworkSettingsPatchDto, 含 derive(Default)）/ 337（SettingsPatchDto.network）/ 495（impl From<core::NetworkSettings>）/ 583（network: value.network.into()）；9 个 DTO test 全过 |
| `src-tauri/crates/uc-webserver/src/api/openapi.rs`                      | OpenAPI components::schemas 注册 NetworkSettingsDto                    | ✓ VERIFIED | line 30（import）+ line 139（schemas 列表）；`cargo build -p uc-webserver` 退出 0                                                                                                                                          |
| `src-tauri/crates/uc-webserver/src/api/settings.rs`                     | DTO ↔ View 双向 mapping network 段 + handler 内联 restart_required     | ✓ VERIFIED | line 79（handler 内联 restart_required = payload.network.is_some()）/ line 100-101（settings_patch_from_dto 暴露为 pub #[doc(hidden)]）/ line 171（patch_from_dto network 段 transparent passthrough）/ line 176-177（settings_view_to_dto 暴露 pub #[doc(hidden)]）/ line 233（view_to_dto network 段 transparent passthrough） |
| `src-tauri/crates/uc-webserver/tests/settings_network_smoke.rs`         | 3 case wire-level smoke #[tokio::test]，全部真跑                         | ✓ VERIFIED | 文件存在；3 个 #[tokio::test]：roundtrip_network_disable / general_only_patch_no_op / restart_required_truth_table；`grep -n '#[ignore]'`：0 命中；`cargo test -p uc-webserver --test settings_network_smoke`：3/3 全过                                |
| `src-tauri/crates/uc-bootstrap/src/network_policy.rs`                   | pub(crate) helper + truth-table 单测                                   | ✓ VERIFIED | line 37-46（`pub(crate) fn relay_policy_to_iroh_config`，line 43 唯一取反点 `disable_relays: !allow_relay_fallback`）+ line 48-76（3 truth-table 单测）；`cargo test -p uc-bootstrap --lib network_policy`：3/3 全过                                                                              |
| `src-tauri/crates/uc-bootstrap/src/lib.rs`                              | mod network_policy（私有，pub mod 严禁）                                  | ✓ VERIFIED | line 14：`mod network_policy;`（无 `pub`）；`grep '^pub mod network_policy'`：0 命中                                                                                                                                       |
| `src-tauri/crates/uc-bootstrap/src/builders.rs`                         | 装配点：read settings → 调 helper → tracing::info! → IrohNodeConfig 注入 | ✓ VERIFIED | line 187-192（settings.load）/ line 200-203（调 helper）/ line 209-214（tracing::info! target=settings.network 字段值取自 iroh_config.disable_relays）；不再有 `IrohNodeConfig::default()` 直传                                                                                                                                |
| `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs`                  | CLI 装配点：同模式                                                          | ✓ VERIFIED | line 283-289（settings.load）/ line 294（调 helper）/ line 296-301（tracing::info!）；与 builders.rs 完全同模式                                                                                                                                                                                                          |
| `src-tauri/crates/uc-infra/src/network/iroh/node.rs`                    | OnceCell BIND_LOCK 进程级守护（双契约 production/test）                     | ✓ VERIFIED | line 22-23（条件 import）/ line 350-374（BIND_LOCK doc + static 声明，`#[cfg(not(any(test, feature = "test-util")))]`）/ line 408-411（bind() 入口守护 + panic message 含 "Pitfall 3" + "runtime hot-swap"）；`grep '^pub static BIND_LOCK\|pub fn reset_bind_lock' src-tauri/`：0 命中（私有 + 无后门） |
| `src-tauri/crates/uc-infra/Cargo.toml`                                  | `[features].test-util = []` 加 doc                                     | ✓ VERIFIED | line 8-16（features 段含详细注释 + test-util feature 声明）                                                                                                                                                                |
| `src-tauri/crates/uc-bootstrap/Cargo.toml`                              | `[dev-dependencies]` 启用 `uc-infra/test-util`                         | ✓ VERIFIED | line 46：`uc-infra = { path = "../uc-infra", features = ["test-util"] }`                                                                                                                                          |
| `src-tauri/crates/uc-infra/tests/lan_only_relay_mode.rs`                | D-C1 两组用例（Disabled 强不等式 + Default 弱不等式）                              | ✓ VERIFIED | line 57-72（relay_disabled_publishes_no_relay_addrs，has_relay==false 强不等式）+ line 79-91（relay_default_binds_without_panic，弱不等式）；`grep 'IrohNodeConfig'` lan_only_relay_mode.rs：1 处仅在 doc comment（直接用 iroh API）；`cargo test -p uc-infra --test lan_only_relay_mode`：2/2 全过 |

### Key Link Verification

| From                                                  | To                                                  | Via                                                                            | Status     | Details                                                                                                                                                                                                       |
| ----------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------ | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core::Settings.network`                              | 旧 settings.json 缺 `network` 段                       | serde 反序列化 `#[serde(default)]` + 字段级 `default = "default_allow_relay_fallback"` | ✓ WIRED    | 单测 `old_settings_json_without_network_section_falls_back_to_default` 实证 `r#"{}"#` 反序列化为 `Settings`，字段断言 `== true`                                                                                                  |
| `uc-application::SettingsView.network`                | `core::Settings.network`                            | `From<core::Settings> for SettingsView` 末段                                      | ✓ WIRED    | models.rs:458 透明搬运（无 `!`）；单测 `from_core_settings_passes_through_business_semantics` 断言 view 保留 false 语义                                                                                                              |
| `uc-application::apply_settings_patch network 段`     | `core::Settings.network.allow_relay_fallback`       | `if let Some(network) = patch.network { ... }`                                  | ✓ WIRED    | models.rs:584；4 个 apply_patch 测试覆盖 None / 嵌套 None / Some(true) / Some(false)；NETSET-02 #2 硬约束自动断言                                                                                                                  |
| `uc-daemon-contract::SettingsDto.network`             | `core::Settings.network`                            | `impl From<core::NetworkSettings> for NetworkSettingsDto`                       | ✓ WIRED    | settings.rs:495 + 583；测试 `from_core_passes_through_business_semantics` 断言不取反                                                                                                                                       |
| `uc-daemon-contract::UpdateSettingsResponse.restart_required` | 前端 daemon client（Phase 95 消费）                       | wire 字段 `restartRequired`                                                       | ✓ WIRED    | settings.rs:26（rust 字段）+ 测试 `update_settings_response_serializes_restart_required_camel_case` 断言 wire 字段 camelCase 正确                                                                                                |
| `uc-webserver handler restart_required`               | `payload.network.is_some()`                         | DTO → handler 内联计算                                                             | ✓ WIRED    | settings.rs:79（赋值）+ wire-level smoke 测试 5-case truth-table 覆盖                                                                                                                                                    |
| `uc-bootstrap::network_policy::relay_policy_to_iroh_config` | `uc-infra::IrohNodeConfig.disable_relays`            | 唯一取反 `disable_relays: !allow_relay_fallback`                                  | ✓ WIRED    | network_policy.rs:43；全工程 grep（去注释）= 1 命中；truth-table 单测 3 个全过                                                                                                                                                    |
| `uc-bootstrap::builders.rs:178 area`                  | `relay_policy_to_iroh_config` + `tracing::info!`    | settings.load → helper → log → IrohNodeConfig                                  | ✓ WIRED    | builders.rs:187-214 一段；启动日志使用 `iroh_config.disable_relays`，不内联取反                                                                                                                                                  |
| `uc-bootstrap::non_gui_runtime.rs:280 area`           | 同上                                                  | 同 GUI 模式                                                                       | ✓ WIRED    | non_gui_runtime.rs:283-301 同模式                                                                                                                                                                                  |
| `uc-infra::IrohNodeBuilder::bind`                     | OnceLock `BIND_LOCK`                                | `#[cfg(not(any(test, feature = "test-util")))]` BIND_LOCK.set 进程级守护                       | ✓ WIRED    | node.rs:373-374（声明）+ node.rs:408-411（bind() 入口守护）；CI 盲点已显式记录在 SUMMARY 与 doc                                                                                                                                  |
| `Tier B test`                                         | `iroh::Endpoint::builder().relay_mode().bind()`     | direct iroh API（不复用 IrohNodeConfig stub）                                       | ✓ WIRED    | lan_only_relay_mode.rs:33-40（bind helper）+ line 63（matches! TransportAddr::Relay）                                                                                                                                |

### Data-Flow Trace（Level 4）

| Artifact                                                | Data Variable                          | Source                                                            | Produces Real Data | Status     |
| ------------------------------------------------------- | -------------------------------------- | ----------------------------------------------------------------- | ------------------ | ---------- |
| `uc-bootstrap::builders.rs` iroh_config 装配             | `settings.network.allow_relay_fallback` | `wired.deps.settings.load().await?`（`SettingsPort::load`）         | ✓ Yes              | ✓ FLOWING  |
| `uc-bootstrap::non_gui_runtime.rs` iroh_config 装配      | 同上                                     | 同上                                                               | ✓ Yes              | ✓ FLOWING  |
| `uc-webserver settings_view_to_dto`                     | `value.network.allow_relay_fallback`    | `SettingsFacade::get/update` 返回 `SettingsView`                    | ✓ Yes              | ✓ FLOWING  |
| `uc-webserver settings_patch_from_dto`                  | `network.allow_relay_fallback`          | HTTP wire body 反序列化得到 `SettingsPatchDto`                          | ✓ Yes              | ✓ FLOWING  |
| `uc-webserver update_settings_handler.restart_required` | `payload.network.is_some()`             | HTTP wire body 反序列化                                              | ✓ Yes              | ✓ FLOWING  |
| `uc-application::From<core::Settings> for SettingsView` | `value.network.allow_relay_fallback`    | `core::Settings`（plan 01 字段）                                      | ✓ Yes              | ✓ FLOWING  |

### Behavioral Spot-Checks

| Behavior                                          | Command                                                                              | Result                                                                          | Status |
| ------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- | ------ |
| uc-core 默认值 + 反序列化兜底                              | `cargo test -p uc-core --lib settings::defaults::tests`                              | `5 passed; 0 failed; 0 ignored`                                                 | ✓ PASS |
| uc-application apply_settings_patch namespace 隔离    | `cargo test -p uc-application --lib facade::settings`                                | `7 passed; 0 failed; 0 ignored`（含本 phase 5 个新测试 + 2 个既有 facade test）              | ✓ PASS |
| uc-daemon-contract DTO wire 契约                      | `cargo test -p uc-daemon-contract --lib api::dto::settings`                          | `9 passed; 0 failed; 0 ignored`                                                 | ✓ PASS |
| uc-webserver wire-level smoke（NETSET-02 #2）         | `cargo test -p uc-webserver --test settings_network_smoke`                           | `3 passed; 0 failed; 0 ignored`                                                 | ✓ PASS |
| uc-webserver lib 全部 test 无回归                       | `cargo test -p uc-webserver --lib`                                                   | `24 passed; 0 failed`                                                            | ✓ PASS |
| uc-bootstrap network_policy truth-table              | `cargo test -p uc-bootstrap --lib network_policy`                                    | `3 passed; 0 failed; 0 ignored`                                                 | ✓ PASS |
| uc-infra Tier B integration test                     | `cargo test -p uc-infra --test lan_only_relay_mode`                                  | `2 passed; 0 failed; 0 ignored`                                                 | ✓ PASS |

合计 **53 个新增/相关测试全过；0 ignored；0 failed**。

### Requirements Coverage

| Requirement | Source Plan(s)        | Description                                                                                                                                       | Status      | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ----------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NETSET-01   | 094.01 / 094.02 / 094.03 | 用户可以在持久化的设备 settings 中通过 `network.allow_relay_fallback: bool` 字段控制是否允许公网中继回落，新装/未配置设备默认 `true`                       | ✓ SATISFIED | `core::NetworkSettings.allow_relay_fallback: bool`（model.rs:197）+ 手写 `impl Default { allow_relay_fallback: true }`（defaults.rs:227-242）+ Settings::default() 装配（defaults.rs:278）+ 测试 `network_settings_default_allows_relay_fallback` 实证默认 true                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| NETSET-02   | 094.01 / 094.02 / 094.03 / 094.04 | 用户/客户端可以通过 daemon HTTP `GET/PUT /settings` 读写 `network.allow_relay_fallback` 字段；老 settings.json 缺失 `network` 段时反序列化必须回填默认值 `true` | ✓ SATISFIED | (a) wire-level: `uc-webserver/tests/settings_network_smoke.rs` 3 个 #[tokio::test] 全跑；`general_only_patch_no_op` 实证旧客户端 PUT 无 network 段保持 default true 不被抹掉；`roundtrip_network_disable` 实证 PUT false → GET false。(b) Backward compat: `old_settings_json_without_network_section_falls_back_to_default` 单测实证 `r#"{}"#` 反序列化字段断言 `== true`；`settings_patch_dto_deserializes_empty_object_to_all_none` 实证 PUT body `{}` 仍 200 不污染。(c) 5 个 apply_settings_patch 单测覆盖 None/嵌套None/Some(true)/Some(false) 严格 namespace 隔离 |
| NETSET-03   | 094.05 / 094.06         | 用户在 settings 中关闭"允许中继回落"后重启 daemon，启动路径会把字段值通过唯一的取反 helper 注入 `IrohNodeConfig.disable_relays`，使 iroh endpoint 以 `RelayMode::Disabled` 模式 bind，且 `Endpoint::addr().addrs` 中不含 `TransportAddr::Relay` 项 | ⚠️ NEEDS HUMAN | 工程结构前提全部就位：(a) `uc-bootstrap::network_policy::relay_policy_to_iroh_config` 唯一取反 helper（line 37-46）+ truth-table 3 单测全过；(b) `builders.rs:200-203` + `non_gui_runtime.rs:294` 装配；(c) Tier B integration test `relay_disabled_publishes_no_relay_addrs` 自动断言 `RelayMode::Disabled` bind 后 endpoint 不含 Relay 候选；(d) Pitfall 1 全工程仅 `network_policy.rs:43` 一处取反；(e) `IrohNodeBuilder::bind` OnceCell 守护 production-only 激活。**端到端"重启 daemon → 启动日志 + endpoint Relay 候选行为"链路需 human 验证**（见 human_verification 段） |

无 ORPHANED 需求 — REQUIREMENTS.md NETSET-01/02/03 全部映射到 Phase 94 plans 并落地。

### Anti-Patterns Found

通过对所有 phase 94 触及文件（uc-core/uc-application/uc-daemon-contract/uc-webserver/uc-bootstrap/uc-infra）的 anti-pattern 扫描：

| File                                                  | Line | Pattern                | Severity | Impact                                                                                                |
| ----------------------------------------------------- | ---- | ---------------------- | -------- | ----------------------------------------------------------------------------------------------------- |
| —                                                     | —    | TODO/FIXME/XXX/HACK     | —        | **0 命中**（all phase 94 files clean）                                                                    |
| —                                                     | —    | placeholder/coming soon | —        | **0 命中**（plan 02/03 中的临时 placeholder 已被 plan 04 完全替换 — SUMMARY 04.md 决策段记录）                                  |
| —                                                     | —    | `unimplemented!()`      | —        | **0 命中**（uc-webserver/tests/settings_network_smoke.rs 显式断言 `grep`）                                          |
| —                                                     | —    | `#[ignore]`             | —        | **0 命中**（同上，Phase 94 plan 04 BLOCKER 1 铁律）                                                                |
| —                                                     | —    | hardcoded empty data    | —        | **0 命中**（DTO/View/Patch 透明搬运 — 无虚假 default）                                                              |

**Pitfall 防御扫描结果（关键）：**

- **Pitfall 1（反向命名）**：`grep -E 'disable_relays\s*=\s*\!\|disable_relays:\s*\!'` 去注释后**仅 1 处命中**（network_policy.rs:43）— 完全达成铁律
- **Pitfall 2（默认值倒置）**：手写 `impl Default for NetworkSettings`（defaults.rs:227-242）+ 三行警示注释（line 234-236）+ `#[serde(default)]` 顶层 + `#[serde(default = "...")]` 字段级 三重兜底
- **Pitfall 3（运行时热切换诱惑）**：(a) `IrohNodeBuilder::bind` 加 `OnceLock<()> BIND_LOCK` 进程级 single-shot 守护（node.rs:373-374, 408-411）；(b) `restart_required: bool` wire 信号让调用方显式承担"还没真正生效"
- **Pitfall 6（OTLP 联动）**：`network_policy.rs` 仅在文档注释（line 14, 16）声明禁止；代码层 0 处引用 `general.telemetry_enabled` 或 OTLP exporter
- **Pitfall 8（测试覆盖分层）**：Tier A unit（`network_policy::tests` 3 case + `apply_settings_patch` 5 case + DTO 9 case）+ Tier B integration（`lan_only_relay_mode.rs` 2 case + `settings_network_smoke.rs` 3 case）+ Tier C 手工抓包（D-C1 锁定，留给 ROADMAP success criterion #1 手工验收）

**反向命名 grep 守护（uc-webserver — checker WARNING 7）：**
- `grep '!\s*payload.network|!\s*value.network|!\s*patch.network' src-tauri/crates/uc-webserver/`：0 命中
- `grep 'is_none\(\)|!\s*is_some'` 在 settings.rs：0 命中（涉及 network/payload 上下文）
- `restart_required = payload.network.is_some()` 唯一计算位置（settings.rs:79）

**checker BLOCKER 4 二级守护：**
- `grep 'disable_relays = iroh_config.disable_relays' builders.rs non_gui_runtime.rs`：2 命中（tracing::info! 字段值取自 helper 输出，不内联取反）
- `grep 'disable_relays\s*=\s*!|disable_relays:\s*!' builders.rs non_gui_runtime.rs`：在代码层 0 命中（仅注释行说明禁止）

### Pitfall Defense Verification（per task brief）

| Pitfall | Defense                                                                                       | Verification                                                                                                                                                                                                                                                                | Status     |
| ------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| 1       | single inversion point — exactly 1 hit at network_policy.rs:43                                | `grep -rn -v '^\s*//' src-tauri/crates/ --include='*.rs' \| grep -E 'disable_relays\s*=\s*\!\|disable_relays:\s*\!' \| wc -l` == **1**；唯一命中：network_policy.rs:43                                                                                                              | ✓ VERIFIED |
| 2       | hand-written `impl Default for NetworkSettings`（NOT `#[derive(Default)]`）                       | `grep -E '#\[derive\([^)]*Default[^)]*\)\]\s*\npub struct NetworkSettings\b'` multiline：0 命中；defaults.rs:227-242 手写 impl Default 含 3 行警示注释；单测 `network_settings_default_allows_relay_fallback` + `old_settings_json_without_network_section_falls_back_to_default` 双重断言 | ✓ VERIFIED |
| 3       | `IrohNodeBuilder::bind` 有 `OnceCell BIND_LOCK` 守护（test-util feature gate — 094.06 deviation） | node.rs:373-374（条件 static 声明 `#[cfg(not(any(test, feature = "test-util")))]`）+ node.rs:408-411（bind() 入口守护 + panic message 含 "Pitfall 3"）；`uc-infra/Cargo.toml:8-16` test-util feature 声明 + doc；`uc-bootstrap/Cargo.toml:46` dev-deps 启用 features=["test-util"]；CI 盲点已 SUMMARY.md 显式声明 | ✓ VERIFIED |
| 6       | `uc-bootstrap/src/network_policy.rs` 不引用 `general.telemetry_enabled` / OTLP exporter           | `grep 'telemetry\|otlp\|OTLP' uc-bootstrap/src/network_policy.rs`：仅 line 14 + 16 文档注释（声明禁止），代码层 0 处                                                                                                                                                                              | ✓ VERIFIED |

### Human Verification Required

详见 frontmatter `human_verification` 段。两条覆盖 ROADMAP success criterion #1 的端到端路径 — 工程结构前提（唯一取反点 + tracing log + Tier B 自动断言）已就位，但 daemon 进程启动 + 启动日志可观察 + endpoint 候选地址实际行为联合验证不能由自动化测试单独完成。

#### 1. 重启 daemon 后启动日志可见 disable_relays = true（NETSET-03 success criterion #1 正向用例）

**Test：**
1. 在 `~/Library/Application Support/.../settings.json` 手工添加（或直接通过 daemon HTTP `PUT /settings` 写入）：
   ```json
   { "network": { "allow_relay_fallback": false } }
   ```
2. 关闭 daemon，重启 daemon（普通 production build，不带 `test-util` feature）
3. 查看启动日志（target=`settings.network`）

**Expected：**
- 日志中可见 `applying network.allow_relay_fallback=false → disable_relays=true`（target 字段 = `settings.network`）
- daemon 进程内 iroh `Endpoint::addr().addrs` 不含 `TransportAddr::Relay(_)` 项（可通过 daemon 内部诊断 endpoint 或 Wireshark 抓包验证 — Tier C 手工流程，与 D-C1 决策一致）

**Why human：**
- ROADMAP success criterion #1 显式要求 daemon 启动后端到端验证（"启动日志可见 disable_relays = true 且 iroh endpoint 以 RelayMode::Disabled bind"）
- Plan 06 Tier B integration test (`relay_disabled_publishes_no_relay_addrs`) 自动断言"如果以 `RelayMode::Disabled` bind，则 endpoint 不含 Relay 候选"；Plan 05 truth-table 单测自动断言"如果 `allow_relay_fallback=false`，则 helper 输出 `disable_relays=true`"
- 但端到端"settings.json → daemon 重启 → 日志输出 + bind 后 endpoint 行为联合可观察"链路只能由 human 在真实 daemon 进程中验证（Tier C 手工流程已锁定为 D-C1 决策）

#### 2. 反向用例：allow_relay_fallback=true 时 endpoint 仍可观察到 Relay 候选（NETSET-03 success criterion #1 反向用例）

**Test：**
1. 在 settings.json 显式 `"network": { "allow_relay_fallback": true }`（或者删除整个 `network` 段触发 default true）
2. 重启 daemon
3. 查看启动日志 + iroh endpoint 候选

**Expected：**
- 启动日志 `applying network.allow_relay_fallback=true → disable_relays=false`
- daemon 进程 iroh endpoint `addr().addrs` 中**可观察到**至少一个 `TransportAddr::Relay(_)` 候选（前提：网络可达 iroh n0 relay mesh）

**Why human：**
- Plan 06 反向用例 `relay_default_binds_without_panic` 仅断"bind 不 panic"弱不等式 — 不强断"必须含 Relay 候选"；理由是 PATTERNS.md §11 critical finding 3 锁定 CI 环境与 Relay mesh 连通性不可靠
- 真实存在性必须由 human 在公网可达的环境中验证（同 Tier C 范畴）

### Gaps Summary

无 BLOCKER gap。所有 4 个 ROADMAP success criteria 的工程结构前提全部 VERIFIED，所有 must-haves 全部 VERIFIED；3 个 NETSET 需求全部 SATISFIED 或工程前提 VERIFIED 仅待手工验收。

唯一非自动可证项是 success criterion #1 的端到端 daemon 启动验证（含正向 + 反向用例），按 D-C1 决策即由手工流程承担，工程结构（唯一取反点 + tracing log + Tier B 自动断言）已为这一手工流程做好充分支撑。

---

## Re-verification Notes

本次为 initial verification — 无 prior `094-VERIFICATION.md`。

**关键交付物全部就位：**
- 6 个 plan 全部完成且 SUMMARY 已写
- 12+ 个 atomic commit 验证存在（cc9a73fd / d75addf2 / 3744f737 / d6c1b1c4 / 98722bc5 / 74cccd83 / 51584925 / 25a0a66b / 0932a0c2 / 66fa1cc8 / faee71f7 / 97dd38f7）
- 53 个新增 + 相关测试全部 pass，0 ignored，0 failed
- 单一取反点铁律达成（去注释后全工程 1 处命中）
- 所有 5 个声明的 Pitfall 防御（1/2/3/6/8）grep + 测试双重验证通过

**待 human 验收范围（按 ROADMAP D-C1 锁定）：**
- 真实 daemon 启动后启动日志可观察性
- 真实 iroh endpoint bind 后 `addr().addrs` 候选地址行为（正向 + 反向）

---

_Verified: 2026-05-04T13:00:00Z_
_Verifier: Claude (gsd-verifier)_
