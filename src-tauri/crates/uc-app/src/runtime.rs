//! # CoreRuntime
//!
//! Tauri-free runtime holding all non-Tauri application state.
//!
//! This struct is the central artifact of RNTM-01: it compiles in uc-app without
//! any Tauri dependency. AppRuntime (in uc-tauri) wraps this and adds only
//! Tauri-specific handles (app_handle).

use std::sync::Arc;

use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ports::host_event_emitter::HostEventEmitterPort;
use uc_core::ports::SettingsPort;
use uc_core::security::state::EncryptionState;

use crate::app_paths::AppPaths;
use crate::deps::AppDeps;
use crate::task_registry::TaskRegistry;
use crate::usecases::setup::SetupOrchestrator;
use crate::usecases::LifecycleStatusPort;

/// Tauri-free runtime holding all non-Tauri application state.
///
/// This struct is the core of RNTM-01: it compiles in uc-app without
/// any Tauri dependency. AppRuntime (in uc-tauri) wraps this and adds
/// only Tauri-specific handles (app_handle).
pub struct CoreRuntime {
    pub(crate) deps: AppDeps,
    /// Shared cell for event emitter. Uses Arc<RwLock<Arc<...>>> so that
    /// consumers (like HostEventSetupPort) can hold a clone of the outer Arc
    /// and always read the current emitter after bootstrap swaps it.
    pub(crate) event_emitter: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub(crate) lifecycle_status: Arc<dyn LifecycleStatusPort>,
    pub(crate) setup_orchestrator: Arc<SetupOrchestrator>,
    pub(crate) clipboard_integration_mode: ClipboardIntegrationMode,
    pub(crate) task_registry: Arc<TaskRegistry>,
    pub(crate) storage_paths: AppPaths,
    /// Single write boundary for programmatic clipboard writes.
    /// `None` for CLI-only runtimes that do not perform clipboard writes.
    pub(crate) clipboard_write_coordinator: Option<Arc<crate::usecases::ClipboardWriteCoordinator>>,
}

impl CoreRuntime {
    /// Construct a new CoreRuntime.
    ///
    /// IMPORTANT: `event_emitter` is a pre-built shared cell
    /// `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>`. The caller creates
    /// this cell and shares it with both CoreRuntime and
    /// build_setup_orchestrator so that HostEventSetupPort reads from
    /// the same cell. CoreRuntime does NOT wrap the emitter internally.
    pub fn new(
        deps: AppDeps,
        event_emitter: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
        lifecycle_status: Arc<dyn LifecycleStatusPort>,
        setup_orchestrator: Arc<SetupOrchestrator>,
        clipboard_integration_mode: ClipboardIntegrationMode,
        task_registry: Arc<TaskRegistry>,
        storage_paths: AppPaths,
    ) -> Self {
        Self {
            deps,
            event_emitter, // store directly — no wrapping
            lifecycle_status,
            setup_orchestrator,
            clipboard_integration_mode,
            task_registry,
            storage_paths,
            clipboard_write_coordinator: None,
        }
    }

    /// Attach a `ClipboardWriteCoordinator` after construction (builder pattern).
    ///
    /// Called by daemon/GUI bootstrap to wire in the coordinator from
    /// `BackgroundRuntimeDeps`. CLI runtimes leave this as `None`.
    pub fn with_clipboard_write_coordinator(
        mut self,
        coordinator: Arc<crate::usecases::ClipboardWriteCoordinator>,
    ) -> Self {
        self.clipboard_write_coordinator = Some(coordinator);
        self
    }

    /// Set the `ClipboardWriteCoordinator` by mutable reference.
    ///
    /// Used by GUI bootstrap via `Arc::get_mut` before the runtime is shared.
    pub fn set_clipboard_write_coordinator(
        &mut self,
        coordinator: Arc<crate::usecases::ClipboardWriteCoordinator>,
    ) {
        self.clipboard_write_coordinator = Some(coordinator);
    }

