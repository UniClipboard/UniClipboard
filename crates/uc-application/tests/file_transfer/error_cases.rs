use uc_application::file_transfer::FileTransferApplicationError;
use uc_core::file_transfer::FileTransferEventStorePort;
use uc_core::{FileTransferCancellationReason, FileTransferEvent, FileTransferFailureReason};

use crate::{
    build_context, cancel_input, fail_input, progress_input, published_events, start_input,
    transfer_history,
};

#[tokio::test]
async fn progress_before_start_is_rejected_without_side_effects() {
    let ctx = build_context();

    let error = ctx
        .report_progress
        .execute(progress_input("transfer-1", "peer-1", 64))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        FileTransferApplicationError::TransferNotStarted {
            transfer_id: "transfer-1".into(),
        }
    );
    assert!(transfer_history(&ctx, "transfer-1").await.is_empty());
    assert!(published_events(&ctx).is_empty());
}

#[tokio::test]
async fn progress_backwards_is_rejected_without_appending_new_event() {
    let ctx = build_context();

    ctx.start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();
    ctx.report_progress
        .execute(progress_input("transfer-1", "peer-1", 64))
        .await
        .unwrap();

    let error = ctx
        .report_progress
        .execute(progress_input("transfer-1", "peer-1", 32))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        FileTransferApplicationError::ProgressWentBackwards {
            transfer_id: "transfer-1".into(),
            previous_bytes: 64,
            new_bytes: 32,
        }
    );
    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 2);
    assert_eq!(published_events(&ctx).len(), 2);
}

#[tokio::test]
async fn peer_mismatch_is_rejected_without_side_effects() {
    let ctx = build_context();

    ctx.start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();

    let error = ctx
        .fail_transfer
        .execute(fail_input(
            "transfer-1",
            "peer-2",
            FileTransferFailureReason::TimedOut,
        ))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        FileTransferApplicationError::PeerMismatch {
            transfer_id: "transfer-1".into(),
            expected_peer_id: "peer-1".into(),
            actual_peer_id: "peer-2".into(),
        }
    );
    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 1);
    assert_eq!(published_events(&ctx).len(), 1);
}

#[tokio::test]
async fn terminal_transfer_rejects_follow_up_events_without_side_effects() {
    let ctx = build_context();

    ctx.start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();
    ctx.cancel_transfer
        .execute(cancel_input(
            "transfer-1",
            "peer-1",
            FileTransferCancellationReason::LocalUser,
        ))
        .await
        .unwrap();

    let error = ctx
        .fail_transfer
        .execute(fail_input(
            "transfer-1",
            "peer-1",
            FileTransferFailureReason::TimedOut,
        ))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        FileTransferApplicationError::TransferAlreadyFinished {
            transfer_id: "transfer-1".into(),
        }
    );
    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 2);
    assert_eq!(published_events(&ctx).len(), 2);
}

#[tokio::test]
async fn duplicate_started_event_in_history_is_rejected_as_invalid_history() {
    let ctx = build_context();

    ctx.store
        .append(FileTransferEvent::started(
            "transfer-1",
            "peer-1",
            "report.pdf",
            Some(128),
        ))
        .await
        .unwrap();
    ctx.store
        .append(FileTransferEvent::started(
            "transfer-1",
            "peer-1",
            "report.pdf",
            Some(128),
        ))
        .await
        .unwrap();

    let error = ctx
        .report_progress
        .execute(progress_input("transfer-1", "peer-1", 64))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        FileTransferApplicationError::InvalidHistory {
            transfer_id: "transfer-1".into(),
            message: "duplicate Started event".into(),
        }
    );
    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 2);
    assert!(published_events(&ctx).is_empty());
}
