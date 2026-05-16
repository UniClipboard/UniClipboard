# Task Plan: 098 · Telemetry 跨设备 Person 聚合 (v2)

## 目标

让"同一真实用户的多台设备"在 PostHog 上聚合为同一个 person，同时保留按设备维度切片留存/funnel 的能力。

现状 (v1)：每台设备各自生成 `anonymous_user_id` (UUIDv7) → PostHog 把每台设备当作独立 person。

v2 后：同一 Space 内所有 paired 设备 → 同一个 person；多个 Space 之间互不污染；Space 维度通过 PostHog Group Analytics 表达。

## 当前阶段

设计完成 (2026-05-15)，等待 PR 1 启动。

## 关键非目标 (本期不做)

- 不引入账号/登录体系。匿名身份模型保持不变 (schema doc §3.3)。
- 不改变隐私契约 (schema doc §6)。`space_person_id` 仍是无 PII 的 UUIDv7。
- 不破坏 v1 wire 兼容 (schema doc §8)。`distinct_id` 字段名不变，只是值的来源从 `anonymous_user_id` 切换到 `space_person_id`，且通过 `$identify` 走 PostHog 标准合并路径。
- 不处理"同一设备多个真实用户"的场景 —— 任何 person 模型都做不到，留给系统层账号体系解决。
- 不实现 client-side 应用级事件队列。`PosthogSink` 仍走 fire-and-forget (schema doc §10.1)。
- 不引入"账号系"的 `distinct_id` —— `space_person_id` 与未来可能上线的账号 ID 必须保持 disjoint。

## 已对齐的需求

1. **Person 边界 = 同一 Space 的所有 paired 设备**。同 Space 多设备 → 一个 person。
2. **ID 分发 = sponsor 派发**。A1 创建 Space 时生成 `space_person_id`；A2 加入时通过 pairing 加密通道携带给新设备。
3. **同时引入 PostHog Group Analytics** (`$groups: { space: space_id_hash }`)，提供 Space/团队维度切片，与 person 维度互补不替代。
4. **完整 v2 分 8 个 PR 推进**。

## 设计概览

### 身份模型 (新)

```rust
// crates/uc-observability/src/analytics/context.rs
pub enum AnalyticsPersonId {
    /// 未加入 Space 的设备：仍用本机 anonymous_user_id 当 distinct_id
    Solo(Uuid),
    /// 已加入 Space：用 sponsor 派发的 space_person_id (同 Space 多设备共享)
    SpaceShared(Uuid),
}
```

`EventContext` 新增 `pub analytics_person_id: AnalyticsPersonId`。原有 `anonymous_user_id` / `analytics_device_id` 字段保留不动 (wire 向后兼容)。

`build_event_payload` (`sinks/mod.rs:62`) 改为从 `analytics_person_id` 派生 `distinct_id`，而非直接拷 `anonymous_user_id`。

### `space_person_id` 生成与分发

- **生成**：A1 flow 中 `setup/initialize_space.rs` 创建新 Space 时同步生成独立的 `space_person_id = Uuid::now_v7()`，**与 `space_id` 完全 disjoint** (不可反推)。
- **持久化**：和 `installation_id` / `analytics_device_id` 同位，存 `general.space_person_id`，不进入 settings 导出 / 同步范围。
- **分发**：A2 flow 中 sponsor 把 `space_person_id` 作为 `JoinerHandshakeOutcome` 的新字段，通过现有 pairing 加密通道传给新设备。新设备在 `redeem_invitation` 持久化阶段一同落盘。

**红线**：
- `space_person_id` 持久化在 settings 的 analytics 命名空间，**不进入 Space 业务数据**。
- `space_person_id` ≠ `space_id` 哈希，**不可** 从前者反推后者 (独立 UUIDv7)。
- 业务层不消费此字段；不参与 trust / membership 决断。
- 与未来账号 ID 保持 disjoint (schema doc §3.3 精神)。

### `$identify` 事件 (distinct_id 切换时机)

PostHog 的标准 person 合并机制：发一条 `$identify` 事件，把旧 anonymous `distinct_id` 链接到新 `distinct_id`，服务端自动合并 person profile 与历史事件。

