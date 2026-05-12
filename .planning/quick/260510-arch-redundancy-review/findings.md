# Findings 汇总 — 架构冗余 Review

Base: `main` (`07adc0bc`) · HEAD: `ea09cdd3` (spot-capricorn)
范围：204 commits / 971 files / +63863/-78089 行 (含工具链，产品代码 ~50K 行)

子文件：[A1 app+core](findings-A1-app-core.md) · [A2 infra+io](findings-A2-infra-io.md) · [A3 bootstrap+shell](findings-A3-bootstrap-shell.md) · [A4 frontend+obs](findings-A4-frontend-obs.md)

## 速答用户的核心问题

> 方案 C 取消 in-process daemon reload 之后，Phase A/B/C 为 reload 做的铺垫是否变成冗余？

**部分是，部分不是**:

| 铺垫 | 当前命运 | 理由 |
|---|---|---|
| `ArcSwapOption<XxxFacade>` (5 字段) | 🔴 **真冗余** | swap 频率从设计的"每次 reload"降为"一辈子 1 次", swap_out 实际撞在进程退出。应回退 OnceCell |
| `SearchFacade::clear_coordinator` | 🔴 **死代码** | 全工程零调用点。set/clear 二选一 |
| `Clone` derive on AppDeps/WiredDependencies | 🟡 **保留，改注释** | clone 调用点都是启动期 fan-out, 不是 reload, 但 derive 本身廉价，论据需重写 |
| `build_process_runtime` / `build_daemon_lifecycle` 拆分 | 🟢 **保留** | 真正理由是 async/sync 边界 + standalone daemon binary 与 GUI shell 共用同一套装配 (`uniclip daemon` 仍是生产路径) |
| `BackgroundRuntimeDeps` 拆出 | 🟢 **保留，改注释** | 物理理由 (mpsc Receiver 不可 Clone) 仍成立，但注释里"为 reload 准备"那层论据要换 |
| `ProcessRuntimeHandles` 4 字段透传 | 🟡 **可精简** | 后 2 字段属于"BackgroundRuntimeDeps 一次性消费"副作用，可再合并一层 |
| `graceful_shutdown_port_reuse` 测试 | 🟢 **保留，改注释** | `app.restart()` 也需要端口及时释放，契约价值未变，但文件头注释还在说 "in-process reload" |

**一句话**: 真正变冗余的是 ArcSwap 这一套 **热切换原语**, 大约 110 行代码 + 120 行注释需要回退。其余 Phase A/B/C 拆分 (函数边界、Clone derive、deps 共享、standalone binary 共用)**理由变了但物理价值仍在**。

---

## 🔴 必删 / 必修清单

### R1. `SearchFacade::clear_coordinator` 零调用点

- `uc-application/src/facade/search/mod.rs:120`
- daemon cleanup 路径 (`uc-desktop/src/daemon/host.rs:255-258`) 只调 `clear_daemon_lifecycle`, 漏调 `search.clear_coordinator`
- **处理**: `coordinator: ArcSwapOption<SearchCoordinator>` → `OnceCell<Arc<SearchCoordinator>>`, 删 `clear_coordinator`, `set_coordinator` 改 `OnceCell::set`

### R2. `AppFacade` 5 个 `ArcSwapOption` daemon-lifecycle 字段 — swap_out 是 dead path

- `uc-application/src/facade/app_facade.rs:75-105,150-168` + ~20 处 `.load_full()`
- `swap_daemon_lifecycle` 全进程 1 次调用，`clear_daemon_lifecycle` 调用时进程已在退出
- **处理**: 改为 `OnceCell<Arc<XxxFacade>>` 启动期 `set` 一次，删 `clear_daemon_lifecycle`, daemon 退出 = 进程退出 = Arc drop 自动清

### R3. `DesktopRuntime::set_event_emitter` + `TauriAppRuntime::set_event_emitter` 两个公开方法死代码 (修订)

> **重要修订** (2026-05-10): A1 原报告建议把 `emitter_cell: Arc<RwLock<Arc<dyn>>>` 整体简化为 `Arc<dyn>`。实施时发现这是 **误判** —— `uc-desktop/src/daemon/app.rs:265-269` 在 daemon.run() 启动时直接 `cell.write()` swap 把 `DaemonApiEventEmitter` 装入。这条 swap **不** 走 `set_event_emitter` 方法，简化 cell 类型会让 daemon 启动后无法装入真 emitter, 上游 publisher 丢事件。
>
> 修订后 R3 只删两个确实 dead 的公开方法; cell 类型 + daemon 内部 swap 全保留。Cleanup PR #4 落地 `12b1ce3c`。

