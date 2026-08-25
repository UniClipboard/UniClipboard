//! Production Windows multi-space composition.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_bootstrap::{
    DesktopClipboardHub, DesktopClipboardHubChangeStream, DesktopClipboardProfileHandle,
    DesktopClipboardStageExecution,
};
use uc_daemon_contract::api::dto::v2::spaces::{
    CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
    SpaceFaultDto, SpaceIncomingSyncStateDto, SpaceProfileSummaryDto, SpaceRuntimeStateDto,
};
use uc_engine::{
    CreateSpaceInput, Engine, JoinSpaceInput, JoinSpaceStatusSummary, ObserveClipboardChangeInput,
    Operation, OperationResult, SecretString,
};
use uc_platform::clipboard::SystemClipboardSnapshot;

use super::clipboard_router::{
    spawn_clipboard_router, ClipboardRouterBackend, ClipboardRouterHandle, ClipboardRouterTask,
};
use super::run_mode::DaemonRunMode;
use super::space_catalog::{SpaceCatalog, SpaceCatalogEntry};
use super::space_runtime_supervisor::{
    SpaceRuntimeLifecycle, SpaceRuntimeRoots, SpaceRuntimeStatus, SpaceRuntimeSupervisor,
};
use super::spaces_http::{SpacesBackendError, SpacesHttpBackend, SpacesHttpService};
use super::startup_recovery::spawn_startup_recovery;
use super::windows_space_authority::{
    CatalogPort, ClipboardRouterPort, RuntimePort, WindowsSpaceAuthority,
    WindowsSpaceAuthorityError,
};

const JOIN_COMPLETION_TIMEOUT: Duration = Duration::from_secs(90);
const JOIN_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
struct CatalogRepository {
    root: PathBuf,
}

impl CatalogRepository {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    async fn read<R, F>(&self, read: F) -> anyhow::Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&SpaceCatalog) -> anyhow::Result<R> + Send + 'static,
    {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let catalog = SpaceCatalog::load_or_migrate(root)?;
            read(&catalog)
        })
        .await
        .map_err(|error| anyhow::anyhow!("catalog task failed: {error}"))?
    }

    async fn mutate<R, F>(&self, mutate: F) -> anyhow::Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut SpaceCatalog) -> anyhow::Result<R> + Send + 'static,
    {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut catalog = SpaceCatalog::load_or_migrate(root)?;
            mutate(&mut catalog)
        })
        .await
        .map_err(|error| anyhow::anyhow!("catalog task failed: {error}"))?
    }

    async fn entries(&self) -> anyhow::Result<Vec<SpaceCatalogEntry>> {
        self.read(|catalog| Ok(catalog.entries().to_vec())).await
    }

    async fn active_profile(&self) -> anyhow::Result<String> {
        self.read(|catalog| {
            catalog
                .entries()
                .iter()
                .find(|entry| entry.active_send)
                .map(|entry| entry.profile_id.clone())
                .ok_or_else(|| anyhow::anyhow!("catalog has no active-send profile"))
        })
        .await
    }

    async fn reserve_entry(&self) -> anyhow::Result<SpaceCatalogEntry> {
        self.read(|catalog| Ok(catalog.new_profile_entry())).await
    }

    async fn publish_entry(&self, entry: SpaceCatalogEntry) -> anyhow::Result<()> {
        self.mutate(move |catalog| Ok(catalog.add_entry(entry)?))
            .await
    }

    async fn set_active(&self, profile_id: String) -> anyhow::Result<()> {
        self.mutate(move |catalog| Ok(catalog.set_active_send(&profile_id)?))
            .await
    }

    async fn remove(&self, profile_id: String) -> anyhow::Result<SpaceCatalogEntry> {
        self.mutate(move |catalog| Ok(catalog.remove_profile(&profile_id)?))
            .await
    }
}

struct ProductionCatalogPort {
    catalog: CatalogRepository,
}

#[async_trait]
impl CatalogPort for ProductionCatalogPort {
    async fn profile_dir(&self, profile_id: &str) -> anyhow::Result<Option<String>> {
        let profile_id = profile_id.to_owned();
        self.catalog
            .read(move |catalog| {
                Ok(catalog
                    .entries()
                    .iter()
                    .find(|entry| entry.profile_id == profile_id)
                    .map(|entry| entry.profile_dir.clone()))
            })
            .await
    }

