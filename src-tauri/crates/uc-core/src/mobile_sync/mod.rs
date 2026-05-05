//! 移动端同步领域模型。
//!
//! 描述移动端客户端（v1: iOS Apple Shortcuts）经局域网 HTTP 与桌面 daemon
//! 同步剪贴板时所需的核心概念：设备身份、token 哈希、客户端类型、LAN 端点
//! 描述、一次性下载凭据等。
//!
//! 本模块只定义"是什么"；"怎么做"由 [`crate::ports::mobile_sync`] 中的端口
//! 抽象，以及 `uc-application` / `uc-infra` / `uc-platform` 中的具体实现承担。
//!
//! 设计参考 `.context/mobile-sync/SPEC.md`。

pub mod device;
pub mod endpoint;
pub mod token;

pub use device::{MobileClientType, MobileDevice, MobileDeviceError, MobileDeviceId};
pub use endpoint::{LanEndpointInfo, RegisteredDownloadToken, ShortcutDownloadToken};
pub use token::{MintedToken, TokenHash};
