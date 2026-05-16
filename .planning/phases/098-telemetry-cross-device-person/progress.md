# Progress: 098 · Telemetry 跨设备 Person 聚合 v2

## 阶段总览

| PR | 状态 | 备注 |
|---|---|---|
| PR 1: AnalyticsPersonId + 持久化 | ✅ 完成 | uc-observability 内部 |
| PR 2: build_event_payload 切换 distinct_id | ✅ 完成 | sink 层 |
| PR 3: $identify + sink 支持 | ✅ 完成 | port + Posthog/Stdout/Gated |
| PR 4: A1 setup_completed identify | ✅ 完成 | 含 AnalyticsIdentityPort 抽象 |
| PR 5: Sponsor pairing payload 携带 space_person_id | ✅ 完成 | wire v2→v3、SponsorConfirm 新字段 |
| PR 6: A2 redeem identify | ✅ 完成 | 含 None 退化 + adopt 失败 fallback |
| PR 7: $groups + $groupidentify | ✅ 完成 | A1 emit group_identify(device_count=1) |
| PR 8: reset / switch_space 联动 | ✅ 完成 | reset_telemetry_identity port + facade method；switch_space commit 后 identify |
| PR 9: schema doc 同步 + progress.md 收尾 | ✅ 完成 | telemetry-events.md §3.1/3.2/3.3/3.4/4/7.0/10.1 |

## 后续工作（不在 task_plan 098 范围内）

- **Settings UI 文案 / 按钮**：PR 8 已实现 `SpaceSetupFacade::reset_telemetry_identity()` 后端；
  GUI 触发入口（settings 页面"重置 telemetry"按钮 + 文案）属于 frontend 域，留作独立 issue。
- **Changelog**：用户可见行为变化（跨设备 person 聚合开始生效）应在下一个 release 的
  `docs/changelog/*.md` 登记，由 release-please 流程在版本切换时统一处理。

## 决策日志

- 2026-05-15：开放问题 1/2/3 全部按 task_plan 倾向选 A（最低侵入 / forward-compatible / fire-and-forget）。
- 2026-05-15：用户选择"一次性做完所有 PR"。在同一分支 `short-feet` 上累积 commits。
- 2026-05-15：开放问题 6（group device_count）改为简化路径——A1 emit 一次 `$groupidentify(device_count=1, created_at=now)`；后续设备数由 PostHog dashboard 直接 query group 下 distinct person 数推算，**不** 让 sponsor 在 pairing_succeeded 时重发 group_identify（task_plan 倾向是重发，但 PostHog group analytics 模型下重发是冗余）。如未来发现 dashboard 需要"实时设备数"再补 sponsor 重发。
- 2026-05-15：v1 sponsor + v2 joiner 的"互操作"在 wire 层无法 forward-compat（postcard 严格），实际行为是 wire version v2→v3 升版本拒连。task_plan 决策 A 的"joiner 退化 Solo"在 wire 升版本统一后语义变为：**v3 sponsor + v3 joiner 但 sponsor 自己未持久化 space_person_id 时，confirm.sponsor_space_person_id 为 None，joiner 退回 Solo**。这是真正的 forward-compatible 形态。

## PR 1-9 改动文件清单

### uc-observability
- `src/analytics/context.rs`：新增 `AnalyticsPersonId` enum（Solo/SpaceShared）、`EventContext.analytics_person_id` 字段（`#[serde(skip)]`，不进 wire）、`EventContextInputs.analytics_person_id` 必传字段
- `src/analytics/ids.rs`：新增 `space_person_id` 文件常量、`load_space_person_id` / `set_space_person_id` / `clear_space_person_id` API
- `src/analytics/identity.rs`：**新建模块**。`AnalyticsIdentityPort` trait（`adopt_space_person` / `release_space_person` / `current_space_person_id` / `reset_telemetry_identity`）、`LocalAnalyticsIdentity` 真实实现、`NoopAnalyticsIdentity` 测试 fallback、`hash_space_id_for_telemetry` helper
- `src/analytics/port.rs`：`AnalyticsPort` 加 `identify(IdentifyPayload)` / `group_identify(GroupIdentifyPayload)` 方法（默认 noop）。新增 `IdentifyPayload` / `GroupIdentifyPayload` 数据类型
- `src/analytics/sinks/mod.rs`：`build_event_payload` 改为从 `ctx.analytics_person_id` 派生 distinct_id（PR 2）
- `src/analytics/sinks/posthog.rs`：实现 `identify` / `group_identify`，wire 形态对齐 PostHog spec（`$anon_distinct_id` 在 properties，`$group_type/key/set` 在 properties）；`inject_posthog_standard_fields` 当 `space_id_hash` 存在时注入 `$groups: { space: hash }`
- `src/analytics/sinks/stdout.rs`：dev sink 镜像 identify / group_identify
- `src/analytics/sinks/gated.rs`：identify / group_identify 受同一 gate 守护
- `src/analytics/mod.rs`：`pub use` 新类型与函数
- `Cargo.toml`：新增 `sha2 = "0.10"`（hash_space_id_for_telemetry）

