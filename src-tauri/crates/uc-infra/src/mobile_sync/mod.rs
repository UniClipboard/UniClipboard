//! 移动端同步（v1：iOS Shortcut）的端口实现。
//!
//! 本模块对应 `uc-core::mobile_sync` + `uc-core::ports::mobile_sync` 的全
//! 套 adapter。Phase 2 内统一以"in-memory + 真实 OS 探测"形态落地，便于
//! daemon / CLI 端到端跑通；持久化（`SqliteMobileDeviceRepository` 等）留
//! 给后续 commit 替换 trait 不变。

pub mod device_repo;
pub mod download_token_store;
pub mod endpoint_info;
pub mod lan_probe;
pub mod token_minter;

pub use device_repo::InMemoryMobileDeviceRepository;
pub use download_token_store::InMemoryShortcutDownloadTokenStore;
pub use endpoint_info::{InMemoryMobileSyncEndpointInfoAdapter, SharedEndpointInfo};
pub use lan_probe::NetworkInterfaceLanProbe;
pub use token_minter::OsRngSha256MobileTokenMinter;
