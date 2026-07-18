use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tempfile::tempdir;
use uc_core::file_transfer::FileTransferCancellationReason;
use uc_core::ids::ProfileId;
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    AttemptError, AttemptState, BeginReceiveAttemptPort, BeginReceiveFailureOutcome,
    BeginReceiveFailurePort, BeginReceiveOutcome, CancelDirectoryAttemptTransfersPort,
    ClaimReceiveCommitPort, DeleteReceiveStateForEntryPort, FinalizeProvisionalReceivePort,
    GetEntryAttemptPort, GetEntryReceiveProgressPort, ListNonTerminalAttemptsPort,
    ListProvisionalReceivesPort, PendingInboundTransfer, ProvisionalInboundTransfer,
    ProvisionalReceiveAction, PurgeTerminalOrphanAttemptsPort, ReceiveItemRole,
    RecordReceiverTransferPort, RequestReceiveCancellationOutcome, RequestReceiveCancellationPort,
    SeedProvisionalReceivePort, TrackedFileTransferStatus, UpdateProvisionalReceivePathPort,
};
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::pool::init_db_pool;
use uc_infra::db::ports::DbExecutor;
use uc_infra::db::repositories::{
    DieselEntryReceiveAttemptRepository, DieselFileTransferRepository,
};
use uc_infra::db::schema::{entry_receive_attempt, file_transfer, receive_artifact_log};

type AttemptRepo = DieselEntryReceiveAttemptRepository<DieselSqliteExecutor>;
type TransferRepo = DieselFileTransferRepository<DieselSqliteExecutor>;

