//! Entry file-set 持久化实现。
//!
//! 为什么需要这个模块:
//! `EntryFileSetRepositoryPort` 的契约要求"按 entry_id 整体替换清单"与
//! "按 entry_id 取回完整清单"。整体替换用同一事务内先 DELETE 再批量
//! INSERT 落地,行序由 `line_index` 保证,FK 由表定义保证(entry 删除时
//! CASCADE)。本文件把 wire 中性的 kind/排除原因枚举与 SQL 字符串做双向
//! 映射,保持表结构稳定。

use std::sync::Arc;

#[cfg(test)]
use diesel::query_dsl::methods::SelectDsl;
use diesel::query_dsl::methods::{FilterDsl, OrderDsl};
use diesel::Connection;
use diesel::ExpressionMethods;
use diesel::RunQueryDsl;
use diesel::SqliteConnection;

use async_trait::async_trait;
use tracing::instrument;
use uc_core::clipboard::{
    ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetExcludeReason, EntryFileSetLine,
    EntryFileSetLineKind, FileSetMemberKind, FileSetMemberLocation,
};
use uc_core::ids::{BlobId, EntryId};
use uc_core::ports::clipboard::EntryFileSetRepositoryPort;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};

use super::entry_file_set_cipher::EntryFileSetPathCipher;
use crate::db::models::entry_file_set::{EntryFileSetRow, NewEntryFileSetRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::entry_file_set;

/// Rows per multi-row INSERT statement. `NewEntryFileSetRow` binds 12 columns,
/// so 80 rows = 960 bound params — safely under SQLite's historical
/// 999-parameter-per-statement limit (older bundled builds), while still
/// collapsing a ~2000-member insert into ~25 statements instead of 2000.
const ENTRY_FILE_SET_INSERT_CHUNK: usize = 80;

/// HKDF `info` label for the entry-file-set path-column AEAD subkey. Distinct
/// from every other subkey label so this key never doubles for another purpose.
const FILE_SET_KEY_INFO: &[u8] = b"uniclipboard-file-set/v1";

pub struct DieselEntryFileSetRepository<E> {
    executor: E,
    /// Derives the per-session subkey that seals the path columns. Held rather
    /// than a fixed key so a lock/unlock or profile switch always re-derives.
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    /// Salts the subkey by the active profile (defense-in-depth alongside the
    /// per-profile database file).
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselEntryFileSetRepository<E> {
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

    /// Derive the path-column cipher for the currently unlocked session.
    ///
    /// Returns `EntryFileSetError::Storage` when the session is locked or the
    /// profile is unavailable — the manifest is only read/written while
    /// unlocked, and the dispatch reader treats a load error as "no manifest"
    /// and falls back to snapshot re-parse.
    async fn cipher(&self) -> Result<EntryFileSetPathCipher, EntryFileSetError> {
        derive_file_set_cipher(self.derive_subkey.as_ref(), self.current_profile.as_ref()).await
    }
}

pub(super) async fn derive_file_set_cipher(
    derive_subkey: &dyn DeriveSpaceSubkeyPort,
    current_profile: &dyn CurrentProfilePort,
) -> Result<EntryFileSetPathCipher, EntryFileSetError> {
    let profile = current_profile.current_profile().await.map_err(|error| {
        EntryFileSetError::Storage(format!("current profile unavailable: {error}"))
    })?;
    let key = derive_subkey
        .derive_subkey(profile.as_ref().as_bytes(), FILE_SET_KEY_INFO)
        .await
        .map_err(|error| match error {
            SpaceAccessError::NotUnlocked => {
                EntryFileSetError::Storage("session locked: cannot derive file-set key".into())
            }
            other => EntryFileSetError::Storage(format!("derive file-set key: {other}")),
        })?;
    Ok(EntryFileSetPathCipher::new(key))
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
    pub const UNSUPPORTED_MEMBER: &str = "unsupported_member";
}

pub(super) fn encode_file_set_rows(
    cipher: &EntryFileSetPathCipher,
    entry_id: &EntryId,
    file_set: &EntryFileSet,
) -> Result<Vec<NewEntryFileSetRow>, EntryFileSetError> {
    file_set
        .lines
        .iter()
        .map(|line| encode_line(cipher, entry_id, line))
        .collect()
}

fn encode_line(
    cipher: &EntryFileSetPathCipher,
    entry_id: &EntryId,
    line: &EntryFileSetLine,
) -> Result<NewEntryFileSetRow, EntryFileSetError> {
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
                EntryFileSetExcludeReason::UnsupportedMember => {
                    exclude_reason_codec::UNSUPPORTED_MEMBER
                }
            };
            (kind_codec::EXCLUDED, None, None, None, Some(reason_str))
        }
    };

    let original_text_ct = cipher
        .seal_original_text(entry_id, line.line_index, &line.original_text)
        .map_err(|e| EntryFileSetError::Storage(format!("seal original_text: {e}")))?;
    let (root_index, relative_path_ct, kind_tag, root_name_ct) = match &line.member_location {
        Some(location) => {
            let relative_path_ct = cipher
                .seal_relative_path(entry_id, line.line_index, &location.relative_path)
                .map_err(|e| EntryFileSetError::Storage(format!("seal relative_path: {e}")))?;
            let root_name_ct = cipher
                .seal_root_name(entry_id, line.line_index, &location.root_name)
                .map_err(|e| EntryFileSetError::Storage(format!("seal root_name: {e}")))?;
            (
                Some(location.root_index),
                Some(relative_path_ct),
                Some(location.kind.as_tag().to_string()),
                Some(root_name_ct),
            )
        }
        None => (None, None, None, None),
    };

    Ok(NewEntryFileSetRow {
        entry_id: entry_id.to_string(),
        line_index: line.line_index,
        original_text_ct: Some(original_text_ct),
        kind: kind.to_string(),
        content_hash,
        blob_id,
        size_bytes,
        exclude_reason: exclude_reason.map(str::to_string),
        root_index,
        relative_path_ct,
        kind_tag,
        root_name_ct,
    })
}

