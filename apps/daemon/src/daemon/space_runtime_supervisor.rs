use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uc_bootstrap::{
    prepare_desktop_engine_host_for_profile_with_hub, DesktopClipboardHub,
    DesktopClipboardProfileHandle, DesktopClipboardStageExecution, DesktopRuntimeProfileConfig,
};
use uc_engine::{Engine, EngineEvent, EventStream, ObserveClipboardChangeInput, Operation};
use uc_platform::clipboard::SystemClipboardSnapshot;

use super::space_catalog::{SpaceCatalog, SpaceCatalogEntry};

const ENGINE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(15);

pub type SpaceRuntimeFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type SpaceRuntimeFailureCallback =
    Arc<dyn Fn(SpaceRuntimeFailure) -> SpaceRuntimeFuture<bool> + Send + Sync>;
pub type SpaceRuntimeEventCallback = Arc<dyn Fn(EngineEvent) -> bool + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfiledEngineEvent {
    pub profile_id: String,
    pub generation: u64,
    pub event: EngineEvent,
}

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

    pub fn shutdown(message: impl Into<String>) -> Self {
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

    async fn dispatch_snapshot(
        &self,
        _snapshot: SystemClipboardSnapshot,
        _cancel: CancellationToken,
    ) -> Result<(), SpaceRuntimeFailure> {
        Err(SpaceRuntimeFailure::runtime(
            "runtime does not support clipboard snapshot dispatch",
        ))
    }

    async fn shutdown(&self, deadline: Duration) -> Result<(), SpaceRuntimeFailure>;
}

#[async_trait]
pub trait SpaceRuntimeFactory: Send + Sync {
    async fn create(
        &self,
        spec: SpaceRuntimeProfileSpec,
        generation: u64,
        report_failure: SpaceRuntimeFailureCallback,
        forward_event: SpaceRuntimeEventCallback,
    ) -> Result<Arc<dyn SupervisedSpaceRuntime>, SpaceRuntimeFailure>;
}

pub struct ProductionSpaceRuntimeFactory {
    clipboard_hub: DesktopClipboardHub,
}

impl ProductionSpaceRuntimeFactory {
    pub fn new(clipboard_hub: DesktopClipboardHub) -> Self {
        Self { clipboard_hub }
    }
}

fn validate_production_profile_spec(
    spec: &SpaceRuntimeProfileSpec,
) -> Result<(), SpaceRuntimeFailure> {
    if spec.profile_dir == "." {
        return Err(SpaceRuntimeFailure::for_category(
            SpaceRuntimeFailureCategory::ProfileConflict,
            "legacy default profile must remain on the compatibility desktop host",
        ));
    }
    Ok(())
}

struct ProductionSpaceRuntime {
    engine: Arc<Engine>,
    clipboard_hub: DesktopClipboardHub,
    clipboard_profile: DesktopClipboardProfileHandle,
    monitor_cancel: CancellationToken,
    monitor: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    shutdown: Arc<StickyShutdown>,
}

struct StickyShutdown {
    started: AtomicBool,
    result: tokio::sync::watch::Sender<Option<Result<(), SpaceRuntimeFailure>>>,
}

impl Default for StickyShutdown {
    fn default() -> Self {
        let (result, _) = tokio::sync::watch::channel(None);
        Self {
            started: AtomicBool::new(false),
            result,
        }
    }
}

impl StickyShutdown {
    // Engine marks itself Stopped after a failed runtime shutdown, so the first outcome is
    // terminal evidence. Retrying Engine::shutdown would only produce an invalid-state error.
    async fn run<F, Fut>(self: &Arc<Self>, work: F) -> Result<(), SpaceRuntimeFailure>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), SpaceRuntimeFailure>> + Send + 'static,
    {
        let mut result = self.result.subscribe();
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let completed = self.result.clone();
            tokio::spawn(async move {
                let worker = tokio::spawn(work());
                let outcome = match worker.await {
                    Ok(outcome) => outcome,
                    Err(error) => Err(SpaceRuntimeFailure::shutdown(format!(
                        "production shutdown task failed: {error}"
                    ))),
                };
                completed.send_replace(Some(outcome));
            });
        }

        loop {
            if let Some(outcome) = result.borrow().clone() {
                return outcome;
            }
            result.changed().await.map_err(|_| {
                SpaceRuntimeFailure::shutdown("production shutdown result channel closed")
            })?;
        }
    }
}

#[async_trait]
impl SupervisedSpaceRuntime for ProductionSpaceRuntime {
    fn engine(&self) -> Option<Arc<Engine>> {
        Some(Arc::clone(&self.engine))
    }

    async fn dispatch_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        cancel: CancellationToken,
    ) -> Result<(), SpaceRuntimeFailure> {
        let engine = Arc::clone(&self.engine);
        let outcome = self
            .clipboard_hub
            .execute_with_staged_snapshot(&self.clipboard_profile, snapshot, move || async move {
                tokio::select! {
                    _ = cancel.cancelled() => anyhow::bail!("clipboard dispatch was cancelled"),
                    result = engine.execute(Operation::ObserveClipboardChange(
                        ObserveClipboardChangeInput { dispatch: true },
                    )) => result.map(|_| ()).map_err(anyhow::Error::new),
                }
            })
            .await
            .map_err(|error| SpaceRuntimeFailure::runtime(error.to_string()))?;
        match outcome {
            DesktopClipboardStageExecution::ConsumedAndCompleted(()) => Ok(()),
            DesktopClipboardStageExecution::CompletedWithoutConsumption(()) => Err(
                SpaceRuntimeFailure::runtime("Engine completed without consuming staged clipboard"),
            ),
            DesktopClipboardStageExecution::FailedBeforeConsumption(error) => {
                Err(SpaceRuntimeFailure::runtime(format!(
                    "clipboard dispatch failed before capture: {error}"
                )))
            }
            DesktopClipboardStageExecution::FailedAfterConsumption(error) => {
                Err(SpaceRuntimeFailure::runtime(format!(
                    "clipboard dispatch failed after capture: {error}"
                )))
            }
        }
    }

    async fn shutdown(&self, deadline: Duration) -> Result<(), SpaceRuntimeFailure> {
        let engine = Arc::clone(&self.engine);
        let monitor_cancel = self.monitor_cancel.clone();
        let monitor = Arc::clone(&self.monitor);
        self.shutdown
            .run(move || async move {
                shutdown_production_runtime(engine, monitor_cancel, monitor, deadline).await
            })
            .await
    }
}

