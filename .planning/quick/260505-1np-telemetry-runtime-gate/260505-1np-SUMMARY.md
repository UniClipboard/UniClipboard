---
id: 260505-1np
slug: telemetry-runtime-gate
description: 后端 Sentry/OTLP 改成 runtime gate，撤回 telemetry 触发 restart_required
status: complete
created: 2026-05-05
completed: 2026-05-05
supersedes: 260505-17q（部分 — 后端 init-time gate 那部分）
---

# 260505-1np SUMMARY

## 背景

`260505-17q` 把后端 Sentry / OTLP 都做成 init-time gate，与已有 OTLP 行为一致。
但用户视角看，"开关 telemetry 应该立即生效"才是合理预期。本轮把后端两端都改成 runtime gate，对齐前端，撤回 `restart_required` 信号。

## 改动

### 1. uc-observability 暴露 telemetry runtime gate — `9da05fb6`
- 新增 `telemetry_gate.rs`：`static AtomicBool TELEMETRY_ENABLED`（默认 `true`，避免启动期事件被吃）
- 公开 `is_telemetry_enabled()` / `set_telemetry_enabled(bool)`
- `lib.rs` re-export

### 2. OTLP 层 runtime gate via FilterFn — `4414e587`
- `otlp/layer.rs` 与 `otlp/logs_layer.rs` 在已有 `EnvFilter` 之上叠 `FilterFn`，
  关掉时丢弃 span / log event
- `otlp/provider.rs` 不再需要 `telemetry_enabled` 参数 — endpoint 配置就 init provider
- 公共 API（`init_otlp_provider` / `init_otlp_pipeline*`）签名变了：去掉了
  `telemetry_enabled` 参数。所有 caller 一并更新

### 3. Sentry runtime gate + webserver 接线 — `626ef96a`
- `uc-bootstrap/src/tracing.rs`：sentry 只看 DSN 即 init；
  `ClientOptions.before_send` 与 `before_breadcrumb` 闭包检查
  `is_telemetry_enabled()`，关掉时返回 `None` —— 包括 sentry-panic integration
  发出的 panic Exception 也走同一过滤路径
- 启动期把 disk 上的 `telemetry_enabled` 推入 atomic（防遗漏初始状态）
- `uc-webserver` 增加 `uc-observability` dep；handler 写盘成功后调
  `set_telemetry_enabled(value)`；`restart_required` 撤回到只看 `network`
- 测试 `telemetry_toggle_signals_restart` 重写为 `telemetry_toggle_runtime_gate_no_restart`，
  锁定两件事：toggle 不再触发 restart + atomic 被正确推进

## 验证

| 检查 | 结果 |
|---|---|
| `cargo check --workspace --all-targets` | ✅ Finished |
| `cargo test -p uc-observability telemetry_gate` | ✅ 2/2 |
| `cargo test -p uc-webserver --test settings_network_smoke` | ✅ 4/4 (含新 `telemetry_toggle_runtime_gate_no_restart`) |

## 结果对照

| 维度 | 17q 完成后 | 本轮 1np 后 |
|---|---|---|
| 前端 Sentry | runtime | 不变 |
| 前端 OTLP | runtime | 不变 |
| 后端 Sentry | **init-time** | **runtime** ✓ |
| 后端 OTLP | **init-time** | **runtime** ✓ |
| `PUT /settings restart_required` | telemetry 变更触发 | **不再触发**（仅 network） |

## 用户视角行为

- **关掉 Telemetry**：前后端 Sentry / OTLP **立即**停止上报，无需任何重启
- **开启 Telemetry**：同样立即恢复

## 实现关键点

1. **FilterFn 类型签名 trick**：`FilterFn::new` 默认 `F = fn(&Metadata<'_>) -> bool`。
   闭包会改变 generic 让公共 type alias 变形 → 改用顶层 `fn` 项 + 显式
   `type TelemetryGateFn = fn(&Metadata<'_>) -> bool` 局部 binding，把 fn item
   coerce 成 fn pointer 再传入。

2. **panic 双重上报防御保留**：`sentry-tracing` layer 仍然 `event_filter` 跳过
   `target = "panic"`；panic event 由 sentry-panic integration 单独上报，
   它的 event 也会过 `before_send` 被运行时 gate 拦下，不会泄露。

3. **写盘 → atomic 顺序**：handler 在 `facade.update().await` 成功后才调
   `set_telemetry_enabled` —— 持久化失败时 atomic 不被污染。

## 不在范围

- 前端任何改动（17q 已经把前端做成 runtime）
- `telemetry_enabled` 默认值或 UI 文案
- Sentry / OpenTelemetry crate 的 feature-gate 化或依赖删除

## 关联 commits

- `9da05fb6` feat(observability): add process-wide telemetry runtime gate
- `4414e587` feat(otlp): runtime-gate trace and logs layers via FilterFn
- `626ef96a` feat(sentry): runtime-gate backend Sentry and wire toggle without restart
