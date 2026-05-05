//! `OsRngSha256MobileTokenMinter` —— [`MobileTokenMinterPort`] 的真实实现。
//!
//! 颁发的产物（与 SPEC §6.1 的 token 模型一致）：
//!
//! 1. `raw_hex` —— 32 字节 OsRng + 小写 hex 编码（64 字符）。
//!    iPhone 端会把它原样写进 `.shortcut` 模板的 `__UC_TOKEN__` 占位。
//! 2. `hash` —— 对原始 32 字节做一次 SHA-256，作为服务端鉴权的索引主键。
//! 3. `device_id` —— 另起一组 16 字节 OsRng + hex 编码，前缀 `did_`，与
//!    token 完全独立熵源。这样 token 暴露不会泄漏 device_id（反之亦然）。
//!
//! 这里没有 trait 之外的副作用：实现是无状态的，可以共享一个 `Arc` 实例
//! 在多线程并发调用。

use rand::rngs::OsRng;
use rand::TryRngCore;
use sha2::{Digest, Sha256};

use uc_core::mobile_sync::{MintedToken, MobileDeviceId, TokenHash};
use uc_core::ports::MobileTokenMinterPort;

/// OsRng + SHA-256 + hex 实现。
///
/// 故意没 `new` —— 类型本身是 ZST，构造时直接 `Arc::new(
/// OsRngSha256MobileTokenMinter)` 即可。
#[derive(Debug, Default)]
pub struct OsRngSha256MobileTokenMinter;

impl MobileTokenMinterPort for OsRngSha256MobileTokenMinter {
    fn mint_token(&self) -> MintedToken {
        // 32 bytes 给 token 原文。
        let mut token_bytes = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut token_bytes)
            .expect("OsRng must not fail");

        let raw_hex = hex::encode(token_bytes);

        // SHA-256 over raw bytes —— 服务端只存哈希。
        let mut hasher = Sha256::new();
        hasher.update(token_bytes);
        let digest = hasher.finalize();
        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(&digest);
        let hash = TokenHash::new(hash_arr);

        // 16 字节给 device_id —— 32 hex 字符，前缀 `did_`（与 SPEC §6.1
        // 的 `did_<32hex>` 形式对齐）。
        let mut id_bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut id_bytes)
            .expect("OsRng must not fail");
        let device_id = MobileDeviceId::new(format!("did_{}", hex::encode(id_bytes)));

        MintedToken {
            raw_hex,
            hash,
            device_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    #[test]
    fn raw_hex_is_64_lowercase_hex_chars() {
        let m = OsRngSha256MobileTokenMinter;
        let t = m.mint_token();
        assert_eq!(t.raw_hex.len(), 64, "raw_hex 必须 64 个 hex 字符");
        assert!(
            t.raw_hex.chars().all(|c| c.is_ascii_hexdigit()),
            "raw_hex 必须全部是 hex"
        );
        assert!(
            t.raw_hex.chars().all(|c| !c.is_ascii_uppercase()),
            "raw_hex 必须小写"
        );
    }

    #[test]
    fn hash_matches_sha256_of_raw_bytes() {
        let m = OsRngSha256MobileTokenMinter;
        let t = m.mint_token();

        // 把 raw_hex 解回 bytes，重新 SHA-256，应与 hash 一致。
        let raw_bytes = hex::decode(&t.raw_hex).expect("raw_hex 必须是合法 hex");
        let mut hasher = Sha256::new();
        hasher.update(&raw_bytes);
        let expected = hasher.finalize();
        assert_eq!(t.hash.as_bytes(), expected.as_slice());
    }

    #[test]
    fn device_id_has_prefix_and_is_32_hex_chars() {
        let m = OsRngSha256MobileTokenMinter;
        let t = m.mint_token();
        let id = t.device_id.as_str();
        assert!(id.starts_with("did_"), "device_id 形式必须为 did_<hex>");
        let suffix = &id["did_".len()..];
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn successive_mints_produce_distinct_outputs() {
        // 弱碰撞防御 —— 32 字节 OsRng 碰撞概率可忽略，连出 8 次必须互不
        // 相同；如果失败说明 minter 接错（例如复用了 Default 单例的内部
        // 状态）。
        let m = OsRngSha256MobileTokenMinter;
        let mut hexes = HashSet::new();
        let mut ids = HashSet::new();
        for _ in 0..8 {
            let t = m.mint_token();
            assert!(hexes.insert(t.raw_hex.clone()), "raw_hex 出现重复");
            assert!(
                ids.insert(t.device_id.as_str().to_string()),
                "device_id 出现重复"
            );
        }
    }
}