| 时机 | 旧 distinct_id | 新 distinct_id | 备注 |
|---|---|---|---|
| A1 `setup_completed` 后 (创建新 Space) | `anonymous_user_id` | 本机刚生成的 `space_person_id` | sponsor 设备自身 identify |
| A2 `pairing_succeeded` 后 (加入现有 Space) | `anonymous_user_id` | 从 sponsor 收到的 `space_person_id` | 新设备 identify |
| `switch_space` 完成 | 当前 `space_person_id` | 新 Space 的 `space_person_id` | 跨 Space 切换 (v2 后期) |
| 用户重置 telemetry | 当前 distinct_id | 新 `anonymous_user_id` | 退回 Solo 状态 |

`$identify` payload：

```json
{
  "event": "$identify",
  "distinct_id": "<new space_person_id>",
  "properties": {
    "$anon_distinct_id": "<old anonymous_user_id>",
    "$set": { "active_device_count": 2 },
    "$set_once": { "first_paired_at": "..." }
  }
}
```

**约束**：`$identify` 只在 distinct_id **变化** 时发一次，不是每事件都发。

### Group Analytics (Space 维度)

每个普通事件 properties 增加：

```json
"$groups": { "space": "<space_id_hash>" }
```

Space 首次 paired 时发一条 `$groupidentify`：

```json
{
  "event": "$groupidentify",
  "distinct_id": "<space_person_id>",
  "properties": {
    "$group_type": "space",
    "$group_key": "<space_id_hash>",
    "$group_set": { "created_at": "...", "device_count": 1 }
  }
}
```

PostHog 控制台后续可：
- 按 person 切片留存 (多设备聚合，方案 A 提供)
- 按 space (group) 切片留存 (团队/家庭维度，方案 C 提供)

### reset / switch_space 语义

| 操作 | `anonymous_user_id` | `analytics_device_id` | `space_person_id` | distinct_id 切换 |
|---|---|---|---|---|
| 用户重置 telemetry | 重新生成 | 重新生成 | **清空**，退回 Solo | 发 `$identify` 回 anonymous |
| 退出 Space (v2 后期) | 不变 | 不变 | 清空 | 同上 |
| 加入新 Space | 不变 | 不变 | 替换 | 发 `$identify` 到新 |

**关键点**：本机 reset **不影响** 其他设备。其他设备仍持有原 `space_person_id`，Space 维度的 person 不消失，只是本机被切回 Solo。

## 关键代码锚点

- 身份模型：`src-tauri/crates/uc-observability/src/analytics/context.rs:25-62` (`EventContext`)
- ID 持久化：`src-tauri/crates/uc-observability/src/analytics/ids.rs`
- distinct_id 派生：`src-tauri/crates/uc-observability/src/analytics/sinks/mod.rs:62-90` (`build_event_payload`)
- PostHog sink：`src-tauri/crates/uc-observability/src/analytics/sinks/posthog.rs`
- Analytics port：`src-tauri/crates/uc-observability/src/analytics/port.rs`
- A1 use case：`src-tauri/crates/uc-application/src/usecases/setup/initialize_space.rs`
- A2 use case：`src-tauri/crates/uc-application/src/usecases/pairing/redeem_invitation.rs:144,218`
- Pairing outcome 结构：`src-tauri/crates/uc-application/src/pairing_outbound/joiner_handshake.rs:65-77` (`JoinerHandshakeOutcome`)
- Pairing sponsor 端：`src-tauri/crates/uc-application/src/pairing_inbound/...` (sponsor confirm payload 写入点)
- Switch space：`src-tauri/crates/uc-application/src/usecases/setup/switch_space/mod.rs`
- Telemetry schema doc：`docs/architecture/telemetry-events.md` (§3 身份、§4 EventContext、§7 事件清单、§10.1 PostHog 实务)

## 架构约束 (必读)

- **schema doc §3.1**：analytics 模块 **不允许** 读取或派生自 `uc-core::DeviceId`。`space_person_id` 必须独立生成，不能从业务身份派生。
- **schema doc §6.1**：永不上传 PII。`space_person_id` 是无 PII 的 UUIDv7，符合契约。
- **schema doc §8**：wire 演化非破坏。`distinct_id` 字段名不变，只换取值来源；引入 `$identify` 是 PostHog 标准事件，不破坏现有 sink 形态。
- **uc-application AGENTS §11.4**：外部 crate 只能通过 `src/facade/` 访问 uc-application。新增的 wiring (如 PR 4/5/6 把 space_person_id 注入 use case) 必须经对应 Facade 暴露；业务子模块保持 `pub(crate)`。
- **GUI 走 in-process facade**：所有埋点路径走 `AppFacade`，不经 webserver。
- **文档/注释中文**；新文件 `//!` 头条段先讲"为什么需要这个模块"。
- **不留并行新旧代码**：v1 → v2 的过渡策略一次性敲定，不接受"双轨运行"的中间态 (见 §开放问题)。

