# Progress Log

## 2026-05-10 启动

- 确认 review 范围：整分支 vs main (用户选项 #3)
- 总 diff: 971 files / +63863/-78089 行
- 已剔除非产品代码 (`.claude/` / `.gsd/` / `.planning/` / `docs/`) 后，
  产品代码改动 ~50K 行
- 按 line count + 模块边界切成 4 个并行 review 主题 (A1-A4)
- Planning 文件落地

## 2026-05-10 Phase 2 (并行 sub-agent review) — ✅ complete

| Agent | 范围 | 状态 |
|---|---|---|
| A1 | uc-application + uc-core | ✅ |
| A2 | uc-infra + uc-webserver + uc-daemon-local + uc-platform | ✅ |
| A3 | uc-bootstrap + uc-desktop + uc-cli + uc-tauri | ✅ |
| A4 | frontend src/ + uc-observability | ✅ |

四份子报告落盘 `findings-A{1-4}-*.md`。

## 2026-05-10 Phase 3 (汇总) — ✅ complete

`findings.md` 汇总完成。最终结论：

- **7 项 R 必删 / 必修** (R1-R7): ArcSwap 死路径 + 死注册 + 注释撒谎 + OTLP 用户文案残留
- **7 项 Y 可削减** (Y1-Y7): 注释更新 / cfg gate / 字段收敛 / UI 复用承诺
- **6 项 G 待定** (G1-G6): doc 重写 / 抽函数 / 模式去重
- **推荐处理顺序**: 5 个 cleanup/refactor PR 分批落地

总改动量预估：~110 行代码删除 + ~120 行注释回写 + 1 处前端 i18n 改名

## 2026-05-10 Cleanup PR #1 落地

按推荐顺序的 #1, 6 项一次性清理：

| ID | Commit | 说明 |
|---|---|---|
| R4 | `da2eeba7` | 删 `.manage(process_handles.clone())` 死注册 + clone→move |
| R6 | `4eb4f5bd` | 重写 `graceful_shutdown_port_reuse` 文件头反映方案 C |
| R7 | `6a3942df` | 精简 `restart.rs` 9 行历史叙事 |
| Y7 | `c9773e1e` | 修 `health_wait` 提及已删 sidecar-lifecycle feature 的 stale 注释 |
| Y5 | `a8f83241` | 删 `SharedEndpointInfo` type alias |
| Y4 | `3dec30ab` | `InMemoryMobileDeviceRepository` mod + re-export 加 `#[cfg(test)]` |

验证：

- `cargo check --workspace` 干净
- `cargo test -p uc-infra -p uc-application -p uc-tauri -p uc-webserver -p uc-daemon-local --lib` 全过 (uc-application 413 / uc-infra 272 / uc-tauri 17 / uc-daemon-local 17 / uc-webserver 45)
- `cargo test -p uc-webserver --test graceful_shutdown_port_reuse` 1/1 passed

## 2026-05-10 Cleanup PR #2 落地

按推荐顺序的 #2 (R5 单项，用户面前的硬伤):

| ID | Commit | 说明 |
|---|---|---|
| R5 | `eb25b3c5` | LanOnly disclosure 类目 OTLP → telemetry (TSX + 双语 i18n + 两处测试断言) |

验证：`pnpm exec vitest run` 410/410 passed.

## 2026-05-10 Cleanup PR #3 落地

本次 review 的核心收获 —— ArcSwap 热切换原语整体回退到 OnceLock startup-once 语义：

| ID | Commit | 说明 |
|---|---|---|
| R1 | `64f00a10` | SearchFacade.coordinator ArcSwap → OnceLock, 删 clear_coordinator |
| R2+G2+Y1+Y2 | `67535ff2` | AppFacade 5 字段 ArcSwap → OnceLock, swap → install, 删 clear_daemon_lifecycle, 6 外部 caller + 18 内部 .load_full() 改 .get().cloned(), 删 arc-swap 依赖，大段 doc 重写 |

验证：

- `cargo check --workspace` 干净
- `cargo test -p uc-application -p uc-infra -p uc-tauri -p uc-webserver -p uc-daemon-local -p uc-desktop -p uc-bootstrap --lib` 824/824 passed
- `cargo test -p uc-webserver --test graceful_shutdown_port_reuse` 1/1 passed
- `cargo test -p uc-application --tests` 全过

收益：删除 1 个 Rust 依赖 (arc-swap), 心智模型从 "运行时多次 swap" 收敛到
"启动期一次装入", 配合方案 C 后 daemon 进程级单例的现实。

## 2026-05-10 Cleanup PR #4 落地 (R3, 修订版)

| ID | Commit | 说明 |
|---|---|---|
| R3 | `12b1ce3c` | 删 DesktopRuntime/TauriAppRuntime 的 set_event_emitter 两个公开方法 (2 文件，+4/-23) |

### Sub-agent A1 R3 误判的复盘

A1 原 R3 建议把 `emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` 整体简化为 `Arc<dyn HostEventEmitterPort>`。基于 "全仓 grep `set_event_emitter` 无外部 caller, emitter_cell 实际是只读单值"。

实施过程中发现这是误判：

- `uc-desktop/src/daemon/app.rs:265-269` 在 daemon.run() 启动时直接 `*self.event_emitter_cell.write() = Arc::new(DaemonApiEventEmitter::new(self.event_tx.clone()))` —— 这是 daemon 把上游 publisher (file_transfer / space_setup outbound / blob_transfer progress) 接到自己事件总线的关键 swap, **不** 走 `set_event_emitter` 方法
- A1 只 grep 方法名，漏看了直接对 cell 的 `.write()`
- 简化为 `Arc<dyn>` 会让 daemon 启动后无法装入真 emitter, 上游 publisher 永远看到 LoggingHostEventEmitter → 前端丢失 file transfer / blob progress 事件 = **回归 bug**

修订后只删确实 dead 的两个公开方法 (DesktopRuntime / TauriAppRuntime 各一个),保留 cell 类型与 daemon 内部 swap 路径。

验证：`cargo test --workspace --lib` (uc-application 413 / uc-tauri 17 / uc-desktop 48 / uc-bootstrap 12 等全过).

教训记录在 findings.md 末尾，提醒未来 review:"grep 方法名 caller" 不能代替"grep 字段的 mutation 操作 (`.write()` / `.swap()` / `=`)"。

## 2026-05-10 Cleanup PR #5 落地 (Y6, 走方案 b)

| ID | Commit | 说明 |
|---|---|---|
| Y6 | `8589cd91` | 删 RestartBanner 复用承诺注释，承认 NetworkSection 专属 (1 文件，+10/-8) |

### 方案对比

A4 给的二选一：(a) banner 接 messageKey props + MobileSync 改调，(b) 删复用承诺。

走 (b) 的理由 (实施前对照): 两个 banner **职责/视觉/错误处理都不同**:

| 维度 | RestartBanner | MobileSync 重写 |
|---|---|---|
| 状态 | loading + error + retry + dismiss-on-error | visible + dismiss |
| 视觉 | 中性灰 + RefreshCw 图标 | 琥珀色警告 |
| 错误 | banner inline error | toast.error, banner 不管 |
| dismiss 语义 | 只 dismiss error | dismiss 整个 banner |

强行抽象 generic banner 会拼出 union 类型，维护成本比保留两份更高。注释
更新明确"NetworkSection 专属", 后续"重启提示"需求各自走 inline UI.

验证：`pnpm exec vitest run RestartBanner.test.tsx` 8/8 passed.

## ✅ Review 任务完成

| PR | Commits | 范围 |
|---|---|---|
| #1 | 7 个 (R4/R6/R7/Y4/Y5/Y7 + planning) | 注释 / 死代码 / cfg gate |
| #2 | 2 个 (R5 + planning) | OTLP → telemetry 用户文案 |
| #3 | 3 个 (R1/R2/G2/Y1/Y2 + planning) | ArcSwap → OnceLock 整套回退 |
| #4 | 2 个 (R3 修订 + planning) | 删 set_event_emitter dead API |
| #5 | 1 个 (Y6) | 删 banner 复用承诺 |

总 15 个 atomic commits, 心智模型完整对齐方案 C "daemon 进程级单例" 现实。

剩余 待定项 (G3-G6): 性价比一般，不阻塞，视未来需求触发。

## 错误记录

| 错误 | 第几次尝试 | 解决 |
|---|---|---|
| (无) | — | — |
