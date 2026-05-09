//! # Scene-Specific Builders
//!
//! Entry-point constructors for CLI and daemon runtime modes. The GUI shell
//! entry-point (`build_gui_app` + `GuiBootstrapContext`) lives in
//! [`uc_desktop::bootstrap`]—this crate stays GUI-shell agnostic and only
//! provides the composition-root assembly tools the desktop crate then
//! uses to wire its own GUI builder.
//!
//! Both builders here share a private `build_core()` helper that:
//! 1. Initializes tracing (idempotent)
//! 2. Resolves application config
//! 3. Wires all dependencies via `wire_dependencies`
//!
//! Each builder returns a context struct containing `AppDeps` (NOT `CoreRuntime`).
//! Callers construct `CoreRuntime` themselves with the appropriate emitter cell,
//! lifecycle status, and task registry.

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::{AppPaths, ClipboardSyncFacade, HostEventEmitterPort};
use uc_core::config::AppConfig;

use crate::assembly::{get_storage_paths, wire_dependencies, BackgroundRuntimeDeps};
use crate::space_setup::{build_space_setup_assembly, SpaceSetupAssembly};

/// Context for CLI entry point. AppDeps + config, no background workers.
/// Caller constructs CoreRuntime from deps as needed.
pub struct CliBootstrapContext {
    pub deps: AppDeps,
    pub config: AppConfig,
}

/// Context for daemon entry point. AppDeps + background deps,
/// workers not started. Caller constructs CoreRuntime and starts background workers.
pub struct DaemonBootstrapContext {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
    /// iroh-stack clipboard sync facade.
    /// Daemon's `DaemonClipboardChangeHandler` calls
    /// `clipboard_sync_facade.dispatch_snapshot(...)`;
    /// `InboundClipboardSyncWorker` subscribes via
    /// `subscribe_inbound_notices()`.
    ///
    /// Same Arc as the one held by `space_setup_assembly.clipboard_sync` —
    /// kept here as a top-level field so daemon entrypoint code reads
    /// off `ctx.clipboard_sync_facade` directly without unwrapping the
    /// assembly.
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    /// Full iroh assembly. Owns the iroh node, pairing/presence/clipboard
    /// handlers, and the auto-spawned ingest loop. Daemon shutdown calls
    /// `space_setup_assembly.shutdown()` to cleanly tear down router +
    /// abort ingest before the Tokio runtime exits.
    pub space_setup_assembly: SpaceSetupAssembly,
    /// Mobile sync LAN endpoint adapter(具体类型旁路)。
    ///
    /// 由 daemon LAN listener 启停时调 inherent `set` / `clear` 写,facade
    /// 通过 `AppDeps.mobile_sync.endpoint_info` 只读 —— 共享同一份 Arc。
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
}

/// Shared core wiring used by all three builders.
/// Initializes tracing, resolves config, wires dependencies, and registers the
/// process-wide product analytics `EventContext`.
///
/// If `log_profile_override` is `Some`, the `UC_LOG_PROFILE` env var is set
/// before tracing initialization so the subscriber picks up the desired profile.
///
/// ## Async because of `compose_event_context`
///
/// Slice 6 / Issue #549 起 `build_core` 转 async：装配 `EventContext` 必须在
/// `wire_dependencies` 之后做，因为它要读 `member_repo` / `setup_status`
/// 这两个 async port 才能算出 `active_device_count` 与 `space_id_hash`。把
/// 装配点放在 composition root 内（一处调用）比让每个 entry 各自补一段
/// `.await` 更不容易遗漏（例如未来再加一个 entry，自动也覆盖）。
async fn build_core(
    log_profile_override: Option<uc_observability::LogProfile>,
) -> anyhow::Result<(AppConfig, crate::assembly::WiredDependencies)> {
    // Apply log profile override before tracing init
    if let Some(profile) = log_profile_override {
        std::env::set_var("UC_LOG_PROFILE", profile.to_string());
    }

    // Idempotent -- safe to call multiple times
    crate::tracing::init_tracing_subscriber()?;

    // 装 panic hook 把 panic 镜像到 jsonl(target = "panic")。必须在
    // tracing init 之后,否则 hook 触发时 subscriber 还没接管 stderr,
    // 等价于啥也没做。同样幂等,内部用 OnceLock 保证三个入口共用同一份。
    crate::tracing::install_panic_logging_hook();

    let config = AppConfig::empty();

    let wired = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;

    // 注册进程级 product analytics `EventContext`。失败不阻断启动 —— analytics
    // 是辅助通道，错误已在 compose 内 warn-log（见 `analytics.rs` 模块文档"失
    // 败语义"）。`get_storage_paths` 重新解析了一次目录布局；它内部纯计算无
    // IO，开销可忽略。
    let storage_paths = get_storage_paths(&config)?;
    if let Err(err) = crate::analytics::compose_event_context(&wired.deps, &storage_paths).await {
        tracing::warn!(
            error = %err,
            "analytics: compose_event_context 失败，本次进程内事件 sink 将拿不到 EventContext 快照"
        );
    }

    Ok((config, wired))
}

