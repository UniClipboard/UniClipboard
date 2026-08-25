//! Serialized authority for Windows multi-space mutations.
//!
//! This module deliberately owns only ordering and failure semantics. Concrete
//! catalog, runtime, and clipboard-router adapters are wired elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

use super::clipboard_router::ClipboardRouterHandle;

#[async_trait]
pub(crate) trait CatalogPort: Send + Sync {
    /// Implementations must be bounded and must not call another mutation on
    /// this authority. Read-only state queries are safe because state locks are
    /// never held across port awaits.
    async fn profile_dir(&self, profile_id: &str) -> anyhow::Result<Option<String>>;

    /// Remove only the catalog record. Implementations must not delete data.
    async fn remove(&self, profile_id: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait RuntimePort: Send + Sync {
    /// Production adapters must enforce their own hard lifecycle deadlines.
    async fn ensure_available(&self, profile_id: &str) -> anyhow::Result<()>;

    async fn stop(&self, profile_id: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait RouterPort: Send + Sync {
    async fn active_profile(&self) -> anyhow::Result<String>;

    async fn set_active(&self, profile_id: &str) -> anyhow::Result<()>;

    async fn drain(&self) -> anyhow::Result<()>;
}

pub(crate) struct ClipboardRouterPort<Snapshot> {
    router: ClipboardRouterHandle<Snapshot>,
}

impl<Snapshot> ClipboardRouterPort<Snapshot> {
    pub(crate) fn new(router: ClipboardRouterHandle<Snapshot>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl<Snapshot> RouterPort for ClipboardRouterPort<Snapshot>
where
    Snapshot: Send + 'static,
{
    async fn active_profile(&self) -> anyhow::Result<String> {
        Ok(self.router.active_profile().await?)
    }

    async fn set_active(&self, profile_id: &str) -> anyhow::Result<()> {
        Ok(self.router.set_active(profile_id.to_owned()).await?)
    }

    async fn drain(&self) -> anyhow::Result<()> {
        Ok(self.router.barrier().await?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum WindowsSpaceAuthorityError {
    #[error("Windows space authority is quiescing")]
    Quiescing,
    #[error("the legacy profile cannot be removed")]
    LegacyProfileCannotBeRemoved,
    #[error("the active-send profile cannot be removed")]
    ActiveProfileCannotBeRemoved,
    #[error("profile not found: {0}")]
    ProfileNotFound(String),
    #[error("runtime operation failed: {0}")]
    Runtime(String),
    #[error("catalog operation failed: {0}")]
    Catalog(String),
    #[error("router transaction failed: {0}")]
    Router(String),
}

pub(crate) struct WindowsSpaceAuthority {
    operation_gate: Arc<Mutex<()>>,
    accepting: AtomicBool,
    catalog: Arc<dyn CatalogPort>,
    runtime: Arc<dyn RuntimePort>,
    router: Arc<dyn RouterPort>,
}

impl WindowsSpaceAuthority {
    pub(crate) fn new(
        catalog: Arc<dyn CatalogPort>,
        runtime: Arc<dyn RuntimePort>,
        router: Arc<dyn RouterPort>,
    ) -> Self {
        Self {
            operation_gate: Arc::new(Mutex::new(())),
            accepting: AtomicBool::new(true),
            catalog,
            runtime,
            router,
        }
    }

    pub(crate) async fn acquire_mutation(
        &self,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, WindowsSpaceAuthorityError> {
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        let gate = Arc::clone(&self.operation_gate).lock_owned().await;
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        Ok(gate)
    }

    #[cfg(test)]
    pub(crate) async fn active_profile(&self) -> Result<String, WindowsSpaceAuthorityError> {
        self.router
            .active_profile()
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Router(error.to_string()))
    }

    /// Wait for the current mutation, then reject all subsequent mutations.
    pub(crate) async fn quiesce(&self) -> Result<(), WindowsSpaceAuthorityError> {
        self.accepting.store(false, Ordering::Release);
        let _gate = self.operation_gate.lock().await;
        self.router
            .drain()
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Router(error.to_string()))
    }

    pub(crate) async fn set_active(&self, target: &str) -> Result<(), WindowsSpaceAuthorityError> {
        let _gate = self.acquire_mutation().await?;
        self.runtime
            .ensure_available(target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Runtime(error.to_string()))?;
        self.router
            .set_active(target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Router(error.to_string()))
    }

    pub(crate) async fn remove(&self, profile_id: &str) -> Result<(), WindowsSpaceAuthorityError> {
        let _gate = self.acquire_mutation().await?;
        let active_profile = self
            .router
            .active_profile()
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Router(error.to_string()))?;
        let profile_dir = self
            .catalog
            .profile_dir(profile_id)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Catalog(error.to_string()))?
            .ok_or_else(|| WindowsSpaceAuthorityError::ProfileNotFound(profile_id.to_owned()))?;
        if profile_dir == "." {
            return Err(WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved);
        }
        if active_profile == profile_id {
            return Err(WindowsSpaceAuthorityError::ActiveProfileCannotBeRemoved);
        }

        self.runtime
            .stop(profile_id)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Runtime(error.to_string()))?;
        self.catalog
            .remove(profile_id)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Catalog(error.to_string()))?;
        Ok(())
    }

    fn ensure_accepting(accepting: bool) -> Result<(), WindowsSpaceAuthorityError> {
        if accepting {
            Ok(())
        } else {
            Err(WindowsSpaceAuthorityError::Quiescing)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CatalogPort, RouterPort, RuntimePort, WindowsSpaceAuthority, WindowsSpaceAuthorityError,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, Notify};
    use tokio_util::sync::CancellationToken;

    use crate::daemon::clipboard_router::{
        spawn_clipboard_router_with_timeouts, ClipboardRouterBackend, ClipboardRouterTask,
    };

    #[derive(Default)]
    struct FakeState {
        active: String,
        directories: HashMap<String, String>,
        calls: Vec<String>,
        fail_catalog_remove: bool,
        fail_runtime_available: bool,
        fail_runtime_stop: bool,
        fail_router: bool,
    }

    #[derive(Clone, Default)]
    struct FakePorts {
        state: Arc<Mutex<FakeState>>,
        available_entered: Arc<Notify>,
        available_release: Arc<Notify>,
        block_available: Arc<Mutex<bool>>,
        persist_committed: Arc<Notify>,
        release_persist: Arc<Notify>,
        block_persist_after_commit: Arc<Mutex<bool>>,
    }

    impl FakePorts {
        async fn seeded() -> Self {
            let ports = Self::default();
            let mut state = ports.state.lock().await;
            state.active = "legacy".into();
            state.directories.insert("legacy".into(), ".".into());
            state
                .directories
                .insert("secondary".into(), "profiles/secondary".into());
            state
                .directories
                .insert("other".into(), "profiles/other".into());
            drop(state);
            ports
        }

        async fn calls(&self) -> Vec<String> {
            self.state.lock().await.calls.clone()
        }
    }

    #[async_trait]
    impl CatalogPort for FakePorts {
        async fn profile_dir(&self, profile_id: &str) -> anyhow::Result<Option<String>> {
            let mut state = self.state.lock().await;
            state
                .calls
                .push(format!("catalog.profile_dir:{profile_id}"));
            Ok(state.directories.get(profile_id).cloned())
        }

        async fn remove(&self, profile_id: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state.calls.push(format!("catalog.remove:{profile_id}"));
            if state.fail_catalog_remove {
                anyhow::bail!("catalog remove failed");
            }
            state.directories.remove(profile_id);
            Ok(())
        }
    }

    #[async_trait]
    impl RuntimePort for FakePorts {
        async fn ensure_available(&self, profile_id: &str) -> anyhow::Result<()> {
            self.state
                .lock()
                .await
                .calls
                .push(format!("runtime.ensure_available:{profile_id}"));
            if *self.block_available.lock().await {
                self.available_entered.notify_one();
                self.available_release.notified().await;
            }
            if self.state.lock().await.fail_runtime_available {
                anyhow::bail!("runtime unavailable");
            }
            Ok(())
        }

        async fn stop(&self, profile_id: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state.calls.push(format!("runtime.stop:{profile_id}"));
            if state.fail_runtime_stop {
                anyhow::bail!("runtime stop failed");
            }
            Ok(())
        }
    }

    #[async_trait]
    impl RouterPort for FakePorts {
        async fn active_profile(&self) -> anyhow::Result<String> {
            let mut state = self.state.lock().await;
            state.calls.push("router.active_profile".into());
            Ok(state.active.clone())
        }

        async fn set_active(&self, profile_id: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state.calls.push(format!("router.set_active:{profile_id}"));
            if state.fail_router {
                anyhow::bail!("router failed");
            }
            state.active = profile_id.to_owned();
            Ok(())
        }

        async fn drain(&self) -> anyhow::Result<()> {
            self.state.lock().await.calls.push("router.drain".into());
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardRouterBackend<String> for FakePorts {
        async fn load_active_profile(&self, cancel: CancellationToken) -> anyhow::Result<String> {
            if cancel.is_cancelled() {
                anyhow::bail!("load cancelled");
            }
            Ok(self.state.lock().await.active.clone())
        }

        async fn dispatch_snapshot(
            &self,
            profile_id: &str,
            _snapshot: String,
            cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            if cancel.is_cancelled() {
                anyhow::bail!("dispatch cancelled");
            }
            self.state
                .lock()
                .await
                .calls
                .push(format!("backend.dispatch:{profile_id}"));
            Ok(())
        }

        async fn persist_active_profile(
            &self,
            profile_id: &str,
            cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            {
                let mut state = self.state.lock().await;
                state.calls.push(format!("backend.persist:{profile_id}"));
                state.active = profile_id.to_owned();
            }
            if *self.block_persist_after_commit.lock().await {
                self.persist_committed.notify_one();
                tokio::select! {
                    _ = self.release_persist.notified() => {}
                    _ = cancel.cancelled() => anyhow::bail!("persist cancelled after commit"),
                }
            }
            Ok(())
        }
    }

    fn authority(ports: &FakePorts) -> Arc<WindowsSpaceAuthority> {
        Arc::new(WindowsSpaceAuthority::new(
            Arc::new(ports.clone()),
            Arc::new(ports.clone()),
            Arc::new(ports.clone()),
        ))
    }

    fn authority_with_real_router(
        ports: &FakePorts,
        operation_timeout: Duration,
    ) -> (Arc<WindowsSpaceAuthority>, ClipboardRouterTask<String>) {
        let backend: Arc<dyn ClipboardRouterBackend<String>> = Arc::new(ports.clone());
        let (router, task) = spawn_clipboard_router_with_timeouts(
            backend,
            operation_timeout,
            Duration::from_secs(1),
        );
        let router = Arc::new(super::ClipboardRouterPort::new(router));
        (
            Arc::new(WindowsSpaceAuthority::new(
                Arc::new(ports.clone()),
                Arc::new(ports.clone()),
                router,
            )),
            task,
        )
    }

    #[tokio::test]
    async fn set_active_orders_runtime_then_router_transaction() {
        let ports = FakePorts::seeded().await;
        authority(&ports).set_active("secondary").await.unwrap();

        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "router.set_active:secondary",
            ]
        );
        assert_eq!(ports.state.lock().await.active, "secondary");
    }

    #[tokio::test]
    async fn unavailable_target_keeps_old_active() {
        let ports = FakePorts::seeded().await;
        ports.state.lock().await.fail_runtime_available = true;

        authority(&ports).set_active("secondary").await.unwrap_err();

        assert_eq!(ports.state.lock().await.active, "legacy");
        assert_eq!(ports.calls().await, ["runtime.ensure_available:secondary"]);
    }

    #[tokio::test]
    async fn same_target_still_checks_runtime_health() {
        let ports = FakePorts::seeded().await;
        ports.state.lock().await.fail_runtime_available = true;

        authority(&ports).set_active("legacy").await.unwrap_err();

        assert_eq!(ports.state.lock().await.active, "legacy");
        assert_eq!(ports.calls().await, ["runtime.ensure_available:legacy"]);
    }

    #[tokio::test]
    async fn router_transaction_failure_keeps_every_active_view_unchanged() {
        let ports = FakePorts::seeded().await;
        ports.state.lock().await.fail_router = true;
        let authority = authority(&ports);

        authority.set_active("secondary").await.unwrap_err();

        assert_eq!(authority.active_profile().await.unwrap(), "legacy");
        assert_eq!(ports.state.lock().await.active, "legacy");
        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "router.set_active:secondary",
                "router.active_profile",
            ]
        );
    }

    #[tokio::test]
    async fn active_profile_is_queried_from_the_router_single_source_of_truth() {
        let ports = FakePorts::seeded().await;
        let authority = authority(&ports);
        ports.state.lock().await.active = "secondary".into();

        assert_eq!(authority.active_profile().await.unwrap(), "secondary");
    }

    #[tokio::test]
    async fn remove_rejects_legacy_without_stopping_or_deleting() {
        let ports = FakePorts::seeded().await;
        let error = authority(&ports).remove("legacy").await.unwrap_err();

        assert_eq!(
            error,
            WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved
        );
        assert_eq!(
            ports.calls().await,
            ["router.active_profile", "catalog.profile_dir:legacy"]
        );
        assert!(ports.state.lock().await.directories.contains_key("legacy"));
    }

    #[tokio::test]
    async fn remove_stops_secondary_before_removing_record() {
        let ports = FakePorts::seeded().await;
        authority(&ports).remove("secondary").await.unwrap();

        assert_eq!(
            ports.calls().await,
            [
                "router.active_profile",
                "catalog.profile_dir:secondary",
                "runtime.stop:secondary",
                "catalog.remove:secondary",
            ]
        );
        assert!(!ports
            .state
            .lock()
            .await
            .directories
            .contains_key("secondary"));
    }

    #[tokio::test]
    async fn stop_failure_preserves_secondary_record() {
        let ports = FakePorts::seeded().await;
        ports.state.lock().await.fail_runtime_stop = true;

        authority(&ports).remove("secondary").await.unwrap_err();

        assert!(ports
            .state
            .lock()
            .await
            .directories
            .contains_key("secondary"));
        assert_eq!(
            ports.calls().await,
            [
                "router.active_profile",
                "catalog.profile_dir:secondary",
                "runtime.stop:secondary"
            ]
        );
    }

    #[tokio::test]
    async fn remove_rejects_current_non_legacy_profile_before_stop() {
        let ports = FakePorts::seeded().await;
        let authority = authority(&ports);
        authority.set_active("secondary").await.unwrap();

        let error = authority.remove("secondary").await.unwrap_err();

        assert_eq!(
            error,
            WindowsSpaceAuthorityError::ActiveProfileCannotBeRemoved
        );
        assert!(!ports
            .calls()
            .await
            .contains(&"runtime.stop:secondary".to_string()));
        assert!(ports
            .state
            .lock()
            .await
            .directories
            .contains_key("secondary"));
    }

    #[tokio::test]
    async fn remove_uses_the_router_actual_active_profile() {
        let ports = FakePorts::seeded().await;
        let authority = authority(&ports);
        ports.state.lock().await.active = "secondary".into();

        let error = authority.remove("secondary").await.unwrap_err();

        assert_eq!(
            error,
            WindowsSpaceAuthorityError::ActiveProfileCannotBeRemoved
        );
        assert!(!ports
            .calls()
            .await
            .contains(&"runtime.stop:secondary".to_string()));
    }

    #[tokio::test]
    async fn operations_are_serialized() {
        let ports = FakePorts::seeded().await;
        *ports.block_available.lock().await = true;
        let authority = authority(&ports);
        let switching = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.set_active("secondary").await })
        };
        ports.available_entered.notified().await;
        let removing = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.remove("other").await })
        };

        tokio::task::yield_now().await;
        assert_eq!(ports.calls().await, ["runtime.ensure_available:secondary"]);
        ports.available_release.notify_one();
        switching.await.unwrap().unwrap();
        removing.await.unwrap().unwrap();

        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "router.set_active:secondary",
                "router.active_profile",
                "catalog.profile_dir:other",
                "runtime.stop:other",
                "catalog.remove:other",
            ]
        );
    }

    #[tokio::test]
    async fn quiescing_rejects_new_mutations() {
        let ports = FakePorts::seeded().await;
        let authority = authority(&ports);
        authority.quiesce().await.unwrap();

        assert_eq!(
            authority.set_active("secondary").await.unwrap_err(),
            WindowsSpaceAuthorityError::Quiescing
        );
        assert_eq!(
            authority.remove("secondary").await.unwrap_err(),
            WindowsSpaceAuthorityError::Quiescing
        );
        assert_eq!(ports.calls().await, ["router.drain"]);
    }

    #[tokio::test]
    async fn quiesce_closes_admission_then_waits_for_the_current_mutation() {
        let ports = FakePorts::seeded().await;
        *ports.block_available.lock().await = true;
        let authority = authority(&ports);
        let switching = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.set_active("secondary").await })
        };
        ports.available_entered.notified().await;
        let mut quiescing = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.quiesce().await })
        };

        tokio::task::yield_now().await;
        assert!(!quiescing.is_finished());
        assert_eq!(
            authority.remove("other").await.unwrap_err(),
            WindowsSpaceAuthorityError::Quiescing
        );

        ports.available_release.notify_one();
        switching.await.unwrap().unwrap();
        (&mut quiescing).await.unwrap().unwrap();
        assert_eq!(authority.active_profile().await.unwrap(), "secondary");
    }

    #[tokio::test]
    async fn cancelled_authority_caller_does_not_cancel_an_enqueued_router_commit() {
        let ports = FakePorts::seeded().await;
        *ports.block_persist_after_commit.lock().await = true;
        let (authority, task) = authority_with_real_router(&ports, Duration::from_secs(1));
        let switching = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.set_active("secondary").await })
        };
        ports.persist_committed.notified().await;

        switching.abort();
        switching.await.unwrap_err();
        ports.release_persist.notify_one();

        assert_eq!(authority.active_profile().await.unwrap(), "secondary");
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn committed_persist_timeout_reconciles_to_success_through_the_real_adapter() {
        let ports = FakePorts::seeded().await;
        *ports.block_persist_after_commit.lock().await = true;
        let (authority, task) = authority_with_real_router(&ports, Duration::from_millis(25));

        authority.set_active("secondary").await.unwrap();

        assert_eq!(authority.active_profile().await.unwrap(), "secondary");
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn quiesce_waits_for_a_cancelled_callers_enqueued_router_command() {
        let ports = FakePorts::seeded().await;
        *ports.block_persist_after_commit.lock().await = true;
        let (authority, task) = authority_with_real_router(&ports, Duration::from_secs(1));
        let switching = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.set_active("secondary").await })
        };
        ports.persist_committed.notified().await;
        switching.abort();
        switching.await.unwrap_err();

        let mut quiescing = {
            let authority = Arc::clone(&authority);
            tokio::spawn(async move { authority.quiesce().await })
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut quiescing)
                .await
                .is_err(),
            "barrier must stay behind the already enqueued set-active command"
        );

        ports.release_persist.notify_one();
        quiescing.await.unwrap().unwrap();
        assert_eq!(authority.active_profile().await.unwrap(), "secondary");
        task.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn rebuilt_real_router_and_remove_share_the_same_durable_active_profile() {
        let ports = FakePorts::seeded().await;
        let (authority, first_task) = authority_with_real_router(&ports, Duration::from_secs(1));
        authority.set_active("secondary").await.unwrap();
        first_task.shutdown().await.unwrap();

        let (rebuilt, rebuilt_task) = authority_with_real_router(&ports, Duration::from_secs(1));
        assert_eq!(rebuilt.active_profile().await.unwrap(), "secondary");
        assert_eq!(
            rebuilt.remove("secondary").await.unwrap_err(),
            WindowsSpaceAuthorityError::ActiveProfileCannotBeRemoved
        );
        assert!(!ports
            .calls()
            .await
            .contains(&"runtime.stop:secondary".to_string()));
        rebuilt_task.shutdown().await.unwrap();
    }
}
