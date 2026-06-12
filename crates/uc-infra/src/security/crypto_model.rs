//! 加密持久化/协议数据模型——uc-infra 内部。
//!
//! 从 `uc-core/src/crypto/model.rs` 物理下沉(Slice 6 U6 候选 F + Slice 7):
//! `KdfParams` / `KdfParamsV1` / `KeySlot` / `WrappedMasterKey` / `EncryptedBlob` /
//! `KeySlotFile` / `KeySlotConvertError` / `KeyScope` 都是磁盘 JSON 与 pairing
//! wire format 的承载结构(ironclad 数据兼容),属于 uc-infra 的持久化职责。
//! uc-core 仅保留 `Passphrase` / `EncryptionError`。
//!
//! serde 形状与历史完全一致——字段名、类型、可选性、skip_serializing_if 行为
//! 字节级保留。`KeyScope` 序列化形状仍是 `{"profile_id":"..."}`——磁盘
//! `KeySlotFile.scope` JSON 嵌套对象字节级兼容。

use chrono::{DateTime, Utc};
use rand::{rngs::OsRng, TryRngCore};
use serde::{Deserialize, Serialize};
use uc_core::crypto::model::EncryptionError;

/// 单 profile 下 KEK/KeySlot 的作用域键。
///
/// Slice 7 (U7 候选 B) 起从 uc-core 搬到 uc-infra——`KeyScopePort` 已改名
/// `CurrentProfilePort` 返回 `ProfileId` 值对象,`KeyScope` 仅作为 adapter
/// 层内部 wrapper 服务于 `KeySlotFile.scope` 字段的磁盘 JSON 兼容 + `kek_key`
/// 前缀格式化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyScope {
    /// Profile ID (user profile)
    pub profile_id: String,
}

/// KDF params
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// KDF 算法名——当前仅支持 `"Argon2id"`,其它值触发 `UnsupportedKdfAlgorithm`。
    pub alg: String,
    pub params: KdfParamsV1,
}

impl KdfParams {
    pub fn for_initialization() -> Self {
        Self {
            alg: "Argon2id".to_string(),
            params: KdfParamsV1::default(),
        }
    }

    pub fn salt_len(&self) -> usize {
        match self.alg.as_str() {
            "Argon2id" => 16,
            _ => 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParamsV1 {
    /// Argon2id (example semantics):
    /// - mem_kib: memory cost in KiB
    /// - iters: time cost (iterations)
    /// - parallelism: lanes/threads
    pub mem_kib: u32,
    pub iters: u32,
    pub parallelism: u32,
}

impl Default for KdfParamsV1 {
    fn default() -> Self {
        Self {
            mem_kib: 128 * 1024, // 128 MB
            iters: 3,
            parallelism: 4,
        }
    }
}

/// KeySlot (persistent; no passphrase, no plaintext keys)
///
/// KeySlot 持久化派生 KEK 所需的参数,以及被包装的 MasterKey。
///
/// 解锁逻辑:
/// 1) derive KEK from passphrase + salt + kdf params
/// 2) unwrap MasterKey from wrapped_master_key
/// 3) store MasterKey in session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySlot {
    /// KeySlot 版本——当前仅支持 `"V1"`,其它值触发 `UnsupportedKeySlotVersion`。
    pub version: String,
    pub scope: KeyScope,
    pub kdf: KdfParams,
    pub salt: Vec<u8>,
    /// MasterKey encrypted (wrapped) by KEK.
    pub wrapped_master_key: Option<WrappedMasterKey>,
}

impl KeySlot {
    pub fn draft_v1(scope: KeyScope) -> Result<Self, EncryptionError> {
        let kdf = KdfParams::for_initialization();
        let mut salt = vec![0u8; kdf.salt_len()];
        OsRng
            .try_fill_bytes(&mut salt)
            .map_err(|_| EncryptionError::CryptoFailure)?;

        Ok(Self {
            version: "V1".to_string(),
            scope,
            kdf,
            salt,
            wrapped_master_key: None,
        })
    }

    pub fn finalize(self, wrapped_master_key: WrappedMasterKey) -> Self {
        Self {
            wrapped_master_key: Some(wrapped_master_key),
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedMasterKey {
    pub blob: EncryptedBlob,
}

/// Encrypted blob container (for disk storage / wrapped key)
///
/// 通用 AEAD 容器,用于:
/// - 包装/解包 MasterKey (KEK 加密 MasterKey)
/// - 加密/解密 clipboard blobs (MasterKey 加密明文)
///
/// 注意:
/// - nonce 长度取决于算法
///   - XChaCha20-Poly1305: 24 bytes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// 格式版本——当前仅支持 `"V1"`,其它值触发 `UnsupportedBlobVersion`。
    pub version: String,
    /// AEAD 算法名——当前仅支持 `"XChaCha20Poly1305"`,其它值视为密文损坏。
    pub aead: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,

    /// Optional: store a short hash/fingerprint of AAD (NOT the AAD itself)
    /// to help debugging "wrong AAD" vs "wrong key" scenarios.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aad_fingerprint: Option<Vec<u8>>,
}

impl EncryptedBlob {
    pub fn validate_basic(&self) -> Result<(), EncryptionError> {
        if self.aead != "XChaCha20Poly1305" || self.nonce.len() != 24 {
            return Err(EncryptionError::InvalidParameter(format!(
                "invalid nonce length for {:?}: {}",
                self.aead,
                self.nonce.len()
            )));
        }

        if self.ciphertext.is_empty() {
            return Err(EncryptionError::InvalidParameter(
                "ciphertext is empty".into(),
            ));
        }

        if self.version != "V1" {
            return Err(EncryptionError::UnsupportedBlobVersion);
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeySlotConvertError {
    #[error("wrapped master key is missing")]
    MissingWrappedMasterKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySlotFile {
    /// KeySlot 版本——当前仅支持 `"V1"`,其它值触发 `UnsupportedKeySlotVersion`。
    pub version: String,
    pub scope: KeyScope,
    pub kdf: KdfParams,
    pub salt: Vec<u8>,
    pub wrapped_master_key: EncryptedBlob,

    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl TryFrom<&KeySlot> for KeySlotFile {
    type Error = KeySlotConvertError;

    fn try_from(ks: &KeySlot) -> Result<Self, Self::Error> {
        let wrapped_master_key = ks
            .wrapped_master_key
            .clone()
            .ok_or(KeySlotConvertError::MissingWrappedMasterKey)?;

        Ok(KeySlotFile {
            version: ks.version.clone(),
            scope: ks.scope.clone(),
            kdf: ks.kdf.clone(),
            salt: ks.salt.clone(),
            wrapped_master_key: wrapped_master_key.blob.clone(),
            created_at: None,
            updated_at: None,
        })
    }
}

impl From<KeySlotFile> for KeySlot {
    fn from(ksf: KeySlotFile) -> Self {
        KeySlot {
            version: ksf.version,
            scope: ksf.scope,
            kdf: ksf.kdf,
            salt: ksf.salt,
            wrapped_master_key: Some(WrappedMasterKey {
                blob: ksf.wrapped_master_key,
            }),
        }
    }
}
