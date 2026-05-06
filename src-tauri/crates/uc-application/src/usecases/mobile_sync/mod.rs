//! 移动端同步相关用例(v1: iOS SyncClipboard Clipboard EX)。
//!
//! 按 `uc-application/AGENTS.md` §11.4 与 `docs/agent/architecture-rules.md`
//! "Implementation Order" 的要求, 每个 use case 文件描述一个用户可感知的
//! 应用动作;外部 crate 经 `crate::facade::mobile_sync::MobileSyncFacade`
//! 访问, 不直接 import 这些用例类型。
//!
//! v3 切到 SyncClipboard 兼容路径后, 用例集合调整:
//! - 删除 `shortcut_packer`(不再维护自建 .shortcut 模板, 用户安装 Apple
//!   签名的 SyncClipboard EX iCloud 链接)
//! - 新增 `authenticate_basic`(LAN HTTP 鉴权热路径, 路由层用)

pub(crate) mod authenticate_basic;
pub(crate) mod get_settings;
pub(crate) mod list_devices;
pub(crate) mod list_lan_interfaces;
pub(crate) mod register_device;
pub(crate) mod revoke_device;
pub(crate) mod update_settings;
