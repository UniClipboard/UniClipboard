//! 集成测试共享 helper：switch-space 4 个新 port 的最小 noop 实现。
//!
//! 现有 slice1/2 e2e 流程不驱动 switch-space，但 `SpaceSetupDeps` 现在
//! 强制要求这 4 个字段；本模块给出 trivial 替身让旧测试继续编译，无需
//! 给每个 e2e 拆出一组拷贝。
//!
//! 这些替身**仅适用于不走 switch-space 路径的测试**。一旦某个测试需要
//! 验证迁移行为，应该换成真实 adapter（`FileMigrationStateRepository` /
//! `DefaultKeyMigrationAdapter` / `DieselBlobMigrationRepository` /
//! `BlobCipherAdapter`）或 mockall 替身。

use std::sync::Arc;

use async_trait::async_trait;

use uc_core::crypto::domain::{Aad, ActiveSpace, Ciphertext, Plaintext};
use uc_core::ids::{EventId, RepresentationId};
use uc_core::ports::clipboard::{BlobMigrationRepoError, BlobMigrationRepoPort, MigrationRecord};
use uc_core::ports::security::{
    BlobCipherError, BlobCipherPort, KeyMigrationError, KeyMigrationPort,
};
use uc_core::ports::setup::{MigrationStateError, MigrationStatePort};
use uc_core::setup::{MigrationPhase, MigrationRunId};

pub struct NoopMigrationState;

#[async_trait]
impl MigrationStatePort for NoopMigrationState {
    async fn get_current(&self) -> Result<Option<MigrationPhase>, MigrationStateError> {
        Ok(None)
    }
    async fn set_current(&self, _phase: Option<MigrationPhase>) -> Result<(), MigrationStateError> {
        Ok(())
    }
}

pub struct NoopKeyMigration;

#[async_trait]
impl KeyMigrationPort for NoopKeyMigration {
    async fn prepare_migration_key(&self) -> Result<MigrationRunId, KeyMigrationError> {
        Ok(MigrationRunId::new("e2e-noop-run"))
    }
    async fn encrypt_with_migration_key(
        &self,
        _run_id: &MigrationRunId,
        plaintext: &Plaintext,
        _aad: &Aad,
    ) -> Result<Ciphertext, KeyMigrationError> {
        Ok(Ciphertext::new(plaintext.as_bytes().to_vec()))
    }
    async fn decrypt_with_migration_key(
        &self,
        _run_id: &MigrationRunId,
        ciphertext: &Ciphertext,
        _aad: &Aad,
    ) -> Result<Plaintext, KeyMigrationError> {
        Ok(Plaintext::new(ciphertext.as_bytes().to_vec()))
    }
    async fn discard_migration_key(
        &self,
        _run_id: &MigrationRunId,
    ) -> Result<(), KeyMigrationError> {
        Ok(())
    }
}

pub struct NoopBlobMigrationRepo;

#[async_trait]
impl BlobMigrationRepoPort for NoopBlobMigrationRepo {
    async fn list_main_inline_representations(
        &self,
    ) -> Result<Vec<(EventId, RepresentationId)>, BlobMigrationRepoError> {
        Ok(Vec::new())
    }
    async fn read_main_inline_data(
        &self,
        _event_id: &EventId,
        _representation_id: &RepresentationId,
    ) -> Result<Option<Vec<u8>>, BlobMigrationRepoError> {
        Ok(None)
    }
    async fn upsert_record(&self, _record: &MigrationRecord) -> Result<(), BlobMigrationRepoError> {
        Ok(())
    }
    async fn count_records(&self) -> Result<u64, BlobMigrationRepoError> {
        Ok(0)
    }
    async fn list_records(&self) -> Result<Vec<MigrationRecord>, BlobMigrationRepoError> {
        Ok(Vec::new())
    }
    async fn update_main_inline_data(
        &self,
        _event_id: &EventId,
        _representation_id: &RepresentationId,
        _new_ciphertext: &[u8],
    ) -> Result<(), BlobMigrationRepoError> {
        Ok(())
    }
    async fn discard_all_records(&self) -> Result<(), BlobMigrationRepoError> {
        Ok(())
    }
}

pub struct NoopBlobCipher;

#[async_trait]
impl BlobCipherPort for NoopBlobCipher {
    async fn encrypt(
        &self,
        _space: &ActiveSpace,
        plaintext: &Plaintext,
        _aad: &Aad,
    ) -> Result<Ciphertext, BlobCipherError> {
        Ok(Ciphertext::new(plaintext.as_bytes().to_vec()))
    }
    async fn decrypt(
        &self,
        _space: &ActiveSpace,
        ciphertext: &Ciphertext,
        _aad: &Aad,
    ) -> Result<Plaintext, BlobCipherError> {
        Ok(Plaintext::new(ciphertext.as_bytes().to_vec()))
    }
}

/// Convenience tuple for splatting all 4 new SpaceSetupDeps fields at once.
///
/// 用法：
/// ```ignore
/// let mig = common::migration_noop_deps();
/// SpaceSetupDeps {
///     // ... 旧字段
///     migration_state: mig.0,
///     key_migration: mig.1,
///     blob_migration_repo: mig.2,
///     blob_cipher: mig.3,
/// }
/// ```
pub fn migration_noop_deps() -> (
    Arc<dyn MigrationStatePort>,
    Arc<dyn KeyMigrationPort>,
    Arc<dyn BlobMigrationRepoPort>,
    Arc<dyn BlobCipherPort>,
) {
    (
        Arc::new(NoopMigrationState),
        Arc::new(NoopKeyMigration),
        Arc::new(NoopBlobMigrationRepo),
        Arc::new(NoopBlobCipher),
    )
}
