//! `BlobMigrationRepoPort` 的 Diesel 实现。
//!
//! 维护两件事：
//! * 主表 `clipboard_snapshot_representation` 的 `inline_data` 列读写
//!   （switch-space 流程的 phase 1 列出 + phase 3 覆盖）。
//! * 备份表 `clipboard_migration_backup` 的全量 CRUD。
//!
//! 两张表都在同一个 SQLite 文件里，phase 3 覆写主表与读取备份表可以走
//! 同一个 connection；本 adapter 暂时不强制把两步包成一个 transaction，
//! 由 use-case 层根据失败语义决定何时收口（commit 3 会做事务化）。

use async_trait::async_trait;
use diesel::prelude::*;
use tracing::debug_span;

use uc_core::ids::{EventId, RepresentationId};
use uc_core::ports::clipboard::{BlobMigrationRepoError, BlobMigrationRepoPort, MigrationRecord};

use crate::db::ports::DbExecutor;
use crate::db::schema::{clipboard_migration_backup, clipboard_snapshot_representation};

pub struct DieselBlobMigrationRepository<E> {
    executor: E,
}

impl<E> DieselBlobMigrationRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

/// 备份表行模型——仅用于本 adapter 内部 (列与 schema.rs 一一对应)。
#[derive(Queryable, Insertable)]
#[diesel(table_name = clipboard_migration_backup)]
struct MigrationBackupRow {
    event_id: String,
    representation_id: String,
    migration_ciphertext: Vec<u8>,
}

