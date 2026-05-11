# findings — 移动端同步首次接入流程简化（B 方案：去掉重启）

调研于 2026-05-11，目的是确认"settings PATCH 后让 mobile_sync LAN listener 同进程热重启"在技术与项目约束上完全可行。

## 0. 用户原始诉求

> 「简化移动端配置。首次使用移动端添加设备的用户，需要先开启配置，然后重启，然后才能设备，输入账号密码。请你简化整个流程。」

现有流程 4 步：开 `Configure` 抽屉 → 开 enabled + lan_listen → 弹安全告警 → 重启 → 重新点 `+ Add` → 填 label。

目标：1 步。点 `+ Add` → 一键开启 + 立即弹填 label 表单 → 拿凭据。

## 1. 当前架构现状（与 B 方案相关）

### 1.1 LAN listener 装配（uc-desktop daemon）

`src-tauri/crates/uc-desktop/src/daemon/app.rs:307-396`：

- daemon `run()` 启动时同步读一次 `mobile_sync_facade.get_settings()`
- 根据 `enabled && lan_listen_enabled` 决定是否 `tokio::spawn` listener
- listener 用 `cancel.child_token()` 接 daemon 主 cancel —— daemon shutdown 时一并退
- 配置变更 **不热重载** —— 注释明写"SPEC §1.2.5: 用户必须 stop+start daemon"

```rust
let lan_cancel = self.cancel.child_token();
tokio::spawn(async move {
    let port = view.lan_port.unwrap_or(42720);
    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    match start_mobile_lan_server(bind, lan_cancel, mobile_sync_facade, lan_file_transfer).await {
        Ok(handle) => { endpoint_info.set(...).await; handle.join_handle.await; ... }
        Err(e) => { endpoint_info.set_bind_failure(...).await; ... }
    }
});
```

### 1.2 LAN listener 本身（uc-webserver）

`src-tauri/crates/uc-webserver/src/mobile_lan/server.rs:30-79`：

```rust
pub struct MobileLanServerHandle {
    pub bound_addr: SocketAddr,
    pub join_handle: JoinHandle<anyhow::Result<()>>,
}

pub async fn start_mobile_lan_server(
    bind: SocketAddr,
    cancel: CancellationToken,
    facade: Arc<MobileSyncFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
) -> anyhow::Result<MobileLanServerHandle> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    ...
    axum::serve(listener, router).with_graceful_shutdown(cancel.cancelled_owned())
}
```

**关键观察**：listener 已经具备热启停所需的所有原语 —— cancel 接 graceful shutdown，return handle 独立可管。

### 1.3 update_settings use case 现状

`src-tauri/crates/uc-application/src/usecases/mobile_sync/update_settings.rs:88-148`：

- 只写 settings store，**不通知 daemon**
- 返回 `UpdateMobileSyncSettingsOutput { restart_required: bool, ... }`
- `restart_required` 当任一字段实际变化即为 true

### 1.4 当前重启路径

`src-tauri/crates/uc-tauri/src/commands/restart.rs:45-95`：

- `restart_app` Tauri command → `app.restart()` → spawn 新进程 + exit 当前进程
- 走 `app.restart()` 的原因：方案 C（2026-05-11，commit `0f4fa652`）取消了 in-process daemon reload
- 取消 in-process reload 是因为 **iroh `BIND_LOCK` 进程级单次约束**（Pitfall 3），不是 mobile_sync 自己的限制

### 1.5 endpoint_info adapter（已经为热重启预留）

`src-tauri/crates/uc-infra/src/mobile_sync/endpoint_info.rs`（grep 出的设计意图）：

- `Arc<RwLock<...>>` 单写多读
- `set(url) / clear() / set_bind_failure(reason)` 三态 API
- 项目内 review（见 §2.2）明确写："即便 daemon 不再 in-process reload, LAN listener 仍能在同进程因 settings PATCH 重启"

## 2. 调研覆盖（兜底证据）

### 2.1 PITFALLS.md 全文（632 行）

`.planning/research/PITFALLS.md`：

| 维度 | 结果 |
|---|---|
| 提及 mobile_sync / SyncClipboard / mobile lan | **零** |
| 提及 axum / TCP listener / 42720 端口 | **零** |
| "settings 变更需重启"的非 iroh 条款 | **零** |
| 反对"同进程热重载 HTTP 服务"的条款 | **零** |

PITFALLS 全部围绕 iroh LAN-only Mode（relay fallback）。

**Pitfall 3** 的 `BIND_LOCK` 只约束 `IrohNodeBuilder::bind`（QUIC over UDP），与 axum HTTP 无关。

**Pitfall 10** 的"持久化重启通知"是针对 iroh `RelayMode` bind-time 约束的，不适用于 mobile_sync。但当前 UI 共用了 `RestartBanner` 视觉模式，B 方案前端要拆这两条路径（NetworkSection 保留 banner，MobileSyncSettingsSheet 改即时生效）。

