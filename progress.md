# Progress Log

## Session 2026-05-09 — Issue #549 启动 + Slices 1-5

**目标**：为 UniClipboard 建立产品 telemetry 体系，覆盖 Activation 漏斗与 Reliability 同步可靠性。后端选 PostHog Cloud（EU），架构与 SDK 解耦。

**单一真相**：`docs/architecture/telemetry-events.md`

### 决策对话脉络

1. 用户问 issue #549 怎么做 → 推荐 PostHog Cloud（不自研、与 Sentry 互补）
2. 用户确认理解 PostHog 与 Sentry 区别 → 推荐 SDK 接入（complexity low）
3. 起草 schema doc（§1-§11，含 4 个开放问题）
4. 用户裁决 4 个开放问题：
   - 双开关（`telemetry_enabled` + `usage_analytics_enabled`）
   - `active_device_count` 进程启动读一次
   - 留存口径设备级，Space 级延后
   - 后端 PostHog Cloud
5. 用户初版 doc 中"`device_id`"字段歧义（与业务 `DeviceId` 重名）→ 改名 `analytics_device_id`，约束 disjoint

### Slice 1 — `analytics_gate` 模块 ✅

**文件**：
- `uc-observability/src/analytics_gate.rs` —— 新增，~75 行
- `uc-observability/src/lib.rs` —— pub mod + re-exports

**测试**：3 个（默认值、setter round-trip、与 telemetry_gate 隔离性）

### Slice 2 — 事件类型骨架 + AnalyticsPort trait ✅

**文件**：
- `uc-observability/src/analytics/mod.rs` —— 模块声明 + re-exports
- `uc-observability/src/analytics/context.rs` —— EventContext + Os/Arch/AppChannel/InstallSource
- `uc-observability/src/analytics/events.rs` —— Event enum + 8 子枚举 + buckets + SyncEventProps
- `uc-observability/src/analytics/port.rs` —— AnalyticsPort trait + NoopAnalyticsSink
- `uc-observability/Cargo.toml` —— +chrono serde、+uuid serde

**关键决策**：
- wire 形态钉死测试（`enum_variants_serialize_to_documented_strings` 等）防止后续误改字符串
- `Event::properties()` 不含 context 字段
- `SyncEventProps::Option` 字段用 `skip_serializing_if`，避免 PostHog 误判 null
- `AnalyticsPort` 同步 fire-and-forget + trait object safe

**测试**：22 → 全绿

### Slice 3a — ID 持久化纯模块 ✅

**文件**：
- `uc-observability/src/analytics/ids.rs` —— ~330 行（含 10 tests）

**API 表面**：
- `AnalyticsIds { anonymous_user_id, analytics_device_id, is_first_run }`
- `load_or_create(&Path) -> Result<AnalyticsIds>`
- `reset(&Path) -> Result<()>`

**关键决策**：
- `is_first_run` 严格语义：只有"两个 ID 都新生成"才标 true
- 部分损坏 / 缺失 → 缺哪个补哪个 + warn log，不触发完整重置
- 原子写：`<file>.tmp` → `rename(2)`
- 不感知 `AppPaths`，纯函数易测
- 不做文件锁，依赖调用方序列化

**测试**：10（含 first_run / 复用 / 单文件丢失 / 损坏 / 写出格式 / 空白容忍 / reset 行为 / reset 幂等 / 目录创建 / 原子性副作用）

### Slice 4 — EventContext factory + 全局注册 + 平台探测 ✅

**文件**：
- `uc-observability/src/analytics/context.rs` —— 重写（移除 timestamp 字段，+`Os::Other`，+factory，+全局注册）
- `uc-observability/src/analytics/probe.rs` —— 新增 ~140 行
- `uc-observability/src/analytics/mod.rs` —— +probe re-export
- `docs/architecture/telemetry-events.md` —— §4 标注 timestamp 是事件级

**关键决策**：
- timestamp 移出 EventContext，由 sink 在 capture 时打或 PostHog SDK 自动注入
- `Os::Other` 兜底 FreeBSD / 嵌入式 unix
- `RwLock<Option<Arc<EventContext>>>` 全局存储（支持替换，重置 telemetry IDs 流程依赖）
- factory 切分输入与探测：调用方提供身份/版本/Space，平台字段走 probe
- 探测失败一律 `"unknown"`，不返回 `Result`
- stdlib + chrono only：v1 已知局限（os_version 占位、Windows locale 命中率低、timezone 为 offset 而非 IANA）写在模块文档里
- POSIX locale 归一化：`zh_CN.UTF-8` → `zh-CN`，`sr_RS@latin` → `sr-RS`，`C` / `POSIX` → `unknown`

**测试**：48 passed（context 新增 5 + probe 全新 10）

### Slice 5 — settings 双开关 + bootstrap gate 拼装 ✅

**文件（跨 6 crate，~66 行净增）**：
- `uc-core/src/settings/model.rs` +字段 +默认 fn
- `uc-core/src/settings/defaults.rs` +默认值
- `uc-application/src/facade/settings/models.rs` +view+patch+convert+update
- `uc-application/src/facade/app_facade.rs` +显式构造点补字段
- `uc-daemon-contract/src/api/dto/settings.rs` +DTO+patchDTO+convert+default fn
- `uc-webserver/src/api/settings.rs` +取值+gate setter+patch+view
- `uc-webserver/tests/settings_network_smoke.rs` +显式构造点
- `uc-bootstrap/src/tracing.rs` +resolve_usage_analytics_enabled +set_analytics_enabled init
- `uc-observability/src/analytics/context.rs` 合并 global tests 消除竞态

**关键决策**：
- 保持 `telemetry_enabled` 字段名不变（零迁移）
- 两个 gate 在 PUT handler 独立分支调用，物理隔离
- bootstrap init 多加一步与 telemetry gate 完全对称
- 全局 RwLock 测试合并到单一 fn 串行化（避免 `serial_test` 依赖）

**遇到的问题**：
- `Uuid` / `chrono::DateTime` 缺 `serde` feature → 加 feature flag
- 两处显式构造点缺字段 → 分别在 `app_facade.rs` 和 `settings_network_smoke.rs` 补
- 全局 EventContext 测试并行竞态 → 合并测试

**预先存在的失败**（git diff 已确认未碰，不归本任务）：
- `uc-daemon-contract::api::auth::DaemonConnectionInfo::fmt` doctest 缺 `use`
- `uc-platform`、`uc-tauri` 几个 doctest 同样 scope 问题

**测试**：
- uc-observability lib：47 passed
- uc-core / uc-application / uc-daemon-contract / uc-webserver / uc-bootstrap：377 / 84 / 27 / 39 / 4 全绿

## 当前状态总览

### 已完成

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿 | ✅ |
| Slice 1 | `analytics_gate` 模块 | ✅ |
| Slice 2 | 事件类型 + AnalyticsPort | ✅ |
| Slice 3a | ID 持久化纯模块 | ✅ |
| Slice 4 | factory + 全局 + probe | ✅ |
| Slice 5 | settings 双开关 + bootstrap gate | ✅ |

### 待完成

| Slice | 内容 | Blocker |
|---|---|---|
| Slice 6 | bootstrap 拼装 EventContext | `active_device_count` 取数源决策；调用点选择 |
| Slice 7 | StdoutSink + PosthogSink | 需 PostHog Cloud account + project key |
| Slice 8 | use case 埋点 | 等 Slice 6 / 7 |
| Slice 9 | 前端 settings UI 拆开关 | 需要前端工作 |
| Slice 10 | dashboard + 验收 | 需积累一周真实数据 |

## Schema doc §11 验收清单

- [x] EventContext 字段穷举且无内容泄露字段
- [x] Activation + Reliability 两段事件清单完备，properties schema 闭合
- [x] 命名规范、隐私契约、演化策略各自有明确条款
- [x] §10 四项开放问题已裁决并落到对应章节
- [x] 无任何 PostHog / Sentry 名称硬编码进 schema 类型签名

子任务 2 启动前清单：

- [x] `AnalyticsPort` trait 定义
- [x] `analytics_gate` 模块实现
- [x] 配置目录中 `installation_id` / `analytics_device_id` 持久化逻辑（纯模块层）
- [ ] settings UI 拆分两个开关并补齐文案 ← Slice 9
- [ ] dev 构建下事件 stdout 打印通路 ← Slice 7

## Files Changed Summary

```
crates/
├── uc-core/src/settings/{model,defaults}.rs            (Slice 5)
├── uc-application/src/facade/{app_facade,settings/models}.rs  (Slice 5)
├── uc-daemon-contract/src/api/dto/settings.rs          (Slice 5)
├── uc-webserver/src/api/settings.rs                    (Slice 5)
├── uc-webserver/tests/settings_network_smoke.rs        (Slice 5)
├── uc-bootstrap/src/tracing.rs                         (Slice 5)
└── uc-observability/
    ├── Cargo.toml                                      (Slice 2)
    ├── src/lib.rs                                      (Slice 1, 2)
    ├── src/analytics_gate.rs                           (Slice 1, 新增)
    └── src/analytics/                                  (Slice 2-4, 新增)
        ├── mod.rs
        ├── context.rs
        ├── events.rs
        ├── ids.rs
        ├── port.rs
        └── probe.rs

docs/architecture/telemetry-events.md                   (schema doc, 新增)
```

## Next Action

进入 Slice 6：bootstrap 拼装 EventContext。需先决定：
1. 调用点：`init_tracing_subscriber` 早期 vs `wire_dependencies` 之后
2. `active_device_count` v1 用 `0` 占位（最简）还是接 membership repo

---

## Session 2026-05-09 续 — Slice 6（根本性重构） ✅

