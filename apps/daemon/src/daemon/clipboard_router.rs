//! Serial authority for process-wide clipboard sends and active-space changes.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClipboardRouterError {
    #[error("clipboard router is closed")]
    Closed,
    #[error("clipboard routing backend failed: {0}")]
    Backend(String),
    #[error("clipboard router operation timed out")]
    TimedOut,
    #[error("active-profile persistence is still uncertain")]
    PersistUncertain,
    #[error("clipboard router is poisoned: {0}")]
    Poisoned(String),
}

const BACKEND_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
const ROUTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

#[async_trait]
pub trait ClipboardRouterBackend<Snapshot>: Send + Sync {
    async fn load_active_profile(&self, cancel: CancellationToken) -> anyhow::Result<String>;

    async fn dispatch_snapshot(
        &self,
        profile_id: &str,
        snapshot: Snapshot,
        cancel: CancellationToken,
    ) -> anyhow::Result<()>;

    async fn persist_active_profile(
        &self,
        profile_id: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<()>;
}

pub struct ClipboardRouterHandle<Snapshot> {
    commands: mpsc::Sender<ClipboardRouterCommand<Snapshot>>,
    admission: Arc<Mutex<bool>>,
}

impl<Snapshot> Clone for ClipboardRouterHandle<Snapshot> {
    fn clone(&self) -> Self {
        Self {
            commands: self.commands.clone(),
            admission: Arc::clone(&self.admission),
        }
    }
}

enum ClipboardRouterCommand<Snapshot> {
    ClipboardChanged {
        snapshot: Snapshot,
        reply: oneshot::Sender<Result<(), ClipboardRouterError>>,
    },
    SetActive {
        profile_id: String,
        reply: oneshot::Sender<Result<(), ClipboardRouterError>>,
    },
    ActiveProfile {
        reply: oneshot::Sender<Result<String, ClipboardRouterError>>,
    },
    Barrier {
        reply: oneshot::Sender<Result<(), ClipboardRouterError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<(), ClipboardRouterError>>,
    },
}

pub struct ClipboardRouterTask<Snapshot> {
    commands: mpsc::Sender<ClipboardRouterCommand<Snapshot>>,
    admission: Arc<Mutex<bool>>,
    join: Option<tokio::task::JoinHandle<()>>,
    shutdown_timeout: Duration,
}

enum ActiveProfileState {
    Ready(String),
    PersistUncertain(PendingPersist),
    Poisoned(ClipboardRouterError),
}

struct PendingPersist {
    target: String,
    failure: ClipboardRouterError,
    completion: tokio::task::JoinHandle<Result<String, ClipboardRouterError>>,
}

impl ActiveProfileState {
    fn ready_profile(&self) -> Result<&str, ClipboardRouterError> {
        match self {
            Self::Ready(profile_id) => Ok(profile_id),
            Self::PersistUncertain(_) => Err(ClipboardRouterError::PersistUncertain),
            Self::Poisoned(error) => Err(error.clone()),
        }
    }
}

async fn reconcile_completed_persist<Snapshot>(
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
    target: String,
    persist_error: ClipboardRouterError,
    operation_timeout: Duration,
) -> (ActiveProfileState, Result<(), ClipboardRouterError>)
where
    Snapshot: Send + 'static,
{
    let load_backend = Arc::clone(&backend);
    match run_backend_operation(operation_timeout, move |cancel| async move {
        load_backend.load_active_profile(cancel).await
    })
    .await
    {
        Ok(durable_profile) => {
            let reached_target = durable_profile == target;
            let state = ActiveProfileState::Ready(durable_profile);
            if reached_target {
                (state, Ok(()))
            } else {
                (state, Err(persist_error))
            }
        }
        Err(reconcile_error) => {
            let error = ClipboardRouterError::Poisoned(format!(
                "{persist_error}; durable active-profile reconciliation failed: {reconcile_error}"
            ));
            (ActiveProfileState::Poisoned(error.clone()), Err(error))
        }
    }
}

async fn complete_timed_out_persist<Snapshot>(
    persist_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
    operation_timeout: Duration,
) -> Result<String, ClipboardRouterError>
where
    Snapshot: Send + 'static,
{
    let terminal = match persist_task.await {
        Ok(Ok(())) => "completed successfully".to_owned(),
        Ok(Err(error)) => format!("completed with backend error: {error}"),
        Err(error) => format!("task failed: {error}"),
    };
    let load_backend = Arc::clone(&backend);
    run_backend_operation(operation_timeout, move |cancel| async move {
        load_backend.load_active_profile(cancel).await
    })
    .await
    .map_err(|error| {
        ClipboardRouterError::Poisoned(format!(
            "timed-out persist {terminal}; durable active-profile reconciliation failed: {error}"
        ))
    })
}

fn finish_pending_result(
    target: String,
    failure: ClipboardRouterError,
    completion: Result<Result<String, ClipboardRouterError>, tokio::task::JoinError>,
) -> (ActiveProfileState, Result<(), ClipboardRouterError>) {
    match completion {
        Ok(Ok(durable_profile)) => {
            let reached_target = durable_profile == target;
            let state = ActiveProfileState::Ready(durable_profile);
            if reached_target {
                (state, Ok(()))
            } else {
                (state, Err(failure))
            }
        }
        Ok(Err(error)) => (ActiveProfileState::Poisoned(error.clone()), Err(error)),
        Err(error) => {
            let error = ClipboardRouterError::Poisoned(format!(
                "persist completion fence task failed: {error}"
            ));
            (ActiveProfileState::Poisoned(error.clone()), Err(error))
        }
    }
}

async fn wait_for_pending(
    mut pending: PendingPersist,
    timeout: Duration,
) -> (ActiveProfileState, Result<(), ClipboardRouterError>) {
    match tokio::time::timeout(timeout, &mut pending.completion).await {
        Ok(completion) => finish_pending_result(pending.target, pending.failure, completion),
        Err(_) => (
            ActiveProfileState::PersistUncertain(pending),
            Err(ClipboardRouterError::PersistUncertain),
        ),
    }
}

async fn settle_active_profile(
    state: ActiveProfileState,
    timeout: Duration,
) -> (ActiveProfileState, Result<(), ClipboardRouterError>) {
    match state {
        ActiveProfileState::Ready(profile_id) => (ActiveProfileState::Ready(profile_id), Ok(())),
        ActiveProfileState::PersistUncertain(pending) => wait_for_pending(pending, timeout).await,
        ActiveProfileState::Poisoned(error) => {
            (ActiveProfileState::Poisoned(error.clone()), Err(error))
        }
    }
}

async fn run_persist_operation<Snapshot>(
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
    target: String,
    operation_timeout: Duration,
) -> (ActiveProfileState, Result<(), ClipboardRouterError>)
where
    Snapshot: Send + 'static,
{
    let cancel = CancellationToken::new();
    let persist_cancel = cancel.clone();
    let persist_backend = Arc::clone(&backend);
    let persist_target = target.clone();
    let mut persist_task = tokio::spawn(async move {
        persist_backend
            .persist_active_profile(&persist_target, persist_cancel)
            .await
    });

    match tokio::time::timeout(operation_timeout, &mut persist_task).await {
        Ok(Ok(Ok(()))) => (ActiveProfileState::Ready(target), Ok(())),
        Ok(Ok(Err(error))) => {
            reconcile_completed_persist(
                backend,
                target,
                ClipboardRouterError::Backend(error.to_string()),
                operation_timeout,
            )
            .await
        }
        Ok(Err(error)) => {
            reconcile_completed_persist(
                backend,
                target,
                ClipboardRouterError::Backend(format!("backend task failed: {error}")),
                operation_timeout,
            )
            .await
        }
        Err(_) => {
            cancel.cancel();
            let completion_backend = Arc::clone(&backend);
            let completion = tokio::spawn(async move {
                complete_timed_out_persist(persist_task, completion_backend, operation_timeout)
                    .await
            });
            wait_for_pending(
                PendingPersist {
                    target,
                    failure: ClipboardRouterError::TimedOut,
                    completion,
                },
                operation_timeout,
            )
            .await
        }
    }
}

pub fn spawn_clipboard_router<Snapshot>(
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
) -> (
    ClipboardRouterHandle<Snapshot>,
    ClipboardRouterTask<Snapshot>,
)
where
    Snapshot: Send + 'static,
{
    spawn_clipboard_router_with_timeouts(
        backend,
        BACKEND_OPERATION_TIMEOUT,
        ROUTER_SHUTDOWN_TIMEOUT,
    )
}

pub(crate) fn spawn_clipboard_router_with_timeouts<Snapshot>(
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
    operation_timeout: Duration,
    shutdown_timeout: Duration,
) -> (
    ClipboardRouterHandle<Snapshot>,
    ClipboardRouterTask<Snapshot>,
)
where
    Snapshot: Send + 'static,
{
    let (commands, mut receiver) = mpsc::channel(64);
    let admission = Arc::new(Mutex::new(true));
    let join = tokio::spawn(async move {
        let load_backend = Arc::clone(&backend);
        let mut active_profile =
            match run_backend_operation(operation_timeout, move |cancel| async move {
                load_backend.load_active_profile(cancel).await
            })
            .await
            {
                Ok(profile_id) => ActiveProfileState::Ready(profile_id),
                Err(error) => ActiveProfileState::Poisoned(error),
            };
        while let Some(command) = receiver.recv().await {
            match command {
                ClipboardRouterCommand::ClipboardChanged { snapshot, reply } => {
                    let profile_id = match active_profile.ready_profile() {
                        Ok(profile_id) => profile_id.to_owned(),
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            continue;
                        }
                    };
                    let dispatch_backend = Arc::clone(&backend);
                    let result =
                        run_backend_operation(operation_timeout, move |cancel| async move {
                            dispatch_backend
                                .dispatch_snapshot(&profile_id, snapshot, cancel)
                                .await
                        })
                        .await;
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::SetActive { profile_id, reply } => {
                    let current = std::mem::replace(
                        &mut active_profile,
                        ActiveProfileState::Poisoned(ClipboardRouterError::Poisoned(
                            "active-profile transition interrupted".into(),
                        )),
                    );
                    let (settled, settle_result) =
                        settle_active_profile(current, operation_timeout).await;
                    active_profile = settled;
                    if let Err(error) = settle_result {
                        let _ = reply.send(Err(error));
                        continue;
                    }

                    if active_profile.ready_profile() == Ok(profile_id.as_str()) {
                        let _ = reply.send(Ok(()));
                        continue;
                    }

                    if let Err(error) = active_profile.ready_profile() {
                        let _ = reply.send(Err(error));
                        continue;
                    }
                    let (state, result) =
                        run_persist_operation(Arc::clone(&backend), profile_id, operation_timeout)
                            .await;
                    active_profile = state;
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::ActiveProfile { reply } => {
                    let current = std::mem::replace(
                        &mut active_profile,
                        ActiveProfileState::Poisoned(ClipboardRouterError::Poisoned(
                            "active-profile query interrupted".into(),
                        )),
                    );
                    let (settled, _) = settle_active_profile(current, operation_timeout).await;
                    active_profile = settled;
                    let result = active_profile.ready_profile().map(str::to_owned);
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::Barrier { reply } => {
                    let current = std::mem::replace(
                        &mut active_profile,
                        ActiveProfileState::Poisoned(ClipboardRouterError::Poisoned(
                            "active-profile barrier interrupted".into(),
                        )),
                    );
                    let (settled, result) = settle_active_profile(current, operation_timeout).await;
                    active_profile = settled;
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::Shutdown { reply } => {
                    let current = std::mem::replace(
                        &mut active_profile,
                        ActiveProfileState::Poisoned(ClipboardRouterError::Poisoned(
                            "active-profile shutdown interrupted".into(),
                        )),
                    );
                    let (_settled, result) =
                        settle_active_profile(current, operation_timeout).await;
                    receiver.close();
                    while let Some(command) = receiver.recv().await {
                        reject_command(command);
                    }
                    let _ = reply.send(result);
                    break;
                }
            }
        }
    });
    let task_commands = commands.clone();
    (
        ClipboardRouterHandle {
            commands,
            admission: Arc::clone(&admission),
        },
        ClipboardRouterTask {
            commands: task_commands,
            admission,
            join: Some(join),
            shutdown_timeout,
        },
    )
}

async fn run_backend_operation<T, Factory, F>(
    timeout: Duration,
    operation: Factory,
) -> Result<T, ClipboardRouterError>
where
    Factory: FnOnce(CancellationToken) -> F,
    F: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let cancel = CancellationToken::new();
    let mut task = AbortOnDropTask::new(tokio::spawn(operation(cancel.clone())));
    match tokio::time::timeout(timeout, task.handle_mut()).await {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(error))) => Err(ClipboardRouterError::Backend(error.to_string())),
        Ok(Err(error)) => Err(ClipboardRouterError::Backend(format!(
            "backend task failed: {error}"
        ))),
        Err(_) => {
            cancel.cancel();
            task.abort();
            Err(ClipboardRouterError::TimedOut)
        }
    }
}

struct AbortOnDropTask<T> {
    handle: tokio::task::JoinHandle<T>,
}

impl<T> AbortOnDropTask<T> {
    fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self { handle }
    }

