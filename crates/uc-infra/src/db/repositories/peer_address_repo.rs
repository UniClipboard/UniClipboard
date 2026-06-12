//! Diesel-backed [`PeerAddressRepositoryPort`] implementation.
//!
//! Persists one address record per device in the `peer_address` table. The
//! stored `addr_blob` is treated as opaque bytes on the persistence layer —
//! the caller (iroh adapter) owns the encoding.

use async_trait::async_trait;
use diesel::prelude::*;

use uc_core::ids::DeviceId;
use uc_core::ports::{PeerAddressError, PeerAddressRecord, PeerAddressRepositoryPort};

use crate::db::models::{NewPeerAddressRow, PeerAddressRow};
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::peer_address::dsl::*;

pub struct DieselPeerAddressRepository<E, M> {
    executor: E,
    mapper: M,
}

impl<E, M> DieselPeerAddressRepository<E, M> {
    pub fn new(executor: E, mapper: M) -> Self {
        Self { executor, mapper }
    }
}

#[async_trait]
impl<E, M> PeerAddressRepositoryPort for DieselPeerAddressRepository<E, M>
where
    E: DbExecutor,
    M: InsertMapper<PeerAddressRecord, NewPeerAddressRow>
        + RowMapper<PeerAddressRow, PeerAddressRecord>
        + Send
        + Sync,
{
    async fn get(&self, device: &DeviceId) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
        let id = device.as_str().to_string();
        self.executor
            .run(move |conn| {
                let row = peer_address
                    .filter(device_id.eq(&id))
                    .first::<PeerAddressRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(row)
            })
            .map_err(|e| PeerAddressError::Internal(e.to_string()))
            .and_then(|row_opt| match row_opt {
                Some(r) => self
                    .mapper
                    .to_domain(&r)
                    .map(Some)
                    .map_err(|e| PeerAddressError::Internal(e.to_string())),
                None => Ok(None),
            })
    }

    async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
        let row = self
            .mapper
            .to_row(record)
            .map_err(|e| PeerAddressError::Internal(e.to_string()))?;

        self.executor
            .run(move |conn| {
                diesel::insert_into(peer_address)
                    .values(&row)
                    .on_conflict(device_id)
                    .do_update()
                    .set((
                        addr_blob.eq(row.addr_blob.clone()),
                        observed_at.eq(row.observed_at),
                    ))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| PeerAddressError::Internal(e.to_string()))
    }

    async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
        let rows = self
            .executor
            .run(|conn| {
                peer_address
                    .load::<PeerAddressRow>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| PeerAddressError::Internal(e.to_string()))?;

        rows.iter()
            .map(|r| {
                self.mapper
                    .to_domain(r)
                    .map_err(|e| PeerAddressError::Internal(e.to_string()))
            })
            .collect()
    }

    async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError> {
        let id = device.as_str().to_string();
        self.executor
            .run(move |conn| {
                diesel::delete(peer_address.filter(device_id.eq(&id)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| PeerAddressError::Internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::peer_address_mapper::PeerAddressRowMapper;
    use crate::db::pool::init_db_pool;
    use chrono::{TimeZone, Utc};
    use tempfile::{tempdir, TempDir};

    fn make_repo() -> (
        DieselPeerAddressRepository<DieselSqliteExecutor, PeerAddressRowMapper>,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("peer-address.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let repo =
            DieselPeerAddressRepository::new(DieselSqliteExecutor::new(pool), PeerAddressRowMapper);
        (repo, tempdir)
    }

    fn fixture_record(id: &str, blob: &[u8]) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: DeviceId::new(id),
            addr_blob: blob.to_vec(),
            observed_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn upsert_then_get_roundtrip() {
        let (repo, _tempdir) = make_repo();
        let rec = fixture_record("dev-a", b"iroh-addr-blob-a");
        repo.upsert(&rec).await.unwrap();

        let loaded = repo.get(&rec.device_id).await.unwrap().unwrap();
        assert_eq!(loaded, rec);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (repo, _tempdir) = make_repo();
        let result = repo.get(&DeviceId::new("missing")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn upsert_is_last_write_wins() {
        let (repo, _tempdir) = make_repo();
        let mut rec = fixture_record("dev-b", b"v1");
        repo.upsert(&rec).await.unwrap();

        rec.addr_blob = b"v2-bigger-blob".to_vec();
        rec.observed_at = Utc.timestamp_opt(1_700_100_000, 0).unwrap();
        repo.upsert(&rec).await.unwrap();

        let loaded = repo.get(&rec.device_id).await.unwrap().unwrap();
        assert_eq!(loaded.addr_blob, b"v2-bigger-blob".to_vec());
        assert_eq!(loaded.observed_at, rec.observed_at);
    }

    #[tokio::test]
    async fn list_returns_all_rows() {
        let (repo, _tempdir) = make_repo();
        repo.upsert(&fixture_record("a", b"addr-a")).await.unwrap();
        repo.upsert(&fixture_record("b", b"addr-b")).await.unwrap();
        repo.upsert(&fixture_record("c", b"addr-c")).await.unwrap();

        let mut rows = repo.list().await.unwrap();
        rows.sort_by(|x, y| x.device_id.as_str().cmp(y.device_id.as_str()));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].device_id.as_str(), "a");
        assert_eq!(rows[2].device_id.as_str(), "c");
    }

    #[tokio::test]
    async fn remove_is_idempotent() {
        let (repo, _tempdir) = make_repo();
        let rec = fixture_record("dev-c", b"addr-c");
        repo.upsert(&rec).await.unwrap();

        repo.remove(&rec.device_id).await.unwrap();
        repo.remove(&rec.device_id).await.unwrap();
        assert!(repo.get(&rec.device_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_on_empty_db_is_empty_vec() {
        let (repo, _tempdir) = make_repo();
        let rows = repo.list().await.unwrap();
        assert!(rows.is_empty());
    }
}