### 2.2 项目内部 review 背书

`.planning/quick/260510-arch-redundancy-review/findings-A2-infra-io.md:22`：

> "适配器本体（`Arc<RwLock<…>>` 单写多读）**不是冗余** —— 即便 daemon 不再 in-process reload，**LAN listener 仍能在同进程因 settings PATCH 重启**（`uc-desktop/src/daemon/app.rs:351` 起的 `tokio::spawn` 可被 cancel 再 spawn），多 reader 仍需要 Arc 共享状态。"

写于 2026-05-10，正是 `InMemoryMobileSyncEndpointInfoAdapter` 为 B 方案预留的官方注脚。

### 2.3 contract test 已经钉死 axum 同端口重 bind

`src-tauri/crates/uc-webserver/tests/graceful_shutdown_port_reuse.rs:48-78`：

```rust
// 同进程 cancel → axum::serve drop listener → 立刻 rebind 同端口
cancel1.cancel();
join1.await;  // server 退出
let (rebound, _) = spawn_server(bound, cancel2).await;  // 同地址 bind 立即成功
assert_eq!(rebound, bound);
```

contract test 在 #612 之前为 in-process reload 引入，方案 C 后保留为 `app.restart()` 端口让渡契约。**B 方案的"stop listener → start 新 listener"完全适用同一条契约**，且不跨进程，更友好。

### 2.4 SPEC §1.2.5 没有外部文档支撑

`grep -rln "SPEC §1.2.5"` 全仓只命中 4 个 Rust 源码文件的注释：

- `uc-application/src/usecases/mobile_sync/get_settings.rs`
- `uc-application/src/usecases/mobile_sync/update_settings.rs`
- `uc-webserver/src/mobile_lan/routes.rs`
- `uc-core/src/settings/model.rs`
- `uc-desktop/src/daemon/app.rs`

**没有任何 markdown / ADR / SPEC 文档定义 §1.2.5**。"mobile sync 必须 stop+start daemon" 是早期源代码注释的偷懒决策。可以与 B 方案一同清理。

### 2.5 iroh BIND_LOCK 边界

`src-tauri/crates/uc-infra/src/network/iroh/node.rs:452-489`：

```rust
static BIND_LOCK: OnceLock<()> = OnceLock::new();
// ...
BIND_LOCK.set(()).expect("...bind called more than once...");
```

仅约束 iroh QUIC endpoint。`uc-webserver/src/mobile_lan` 与 `uc-application/src/usecases/mobile_sync` 完全不引用 iroh，零交集。

### 2.6 MobileSyncFacade 已经走 ArcSwap（#612）

`uc-application/src/facade/app_facade.rs`：

- `AppFacade.mobile_sync` 字段已改为 `Arc<arc_swap::ArcSwapOption<MobileSyncFacade>>`
- 运行时可见性 OK，listener 与 GUI command 共用同一份 facade

## 3. 字段级影响分析（哪些不需要重启）

`MobileSyncSettings` 4 个可变字段：

| 字段 | 当前行为 | B 方案后 |
|---|---|---|
| `enabled` | off→on 走 restart_app | 即时 `controller.apply(target_on)` 启动 listener |
| `lan_listen_enabled` | 同上 | 同上 |
| `lan_port` | rebind 走 restart_app | 即时 stop + 新 port bind（撞占用走 set_bind_failure 反馈） |
| `lan_advertise_ip` | daemon 永远 bind `0.0.0.0`，**当前其实就不影响 listener bind**，只影响 `register_device` 拼 base_url，但 UI 仍弹 restart 提示是误报 | 即时生效（本来就不需 restart） |

**结论：4 个字段全部可热切**，`UpdateMobileSyncSettingsOutput::restart_required` 永远为 false。

## 4. B 方案技术路线（细节）

### 4.1 新增 port（uc-core）

```rust
// uc-core/src/ports/mobile_sync_lan_lifecycle.rs
#[async_trait]
pub trait MobileLanLifecyclePort: Send + Sync {
    /// 把 listener 状态对齐到 target。idempotent。
    /// - `Disabled` → 若有 listener，stop。
    /// - `Enabled { port }` → 若没 listener，bind；若 port 变，stop+rebind；若 port 同，no-op。
    async fn apply(&self, target: MobileLanTarget);
}

pub enum MobileLanTarget {
    Disabled,
    Enabled { port: u16 },
}
```

放在 `uc-core/src/ports/`。doc-comment 只描述 idempotent 语义，不提具体 use case（守住 uc-core/AGENTS.md §5.4 port 文档纪律）。

### 4.2 新增 adapter（uc-desktop）

`uc-desktop/src/daemon/mobile_lan_lifecycle.rs`（新文件）：

