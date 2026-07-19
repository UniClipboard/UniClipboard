use async_trait::async_trait;
use diesel::prelude::*;
use tracing::{info, warn};

use crate::db::ports::DbExecutor;
use crate::db::schema::file_transfer_privacy_maintenance;
use uc_core::ports::{
    EnsureFileTransferPrivacyMaintenancePort, FileTransferPrivacyMaintenanceError,
};

pub struct SqliteFileTransferPrivacyMaintenance<E> {
    executor: E,
}

impl<E> SqliteFileTransferPrivacyMaintenance<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E> EnsureFileTransferPrivacyMaintenancePort for SqliteFileTransferPrivacyMaintenance<E>
where
    E: DbExecutor + Clone + 'static,
{
    async fn ensure_file_transfer_privacy_maintenance(
        &self,
    ) -> Result<(), FileTransferPrivacyMaintenanceError> {
        let executor = self.executor.clone();
        tokio::task::spawn_blocking(move || {
            executor.run(|conn| {
                let state = file_transfer_privacy_maintenance::table
                    .filter(file_transfer_privacy_maintenance::id.eq(1))
                    .select(file_transfer_privacy_maintenance::state)
                    .first::<String>(conn)
                    .optional()?;

                match state.as_deref() {
                    Some("completed") => return Ok(()),
                    Some("pending_physical_purge") => {}
                    Some(other) => {
                        anyhow::bail!("invalid file transfer privacy maintenance state: {other}")
                    }
                    None => anyhow::bail!("file transfer privacy maintenance marker is missing"),
                }

                let started = std::time::Instant::now();
                run_with_busy_retry(conn, "PRAGMA wal_checkpoint(TRUNCATE)")?;
                run_with_busy_retry(conn, "VACUUM")?;

                let updated = diesel::update(
                    file_transfer_privacy_maintenance::table
                        .filter(file_transfer_privacy_maintenance::id.eq(1))
                        .filter(
                            file_transfer_privacy_maintenance::state.eq("pending_physical_purge"),
                        ),
                )
                .set((
                    file_transfer_privacy_maintenance::state.eq("completed"),
                    file_transfer_privacy_maintenance::updated_at_ms
                        .eq(chrono::Utc::now().timestamp_millis()),
                ))
                .execute(conn)?;
                if updated != 1 {
                    anyhow::bail!("file transfer privacy maintenance marker changed during purge");
                }

                info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "file transfer plaintext residue purge completed"
                );
                Ok(())
            })
        })
        .await
        .map_err(|error| FileTransferPrivacyMaintenanceError::Backend(error.to_string()))?
        .map_err(|error| FileTransferPrivacyMaintenanceError::Backend(error.to_string()))
    }
}

fn run_with_busy_retry(
    conn: &mut diesel::sqlite::SqliteConnection,
    sql: &str,
) -> anyhow::Result<()> {
    const MAX_ATTEMPTS: usize = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        match diesel::sql_query(sql).execute(conn) {
            Ok(_) => return Ok(()),
            Err(diesel::result::Error::DatabaseError(_, ref info))
                if attempt < MAX_ATTEMPTS && is_busy_or_locked(info.message()) =>
            {
                warn!(
                    sql,
                    attempt, "file transfer privacy maintenance is busy; retrying"
                );
                std::thread::sleep(std::time::Duration::from_millis(50 * attempt as u64));
            }
            Err(error) => anyhow::bail!("maintenance statement `{sql}` failed: {error}"),
        }
    }
    anyhow::bail!("maintenance statement `{sql}` exhausted its retry budget")
}

fn is_busy_or_locked(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database is busy")
}
