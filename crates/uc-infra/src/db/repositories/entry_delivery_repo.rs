//! Entry delivery 持久化实现。
//!
//! 为什么需要这个模块:
//! `EntryDeliveryRepositoryPort` 的契约要求"按 (entry_id, target_device_id)
//! upsert"与"按 entry 列出所有目标结果"。SQLite 用 `INSERT OR REPLACE`
//! 自然落地 upsert 语义,FK 由表定义保证(entry 删除时 CASCADE)。本文件
//! 把 wire 中性的 status 枚举与 SQL 字符串做双向映射,保持表结构稳定。

use crate::db::models::entry_delivery::{EntryDeliveryRow, NewEntryDeliveryRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::clipboard_entry_delivery;
use async_trait::async_trait;
use diesel::query_dsl::methods::FilterDsl;
use diesel::ExpressionMethods;
use diesel::RunQueryDsl;
use tracing::instrument;
use uc_core::clipboard::{
    DeliveryFailureReason, EntryDeliveryError, EntryDeliveryRecord, EntryDeliveryStatus,
};
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::EntryDeliveryRepositoryPort;

pub struct DieselEntryDeliveryRepository<E> {
    executor: E,
}

impl<E> DieselEntryDeliveryRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

/// 状态在持久化层的字符串编码。变体名保持稳定,不随上层重命名变动。
mod status_codec {
    use super::*;

    pub const DELIVERED: &str = "delivered";
    pub const DUPLICATE: &str = "duplicate";
    pub const UNREACHABLE: &str = "unreachable";
    // Legacy alias: rows written before the Unreachable promotion decode as
    // Unreachable for seamless migration without a schema rewrite.
    const LEGACY_FAILED_OFFLINE: &str = "failed_offline";
    pub const FAILED_LOCAL_POLICY: &str = "failed_local_policy";
    pub const FAILED_PEER_REJECTED: &str = "failed_peer_rejected";
    pub const FAILED_IO: &str = "failed_io";
    pub const FAILED_INTERNAL: &str = "failed_internal";

    pub fn encode(status: &EntryDeliveryStatus) -> &'static str {
        match status {
            EntryDeliveryStatus::Delivered => DELIVERED,
            EntryDeliveryStatus::Duplicate => DUPLICATE,
            EntryDeliveryStatus::Unreachable => UNREACHABLE,
            EntryDeliveryStatus::Failed { reason } => match reason {
                DeliveryFailureReason::LocalPolicy => FAILED_LOCAL_POLICY,
                DeliveryFailureReason::PeerRejected => FAILED_PEER_REJECTED,
                DeliveryFailureReason::Io => FAILED_IO,
                DeliveryFailureReason::Internal => FAILED_INTERNAL,
            },
        }
    }

    pub fn decode(raw: &str) -> Result<EntryDeliveryStatus, EntryDeliveryError> {
        match raw {
            DELIVERED => Ok(EntryDeliveryStatus::Delivered),
            DUPLICATE => Ok(EntryDeliveryStatus::Duplicate),
            UNREACHABLE | LEGACY_FAILED_OFFLINE => Ok(EntryDeliveryStatus::Unreachable),
            FAILED_LOCAL_POLICY => Ok(EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::LocalPolicy,
            }),
            FAILED_PEER_REJECTED => Ok(EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::PeerRejected,
            }),
            FAILED_IO => Ok(EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::Io,
            }),
            FAILED_INTERNAL => Ok(EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::Internal,
            }),
            other => Err(EntryDeliveryError::Storage(format!(
                "unknown delivery status code: {other}"
            ))),
        }
    }
}

fn row_to_record(row: EntryDeliveryRow) -> Result<EntryDeliveryRecord, EntryDeliveryError> {
    Ok(EntryDeliveryRecord {
        entry_id: EntryId::from(row.entry_id),
        target_device_id: DeviceId::new(row.target_device_id),
        status: status_codec::decode(&row.status)?,
        reason_detail: row.reason_detail,
        updated_at_ms: row.updated_at_ms,
    })
}

