//! Adapter for [`CheckEntryAvailabilityPort`]: answers "is this entry's content
//! fully held and usable locally?" by reading representation state from SQLite
//! and stat-ing the local files a file entry points at. Availability is
//! computed live on every call (never read from a denormalized column), because
//! representation state is rewritten asynchronously by materialization and
//! reconciliation.

use std::path::{Path, PathBuf};

use diesel::prelude::*;
use tracing::instrument;
use uc_core::clipboard::ClipboardRepositoryError;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::CheckEntryAvailabilityPort;

use crate::db::ports::DbExecutor;
use crate::db::schema::{clipboard_entry, clipboard_snapshot_representation};

/// Representation columns availability depends on.
type RepRow = (Option<String>, Option<Vec<u8>>, String);

pub struct DieselEntryAvailabilityRepository<E> {
    executor: E,
}

impl<E> DieselEntryAvailabilityRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl<E> CheckEntryAvailabilityPort for DieselEntryAvailabilityRepository<E>
where
    E: DbExecutor,
{
    #[instrument(
        name = "infra.sqlite.is_entry_available",
        skip_all,
        fields(operation = "is_entry_available", entry_id = %entry_id)
    )]
    async fn is_entry_available(
        &self,
        entry_id: &EntryId,
    ) -> Result<bool, ClipboardRepositoryError> {
        let entry_id_str = entry_id.to_string();

        // Read the representation rows in the DB closure, then release the
        // connection before doing any filesystem checks — availability is not a
        // hot path and a pooled connection must not be held across file I/O.
        let reps: Option<Vec<RepRow>> = self
            .executor
            .run(move |conn| {
                let event_id: Option<String> = clipboard_entry::table
                    .filter(clipboard_entry::entry_id.eq(&entry_id_str))
                    .select(clipboard_entry::event_id)
                    .first::<String>(conn)
                    .optional()?;
                let Some(event_id) = event_id else {
                    return Ok(None);
                };
                let reps = clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::event_id.eq(&event_id))
                    .select((
                        clipboard_snapshot_representation::mime_type,
                        clipboard_snapshot_representation::inline_data,
                        clipboard_snapshot_representation::payload_state,
                    ))
                    .load::<RepRow>(conn)?;
                Ok(Some(reps))
            })
            .map_err(|e| ClipboardRepositoryError::Storage(e.to_string()))?;

        // An entry that does not exist is never "available".
        let Some(reps) = reps else {
            return Ok(false);
        };
        Ok(reps_indicate_available(&reps))
    }
}

/// An entry is available iff it has at least one representation and none of its
/// representations indicate missing content: no `Failed`/`Lost` payload state,
/// and — for file-list representations — no `uniclip-missing://` placeholder and
/// every referenced local file present.
fn reps_indicate_available(reps: &[RepRow]) -> bool {
    if reps.is_empty() {
        return false;
    }
    for (mime, inline, payload_state) in reps {
        if payload_state == "Failed" || payload_state == "Lost" {
            return false;
        }
        let is_uri_list = mime
            .as_deref()
            .map(|m| m.contains("uri-list"))
            .unwrap_or(false);
        if is_uri_list {
            // A file-list rep with no inline body cannot be confirmed held.
            let Some(bytes) = inline.as_deref() else {
                return false;
            };
            if !file_list_is_fully_present(bytes) {
                return false;
            }
        }
    }
    true
}

/// A file-list body is "fully present" when it carries no `uniclip-missing://`
/// placeholder (left by a cancelled / partial transfer) and every `file://`
/// line points at a local file that exists, is readable, and is a regular file.
fn file_list_is_fully_present(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("uniclip-missing:") {
            return false;
        }
        if let Some(path) = file_uri_to_path(line) {
            if !path_is_readable_regular_file(&path) {
                return false;
            }
        } else if line.starts_with("file://") {
            // A `file://` line we cannot parse/convert to a local path: we can't
            // confirm the file is present, so treat the list as unavailable
            // rather than converging possibly-stale content.
            return false;
        }
        // Non-file URIs (http/data/custom) are not local files and impose no
        // local-availability requirement.
    }
    true
}

fn file_uri_to_path(line: &str) -> Option<PathBuf> {
    if line.starts_with("file://") {
        url::Url::parse(line)
            .ok()
            .and_then(|u| u.to_file_path().ok())
    } else if line.starts_with('/') || line.contains(":\\") {
        // Bare absolute path (some adapters store paths without a scheme).
        Some(PathBuf::from(line))
    } else {
        None
    }
}

