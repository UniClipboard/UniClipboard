//! Wiring output bundles.
//!
//! The internal data types produced by engine dependency assembly:
//! the process-resident `WiredDependencies` plus the consumer-grouped bundles
//! (`SyncEngineDeps` / `DaemonRuntimeDeps` / `SharedRuntimeDeps`) and the
//! one-shot `BackgroundRuntimeDeps`. These carry no behavior — the wiring logic
//! that fills them lives in `wiring::wire`.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc};

use uc_application::deps::AppDeps;
use uc_core::clipboard::ActiveClipboardState;
use uc_core::ids::RepresentationId;
use uc_core::ports::blob::BlobReferenceRepositoryPort;
use uc_infra::clipboard::{RepresentationCache, SpoolManager};
use uc_observability_contract::analytics::AnalyticsFacade;

/// Result type for wiring operations
pub type WiringResult<T> = Result<T, WiringError>;

/// Errors during dependency injection
#[derive(Debug, thiserror::Error)]
pub enum WiringError {
    #[error("Database initialization failed: {0}")]
    DatabaseInit(String),

    #[error("Clipboard initialization failed: {0}")]
    ClipboardInit(String),

    #[error("Blob storage initialization failed: {0}")]
    BlobStorageInit(String),

    #[error("Settings repository initialization failed: {0}")]
    SettingsInit(String),

    #[error("Thumbnail generator initialization failed: {0}")]
    ThumbnailInit(String),
}

/// Background runtime components that must be started after async runtime is ready.
pub struct BackgroundRuntimeDeps {
    pub representation_cache: Arc<RepresentationCache>,
    pub spool_manager: Arc<SpoolManager>,
    pub worker_rx: mpsc::Receiver<RepresentationId>,
    pub spool_dir: PathBuf,
    pub spool_ttl_days: u64,
    pub worker_retry_max_attempts: u32,
    pub worker_retry_backoff_ms: u64,
    /// Event-sourced file transfer lifecycle: receiver-side projection
    /// plumbing + sweep/reconcile runtime tasks. Holds a clone of the shared
    /// `host_event_bus` so it automatically picks up emitters registered
    /// later (Tauri webview, daemon WS). The 5 lifecycle actions
    /// (start/report_progress/complete/fail/cancel) live inside the
    /// `file_transfer_facade` carried on [`WiredDependencies`].
    pub file_transfer_lifecycle: Arc<crate::assembly::file_transfer::FileTransferLifecycle>,
}

/// P2P / iroh sync-engine assembly inputs. Sole consumer:
/// The internal sync-engine assembly consumes these ports and paths. They never
/// flow through `AppDeps` — the `SpaceSetupFacade` they assemble lives in
/// uc-application and is injected by this bundle at wire time, not by the
/// AppFacade path.
#[derive(Clone)]
pub struct SyncEngineDeps {
    /// peer address repo — best-effort transport-address writes after pairing,
    /// dialed by F1 `ensure_reachable_all`.
    pub peer_addr_repo: Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
    /// plaintext-hash → ciphertext-digest dedupe cache (Slice 3 Phase 1).
    pub blob_reference_repo: Arc<dyn BlobReferenceRepositoryPort>,
    /// switch-space backup table + main-table inline_data batch IO.
    pub blob_migration_repo: Arc<dyn uc_core::ports::clipboard::BlobMigrationRepoPort>,
    /// switch-space re-encryption migration stage persistence
    /// (`.migration_state` file alongside `.setup_status`).
    pub migration_state: Arc<dyn uc_core::ports::setup::MigrationStatePort>,
    /// switch-space one-shot migration_key keyring management.
    pub key_migration: Arc<dyn uc_core::ports::security::KeyMigrationPort>,
    /// iroh-blobs store dir, used when assembling the iroh blob handler.
    pub iroh_blob_store_dir: PathBuf,
    /// Application-facing analytics entry point (pairing / switch-space events).
    pub analytics_facade: Arc<dyn AnalyticsFacade>,
}

/// daemon main-loop-only bypass deps.
#[cfg(feature = "lan-compat")]
#[derive(Clone)]
pub struct DaemonRuntimeDeps {
    /// Mobile-sync LAN endpoint-state singleton. **Concrete type**, not a trait
    /// object: the daemon LAN listener calls inherent `set` / `clear` on it
    /// (write side), which are not on the read-only `MobileSyncEndpointInfoPort`.
    /// The same Arc is also coerced into `AppDeps.mobile_sync.endpoint_info`
    /// (facade read side), sharing one allocation — daemon writes, facade reads
    /// (ports.md §8.3 single-adapter-reuse).
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
}