**决策对话脉络**：
1. 我先按"v1 简单"思路推荐方案 A（`init_tracing_subscriber` 内 + `0` 占位），用户拒绝："我要根本性的优化和重构，不要 fast ship"
2. 重新出案：架构上把 EventContext 装配从 sync 早期路径搬到 `wire_dependencies` 之后；让 `build_core` 转 async
3. 用户裁决 4 个子问题：(1) build_core 转 async 同意；(2) app_channel 从版本号 **前缀** 解析；(3) install_source **暂时不做**，固定 Unknown；(4) space_id_hash **算** 入 Slice 6 范围

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| call site | `wire_dependencies` 之后、`build_core` 内 | 单一调用点，未来加 async 装配步骤不用四处补；`init_tracing_subscriber` 保持职责纯净（只做 subscriber + 两个 gate） |
| `build_core` 签名 | sync → async | 装配 EventContext 必须 await `member_repo.list()` / `setup_status.get_status()`；3 个 builder + `build_cli_app_facade` 跟着 async；GUI shell sync `run()` 用 `tauri::async_runtime::block_on` 桥接 |
| `active_device_count` | `member_repo.list().await.len() as u32` | v1 不糊弄；schema doc §4 "启动读一次缓存"语义现在真正成立 |
| `space_id_hash` | `SHA-256(space_id)[..16 hex]`，未 setup → None | schema doc §6.3：原始 `space_id` 永不上传；64 bit 截断已足够跨事件聚合 |
| `app_channel` | 解析版本号前缀（`-alpha*` / `-beta*` / 其他 prerelease 退化 Alpha / 干净 semver Stable） | 无配置维护成本；与现有 release-please 流程对齐；rc/dev 也按"未 GA"走 Alpha，避免误标 stable |
| `install_source` | v1 固定 `Unknown` | release pipeline 没准备好，schema 字段保留 |
| 幂等性 | `compose_event_context` 检测 `global_event_context().is_some()` 即跳过 | GUI in-process daemon 场景下 GUI 的 `build_gui_app` + daemon 的 `build_core` 会两次触达；不去重会让第二次的 `is_first_run=false` 覆盖第一次的 `true`，丢失"首次激活"信号 |

### 文件改动（10 个文件，~120 行净增）

```
src-tauri/crates/uc-bootstrap/
├── Cargo.toml                                  +sha2 = "0.10"
├── src/lib.rs                                  +pub mod analytics; pub use compose_event_context
├── src/analytics.rs                            新增 ~180 行（含 6 unit tests）
├── src/builders.rs                             build_core / 3 builder → async；build_core 末尾调 compose
└── src/non_gui_runtime.rs                      build_cli_app_facade → async

src-tauri/crates/uc-desktop/src/bootstrap.rs    build_gui_app → async + 调 compose
src-tauri/crates/uc-tauri/src/run.rs            block_on(build_gui_app())
src-tauri/crates/uc-cli/src/commands/
├── status.rs                                   .await
├── upgrade.rs                                  .await
└── search.rs                                   build_search_facade → async + .await
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-bootstrap (lib) | **19** (含 6 个新增：parse_app_channel × 4 + hash_space_id × 3) |
| uc-bootstrap (e2e: slice1 / slice2_p1 / slice2_p2) | 1 + 2 + 2 |
| uc-observability | 47 (无变化) |
| uc-application | 377 |
| uc-cli | 31 |
| uc-desktop | 48 |
| uc-webserver | 39 |

整体 build：`cargo check` 跨 11 个 crate 一次通过；`cargo test` 五个核心 crate 全绿。

### 遇到的问题

| 问题 | 解决 |
|---|---|
| 想直接 `AnalyticsIds::load_or_create`，但 ids.rs 暴露的是自由函数 | 改用 re-exported 名字 `load_or_create_ids` |
| GUI 进程内拉起 daemon 会让 `compose_event_context` 触达两次，第二次 `is_first_run=false` 会覆盖第一次 `true` | `compose_event_context` 入口检 `global_event_context().is_some()` 跳过 |
| `uc-cli/commands/search.rs` 中 sync helper `build_search_facade` 调 sync builder | helper 转 async，caller 加 `.await`（caller 本来就是 async fn run） |

### 跳过的事项

- **integration test for `compose_event_context`**：评估为低 ROI。要在 tests/ 下构造完整 AppDeps（~20 个 fake port），现有 47 个 uc-observability 测试已覆盖 EventContext factory + 全局注册行为；compose 函数本身的纯部分（hash_space_id、parse_app_channel）已有 6 个 unit test；async 部分（read_active_device_count / read_space_id_hash）已是单调 wrapper。完整 wiring 验证通过 `slice1_handshake_e2e` 这种已有 E2E 间接覆盖。

## Slice 6 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿 | ✅ |
| Slice 1 | `analytics_gate` 模块 | ✅ |
| Slice 2 | 事件类型 + AnalyticsPort | ✅ |
| Slice 3a | ID 持久化纯模块 | ✅ |
| Slice 4 | factory + 全局 + probe | ✅ |
| Slice 5 | settings 双开关 + bootstrap gate | ✅ |
| Slice 6 | bootstrap 拼装 EventContext + build_core async 化 | ✅ |
| Slice 7 | StdoutSink + PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8 | use case 埋点 | 等 7 |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

---

## Session 2026-05-09 续 — Slice 7a（StdoutSink + 共享 wire 合并） ✅

**决策对话脉络**：
1. 用户"继续下一个任务"——Slice 7 待开。我探查后发现两个未决（task_plan Key Q #3 #4），不动手先问
2. AskUserQuestion 一次性问两个：
   - Slice 7 范围 → 用户选"仅做 StdoutSink"
   - Sink 切换机制 → 用户选"runtime（推荐）"
3. 据此把 Slice 7 拆为 7a（StdoutSink，本次完成）/ 7b（PosthogSink，等 PostHog 账号），同时把 sink 注入 AppDeps 的工作明确划归 Slice 8

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| 输出 channel | `tracing::debug!` (target = `uc_observability::analytics`) | schema doc §6.5 明文；与 dual-output（pretty console + JSON file）一致；release 默认级别自然吞掉，避免误投生产 stdout |
| wire 合并 | `build_event_payload(event, ctx) -> Map` 抽到 `sinks/mod.rs` | 跨 sink 共用；StdoutSink / 未来 PosthogSink 字段形态等价；切 sink 不改 dashboard |
| `distinct_id` | 平铺时拷贝自 `anonymous_user_id` | PostHog 漏斗主键约定；schema §3.1 留存锚点 |
| 字段冲突仲裁顺序 | context 先入、event.properties 后入 | events 模块 invariant 守住两者无重叠（`properties_are_pure_event_fields_only` 测试）；万一未来违约时 event-specific 字段优先 |
| context 缺失行为 | 丢事件 + warn 节流（一次/sink 实例） | 半截事件比缺事件更难排查；启动早期可能连发多条会被 `AtomicBool::swap` 收敛到一条 warn |
| 测试串行化 | 全部走 `stdout_sink_lifecycle` 单一 fn | 与 `analytics::context::tests::global_event_context_lifecycle` 同样思路——全局 RwLock 在并行 cargo test 下会竞态，单一 fn 串行避坑 |
| 测试 capture 实现 | 自定义 `MakeWriter` + `tracing_subscriber::registry()` + `with_default` | 不引入 tracing-test / tracing-mock 依赖；零额外 crate |

### 文件改动（3 个文件，~310 行净增）

```
src-tauri/crates/uc-observability/src/analytics/
├── mod.rs                                +pub mod sinks; +pub use sinks::{build_event_payload, StdoutSink}
├── sinks/mod.rs                          新增 ~180 行（build_event_payload + 4 单元测试）
└── sinks/stdout.rs                       新增 ~210 行（StdoutSink + lifecycle 测试）
```

无新依赖（`serde_json`、`tracing` 已在 Cargo.toml；`tracing-subscriber` 已在 dev-dependencies）。

### 测试结果

| crate | tests passed |
|---|---|
| uc-observability (lib) | **52** (新增 5：payload × 4 + stdout_sink_lifecycle × 1) |

未跑跨 crate——本 slice 仅在 observability 内增量加了 pub items，没改任何已有 API 签名，下游不会受影响。

### 跳过的事项

- **PosthogSink**：等 PostHog Cloud account + project key 到位再做，归 Slice 7b
- **sink 注入 AppDeps**：归 Slice 8。不在 7a 范围内提前 wire——让 7a 完全独立可发布
- **e2e 验证 dev 跑起来能看到事件**：需要 Slice 8 把 sink 装到调用点才有意义；7a 的单元测试已覆盖 wire 形态

## Slice 7a 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿 | ✅ |
| Slice 1 | `analytics_gate` 模块 | ✅ |
| Slice 2 | 事件类型 + AnalyticsPort | ✅ |
| Slice 3a | ID 持久化纯模块 | ✅ |
| Slice 4 | factory + 全局 + probe | ✅ |
| Slice 5 | settings 双开关 + bootstrap gate | ✅ |
| Slice 6 | bootstrap 拼装 EventContext + build_core async 化 | ✅ |
| Slice 7a | StdoutSink + 共享 wire 合并 | ✅ |
| Slice 7b | PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8 | sink 注入 AppDeps + use case 埋点 | 可开始（用 StdoutSink 在 dev 验证 wire） |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

---

## Session 2026-05-09 续 — Slice 8a（sink 注入 AppDeps + `app_first_open`） ✅

**决策对话脉络**：
1. 用户"继续"——Slice 7a 已完，进入 Slice 8a
2. 我探查后发现 task_plan.md Slice 8a 子任务里有未决项："gate 运行时切换：装配点决策需对齐"
3. 我列出 3 个候选并推荐 C，用户选 C：在 `capture` 入口的统一守卫
4. 顺带：用户让我把 `build_slice1_cli_context` 这个历史命名重命名

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| gate 运行时切换 | `GatedAnalyticsSink<inner>` wrapper 在 `capture` 入口 atomic 守卫 | gate 是横切关注点，不该污染每个 sink 实现；`StdoutSink` / 未来 `PosthogSink` 都不感知 gate；与 telemetry_gate 在 tracing layer 做 filter 的思路对称 |
| sink 装配寿命 | 装一次永不替换 | settings PUT handler 翻 `usage_analytics_enabled` 时只动 `analytics_gate` 静态值，sink 本身不重建——0 重建成本 |
| AppDeps 字段位置 | `analytics: Arc<dyn AnalyticsPort>` 顶层横切字段 | 与 `settings`、`setup_status` 同层；按"横切关注点不归属任何 *Ports bundle"原则放置 |
| dev/release sink 选择 | `cfg!(debug_assertions)` → `StdoutSink`，否则 → `NoopAnalyticsSink` | dev 跑起来 `RUST_LOG=uc_observability::analytics=debug` 就能看到事件；release 是临时态，等 Slice 7b 接 `PosthogSink` 直接替换 inner，调用方零感知 |
| `app_first_open` 触发点 | `compose_event_context` 内 `set_global_event_context` 之后 + `if ids.is_first_run` | 幂等门控由 compose 顶部 `global_event_context().is_some()` 守住——GUI 进程内拉起 daemon 时 compose 触达两次，第二次直接 return，不会重复 fire |
| 重命名 | `build_slice1_cli_context` → `build_cli_wiring_context` | "Slice 1" 是历史迭代编号；新名字与 `wire_dependencies` / `WiredDependencies` 同源，强调返回值是完整 wiring 而非扁平 `AppDeps` |

### 文件改动（6 个文件，~100 行净增）

```
src-tauri/crates/uc-observability/src/analytics/
├── mod.rs                                +pub use sinks::GatedAnalyticsSink
├── sinks/mod.rs                          +pub mod gated; +pub use gated::GatedAnalyticsSink
└── sinks/gated.rs                        新增 ~100 行（含 1 unit test）

