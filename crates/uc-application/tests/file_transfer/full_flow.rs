use uc_core::FileTransferEvent;

use crate::{
    build_context, complete_input, progress_input, published_events, start_input, transfer_history,
};

#[tokio::test]
async fn full_flow_persists_and_publishes_started_progress_and_completed_events() {
    let ctx = build_context();

    let started = ctx
        .start_transfer
        .execute(start_input("transfer-1", "peer-1"))
        .await
        .unwrap();
    let progressed = ctx
        .report_progress
        .execute(progress_input("transfer-1", "peer-1", 64))
        .await
        .unwrap();
    let completed = ctx
        .complete_transfer
        .execute(complete_input("transfer-1", "peer-1"))
        .await
        .unwrap();

    assert_eq!(
        progressed,
        FileTransferEvent::Progress {
            transfer_id: "transfer-1".into(),
            peer_id: "peer-1".into(),
            progress: crate::sending_progress(64, 128),
        }
    );
    assert_eq!(
        transfer_history(&ctx, "transfer-1").await,
        vec![started.clone(), progressed.clone(), completed.clone()]
    );
    assert_eq!(published_events(&ctx), vec![started, progressed, completed]);
}