/// Build CLI bootstrap context. Returns AppDeps for the caller to construct
/// CoreRuntime as needed. No background workers are started.
pub async fn build_cli_context() -> anyhow::Result<CliBootstrapContext> {
    build_cli_context_with_profile(Some(uc_observability::LogProfile::Cli)).await
}

/// Build CLI bootstrap context with an explicit log profile override.
///
/// When `verbose` mode is active, callers pass `Some(LogProfile::Dev)` to
/// get full console tracing. The default `build_cli_context()` uses `Cli`
/// profile which suppresses console output.
pub async fn build_cli_context_with_profile(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CliBootstrapContext> {
    let (config, wired) = build_core(log_profile).await?;

    // [Codex Review R1] Return AppDeps, not CoreRuntime.
    // CLI entry point constructs CoreRuntime itself with appropriate emitter.
    Ok(CliBootstrapContext {
        deps: wired.deps,
        config,
    })
}

/// CLI composition-root entry returning the full
/// [`crate::assembly::WiredDependencies`] so the caller can hand it to
/// [`crate::space_setup::build_space_setup_assembly`]; unlike
/// [`build_cli_context_with_profile`], this does not flatten to `AppDeps`
/// and therefore preserves access to `trusted_peer_repo` and other ports
/// the `SpaceSetupFacade` needs (pairing / roster / send / watch / blob 等
/// 需要 iroh 网络栈的 CLI 命令走这条路径)。
pub async fn build_cli_wiring_context(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<(AppConfig, crate::assembly::WiredDependencies)> {
    build_core(log_profile).await
}

/// Build daemon bootstrap context. Returns AppDeps + background deps.
/// Caller constructs CoreRuntime and starts background workers.
///
/// Also binds the iroh node + builds the full `SpaceSetupAssembly`
/// (pairing + presence + clipboard handlers) and exposes
/// `clipboard_sync_facade` so daemon workers can dispatch / subscribe
/// via the iroh stack. `trusted_peer_repo` is consumed by
/// `build_space_setup_assembly` (the setup-v2 flow) and is not
/// re-exposed on the returned ctx.
pub async fn build_daemon_app() -> anyhow::Result<DaemonBootstrapContext> {
    let (config, wired) = build_core(None).await?;
    let storage_paths = get_storage_paths(&config)?;

    // 启动期 reconcile:把 peer_addr_repo / trusted_peer_repo 中
    // member_repo 已不再持有的孤儿条目清掉,恢复设计意图的不变量
    // `peer_addr ⊆ member`、`trusted_peer ⊆ member`(见
    // `dispatch_entry.rs` module doc 关于 paired-members 权威集合,
    // `trust_peer.rs` 关于"先 Distrust 再 Trust" 的显式流程,以及
    // `init.rs::reconcile_*`)。两者都在 `build_space_setup_assembly` 之前
    // 执行,确保 dispatch / presence / 重新配对路径一上线就是干净状态。
    // 失败只 log 不阻断启动 —— reconcile 是治理性的。
    if let Err(err) = crate::init::reconcile_peer_addresses(
        Arc::clone(&wired.deps.device.member_repo),
        Arc::clone(&wired.peer_addr_repo),
    )
    .await
    {
        tracing::warn!(
            error = %err,
            "peer_addr reconcile failed at boot; daemon continues with whatever orphans remain"
        );
    }
    if let Err(err) = crate::init::reconcile_trusted_peers(
        Arc::clone(&wired.deps.device.member_repo),
        Arc::clone(&wired.trusted_peer_repo),
    )
    .await
    {
        tracing::warn!(
            error = %err,
            "trusted_peer reconcile failed at boot; daemon continues with whatever orphans remain"
        );
    }

    // Build the iroh-stack assembly on the caller's runtime. Must NOT spin up
    // a throwaway current-thread rt here: `Endpoint::bind` spawns magicsock /
    // relay / STUN actors via `tokio::spawn`, which attach to whatever runtime
    // is running the bind. If that runtime drops (as a short-lived local rt
    // would), those actors are aborted and the Endpoint becomes a zombie —
    // `connect()` then returns "Unable to connect to remote" instantly and
    // `accept` sees no incoming traffic. Keeping the bind on the caller's
    // long-lived daemon runtime keeps iroh's tasks alive for the process
    // lifetime.

    // Phase 94 NETSET-03：从 settings 读取 LAN-only Mode 偏好后翻译为
    // `IrohNodeConfig`。`SettingsPort::load` 当前错误返回类型 `anyhow::Result`
    // 不区分 NotFound vs Parse；`FileSettingsRepository::load`
    // (`repository.rs:166-168`) 已对 NotFound 兜底返回 `Settings::default()`
    // (即 `allow_relay_fallback: true`)。故此处只需对剩余 Parse/IO 错误硬失败
    // —— LAN-only 信任锚点不容许脏 settings 撒谎（D-B1 选项 B 现状决策 — 见
    // 094-CONTEXT.md `<deferred>` 已记录后续 phase 实施 `SettingsLoadError`
    // 偿还此隐式契约）。
    let settings = wired
        .deps
        .settings
        .load()
        .await
        .map_err(|err| anyhow::anyhow!("settings load failed at startup: {err}"))?;
    let allow_relay_fallback = settings.network.allow_relay_fallback;
    let allow_overlay_network_addrs = settings.network.allow_overlay_network_addrs;

    // 【checker BLOCKER 4 — 单一取反点铁律】
    // `disable_relays` 的值**只能**通过 `relay_policy_to_iroh_config` 取得，
    // **不**在此处内联写 `let disable_relays = !allow_relay_fallback;`（这会让
    // 取反点泄漏到第二处，违反 Pattern A）。下方 tracing::info! 字段值从
    // `iroh_config.disable_relays` 读取。
    let iroh_config = crate::network_policy::relay_policy_to_iroh_config(
        allow_relay_fallback,
        allow_overlay_network_addrs,
        None, // production 不 override rendezvous，使用默认 RENDEZVOUS_BASE_URL
    );

    // D-B3：方便 support 排障 — 字段名固定为 `allow_relay_fallback` /
    // `disable_relays`（与代码一致）。**不**在 OTLP 加 attribute（Pitfall 6）。
    // 【checker BLOCKER 4 / W1】tracing 字段值通过 `iroh_config.disable_relays`
    // 读取，保证唯一取反点位于 network_policy.rs。
    tracing::info!(
        target: "settings.network",
        allow_relay_fallback,
        disable_relays = iroh_config.disable_relays,
        allow_overlay_network_addrs = iroh_config.allow_overlay_network_addrs,
        "applying network settings: allow_relay_fallback={} → disable_relays={}, allow_overlay_network_addrs={}",
        allow_relay_fallback,
        iroh_config.disable_relays,
        iroh_config.allow_overlay_network_addrs,
    );

    let space_setup_assembly = build_space_setup_assembly(&wired, iroh_config)
        .await
        .map_err(|e| anyhow::anyhow!("Slice 1+ assembly build failed: {e}"))?;

    // Now safe to consume `wired` — assembly is built and owns its own
    // Arcs to the underlying ports.
    let deps = wired.deps;
    let background = wired.background;
    let emitter_cell = wired.emitter_cell;
    let mobile_sync_endpoint_info = wired.mobile_sync_endpoint_info;

    // Same Arc the assembly holds — handed up to ctx so daemon entrypoint
    // (T6) can wire it into the two clipboard workers without unpacking
    // the assembly.
    let clipboard_sync_facade = Arc::clone(&space_setup_assembly.clipboard_sync);

    Ok(DaemonBootstrapContext {
        deps,
        background,
        emitter_cell,
        storage_paths,
        config,
        clipboard_sync_facade,
        space_setup_assembly,
        mobile_sync_endpoint_info,
    })
}
