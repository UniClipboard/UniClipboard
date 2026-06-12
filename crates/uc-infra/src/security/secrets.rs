//! 运行时密钥物料类型(`MasterKey` / `Kek`)——uc-infra 内部。
//!
//! Slice 4 (B.4.5) 起从 uc-core 物理下沉:这两个 newtype 只在 adapter 层
//! 存在(KeyMaterialStore / InMemorySession / v1_aead / chunked_transfer / ...),
//! 从不出现在磁盘或 wire 格式中——uc-core 不再暴露它们。
//!
//! - `MasterKey` 是 DEK(Data Encryption Key),32 字节,随机生成,会话持有。
//! - `Kek` 是 Key Encryption Key,从 passphrase + Argon2id 派生,仅用于
//!   包装/解包 `MasterKey`,使用后立即丢弃。
//!
//! 注:两种类型均不实现 `Serialize` / `Deserialize`,防止不小心序列化到磁盘。
//!
//! 内存卫生:两种类型都派生 `ZeroizeOnDrop`,被 drop 时(包括 `session.clear()`、
//! `set_master_key` 替换旧值、短生命周期克隆/派生 KEK 用完丢弃等路径)32 字节
//! 密钥就地清零,降低进程内存快照 / crash dump / swap 残留中残留密钥物料的概率。

use rand::{rngs::OsRng, TryRngCore};
use std::fmt;
use uc_core::crypto::model::EncryptionError;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The data-encryption key (DEK) used to encrypt clipboard blobs.
///
/// - 32 bytes is suitable for XChaCha20-Poly1305 / AES-256-GCM keys.
/// - Do NOT implement Serialize/Deserialize.
/// - Drops zero out the inner bytes via `ZeroizeOnDrop`.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey([u8; 32]);

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MasterKey([REDACTED])")
    }
}

impl MasterKey {
    pub const LEN: usize = 32;

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn generate() -> Result<Self, EncryptionError> {
        let mut buf = [0u8; Self::LEN];
        OsRng
            .try_fill_bytes(&mut buf)
            .map_err(|_| EncryptionError::CryptoFailure)?;
        Self::from_bytes(&buf)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptionError> {
        if bytes.len() != Self::LEN {
            return Err(EncryptionError::InvalidParameter(format!(
                "invalid MasterKey length: expected {}, got {}",
                Self::LEN,
                bytes.len()
            )));
        }
        let mut mk_bytes = [0u8; Self::LEN];
        mk_bytes.copy_from_slice(bytes);
        Ok(MasterKey(mk_bytes))
    }

    /// 消费 self 取出原始字节,只在必须移交所有权(如把字节交给 `ProofDerivedKey`
    /// 这种已经自身负责 zeroize 的目标类型)的极少数路径上使用。
    pub(crate) fn into_bytes(self) -> [u8; 32] {
        // 拷贝出去后 self 仍会被 drop,届时 self.0 会被自身 ZeroizeOnDrop 清零;
        // 调用方持有的副本由调用方负责保护。
        self.0
    }
}

/// The key-encryption key (KEK) derived from passphrase via KDF.
/// KEK is used ONLY to wrap/unwrap the MasterKey.
///
/// Keep KEK ephemeral (avoid long-lived storage). Drops zero out the inner
/// bytes via `ZeroizeOnDrop`.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Kek([u8; 32]);

impl fmt::Debug for Kek {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Kek([REDACTED])")
    }
}

impl Kek {
    pub const LEN: usize = 32;

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptionError> {
        if bytes.len() != Self::LEN {
            return Err(EncryptionError::InvalidParameter(format!(
                "invalid KEK length: expected {}, got {}",
                Self::LEN,
                bytes.len()
            )));
        }
        let mut kek_bytes = [0u8; Self::LEN];
        kek_bytes.copy_from_slice(bytes);
        Ok(Kek(kek_bytes))
    }
}