---

## Phases

### PR 1: ID 持久化 + `AnalyticsPersonId` 类型

**目的**：在 `uc-observability` 内部引入 `space_person_id` 的 wire 表达和持久化能力，不接 sink，不影响现有埋点。

**改动文件**：
- `crates/uc-observability/src/analytics/context.rs`：新增 `AnalyticsPersonId` enum，`EventContext` 加字段 (默认 `Solo(anonymous_user_id)`)
- `crates/uc-observability/src/analytics/ids.rs`：新增 `load_or_init_space_person_id` / `clear_space_person_id` / `set_space_person_id` 函数 (镜像现有 `installation_id` API)
- `crates/uc-observability/src/analytics/mod.rs`：`pub use` 新类型

**测试**：
- `space_person_id` 持久化 round-trip
- `AnalyticsPersonId::Solo` / `SpaceShared` 序列化形态
- `EventContext` 序列化字段名 (snake_case 不变)

**不做**：还没改 `build_event_payload`；sink 仍发 `distinct_id = anonymous_user_id`。

---

### PR 2: `build_event_payload` 切换 distinct_id 源

**目的**：把 `distinct_id` 派生从 `anonymous_user_id` 改成 `analytics_person_id`。

**改动文件**：
- `crates/uc-observability/src/analytics/sinks/mod.rs:62` (`build_event_payload`)：根据 `ctx.analytics_person_id` 取 distinct_id
- 现有所有测试更新 fixture (大多数 `Solo` 状态下值不变，少量 `SpaceShared` 用例)

**测试**：
- `Solo` 状态下 `distinct_id == anonymous_user_id` (向后兼容)
- `SpaceShared` 状态下 `distinct_id == space_person_id`
- properties 仍保留 `anonymous_user_id` flat 字段 (schema doc §10.1 "Flat-name 字段同时保留")

**不做**：还没引入 `$identify`，所以 `SpaceShared` 状态下历史事件归不到新 person。

---

### PR 3: `$identify` 事件 + `PosthogSink` 支持

**目的**：让 sink 能发 `$identify` (与业务事件并列的系统事件)。

**改动文件**：
- `crates/uc-observability/src/analytics/port.rs`：`AnalyticsPort` 增加 `identify(old_distinct_id, new_distinct_id, set, set_once)` 方法
- `crates/uc-observability/src/analytics/sinks/posthog.rs`：`build_capture_body` 支持 `$identify` event；wire 形态遵守 §10.1
- `crates/uc-observability/src/analytics/sinks/stdout.rs`：dev 路径打印
- `crates/uc-observability/src/analytics/sinks/gated.rs`：`identify` 走同样的 gate

**测试**：
- `$identify` wire 形态符合 PostHog spec (`$anon_distinct_id` 在 properties 而非顶层)
- analytics gate 关闭时 `identify` 不发
- Stdout sink dev 模式下输出可读形态

**不做**：还没有任何 use case 调用 `identify`。

---

### PR 4: A1 `setup_completed` 触发 identify

**目的**：sponsor 设备在创建新 Space 时生成 `space_person_id` 并 identify。

**改动文件**：
- `usecases/setup/initialize_space.rs`：在 `SetupStatus.has_completed = true` 落地之后、emit `setup_completed` 之前：
  1. 生成新的 `space_person_id = Uuid::now_v7()` 并 persist
  2. 调用 `analytics.identify(old=anonymous_user_id, new=space_person_id, ...)`
  3. 更新进程级 `EventContext` (replace via `set_global_event_context`)
  4. 然后 emit `setup_completed` (此事件已经用新 distinct_id)
- bootstrap wiring：`facade/space_setup/facade.rs` 注入 ids 持久化层

**测试**：
- A1 happy path：identify 在 setup_completed 之前发出
- `space_person_id` persist 失败时不 emit identify (复用 setup_completed_not_emitted_on_failure 模式)
- replace_global_event_context 后续事件 distinct_id 正确

