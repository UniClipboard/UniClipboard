# Task Plan: 产品 Telemetry / Analytics（Issue #549）

## Goal

为 UniClipboard 建立隐私友好的产品 metrics 体系，覆盖 issue #549 的"第一版必须埋点"中 **最关键的两段**——Activation 漏斗与 Reliability 同步可靠性——能回答"用户从哪里来 / 是否完成首次配对与首次跨设备同步 / 完成首次同步用户是否留存 / 同步失败发生在哪些组合 / 哪些摩擦点导致流失"。

Schema 与隐私契约定稿在：`docs/architecture/telemetry-events.md`。

## Strategy

- **后端**：PostHog Cloud（US ingestion endpoint，实际注册区域），不自研 ingestion / dashboard
- **架构**：schema 与 SDK 完全解耦——所有事件类型驻在 `uc-observability::analytics`，sink 通过 `AnalyticsPort` trait 注入；将来换 self-host 或换后端只换 sink
- **隐私双开关**：`general.telemetry_enabled`（Sentry 错误）+ `general.usage_analytics_enabled`（产品 telemetry）独立勾选，GDPR 友好
- **ID 分层**：`anonymous_user_id` / `analytics_device_id` / `session_id`，全部 UUIDv7。`analytics_device_id` 与 `uc-core::DeviceId` **完全 disjoint**，零 cross-system correlation 风险

## Current Phase

Slice 8 全部完成（8a / 8b / 8b' / 8c-1 / 8c-2 / 8d）。outbound funnel + reliability 全链路通。

**完成：Slice 7b（PostHog Cloud 接入）** —— 7b-1 / 7b-2 / 7b-3 已落地代码（uc-observability 63 + uc-bootstrap 24 lib tests 全绿）。7b-4 docs 与 CI workflow 改动完成，GitHub repo secret 已确认存在，带 `POSTHOG_PROJECT_KEY` 的 release 编译探针已通过，真实 new space / invite / pairing 流程已在 PostHog 后台确认有记录。

Slice 9 / 10 待前端工作 / 真实数据积累。

## Phases

### Slice 1: `analytics_gate` 模块
镜像现有 `telemetry_gate`，作为 `usage_analytics_enabled` 的进程级运行时门控。
- [x] 新增 `uc-observability/src/analytics_gate.rs`（is_analytics_enabled / set_analytics_enabled）
- [x] 与 `telemetry_gate` 隔离性测试
- **Status:** complete

### Slice 2: 事件类型骨架 + AnalyticsPort trait
所有事件类型为 pure data，可被 `uc-application` use case 直接构造。
- [x] `analytics/context.rs`：EventContext + Os/Arch/AppChannel/InstallSource
- [x] `analytics/events.rs`：Event enum + 8 子枚举 + buckets + SyncEventProps
- [x] `analytics/port.rs`：AnalyticsPort trait + NoopAnalyticsSink
- [x] wire 形态钉死测试（防止后续误改字符串导致破坏向后兼容）
- **Status:** complete

### Slice 3a: ID 持久化（纯模块）
- [x] `analytics/ids.rs`：`load_or_create` + `reset` + 原子写
- [x] 损坏 / 部分缺失场景的恢复策略测试
- **Status:** complete

### Slice 4: EventContext factory + 全局注册 + 平台探测
- [x] `analytics/context.rs`：EventContextInputs + build_event_context + RwLock<Option<Arc>> 全局
- [x] `analytics/probe.rs`：detect_os / detect_arch / detect_locale / detect_timezone / detect_os_version
- [x] schema doc：澄清 timestamp 是事件级，非 EventContext 字段；新增 `Os::Other` 兜底
- **Status:** complete

### Slice 5: settings 双开关 + bootstrap gate 拼装
跨 6 个 crate 的同位映射，让 `usage_analytics_enabled` 走完和 `telemetry_enabled` 一模一样的流水。
- [x] `uc-core/settings/model.rs`：新增字段 + 默认 fn
- [x] `uc-core/settings/defaults.rs`：默认值
- [x] `uc-application/facade/settings/models.rs`：view / patch / convert / update
- [x] `uc-application/facade/app_facade.rs`：显式构造点补字段
- [x] `uc-daemon-contract/api/dto/settings.rs`：DTO + patchDTO + convert + default fn
- [x] `uc-webserver/api/settings.rs`：取值 + gate setter + patch + view
- [x] `uc-webserver/tests/settings_network_smoke.rs`：补字段
- [x] `uc-bootstrap/tracing.rs`：resolve_usage_analytics_enabled + set_analytics_enabled init 调用
- [x] uc-observability：合并 global 测试消除竞态
- **Status:** complete

### Slice 6: bootstrap 拼装 EventContext（根本性重构）

**决策定稿（2026-05-09 用户裁决）**：
- 调用点：放在 `wire_dependencies` 之后；`build_core` 改 async（"根本性"代价：破坏 sync builder API，传染 4 个 entry caller）
- `active_device_count`：实读 `member_repo.list().await.len()`，不用 0 占位
- `app_channel`：从版本号前缀解析（`-alpha*` / `-beta*` / 否则 stable）
- `install_source`：v1 固定 `Unknown`，不接 env / build flag（schema 字段保留，后续 release pipeline 准备好再批 inject）
- `space_id_hash`：从 `setup_status.get_status().await.space_id` 读，SHA-256 取前 16 hex；未 setup → None