    fn handle_mut(&mut self) -> &mut tokio::task::JoinHandle<T> {
        &mut self.handle
    }

    fn abort(&self) {
        self.handle.abort();
    }
}

impl<T> Drop for AbortOnDropTask<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn reject_command<Snapshot>(command: ClipboardRouterCommand<Snapshot>) {
    match command {
        ClipboardRouterCommand::ClipboardChanged { reply, .. }
        | ClipboardRouterCommand::SetActive { reply, .. }
        | ClipboardRouterCommand::Barrier { reply } => {
            let _ = reply.send(Err(ClipboardRouterError::Closed));
        }
        ClipboardRouterCommand::ActiveProfile { reply } => {
            let _ = reply.send(Err(ClipboardRouterError::Closed));
        }
        ClipboardRouterCommand::Shutdown { reply } => {
            let _ = reply.send(Err(ClipboardRouterError::Closed));
        }
    }
}

impl<Snapshot> ClipboardRouterHandle<Snapshot>
where
    Snapshot: Send + 'static,
{
    pub async fn clipboard_changed(&self, snapshot: Snapshot) -> Result<(), ClipboardRouterError> {
        let accepting = self.admission.lock().await;
        if !*accepting {
            return Err(ClipboardRouterError::Closed);
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::ClipboardChanged { snapshot, reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        drop(accepting);
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }

    pub async fn set_active(&self, profile_id: String) -> Result<(), ClipboardRouterError> {
        let accepting = self.admission.lock().await;
        if !*accepting {
            return Err(ClipboardRouterError::Closed);
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::SetActive { profile_id, reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        drop(accepting);
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }

    pub async fn active_profile(&self) -> Result<String, ClipboardRouterError> {
        let accepting = self.admission.lock().await;
        if !*accepting {
            return Err(ClipboardRouterError::Closed);
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::ActiveProfile { reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        drop(accepting);
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }

    pub async fn barrier(&self) -> Result<(), ClipboardRouterError> {
        let accepting = self.admission.lock().await;
        if !*accepting {
            return Err(ClipboardRouterError::Closed);
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::Barrier { reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        drop(accepting);
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }
}

impl<Snapshot> ClipboardRouterTask<Snapshot>
where
    Snapshot: Send + 'static,
{
    pub async fn shutdown(mut self) -> Result<(), ClipboardRouterError> {
        let deadline = tokio::time::Instant::now() + self.shutdown_timeout;
        let mut accepting = tokio::time::timeout_at(deadline, self.admission.lock())
            .await
            .map_err(|_| ClipboardRouterError::TimedOut)?;
        *accepting = false;
        let (reply, acknowledged) = oneshot::channel();
        tokio::time::timeout_at(
            deadline,
            self.commands
                .send(ClipboardRouterCommand::Shutdown { reply }),
        )
        .await
        .map_err(|_| ClipboardRouterError::TimedOut)?
        .map_err(|_| ClipboardRouterError::Closed)?;
        drop(accepting);
        tokio::time::timeout_at(deadline, acknowledged)
            .await
            .map_err(|_| ClipboardRouterError::TimedOut)?
            .map_err(|_| ClipboardRouterError::Closed)??;
        let Some(mut join) = self.join.take() else {
            return Ok(());
        };
        match tokio::time::timeout_at(deadline, &mut join).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                return Err(ClipboardRouterError::Backend(format!(
                    "router task failed: {error}"
                )));
            }
            Err(_) => {
                join.abort();
                let _ = join.await;
                return Err(ClipboardRouterError::TimedOut);
            }
        }
        Ok(())
    }
}

impl<Snapshot> Drop for ClipboardRouterTask<Snapshot> {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tokio::sync::{Mutex, Notify};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Recorded {
        Dispatch { profile_id: String, value: String },
        SetActive(String),
    }

    struct RecordingBackend {
        events: Mutex<Vec<Recorded>>,
        durable_active: Mutex<String>,
        rejected_profiles: Mutex<HashSet<String>>,
        late_commit_profiles: Mutex<HashSet<String>>,
        never_complete_profiles: Mutex<HashSet<String>>,
        persist_started: Notify,
        release_late_commit: Notify,
        rejected_snapshots: Mutex<HashSet<String>>,
        pending_snapshots: Mutex<HashSet<String>>,
        pending_entered: Notify,
        dispatch_entered: Notify,
        release_dispatch: Notify,
        block_dispatch: Mutex<bool>,
    }

    impl Default for RecordingBackend {
        fn default() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
                durable_active: Mutex::new("profile-a".into()),
                rejected_profiles: Mutex::new(HashSet::new()),
                late_commit_profiles: Mutex::new(HashSet::new()),
                never_complete_profiles: Mutex::new(HashSet::new()),
                persist_started: Notify::new(),
                release_late_commit: Notify::new(),
                rejected_snapshots: Mutex::new(HashSet::new()),
                pending_snapshots: Mutex::new(HashSet::new()),
                pending_entered: Notify::new(),
                dispatch_entered: Notify::new(),
                release_dispatch: Notify::new(),
                block_dispatch: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl ClipboardRouterBackend<String> for RecordingBackend {
        async fn load_active_profile(&self, _cancel: CancellationToken) -> anyhow::Result<String> {
            Ok(self.durable_active.lock().await.clone())
        }

        async fn dispatch_snapshot(
            &self,
            profile_id: &str,
            snapshot: String,
            cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            if self.pending_snapshots.lock().await.contains(&snapshot) {
                self.pending_entered.notify_one();
                cancel.cancelled().await;
                anyhow::bail!("cancelled pending snapshot");
            }
            if self.rejected_snapshots.lock().await.contains(&snapshot) {
                anyhow::bail!("rejected snapshot {snapshot}");
            }
            self.events.lock().await.push(Recorded::Dispatch {
                profile_id: profile_id.to_string(),
                value: snapshot,
            });
            if *self.block_dispatch.lock().await {
                self.dispatch_entered.notify_one();
                self.release_dispatch.notified().await;
            }
            Ok(())
        }

        async fn persist_active_profile(
            &self,
            profile_id: &str,
            cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            if self.rejected_profiles.lock().await.contains(profile_id) {
                anyhow::bail!("rejected profile {profile_id}");
            }
            if self.late_commit_profiles.lock().await.contains(profile_id) {
                self.persist_started.notify_one();
                cancel.cancelled().await;
                self.release_late_commit.notified().await;
            }
            if self
                .never_complete_profiles
                .lock()
                .await
                .contains(profile_id)
            {
                self.persist_started.notify_one();
                cancel.cancelled().await;
                std::future::pending::<()>().await;
            }
            self.events
                .lock()
                .await
                .push(Recorded::SetActive(profile_id.to_string()));
            *self.durable_active.lock().await = profile_id.to_owned();
            Ok(())
        }
    }

    #[tokio::test]
    async fn clipboard_events_before_and_after_switch_use_the_matching_profile() {
        let backend = Arc::new(RecordingBackend::default());
        let (router, task) = spawn_clipboard_router(backend.clone());

        router.clipboard_changed("before".into()).await.unwrap();
        router.set_active("profile-b".into()).await.unwrap();
        router.clipboard_changed("after".into()).await.unwrap();

        assert_eq!(
            *backend.events.lock().await,
            vec![
                Recorded::Dispatch {
                    profile_id: "profile-a".into(),
                    value: "before".into(),
                },
                Recorded::SetActive("profile-b".into()),
                Recorded::Dispatch {
                    profile_id: "profile-b".into(),
                    value: "after".into(),
                },
            ]
        );
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn router_rebuild_uses_the_durable_active_profile() {
        let backend = Arc::new(RecordingBackend::default());
        *backend.durable_active.lock().await = "profile-b".into();
        let (router, task) = spawn_clipboard_router(backend);

        assert_eq!(router.active_profile().await.unwrap(), "profile-b");

        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_persistence_keeps_the_previous_active_profile() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .rejected_profiles
            .lock()
            .await
            .insert("profile-b".into());
        let (router, task) = spawn_clipboard_router(backend.clone());

        let error = router.set_active("profile-b".into()).await.unwrap_err();
        assert!(matches!(error, ClipboardRouterError::Backend(_)));
        router.clipboard_changed("still-a".into()).await.unwrap();

        assert_eq!(
            backend.events.lock().await.last(),
            Some(&Recorded::Dispatch {
                profile_id: "profile-a".into(),
                value: "still-a".into(),
            })
        );
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn set_active_waits_for_an_already_accepted_clipboard_dispatch() {
        let backend = Arc::new(RecordingBackend::default());
        *backend.block_dispatch.lock().await = true;
        let (router, task) = spawn_clipboard_router(backend.clone());

        let dispatch_router = router.clone();
        let dispatch = tokio::spawn(async move {
            dispatch_router
                .clipboard_changed("blocked".into())
                .await
                .unwrap();
        });
        backend.dispatch_entered.notified().await;

        let switch_router = router.clone();
        let mut switch = tokio::spawn(async move {
            switch_router.set_active("profile-b".into()).await.unwrap();
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut switch)
                .await
                .is_err(),
            "switch must not overtake the in-flight clipboard event"
        );

        backend.release_dispatch.notify_waiters();
        dispatch.await.unwrap();
        switch.await.unwrap();
        assert_eq!(
            *backend.events.lock().await,
            vec![
                Recorded::Dispatch {
                    profile_id: "profile-a".into(),
                    value: "blocked".into(),
                },
                Recorded::SetActive("profile-b".into()),
            ]
        );
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn one_failed_dispatch_does_not_kill_the_router() {
        let backend = Arc::new(RecordingBackend::default());
        backend.rejected_snapshots.lock().await.insert("bad".into());
        let (router, task) = spawn_clipboard_router(backend.clone());

        let error = router.clipboard_changed("bad".into()).await.unwrap_err();
        assert!(matches!(error, ClipboardRouterError::Backend(_)));
        router.clipboard_changed("good".into()).await.unwrap();

        assert_eq!(
            backend.events.lock().await.last(),
            Some(&Recorded::Dispatch {
                profile_id: "profile-a".into(),
                value: "good".into(),
            })
        );
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn pending_backend_times_out_without_killing_the_router() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .pending_snapshots
            .lock()
            .await
            .insert("stuck".into());
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend.clone(),
            Duration::from_millis(25),
            Duration::from_secs(1),
        );

        assert_eq!(
            router.clipboard_changed("stuck".into()).await,
            Err(ClipboardRouterError::TimedOut)
        );
        router.clipboard_changed("good".into()).await.unwrap();
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn barrier_waits_for_a_late_persist_commit_and_reconciliation() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .late_commit_profiles
            .lock()
            .await
            .insert("profile-b".into());
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend.clone(),
            Duration::from_millis(250),
            Duration::from_secs(1),
        );

        let switching = {
            let router = router.clone();
            tokio::spawn(async move { router.set_active("profile-b".into()).await })
        };
        backend.persist_started.notified().await;
        assert!(switching.await.unwrap().is_err());
        assert_eq!(*backend.durable_active.lock().await, "profile-a");

        let mut barrier = {
            let router = router.clone();
            tokio::spawn(async move { router.barrier().await })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut barrier)
                .await
                .is_err(),
            "barrier must not pass while a timed-out persist can still commit"
        );

        backend.release_late_commit.notify_one();
        barrier.await.unwrap().unwrap();
        assert_eq!(*backend.durable_active.lock().await, "profile-b");
        assert_eq!(router.active_profile().await.unwrap(), "profile-b");

        router
            .clipboard_changed("after-reconcile".into())
            .await
            .unwrap();
        assert_eq!(
            backend.events.lock().await.last(),
            Some(&Recorded::Dispatch {
                profile_id: "profile-b".into(),
                value: "after-reconcile".into(),
            })
        );
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn permanently_uncertain_persist_blocks_barrier_and_clean_shutdown() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .never_complete_profiles
            .lock()
            .await
            .insert("profile-b".into());
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend.clone(),
            Duration::from_millis(25),
            Duration::from_secs(1),
        );

        let switching = {
            let router = router.clone();
            tokio::spawn(async move { router.set_active("profile-b".into()).await })
        };
        backend.persist_started.notified().await;
        assert!(switching.await.unwrap().is_err());

        assert!(router.barrier().await.is_err());
        assert!(router.active_profile().await.is_err());
        assert!(router.clipboard_changed("blocked".into()).await.is_err());
        assert!(router.set_active("profile-a".into()).await.is_err());
        assert!(task.shutdown().await.is_err());
    }

    #[tokio::test]
    async fn shutdown_closes_admission_before_its_queue_marker() {
        let backend = Arc::new(RecordingBackend::default());
        *backend.block_dispatch.lock().await = true;
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend.clone(),
            Duration::from_secs(1),
            Duration::from_secs(1),
        );

        let dispatch_router = router.clone();
        let dispatch = tokio::spawn(async move {
            dispatch_router
                .clipboard_changed("blocked".into())
                .await
                .unwrap();
        });
        backend.dispatch_entered.notified().await;

        let shutdown = tokio::spawn(task.shutdown());
        loop {
            if !*router.admission.lock().await {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            router.set_active("profile-b".into()).await,
            Err(ClipboardRouterError::Closed),
            "commands racing after shutdown starts must be rejected before enqueue"
        );

        backend.release_dispatch.notify_one();
        dispatch.await.unwrap();
        shutdown.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn hard_shutdown_deadline_aborts_a_stuck_backend_task() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .pending_snapshots
            .lock()
            .await
            .insert("stuck".into());
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend.clone(),
            Duration::from_secs(30),
            Duration::from_millis(25),
        );
        let dispatch = tokio::spawn(async move { router.clipboard_changed("stuck".into()).await });
        backend.pending_entered.notified().await;

        assert_eq!(task.shutdown().await, Err(ClipboardRouterError::TimedOut));
        assert_eq!(dispatch.await.unwrap(), Err(ClipboardRouterError::Closed));
    }
}