src-tauri/crates/uc-application/src/deps.rs    +use AnalyticsPort; +pub analytics 字段

src-tauri/crates/uc-bootstrap/src/
├── analytics.rs                          +build_analytics_sink(); +Event imports;
│                                          compose 内尾部 if ids.is_first_run
│                                          { deps.analytics.capture(Event::AppFirstOpen) }
├── assembly.rs                           AppDeps 构造点补 analytics 字段
├── builders.rs                           build_slice1_cli_context → build_cli_wiring_context
├── lib.rs                                +pub use analytics::build_analytics_sink;
│                                          re-export 重命名
└── non_gui_runtime.rs                    调用点重命名
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-observability (lib) | **53** (新增 1：`gated_sink_lifecycle` 三 case) |
| uc-bootstrap (lib) | 19（无变化） |
| uc-bootstrap (e2e: slice1 / slice2_p1 / slice2_p2) | 1 + 2 + 2 |
| uc-application | 377 |
| uc-cli | 31 |
| uc-desktop | 48 |
| uc-webserver | 39 |

整体 build：`cargo check --workspace` 跨 11 crate 一次通过。

### 跳过的事项

- **bootstrap test 钉住 sink 类型**：`build_analytics_sink` 返回 `Arc<dyn AnalyticsPort>`，trait object 不可下转回具体类型。要测 dev = `StdoutSink` / release = `NoopAnalyticsSink` 必须把 inner 类型暴露出来。`gated_sink_lifecycle` + `stdout_sink_lifecycle` + `payload` 系列已端到端覆盖 wire 形态；factory 本身的 `cfg!` 分支是 trivial 编译期分歧，新增 mock-trait 钉法 ROI 低
- **`app_first_open` 在 first_run = true / false 两路径行为的 integration test**：评估为低 ROI——`compose_event_context` 的 first-run 分支只是一行 `if ids.is_first_run { deps.analytics.capture(...) }`；要真验证需要构造完整 AppDeps + fake `AnalyticsPort` + 篡改 ids 文件——构造代价 vs 一行代码的覆盖率不划算。`load_or_create_ids` 的 `is_first_run` 语义已被 10 个 ids tests 守住

### 顺带做的事

- **`build_slice1_cli_context` → `build_cli_wiring_context`** 重命名。3 处 .rs 引用全清（builders.rs 定义 / non_gui_runtime.rs 调用 / lib.rs re-export）。`.planning/` 下历史归档不动

## Slice 8a 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿 | ✅ |
| Slice 1 | `analytics_gate` 模块 | ✅ |
| Slice 2 | 事件类型 + AnalyticsPort | ✅ |
| Slice 3a | ID 持久化纯模块 | ✅ |
| Slice 4 | factory + 全局 + probe | ✅ |
| Slice 5 | settings 双开关 + bootstrap gate | ✅ |
| Slice 6 | bootstrap 拼装 EventContext + build_core async 化 | ✅ |
| Slice 7a | StdoutSink + 共享 wire 合并 | ✅ |
| Slice 7b | PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8a | sink 注入 AppDeps + factory + `app_first_open` + GatedAnalyticsSink | ✅ |
| Slice 8b | pairing 三事件 | 可开始（PairingFacade 调 `analytics.capture` 即可） |
| Slice 8c | sync 三事件 + 新增 `FirstSyncStatePort` | 可开始 |
| Slice 8d | setup 两事件 | 可开始 |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

### 验证 dev 跑起来事件链路

`RUST_LOG=uc_observability::analytics=debug cargo run -p uc-cli -- status`（或 GUI dev build）：
- 首次：应看到一行 `app_first_open` JSON（含完整 EventContext 字段）
- 第二次：IDs 已落盘，`is_first_run = false`，不再 fire `app_first_open`
- 关掉 `usage_analytics_enabled` 后所有事件应被 `GatedAnalyticsSink` 静默吞掉

---

## Session 2026-05-09 续 — Slice 8b（pairing 三事件 / joiner 端） ✅

**决策对话脉络**：
1. 用户"继续下一个任务"——进入 Slice 8b
2. 探索发现：实际不存在独立 PairingFacade，pairing 全归 `SpaceSetupFacade`；joiner 端 use case `RedeemPairingInvitationUseCase::execute()` 同步返回 = 完整握手 + persist 完成；sponsor 端走 broadcast `PairingOutcome` 异步路径，且 `Failure { reason: String }` 已丢失结构化信息
3. AskUserQuestion 一次性问 4 决策，全部用户裁决：
   - 仅 joiner 端发三事件（sponsor 端留待后续）
   - PairingMethod v1 固定占位 `Code`（execute 签名零改动）
   - 新增专用 `PairingFailureReason`，与 sync 的 `FailureReason` 解耦
   - use case 单测用 fake AnalyticsPort 验证 capture 调用次数 + variant + failure_reason 映射

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| 触达端 | 仅 joiner 端 | joiner 是 funnel 主路径且 use case 同步语义清晰；sponsor 端 broadcast `Failure { reason: String }` 已丢结构化信息，要发只能在 `PairingInboundOrchestrator` 自带 `Arc<dyn AnalyticsPort>` —— 留 8b' 后续 PR |
| `PairingMethod` 占位 | v1 固定 `Code` | use case 签名零改动；GUI 区分维度（QR / Code / Discovery）当前在更上层处理后都进同一入口，下推到 use case 是更大的 refactor，不阻塞主路径 |
| `PairingFailureReason` enum | 新增专用，与 sync `FailureReason` 解耦 | pairing 与 sync 失败语义不重叠（pairing 关心 passphrase / sponsor 决断；sync 关心 transport / payload）；共享 enum 会让 funnel 漏点信号在跨 domain dashboard 误聚合；schema doc §7.4 写明 domain-specific enum 的演化方向 |
| 14 变体 1:1 映射 | 与 `RedeemPairingInvitationError` 一一对应 | 探索时漏数（最初算 13），wiring 时发现 `SponsorInternal(String)` 单独存在（与本机 `Internal` 不同语义）；funnel 上"sponsor 端 internal"vs"本机端 internal"属不同漏点，值得分开 |
| analytics 注入位置 | `SpaceSetupDeps` 加横切字段 → facade 解构 → use case `new` 参数 | 与 AppDeps（Slice 8a）顶层 `analytics: Arc<dyn AnalyticsPort>` 同层；use case 直接持有 Arc，不依赖全局 |
| capture 时机 | execute 入口 fire `PairingStarted`，Result match 后 fire `Succeeded` / `Failed` | "early dial 失败"也保证 funnel 第一步留信号；实现：把原 `handshake.handshake → persist` 包到 async block，外层 match 后 fire 再 return |
| `peer_os` v1 | 固定 `None`（schema 已是 `Option`） | 当前握手 outcome 没有对端 OS 字段；后续协议加入对端 OS 自报后回填，schema 兼容 |

### 文件改动（10 个文件，~290 行净增）

