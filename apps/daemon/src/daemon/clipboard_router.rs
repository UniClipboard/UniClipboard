//! Serial authority for process-wide clipboard sends and active-space changes.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClipboardRouterError {
    #[error("clipboard router is closed")]
    Closed,
    #[error("clipboard routing backend failed: {0}")]
    Backend(String),
}

#[async_trait]
pub trait ClipboardRouterBackend<Snapshot>: Send + Sync {
    async fn dispatch_snapshot(&self, profile_id: &str, snapshot: Snapshot) -> anyhow::Result<()>;

    async fn persist_active_profile(&self, profile_id: &str) -> anyhow::Result<()>;
}

pub struct ClipboardRouterHandle<Snapshot> {
    commands: mpsc::Sender<ClipboardRouterCommand<Snapshot>>,
}

impl<Snapshot> Clone for ClipboardRouterHandle<Snapshot> {
    fn clone(&self) -> Self {
        Self {
            commands: self.commands.clone(),
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
    join: tokio::task::JoinHandle<()>,
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
    let (commands, mut receiver) = mpsc::channel(64);
    let join = tokio::spawn(async move {
        let mut active_profile = initial_active_profile;
        while let Some(command) = receiver.recv().await {
            match command {
                ClipboardRouterCommand::ClipboardChanged { snapshot, reply } => {
                    let result = backend
                        .dispatch_snapshot(&active_profile, snapshot)
                        .await
                        .map_err(|error| ClipboardRouterError::Backend(error.to_string()));
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::SetActive { profile_id, reply } => {
                    let result = backend
                        .persist_active_profile(&profile_id)
                        .await
                        .map_err(|error| ClipboardRouterError::Backend(error.to_string()));
                    if result.is_ok() {
                        active_profile = profile_id;
                    }
                    let _ = reply.send(result);
                }
                ClipboardRouterCommand::Shutdown { reply } => {
                    let _ = reply.send(());
                    break;
                }
            }
        }
    });
    let task_commands = commands.clone();
    (
        ClipboardRouterHandle { commands },
        ClipboardRouterTask {
            commands: task_commands,
            join,
        },
    )
}

impl<Snapshot> ClipboardRouterHandle<Snapshot>
where
    Snapshot: Send + 'static,
{
    pub async fn clipboard_changed(&self, snapshot: Snapshot) -> Result<(), ClipboardRouterError> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::ClipboardChanged { snapshot, reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }

    pub async fn set_active(&self, profile_id: String) -> Result<(), ClipboardRouterError> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::SetActive { profile_id, reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        result.await.map_err(|_| ClipboardRouterError::Closed)?
    }
}

impl<Snapshot> ClipboardRouterTask<Snapshot>
where
    Snapshot: Send + 'static,
{
    pub async fn shutdown(self) -> Result<(), ClipboardRouterError> {
        let (reply, acknowledged) = oneshot::channel();
        self.commands
            .send(ClipboardRouterCommand::Shutdown { reply })
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        acknowledged
            .await
            .map_err(|_| ClipboardRouterError::Closed)?;
        self.join
            .await
            .map_err(|error| ClipboardRouterError::Backend(error.to_string()))?;
        Ok(())
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
        ) -> anyhow::Result<()> {
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

        async fn persist_active_profile(&self, profile_id: &str) -> anyhow::Result<()> {
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
}