#[async_trait]
impl<E> EntryDeliveryRepositoryPort for DieselEntryDeliveryRepository<E>
where
    E: DbExecutor,
{
    #[instrument(
        name = "infra.sqlite.upsert_entry_delivery",
        skip_all,
        fields(
            operation = "record_attempt",
            table = "clipboard_entry_delivery",
            entry_id = %record.entry_id,
            target_device_id = %record.target_device_id,
        )
    )]
    async fn record_attempt(&self, record: &EntryDeliveryRecord) -> Result<(), EntryDeliveryError> {
        let new_row = NewEntryDeliveryRow {
            entry_id: record.entry_id.to_string(),
            target_device_id: record.target_device_id.to_string(),
            status: status_codec::encode(&record.status).to_string(),
            reason_detail: record.reason_detail.clone(),
            updated_at_ms: record.updated_at_ms,
        };

        let entry_id_for_err = record.entry_id.to_string();
        self.executor
            .run(move |conn| {
                diesel::replace_into(clipboard_entry_delivery::table)
                    .values(&new_row)
                    .execute(conn)?;
                Ok(())
            })
            .map_err(|err| translate_storage_error(err, &entry_id_for_err))
    }

    #[instrument(
        name = "infra.sqlite.query_entry_delivery",
        skip_all,
        fields(
            operation = "list_by_entry",
            table = "clipboard_entry_delivery",
            entry_id = %entry_id,
        )
    )]
    async fn list_by_entry(
        &self,
        entry_id: &EntryId,
    ) -> Result<Vec<EntryDeliveryRecord>, EntryDeliveryError> {
        let entry_id_str = entry_id.to_string();
        let entry_id_for_err = entry_id_str.clone();
        let rows: Vec<EntryDeliveryRow> = self
            .executor
            .run(move |conn| {
                Ok(clipboard_entry_delivery::table
                    .filter(clipboard_entry_delivery::entry_id.eq(&entry_id_str))
                    .load::<EntryDeliveryRow>(conn)?)
            })
            .map_err(|err| translate_storage_error(err, &entry_id_for_err))?;

        rows.into_iter().map(row_to_record).collect()
    }
}