    async fn remove(&self, profile_id: &str) -> anyhow::Result<()> {
        self.catalog.remove(profile_id.to_owned()).await.map(drop)
    }
}

struct ProductionRuntimePort {
    legacy_profile_id: String,
    supervisor: Arc<SpaceRuntimeSupervisor>,
}

#[async_trait]
impl RuntimePort for ProductionRuntimePort {
    async fn ensure_available(&self, profile_id: &str) -> anyhow::Result<()> {
        if profile_id == self.legacy_profile_id {
            return Ok(());
        }
        let status = self
            .supervisor
            .status(profile_id)
            .ok_or_else(|| anyhow::anyhow!("profile runtime is not registered"))?;
        if status.lifecycle != SpaceRuntimeLifecycle::Running {
            anyhow::bail!("profile runtime is not running");
        }
        Ok(())
    }

    async fn stop(&self, profile_id: &str) -> anyhow::Result<()> {
        let status = self
            .supervisor
            .stop_profile(profile_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("profile runtime is not registered"))?;
        if status.lifecycle != SpaceRuntimeLifecycle::Stopped {
            anyhow::bail!("profile runtime did not stop cleanly");
        }
        Ok(())
    }
}

struct ProductionClipboardRouterBackend {
    catalog: CatalogRepository,
    legacy_profile_id: String,
    legacy_engine: Arc<Engine>,
    hub: DesktopClipboardHub,
    legacy_clipboard: DesktopClipboardProfileHandle,
    supervisor: Arc<SpaceRuntimeSupervisor>,
}

#[async_trait]
impl ClipboardRouterBackend<SystemClipboardSnapshot> for ProductionClipboardRouterBackend {
    async fn load_active_profile(&self, cancel: CancellationToken) -> anyhow::Result<String> {
        tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("active-profile load was cancelled"),
            result = self.catalog.active_profile() => result,
        }
    }

    async fn dispatch_snapshot(
        &self,
        profile_id: &str,
        snapshot: SystemClipboardSnapshot,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if profile_id != self.legacy_profile_id {
            return self
                .supervisor
                .dispatch_snapshot(profile_id, snapshot, cancel)
                .await
                .map_err(anyhow::Error::new);
        }

        let engine = Arc::clone(&self.legacy_engine);
        let outcome = self
            .hub
            .execute_with_staged_snapshot(&self.legacy_clipboard, snapshot, move || async move {
                tokio::select! {
                    _ = cancel.cancelled() => anyhow::bail!("clipboard dispatch was cancelled"),
                    result = engine.execute(Operation::ObserveClipboardChange(
                        ObserveClipboardChangeInput { dispatch: true },
                    )) => result.map(|_| ()).map_err(anyhow::Error::new),
                }
            })
            .await?;
        match outcome {
            DesktopClipboardStageExecution::ConsumedAndCompleted(()) => Ok(()),
            DesktopClipboardStageExecution::CompletedWithoutConsumption(()) => {
                anyhow::bail!("Engine completed without consuming staged clipboard")
            }
            DesktopClipboardStageExecution::FailedBeforeConsumption(error) => {
                anyhow::bail!("clipboard dispatch failed before capture: {error}")
            }
            DesktopClipboardStageExecution::FailedAfterConsumption(error) => {
                anyhow::bail!("clipboard dispatch failed after capture: {error}")
            }
        }
    }

    async fn persist_active_profile(
        &self,
        profile_id: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("active-profile persist was cancelled"),
            result = self.catalog.set_active(profile_id.to_owned()) => result,
        }
    }
}

struct ProductionSpacesBackend {
    catalog: CatalogRepository,
    legacy_profile_id: String,
    legacy_engine: Arc<Engine>,
    supervisor: Arc<SpaceRuntimeSupervisor>,
    authority: Arc<WindowsSpaceAuthority>,
}

impl ProductionSpacesBackend {
    async fn engine_for(&self, profile_id: &str) -> Option<Arc<Engine>> {
        if profile_id == self.legacy_profile_id {
            Some(Arc::clone(&self.legacy_engine))
        } else {
            self.supervisor.engine(profile_id)
        }
    }

