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

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::warn;

use uc_application::facade::{
    BlobTransferDeps, BlobTransferFacade, ClipboardSyncDeps, ClipboardSyncFacade, HostEvent,
    HostEventEmitterPort, IngestHandle, MemberRosterDeps, MemberRosterFacade, SpaceSetupDeps,
    SpaceSetupFacade, TransferHostEvent,
};
use uc_application::proof::HmacProofAdapter;
use uc_core::file_transfer::{FileTransferDirection, OutboundProgressStatus};
use uc_core::ports::blob::{BlobReferenceRepositoryPort, BlobTransferPort};
use uc_core::ports::space::ProofPort;
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardReceiverPort, ConnectionChannelPort, LocalIdentityPort,
    PresencePort,
};
use uc_infra::network::iroh::transfer_progress_adapter::InboundProgressEvent;
use uc_infra::network::iroh::{
    BlobHandlers, ClipboardHandlers, IrohIdentityStore, IrohNode, IrohNodeBuilder, IrohNodeError,
    TransferProgressHandlers, IDENTITY_STORE_KEY,
};
// Re-exported so external callers can parametrise the assembly without
// having to `use uc_infra` themselves.
pub use uc_infra::network::iroh::IrohNodeConfig;
use uc_infra::security::Sha256IdentityFingerprintFactory;
use uc_platform::file_secure_storage::FileSecureStorage;
use uc_platform::migrating_secure_storage::MigratingSecureStorage;

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
    /// 反向"传输进度"翻译 worker 的 join handle。订阅
    /// `IrohTransferProgressAdapter` 的 inbound 流,将每帧 progress 翻译
    /// 为 `HostEvent::Transfer { direction: Sending, ... }` 并发到 emitter。
    /// 与 ingest_handle 同生命周期。
    outbound_progress_translator: JoinHandle<()>,
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
        self.outbound_progress_translator.abort();
        self.facade.on_shutdown().await;
        self.iroh_node.shutdown().await;
    }
}