- `uc-desktop/src/runtime.rs::DesktopRuntime::set_event_emitter` 零外部 caller
- `uc-tauri/src/bootstrap/runtime.rs::TauriAppRuntime::set_event_emitter` 零外部 caller (转发给 DesktopRuntime)
- daemon 内部 swap 走 `daemon/app.rs:265 *cell.write() = ...`, 不调上述方法
- **处理**: 删两个公开方法; emitter_cell 字段类型与 daemon 内部 swap 路径保留

### R4. `tauri::Builder::manage(process_handles.clone())` 死注册

- `uc-tauri/src/run.rs:181`
- 全工程零 `State<ProcessRuntimeHandles>` 消费者
- **处理**: 删 line 181; line 269 `clone()` 改 `move`

### R5. `LanOnlyDisclosure` 用户披露面板仍展示 "OTLP" 类目

- `src/components/setting/LanOnlyDisclosure.tsx:14` + i18n `en-US.json:215-218` / `zh-CN.json:206-208`
- OTLP 后端已废弃 (commits `c8a4d5d9` / `642a0690`), 走 Sentry, 但用户面前还顶着 "OTLP telemetry" 标题
- **处理**: `DISCLOSURE_KEYS` 里 `'otlp'` 改 `'telemetry'`, i18n 标题改 "遥测 / Telemetry"

### R6. `graceful_shutdown_port_reuse.rs` 文件级注释撒谎

- `uc-webserver/tests/graceful_shutdown_port_reuse.rs:1-11`
- 仍把测试定位成 "P1 in-process daemon reload 的契约测试" —— 那条路径已删
- 测试本体仍然有价值 (`app.restart()` 也走 fork+exec, 需要端口及时让渡)
- **处理**: 注释重写为 "`app.restart()` 端口让渡契约"。否则下一个清理人可能误删测试

### R7. `restart.rs` 模块说明 9 行历史叙事可精简

- `uc-tauri/src/commands/restart.rs:4-13`
- `restart_daemon` / `reload_in_process_daemon` / `RestartDaemonError` / `ReloadInProcessDaemonError` 全工程零命中，历史叙事容易误导后人
- **处理**: 精简到 2 行

---

## 🟡 可削减清单 (机制合理，当前规模过大 / 抽象过早)

### Y1. `Clone` derive 注释要改

- `uc-bootstrap/src/assembly.rs:146-150`
- 现注释："in-process daemon 路径每次 daemon spawn 时 clone 一份"
- 应改："启动期 GUI shell 把同一份 deps fan-out 给 TauriAppRuntime / daemon spawn"
- derive 本身保留 (clone 廉价 + 启动期 5 处 clone 调用)

### Y2. `BackgroundRuntimeDeps` 拆出注释要改

- `uc-bootstrap/src/assembly.rs:141-148`
- 现注释："可以在 daemon reload 时被多次借用"
- 应改："Receiver 不可 Clone, 而 WiredDependencies 需要被 standalone binary 与 GUI shell 两种入口共用"

### Y3. `ProcessRuntimeHandles` 字段再合并一层

- `uc-desktop/src/daemon/host.rs:56-62`
- `clipboard_write_coordinator` / `file_transfer_lifecycle` 可挪进 `WiredDependencies`, 让 handle 只剩 `wired` + `storage_paths`
- 不阻塞，性质是 "还能再精简一层"

### Y4. `InMemoryMobileDeviceRepository` pub 暴露但只在自身测试用

- `uc-infra/src/mobile_sync/device_repo.rs:28` + `mobile_sync/mod.rs:23 pub use`
- 生产全部走 `DieselMobileDeviceRepository`, InMemory 只服务 12 个自身 `#[test]`
- 违反 `uc-infra/AGENTS.md` "测试用 InMemory 不应外泄 API"
- **处理**: 移到 `#[cfg(test)] mod tests` 或 `#[cfg(any(test, feature = "test-support"))]`

### Y5. `SharedEndpointInfo` type alias 无人用

- `uc-infra/src/mobile_sync/endpoint_info.rs:86`
- 调用方全部直接写裸 `Arc<…Adapter>`
- **处理**: 删 alias, 省一层认知开销
- (适配器本体 `Arc<RwLock<…>>` 仍需要 —— LAN listener 仍能因 settings PATCH 在同进程 cancel + spawn, 多 reader 仍存在)

### Y6. `RestartBanner` 复用承诺未兑现，`MobileSyncSettingsSheet` 平行重写

- `src/components/setting/RestartBanner.tsx:13-15` 注释承诺 Phase 96/97 接 `props.messageKey`
- 实际 `src/components/device/MobileSyncSettingsSheet.tsx:256-277` 自己用 shadcn `<Alert>` 写了一份 (正是 banner 想避免的反例) + 独立 i18n key + 独立 dismiss state
- **处理**: 二选一 — (a) banner 加 `messageKey` / `restartLabelKey` props, MobileSync 改调; (b) 承认 banner 只是 NetworkSection 专属，删掉那段复用承诺

