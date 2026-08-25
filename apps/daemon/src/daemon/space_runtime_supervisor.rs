use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use uc_bootstrap::{prepare_desktop_engine_host_for_profile, DesktopRuntimeProfileConfig};
use uc_engine::{Engine, EventStream};

use super::space_catalog::{SpaceCatalog, SpaceCatalogEntry};

const ENGINE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(15);

pub type SpaceRuntimeFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type SpaceRuntimeFailureCallback =
    Arc<dyn Fn(SpaceRuntimeFailure) -> SpaceRuntimeFuture<bool> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRuntimeFailureCategory {
    Bootstrap,
    Runtime,
    Shutdown,
    ProfileConflict,
    Disabled,
    UnknownProfile,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{category:?}: {message}")]
pub struct SpaceRuntimeFailure {
    pub category: SpaceRuntimeFailureCategory,
    pub message: String,
}

impl SpaceRuntimeFailure {
    pub fn bootstrap(message: impl Into<String>) -> Self {
        Self {
            category: SpaceRuntimeFailureCategory::Bootstrap,
            message: message.into(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            category: SpaceRuntimeFailureCategory::Runtime,
            message: message.into(),
        }
    }

    fn shutdown(message: impl Into<String>) -> Self {
        Self {
            category: SpaceRuntimeFailureCategory::Shutdown,
            message: message.into(),
        }
    }

    fn for_category(category: SpaceRuntimeFailureCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRuntimeRoots {
    data_root: PathBuf,
    cache_root: PathBuf,
    log_root: PathBuf,
}

impl SpaceRuntimeRoots {
    pub fn new(data_root: PathBuf, cache_root: PathBuf, log_root: PathBuf) -> Self {
        Self {
            data_root,
            cache_root,
            log_root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRuntimeProfileSpec {
    pub profile_id: String,
    pub profile_dir: String,
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub log_dir: PathBuf,
    pub temporary_root: PathBuf,
    pub secure_storage_namespace: String,
}

#[async_trait]
pub trait SupervisedSpaceRuntime: Send + Sync {
    fn engine(&self) -> Option<Arc<Engine>> {
        None
    }

    async fn shutdown(&self) -> Result<(), SpaceRuntimeFailure>;
}

#[async_trait]
pub trait SpaceRuntimeFactory: Send + Sync {
    async fn create(
        &self,
        spec: SpaceRuntimeProfileSpec,
        generation: u64,
        report_failure: SpaceRuntimeFailureCallback,
    ) -> Result<Arc<dyn SupervisedSpaceRuntime>, SpaceRuntimeFailure>;
}

pub struct ProductionSpaceRuntimeFactory;

struct ProductionSpaceRuntime {
    engine: Arc<Engine>,
    _events: Mutex<Option<EventStream>>,
}

#[async_trait]
impl SupervisedSpaceRuntime for ProductionSpaceRuntime {
    fn engine(&self) -> Option<Arc<Engine>> {
        Some(Arc::clone(&self.engine))
    }

    async fn shutdown(&self) -> Result<(), SpaceRuntimeFailure> {
        self.engine
            .shutdown(ENGINE_SHUTDOWN_DEADLINE)
            .await
            .map_err(|error| SpaceRuntimeFailure::shutdown(error.to_string()))
    }
}

#[async_trait]
impl SpaceRuntimeFactory for ProductionSpaceRuntimeFactory {
    async fn create(
        &self,
        spec: SpaceRuntimeProfileSpec,
        _generation: u64,
        _report_failure: SpaceRuntimeFailureCallback,
    ) -> Result<Arc<dyn SupervisedSpaceRuntime>, SpaceRuntimeFailure> {
        let config = DesktopRuntimeProfileConfig::new(
            spec.profile_id,
            spec.data_root,
            spec.cache_root,
            spec.log_dir,
        )
        .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        let prepared = prepare_desktop_engine_host_for_profile(config)
            .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        let (engine_config, host_capabilities) = prepared.into_engine_start();
        let (engine, events) = Engine::start(engine_config, host_capabilities)
            .await
            .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        Ok(Arc::new(ProductionSpaceRuntime {
            engine: Arc::new(engine),
            _events: Mutex::new(Some(events)),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRuntimeLifecycle {
    Starting,
    Running,
    Stopping,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRuntimeStatus {
    pub profile_id: String,
    pub generation: u64,
    pub lifecycle: SpaceRuntimeLifecycle,
    pub last_failure: Option<SpaceRuntimeFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRuntimeStartDisposition {
    Started,
    Existing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRuntimeStart {
    pub disposition: SpaceRuntimeStartDisposition,
    pub status: SpaceRuntimeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("profile {profile_id} generation {generation} failed: {failure}")]
pub struct SpaceRuntimeStartError {
    pub profile_id: String,
    pub generation: u64,
    pub failure: SpaceRuntimeFailure,
}

struct SpaceRuntimeSlot {
    spec: SpaceRuntimeProfileSpec,
    generation: u64,
    lifecycle: SpaceRuntimeLifecycle,
    last_failure: Option<SpaceRuntimeFailure>,
    pending_start_generation: Option<u64>,
    lifecycle_notify: Arc<tokio::sync::Notify>,
    runtime: Option<Arc<dyn SupervisedSpaceRuntime>>,
}

impl SpaceRuntimeSlot {
    fn starting(spec: SpaceRuntimeProfileSpec, generation: u64) -> Self {
        Self {
            spec,
            generation,
            lifecycle: SpaceRuntimeLifecycle::Starting,
            last_failure: None,
            pending_start_generation: Some(generation),
            lifecycle_notify: Arc::new(tokio::sync::Notify::new()),
            runtime: None,
        }
    }

    fn status(&self) -> SpaceRuntimeStatus {
        SpaceRuntimeStatus {
            profile_id: self.spec.profile_id.clone(),
            generation: self.generation,
            lifecycle: self.lifecycle,
            last_failure: self.last_failure.clone(),
        }
    }

    fn advance_generation(&mut self) -> u64 {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("space runtime generation exhausted");
        self.generation
    }

    fn begin_start(&mut self) -> u64 {
        let generation = self.advance_generation();
        self.lifecycle = SpaceRuntimeLifecycle::Starting;
        self.last_failure = None;
        self.pending_start_generation = Some(generation);
        self.runtime = None;
        generation
    }
}

pub struct SpaceRuntimeSupervisor {
    factory: Arc<dyn SpaceRuntimeFactory>,
    roots: SpaceRuntimeRoots,
    slots: Mutex<HashMap<String, SpaceRuntimeSlot>>,
}

enum StopAction {
    Return(SpaceRuntimeStatus),
    Wait {
        generation: u64,
        notify: Arc<tokio::sync::Notify>,
    },
    Shutdown {
        generation: u64,
        pending_start_generation: Option<u64>,
        runtime: Option<Arc<dyn SupervisedSpaceRuntime>>,
        notify: Arc<tokio::sync::Notify>,
    },
}

impl SpaceRuntimeSupervisor {
    pub fn new(factory: Arc<dyn SpaceRuntimeFactory>, roots: SpaceRuntimeRoots) -> Arc<Self> {
        Arc::new(Self {
            factory,
            roots,
            slots: Mutex::new(HashMap::new()),
        })
    }

    pub fn production(roots: SpaceRuntimeRoots) -> Arc<Self> {
        Self::new(Arc::new(ProductionSpaceRuntimeFactory), roots)
    }

    pub async fn start_enabled(
        self: &Arc<Self>,
        catalog: &SpaceCatalog,
    ) -> Vec<Result<SpaceRuntimeStart, SpaceRuntimeStartError>> {
        let mut starts = tokio::task::JoinSet::new();
        for entry in catalog
            .entries()
            .iter()
            .filter(|entry| entry.enabled)
            .cloned()
        {
            let supervisor = Arc::clone(self);
            starts.spawn(async move { supervisor.start_entry(entry).await });
        }

        let mut results = Vec::new();
        while let Some(result) = starts.join_next().await {
            match result {
                Ok(result) => results.push(result),
                Err(error) => results.push(Err(SpaceRuntimeStartError {
                    profile_id: "<task>".to_string(),
                    generation: 0,
                    failure: SpaceRuntimeFailure::runtime(error.to_string()),
                })),
            }
        }
        results.sort_by(|left, right| result_profile_id(left).cmp(result_profile_id(right)));
        results
    }

    pub async fn start_profile(
        self: &Arc<Self>,
        catalog: &SpaceCatalog,
        profile_id: &str,
    ) -> Result<SpaceRuntimeStart, SpaceRuntimeStartError> {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.profile_id == profile_id)
            .cloned()
            .ok_or_else(|| SpaceRuntimeStartError {
                profile_id: profile_id.to_string(),
                generation: 0,
                failure: SpaceRuntimeFailure::for_category(
                    SpaceRuntimeFailureCategory::UnknownProfile,
                    "profile is not present in the catalog",
                ),
            })?;
        self.start_entry(entry).await
    }

    async fn start_entry(
        self: &Arc<Self>,
        entry: SpaceCatalogEntry,
    ) -> Result<SpaceRuntimeStart, SpaceRuntimeStartError> {
        if !entry.enabled {
            return Err(SpaceRuntimeStartError {
                profile_id: entry.profile_id,
                generation: 0,
                failure: SpaceRuntimeFailure::for_category(
                    SpaceRuntimeFailureCategory::Disabled,
                    "profile is disabled",
                ),
            });
        }
        let spec = self
            .profile_spec(&entry)
            .map_err(|failure| SpaceRuntimeStartError {
                profile_id: entry.profile_id.clone(),
                generation: 0,
                failure,
            })?;
        let profile_id = spec.profile_id.clone();
        let generation = {
            let mut slots = self.lock_slots();
            match slots.get_mut(&profile_id) {
                Some(slot) if slot.spec != spec => {
                    return Err(SpaceRuntimeStartError {
                        profile_id,
                        generation: slot.generation,
                        failure: SpaceRuntimeFailure::for_category(
                            SpaceRuntimeFailureCategory::ProfileConflict,
                            "profile runtime paths changed while registered",
                        ),
                    });
                }
                Some(slot)
                    if matches!(
                        slot.lifecycle,
                        SpaceRuntimeLifecycle::Starting
                            | SpaceRuntimeLifecycle::Running
                            | SpaceRuntimeLifecycle::Stopping
                    ) =>
                {
                    return Ok(SpaceRuntimeStart {
                        disposition: SpaceRuntimeStartDisposition::Existing,
                        status: slot.status(),
                    });
                }
                Some(slot) => slot.begin_start(),
                None => {
                    let generation = 1;
                    slots.insert(
                        profile_id.clone(),
                        SpaceRuntimeSlot::starting(spec.clone(), generation),
                    );
                    generation
                }
            }
        };

        let weak_supervisor = Arc::downgrade(self);
        let callback_profile_id = profile_id.clone();
        let report_failure: SpaceRuntimeFailureCallback = Arc::new(move |failure| {
            let weak_supervisor = weak_supervisor.clone();
            let profile_id = callback_profile_id.clone();
            Box::pin(async move {
                match weak_supervisor.upgrade() {
                    Some(supervisor) => {
                        supervisor
                            .report_failure(&profile_id, generation, failure)
                            .await
                    }
                    None => false,
                }
            })
        });

        match self.factory.create(spec, generation, report_failure).await {
            Ok(runtime) => {
                let committed = {
                    let mut slots = self.lock_slots();
                    let slot = slots
                        .get_mut(&profile_id)
                        .expect("starting slot must remain registered");
                    if slot.generation == generation
                        && slot.lifecycle == SpaceRuntimeLifecycle::Starting
                        && slot.pending_start_generation == Some(generation)
                    {
                        slot.runtime = Some(Arc::clone(&runtime));
                        slot.pending_start_generation = None;
                        slot.lifecycle = SpaceRuntimeLifecycle::Running;
                        slot.last_failure = None;
                        Some((slot.status(), Arc::clone(&slot.lifecycle_notify)))
                    } else {
                        None
                    }
                };
                if let Some((status, notify)) = committed {
                    notify.notify_waiters();
                    Ok(SpaceRuntimeStart {
                        disposition: SpaceRuntimeStartDisposition::Started,
                        status,
                    })
                } else {
                    let _ = runtime.shutdown().await;
                    self.finish_pending_start(&profile_id, generation);
                    Err(SpaceRuntimeStartError {
                        profile_id,
                        generation,
                        failure: SpaceRuntimeFailure::for_category(
                            SpaceRuntimeFailureCategory::Superseded,
                            "start was superseded by a newer lifecycle operation",
                        ),
                    })
                }
            }
            Err(failure) => {
                let notify = {
                    let mut slots = self.lock_slots();
                    let slot = slots
                        .get_mut(&profile_id)
                        .expect("starting slot must remain registered");
                    if slot.generation == generation
                        && slot.lifecycle == SpaceRuntimeLifecycle::Starting
                        && slot.pending_start_generation == Some(generation)
                    {
                        slot.pending_start_generation = None;
                        slot.lifecycle = SpaceRuntimeLifecycle::Failed;
                        slot.last_failure = Some(failure.clone());
                        Some(Arc::clone(&slot.lifecycle_notify))
                    } else {
                        None
                    }
                };
                if let Some(notify) = notify {
                    notify.notify_waiters();
                } else {
                    self.finish_pending_start(&profile_id, generation);
                }
                Err(SpaceRuntimeStartError {
                    profile_id,
                    generation,
                    failure,
                })
            }
        }
    }

    pub async fn stop_profile(&self, profile_id: &str) -> Option<SpaceRuntimeStatus> {
        loop {
            let action = {
                let mut slots = self.lock_slots();
                let slot = slots.get_mut(profile_id)?;
                match slot.lifecycle {
                    SpaceRuntimeLifecycle::Stopped => StopAction::Return(slot.status()),
                    SpaceRuntimeLifecycle::Stopping => StopAction::Wait {
                        generation: slot.generation,
                        notify: Arc::clone(&slot.lifecycle_notify),
                    },
                    SpaceRuntimeLifecycle::Failed => {
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopped;
                        slot.last_failure = None;
                        StopAction::Return(slot.status())
                    }
                    SpaceRuntimeLifecycle::Starting | SpaceRuntimeLifecycle::Running => {
                        let pending_start_generation = slot.pending_start_generation;
                        let generation = slot.advance_generation();
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopping;
                        slot.last_failure = None;
                        StopAction::Shutdown {
                            generation,
                            pending_start_generation,
                            runtime: slot.runtime.take(),
                            notify: Arc::clone(&slot.lifecycle_notify),
                        }
                    }
                }
            };

            match action {
                StopAction::Return(status) => return Some(status),
                StopAction::Wait { generation, notify } => {
                    self.wait_until_not_stopping(profile_id, generation, &notify)
                        .await;
                }
                StopAction::Shutdown {
                    generation,
                    pending_start_generation,
                    runtime,
                    notify,
                } => {
                    let failure = match runtime {
                        Some(runtime) => runtime.shutdown().await.err(),
                        None => None,
                    };
                    if let Some(pending) = pending_start_generation {
                        self.wait_for_pending_start(profile_id, generation, pending, &notify)
                            .await;
                    }
                    return self.complete_stopping(
                        profile_id,
                        generation,
                        if failure.is_some() {
                            SpaceRuntimeLifecycle::Failed
                        } else {
                            SpaceRuntimeLifecycle::Stopped
                        },
                        failure,
                    );
                }
            }
        }
    }

    pub async fn shutdown_all(&self) -> Vec<SpaceRuntimeStatus> {
        let profile_ids: Vec<_> = self
            .list()
            .into_iter()
            .map(|status| status.profile_id)
            .collect();
        let mut stopped = Vec::with_capacity(profile_ids.len());
        for profile_id in profile_ids {
            if let Some(status) = self.stop_profile(&profile_id).await {
                stopped.push(status);
            }
        }
        stopped.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        stopped
    }

    pub async fn report_failure(
        &self,
        profile_id: &str,
        generation: u64,
        failure: SpaceRuntimeFailure,
    ) -> bool {
        let (failure_generation, runtime) = {
            let mut slots = self.lock_slots();
            let Some(slot) = slots.get_mut(profile_id) else {
                return false;
            };
            if slot.generation != generation || slot.lifecycle != SpaceRuntimeLifecycle::Running {
                return false;
            }
            let failure_generation = slot.advance_generation();
            slot.lifecycle = SpaceRuntimeLifecycle::Stopping;
            slot.last_failure = None;
            (failure_generation, slot.runtime.take())
        };

        if let Some(runtime) = runtime {
            let _ = runtime.shutdown().await;
        }
        self.complete_stopping(
            profile_id,
            failure_generation,
            SpaceRuntimeLifecycle::Failed,
            Some(failure),
        );
        true
    }

    pub fn runtime(&self, profile_id: &str) -> Option<Arc<dyn SupervisedSpaceRuntime>> {
        let slots = self.lock_slots();
        let slot = slots.get(profile_id)?;
        (slot.lifecycle == SpaceRuntimeLifecycle::Running)
            .then(|| slot.runtime.as_ref().map(Arc::clone))
            .flatten()
    }

    pub fn engine(&self, profile_id: &str) -> Option<Arc<Engine>> {
        self.runtime(profile_id)?.engine()
    }

    pub fn status(&self, profile_id: &str) -> Option<SpaceRuntimeStatus> {
        self.lock_slots()
            .get(profile_id)
            .map(SpaceRuntimeSlot::status)
    }

    pub fn list(&self) -> Vec<SpaceRuntimeStatus> {
        let mut statuses: Vec<_> = self
            .lock_slots()
            .values()
            .map(SpaceRuntimeSlot::status)
            .collect();
        statuses.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
        statuses
    }

    fn profile_spec(
        &self,
        entry: &SpaceCatalogEntry,
    ) -> Result<SpaceRuntimeProfileSpec, SpaceRuntimeFailure> {
        let profile_component = format!("profile-{}", entry.profile_id);
        let data_root = match entry.profile_dir.as_str() {
            "." => self.roots.data_root.clone(),
            profile_dir if profile_dir == profile_component => {
                self.roots.data_root.join(profile_dir)
            }
            _ => {
                return Err(SpaceRuntimeFailure::for_category(
                    SpaceRuntimeFailureCategory::ProfileConflict,
                    "catalog profile directory is not canonical",
                ));
            }
        };
        let cache_root = self.roots.cache_root.join(&profile_component);
        let log_dir = self.roots.log_root.join(&profile_component);
        Ok(SpaceRuntimeProfileSpec {
            profile_id: entry.profile_id.clone(),
            profile_dir: entry.profile_dir.clone(),
            data_root,
            temporary_root: cache_root.join("engine-tmp"),
            cache_root,
            log_dir,
            secure_storage_namespace: entry.profile_id.clone(),
        })
    }

    fn finish_pending_start(&self, profile_id: &str, generation: u64) {
        let notify = {
            let mut slots = self.lock_slots();
            let Some(slot) = slots.get_mut(profile_id) else {
                return;
            };
            if slot.pending_start_generation == Some(generation) {
                slot.pending_start_generation = None;
                Some(Arc::clone(&slot.lifecycle_notify))
            } else {
                None
            }
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    async fn wait_for_pending_start(
        &self,
        profile_id: &str,
        stopping_generation: u64,
        pending_start_generation: u64,
        notify: &Arc<tokio::sync::Notify>,
    ) {
        loop {
            let notified = notify.notified();
            let still_pending = {
                let slots = self.lock_slots();
                slots.get(profile_id).is_some_and(|slot| {
                    slot.generation == stopping_generation
                        && slot.lifecycle == SpaceRuntimeLifecycle::Stopping
                        && slot.pending_start_generation == Some(pending_start_generation)
                })
            };
            if !still_pending {
                return;
            }
            notified.await;
        }
    }

    async fn wait_until_not_stopping(
        &self,
        profile_id: &str,
        generation: u64,
        notify: &Arc<tokio::sync::Notify>,
    ) {
        loop {
            let notified = notify.notified();
            let is_stopping = {
                let slots = self.lock_slots();
                slots.get(profile_id).is_some_and(|slot| {
                    slot.generation == generation
                        && slot.lifecycle == SpaceRuntimeLifecycle::Stopping
                })
            };
            if !is_stopping {
                return;
            }
            notified.await;
        }
    }

    fn complete_stopping(
        &self,
        profile_id: &str,
        generation: u64,
        lifecycle: SpaceRuntimeLifecycle,
        last_failure: Option<SpaceRuntimeFailure>,
    ) -> Option<SpaceRuntimeStatus> {
        let completed = {
            let mut slots = self.lock_slots();
            let slot = slots.get_mut(profile_id)?;
            if slot.generation == generation && slot.lifecycle == SpaceRuntimeLifecycle::Stopping {
                slot.lifecycle = lifecycle;
                slot.last_failure = last_failure;
                Some((slot.status(), Arc::clone(&slot.lifecycle_notify)))
            } else {
                None
            }
        };
        if let Some((status, notify)) = completed {
            notify.notify_waiters();
            Some(status)
        } else {
            self.status(profile_id)
        }
    }

    fn lock_slots(&self) -> MutexGuard<'_, HashMap<String, SpaceRuntimeSlot>> {
        match self.slots.lock() {
            Ok(slots) => slots,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

fn result_profile_id(result: &Result<SpaceRuntimeStart, SpaceRuntimeStartError>) -> &str {
    match result {
        Ok(start) => &start.status.profile_id,
        Err(error) => &error.profile_id,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use tokio::sync::Barrier;

    use super::{
        SpaceRuntimeFactory, SpaceRuntimeFailure, SpaceRuntimeFailureCallback,
        SpaceRuntimeLifecycle, SpaceRuntimeProfileSpec, SpaceRuntimeRoots,
        SpaceRuntimeStartDisposition, SpaceRuntimeSupervisor, SupervisedSpaceRuntime,
    };
    use crate::daemon::space_catalog::SpaceCatalog;

    #[derive(Default)]
    struct FakeRuntime {
        shutdowns: Mutex<usize>,
    }

    #[async_trait]
    impl SupervisedSpaceRuntime for FakeRuntime {
        async fn shutdown(&self) -> Result<(), SpaceRuntimeFailure> {
            *self.shutdowns.lock().expect("lock shutdown count") += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingFactory {
        starts: Mutex<HashMap<String, usize>>,
        specs: Mutex<Vec<SpaceRuntimeProfileSpec>>,
        callbacks: Mutex<HashMap<String, Vec<SpaceRuntimeFailureCallback>>>,
        failures: Mutex<HashSet<String>>,
        runtimes: Mutex<HashMap<String, Vec<Arc<FakeRuntime>>>>,
        start_barrier: Mutex<Option<Arc<Barrier>>>,
    }

    impl RecordingFactory {
        fn fail(&self, profile_id: &str) {
            self.failures
                .lock()
                .expect("lock failures")
                .insert(profile_id.to_string());
        }

        fn with_start_barrier(self: &Arc<Self>, parties: usize) {
            *self.start_barrier.lock().expect("lock start barrier") =
                Some(Arc::new(Barrier::new(parties)));
        }

        fn start_count(&self, profile_id: &str) -> usize {
            self.starts
                .lock()
                .expect("lock starts")
                .get(profile_id)
                .copied()
                .unwrap_or_default()
        }

        fn callback(&self, profile_id: &str, index: usize) -> SpaceRuntimeFailureCallback {
            self.callbacks.lock().expect("lock callbacks")[profile_id][index].clone()
        }

        fn runtime(&self, profile_id: &str, index: usize) -> Arc<FakeRuntime> {
            self.runtimes.lock().expect("lock runtimes")[profile_id][index].clone()
        }
    }

    #[async_trait]
    impl SpaceRuntimeFactory for RecordingFactory {
        async fn create(
            &self,
            spec: SpaceRuntimeProfileSpec,
            _generation: u64,
            report_failure: SpaceRuntimeFailureCallback,
        ) -> Result<Arc<dyn SupervisedSpaceRuntime>, SpaceRuntimeFailure> {
            *self
                .starts
                .lock()
                .expect("lock starts")
                .entry(spec.profile_id.clone())
                .or_default() += 1;
            self.specs.lock().expect("lock specs").push(spec.clone());
            self.callbacks
                .lock()
                .expect("lock callbacks")
                .entry(spec.profile_id.clone())
                .or_default()
                .push(report_failure);
            let barrier = self
                .start_barrier
                .lock()
                .expect("lock start barrier")
                .clone();
            if let Some(barrier) = barrier {
                barrier.wait().await;
            }
            if self
                .failures
                .lock()
                .expect("lock failures")
                .contains(&spec.profile_id)
            {
                return Err(SpaceRuntimeFailure::bootstrap("injected start failure"));
            }
            let runtime = Arc::new(FakeRuntime::default());
            self.runtimes
                .lock()
                .expect("lock runtimes")
                .entry(spec.profile_id)
                .or_default()
                .push(runtime.clone());
            Ok(runtime)
        }
    }

    fn test_catalog() -> (tempfile::TempDir, SpaceCatalog) {
        let root = tempfile::tempdir().expect("create data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("create catalog");
        catalog.add_profile().expect("add second profile");
        (root, catalog)
    }

    fn test_roots(root: &tempfile::TempDir) -> SpaceRuntimeRoots {
        SpaceRuntimeRoots::new(
            root.path().to_path_buf(),
            root.path().join("cache"),
            root.path().join("logs"),
        )
    }

    #[tokio::test]
    async fn enabled_profiles_start_concurrently_with_isolated_paths() {
        let (root, catalog) = test_catalog();
        let factory = Arc::new(RecordingFactory::default());
        factory.with_start_barrier(2);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));

        let starts = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            supervisor.start_enabled(&catalog),
        )
        .await
        .expect("both factories must reach the double barrier");

        assert_eq!(starts.len(), 2);
        assert!(starts.iter().all(Result::is_ok));
        let specs = factory.specs.lock().expect("lock specs");
        let legacy = specs
            .iter()
            .find(|spec| spec.profile_dir == ".")
            .expect("legacy profile spec");
        let added = specs
            .iter()
            .find(|spec| spec.profile_dir != ".")
            .expect("added profile spec");
        assert_eq!(legacy.data_root, root.path());
        assert_eq!(
            added.data_root,
            root.path().join(format!("profile-{}", added.profile_id))
        );
        assert_ne!(legacy.cache_root, added.cache_root);
        assert_ne!(legacy.log_dir, added.log_dir);
        assert_ne!(legacy.temporary_root, added.temporary_root);
        assert_eq!(legacy.secure_storage_namespace, legacy.profile_id);
        assert_eq!(added.secure_storage_namespace, added.profile_id);
    }

    #[tokio::test]
    async fn one_profile_failure_does_not_block_another_profile() {
        let (root, catalog) = test_catalog();
        let failed_id = catalog.entries()[0].profile_id.clone();
        let running_id = catalog.entries()[1].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.fail(&failed_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory, test_roots(&root));

        let results = supervisor.start_enabled(&catalog).await;

        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(
            supervisor
                .status(&failed_id)
                .expect("failed status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
        assert_eq!(
            supervisor
                .status(&running_id)
                .expect("running status")
                .lifecycle,
            SpaceRuntimeLifecycle::Running
        );
    }

    #[tokio::test]
    async fn concurrent_duplicate_start_creates_only_one_runtime() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));

        let (left, right) = tokio::join!(
            supervisor.start_profile(&catalog, &profile_id),
            supervisor.start_profile(&catalog, &profile_id),
        );

        assert!(left.is_ok());
        assert!(right.is_ok());
        assert_eq!(factory.start_count(&profile_id), 1);
        assert!(matches!(
            (left.unwrap().disposition, right.unwrap().disposition),
            (
                SpaceRuntimeStartDisposition::Started,
                SpaceRuntimeStartDisposition::Existing
            ) | (
                SpaceRuntimeStartDisposition::Existing,
                SpaceRuntimeStartDisposition::Started
            )
        ));
    }

    #[tokio::test]
    async fn stop_restart_rejects_old_generation_failure_callback() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let first = supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("start first generation");
        let stale_callback = factory.callback(&profile_id, 0);

        supervisor
            .stop_profile(&profile_id)
            .await
            .expect("stop profile");
        let second = supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("restart profile");
        assert!(second.status.generation > first.status.generation);
        assert!(!(stale_callback)(SpaceRuntimeFailure::runtime("late failure")).await);
        assert_eq!(
            supervisor.status(&profile_id).expect("replacement status"),
            second.status
        );
        assert!(supervisor.engine(&profile_id).is_none());
    }

    #[tokio::test]
    async fn shutdown_all_stops_every_running_runtime() {
        let (root, catalog) = test_catalog();
        let ids: Vec<_> = catalog
            .entries()
            .iter()
            .map(|entry| entry.profile_id.clone())
            .collect();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        supervisor.start_enabled(&catalog).await;
        let runtimes: Vec<_> = ids
            .iter()
            .map(|profile_id| factory.runtime(profile_id, 0))
            .collect();

        supervisor.shutdown_all().await;

        assert!(supervisor
            .list()
            .iter()
            .all(|status| status.lifecycle == SpaceRuntimeLifecycle::Stopped));
        assert!(runtimes
            .iter()
            .all(|runtime| { *runtime.shutdowns.lock().expect("lock shutdown count") == 1 }));
    }

    #[tokio::test]
    async fn restoring_active_send_only_looks_up_the_existing_runtime() {
        let (root, mut catalog) = test_catalog();
        let active_id = catalog.entries()[1].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        supervisor.start_enabled(&catalog).await;
        let starts_before: usize = catalog
            .entries()
            .iter()
            .map(|entry| factory.start_count(&entry.profile_id))
            .sum();

        catalog
            .set_active_send(&active_id)
            .expect("restore active-send target");
        let runtime = supervisor.runtime(&active_id).expect("active runtime");

        assert!(Arc::strong_count(&runtime) >= 2);
        let starts_after: usize = catalog
            .entries()
            .iter()
            .map(|entry| factory.start_count(&entry.profile_id))
            .sum();
        assert_eq!(starts_after, starts_before);
    }
}