    async fn summary(
        &self,
        entry: &SpaceCatalogEntry,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        let status = if entry.profile_id == self.legacy_profile_id {
            None
        } else {
            self.supervisor.status(&entry.profile_id)
        };
        let runtime_state = runtime_state(entry, status.as_ref());
        let last_fault = status
            .as_ref()
            .and_then(|status| status.last_failure.as_ref())
            .map(|failure| SpaceFaultDto {
                category: format!("{:?}", failure.category).to_ascii_lowercase(),
                message_code: None,
            });
        let Some(engine) = self.engine_for(&entry.profile_id).await else {
            return Ok(SpaceProfileSummaryDto {
                profile_id: entry.profile_id.clone(),
                space_id: None,
                display_name: None,
                device_name: None,
                runtime_state,
                incoming_sync_state: SpaceIncomingSyncStateDto::Disabled,
                last_fault,
                is_active_send: entry.active_send,
            });
        };

        let (space_id, device_name, configured) =
            match engine.execute(Operation::QuerySetupState).await {
                Ok(OperationResult::SetupState(setup)) => {
                    (setup.space_id, setup.device_name, setup.has_completed)
                }
                Ok(_) | Err(_) => (None, None, false),
            };
        let incoming_sync_state = match engine.execute(Operation::QueryReceiveReadiness).await {
            Ok(OperationResult::ReceiveReadiness(readiness)) if readiness.degraded => {
                SpaceIncomingSyncStateDto::Degraded
            }
            Ok(OperationResult::ReceiveReadiness(readiness)) if readiness.ready => {
                SpaceIncomingSyncStateDto::Enabled
            }
            _ => SpaceIncomingSyncStateDto::Disabled,
        };
        Ok(SpaceProfileSummaryDto {
            profile_id: entry.profile_id.clone(),
            space_id,
            display_name: None,
            device_name,
            runtime_state: if configured {
                runtime_state
            } else if runtime_state == SpaceRuntimeStateDto::Running {
                SpaceRuntimeStateDto::Locked
            } else {
                runtime_state
            },
            incoming_sync_state,
            last_fault,
            is_active_send: entry.active_send,
        })
    }

    async fn start_reserved_runtime(
        &self,
        entry: &SpaceCatalogEntry,
    ) -> Result<Arc<Engine>, SpacesBackendError> {
        if let Err(error) = self.supervisor.start_entry(entry.clone()).await {
            warn!(
                profile_id = %entry.profile_id,
                error = %error.failure,
                "reserved space runtime failed to start"
            );
            let _ = self.supervisor.stop_profile(&entry.profile_id).await;
            return Err(SpacesBackendError::runtime_unavailable(
                "runtime_start_failed",
                "space runtime could not start",
            ));
        }
        self.supervisor.engine(&entry.profile_id).ok_or_else(|| {
            SpacesBackendError::runtime_unavailable(
                "runtime_unavailable",
                "space runtime did not expose an Engine",
            )
        })
    }

    async fn rollback_unpublished_runtime(&self, profile_id: &str) {
        let _ = self.supervisor.stop_profile(profile_id).await;
    }

    async fn publish_and_summarize(
        &self,
        entry: SpaceCatalogEntry,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        if let Err(error) = self.catalog.publish_entry(entry.clone()).await {
            self.rollback_unpublished_runtime(&entry.profile_id).await;
            return Err(SpacesBackendError::internal(format!(
                "failed to publish completed profile: {error}"
            )));
        }
        self.summary(&entry).await
    }
}

#[async_trait]
impl SpacesHttpBackend for ProductionSpacesBackend {
    async fn list_spaces(&self) -> Result<Vec<SpaceProfileSummaryDto>, SpacesBackendError> {
        let entries = self
            .catalog
            .entries()
            .await
            .map_err(|error| SpacesBackendError::internal(error.to_string()))?;
        let mut summaries = Vec::with_capacity(entries.len());
        for entry in entries {
            summaries.push(self.summary(&entry).await?);
        }
        Ok(summaries)
    }

