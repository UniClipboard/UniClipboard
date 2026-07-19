use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tempfile::tempdir;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardEvent, ClipboardSelection, ClipboardSelectionDecision, MimeType,
    PersistedClipboardRepresentation, SelectionPolicyVersion,
};
use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, ProfileId, RepresentationId};
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    BeginReceiveAttemptPort, BeginReceiveFailurePort, ClaimReceiveCommitPort,
    CommitInboundReceivePort, CompletedReceiveArtifacts, GetEntryAttemptPort,
    InboundReceiveCommitError, InboundReceiveRecord, InboundReceiveSettlement,
    NoEntryReceiveArtifacts, PartialReceiveTerminal, ReceiveArtifact, ReceiveArtifactOwnership,
    ReceiveArtifactPhase, ReceiveArtifactRecord, ReceiveArtifactResolution,
    RecordReceiveArtifactsPort, RequestReceiveCancellationPort,
};
use uc_core::SnapshotHash;
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::pool::init_db_pool;
use uc_infra::db::ports::DbExecutor;
use uc_infra::db::repositories::{
    DieselEntryReceiveAttemptRepository, DieselInboundReceiveCommitRepository,
    DieselReceiveArtifactLogRepository,
};
use uc_infra::db::schema::{clipboard_entry, entry_receive_attempt, receive_artifact_log};

struct FixedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for FixedSubkey {
    async fn derive_subkey(&self, _salt: &[u8], info: &[u8]) -> Result<[u8; 32], SpaceAccessError> {
        Ok(if info.ends_with(b"file-set/v1") {
            [3; 32]
        } else {
            [4; 32]
        })
    }
}

struct FixedProfile;

#[async_trait]
impl CurrentProfilePort for FixedProfile {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
        Ok(ProfileId::from("test"))
    }
}

fn record(entry_id: &str) -> InboundReceiveRecord {
    let event_id = EventId::from(format!("event-{entry_id}"));
    let rep_id = RepresentationId::from(format!("rep-{entry_id}"));
    let event = ClipboardEvent::new(
        event_id.clone(),
        1,
        DeviceId::new("peer"),
        SnapshotHash::parse(&format!("blake3v1:{}", "00".repeat(32))).unwrap(),
    );
    let representations = vec![PersistedClipboardRepresentation::new(
        rep_id.clone(),
        FormatId::from("text"),
        Some(MimeType("text/plain".to_owned())),
        5,
        Some(b"hello".to_vec()),
        None,
    )];
    let selection = ClipboardSelectionDecision::new(
        EntryId::from(entry_id),
        ClipboardSelection {
            primary_rep_id: rep_id.clone(),
            secondary_rep_ids: Vec::new(),
            preview_rep_id: rep_id.clone(),
            paste_rep_id: rep_id,
            policy_version: SelectionPolicyVersion::V1,
        },
    );
    InboundReceiveRecord::Create {
        entry: ClipboardEntry::new(EntryId::from(entry_id), event_id, 1, 5),
        event,
        representations,
        selection,
    }
}