```
src-tauri/crates/uc-observability/src/analytics/events.rs    +SponsorInternal variant + 钉死测试 + PairingFailureReason 新 enum
src-tauri/crates/uc-application/src/facade/space_setup/
├── deps.rs                                                  +analytics: Arc<dyn AnalyticsPort>
└── facade.rs                                                +解构 analytics + 透传 RedeemPairingInvitationUseCase::new + 内测 fake noop
src-tauri/crates/uc-application/src/usecases/pairing/redeem_invitation.rs
                                                            +analytics 字段、execute 三事件 fire + Result match
                                                            +map_redeem_error_to_pairing_failure_reason 14 变体映射
                                                            +CapturingAnalyticsSink test fake + 4 测试断言扩展
                                                            +map_redeem_error_covers_all_variants 1:1 映射钉死
src-tauri/crates/uc-bootstrap/src/space_setup.rs            +analytics: Arc::clone(&deps.analytics) 透传
src-tauri/crates/uc-bootstrap/tests/{slice1, slice2_phase1, slice2_phase2}_e2e.rs
                                                            +analytics: NoopAnalyticsSink（×3）
docs/architecture/telemetry-events.md                       +SponsorInternal 行
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-observability (lib) | **53**（无变化——8b commit A 已加 +PairingFailureReason 钉死 + PairingMethod 钉死） |
| uc-application (lib) | **378**（含 8 个 redeem_invitation 测试：5 happy/失败路径 + 3 个 t5 + map_redeem_error_covers_all_variants） |
| uc-bootstrap (lib) | 19 |
| uc-bootstrap (e2e: slice1 / slice2_p1 / slice2_p2) | 1 + 2 + 2 |
| uc-webserver | 39 |
| uc-desktop | 48 |

整体 build：`cargo check --workspace` 跨 11 crate 一次通过。

### 跳过的事项

- **sponsor 端三事件**：joiner 是 funnel 主路径，先把数据通起来；sponsor 端要发需要在 `PairingInboundOrchestrator` 持有 `Arc<dyn AnalyticsPort>`（构造点在 `SpaceSetupFacade::new` 内部装配），属于独立子任务，留 Slice 8b' 后续 PR
- **PassphraseMismatch / SponsorTimedOut 端到端集成测试**：构造能让 `JoinerHandshakeCoordinator` 真实返回这些 variant 的 session fake 复杂度高（要造 `Reject(PassphraseMismatch)` 帧 / TTL 触发），ROI 低；映射函数本身的覆盖已被 `map_redeem_error_covers_all_variants` 14 case 钉死
- **GUI/CLI 把 PairingMethod 维度下推到 use case**：v1 固定 `Code` 占位；后续 GUI 拆区分维度时再做（涉及 facade 入口签名 + commands.rs 多 fn / 多字段，是独立 refactor）

### 顺带做的事

- schema 修补：`PairingFailureReason` 从最初 13 变体扩到 14（补 `SponsorInternal`）。事故复盘：探索时只看 `enum` 顶层 13 行，没数到中间夹的 `SponsorInternal(String)`；wiring 写映射时编译器穷尽匹配检查暴露遗漏。下次类似映射工作直接 `cargo check` 让编译器先把缺漏点出来再决定 schema 形状

## Slice 8b 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿（§7.4 PairingFailureReason 新增） | ✅ |
| Slice 1 | `analytics_gate` 模块 | ✅ |
| Slice 2 | 事件类型 + AnalyticsPort | ✅ |
| Slice 3a | ID 持久化纯模块 | ✅ |
| Slice 4 | factory + 全局 + probe | ✅ |
| Slice 5 | settings 双开关 + bootstrap gate | ✅ |
| Slice 6 | bootstrap 拼装 EventContext + build_core async 化 | ✅ |
| Slice 7a | StdoutSink + 共享 wire 合并 | ✅ |
| Slice 7b | PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8a | sink 注入 AppDeps + factory + `app_first_open` + GatedAnalyticsSink | ✅ |
| Slice 8b | pairing 三事件（joiner 端 / `PairingFailureReason` 新 enum） | ✅ |
| Slice 8b' | sponsor 端三事件（PairingInboundOrchestrator 内 fire） | 可开始 |
| Slice 8c | sync 三事件 + 新增 `FirstSyncStatePort` | 可开始 |
| Slice 8d | setup 两事件 | 可开始 |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

### 验证 dev 跑起来事件链路（增 pairing）

`RUST_LOG=uc_observability::analytics=debug cargo run -p uc-cli -- join <code> --passphrase <pp>`：
- joiner 入口：应看到 `pairing_started` 一行 JSON（method=`code`）
- 握手完成：`pairing_succeeded`（含 `duration_ms`，`peer_os: null`）
- 失败时：`pairing_failed`（含 `failure_reason` 与业务错误一一对应）

---

## Session 2026-05-09 续 — Slice 8b'（sponsor 端 pairing 三事件 + broadcast 链路重构） ✅

**决策对话脉络**：
1. 用户"继续 Slice 8b'/8c/8d"——三个 slice 都要做。我建议 **8b' → 8d → 8c** 顺序（8b' 同域紧接，8d 最简单，8c 改动面最大放最后）
2. 探索三个 slice 关键代码点（并行 3 个 Explore subagent）
3. 8b' 探索发现：sponsor 端 emit_failure 走 `String` 化 `PairingOutcome::Failure { reason: String }`，funnel 漏点结构化信号丢失
4. AskUserQuestion 一次性问 3 决策，全部用户裁决：
   - sponsor 端 broadcast 链路重构：`PairingOutcome::Failure.reason` 字段类型 `String` → `PairingFailureReason` enum
   - sponsor 端 `pairing_started` 在 `IssuePairingInvitationUseCase::execute()` 入口 fire（与 joiner 端 funnel 起点对齐）
   - `FirstSyncStatePort`（Slice 8c）落到 `<app_data_root>/first-sync-state.json`（与 `AppVersionStatePort` 单一职责文件保持一致）

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| `PairingOutcome::Failure` 字段类型 | `String` → `PairingFailureReason` | broadcast 上的 string 化 reason 已丢结构化信号——funnel 漏点全归"unknown bucket"；改 enum 后 sponsor 端 7 个分支与 dashboard 一一对应；下游 subscriber（CLI / webserver）通过 `Display`（`snake_case`）恢复人类可读字符串 |
| `Display` impl for `PairingFailureReason` | wire 形态完全等价 `Serialize`（`snake_case`） | 同一标识符在 telemetry payload 与 webserver pairing-completed 事件 payload 一致——dashboard / 排障日志一目了然 |
| sponsor 端 7 个 emit_failure 调用点的 enum 选择 | 见下表 | sponsor 自身 admit / trust / confirm 失败统一 `Internal`（"sponsor 本机持久化错"），细分留 tracing log；不引入 sponsor-only 的 7 个新变体污染 schema |
| `pairing_started` 触发位置 | `IssuePairingInvitationUseCase::execute()` 入口 | 与 joiner 端 redeem use case 入口对齐——funnel 起点 = "sponsor/joiner 用户开始 pair"；早期 dial 失败（NetworkNotStarted / ServiceUnavailable）也留 funnel 第一步信号 |
| `pairing_succeeded.duration_ms` 起算点 | `on_incoming` 入口（per-session `Instant`）→ `finalise_verified` Success | "实际握手时长"——不含 sponsor 发码到 joiner 输入码的人类等待时间；funnel 上的 user-level "started ~ succeeded gap" 由 PostHog 自身从 timestamp 算出（两个口径独立可比） |
| sponsor `peer_os` v1 | 固定 `None` | 现有握手 outcome 没有对端 OS 字段；schema 已是 `Option<Os>`，未来协议补充后零变更回填 |
| `PairingMethod` v1 | 固定 `Code` 占位 | 与 joiner 端一致；GUI/CLI 区分维度下推留独立 refactor |

### sponsor 端 7 个 emit_failure 调用点 → `PairingFailureReason` 映射

| 触发条件 | 调用位置 | enum 变体 |
|---|---|---|
| invitation 已过期 | `match_invitation` Expired | `InvitationExpired` |
| holder 不变量被破 | `match_invitation` Internal(msg) | `Internal` |
| joiner proof 失败 | `on_message_received` Verdict::Rejected | `PassphraseMismatch` |
| sponsor clock 越界 | `finalise_verified` clock OOR | `Internal` |
| `admit_member` 持久化失败 | `finalise_verified` admit Err | `Internal` |
| `trust_peer` 持久化失败 | `finalise_verified` trust Err | `Internal` |
| `Confirm` send 失败（commit 后） | `finalise_verified` confirm Err | `ConnectionLost` |

### 文件改动（8 个文件，~210 行净增）

```
src-tauri/crates/uc-observability/src/analytics/
├── events.rs                              +PairingFailureReason::as_str + Display impl
└── mod.rs                                 +pub use PairingFailureReason

src-tauri/crates/uc-application/src/
├── facade/mod.rs                          +pub use PairingFailureReason
├── facade/space_setup/events.rs           PairingOutcome::Failure { reason: String → PairingFailureReason }; +pub use PairingFailureReason
├── facade/space_setup/mod.rs              +pub use PairingFailureReason
├── facade/space_setup/facade.rs           IssuePairingInvitation::new + Inbound::new 注入 analytics
├── pairing_inbound/orchestrator.rs        +analytics 字段 + handshake_started_at HashMap<PairingSessionId, Instant>
│                                           emit_failure 签名改 (&self, session: &PairingSessionId, reason: PairingFailureReason)
│                                           内部 fire pairing_failed event + send PairingOutcome
│                                           on_incoming 写 started_at；finalise_verified Success fire pairing_succeeded { duration_ms }
│                                           7 个 emit_failure 调用点改 enum；4 reason.contains 测试改 enum match
│                                           +CapturingAnalyticsSink + 3 新测试（succeeded / failed PassphraseMismatch / failed InvitationExpired）
│                                           Bundle::analytics 字段 + happy() 默认 NoopAnalyticsSink
└── usecases/pairing/issue_invitation.rs   +analytics 字段、execute 入口 fire pairing_started
                                           +CapturingAnalyticsSink test fake + assert_pairing_started helper
                                           4 个测试加 capture 断言（happy + 3 失败路径）

