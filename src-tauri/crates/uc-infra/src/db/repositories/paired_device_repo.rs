use async_trait::async_trait;
use diesel::prelude::*;

use uc_core::network::{PairedDevice, PairingState};
use uc_core::ports::{PairedDeviceRepositoryError, PairedDeviceRepositoryPort};
use uc_core::settings::model::SyncSettings;
use uc_core::PeerId;

use crate::db::models::{NewPairedDeviceRow, PairedDeviceRow};
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::paired_device::dsl::*;

pub struct DieselPairedDeviceRepository<E, M> {
    executor: E,
    mapper: M,
}

impl<E, M> DieselPairedDeviceRepository<E, M> {
    pub fn new(executor: E, mapper: M) -> Self {
        Self { executor, mapper }
    }
}

#[async_trait]
impl<E, M> PairedDeviceRepositoryPort for DieselPairedDeviceRepository<E, M>
where
    E: DbExecutor,
    M: InsertMapper<PairedDevice, NewPairedDeviceRow>
        + RowMapper<PairedDeviceRow, PairedDevice>
        + Send
        + Sync,
{
    async fn get_by_peer_id(
        &self,
        peer_id_value: &PeerId,
    ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
        let peer_id_str = peer_id_value.as_str().to_string();
        self.executor
            .run(move |conn| {
                let row = paired_device
                    .filter(peer_id.eq(&peer_id_str))
                    .first::<PairedDeviceRow>(conn)
                    .optional()
                    .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

                match row {
                    Some(r) => {
                        let device = self
                            .mapper
                            .to_domain(&r)
                            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;
                        Ok(Some(device))
                    }
                    None => Ok(None),
                }
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))
    }

    async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
        self.executor
            .run(|conn| {
                let rows = paired_device
                    .load::<PairedDeviceRow>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                let mut devices = Vec::with_capacity(rows.len());
                for row in rows {
                    let peer_id_value = row.peer_id.clone();
                    let device = self.mapper.to_domain(&row).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to map paired_device peer_id {}: {}",
                            peer_id_value,
                            e
                        )
                    })?;
                    devices.push(device);
                }

                Ok(devices)
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))
    }

    async fn upsert(&self, device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
        let row = self
            .mapper
            .to_row(&device)
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

        self.executor
            .run(move |conn| {
                diesel::insert_into(paired_device)
                    .values(&row)
                    .on_conflict(peer_id)
                    .do_update()
                    .set((
                        pairing_state.eq(row.pairing_state.clone()),
                        identity_fingerprint.eq(row.identity_fingerprint.clone()),
                        paired_at.eq(row.paired_at),
                        last_seen_at.eq(row.last_seen_at),
                        device_name.eq(row.device_name.clone()),
                    ))
                    .execute(conn)
                    .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))
    }

    async fn set_state(
        &self,
        peer_id_value: &PeerId,
        state: PairingState,
    ) -> Result<(), PairedDeviceRepositoryError> {
        let peer_id_str = peer_id_value.as_str().to_string();
        let state_str = state.to_string();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::update(paired_device.filter(peer_id.eq(&peer_id_str)))
                    .set(pairing_state.eq(state_str))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

        if affected == 0 {
            return Err(PairedDeviceRepositoryError::NotFound);
        }

        Ok(())
    }

    async fn update_last_seen(
        &self,
        peer_id_value: &PeerId,
        last_seen_at_value: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        let peer_id_str = peer_id_value.as_str().to_string();
        let last_seen_ts = last_seen_at_value.timestamp();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::update(paired_device.filter(peer_id.eq(&peer_id_str)))
                    .set(last_seen_at.eq(last_seen_ts))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

        if affected == 0 {
            return Err(PairedDeviceRepositoryError::NotFound);
        }

        Ok(())
    }

    async fn delete(&self, peer_id_value: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
        let peer_id_str = peer_id_value.as_str().to_string();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::delete(paired_device.filter(peer_id.eq(&peer_id_str)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

        if affected == 0 {
            return Err(PairedDeviceRepositoryError::NotFound);
        }

        Ok(())
    }

    async fn update_sync_settings(
        &self,
        peer_id_value: &PeerId,
        settings: Option<SyncSettings>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        let peer_id_str = peer_id_value.as_str().to_string();
        let json_value = settings
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| {
                PairedDeviceRepositoryError::Storage(format!("serialize sync_settings: {}", e))
            })?;

        let affected = self
            .executor
            .run(move |conn| {
                diesel::update(paired_device.filter(peer_id.eq(&peer_id_str)))
                    .set(sync_settings.eq(json_value))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| PairedDeviceRepositoryError::Storage(e.to_string()))?;

        if affected == 0 {
            return Err(PairedDeviceRepositoryError::NotFound);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::paired_device_mapper::PairedDeviceRowMapper;
    use crate::db::pool::init_db_pool;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_paired_device_persistence() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = dir.path().join("paired.db");
        let pool = init_db_pool(db_path.to_str().unwrap()).expect("Failed to init db");
        let repo = DieselPairedDeviceRepository::new(
            DieselSqliteExecutor::new(pool.clone()),
            PairedDeviceRowMapper,
        );

        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: chrono::Utc::now(),
            last_seen_at: None,
            device_name: "Test Device".to_string(),
            sync_settings: None,
        };

        repo.upsert(device.clone()).await.unwrap();

        let fresh_pool = init_db_pool(db_path.to_str().unwrap()).expect("Failed to init db");
        let fresh_repo = DieselPairedDeviceRepository::new(
            DieselSqliteExecutor::new(fresh_pool),
            PairedDeviceRowMapper,
        );

        let loaded = fresh_repo.get_by_peer_id(&device.peer_id).await.unwrap();
        assert!(loaded.is_some());
        let loaded_device = loaded.unwrap();
        assert_eq!(loaded_device.device_name, "Test Device");
    }
}