    async fn create_space(
        &self,
        request: CreateSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        if request.passphrase != request.passphrase_confirm {
            return Err(SpacesBackendError::bad_request(
                "passphrase_mismatch",
                "passphrase confirmation does not match",
            ));
        }
        let _mutation = self
            .authority
            .acquire_mutation()
            .await
            .map_err(map_authority)?;
        let entry = self
            .catalog
            .reserve_entry()
            .await
            .map_err(|error| SpacesBackendError::internal(error.to_string()))?;
        let engine = self.start_reserved_runtime(&entry).await?;
        let result = engine
            .execute(Operation::CreateSpace(CreateSpaceInput {
                passphrase: SecretString::new(request.passphrase),
                passphrase_confirmation: SecretString::new(request.passphrase_confirm),
                device_name: request.device_name,
            }))
            .await;
        if !matches!(result, Ok(OperationResult::SpaceCreated { .. })) {
            self.rollback_unpublished_runtime(&entry.profile_id).await;
            return Err(SpacesBackendError::runtime_unavailable(
                "create_failed",
                "space creation did not complete",
            ));
        }
        self.publish_and_summarize(entry).await
    }

    async fn join_space(
        &self,
        request: JoinSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        if request.code.trim().is_empty() {
            return Err(SpacesBackendError::bad_request(
                "invitation_required",
                "invitation code is required",
            ));
        }
        let _mutation = self
            .authority
            .acquire_mutation()
            .await
            .map_err(map_authority)?;
        let entry = self
            .catalog
            .reserve_entry()
            .await
            .map_err(|error| SpacesBackendError::internal(error.to_string()))?;
        let engine = self.start_reserved_runtime(&entry).await?;
        let result = engine
            .execute(Operation::JoinSpace(JoinSpaceInput {
                invitation_code: request.code,
                device_name: request.device_name,
                passphrase: SecretString::new(request.passphrase),
                preserve_unreadable_history: false,
            }))
            .await;
        let status = match result {
            Ok(OperationResult::JoinSpace(status)) => status,
            _ => {
                self.rollback_unpublished_runtime(&entry.profile_id).await;
                return Err(SpacesBackendError::runtime_unavailable(
                    "join_failed",
                    "space join could not be started",
                ));
            }
        };
        if let Err(error) = wait_for_join_completion(&engine, status).await {
            self.rollback_unpublished_runtime(&entry.profile_id).await;
            return Err(error);
        }
        self.publish_and_summarize(entry).await
    }

    async fn set_active_send(
        &self,
        request: SetActiveSendSpaceRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        self.authority
            .set_active(&request.profile_id)
            .await
            .map_err(map_authority)?;
        let profile_id = request.profile_id;
        let entry = self
            .catalog
            .read(move |catalog| {
                catalog
                    .entries()
                    .iter()
                    .find(|entry| entry.profile_id == profile_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("active profile disappeared from catalog"))
            })
            .await
            .map_err(|error| SpacesBackendError::internal(error.to_string()))?;
        self.summary(&entry).await
    }

    async fn remove_space(
        &self,
        profile_id: String,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        let before = self
            .catalog
            .read({
                let profile_id = profile_id.clone();
                move |catalog| {
                    catalog
                        .entries()
                        .iter()
                        .find(|entry| entry.profile_id == profile_id)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("profile not found"))
                }
            })
            .await
            .map_err(|_| {
                SpacesBackendError::not_found("profile_not_found", "space was not found")
            })?;
        self.authority
            .remove(&profile_id)
            .await
            .map_err(map_authority)?;
        let mut summary = self.summary(&before).await?;
        summary.runtime_state = SpaceRuntimeStateDto::Stopped;
        summary.incoming_sync_state = SpaceIncomingSyncStateDto::Disabled;
        summary.is_active_send = false;
        Ok(summary)
    }
}

async fn wait_for_join_completion(
    engine: &Arc<Engine>,
    mut status: JoinSpaceStatusSummary,
) -> Result<(), SpacesBackendError> {
    let deadline = tokio::time::Instant::now() + JOIN_COMPLETION_TIMEOUT;
    loop {
        match status {
            JoinSpaceStatusSummary::Active { .. } => return Ok(()),
            JoinSpaceStatusSummary::Rejected { .. } => {
                return Err(SpacesBackendError::conflict(
                    "join_rejected",
                    "space join was rejected",
                ))
            }
            JoinSpaceStatusSummary::Pending { ref join_id, .. } => {
                let expected_join_id = join_id.clone();
                if tokio::time::Instant::now() >= deadline {
                    return Err(SpacesBackendError::runtime_unavailable(
                        "join_timeout",
                        "space join is still pending",
                    ));
                }
                tokio::time::sleep(JOIN_POLL_INTERVAL).await;
                status = match engine.execute(Operation::QueryDeviceTrust).await {
                    Ok(OperationResult::DeviceTrust(snapshot)) => match snapshot.current_join {
                        Some(candidate) if join_id_of(&candidate) == expected_join_id => candidate,
                        _ => continue,
                    },
                    _ => continue,
                };
            }
        }
    }
}

