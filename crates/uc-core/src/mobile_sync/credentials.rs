//! 移动端 Basic Auth 凭据(v3 SyncClipboard 兼容版)。
//!
//! v1/v2 这里曾经放 `TokenHash` + `MintedToken` —— Bearer 签名鉴权用的
//! 32 字节 token + SHA-256 哈希。v3 切到 SyncClipboard 协议后,鉴权改为
//! HTTP Basic Auth(`Authorization: basic base64(username:password)`),原
//! 来的类型整体下线,本文件改放与 Basic Auth 相关的凭据领域类型。

use super::device::MobileDeviceId;

/// `MobileCredentialsMinterPort::mint_credentials` 的成功返回。
///
/// `username` 是稳定唯一标识(在所有已登记设备中唯一),`password` 是给用户
/// 一次性回显的 base64 url-safe 字符串(写进 SyncClipboard shortcut),
/// `password_hash` 是落库用的 Argon2id PHC 字符串。`device_id` 由 minter 一并
/// 生成,绑定本次 minting 的设备身份(保证三者同源生成,避免 use case 自己
/// 拼装时引入竞态 / 重复)。
#[derive(Debug, Clone)]
pub struct MintedCredentials {
    /// 形如 `mobile_<8hex>`(adapter 决定具体长度,这里只描述意图)。
    pub username: String,
    /// 用户一次性可见的明文密码(base64-url-safe 无填充,约 22 字符)。
    pub password: String,
    /// Argon2id PHC 字符串,落库用。
    pub password_hash: String,
    /// 同次 mint 生成的稳定设备 id。
    pub device_id: MobileDeviceId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_credentials_is_constructible() {
        // 锁住字段名 —— 重命名会让 use case 一并 break。
        let m = MintedCredentials {
            username: "mobile_aabbccdd".into(),
            password: "abcdefghij".into(),
            password_hash: "$argon2id$...".into(),
            device_id: MobileDeviceId::new("did_test"),
        };
        assert_eq!(m.username, "mobile_aabbccdd");
        assert_eq!(m.device_id.as_str(), "did_test");
    }
}