src-tauri/crates/uc-webserver/src/api/setup_events.rs
                                           handler reason.to_string() 透出 Display 形态
                                           2 个测试构造点改 enum variant；reason payload 断言改 "passphrase_mismatch" snake_case
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-observability (lib) | **55** |
| uc-application (lib) | **381**（含 8 issue_invitation + 12 pairing_inbound::orchestrator） |
| uc-bootstrap (lib) | 19 |
| uc-bootstrap (e2e: slice1 / slice2_p1 / slice2_p2) | 1 + 2 + 2 |
| uc-webserver (lib) | 39 |
| uc-desktop (lib) | 48 |

整体 build：`cargo check --workspace` 跨 11 crate 一次通过。

### 跳过的事项

- **sponsor 端 admit / trust / confirm 失败的 funnel 细分**：v1 统一归 `Internal`，funnel 上看不到 admit vs trust 的区分；细分信号留 tracing log。理由：在共享 `PairingFailureReason` 里加 sponsor-only 的 `SponsorAdmitFailed` / `SponsorTrustFailed` / `SponsorConfirmSendFailed` 三个变体会破坏 joiner 端 14:14 1:1 映射——schema 边界更重要
- **跨 use case/orchestrator 共享 `started_at` 拿到端到端 duration**：`pairing_started` 在 `IssuePairing` 入口、`pairing_succeeded` 在 `Inbound finalise_verified`，要算两者跨度需要共享 timing tracker。v1 用 `on_incoming → finalise_verified` 的"实际握手时长"代替——两端语义自然一致（joiner 端 redeem.execute 也是从用户输入完口令开始算）；端到端 funnel duration 由 PostHog `pairing_started → pairing_succeeded` 自动算出
- **`SponsorInternal` 变体**：当前 sponsor 端各处 internal 失败统一归 `Internal`（本机视角）；`SponsorInternal` 留给 joiner 端 redeem 收到 sponsor 返回 internal-rej 时使用——14:14 映射保留这个区分

### 顺带做的事

- `PairingFailureReason::as_str` + `Display`：让下游 subscriber 直接 `.to_string()` 拿稳定 wire 形态；不再各处自己 match → format

## Slice 8b' 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| schema doc | §1-§11 全章节定稿 | ✅ |
| Slice 1-7a | 见前 | ✅ |
| Slice 7b | PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8a | sink 注入 AppDeps + factory + `app_first_open` + GatedAnalyticsSink | ✅ |
| Slice 8b | pairing 三事件（joiner 端 / `PairingFailureReason` 新 enum） | ✅ |
| Slice 8b' | sponsor 端 pairing 三事件（PairingInboundOrchestrator + IssuePairing 注入 analytics） | ✅ |
| Slice 8c | sync 三事件 + 新增 `FirstSyncStatePort` | 可开始（落 `<app_data_root>/first-sync-state.json`） |
| Slice 8d | setup 两事件 | 可开始 |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

---

## Session 2026-05-09 续 — Slice 8d（setup 两事件） ✅

**决策对话脉络**：用户"继续 8b'/8c/8d"——按 8b' → 8d → 8c 顺序。8d 最简单，触达点：A1 `InitializeSpaceUseCase`。

### 关键决策

| 项 | 决策 | 理由 |
|---|---|---|
| `setup_started` 触发位置 | `InitializeSpaceUseCase::execute()` 入口 | funnel 起点 = "用户开始 setup"；与 pairing 端 funnel 起点对齐；AlreadySetup / PassphraseMismatch 等早期失败也留 funnel 第一步信号 |
| `setup_started.entry` v1 | 固定 `SetupEntry::FirstRun` | A1 use case 本就是 fresh-device 流程；`Manual` retry 入口未开发 |
| `device_name_set` 触发位置 | `resolve_and_persist_device_name` 成功收尾（在 `Ok(effective)` 之前） | 仅在 device_name 真正确认后 fire；DeviceNameRequired 失败路径自然不 fire——funnel 漏点统计需要这个语义 |
| `name_length_bucket` | `NameLengthBucket::from_char_count(effective.chars().count())` | 字符数（非字节数）切区间；按 schema doc §6.4 隐私契约，原文永不上传 |

### 文件改动（2 个文件，~100 行净增）

```
src-tauri/crates/uc-application/src/usecases/setup/initialize_space.rs
                                            +analytics 字段、execute 入口 fire setup_started
                                            +resolve_and_persist_device_name 收尾 fire device_name_set
                                            +CapturingAnalyticsSink test fake + Harness::analytics 字段
                                            +3 测试加 capture 断言：happy / DeviceNameRequired / PassphraseMismatch
                                            +`device_name_set_uses_name_length_bucket_boundaries`（Lt8 / Range8To16 / Gt16 三 case）
src-tauri/crates/uc-application/src/facade/space_setup/facade.rs
                                            InitializeSpaceUseCase::new 透传 analytics（第 8 参数）
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-application (lib) | **382**（含 10 个 setup::initialize_space 测试，新增 1 个 NameLengthBucket 边界 case） |
| 跨 crate 全部 | `cargo check --workspace` 一次通过 |

### 跳过的事项

- **`SetupEntry::Manual` 区分**：v1 占位 `FirstRun`；A1 use case 仅服务 fresh-install 路径，`Manual` 留待"settings 内重新初始化空间"入口实现时再加
- **A2 `UnlockSpaceUseCase` 是否 fire setup_started**：v1 否——A2 是日常 unlock 操作，不属于"setup"流程；schema doc 也仅列 A1 路径

---

## Session 2026-05-09 续 — Slice 8c-1（sync 三事件 / outbound per-peer） ✅

**决策对话脉络**：
1. 用户裁决拆 8c 为 8c-1（sync 三事件，本次）+ 8c-2（FirstSyncStatePort + first_*，后续 PR）
2. 用户裁决：per-peer 事件粒度（每个 fan-out 目标一条），与 SyncEventProps schema 自然对齐

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| 事件粒度 | per-peer | `SyncEventProps.peer_os` / `sync_latency_ms` 单一值——天然 per-peer；dashboard reliability 也按 peer 切；事件量 5×但可接受 |
| 触发位置 | spawn 内：先 fire `SyncAttempted`，dispatch.dispatch.await，再按 Result fire `SyncSucceeded`/`SyncFailed` | 保证 analytics 与单 peer outcome 原子配对；不需要在 join_next 合并循环里再 match 一次 |
| `payload_type` 推导 | File > Image > Text 优先级；空集合 fallback Text | 与隐私 §6 约束一致——coarse bucket 优于 missing field |
| `transport_type` v1 | 固定 `P2pDirect` | iroh ALPN 抽象了底层传输（direct/relay/QUIC），v1 不下钻；后续若 dispatch port 暴露 transport hint 可改 |
| `peer_os` v1 | `None` | 当前协议握手不携带对端 OS；schema 已是 `Option<Os>`，未来零变更 |
| `sync_latency_ms`（succeeded only） | per-peer `Instant` 计时（spawn 内 started_at → dispatch 返回） | 真实"单 peer 握手 + 写 stream + ack" 时长；满足 P95 分析 |
| `failure_reason`（failed only） | `ClipboardDispatchError` 1:1 映射到 `FailureReason` enum | funnel 上 5 个明确失败原因 vs string parse；映射函数被穷尽匹配测试钉死 |

### `ClipboardDispatchError` → `FailureReason` 映射表

| 错误变体 | enum 映射 | 备注 |
|---|---|---|
| `Offline` | `PeerOffline` | dial 失败 / 无可达地址 |
| `LocalPolicyExceeded(_)` | `FileTooLarge` | v1 仅 `MAX_PAYLOAD_SIZE` 触发；后续若加新策略再细分 |
| `PeerRejected(_)` | `NetworkError` | 协议层 reject（bad header / 不支持版本） |
| `Io(_)` | `NetworkError` | stream IO 失败 |
| `Internal(_)` | `Unknown` | 兜底；schema §7.3 监控 Unknown 占比 > 5% 视为架构债务 |

### 文件改动（4 个文件，~290 行净增）

```
src-tauri/crates/uc-application/src/usecases/clipboard_sync/dispatch_entry.rs
                                            +analytics 字段、payload_type_from_categories + map_dispatch_error_to_failure_reason 私有 fn
                                            execute fan-out 改：spawn 内自 fire SyncAttempted → dispatch → SyncSucceeded/SyncFailed
                                            +CapturingAnalyticsSink test fake + build_uc_with_analytics helper
                                            +3 新测试：happy 4 events 顺序+字段断言 / Offline → SyncFailed{PeerOffline} / map 5 变体钉死
src-tauri/crates/uc-application/src/facade/clipboard/facade.rs
                                            ClipboardSyncDeps 加 analytics 字段
                                            ClipboardSyncFacade::new 透传给 dispatch_uc
                                            内测 build_facade 补 NoopAnalyticsSink
src-tauri/crates/uc-bootstrap/src/space_setup.rs
                                            ClipboardSyncDeps 构造点补 analytics: Arc::clone(&deps.analytics)
src-tauri/crates/uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs
                                            ClipboardSyncDeps 构造点补 NoopAnalyticsSink
