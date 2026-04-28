//! Slice 1 composition root for [`SpaceSetupFacade`].
//!
//! Assembles the pairing stack (rendezvous client + iroh session adapter +
//! identity store + proof verifier) plus the pre-existing persistence /
//! identity ports from [`WiredDependencies`] into a single
//! [`SpaceSetupAssembly`] that external callers (Tauri commands, CLI, daemon)
//! can drive through `Arc<SpaceSetupFacade>`.
//!
//! Everything iroh-specific stays inside
//! [`uc_infra::network::iroh::IrohNode`] so this module depends only on
//! core ports + the `IrohNode` handle. When Slice 2 adds a clipboard-sync
//! transport, the extension point is `IrohNode::install_clipboard` rather
//! than a second stack.
//!
//! Shutdown is a two-step coordinated teardown: first drive the facade's
//! own shutdown (aborts the sponsor-side inbound orchestrator task + best-
//! effort `stop_network`), then shut the iroh router down so live
//! connections see a clean `CONNECTION_CLOSE` rather than waiting for peer
//! timeouts.

use std::sync::Arc;

use tracing::{info, instrument};

use uc_application::facade::{
    BlobTransferDeps, BlobTransferFacade, ClipboardSyncDeps, ClipboardSyncFacade, IngestHandle,
    MemberRosterDeps, MemberRosterFacade, SpaceSetupDeps, SpaceSetupFacade,
};
use uc_application::proof::HmacProofAdapter;
use uc_core::ports::blob::{BlobReferenceRepositoryPort, BlobTransferPort};
use uc_core::ports::space::ProofPort;
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardReceiverPort, LocalIdentityPort, PresencePort,
};
use uc_infra::network::iroh::{
    BlobHandlers, ClipboardHandlers, IrohIdentityStore, IrohNode, IrohNodeBuilder, IrohNodeError,
};
// Re-exported so external callers can parametrise the assembly without
// having to `use uc_infra` themselves.
pub use uc_infra::network::iroh::IrohNodeConfig;
use uc_infra::security::Sha256IdentityFingerprintFactory;

use crate::assembly::WiredDependencies;

/// Output of [`build_space_setup_assembly`]. External callers keep the
/// whole assembly alive for the process lifetime; they only dispatch
/// user-facing commands through [`Self::facade`] / [`Self::roster`] and
/// run [`Self::shutdown`] once on exit.
pub struct SpaceSetupAssembly {
    pub facade: Arc<SpaceSetupFacade>,
    /// Slice 2 Phase 1 · T9:roster 查询门面(`list_with_presence` +
    /// `subscribe_presence_events`)。CLI `members` 命令从这里拿状态,
    /// tauri `get_roster` 将来也走同一条。共享同一个 `peer_addr_repo` /
    /// `presence` 实例,所以 F1 hook 填好的缓存这里能直接读到。
    pub roster: Arc<MemberRosterFacade>,
    /// Slice 2 Phase 2 · T10:剪切板同步门面(`dispatch_entry` +
    /// `subscribe_inbound_notices`)。CLI `send` / `watch` 通过这里走。
    /// 与 `roster` 同样共享 `peer_addr_repo` / `presence`,所以 F1 hook
    /// 喂好的 presence 缓存,`dispatch_entry` 能直接读到。
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    /// Slice 3 Phase 2:大 payload 发布 / 拉取门面。CLI 与后续 daemon/UI
    /// 都从这里走完整的 hash 去重、加解密和 blob 传输编排。
    pub blob: Arc<BlobTransferFacade>,
    /// Slice 3 Phase 1:大 payload 的 iroh-blobs 传输能力。Phase 2 的
    /// blob use case 会从这里接入。
    pub blob_transfer: Arc<dyn BlobTransferPort>,
    /// Slice 3 Phase 1:明文 hash → 密文 digest 去重缓存。与
    /// `blob_transfer` 分开装配,保持传输和 sqlite 缓存职责独立。
    pub blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
    /// Slice 4 Phase 1:presence port 直出。`facade` / `roster` /
    /// `clipboard_sync` 内部都已经持有同一份 Arc;daemon `PresenceMonitor`
    /// 也需要直接订阅事件流,所以这里再 clone 一份对外暴露,避免门面层
    /// 多包一道 subscribe 转发。
    pub presence: Arc<dyn PresencePort>,
    /// The shared iroh node. Held privately so callers can't bind a second
    /// node or install additional handlers after `spawn` — that would
    /// fragment peer identity (§"共用网络栈" decision, Slice 1 planning).
    iroh_node: IrohNode,
    /// Slice 2 Phase 2 · T10:ingest loop 的 join handle。装配时立即起一
    /// 次,与 receiver handler 同生命周期(handler 装在 `iroh_node` 的
    /// router 上,router shutdown 时 broadcast Sender 释放,loop 自然退
    /// 出 `RecvError::Closed`)。`shutdown` 显式 abort 一次走在 router
    /// 关闭之前,加快进程退出。
    ingest_handle: IngestHandle,
}