**子任务**：
- [x] uc-bootstrap/Cargo.toml：加 `sha2` 依赖
- [x] uc-bootstrap/src/analytics.rs：`compose_event_context(deps, paths) -> Result<()>` async + 幂等门控
- [x] uc-bootstrap/src/lib.rs：pub mod + pub use
- [x] uc-bootstrap/src/builders.rs：`build_core` / 3 个 builder 转 async
- [x] uc-bootstrap/src/non_gui_runtime.rs：`build_cli_app_facade` 转 async；`build_cli_app_runtime` 加 `.await`
- [x] uc-desktop/src/bootstrap.rs：`build_gui_app` 转 async + 调 compose
- [x] uc-tauri/src/run.rs：用 `tauri::async_runtime::block_on(build_gui_app())`
- [x] uc-cli/src/commands/{status,upgrade,search}.rs：加 `.await`（含 sync helper `build_search_facade` 转 async）
- [x] 单元测试：6 个新增（hash_space_id × 3、parse_app_channel × 4）。compose 端到端 integration test 评估为低 ROI（要构造完整 AppDeps，~20 个 fake port）；schema 行为已被 uc-observability 47 个 lib 测试覆盖
- **Status:** complete

### Slice 7a: StdoutSink + 共享 wire 合并
- [x] `analytics/sinks/mod.rs`：`build_event_payload(event, ctx)` —— 跨 sink 复用的 wire 形态合并
- [x] `analytics/sinks/stdout.rs`：`StdoutSink` 走 `tracing::debug!` 单行 JSON
- [x] sink 通过 `global_event_context()` 拿快照与事件 properties 合并
- [x] context 缺失 → 丢事件 + warn 节流（一次/sink 实例）
- [x] dev/prod 走 runtime 切换（按用户裁决）—— sink 注入点留给 Slice 8 wire 到 AppDeps
- [x] 5 个新测试：payload 4 case + stdout_sink_lifecycle 串行 fn
- **Status:** complete

### Slice 7b: PosthogSink（PostHog Cloud 接入）

**目标**：把 release 构建的 `build_analytics_sink()` 从临时态 `Gated(NoopAnalyticsSink)` 切到 `Gated(PosthogSink)`，事件真正落到 PostHog Cloud（US endpoint，2026-05-09 实际注册区域）。dev 路径继续走 `Gated(StdoutSink)` 不变。

**外部阻塞**（在 7b-3 落地前必须就绪）：
1. PostHog Cloud 账号开好 + 项目建好 + 拿到 `phc_*` project key
2. CI secret `POSTHOG_PROJECT_KEY` 加到 GitHub repository（与 `SENTRY_DSN` 同位）

**用户裁决项（Slice 7b 启动时一次定稿）**：
- **key 注入策略** = 完全镜像 SENTRY_DSN 三级回退：运行时 env `POSTHOG_PROJECT_KEY` 优先 → 编译期 `option_env!("POSTHOG_PROJECT_KEY")` → 都缺时 release 降级到 `Gated(NoopAnalyticsSink)` + 一次 info 日志。理由：CI 注入路径已经验证过（`uc-bootstrap/src/tracing.rs:155-170`），不引入新机制
- **endpoint** = 固定 `https://us.i.posthog.com/i/v0/e/`（PostHog capture API），不暴露 self-host 配置（schema doc 已选 US；后期迁移 self-host 或切 EU 是 Slice 11+ 范围）
- **HTTP 客户端 = 自写 reqwest 0.12，不用 posthog-rs SDK** ⚠️ **2026-05-09 决策转向**：原计划用 `posthog-rs = "0.7"` SDK，cargo tree 验证时发现 SDK hardcode `reqwest = "0.13.2"` 带 `features = ["rustls"]`——reqwest 0.13 的 `rustls` feature 隐式选 `aws-lc-rs`（C 库 + CMake 编译，musl cross 不友好）。这与项目早已建立的硬约束冲突（见 `uc-bootstrap/Cargo.toml:27-34` sentry 注释：刻意用 `ureq + rustls(ring)` 避开 reqwest 0.13）。cargo features unification 是 workspace 级，无法用 `optional`/feature gate 把 uc-cli 排除。改为自写 ~100 行 minimal HTTP client：
  - 用项目已有的 `reqwest = "0.12", default-features = false, features = ["json", "rustls-tls", "rustls-tls-webpki-roots"]`（uc-infra/uc-daemon-client/uc-cli/uc-desktop 全部已用，走 ring，无 aws-lc）
  - PostHog capture endpoint 极简：`POST /i/v0/e/` + `{api_key, event, distinct_id, properties, timestamp?}` JSON body
  - 失去 SDK 的 batching + retry，但 schema doc §10 已允许 < 1% 丢失，fire-and-forget 单条 POST 够用
- **构造异步化** = `reqwest::Client::new()` 是 sync；`build_analytics_sink()` 暂可保持 sync 签名。client 内部 `client.post(url).json(body).send().await` 走 spawn 隔离
- **fire-and-forget 模型** = `PosthogSink::capture` 内部 `tokio::spawn` 一条独立 task 调 `reqwest::Client::post(...).await`；调用方 zero-await。HTTP 失败仅 `tracing::warn!` 不传播
- **进程退出 flush** = 不挂显式 shutdown hook。reqwest::Client 内有连接池但无应用级队列（自写 client 没有 batching 队列），单条 POST 一旦 spawn 就走自己的网络生命周期。schema doc §10 已允许 < 1% 丢失。后续若发现丢失率高再补 `tauri::App::on_exit` 钩子做 best-effort drain
- **隐私 header / `disable_geoip` 等价** = 自写 client 无 `disable_geoip` 参数 ——  PostHog 服务端的 geoip 是基于请求 IP，自写 client 默认就不会发任何 IP 增强字段（属性平铺由我们控制）。schema doc §6 隐私契约（客户端 IP 不上传）由 client 不主动 inject IP-derived 属性自然实现。后续若发现 PostHog 服务端仍按请求 IP 落地理字段，可在 properties 显式置 `"$geoip_disable": true`
- **测试策略** = 不联真实 PostHog；用 `wiremock` 或 `mockito` 起本地 HTTP server 验证 POST 形状（method + endpoint + body 字段）。或更简：把"构造 reqwest body"提到纯 fn `build_capture_body(event, ctx, api_key) -> serde_json::Value` 单测，HTTP 行为只测一次烟测

