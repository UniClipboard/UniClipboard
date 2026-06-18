//! 移动端同步(v1: iOS SyncClipboard Clipboard EX)的端口实现。
//!
//! 本模块对应 `uc-core::mobile_sync` + `uc-core::ports::mobile_sync` 的全
//! 套 adapter(v3 SyncClipboard 兼容版):
//! - `credentials_minter`:`MobileCredentialsMinterPort` 的 OsRng + Argon2id
//!   实装(单次原子颁发 username / password / password_hash / device_id)
//! - `password_hasher`:`PasswordHasherPort` 的 Argon2id PHC 实装,鉴权热路
//!   径走它做 verify
//! - `device_repo`(`#[cfg(test)]` 限定):`MobileDeviceStore` 的进
//!   程内实装,仅作为本 crate 单测的轻量替身。生产路径走
//!   `db::repositories::DieselMobileDeviceRepository`
//! - `endpoint_info`:`MobileSyncEndpointInfoPort` 的 in-memory adapter,
//!   daemon listener 启停时旁路写入它
//! - `lan_probe`:`LanInterfaceProbePort` 的真实 OS 实装

pub mod credentials_minter;
#[cfg(test)]
pub mod device_repo;
pub mod endpoint_info;
pub mod file_staging;
pub mod lan_probe;
pub mod password_hasher;

pub use credentials_minter::OsRngCredentialsMinter;
#[cfg(test)]
pub use device_repo::InMemoryMobileDeviceRepository;
pub use endpoint_info::InMemoryMobileSyncEndpointInfoAdapter;
pub use file_staging::FilesystemMobileFileStaging;
pub use lan_probe::NetworkInterfaceLanProbe;
pub use password_hasher::{Argon2idPasswordHasher, SharedPasswordHasher};
