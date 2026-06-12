//! LAN 监听端点信息(v3 SyncClipboard 兼容版)。
//!
//! v1/v2 这里曾经放 `ShortcutDownloadToken` + `RegisteredDownloadToken`,
//! 用作"登记设备 → iPhone Safari 一次性下载注入 token 的 .shortcut"中间
//! 凭据。v3 切到 SyncClipboard 兼容路径后,iPhone 直接安装 SyncClipboard
//! 项目现成的 iCloud 共享链接,不再有自建模板下载流程,这两个类型整体下线。
//!
//! 本文件描述"daemon 当前监听在哪个 LAN URL 上"以及配套的运行时状态枚举。

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

/// LAN listener 的运行时状态。
///
/// 是 `MobileSyncEndpointInfoPort::current_status` 的返回值,把 daemon 的
/// listener 子任务实际处于"未起 / 已就位 / 启动失败"哪一种显式表达出来。
///
/// 历史:旧 port 只返回 `Option<LanEndpointInfo>`,导致"用户开了 lanListen
/// 但 daemon 端 bind 失败"和"用户根本没开"两种语义在 view 上无法区分,UI
/// 只能用文案"等待 daemon 重启"含糊覆盖,bind 失败被悄悄吞掉。新增此 enum
/// 让失败原因能从 daemon → adapter → use case → view → UI 完整冒泡。
///
/// 设计取舍:
/// - `BindFailed.reason` 用 `String` 而非另一个 enum —— `std::io::Error` 信息
///   面向用户排障已经够用(`Address already in use` / `Cannot assign requested
///   address` / `Permission denied`),无需在 core 层引入新错误分类。
/// - core 不导入 tokio / std::net 类型,只持有 String,符合"领域不依赖技术"
///   原则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanListenerStatus {
    /// listener 当前未运行。包含两种业务场景:
    /// 1. `enabled` 或 `lan_listen_enabled` 为 false → daemon 启动时不起 listener;
    /// 2. daemon 还没启动 / 已停止 → adapter 初始即此态。
    /// UI 不需要区分,显示"未开启"语义即可。
    Stopped,

    /// listener 已成功 bind 并对外服务。`endpoint` 是实际的 `http://ip:port`。
    Listening(LanEndpointInfo),

    /// listener 启动失败(典型:端口占用 / IP 不存在 / 权限不足)。`reason` 是
    /// adapter 从底层错误格式化出的人话字符串,直接显示给用户排障。
    BindFailed { reason: String },
}

impl LanListenerStatus {
    /// 便捷:把状态降级为旧 port 形态的 `Option<LanEndpointInfo>`,只在
    /// `Listening` 时返回 `Some`。提供它是为了让仍依赖旧形态的代码点(如
    /// `current_lan_endpoint` 默认转发实现)无需自己 match。
    pub fn endpoint(&self) -> Option<&LanEndpointInfo> {
        match self {
            Self::Listening(ep) => Some(ep),
            Self::Stopped | Self::BindFailed { .. } => None,
        }
    }
}