### uc-core
- `src/pairing/session_message.rs`：`SponsorConfirm` 新增 `sponsor_space_person_id: Option<Uuid>` 字段

### uc-infra
- `src/pairing/wire.rs`：`WIRE_VERSION` v2→v3、`WireSponsorConfirm` 新增 `sponsor_space_person_id: Option<String>`、`WireDecodeError::InvalidSpacePersonId` 新错误变体、to_wire / from_wire 处理。新增两条 wire round-trip 测试（Some/None）
- `src/pairing/session.rs`：decode 错误 match 增补 `InvalidSpacePersonId` 分支

### uc-application
- `src/usecases/setup/initialize_space.rs`：A1 use case 接 `AnalyticsIdentityPort`，setup_status 落地后→adopt → identify → group_identify（device_count=1, created_at=now）→ emit setup_completed。adopt 失败跳过 identify+group_identify，setup_completed 仍 emit
- `src/usecases/pairing/redeem_invitation.rs`：A2 use case 接 `AnalyticsIdentityPort`，setup_status 落地后→若 outcome.sponsor_space_person_id=Some 则 adopt → identify。adopt 失败跳过 identify
- `src/usecases/setup/switch_space/mod.rs`：`SwitchSpaceUseCase` 接 `analytics` + `analytics_identity`；commit phase 完成后用 `outcome.sponsor_space_person_id` 决定 adopt / release，发 identify 把 distinct_id 切到目标 Space 的 person（None → release 退回 Solo）
- `src/pairing_inbound/sponsor_handshake.rs`：`SponsorHandshakeCoordinator` 新增 `analytics_identity` 依赖，构造 `SponsorConfirm` 时调 `current_space_person_id()` 派给 joiner
- `src/pairing_outbound/joiner_handshake.rs`：`JoinerHandshakeOutcome` 新增 `sponsor_space_person_id: Option<Uuid>` 字段，从 confirm 透传
- `src/facade/space_setup/deps.rs`：`SpaceSetupDeps` 新增 `analytics_identity` 字段
- `src/facade/space_setup/facade.rs`：把 `analytics_identity` 透传给所有 use case；新增 `pub fn reset_telemetry_identity(&self) -> Result<(), ResetTelemetryError>` thin method
- `src/facade/space_setup/errors.rs`：新增 `ResetTelemetryError` 类型
- `src/facade/space_setup/mod.rs`：`pub use ResetTelemetryError`
- `src/usecases/setup/switch_space/tests.rs`：fixture 加 `sponsor_space_person_id: None` + `NoopAnalyticsSink/Identity`
- `src/pairing_inbound/orchestrator.rs`：测试 fixture 加 `NoopAnalyticsIdentity`
- `Cargo.toml`：新增 `serde_json = "1"`

### uc-bootstrap
- `src/analytics.rs`：`compose_event_context` 调 `load_space_person_id` 决定 Solo vs SpaceShared；`hash_space_id` 函数移到 uc-observability，bootstrap 改用 `hash_space_id_for_telemetry`；删除三个本地 hash 测试（已在 uc-observability 覆盖）
- `src/assembly.rs`：`WiredDependencies` 新增 `analytics_identity` 字段；wire_dependencies 装配 `LocalAnalyticsIdentity::new(<app_data>/analytics)`
- `src/space_setup.rs`：把 `analytics_identity` 透传到 `SpaceSetupDeps`
- 三个 e2e 测试（slice1/slice2 phase1/phase2）：fixture 加 `analytics_identity: NoopAnalyticsIdentity`

### docs
- `docs/architecture/telemetry-events.md`：
  - §3.1 三层 ID 表新增 `space_person_id` 行
  - §3.1 末尾红线补 `space_person_id` 独立性约束
  - §3.2 持久化路径改为 `<app_data>/analytics/` 目录布局，覆盖三个文件
  - §3.3 重置语义补"v2 reset 同时清 `space_person_id`、不影响其他设备、自动发 `$identify`"
  - §3.4 全面重写为"v2 跨设备 person 聚合"小节，含 AnalyticsPersonId 形态、生成时机表、v1→v2 升级策略、wire 互操作语义
  - §4 EventContext 加 `analytics_person_id: AnalyticsPersonId` 字段（标注 `#[serde(skip)]`）
  - §7.0 新增系统事件章节（`$identify` / `$groupidentify`）
  - §10.1 PostHog wire 示例补 `$groups`；字段映射表新增 `$groups` 行；`v2 distinct_id 切换`说明；`$set 不接受 null` 同款语义扩展到 `$groups`

## 测试通过情况（最终）

- `cargo build --workspace`：✅
- `cargo test -p uc-observability --lib`：119 passed
- `cargo test -p uc-application --lib`：481 passed
- `cargo test -p uc-infra --lib`：298 passed (1 ignored)

总计 898 个测试通过。
