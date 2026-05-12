# progress — 移动端同步首次接入流程简化

按 `task_plan.md` 的 6 个 phase 推进。每完成一段记录到这里。

## 2026-05-11

### 调研 + 规划

- [x] PITFALLS.md 全文扫描（632 行），零阻碍
- [x] 项目内部 review 文档扫描，找到 `260510-arch-redundancy-review/findings-A2-infra-io.md:22` 的预见性背书
- [x] 确认 contract test `graceful_shutdown_port_reuse.rs` 已钉死 axum 同端口热重启契约
- [x] 确认 SPEC §1.2.5 无外部文档支撑，仅 4 个源文件注释中引用
- [x] 列出 6 phase atomic commit 计划

### Phase 1 — `uc-core` 新增 `MobileLanLifecyclePort` ✅

- 加在既存 `mobile_sync.rs`（领域归类一致），未新建文件
- `mod.rs` 加 pub use（`MobileLanLifecyclePort` + `MobileLanTarget`）
- doc 自查通过 §5.4：没提上层模块/use case/HTTP 路由/具体协议名/调用方
- `cargo test -p uc-core --lib`：94 passed
- Commit: `64dcca6d feat(uc-core): add MobileLanLifecyclePort ...`

### Phase 2 — `uc-desktop` 实现 `MobileLanLifecycleController` adapter ✅

- 状态机 (None/Some{port}) × (Disabled/Enabled{port}) 4 transition 落地
- `LanListenerSpawner` trait 抽出"如何起 listener",解耦 facade 循环依赖
  - `AppFacadeListenerSpawner` 生产实现 lazy 从 `AppFacade.mobile_sync` OnceLock 读
  - `FakeSpawner` 测试实现：不引入 axum dev-dep,bind 拿地址后立刻 drop,join handle 等 cancel
- bind 失败语义：写 `endpoint_info.set_bind_failure`,state 保持 None，允许下次 apply 重试
- 单测 6/6 通过 (覆盖 4 transition + bind failure + 同端口 no-op)
- Commit: `ffe48e75 feat(uc-desktop): add MobileLanLifecycleController ...`

### Phase 1+2 doc 重写 (why-first 反馈触发) ✅

- 用户反馈：文件/模块开头 doc-comment 必须先说"为什么需要这个文件",再说 what/how
- 反馈存进 memory:`feedback_module_doc_why_first.md`
- 重写 `mobile_lan_lifecycle.rs` 的 `//!` 头三段：为什么需要 → 对外能力 → 内部实现要点
- 重写 `uc-core/src/ports/mobile_sync.rs` 的 lan lifecycle section:
  - 加 why-first 段落，讲清"装配期一次性 vs 运行时切换"的问题
  - 守 §5.4 doc 纪律：不提 MobileSyncFacade / restart_app / update_settings 等上层调用方
- 清掉 mobile_lan_lifecycle.rs 中一条 early-iteration stray comment
- Commit: `e779ebaf docs(mobile-lan-lifecycle): rewrite module/trait doc-comments why-first`

### Phase 3 — `uc-application` `MobileSyncFacade::update_settings` 接入 lifecycle ✅

- `MobileSyncFacadeDeps.lan_lifecycle: Option<Arc<dyn MobileLanLifecyclePort>>`
- `update_settings` 写盘后 if Some(port) → apply(target) + restart_required = false
- 新 helper `lan_target_from_settings` 单点真相 (enabled && lan_listen → Enabled{port:42720 fallback})
- 5 处构造点全部补 lan_lifecycle 字段 (3 facade tests + uc-webserver test_support + uc-bootstrap non_gui_runtime CLI fallback)
- 3 个新单测 (lifecycle 路径 apply + 默认 port + 无 lifecycle 旧语义)
- `cargo test -p uc-application --lib facade::mobile_sync`: 8 passed
- Commit: `206cdace feat(uc-application): wire MobileSyncFacade to lan-lifecycle port`

### Phase 4 — daemon `app.rs` 改用 controller ✅