**架构图**（与 7a 对比）

```
                   AppDeps.analytics: Arc<dyn AnalyticsPort>
                              │
                  Gated(inner)  ← analytics_gate 守卫
                              │
            ┌─────────────────┼─────────────────┐
            ↓                                   ↓
       dev / debug                         release
       StdoutSink                          PosthogSink   ← 7b 新增
       tracing::debug                      tokio::spawn(reqwest POST)
                                                ↓
                                          reqwest 0.12 + rustls (ring)
                                                ↓
                                          POST https://us.i.posthog.com/i/v0/e/
                                          Body: {api_key, event, distinct_id, properties}
```

#### Slice 7b-1: 依赖与 sink 骨架（自写 reqwest client）
- [x] `uc-observability/Cargo.toml`：加 `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "rustls-tls-webpki-roots"] }` + `tokio = { version = "1", default-features = false, features = ["rt"] }`（仅 rt feature，spawn 用）
- [x] 验证：`cargo tree -p uc-observability -e features | rg -i 'aws-lc|openssl|native-tls'` 应为空（出现即 features 选错）—— ✅ 实测干净
- [x] `uc-observability/src/analytics/sinks/posthog.rs`：新增 `PosthogSink { client: reqwest::Client, api_key: String, endpoint: String, warned_missing_context: AtomicBool }` struct + `pub fn new(api_key: String) -> Self`（默认 endpoint = `POSTHOG_US_CAPTURE_ENDPOINT` 常量 = `https://us.i.posthog.com/i/v0/e/`）+ `pub fn with_endpoint(api_key: String, endpoint: String) -> Self`（测试 / self-host 用）
- [x] `sinks/mod.rs`：`pub mod posthog;` + `pub use posthog::PosthogSink;`
- [x] 暂时空 `impl AnalyticsPort`（capture 仅 `let _ = event;` 占位），先通过编译，capture 行为留 7b-2
- [x] 3 个骨架单测：默认 endpoint / 自定义 endpoint / `Box<dyn AnalyticsPort>` object safety
- **Status:** complete

#### Slice 7b-2: capture 实现 + payload wire
- [x] 私有纯 fn `build_capture_body(event_name, payload, api_key) -> Value`：移出 `event`/`distinct_id`、剩余字段进 `properties`、顶层加 `api_key`/`event`/`distinct_id`/`properties`/`timestamp(RFC3339)`
- [x] `PosthogSink::capture`：context 缺失 warn 节流 + `build_event_payload` + `build_capture_body` + `tokio::spawn` fire-and-forget reqwest POST + 非 2xx / Err 仅 warn
- [x] 4 个 `build_capture_body_*` 单测：顶层字段齐 / 字段冲突 invariant（properties 不含 event+distinct_id）/ distinct_id 缺失 fallback 空串 / property value 类型保留（number/bool 不退化为 string）
- [x] `posthog_sink_lifecycle` wiremock 烟测：起 MockServer，capture(AppFirstOpen) → 200ms 后断言收到 1 个 POST + body 顶层 + properties 不重复 / context 缺失分支 0 POST
- [x] **顺带修**：跨 fn 全局 `EventContext` 竞态——`context.rs` 加 `#[cfg(test)] lock_global_event_context_for_tests()` `OnceLock<Mutex<()>>` helper，`context::global_event_context_lifecycle` / `stdout_sink_lifecycle` / `posthog_sink_lifecycle` 三处 fn 入口都拿同一把锁（之前依赖"单 fn 内串行"约束在仅 1 个 lifecycle fn 时成立，现 3 个 fn 必须显式串行）
- **Status:** complete

#### Slice 7b-3: key 注入 + bootstrap factory + 降级路径
- [x] `uc-bootstrap/src/analytics.rs::build_analytics_sink` 保持 sync（自写 client 不需要 async 构造）—— assembly.rs 调用点零改动
- [x] release 路径：runtime env > `option_env!` > `info!("POSTHOG_PROJECT_KEY 未配置...") + Gated(NoopAnalyticsSink)`；拿到 key → `Gated(PosthogSink::new(key))`
- [x] dev (`cfg!(debug_assertions)`) 路径不变，仍 `Gated(StdoutSink)`
- [x] 抽出 `resolve_posthog_key(runtime: Option<String>, compile: Option<&'static str>) -> Option<String>` 私有 fn
- [x] 5 个 `resolve_posthog_key_*` 单测：runtime_only / compile_only / both（runtime 胜出）/ none / 空字符串等价于"未设置"
- [x] `uc-observability/src/analytics/mod.rs` re-export `PosthogSink`，给 uc-bootstrap 使用
- **Status:** complete

