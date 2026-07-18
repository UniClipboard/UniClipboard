use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::sync::{Mutex, Notify};
use tracing::{error, instrument};
use uc_core::ids::EntryId;
use uc_core::ports::{
    AttemptState, BeginReceiveFailureOutcome, BeginReceiveFailurePort, CleanupReceiveArtifactsPort,
    ClockPort, CommitInboundReceivePort, FinalizeProvisionalReceivePort,
    GetDirectoryPublishRecordPort, GetEntryAttemptPort, InboundReceiveSettlement,
    ListNonTerminalAttemptsPort, ListProvisionalReceivesPort, ListUnsettledReceiveArtifactsPort,
    NoEntryReceiveArtifacts, PartialReceiveTerminal, ProvisionalReceiveAction, ReceiveArtifact,
    ReceiveArtifactOwnership,
};

pub struct ReceiveReadinessCoordinator {
    ready: AtomicBool,
    notify: Notify,
    recovery_lock: Mutex<()>,
    degraded_reason: RwLock<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveReadinessStatus {
    pub ready: bool,
    pub degraded_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiveReadinessError {
    #[error("receive recovery failed: {0}")]
    Recovery(String),
}

#[async_trait]
pub trait EnsureReceiveReadyPort: Send + Sync {
    async fn ensure_receive_ready(&self) -> Result<(), ReceiveReadinessError>;
    fn close_receive_gate(&self);
    fn receive_readiness_status(&self) -> ReceiveReadinessStatus;
}

impl ReceiveReadinessCoordinator {
    pub fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            notify: Notify::new(),
            recovery_lock: Mutex::new(()),
            degraded_reason: RwLock::new(None),
        }
    }

