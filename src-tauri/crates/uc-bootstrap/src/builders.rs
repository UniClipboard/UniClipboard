//! # Scene-Specific Builders
//!
//! Entry-point constructors for CLI runtime modes + daemon-lifecycle
//!装配。进程级 runtime 入口 (`build_process_runtime` +
//! `ProcessRuntimeContext`) 住在 [`uc_desktop::bootstrap`]——本 crate
//! 保持 shell-agnostic,只提供 composition-root 装配工具。
//!
//! CLI 入口 (`build_cli_context_with_profile` / `build_slice1_cli_context`)
//! 通过私有 `build_core()` 跑 tracing init + `wire_dependencies`,返回
//! 各自的 context struct;CLI 不 spawn background workers,装出来的
//! `BackgroundRuntimeDeps` 直接 drop。
//!
//! [`build_daemon_lifecycle`] 接受已有 [`crate::assembly::WiredDependencies`]
//! 作输入,**不** 再次 wire —— sqlite pool / repos / settings 等跨 daemon
//! reload 复用,daemon-lifecycle 只装 iroh node + space_setup + 启动期
//! reconcile。

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::ClipboardSyncFacade;
use uc_core::config::AppConfig;

use crate::assembly::{get_storage_paths, wire_dependencies, BackgroundRuntimeDeps};
use crate::space_setup::{build_space_setup_assembly, SpaceSetupAssembly};

/// Context for CLI entry point. AppDeps + config, no background workers.
/// Caller constructs CoreRuntime from deps as needed.
pub struct CliBootstrapContext {
    pub deps: AppDeps,
    pub config: AppConfig,
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
) -> anyhow::Result<(
    AppConfig,
    crate::assembly::WiredDependencies,
    BackgroundRuntimeDeps,
)> {
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

    let (wired, background) = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;

    // 注册进程级 product analytics `EventContext`。失败不阻断启动 —— analytics
    // 是辅助通道，错误已在 compose 内 warn-log（见 `analytics.rs` 模块文档"失
    // 败语义"）。`get_storage_paths` 重新解析了一次目录布局；它内部纯计算无
    // IO，开销可忽略。
    let storage_paths = get_storage_paths(&config)?;
    // 旧 CLI 进程内路径不是临时 daemon residency → 永不抑制设备级 presence 事件
    // （ADR-008 D20），保持 pre-P5 行为；P5-1/P5-2 会整体退役这条路径。
    if let Err(err) =
        crate::analytics::compose_event_context(&wired.deps, &storage_paths, false).await
    {
        tracing::warn!(
            error = %err,
            "analytics: compose_event_context 失败，本次进程内事件 sink 将拿不到 EventContext 快照"
        );
    }

    Ok((config, wired, background))
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
    // CLI 不跑 background workers,装出来的 BackgroundRuntimeDeps 直接 drop。
    let (config, wired, _background) = build_core(log_profile).await?;

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
    let (config, wired, _background) = build_core(log_profile).await?;
    Ok((config, wired))
}

