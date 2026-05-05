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
    RepresentationCachePort, SpoolQueuePort, SystemClipboardPort, ThumbnailGeneratorPort,
    ThumbnailRepositoryPort,
};
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::search::search_pipeline::SearchPipelinePort;
use uc_core::ports::*;
use uc_core::ports::{MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort};
use uc_core::MemberRepositoryPort;

/// Clipboard-domain ports bundle.
/// 剪贴板领域端口组。
pub struct ClipboardPorts {
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,
    pub clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    pub clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    pub representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    pub representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    pub representation_cache: Arc<dyn RepresentationCachePort>,
    pub spool_queue: Arc<dyn SpoolQueuePort>,
    pub clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    pub worker_tx: mpsc::Sender<RepresentationId>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
}

/// Security-domain ports bundle.
/// 安全领域端口组。
pub struct SecurityPorts {
    pub current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
    pub secure_storage: Arc<dyn SecureStoragePort>,
    /// 单一空间访问 port——initialize / unlock / try_resume_session /
    /// verify_keychain_access / derive_subkey / current_session_proof_key 等
    /// 业务动作的统一入口。所有会话/密钥访问都从这里走。
    pub space_access: Arc<dyn uc_core::ports::space::SpaceAccessPort>,
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
pub struct DevicePorts {
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    /// Authoritative repository of admitted space members (phase 4b PR-4：
    /// `paired_device_repo` 已下线，成员身份与同步偏好的唯一持久层)。
    pub member_repo: Arc<dyn MemberRepositoryPort>,
}

/// Storage-domain ports bundle (blobs, thumbnails, file transfer tracking).
/// 存储领域端口组（Blob、缩略图、文件传输追踪）。
pub struct StoragePorts {
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub blob_writer: Arc<dyn BlobWriterPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
    pub file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
}

/// Search-domain ports bundle.
///
/// Groups the three search infrastructure pieces that must travel together:
/// the index port (query + CRUD), the key derivation port (HMAC term tags),
/// and the pipeline port (tokenization + text extraction). Keeping them in
/// one bundle prevents uc-daemon code from constructing these pieces ad hoc.
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
/// adapter 类型(`SharedEndpointInfo` 旁路)调 `set` / `clear` 来更新这份
/// 状态;facade 通过本字段读它,看到 daemon 当前真实绑定的 LAN URL。两端
/// 共享同一 `Arc<InMemoryMobileSyncEndpointInfoAdapter>`,通过 unsizing
/// coercion 转成 trait object。
pub struct MobileSyncPorts {
    pub device_repo: Arc<dyn MobileDeviceRepositoryPort>,
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
pub struct AppDeps {
    /// Clipboard-domain ports / 剪贴板领域端口
    pub clipboard: ClipboardPorts,
    /// Security-domain ports / 安全领域端口
    pub security: SecurityPorts,
    /// Device-domain ports (includes pairing) / 设备领域端口（含配对）
    pub device: DevicePorts,
    /// Setup status (setup-specific) / 设置状态（设置流程专用）
    pub setup_status: Arc<dyn SetupStatusPort>,
    /// 升级游标端口：持久化"上次运行的应用版本"。
    /// 由 `UpgradeFacade::detect_on_startup` 在启动期读取并比较。
    pub app_version_state: Arc<dyn AppVersionStatePort>,
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
}
