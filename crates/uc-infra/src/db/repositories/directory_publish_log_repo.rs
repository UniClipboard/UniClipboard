use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use uc_core::ids::EntryId;
use uc_core::ports::directory_publish_log::{
    DirectoryPublishRecord, GetDirectoryPublishRecordPort, PublishLogError, PublishPhase,
    RecordDirectoryPublishPort,
};
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};

use super::directory_publish_log_cipher::DirectoryPublishLogCipher;
use crate::db::models::{DirectoryPublishLogRow, NewDirectoryPublishLogRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::directory_publish_log;

const PUBLISH_LOG_KEY_INFO: &[u8] = b"uniclipboard-directory-publish-log/v1";

/// SQLite adapter for encrypted directory publication recovery metadata.
pub struct DieselDirectoryPublishLogRepository<E> {
    executor: E,
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselDirectoryPublishLogRepository<E> {
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

    async fn cipher(&self) -> Result<DirectoryPublishLogCipher, PublishLogError> {
        let profile = self
            .current_profile
            .current_profile()
            .await
            .map_err(|error| {
                PublishLogError::EncryptionUnavailable(format!(
                    "current profile unavailable: {error}"
                ))
            })?;
        let key = self
            .derive_subkey
            .derive_subkey(profile.as_ref().as_bytes(), PUBLISH_LOG_KEY_INFO)
            .await
            .map_err(|error| match error {
                SpaceAccessError::NotUnlocked => PublishLogError::EncryptionUnavailable(
                    "session locked: cannot derive directory publish log key".to_owned(),
                ),
                other => PublishLogError::EncryptionUnavailable(format!(
                    "derive directory publish log key: {other}"
                )),
            })?;
        Ok(DirectoryPublishLogCipher::new(key))
    }
}

fn backend(error: anyhow::Error) -> PublishLogError {
    PublishLogError::Backend(error.to_string())
}

#[async_trait]
impl<E: DbExecutor> RecordDirectoryPublishPort for DieselDirectoryPublishLogRepository<E> {
    async fn record_phase(
        &self,
        entry_id: &str,
        attempt_id: &str,
        phase: PublishPhase,
        root_map: &[(PathBuf, PathBuf)],
        now_ms: i64,
    ) -> Result<(), PublishLogError> {
        if phase == PublishPhase::Landed {
            return Err(PublishLogError::Backend(
                "landed phase requires the atomic directory receive commit".to_owned(),
            ));
        }
        let ciphertext = self
            .cipher()
            .await?
            .seal(&EntryId::from(entry_id), attempt_id, root_map)
            .map_err(|error| PublishLogError::Backend(error.to_string()))?;
        let row = NewDirectoryPublishLogRow {
            entry_id: entry_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            phase: phase.to_string(),
            root_map_ciphertext: Some(ciphertext),
            partial_publication: false,
            partial_root_count: 0,
            landed: false,
            updated_at_ms: now_ms,
        };
        self.executor
            .run(move |conn| {
                diesel::insert_into(directory_publish_log::table)
                    .values(&row)
                    .on_conflict((
                        directory_publish_log::entry_id,
                        directory_publish_log::attempt_id,
                    ))
                    .do_update()
                    .set((
                        directory_publish_log::phase.eq(excluded(directory_publish_log::phase)),
                        directory_publish_log::root_map_ciphertext
                            .eq(excluded(directory_publish_log::root_map_ciphertext)),
                        directory_publish_log::updated_at_ms
                            .eq(excluded(directory_publish_log::updated_at_ms)),
                    ))
                    .execute(conn)?;
                Ok(())
            })
            .map_err(backend)
    }

    async fn record_partial(
        &self,
        entry_id: &str,
        attempt_id: &str,
        visible_roots: u32,
        now_ms: i64,
    ) -> Result<(), PublishLogError> {
        let visible_roots = i32::try_from(visible_roots)
            .map_err(|_| PublishLogError::Backend("visible root count exceeds i32".to_owned()))?;
        let entry_id = entry_id.to_owned();
        let attempt_id = attempt_id.to_owned();
        self.executor
            .run(move |conn| {
                let affected = diesel::update(
                    directory_publish_log::table
                        .filter(directory_publish_log::entry_id.eq(entry_id))
                        .filter(directory_publish_log::attempt_id.eq(attempt_id)),
                )
                .set((
                    directory_publish_log::partial_publication.eq(true),
                    directory_publish_log::partial_root_count.eq(visible_roots),
                    directory_publish_log::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?;
                if affected != 1 {
                    anyhow::bail!("directory publish record does not exist");
                }
                Ok(())
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> GetDirectoryPublishRecordPort for DieselDirectoryPublishLogRepository<E> {
    async fn get_publish_record(
        &self,
        entry_id: &str,
        attempt_id: &str,
    ) -> Result<Option<DirectoryPublishRecord>, PublishLogError> {
        let entry_id = entry_id.to_owned();
        let attempt_id = attempt_id.to_owned();
        let row = self
            .executor
            .run(move |conn| {
                directory_publish_log::table
                    .filter(directory_publish_log::entry_id.eq(entry_id))
                    .filter(directory_publish_log::attempt_id.eq(attempt_id))
                    .select(DirectoryPublishLogRow::as_select())
                    .first(conn)
                    .optional()
                    .map_err(anyhow::Error::from)
            })
            .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let phase = PublishPhase::from_str(&row.phase)?;
        let root_map = match row.root_map_ciphertext {
            Some(ciphertext) => self
                .cipher()
                .await?
                .open(
                    &EntryId::from(row.entry_id.as_str()),
                    &row.attempt_id,
                    &ciphertext,
                )
                .map_err(|_| PublishLogError::InvalidCiphertext)?,
            None => Vec::new(),
        };
        let partial_visible_roots = if row.partial_publication {
            Some(u32::try_from(row.partial_root_count).map_err(|_| {
                PublishLogError::Backend("negative persisted visible root count".to_owned())
            })?)
        } else {
            None
        };
        Ok(Some(DirectoryPublishRecord {
            entry_id: row.entry_id,
            attempt_id: row.attempt_id,
            phase,
            root_map,
            partial_visible_roots,
            landed: row.landed,
        }))
    }
}
