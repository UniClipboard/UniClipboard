use async_trait::async_trait;
use diesel::prelude::*;

use uc_core::{DeviceId, TrustedPeer, TrustedPeerError, TrustedPeerRepositoryPort};

use crate::db::models::{NewTrustedPeerRow, TrustedPeerRow};
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::trusted_peer::dsl::*;

pub struct DieselTrustedPeerRepository<E, M> {
    executor: E,
    mapper: M,
}

impl<E, M> DieselTrustedPeerRepository<E, M> {
    pub fn new(executor: E, mapper: M) -> Self {
        Self { executor, mapper }
    }
}

#[async_trait]
impl<E, M> TrustedPeerRepositoryPort for DieselTrustedPeerRepository<E, M>
where
    E: DbExecutor,
    M: InsertMapper<TrustedPeer, NewTrustedPeerRow>
        + RowMapper<TrustedPeerRow, TrustedPeer>
        + Send
        + Sync,
{
    async fn get(
        &self,
        peer_device_id_value: &DeviceId,
    ) -> Result<Option<TrustedPeer>, TrustedPeerError> {
        let id = peer_device_id_value.as_str().to_string();
        self.executor
            .run(move |conn| {
                let row = trusted_peer
                    .filter(peer_device_id.eq(&id))
                    .first::<TrustedPeerRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                match row {
                    Some(r) => {
                        let peer = self
                            .mapper
                            .to_domain(&r)
                            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                        Ok(Some(peer))
                    }
                    None => Ok(None),
                }
            })
            .map_err(|e| TrustedPeerError::Repository(e.to_string()))
    }

    async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
        self.executor
            .run(|conn| {
                let rows = trusted_peer
                    .load::<TrustedPeerRow>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                let mut peers = Vec::with_capacity(rows.len());
                for row in rows {
                    let id = row.peer_device_id.clone();
                    let peer = self.mapper.to_domain(&row).map_err(|e| {
                        anyhow::anyhow!("Failed to map trusted_peer peer_device_id {}: {}", id, e)
                    })?;
                    peers.push(peer);
                }

                Ok(peers)
            })
            .map_err(|e| TrustedPeerError::Repository(e.to_string()))
    }

    async fn save(&self, peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
        let row = self
            .mapper
            .to_row(peer)
            .map_err(|e| TrustedPeerError::Repository(e.to_string()))?;

        self.executor
            .run(move |conn| {
                diesel::insert_into(trusted_peer)
                    .values(&row)
                    .on_conflict(peer_device_id)
                    .do_update()
                    .set((
                        local_device_id.eq(row.local_device_id.clone()),
                        peer_fingerprint.eq(row.peer_fingerprint.clone()),
                        trusted_at.eq(row.trusted_at),
                    ))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| TrustedPeerError::Repository(e.to_string()))
    }

    async fn remove(&self, peer_device_id_value: &DeviceId) -> Result<bool, TrustedPeerError> {
        let id = peer_device_id_value.as_str().to_string();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::delete(trusted_peer.filter(peer_device_id.eq(&id)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| TrustedPeerError::Repository(e.to_string()))?;

        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::trusted_peer_mapper::TrustedPeerRowMapper;
    use crate::db::pool::init_db_pool;
    use chrono::Utc;
    use tempfile::{tempdir, TempDir};
    use uc_core::security::IdentityFingerprint;
    use uc_core::{DeviceId, TrustedPeer};

    fn make_repo() -> (
        DieselTrustedPeerRepository<DieselSqliteExecutor, TrustedPeerRowMapper>,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("trusted-peer.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let repo =
            DieselTrustedPeerRepository::new(DieselSqliteExecutor::new(pool), TrustedPeerRowMapper);
        (repo, tempdir)
    }

    /// Pad a short seed into a valid 16-char alphanumeric fingerprint.
    fn fixture_fingerprint(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn fixture_peer(peer: &str, local: &str) -> TrustedPeer {
        TrustedPeer {
            local_device_id: DeviceId::new(local),
            peer_device_id: DeviceId::new(peer),
            peer_fingerprint: fixture_fingerprint(&format!("FP{peer}")),
            trusted_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn save_then_get_roundtrip() {
        let (repo, _tempdir) = make_repo();
        let peer = fixture_peer("peer-a", "local-1");
        repo.save(&peer).await.unwrap();

        let loaded = repo.get(&peer.peer_device_id).await.unwrap().unwrap();
        assert_eq!(loaded.peer_device_id, peer.peer_device_id);
        assert_eq!(loaded.local_device_id, peer.local_device_id);
        assert_eq!(loaded.peer_fingerprint, peer.peer_fingerprint);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (repo, _tempdir) = make_repo();
        let result = repo.get(&DeviceId::new("missing")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn save_is_upsert() {
        let (repo, _tempdir) = make_repo();
        let mut peer = fixture_peer("peer-b", "local-1");
        repo.save(&peer).await.unwrap();

        let rotated = fixture_fingerprint("ROTATEDFP");
        peer.peer_fingerprint = rotated.clone();
        repo.save(&peer).await.unwrap();

        let loaded = repo.get(&peer.peer_device_id).await.unwrap().unwrap();
        assert_eq!(loaded.peer_fingerprint, rotated);
    }

    #[tokio::test]
    async fn list_returns_all_saved() {
        let (repo, _tempdir) = make_repo();
        repo.save(&fixture_peer("a", "local-1")).await.unwrap();
        repo.save(&fixture_peer("b", "local-1")).await.unwrap();
        repo.save(&fixture_peer("c", "local-1")).await.unwrap();

        let mut peers = repo.list().await.unwrap();
        peers.sort_by(|x, y| x.peer_device_id.as_str().cmp(y.peer_device_id.as_str()));
        assert_eq!(peers.len(), 3);
        assert_eq!(peers[0].peer_device_id.as_str(), "a");
        assert_eq!(peers[2].peer_device_id.as_str(), "c");
    }

    #[tokio::test]
    async fn remove_returns_true_when_present_false_when_absent() {
        let (repo, _tempdir) = make_repo();
        let peer = fixture_peer("peer-c", "local-1");
        repo.save(&peer).await.unwrap();

        let first = repo.remove(&peer.peer_device_id).await.unwrap();
        let second = repo.remove(&peer.peer_device_id).await.unwrap();
        assert!(first);
        assert!(!second);
        assert!(repo.get(&peer.peer_device_id).await.unwrap().is_none());
    }
}