```rust
pub struct MobileLanLifecycleController {
    endpoint_info: Arc<InMemoryMobileSyncEndpointInfoAdapter>,
    mobile_sync_facade: Arc<MobileSyncFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
    state: Mutex<Option<RunningListener>>,
}

struct RunningListener {
    port: u16,
    cancel: CancellationToken,
    join: JoinHandle<...>,
}

#[async_trait]
impl MobileLanLifecyclePort for MobileLanLifecycleController {
    async fn apply(&self, target: MobileLanTarget) {
        let mut guard = self.state.lock().await;
        match (guard.as_ref().map(|r| r.port), target) {
            (None, Disabled) => return, // no-op
            (Some(_), Disabled) => self.stop(&mut guard).await,
            (None, Enabled { port }) => self.start(&mut guard, port).await,
            (Some(p), Enabled { port }) if p == port => return, // no-op
            (Some(_), Enabled { port }) => {
                self.stop(&mut guard).await;
                self.start(&mut guard, port).await;
            }
        }
    }
}
```

stop / start 内部复用 `start_mobile_lan_server`，写 endpoint_info。

### 4.3 facade 改动（uc-application）

`MobileSyncFacade` 持有 `Arc<dyn MobileLanLifecyclePort>`，`update_settings` 写盘后调 `port.apply(target)`：

```rust
// uc-application/src/facade/mobile_sync/facade.rs
pub async fn update_settings(&self, input: UpdateMobileSyncSettingsInput)
    -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError>
{
    let out = self.update_settings_uc.execute(input).await?;
    let target = if out.enabled && out.lan_listen_enabled {
        MobileLanTarget::Enabled { port: out.lan_port.unwrap_or(42720) }
    } else {
        MobileLanTarget::Disabled
    };
    self.lan_lifecycle.apply(target).await;
    Ok(UpdateMobileSyncSettingsOutput { restart_required: false, ..out })
}
```

`restart_required` 永远 false。

### 4.4 daemon 装配改动（uc-desktop）

`uc-desktop/src/daemon/app.rs:307-396`：

- 删除一次性 `tokio::spawn(start_mobile_lan_server(...))`
- 改成 daemon `run()` 起来时调 `controller.apply(initial_target)`
- daemon shutdown 时调 `controller.apply(Disabled)`

`MobileLanLifecycleController` 在 `bootstrap.rs` 装配时构造，注入到 `MobileSyncFacade` 与 daemon 两边。

### 4.5 前端 UX 改动

`src/components/device/MobileSyncDevicesPanel.tsx`：

- 「+ Add」按钮 **始终启用**
- 点击时若 `!enabled || !lanListenEnabled`，弹引导对话框「移动同步未开启，是否开启？」+ 默认勾选 + 「开启并继续」按钮
- 开启走 `updateMobileSyncSettings({ enabled: true, lanListenEnabled: true })` → 0.5s 内 `endpoint_info.url` 写入 → 直接进 `AddMobileSyncDeviceDialog` 让填 label

`src/components/device/MobileSyncSettingsSheet.tsx`：

- 删 `restartRequired` state + amber banner + 「立即重启」按钮
- 改为 toast 即时确认「移动同步已开启，监听 http://0.0.0.0:42720」
- 安全告警 `AlertDialog` 保留（首次开 LAN listener 仍要二次确认风险），但仅在「关→开 lanListen」时弹

`src/components/setting/NetworkSection.tsx`：

- **保持不变**（iroh RelayMode 仍走 restart_app，受 Pitfall 3 / 10 约束）

## 5. 风险点与缓解

| 风险 | 缓解 |
|---|---|
| daemon shutdown 时 controller 没 stop listener → 端口持有进 daemon 下次启动 | daemon `run()` 退出前 `controller.apply(Disabled).await` 兜底 + 测试覆盖 |
| rebind 撞端口占用 → endpoint_info.set_bind_failure 但 controller state 错乱 | bind 失败时 controller state 保持 `None`，下次 apply 重试；前端读 `lanListenerError` 显示 |
| 用户在 LAN listener 跑着时改 port，旧连接被切断 | listener 用 `with_graceful_shutdown`，最长 5s drain；iPhone 端 401 会重试，可接受 |
| 注释 `SPEC §1.2.5` 删了之后下个人不知道这条约束已经被取消 | 在新增的 controller 文件 + CHANGELOG 写明 2026-05-11 撤销该约束 |
| FacadeAssembly 构造顺序：facade 需要 controller，controller 需要 facade（循环）| `Arc<dyn MobileLanLifecyclePort>` 在 facade 持有；controller 不持 facade，而是持 facade 的下游 ports（endpoint_info、file_transfer），避免循环 |

## 6. 改动文件清单（预估）

