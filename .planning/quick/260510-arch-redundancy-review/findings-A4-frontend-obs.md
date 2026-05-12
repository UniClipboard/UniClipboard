# A4 Findings — Frontend (`src/`) + `uc-observability`

Base: `main` · HEAD: `ea09cdd3`
范围：`src/` (+5666/-1648) + `src-tauri/crates/uc-observability/` (858 行)

## 结论速读

| 信号 | 等级 |
|---|---|
| OTLP 后端 Rust 代码 / Cargo 依赖清理干净 | 绿 |
| OTLP 在前端 **用户披露文案** 中残留 | 🔴 |
| 方案 C 切换后 `restart_daemon` / `daemon-restarting` 残留 | 绿 (零命中) |
| `RestartBanner` 存在但 MobileSync 平行重写一份 | 🟡 |
| `experimental-features.ts` 注册表只覆盖 2 key | 🟢 |
| `clipboardSlice` ↔ `SettingContext` 状态重叠 | 绿 |

## 🔴 必修

### OTLP 在 LanOnlyDisclosure 用户披露面板未跟随后端下线

OTLP 整套已在 `c8a4d5d9` / `642a0690` 废弃，走 Sentry Logs, 但披露给用户的类目仍是 "OTLP":

- `src/components/setting/LanOnlyDisclosure.tsx:14` — `const DISCLOSURE_KEYS = ['rendezvous', 'otlp', 'pkarr', 'autoUpdate']`
- `src/i18n/locales/en-US.json:215-218` — `"otlp": { "title": "OTLP telemetry", ... }`
- `src/i18n/locales/zh-CN.json:206-208` — 中文同步过时
- 测试：`LanOnlyDisclosure.test.tsx:67`, `NetworkSection.test.tsx:456`

文案描述本身只是说"遥测由通用开关控制"还能用，但 **类目名 `otlp` + 标题 "OTLP"** 是对已删除技术栈的不必要泄露。建议 key 改名 `'telemetry'`, 标题改 "遥测 / Telemetry"。

## 🟡 可削减

### RestartBanner 复用承诺未兑现，MobileSyncSettingsSheet 平行重写

`src/components/setting/RestartBanner.tsx:13-15` 自述：

> "Phase 95 暂硬编码 `settings.sections.network.restartBanner.*` 路径; 后续 Phase 96/97 若复用，重构为 props.messageKey 注入即可。"

注释还强调 "**per CONTEXT D-A1: 不复用 shadcn Alert**"。

但 `src/components/device/MobileSyncSettingsSheet.tsx:256-277` 又写了：

- 用了 shadcn `<Alert>` (正是 RestartBanner 想避免的反例)
- 三按钮 UI 自实现
- 独立 i18n key `devices.mobileSync.lanListener.restartRequired.{message,restartButton,dismissAriaLabel}`
- 独立 `restartRequired` + `restartDismissed` state

**同一 UI 概念两套实现 + 两套文案**。RestartBanner 的 i18n key 硬编码是没复用的直接借口。建议 banner 加 `messageKey` / `restartLabelKey` props (或接 `messages: {message, restart, restarting, retry, dismissAria}` 对象), MobileSync 改调 `<RestartBanner>`; 否则就承认 RestartBanner 只是 NetworkSection 专属，删掉那段复用承诺。

## 🟢 待定

### 1. `experimental-features.ts` 注册表早于第二个 caller

当前只 2 个 key (`network.lanOnly`, `network.allowOverlayAddrs`), 都在同一 NetworkSection。三件套 (registry + Badge + SettingRow.experimentalKey prop) 比 inline `<ExperimentalBadge />` 多一层间接。半年内若无第二组进实验，可回归 inline。

### 2. `RotatedPasswordModal` ↔ `MobileSyncCredentialModal` 部分模式重复

共享 "一次性凭据 + ack-to-close + secret-not-logged" 模式，注释互相 cross-ref "与 X 的关键差异"。差异真实，不建议合并; 但 `acknowledged / hintActive / tryClose / handleClose` 四态可抽 `useOneShotCredentialAck()` hook。

## 已核查无问题

- **P0 后端**: `uc-observability/Cargo.toml` 无 `opentelemetry*` / `sentry*` 依赖，`src/*.rs` 仅 `redact.rs:3` 一处历史注释提及 "OTLP" (说明用，可留)。`telemetry_gate::set_telemetry_enabled` 是新的统一控制点，被 `uc-webserver/src/api/settings.rs:117`, `uc-bootstrap`, 测试正确消费。
- **P1 方案 C**: `restart_daemon` / `daemon-restarting` / `daemon-ready` / `registerDaemonRestartListener` 在 `src/` 全部零命中。`registerDaemonShutdownListener` 是新增 graceful-shutdown 机制 (配 `uc-tauri/src/run.rs:51` 的 `app://shutting-down`), 不是 reload 残留。`daemonReady` 是 health-check 字段，跟 reload 无关。
- **P3 i18n daemon-restart**: 零命中。`restartBanner` (settings) 与 `restartRequired` (mobileSync) 两套 key 都有 consumer, 没有死 key (但已记入 🟡 #2)。
- **P4 测试对称**: 新增的 `NetworkSection.test.tsx` (467 行) / `SettingContext.network.test.tsx` (289 行) / `RestartBanner.test.tsx` / `LanOnlyDisclosure.test.tsx` 全部针对新建产品代码，未覆盖已删除的旧 OTLP / daemon-reload 分支。
- **P5 死 export**: `src/types/setting.ts` (+20 行) 是公共类型 hub, 至少 12 处 consumer; `experimental-features.ts` 见 🟢 #1。
- **P6 状态重叠**: `clipboardSlice` (+10 行) 是 410 → `PAYLOAD_UNAVAILABLE` 错误分支扩展，与 `SettingContext` (+36 行，主要是 `updateNetworkSetting` + telemetry mirror useEffect) 无概念重叠。
- **sentry.ts localStorage 镜像** `uc.telemetry_enabled` 不是冗余 — 前端无法同步读盘 settings.json, 后端 `uc-bootstrap::tracing` 同步读盘后调 `set_telemetry_enabled` 是对称的等价机制。

## 一句话总结

主线干净。**唯一硬伤** 是 OTLP 在用户披露文案里残留 (`LanOnlyDisclosure` 的 `'otlp'` key + i18n title), 必须随后端 OTLP 下线同步改名。**最值得削减** 的是 RestartBanner 喊了 Phase 96 复用却在 MobileSync 平行重写一份 Alert, 应该接 `messageKey` props 让 MobileSyncSettingsSheet 复用。其它都是抽象密度的判断题，不是冗余。
