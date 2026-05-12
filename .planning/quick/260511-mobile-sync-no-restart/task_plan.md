# task_plan — 移动端同步首次接入流程简化（B 方案）

## 目标

让首次使用移动端添加设备的用户从「点 + Add」到「拿到凭据」全程 **零进程重启、零页面跳转**。`MobileSyncSettings` 任何字段切换都即时生效。

## 范围边界

- ✅ 改 mobile_sync 字段（enabled / lan_listen_enabled / lan_port / lan_advertise_ip）的生效路径
- ✅ 改 `MobileSyncDevicesPanel` 与 `MobileSyncSettingsSheet` 的 UX
- ❌ 不动 `NetworkSection`（iroh RelayMode 仍受 Pitfall 3 / 10 约束）
- ❌ 不动 iroh / BIND_LOCK 相关任何代码
- ❌ 不改 SyncClipboard 协议本身（4 个路由 + Basic Auth 保持原样）

## 不变量

详见 [findings.md §9](./findings.md#9-关键不变量动手时不能破的)。

## Phases

### Phase 1 — `uc-core` 新增 `MobileLanLifecyclePort` (P0, ATOMIC)

**状态**：`complete`

**目标**：在 `uc-core/src/ports/` 加 port trait + `MobileLanTarget` enum。纯领域定义，无任何 mobile_sync 实现细节。

**关键产物**：
- `uc-core/src/ports/mobile_sync_lan_lifecycle.rs` 新文件
- `uc-core/src/ports/mod.rs` 加 `pub mod` + `pub use`

**接口契约**：
```rust
#[async_trait]
pub trait MobileLanLifecyclePort: Send + Sync {
    async fn apply(&self, target: MobileLanTarget);
}

pub enum MobileLanTarget {
    Disabled,
    Enabled { port: u16 },
}
```

**doc 纪律**（uc-core/AGENTS.md §5.4）：
- 不提 mobile_sync / SyncClipboard / iPhone / axum / 42720
- 只描述「idempotent state-alignment 语义」与「Disabled/Enabled 的领域含义」

**验收**：
- `cargo check -p uc-core` 通过
- `cargo test -p uc-core --lib` 通过
- grep doc-comment 无禁词

**Commit**: `feat(uc-core): add MobileLanLifecyclePort for runtime listener state alignment`

---

### Phase 2 — `uc-desktop` 实现 `MobileLanLifecycleController` adapter (P0, ATOMIC)

**状态**：`complete`

**目标**：在 `uc-desktop/src/daemon/` 加 controller，实现 port 接口。独立于 facade，可单测。

**关键产物**：
- `uc-desktop/src/daemon/mobile_lan_lifecycle.rs` 新文件
- `uc-desktop/src/daemon/mod.rs` 加 `mod`
- 4 个单测覆盖状态转移矩阵

**状态机**（idempotent）：

| 当前 \ 目标 | Disabled | Enabled{port} |
|---|---|---|
| None | no-op | start(port) |
| Some{p}, p == port | stop | no-op |
| Some{p}, p != port | stop | stop + start(port) |

**内部状态**：
```rust
struct RunningListener {
    port: u16,
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<()>>,
}
state: tokio::sync::Mutex<Option<RunningListener>>
```

**测试**：
1. `apply_enabled_from_none_starts_listener`
2. `apply_disabled_from_some_stops_listener`
3. `apply_same_port_is_noop`
4. `apply_different_port_stops_and_rebinds`
5. `apply_bind_failure_keeps_state_none_and_sets_endpoint_error`

**验收**：
- `cargo test -p uc-desktop --lib mobile_lan_lifecycle` 通过
- controller 不持 `Arc<MobileSyncFacade>`（避免 facade ↔ controller 循环依赖）—— 改持下游 ports

**Commit**: `feat(uc-desktop): add MobileLanLifecycleController for in-process listener hot-swap`

---

### Phase 3 — `uc-application` `MobileSyncFacade::update_settings` 接入 lifecycle (P0, ATOMIC)

**状态**：`complete`

**目标**：写盘后立即调 `port.apply(target)`，`restart_required` 永远 false。

**关键产物**：
- `uc-application/src/facade/mobile_sync/facade.rs` 改 `update_settings` 方法
- `uc-application/src/facade/mobile_sync/mod.rs` 改 `MobileSyncFacadeDeps` 加 `lan_lifecycle: Arc<dyn MobileLanLifecyclePort>` 字段
- `uc-application/src/usecases/mobile_sync/update_settings.rs` 改 `UpdateMobileSyncSettingsOutput::restart_required` 注释与语义（永远 false，保留字段以兼容 wire 类型）

**facade 改动伪码**：
```rust
let out = self.update_settings_uc.execute(input).await?;
let target = match (out.enabled, out.lan_listen_enabled) {
    (true, true) => MobileLanTarget::Enabled { port: out.lan_port.unwrap_or(42720) },
    _ => MobileLanTarget::Disabled,
};
self.lan_lifecycle.apply(target).await;
Ok(UpdateMobileSyncSettingsOutput { restart_required: false, ..out })
```

**测试**：
- facade test 用 in-memory port mock，验证：
  1. enable 写盘 → port.apply(Enabled) 被调一次
  2. disable 写盘 → port.apply(Disabled) 被调一次
  3. port 改变 → port.apply(Enabled{new_port}) 被调一次
  4. 同值写盘 → 写盘跳过（use case 内已实现）+ port.apply 仍然调用（保持 idempotent）
  5. restart_required 在所有 case 中都是 false

**验收**：
- `cargo test -p uc-application --lib mobile_sync` 通过
- 现有 mobile_sync facade 测试更新到通过

**Commit**: `feat(uc-application): wire MobileSyncFacade to lan-lifecycle port + restart_required always false`

---

### Phase 4 — daemon `app.rs` 改用 controller (P0, ATOMIC)

**状态**：`complete`

**目标**：删 `app.rs:307-396` 的一次性 spawn，daemon `run()` 启动时调 `controller.apply(initial_target)`，shutdown 时调 `controller.apply(Disabled)`。装配链补 controller。

**关键产物**：
- `uc-desktop/src/daemon/app.rs` 改 `run()` 方法
- `uc-desktop/src/daemon/bootstrap.rs` 或 `app_facade_assembly.rs` 装配 controller，注入到 facade deps
- `uc-bootstrap/...` 如有 `WireOverrides` 链路涉及 mobile_sync 也要补

**装配顺序**（避免循环）：
1. 构造 `endpoint_info: Arc<InMemoryMobileSyncEndpointInfoAdapter>`
2. 构造 `MobileLanLifecycleController` 持 `endpoint_info` + `file_transfer_facade`
3. 构造 `MobileSyncFacade` 持 `Arc<dyn MobileLanLifecyclePort>`（即上一步的 controller）
4. 把 `mobile_sync_facade` swap 进 `AppFacade.mobile_sync` (ArcSwapOption)
5. **回填**：把 `mobile_sync_facade` 注入 controller —— 通过 `OnceCell` 或在 controller `apply` 时 lazy 读 `AppFacade.mobile_sync.load_full()`，避免装配期循环

**daemon shutdown 兜底**：
```rust
// 在 run() 退出 select! loop 之后:
if let Some(controller) = self.mobile_lan_controller.clone() {
    controller.apply(MobileLanTarget::Disabled).await;
}
```

**集成测试**：
- 新文件 `uc-webserver/tests/mobile_lan_lifecycle.rs`（参考 `graceful_shutdown_port_reuse.rs` 范式）
- 用例：
  1. daemon 起来 + lan_listen=false → no listener
  2. update_settings(enabled=true, lan_listen=true) → listener 起来，可 GET /SyncClipboard.json 拿 401
  3. update_settings(lan_listen=false) → listener 停，bind 端口立刻可被重用
  4. update_settings(lan_port=新端口) → 旧端口释放，新端口监听

**验收**：
- `cargo test -p uc-desktop --lib` 全通
- `cargo test -p uc-webserver --test mobile_lan_lifecycle` 全通
- `cargo test -p uc-webserver --test graceful_shutdown_port_reuse` 保持通过（回归）

**Commit**: `feat(uc-desktop): swap daemon's one-shot LAN listener spawn for hot-swap controller`

---

### Phase 5 — 清理 SPEC §1.2.5 注释 + restart_required 字段语义文档 (P1, ATOMIC)

**状态**：`complete`

**目标**：删除 5 个 Rust 文件中"SPEC §1.2.5: 用户必须 stop+start daemon"的过时注释，替换为「即时生效」说明。

**关键产物**：
- `uc-application/src/usecases/mobile_sync/get_settings.rs` 注释更新
- `uc-application/src/usecases/mobile_sync/update_settings.rs` 注释更新 + restart_required 字段 doc 改为"保留兼容前端字段，永远 false"
- `uc-webserver/src/mobile_lan/routes.rs` 注释更新
- `uc-core/src/settings/model.rs` 注释更新
- `uc-desktop/src/daemon/app.rs`（如还有残留）

**前端类型同步**：
- `src/api/tauri-command/mobile_sync.ts` `UpdateMobileSyncSettingsResult.restartRequired` 字段 doc 更新

**验收**：
- `grep -r "SPEC §1.2.5"` 全仓零命中
- `grep -r "stop+start daemon"` 全仓零命中
- `cargo check --workspace` 通过

**Commit**: `docs(mobile_sync): drop SPEC §1.2.5 "must restart daemon" note — superseded by in-process hot-swap`

---

### Phase 6 — 前端 UX 重做 (P0, ATOMIC)

**状态**：`complete`

**目标**：「+ Add」始终启用 → 未配置则一键开启 → 直接进 register 表单。删 restart banner。

**关键产物**：

**`src/components/device/MobileSyncDevicesPanel.tsx`**：
- 「+ Add」按钮 `disabled` 条件改为：只在 `lanListenerError != null` 时 disable（仍保留 bind 失败的硬阻断）
- 点击 handler：
  ```ts
  if (!enabled || !lanListenEnabled) {
    // 走引导对话框
    setEnableConfirmOpen(true)
  } else {
    setAddDialogOpen(true)
  }
  ```
- 新增 `<EnableMobileSyncDialog>` 引导子组件，确认后调 `updateMobileSyncSettings({ enabled: true, lanListenEnabled: true })`，成功后 **自动** 打开 `AddMobileSyncDeviceDialog`

**`src/components/device/MobileSyncSettingsSheet.tsx`**：
- 删除 `restartRequired` state / `restartDismissed` state
- 删除 amber RestartBanner JSX 块
- 删除 `handleRestart` 调用
- 字段切换成功后用 `toast.success(...)` 即时反馈（i18n key `devices.mobileSync.feedback.applied`）
- 保留 `pendingLanEnable` 安全告警 AlertDialog（关→开 lanListen 时的二次确认，UX 上仍需要）

**`src/components/device/EnableMobileSyncDialog.tsx`** 新建子组件：
- 单一确认 dialog，明确告诉用户「开启后会在端口 42720 监听 LAN HTTP，仅受信网络下使用」
- 「开启并继续」按钮 → updateMobileSyncSettings → 立即 setAddDialogOpen(true)

**i18n**：
- 删除 `devices.mobileSync.lanListener.restartRequired.*` 相关 keys
- 新增 `devices.mobileSync.enableConfirm.{title,body,confirm,cancel}`
- 新增 `devices.mobileSync.feedback.applied`

**测试**：
- 改 `MobileSyncDevicesPanel.test.tsx`：
  - 新用例：未配置 + 点 Add → 弹引导 → 确认 → updateMobileSyncSettings 被调 + AddDialog 自动打开
  - 删除"Add disabled 时显示提示"用例（已改为 always enabled）
- 改 `MobileSyncSettingsSheet.test.tsx`：
  - 删除 restart banner / restartRequired 相关测试
  - 新增"toggle enabled → applied toast"测试

**验收**：
- `pnpm exec vitest run src/components/device/` 全通
- `pnpm typecheck` 通过
- 手动测试：开发模式下首次添加移动设备，全程不弹 restart banner、不需要重启 App

**Commit**: `refactor(ui): rebuild mobile-sync first-time flow around one-tap onboarding (no restart)`

---

## 提交策略

按 atomic commits 顺序提交，每个 commit 独立通过 CI。Phase 1 → 6 顺序不可乱：
- Phase 1 (port trait) 是 Phase 2/3 的依赖
- Phase 2 (adapter) 是 Phase 4 的依赖
- Phase 3 (facade) 是 Phase 6 (前端) 的依赖（前端依赖 restart_required 永远 false 的契约）
- Phase 4 (daemon) 让运行时 hot-swap 真正生效
- Phase 5 (注释清理) 跟在 Phase 4 后，避免短时间内出现"代码已支持，注释还说不支持"
- Phase 6 (前端) 是用户可见效果

中间 commit 之间用户感受到的行为：
- Phase 1-3 提交后：前端仍跑老逻辑（仍弹 restart banner），但后端已经 hot-swap。**用户行为不变**。
- Phase 4 提交后：daemon 真的 hot-swap listener，但前端仍弹 banner（实际是误报，因为已不需要 restart）。**这是中间态**。
- Phase 5 提交后：注释已清理，但前端 UI 仍展示 restart banner（视觉残留）。
- Phase 6 提交后：前端切到新流程，**用户看到完整简化**。

> 这条 commit 链中所有中间态都不破坏功能（restart_required 误报最多让用户多点一次重启，行为退化到旧路径），全 CI 友好。

## 错误日志

| Phase | Error | Attempt | Resolution |
|---|---|---|---|

## 决策记录

| 日期 | 决策 | 理由 |
|---|---|---|
| 2026-05-11 | 选 B 方案而非 A | A 仍需重启，对首次添加用户反直觉。B 工作量大但 UX 收益高且无技术阻碍（见 findings §2） |
| 2026-05-11 | port 放 uc-core，adapter 放 uc-desktop | 守 hexagonal 边界（uc-core/AGENTS.md §5）+ controller 涉及 tokio/cancel token 不能进 uc-core |
| 2026-05-11 | controller 持 endpoint_info + file_transfer 而非 facade | 避免 facade ↔ controller 循环依赖 |
| 2026-05-11 | UpdateMobileSyncSettingsOutput.restart_required 字段保留但永远 false | 不破坏前端 wire 契约 + 文档明示"为未来字段预留" |
| 2026-05-11 | NetworkSection 保持不变 | iroh BIND_LOCK 是真约束，Pitfall 3 / 10 仍有效 |
