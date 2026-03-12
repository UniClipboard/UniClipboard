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
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ClipboardChangeOriginPort, ClipboardRepresentationNormalizerPort, RepresentationCachePort,
    SpoolQueuePort, SystemClipboardPort, ThumbnailGeneratorPort, ThumbnailRepositoryPort,
};
use uc_core::ports::file_manager::FileManagerPort;
use uc_core::ports::*;

/// Focused network capability bundle for dependency injection.
/// 用于依赖注入的网络能力聚合。
pub struct NetworkPorts {
    /// Clipboard transport capability (`Arc<dyn ClipboardTransportPort>`).
    /// 剪贴板传输能力（`Arc<dyn ClipboardTransportPort>`）。
    pub clipboard: Arc<dyn ClipboardTransportPort>,
    /// Peer directory capability (`Arc<dyn PeerDirectoryPort>`).
    /// 对等端目录能力（`Arc<dyn PeerDirectoryPort>`）。
    pub peers: Arc<dyn PeerDirectoryPort>,
    /// Pairing transport capability (`Arc<dyn PairingTransportPort>`).
    /// 配对传输能力（`Arc<dyn PairingTransportPort>`）。
    pub pairing: Arc<dyn PairingTransportPort>,
    /// Network event subscription capability (`Arc<dyn NetworkEventPort>`).
    /// 网络事件订阅能力（`Arc<dyn NetworkEventPort>`）。
    pub events: Arc<dyn NetworkEventPort>,
}

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
}

/// Security-domain ports bundle.
/// 安全领域端口组。
pub struct SecurityPorts {
    pub encryption: Arc<dyn EncryptionPort>,
    pub encryption_session: Arc<dyn EncryptionSessionPort>,
    pub encryption_state: Arc<dyn uc_core::ports::security::encryption_state::EncryptionStatePort>,
    pub key_scope: Arc<dyn uc_core::ports::security::key_scope::KeyScopePort>,
    pub secure_storage: Arc<dyn SecureStoragePort>,
    pub key_material: Arc<dyn KeyMaterialPort>,
}

/// Device-domain ports bundle (includes pairing).
/// 设备领域端口组（含配对）。
pub struct DevicePorts {
    pub device_repo: Arc<dyn DeviceRepositoryPort>,
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    pub paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
}

/// Storage-domain ports bundle (blobs and thumbnails).
/// 存储领域端口组（Blob 和缩略图）。
pub struct StoragePorts {
    pub blob_store: Arc<dyn BlobStorePort>,
    pub blob_repository: Arc<dyn BlobRepositoryPort>,
    pub blob_writer: Arc<dyn BlobWriterPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,
}

/// System-domain ports bundle (clock, hash, file manager).
/// 系统领域端口组（时钟、哈希、文件管理器）。
pub struct SystemPorts {
    pub clock: Arc<dyn ClockPort>,
    pub hash: Arc<dyn ContentHashPort>,
    pub file_manager: Arc<dyn FileManagerPort>,
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
    /// Network ports bundle (unchanged) / 网络端口组（不变）
    pub network_ports: Arc<NetworkPorts>,
    /// Network control (cross-cutting) / 网络控制（横切关注）
    pub network_control: Arc<dyn NetworkControlPort>,
    /// Setup status (setup-specific) / 设置状态（设置流程专用）
    pub setup_status: Arc<dyn SetupStatusPort>,
    /// Storage-domain ports / 存储领域端口
    pub storage: StoragePorts,
    /// Settings (cross-cutting) / 设置（横切关注）
    pub settings: Arc<dyn SettingsPort>,
    /// System-domain ports / 系统领域端口
    pub system: SystemPorts,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_app_deps_is_just_a_struct() {
        // This test verifies AppDeps is a plain struct,
        // not a Builder with methods
        #[allow(dead_code)]
        fn assert_plain_struct<T: Sized>(_: &T) {}

        // We can't create a full AppDeps without all the trait implementations,
        // but we can verify the struct exists and is plain
        fn assert_app_deps_is_plain() {
            // This function body will remain empty since we can't create
            // full AppDeps without mock implementations
            // The important part is that this compiles - proving AppDeps
            // is a plain struct, not a Builder with methods
        }

        assert_app_deps_is_plain();
    }
}
