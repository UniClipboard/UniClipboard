//! # Application Dependencies / 应用依赖
//!
//! This module defines the dependency grouping for App construction.
//! 此模块定义 App 构造的依赖分组。
//!
//! **Note / 注意**: This is NOT a Builder pattern.
//! **这不是 Builder 模式。**
//! - No build steps / 无构建步骤
//! - No default values / 无默认值
//! - No hidden logic / 无隐藏逻辑
//! - Just parameter grouping / 仅用于参数打包

use std::sync::Arc;
use tokio::sync::mpsc;
use uc_core::blob::ports::{BlobReaderPort, BlobWriterPort};
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ClipboardChangeOriginPort, ClipboardPayloadResolverPort, ClipboardRepresentationNormalizerPort,
    DeleteClipboardEntryPort, FindEntryIdBySnapshotHashPort, GetClipboardEntryPort,
    GetRepresentationByBlobIdPort, GetRepresentationPort, ListClipboardEntriesPort,
    ListRepresentationsForEventPort, RepresentationCachePort, SaveClipboardEntryPort,
    SpoolQueuePort, SystemClipboardPort, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
    TouchClipboardEntryPort, UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::search::search_pipeline::SearchPipelinePort;
use uc_core::ports::space::{
    CurrentSessionProofKeyPort, DeriveProofKeyPort, DeriveSpaceSubkeyPort, FactoryResetSpacePort,
    InitializeSpacePort, IsSpaceUnlockedPort, LockSpacePort, PrepareJoinOfferPort,
    ResumeSpaceSessionPort, UnlockSpacePort, VerifyKeychainAccessPort,
};
use uc_core::ports::*;
use uc_core::MemberRepositoryPort;
use uc_observability::analytics::AnalyticsPort;

/// Clipboard entry intent ports.
///
/// The composition root coerces the single Diesel entry adapter into each of
/// these narrow ports; a consumer declares only the capability it calls.
#[derive(Clone)]
pub struct ClipboardEntryPorts {
    pub get: Arc<dyn GetClipboardEntryPort>,
    pub list: Arc<dyn ListClipboardEntriesPort>,
    pub save: Arc<dyn SaveClipboardEntryPort>,
    pub touch: Arc<dyn TouchClipboardEntryPort>,
    pub delete: Arc<dyn DeleteClipboardEntryPort>,
    pub find_by_snapshot_hash: Arc<dyn FindEntryIdBySnapshotHashPort>,
}

/// Clipboard representation intent ports facing the application layer.
///
/// The decrypting decorator is coerced into each of these. Background payload
/// workers keep the wider inner store; the application layer sees only this
/// read-plus-processing slice.
#[derive(Clone)]
pub struct ClipboardRepresentationPorts {
    pub get: Arc<dyn GetRepresentationPort>,
    pub get_by_blob_id: Arc<dyn GetRepresentationByBlobIdPort>,
    pub list_for_event: Arc<dyn ListRepresentationsForEventPort>,
    pub update_processing_result: Arc<dyn UpdateRepresentationProcessingResultPort>,
}

/// Clipboard-domain ports bundle.
/// 剪贴板领域端口组。
#[derive(Clone)]
pub struct ClipboardPorts {
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,
    pub entry_ports: ClipboardEntryPorts,
    pub clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    /// Inner representation store (the full aggregate surface). Threaded by the
    /// composition root to the background payload workers only; the application
    /// layer depends on `representation_ports` instead.
    pub representation_store: Arc<dyn ClipboardRepresentationStore>,
    pub representation_ports: ClipboardRepresentationPorts,
    pub representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    pub representation_cache: Arc<dyn RepresentationCachePort>,
    pub spool_queue: Arc<dyn SpoolQueuePort>,
    pub clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    pub worker_tx: mpsc::Sender<RepresentationId>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
}

