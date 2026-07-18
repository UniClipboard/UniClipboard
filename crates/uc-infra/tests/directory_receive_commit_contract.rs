use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tempfile::tempdir;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardEntryContentCategory, ClipboardEvent, ClipboardSelection,
    ClipboardSelectionDecision, ContentHash, EntryFileSet, EntryFileSetLine, EntryFileSetLineKind,
    FileSetMemberKind, FileSetMemberLocation, HashAlgorithm, MimeType,
    PersistedClipboardRepresentation, SelectionPolicyVersion,
};
use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, ProfileId, RepresentationId};
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    AttemptState, BeginReceiveAttemptPort, ClaimReceiveCommitPort, CommitInboundReceivePort,
    CompletedReceiveArtifacts, InboundReceiveCommitError, InboundReceiveRecord,
    InboundReceiveSettlement, PublishPhase, RecordDirectoryPublishPort,
};
use uc_core::SnapshotHash;
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::models::snapshot_representation::NewSnapshotRepresentationRow;
use uc_infra::db::models::{NewClipboardEntryRow, NewClipboardEventRow, NewClipboardSelectionRow};
use uc_infra::db::pool::init_db_pool;
use uc_infra::db::ports::DbExecutor;
use uc_infra::db::repositories::{
    DieselDirectoryPublishLogRepository, DieselEntryReceiveAttemptRepository,
    DieselInboundReceiveCommitRepository,
};
use uc_infra::db::schema::{
    clipboard_entry, clipboard_event, clipboard_selection, clipboard_snapshot_representation,
    directory_publish_log, entry_file_set, entry_receive_attempt, file_transfer,
};

struct FixedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for FixedSubkey {
    async fn derive_subkey(
        &self,
        _salt: &[u8],
        _info: &[u8],
    ) -> Result<[u8; 32], SpaceAccessError> {
        Ok([11; 32])
    }
}

struct FixedProfile;

#[async_trait]
impl CurrentProfilePort for FixedProfile {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
        Ok(ProfileId::from("default"))
    }
}

fn selection(entry_id: &str) -> ClipboardSelectionDecision {
    ClipboardSelectionDecision::new(
        EntryId::from(entry_id),
        ClipboardSelection {
            primary_rep_id: RepresentationId::from("rep"),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: RepresentationId::from("rep"),
            paste_rep_id: RepresentationId::from("rep"),
            policy_version: SelectionPolicyVersion::V1,
        },
    )
}

fn file_set() -> EntryFileSet {
    EntryFileSet {
        lines: vec![EntryFileSetLine {
            line_index: 1,
            original_text: "/tmp/private-root".to_owned(),
            member_location: Some(FileSetMemberLocation {
                root_index: 0,
                root_name: "private-root".to_owned(),
                relative_path: "member.txt".to_owned(),
                kind: FileSetMemberKind::File,
            }),
            kind: EntryFileSetLineKind::File {
                content_hash: ContentHash {
                    alg: HashAlgorithm::Blake3V1,
                    bytes: [3; 32],
                },
                blob_id: None,
                size_bytes: Some(5),
            },
        }],
    }
}

fn create_event() -> ClipboardEvent {
    ClipboardEvent::new(
        EventId::from("event"),
        1,
        DeviceId::new("peer"),
        SnapshotHash::parse(&format!("blake3v1:{}", "00".repeat(32))).unwrap(),
    )
}

fn create_representations() -> Vec<PersistedClipboardRepresentation> {
    vec![PersistedClipboardRepresentation::new(
        RepresentationId::from("rep"),
        FormatId::from("text/uri-list"),
        Some(MimeType("text/uri-list".to_owned())),
        5,
        Some(b"file:///tmp/private-root".to_vec()),
        None,
    )]
}

