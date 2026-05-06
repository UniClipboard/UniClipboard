//! `Argon2idPasswordHasher` —— [`PasswordHasherPort`] 的真实实现。
//!
//! 用 Argon2id PHC 字符串(`$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>`)
//! 作为密码哈希的 wire format。参数与 `crate::security::hashing::pin_hash`
//! 对齐(m=65536, t=3, p=4, parallelism=4),让项目里所有 password-style 哈
//! 希共享同一档 RFC 9106 推荐内存优先档。
//!
//! 鉴权热路径(`verify`)是 LAN HTTP 每次 PUT/GET 都会跑一次 —— Argon2 在
//! 当前参数档下单次验证 ~50-100ms,iPhone 一次手动同步可接受。但要避免
//! 阻塞 tokio worker:用 `tokio::task::spawn_blocking` 把 CPU 密集计算搬
//! 到 blocking 线程池。port 本身已经是 async,这是落地里的一个内部细节。

use std::sync::Arc;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use async_trait::async_trait;
use rand::rngs::OsRng;
use rand::TryRngCore;

use uc_core::ports::{PasswordHasherError, PasswordHasherPort};

/// PHC salt 长度(字节)。详见 `credentials_minter::SALT_BYTES` 的注释。
const SALT_BYTES: usize = 16;

/// Argon2id 参数 —— 与 `pin_hash::argon_params` 一致。
fn argon_params() -> Params {
    Params::new(65536, 3, 4, None).expect("hardcoded Argon2 params must be valid")
}

#[derive(Debug, Default, Clone)]
pub struct Argon2idPasswordHasher;

#[async_trait]
impl PasswordHasherPort for Argon2idPasswordHasher {
    async fn hash(&self, password: &str) -> Result<String, PasswordHasherError> {
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || {
            // 自己用 rand 0.9 的 OsRng 出 salt,再走 SaltString::encode_b64 ——
            // SaltString::generate 仍用 rand_core 0.6 的 trait bound,与
            // workspace 主用的 rand 0.9 不直通(同样规避见 pin_hash.rs)。
            let mut salt_bytes = [0u8; SALT_BYTES];
            OsRng
                .try_fill_bytes(&mut salt_bytes)
                .map_err(|e| PasswordHasherError::Internal(format!("OsRng fill failed: {e}")))?;
            let salt = SaltString::encode_b64(&salt_bytes)
                .map_err(|e| PasswordHasherError::Internal(format!("salt encode failed: {e}")))?;
            let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params());
            argon
                .hash_password(password.as_bytes(), &salt)
                .map(|h| h.to_string())
                .map_err(|e| PasswordHasherError::Internal(format!("argon2 hash failed: {e}")))
        })
        .await
        .map_err(|e| PasswordHasherError::Internal(format!("spawn_blocking join failed: {e}")))?
    }

    async fn verify(&self, password: &str, phc: &str) -> Result<bool, PasswordHasherError> {
        let password = password.to_owned();
        let phc = phc.to_owned();
        tokio::task::spawn_blocking(move || {
            let parsed = PasswordHash::new(&phc)
                .map_err(|e| PasswordHasherError::InvalidPhc(e.to_string()))?;
            // verify_password 在不匹配时返回 Err(Error::Password),其它错误
            // 表示 phc 字符串损坏或参数不识别 —— 后者翻译为 InvalidPhc 让上
            // 层把记录视为"需要重新登记",而不是误判为"密码错"。
            match Argon2::default().verify_password(password.as_bytes(), &parsed) {
                Ok(()) => Ok(true),
                Err(argon2::password_hash::Error::Password) => Ok(false),
                Err(e) => Err(PasswordHasherError::InvalidPhc(e.to_string())),
            }
        })
        .await
        .map_err(|e| PasswordHasherError::Internal(format!("spawn_blocking join failed: {e}")))?
    }
}

/// 给 bootstrap / wiring 用的便捷别名。
pub type SharedPasswordHasher = Arc<Argon2idPasswordHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify_round_trips() {
        let h = Argon2idPasswordHasher;
        let phc = h.hash("hunter2").await.unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(h.verify("hunter2", &phc).await.unwrap());
        assert!(!h.verify("wrong", &phc).await.unwrap());
    }

    #[tokio::test]
    async fn invalid_phc_returns_invalid_phc_error() {
        let h = Argon2idPasswordHasher;
        let err = h.verify("anything", "not-a-phc-string").await.unwrap_err();
        assert!(matches!(err, PasswordHasherError::InvalidPhc(_)));
    }

    #[tokio::test]
    async fn distinct_hashes_for_same_password_due_to_salt() {
        let h = Argon2idPasswordHasher;
        let p1 = h.hash("samepw").await.unwrap();
        let p2 = h.hash("samepw").await.unwrap();
        assert_ne!(p1, p2, "same password 必须每次出不同 PHC(随机 salt)");
        assert!(h.verify("samepw", &p1).await.unwrap());
        assert!(h.verify("samepw", &p2).await.unwrap());
    }
}