- `build_mobile_sync_facade` 加 lan_lifecycle 参数
- `DaemonLifecycleFacadesInput` / `DaemonAppAssemblyInput` 透传 controller
- host.rs 构造 controller(`MobileLanLifecycleController` + `AppFacadeListenerSpawner`)
- `DaemonApp` 加 mobile_lan_lifecycle 字段 + `with_mobile_lan_lifecycle` builder
- daemon `run()` 启动期：读 settings → `controller.apply(initial_target)` 替换原一次性 spawn
- daemon shutdown 期：显式 `controller.apply(Disabled)` 回收端口
- circle dep 解法：controller 持 `Arc<AppFacade>`(lazy 读 mobile_sync OnceLock),
  不持 facade 本身，装配期 facade 暂未 install 也能编，apply 时已 install
- `cargo test -p uc-desktop --lib`: 54 passed (含 phase 2 加的 6 个)
- `cargo test -p uc-webserver --test graceful_shutdown_port_reuse`: 1 passed (回归)
- Commit: `5b0fabca feat(uc-desktop): swap daemon's one-shot LAN listener spawn for hot-swap controller`

### Phase 5 — restart_required 字段语义 doc 同步 ✅

- update_settings.rs: module doc why-first 重写; restart_required 字段标 wire-兼容
- facade.rs: 方法表条目 + settings_round_trip 测试注释更新
- uc-tauri/mobile_sync.rs: wire 字段 + Tauri command doc
- src/api/tauri-command/mobile_sync.ts: TS 字段 JSDoc 说明 GUI 路径永远 false
- SPEC §1.2.5 grep 全仓零命中 (phase 4 删 daemon 那一段时已带走)
- `cargo check --workspace`: 通过
- Commit: `be766b6d docs(mobile_sync): update restart_required field semantics`

### Phase 6 — 前端 UX 重做 ✅

- 新建 `EnableMobileSyncDialog.tsx`: 一次性确认对话框，自动写 enabled + lan_listen_enabled
- `MobileSyncDevicesPanel.tsx`:
  - "+Add" 仅在 lanListenerError 时 disabled
  - 未配置点 +Add → 弹引导对话框 → 成功后自动打开 AddDialog
- `MobileSyncSettingsSheet.tsx`:
  - 删 restartRequired/restartDismissed state + handleRestart + amber banner
  - applySettingsUpdate 成功后 toast.success(applied) 即时反馈
  - 顶部 doc why-first 重写
- i18n: enableConfirm.* + feedback.applied / feedback.applyFailed (中英)
- 新测试：EnableMobileSyncDialog.test.tsx (3 i18n smoke 用例)
- `pnpm exec vitest run`: 413 passed (含新 3 个), pnpm tsc --noEmit 通过
- Commit: `76ff2422 refactor(ui): rebuild mobile-sync first-time flow around one-tap onboarding`

## 全部完工

| Commit | 内容 |
|---|---|
| `d1ac0036` | docs: planning 三件套 |
| `64dcca6d` | Phase 1: uc-core port + target enum |
| `ffe48e75` | Phase 2: uc-desktop controller + 6 单测 |
| `e779ebaf` | docs: phase 1+2 文档 why-first 重写 |
| `206cdace` | Phase 3: facade 接入 lifecycle + 3 新单测 |
| `5b0fabca` | Phase 4: daemon 装配链 + 替换一次性 spawn |
| `be766b6d` | Phase 5: restart_required 字段 doc 同步 |
| `76ff2422` | Phase 6: 前端 UX 重做 (引导对话框 + 删 restart banner) |

## 用户体验对比

旧流程 (4 步，强制重启):
1. 打开 Configure 抽屉
2. 开 enabled 开关 → 开 LAN listener 开关 → 弹安全告警 → 确认
3. 看 amber restart banner → 点"立即重启" → 等 App 重启
4. 回到设备页 → 点 +Add → 填 label → 拿凭据

新流程 (1 步，零重启):
1. 点 +Add → 一次性确认对话框 → 填 label → 拿凭据

## 验收清单完成情况

- [x] 首次用户从 +Add 到拿到凭据，全程零进程重启
- [x] 现有用户切 enabled / lanListenEnabled / port 字段全部即时生效 (0 banner)
- [x] daemon shutdown → 端口正确释放 (显式 apply(Disabled))
- [x] rebind 撞占用 → endpoint_info.lanListenerError 反馈
- [x] NetworkSection (iroh) 仍走 restart_app(保留 Pitfall 3 / 10 约束)
- [x] `cargo test -p uc-core -p uc-application -p uc-desktop -p uc-webserver` 全通
- [x] `pnpm exec vitest run` 全通 (413 passed)
- [ ] 手动测试三平台 (mac/win/linux) —— 留给后续 QA