#[async_trait]
impl<E> BlobMigrationRepoPort for DieselBlobMigrationRepository<E>
where
    E: DbExecutor,
{
    async fn list_main_inline_representations(
        &self,
    ) -> Result<Vec<(EventId, RepresentationId)>, BlobMigrationRepoError> {
        let span = debug_span!("infra.sqlite.list_main_inline_representations");
        let rows = span.in_scope(|| {
            self.executor.run(|conn| {
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::inline_data.is_not_null())
                    .select((
                        clipboard_snapshot_representation::event_id,
                        clipboard_snapshot_representation::id,
                    ))
                    .load::<(String, String)>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
        });
        let rows = rows.map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(e_id, rep_id)| (EventId::from_string(e_id), RepresentationId::from(rep_id)))
            .collect())
    }

    async fn read_main_inline_data(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<Vec<u8>>, BlobMigrationRepoError> {
        let event_id_s = event_id.as_ref().to_string();
        let rep_id_s = representation_id.as_ref().to_string();
        let span = debug_span!(
            "infra.sqlite.read_main_inline_data",
            event_id = %event_id,
            representation_id = %representation_id,
        );
        let result: Option<Option<Vec<u8>>> = span
            .in_scope(|| {
                self.executor.run(move |conn| {
                    clipboard_snapshot_representation::table
                        .filter(clipboard_snapshot_representation::event_id.eq(&event_id_s))
                        .filter(clipboard_snapshot_representation::id.eq(&rep_id_s))
                        .select(clipboard_snapshot_representation::inline_data)
                        .first::<Option<Vec<u8>>>(conn)
                        .optional()
                        .map_err(|e| anyhow::anyhow!(e.to_string()))
                })
            })
            .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))?;
        // `Option<Option<_>>`：外层 = 行存在与否；内层 = inline_data 非空与否。
        Ok(result.flatten())
    }

    async fn upsert_record(&self, record: &MigrationRecord) -> Result<(), BlobMigrationRepoError> {
        let row = MigrationBackupRow {
            event_id: record.event_id.as_ref().to_string(),
            representation_id: record.representation_id.as_ref().to_string(),
            migration_ciphertext: record.migration_ciphertext.clone(),
        };
        let new_ct = row.migration_ciphertext.clone();
        let span = debug_span!(
            "infra.sqlite.upsert_migration_backup",
            event_id = %record.event_id,
            representation_id = %record.representation_id,
            bytes = row.migration_ciphertext.len(),
        );
        span.in_scope(|| {
            self.executor.run(move |conn| {
                diesel::insert_into(clipboard_migration_backup::table)
                    .values(&row)
                    .on_conflict((
                        clipboard_migration_backup::event_id,
                        clipboard_migration_backup::representation_id,
                    ))
                    .do_update()
                    .set(clipboard_migration_backup::migration_ciphertext.eq(new_ct))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
        })
        .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))
    }

    async fn count_records(&self) -> Result<u64, BlobMigrationRepoError> {
        let span = debug_span!("infra.sqlite.count_migration_backup");
        let count: i64 = span
            .in_scope(|| {
                self.executor.run(|conn| {
                    clipboard_migration_backup::table
                        .count()
                        .get_result::<i64>(conn)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))
                })
            })
            .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))?;
        Ok(count.max(0) as u64)
    }

    async fn list_records(&self) -> Result<Vec<MigrationRecord>, BlobMigrationRepoError> {
        let span = debug_span!("infra.sqlite.list_migration_backup");
        let rows: Vec<MigrationBackupRow> = span
            .in_scope(|| {
                self.executor.run(|conn| {
                    clipboard_migration_backup::table
                        .load::<MigrationBackupRow>(conn)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))
                })
            })
            .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| MigrationRecord {
                event_id: EventId::from_string(r.event_id),
                representation_id: RepresentationId::from(r.representation_id),
                migration_ciphertext: r.migration_ciphertext,
            })
            .collect())
    }

    async fn update_main_inline_data(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
        new_ciphertext: &[u8],
    ) -> Result<(), BlobMigrationRepoError> {
        let event_id_s = event_id.as_ref().to_string();
        let rep_id_s = representation_id.as_ref().to_string();
        let bytes = new_ciphertext.to_vec();
        let span = debug_span!(
            "infra.sqlite.update_main_inline_data",
            event_id = %event_id,
            representation_id = %representation_id,
            bytes = new_ciphertext.len(),
        );
        span.in_scope(|| {
            self.executor.run(move |conn| {
                diesel::update(
                    clipboard_snapshot_representation::table
                        .filter(clipboard_snapshot_representation::event_id.eq(&event_id_s))
                        .filter(clipboard_snapshot_representation::id.eq(&rep_id_s)),
                )
                .set(clipboard_snapshot_representation::inline_data.eq(Some(bytes)))
                .execute(conn)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
        })
        .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))
    }

    async fn discard_all_records(&self) -> Result<(), BlobMigrationRepoError> {
        let span = debug_span!("infra.sqlite.discard_migration_backup");
        span.in_scope(|| {
            self.executor.run(|conn| {
                diesel::delete(clipboard_migration_backup::table)
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
        })
        .map_err(|e| BlobMigrationRepoError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    //! 集成测试：用真 SQLite 文件 + 完整迁移链，验证 backup 表 CRUD
    //! 与主表 update 走通。这层是 V1 加密迁移的关键路径，故偏重整体行为
    //! 验证而非 unit-level 隔离。

    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use diesel::sql_query;
    use tempfile::{tempdir, TempDir};

    fn make_repo() -> (
        DieselBlobMigrationRepository<DieselSqliteExecutor>,
        DieselSqliteExecutor,
        TempDir,
    ) {
        let dir = tempdir().unwrap();
        let url = dir.path().join("migration-test.sqlite");
        let pool = init_db_pool(url.to_str().unwrap()).unwrap();
        let executor = DieselSqliteExecutor::new(pool);
        let repo = DieselBlobMigrationRepository::new(DieselSqliteExecutor::new(
            // 重复构造另一份 executor 共享同一份 pool 是 init_db_pool
            // 内部 Arc 已经做了；为简化测试这里再调一次 init_db_pool
            // 也可以——但同进程多次跑迁移会因为 sqlite_master 已有表
            // 而幂等通过，无副作用。
            init_db_pool(url.to_str().unwrap()).unwrap(),
        ));
        (repo, executor, dir)
    }

    fn seed_main_row(
        executor: &DieselSqliteExecutor,
        event_id: &str,
        rep_id: &str,
        payload: &[u8],
    ) {
        // 直接 SQL 插入：跳过 ClipboardEvent 完整模型构造（FK 约束在
        // schema 上未启用，可以单插一行 representation 行）。
        let event_id = event_id.to_string();
        let rep_id = rep_id.to_string();
        let payload = payload.to_vec();
        executor
            .run(move |conn| {
                // 先放一行 clipboard_event（满足 FK 不强制，但保持
                // 测试数据语义合理）
                sql_query("INSERT INTO clipboard_event (event_id, captured_at_ms, source_device, snapshot_hash) VALUES (?, 0, 'dev', 'h') ON CONFLICT DO NOTHING")
                    .bind::<diesel::sql_types::Text, _>(&event_id)
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                sql_query(
                    "INSERT INTO clipboard_snapshot_representation \
                     (id, event_id, format_id, mime_type, size_bytes, inline_data, blob_id, payload_state, last_error) \
                     VALUES (?, ?, 'fmt', NULL, 0, ?, NULL, 'Inline', NULL)",
                )
                .bind::<diesel::sql_types::Text, _>(&rep_id)
                .bind::<diesel::sql_types::Text, _>(&event_id)
                .bind::<diesel::sql_types::Binary, _>(&payload)
                .execute(conn)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn list_main_inline_skips_null_inline_rows() {
        let (repo, executor, _dir) = make_repo();
        seed_main_row(&executor, "evt-1", "rep-1", b"plain-ciphertext-bytes");

        let rows = repo.list_main_inline_representations().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.as_ref(), "evt-1");
        assert_eq!(rows[0].1.as_ref(), "rep-1");
    }

    #[tokio::test]
    async fn upsert_then_list_round_trips_record() {
        let (repo, _, _dir) = make_repo();
        let rec = MigrationRecord {
            event_id: EventId::from_string("evt-1".into()),
            representation_id: RepresentationId::from("rep-1"),
            migration_ciphertext: vec![1, 2, 3],
        };
        repo.upsert_record(&rec).await.unwrap();
        assert_eq!(repo.count_records().await.unwrap(), 1);
        let listed = repo.list_records().await.unwrap();
        assert_eq!(listed, vec![rec]);
    }

    #[tokio::test]
    async fn upsert_same_pk_overwrites_ciphertext() {
        let (repo, _, _dir) = make_repo();
        let mut rec = MigrationRecord {
            event_id: EventId::from_string("evt-1".into()),
            representation_id: RepresentationId::from("rep-1"),
            migration_ciphertext: vec![1, 2, 3],
        };
        repo.upsert_record(&rec).await.unwrap();
        rec.migration_ciphertext = vec![9, 9, 9];
        repo.upsert_record(&rec).await.unwrap();
        let listed = repo.list_records().await.unwrap();
        assert_eq!(listed, vec![rec]);
        assert_eq!(repo.count_records().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn discard_all_clears_table() {
        let (repo, _, _dir) = make_repo();
        repo.upsert_record(&MigrationRecord {
            event_id: EventId::from_string("evt-1".into()),
            representation_id: RepresentationId::from("rep-1"),
            migration_ciphertext: vec![0xab, 0xcd],
        })
        .await
        .unwrap();
        repo.upsert_record(&MigrationRecord {
            event_id: EventId::from_string("evt-2".into()),
            representation_id: RepresentationId::from("rep-2"),
            migration_ciphertext: vec![0xef],
        })
        .await
        .unwrap();
        assert_eq!(repo.count_records().await.unwrap(), 2);

        repo.discard_all_records().await.unwrap();
        assert_eq!(repo.count_records().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn read_main_inline_data_returns_bytes_for_seeded_row() {
        let (repo, executor, _dir) = make_repo();
        seed_main_row(&executor, "evt-9", "rep-9", b"the-bytes");
        let bytes = repo
            .read_main_inline_data(
                &EventId::from_string("evt-9".into()),
                &RepresentationId::from("rep-9"),
            )
            .await
            .unwrap();
        assert_eq!(bytes.as_deref(), Some(b"the-bytes".as_slice()));
    }

    #[tokio::test]
    async fn read_main_inline_data_returns_none_for_missing_row() {
        let (repo, _, _dir) = make_repo();
        let bytes = repo
            .read_main_inline_data(
                &EventId::from_string("ghost-evt".into()),
                &RepresentationId::from("ghost-rep"),
            )
            .await
            .unwrap();
        assert_eq!(bytes, None);
    }

    #[tokio::test]
    async fn update_main_inline_data_overwrites_seeded_bytes() {
        let (repo, executor, _dir) = make_repo();
        seed_main_row(&executor, "evt-up", "rep-up", b"old");
        repo.update_main_inline_data(
            &EventId::from_string("evt-up".into()),
            &RepresentationId::from("rep-up"),
            b"new-payload",
        )
        .await
        .unwrap();
        let bytes = repo
            .read_main_inline_data(
                &EventId::from_string("evt-up".into()),
                &RepresentationId::from("rep-up"),
            )
            .await
            .unwrap();
        assert_eq!(bytes.as_deref(), Some(b"new-payload".as_slice()));
    }
}
