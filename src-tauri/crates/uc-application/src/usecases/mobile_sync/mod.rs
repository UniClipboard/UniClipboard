//! 移动端同步相关用例（v1：iOS Shortcut 客户端）。
//!
//! 按 `uc-application/AGENTS.md` §11.4 与 `docs/agent/architecture-rules.md`
//! "Implementation Order" 的要求，每个 use case 文件描述一个用户可感知的
//! 应用动作；外部 crate 经 `crate::facade::mobile_sync::MobileSyncFacade`
//! 访问，不直接 import 这些用例类型。

pub(crate) mod authenticate_request;
pub(crate) mod get_settings;
pub(crate) mod list_devices;
pub(crate) mod list_lan_interfaces;
pub(crate) mod register_device;
pub(crate) mod revoke_device;
pub(crate) mod shortcut_packer;
pub(crate) mod update_settings;
