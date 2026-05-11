//! 移动端同步领域模型(v3 SyncClipboard 兼容版)。
//!
//! 描述移动端客户端(v1: iOS SyncClipboard Clipboard EX)经局域网 HTTP 与
//! 桌面 daemon 同步剪贴板时所需的核心概念:设备身份、Basic Auth 凭据、客户
//! 端类型、LAN 端点等。
//!
//! 本模块只定义"是什么";"怎么做"由 [`crate::ports::mobile_sync`] 中的端口
//! 抽象,以及 `uc-application` / `uc-infra` / `uc-platform` 中的具体实现承担。
//!
//! 设计参考 `.context/mobile-sync/SPEC.md` §14(v3 权威章节)。

pub mod credentials;
pub mod device;
pub mod endpoint;
pub mod lan_interface;
pub mod latest_paste;
pub mod staged_file;

pub use credentials::MintedCredentials;
pub use device::{MobileClientType, MobileDevice, MobileDeviceError, MobileDeviceId};
pub use endpoint::{LanEndpointInfo, LanListenerStatus};
pub use lan_interface::LanInterface;
pub use latest_paste::LatestPasteRepresentation;
pub use staged_file::{StagedFile, StagedFileUri, StagingHandle};