| 文件 | 操作 | 行数估算 |
|---|---|---|
| `uc-core/src/ports/mobile_sync_lan_lifecycle.rs` | 新建 | ~30 |
| `uc-core/src/ports/mod.rs` | 加 pub use | ~2 |
| `uc-desktop/src/daemon/mobile_lan_lifecycle.rs` | 新建 + 单测 | ~250 |
| `uc-desktop/src/daemon/mod.rs` | 加 mod | ~2 |
| `uc-application/src/facade/mobile_sync/facade.rs` | update_settings 改 + 测试 | ~80 |
| `uc-application/src/facade/mobile_sync/deps.rs` | 加 lan_lifecycle 字段 | ~15 |
| `uc-application/src/usecases/mobile_sync/update_settings.rs` | restart_required 永远 false + 注释 | ~30 |
| `uc-desktop/src/daemon/app.rs` | 删 spawn 改 controller | ~50 |
| `uc-desktop/src/daemon/bootstrap.rs` 或 `app_facade_assembly.rs` | 装配 controller | ~30 |
| `uc-webserver/src/mobile_lan/server.rs` 注释 SPEC §1.2.5 删除 | ~5 |
| `uc-core/src/settings/model.rs` 注释 SPEC §1.2.5 删除 | ~5 |
| `src/api/tauri-command/mobile_sync.ts` | UpdateMobileSyncSettingsResult 字段意义改 | ~10 |
| `src/components/device/MobileSyncDevicesPanel.tsx` | Add 按钮 unconditional + 引导对话框 | ~80 |
| `src/components/device/MobileSyncSettingsSheet.tsx` | 删 restart banner + toast 反馈 | ~60 |
| `src/components/device/__tests__/MobileSyncSettingsSheet.test.tsx` | 测试改 | ~50 |
| `src/components/device/__tests__/MobileSyncDevicesPanel.test.tsx` | 测试改 + 新增首次添加 e2e | ~80 |
| `src/i18n/locales/*.json` | 删 restart 文案 + 加引导文案 | ~20 |
| 集成测试 `uc-webserver/tests/mobile_lan_lifecycle.rs` | 新建 | ~150 |
| **合计** | | **~950 行** |

## 7. Atomic commits 拆分（预计 6 个）

按 `docs/agent/architecture-rules.md` 的"边界 atomic"原则：

1. **commit 1**: `uc-core` 加 `MobileLanLifecyclePort` trait + `MobileLanTarget` enum（纯领域定义）
2. **commit 2**: `uc-desktop` 实现 `MobileLanLifecycleController` adapter + 单测（不接 facade，独立可测）
3. **commit 3**: `uc-application` `MobileSyncFacade::update_settings` 接 lifecycle port + facade 测试（mock port）
4. **commit 4**: `uc-desktop` daemon `app.rs` 改用 controller + 装配链 + 集成测试
5. **commit 5**: `update_settings` use case `restart_required` 永远 false + 清理 SPEC §1.2.5 注释
6. **commit 6**: 前端 UX 重做（Panel + Sheet + 测试 + i18n）

## 8. 验收标准

- [ ] 首次用户从 `+ Add` 到拿到凭据 ≤ 5s，**零进程重启**
- [ ] 现有用户在 `MobileSyncSettingsSheet` 切 enabled / lanListenEnabled / port 全部即时生效，0 banner
- [ ] daemon shutdown → restart → listener 状态正确同步
- [ ] rebind 撞占用端口 → `endpoint_info.lanListenerError` 反馈 + UI 显示
- [ ] `NetworkSection`（iroh）仍走 restart_app（保留 Pitfall 3 / 10 约束）
- [ ] `cargo test -p uc-core -p uc-application -p uc-desktop -p uc-webserver` 全通
- [ ] `pnpm exec vitest run` 全通
- [ ] 手动测试三平台（mac/win/linux）首次添加流程

## 9. 关键不变量（动手时不能破的）

1. **iroh BIND_LOCK 不能碰**（Pitfall 3）—— 本次完全不动 iroh / NetworkSection
2. **凭据一次性回显**（`RegisterMobileDeviceResult.password` 仅展示一次）—— UI 改流程时不能丢失这条
3. **graceful shutdown ≤ 5s**（SPEC §3.3，仍有效）—— `with_graceful_shutdown` 保留
4. **port doc 不污染 core**（uc-core/AGENTS.md §5.4）—— `MobileLanLifecyclePort` doc 只描述领域语义，不提 mobile_sync / SyncClipboard / axum
5. **facade 是唯一对外出口**（uc-application/AGENTS.md §11.4）—— controller 不能被外部 crate 直接 import，必须经 facade

## 10. 调研时间戳

- 开始调研：2026-05-11
- 调研完成：2026-05-11
- 项目状态：`stitch-grease` 分支，干净工作树，HEAD `c825d96c`（refactor(daemon-restart) 已合入）
