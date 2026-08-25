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
}

const BACKEND_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
const ROUTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

#[async_trait]
pub trait ClipboardRouterBackend<Snapshot>: Send + Sync {
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
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

pub struct ClipboardRouterTask<Snapshot> {
    commands: mpsc::Sender<ClipboardRouterCommand<Snapshot>>,
    admission: Arc<Mutex<bool>>,
    join: Option<tokio::task::JoinHandle<()>>,
    shutdown_timeout: Duration,
}

pub fn spawn_clipboard_router<Snapshot>(
    initial_active_profile: String,
    backend: Arc<dyn ClipboardRouterBackend<Snapshot>>,
) -> (
    ClipboardRouterHandle<Snapshot>,
    ClipboardRouterTask<Snapshot>,
)
where
    Snapshot: Send + 'static,
{
    spawn_clipboard_router_with_timeouts(
        initial_active_profile,
        backend,
        BACKEND_OPERATION_TIMEOUT,
        ROUTER_SHUTDOWN_TIMEOUT,
    )
}

fn spawn_clipboard_router_with_timeouts<Snapshot>(
    initial_active_profile: String,
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
        let mut active_profile = initial_active_profile;
        while let Some(command) = receiver.recv().await {
            match command {
                ClipboardRouterCommand::ClipboardChanged { snapshot, reply } => {
                    let dispatch_backend = Arc::clone(&backend);
                    let profile_id = active_profile.clone();
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
                    let persist_backend = Arc::clone(&backend);
                    let target = profile_id.clone();
                    let result =
                        run_backend_operation(operation_timeout, move |cancel| async move {
                            persist_backend
                                .persist_active_profile(&target, cancel)
                                .await
                        })
                        .await;
                    if result.is_ok() {
                        active_profile = profile_id;
                    }
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::Shutdown { reply } => {
                    receiver.close();
                    while let Some(command) = receiver.recv().await {
                        reject_command(command);
                    }
                    let _ = reply.send(());
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

async fn run_backend_operation<Factory, F>(
    timeout: Duration,
    operation: Factory,
) -> Result<(), ClipboardRouterError>
where
    Factory: FnOnce(CancellationToken) -> F,
    F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let cancel = CancellationToken::new();
    let mut task = AbortOnDropTask::new(tokio::spawn(operation(cancel.clone())));
    match tokio::time::timeout(timeout, task.handle_mut()).await {
        Ok(Ok(Ok(()))) => Ok(()),
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
        | ClipboardRouterCommand::SetActive { reply, .. } => {
            let _ = reply.send(Err(ClipboardRouterError::Closed));
        }
        ClipboardRouterCommand::Shutdown { reply } => {
            let _ = reply.send(());
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
            .map_err(|_| ClipboardRouterError::Closed)?;
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

    #[derive(Default)]
    struct RecordingBackend {
        events: Mutex<Vec<Recorded>>,
        rejected_profiles: Mutex<HashSet<String>>,
        rejected_snapshots: Mutex<HashSet<String>>,
        pending_snapshots: Mutex<HashSet<String>>,
        pending_entered: Notify,
        dispatch_entered: Notify,
        release_dispatch: Notify,
        block_dispatch: Mutex<bool>,
    }

    #[async_trait]
    impl ClipboardRouterBackend<String> for RecordingBackend {
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
            _cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            if self.rejected_profiles.lock().await.contains(profile_id) {
                anyhow::bail!("rejected profile {profile_id}");
            }
            self.events
                .lock()
                .await
                .push(Recorded::SetActive(profile_id.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn clipboard_events_before_and_after_switch_use_the_matching_profile() {
        let backend = Arc::new(RecordingBackend::default());
        let (router, task) = spawn_clipboard_router("profile-a".into(), backend.clone());

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
    async fn failed_persistence_keeps_the_previous_active_profile() {
        let backend = Arc::new(RecordingBackend::default());
        backend
            .rejected_profiles
            .lock()
            .await
            .insert("profile-b".into());
        let (router, task) = spawn_clipboard_router("profile-a".into(), backend.clone());

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
        let (router, task) = spawn_clipboard_router("profile-a".into(), backend.clone());

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
        let (router, task) = spawn_clipboard_router("profile-a".into(), backend.clone());

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
            "profile-a".into(),
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
    async fn shutdown_closes_admission_before_its_queue_marker() {
        let backend = Arc::new(RecordingBackend::default());
        *backend.block_dispatch.lock().await = true;
        let (router, task) = spawn_clipboard_router_with_timeouts(
            "profile-a".into(),
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
            "profile-a".into(),
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
