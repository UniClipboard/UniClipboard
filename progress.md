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