### Y7. `daemon-local/health_wait.rs:5` 注释提到已删的 feature

- 注释提 `"其它 shell (不启用 sidecar-lifecycle feature)"`
- 该 feature 已在 in-process 化迁移后删除 (见 `uc-daemon-local/AGENTS.md`)
- **处理**: 改一行字

---

## 🟢 待定 (需更多 context 或低优先级)

### G1. `build_daemon_lifecycle` 单独成函数

- 物理上必须拆 (tokio runtime 上下文边界), 保留
- 但 `builders.rs` 顶部注释 "跨 daemon reload 复用" 要换成 "async/sync 边界 + standalone binary 共用"

### G2. `app_facade.rs:61-104` doc 注释整体重写

- 如果接受 R2 (改 OnceCell), 这一整段提及 "swap_daemon_lifecycle / clear_daemon_lifecycle 启停" 的注释必须改

### G3. graceful shutdown 序列在 `restart.rs` 与 `run.rs` 重复

- `uc-tauri/src/commands/restart.rs:67-92` vs `uc-tauri/src/run.rs:520-566`
- 可抽 `pub(crate) async fn graceful_daemon_shutdown(...)`
- control flow 差异让收益有限，不阻塞

### G4. `MobileSyncFacadeDeps.apply_inbound: Arc<ApplyInboundClipboardUseCase>` 裸 UseCase 共享

- `uc-application/src/facade/mobile_sync/facade.rs:125`
- 与 §11.4.4 "bootstrap 只持 Facade 不持 UseCase" 存在张力
- 需确认：UseCase 是否 stateless 让 Arc 共享语义上等价

### G5. `experimental-features.ts` 注册表只覆盖 2 key

- 当前 2 个都在 NetworkSection 内，三件套 (registry + Badge + SettingRow.experimentalKey prop) 比 inline `<ExperimentalBadge />` 多一层
- 半年内若无第二组进实验，可回归 inline

### G6. `RotatedPasswordModal` ↔ `MobileSyncCredentialModal` 部分模式重复

- 共享 "一次性凭据 + ack-to-close + secret-not-logged" 模式
- 不建议整体合并 (差异真实), 但 `useOneShotCredentialAck()` 可抽

---

## 推荐处理顺序

| 阶段 | 内容 | 收益 |
|---|---|---|
| **Cleanup PR #1** | R3 + R4 + R6 + R7 + Y4 + Y5 + Y7 (注释/死代码/cfg gate) | 低风险，一次性删干净 |
| **Cleanup PR #2** | R5 (OTLP → telemetry 改名 + i18n) | 用户可见，单独 PR 便于回滚 |
| **Refactor PR #3** | R1 + R2 + G2 (ArcSwap → OnceCell 整套回退) + Y1/Y2 注释更新 | ~110 行代码 + ~120 行注释，心智模型对齐方案 C |
| **Refactor PR #4** | R3 emitter_cell 简化 (跨 crate, 单独评审) | 影响 uc-application/bootstrap/desktop/tauri |
| **Refactor PR #5** | Y6 RestartBanner 接 messageKey + MobileSyncSettingsSheet 改调 | 前端 UI 收敛 |
| **可选** | Y3 ProcessRuntimeHandles 字段收敛 + G3 graceful_shutdown 抽函数 | 性价比一般，不阻塞 |

## 总体评价

整个 spot-capricorn 分支 (204 commits) 在"架构是否冗余"维度上 **整体健康**:

1. **新增的 mobile_sync / connection_channel / network UX / iroh LAN-only / packaging RPM 等大功能模块** 都符合 hex arch, port 抽象每个都有 ≥1 production adapter + N 个 test fake, 未发现"为未来扩展预留的空 port"
2. **OTLP 整套被废弃后，后端清理干净** (Cargo 依赖 / 模块文件全删), 唯一硬伤是前端用户披露文案 R5
3. **方案 C 留下的最大遗产是 ArcSwap**: 这是为高频 reload 准备的热切换原语，决策 C 之后没用上，应回退到 OnceCell。这是用户最自然怀疑的角度，也是事实
4. **Phase A/B/C 的其余拆分** (build_process_runtime / build_daemon_lifecycle / Clone derive / deps 共享) 的 **理由变了，但物理价值还在** —— standalone daemon binary 通过 `uniclip start` 仍是生产路径，"共用进程级装配"的论据成立

需要处理的实际代码量约 110 行删除 + ~120 行注释回写 + 1 个前端 i18n 改名。
