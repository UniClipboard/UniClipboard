//! Serialized authority for Windows multi-space mutations.
//!
//! This module deliberately owns only ordering and failure semantics. Concrete
//! catalog, runtime, and clipboard-router adapters are wired elsewhere.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

#[async_trait]
pub(crate) trait CatalogPort: Send + Sync {
    /// Persist an active-profile transition using compare-and-set semantics.
    async fn set_active(&self, expected: &str, target: &str) -> anyhow::Result<()>;

    async fn profile_dir(&self, profile_id: &str) -> anyhow::Result<Option<String>>;

    /// Remove only the catalog record. Implementations must not delete data.
    async fn remove(&self, profile_id: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait RuntimePort: Send + Sync {
    async fn ensure_available(&self, profile_id: &str) -> anyhow::Result<()>;

    async fn stop(&self, profile_id: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait RouterPort: Send + Sync {
    async fn set_active(&self, profile_id: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum WindowsSpaceAuthorityError {
    #[error("Windows space authority is quiescing")]
    Quiescing,
    #[error("the legacy profile cannot be removed")]
    LegacyProfileCannotBeRemoved,
    #[error("profile not found: {0}")]
    ProfileNotFound(String),
    #[error("runtime operation failed: {0}")]
    Runtime(String),
    #[error("catalog operation failed: {0}")]
    Catalog(String),
    #[error("router update failed: {router}; catalog rollback failed: {rollback:?}")]
    Router {
        router: String,
        rollback: Option<String>,
    },
}

struct AuthorityState {
    active_profile: String,
    quiescing: bool,
}

pub(crate) struct WindowsSpaceAuthority {
    state: Mutex<AuthorityState>,
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
                quiescing: false,
            }),
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
        self.state.lock().await.quiescing = true;
    }

    pub(crate) async fn set_active(&self, target: &str) -> Result<(), WindowsSpaceAuthorityError> {
        let mut state = self.state.lock().await;
        Self::ensure_accepting(&state)?;
        if state.active_profile == target {
            return Ok(());
        }

        let previous = state.active_profile.clone();
        self.runtime
            .ensure_available(target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Runtime(error.to_string()))?;
        self.catalog
            .set_active(&previous, target)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Catalog(error.to_string()))?;

        if let Err(router_error) = self.router.set_active(target).await {
            let rollback = self
                .catalog
                .set_active(target, &previous)
                .await
                .err()
                .map(|error| error.to_string());
            return Err(WindowsSpaceAuthorityError::Router {
                router: router_error.to_string(),
                rollback,
            });
        }

        state.active_profile = target.to_owned();
        Ok(())
    }

    pub(crate) async fn remove(&self, profile_id: &str) -> Result<(), WindowsSpaceAuthorityError> {
        let state = self.state.lock().await;
        Self::ensure_accepting(&state)?;

        let profile_dir = self
            .catalog
            .profile_dir(profile_id)
            .await
            .map_err(|error| WindowsSpaceAuthorityError::Catalog(error.to_string()))?
            .ok_or_else(|| WindowsSpaceAuthorityError::ProfileNotFound(profile_id.to_owned()))?;
        if profile_dir == "." {
            return Err(WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved);
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

    fn ensure_accepting(state: &AuthorityState) -> Result<(), WindowsSpaceAuthorityError> {
        if state.quiescing {
            Err(WindowsSpaceAuthorityError::Quiescing)
        } else {
            Ok(())
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
        fail_catalog_set: bool,
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
            drop(state);
            ports
        }

        async fn calls(&self) -> Vec<String> {
            self.state.lock().await.calls.clone()
        }
    }

    #[async_trait]
    impl CatalogPort for FakePorts {
        async fn set_active(&self, expected: &str, target: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state
                .calls
                .push(format!("catalog.set_active:{expected}->{target}"));
            if state.fail_catalog_set {
                anyhow::bail!("catalog set failed");
            }
            anyhow::ensure!(state.active == expected, "unexpected active profile");
            state.active = target.to_owned();
            Ok(())
        }

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
        async fn set_active(&self, profile_id: &str) -> anyhow::Result<()> {
            let mut state = self.state.lock().await;
            state.calls.push(format!("router.set_active:{profile_id}"));
            if state.fail_router {
                anyhow::bail!("router failed");
            }
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
    async fn set_active_orders_runtime_catalog_then_router() {
        let ports = FakePorts::seeded().await;
        authority(&ports).set_active("secondary").await.unwrap();

        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "catalog.set_active:legacy->secondary",
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
    async fn catalog_failure_keeps_old_active_and_skips_router() {
        let ports = FakePorts::seeded().await;
        ports.state.lock().await.fail_catalog_set = true;

        authority(&ports).set_active("secondary").await.unwrap_err();

        assert_eq!(ports.state.lock().await.active, "legacy");
        assert_eq!(
            ports.calls().await,
            [
                "runtime.ensure_available:secondary",
                "catalog.set_active:legacy->secondary",
            ]
        );
    }

    #[tokio::test]
    async fn router_failure_rolls_catalog_back_and_keeps_authority_active() {
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
                "catalog.set_active:legacy->secondary",
                "router.set_active:secondary",
                "catalog.set_active:secondary->legacy",
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
            tokio::spawn(async move { authority.remove("secondary").await })
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
                "catalog.set_active:legacy->secondary",
                "router.set_active:secondary",
                "catalog.profile_dir:secondary",
                "runtime.stop:secondary",
                "catalog.remove:secondary",
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
}
