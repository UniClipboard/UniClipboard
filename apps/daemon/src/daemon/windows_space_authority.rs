//! Serialized authority for Windows multi-space mutations.
//!
//! This module deliberately owns only ordering and failure semantics. Concrete
//! catalog, runtime, and clipboard-router adapters are wired elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

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
    /// Commit the durable catalog target and the router's in-memory target as
    /// one cancellation-safe, internally bounded actor operation. `Err` means
    /// neither is changed. The adapter must not re-enter an authority mutation.
    async fn set_active_transaction(&self, profile_id: &str) -> anyhow::Result<()>;
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

struct AuthorityState {
    active_profile: String,
}

pub(crate) struct WindowsSpaceAuthority {
    state: Mutex<AuthorityState>,
    operation_gate: Mutex<()>,
    accepting: AtomicBool,
    catalog: Arc<dyn CatalogPort>,
    runtime: Arc<dyn RuntimePort>,
    router: Arc<dyn RouterPort>,
}

impl WindowsSpaceAuthority {
    pub(crate) fn new(
        initial_active_profile: String,
        catalog: Arc<dyn CatalogPort>,
        runtime: Arc<dyn RuntimePort>,
        router: Arc<dyn RouterPort>,
    ) -> Self {
        Self {
            state: Mutex::new(AuthorityState {
                active_profile: initial_active_profile,
            }),
            operation_gate: Mutex::new(()),
            accepting: AtomicBool::new(true),
            catalog,
            runtime,
            router,
        }
    }

    pub(crate) async fn active_profile(&self) -> String {
        self.state.lock().await.active_profile.clone()
    }

    /// Wait for the current mutation, then reject all subsequent mutations.
    pub(crate) async fn quiesce(&self) {
        self.accepting.store(false, Ordering::Release);
        let _gate = self.operation_gate.lock().await;
    }

    pub(crate) async fn set_active(&self, target: &str) -> Result<(), WindowsSpaceAuthorityError> {
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        let _gate = self.operation_gate.lock().await;
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        self.runtime
            .ensure_available(target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Runtime(error.to_string()))?;
        if self.state.lock().await.active_profile == target {
            return Ok(());
        }
        self.router
            .set_active_transaction(target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Router(error.to_string()))?;
        self.state.lock().await.active_profile = target.to_owned();
        Ok(())
    }

    pub(crate) async fn remove(&self, profile_id: &str) -> Result<(), WindowsSpaceAuthorityError> {
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        let _gate = self.operation_gate.lock().await;
        Self::ensure_accepting(self.accepting.load(Ordering::Acquire))?;
        let profile_dir = self
            .catalog
            .profile_dir(profile_id)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Catalog(error.to_string()))?
            .ok_or_else(|| WindowsSpaceAuthorityError::ProfileNotFound(profile_id.to_owned()))?;
        if profile_dir == "." {
            return Err(WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved);
        }
        if self.state.lock().await.active_profile == profile_id {
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
    use tokio::sync::{Mutex, Notify};

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
        async fn set_active_transaction(&self, profile_id: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state
                .calls
                .push(format!("router.set_active_transaction:{profile_id}"));
            if state.fail_router {
                anyhow::bail!("router failed");
            }
            state.active = profile_id.to_owned();
            Ok(())
        }
    }

    fn authority(ports: &FakePorts) -> Arc<WindowsSpaceAuthority> {
        Arc::new(WindowsSpaceAuthority::new(
            "legacy".into(),
            Arc::new(ports.clone()),
            Arc::new(ports.clone()),
            Arc::new(ports.clone()),
        ))
    }

    #[tokio::test]
    async fn set_active_orders_runtime_then_router_transaction() {
        let ports = FakePorts::seeded().await;
        authority(&ports).set_active("secondary").await.unwrap();

        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "router.set_active_transaction:secondary",
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

        assert_eq!(authority.active_profile().await, "legacy");
        assert_eq!(ports.state.lock().await.active, "legacy");
        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "router.set_active_transaction:secondary",
            ]
        );
    }

    #[tokio::test]
    async fn remove_rejects_legacy_without_stopping_or_deleting() {
        let ports = FakePorts::seeded().await;
        let error = authority(&ports).remove("legacy").await.unwrap_err();

        assert_eq!(
            error,
            WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved
        );
        assert_eq!(ports.calls().await, ["catalog.profile_dir:legacy"]);
        assert!(ports.state.lock().await.directories.contains_key("legacy"));
    }

    #[tokio::test]
    async fn remove_stops_secondary_before_removing_record() {
        let ports = FakePorts::seeded().await;
        authority(&ports).remove("secondary").await.unwrap();

        assert_eq!(
            ports.calls().await,
            [
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
            ["catalog.profile_dir:secondary", "runtime.stop:secondary"]
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
                "router.set_active_transaction:secondary",
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
        authority.quiesce().await;

        assert_eq!(
            authority.set_active("secondary").await.unwrap_err(),
            WindowsSpaceAuthorityError::Quiescing
        );
        assert_eq!(
            authority.remove("secondary").await.unwrap_err(),
            WindowsSpaceAuthorityError::Quiescing
        );
        assert!(ports.calls().await.is_empty());
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
        (&mut quiescing).await.unwrap();
        assert_eq!(authority.active_profile().await, "secondary");
    }
}
