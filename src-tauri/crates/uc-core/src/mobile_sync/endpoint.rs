//! LAN 监听端点信息，以及一次性下载 token 的领域模型。
//!
//! 二者都属于"daemon 当前的运行态投影"：endpoint 描述 daemon 现在监听
//! 在哪个 LAN URL 上；download token 是登记 iPhone 设备时颁发的、用于
//! Safari 一次性下载已注入 token 的 `.shortcut` 的临时凭据。

use serde::{Deserialize, Serialize};

/// 当前 daemon 暴露给 iPhone 的 LAN 端点。
///
/// `url` 已含协议 + host + port，如 `http://192.168.1.5:42720`。客户端拿到
/// 后直接 append 路径即可。当 daemon 未启用 LAN 监听时，调用方会收到
/// `None` 而非空字符串，避免拼出畸形 URL。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanEndpointInfo {
    pub url: String,
}

/// 一次性下载凭据（短 TTL）。
///
/// 客户端用它从 `/mobile/v1/shortcut/install?dt=<token>` 拉取已注入 token
/// 的 `.shortcut` 二进制。被消费一次后即作废；TTL 到期未使用也作废。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShortcutDownloadToken(String);

impl ShortcutDownloadToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ShortcutDownloadToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 注册成功后返回的下载凭据 + 过期时刻。
///
/// `expires_at_ms` 是 Unix 毫秒 —— 服务端为权威，客户端 / UI 仅用于展示
/// 倒计时。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredDownloadToken {
    pub token: ShortcutDownloadToken,
    pub expires_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_download_token_round_trip_through_str() {
        let t = ShortcutDownloadToken::new("dt_xyz");
        assert_eq!(t.as_str(), "dt_xyz");
        assert_eq!(t.to_string(), "dt_xyz");
        assert_eq!(t.clone().into_string(), "dt_xyz");
    }
}