/// 把接收端推回的进度帧翻译成 `HostEvent::Transfer` 发给 emitter。
///
/// 每帧:
/// * 先发一条 `Progress { direction: Sending }`,前端用它更新 sender 端
///   transfer 进度条 + 文案。
/// * 终态(`Completed` / `Failed`)再补一条 `StatusChanged`,前端把
///   `entryStatusById[transfer_id]` 切到对应状态,UI 退出 transferring。
///
/// transfer_id 字段直接复用帧里的 sender 端 entry_id —— sender 本地
/// entry_id == transfer_id 是发送侧的协议约定(同接收侧约定对称)。
fn spawn_outbound_progress_translator(
    mut rx: broadcast::Receiver<InboundProgressEvent>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let emitter = emitter_cell
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();

                    let progress = HostEvent::Transfer(TransferHostEvent::Progress {
                        transfer_id: event.transfer_id.clone(),
                        entry_id: Some(event.transfer_id.clone()),
                        peer_id: event.from_device.as_str().to_string(),
                        direction: FileTransferDirection::Sending,
                        bytes_transferred: event.bytes_transferred,
                        total_bytes: event.total_bytes,
                    });
                    if let Err(err) = emitter.emit(progress) {
                        warn!(error = %err, "outbound progress translator: emit Progress failed");
                    }

                    let terminal = match event.status {
                        OutboundProgressStatus::InProgress => None,
                        OutboundProgressStatus::Completed => Some(("completed", None)),
                        OutboundProgressStatus::Failed => {
                            Some(("failed", Some("receiver fetch failed".to_string())))
                        }
                    };
                    if let Some((status, reason)) = terminal {
                        let status_event = HostEvent::Transfer(TransferHostEvent::StatusChanged {
                            transfer_id: event.transfer_id.clone(),
                            entry_id: event.transfer_id,
                            status: status.to_string(),
                            reason,
                        });
                        if let Err(err) = emitter.emit(status_event) {
                            warn!(error = %err, "outbound progress translator: emit StatusChanged failed");
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        skipped = n,
                        "outbound progress translator: lagged; some frames skipped"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    })
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
    //
    // **Storage backend separation**: iroh 长期 Ed25519 设备密钥走独立的
    // `FileSecureStorage`(落地 `<app_data>/iroh-identity[_<profile>]/`),不
    // 复用 `deps.security.secure_storage`(即 KEK 用的系统 keychain)。
    //
    // Why: `IrohNodeBuilder::bind` 在应用启动期被调用,会 `ensure_secret_key`
    // → `secure_storage.get/set("iroh-identity:v1")`。如果用 keychain 后端,
    // 这条路径会在用户**没有任何操作**(没点 unlock、没启用 auto-unlock、
    // 没设置加密口令)的情况下触发 macOS keychain 弹窗,违反"keychain 只
    // 在用户解锁/初始化加密时访问"的边界规则。
    //
    // 设备身份密钥不是用户秘密,本身只能用于 P2P 网络握手身份伪冒(且
    // 攻击者还需要 KEK 才能解密剪贴板内容),用 0600 文件 + FileVault
    // 全盘加密保护已足够,与 SSH/IPFS/Tailscale 等同类工具实践一致。
    //
    // Migration (0.6.x → 0.7+): 0.6.x 把 `iroh-identity:v1` 写在系统 keychain
    // (与 KEK 同 service "UniClipboard")。直接换 file backend 会让升级用户
    // 的 iroh 设备身份重置 → 对端 `trusted_peer.peer_fingerprint` 不再匹配
    // → 必须重新走完整 pairing 流程。这里用 `MigratingSecureStorage` 做
    // 一次性迁移装饰:`get` 优先 file,miss 时 fallback 查 keychain,命中
    // 后写 file 并 best-effort 删 keychain。后续 `set` / `delete` 只走 file。
    //
    // 迁移仅作用于 `IDENTITY_STORE_KEY` 白名单——其他 key 的访问永远不会
    // 触碰 keychain,因此 fresh 安装零额外 keychain 调用(平台 NoEntry 不
    // 弹窗);只有"keychain 里恰好有 iroh-identity:v1 条目"的升级路径会
    // 读一次 keychain。在生产签名稳定的 build 上,同应用同 service 的读取
    // 命中已有 ACL 白名单 → 不弹 prompt;最坏情况(codesign drift)弹一次
    // 也比让用户重新配对友好得多。
    //
    // 迁移代码保留至 1.0:确保跳版本升级 (e.g. 0.6.x → 0.7.5 跳过中间版本)
    // 仍能拾起残留的 keychain 条目;清理时机与 0.6.x EOL 对齐。
    let file_backend: Arc<dyn uc_core::ports::SecureStoragePort> = Arc::new(
        FileSecureStorage::with_base_dir(wired.iroh_identity_dir.clone()),
    );
    let iroh_identity_storage: Arc<dyn uc_core::ports::SecureStoragePort> =
        Arc::new(MigratingSecureStorage::new(
            file_backend,
            Arc::clone(&deps.security.secure_storage),
            vec![IDENTITY_STORE_KEY.to_string()],
        ));
    let identity_store = Arc::new(IrohIdentityStore::new(
        iroh_identity_storage,
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
        Arc::clone(&deps.device.member_repo),
        Arc::clone(&deps.security.fingerprint),
        Arc::clone(&deps.system.clock),
    );
    // Phase 96 INDIC-01:连接通道单一真相源。复用同一 endpoint +
    // peer_addr_repo,纯读 adapter 不装 ALPN handler。
    let connection_channel: Arc<dyn ConnectionChannelPort> =
        builder.install_connection_channel(Arc::clone(&wired.peer_addr_repo));
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
    // 反向"传输进度"通道(receiver → sender):同一节点装第四个 ALPN。
    // 装在 install_blobs 之前是为了让 `IrohTransferProgressAdapter` 的
    // reporter 能在 BlobTransferDeps 构造时一起接入 facade。inbound_events
    // 由下面的 translator worker 消费,翻译为 host event。
    let TransferProgressHandlers {
        reporter: outbound_progress_reporter,
        inbound_events: outbound_progress_events,
    } = builder.install_transfer_progress(
        Arc::clone(&wired.peer_addr_repo),
        Arc::clone(&deps.device.member_repo),
        Arc::clone(&deps.security.fingerprint),
    );

    // Slice 3 Phase 1:同一节点装第五个 ALPN(iroh-blobs)。BlobReference
    // 是 sqlite 仓储,不跟 router 绑定;这里只拿传输 port。
    let BlobHandlers { blob_transfer } = builder
        .install_blobs(wired.iroh_blob_store_dir.clone())
        .await?;
    let iroh_node = builder.spawn();

    // Translator worker:从 sender 端的反向通道收 InboundProgressEvent,
    // 翻译为 application 层 HostEvent(Sending 方向)发到 emitter_cell。
    // 每次 progress → `TransferHostEvent::Progress`;终态 → 额外一帧
    // `StatusChanged`。任务跟 ingest_handle 同生命周期,shutdown 显式 abort。
    let outbound_progress_translator = spawn_outbound_progress_translator(
        outbound_progress_events,
        Arc::clone(&wired.emitter_cell),
    );

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
        peer_addr_repo: Arc::clone(&wired.peer_addr_repo),
        trusted_peer_repo: Arc::clone(&wired.trusted_peer_repo),
        local_identity: Arc::clone(&local_identity),
        presence: Arc::clone(&presence),
        connection_channel: Some(Arc::clone(&connection_channel)),
    }));

    // Slice 2 Phase 2 · T10:剪切板同步门面。`dispatch_entry` 共享同一份
    // `peer_addr_repo` / `presence` 让 F1 hook 喂的 presence 缓存直接生
    // 效;`transfer_cipher` 与已有 file_transfer 路径同享 V3 chunked
    // AEAD adapter。ingest 后台 loop 立刻起一次,与 receiver handler 同
    // 生命周期(随 `iroh_node.shutdown()` 自然退出 `RecvError::Closed`,
    // `SpaceSetupAssembly::shutdown` 显式 `abort()` 加速过程)。
    let clipboard_sync = Arc::new(ClipboardSyncFacade::new(ClipboardSyncDeps {
        peer_addr_repo: Arc::clone(&wired.peer_addr_repo),
        member_repo: Arc::clone(&deps.device.member_repo),
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
        // fetch_blob 就会自动开始向前端发送 progress 事件;CLI 模式下 cell
        // 里挂的是 noop emitter,事件被静默吞掉,不影响行为。状态切换
        // (transferring / completed / failed)走 file_transfer lifecycle,
        // 由 `FileTransferHostEventPublisher` 统一发出。
        host_event_emitter: Some(Arc::clone(&wired.emitter_cell)),
        // 反向进度上报端口:接收端 fetch 进度通过新 ALPN 推回 sender。
        outbound_progress_reporter: Some(outbound_progress_reporter),
        // file_transfer lifecycle facade —— iroh 路径每次 fetch 通过它落
        // `Started` / `Completed` / `Failed` 事件,让 file_transfer 表的
        // 状态投影与 sweep / reconcile workers 真正发挥作用。
        file_transfer: Some(Arc::clone(&wired.background.file_transfer_facade)),
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
        outbound_progress_translator,
    })
}