    pub fn mark_ready(&self) {
        *self
            .degraded_reason
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.ready.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn mark_not_ready(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub async fn wait_ready(&self) {
        while !self.is_ready() {
            let notified = self.notify.notified();
            if self.is_ready() {
                return;
            }
            notified.await;
        }
    }

    pub fn degraded_reason(&self) -> Option<String> {
        self.degraded_reason
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn status(&self) -> ReceiveReadinessStatus {
        ReceiveReadinessStatus {
            ready: self.is_ready(),
            degraded_reason: self.degraded_reason(),
        }
    }

    pub async fn ensure_ready<F, Fut>(&self, recover: F) -> Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        if self.is_ready() {
            return Ok(());
        }

        let _guard = self.recovery_lock.lock().await;
        if self.is_ready() {
            return Ok(());
        }

        match recover().await {
            Ok(()) => {
                self.mark_ready();
                Ok(())
            }
            Err(error) => {
                self.mark_not_ready();
                *self
                    .degraded_reason
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.to_string());
                Err(error)
            }
        }
    }
}

impl Default for ReceiveReadinessCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReconcileReceiveAttemptsUseCase {
    get_attempt: Arc<dyn GetEntryAttemptPort>,
    list_attempts: Arc<dyn ListNonTerminalAttemptsPort>,
    list_artifacts: Arc<dyn ListUnsettledReceiveArtifactsPort>,
    get_directory_publish: Arc<dyn GetDirectoryPublishRecordPort>,
    begin_failure: Arc<dyn BeginReceiveFailurePort>,
    cleanup_artifacts: Arc<dyn CleanupReceiveArtifactsPort>,
    list_provisional: Arc<dyn ListProvisionalReceivesPort>,
    finalize_provisional: Arc<dyn FinalizeProvisionalReceivePort>,
    commit: Arc<dyn CommitInboundReceivePort>,
    clock: Arc<dyn ClockPort>,
}

impl ReconcileReceiveAttemptsUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        get_attempt: Arc<dyn GetEntryAttemptPort>,
        list_attempts: Arc<dyn ListNonTerminalAttemptsPort>,
        list_artifacts: Arc<dyn ListUnsettledReceiveArtifactsPort>,
        get_directory_publish: Arc<dyn GetDirectoryPublishRecordPort>,
        begin_failure: Arc<dyn BeginReceiveFailurePort>,
        cleanup_artifacts: Arc<dyn CleanupReceiveArtifactsPort>,
        list_provisional: Arc<dyn ListProvisionalReceivesPort>,
        finalize_provisional: Arc<dyn FinalizeProvisionalReceivePort>,
        commit: Arc<dyn CommitInboundReceivePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            get_attempt,
            list_attempts,
            list_artifacts,
            get_directory_publish,
            begin_failure,
            cleanup_artifacts,
            list_provisional,
            finalize_provisional,
            commit,
            clock,
        }
    }

    #[instrument(name = "usecase.reconcile_receive_attempts.execute", skip_all)]
    pub async fn execute(&self) -> Result<u32> {
        let mut reconciled = 0u32;
        for provisional in self
            .list_provisional
            .list_provisional_receives()
            .await
            .map_err(|error| anyhow!(error.to_string()))?
        {
            if let Some(uri) = provisional.cached_path {
                let path = url::Url::parse(&uri)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                    .ok_or_else(|| anyhow!("provisional receive path is not a valid file URI"))?;
                self.cleanup_artifacts
                    .cleanup_receive_artifacts(&[ReceiveArtifact {
                        item_id: provisional.transfer_id.clone(),
                        staged_path: path.clone(),
                        final_path: path,
                        ownership: ReceiveArtifactOwnership::ManagedStaging,
                    }])
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
            }
            self.finalize_provisional
                .finalize_provisional_receive(
                    &provisional.transfer_id,
                    ProvisionalReceiveAction::DiscardAsFullyHeld,
                    self.clock.now_ms(),
                )
                .await
                .map_err(|error| anyhow!(error.to_string()))?;
            reconciled = reconciled.saturating_add(1);
        }

        let mut artifacts = self
            .list_artifacts
            .list_unsettled_receive_artifacts()
            .await
            .map_err(|error| anyhow!(error.to_string()))?
            .into_iter()
            .map(|record| ((record.entry_id.clone(), record.attempt_id.clone()), record))
            .collect::<HashMap<_, _>>();

        for ((entry_id, attempt_id), _) in &artifacts {
            let current = self
                .get_attempt
                .get_entry_attempt(entry_id)
                .await
                .map_err(|error| anyhow!(error.to_string()))?
                .ok_or_else(|| anyhow!("unsettled receive artifacts have no attempt authority"))?;
            if current.current_attempt_id != *attempt_id || current.state.is_terminal() {
                error!(
                    entry_id = %entry_id,
                    attempt_id = %attempt_id,
                    "reconcile: unsettled artifact metadata belongs to a terminal or superseded receive"
                );
                return Err(anyhow!(
                    "terminal or superseded receive has unsettled artifact metadata"
                ));
            }
        }

        let attempts = self
            .list_attempts
            .list_non_terminal_attempts()
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        for attempt in attempts {
            let terminal = if attempt.state == AttemptState::Cancelling {
                PartialReceiveTerminal::Cancelled
            } else {
                if matches!(
                    attempt.state,
                    AttemptState::Receiving | AttemptState::Committing
                ) {
                    let outcome = self
                        .begin_failure
                        .begin_receive_failure(
                            &attempt.entry_id,
                            &attempt.current_attempt_id,
                            self.clock.now_ms(),
                        )
                        .await
                        .map_err(|error| anyhow!(error.to_string()))?;
                    if outcome != BeginReceiveFailureOutcome::Begun {
                        error!(
                            entry_id = %attempt.entry_id,
                            attempt_id = %attempt.current_attempt_id,
                            "reconcile: interrupted receive lost its failure claim during recovery"
                        );
                        return Err(anyhow!("interrupted receive failure claim was lost"));
                    }
                }
                PartialReceiveTerminal::Failed
            };

            let artifact_record =
                artifacts.remove(&(attempt.entry_id.clone(), attempt.current_attempt_id.clone()));
            let mut cleanup = artifact_record
                .as_ref()
                .map(|record| record.artifacts.clone())
                .unwrap_or_default();
            if let Some(directory) = self
                .get_directory_publish
                .get_publish_record(&attempt.entry_id, &attempt.current_attempt_id)
                .await
                .map_err(|error| anyhow!(error.to_string()))?
            {
                cleanup.extend(directory.root_map.into_iter().enumerate().map(
                    |(index, (staged_path, final_path))| ReceiveArtifact {
                        item_id: format!("directory-root-{index}"),
                        staged_path,
                        final_path,
                        ownership: ReceiveArtifactOwnership::UserDestination,
                    },
                ));
            }
            self.cleanup_artifacts
                .cleanup_receive_artifacts(&cleanup)
                .await
                .map_err(|error| anyhow!(error.to_string()))?;

            self.commit
                .commit_inbound_receive(&InboundReceiveSettlement::NoEntry {
                    entry_id: EntryId::from(attempt.entry_id.as_str()),
                    attempt_id: attempt.current_attempt_id,
                    terminal,
                    artifacts: if artifact_record.is_some() {
                        NoEntryReceiveArtifacts::RolledBack
                    } else {
                        NoEntryReceiveArtifacts::None
                    },
                    now_ms: self.clock.now_ms(),
                })
                .await
                .map_err(|error| anyhow!(error.to_string()))?;
            reconciled = reconciled.saturating_add(1);
        }

        if !artifacts.is_empty() {
            error!(
                orphaned = artifacts.len(),
                "reconcile: unsettled receive artifacts had no matching non-terminal attempt"
            );
            return Err(anyhow!("unsettled receive artifacts were not reconciled"));
        }
        Ok(reconciled)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use uc_core::ports::{
        AttemptError, DirectoryPublishRecord, EntryReceiveAttempt, InboundReceiveCommitError,
        ProvisionalReceiveError, ProvisionalReceiveRecovery, PublishLogError,
        ReceiveArtifactLogError, ReceiveArtifactPhase, ReceiveArtifactRecord,
        ReceiveArtifactResolution,
    };

    use super::*;

    #[tokio::test]
    async fn readiness_waiters_open_only_after_reconciliation_succeeds() {
        let readiness = Arc::new(ReceiveReadinessCoordinator::new());
        let waiter = {
            let readiness = Arc::clone(&readiness);
            tokio::spawn(async move { readiness.wait_ready().await })
        };

        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        readiness.mark_ready();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter must be released")
            .expect("waiter task must succeed");
    }

    #[tokio::test]
    async fn readiness_failure_stays_closed_and_successful_retry_opens_gate() {
        let readiness = Arc::new(ReceiveReadinessCoordinator::new());
        let waiter = {
            let readiness = Arc::clone(&readiness);
            tokio::spawn(async move { readiness.wait_ready().await })
        };

        let error = readiness
            .ensure_ready(|| async { Err(anyhow!("artifact metadata cannot be decrypted")) })
            .await
            .expect_err("failed recovery must remain observable");

        assert_eq!(error.to_string(), "artifact metadata cannot be decrypted");
        assert_eq!(
            readiness.degraded_reason().as_deref(),
            Some("artifact metadata cannot be decrypted")
        );
        assert!(!readiness.is_ready());
        assert!(!waiter.is_finished());

        readiness
            .ensure_ready(|| async { Ok(()) })
            .await
            .expect("recovery retry should succeed");

        assert!(readiness.is_ready());
        assert!(readiness.degraded_reason().is_none());
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter must be released after successful retry")
            .expect("waiter task must succeed");
    }

    struct Store {
        attempt: EntryReceiveAttempt,
        artifact: ReceiveArtifactRecord,
        include_artifact: bool,
        provisional: Mutex<Vec<ProvisionalReceiveRecovery>>,
        finalized_provisional: AtomicUsize,
        commit_calls: AtomicUsize,
    }

    #[async_trait]
    impl GetEntryAttemptPort for Store {
        async fn get_entry_attempt(
            &self,
            _entry_id: &str,
        ) -> Result<Option<EntryReceiveAttempt>, AttemptError> {
            Ok(Some(self.attempt.clone()))
        }
    }

    #[async_trait]
    impl ListNonTerminalAttemptsPort for Store {
        async fn list_non_terminal_attempts(
            &self,
        ) -> Result<Vec<EntryReceiveAttempt>, AttemptError> {
            Ok((!self.attempt.state.is_terminal())
                .then(|| self.attempt.clone())
                .into_iter()
                .collect())
        }
    }

    #[async_trait]
    impl ListUnsettledReceiveArtifactsPort for Store {
        async fn list_unsettled_receive_artifacts(
            &self,
        ) -> Result<Vec<ReceiveArtifactRecord>, ReceiveArtifactLogError> {
            Ok(self
                .include_artifact
                .then(|| self.artifact.clone())
                .into_iter()
                .collect())
        }
    }

    #[async_trait]
    impl ListProvisionalReceivesPort for Store {
        async fn list_provisional_receives(
            &self,
        ) -> Result<Vec<ProvisionalReceiveRecovery>, ProvisionalReceiveError> {
            Ok(self.provisional.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl FinalizeProvisionalReceivePort for Store {
        async fn finalize_provisional_receive(
            &self,
            _provisional_transfer_id: &str,
            _action: ProvisionalReceiveAction,
            _now_ms: i64,
        ) -> Result<(), ProvisionalReceiveError> {
            self.finalized_provisional.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait]
    impl GetDirectoryPublishRecordPort for Store {
        async fn get_publish_record(
            &self,
            _entry_id: &str,
            _attempt_id: &str,
        ) -> Result<Option<DirectoryPublishRecord>, PublishLogError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl BeginReceiveFailurePort for Store {
        async fn begin_receive_failure(
            &self,
            _entry_id: &str,
            _attempt_id: &str,
            _now_ms: i64,
        ) -> Result<BeginReceiveFailureOutcome, AttemptError> {
            Ok(BeginReceiveFailureOutcome::Begun)
        }
    }

    #[async_trait]
    impl CommitInboundReceivePort for Store {
        async fn commit_inbound_receive(
            &self,
            settlement: &InboundReceiveSettlement,
        ) -> Result<(), InboundReceiveCommitError> {
            assert!(matches!(
                settlement,
                InboundReceiveSettlement::NoEntry {
                    artifacts: NoEntryReceiveArtifacts::RolledBack,
                    ..
                }
            ));
            self.commit_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Clock;

    impl ClockPort for Clock {
        fn now_ms(&self) -> i64 {
            10
        }
    }

    fn store(state: AttemptState, path: PathBuf) -> Arc<Store> {
        Arc::new(Store {
            attempt: EntryReceiveAttempt {
                entry_id: "entry-1".to_owned(),
                current_attempt_id: "attempt-1".to_owned(),
                state,
                updated_at_ms: 1,
            },
            artifact: ReceiveArtifactRecord {
                entry_id: "entry-1".to_owned(),
                attempt_id: "attempt-1".to_owned(),
                phase: ReceiveArtifactPhase::Preparing,
                resolution: ReceiveArtifactResolution::Pending,
                artifacts: vec![ReceiveArtifact {
                    item_id: "item-1".to_owned(),
                    staged_path: path.clone(),
                    final_path: path,
                    ownership: ReceiveArtifactOwnership::ManagedStaging,
                }],
                updated_at_ms: 1,
            },
            include_artifact: true,
            provisional: Mutex::new(Vec::new()),
            finalized_provisional: AtomicUsize::new(0),
            commit_calls: AtomicUsize::new(0),
        })
    }

    fn use_case(store: Arc<Store>) -> ReconcileReceiveAttemptsUseCase {
        ReconcileReceiveAttemptsUseCase::new(
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            Arc::new(uc_infra::fs::FsReceiveArtifactCleaner),
            store.clone(),
            store.clone(),
            store,
            Arc::new(Clock),
        )
    }

    #[tokio::test]
    async fn interrupted_receive_is_cleaned_before_it_is_failed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("partial.bin");
        tokio::fs::write(&path, b"partial").await.unwrap();
        let store = store(AttemptState::Receiving, path.clone());

        assert_eq!(use_case(store.clone()).execute().await.unwrap(), 1);
        assert!(!path.exists());
        assert_eq!(store.commit_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn provisional_orphan_file_is_deleted_before_its_row_is_discarded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provisional.bin");
        tokio::fs::write(&path, b"orphan").await.unwrap();
        let staged_uri = url::Url::from_file_path(&path).unwrap().to_string();
        let placeholder = path.with_file_name("unused.bin");
        let store = Arc::new(Store {
            attempt: EntryReceiveAttempt {
                entry_id: "completed-entry".to_owned(),
                current_attempt_id: "completed-attempt".to_owned(),
                state: AttemptState::Completed,
                updated_at_ms: 1,
            },
            artifact: ReceiveArtifactRecord {
                entry_id: "completed-entry".to_owned(),
                attempt_id: "completed-attempt".to_owned(),
                phase: ReceiveArtifactPhase::Settled,
                resolution: ReceiveArtifactResolution::Landed,
                artifacts: vec![ReceiveArtifact {
                    item_id: "unused".to_owned(),
                    staged_path: placeholder.clone(),
                    final_path: placeholder,
                    ownership: ReceiveArtifactOwnership::ManagedStaging,
                }],
                updated_at_ms: 1,
            },
            include_artifact: false,
            provisional: Mutex::new(vec![ProvisionalReceiveRecovery {
                transfer_id: "mobile-orphan".to_owned(),
                cached_path: Some(staged_uri),
            }]),
            finalized_provisional: AtomicUsize::new(0),
            commit_calls: AtomicUsize::new(0),
        });

        assert_eq!(use_case(store.clone()).execute().await.unwrap(), 1);
        assert!(!path.exists());
        assert_eq!(store.finalized_provisional.load(Ordering::SeqCst), 1);
        assert_eq!(store.commit_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn terminal_attempt_with_pending_artifacts_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("must-remain.bin");
        tokio::fs::write(&path, b"keep").await.unwrap();
        let store = store(AttemptState::Completed, path.clone());

        assert!(use_case(store.clone()).execute().await.is_err());
        assert!(path.exists());
        assert_eq!(store.commit_calls.load(Ordering::SeqCst), 0);
    }
}
