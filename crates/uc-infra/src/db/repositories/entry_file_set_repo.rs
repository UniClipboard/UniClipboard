//! Entry file-set 持久化实现。
//!
//! 为什么需要这个模块:
//! `EntryFileSetRepositoryPort` 的契约要求"按 entry_id 整体替换清单"与
//! "按 entry_id 取回完整清单"。整体替换用同一事务内先 DELETE 再批量
//! INSERT 落地,行序由 `line_index` 保证,FK 由表定义保证(entry 删除时
//! CASCADE)。本文件把 wire 中性的 kind/排除原因枚举与 SQL 字符串做双向
//! 映射,保持表结构稳定。

use diesel::query_dsl::methods::{FilterDsl, OrderDsl};
use diesel::Connection;
use diesel::ExpressionMethods;
use diesel::RunQueryDsl;

use async_trait::async_trait;
use tracing::instrument;
use uc_core::clipboard::{
    ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetExcludeReason, EntryFileSetLine,
    EntryFileSetLineKind,
};
use uc_core::ids::{BlobId, EntryId};
use uc_core::ports::clipboard::EntryFileSetRepositoryPort;

use crate::db::models::entry_file_set::{EntryFileSetRow, NewEntryFileSetRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::entry_file_set;

/// Rows per multi-row INSERT statement. `NewEntryFileSetRow` binds 8 columns,
/// so 100 rows = 800 bound params — safely under SQLite's historical
/// 999-parameter-per-statement limit (older bundled builds), while still
/// collapsing a ~2000-member insert into ~20 statements instead of 2000.
const ENTRY_FILE_SET_INSERT_CHUNK: usize = 100;

pub struct DieselEntryFileSetRepository<E> {
    executor: E,
}

impl<E> DieselEntryFileSetRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

/// `kind` / `exclude_reason` 在持久化层的字符串编码。变体名保持稳定,不随
/// 上层重命名变动。
mod kind_codec {
    pub const FILE: &str = "file";
    pub const NON_FILE: &str = "non_file";
    pub const EXCLUDED: &str = "excluded";
}

mod exclude_reason_codec {
    pub const SIZE_CAP_EXCEEDED: &str = "size_cap_exceeded";
    pub const INGEST_FAILED: &str = "ingest_failed";
}

fn encode_line(entry_id: &str, line: &EntryFileSetLine) -> NewEntryFileSetRow {
    let (kind, content_hash, blob_id, size_bytes, exclude_reason) = match &line.kind {
        EntryFileSetLineKind::File {
            content_hash,
            blob_id,
            size_bytes,
        } => (
            kind_codec::FILE,
            Some(content_hash.to_string()),
            blob_id.as_ref().map(|id| id.to_string()),
            *size_bytes,
            None,
        ),
        EntryFileSetLineKind::NonFile => (kind_codec::NON_FILE, None, None, None, None),
        EntryFileSetLineKind::Excluded { reason } => {
            let reason_str = match reason {
                EntryFileSetExcludeReason::SizeCapExceeded => {
                    exclude_reason_codec::SIZE_CAP_EXCEEDED
                }
                EntryFileSetExcludeReason::IngestFailed => exclude_reason_codec::INGEST_FAILED,
            };
            (kind_codec::EXCLUDED, None, None, None, Some(reason_str))
        }
    };

    NewEntryFileSetRow {
        entry_id: entry_id.to_string(),
        line_index: line.line_index,
        original_text: line.original_text.clone(),
        kind: kind.to_string(),
        content_hash,
        blob_id,
        size_bytes,
        exclude_reason: exclude_reason.map(str::to_string),
    }
}

fn decode_row(row: EntryFileSetRow) -> Result<EntryFileSetLine, EntryFileSetError> {
    let kind = match row.kind.as_str() {
        kind_codec::FILE => {
            let content_hash = row.content_hash.ok_or_else(|| {
                EntryFileSetError::Storage("file row missing content_hash".into())
            })?;
            EntryFileSetLineKind::File {
                content_hash: ContentHash::from(content_hash),
                blob_id: row.blob_id.map(BlobId::from),
                size_bytes: row.size_bytes,
            }
        }
        kind_codec::NON_FILE => EntryFileSetLineKind::NonFile,
        kind_codec::EXCLUDED => {
            let reason_str = row.exclude_reason.ok_or_else(|| {
                EntryFileSetError::Storage("excluded row missing exclude_reason".into())
            })?;
            let reason = match reason_str.as_str() {
                exclude_reason_codec::SIZE_CAP_EXCEEDED => {
                    EntryFileSetExcludeReason::SizeCapExceeded
                }
                exclude_reason_codec::INGEST_FAILED => EntryFileSetExcludeReason::IngestFailed,
                other => {
                    return Err(EntryFileSetError::Storage(format!(
                        "unknown exclude_reason code: {other}"
                    )))
                }
            };
            EntryFileSetLineKind::Excluded { reason }
        }
        other => {
            return Err(EntryFileSetError::Storage(format!(
                "unknown file-set line kind code: {other}"
            )))
        }
    };

    Ok(EntryFileSetLine {
        line_index: row.line_index,
        original_text: row.original_text,
        kind,
    })
}

#[async_trait]
impl<E> EntryFileSetRepositoryPort for DieselEntryFileSetRepository<E>
where
    E: DbExecutor,
{
    #[instrument(
        name = "infra.sqlite.replace_entry_file_set",
        skip_all,
        fields(
            operation = "save",
            table = "entry_file_set",
            entry_id = %entry_id,
            line_count = file_set.lines.len(),
        )
    )]
    async fn save(
        &self,
        entry_id: &EntryId,
        file_set: &EntryFileSet,
    ) -> Result<(), EntryFileSetError> {
        let entry_id_str = entry_id.to_string();
        let new_rows: Vec<NewEntryFileSetRow> = file_set
            .lines
            .iter()
            .map(|line| encode_line(&entry_id_str, line))
            .collect();

        let entry_id_for_err = entry_id_str.clone();
        self.executor
            .run(move |conn| {
                conn.transaction(|conn| {
                    diesel::delete(entry_file_set::table)
                        .filter(entry_file_set::entry_id.eq(&entry_id_str))
                        .execute(conn)?;

                    // Batch insert in chunks: a directory copy can carry many
                    // members (ADR-010 caps the set at ~2000), so a multi-row
                    // insert beats per-row round-trips. Chunk so a single
                    // statement stays well under SQLite's bound-parameter limit
                    // (8 columns × CHUNK params); the whole loop is inside the
                    // surrounding transaction, so it's still all-or-nothing.
                    for chunk in new_rows.chunks(ENTRY_FILE_SET_INSERT_CHUNK) {
                        diesel::insert_into(entry_file_set::table)
                            .values(chunk)
                            .execute(conn)?;
                    }

                    Ok(())
                })
            })
            .map_err(|err| translate_storage_error(err, &entry_id_for_err))
    }

    #[instrument(
        name = "infra.sqlite.query_entry_file_set",
        skip_all,
        fields(
            operation = "load",
            table = "entry_file_set",
            entry_id = %entry_id,
        )
    )]
    async fn load(&self, entry_id: &EntryId) -> Result<Option<EntryFileSet>, EntryFileSetError> {
        let entry_id_str = entry_id.to_string();
        let entry_id_for_err = entry_id_str.clone();
        let rows: Vec<EntryFileSetRow> = self
            .executor
            .run(move |conn| {
                Ok(entry_file_set::table
                    .filter(entry_file_set::entry_id.eq(&entry_id_str))
                    .order(entry_file_set::line_index.asc())
                    .load::<EntryFileSetRow>(conn)?)
            })
            .map_err(|err| translate_storage_error(err, &entry_id_for_err))?;

        if rows.is_empty() {
            return Ok(None);
        }

        let lines = rows
            .into_iter()
            .map(decode_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(EntryFileSet { lines }))
    }
}

