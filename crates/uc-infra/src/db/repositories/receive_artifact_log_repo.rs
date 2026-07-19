use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    GetReceiveArtifactRecordPort, ListUnsettledReceiveArtifactsPort, ReceiveArtifactLogError,
    ReceiveArtifactPhase, ReceiveArtifactRecord, ReceiveArtifactResolution,
    RecordReceiveArtifactsPort,
};

use super::receive_artifact_cipher::ReceiveArtifactCipher;
use crate::db::models::{NewReceiveArtifactLogRow, ReceiveArtifactLogRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::receive_artifact_log;

const ARTIFACT_KEY_INFO: &[u8] = b"uniclipboard-receive-artifact-log/v1";

pub struct DieselReceiveArtifactLogRepository<E> {
    executor: E,
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselReceiveArtifactLogRepository<E> {
    pub fn new(
        executor: E,
        derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
        current_profile: Arc<dyn CurrentProfilePort>,
    ) -> Self {
        Self {
            executor,
            derive_subkey,
            current_profile,
        }
    }

    async fn cipher(&self) -> Result<ReceiveArtifactCipher, ReceiveArtifactLogError> {
        let profile = self
            .current_profile
            .current_profile()
            .await
            .map_err(|error| {
                ReceiveArtifactLogError::EncryptionUnavailable(format!(
                    "current profile unavailable: {error}"
                ))
            })?;
        let key = self
            .derive_subkey
            .derive_subkey(profile.as_ref().as_bytes(), ARTIFACT_KEY_INFO)
            .await
            .map_err(|error| match error {
                SpaceAccessError::NotUnlocked => ReceiveArtifactLogError::EncryptionUnavailable(
                    "session locked: receive artifact key unavailable".to_owned(),
                ),
                other => ReceiveArtifactLogError::EncryptionUnavailable(format!(
                    "derive receive artifact key: {other}"
                )),
            })?;
        Ok(ReceiveArtifactCipher::new(key))
    }
}

fn backend(error: anyhow::Error) -> ReceiveArtifactLogError {
    ReceiveArtifactLogError::Backend(error.to_string())
}

fn decode_row(
    cipher: &ReceiveArtifactCipher,
    row: ReceiveArtifactLogRow,
) -> Result<ReceiveArtifactRecord, ReceiveArtifactLogError> {
    let phase = ReceiveArtifactPhase::parse(&row.phase)?;
    let resolution = ReceiveArtifactResolution::parse(&row.resolution)?;
    let artifacts = cipher
        .open(&row.entry_id, &row.attempt_id, &row.artifact_ciphertext)
        .map_err(|error| ReceiveArtifactLogError::Backend(error.to_string()))?;
    Ok(ReceiveArtifactRecord {
        entry_id: row.entry_id,
        attempt_id: row.attempt_id,
        phase,
        resolution,
        artifacts,
        updated_at_ms: row.updated_at_ms,
    })
}

#[async_trait]
impl<E: DbExecutor> RecordReceiveArtifactsPort for DieselReceiveArtifactLogRepository<E> {
    async fn record_receive_artifacts(
        &self,
        record: &ReceiveArtifactRecord,
    ) -> Result<(), ReceiveArtifactLogError> {
        let ciphertext = self
            .cipher()
            .await?
            .seal(&record.entry_id, &record.attempt_id, &record.artifacts)
            .map_err(|error| ReceiveArtifactLogError::Backend(error.to_string()))?;
        let row = NewReceiveArtifactLogRow {
            entry_id: record.entry_id.clone(),
            attempt_id: record.attempt_id.clone(),
            phase: record.phase.as_str().to_owned(),
            resolution: record.resolution.as_str().to_owned(),
            artifact_ciphertext: ciphertext,
            updated_at_ms: record.updated_at_ms,
        };
        self.executor
            .run(move |conn| {
                diesel::insert_into(receive_artifact_log::table)
                    .values(&row)
                    .on_conflict((
                        receive_artifact_log::entry_id,
                        receive_artifact_log::attempt_id,
                    ))
                    .do_update()
                    .set((
                        receive_artifact_log::phase.eq(excluded(receive_artifact_log::phase)),
                        receive_artifact_log::resolution
                            .eq(excluded(receive_artifact_log::resolution)),
                        receive_artifact_log::artifact_ciphertext
                            .eq(excluded(receive_artifact_log::artifact_ciphertext)),
                        receive_artifact_log::updated_at_ms
                            .eq(excluded(receive_artifact_log::updated_at_ms)),
                    ))
                    .execute(conn)?;
                Ok(())
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> GetReceiveArtifactRecordPort for DieselReceiveArtifactLogRepository<E> {
    async fn get_receive_artifact_record(
        &self,
        entry_id: &str,
        attempt_id: &str,
    ) -> Result<Option<ReceiveArtifactRecord>, ReceiveArtifactLogError> {
        let entry_id = entry_id.to_owned();
        let attempt_id = attempt_id.to_owned();
        let cipher = self.cipher().await?;
        let row = self
            .executor
            .run(move |conn| {
                receive_artifact_log::table
                    .filter(receive_artifact_log::entry_id.eq(entry_id))
                    .filter(receive_artifact_log::attempt_id.eq(attempt_id))
                    .select(ReceiveArtifactLogRow::as_select())
                    .first(conn)
                    .optional()
                    .map_err(anyhow::Error::from)
            })
            .map_err(backend)?;
        row.map(|row| decode_row(&cipher, row)).transpose()
    }
}

#[async_trait]
impl<E: DbExecutor> ListUnsettledReceiveArtifactsPort for DieselReceiveArtifactLogRepository<E> {
    async fn list_unsettled_receive_artifacts(
        &self,
    ) -> Result<Vec<ReceiveArtifactRecord>, ReceiveArtifactLogError> {
        let cipher = self.cipher().await?;
        let rows = self
            .executor
            .run(move |conn| {
                receive_artifact_log::table
                    .filter(
                        receive_artifact_log::resolution
                            .eq(ReceiveArtifactResolution::Pending.as_str()),
                    )
                    .order(receive_artifact_log::updated_at_ms.asc())
                    .select(ReceiveArtifactLogRow::as_select())
                    .load(conn)
                    .map_err(anyhow::Error::from)
            })
            .map_err(backend)?;
        rows.into_iter()
            .map(|row| decode_row(&cipher, row))
            .collect()
    }
}