**不做**：A2 路径还没接；现有 v1 用户升级到 v2 时 sponsor 设备的 `space_person_id` 空缺 —— 见 §开放问题。

---

### PR 5: Sponsor 端 pairing payload 携带 `space_person_id`

**目的**：sponsor 在 pairing handshake 中把 `space_person_id` 通过加密通道发给 joiner。

**改动文件**：
- pairing wire protocol：sponsor confirm payload (or equivalent) 增加 `space_person_id: Uuid` 字段
- `pairing_inbound/sponsor_handshake.rs` (具体路径以代码为准)：构造 payload 时填入本机 `space_person_id`
- `pairing_outbound/joiner_handshake.rs:65-77` (`JoinerHandshakeOutcome`)：新增 `sponsor_space_person_id: Uuid` 字段
- pairing wire schema 文档更新

**测试**：
- sponsor 端 payload 序列化包含字段
- joiner 端 outcome 字段被正确解析
- 老版 sponsor (v1) 与新版 joiner (v2) 互操作：joiner 收到 None → 走 fallback (临时生成本地 `space_person_id`，发 `pairing_failed`？还是按 Solo 继续？) —— 见 §开放问题

**不做**：joiner 端还没用这个字段做 identify。

---

### PR 6: A2 redeem 端 identify

**目的**：joiner 设备完成 pairing 后用 sponsor 派发的 `space_person_id` identify。

**改动文件**：
- `usecases/pairing/redeem_invitation.rs:144-204`：在 `space_id` adopt 之后、emit `pairing_succeeded` 之前：
  1. 把 outcome.sponsor_space_person_id persist 到本机 settings
  2. 调用 `analytics.identify(old=anonymous_user_id, new=sponsor_space_person_id, ...)`
  3. 更新进程级 `EventContext`
  4. 然后 emit `pairing_succeeded` (已经用新 distinct_id)
- bootstrap wiring：`facade/space_setup/facade.rs` 注入

**测试**：
- A2 happy path：identify 在 pairing_succeeded 之前发出
- 持久化失败时不 emit identify
- 多设备加入同一 Space → PostHog 上是同一 person (集成测试 mock sink 断言)

**不做**：v1 升级路径处理 (§开放问题)。

---

### PR 7: `$groups` + `$groupidentify`

**目的**：每事件附带 Space 维度的 group key，首次 Space 形成时发 `$groupidentify`。

**改动文件**：
- `crates/uc-observability/src/analytics/sinks/posthog.rs`：`build_capture_body` 在 ctx 有 `space_id_hash` 时 inject `$groups: { space: <hash> }`
- `crates/uc-observability/src/analytics/port.rs`：`AnalyticsPort` 增加 `group_identify` 方法
- A1 `initialize_space.rs` 在 identify 之后立即 group_identify 第一台设备
- A2 `redeem_invitation.rs` 不发 group_identify (sponsor 已经发过)，但 device_count 通过 `$set` 自动更新

**测试**：
- `$groups` 在 ctx 有 space_id_hash 时出现；Solo 状态下不出现
- `$groupidentify` wire 形态符合 PostHog spec

---

### PR 8: reset / switch_space 联动 + UI

**目的**：闭环用户可见的 ID 重置和 Space 切换语义。

**改动文件**：
- 用户重置 telemetry 入口 (settings UI + 后端 use case)：清 `space_person_id` + 重新生成 `anonymous_user_id` + `analytics_device_id` + 发 identify 回 Solo
- `usecases/setup/switch_space/mod.rs`：commit 完成后切换到目标 Space 的 `space_person_id` (从其 sponsor 那侧拿)，发 identify
- Settings 页面文案更新

**测试**：
- reset 后 distinct_id 回到 Solo 形态
- switch_space 后 distinct_id 切换到新 Space
- reset 不影响同 Space 其他设备 (本机隔离)

---

## Schema doc 修订计划

PR 1 / PR 4 / PR 7 / PR 8 各自负责对应章节更新 (不要一次性大改 schema doc)：