#### Slice 7b-4: CI secret 注入 + 文档 + 真实 dev 验证
- [x] `.github/workflows/build.yml`：在 `tauri-action` 与 `bun run tauri build` 两段 env 块加 `POSTHOG_PROJECT_KEY: ${{ secrets.POSTHOG_PROJECT_KEY }}`（与 `SENTRY_DSN` 同位）
- [x] `.github/workflows/alpha-build.yml`：同上（当前 alpha workflow 只有 `tauri-action` 构建段）
- [x] `docs/architecture/telemetry-events.md`：新增 §10.1 "PostHog Cloud 接入实务（v1）"——key 注入路径（三级回退）、endpoint / region、自写 reqwest client 根因（aws-lc-rs 与 musl 硬约束冲突）、fire-and-forget + 进程退出语义、`disable_geoip` 等价语义（自写 client 不主动 inject IP 字段）、CI secret 注入位置（计划）
- [x] `CONTRIBUTING.md` + `CONTRIBUTING_ZH.md`：在"Build a Release Bundle / 构建发行包"节后新增"Release-time Secrets (Telemetry) / 发布期 Telemetry Secrets"——3 个 secret 表（`SENTRY_DSN` / `VITE_SENTRY_DSN` / `POSTHOG_PROJECT_KEY`）含通道用途、编译期读取位置、CI 注入位置；强调缺 key 不阻塞构建 + 空串等价"未设置" + 永不提交到仓库
- [x] 真实 dev 验证步骤（不进自动化测试）：
  1. 本地 export `POSTHOG_PROJECT_KEY=phc_xxx`
  2. `cargo build --release -p uc-tauri`（绕过 dev 路径，强制走 PosthogSink）
  3. 跑一遍首次 onboarding，PostHog 控制台应能在 ~10s 内看到 `app_first_open` / `setup_started` / `pairing_*` 序列
  4. 翻 settings → "使用情况统计"关 → 再触发任意事件，PostHog 端应不再有新事件（gate wrapper 验证）
  5. unset env → 重启 → 再触发事件，sink 应静默（noop fallback 验证）
- [x] 验证完毕后把 Slice 7b 整体 status 翻 complete，并通知用户开始观察首批数据
- **Status:** complete

**子任务依赖图**：

```
7b-1 (依赖 + 骨架)
   ↓
7b-2 (capture 实现 + payload wire)
   ↓
7b-3 (key 注入 + factory async 化 + 降级)
   ↓
7b-4 (CI secret + docs + 手工验收)
```

7b-1 / 7b-2 不阻塞外部 PostHog account（用 disabled client 测）；7b-3 的"降级到 Noop"路径也不阻塞；只有 7b-4 真实事件验收必须等 PostHog 项目 + CI secret 就绪。

### Slice 8: 业务 use case 埋点（按 schema doc §7.1 / §7.2 接入）

整体侵入面太大（4 个 orchestrator + 1 个新 port + AppDeps 改动），按用户裁决拆 4 个独立 commit / 子 slice。

#### Slice 8a: sink 注入 AppDeps + factory + `app_first_open`
基础设施先行。完成后 dev 跑起来应能在 `RUST_LOG=uc_observability::analytics=debug` 下看到 `app_first_open` 单行 JSON。
- [x] `uc-observability/src/analytics/sinks/gated.rs`：新增 `GatedAnalyticsSink`（`capture` 入口统一 gate 守卫）
- [x] `uc-application/src/deps.rs`：AppDeps 加 `analytics: Arc<dyn AnalyticsPort>` 顶层横切字段
- [x] `uc-bootstrap/src/analytics.rs`：`build_analytics_sink()` factory
  - `cfg!(debug_assertions)` → `Gated(StdoutSink)`
  - 否则 → `Gated(NoopAnalyticsSink)`（release 临时态，直到 Slice 7b 接 PosthogSink）
- [x] `compose_event_context` 之后立刻 `analytics.capture(Event::AppFirstOpen)`，仅当 `is_first_run = true` 时（幂等门控由 compose 顶部 `global_event_context().is_some()` 守住）
- [x] `wire_dependencies`（`assembly.rs:879` AppDeps 构造点）补 `analytics: build_analytics_sink()`
- [x] gate 运行时切换走 `GatedAnalyticsSink` 包装层：sink 装一次永不替换，PUT handler 切 `usage_analytics_enabled` 只动 `analytics_gate` 静态值，wrapper 在 `capture` 入口 atomic load gate 决定是否 forward 给 inner sink。所有真实 sink 实现（StdoutSink、未来 PosthogSink）零 gate 感知
- [x] 测试：`gated_sink_lifecycle`（gate on/off forward 行为）。`app_first_open` first_run 双路径 integration test 评估为低 ROI 跳过——`load_or_create_ids` 的 first_run 语义已被 10 个 ids tests 守住
- **Status:** complete

#### Slice 8b: pairing 三事件

**触达端**：仅 joiner 端（`RedeemPairingInvitationUseCase`），sponsor 端留待后续。joiner 端是漏斗主路径且 use case 同步语义清晰（execute Ok = 完整握手 + persist 完成）。

**用户裁决（2026-05-09）**：
- 仅 joiner 端发三事件（sponsor 端 broadcast 链路留 Slice 8b' 后续 PR）
- `PairingMethod` v1 固定占位 `Code`（execute 签名零改动；GUI 区分维度延后）
- 新增专用 `PairingFailureReason` enum，与 `FailureReason` 解耦（每 domain 一份的演化方向）
- 测试：fake AnalyticsPort 带 Vec 缓冲，验证 capture 次数 + 事件 variant + failure_reason 映射

**子任务**：
- [x] schema doc `docs/architecture/telemetry-events.md`：§7.1 `pairing_failed` 字段从 `FailureReason` 改成 `PairingFailureReason`；§7.3 标注 sync 专用；§7.4 新增 `PairingFailureReason` 章节（**14** 个变体，含 `SponsorInternal` 与 `Internal`）
- [x] `uc-observability/src/analytics/events.rs`：
  - 新增 `PairingFailureReason` enum（snake_case wire 形态，14 变体）
  - `Event::PairingFailed.failure_reason` 字段类型改 `PairingFailureReason`
  - 钉死测试：`pairing_failure_reason_wire_format` + `pairing_method_wire_format`
