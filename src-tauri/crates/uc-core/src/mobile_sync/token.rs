//! 移动端同步的 token 哈希与 minting 输出。
//!
//! Token 的真实字节由 `MobileTokenMinterPort` 的 adapter 生成（v1: 32 字节
//! OsRng）。本模块只定义"持久化时存什么"以及"minting 调用方拿到什么"的
//! 数据形态 —— 不掺杂算法实现。

use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

use super::device::MobileDeviceId;

/// Token 的 SHA-256 哈希 —— 32 字节定长。
///
/// 持久化与跨进程传输统一用这个 newtype，避免误把 `[u8; 32]` 当成"任意 32
/// 字节缓冲"处理。`Serialize` / `Deserialize` 走 base64 表示，便于直接落
/// JSON / TOML，而不是数组的 `[u8;32]` 字面量。
#[serde_as]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenHash(#[serde_as(as = "Base64")] [u8; 32]);

impl TokenHash {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// `MobileTokenMinterPort::mint_token` 的成功返回。
///
/// `raw_hex` 是给用户看 + 写进 `.shortcut` 的 64 字符 hex；`hash` 是落库
/// 用的 SHA-256；`device_id` 由 minter 一并生成，绑定本次 minting 的设备
/// 身份（保证 token 与 device_id 同源生成，避免 use case 自己拼装时引入
/// 竞态 / 重复）。
#[derive(Debug, Clone)]
pub struct MintedToken {
    pub raw_hex: String,
    pub hash: TokenHash,
    pub device_id: MobileDeviceId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_byte_addressable() {
        let bytes = [7u8; 32];
        let h = TokenHash::new(bytes);
        assert_eq!(h.as_bytes(), &bytes);
        assert_eq!(h.into_bytes(), bytes);
    }
}
