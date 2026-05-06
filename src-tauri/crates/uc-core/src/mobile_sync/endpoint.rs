//! LAN 监听端点信息(v3 SyncClipboard 兼容版)。
//!
//! v1/v2 这里曾经放 `ShortcutDownloadToken` + `RegisteredDownloadToken`,
//! 用作"登记设备 → iPhone Safari 一次性下载注入 token 的 .shortcut"中间
//! 凭据。v3 切到 SyncClipboard 兼容路径后,iPhone 直接安装 SyncClipboard
//! 项目现成的 iCloud 共享链接,不再有自建模板下载流程,这两个类型整体下线。
//!
//! 本文件保留只描述"daemon 当前监听在哪个 LAN URL 上"。

use serde::{Deserialize, Serialize};

/// 当前 daemon 暴露给 iPhone 的 LAN 端点。
///
/// `url` 已含协议 + host + port,如 `http://192.168.1.5:42720`。SyncClipboard
/// shortcut 用户在客户端里手动填入这个 URL 作为 `{base}`,后续每次请求
/// daemon 都拼成 `{base}/SyncClipboard.json` / `{base}/file/{name}`。当
/// daemon 未启用 LAN 监听时,调用方会收到 `None` 而非空字符串,避免拼出畸形 URL。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanEndpointInfo {
    pub url: String,
}