struct FixedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for FixedSubkey {
    async fn derive_subkey(&self, _salt: &[u8], info: &[u8]) -> Result<[u8; 32], SpaceAccessError> {
        Ok(if info.ends_with(b"metadata/v1") {
            [0x5A; 32]
        } else {
            [0xA5; 32]
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

fn repos() -> (
    AttemptRepo,
    TransferRepo,
    DieselSqliteExecutor,
    tempfile::TempDir,
) {
    let directory = tempdir().unwrap();
    let database = directory.path().join("attempt.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let derive_subkey: Arc<dyn DeriveSpaceSubkeyPort> = Arc::new(FixedSubkey);
    let current_profile: Arc<dyn CurrentProfilePort> = Arc::new(FixedProfile);
    (
        AttemptRepo::new(DieselSqliteExecutor::new(pool.clone())),
        TransferRepo::new(
            DieselSqliteExecutor::new(pool.clone()),
            derive_subkey,
            current_profile,
        ),
        DieselSqliteExecutor::new(pool),
        directory,
    )
}

async fn seed_transfer(
    repo: &TransferRepo,
    transfer_id: &str,
    entry_id: &str,
    attempt_id: Option<&str>,
    file_size: Option<i64>,
) {
    repo.upsert_pending_transfer(&PendingInboundTransfer {
        transfer_id: transfer_id.to_owned(),
        entry_id: entry_id.to_owned(),
        attempt_id: attempt_id.map(str::to_owned),
        origin_device_id: "peer-a".to_owned(),
        filename: "member.bin".to_owned(),
        file_size,
        cached_path: String::new(),
        created_at_ms: 1,
    })
    .await
    .unwrap();
}

fn set_completed(writer: &DieselSqliteExecutor, transfer_id: &str) {
    let transfer_id = transfer_id.to_owned();
    writer
        .run(move |conn| {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set(file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()))
                .execute(conn)?;
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn provisional_mobile_transfer_is_adopted_only_by_the_exact_receiving_attempt() {
    let (attempts, transfers, writer, _directory) = repos();
    transfers
        .seed_provisional_receive(&ProvisionalInboundTransfer {
            transfer_id: "mobile-1".to_owned(),
            origin_device_id: "mobile:phone".to_owned(),
            filename: "photo.jpg".to_owned(),
            file_size: Some(42),
            created_at_ms: 1,
        })
        .await
        .unwrap();
    attempts
        .begin_first_receive("entry-1", "attempt-1", 2)
        .await
        .unwrap();

    let stale = transfers
        .finalize_provisional_receive(
            "mobile-1",
            ProvisionalReceiveAction::AdoptIntoAttempt {
                entry_id: "entry-1".to_owned(),
                attempt_id: "stale".to_owned(),
                item_id: "mobile-item-1".to_owned(),
                role: ReceiveItemRole::Representation,
            },
            3,
        )
        .await;
    assert!(stale.is_err());

    transfers
        .finalize_provisional_receive(
            "mobile-1",
            ProvisionalReceiveAction::AdoptIntoAttempt {
                entry_id: "entry-1".to_owned(),
                attempt_id: "attempt-1".to_owned(),
                item_id: "mobile-item-1".to_owned(),
                role: ReceiveItemRole::Representation,
            },
            4,
        )
        .await
        .unwrap();

    let row = writer
        .run(|conn| {
            file_transfer::table
                .filter(file_transfer::transfer_id.eq("mobile-1"))
                .select((
                    file_transfer::entry_id,
                    file_transfer::attempt_id,
                    file_transfer::binding_state,
                    file_transfer::receive_item_id,
                    file_transfer::item_role,
                ))
                .first::<(
                    Option<String>,
                    Option<String>,
                    String,
                    Option<String>,
                    Option<String>,
                )>(conn)
                .map_err(anyhow::Error::from)
        })
        .unwrap();
    assert_eq!(
        row,
        (
            Some("entry-1".to_owned()),
            Some("attempt-1".to_owned()),
            "attempt_bound".to_owned(),
            Some("mobile-item-1".to_owned()),
            Some("representation".to_owned()),
        )
    );
}

#[tokio::test]
async fn fully_held_mobile_transfer_is_discarded_without_an_attempt() {
    let (_attempts, transfers, writer, _directory) = repos();
    transfers
        .seed_provisional_receive(&ProvisionalInboundTransfer {
            transfer_id: "mobile-duplicate".to_owned(),
            origin_device_id: "mobile:phone".to_owned(),
            filename: "duplicate.bin".to_owned(),
            file_size: Some(7),
            created_at_ms: 1,
        })
        .await
        .unwrap();
    transfers
        .finalize_provisional_receive(
            "mobile-duplicate",
            ProvisionalReceiveAction::DiscardAsFullyHeld,
            2,
        )
        .await
        .unwrap();

    let remaining = writer
        .run(|conn| {
            file_transfer::table
                .filter(file_transfer::transfer_id.eq("mobile-duplicate"))
                .count()
                .get_result::<i64>(conn)
                .map_err(anyhow::Error::from)
        })
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn provisional_mobile_recovery_path_round_trips_only_through_encrypted_metadata() {
    let (_attempts, transfers, writer, _directory) = repos();
    transfers
        .seed_provisional_receive(&ProvisionalInboundTransfer {
            transfer_id: "mobile-orphan".to_owned(),
            origin_device_id: "mobile:phone".to_owned(),
            filename: "orphan.bin".to_owned(),
            file_size: Some(7),
            created_at_ms: 1,
        })
        .await
        .unwrap();
    let uri = "file:///tmp/mobile-provisional-private-sentinel.bin";
    transfers
        .update_provisional_receive_path("mobile-orphan", uri, 2)
        .await
        .unwrap();

    assert_eq!(
        transfers.list_provisional_receives().await.unwrap(),
        vec![uc_core::ports::ProvisionalReceiveRecovery {
            transfer_id: "mobile-orphan".to_owned(),
            cached_path: Some(uri.to_owned()),
        }]
    );
    let ciphertext = writer
        .run(|conn| {
            file_transfer::table
                .filter(file_transfer::transfer_id.eq("mobile-orphan"))
                .select(file_transfer::metadata_ciphertext)
                .first::<Vec<u8>>(conn)
                .map_err(anyhow::Error::from)
        })
        .unwrap();
    assert!(!ciphertext
        .windows(uri.len())
        .any(|window| window == uri.as_bytes()));
}

#[tokio::test]
async fn generic_receive_state_machine_resolves_competing_settlements() {
    let (repo, _transfers, writer, _directory) = repos();
    assert_eq!(
        repo.begin_first_receive("entry", "a1", 1).await.unwrap(),
        BeginReceiveOutcome::Begun
    );
    assert_eq!(
        repo.begin_first_receive("entry", "other", 2).await.unwrap(),
        BeginReceiveOutcome::AlreadyReceiving
    );
    assert_eq!(
        repo.request_receive_cancellation("entry", "stale", 3)
            .await
            .unwrap(),
        RequestReceiveCancellationOutcome::Superseded
    );
    assert_eq!(
        repo.request_receive_cancellation("entry", "a1", 4)
            .await
            .unwrap(),
        RequestReceiveCancellationOutcome::Requested
    );
    assert_eq!(
        repo.begin_receive_failure("entry", "a1", 5).await.unwrap(),
        BeginReceiveFailureOutcome::CancellationWon
    );
    assert_eq!(
        repo.request_receive_cancellation("entry", "a1", 6)
            .await
            .unwrap(),
        RequestReceiveCancellationOutcome::AlreadyCancelling
    );

    writer
        .run(|conn| {
            diesel::update(
                entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq("entry")),
            )
            .set(entry_receive_attempt::attempt_state.eq(AttemptState::Cancelled.to_string()))
            .execute(conn)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        repo.begin_redelivery("entry", "a1", "a2", 7).await.unwrap(),
        BeginReceiveOutcome::Begun
    );
    assert!(repo.claim_receive_commit("entry", "a2", 8).await.unwrap());
    assert_eq!(
        repo.request_receive_cancellation("entry", "a2", 9)
            .await
            .unwrap(),
        RequestReceiveCancellationOutcome::TooLate
    );
    assert_eq!(
        repo.begin_receive_failure("entry", "a2", 10).await.unwrap(),
        BeginReceiveFailureOutcome::Begun
    );

    let current = repo.get_entry_attempt("entry").await.unwrap().unwrap();
    assert_eq!(current.current_attempt_id, "a2");
    assert_eq!(current.state, AttemptState::Failing);
    assert_eq!(
        repo.list_non_terminal_attempts().await.unwrap(),
        vec![current]
    );
}

#[tokio::test]
async fn concurrent_cancel_and_commit_claim_have_exactly_one_winner() {
    let (repo, _transfers, _writer, _directory) = repos();
    let repo = Arc::new(repo);
    repo.begin_first_receive("entry", "a1", 1).await.unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(3));

    let commit = {
        let repo = Arc::clone(&repo);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            repo.claim_receive_commit("entry", "a1", 2).await.unwrap()
        })
    };
    let cancel = {
        let repo = Arc::clone(&repo);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            repo.request_receive_cancellation("entry", "a1", 2)
                .await
                .unwrap()
        })
    };
    barrier.wait().await;

    let commit_won = commit.await.unwrap();
    let cancel_outcome = cancel.await.unwrap();
    assert_ne!(
        commit_won,
        cancel_outcome == RequestReceiveCancellationOutcome::Requested
    );
    let current = repo.get_entry_attempt("entry").await.unwrap().unwrap();
    if commit_won {
        assert_eq!(cancel_outcome, RequestReceiveCancellationOutcome::TooLate);
        assert_eq!(current.state, AttemptState::Committing);
    } else {
        assert_eq!(cancel_outcome, RequestReceiveCancellationOutcome::Requested);
        assert_eq!(current.state, AttemptState::Cancelling);
    }
}

#[tokio::test]
async fn redelivery_replaces_only_failed_or_cancelled_attempts() {
    let (repo, _transfers, writer, _directory) = repos();
    repo.begin_first_receive("entry", "a1", 1).await.unwrap();
    assert_eq!(
        repo.begin_redelivery("entry", "a1", "a2", 2).await.unwrap(),
        BeginReceiveOutcome::AlreadyReceiving
    );

    writer
        .run(|conn| {
            diesel::update(
                entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq("entry")),
            )
            .set(entry_receive_attempt::attempt_state.eq(AttemptState::Completed.to_string()))
            .execute(conn)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        repo.begin_redelivery("entry", "a1", "a2", 3).await.unwrap(),
        BeginReceiveOutcome::Superseded
    );

    writer
        .run(|conn| {
            diesel::update(
                entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq("entry")),
            )
            .set(entry_receive_attempt::attempt_state.eq(AttemptState::Failed.to_string()))
            .execute(conn)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        repo.begin_redelivery("entry", "a1", "a2", 4).await.unwrap(),
        BeginReceiveOutcome::Begun
    );
    let current = repo.get_entry_attempt("entry").await.unwrap().unwrap();
    assert_eq!(current.current_attempt_id, "a2");
    assert_eq!(current.state, AttemptState::Receiving);
}

#[tokio::test]
async fn corrupted_attempt_state_is_rejected() {
    let (repo, _transfers, writer, _directory) = repos();
    repo.begin_first_receive("entry", "a1", 1).await.unwrap();
    writer
        .run(|conn| {
            diesel::update(
                entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq("entry")),
            )
            .set(entry_receive_attempt::attempt_state.eq("unknown"))
            .execute(conn)?;
            Ok(())
        })
        .unwrap();

    assert!(matches!(
        repo.get_entry_attempt("entry").await,
        Err(AttemptError::InvalidState(state)) if state == "unknown"
    ));
    assert!(matches!(
        repo.request_receive_cancellation("entry", "a1", 2).await,
        Err(AttemptError::InvalidState(state)) if state == "unknown"
    ));
}

#[tokio::test]
async fn receive_progress_uses_only_the_current_attempt() {
    let (attempts, transfers, writer, _directory) = repos();
    attempts
        .begin_first_receive("entry", "a1", 1)
        .await
        .unwrap();
    seed_transfer(&transfers, "old-member", "entry", Some("a1"), Some(7)).await;
    set_completed(&writer, "old-member");

    writer
        .run(|conn| {
            diesel::update(
                entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq("entry")),
            )
            .set(entry_receive_attempt::attempt_state.eq(AttemptState::Failed.to_string()))
            .execute(conn)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        attempts
            .begin_redelivery("entry", "a1", "a2", 2)
            .await
            .unwrap(),
        BeginReceiveOutcome::Begun
    );
    seed_transfer(&transfers, "new-pending", "entry", Some("a2"), Some(20)).await;
    seed_transfer(&transfers, "new-complete", "entry", Some("a2"), Some(30)).await;
    set_completed(&writer, "new-complete");

    let progress = transfers
        .get_entry_receive_progress("entry")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(progress.attempt_id, "a2");
    assert_eq!(progress.state, AttemptState::Receiving);
    assert_eq!(progress.total_bytes, 50);
    assert_eq!(progress.completed_bytes, 30);
    assert_eq!(progress.items_total, 2);
    assert_eq!(progress.items_completed, 1);
}

#[tokio::test]
async fn bulk_cancel_only_updates_non_terminal_members_of_the_exact_attempt() {
    let (_attempts, transfers, writer, _directory) = repos();
    seed_transfer(&transfers, "current-pending", "entry", Some("a2"), Some(10)).await;
    seed_transfer(
        &transfers,
        "current-complete",
        "entry",
        Some("a2"),
        Some(20),
    )
    .await;
    seed_transfer(&transfers, "old-pending", "entry", Some("a1"), Some(30)).await;
    seed_transfer(&transfers, "legacy", "entry", None, Some(40)).await;
    set_completed(&writer, "current-complete");

    let changed = transfers
        .cancel_attempt_transfers("entry", "a2", FileTransferCancellationReason::LocalUser, 9)
        .await
        .unwrap();
    assert_eq!(changed, 1);

    let rows = writer
        .run(|conn| {
            Ok(file_transfer::table
                .select((
                    file_transfer::transfer_id,
                    file_transfer::status,
                    file_transfer::failure_code,
                ))
                .order(file_transfer::transfer_id.asc())
                .load::<(String, String, Option<String>)>(conn)?)
        })
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("current-complete".into(), "completed".into(), None),
            (
                "current-pending".into(),
                "cancelled".into(),
                Some("local_user".into())
            ),
            ("legacy".into(), "pending".into(), None),
            ("old-pending".into(), "pending".into(), None),
        ]
    );
}