    /// Returns the `ClipboardWriteCoordinator` if available.
    ///
    /// `None` in CLI-only runtimes that do not perform clipboard writes.
    pub fn clipboard_write_coordinator(
        &self,
    ) -> Option<&Arc<crate::usecases::ClipboardWriteCoordinator>> {
        self.clipboard_write_coordinator.as_ref()
    }

    /// Returns a clone of the shared emitter cell (Arc<RwLock<...>>).
    /// Used by HostEventSetupPort to read-through after emitter swap.
    pub fn emitter_cell(&self) -> Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>> {
        self.event_emitter.clone()
    }

    /// Returns the current emitter value (clones the inner Arc).
    pub fn event_emitter(&self) -> Arc<dyn HostEventEmitterPort> {
        self.event_emitter
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Swap the event emitter. Called from Tauri setup callback.
    pub fn set_event_emitter(&self, emitter: Arc<dyn HostEventEmitterPort>) {
        *self
            .event_emitter
            .write()
            .unwrap_or_else(|p| p.into_inner()) = emitter;
    }

    pub fn device_id(&self) -> String {
        self.deps
            .device
            .device_identity
            .current_device_id()
            .to_string()
    }

    pub async fn is_encryption_ready(&self) -> bool {
        self.deps.security.encryption_session.is_ready().await
    }

    pub async fn encryption_state(&self) -> Result<EncryptionState, String> {
        self.deps
            .security
            .encryption_state
            .load_state()
            .await
            .map_err(|e| e.to_string())
    }

    pub fn settings_port(&self) -> Arc<dyn SettingsPort> {
        self.deps.settings.clone()
    }

    pub fn wiring_deps(&self) -> &AppDeps {
        &self.deps
    }

    pub fn clipboard_integration_mode(&self) -> ClipboardIntegrationMode {
        self.clipboard_integration_mode
    }

    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        &self.task_registry
    }

    pub fn setup_orchestrator(&self) -> &Arc<SetupOrchestrator> {
        &self.setup_orchestrator
    }

    pub fn lifecycle_status(&self) -> &Arc<dyn LifecycleStatusPort> {
        &self.lifecycle_status
    }

    pub fn storage_paths(&self) -> &AppPaths {
        &self.storage_paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use uc_core::ports::host_event_emitter::{EmitError, HostEvent, SetupHostEvent};

    struct NoopEmitter;

    impl HostEventEmitterPort for NoopEmitter {
        fn emit(&self, _event: HostEvent) -> Result<(), EmitError> {
            Ok(())
        }
    }

    struct RecordingEmitter {
        events: Arc<StdMutex<Vec<String>>>,
    }

    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
            self.events.lock().unwrap().push(format!("{:?}", event));
            Ok(())
        }
    }

    #[test]
    fn emitter_cell_reflects_swap() {
        // This tests the underlying shared-cell pattern: a cloned Arc<RwLock<...>>
        // sees changes made through the original. This is the foundation for the
        // stale-emitter fix, but does NOT test HostEventSetupPort itself.
        let initial: Arc<dyn HostEventEmitterPort> = Arc::new(NoopEmitter);
        let cell = Arc::new(std::sync::RwLock::new(initial));
        let cell_clone = cell.clone();

        let events = Arc::new(StdMutex::new(vec![]));
        let recording: Arc<dyn HostEventEmitterPort> = Arc::new(RecordingEmitter {
            events: events.clone(),
        });

        // Swap via original
        *cell.write().unwrap() = recording;

        // Read from clone — should see new emitter
        let current = cell_clone.read().unwrap().clone();
        current
            .emit(HostEvent::Setup(SetupHostEvent::StateChanged {
                state: uc_core::setup::SetupState::Welcome,
                session_id: None,
            }))
            .unwrap();

        assert_eq!(events.lock().unwrap().len(), 1);
    }
}
