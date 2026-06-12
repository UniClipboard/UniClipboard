//! [`BlobReferenceRepositoryPort`] 的 Diesel 实现。
//!
//! 每个明文 hash 保存一条明文 hash → 密文 digest 映射。hash 以十六进制
//! 字符串落库，方便直接查看 sqlite dump。

use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;

use uc_core::ports::blob::{
    BlobDigest, BlobReferenceError, BlobReferenceRepositoryPort, PlaintextHash,
};

use crate::db::mappers::blob_reference_mapper::BlobReferenceRowMapper;
use crate::db::models::BlobReferenceRow;
use crate::db::ports::DbExecutor;
use crate::db::schema::blob_reference::dsl::*;

pub struct DieselBlobReferenceRepository<E> {
    executor: E,
    mapper: BlobReferenceRowMapper,
}

impl<E> DieselBlobReferenceRepository<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            mapper: BlobReferenceRowMapper,
        }
    }
}

#[async_trait]
impl<E> BlobReferenceRepositoryPort for DieselBlobReferenceRepository<E>
where
    E: DbExecutor,
{
    async fn find_by_plaintext_hash(
        &self,
        hash: &PlaintextHash,
    ) -> Result<Option<BlobDigest>, BlobReferenceError> {
        let hash_hex = hex::encode(hash.as_bytes());
        let row = self
            .executor
            .run(move |conn| {
                blob_reference
                    .filter(plaintext_hash.eq(&hash_hex))
                    .first::<BlobReferenceRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| BlobReferenceError::Repository(e.to_string()))?;

        row.as_ref()
            .map(|r| self.mapper.digest_from_row(r))
            .transpose()
            .map_err(|e| BlobReferenceError::Repository(e.to_string()))
    }

    async fn save(
        &self,
        hash: PlaintextHash,
        digest_value: BlobDigest,
    ) -> Result<(), BlobReferenceError> {
        let row = self
            .mapper
            .to_row(hash, digest_value, Utc::now().timestamp());
        self.executor
            .run(move |conn| {
                diesel::insert_into(blob_reference)
                    .values(&row)
                    .on_conflict(plaintext_hash)
                    .do_update()
                    .set((digest.eq(row.digest.clone()), created_at.eq(row.created_at)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| BlobReferenceError::Repository(e.to_string()))
    }

    async fn forget(&self, hash: &PlaintextHash) -> Result<(), BlobReferenceError> {
        let hash_hex = hex::encode(hash.as_bytes());
        self.executor
            .run(move |conn| {
                diesel::delete(blob_reference.filter(plaintext_hash.eq(&hash_hex)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| BlobReferenceError::Repository(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use tempfile::{tempdir, TempDir};

    fn make_repo() -> (DieselBlobReferenceRepository<DieselSqliteExecutor>, TempDir) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("blob-reference.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let repo = DieselBlobReferenceRepository::new(DieselSqliteExecutor::new(pool));
        (repo, tempdir)
    }

    fn hash(byte: u8) -> PlaintextHash {
        PlaintextHash::from_bytes([byte; 32])
    }

    fn digest(byte: u8) -> BlobDigest {
        BlobDigest::from_bytes([byte; 32])
    }

    #[tokio::test]
    async fn save_then_find_returns_digest() {
        let (repo, _tempdir) = make_repo();
        let plaintext = hash(0x11);
        let ciphertext = digest(0xaa);

        repo.save(plaintext, ciphertext).await.unwrap();

        let loaded = repo.find_by_plaintext_hash(&plaintext).await.unwrap();
        assert_eq!(loaded, Some(ciphertext));
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (repo, _tempdir) = make_repo();

        let loaded = repo.find_by_plaintext_hash(&hash(0x22)).await.unwrap();

        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn save_same_hash_is_last_write_wins() {
        let (repo, _tempdir) = make_repo();
        let plaintext = hash(0x33);

        repo.save(plaintext, digest(0xa1)).await.unwrap();
        repo.save(plaintext, digest(0xb2)).await.unwrap();

        let loaded = repo.find_by_plaintext_hash(&plaintext).await.unwrap();
        assert_eq!(loaded, Some(digest(0xb2)));
    }

    #[tokio::test]
    async fn forget_removes_mapping() {
        let (repo, _tempdir) = make_repo();
        let plaintext = hash(0x44);
        repo.save(plaintext, digest(0xcc)).await.unwrap();

        repo.forget(&plaintext).await.unwrap();

        let loaded = repo.find_by_plaintext_hash(&plaintext).await.unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn forget_missing_is_idempotent() {
        let (repo, _tempdir) = make_repo();
        let plaintext = hash(0x55);

        repo.forget(&plaintext).await.unwrap();
        repo.forget(&plaintext).await.unwrap();

        let loaded = repo.find_by_plaintext_hash(&plaintext).await.unwrap();
        assert_eq!(loaded, None);
    }
}