/// Process-level handles shared by ≥2 assembly targets (space-setup,
/// daemon-runtime, CLI-appfacade). Grouped into a named "shared" bundle rather
/// than left top-level because "shared by multiple targets" is itself the
/// meaningful boundary; mirrors the [`BackgroundRuntimeDeps`] precedent.
#[derive(Clone)]
pub struct SharedRuntimeDeps {
    pub receive_readiness: Arc<uc_application::receive_reconciliation::ReceiveReadinessCoordinator>,
    /// Shared host-event bus created at wire time with the "logging" emitter
    /// already registered (event type names → `tracing::debug`), so non-GUI /
    /// CLI processes have a sensible default transport. Callers register their
    /// own transports on top; all consumers fan out into whatever transports
    /// are currently registered.
    pub host_event_bus: Arc<uc_application::facade::HostEventBus>,
    /// Delivery-result repo: `ClipboardSyncFacade` writes on fan-out completion,
    /// the view side reads.
    pub entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort>,
    /// Read port over the same Diesel impl as
    /// `AppDeps.clipboard.clipboard_event_repo`; the view layer resolves the
    /// source device through it.
    pub clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort>,
    /// Application entry point for the file-transfer lifecycle actions + seed +
    /// link. Shared by daemon runtime, `MobileSyncFacade` assembly, and the iroh
    /// blob path in `build_sync_engine_assembly`.
    pub file_transfer_facade: Arc<uc_application::facade::FileTransferFacade>,
    /// Single write boundary for all programmatic clipboard writes (guard
    /// registration + write + cleanup-on-error). Shared so the active-clipboard
    /// inbound worker and the restore/capture path keep one circuit-breaker +
    /// origin-guard state.
    pub clipboard_write_coordinator:
        Arc<uc_application::clipboard_write::ClipboardWriteCoordinator>,
    /// Local cache dir for inbound blob materialization
    /// (`<file_cache_dir>/iroh-blobs/<entry_id>/`).
    pub file_cache_dir: PathBuf,
    /// Trusted-peer repository — pairing persist boundary (D19), roster trust
    /// checks, dispatch target filtering, CLI resend source lookup. Read by
    /// space-setup, daemon runtime, and the CLI AppFacade path, hence shared.
    pub trusted_peer_repo: Arc<dyn uc_core::TrustedPeerRepositoryPort>,
    /// Fan-out source for active-clipboard register advances. `BroadcastingAdvance`
    /// (wired into `active_clipboard_register`) is the sole publisher; the
    /// mobile-sync LAN SSE endpoint is the sole subscriber, cloning a `Receiver`
    /// per connection. Shared because both the wire-time decorator and the
    /// daemon's mobile-sync listener assembly need a handle to the same sender.
    pub active_clipboard_sse_source: broadcast::Sender<ActiveClipboardState>,
}

/// 进程级一次性装配产出的"持久"部分:进程内常驻的 `deps` 与按消费者归类的
/// 旁路 bundle(`sync_engine` / `daemon_runtime` / `shared`)。
///
/// 一次性消费的 [`BackgroundRuntimeDeps`](含 blob worker receiver)通过
/// 通过 tuple 返回值单独移交,不嵌在这里 —— 因为 mpsc
/// `Receiver` 不可 Clone。
///
/// 只被 daemon 进程路径消费(`apps/daemon` process_bootstrap → host →
/// bootstrap,加上 uc-bootstrap 的两个 assembler)。GUI/Tauri shell 走
/// `uc_desktop::gui_wiring::build_gui_client_context` 的 daemon HTTP client 路径,**不**碰
/// `WiredDependencies`;fan-out 是进程内 `ProcessRuntimeHandles` clone。
///
/// `Clone` 派生:所有字段都是 `Arc<dyn Port>` / `PathBuf` / Clone-able 嵌套
/// bundle,clone 廉价。
#[derive(Clone)]
pub struct WiredDependencies {
    /// 应用层 facade 装配输入(查询/历史/加密/搜索)。喂给
    /// `build_app_facade_from_deps`;CLI 与 daemon 路径共用。
    pub deps: AppDeps,
    /// P2P / iroh sync-engine assembly inputs (see [`SyncEngineDeps`]).
    pub sync_engine: SyncEngineDeps,
    /// daemon main-loop-only bypass deps (see [`DaemonRuntimeDeps`]).
    #[cfg(feature = "lan-compat")]
    pub daemon_runtime: DaemonRuntimeDeps,
    /// Process-level handles shared by ≥2 assembly targets (see
    /// [`SharedRuntimeDeps`]).
    pub shared: SharedRuntimeDeps,
}
