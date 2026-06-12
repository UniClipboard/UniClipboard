//! Security / Encryption domain models.
//!
//! 本模块最终保留的跨 crate 领域符号:
//! - `Passphrase`: 用户提供的解锁口令;uc-application / cli 等领域输入类型
//! - `EncryptionError`: 跨 crate 错误类型(uc-infra `KeySlotStore` port 等返回)
//!
//! 其余符号已物理下沉到 `uc-infra/src/security/`:
//! - `Kek` / `MasterKey`  → `secrets.rs`(Slice 4 B.4.5)
//! - `KdfParams` / `KdfParamsV1` / `KeySlot` / `WrappedMasterKey` /
//!   `EncryptedBlob` / `KeySlotFile` / `KeySlotConvertError` / `KeyScope`
//!   → `crypto_model.rs`(Slice 6-7)
//!
//! `KeyScopePort` → `CurrentProfilePort`(Slice 7 U7 候选 B),返回
//! `uc_core::ids::ProfileId` 值对象。

use std::fmt;

/// Passphrase provided by user. Only used to derive KEK inside use cases.
/// Avoid storing this beyond the unlock/initialize flow.
#[derive(Clone)]
pub struct Passphrase(pub String);

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Passphrase([REDACTED])")
    }
}

impl Passphrase {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    #[error("encryption is not initialized")]
    NotInitialized,

    #[error("encryption is locked")]
    Locked,

    #[error("wrong passphrase")]
    WrongPassphrase,

    #[error("unsupported keyslot version")]
    UnsupportedKeySlotVersion,

    #[error("unsupported blob format version")]
    UnsupportedBlobVersion,

    #[error("corrupted keyslot data")]
    CorruptedKeySlot,

    #[error("corrupted encrypted blob")]
    CorruptedBlob,

    #[error("internal crypto failure")]
    CryptoFailure,

    #[error("invalid key")]
    InvalidKey,

    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("KDF operation failed")]
    KdfFailed,

    #[error("unsupported KDF algorithm")]
    UnsupportedKdfAlgorithm,

    #[error("encryption failed")]
    EncryptFailed,
    /// Keyring / Key Material errors

    #[error("key material not found")]
    KeyNotFound, // keyring 或 keyslot 缺失

    #[error("key material is corrupt")]
    KeyMaterialCorrupt, // keyslot 或 keyring 内容损坏/长度不对/反序列化失败

    #[error("other encryption error: {0}")]
    KeyringError(String),

    #[error("permission denied for key material access")]
    PermissionDenied, // keyring 权限/系统拒绝

    #[error("I/O failure during key material access")]
    IoFailure, // 文件/DB IO

    #[error("unsupported version for key material")]
    UnsupportedVersion, // keyslot/blob 版本不支持
}
