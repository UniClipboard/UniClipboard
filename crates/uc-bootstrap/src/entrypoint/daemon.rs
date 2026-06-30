//! Daemon-lifecycle composition-root entry.
//!
//! [`build_daemon_lifecycle`] accepts already-wired
//! [`crate::wiring::deps::WiredDependencies`] as input and does **not** re-run
//! `wire_dependencies` — sqlite pool / repos / settings / secure storage are
//! wired once at process start and shared between the GUI shell and the
//! daemon lifecycle. This entry only binds the iroh node + `SyncEngineAssembly`
//! and runs the startup reconcile passes; it is the async/sync boundary of the
//! assembly chain (iroh `Endpoint::bind` must run inside a tokio runtime).

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::ClipboardSyncFacade;

use crate::subsystem::sync_engine::{build_sync_engine_assembly, SyncEngineAssembly};
use crate::wiring::deps::{SharedRuntimeDeps, SyncEngineDeps};

/// daemon-lifecycle 装配产出。
///
/// 不再持有 `deps` / `background` —— 那两块属于进程级 (sqlite pool /
/// repos / blob worker), 由 caller 一次性 wire 后移交
/// [`build_daemon_lifecycle`]。方案 C 后 daemon 在进程内只起一次, 装配
/// 也只跑一次。本结构体的物理意义是 "async (tokio) 装配链路上需要 iroh
/// bind 与 SyncEngineAssembly 的那一段"。
pub struct DaemonLifecycle {
    /// iroh-stack clipboard sync facade.
    /// daemon 的 `DaemonClipboardChangeHandler` 调
    /// `clipboard_sync_facade.dispatch_snapshot(...)`;
    /// `InboundClipboardSyncWorker` 通过 `subscribe_inbound_notices()` 订阅。
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    /// 完整 iroh assembly。持有 iroh node、pairing/presence/clipboard
    /// handler、auto-spawned ingest loop。daemon shutdown 调
    /// `sync_engine_assembly.shutdown()` 干净拆 router + abort ingest。
    pub sync_engine_assembly: SyncEngineAssembly,
}

/// 装 daemon-lifecycle 资源 —— iroh node bind、SyncEngineAssembly、startup
/// reconcile。接受已 wire 好的进程级 [`crate::wiring::deps::WiredDependencies`] 作输入,
/// **不** 再次跑 `wire_dependencies` —— sqlite pool / repos / settings / secure
/// storage 在进程启动期 wire 一次后由 GUI shell 与 daemon-lifecycle 共用。
/// 本函数是 async/sync 装配链的边界点 (iroh `Endpoint::bind` 必须在 tokio
/// runtime 内执行)。
///
/// `startup::reconcile::reconcile_*` 在每次 daemon 启动时跑(治理性、失败只 log),
/// 与 `build_sync_engine_assembly` 之前执行,确保 dispatch / presence /
/// 重新配对路径一上线就是干净状态。
///
/// caller 必须在 tokio runtime 上下文中调用 —— `build_sync_engine_assembly`
/// 内部 `Endpoint::bind` 会 spawn magicsock / relay / STUN actor。
pub async fn build_daemon_lifecycle(
    deps: &AppDeps,
    space_setup: &SyncEngineDeps,
    shared: &SharedRuntimeDeps,
) -> anyhow::Result<DaemonLifecycle> {
    // 启动期 reconcile:把 peer_addr_repo / trusted_peer_repo 中
    // member_repo 已不再持有的孤儿条目清掉,恢复设计意图的不变量
    // `peer_addr ⊆ member`、`trusted_peer ⊆ member`。失败只 log 不阻断
    // 启动 —— reconcile 是治理性的。
    if let Err(err) = crate::startup::reconcile::reconcile_peer_addresses(
        Arc::clone(&deps.device.member_repo),
        Arc::clone(&space_setup.peer_addr_repo),
    )
    .await
    {
        tracing::warn!(
            error = %err,
            "peer_addr reconcile failed at boot; daemon continues with whatever orphans remain"
        );
    }
    if let Err(err) = crate::startup::reconcile::reconcile_trusted_peers(
        Arc::clone(&deps.device.member_repo),
        Arc::clone(&shared.trusted_peer_repo),
    )
    .await
    {
        tracing::warn!(
            error = %err,
            "trusted_peer reconcile failed at boot; daemon continues with whatever orphans remain"
        );
    }

    // Phase 94 NETSET-03:从 settings 读取 LAN-only Mode 偏好后翻译为
    // `IrohNodeConfig`。`SettingsPort::load` 当前错误返回类型 `anyhow::Result`
    // 不区分 NotFound vs Parse;`FileSettingsRepository::load` 已对 NotFound
    // 兜底返回 `Settings::default()` (即 `allow_relay_fallback: true`)。
    // 故此处只需对剩余 Parse/IO 错误硬失败 —— LAN-only 信任锚点不容许脏
    // settings 撒谎。
    let settings = deps
        .settings
        .load()
        .await
        .map_err(|err| anyhow::anyhow!("settings load failed at startup: {err}"))?;
    let allow_relay_fallback = settings.network.allow_relay_fallback;
    let allow_overlay_network_addrs = settings.network.allow_overlay_network_addrs;
    let custom_relay_urls = settings.network.custom_relay_urls.clone();
    let congestion_controller = settings.network.congestion_controller;

    // 【checker BLOCKER 4 — 单一取反点铁律】
    // `disable_relays` 的值**只能**通过 `relay_policy_to_iroh_config` 取得,
    // **不**在此处内联写 `let disable_relays = !allow_relay_fallback;`。
    let mut iroh_config = crate::wiring::network_policy::relay_policy_to_iroh_config(
        allow_relay_fallback,
        allow_overlay_network_addrs,
        custom_relay_urls,
        congestion_controller,
        None, // production 不 override rendezvous,使用默认 RENDEZVOUS_BASE_URL
    );
    // #900：从 env 读取直连可达性（固定 UDP 端口 + 广播公网地址）并写入。
    // 必须在 `build_sync_engine_assembly`（首次 endpoint 快照/配对交换）之前。
    crate::wiring::network_policy::apply_iroh_direct_reachability_from_env(&mut iroh_config);
    crate::wiring::network_policy::apply_congestion_controller_from_env(&mut iroh_config);

    tracing::info!(
        target: "settings.network",
        allow_relay_fallback,
        disable_relays = iroh_config.disable_relays,
        allow_overlay_network_addrs = iroh_config.allow_overlay_network_addrs,
        custom_relay_count = iroh_config.custom_relay_urls.len(),
        congestion_controller = %iroh_config.congestion_controller,
        "applying network settings: allow_relay_fallback={} → disable_relays={}, allow_overlay_network_addrs={}, custom_relay_count={}, cc={}",
        allow_relay_fallback,
        iroh_config.disable_relays,
        iroh_config.allow_overlay_network_addrs,
        iroh_config.custom_relay_urls.len(),
        iroh_config.congestion_controller,
    );

    let sync_engine_assembly = build_sync_engine_assembly(deps, space_setup, shared, iroh_config)
        .await
        .map_err(|e| anyhow::anyhow!("Slice 1+ assembly build failed: {e}"))?;

    // Same Arc the assembly holds — handed up so daemon entrypoint can
    // wire it into the two clipboard workers without unpacking the assembly.
    let clipboard_sync_facade = Arc::clone(&sync_engine_assembly.clipboard_sync);

    Ok(DaemonLifecycle {
        clipboard_sync_facade,
        sync_engine_assembly,
    })
}
