use std::sync::Arc;

use uc_core::file_transfer::FileTransferCancellationReason;
use uc_core::ids::EntryId;
use uc_core::ports::{
    CancelDirectoryAttemptTransfersPort, CleanupDirectoryStagingPort, ClockPort,
    CommitInboundReceivePort, GetDirectoryPublishRecordPort, GetEntryAttemptPort,
    GetEntryReceiveProgressPort, InboundReceiveSettlement, NoEntryReceiveArtifacts,
    PartialReceiveTerminal, RequestReceiveCancellationOutcome, RequestReceiveCancellationPort,
};

use tracing::instrument;

use crate::facade::blob_transfer::BlobTransferFacade;
use crate::facade::{ClipboardHostEvent, HostEvent, HostEventBus};

#[async_trait::async_trait]
pub(super) trait AttemptTransferCancellation: Send + Sync {
    async fn cancel_attempt(&self, attempt_id: &str) -> Result<usize, String>;
}

#[async_trait::async_trait]
impl AttemptTransferCancellation for BlobTransferFacade {
    async fn cancel_attempt(&self, attempt_id: &str) -> Result<usize, String> {
        self.cancel_inbound_attempt(attempt_id, FileTransferCancellationReason::LocalUser)
            .await
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelEntryReceiveOutcome {
    CancellationRequested,
    Cancelled,
    NotReceiving,
    TooLate,
    AlreadyTerminal,
    Superseded,
}

#[derive(Debug, thiserror::Error)]
pub enum CancelEntryReceiveError {
    #[error("entry receive cancellation is unavailable")]
    Unavailable,
    #[error("receive attempt state is unavailable: {0}")]
    Attempt(String),
    #[error("receive publication metadata is unavailable: {0}")]
    PublishLog(String),
    #[error("one or more member transfers could not be cancelled: {0}")]
    Transfer(String),
    #[error("receive staging cleanup failed: {0}")]
    Cleanup(String),
}

pub(super) struct CancelEntryReceiveUseCase {
    get_attempt: Arc<dyn GetEntryAttemptPort>,
    request_cancel: Arc<dyn RequestReceiveCancellationPort>,
    progress: Arc<dyn GetEntryReceiveProgressPort>,
    commit_inbound: Arc<dyn CommitInboundReceivePort>,
    get_publish: Arc<dyn GetDirectoryPublishRecordPort>,
    staging_cleanup: Arc<dyn CleanupDirectoryStagingPort>,
    cancel_projection: Arc<dyn CancelDirectoryAttemptTransfersPort>,
    transfer_cancellation: Arc<dyn AttemptTransferCancellation>,
    clock: Arc<dyn ClockPort>,
    host_event_bus: Arc<HostEventBus>,
}

impl CancelEntryReceiveUseCase {
    pub(super) fn new(
        get_attempt: Arc<dyn GetEntryAttemptPort>,
        request_cancel: Arc<dyn RequestReceiveCancellationPort>,
        progress: Arc<dyn GetEntryReceiveProgressPort>,
        commit_inbound: Arc<dyn CommitInboundReceivePort>,
        get_publish: Arc<dyn GetDirectoryPublishRecordPort>,
        staging_cleanup: Arc<dyn CleanupDirectoryStagingPort>,
        cancel_projection: Arc<dyn CancelDirectoryAttemptTransfersPort>,
        transfer_cancellation: Arc<dyn AttemptTransferCancellation>,
        clock: Arc<dyn ClockPort>,
        host_event_bus: Arc<HostEventBus>,
    ) -> Self {
        Self {
            get_attempt,
            request_cancel,
            progress,
            commit_inbound,
            get_publish,
            staging_cleanup,
            cancel_projection,
            transfer_cancellation,
            clock,
            host_event_bus,
        }
    }

    #[instrument(
        name = "usecase.cancel_entry_receive.execute",
        skip_all,
        fields(entry_id = %entry_id, attempt_id = %expected_attempt_id)
    )]
    pub(super) async fn execute(
        &self,
        entry_id: &EntryId,
        expected_attempt_id: &str,
    ) -> Result<CancelEntryReceiveOutcome, CancelEntryReceiveError> {
        let Some(attempt) = self
            .get_attempt
            .get_entry_attempt(entry_id.as_ref())
            .await
            .map_err(|error| CancelEntryReceiveError::Attempt(error.to_string()))?
        else {
            return Ok(CancelEntryReceiveOutcome::NotReceiving);
        };

        if attempt.current_attempt_id != expected_attempt_id {
            return Ok(CancelEntryReceiveOutcome::Superseded);
        }

        match self
            .request_cancel
            .request_receive_cancellation(
                entry_id.as_ref(),
                expected_attempt_id,
                self.clock.now_ms(),
            )
            .await
            .map_err(|error| CancelEntryReceiveError::Attempt(error.to_string()))?
        {
            RequestReceiveCancellationOutcome::Requested
            | RequestReceiveCancellationOutcome::AlreadyCancelling => {}
            RequestReceiveCancellationOutcome::TooLate => {
                return Ok(CancelEntryReceiveOutcome::TooLate)
            }
            RequestReceiveCancellationOutcome::Terminal => {
                return Ok(CancelEntryReceiveOutcome::AlreadyTerminal)
            }
            RequestReceiveCancellationOutcome::Superseded => {
                return Ok(CancelEntryReceiveOutcome::Superseded)
            }
        }

        self.host_event_bus.emit_or_warn(HostEvent::Clipboard(
            ClipboardHostEvent::ReceiveAttemptStateChanged {
                entry_id: entry_id.as_ref().to_owned(),
                attempt_id: expected_attempt_id.to_owned(),
                state: "cancelling".to_owned(),
            },
        ));

        let mut transfer_errors = Vec::new();
        if let Some(error) = self
            .transfer_cancellation
            .cancel_attempt(expected_attempt_id)
            .await
            .err()
        {
            transfer_errors.push(error);
        }
        if let Err(error) = self
            .cancel_projection
            .cancel_attempt_transfers(
                entry_id.as_ref(),
                expected_attempt_id,
                FileTransferCancellationReason::LocalUser,
                self.clock.now_ms(),
            )
            .await
        {
            transfer_errors.push(error.to_string());
        }

        let record = self
            .get_publish
            .get_publish_record(entry_id.as_ref(), expected_attempt_id)
            .await
            .map_err(|error| CancelEntryReceiveError::PublishLog(error.to_string()))?;
        let directory_receive = record.is_some();
        if let Some(record) = record {
            let staged_roots = record
                .root_map
                .into_iter()
                .map(|(staged, _)| staged)
                .collect::<Vec<_>>();
            self.staging_cleanup
                .cleanup_staging_roots(&staged_roots)
                .await
                .map_err(|error| CancelEntryReceiveError::Cleanup(error.to_string()))?;
        }

        if !transfer_errors.is_empty() {
            return Err(CancelEntryReceiveError::Transfer(
                transfer_errors.join("; "),
            ));
        }
        let zero_item_receive = self
            .progress
            .get_entry_receive_progress(entry_id.as_ref())
            .await
            .map_err(|error| CancelEntryReceiveError::Attempt(error.to_string()))?
            .is_none_or(|progress| progress.items_total == 0);
        if directory_receive || zero_item_receive {
            self.commit_inbound
                .commit_inbound_receive(&InboundReceiveSettlement::NoEntry {
                    entry_id: entry_id.clone(),
                    attempt_id: expected_attempt_id.to_owned(),
                    terminal: PartialReceiveTerminal::Cancelled,
                    artifacts: NoEntryReceiveArtifacts::None,
                    now_ms: self.clock.now_ms(),
                })
                .await
                .map_err(|error| CancelEntryReceiveError::Attempt(error.to_string()))?;
            self.host_event_bus.emit_or_warn(HostEvent::Clipboard(
                ClipboardHostEvent::ReceiveAttemptStateChanged {
                    entry_id: entry_id.as_ref().to_owned(),
                    attempt_id: expected_attempt_id.to_owned(),
                    state: "cancelled".to_owned(),
                },
            ));
            Ok(CancelEntryReceiveOutcome::Cancelled)
        } else {
            Ok(CancelEntryReceiveOutcome::CancellationRequested)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use uc_core::ports::{
        AttemptError, AttemptState, DirectoryPublishRecord, DirectoryStagingCleanupError,
        EntryReceiveAttempt, EntryReceiveProgress, FileTransferProjectionError,
        InboundReceiveCommitError, PublishLogError, PublishPhase,
    };

    use super::*;

    struct AttemptMock {
        state: AttemptState,
        cancel_outcome: RequestReceiveCancellationOutcome,
        cancel_calls: AtomicUsize,
    }

    #[async_trait]
    impl GetEntryAttemptPort for AttemptMock {
        async fn get_entry_attempt(
            &self,
            entry_id: &str,
        ) -> Result<Option<EntryReceiveAttempt>, AttemptError> {
            Ok(Some(EntryReceiveAttempt {
                entry_id: entry_id.to_owned(),
                current_attempt_id: "attempt-1".into(),
                state: self.state,
                updated_at_ms: 1,
            }))
        }
    }

    #[async_trait]
    impl RequestReceiveCancellationPort for AttemptMock {
        async fn request_receive_cancellation(
            &self,
            _entry_id: &str,
            _attempt_id: &str,
            _now_ms: i64,
        ) -> Result<RequestReceiveCancellationOutcome, AttemptError> {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.cancel_outcome)
        }
    }

    struct ProgressMock;

    #[async_trait]
    impl GetEntryReceiveProgressPort for ProgressMock {
        async fn get_entry_receive_progress(
            &self,
            _entry_id: &str,
        ) -> Result<Option<EntryReceiveProgress>, FileTransferProjectionError> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct CommitMock {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl CommitInboundReceivePort for CommitMock {
        async fn commit_inbound_receive(
            &self,
            settlement: &InboundReceiveSettlement,
        ) -> Result<(), InboundReceiveCommitError> {
            assert!(matches!(
                settlement,
                InboundReceiveSettlement::NoEntry { .. }
            ));
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct PublishMock;

    #[async_trait]
    impl GetDirectoryPublishRecordPort for PublishMock {
        async fn get_publish_record(
            &self,
            entry_id: &str,
            attempt_id: &str,
        ) -> Result<Option<DirectoryPublishRecord>, PublishLogError> {
            Ok(Some(DirectoryPublishRecord {
                entry_id: entry_id.to_owned(),
                attempt_id: attempt_id.to_owned(),
                phase: PublishPhase::Staging,
                root_map: vec![(
                    PathBuf::from("/tmp/.uniclip-incoming-entry/root"),
                    PathBuf::from("/tmp/final/root"),
                )],
                partial_visible_roots: None,
                landed: false,
            }))
        }
    }

    #[derive(Default)]
    struct CleanerMock {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl CleanupDirectoryStagingPort for CleanerMock {
        async fn cleanup_staging_roots(
            &self,
            staged_roots: &[PathBuf],
        ) -> Result<(), DirectoryStagingCleanupError> {
            assert_eq!(
                staged_roots,
                &[PathBuf::from("/tmp/.uniclip-incoming-entry/root")]
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TransferMock {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl AttemptTransferCancellation for TransferMock {
        async fn cancel_attempt(&self, attempt_id: &str) -> Result<usize, String> {
            assert_eq!(attempt_id, "attempt-1");
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(2)
        }
    }

    #[derive(Default)]
    struct ProjectionMock {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl CancelDirectoryAttemptTransfersPort for ProjectionMock {
        async fn cancel_attempt_transfers(
            &self,
            _entry_id: &str,
            attempt_id: &str,
            reason: FileTransferCancellationReason,
            _now_ms: i64,
        ) -> Result<u32, uc_core::ports::FileTransferProjectionError> {
            assert_eq!(attempt_id, "attempt-1");
            assert_eq!(reason, FileTransferCancellationReason::LocalUser);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(3)
        }
    }

    struct ClockMock;

    impl ClockPort for ClockMock {
        fn now_ms(&self) -> i64 {
            42
        }
    }

    fn use_case(
        attempts: Arc<AttemptMock>,
        cleaner: Arc<CleanerMock>,
        transfers: Arc<TransferMock>,
        projection: Arc<ProjectionMock>,
    ) -> CancelEntryReceiveUseCase {
        CancelEntryReceiveUseCase::new(
            attempts.clone(),
            attempts,
            Arc::new(ProgressMock),
            Arc::new(CommitMock::default()),
            Arc::new(PublishMock),
            cleaner,
            projection,
            transfers,
            Arc::new(ClockMock),
            Arc::new(HostEventBus::new()),
        )
    }

    #[tokio::test]
    async fn active_attempt_is_cancelled_before_transfers_and_staging_cleanup() {
        let attempts = Arc::new(AttemptMock {
            state: AttemptState::Receiving,
            cancel_outcome: RequestReceiveCancellationOutcome::Requested,
            cancel_calls: AtomicUsize::new(0),
        });
        let cleaner = Arc::new(CleanerMock::default());
        let transfers = Arc::new(TransferMock::default());
        let projection = Arc::new(ProjectionMock::default());
        let result = use_case(
            attempts.clone(),
            cleaner.clone(),
            transfers.clone(),
            projection.clone(),
        )
        .execute(&EntryId::new(), "attempt-1")
        .await;

        assert!(matches!(result, Ok(CancelEntryReceiveOutcome::Cancelled)));
        assert_eq!(attempts.cancel_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transfers.calls.load(Ordering::SeqCst), 1);
        assert_eq!(projection.calls.load(Ordering::SeqCst), 1);
        assert_eq!(cleaner.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_attempt_retries_transfer_and_staging_compensation() {
        let attempts = Arc::new(AttemptMock {
            state: AttemptState::Cancelling,
            cancel_outcome: RequestReceiveCancellationOutcome::AlreadyCancelling,
            cancel_calls: AtomicUsize::new(0),
        });
        let cleaner = Arc::new(CleanerMock::default());
        let transfers = Arc::new(TransferMock::default());
        let projection = Arc::new(ProjectionMock::default());
        let result = use_case(
            attempts.clone(),
            cleaner.clone(),
            transfers.clone(),
            projection.clone(),
        )
        .execute(&EntryId::new(), "attempt-1")
        .await;

        assert!(matches!(result, Ok(CancelEntryReceiveOutcome::Cancelled)));
        assert_eq!(attempts.cancel_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transfers.calls.load(Ordering::SeqCst), 1);
        assert_eq!(projection.calls.load(Ordering::SeqCst), 1);
        assert_eq!(cleaner.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn promotion_winner_blocks_cancellation_side_effects() {
        let attempts = Arc::new(AttemptMock {
            state: AttemptState::Receiving,
            cancel_outcome: RequestReceiveCancellationOutcome::TooLate,
            cancel_calls: AtomicUsize::new(0),
        });
        let cleaner = Arc::new(CleanerMock::default());
        let transfers = Arc::new(TransferMock::default());
        let projection = Arc::new(ProjectionMock::default());
        let result = use_case(
            attempts.clone(),
            cleaner.clone(),
            transfers.clone(),
            projection.clone(),
        )
        .execute(&EntryId::new(), "attempt-1")
        .await;

        assert!(matches!(result, Ok(CancelEntryReceiveOutcome::TooLate)));
        assert_eq!(attempts.cancel_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transfers.calls.load(Ordering::SeqCst), 0);
        assert_eq!(projection.calls.load(Ordering::SeqCst), 0);
        assert_eq!(cleaner.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stale_cancellation_does_not_touch_newer_attempt() {
        let attempts = Arc::new(AttemptMock {
            state: AttemptState::Receiving,
            cancel_outcome: RequestReceiveCancellationOutcome::Requested,
            cancel_calls: AtomicUsize::new(0),
        });
        let cleaner = Arc::new(CleanerMock::default());
        let transfers = Arc::new(TransferMock::default());
        let projection = Arc::new(ProjectionMock::default());
        let result = use_case(
            attempts,
            cleaner.clone(),
            transfers.clone(),
            projection.clone(),
        )
        .execute(&EntryId::new(), "stale-attempt")
        .await;

        assert!(matches!(result, Ok(CancelEntryReceiveOutcome::Superseded)));
        assert_eq!(transfers.calls.load(Ordering::SeqCst), 0);
        assert_eq!(projection.calls.load(Ordering::SeqCst), 0);
        assert_eq!(cleaner.calls.load(Ordering::SeqCst), 0);
    }
}
