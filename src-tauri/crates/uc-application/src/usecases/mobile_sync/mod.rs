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
//! - 新增 `apply_incoming`(P5a.3 移动端入站剪贴板, 把 SyncClipboard 协议
//!   的两步 PUT 翻成 V3 envelope 喂给 ApplyInbound 复用整套管线)
//! - 新增 `get_latest_doc`(P5a.4 移动端出站元数据, 把最近一条 paste-priority
//!   rep 翻成 SyncClipboard 协议的 `GET /SyncClipboard.json` 响应形态)
//! - 新增 `get_file`(P5a.5 移动端出站文件字节, 实现 `GET /file/{dataName}`,
//!   按 dataName 匹配最新 entry 的 paste rep, 返回 `(mime, bytes)`)
//! - 新增 `sync_clipboard_mapping`(P5a.5 抽出的 shared helper —— rep ↔
//!   SyncClipboard wire 的 type / dataName 派生规则唯一处)

pub(crate) mod apply_incoming;
pub(crate) mod authenticate_basic;
pub(crate) mod clipboard_doc;
pub(crate) mod get_file;
pub(crate) mod get_latest_doc;
pub(crate) mod get_settings;
pub(crate) mod latest_snapshot_adapter;
pub(crate) mod list_devices;
pub(crate) mod list_lan_interfaces;
pub(crate) mod register_device;
pub(crate) mod revoke_device;
pub(crate) mod rotate_password;
pub(crate) mod sync_clipboard_mapping;
pub(crate) mod update_settings;