- [x] `uc-application/src/facade/space_setup/deps.rs`：`SpaceSetupDeps` 加 `analytics: Arc<dyn AnalyticsPort>` 字段
- [x] `uc-bootstrap/src/space_setup.rs`：`SpaceSetupDeps` 构造点补 `analytics: Arc::clone(&deps.analytics)`（从 AppDeps 取）
- [x] `uc-application/src/usecases/pairing/redeem_invitation.rs`：
  - `RedeemPairingInvitationUseCase` 加 `analytics: Arc<dyn AnalyticsPort>` 字段（构造器 + new()）
  - `execute()` 入口：`Instant::now()` + `analytics.capture(Event::PairingStarted { method: PairingMethod::Code })`
  - 包到 async block，外层 match Result 后 fire `PairingSucceeded { method, peer_os: None, duration_ms }` 或 `PairingFailed { method, failure_reason: map(&e) }` 再 return
  - 私有 fn `map_redeem_error_to_pairing_failure_reason(&RedeemPairingInvitationError) -> PairingFailureReason`（14:14 完全覆盖）
- [x] `uc-application/src/facade/space_setup/facade.rs`：`SpaceSetupFacade::new()` 把 analytics 解构 + 传给 `RedeemPairingInvitationUseCase::new()`；内测 fake 用 NoopAnalyticsSink
- [x] 3 个 e2e tests（slice1 / slice2_phase1 / slice2_phase2）的 SpaceSetupDeps 构造点补 NoopAnalyticsSink
- [x] 测试：
  - `redeem_invitation::tests` 5 个原有测试每个加 capture 序列断言（happy: Started+Succeeded；4 个 failure: Started+Failed{对应 variant}）
  - `CapturingAnalyticsSink` test infra 落在 tests module 内（StdMutex<Vec<Event>>，与既有 Recording fakes 同款）
  - `map_redeem_error_covers_all_variants` 钉死 14:14 映射
- **Status:** complete

#### Slice 8b': sponsor 端 pairing 三事件 + broadcast 链路重构

**用户裁决（2026-05-09）**：
- 重构 broadcast 链路：`PairingOutcome::Failure` 字段类型 `String` → `PairingFailureReason`（最干净）
- sponsor 端 `pairing_started` 在 `IssuePairingInvitationUseCase::execute()` 入口 fire（与 joiner funnel 起点对齐）

**子任务**：
- [x] `uc-observability/src/analytics/events.rs`：`PairingFailureReason::as_str` + `Display` impl（snake_case wire 形态）
- [x] `uc-application/src/facade/space_setup/events.rs`：`PairingOutcome::Failure { reason: PairingFailureReason }` + re-export
- [x] `uc-application/src/facade/{mod,space_setup/mod}.rs`：`PairingFailureReason` 顶层 re-export
- [x] `uc-application/src/pairing_inbound/orchestrator.rs`：
  - 加 `analytics: Arc<dyn AnalyticsPort>` 字段
  - 加 `handshake_started_at: Arc<StdMutex<HashMap<PairingSessionId, Instant>>>`
  - `emit_failure(session, reason)` 改签名：先 fire `pairing_failed` event，再 send `PairingOutcome::Failure { reason }`
  - `on_incoming` 写 started_at；`finalise_verified` Success 时 fire `pairing_succeeded { duration_ms }`
  - 7 个 emit_failure 调用点改 enum；4 个 reason.contains 测试改 enum 等值
  - 加 3 个 analytics-focused 测试（succeeded / failed PassphraseMismatch / failed InvitationExpired）
- [x] `uc-application/src/usecases/pairing/issue_invitation.rs`：加 analytics + execute 入口 fire `pairing_started` + 4 测试加 capture 断言
- [x] `uc-application/src/facade/space_setup/facade.rs`：注入 analytics 给 IssuePairing + Inbound
- [x] `uc-webserver/src/api/setup_events.rs`：handler 用 `reason.to_string()`；2 个测试构造点改 enum；reason payload 断言改 `passphrase_mismatch` snake_case
- **Status:** complete

#### Slice 8c-1: sync 三事件（outbound per-peer，不动 port）

**用户裁决（2026-05-09）**：拆 8c → 8c-1（本切片）+ 8c-2（FirstSyncStatePort + first_*）；事件粒度 per-peer

**子任务**：
- [x] `uc-application/src/usecases/clipboard_sync/dispatch_entry.rs`：
  - 加 `analytics: Arc<dyn AnalyticsPort>` 字段
  - `payload_type_from_categories` 私有 fn（File > Image > Text 优先级）
  - `map_dispatch_error_to_failure_reason` 私有 fn（5 变体 1:1）
  - fan-out 改：spawn 内 fire `SyncAttempted` → dispatch → 按 Result fire `SyncSucceeded`/`SyncFailed`
  - `transport_type=P2pDirect`、`peer_os=None`、`sync_latency_ms` per-peer Instant 计时
  - 加 `CapturingAnalyticsSink` test fake + `build_uc_with_analytics` helper
  - 加 3 测试（happy 4 events 顺序 + 字段断言 / Offline → SyncFailed{PeerOffline} / `map_dispatch_error_covers_all_variants`）