/// 把底层错误翻译为领域错误。FK violation 反映"引用了不存在的 entry",
/// 其它一律按 Storage 归类。
///
/// The executor threads the original `diesel::result::Error` through the
/// `anyhow` chain, so classify on the typed error kind rather than the
/// SQLite-specific message wording.
fn translate_storage_error(err: anyhow::Error, entry_id: &str) -> EntryFileSetError {
    if let Some(diesel::result::Error::DatabaseError(
        diesel::result::DatabaseErrorKind::ForeignKeyViolation,
        _,
    )) = err.downcast_ref::<diesel::result::Error>()
    {
        return EntryFileSetError::EntryNotFound(entry_id.to_string());
    }
    EntryFileSetError::Storage(err.to_string())
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
    use uc_core::clipboard::HashAlgorithm;

    type Repo = DieselEntryFileSetRepository<DieselSqliteExecutor>;

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("file-set-repo.sqlite");
        let path_str = path.to_str().unwrap();
        let pool_for_repo = init_db_pool(path_str).unwrap();
        let pool_for_seed = init_db_pool(path_str).unwrap();
        let repo = DieselEntryFileSetRepository::new(DieselSqliteExecutor::new(pool_for_repo));
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
            total_size: 0,
            pinned: false,
            delivery_tracked: true,
            is_favorited: false,
            content_category: "file".into(),
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

    fn sample_file_set() -> EntryFileSet {
        EntryFileSet {
            lines: vec![
                EntryFileSetLine {
                    line_index: 0,
                    original_text: "file:///a.txt".into(),
                    kind: EntryFileSetLineKind::File {
                        content_hash: ContentHash {
                            alg: HashAlgorithm::Blake3V1,
                            bytes: [1u8; 32],
                        },
                        blob_id: Some(BlobId::from("blob-a")),
                        size_bytes: Some(10),
                    },
                },
                EntryFileSetLine {
                    line_index: 1,
                    original_text: "# comment".into(),
                    kind: EntryFileSetLineKind::NonFile,
                },
                EntryFileSetLine {
                    line_index: 2,
                    original_text: "file:///too-big.bin".into(),
                    kind: EntryFileSetLineKind::Excluded {
                        reason: EntryFileSetExcludeReason::SizeCapExceeded,
                    },
                },
            ],
        }
    }

    #[tokio::test]
    async fn save_then_load_round_trips() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");

        let entry_id = EntryId::from("entry-1");
        repo.save(&entry_id, &sample_file_set()).await.unwrap();

        let loaded = repo.load(&entry_id).await.unwrap().expect("should exist");
        assert_eq!(loaded, sample_file_set());
    }

    #[tokio::test]
    async fn load_returns_none_for_unknown_entry() {
        let (repo, _seed_exec, _tempdir) = make_repo();
        let loaded = repo.load(&EntryId::from("never-existed")).await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn save_replaces_rather_than_appends() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");
        let entry_id = EntryId::from("entry-1");

        repo.save(&entry_id, &sample_file_set()).await.unwrap();

        let second = EntryFileSet {
            lines: vec![EntryFileSetLine {
                line_index: 0,
                original_text: "file:///only.txt".into(),
                kind: EntryFileSetLineKind::File {
                    content_hash: ContentHash {
                        alg: HashAlgorithm::Blake3V1,
                        bytes: [9u8; 32],
                    },
                    blob_id: Some(BlobId::from("blob-only")),
                    size_bytes: Some(5),
                },
            }],
        };
        repo.save(&entry_id, &second).await.unwrap();

        let loaded = repo.load(&entry_id).await.unwrap().unwrap();
        assert_eq!(loaded, second, "第二次 save 应整体替换而非追加");
    }

    #[tokio::test]
    async fn save_on_missing_entry_returns_entry_not_found() {
        let (repo, _seed_exec, _tempdir) = make_repo();
        let result = repo
            .save(&EntryId::from("ghost-entry"), &sample_file_set())
            .await;
        match result {
            Err(EntryFileSetError::EntryNotFound(id)) => assert_eq!(id, "ghost-entry"),
            other => panic!("预期 EntryNotFound,实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn save_then_load_round_trips_across_insert_chunk_boundary() {
        // A large member count must span multiple batched INSERT statements
        // (ENTRY_FILE_SET_INSERT_CHUNK per statement) and still round-trip in
        // full, in order.
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-big");
        let entry_id = EntryId::from("entry-big");

        let line_count = ENTRY_FILE_SET_INSERT_CHUNK * 2 + 5;
        let big = EntryFileSet {
            lines: (0..line_count as i64)
                .map(|idx| EntryFileSetLine {
                    line_index: idx,
                    original_text: format!("file:///f{idx}.txt"),
                    kind: EntryFileSetLineKind::File {
                        content_hash: ContentHash {
                            alg: HashAlgorithm::Blake3V1,
                            bytes: [(idx % 256) as u8; 32],
                        },
                        blob_id: None,
                        size_bytes: None,
                    },
                })
                .collect(),
        };

        repo.save(&entry_id, &big).await.unwrap();
        let loaded = repo.load(&entry_id).await.unwrap().expect("should exist");
        assert_eq!(loaded, big, "跨 insert chunk 边界应完整、保序地回读");
    }

    #[tokio::test]
    async fn fk_cascade_deletes_file_set_rows() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");
        let entry_id = EntryId::from("entry-1");
        repo.save(&entry_id, &sample_file_set()).await.unwrap();

        seed_exec
            .run(move |conn| {
                diesel::delete(clipboard_entry::table)
                    .filter(clipboard_entry::entry_id.eq("entry-1"))
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();

        let loaded = repo.load(&entry_id).await.unwrap();
        assert!(loaded.is_none(), "FK CASCADE 应清理 file-set 行");
    }
}