fn decode_row(
    cipher: &EntryFileSetPathCipher,
    entry_id: &EntryId,
    row: EntryFileSetRow,
) -> Result<EntryFileSetLine, EntryFileSetError> {
    let original_text_ct = row.original_text_ct.ok_or_else(|| {
        EntryFileSetError::Storage("file-set row missing sealed original_text".into())
    })?;
    let original_text = cipher
        .open_original_text(entry_id, row.line_index, &original_text_ct)
        .map_err(|e| EntryFileSetError::Storage(format!("open original_text: {e}")))?;
    let member_location = match (
        row.root_index,
        row.relative_path_ct,
        row.kind_tag,
        row.root_name_ct,
    ) {
        (None, None, None, None) => None,
        (Some(root_index), Some(relative_path_ct), Some(kind_tag), Some(root_name_ct)) => {
            let relative_path = cipher
                .open_relative_path(entry_id, row.line_index, &relative_path_ct)
                .map_err(|e| EntryFileSetError::Storage(format!("open relative_path: {e}")))?;
            let kind = FileSetMemberKind::from_tag(&kind_tag).ok_or_else(|| {
                EntryFileSetError::Storage(format!("unknown member kind tag: {kind_tag}"))
            })?;
            let root_name = cipher
                .open_root_name(entry_id, row.line_index, &root_name_ct)
                .map_err(|e| EntryFileSetError::Storage(format!("open root_name: {e}")))?;
            Some(FileSetMemberLocation {
                root_index,
                root_name,
                relative_path,
                kind,
            })
        }
        _ => {
            return Err(EntryFileSetError::Storage(
                "file-set row has incomplete member location".into(),
            ))
        }
    };

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
                exclude_reason_codec::UNSUPPORTED_MEMBER => {
                    EntryFileSetExcludeReason::UnsupportedMember
                }
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
        original_text,
        member_location,
        kind,
    })
}

