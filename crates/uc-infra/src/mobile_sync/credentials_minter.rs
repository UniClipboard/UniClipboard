//! `OsRngCredentialsMinter` —— [`MobileCredentialsMinterPort`] 的真实实现
//! (v3 SyncClipboard 兼容版)。
//!
//! 颁发的产物(与 `.context/mobile-sync/SPEC.md` §14.9 + `findings.md` v3 段
//! 落对齐):
//!
//! 1. `username` —— `mobile_<8hex>`,4 字节 OsRng + lowercase hex,在所有
//!    已登记设备中通过 repository 的 UNIQUE 约束保证唯一。8 hex(4 字节)
//!    碰撞概率约 2^-32,设备总数 << 2^16 时碰撞期望 ≈ n^2 / 2^33,实际可忽略;
//!    若仍命中冲突,repository 会回报 `UsernameCollision`,use case 重试一次
//!    即可。
//! 2. `password` —— 16 字节 OsRng + base64 url-safe 无填充(约 22 字符)。
//!    给用户一次性可见,写进 SyncClipboard shortcut 的 `password` 输入框。
//! 3. `password_hash` —— 同步用 Argon2id 算 PHC 字符串(参数对齐
//!    `crate::security::hashing::pin_hash`:m=65536, t=3, p=4)。RFC 9106
//!    推荐内存优先档,登记是用户级动作(秒级延迟可接受),不引入 spawn_blocking,
//!    保持 minter 接口同步。
//! 4. `device_id` —— 16 字节 OsRng + lowercase hex,前缀 `did_`,与
//!    username / password 完全独立熵源。一个泄漏不会带出另一个。
//!
//! 实现是无状态的,可以共享一个 `Arc` 实例在多线程并发调用。

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use rand::rngs::OsRng;
use rand::TryRngCore;

/// Argon2 PHC salt 长度(字节)。SaltString 的 base64 字符串内部就是 16 字节随机
/// 解码后的形式,RFC 9106 推荐 ≥ 8 字节、对密码学常用 16 字节,与 pin_hash 一致。
const SALT_BYTES: usize = 16;

use uc_core::mobile_sync::{MintedCredentials, MobileDeviceId};
use uc_core::ports::MobileCredentialsMinterPort;

/// Argon2id 参数 —— 与 `pin_hash::argon_params` 一致(m=65536, t=3, p=4),
/// 让项目里所有 password-style 哈希共享同一档。
fn argon_params() -> Params {
    Params::new(65536, 3, 4, None).expect("hardcoded Argon2 params must be valid")
}

#[derive(Debug, Default)]
pub struct OsRngCredentialsMinter;

impl MobileCredentialsMinterPort for OsRngCredentialsMinter {
    fn mint_credentials(&self) -> MintedCredentials {
        // 4 bytes → 8 hex chars for username suffix.
        let mut username_bytes = [0u8; 4];
        OsRng
            .try_fill_bytes(&mut username_bytes)
            .expect("OsRng must not fail");
        let username = format!("mobile_{}", hex::encode(username_bytes));

        // 16 bytes → ~22 base64 url-safe (no padding) chars for password.
        let mut password_bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut password_bytes)
            .expect("OsRng must not fail");
        let password = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(password_bytes);

        // Argon2id PHC string. 自己用 rand 0.9 的 OsRng 出 salt 字节,再走
        // SaltString::encode_b64 —— 不调 SaltString::generate,因为它的 RNG
        // bound 仍指向 rand_core 0.6,与项目主用的 rand 0.9 在 lockfile 里
        // 是两份 rand_core(workspace 里多处类似规避,见 pin_hash.rs)。
        let mut salt_bytes = [0u8; SALT_BYTES];
        OsRng
            .try_fill_bytes(&mut salt_bytes)
            .expect("OsRng must not fail");
        let salt = SaltString::encode_b64(&salt_bytes)
            .expect("16-byte salt must encode into a valid PHC SaltString");
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params());
        let password_hash = argon
            .hash_password(password.as_bytes(), &salt)
            .expect("argon2id hash must succeed for fresh credentials")
            .to_string();

        // 16 bytes → 32 hex chars for device id.
        let mut id_bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut id_bytes)
            .expect("OsRng must not fail");
        let device_id = MobileDeviceId::new(format!("did_{}", hex::encode(id_bytes)));

        MintedCredentials {
            username,
            password,
            password_hash,
            device_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    use argon2::password_hash::{PasswordHash, PasswordVerifier};

    #[test]
    fn username_has_expected_shape() {
        let m = OsRngCredentialsMinter;
        let c = m.mint_credentials();
        assert!(c.username.starts_with("mobile_"));
        let suffix = &c.username["mobile_".len()..];
        assert_eq!(suffix.len(), 8, "username suffix must be 8 hex chars");
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(suffix.chars().all(|ch| !ch.is_ascii_uppercase()));
    }

    #[test]
    fn password_is_url_safe_base64_without_padding() {
        let m = OsRngCredentialsMinter;
        let c = m.mint_credentials();
        // 16 字节 → base64 22 字符(no padding)。
        assert_eq!(c.password.len(), 22);
        for ch in c.password.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '-' || ch == '_',
                "url-safe base64 only allows A-Z a-z 0-9 - _"
            );
        }
    }

    #[test]
    fn password_hash_is_phc_argon2id_string_and_verifies() {
        let m = OsRngCredentialsMinter;
        let c = m.mint_credentials();
        assert!(
            c.password_hash.starts_with("$argon2id$"),
            "password_hash must be a PHC argon2id string"
        );

        // Round trip:解析出 PasswordHash 并用同一 password 验证通过。
        let phc = PasswordHash::new(&c.password_hash).expect("phc parse");
        Argon2::default()
            .verify_password(c.password.as_bytes(), &phc)
            .expect("freshly minted password must verify");
    }

    #[test]
    fn device_id_has_prefix_and_is_32_hex_chars() {
        let m = OsRngCredentialsMinter;
        let c = m.mint_credentials();
        let id = c.device_id.as_str();
        assert!(id.starts_with("did_"), "device_id 形式必须为 did_<hex>");
        let suffix = &id["did_".len()..];
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn successive_mints_produce_distinct_outputs() {
        // 弱碰撞防御 —— 16 字节 OsRng 碰撞概率可忽略,连出 6 次必须互不
        // 相同;若失败说明 minter 复用了静态 RNG。Argon2 不快,这里只跑 6
        // 次,够拍 CI 但又不至于显著延长测试时长。
        let m = OsRngCredentialsMinter;
        let mut usernames = HashSet::new();
        let mut passwords = HashSet::new();
        let mut hashes = HashSet::new();
        let mut ids = HashSet::new();
        for _ in 0..6 {
            let c = m.mint_credentials();
            assert!(usernames.insert(c.username.clone()), "username 出现重复");
            assert!(passwords.insert(c.password.clone()), "password 出现重复");
            assert!(hashes.insert(c.password_hash.clone()), "phc 字符串重复");
            assert!(
                ids.insert(c.device_id.as_str().to_string()),
                "device_id 出现重复"
            );
        }
    }
}