/// Narrow space-access intent ports facing the application layer.
///
/// The composition root coerces one space-access adapter into each of these;
/// every consumer takes only the slice it needs, never a catch-all surface
/// (ports.md §8.1/§8.3). Distribution hubs (facade dep bundles) carry the
/// whole struct and hand each use case its slice.
#[derive(Clone)]
pub struct SpaceAccessPorts {
    pub initialize: Arc<dyn InitializeSpacePort>,
    pub unlock: Arc<dyn UnlockSpacePort>,
    pub is_unlocked: Arc<dyn IsSpaceUnlockedPort>,
    pub lock: Arc<dyn LockSpacePort>,
    pub factory_reset: Arc<dyn FactoryResetSpacePort>,
    pub resume_session: Arc<dyn ResumeSpaceSessionPort>,
    pub verify_keychain_access: Arc<dyn VerifyKeychainAccessPort>,
    pub derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    pub current_session_proof_key: Arc<dyn CurrentSessionProofKeyPort>,
    pub prepare_join_offer: Arc<dyn PrepareJoinOfferPort>,
    pub derive_proof_key: Arc<dyn DeriveProofKeyPort>,
}

impl SpaceAccessPorts {
    /// Fan one concrete adapter — implementing every narrow space-access intent
    /// port — out into the bundle. Used by the composition root and by
    /// integration tests that build a single adapter (ports.md §8.3).
    pub fn from_adapter<A>(adapter: Arc<A>) -> Self
    where
        A: InitializeSpacePort
            + UnlockSpacePort
            + IsSpaceUnlockedPort
            + LockSpacePort
            + FactoryResetSpacePort
            + ResumeSpaceSessionPort
            + VerifyKeychainAccessPort
            + DeriveSpaceSubkeyPort
            + CurrentSessionProofKeyPort
            + PrepareJoinOfferPort
            + DeriveProofKeyPort
            + 'static,
    {
        Self {
            initialize: adapter.clone(),
            unlock: adapter.clone(),
            is_unlocked: adapter.clone(),
            lock: adapter.clone(),
            factory_reset: adapter.clone(),
            resume_session: adapter.clone(),
            verify_keychain_access: adapter.clone(),
            derive_subkey: adapter.clone(),
            current_session_proof_key: adapter.clone(),
            prepare_join_offer: adapter.clone(),
            derive_proof_key: adapter,
        }
    }
}

/// Security-domain ports bundle.
/// 安全领域端口组。
#[derive(Clone)]
pub struct SecurityPorts {
    pub current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
    pub secure_storage: Arc<dyn SecureStoragePort>,
    /// Narrow space-access intent ports (initialize / unlock / lock / resume /
    /// factory-reset / keychain-probe / subkey / proof-key, etc.). Each
    /// consumer depends on the slice it calls; nothing holds a catch-all
    /// space-access surface.
    pub space_access_ports: SpaceAccessPorts,
    /// 业务 blob 加解密 port——4 个剪切板 decorator 通过此 port 加解密
    /// inline_data。adapter 内部端到端自管会话与 V1 AEAD。
    pub blob_cipher: Arc<dyn uc_core::ports::security::BlobCipherPort>,
    /// 剪切板传输 AEAD port——`uc_application::usecases::clipboard_sync` 的
    /// `dispatch_entry` / `ingest_inbound` 通过此 port 加解密 V3 网络字节。
    /// adapter 内部端到端自管会话。
    pub transfer_cipher: Arc<dyn uc_core::ports::security::TransferCipherPort>,
    /// Argon2 PIN hasher for pairing.
    pub pin_hasher: Arc<dyn uc_core::ports::security::PinHasherPort>,
    /// Short pairing-code derivation.
    pub short_code: Arc<dyn uc_core::ports::security::ShortCodeGeneratorPort>,
    /// Identity-fingerprint factory used by pairing.
    pub fingerprint: Arc<dyn uc_core::ports::security::IdentityFingerprintFactoryPort>,
}