```

### 测试结果

| crate | tests passed |
|---|---|
| uc-application (lib) | **385**（dispatch_entry 9 → 12，新增 analytics_fires_attempted_then_succeeded × 1 + analytics_fires_failed_with_peer_offline × 1 + map_dispatch_error_covers_all_variants × 1） |
| uc-bootstrap (lib) | 19 |
| uc-bootstrap (e2e: slice1 / slice2_p1 / slice2_p2) | 1 + 2 + 2 |

整体 build：`cargo check --workspace` 跨 11 crate 一次通过。

### 跳过的事项

- **`FirstSyncStatePort` + `first_clipboard_sync_attempted` / `first_clipboard_sync_succeeded` / `first_file_sync_succeeded`**：拆 Slice 8c-2 后续 PR。要新 port trait（uc-core）+ 单文件 JSON 持久化（uc-infra，仿 `AppVersionStatePort`）+ 4 个构造点补 first_sync_state 字段。本次先把 funnel reliability 数据通起来
- **inbound（`IngestInboundClipboardUseCase`）的 sync 三事件**：v1 仅做 outbound——funnel 上"我发出的 sync"是更主要的 reliability 指标；inbound 留待后续
- **`peer_os` 真值**：握手协议未携带；后续 sponsor handshake outcome 加字段后回填，schema `Option<Os>` 已兼容
- **`transport_type` 真值**：需 dispatch port 暴露 hint；当前 iroh ALPN 不传出来，v1 占位 `P2pDirect`

## Slice 8d / 8c-1 后状态

| Slice | 内容 | 状态 |
|---|---|---|
| Slice 1-7a | 见前 | ✅ |
| Slice 7b | PosthogSink | 待 PostHog Cloud account + project key |
| Slice 8a | sink 注入 AppDeps + factory + `app_first_open` + GatedAnalyticsSink | ✅ |
| Slice 8b | joiner 端 pairing 三事件 + `PairingFailureReason` 新 enum | ✅ |
| Slice 8b' | sponsor 端 pairing 三事件 + broadcast 链路重构 | ✅ |
| Slice 8c-1 | sync 三事件（outbound per-peer） | ✅ |
| Slice 8c-2 | `FirstSyncStatePort` + `first_clipboard_sync_*` 事件 | 可开始 |
| Slice 8d | setup 两事件（A1 路径） | ✅ |
| Slice 9 | 前端 settings UI 拆开关 | 前端工作 |
| Slice 10 | dashboard + 验收 | 真实数据积累 |

### 验证 dev 跑起来事件链路（增 sync）

`RUST_LOG=uc_observability::analytics=debug` 启动 daemon，复制一段文本：
- 每个 paired peer 应看到一对 JSON：`sync_attempted` + `sync_succeeded`（或 `sync_failed`）
- `direction=outbound`、`payload_type=text`、`payload_size_bucket` 按字节切片、`transport_type=p2p_direct`、`sync_latency_ms` 毫秒级
- 失败时 `failure_reason` ∈ `peer_offline` / `file_too_large` / `network_error` / `unknown`

---

## Session 2026-05-09 续 — Slice 8c-2 启动 + 决策定稿（规划阶段） 🔄

**目标**：新增 `FirstSyncStatePort`（uc-core）+ `FileFirstSyncStateRepository`（uc-infra）落 `<app_data_root>/first-sync-state.json`；wire 4 个构造点；在 outbound dispatch_entry spawn 内 fire `first_clipboard_sync_attempted` / `first_clipboard_sync_succeeded` / `first_file_sync_succeeded` 三个事件。

### 决策对话脉络

1. 用户"继续 Slice 8c-2"——三个 first_* 事件 + 新 port
2. 并行两个 Explore subagent：(a) AppVersionStatePort 模板 + AppPaths/AppDeps 注入风格；(b) 当前 sync 三事件 fire 点 + 4 个构造点定位
3. 实测 `events.rs:57-73` 三个 first_* 事件 schema **已存在**——本 slice 仅 wire 不动 schema
4. AskUserQuestion 一次问 4 决策，用户全裁决

### 关键架构决策

| 项 | 决策 | 理由 |
|---|---|---|
| `_attempted` 触发语义 | 双 flag 独立：`_attempted` 在首次 attempt（成功/失败均记）记一次；`_succeeded` 在首次成功记一次 | 事件名字面意思；funnel 漏点信号完整——"用户尝试过但首次失败"会留 attempted 但无 succeeded 的信号；多写 1 行可接受 |
| Race 防护落点 | port impl 内部 `tokio::sync::Mutex` 串行 read-check-write | 与 `AppVersionStatePort` 文件实现风格对称；fan-out N 个 peer 全过同一锁；race 测试可显式覆盖；fan-out 量级 < 10 不到磁盘 IO 瓶颈 |
| `first_file_sync_succeeded` 范围 | 一并做：Port 三 flag、JSON schema 三字段；dispatch_entry 内 `payload_type=File` 分支额外 fire | schema doc §7 已预留；Port 三 flag 一次到位避免后续重 wiring；payload_type 推导逻辑 8c-1 已落地，复用 |
| 测试矩阵 | infra 7 tokio test（仿 AppVersionStatePort）+ 1 race test + use case 1 first-path | infra 完整契约覆盖；race 测试用 `tokio::join!` 多 spawn 同 mark 断言 true 仅一次；use case first-path 验证三事件序列 |
| Port API 形状 | `mark_first_sync_attempted/succeeded/file_sync_succeeded` 三方法都返回 `Result<bool>` | `bool` = 本次为首次置位（true 调用方 fire 事件）；语义最清晰；调用方无需 if-then-write 两步 |
| mark/fire 顺序 | mark 在 fire 之前（mark 返回 true 才 fire） | "先置位再 fire" — 事件丢一次比误报多次更可接受；首次同步只该有一次 |
| 持久化字段 | `{schema_version:1, attempted: bool, succeeded: bool, file_succeeded: bool}` | 仿 AppVersionStateFile schema 版本化；三 bool 各一字段，未来加新事件继续扩 |

### 4 个构造点 wiring 计划

| # | 文件 | 行号 | 改动概要 |
|---|---|---|---|
| 1 | `uc-application/src/deps.rs` + `uc-bootstrap/src/assembly.rs` | 139-168 / 404-410 | AppDeps 加 `first_sync_state` 字段；InfraLayer 构造 + 聚合点装配 |
| 2 | `uc-application/src/facade/clipboard/facade.rs` + `uc-bootstrap/src/space_setup.rs` | 42-57 / 390-402 | ClipboardSyncDeps 加字段 + facade 透传；构造点 `Arc::clone` |
| 3 | `uc-application/src/usecases/clipboard_sync/dispatch_entry.rs` | 158-198 / 287-323 | use case struct field + new 参数；spawn 内三处 mark + 条件 fire |
| 4 | `uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs` | 测试构造点 | 补 fake/in-memory `first_sync_state` |

### 子任务（已 TaskCreate 持久化跟踪）

7 个子任务带 blockedBy 依赖图：
1. uc-core trait
2. uc-infra impl + 8 测试（7 行为 + 1 race） ← blockedBy 1
3. AppDeps + assembly 装配（构造点 1） ← blockedBy 2
4. ClipboardSyncDeps + space_setup 装配（构造点 2） ← blockedBy 3
5. dispatch_entry use case fire 三事件 + 1 first-path 测试（构造点 3） ← blockedBy 4
6. e2e 测试构造点补字段（构造点 4） ← blockedBy 3
7. cargo check + test 全 workspace ← blockedBy 5,6

### Status

**规划完成、实现待开始。** 决策全部定稿在 `task_plan.md` Slice 8c-2 子任务清单 + Decisions Made 表底新增 4 行；探索发现归档到 `findings.md` 的 "Slice 8c-2 探索发现" 章节。下一步：按子任务依赖序逐个执行（建议从 task #1 uc-core trait 开始）。

---

## Session 2026-05-10 — Slice 8c-2 实现完成 ✅

按规划完成 7 个 task：uc-core trait → uc-infra impl → 3 个 wiring 构造点 → use case fire → e2e 构造点补 → cargo check + test 全绿。

### 文件改动（10 个文件，~530 行净增）

```
src-tauri/crates/uc-core/src/ports/
├── first_sync_state.rs                            新增 ~55 行（trait + 3 method + Error enum）
└── mod.rs                                          +pub mod + re-export

src-tauri/crates/uc-infra/src/
├── first_sync_state.rs                            新增 ~280 行（FileFirstSyncStateRepository + tokio::sync::Mutex 串行 + tempfile+rename 原子写 + 8 tokio test）
└── lib.rs                                          +pub mod + re-export

src-tauri/crates/uc-application/src/
├── deps.rs                                         AppDeps +first_sync_state 字段
├── facade/clipboard/facade.rs                     ClipboardSyncDeps +first_sync_state 字段；ClipboardSyncFacade::new 透传给 DispatchUseCase；内测 build_facade +NoopFirstSyncState fake
└── usecases/clipboard_sync/dispatch_entry.rs      use case +first_sync_state 字段；spawn 内 mark + 条件 fire 三 first_* 事件；tests +AllMarkedFirstSyncState/InMemoryFirstSyncState fakes + build_uc_with_first_sync_state helper + first_path test

src-tauri/crates/uc-bootstrap/src/
├── assembly.rs                                     InfraLayer struct +字段；first_sync_state 实例化（FileFirstSyncStateRepository::with_defaults(app_data_root)）；AppDeps 聚合点
└── space_setup.rs                                  ClipboardSyncDeps 构造点 +first_sync_state: Arc::clone(&deps.first_sync_state)