- [x] `uc-application/src/facade/clipboard/facade.rs`：`ClipboardSyncDeps` 加 analytics 字段；facade::new 透传；内测 build_facade 补 NoopAnalyticsSink
- [x] `uc-bootstrap/src/space_setup.rs`：构造点补 `analytics: Arc::clone(&deps.analytics)`
- [x] `uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs`：构造点补 NoopAnalyticsSink
- **Status:** complete

#### Slice 8c-2: `FirstSyncStatePort` + first_clipboard_sync_* / first_file_sync_succeeded 事件

**用户裁决（2026-05-09）**：
- **触发语义** = 双 flag 独立：`_attempted` 在首次 attempt（成功/失败均记）记一次；`_succeeded` 在首次成功记一次。事件名字面意思 + funnel 漏点信号完整
- **Race 防护** = port impl 内部 `tokio::sync::Mutex` 串行 read-check-write（与 `AppVersionStatePort` 风格对称；fan-out 全过此锁）
- **范围** = 一并做 `first_file_sync_succeeded`：Port 三 flag、JSON schema 三字段；dispatch_entry 内根据 `payload_type=File` 分支额外 fire
- **测试** = infra 7 个 tokio test（仿 AppVersionStatePort：missing/round-trip/overwrite/corrupt/empty/schema-mismatch/parent-dir）+ use case 1 个 first-path 断言 + 显式 race 测试（`tokio::join!` 多 spawn 同 mark，断言 true 仅一次）

**Port API 形状**（uc-core/src/ports/first_sync_state.rs）：
```rust
#[async_trait]
pub trait FirstSyncStatePort: Send + Sync {
    async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError>;
    async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError>;
    async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError>;
}
```
返回值约定：`Ok(true)` = 本次为首次置位（调用方应 fire 事件）；`Ok(false)` = 已被 mark（不 fire）。

**JSON schema**（uc-infra）：
```json
{ "schema_version": 1, "attempted": bool, "succeeded": bool, "file_succeeded": bool }
```

**4 个构造点 wiring**：
1. `AppDeps`（`uc-application/src/deps.rs:139-168`）加 `first_sync_state: Arc<dyn FirstSyncStatePort>`；`uc-bootstrap/src/assembly.rs:404-410` InfraLayer 构造点 + AppDeps 聚合点装配
2. `ClipboardSyncDeps`（`uc-application/src/facade/clipboard/facade.rs:42-57`）加同字段；`uc-bootstrap/src/space_setup.rs:390-402` 透传 `Arc::clone(&deps.first_sync_state)`
3. `DispatchClipboardEntryUseCase::new`（`uc-application/src/usecases/clipboard_sync/dispatch_entry.rs:176-198`）加参数；spawn 内三处 mark + 条件 fire（attempted 在 SyncAttempted 后；succeeded + file_succeeded 在 SyncSucceeded 后）
4. e2e 测试构造点（`uc-bootstrap/tests/slice2_phase2_clipboard_e2e.rs` 必补；slice1/slice2_phase1 若引用 AppDeps 也需补）

**子任务**：
- [x] `uc-core/src/ports/first_sync_state.rs`：新增 trait + `FirstSyncStateError` enum（Read/Write/Corrupt 三变体仿 `AppVersionStateError`）；`mod.rs` re-export
- [x] `uc-infra/src/first_sync_state.rs`：`FileFirstSyncStateRepository` 仿 `FileAppVersionStateRepository` 模板（`with_defaults(app_data_root)` → `app_data_root/first-sync-state.json`）；内部 `tokio::sync::Mutex` 串 read-check-write；tempfile + rename 原子写；`lib.rs` re-export
- [x] infra 7 测试：missing → 全 false / round-trip / overwrite / corrupt JSON / empty file / schema-mismatch / parent-dir 自动创建
- [x] infra 1 race 测试：`tokio::join!` 8 个 spawn 同时 `mark_first_sync_attempted`，断言只有 1 个返回 true
- [x] 构造点 1：`AppDeps.first_sync_state` 字段 + `assembly.rs` InfraLayer + AppDeps 聚合
- [x] 构造点 2：`ClipboardSyncDeps.first_sync_state` 字段 + facade 透传 + `space_setup.rs` 装配
- [x] 构造点 3：`DispatchClipboardEntryUseCase` struct field + new 参数；spawn 内 mark + 条件 fire 三 first_* 事件
- [x] 构造点 4：`slice2_phase2_clipboard_e2e.rs` 补 NoopFirstSyncState；slice1 / slice2_phase1 不需要（不构造 ClipboardSyncDeps）
- [x] use case 1 测试：first-path 断言事件序列含 `first_clipboard_sync_attempted` + `first_clipboard_sync_succeeded` + `first_file_sync_succeeded`（payload_type=File 分支三事件 each-once 验证）
- [x] cargo check --workspace + 跨 crate test 全绿
- **Status:** complete

#### Slice 8d: setup 两事件

**用户裁决（2026-05-09）**：A1 `InitializeSpaceUseCase` 入口 fire `setup_started`；`resolve_and_persist_device_name` 收尾 fire `device_name_set`

**子任务**：
- [x] `uc-application/src/usecases/setup/initialize_space.rs`：
  - 加 `analytics: Arc<dyn AnalyticsPort>` 字段
  - `execute` 入口 fire `Event::SetupStarted { entry: SetupEntry::FirstRun }`（v1 占位）
  - `resolve_and_persist_device_name` 收尾 fire `Event::DeviceNameSet { name_length_bucket: NameLengthBucket::from_char_count(...) }`
  - 加 `CapturingAnalyticsSink` + Harness::analytics 字段
  - 加 4 测试断言（happy / DeviceNameRequired / PassphraseMismatch / NameLengthBucket 三 case 边界）