async fn shutdown_production_runtime(
    engine: Arc<Engine>,
    monitor_cancel: CancellationToken,
    monitor: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    deadline: Duration,
) -> Result<(), SpaceRuntimeFailure> {
    let deadline_at = tokio::time::Instant::now() + deadline;
    monitor_cancel.cancel();
    let monitor_task = match monitor.lock() {
        Ok(mut monitor) => monitor.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    let monitor_failure = match monitor_task {
        Some(mut monitor_task) => {
            match tokio::time::timeout_at(deadline_at, &mut monitor_task).await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(SpaceRuntimeFailure::shutdown(format!(
                    "engine event monitor failed: {error}"
                ))),
                Err(_) => {
                    match monitor.lock() {
                        Ok(mut registered) => *registered = Some(monitor_task),
                        Err(poisoned) => *poisoned.into_inner() = Some(monitor_task),
                    }
                    Some(SpaceRuntimeFailure::shutdown(
                        "engine event monitor shutdown deadline exceeded",
                    ))
                }
            }
        }
        None => None,
    };
    let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
    let engine_result = engine
        .shutdown(remaining)
        .await
        .map_err(|error| SpaceRuntimeFailure::shutdown(error.to_string()));
    engine_result.and_then(|()| match monitor_failure {
        Some(failure) => Err(failure),
        None => Ok(()),
    })
}

#[async_trait]
impl SpaceRuntimeFactory for ProductionSpaceRuntimeFactory {
    async fn create(
        &self,
        spec: SpaceRuntimeProfileSpec,
        _generation: u64,
        report_failure: SpaceRuntimeFailureCallback,
        forward_event: SpaceRuntimeEventCallback,
    ) -> Result<Arc<dyn SupervisedSpaceRuntime>, SpaceRuntimeFailure> {
        validate_production_profile_spec(&spec)?;
        let clipboard_profile = self.clipboard_hub.profile_handle();
        let config = DesktopRuntimeProfileConfig::new(
            spec.profile_id,
            spec.data_root,
            spec.cache_root,
            spec.log_dir,
        )
        .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        let prepared =
            prepare_desktop_engine_host_for_profile_with_hub(config, clipboard_profile.clone())
                .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        let (engine_config, host_capabilities) = prepared.into_engine_start();
        let (engine, events) = Engine::start(engine_config, host_capabilities)
            .await
            .map_err(|error| SpaceRuntimeFailure::bootstrap(error.to_string()))?;
        let monitor_cancel = CancellationToken::new();
        let monitor = spawn_engine_event_monitor(
            Box::new(events),
            monitor_cancel.clone(),
            forward_event,
            report_failure,
        );
        Ok(Arc::new(ProductionSpaceRuntime {
            engine: Arc::new(engine),
            clipboard_hub: self.clipboard_hub.clone(),
            clipboard_profile,
            monitor_cancel,
            monitor: Arc::new(Mutex::new(Some(monitor))),
            shutdown: Arc::new(StickyShutdown::default()),
        }))
    }
}

#[async_trait]
trait SpaceEngineEventStream: Send {
    async fn next(&mut self) -> Option<EngineEvent>;
}

#[async_trait]
impl SpaceEngineEventStream for EventStream {
    async fn next(&mut self) -> Option<EngineEvent> {
        EventStream::next(self).await
    }
}

fn spawn_engine_event_monitor(
    mut events: Box<dyn SpaceEngineEventStream>,
    cancel: CancellationToken,
    forward_event: SpaceRuntimeEventCallback,
    report_failure: SpaceRuntimeFailureCallback,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                event = events.next() => match event {
                    Some(EngineEvent::Fatal { error }) => {
                        if !cancel.is_cancelled() {
                            let report_failure = Arc::clone(&report_failure);
                            tokio::spawn(async move {
                                let _ = report_failure(SpaceRuntimeFailure::runtime(error.to_string())).await;
                            });
                        }
                        break;
                    }
                    Some(event) => {
                        let _ = forward_event(event);
                    }
                    None => {
                        if !cancel.is_cancelled() {
                            let report_failure = Arc::clone(&report_failure);
                            tokio::spawn(async move {
                                let _ = report_failure(SpaceRuntimeFailure::runtime(
                                    "engine event stream exited unexpectedly",
                                )).await;
                            });
                        }
                        break;
                    }
                }
            }
        }
    })
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
    #[cfg(test)]
    start_waiter_notify: Arc<tokio::sync::Notify>,
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
            #[cfg(test)]
            start_waiter_notify: Arc::new(tokio::sync::Notify::new()),
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
    event_tx: tokio::sync::broadcast::Sender<ProfiledEngineEvent>,
}