/// Device-domain ports bundle (includes pairing).
/// 设备领域端口组（含配对）。
#[derive(Clone)]
pub struct DevicePorts {
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    /// Authoritative repository of admitted space members (phase 4b PR-4：
    /// `paired_device_repo` 已下线，成员身份与同步偏好的唯一持久层)。
    pub member_repo: Arc<dyn MemberRepositoryPort>,
}

/// Receiver-side file-transfer projection ports (ADR-009).
///
/// The composition root coerces the single Diesel adapter into these intent
/// ports; each downstream consumer takes only the slice it needs, never the
/// whole bundle.
#[derive(Clone)]
pub struct FileTransferPorts {
    pub record: Arc<dyn RecordReceiverTransferPort>,
    pub entry_summary: Arc<dyn GetEntryTransferSummaryPort>,
    pub find_entry_id: Arc<dyn FindEntryIdForTransferPort>,
    pub list_expired: Arc<dyn ListExpiredInflightTransfersPort>,
    pub fail_inflight: Arc<dyn FailInflightTransfersPort>,
}

/// Storage-domain ports bundle (blobs, thumbnails, file transfer tracking).
/// 存储领域端口组（Blob、缩略图、文件传输追踪）。
#[derive(Clone)]
pub struct StoragePorts {
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub blob_writer: Arc<dyn BlobWriterPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
    pub file_transfer: FileTransferPorts,
}

/// Search-domain ports bundle.
///
/// Groups the three search infrastructure pieces that must travel together:
/// the index port (query + CRUD), the key derivation port (HMAC term tags),
/// and the pipeline port (tokenization + text extraction). Keeping them in
/// one bundle prevents uc-daemon code from constructing these pieces ad hoc.
#[derive(Clone)]
pub struct SearchPorts {
    /// Encrypted search index: query, index_entry, remove_entry, rebuild.
    pub search_index: Arc<dyn SearchIndexPort>,
    /// HMAC search key derivation (profile-scoped, HKDF-SHA256).
    pub search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    /// Tokenization + text extraction pipeline used for building search documents.
    pub search_pipeline: Arc<dyn SearchPipelinePort>,
}

/// System-domain ports bundle (clock, hash, cache filesystem).
/// 系统领域端口组（时钟、哈希、缓存文件系统）。
#[derive(Clone)]
pub struct SystemPorts {
    pub clock: Arc<dyn ClockPort>,
    pub hash: Arc<dyn ContentHashPort>,
    pub cache_fs: Arc<dyn uc_core::ports::cache_fs::CacheFsPort>,
}

/// Mobile-sync 领域端口组。
///
/// 装配 daemon listener / facade 需要共享的"有外部资源 / 跨主体共享"的
/// 端口。无外部资源的进程内 adapter(`token_minter` / `download_tokens` /
/// `lan_interface_probe` 等)在 `MobileSyncFacade` 装配处就地构造,无需穿过
/// `AppDeps`。
///
/// `endpoint_info` 是只读视图:daemon LAN listener 启停时通过自己持有的具体
/// adapter 类型 (`Arc<InMemoryMobileSyncEndpointInfoAdapter>` 旁路) 调 `set` /
/// `clear` 来更新这份状态;facade 通过本字段读它,看到 daemon 当前真实绑定
/// 的 LAN URL。两端共享同一 `Arc<InMemoryMobileSyncEndpointInfoAdapter>`,通过
/// unsizing coercion 转成 trait object。
/// Registered-device intent ports facing the application layer.
///
/// The composition root coerces one Diesel device-repository adapter into each
/// of these; each consumer takes only the slice it needs, never the whole
/// aggregate store (see ports.md §8.3).
#[derive(Clone)]
pub struct MobileDevicePorts {
    pub find_by_username: Arc<dyn FindMobileDeviceByUsernamePort>,
    pub find_by_id: Arc<dyn FindMobileDeviceByIdPort>,
    pub list: Arc<dyn ListMobileDevicesPort>,
    pub save: Arc<dyn SaveMobileDevicePort>,
    pub delete: Arc<dyn DeleteMobileDevicePort>,
    pub update: Arc<dyn UpdateMobileDevicePort>,
}