fn path_is_readable_regular_file(path: &Path) -> bool {
    match std::fs::File::open(path) {
        Ok(file) => file.metadata().map(|m| m.is_file()).unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_placeholder_makes_file_list_unavailable() {
        let body = b"file:///tmp/a.txt\r\nuniclip-missing:///b.iso?size=10&reason=cancelled";
        assert!(!file_list_is_fully_present(body));
    }

    #[test]
    fn nonexistent_file_uri_is_unavailable() {
        let body = b"file:///definitely/not/here/zzz-uniclip-test.bin";
        assert!(!file_list_is_fully_present(body));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("present.txt");
        std::fs::write(&path, b"hi").unwrap();
        let uri = url::Url::from_file_path(&path).unwrap();
        let body = format!("# a comment\r\n\r\n{uri}\r\n");
        assert!(file_list_is_fully_present(body.as_bytes()));
    }

    #[test]
    fn existing_regular_file_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.bin");
        std::fs::write(&path, b"bytes").unwrap();
        let uri = url::Url::from_file_path(&path).unwrap();
        assert!(file_list_is_fully_present(uri.as_str().as_bytes()));
    }

    #[test]
    fn directory_target_is_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let uri = url::Url::from_file_path(dir.path()).unwrap();
        assert!(!file_list_is_fully_present(uri.as_str().as_bytes()));
    }

    #[test]
    fn failed_payload_state_is_unavailable() {
        let reps = vec![(Some("image/png".to_string()), None, "Failed".to_string())];
        assert!(!reps_indicate_available(&reps));
    }

    #[test]
    fn inline_text_entry_is_available() {
        let reps = vec![(
            Some("text/plain".to_string()),
            Some(b"hello".to_vec()),
            "Inline".to_string(),
        )];
        assert!(reps_indicate_available(&reps));
    }

    #[test]
    fn empty_rep_set_is_unavailable() {
        assert!(!reps_indicate_available(&[]));
    }

    // --- SQLite integration: the full entry → event → reps path -------------

    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::models::snapshot_representation::NewSnapshotRepresentationRow;
    use crate::db::models::{NewClipboardEntryRow, NewClipboardEventRow};
    use crate::db::pool::init_db_pool;
    use std::sync::Arc;
    use uc_core::ids::EntryId as CoreEntryId;
    use uc_core::ports::clipboard::CheckEntryAvailabilityPort;

    fn make_repo() -> (
        DieselEntryAvailabilityRepository<Arc<DieselSqliteExecutor>>,
        Arc<DieselSqliteExecutor>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("availability.sqlite");
        let pool = init_db_pool(path.to_str().unwrap()).unwrap();
        let executor = Arc::new(DieselSqliteExecutor::new(pool));
        let repo = DieselEntryAvailabilityRepository::new(Arc::clone(&executor));
        (repo, executor, dir)
    }

    fn seed_uri_list_entry(executor: &Arc<DieselSqliteExecutor>, uri_list_body: &str) {
        let body = uri_list_body.to_string();
        executor
            .run(move |conn| {
                diesel::insert_into(crate::db::schema::clipboard_event::table)
                    .values(&NewClipboardEventRow {
                        event_id: "ev".into(),
                        captured_at_ms: 0,
                        source_device: "dev".into(),
                        snapshot_hash: "blake3v1:00".into(),
                    })
                    .execute(conn)?;
                diesel::insert_into(crate::db::schema::clipboard_entry::table)
                    .values(&NewClipboardEntryRow {
                        entry_id: "e1".into(),
                        event_id: "ev".into(),
                        created_at_ms: 0,
                        active_time_ms: 0,
                        title: None,
                        total_size: 0,
                        pinned: false,
                        delivery_tracked: true,
                        is_favorited: false,
                    })
                    .execute(conn)?;
                diesel::insert_into(crate::db::schema::clipboard_snapshot_representation::table)
                    .values(&NewSnapshotRepresentationRow {
                        id: "rep".into(),
                        event_id: "ev".into(),
                        format_id: "files".into(),
                        mime_type: Some("text/uri-list".into()),
                        size_bytes: body.len() as i64,
                        inline_data: Some(body.into_bytes()),
                        blob_id: None,
                        payload_state: "Inline".into(),
                        last_error: None,
                    })
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn file_entry_flips_to_unavailable_when_local_file_disappears() {
        let (repo, executor, dir) = make_repo();
        let file = dir.path().join("payload.bin");
        std::fs::write(&file, b"bytes").unwrap();
        let uri = url::Url::from_file_path(&file).unwrap();
        seed_uri_list_entry(&executor, uri.as_str());

        assert!(
            repo.is_entry_available(&CoreEntryId::from("e1"))
                .await
                .unwrap(),
            "present local file ⇒ available"
        );

        std::fs::remove_file(&file).unwrap();
        assert!(
            !repo
                .is_entry_available(&CoreEntryId::from("e1"))
                .await
                .unwrap(),
            "removed local file ⇒ not available (must not converge stale content)"
        );
    }

    #[tokio::test]
    async fn partial_placeholder_entry_is_unavailable() {
        let (repo, executor, _dir) = make_repo();
        seed_uri_list_entry(
            &executor,
            "uniclip-missing:///big.iso?size=950000000&reason=cancelled",
        );
        assert!(
            !repo
                .is_entry_available(&CoreEntryId::from("e1"))
                .await
                .unwrap(),
            "uniclip-missing placeholder ⇒ partial ⇒ not available"
        );
    }

    #[tokio::test]
    async fn absent_entry_is_unavailable() {
        let (repo, _executor, _dir) = make_repo();
        assert!(!repo
            .is_entry_available(&CoreEntryId::from("nope"))
            .await
            .unwrap());
    }
}