#[tokio::test]
async fn create_commit_atomically_publishes_entry_manifest_attempt_and_log() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("commit.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let publish = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey.clone(),
        profile.clone(),
    );
    let commit_repo = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );
    let writer = DieselSqliteExecutor::new(pool);
    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    assert!(attempts
        .claim_receive_commit("entry", "a1", 2)
        .await
        .unwrap());
    publish
        .record_phase(
            "entry",
            "a1",
            PublishPhase::Publishing,
            &[("staged".into(), "final".into())],
            2,
        )
        .await
        .unwrap();

    commit_repo
        .commit_inbound_receive(&InboundReceiveSettlement::Complete {
            record: InboundReceiveRecord::Create {
                entry: ClipboardEntry::new(EntryId::from("entry"), EventId::from("event"), 1, 5),
                event: create_event(),
                representations: create_representations(),
                selection: selection("entry"),
            },
            file_set: Some(file_set()),
            artifacts: CompletedReceiveArtifacts::DirectoryPublished,
            attempt_id: "a1".to_owned(),
            now_ms: 3,
        })
        .await
        .unwrap();

    writer
        .run(|conn| {
            assert_eq!(clipboard_entry::table.count().get_result::<i64>(conn)?, 1);
            assert_eq!(clipboard_event::table.count().get_result::<i64>(conn)?, 1);
            assert_eq!(
                clipboard_snapshot_representation::table
                    .count()
                    .get_result::<i64>(conn)?,
                1
            );
            assert_eq!(entry_file_set::table.count().get_result::<i64>(conn)?, 1);
            let state = entry_receive_attempt::table
                .select(entry_receive_attempt::attempt_state)
                .first::<String>(conn)?;
            assert_eq!(state, AttemptState::Completed.to_string());
            let (phase, landed) = directory_publish_log::table
                .select((directory_publish_log::phase, directory_publish_log::landed))
                .first::<(String, bool)>(conn)?;
            assert_eq!(phase, PublishPhase::Landed.to_string());
            assert!(landed);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn missing_publish_log_rolls_back_entry_and_manifest_after_their_inserts() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("rollback.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let commit_repo = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );
    let writer = DieselSqliteExecutor::new(pool);
    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    assert!(attempts
        .claim_receive_commit("entry", "a1", 2)
        .await
        .unwrap());

    let result = commit_repo
        .commit_inbound_receive(&InboundReceiveSettlement::Complete {
            record: InboundReceiveRecord::Create {
                entry: ClipboardEntry::new(EntryId::from("entry"), EventId::from("event"), 1, 5),
                event: create_event(),
                representations: create_representations(),
                selection: selection("entry"),
            },
            file_set: Some(file_set()),
            artifacts: CompletedReceiveArtifacts::DirectoryPublished,
            attempt_id: "a1".to_owned(),
            now_ms: 3,
        })
        .await;
    assert!(matches!(
        result,
        Err(InboundReceiveCommitError::DirectoryPublishRecordMissing)
    ));

    writer
        .run(|conn| {
            assert_eq!(clipboard_entry::table.count().get_result::<i64>(conn)?, 0);
            assert_eq!(clipboard_event::table.count().get_result::<i64>(conn)?, 0);
            assert_eq!(
                clipboard_snapshot_representation::table
                    .count()
                    .get_result::<i64>(conn)?,
                0
            );
            assert_eq!(entry_file_set::table.count().get_result::<i64>(conn)?, 0);
            let state = entry_receive_attempt::table
                .select(entry_receive_attempt::attempt_state)
                .first::<String>(conn)?;
            assert_eq!(state, AttemptState::Committing.to_string());
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn replace_commit_preserves_attempt_transfers_and_removes_legacy_transfers() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("replace.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let publish = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey.clone(),
        profile.clone(),
    );
    let commit_repo = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );
    let writer = DieselSqliteExecutor::new(pool);
    writer
        .run(|conn| {
            diesel::insert_into(clipboard_event::table)
                .values(NewClipboardEventRow {
                    event_id: "old-event".to_owned(),
                    captured_at_ms: 1,
                    source_device: "peer".to_owned(),
                    snapshot_hash: format!("blake3v1:{}", "11".repeat(32)),
                })
                .execute(conn)?;
            diesel::insert_into(clipboard_entry::table)
                .values(NewClipboardEntryRow {
                    entry_id: "entry".to_owned(),
                    event_id: "old-event".to_owned(),
                    created_at_ms: 1,
                    active_time_ms: 1,
                    total_size: 3,
                    pinned: false,
                    delivery_tracked: false,
                    is_favorited: false,
                    content_category: "text".to_owned(),
                })
                .execute(conn)?;
            diesel::insert_into(clipboard_snapshot_representation::table)
                .values(NewSnapshotRepresentationRow {
                    id: "old-rep".to_owned(),
                    event_id: "old-event".to_owned(),
                    format_id: "text".to_owned(),
                    mime_type: Some("text/plain".to_owned()),
                    size_bytes: 3,
                    inline_data: Some(b"old".to_vec()),
                    blob_id: None,
                    payload_state: "Inline".to_owned(),
                    last_error: None,
                })
                .execute(conn)?;
            diesel::insert_into(clipboard_selection::table)
                .values(NewClipboardSelectionRow {
                    entry_id: "entry".to_owned(),
                    primary_rep_id: "old-rep".to_owned(),
                    secondary_rep_ids: String::new(),
                    preview_rep_id: "old-rep".to_owned(),
                    paste_rep_id: "old-rep".to_owned(),
                    policy_version: "v1".to_owned(),
                })
                .execute(conn)?;
            diesel::sql_query(
                "INSERT INTO file_transfer \
                 (transfer_id, entry_id, attempt_id, binding_state, status, source_device, \
                  metadata_ciphertext, created_at_ms, updated_at_ms) VALUES \
                 ('directory-member', 'entry', 'a1', 'attempt_bound', 'completed', 'peer', X'01', 1, 1), \
                 ('legacy-member', 'entry', NULL, 'legacy', 'completed', 'peer', X'01', 1, 1)",
            )
            .execute(conn)?;
            Ok(())
        })
        .unwrap();
    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    assert!(attempts
        .claim_receive_commit("entry", "a1", 2)
        .await
        .unwrap());
    publish
        .record_phase("entry", "a1", PublishPhase::Publishing, &[], 2)
        .await
        .unwrap();

    let new_event = ClipboardEvent::new(
        EventId::from("new-event"),
        2,
        DeviceId::new("peer".to_owned()),
        SnapshotHash::parse(&format!("blake3v1:{}", "22".repeat(32))).unwrap(),
    );
    let new_representations = vec![PersistedClipboardRepresentation::new(
        RepresentationId::from("new-rep"),
        FormatId::from("text/uri-list"),
        Some(MimeType("text/uri-list".to_owned())),
        5,
        Some(b"file:///tmp/private-root".to_vec()),
        None,
    )];
    let new_selection = ClipboardSelectionDecision::new(
        EntryId::from("entry"),
        ClipboardSelection {
            primary_rep_id: RepresentationId::from("new-rep"),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: RepresentationId::from("new-rep"),
            paste_rep_id: RepresentationId::from("new-rep"),
            policy_version: SelectionPolicyVersion::V1,
        },
    );
    commit_repo
        .commit_inbound_receive(&InboundReceiveSettlement::Complete {
            record: InboundReceiveRecord::Replace {
                entry_id: EntryId::from("entry"),
                new_event,
                new_representations,
                new_selection,
                new_total_size: 5,
                new_content_category: ClipboardEntryContentCategory::File,
            },
            file_set: Some(file_set()),
            artifacts: CompletedReceiveArtifacts::DirectoryPublished,
            attempt_id: "a1".to_owned(),
            now_ms: 3,
        })
        .await
        .unwrap();

    writer
        .run(|conn| {
            let event_id = clipboard_entry::table
                .select(clipboard_entry::event_id)
                .first::<String>(conn)?;
            assert_eq!(event_id, "new-event");
            let transfer_ids = file_transfer::table
                .select(file_transfer::transfer_id)
                .order(file_transfer::transfer_id.asc())
                .load::<String>(conn)?;
            assert_eq!(transfer_ids, vec!["directory-member"]);
            let state = entry_receive_attempt::table
                .select(entry_receive_attempt::attempt_state)
                .first::<String>(conn)?;
            assert_eq!(state, AttemptState::Completed.to_string());
            Ok(())
        })
        .unwrap();
}
