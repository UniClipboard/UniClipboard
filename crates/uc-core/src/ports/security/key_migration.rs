//! Switch-space 重加密迁移使用的临时密钥能力 port。
//!
//! 流程见 [`crate::setup::migration::MigrationPhase`] 的模块文档。本 port
//! 把"按 `MigrationRunId` 生成 / 加解密 / 销毁一把临时 32 字节密钥"
//! 抽象出来——具体存放在哪里（macOS Keychain / Windows Credential Manager /
//! Linux secret-service / 测试用内存 fake）由 adapter 决定。
//!
//! 与 [`super::blob_cipher::BlobCipherPort`] 的边界：
//! * `BlobCipherPort` 用"已解锁空间"的 master_key 加解密，是数据面常态路径。
//! * `KeyMigrationPort` 用一次性 migration_key 加解密，仅在 switch-space
//!   过渡期使用，结束就销毁。两者算法可以一致（都是 V1 XChaCha20-Poly1305），
//!   但密钥来源、生命周期、调用方完全不同，所以分两个 port。
//!
//! AAD 由调用方提供，与 `BlobCipherPort` 一样原样喂进 AEAD。

use async_trait::async_trait;

use crate::crypto::domain::{Aad, Ciphertext, Plaintext};
use crate::setup::migration::MigrationRunId;

/// `KeyMigrationPort` 操作失败原因。
///
/// 粒度与 `BlobCipherError` 对齐——调用方一般只需要区分"密文坏了"
/// 和"密钥找不到"两类失败，算法细节由 adapter 在日志里补。
#[derive(Debug, thiserror::Error)]
pub enum KeyMigrationError {
    /// `prepare_migration_key` 时指定的 run_id 已经在 secure storage 里
    /// 占位（未通过 `discard_migration_key` 销毁）。adapter 应该让调用方
    /// 显式选择"复用旧 key"或"先销毁再重建"，不能静默覆写。
    #[error("migration key already exists for run_id {0}")]
    AlreadyExists(MigrationRunId),

    /// 在 `*_with_migration_key` 调用里指定的 run_id 找不到对应的密钥。
    /// 通常意味着 keyring 项被外部清理了，或者用了错误的 run_id。
    #[error("migration key not found for run_id {0}")]
    NotFound(MigrationRunId),

    /// 密文损坏 / AAD 不匹配 / AEAD 解包失败——数据层故障，
    /// 与 `BlobCipherError::InvalidCiphertext` 同义。
    #[error("invalid ciphertext or aad mismatch")]
    InvalidCiphertext,

    /// 其它内部失败（keyring API 失败、随机数生成失败等）。
    #[error("key migration internal error: {0}")]
    Internal(String),
}

/// 临时迁移密钥能力。
///
/// 方法契约：
/// * `prepare_migration_key` 生成 32 字节随机密钥，按返回的 `MigrationRunId`
///   持久化到 secure storage，用于跨进程崩溃恢复。
/// * `encrypt_with_migration_key` / `decrypt_with_migration_key` 必须按
///   同一个 `run_id` 找回上面那把密钥；找不到时返回 `NotFound`。
/// * `discard_migration_key` 幂等——重复销毁同一个 `run_id` 不报错，
///   保证 phase 4 cleanup 与启动期补偿路径都能放心调。
#[async_trait]
pub trait KeyMigrationPort: Send + Sync {
    /// 准备一把新的 migration_key 并落盘到 secure storage。返回的
    /// `MigrationRunId` 必须由调用方原样写进 `MigrationStatePort`，否则
    /// 后续 `*_with_migration_key` 调用会找不到这把密钥。
    async fn prepare_migration_key(&self) -> Result<MigrationRunId, KeyMigrationError>;

    /// 用 `run_id` 对应的 migration_key 加密一段明文。aad 透传给 AEAD。
    async fn encrypt_with_migration_key(
        &self,
        run_id: &MigrationRunId,
        plaintext: &Plaintext,
        aad: &Aad,
    ) -> Result<Ciphertext, KeyMigrationError>;

    /// 用 `run_id` 对应的 migration_key 解密一段密文。
    async fn decrypt_with_migration_key(
        &self,
        run_id: &MigrationRunId,
        ciphertext: &Ciphertext,
        aad: &Aad,
    ) -> Result<Plaintext, KeyMigrationError>;

    /// 从 secure storage 销毁 `run_id` 对应的 migration_key。幂等：
    /// 不存在的 run_id 视为已销毁，返回 `Ok(())`。
    async fn discard_migration_key(&self, run_id: &MigrationRunId)
        -> Result<(), KeyMigrationError>;
}