#[derive(Clone)]
pub struct MobileSyncPorts {
    pub devices: MobileDevicePorts,
    pub endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
}

/// Application dependency grouping (non-Builder, just parameter grouping)
/// 应用依赖分组（非 Builder，仅参数打包）
///
/// **NOT a Builder pattern** - this is just a struct to group parameters.
/// **不是 Builder 模式** - 这只是一个打包参数的结构体。
///
/// All dependencies are required - no defaults, no optional fields.
/// 所有依赖都是必需的 - 无默认值，无可选字段。
///
/// `Clone` is derived because every field bottoms out at `Arc<dyn Port>`
/// or `mpsc::Sender` —— cloning is cheap, and lets process-level callers
/// hand the same wired set to both `DesktopRuntime` and the in-process
/// daemon-lifecycle装配 without re-running `wire_dependencies`.
#[derive(Clone)]
pub struct AppDeps {
    /// Clipboard-domain ports / 剪贴板领域端口
    pub clipboard: ClipboardPorts,
    /// Security-domain ports / 安全领域端口
    pub security: SecurityPorts,
    /// Device-domain ports (includes pairing) / 设备领域端口（含配对）
    pub device: DevicePorts,
    /// Setup status (setup-specific) / 设置状态（设置流程专用）
    pub setup_status: Arc<dyn SetupStatusPort>,
    /// 整机配置迁移 facade（导出 / 导入预览 / 暂存导入）。
    ///
    /// 与其它抽象 port 不同,这里直接携带组装好的 facade:它的依赖
    /// (db_pool / local_identity / profile_id 等)只在 `wire_dependencies`
    /// 的同步上下文里齐全,无法仅凭 `AppDeps` 里的抽象 port 在
    /// `build_app_facade_from_deps` 处重新组装。因此在 wiring 处构造好后随
    /// `AppDeps` 流转,与 `setup_status` 一样是"携带即用"的句柄。
    pub config_migration: Arc<crate::facade::ConfigMigrationFacade>,
    /// 升级游标端口：持久化"上次运行的应用版本"。
    /// 由 `UpgradeFacade::detect_on_startup` 在启动期读取并比较。
    pub app_version_state: Arc<dyn AppVersionStatePort>,
    /// 首次同步事件去重端口：持久化"是否已 fire 过 `first_clipboard_sync_*` /
    /// `first_file_sync_succeeded`"flag。outbound `dispatch_entry` 在 fan-out
    /// 每个 peer 的 spawn 内 mark + 条件 fire；race 防护由 port impl 内部
    /// `tokio::sync::Mutex` 守护，调用方只关心 `Ok(true)` 才 fire。
    pub first_sync_state: Arc<dyn FirstSyncStatePort>,
    /// Storage-domain ports / 存储领域端口
    pub storage: StoragePorts,
    /// Settings (cross-cutting) / 设置（横切关注）
    pub settings: Arc<dyn SettingsPort>,
    /// System-domain ports / 系统领域端口
    pub system: SystemPorts,
    /// Search-domain ports (index, key derivation, pipeline) / 搜索领域端口
    pub search: SearchPorts,
    /// Mobile-sync 领域端口 / Mobile sync domain ports.
    pub mobile_sync: MobileSyncPorts,
    /// 产品 telemetry 上报 sink（横切关注点）。
    ///
    /// bootstrap 装配时用 [`uc_observability::analytics::GatedAnalyticsSink`]
    /// 包一层真实 sink（dev=`StdoutSink`、release=`NoopAnalyticsSink`，未来
    /// 接 `PosthogSink`），调用方只需 `analytics.capture(event)`，不必自己
    /// 查 `usage_analytics_enabled`——wrapper 内部已 atomic 守卫。
    pub analytics: Arc<dyn AnalyticsPort>,
}