/// Backward-compatible alias for older Slice 1 callers.
///
/// Slice 1 CLI doesn't spawn the blob/spool workers — the
/// [`BackgroundRuntimeDeps`] produced alongside is dropped here.
/// and therefore preserves access to `trusted_peer_repo` and other Slice
/// 1-only ports the `SpaceSetupFacade` needs.
pub async fn build_slice1_cli_context(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<(AppConfig, crate::assembly::WiredDependencies)> {
    let (config, wired, _background) = build_core(log_profile).await?;
    Ok((config, wired))
}

/// daemon-lifecycle 装配产出。
///
/// 不再持有 `deps` / `background` —— 那两块属于进程级 (sqlite pool /
/// repos / blob worker), 由 caller 一次性 wire 后移交
/// [`build_daemon_lifecycle`]。方案 C 后 daemon 在进程内只起一次, 装配
/// 也只跑一次。本结构体的物理意义是 "async (tokio) 装配链路上需要 iroh
/// bind 与 SpaceSetupAssembly 的那一段"。
pub struct DaemonLifecycle {
    /// iroh-stack clipboard sync facade.
    /// daemon 的 `DaemonClipboardChangeHandler` 调
    /// `clipboard_sync_facade.dispatch_snapshot(...)`;
    /// `InboundClipboardSyncWorker` 通过 `subscribe_inbound_notices()` 订阅。
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    /// 完整 iroh assembly。持有 iroh node、pairing/presence/clipboard
    /// handler、auto-spawned ingest loop。daemon shutdown 调
    /// `space_setup_assembly.shutdown()` 干净拆 router + abort ingest。
    pub space_setup_assembly: SpaceSetupAssembly,
}

/// 装 daemon-lifecycle 资源 —— iroh node bind、SpaceSetupAssembly、startup
/// reconcile。接受已 wire 好的进程级 [`WiredDependencies`] 作输入,**不**
/// 再次跑 `wire_dependencies` —— sqlite pool / repos / settings / secure
/// storage 在进程启动期 wire 一次后由 GUI shell 与 daemon-lifecycle 共用。
/// 本函数是 async/sync 装配链的边界点 (iroh `Endpoint::bind` 必须在 tokio
/// runtime 内执行)。
///
/// `init::reconcile_*` 在每次 daemon 启动时跑(治理性、失败只 log),
/// 与 `build_space_setup_assembly` 之前执行,确保 dispatch / presence /
/// 重新配对路径一上线就是干净状态。
///
/// caller 必须在 tokio runtime 上下文中调用 —— `build_space_setup_assembly`
/// 内部 `Endpoint::bind` 会 spawn magicsock / relay / STUN actor。
pub async fn build_daemon_lifecycle(
    wired: &crate::assembly::WiredDependencies,
) -> anyhow::Result<DaemonLifecycle> {
    // 启动期 reconcile:把 peer_addr_repo / trusted_peer_repo 中
    // member_repo 已不再持有的孤儿条目清掉,恢复设计意图的不变量
    // `peer_addr ⊆ member`、`trusted_peer ⊆ member`。失败只 log 不阻断
    // 启动 —— reconcile 是治理性的。
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

    // Phase 94 NETSET-03:从 settings 读取 LAN-only Mode 偏好后翻译为
    // `IrohNodeConfig`。`SettingsPort::load` 当前错误返回类型 `anyhow::Result`
    // 不区分 NotFound vs Parse;`FileSettingsRepository::load` 已对 NotFound
    // 兜底返回 `Settings::default()` (即 `allow_relay_fallback: true`)。
    // 故此处只需对剩余 Parse/IO 错误硬失败 —— LAN-only 信任锚点不容许脏
    // settings 撒谎。
    let settings = wired
        .deps
        .settings
        .load()
        .await
        .map_err(|err| anyhow::anyhow!("settings load failed at startup: {err}"))?;
    let allow_relay_fallback = settings.network.allow_relay_fallback;
    let allow_overlay_network_addrs = settings.network.allow_overlay_network_addrs;
    let custom_relay_urls = settings.network.custom_relay_urls.clone();

    // 【checker BLOCKER 4 — 单一取反点铁律】
    // `disable_relays` 的值**只能**通过 `relay_policy_to_iroh_config` 取得,
    // **不**在此处内联写 `let disable_relays = !allow_relay_fallback;`。
    let mut iroh_config = crate::network_policy::relay_policy_to_iroh_config(
        allow_relay_fallback,
        allow_overlay_network_addrs,
        custom_relay_urls,
        None, // production 不 override rendezvous,使用默认 RENDEZVOUS_BASE_URL
    );
    // #900：从 env 读取直连可达性（固定 UDP 端口 + 广播公网地址）并写入。
    // 必须在 `build_space_setup_assembly`（首次 endpoint 快照/配对交换）之前。
    crate::network_policy::apply_iroh_direct_reachability_from_env(&mut iroh_config);

    tracing::info!(
        target: "settings.network",
        allow_relay_fallback,
        disable_relays = iroh_config.disable_relays,
        allow_overlay_network_addrs = iroh_config.allow_overlay_network_addrs,
        custom_relay_count = iroh_config.custom_relay_urls.len(),
        "applying network settings: allow_relay_fallback={} → disable_relays={}, allow_overlay_network_addrs={}, custom_relay_count={}",
        allow_relay_fallback,
        iroh_config.disable_relays,
        iroh_config.allow_overlay_network_addrs,
        iroh_config.custom_relay_urls.len(),
    );

    let space_setup_assembly = build_space_setup_assembly(wired, iroh_config)
        .await
        .map_err(|e| anyhow::anyhow!("Slice 1+ assembly build failed: {e}"))?;

    // Same Arc the assembly holds — handed up so daemon entrypoint can
    // wire it into the two clipboard workers without unpacking the assembly.
    let clipboard_sync_facade = Arc::clone(&space_setup_assembly.clipboard_sync);

    Ok(DaemonLifecycle {
        clipboard_sync_facade,
        space_setup_assembly,
    })
}
