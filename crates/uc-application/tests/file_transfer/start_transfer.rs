use uc_core::FileTransferEvent;

use crate::{build_context, progress_input, published_events, start_input, transfer_history};

#[tokio::test]
async fn start_transfer_persists_and_publishes_started_event() {
    let ctx = build_context();

    let event = ctx
        .start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();

    assert_eq!(
        event,
        FileTransferEvent::started("transfer-1", "peer-1", "report.pdf", Some(128))
    );
    assert_eq!(
        transfer_history(&ctx, "transfer-1").await,
        vec![event.clone()]
    );
    assert_eq!(published_events(&ctx), vec![event]);
}

#[tokio::test]
async fn start_transfer_rejects_duplicate_start_after_existing_history() {
    let ctx = build_context();

    ctx.start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();

    let error = ctx
        .start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        uc_application::file_transfer::FileTransferApplicationError::TransferAlreadyStarted {
            transfer_id: "transfer-1".into(),
        }
    );
    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 1);
    assert_eq!(published_events(&ctx).len(), 1);
}

#[tokio::test]
async fn start_transfer_keeps_other_transfer_history_isolated() {
    let ctx = build_context();

    ctx.start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();
    ctx.start_transfer
        .execute(start_input("transfer-2", "peer-2"))
        .await
        .unwrap();
    ctx.report_progress
        .execute(progress_input("transfer-2", "peer-2", 64))
        .await
        .unwrap();

    assert_eq!(transfer_history(&ctx, "transfer-1").await.len(), 1);
    assert_eq!(transfer_history(&ctx, "transfer-2").await.len(), 2);
    assert_eq!(published_events(&ctx).len(), 3);
}

#[tokio::test]
async fn start_transfer_allows_unknown_file_size() {
    let ctx = build_context();

    let event = ctx
        .start_transfer
        .execute(uc_application::file_transfer::StartTransfer {
            transfer_id: "transfer-unknown".into(),
            peer_id: "peer-1".into(),
            filename: "report.pdf".into(),
            file_size: None,
        })
        .await
        .unwrap();

    assert_eq!(
        event,
        FileTransferEvent::started("transfer-unknown", "peer-1", "report.pdf", None)
    );
    assert_eq!(
        transfer_history(&ctx, "transfer-unknown").await,
        vec![event.clone()]
    );
    assert_eq!(published_events(&ctx), vec![event]);
}
