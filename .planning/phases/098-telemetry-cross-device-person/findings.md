# Findings: 098 · Telemetry 跨设备 Person 聚合 v2

## 关键代码锚点（已实地核对）

### `uc-observability` 内部
- `src-tauri/crates/uc-observability/src/analytics/context.rs`
  - `EventContext` 字段表（`anonymous_user_id` / `analytics_device_id` / `session_id` / `app_version` / `app_channel` / `os` / `os_version` / `arch` / `locale` / `timezone` / `install_source` / `is_first_run` / `active_device_count` / `space_id_hash`）
  - `EventContextInputs` 镜像字段（无 `session_id`）
  - `build_event_context(EventContextInputs) -> EventContext`
  - 进程级 `static GLOBAL_EVENT_CONTEXT: RwLock<Option<Arc<EventContext>>>`
  - 测试用串行锁 `lock_global_event_context_for_tests`
- `src-tauri/crates/uc-observability/src/analytics/ids.rs`
  - 持久化 API `load_or_create(analytics_dir) -> AnalyticsIds { anonymous_user_id, analytics_device_id, is_first_run }`
  - `reset(analytics_dir)`：删除两个文件
  - 文件名常量 `INSTALLATION_ID_FILE = "installation_id"`、`ANALYTICS_DEVICE_ID_FILE = "analytics_device_id"`
  - 原子写：`atomic_write(path, content)` 走 `<file>.tmp -> rename`
- `src-tauri/crates/uc-observability/src/analytics/sinks/mod.rs:62`
  - `build_event_payload(&Event, &EventContext) -> Map<String, Value>`
  - 当前 `distinct_id` 直接拷 `payload["anonymous_user_id"]`
- `src-tauri/crates/uc-observability/src/analytics/sinks/posthog.rs`
- `src-tauri/crates/uc-observability/src/analytics/sinks/stdout.rs`
- `src-tauri/crates/uc-observability/src/analytics/sinks/gated.rs`
- `src-tauri/crates/uc-observability/src/analytics/port.rs`：`AnalyticsPort` 当前接口

### `uc-application` use case
- `src-tauri/crates/uc-application/src/usecases/setup/initialize_space.rs`
  - A1 入口：emit `setup_completed`
- `src-tauri/crates/uc-application/src/usecases/pairing/redeem_invitation.rs:144,218`
  - A2 入口：emit `pairing_succeeded`
- `src-tauri/crates/uc-application/src/usecases/setup/switch_space/mod.rs`
- `src-tauri/crates/uc-application/src/pairing_outbound/joiner_handshake.rs:65-77`
  - `JoinerHandshakeOutcome`：joiner 端解析 sponsor 回包
- `src-tauri/crates/uc-application/src/pairing_inbound/...`
  - sponsor confirm payload 写入点

### Facade
- `src-tauri/crates/uc-application/src/facade/space_setup/facade.rs`
  - 外部 crate 唯一入口（AGENTS §11.4）

### 文档
- `docs/architecture/telemetry-events.md`
  - §3 身份模型、§3.1 三层 ID 表、§3.2 持久化路径、§3.3 重置语义、§3.4 v2 口径、§4 EventContext 字段、§7 事件清单、§8 wire 演化、§10.1 PostHog wire 形态、§6 隐私契约

## 已敲定的开放问题决策

| 问题 | 决策 |
|---|---|
| v1→v2 升级老 Space 处理 | A：不做迁移。已升级设备继续 Solo，直到 Space 内有新 pairing 时才生成 space_person_id 并下发 |
| Pairing payload v1↔v2 互操作 | A：joiner 端字段为 `Option<Uuid>`；None 时退化为 Solo |
| `$identify` 失败 fallback | A：维持 fire-and-forget。允许 <1% 丢失 |
| `switch_space` 跨 Space 事务性 | identify 在 switch_space commit phase 之后发，commit 失败则不发 |
| `space_person_id` 是否进 settings 导出 | 不进入 export-to-file（避免泄露）；进入 Space 级别同步范围 |
| Group device_count 更新时机 | sponsor 在 `pairing_succeeded` 之后重发 `$groupidentify` 自增 device_count |

## 架构红线（必须遵守）

1. analytics 模块 **不允许** 读取或派生自 `uc-core::DeviceId`（schema doc §3.1）
2. `space_person_id` 必须独立生成，不可从 `space_id` 反推（独立 UUIDv7）
3. 永不上传 PII（schema doc §6.1）
4. wire 演化非破坏：`distinct_id` 字段名不变，只换取值来源
5. 外部 crate 经 Facade 访问 uc-application（AGENTS §11.4）
6. GUI 走 in-process facade，不经 webserver
7. 文档/注释中文，新文件 `//!` 头条段先讲"为什么需要这个模块"
8. 不留并行新旧代码 —— v1→v2 一次性切换