src-tauri/crates/uc-bootstrap/tests/
└── slice2_phase2_clipboard_e2e.rs                 +NoopFirstSyncState struct + ClipboardSyncDeps 构造点补字段
```

### 关键实现决策（与规划裁决全部吻合）

| 项 | 实现 |
|---|---|
| Port API | 三方法 `mark_first_sync_attempted/succeeded/file_sync_succeeded`，全部 `Result<bool>`；`Ok(true)` = 本次首次置位 |
| Race 防护 | `FileFirstSyncStateRepository` 内部 `tokio::sync::Mutex<()>` 守 read-check-write 整段 critical section；fan-out N 个 spawn 全过此锁，only 1 个返回 true |
| JSON schema | `{schema_version: 1, attempted: bool, succeeded: bool, file_succeeded: bool}`；schema_version=1，未来扩字段走 migrate 分支 |
| 持久化 | tempfile (`<file>.tmp`) → fsync → rename 三步原子写，与 AppVersionStatePort 实现等价 |
| dispatch_entry spawn 内顺序 | `SyncAttempted` capture → `mark_first_sync_attempted` → 条件 fire `FirstClipboardSyncAttempted` → dispatch.dispatch().await → `SyncSucceeded`/`SyncFailed` capture → 成功路径 `mark_first_sync_succeeded` → 条件 fire → `payload_type=File` 分支 `mark_first_file_sync_succeeded` → 条件 fire |
| mark 失败处理 | `Err(_)` → `warn!` log + 不 fire；funnel 事件丢一次比误报多次更可接受 |
| 测试 fake 分层 | `AllMarkedFirstSyncState`（永远 false，原 sync 三事件 test 用）+ `InMemoryFirstSyncState`（默认 unmarked，新 first-path test 用）+ `NoopFirstSyncState`（facade 内测 / e2e 用） |

### 测试结果

| crate | tests passed | 增量 |
|---|---|---|
| uc-core (lib) | 84 | 0（trait + Error 编译型，无 unit test） |
| uc-infra (lib) | **241** | +8（first_sync_state: missing/round-trip/overwrite/corrupt/empty/schema-mismatch/parent-dir + race） |
| uc-application (lib) | **386** | +1（dispatch_entry::first_path_fires_clipboard_and_file_first_events_exactly_once_per_flag） |
| uc-observability (lib) | 55 | 0（事件 schema 已存在） |
| uc-bootstrap (lib) | 19 | 0 |
| uc-bootstrap (e2e: slice2_phase2_clipboard_e2e) | 2 | 无回归 |
| uc-webserver (lib) | 39 | 0 |
| uc-desktop (lib) | 48 | 0 |
| uc-daemon-contract (lib) | 27 | 0 |

整体 build：`cargo check --workspace` 跨 11 crate 一次通过。

### 跳过的事项

- **inbound（`IngestInboundClipboardUseCase`）first_* 事件**：与 8c-1 跳过 inbound 三事件保持一致——funnel 上"我发出的 sync"是更主要 reliability/activation 指标；inbound 留待后续。schema 上 `FirstClipboardSyncAttempted/Succeeded` 已带 `direction` 字段，未来加 inbound 触发只需 fire `Direction::Inbound`，schema 0 改动
- **dispatch_entry 第二次 dispatch 不再 fire 的幂等显式测试**：infra 端 `three_flags_round_trip_independently` 已覆盖"第二次 mark 返回 false"行为；use case 端的 first_path test + 各 fake 的 mutex 已隐式覆盖，再加显式幂等 test 增量信息低
- **bootstrap-level integration test**：要构造完整 AppDeps + 真磁盘 first-sync-state.json + 触发 dispatch 路径成本远高于 ROI；`slice2_phase2_clipboard_e2e` 已端到端验证 ClipboardSyncDeps wiring 不挂；`AllMarkedFirstSyncState` 防止 e2e 受 funnel 事件污染

### 顺带做的事

- 测试 fake 模式总结写进 dispatch_entry tests doc-comment：三层 fake 各司其职（`AllMarkedFirstSyncState` 永不触发 / `InMemoryFirstSyncState` 默认 unmarked 一次触发 / production `FileFirstSyncStateRepository` 走磁盘）

### Next Action

可推进的下一节点：
- Slice 7b（PostHog Cloud account + project key 到位后接 PosthogSink）
- Slice 9（前端 settings UI 拆 `usage_analytics_enabled` 开关）
- Slice 10（dashboard + 真实数据验收）

## Session 2026-05-09（晚）— Slice 7b 规划展开

**触发**：用户 `/planning-with-files Slice 7b（PostHog Cloud 接入）`，要求把先前在 task_plan.md 里只占 4 行的 Slice 7b 拆细。

### 探索路径（不写代码，纯调研 + 决策）

1. 读 `task_plan.md` / `findings.md` 确认上下文：Slice 8 全完，7b 仍 pending、外部 blocker = PostHog 账号 + project key
2. 读 `uc-bootstrap/src/tracing.rs:155-170`：SENTRY_DSN 三级注入（运行时 env > `option_env!` > 关闭）—— 作为 PosthogSink key 注入完全镜像范本
3. 读 `uc-bootstrap/src/analytics.rs::build_analytics_sink`：当前 release 临时态 `Gated(NoopAnalyticsSink)`，留好了 PosthogSink 接入位
4. 读 `uc-observability/src/analytics/sinks/{mod,stdout,gated,port}.rs`：sink 抽象、wire 合并、gate wrapper 的契约边界都已就位
5. 读 `.github/workflows/{build,alpha-build}.yml`：SENTRY_DSN 在 `tauri-action` 与 `bun run tauri build` 两段 env 块同位注入；`POSTHOG_PROJECT_KEY` 直接同位加
6. context7 查 `/posthog/posthog-rs` 0.7 API：`ClientOptionsBuilder` + `EU_INGESTION_ENDPOINT` + `disable_geoip` + `disabled` + async client 是 async fn
7. 确认 `cargo tree -e features` 路径可用于守 features 选错（防 transitive openssl）

### 关键裁决（已落到 task_plan.md Decisions Made + findings.md Slice 7b 节）

| 维度 | 裁决 | 理由 |
|---|---|---|
| key 注入 | 三级回退（runtime env > `option_env!` > 关闭） | SENTRY_DSN 路径已验证，不引第二种机制 |
| endpoint | 固定 `EU_INGESTION_ENDPOINT` | schema doc 选定；self-host 留 11+ |
| `build_analytics_sink` 形态 | 转 async（传染面 1 处） | `compose_event_context` 已 async；OnceCell 懒初始化更复杂 |
| capture 模型 | `tokio::spawn` fire-and-forget | sync 签名 + async client 唯一干净桥；SDK 内部 batch 不重造 |
| 缺 key 处置 | release → `Gated(NoopAnalyticsSink)` + info 一次 | telemetry 不能反向影响 daemon / GUI 启动 |
| `disable_geoip` | true | schema doc §6 隐私契约：客户端 IP 不上传 |
| 进程退出 flush | v1 不挂钩子 | < 1% 丢失符合 schema doc §10；后续观察实测再补 |
| 测试 | `disabled(true)` client + fake；不联真实 PostHog | CI 不污染生产 telemetry；HTTP 行为不归本仓库 |
| Cargo features | `default-features = false, features = ["async-client"]` | 防 transitive openssl（与项目 reqwest+rustls 一致） |

### 子切片拆分（4 个独立 commit / PR）

| 子切片 | 内容 | 阻塞外部条件 |
|---|---|---|
| 7b-1 | 依赖引入 + `PosthogSink` 骨架（空 impl，仅过编译） | 无 |
| 7b-2 | `capture` 实现 + `build_event_payload` wire 集成 + `disabled(true)` lifecycle 单测 | 无 |
| 7b-3 | key 注入 + `build_analytics_sink` 转 async + 缺 key 降级路径 | 无（降级路径不需要真 key） |
| 7b-4 | CI secret 注入 + docs + 真实 dev 验收 | 需 PostHog account + `POSTHOG_PROJECT_KEY` secret |

7b-1/7b-2/7b-3 可并行 PostHog account 准备启动；唯有 7b-4 真实事件验收必须等账号 + key。

### 文件改动清单（规划阶段，仅写入计划文件）

- `task_plan.md`：Slice 7b 节从 4 行展开为 ~80 行（4 子切片 + 用户裁决项 + 架构图 + 子任务依赖图）；Current Phase 翻"进行中：Slice 7b 规划展开"；Decisions Made 表 +7 条；Key Questions #4 标解决
- `findings.md`：新增"Slice 7b 探索发现"章节（SENTRY_DSN 注入范本、posthog-rs 0.7 API surface、wire 字段映射、测试取舍、进程退出风险评估、关键决策汇总）
- `progress.md`：本 session 条目

### 下一步行动

外部条件就绪前可立即推进的事：
- 启动 7b-1（依赖引入 + sink 骨架）—— 不需要 PostHog 账号
- 通知用户开 PostHog Cloud 账号、建项目、把 `phc_*` key 加到 GitHub `POSTHOG_PROJECT_KEY` secret

阻塞中：
- 7b-4 的真实事件 dev 验收等账号 + secret 就绪

## Session 2026-05-09（深夜）— Slice 7b-1 落地

**触发**：用户 `/planning-with-files 继续任务`。task_plan.md / findings.md 已为 Slice 7b 写好详尽规划与决策；外部 blocker（PostHog account + project key）只阻塞 7b-4 真实事件验收，7b-1/7b-2/7b-3 可立即推进。本 session 推 7b-1（依赖与 sink 骨架）。

### 文件改动（3 个文件）

```
src-tauri/crates/uc-observability/Cargo.toml                 +reqwest 0.12 + tokio rt（含决策注释引用 sentry 同款约束）
src-tauri/crates/uc-observability/src/analytics/sinks/
├── posthog.rs                                                新增 ~95 行（PosthogSink 骨架 + 3 单测）
└── mod.rs                                                    +pub mod posthog + pub use PosthogSink
```

### 关键确认

| 项 | 实测结果 |
|---|---|
| `cargo tree -p uc-observability -e features` | 无 aws-lc / openssl / native-tls 命中（reqwest 0.12 rustls 走 ring） |
| `cargo check -p uc-observability` | 通过，依赖图含 reqwest 0.12.28 / hyper-rustls 0.27.7 / rustls-webpki 0.103.8 |
| `cargo check --workspace` | 跨 11 crate 一次过 |
| `cargo test -p uc-observability --lib` | 58 passed（基线 55 + 7b-1 新增 3） |

### 关键实现取舍（与规划一致）

- `PosthogSink::new(api_key)` 用 `POSTHOG_US_CAPTURE_ENDPOINT` 常量；`with_endpoint(...)` 是测试 / self-host 入口；endpoint 字段独立持有让 7b-2 wiremock 烟测无需 mock 全局常量
- `client: reqwest::Client::new()` 同步构造；`build_analytics_sink` 保持 sync 签名的承诺成立
- `warned_missing_context: AtomicBool` 字段先占位；warn 节流逻辑随 7b-2 capture 实现一起落（与 StdoutSink 同款 `swap` 模式）
- `impl AnalyticsPort::capture` 占位用 `let _ = event;` + `let _ = (&self.client, &self.api_key, &self.endpoint, &self.warned_missing_context);` 让 unused warnings 安静；object safety 单测 `Box<dyn AnalyticsPort>` 已守住 trait shape
- 模块文档块完整解释"为什么不用 posthog-rs SDK"（aws-lc-rs C 库依赖与 uc-cli musl 静态编译硬约束的冲突）——后续 reviewer 看到这个文件就有完整 context

### 跳过的事项

- **7b-1 阶段不做 wiremock 测试**：HTTP 行为还没接，纯 noop 上 wiremock 没有信号；留 7b-2 一并做
- **不在 7b-1 引 chrono::Utc::now() 工具**：timestamp 拼装是 7b-2 的 `build_capture_body` 职责，避免单 commit 噪音

### Next Action

7b-2：`capture` 实现 + `build_capture_body` 纯 fn + wiremock 烟测 + 字段冲突 invariant 单测。

## Session 2026-05-09（深夜续）— Slice 7b-2 落地

**触发**：用户 `/planning-with-files 继续`，承接 7b-1。

### 文件改动（4 个文件）

```
src-tauri/crates/uc-observability/Cargo.toml                              +wiremock 0.6 + tokio (rt-multi-thread/macros/time) dev-dep
src-tauri/crates/uc-observability/src/analytics/context.rs                +lock_global_event_context_for_tests() helper（跨 fn 串行化全局 RwLock 的测试锁）
src-tauri/crates/uc-observability/src/analytics/sinks/posthog.rs          ~+170 行：build_capture_body 纯 fn + capture 实 wire + warn 节流 + 4 纯 fn 单测 + wiremock 烟测
src-tauri/crates/uc-observability/src/analytics/sinks/stdout.rs           lifecycle 测试加 `_guard = lock_global_event_context_for_tests()`
```

### 关键实现取舍

| 项 | 实现 |
|---|---|
| `build_capture_body` 输入 | 直接吃 `build_event_payload` 输出的 `Map<String, Value>`，不在 sinks 间发明第二种 wire 形态 |
| 字段冲突 invariant | properties 移除 `event` / `distinct_id` 两键；顶层独立放置；invariant 单测显式断言 properties 不含此二键 |
| `timestamp` | `chrono::Utc::now().to_rfc3339()`，PostHog 服务端用此字段而非 envelope `$timestamp` 推断事件时间（与 schema doc §4 时间戳"事件级"约定吻合） |
| `tokio::spawn` 跨线程数据 | client 是 `reqwest::Client`（内部 `Arc`，clone 廉价）；endpoint/api_key 取 `String` 副本；event_name 是 `&'static str`（Event::name 返回值） |
| 错误处理 | 非 2xx → warn(status)；reqwest::Error → warn(error)；从不向 capture 调用方传播 |
| context 缺失节流 | 与 StdoutSink 同款 `AtomicBool::swap(true, Relaxed)`，单测验证"两次 capture 仅 0 个 POST" |