#[tokio::test]
async fn legacy_transfer_seed_remains_outside_directory_attempt_projection() {
    let (_attempts, transfers, writer, _directory) = repos();
    seed_transfer(&transfers, "legacy", "legacy-entry", None, None).await;

    let stored = writer
        .run(|conn| {
            Ok(file_transfer::table
                .filter(file_transfer::transfer_id.eq("legacy"))
                .select((file_transfer::attempt_id, file_transfer::file_size))
                .first::<(Option<String>, Option<i64>)>(conn)?)
        })
        .unwrap();
    assert_eq!(stored, (None, None));
    assert!(transfers
        .get_entry_receive_progress("legacy-entry")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn settled_receive_state_cleanup_is_atomic_and_orphans_respect_pending_artifacts() {
    let (attempts, transfers, writer, _directory) = repos();
    attempts
        .begin_first_receive("delete-me", "a1", 1)
        .await
        .unwrap();
    seed_transfer(
        &transfers,
        "delete-transfer",
        "delete-me",
        Some("a1"),
        Some(1),
    )
    .await;
    assert!(attempts
        .delete_receive_state_for_entry("delete-me")
        .await
        .unwrap());
    assert!(attempts
        .get_entry_attempt("delete-me")
        .await
        .unwrap()
        .is_none());
    assert!(
        writer
            .run(|conn| {
                Ok(file_transfer::table
                    .filter(file_transfer::entry_id.eq("delete-me"))
                    .count()
                    .get_result::<i64>(conn)?)
            })
            .unwrap()
            == 0
    );

    for entry_id in ["old-orphan", "pending-artifact", "still-receiving"] {
        attempts
            .begin_first_receive(entry_id, &format!("attempt-{entry_id}"), 1)
            .await
            .unwrap();
    }
    writer
        .run(|conn| {
            diesel::update(entry_receive_attempt::table.filter(
                entry_receive_attempt::entry_id.eq_any(["old-orphan", "pending-artifact"]),
            ))
            .set(entry_receive_attempt::attempt_state.eq(AttemptState::Cancelled.to_string()))
            .execute(conn)?;
            diesel::insert_into(receive_artifact_log::table)
                .values((
                    receive_artifact_log::entry_id.eq("pending-artifact"),
                    receive_artifact_log::attempt_id.eq("attempt-pending-artifact"),
                    receive_artifact_log::phase.eq("preparing"),
                    receive_artifact_log::resolution.eq("pending"),
                    receive_artifact_log::artifact_ciphertext.eq(vec![1_u8]),
                    receive_artifact_log::updated_at_ms.eq(1_i64),
                ))
                .execute(conn)?;
            Ok(())
        })
        .unwrap();

    assert_eq!(
        attempts.purge_terminal_orphan_attempts(10).await.unwrap(),
        1
    );
    assert!(attempts
        .get_entry_attempt("old-orphan")
        .await
        .unwrap()
        .is_none());
    assert!(attempts
        .get_entry_attempt("pending-artifact")
        .await
        .unwrap()
        .is_some());
    assert!(attempts
        .get_entry_attempt("still-receiving")
        .await
        .unwrap()
        .is_some());
}