enum StartAction {
    Start(u64),
    Return(SpaceRuntimeStatus),
    Wait {
        generation: u64,
        notify: Arc<tokio::sync::Notify>,
    },
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
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        Arc::new(Self {
            factory,
            roots,
            slots: Mutex::new(HashMap::new()),
            event_tx,
        })
    }

    pub fn production(roots: SpaceRuntimeRoots, clipboard_hub: DesktopClipboardHub) -> Arc<Self> {
        Self::new(
            Arc::new(ProductionSpaceRuntimeFactory::new(clipboard_hub)),
            roots,
        )
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

    #[cfg(test)]
    async fn start_entry_for_test(
        self: &Arc<Self>,
        entries: Vec<SpaceCatalogEntry>,
        profile_id: &str,
    ) -> Result<SpaceRuntimeStart, SpaceRuntimeStartError> {
        let entry = entries
            .into_iter()
            .find(|entry| entry.profile_id == profile_id)
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
        let action = {
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
                Some(slot) if slot.lifecycle == SpaceRuntimeLifecycle::Running => {
                    StartAction::Return(slot.status())
                }
                Some(slot)
                    if slot.pending_start_generation.is_some()
                        && !matches!(
                            slot.lifecycle,
                            SpaceRuntimeLifecycle::Starting | SpaceRuntimeLifecycle::Stopping
                        ) =>
                {
                    return Err(SpaceRuntimeStartError {
                        profile_id,
                        generation: slot.generation,
                        failure: slot.last_failure.clone().unwrap_or_else(|| {
                            SpaceRuntimeFailure::shutdown(
                                "previous runtime start is still pending cleanup",
                            )
                        }),
                    });
                }
                Some(slot)
                    if slot.lifecycle == SpaceRuntimeLifecycle::Failed
                        && slot.runtime.is_some() =>
                {
                    return Err(SpaceRuntimeStartError {
                        profile_id,
                        generation: slot.generation,
                        failure: slot.last_failure.clone().unwrap_or_else(|| {
                            SpaceRuntimeFailure::shutdown(
                                "failed runtime must be stopped before restart",
                            )
                        }),
                    });
                }
                Some(slot)
                    if matches!(
                        slot.lifecycle,
                        SpaceRuntimeLifecycle::Starting | SpaceRuntimeLifecycle::Stopping
                    ) =>
                {
                    StartAction::Wait {
                        generation: slot.generation,
                        notify: Arc::clone(&slot.lifecycle_notify),
                    }
                }
                Some(slot) => StartAction::Start(slot.begin_start()),
                None => {
                    let generation = 1;
                    slots.insert(
                        profile_id.clone(),
                        SpaceRuntimeSlot::starting(spec.clone(), generation),
                    );
                    StartAction::Start(generation)
                }
            }
        };

        let generation = match action {
            StartAction::Start(generation) => generation,
            StartAction::Return(status) => {
                return Ok(SpaceRuntimeStart {
                    disposition: SpaceRuntimeStartDisposition::Existing,
                    status,
                });
            }
            StartAction::Wait { generation, notify } => {
                return self
                    .wait_for_start_conclusion(&profile_id, generation, &notify)
                    .await;
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
        let weak_supervisor = Arc::downgrade(self);
        let event_profile_id = profile_id.clone();
        let forward_event: SpaceRuntimeEventCallback = Arc::new(move |event| {
            weak_supervisor.upgrade().is_some_and(|supervisor| {
                supervisor.publish_event(&event_profile_id, generation, event)
            })
        });
        let factory = Arc::clone(&self.factory);
        let factory_result = tokio::spawn(async move {
            factory
                .create(spec, generation, report_failure, forward_event)
                .await
        })
        .await;
        let factory_result = match factory_result {
            Ok(result) => result,
            Err(error) => Err(SpaceRuntimeFailure::bootstrap(format!(
                "runtime factory task failed: {error}"
            ))),
        };

        match factory_result {
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
                    self.attach_superseded_runtime(&profile_id, generation, Arc::clone(&runtime));
                    let shutdown = shutdown_runtime_until(
                        &runtime,
                        tokio::time::Instant::now() + ENGINE_SHUTDOWN_DEADLINE,
                    )
                    .await;
                    self.finish_superseded_start(
                        &profile_id,
                        generation,
                        &runtime,
                        shutdown.clone(),
                    );
                    let message = match shutdown {
                        Ok(()) => "start was superseded by a newer lifecycle operation".to_string(),
                        Err(error) => format!(
                            "start was superseded; runtime retained after shutdown failed: {error}"
                        ),
                    };
                    Err(SpaceRuntimeStartError {
                        profile_id,
                        generation,
                        failure: SpaceRuntimeFailure::for_category(
                            SpaceRuntimeFailureCategory::Superseded,
                            message,
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
                    self.finish_pending_start_without_runtime(&profile_id, generation);
                }
                Err(SpaceRuntimeStartError {
                    profile_id,
                    generation,
                    failure,
                })
            }
        }
    }

    async fn wait_for_start_conclusion(
        &self,
        profile_id: &str,
        generation: u64,
        notify: &Arc<tokio::sync::Notify>,
    ) -> Result<SpaceRuntimeStart, SpaceRuntimeStartError> {
        #[cfg(test)]
        if let Some(waiter_notify) = self
            .lock_slots()
            .get(profile_id)
            .map(|slot| Arc::clone(&slot.start_waiter_notify))
        {
            waiter_notify.notify_one();
        }
        loop {
            let notified = notify.notified();
            let status = self
                .status(profile_id)
                .ok_or_else(|| SpaceRuntimeStartError {
                    profile_id: profile_id.to_string(),
                    generation,
                    failure: SpaceRuntimeFailure::for_category(
                        SpaceRuntimeFailureCategory::Superseded,
                        "profile runtime registration disappeared",
                    ),
                })?;
            if status.generation != generation {
                return Err(SpaceRuntimeStartError {
                    profile_id: profile_id.to_string(),
                    generation,
                    failure: SpaceRuntimeFailure::for_category(
                        SpaceRuntimeFailureCategory::Superseded,
                        "start was superseded by a newer lifecycle operation",
                    ),
                });
            }
            match status.lifecycle {
                SpaceRuntimeLifecycle::Starting => notified.await,
                SpaceRuntimeLifecycle::Running => {
                    return Ok(SpaceRuntimeStart {
                        disposition: SpaceRuntimeStartDisposition::Existing,
                        status,
                    });
                }
                SpaceRuntimeLifecycle::Failed => {
                    return Err(SpaceRuntimeStartError {
                        profile_id: profile_id.to_string(),
                        generation,
                        failure: status.last_failure.unwrap_or_else(|| {
                            SpaceRuntimeFailure::runtime("runtime start failed")
                        }),
                    });
                }
                SpaceRuntimeLifecycle::Stopping | SpaceRuntimeLifecycle::Stopped => {
                    return Err(SpaceRuntimeStartError {
                        profile_id: profile_id.to_string(),
                        generation,
                        failure: SpaceRuntimeFailure::for_category(
                            SpaceRuntimeFailureCategory::Superseded,
                            "start was superseded by a stop operation",
                        ),
                    });
                }
            }
        }
    }

    pub async fn stop_profile(&self, profile_id: &str) -> Option<SpaceRuntimeStatus> {
        self.stop_profile_until(
            profile_id,
            tokio::time::Instant::now() + ENGINE_SHUTDOWN_DEADLINE,
        )
        .await
    }

    async fn stop_profile_until(
        &self,
        profile_id: &str,
        deadline_at: tokio::time::Instant,
    ) -> Option<SpaceRuntimeStatus> {
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
                    SpaceRuntimeLifecycle::Failed
                        if slot.runtime.is_none() && slot.pending_start_generation.is_none() =>
                    {
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopped;
                        slot.last_failure = None;
                        StopAction::Return(slot.status())
                    }
                    SpaceRuntimeLifecycle::Starting
                    | SpaceRuntimeLifecycle::Running
                    | SpaceRuntimeLifecycle::Failed => {
                        let pending_start_generation = slot.pending_start_generation;
                        let generation = slot.advance_generation();
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopping;
                        slot.last_failure = None;
                        StopAction::Shutdown {
                            generation,
                            pending_start_generation,
                            runtime: slot.runtime.as_ref().map(Arc::clone),
                            notify: Arc::clone(&slot.lifecycle_notify),
                        }
                    }
                }
            };

            match action {
                StopAction::Return(status) => return Some(status),
                StopAction::Wait { generation, notify } => {
                    if !self
                        .wait_until_not_stopping(profile_id, generation, &notify, deadline_at)
                        .await
                    {
                        return self.fail_stopping_deadline(profile_id, generation);
                    }
                }
                StopAction::Shutdown {
                    generation,
                    pending_start_generation,
                    mut runtime,
                    notify,
                } => {
                    if let Some(pending) = pending_start_generation {
                        if !self
                            .wait_for_pending_start(
                                profile_id,
                                generation,
                                pending,
                                &notify,
                                deadline_at,
                            )
                            .await
                        {
                            return self.fail_stopping_deadline(profile_id, generation);
                        }
                        runtime = self.runtime_for_stopping(profile_id, generation);
                    }
                    let shutdown = match runtime.as_ref() {
                        Some(runtime) => shutdown_runtime_until(runtime, deadline_at).await,
                        None => Ok(()),
                    };
                    return self.complete_shutdown(
                        profile_id,
                        generation,
                        runtime.as_ref(),
                        shutdown,
                    );
                }
            }
        }
    }

    pub async fn shutdown_all(self: &Arc<Self>) -> Vec<SpaceRuntimeStatus> {
        self.shutdown_all_with_deadline(ENGINE_SHUTDOWN_DEADLINE)
            .await
    }

    pub async fn shutdown_all_with_deadline(
        self: &Arc<Self>,
        deadline: Duration,
    ) -> Vec<SpaceRuntimeStatus> {
        let profile_ids: Vec<_> = self
            .list()
            .into_iter()
            .map(|status| status.profile_id)
            .collect();
        let deadline_at = tokio::time::Instant::now() + deadline;
        let mut shutdowns = tokio::task::JoinSet::new();
        for profile_id in &profile_ids {
            let supervisor = Arc::clone(self);
            let profile_id = profile_id.clone();
            shutdowns.spawn(async move {
                supervisor
                    .stop_profile_until(&profile_id, deadline_at)
                    .await
            });
        }

        loop {
            match tokio::time::timeout_at(deadline_at, shutdowns.join_next()).await {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    shutdowns.abort_all();
                    break;
                }
            }
        }
        drop(shutdowns);
        for profile_id in &profile_ids {
            self.fail_current_stopping_deadline(profile_id);
        }
        let mut stopped: Vec<_> = profile_ids
            .iter()
            .filter_map(|profile_id| self.status(profile_id))
            .collect();
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
            (failure_generation, slot.runtime.as_ref().map(Arc::clone))
        };

        let shutdown = match runtime.as_ref() {
            Some(runtime) => {
                shutdown_runtime_until(
                    runtime,
                    tokio::time::Instant::now() + ENGINE_SHUTDOWN_DEADLINE,
                )
                .await
            }
            None => Ok(()),
        };
        self.complete_failure_shutdown(
            profile_id,
            failure_generation,
            runtime.as_ref(),
            failure,
            shutdown,
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

    pub async fn dispatch_snapshot(
        &self,
        profile_id: &str,
        snapshot: SystemClipboardSnapshot,
        cancel: CancellationToken,
    ) -> Result<(), SpaceRuntimeFailure> {
        let runtime = self.runtime(profile_id).ok_or_else(|| {
            SpaceRuntimeFailure::for_category(
                SpaceRuntimeFailureCategory::Runtime,
                "profile runtime is not running",
            )
        })?;
        runtime.dispatch_snapshot(snapshot, cancel).await
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ProfiledEngineEvent> {
        self.event_tx.subscribe()
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

    #[cfg(test)]
    fn start_waiter_notification(&self, profile_id: &str) -> Arc<tokio::sync::Notify> {
        Arc::clone(
            &self
                .lock_slots()
                .get(profile_id)
                .expect("profile slot must exist")
                .start_waiter_notify,
        )
    }

    fn publish_event(&self, profile_id: &str, generation: u64, event: EngineEvent) -> bool {
        let accepted = self.lock_slots().get(profile_id).is_some_and(|slot| {
            slot.generation == generation
                && matches!(
                    slot.lifecycle,
                    SpaceRuntimeLifecycle::Starting | SpaceRuntimeLifecycle::Running
                )
        });
        if !accepted {
            return false;
        }
        let _ = self.event_tx.send(ProfiledEngineEvent {
            profile_id: profile_id.to_string(),
            generation,
            event,
        });
        true
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

    fn attach_superseded_runtime(
        &self,
        profile_id: &str,
        start_generation: u64,
        runtime: Arc<dyn SupervisedSpaceRuntime>,
    ) {
        let mut slots = self.lock_slots();
        let Some(slot) = slots.get_mut(profile_id) else {
            return;
        };
        if slot.pending_start_generation == Some(start_generation)
            && matches!(
                slot.lifecycle,
                SpaceRuntimeLifecycle::Stopping | SpaceRuntimeLifecycle::Failed
            )
            && slot.runtime.is_none()
        {
            slot.runtime = Some(runtime);
        }
    }

    fn finish_superseded_start(
        &self,
        profile_id: &str,
        start_generation: u64,
        runtime: &Arc<dyn SupervisedSpaceRuntime>,
        shutdown: Result<(), SpaceRuntimeFailure>,
    ) {
        let notify = {
            let mut slots = self.lock_slots();
            let Some(slot) = slots.get_mut(profile_id) else {
                return;
            };
            if slot.pending_start_generation == Some(start_generation) {
                slot.pending_start_generation = None;
                match shutdown {
                    Ok(()) => {
                        if slot
                            .runtime
                            .as_ref()
                            .is_some_and(|registered| Arc::ptr_eq(registered, runtime))
                        {
                            slot.runtime = None;
                        }
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopped;
                        slot.last_failure = None;
                    }
                    Err(failure) => {
                        if slot.runtime.is_none() {
                            slot.runtime = Some(Arc::clone(runtime));
                        }
                        slot.lifecycle = SpaceRuntimeLifecycle::Failed;
                        slot.last_failure = Some(failure);
                    }
                }
                Some(Arc::clone(&slot.lifecycle_notify))
            } else {
                None
            }
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    fn finish_pending_start_without_runtime(&self, profile_id: &str, generation: u64) {
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
        deadline_at: tokio::time::Instant,
    ) -> bool {
        tokio::time::timeout_at(deadline_at, async {
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
        })
        .await
        .is_ok()
    }

    async fn wait_until_not_stopping(
        &self,
        profile_id: &str,
        generation: u64,
        notify: &Arc<tokio::sync::Notify>,
        deadline_at: tokio::time::Instant,
    ) -> bool {
        tokio::time::timeout_at(deadline_at, async {
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
        })
        .await
        .is_ok()
    }

    fn runtime_for_stopping(
        &self,
        profile_id: &str,
        generation: u64,
    ) -> Option<Arc<dyn SupervisedSpaceRuntime>> {
        let slots = self.lock_slots();
        let slot = slots.get(profile_id)?;
        if slot.generation != generation || slot.lifecycle != SpaceRuntimeLifecycle::Stopping {
            return None;
        }
        slot.runtime.as_ref().map(Arc::clone)
    }

    fn complete_shutdown(
        &self,
        profile_id: &str,
        generation: u64,
        runtime: Option<&Arc<dyn SupervisedSpaceRuntime>>,
        shutdown: Result<(), SpaceRuntimeFailure>,
    ) -> Option<SpaceRuntimeStatus> {
        let completed = {
            let mut slots = self.lock_slots();
            let slot = slots.get_mut(profile_id)?;
            if slot.generation == generation && slot.lifecycle == SpaceRuntimeLifecycle::Stopping {
                match shutdown {
                    Ok(()) => {
                        if runtime.is_none_or(|runtime| {
                            slot.runtime
                                .as_ref()
                                .is_some_and(|registered| Arc::ptr_eq(registered, runtime))
                        }) {
                            slot.runtime = None;
                        }
                        slot.lifecycle = SpaceRuntimeLifecycle::Stopped;
                        slot.last_failure = None;
                    }
                    Err(failure) => {
                        if slot.runtime.is_none() {
                            slot.runtime = runtime.map(Arc::clone);
                        }
                        slot.lifecycle = SpaceRuntimeLifecycle::Failed;
                        slot.last_failure = Some(failure);
                    }
                }
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

    fn complete_failure_shutdown(
        &self,
        profile_id: &str,
        generation: u64,
        runtime: Option<&Arc<dyn SupervisedSpaceRuntime>>,
        runtime_failure: SpaceRuntimeFailure,
        shutdown: Result<(), SpaceRuntimeFailure>,
    ) -> Option<SpaceRuntimeStatus> {
        let completed = {
            let mut slots = self.lock_slots();
            let slot = slots.get_mut(profile_id)?;
            if slot.generation == generation && slot.lifecycle == SpaceRuntimeLifecycle::Stopping {
                slot.lifecycle = SpaceRuntimeLifecycle::Failed;
                match shutdown {
                    Ok(()) => {
                        if runtime.is_none_or(|runtime| {
                            slot.runtime
                                .as_ref()
                                .is_some_and(|registered| Arc::ptr_eq(registered, runtime))
                        }) {
                            slot.runtime = None;
                        }
                        slot.last_failure = Some(runtime_failure);
                    }
                    Err(shutdown_failure) => {
                        if slot.runtime.is_none() {
                            slot.runtime = runtime.map(Arc::clone);
                        }
                        slot.last_failure = Some(SpaceRuntimeFailure::shutdown(format!(
                            "{runtime_failure}; shutdown failed: {shutdown_failure}"
                        )));
                    }
                }
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

    fn fail_stopping_deadline(
        &self,
        profile_id: &str,
        generation: u64,
    ) -> Option<SpaceRuntimeStatus> {
        let completed = {
            let mut slots = self.lock_slots();
            let slot = slots.get_mut(profile_id)?;
            if slot.generation == generation && slot.lifecycle == SpaceRuntimeLifecycle::Stopping {
                slot.lifecycle = SpaceRuntimeLifecycle::Failed;
                slot.last_failure = Some(SpaceRuntimeFailure::shutdown(
                    "global shutdown deadline exceeded",
                ));
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

    fn fail_current_stopping_deadline(&self, profile_id: &str) {
        let generation = self.lock_slots().get(profile_id).and_then(|slot| {
            (slot.lifecycle == SpaceRuntimeLifecycle::Stopping).then_some(slot.generation)
        });
        if let Some(generation) = generation {
            self.fail_stopping_deadline(profile_id, generation);
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

fn remaining_until(deadline_at: tokio::time::Instant) -> Duration {
    deadline_at.saturating_duration_since(tokio::time::Instant::now())
}

async fn shutdown_runtime_until(
    runtime: &Arc<dyn SupervisedSpaceRuntime>,
    deadline_at: tokio::time::Instant,
) -> Result<(), SpaceRuntimeFailure> {
    match tokio::time::timeout_at(deadline_at, runtime.shutdown(remaining_until(deadline_at))).await
    {
        Ok(result) => result,
        Err(_) => Err(SpaceRuntimeFailure::shutdown(
            "global shutdown deadline exceeded",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use async_trait::async_trait;
    use tokio::sync::{Barrier, Notify};
    use tokio_util::sync::CancellationToken;
    use uc_engine::{EngineError, EngineErrorCategory, EngineEvent, EngineState};

    use super::{
        spawn_engine_event_monitor, validate_production_profile_spec, ProfiledEngineEvent,
        SpaceEngineEventStream, SpaceRuntimeEventCallback, SpaceRuntimeFactory,
        SpaceRuntimeFailure, SpaceRuntimeFailureCallback, SpaceRuntimeFailureCategory,
        SpaceRuntimeLifecycle, SpaceRuntimeProfileSpec, SpaceRuntimeRoots,
        SpaceRuntimeStartDisposition, SpaceRuntimeSupervisor, SupervisedSpaceRuntime,
    };
    use crate::daemon::space_catalog::SpaceCatalog;

    struct FakeRuntime {
        shutdowns: AtomicUsize,
        shutdown_results: Mutex<VecDeque<Result<(), SpaceRuntimeFailure>>>,
        shutdown_barrier: Option<Arc<Barrier>>,
        shutdown_delay: Duration,
        ignore_shutdown_deadline: bool,
    }

    impl Default for FakeRuntime {
        fn default() -> Self {
            Self {
                shutdowns: AtomicUsize::new(0),
                shutdown_results: Mutex::new(VecDeque::new()),
                shutdown_barrier: None,
                shutdown_delay: Duration::ZERO,
                ignore_shutdown_deadline: false,
            }
        }
    }

    #[test]
    fn production_factory_rejects_the_legacy_default_profile() {
        let spec = SpaceRuntimeProfileSpec {
            profile_id: "11111111-1111-4111-8111-111111111111".to_string(),
            profile_dir: ".".to_string(),
            data_root: PathBuf::from("legacy-data"),
            cache_root: PathBuf::from("legacy-cache"),
            log_dir: PathBuf::from("legacy-logs"),
            temporary_root: PathBuf::from("legacy-temp"),
            secure_storage_namespace: "11111111-1111-4111-8111-111111111111".to_string(),
        };

        let error = validate_production_profile_spec(&spec)
            .expect_err("legacy must stay on the compatibility host");

        assert_eq!(error.category, SpaceRuntimeFailureCategory::ProfileConflict);
        assert!(error.message.contains("legacy"));
    }

    #[async_trait]
    impl SupervisedSpaceRuntime for FakeRuntime {
        async fn shutdown(&self, deadline: Duration) -> Result<(), SpaceRuntimeFailure> {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            let work = async {
                if let Some(barrier) = &self.shutdown_barrier {
                    barrier.wait().await;
                }
                if !self.shutdown_delay.is_zero() {
                    tokio::time::sleep(self.shutdown_delay).await;
                }
                self.shutdown_results
                    .lock()
                    .expect("lock shutdown results")
                    .pop_front()
                    .unwrap_or(Ok(()))
            };
            if self.ignore_shutdown_deadline {
                return work.await;
            }
            match tokio::time::timeout(deadline, work).await {
                Ok(result) => result,
                Err(_) => Err(SpaceRuntimeFailure::shutdown(
                    "injected shutdown deadline exceeded",
                )),
            }
        }
    }

    #[derive(Clone, Default)]
    struct RuntimePlan {
        shutdown_results: VecDeque<Result<(), SpaceRuntimeFailure>>,
        shutdown_barrier: Option<Arc<Barrier>>,
        shutdown_delay: Duration,
        ignore_shutdown_deadline: bool,
    }

    struct StartGate {
        entered: Arc<Barrier>,
        release: Arc<Notify>,
    }

    impl StartGate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: Arc::new(Barrier::new(2)),
                release: Arc::new(Notify::new()),
            })
        }
    }

    #[derive(Default)]
    struct RecordingFactory {
        starts: Mutex<HashMap<String, usize>>,
        specs: Mutex<Vec<SpaceRuntimeProfileSpec>>,
        callbacks: Mutex<HashMap<String, Vec<SpaceRuntimeFailureCallback>>>,
        event_callbacks: Mutex<HashMap<String, Vec<SpaceRuntimeEventCallback>>>,
        failures: Mutex<HashSet<String>>,
        panics: Mutex<HashSet<String>>,
        runtimes: Mutex<HashMap<String, Vec<Arc<FakeRuntime>>>>,
        start_barrier: Mutex<Option<Arc<Barrier>>>,
        start_gates: Mutex<HashMap<String, Arc<StartGate>>>,
        runtime_plans: Mutex<HashMap<String, RuntimePlan>>,
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

        fn block_start(&self, profile_id: &str) -> Arc<StartGate> {
            let gate = StartGate::new();
            self.start_gates
                .lock()
                .expect("lock start gates")
                .insert(profile_id.to_string(), Arc::clone(&gate));
            gate
        }

        fn panic_on_start(&self, profile_id: &str) {
            self.panics
                .lock()
                .expect("lock panic profiles")
                .insert(profile_id.to_string());
        }

        fn set_runtime_plan(&self, profile_id: &str, plan: RuntimePlan) {
            self.runtime_plans
                .lock()
                .expect("lock runtime plans")
                .insert(profile_id.to_string(), plan);
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

        fn event_callback(&self, profile_id: &str, index: usize) -> SpaceRuntimeEventCallback {
            self.event_callbacks.lock().expect("lock event callbacks")[profile_id][index].clone()
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
            forward_event: SpaceRuntimeEventCallback,
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
            self.event_callbacks
                .lock()
                .expect("lock event callbacks")
                .entry(spec.profile_id.clone())
                .or_default()
                .push(forward_event);
            let barrier = self
                .start_barrier
                .lock()
                .expect("lock start barrier")
                .clone();
            if let Some(barrier) = barrier {
                barrier.wait().await;
            }
            let start_gate = self
                .start_gates
                .lock()
                .expect("lock start gates")
                .get(&spec.profile_id)
                .cloned();
            if let Some(gate) = start_gate {
                gate.entered.wait().await;
                gate.release.notified().await;
            }
            if self
                .panics
                .lock()
                .expect("lock panic profiles")
                .contains(&spec.profile_id)
            {
                panic!("injected factory panic");
            }
            if self
                .failures
                .lock()
                .expect("lock failures")
                .contains(&spec.profile_id)
            {
                return Err(SpaceRuntimeFailure::bootstrap("injected start failure"));
            }
            let plan = self
                .runtime_plans
                .lock()
                .expect("lock runtime plans")
                .get(&spec.profile_id)
                .cloned()
                .unwrap_or_default();
            let runtime = Arc::new(FakeRuntime {
                shutdowns: AtomicUsize::new(0),
                shutdown_results: Mutex::new(plan.shutdown_results),
                shutdown_barrier: plan.shutdown_barrier,
                shutdown_delay: plan.shutdown_delay,
                ignore_shutdown_deadline: plan.ignore_shutdown_deadline,
            });
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

    async fn wait_for_lifecycle(
        supervisor: &SpaceRuntimeSupervisor,
        profile_id: &str,
        lifecycle: SpaceRuntimeLifecycle,
    ) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if supervisor
                    .status(profile_id)
                    .is_some_and(|status| status.lifecycle == lifecycle)
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("lifecycle transition timed out");
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
    async fn concurrent_duplicate_start_waits_for_the_same_success() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));

        let first = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let catalog = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(catalog, &profile_id).await }
        });
        gate.entered.wait().await;
        let waiter = supervisor.start_waiter_notification(&profile_id);
        let second = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let catalog = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(catalog, &profile_id).await }
        });
        waiter.notified().await;
        gate.release.notify_one();
        let left = first.await.expect("first start task");
        let right = second.await.expect("second start task");

        assert!(left.is_ok());
        assert!(right.is_ok());
        assert_eq!(factory.start_count(&profile_id), 1);
        assert_eq!(
            left.as_ref().unwrap().status.lifecycle,
            SpaceRuntimeLifecycle::Running
        );
        assert_eq!(
            right.as_ref().unwrap().status.lifecycle,
            SpaceRuntimeLifecycle::Running
        );
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
    async fn concurrent_duplicate_start_waits_for_the_same_failure() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.fail(&profile_id);
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));

        let first = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        gate.entered.wait().await;
        let waiter = supervisor.start_waiter_notification(&profile_id);
        let second = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        waiter.notified().await;
        gate.release.notify_one();
        let left = first.await.expect("first start task").unwrap_err();
        let right = second.await.expect("second start task").unwrap_err();

        assert_eq!(factory.start_count(&profile_id), 1);
        assert_eq!(right.generation, left.generation);
        assert_eq!(right.failure, left.failure);
        assert_eq!(
            supervisor
                .status(&profile_id)
                .expect("failed status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
    }

    #[tokio::test]
    async fn factory_panic_becomes_typed_failure_and_wakes_duplicate_waiter() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.panic_on_start(&profile_id);
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory, test_roots(&root));

        let first = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        gate.entered.wait().await;
        let waiter = supervisor.start_waiter_notification(&profile_id);
        let second = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        waiter.notified().await;
        gate.release.notify_one();

        let left = first.await.expect("first wrapper task").unwrap_err();
        let right = second.await.expect("second wrapper task").unwrap_err();
        assert_eq!(left.failure, right.failure);
        assert_eq!(
            left.failure.category,
            SpaceRuntimeFailureCategory::Bootstrap
        );
        assert_eq!(
            supervisor
                .status(&profile_id)
                .expect("failed status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
    }

    #[tokio::test]
    async fn stop_while_starting_supersedes_and_shuts_down_created_runtime() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let start = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        gate.entered.wait().await;
        let stop = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let profile_id = profile_id.clone();
            async move { supervisor.stop_profile(&profile_id).await }
        });
        wait_for_lifecycle(&supervisor, &profile_id, SpaceRuntimeLifecycle::Stopping).await;
        gate.release.notify_one();

        assert_eq!(
            start
                .await
                .expect("start task")
                .unwrap_err()
                .failure
                .category,
            SpaceRuntimeFailureCategory::Superseded
        );
        assert_eq!(
            stop.await
                .expect("stop task")
                .expect("stopped status")
                .lifecycle,
            SpaceRuntimeLifecycle::Stopped
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn superseded_start_shutdown_failure_keeps_handle_for_stop_retry() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.set_runtime_plan(
            &profile_id,
            RuntimePlan {
                shutdown_results: VecDeque::from([
                    Err(SpaceRuntimeFailure::shutdown("first shutdown failed")),
                    Ok(()),
                ]),
                ..RuntimePlan::default()
            },
        );
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let start = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        gate.entered.wait().await;
        let stop = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let profile_id = profile_id.clone();
            async move { supervisor.stop_profile(&profile_id).await }
        });
        wait_for_lifecycle(&supervisor, &profile_id, SpaceRuntimeLifecycle::Stopping).await;
        gate.release.notify_one();

        assert!(start.await.expect("start task").is_err());
        assert_eq!(
            stop.await
                .expect("stop task")
                .expect("stop status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(
            supervisor
                .stop_profile(&profile_id)
                .await
                .expect("retry stop status")
                .lifecycle,
            SpaceRuntimeLifecycle::Stopped
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn stop_timeout_blocks_restart_until_superseded_runtime_is_recovered() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.set_runtime_plan(
            &profile_id,
            RuntimePlan {
                shutdown_results: VecDeque::from([
                    Err(SpaceRuntimeFailure::shutdown("superseded cleanup failed")),
                    Ok(()),
                ]),
                ..RuntimePlan::default()
            },
        );
        let gate = factory.block_start(&profile_id);
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let start = tokio::spawn({
            let supervisor = Arc::clone(&supervisor);
            let entries = catalog.entries().to_vec();
            let profile_id = profile_id.clone();
            async move { supervisor.start_entry_for_test(entries, &profile_id).await }
        });
        gate.entered.wait().await;

        let timed_out = supervisor
            .stop_profile_until(
                &profile_id,
                tokio::time::Instant::now() + Duration::from_millis(25),
            )
            .await
            .expect("timed out stop status");
        assert_eq!(timed_out.lifecycle, SpaceRuntimeLifecycle::Failed);

        let restart = tokio::time::timeout(
            Duration::from_millis(100),
            supervisor.start_profile(&catalog, &profile_id),
        )
        .await
        .expect("restart must be rejected while old start is pending")
        .expect_err("pending generation must not be replaced");
        assert_eq!(
            restart.failure.category,
            SpaceRuntimeFailureCategory::Shutdown
        );
        assert_eq!(factory.start_count(&profile_id), 1);

        let retry_stop = supervisor
            .stop_profile_until(
                &profile_id,
                tokio::time::Instant::now() + Duration::from_millis(25),
            )
            .await
            .expect("second timed out stop status");
        assert_eq!(retry_stop.lifecycle, SpaceRuntimeLifecycle::Failed);
        assert!(supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .is_err());
        assert_eq!(factory.start_count(&profile_id), 1);

        gate.release.notify_one();
        assert_eq!(
            start
                .await
                .expect("start task")
                .unwrap_err()
                .failure
                .category,
            SpaceRuntimeFailureCategory::Superseded
        );
        assert_eq!(
            supervisor
                .status(&profile_id)
                .expect("retained status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            1
        );

        assert!(supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .is_err());
        assert_eq!(factory.start_count(&profile_id), 1);
        assert_eq!(
            supervisor
                .stop_profile(&profile_id)
                .await
                .expect("cleanup retry status")
                .lifecycle,
            SpaceRuntimeLifecycle::Stopped
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            2
        );
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
    async fn current_generation_failure_stops_only_that_runtime() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let started = supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("start runtime");

        assert!((factory.callback(&profile_id, 0))(SpaceRuntimeFailure::runtime("fatal")).await);

        let status = supervisor.status(&profile_id).expect("failed status");
        assert!(status.generation > started.status.generation);
        assert_eq!(status.lifecycle, SpaceRuntimeLifecycle::Failed);
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn shutdown_failure_retains_runtime_and_failed_stop_can_retry() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.set_runtime_plan(
            &profile_id,
            RuntimePlan {
                shutdown_results: VecDeque::from([
                    Err(SpaceRuntimeFailure::shutdown("injected shutdown failure")),
                    Ok(()),
                ]),
                ..RuntimePlan::default()
            },
        );
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("start runtime");

        let failed = supervisor
            .stop_profile(&profile_id)
            .await
            .expect("failed stop status");
        assert_eq!(failed.lifecycle, SpaceRuntimeLifecycle::Failed);
        let restart = supervisor.start_profile(&catalog, &profile_id).await;
        assert_eq!(
            restart.unwrap_err().failure.category,
            SpaceRuntimeFailureCategory::Shutdown
        );
        assert_eq!(factory.start_count(&profile_id), 1);
        let stopped = supervisor
            .stop_profile(&profile_id)
            .await
            .expect("retried stop status");

        assert_eq!(stopped.lifecycle, SpaceRuntimeLifecycle::Stopped);
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn failure_callback_shutdown_error_remains_retryable() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        factory.set_runtime_plan(
            &profile_id,
            RuntimePlan {
                shutdown_results: VecDeque::from([
                    Err(SpaceRuntimeFailure::shutdown("callback shutdown failure")),
                    Ok(()),
                ]),
                ..RuntimePlan::default()
            },
        );
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("start runtime");

        assert!((factory.callback(&profile_id, 0))(SpaceRuntimeFailure::runtime("fatal")).await);
        assert_eq!(
            supervisor
                .status(&profile_id)
                .expect("failed status")
                .lifecycle,
            SpaceRuntimeLifecycle::Failed
        );
        assert_eq!(
            supervisor
                .stop_profile(&profile_id)
                .await
                .expect("retry status")
                .lifecycle,
            SpaceRuntimeLifecycle::Stopped
        );
        assert_eq!(
            factory
                .runtime(&profile_id, 0)
                .shutdowns
                .load(Ordering::SeqCst),
            2
        );
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
            .all(|runtime| runtime.shutdowns.load(Ordering::SeqCst) == 1));
    }

    #[tokio::test]
    async fn shutdown_all_runs_profile_shutdowns_concurrently() {
        let (root, catalog) = test_catalog();
        let barrier = Arc::new(Barrier::new(2));
        let factory = Arc::new(RecordingFactory::default());
        for entry in catalog.entries() {
            factory.set_runtime_plan(
                &entry.profile_id,
                RuntimePlan {
                    shutdown_barrier: Some(Arc::clone(&barrier)),
                    ..RuntimePlan::default()
                },
            );
        }
        let supervisor = SpaceRuntimeSupervisor::new(factory, test_roots(&root));
        supervisor.start_enabled(&catalog).await;

        let statuses = supervisor
            .shutdown_all_with_deadline(Duration::from_secs(1))
            .await;

        assert_eq!(statuses.len(), 2);
        assert!(statuses
            .iter()
            .all(|status| status.lifecycle == SpaceRuntimeLifecycle::Stopped));
    }

    #[tokio::test]
    async fn shutdown_all_hard_bounds_non_cooperative_runtime_and_retains_handles() {
        let (root, catalog) = test_catalog();
        let factory = Arc::new(RecordingFactory::default());
        for entry in catalog.entries() {
            factory.set_runtime_plan(
                &entry.profile_id,
                RuntimePlan {
                    shutdown_delay: Duration::from_secs(1),
                    ignore_shutdown_deadline: true,
                    ..RuntimePlan::default()
                },
            );
        }
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        supervisor.start_enabled(&catalog).await;
        let started_at = Instant::now();

        let statuses = supervisor
            .shutdown_all_with_deadline(Duration::from_millis(50))
            .await;

        assert!(started_at.elapsed() < Duration::from_millis(300));
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|status| {
            status.lifecycle == SpaceRuntimeLifecycle::Failed
                && status.last_failure.as_ref().is_some_and(|failure| {
                    failure.category == SpaceRuntimeFailureCategory::Shutdown
                })
        }));
        for entry in catalog.entries() {
            assert_eq!(
                factory
                    .runtime(&entry.profile_id, 0)
                    .shutdowns
                    .load(Ordering::SeqCst),
                1
            );
        }
    }

    #[tokio::test]
    async fn sticky_production_shutdown_failure_is_never_retried_as_success() {
        let shutdown = Arc::new(super::StickyShutdown::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let failure = SpaceRuntimeFailure::shutdown("engine stopped after failed cleanup");

        let first = shutdown
            .run({
                let calls = Arc::clone(&calls);
                let failure = failure.clone();
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(failure)
                }
            })
            .await;
        let second = shutdown
            .run({
                let calls = Arc::clone(&calls);
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;

        assert_eq!(first, Err(failure.clone()));
        assert_eq!(second, Err(failure));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn supervisor_forwards_ordinary_events_with_profile_and_generation() {
        let (root, catalog) = test_catalog();
        let profile_id = catalog.entries()[0].profile_id.clone();
        let factory = Arc::new(RecordingFactory::default());
        let supervisor = SpaceRuntimeSupervisor::new(factory.clone(), test_roots(&root));
        let started = supervisor
            .start_profile(&catalog, &profile_id)
            .await
            .expect("start runtime");
        let mut events = supervisor.subscribe_events();

        assert!((factory.event_callback(&profile_id, 0))(
            EngineEvent::StateChanged {
                state: EngineState::Running,
            }
        ));
        let ProfiledEngineEvent {
            profile_id: actual_profile,
            generation,
            event,
        } = events.recv().await.expect("profile-tagged event");

        assert_eq!(actual_profile, profile_id);
        assert_eq!(generation, started.status.generation);
        assert!(matches!(
            event,
            EngineEvent::StateChanged {
                state: EngineState::Running
            }
        ));
    }

    struct ChannelEventStream {
        receiver: tokio::sync::mpsc::UnboundedReceiver<EngineEvent>,
    }

    #[async_trait]
    impl SpaceEngineEventStream for ChannelEventStream {
        async fn next(&mut self) -> Option<EngineEvent> {
            self.receiver.recv().await
        }
    }

    #[tokio::test]
    async fn event_monitor_forwards_ordinary_events_and_normal_cancel_is_not_failure() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let forwarded = Arc::new(Mutex::new(Vec::new()));
        let failures = Arc::new(AtomicUsize::new(0));
        let event_seen = Arc::new(Notify::new());
        let forward_event: SpaceRuntimeEventCallback = {
            let forwarded = Arc::clone(&forwarded);
            let event_seen = Arc::clone(&event_seen);
            Arc::new(move |event| {
                forwarded.lock().expect("lock forwarded events").push(event);
                event_seen.notify_one();
                true
            })
        };
        let report_failure: SpaceRuntimeFailureCallback = {
            let failures = Arc::clone(&failures);
            Arc::new(move |_| {
                let failures = Arc::clone(&failures);
                Box::pin(async move {
                    failures.fetch_add(1, Ordering::SeqCst);
                    true
                })
            })
        };
        let monitor = spawn_engine_event_monitor(
            Box::new(ChannelEventStream { receiver }),
            cancel.clone(),
            forward_event,
            report_failure,
        );

        sender
            .send(EngineEvent::StateChanged {
                state: EngineState::Running,
            })
            .expect("send ordinary event");
        event_seen.notified().await;
        cancel.cancel();
        monitor.await.expect("monitor join");

        assert_eq!(forwarded.lock().expect("lock forwarded events").len(), 1);
        assert_eq!(failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unexpected_event_stream_exit_reports_runtime_failure() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let failure_seen = Arc::new(Notify::new());
        let failure_value = Arc::new(Mutex::new(None));
        let report_failure: SpaceRuntimeFailureCallback = {
            let failure_seen = Arc::clone(&failure_seen);
            let failure_value = Arc::clone(&failure_value);
            Arc::new(move |failure| {
                let failure_seen = Arc::clone(&failure_seen);
                let failure_value = Arc::clone(&failure_value);
                Box::pin(async move {
                    *failure_value.lock().expect("lock failure value") = Some(failure);
                    failure_seen.notify_one();
                    true
                })
            })
        };
        let monitor = spawn_engine_event_monitor(
            Box::new(ChannelEventStream { receiver }),
            cancel,
            Arc::new(|_| true),
            report_failure,
        );

        drop(sender);
        failure_seen.notified().await;
        monitor.await.expect("monitor join");

        assert_eq!(
            failure_value
                .lock()
                .expect("lock failure value")
                .as_ref()
                .expect("runtime failure")
                .category,
            SpaceRuntimeFailureCategory::Runtime
        );
    }

    #[tokio::test]
    async fn fatal_event_reports_from_detached_task_so_monitor_can_finish_before_shutdown() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let callback_entered = Arc::new(Notify::new());
        let release_callback = Arc::new(Notify::new());
        let callback_finished = Arc::new(Notify::new());
        let report_failure: SpaceRuntimeFailureCallback = {
            let callback_entered = Arc::clone(&callback_entered);
            let release_callback = Arc::clone(&release_callback);
            let callback_finished = Arc::clone(&callback_finished);
            Arc::new(move |_| {
                let callback_entered = Arc::clone(&callback_entered);
                let release_callback = Arc::clone(&release_callback);
                let callback_finished = Arc::clone(&callback_finished);
                Box::pin(async move {
                    callback_entered.notify_one();
                    release_callback.notified().await;
                    callback_finished.notify_one();
                    true
                })
            })
        };
        let monitor = spawn_engine_event_monitor(
            Box::new(ChannelEventStream { receiver }),
            cancel,
            Arc::new(|_| true),
            report_failure,
        );
        sender
            .send(EngineEvent::Fatal {
                error: EngineError::new(9999, EngineErrorCategory::Internal, false),
            })
            .expect("send fatal event");

        callback_entered.notified().await;
        tokio::time::timeout(Duration::from_secs(1), monitor)
            .await
            .expect("monitor must not await its failure callback")
            .expect("monitor join");
        release_callback.notify_one();
        callback_finished.notified().await;
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