fn join_id_of(status: &JoinSpaceStatusSummary) -> &str {
    match status {
        JoinSpaceStatusSummary::Active { join_id, .. }
        | JoinSpaceStatusSummary::Pending { join_id, .. }
        | JoinSpaceStatusSummary::Rejected { join_id, .. } => join_id,
    }
}

fn runtime_state(
    entry: &SpaceCatalogEntry,
    status: Option<&SpaceRuntimeStatus>,
) -> SpaceRuntimeStateDto {
    if entry.profile_dir == "." {
        return SpaceRuntimeStateDto::Running;
    }
    match status.map(|status| status.lifecycle) {
        Some(SpaceRuntimeLifecycle::Running) => SpaceRuntimeStateDto::Running,
        Some(SpaceRuntimeLifecycle::Starting) => SpaceRuntimeStateDto::Starting,
        Some(SpaceRuntimeLifecycle::Failed) => SpaceRuntimeStateDto::Failed,
        Some(SpaceRuntimeLifecycle::Stopping | SpaceRuntimeLifecycle::Stopped) | None => {
            SpaceRuntimeStateDto::Stopped
        }
    }
}

fn map_authority(error: WindowsSpaceAuthorityError) -> SpacesBackendError {
    match error {
        WindowsSpaceAuthorityError::ProfileNotFound(_) => {
            SpacesBackendError::not_found("profile_not_found", "space was not found")
        }
        WindowsSpaceAuthorityError::LegacyProfileCannotBeRemoved => SpacesBackendError::conflict(
            "legacy_profile",
            "the compatibility space cannot be removed",
        ),
        WindowsSpaceAuthorityError::ActiveProfileCannotBeRemoved => SpacesBackendError::conflict(
            "active_profile",
            "the active-send space cannot be removed",
        ),
        WindowsSpaceAuthorityError::Quiescing => {
            SpacesBackendError::runtime_unavailable("daemon_stopping", "the daemon is stopping")
        }
        WindowsSpaceAuthorityError::Runtime(message) => {
            SpacesBackendError::runtime_unavailable("runtime_unavailable", message)
        }
        WindowsSpaceAuthorityError::Catalog(message)
        | WindowsSpaceAuthorityError::Router(message) => SpacesBackendError::internal(message),
    }
}

struct ClipboardForwarder {
    cancel: CancellationToken,
    join: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl ClipboardForwarder {
    fn spawn(
        stream: Option<DesktopClipboardHubChangeStream>,
        router: ClipboardRouterHandle<SystemClipboardSnapshot>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let join = stream.map(|mut stream| {
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = task_cancel.cancelled() => break,
                        next = stream.next() => match next? {
                            Some(snapshot) => router.clipboard_changed(snapshot).await?,
                            None => break,
                        },
                    }
                }
                stream.shutdown().await?;
                Ok(())
            })
        });
        Self { cancel, join }
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        if let Some(join) = self.join.take() {
            join.await
                .map_err(|error| anyhow::anyhow!("clipboard watcher task failed: {error}"))??;
        }
        Ok(())
    }
}

pub(crate) struct WindowsMultiSpace {
    pub(crate) service: SpacesHttpService,
    authority: Arc<WindowsSpaceAuthority>,
    forwarder: ClipboardForwarder,
    router_task: Option<ClipboardRouterTask<SystemClipboardSnapshot>>,
    supervisor: Arc<SpaceRuntimeSupervisor>,
}