- §3.1 三层 ID 表新增第 4 行 `space_person_id` (PR 1)
- §3.2 持久化路径补 `general.space_person_id` (PR 1)
- §3.3 重置语义补"reset 会清 `space_person_id`，但不影响其他设备" (PR 8)
- §3.4 v2 口径升级：把"共享 anonymous_user_id"改为"共享 space_person_id" (PR 6)
- §4 `EventContext` 字段表新增 `analytics_person_id` enum (PR 1)
- §7 新增 `$identify` / `$groupidentify` 两条系统事件 (PR 3 / PR 7)
- §10.1 PosthogSink wire 形态示例补充 `$groups` 字段；字段映射表新增 `$groups` 行 (PR 7)

每个 PR 必须同步更新对应章节，且在 `docs/changelog/*.md` 登记。

---

## 开放问题 (PR 启动前敲定)

1. **v1 → v2 升级时已有 Space 的处理**

   场景：v1 期间已经配对的设备升级到 v2 后，本机没有 `space_person_id`。

   候选方案：
   - **A**：什么都不做。已升级设备继续按 Solo 发事件 (`distinct_id = anonymous_user_id`)，直到该 Space 内某台设备完成新的 pairing (即新设备加入) 时才生成 `space_person_id` 并下发给在线设备。代价：v2 启动后已配对的设备仍是设备级 person，直到下次 pairing；dashboard 上呈现"老用户 v1 行为 + 新用户 v2 行为"的混合。
   - **B**：升级时由设备投票/选举生成确定性 `space_person_id` (HMAC 派生)，老设备 v2 启动后自动迁移。代价：违反"reset 应该重新算"的精神 (但仅对老用户)。
   - **C**：v2 启动检测到老 Space，触发"一次性 re-pairing"流程让用户重新配对一次。代价：UX 摩擦大。

   倾向：A (最低侵入)，但请确认。

2. **Pairing payload v1↔v2 互操作**

   v1 sponsor + v2 joiner：joiner 期望的 `space_person_id` 字段在 sponsor payload 里缺失。

   候选方案：
   - **A**：joiner 把字段标为 `Option<Uuid>`；None 时 joiner 退化为 Solo (本机生成临时 `anonymous_user_id` 作 distinct_id，等下次再有新 sponsor 派发)。
   - **B**：把 v2 sponsor 的 `space_person_id` 字段作为 wire schema 强制项，老 sponsor 加 minor version bump，互操作时 fall back 到 v1 行为。

   倾向：A (forward-compatible)。

3. **`$identify` 失败时的 fallback 策略**

   PosthogSink 是 fire-and-forget，`$identify` 失败后本机已经把 `space_person_id` 持久化，后续事件的 `distinct_id` 已切换 —— 但 PostHog 服务端没收到 alias，老 anonymous person 与新 space person 不会合并。

   候选方案：
   - **A**：维持 fire-and-forget，schema doc §10.1 已允许 < 1% 丢失。罕见情况下损失老事件归属。
   - **B**：bootstrap 启动时检测"本机已是 SpaceShared 但 identify 未确认"标志，重发 identify。增加状态追踪复杂度。

   倾向：A。

4. **`switch_space` 跨 Space identity 切换的事务性**

   `switch_space` 失败时如果已经发 identify，PostHog 上 person 已切走，但本机仍在旧 Space。需要 emit 一条"identify 回滚"事件？还是允许暂时不一致？

   倾向：identify 在 switch_space commit phase 之后发，commit 失败则不发。

5. **`space_person_id` 进入 settings 导出范围？**

   `analytics_device_id` 明确不进入导出 (schema doc §3.2)，因为"必须随设备绑定"。`space_person_id` 反过来：必须随 Space 绑定，跨设备共享。

   决策：`space_person_id` 进入 Space 级别的同步范围 (与 `space_id` 同位)，但 **不** 进入"settings export to file"导出 (避免泄露给 backup 文件)。

6. **Group device_count 更新时机**

   `$groupidentify.$group_set.device_count` 由谁负责更新？sponsor 每次新设备 pairing 完都重发 `$groupidentify`？还是各设备 emit 事件时通过 `$groups.$set` 自己更新？

   倾向：sponsor 在 `pairing_succeeded` (本机视角的"接受新设备") 之后重发 `$groupidentify`，把 `device_count` 自增。

---

## 后续

PR 1 启动时复制本文件到 `findings.md` / `progress.md` 起点。每个 PR 完成后在 progress.md 加一行 + 更新 schema doc 对应章节。