pub(super) fn replace_entry_file_set_rows(
    conn: &mut SqliteConnection,
    entry_id: &str,
    new_rows: &[NewEntryFileSetRow],
) -> anyhow::Result<()> {
    diesel::delete(entry_file_set::table)
        .filter(entry_file_set::entry_id.eq(entry_id))
        .execute(conn)?;
    for chunk in new_rows.chunks(ENTRY_FILE_SET_INSERT_CHUNK) {
        diesel::insert_into(entry_file_set::table)
            .values(chunk)
            .execute(conn)?;
    }
    Ok(())
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
        // An empty manifest is a pure DELETE, so it needs no key — deriving one
        // would fail an otherwise-valid replace when the session is locked.
        let new_rows: Vec<NewEntryFileSetRow> = if file_set.lines.is_empty() {
            Vec::new()
        } else {
            let cipher = self.cipher().await?;
            encode_file_set_rows(&cipher, entry_id, file_set)?
        };

        let entry_id_str = entry_id.to_string();
        let entry_id_for_err = entry_id_str.clone();
        self.executor
            .run(move |conn| {
                conn.transaction(|conn| replace_entry_file_set_rows(conn, &entry_id_str, &new_rows))
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

        let cipher = self.cipher().await?;
        let lines = rows
            .into_iter()
            .map(|row| decode_row(&cipher, entry_id, row))
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
    use uc_core::ids::ProfileId;
    use uc_core::ports::security::current_profile::CurrentProfileError;

    type Repo = DieselEntryFileSetRepository<DieselSqliteExecutor>;

    /// Fixed-key subkey derivation: returns a constant regardless of salt/info,
    /// so the repo round-trips deterministically without a real unlocked session.
    struct FixedSubkey([u8; 32]);
    #[async_trait]
    impl DeriveSpaceSubkeyPort for FixedSubkey {
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok(self.0)
        }
    }

    /// Subkey derivation that always reports a locked session.
    struct LockedSubkey;
    #[async_trait]
    impl DeriveSpaceSubkeyPort for LockedSubkey {
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Err(SpaceAccessError::NotUnlocked)
        }
    }

    struct FixedProfile;
    #[async_trait]
    impl CurrentProfilePort for FixedProfile {
        async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
            Ok(ProfileId::from("default"))
        }
    }

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        make_repo_with_subkey(Arc::new(FixedSubkey([7u8; 32])))
    }

    fn make_repo_with_subkey(
        derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    ) -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("file-set-repo.sqlite");
        let path_str = path.to_str().unwrap();
        let pool_for_repo = init_db_pool(path_str).unwrap();
        let pool_for_seed = init_db_pool(path_str).unwrap();
        let repo = DieselEntryFileSetRepository::new(
            DieselSqliteExecutor::new(pool_for_repo),
            derive_subkey,
            Arc::new(FixedProfile),
        );
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
                    member_location: Some(FileSetMemberLocation {
                        root_index: 0,
                        root_name: "a.txt".into(),
                        relative_path: "a.txt".into(),
                        kind: FileSetMemberKind::File,
                    }),
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
                    member_location: None,
                    kind: EntryFileSetLineKind::NonFile,
                },
                EntryFileSetLine {
                    line_index: 2,
                    original_text: "file:///too-big.bin".into(),
                    member_location: None,
                    kind: EntryFileSetLineKind::Excluded {
                        reason: EntryFileSetExcludeReason::SizeCapExceeded,
                    },
                },
                EntryFileSetLine {
                    line_index: 3,
                    original_text: "file:///root/run.sh".into(),
                    member_location: Some(FileSetMemberLocation {
                        root_index: 1,
                        root_name: "root".into(),
                        relative_path: "run.sh".into(),
                        kind: FileSetMemberKind::Executable,
                    }),
                    kind: EntryFileSetLineKind::File {
                        content_hash: ContentHash {
                            alg: HashAlgorithm::Blake3V1,
                            bytes: [2u8; 32],
                        },
                        blob_id: None,
                        size_bytes: Some(20),
                    },
                },
                EntryFileSetLine {
                    line_index: 4,
                    original_text: "file:///root".into(),
                    member_location: Some(FileSetMemberLocation {
                        root_index: 1,
                        root_name: "root".into(),
                        relative_path: "empty".into(),
                        kind: FileSetMemberKind::EmptyDirectory,
                    }),
                    kind: EntryFileSetLineKind::NonFile,
                },
                EntryFileSetLine {
                    line_index: 5,
                    original_text: "file:///root/link".into(),
                    member_location: Some(FileSetMemberLocation {
                        root_index: 1,
                        root_name: "root".into(),
                        relative_path: "link".into(),
                        kind: FileSetMemberKind::File,
                    }),
                    kind: EntryFileSetLineKind::Excluded {
                        reason: EntryFileSetExcludeReason::UnsupportedMember,
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
                member_location: None,
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
                    member_location: None,
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
    async fn sealed_path_column_is_not_plaintext_on_disk() {
        let (repo, seed_exec, _tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");
        let entry_id = EntryId::from("entry-1");
        repo.save(&entry_id, &sample_file_set()).await.unwrap();

        // Read the raw sealed column back out and assert the device-local path
        // never appears as plaintext bytes on disk.
        let cts: Vec<Option<Vec<u8>>> = seed_exec
            .run(move |conn| {
                Ok(entry_file_set::table
                    .filter(entry_file_set::entry_id.eq("entry-1"))
                    .order(entry_file_set::line_index.asc())
                    .select(entry_file_set::original_text_ct)
                    .load::<Option<Vec<u8>>>(conn)?)
            })
            .unwrap();

        let needle = b"a.txt";
        for ct in &cts {
            let bytes = ct.as_ref().expect("sealed column must be present");
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "sealed original_text must not carry plaintext path bytes"
            );
        }

        let relative_cts: Vec<Option<Vec<u8>>> = seed_exec
            .run(move |conn| {
                Ok(entry_file_set::table
                    .filter(entry_file_set::entry_id.eq("entry-1"))
                    .order(entry_file_set::line_index.asc())
                    .select(entry_file_set::relative_path_ct)
                    .load::<Option<Vec<u8>>>(conn)?)
            })
            .unwrap();
        for ct in relative_cts.iter().flatten() {
            assert!(
                !ct.windows(needle.len()).any(|window| window == needle),
                "sealed relative_path must not carry plaintext path bytes"
            );
        }

        let root_name_cts: Vec<Option<Vec<u8>>> = seed_exec
            .run(move |conn| {
                Ok(entry_file_set::table
                    .filter(entry_file_set::entry_id.eq("entry-1"))
                    .order(entry_file_set::line_index.asc())
                    .select(entry_file_set::root_name_ct)
                    .load::<Option<Vec<u8>>>(conn)?)
            })
            .unwrap();
        let root_name = b"root";
        for ct in root_name_cts.iter().flatten() {
            assert!(
                !ct.windows(root_name.len())
                    .any(|window| window == root_name),
                "sealed root_name must not carry plaintext path bytes"
            );
        }
    }

    #[tokio::test]
    async fn empty_manifest_save_succeeds_even_when_locked() {
        // An empty manifest is a pure DELETE and needs no key, so it must not be
        // blocked by a locked session (no path bytes to seal).
        let (repo, seed_exec, _tempdir) = make_repo_with_subkey(Arc::new(LockedSubkey));
        seed_entry(&seed_exec, "entry-1");
        let entry_id = EntryId::from("entry-1");
        repo.save(&entry_id, &EntryFileSet { lines: vec![] })
            .await
            .expect("empty manifest save must not require a key");
        assert!(repo.load(&entry_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_fails_when_session_locked() {
        let (repo, seed_exec, _tempdir) = make_repo_with_subkey(Arc::new(LockedSubkey));
        seed_entry(&seed_exec, "entry-1");
        let result = repo
            .save(&EntryId::from("entry-1"), &sample_file_set())
            .await;
        assert!(
            matches!(result, Err(EntryFileSetError::Storage(_))),
            "locked session must surface a Storage error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn load_fails_when_session_locked() {
        // Seed a row with a working key, then attempt to read it with a locked
        // session — the load must error rather than return garbage.
        let (repo, seed_exec, tempdir) = make_repo();
        seed_entry(&seed_exec, "entry-1");
        let entry_id = EntryId::from("entry-1");
        repo.save(&entry_id, &sample_file_set()).await.unwrap();

        let path = tempdir.path().join("file-set-repo.sqlite");
        let locked_repo = DieselEntryFileSetRepository::new(
            DieselSqliteExecutor::new(init_db_pool(path.to_str().unwrap()).unwrap()),
            Arc::new(LockedSubkey),
            Arc::new(FixedProfile),
        );
        let result = locked_repo.load(&entry_id).await;
        assert!(
            matches!(result, Err(EntryFileSetError::Storage(_))),
            "locked session must surface a Storage error on load, got {result:?}"
        );
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