### 跨 fn 全局 EventContext 竞态修复（顺带）

**问题**：`stdout_sink_lifecycle` + `posthog_sink_lifecycle` + `context::global_event_context_lifecycle` 三 fn 都改全局 `RwLock<Option<Arc<EventContext>>>`。cargo test 默认线程并发，posthog 用 `tokio::time::sleep(200ms)` 给了竞态窗口让其它 fn `clear_global_event_context()` 把 ctx 顶掉，触发 `context not yet set` warn 路径，POST 0 次断言挂。

**修复**：`context.rs` 加 `#[cfg(test)] pub(crate) fn lock_global_event_context_for_tests() -> MutexGuard<'static, ()>`（`OnceLock<Mutex<()>>`）。三处 lifecycle fn 入口拿一次 guard，整个 fn 体作为 critical section。锁中毒走 `into_inner` 兜底，前一测试 panic 不级联失败。

之前 task_plan.md decisions 表第 10 条说"全局测试用单一 fn 而非 serial_test 依赖"——前提是只有 1 个 fn 触达全局。现在 3 个 fn 都需要 fire-and-forget 测试，单 fn 化已不现实，引入 stdlib `OnceLock<Mutex<()>>` 是最小代价，仍未引第三方 `serial_test`。

### 测试结果

| crate | passed | 增量 |
|---|---|---|
| uc-observability (lib) | **63** | +5（4 个 build_capture_body 纯 fn + 1 个 posthog_sink_lifecycle 烟测）|
| `cargo check --workspace` | ✅ | reqwest 0.12 + wiremock 0.6 引入，11 crate 全过 |

### 跳过的事项

- **不测真实 PostHog endpoint**：CI 不应往生产 telemetry 服务发数据。wiremock 烟测覆盖 POST 形态后已足够
- **不测 reqwest 重试 / connection 复用**：reqwest 0.12 内部行为非本仓库责任
- **不测 spawn task 在进程退出时被中断**：决策 #92 已说清 < 1% 丢失可接受；schema doc §10 兜底
- **不在 7b-2 加 docs 与 CI secret**：留 7b-3（key 注入降级）+ 7b-4（CI secret + 真实 dev 验证）

### Next Action

7b-3：`build_analytics_sink` release 路径 = `resolve_posthog_key(runtime_env, option_env!) → Some(key) → Gated(PosthogSink::new(key)) | None → info("PostHog 未配置，产品 telemetry 关闭") + Gated(NoopAnalyticsSink)`；4 个 `resolve_posthog_key_*` 单测。

## Session 2026-05-09（深夜续 2）— Slice 7b-3 落地

**触发**：用户 `commit and continue`，承接 7b-2。先 commit `feat(observability): add PosthogSink for product analytics capture` + `docs(planning): commit Slice 7b planning + 7b-1/7b-2 progress` 两个 commit，然后推 7b-3。

### 文件改动（3 个文件）

```
src-tauri/crates/uc-observability/src/analytics/mod.rs                    +PosthogSink re-export
src-tauri/crates/uc-bootstrap/src/analytics.rs                            build_analytics_sink release 路径接 PosthogSink + resolve_posthog_key 私有 fn + 5 单测
```

### 关键实现取舍

| 项 | 实现 |
|---|---|
| `build_analytics_sink` 签名 | 保持 sync（与规划裁决一致；自写 reqwest client 同步构造，传染面 0，assembly.rs 调用点零改动） |
| 三级回退顺序 | runtime env > `option_env!` 编译期 > `None`；空字符串等价"未设置"（CI secret 未注入时 `${{ secrets.X }}` 渲染为空，不能让空 api_key 调 PostHog 触发 401） |
| 缺 key 处置 | release path → `tracing::info!`（非 warn）+ `Gated(NoopAnalyticsSink)`；缺 key 是合法配置（dev 自部署 / PR review build 都不应注入生产 key），不应让 daemon / GUI 启动失败也不应吓 ops |
| dev 路径 | `cfg!(debug_assertions)` 守住，仍 `Gated(StdoutSink)`，与 7a 落地保持一致 |
| `resolve_posthog_key` 抽私有 fn | `std::env::var` + `option_env!` 是 macro 语境，行内会让单测无法穿透；抽 fn 后 5 个 case 全可纯函数测试 |

### 测试结果

| crate | passed | 增量 |
|---|---|---|
| uc-bootstrap (lib) | **24** | +5（resolve_posthog_key 4 个 happy path + 1 个空字符串等价） |
| uc-observability (lib) | 63 | 0（仅加 re-export，无新行为） |
| `cargo check --workspace` | ✅ | 11 crate 全过 |

### 跳过的事项

- **不写 `build_analytics_sink` 集成测试**：要构造完整 release 路径需要把 `cfg!(debug_assertions)` 翻成 `false`，testharness 不友好；release 路径分支已被 5 个 `resolve_posthog_key_*` + 1 个 dev `Gated(StdoutSink)` 既有 7a 测试覆盖完整
- **不修改 `option_env!` 调用为 wrapper**：直接 inline 在 `build_analytics_sink`，把行为隔离在 `resolve_posthog_key` fn——单测穿不透 macro 但能穿透 fn 边界
- **不在 7b-3 加 sentry 同款 init 时 redact 处理**：PostHog client 不像 sentry SDK 接前后端 hook；自写 reqwest 不发 properties 中没有的字段，IP 字段从源头不上传。schema doc §6 已守住

### Next Action

7b-4：`.github/workflows/{build,alpha-build}.yml` 加 `POSTHOG_PROJECT_KEY: ${{ secrets.POSTHOG_PROJECT_KEY }}` env block；schema doc §10 / CONTRIBUTING.md 补 PostHog key 注入文档；通知用户开 PostHog 账号 + 把 key 放 GitHub secrets；按 task_plan.md 7b-4 步骤跑真实 dev 验证（onboarding → 控制台看 app_first_open / setup_started / pairing_* 序列；toggle settings → noop fallback 验证；unset env + 重启 → 静默验证）。