#[tokio::test]
async fn complete_create_and_missing_artifact_are_atomic() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("common-commit.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let commit = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );
    let reader = DieselSqliteExecutor::new(pool);

    attempts
        .begin_first_receive("entry-ok", "a1", 1)
        .await
        .unwrap();
    assert!(attempts
        .claim_receive_commit("entry-ok", "a1", 2)
        .await
        .unwrap());
    commit
        .commit_inbound_receive(&InboundReceiveSettlement::Complete {
            record: record("entry-ok"),
            attempt_id: "a1".to_owned(),
            file_set: None,
            artifacts: CompletedReceiveArtifacts::None,
            now_ms: 3,
        })
        .await
        .unwrap();

    attempts
        .begin_first_receive("entry-rollback", "a2", 1)
        .await
        .unwrap();
    assert!(attempts
        .claim_receive_commit("entry-rollback", "a2", 2)
        .await
        .unwrap());
    let result = commit
        .commit_inbound_receive(&InboundReceiveSettlement::Complete {
            record: record("entry-rollback"),
            attempt_id: "a2".to_owned(),
            file_set: None,
            artifacts: CompletedReceiveArtifacts::Landed,
            now_ms: 3,
        })
        .await;
    assert!(matches!(
        result,
        Err(InboundReceiveCommitError::ArtifactRecordMissing)
    ));

    reader
        .run(|conn| {
            assert_eq!(
                clipboard_entry::table
                    .filter(clipboard_entry::entry_id.eq("entry-ok"))
                    .count()
                    .get_result::<i64>(conn)?,
                1
            );
            assert_eq!(
                clipboard_entry::table
                    .filter(clipboard_entry::entry_id.eq("entry-rollback"))
                    .count()
                    .get_result::<i64>(conn)?,
                0
            );
            let states = entry_receive_attempt::table
                .order(entry_receive_attempt::entry_id.asc())
                .select((
                    entry_receive_attempt::entry_id,
                    entry_receive_attempt::attempt_state,
                ))
                .load::<(String, String)>(conn)?;
            assert_eq!(states[0], ("entry-ok".to_owned(), "completed".to_owned()));
            assert_eq!(
                states[1],
                ("entry-rollback".to_owned(), "committing".to_owned())
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn no_entry_cancel_settles_attempt_and_artifacts_without_history() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("no-entry.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let artifacts = DieselReceiveArtifactLogRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey.clone(),
        profile.clone(),
    );
    let commit = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );
    let reader = DieselSqliteExecutor::new(pool);
    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    attempts
        .request_receive_cancellation("entry", "a1", 2)
        .await
        .unwrap();
    artifacts
        .record_receive_artifacts(&ReceiveArtifactRecord {
            entry_id: "entry".to_owned(),
            attempt_id: "a1".to_owned(),
            phase: ReceiveArtifactPhase::RollingBack,
            resolution: ReceiveArtifactResolution::Pending,
            artifacts: vec![ReceiveArtifact {
                item_id: "item".to_owned(),
                staged_path: "/tmp/staged".into(),
                final_path: "/tmp/final".into(),
                ownership: ReceiveArtifactOwnership::ManagedStaging,
            }],
            updated_at_ms: 2,
        })
        .await
        .unwrap();

    commit
        .commit_inbound_receive(&InboundReceiveSettlement::NoEntry {
            entry_id: EntryId::from("entry"),
            attempt_id: "a1".to_owned(),
            terminal: PartialReceiveTerminal::Cancelled,
            artifacts: NoEntryReceiveArtifacts::RolledBack,
            now_ms: 3,
        })
        .await
        .unwrap();

    reader
        .run(|conn| {
            assert_eq!(clipboard_entry::table.count().get_result::<i64>(conn)?, 0);
            assert_eq!(
                entry_receive_attempt::table
                    .select(entry_receive_attempt::attempt_state)
                    .first::<String>(conn)?,
                "cancelled"
            );
            assert_eq!(
                receive_artifact_log::table
                    .select(receive_artifact_log::resolution)
                    .first::<String>(conn)?,
                "rolled_back"
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn stale_old_attempt_settlement_cannot_change_a_redelivery() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("stale-settlement.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    let attempts =
        DieselEntryReceiveAttemptRepository::new(DieselSqliteExecutor::new(pool.clone()));
    let commit = DieselInboundReceiveCommitRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        subkey,
        profile,
    );

    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    assert_eq!(
        attempts
            .begin_receive_failure("entry", "a1", 2)
            .await
            .unwrap(),
        uc_core::ports::BeginReceiveFailureOutcome::Begun
    );
    commit
        .commit_inbound_receive(&InboundReceiveSettlement::Partial {
            record: record("entry"),
            attempt_id: "a1".to_owned(),
            terminal: PartialReceiveTerminal::Failed,
            file_set: None,
            artifacts: uc_core::ports::PartialReceiveArtifacts::None,
            now_ms: 3,
        })
        .await
        .unwrap();
    assert_eq!(
        attempts
            .begin_redelivery("entry", "a1", "a2", 4)
            .await
            .unwrap(),
        uc_core::ports::BeginReceiveOutcome::Begun
    );

    let stale = commit
        .commit_inbound_receive(&InboundReceiveSettlement::NoEntry {
            entry_id: EntryId::from("entry"),
            attempt_id: "a1".to_owned(),
            terminal: PartialReceiveTerminal::Failed,
            artifacts: NoEntryReceiveArtifacts::None,
            now_ms: 5,
        })
        .await;
    assert!(matches!(
        stale,
        Err(InboundReceiveCommitError::AttemptStateMismatch)
    ));

    let current = attempts.get_entry_attempt("entry").await.unwrap().unwrap();
    assert_eq!(current.current_attempt_id, "a2");
    assert_eq!(current.state, uc_core::ports::AttemptState::Receiving);
}