- [x] `uc-application/src/facade/space_setup/facade.rs`：InitializeSpaceUseCase::new 透传 analytics（第 8 参数）
- **Status:** complete

### Slice 9: settings UI 拆分两个开关
- [ ] 前端 SettingsView 新增 `usage_analytics_enabled` 控件
- [ ] 文案区分"错误与崩溃上报" vs "使用情况统计"
- [ ] e2e 验收：toggle 后 telemetry 立即生效，无需重启
- **Status:** pending

### Slice 10: 验收 + dashboard
- [ ] PostHog Cloud 项目创建、API key 配置、US endpoint（2026-05-09 注册区域；schema doc §10）
- [ ] 5 张 dashboard：渠道漏斗 / 首次同步成功率趋势 / 同步成功率 + p95 + 失败原因 / D1+D7 留存 / OS 组合矩阵
- [ ] 跑一周真实数据后 close issue
- **Status:** pending

## Key Questions

1. ~~**`active_device_count` 取数源**~~：✅ Slice 6 裁决 `member_repo.list().await.len()`，不用 0 占位。
2. ~~**bootstrap 调用点**~~：✅ Slice 6 裁决放在 `wire_dependencies` 之后，`build_core` 转 async。
3. ~~**Slice 7 dev/prod sink 切换**~~：✅ Slice 7a 裁决走 runtime，与 telemetry_gate 一致。
4. ~~**PostHog Cloud 账号**（Slice 7b 阻塞项）~~：✅ 规划阶段裁决：注入策略完全镜像 SENTRY_DSN 三级回退（运行时 env > `option_env!` > 关闭）。账号创建与 CI secret 注入仍为外部阻塞，但代码侧落地已不依赖该决策。
5. **Slice 8 sink 注入位置**：`AppDeps` 加 `analytics: Arc<dyn AnalyticsPort>` vs 全局单例（类似 `global_event_context`）？前者 testability 更好，后者更轻。

## Decisions Made

| Decision | Rationale |
|---|---|
| 后端选 PostHog Cloud（US endpoint，实际注册区域），不自研 | 早期 < 10 用户、self-host 维护成本不划算；schema 与 SDK 解耦保证迁移自由；US 与 EU region 隐私模型等价（SOC 2 + GDPR DPA + SCC），§6 隐私契约与 region 正交 |
| 双开关而非合并 | GDPR 友好——"报错"和"用量统计"两件事；保留 `telemetry_enabled` 字段名零迁移 |
| `analytics_device_id` 独立于 `uc-core::DeviceId` | 防 cross-system correlation；schema doc §3.1 明确 disjoint 约束 |
| EventContext 用 `RwLock<Option<Arc<...>>>` 而非 `OnceLock` | 用户重置 telemetry IDs 后需要原地替换 context |
| `timestamp` 不放进 EventContext | context 是 session 级、事件 timestamp 是事件级。PostHog SDK 自动注入 `$timestamp` |
| 探测失败一律 `"unknown"` 占位 | Telemetry 缺字段比缺事件代价小 |
| stdlib + chrono only，不引 `os_info` / `sys-locale` | v1 已知局限留作 polish |
| 事件名永不重命名 | schema doc §5.3 / §8；变更走 `*_v2` 新 variant + deprecated 标注 |
| `Event::properties` 不含 context 字段 | sink 负责合并；调用方只关心"发生了什么" |
| 全局测试用单一 fn 而非 `serial_test` 依赖 | Rust `#[test]` 默认线程并行，多个 fn 改同一 RwLock 会竞态 |
| StdoutSink 走 `tracing::debug!` 而非裸 `println!` | schema doc §6.5 约定；与 dual-output（pretty console + JSON file）一致；release 默认级别自然吞掉 |
| `build_event_payload` 提到 sinks/mod.rs 共用 | StdoutSink / 未来 PosthogSink wire 形态等价；切 sink 不改 dashboard 字段 |
| Sink 输出加 `distinct_id = anonymous_user_id` | PostHog 漏斗主键约定；schema §3.1 留存锚点 |
| context 缺失 → 丢事件 + warn 节流 | 半截事件比缺事件更难调；warn 一次/sink 实例避免启动早期 spam |
| Slice 7 仅做 StdoutSink，PosthogSink 留 7b | PostHog 账号 + key 是外部 blocker，不阻塞 Slice 8 进度 |
| Sink 切换走 runtime 而非 cargo feature | 与 telemetry_gate 风格一致；单一二进制可调试 |
| `usage_analytics_enabled` 运行时门控走 `GatedAnalyticsSink<inner>` wrapper | gate 是横切关注点不该污染 sink 实现；与 telemetry_gate 在 tracing layer 做 filter 的思路对称；sink 装一次永不替换，settings PUT handler 仅改 atomic 静态值 |
| Slice 8c-2 first_* 事件：`_attempted` 与 `_succeeded` 双 flag 独立 | 事件名字面意思（`_attempted` = 任何首次 attempt，无论后续成功失败）；funnel 漏点信号完整——"用户尝试过但首次失败"会留 attempted 但无 succeeded 的间隙信号；多写 1 行代价可接受 |
| Slice 8c-2 race 防护放 port impl 内部 `tokio::sync::Mutex` | 与 `AppVersionStatePort` 文件实现风格对称；fan-out N 个 peer 全过同一锁、串行 read-check-write，无须 use case 层 atomic CAS 兜底；race 测试可显式覆盖；fan-out 量级（< 10 peer）远不到磁盘 IO 瓶颈 |
| Slice 8c-2 范围含 `first_file_sync_succeeded` | schema doc §7 已预留；Port 三 flag 一次到位避免后续重打开 wiring；dispatch_entry 已能从 categories 推断 `payload_type=File` 分支，多 fire 一行成本极低 |
| Slice 8c-2 mark 在 fire 之前 | port `mark_*` 返回 `Ok(true)` 才 fire，意味着"先置位再 fire"；事件丢一次比误报多次更可接受（首次同步事件只该有一次） |
| Slice 7b PosthogSink fire-and-forget = `tokio::spawn` 而非 `block_on` / 内部队列 | `AnalyticsPort::capture` 是同步签名（schema doc §10）；`reqwest::Client::post(...).send()` 是 async。spawn 一条独立 task 保 capture 不阻塞业务（< 几 µs）。HTTP 失败仅 warn，不传播 |
| Slice 7b key 注入完全镜像 SENTRY_DSN 三级 | 运行时 env > `option_env!` 编译期 > 关闭。已在 SENTRY_DSN 路径验证（`tracing.rs:155-170`）；不引入新 secret 注入机制。CI 在 build.yml / alpha-build.yml 同位加 `POSTHOG_PROJECT_KEY` |
| Slice 7b release 缺 key → `Gated(NoopAnalyticsSink)` 而非启动失败 | 产品 telemetry 是辅助通道，不应反向影响 daemon / GUI 启动可用性。降级时打一次 info（非 warn，缺 key 是合法配置）记录"PostHog 未配置" |
| Slice 7b 自写 reqwest client，不用 posthog-rs SDK ⚠️ 转向决策 | posthog-rs 0.7 hardcode `reqwest = "0.13.2"` + features `["rustls"]`，reqwest 0.13 的 rustls feature 隐式选 aws-lc-rs（C 库 + CMake），破坏 uc-cli musl 静态编译"零 C 工具链"硬约束（sentry 已为此用 ureq + ring 而非 reqwest 0.13）。cargo features unification workspace 级，无法 gate 排除。自写 ~100 行 reqwest 0.12 + rustls(ring) HTTP client：失去 SDK batching + retry，但 schema doc §10 允许 < 1% 丢失，fire-and-forget 单条 POST 够用 |
| Slice 7b 隐私契约 = 不主动 inject IP 字段（替代 SDK `disable_geoip`） | 自写 client 不发 IP-derived 属性，PostHog 端的 geoip 默认基于请求 IP；schema doc §6 由 client 不主动 inject 自然实现。如服务端仍按 IP 落地理字段，可在 properties 显式置 `"$geoip_disable": true` 兜底 |
| Slice 7b 不挂显式进程退出 flush 钩子 | 自写 client 无应用级队列，单条 POST 一旦 spawn 走自己网络生命周期；reqwest 连接池 Drop 时关闭。产品事件丢失 < 1% 在 schema doc §10 已可接受。后续若发现首条事件丢失率高再补 `tauri::App::on_exit` 做 best-effort drain |
| Slice 7b `build_analytics_sink` 保持 sync | 自写 client 构造（reqwest::Client::new + String 字段）全部同步；与 SDK 方案需要 `async fn client(opts)` 不同，传染面 = 0，`assembly.rs:947` 调用点零改动 |
| Slice 7b 测试不联真实 PostHog | CI 不应往生产 telemetry 服务发数据。纯 fn `build_capture_body` 单测覆盖 body 形态；HTTP 路径用 `wiremock` 起本地 mock server 烟测 1 case |