impl SpaceSetupAssembly {
    /// Coordinated teardown. Order matters:
    ///
    /// 1. [`SpaceSetupFacade::on_shutdown`] aborts the sponsor-side inbound
    ///    orchestrator task so the `pairing_events` receiver is dropped
    ///    before the adapter is torn down.
    /// 2. [`IrohNode::shutdown`] shuts the iroh router, which fires
    ///    `ProtocolHandler::shutdown` on the pairing handler and emits
    ///    `CONNECTION_CLOSE` to any live peer.
    #[instrument(skip_all)]
    pub async fn shutdown(self) {
        // Abort ingest loop ahead of router shutdown so the broadcast
        // receiver task exits before its sender (the receiver adapter
        // owned by the router) drops. Drop on `IngestHandle` would do the
        // same when `self` falls out of scope; the explicit call only
        // shaves a tick off teardown latency and makes ordering obvious.
        self.ingest_handle.abort();
        self.facade.on_shutdown().await;
        self.iroh_node.shutdown().await;
    }
}

/// Failures during Slice 1 assembly. Bootstrap callers surface these as
/// fatal startup errors — there is no useful retry here.
#[derive(Debug, thiserror::Error)]
pub enum SpaceSetupAssemblyError {
    #[error(transparent)]
    IrohNode(#[from] IrohNodeError),
}

/// Assemble the Slice 1 `SpaceSetupFacade` from an already-wired dependency
/// graph. Blocking responsibility: binds an iroh `Endpoint` and spawns its
/// router, so must be called inside a tokio runtime.
///
/// The resulting facade owns the entire Slice 1 lifecycle surface (A1 / A2
/// / B1 / B2 / F2). Call sites that also want to drive legacy setup should
/// keep holding their pre-existing `SetupFacade` alongside; the two
/// coexist until Slice 5 retires libp2p.
#[instrument(skip_all)]
pub async fn build_space_setup_assembly(
    wired: &WiredDependencies,
    iroh_config: IrohNodeConfig,
) -> Result<SpaceSetupAssembly, SpaceSetupAssemblyError> {
    let deps = &wired.deps;

    // IdentityFingerprintFactory is stateless — the one in SecurityPorts is
    // the same `Sha256IdentityFingerprintFactory` ZST, but we construct a
    // fresh one here rather than down-casting through `dyn` because
    // `IrohIdentityStore::new` takes the concrete factory trait object and
    // we'd have to re-wrap anyway.
    let identity_store = Arc::new(IrohIdentityStore::new(
        Arc::clone(&deps.security.secure_storage),
        Arc::new(Sha256IdentityFingerprintFactory),
    ));

    // Bind the shared iroh node + install the pairing transport. The
    // returned PairingHandlers carry the trait objects SpaceSetupDeps
    // wants; the iroh Router stays inside `IrohNode` so iroh types don't
    // leak out of this module.
    let mut builder = IrohNodeBuilder::bind(&identity_store, iroh_config).await?;
    let handlers = builder.install_pairing(
        Arc::clone(&deps.device.device_identity),
        Arc::clone(&deps.settings),
    );
    // Slice 2 Phase 1 · T8:在同一 iroh 节点上装 presence handler。must
    // be before `builder.spawn()`(install_* 要求 router 未 spawn)。
    // `Arc<dyn PresencePort>` 喂给 SpaceSetupDeps,facade 内部再构造
    // `EnsureReachableAllUseCase` 给 F1 hook 用。
    let presence: Arc<dyn PresencePort> = builder.install_presence(
        Arc::clone(&wired.peer_addr_repo),
        Arc::clone(&deps.system.clock),
    );
    // Slice 2 Phase 2 · T10:同一节点装第三个 ALPN(剪切板同步)。dispatch
    // 复用 endpoint + peer_addr_repo,与 presence 共享 NAT/relay 映射;
    // receiver handler 通过 `member_repo` 把 `Connection::remote_id()` 反查
    // 成 DeviceId 再喂给应用层 broadcast。同样必须在 `spawn` 前装。
    let ClipboardHandlers {
        dispatch: clipboard_dispatch,
        receiver: clipboard_receiver,
    } = builder.install_clipboard(
        Arc::clone(&wired.peer_addr_repo),
        Arc::clone(&deps.device.member_repo),
        Arc::clone(&deps.security.fingerprint),
    );
    let clipboard_dispatch: Arc<dyn ClipboardDispatchPort> = clipboard_dispatch;
    let clipboard_receiver: Arc<dyn ClipboardReceiverPort> = clipboard_receiver;
    // Slice 3 Phase 1:同一节点装第四个 ALPN(iroh-blobs)。BlobReference
    // 是 sqlite 仓储,不跟 router 绑定;这里只拿传输 port。
    let BlobHandlers { blob_transfer } = builder
        .install_blobs(wired.iroh_blob_store_dir.clone())
        .await?;
    let iroh_node = builder.spawn();

    // HMAC proof adapter verifies the joiner's ChallengeResponse against
    // the master key that `SpaceAccessPort::derive_master_key_for_proof`
    // stashes in-session. Fed the same `space_access` the use cases use
    // so the cache-miss fallback can still find the current session key.
    let proof_port: Arc<dyn ProofPort> = Arc::new(HmacProofAdapter::new_with_space_access(
        Arc::clone(&deps.security.space_access),
    ));

    let local_identity: Arc<dyn LocalIdentityPort> = identity_store;

    let facade = Arc::new(SpaceSetupFacade::new(SpaceSetupDeps {
        space_access: Arc::clone(&deps.security.space_access),
        local_identity: Arc::clone(&local_identity),
        device_identity: Arc::clone(&deps.device.device_identity),
        member_repo: Arc::clone(&deps.device.member_repo),
        setup_status: Arc::clone(&deps.setup_status),
        settings: Arc::clone(&deps.settings),
        clock: Arc::clone(&deps.system.clock),
        pairing_invitation: handlers.invitation,
        pairing_session: handlers.session,
        pairing_events: handlers.events,
        proof_port,
        trusted_peer_repo: Arc::clone(&wired.trusted_peer_repo),
        peer_addr_repo: Arc::clone(&wired.peer_addr_repo),
        presence: Arc::clone(&presence),
        // Switch-space 4 阶段重加密迁移依赖（commit 4 接入）。`blob_cipher`
        // 复用既有 `EncryptingClipboardEventWriter` /
        // `DecryptingClipboardRepresentationRepository` 同款 adapter Arc，
        // 共享 master_key session。
        migration_state: Arc::clone(&wired.migration_state),
        key_migration: Arc::clone(&wired.key_migration),
        blob_migration_repo: Arc::clone(&wired.blob_migration_repo),
        blob_cipher: Arc::clone(&deps.security.blob_cipher),
    }));

    // Slice 2 Phase 1 · T9:roster 门面和 space_setup facade 共享同一组
    // 实例(`member_repo` / `local_identity` / `presence`),这样 F1 hook
    // 通过 `presence.ensure_reachable_all` 填好的缓存,`list_with_presence`
    // 能直接读到。Facade 本身是纯 thin wrapper,构造非常便宜。
    let roster = Arc::new(MemberRosterFacade::new(MemberRosterDeps {
        member_repo: Arc::clone(&deps.device.member_repo),
        local_identity: Arc::clone(&local_identity),
        presence: Arc::clone(&presence),
    }));

    // Slice 2 Phase 2 · T10:剪切板同步门面。`dispatch_entry` 共享同一份
    // `peer_addr_repo` / `presence` 让 F1 hook 喂的 presence 缓存直接生
    // 效;`transfer_cipher` 与已有 file_transfer 路径同享 V3 chunked
    // AEAD adapter。ingest 后台 loop 立刻起一次,与 receiver handler 同
    // 生命周期(随 `iroh_node.shutdown()` 自然退出 `RecvError::Closed`,
    // `SpaceSetupAssembly::shutdown` 显式 `abort()` 加速过程)。
    let clipboard_sync = Arc::new(ClipboardSyncFacade::new(ClipboardSyncDeps {
        peer_addr_repo: Arc::clone(&wired.peer_addr_repo),
        presence: Arc::clone(&presence),
        transfer_cipher: Arc::clone(&deps.security.transfer_cipher),
        clipboard_dispatch,
        clipboard_receiver,
        device_identity: Arc::clone(&deps.device.device_identity),
        local_identity,
        settings: Arc::clone(&deps.settings),
        clock: Arc::clone(&deps.system.clock),
    }));
    let ingest_handle = clipboard_sync.spawn_ingest_loop();
    let blob = Arc::new(BlobTransferFacade::new(BlobTransferDeps {
        hash: Arc::clone(&deps.system.hash),
        blob_transfer: Arc::clone(&blob_transfer),
        blob_reference: Arc::clone(&wired.blob_reference_repo),
        // 共享同一个 emitter_cell —— daemon bootstrap 注入真实 emitter 后,
        // fetch_blob 就会自动开始向前端发送 progress / status_changed 事件;
        // CLI 模式下 cell 里挂的是 noop emitter,事件被静默吞掉,不影响行为。
        host_event_emitter: Some(Arc::clone(&wired.emitter_cell)),
    }));

    info!("Slice 2/3 SpaceSetupFacade + MemberRosterFacade + ClipboardSyncFacade + BlobTransferFacade assembled");
    Ok(SpaceSetupAssembly {
        facade,
        roster,
        clipboard_sync,
        blob,
        blob_transfer,
        blob_reference: Arc::clone(&wired.blob_reference_repo),
        presence,
        iroh_node,
        ingest_handle,
    })
}