impl WindowsMultiSpace {
    pub(crate) async fn start(
        catalog_root: PathBuf,
        roots: SpaceRuntimeRoots,
        legacy_engine: Arc<Engine>,
        hub: DesktopClipboardHub,
        legacy_clipboard: DesktopClipboardProfileHandle,
        run_mode: DaemonRunMode,
    ) -> anyhow::Result<Self> {
        let catalog = CatalogRepository::new(catalog_root);
        let entries = catalog.entries().await?;
        let legacy_profile_id = entries
            .iter()
            .find(|entry| entry.profile_dir == ".")
            .map(|entry| entry.profile_id.clone())
            .ok_or_else(|| anyhow::anyhow!("space catalog has no legacy compatibility profile"))?;
        let supervisor = SpaceRuntimeSupervisor::production(roots, hub.clone());
        for entry in entries
            .iter()
            .filter(|entry| entry.profile_dir != "." && entry.enabled)
            .cloned()
        {
            match supervisor.start_entry(entry.clone()).await {
                Ok(_) => {
                    if let Some(engine) = supervisor.engine(&entry.profile_id) {
                        spawn_startup_recovery(run_mode, engine);
                    }
                }
                Err(error) => warn!(
                    profile_id = %entry.profile_id,
                    error = %error.failure,
                    "secondary space runtime recovery failed"
                ),
            }
        }

        let backend = Arc::new(ProductionClipboardRouterBackend {
            catalog: catalog.clone(),
            legacy_profile_id: legacy_profile_id.clone(),
            legacy_engine: Arc::clone(&legacy_engine),
            hub: hub.clone(),
            legacy_clipboard,
            supervisor: Arc::clone(&supervisor),
        });
        let (router, router_task) = spawn_clipboard_router(backend);
        let authority = Arc::new(WindowsSpaceAuthority::new(
            Arc::new(ProductionCatalogPort {
                catalog: catalog.clone(),
            }),
            Arc::new(ProductionRuntimePort {
                legacy_profile_id: legacy_profile_id.clone(),
                supervisor: Arc::clone(&supervisor),
            }),
            Arc::new(ClipboardRouterPort::new(router.clone())),
        ));
        let service = SpacesHttpService::new(Arc::new(ProductionSpacesBackend {
            catalog,
            legacy_profile_id,
            legacy_engine,
            supervisor: Arc::clone(&supervisor),
            authority: Arc::clone(&authority),
        }));
        let stream = hub.take_change_stream()?;
        let forwarder = ClipboardForwarder::spawn(stream, router);
        info!("Windows multi-space runtime started");
        Ok(Self {
            service,
            authority,
            forwarder,
            router_task: Some(router_task),
            supervisor,
        })
    }

    pub(crate) async fn quiesce(&self) -> anyhow::Result<()> {
        self.authority.quiesce().await.map_err(anyhow::Error::new)
    }

    pub(crate) async fn shutdown_clipboard(&mut self) -> anyhow::Result<()> {
        self.forwarder.shutdown().await?;
        if let Some(router_task) = self.router_task.take() {
            router_task.shutdown().await?;
        }
        Ok(())
    }

    pub(crate) async fn shutdown_secondaries(&self) -> anyhow::Result<()> {
        let statuses = self.supervisor.shutdown_all().await;
        let failed: Vec<_> = statuses
            .into_iter()
            .filter(|status| status.lifecycle != SpaceRuntimeLifecycle::Stopped)
            .map(|status| status.profile_id)
            .collect();
        if !failed.is_empty() {
            anyhow::bail!("secondary space runtimes did not stop cleanly: {failed:?}");
        }
        Ok(())
    }
}

pub(crate) fn catalog_root(process_data_root: &Path) -> PathBuf {
    process_data_root.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reserved_profile_is_invisible_until_successful_publication() {
        let root = tempfile::tempdir().unwrap();
        let catalog = CatalogRepository::new(root.path().to_path_buf());
        let before = catalog.entries().await.unwrap();

        let reserved = catalog.reserve_entry().await.unwrap();
        assert_eq!(catalog.entries().await.unwrap(), before);

        catalog.publish_entry(reserved.clone()).await.unwrap();
        let after = catalog.entries().await.unwrap();
        assert_eq!(after.len(), before.len() + 1);
        assert!(after.iter().any(|entry| entry == &reserved));
    }

    #[test]
    fn runtime_projection_keeps_stopping_non_runnable() {
        let entry = SpaceCatalogEntry {
            profile_id: uuid::Uuid::new_v4().to_string(),
            profile_dir: "profile-test".into(),
            enabled: true,
            active_send: false,
        };
        let status = SpaceRuntimeStatus {
            profile_id: entry.profile_id.clone(),
            generation: 2,
            lifecycle: SpaceRuntimeLifecycle::Stopping,
            last_failure: None,
        };
        assert_eq!(
            runtime_state(&entry, Some(&status)),
            SpaceRuntimeStateDto::Stopped
        );
    }
}