## Errors Encountered

| Error | Attempt | Resolution |
|---|---|---|
| `Uuid: Deserialize` trait not satisfied | 1 | uc-observability/Cargo.toml 给 uuid 加 `serde` feature |
| `chrono::DateTime: Deserialize` trait not satisfied | 1 | 同上，给 chrono 加 `serde` feature |
| `missing field usage_analytics_enabled in GeneralSettingsPatch` | 1 | uc-application/facade/app_facade.rs 显式构造点补字段 |
| `missing field usage_analytics_enabled in GeneralSettingsPatchDto` | 1 | uc-webserver/tests/settings_network_smoke.rs 显式构造点补字段 |
| 全局 EventContext 测试在并行 cargo test 下竞态 | 1 | 合并 `round_trip` + `supports_replacement` 到单个 test fn 串行化 |
| Python 缺 `yaml` 模块，无法用 PyYAML 解析 workflow | 1 | 改用系统 Ruby `YAML.load_file` 解析 `.github/workflows/build.yml` 与 `.github/workflows/alpha-build.yml`，两者均通过 |
| release 编译发现 `default_usage_analytics_enabled` 不存在 | 1 | 删除字段级默认函数引用，沿用 `GeneralSettings::Default` 的统一缺字段回退；`cargo check -p uc-core`、`cargo test -p uc-core`、`POSTHOG_PROJECT_KEY=phc_compile_probe cargo build --release -p uc-tauri` 均通过 |

## Pre-existing failures（不属于本任务）

- `uc-daemon-contract::api::auth::DaemonConnectionInfo::fmt` doctest 编译失败：缺 `use` 语句，git diff 确认未碰。建议另起 PR 修
- `uc-platform`、`uc-tauri` 几个 doctest 同样 scope 问题，预先存在

## Notes

- 已建文件全部在 `uc-observability/src/analytics/` 下，符合 schema doc §9 的边界（不污染 uc-core）
- 共改 6 个 crate（uc-core / uc-application / uc-daemon-contract / uc-webserver / uc-bootstrap / uc-observability），所有 Slice 5 修改严格按 `telemetry_enabled` 同位映射
- 文档：`docs/architecture/telemetry-events.md` 是 v1 schema 的单一真相源；任何后续字段改动必须先改文档再改代码