/// 把底层错误翻译为领域错误。FK violation 反映"引用了不存在的 entry",
/// 其它一律按 Storage 归类。
fn translate_storage_error(err: anyhow::Error, entry_id: &str) -> EntryDeliveryError {
    let msg = err.to_string();
    // SQLite 的外键违反字符串里会包含 "FOREIGN KEY constraint failed",
    // diesel 把它包成 DatabaseError(ForeignKeyViolation, ...)。两种渠道都覆盖。
    if msg.contains("FOREIGN KEY") || msg.to_ascii_lowercase().contains("foreign key") {
        EntryDeliveryError::EntryNotFound(entry_id.to_string())
    } else {
        EntryDeliveryError::Storage(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::models::{NewClipboardEntryRow, NewClipboardEventRow};
    use crate::db::pool::init_db_pool;
    use crate::db::ports::DbExecutor;
    use crate::db::schema::{clipboard_entry, clipboard_event};
    use tempfile::{tempdir, TempDir};

    type Repo = DieselEntryDeliveryRepository<DieselSqliteExecutor>;

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("delivery-repo.sqlite");
        let path_str = path.to_str().unwrap();
        let pool_for_repo = init_db_pool(path_str).unwrap();
        let pool_for_seed = init_db_pool(path_str).unwrap();
        let repo = DieselEntryDeliveryRepository::new(DieselSqliteExecutor::new(pool_for_repo));
        (repo, DieselSqliteExecutor::new(pool_for_seed), tempdir)
    }

    fn seed_entry(executor: &DieselSqliteExecutor, entry_id: &str) {
        let event_id = format!("ev-{entry_id}");
        let event_row = NewClipboardEventRow {
            event_id: event_id.clone(),
            captured_at_ms: 1_700_000_000_000,
            source_device: "test-device".into(),
            snapshot_hash: format!("blake3v1:{entry_id}"),
        };
        let entry_row = NewClipboardEntryRow {
            entry_id: entry_id.to_string(),
            event_id,
            created_at_ms: 1_700_000_000_000,
            active_time_ms: 1_700_000_000_000,
            title: None,
            total_size: 0,
            pinned: false,
            delivery_tracked: true,
            is_favorited: false,
        };
        executor
            .run(move |conn| {
                diesel::insert_into(clipboard_event::table)
                    .values(&event_row)
                    .execute(conn)?;
                diesel::insert_into(clipboard_entry::table)
                    .values(&entry_row)
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();
    }

    fn make_record(
        entry_id: &str,
        target: &str,
        status: EntryDeliveryStatus,
    ) -> EntryDeliveryRecord {
        EntryDeliveryRecord {
            entry_id: EntryId::from(entry_id.to_string()),
            target_device_id: DeviceId::new(target.to_string()),
            status,
            reason_detail: None,
            updated_at_ms: 1_700_000_000_001,
        }
    }

    #[test]
    fn decode_legacy_failed_offline_as_unreachable() {
        let status = status_codec::decode("failed_offline").unwrap();
        assert!(matches!(status, EntryDeliveryStatus::Unreachable));
    }

    #[test]
    fn decode_new_unreachable_round_trips() {
        let encoded = status_codec::encode(&EntryDeliveryStatus::Unreachable);
        assert_eq!(encoded, "unreachable");
        let decoded = status_codec::decode(encoded).unwrap();
        assert!(matches!(decoded, EntryDeliveryStatus::Unreachable));
    }

    #[tokio::test]
    async fn record_attempt_inserts_new_row() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");

        let rec = make_record("entry-1", "peer-A", EntryDeliveryStatus::Delivered);
        repo.record_attempt(&rec).await.expect("upsert ok");

        let listed = repo
            .list_by_entry(&EntryId::from("entry-1"))
            .await
            .expect("list ok");
        assert_eq!(listed.len(), 1);
        assert!(matches!(listed[0].status, EntryDeliveryStatus::Delivered));
        assert_eq!(listed[0].target_device_id.to_string(), "peer-A");
    }

    #[tokio::test]
    async fn record_attempt_upserts_existing_row() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");

        repo.record_attempt(&make_record(
            "entry-1",
            "peer-A",
            EntryDeliveryStatus::Delivered,
        ))
        .await
        .unwrap();
        repo.record_attempt(&make_record(
            "entry-1",
            "peer-A",
            EntryDeliveryStatus::Unreachable,
        ))
        .await
        .unwrap();

        let listed = repo.list_by_entry(&EntryId::from("entry-1")).await.unwrap();
        assert_eq!(listed.len(), 1, "upsert 不应增加行数");
        assert!(matches!(listed[0].status, EntryDeliveryStatus::Unreachable));
    }

    #[tokio::test]
    async fn list_by_entry_returns_all_targets() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");

        for (peer, status) in [
            ("peer-A", EntryDeliveryStatus::Delivered),
            ("peer-B", EntryDeliveryStatus::Duplicate),
            (
                "peer-C",
                EntryDeliveryStatus::Failed {
                    reason: DeliveryFailureReason::Io,
                },
            ),
        ] {
            repo.record_attempt(&make_record("entry-1", peer, status))
                .await
                .unwrap();
        }

        let mut listed = repo.list_by_entry(&EntryId::from("entry-1")).await.unwrap();
        listed.sort_by(|a, b| {
            a.target_device_id
                .to_string()
                .cmp(&b.target_device_id.to_string())
        });
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].target_device_id.to_string(), "peer-A");
        assert_eq!(listed[2].target_device_id.to_string(), "peer-C");
    }

    #[tokio::test]
    async fn record_attempt_on_missing_entry_returns_entry_not_found() {
        let (repo, _seed_exec, _tempdir) = make_repo();
        let result = repo
            .record_attempt(&make_record(
                "ghost-entry",
                "peer-A",
                EntryDeliveryStatus::Delivered,
            ))
            .await;
        match result {
            Err(EntryDeliveryError::EntryNotFound(id)) => {
                assert_eq!(id, "ghost-entry");
            }
            other => panic!("预期 EntryNotFound,实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_by_entry_returns_empty_for_unknown() {
        let (repo, _seed_exec, _tempdir) = make_repo();
        let listed = repo
            .list_by_entry(&EntryId::from("never-existed"))
            .await
            .unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn fk_cascade_deletes_delivery_rows() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");

        repo.record_attempt(&make_record(
            "entry-1",
            "peer-A",
            EntryDeliveryStatus::Delivered,
        ))
        .await
        .unwrap();

        // 删 entry,delivery 应被 CASCADE
        seed_exec
            .run(move |conn| {
                diesel::delete(clipboard_entry::table)
                    .filter(clipboard_entry::entry_id.eq("entry-1"))
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();

        let listed = repo.list_by_entry(&EntryId::from("entry-1")).await.unwrap();
        assert!(listed.is_empty(), "FK CASCADE 应清理 delivery 行");
    }
}
