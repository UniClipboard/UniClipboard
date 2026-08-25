//! Desktop host preparation for `uc-engine`.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use uc_app_paths::DesktopRuntimeProfileConfig;
use uc_engine::{
    EngineConfig, HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory,
    HostClipboard, HostClipboardChange, HostClipboardChangeStream, HostClipboardRepresentation,
    HostClipboardSnapshot, HostDirectories, HostFileAccess, HostFileHandle, HostFileMetadata,
    HostSecureStorage,
};
use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent};
use uc_platform::clipboard::{build_event_loop, shutdown_channel, ShutdownTx};
use uc_platform::clipboard::{
    ClipboardPayloadSource, ObservedClipboardRepresentation, SystemClipboard,
    SystemClipboardSnapshot,
};
use uc_platform::ports::{SecureStorageError, SecureStorageProvider};

use crate::layer::paths::{resolve_desktop_host_paths, DesktopHostPaths};
use crate::layer::platform::{create_desktop_system_clipboard, SystemClipboardWiring};
use crate::wiring::analytics::DesktopHostAnalytics;
use crate::wiring::desktop_clipboard_hub::DesktopClipboardProfileHandle;
use crate::wiring::error::{WiringError, WiringResult};
use crate::wiring::secure_storage::{
    build_secure_storage_prelude, build_secure_storage_prelude_for_profile,
};

pub struct DesktopEngineHost {
    engine_config: EngineConfig,
    capabilities: HostCapabilities,
    process_paths: DesktopHostProcessPaths,
    file_handles: DesktopHostFileHandles,
    analytics: DesktopHostAnalytics,
}

impl DesktopEngineHost {
    pub fn process_paths(&self) -> &DesktopHostProcessPaths {
        &self.process_paths
    }

    pub fn file_handles(&self) -> DesktopHostFileHandles {
        self.file_handles.clone()
    }

    pub fn analytics(&self) -> DesktopHostAnalytics {
        self.analytics.clone()
    }

    pub fn into_engine_start(self) -> (EngineConfig, HostCapabilities) {
        (self.engine_config, self.capabilities)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopClipboardMode {
    EngineManaged,
    ExternalRouter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopHostProcessPaths {
    app_data_root: PathBuf,
    daemon_pid: PathBuf,
}

impl DesktopHostProcessPaths {
    fn from_app_paths(paths: &DesktopHostPaths) -> Self {
        Self {
            app_data_root: paths.app_data_root_dir.clone(),
            daemon_pid: paths.app_data_root_dir.join(".daemon-pid"),
        }
    }

    pub fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    pub fn daemon_pid(&self) -> PathBuf {
        self.daemon_pid.clone()
    }
}

fn host_directories(paths: &DesktopHostPaths, temporary_dir: PathBuf) -> HostDirectories {
    HostDirectories::new(
        paths.app_data_root_dir.clone(),
        paths.cache_dir.clone(),
        temporary_dir,
        paths.logs_dir.clone(),
    )
}

fn default_desktop_engine_config() -> EngineConfig {
    EngineConfig::new(env!("CARGO_PKG_VERSION")).with_portable_storage(uc_app_paths::is_portable())
}

fn explicit_desktop_engine_config(config: &DesktopRuntimeProfileConfig) -> EngineConfig {
    EngineConfig::new(env!("CARGO_PKG_VERSION"))
        .with_profile_id(config.profile_id())
        .with_portable_storage(uc_app_paths::is_portable())
}

pub fn prepare_desktop_engine_host() -> WiringResult<DesktopEngineHost> {
    let paths = resolve_desktop_host_paths()?;
    let secure_storage = build_secure_storage_prelude(&paths)?.secure_storage;
    let engine_config = default_desktop_engine_config();
    prepare_desktop_engine_host_from_parts(
        paths,
        engine_config,
        secure_storage,
        DesktopClipboardMode::EngineManaged,
        None,
    )
}

/// Prepare one isolated desktop Engine host from explicit profile roots.
///
/// This entry never reads or modifies `UC_PROFILE`. It keeps real clipboard
/// read/write support for routed operations, but it never exposes a change
/// stream or starts a profile-local watcher; the daemon-level external router
/// owns capture for multi-profile runtimes.
pub fn prepare_desktop_engine_host_for_profile(
    config: DesktopRuntimeProfileConfig,
) -> WiringResult<DesktopEngineHost> {
    let paths = DesktopHostPaths::from_profile_config(&config);
    let secure_storage =
        build_secure_storage_prelude_for_profile(&paths, config.secure_storage_namespace())?
            .secure_storage;
    let engine_config = explicit_desktop_engine_config(&config);
    prepare_desktop_engine_host_from_parts(
        paths,
        engine_config,
        secure_storage,
        DesktopClipboardMode::ExternalRouter,
        None,
    )
}

/// Prepare one isolated desktop Engine host using a shared clipboard Hub
/// profile handle.
///
/// The handle preserves real read/write support, routes every programmatic
/// write through the Hub's global serializer and echo guard, and never exposes
/// an Engine-managed change stream. The caller retains the Hub and handle so a
/// daemon-level actor can stage an exact watcher snapshot before executing
/// `Operation::ObserveClipboardChange` for the selected profile.
pub fn prepare_desktop_engine_host_for_profile_with_hub(
    config: DesktopRuntimeProfileConfig,
    clipboard: DesktopClipboardProfileHandle,
) -> WiringResult<DesktopEngineHost> {
    let paths = DesktopHostPaths::from_profile_config(&config);
    let secure_storage =
        build_secure_storage_prelude_for_profile(&paths, config.secure_storage_namespace())?
            .secure_storage;
    let engine_config = explicit_desktop_engine_config(&config);
    let shared_clipboard: Arc<dyn SystemClipboard> = Arc::new(clipboard);
    prepare_desktop_engine_host_from_parts(
        paths,
        engine_config,
        secure_storage,
        DesktopClipboardMode::ExternalRouter,
        Some(shared_clipboard),
    )
}

fn prepare_desktop_engine_host_from_parts(
    paths: DesktopHostPaths,
    engine_config: EngineConfig,
    secure_storage: Arc<dyn SecureStorageProvider>,
    clipboard_mode: DesktopClipboardMode,
    shared_clipboard: Option<Arc<dyn SystemClipboard>>,
) -> WiringResult<DesktopEngineHost> {
    let (system_clipboard, changes_enabled) = match shared_clipboard {
        Some(clipboard) => (clipboard, false),
        None => {
            let (_, clipboard, wiring) = create_desktop_system_clipboard()?.into_parts();
            (clipboard, wiring == SystemClipboardWiring::Real)
        }
    };
    let file_handles = DesktopHostFileHandles::default();
    let file_registry = Arc::clone(&file_handles.file_registry);
    let pending_snapshot = Arc::new(Mutex::new(None));
    let temporary_dir = paths.cache_dir.join("engine-tmp");
    std::fs::create_dir_all(&temporary_dir).map_err(|error| {
        WiringError::ConfigInit(format!(
            "failed to create engine temporary directory: {error}"
        ))
    })?;
    #[cfg(feature = "e2e-rendezvous")]
    let engine_config = match std::env::var("UC_E2E_RENDEZVOUS_BASE_URL") {
        Ok(base_url) if !base_url.trim().is_empty() => {
            engine_config.with_rendezvous_base_url(base_url.trim().to_string())
        }
        _ => engine_config,
    };
    let analytics = DesktopHostAnalytics::new(paths.app_data_root_dir.join("analytics"));
    let capabilities = HostCapabilities::new(
        host_directories(&paths, temporary_dir),
        Box::new(DesktopSecureStorage { secure_storage }),
        Box::new(DesktopClipboard {
            system_clipboard,
            file_registry: Arc::clone(&file_registry),
            pending_snapshot,
            change_stream_taken: false,
            changes_enabled,
            mode: clipboard_mode,
        }),
        Box::new(file_handles.clone()),
    )
    .with_analytics(analytics.sink(), analytics.identity());

    Ok(DesktopEngineHost {
        engine_config,
        capabilities,
        process_paths: DesktopHostProcessPaths::from_app_paths(&paths),
        file_handles,
        analytics,
    })
}

struct DesktopSecureStorage {
    secure_storage: Arc<dyn SecureStorageProvider>,
}

impl HostSecureStorage for DesktopSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        self.secure_storage
            .get(key)
            .map_err(map_secure_storage_error)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.secure_storage
            .set(key, value)
            .map_err(map_secure_storage_error)
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.secure_storage
            .delete(key)
            .map_err(map_secure_storage_error)
    }
}

fn map_secure_storage_error(error: SecureStorageError) -> HostCapabilityError {
    let category = match error {
        SecureStorageError::Unavailable(_) => HostCapabilityErrorCategory::Unavailable,
        SecureStorageError::PermissionDenied(_) => HostCapabilityErrorCategory::PermissionDenied,
        SecureStorageError::Corrupt(_) | SecureStorageError::Other(_) => {
            HostCapabilityErrorCategory::Io
        }
    };
    HostCapabilityError::new(category, "desktop secure storage failure")
}

struct DesktopClipboard {
    system_clipboard: Arc<dyn SystemClipboard>,
    file_registry: Arc<DesktopFileRegistry>,
    pending_snapshot: Arc<Mutex<Option<SystemClipboardSnapshot>>>,
    change_stream_taken: bool,
    changes_enabled: bool,
    mode: DesktopClipboardMode,
}

impl HostClipboard for DesktopClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        let snapshot = match self.pending_snapshots().take() {
            Some(snapshot) => snapshot,
            None => self
                .system_clipboard
                .read_snapshot()
                .map_err(|_| host_io_error("desktop clipboard read failed"))?,
        };
        let representations = snapshot
            .representations
            .into_iter()
            .map(|representation| self.to_host_representation(representation))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(HostClipboardSnapshot {
            observed_at_ms: snapshot.ts_ms,
            representations,
        })
    }

    fn write(&self, snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        let representations = snapshot
            .representations
            .into_iter()
            .map(|representation| match representation {
                HostClipboardRepresentation::Inline {
                    format,
                    mime_type,
                    bytes,
                } => Ok(ObservedClipboardRepresentation::new(
                    uc_platform::clipboard::RepresentationId::new(),
                    uc_platform::clipboard::FormatId::from(format),
                    mime_type.map(uc_platform::clipboard::MimeType),
                    bytes,
                )),
                HostClipboardRepresentation::File { .. } => Err(HostCapabilityError::new(
                    HostCapabilityErrorCategory::InvalidHandle,
                    "file representations cannot be written directly",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.system_clipboard
            .write_snapshot(SystemClipboardSnapshot {
                ts_ms: snapshot.observed_at_ms,
                representations,
                file_content_digests: Vec::new(),
                file_set_v1_component: None,
            })
            .map_err(|_| host_io_error("desktop clipboard write failed"))
    }

    fn take_change_stream(
        &mut self,
    ) -> Result<Option<Box<dyn HostClipboardChangeStream>>, HostCapabilityError> {
        if self.mode == DesktopClipboardMode::ExternalRouter
            || !self.changes_enabled
            || self.change_stream_taken
        {
            return Ok(None);
        }
        self.change_stream_taken = true;
        Ok(Some(Box::new(DesktopClipboardChanges {
            system_clipboard: Arc::clone(&self.system_clipboard),
            pending_snapshot: Arc::clone(&self.pending_snapshot),
            running: None,
        })))
    }
}

impl DesktopClipboard {
    fn pending_snapshots(&self) -> MutexGuard<'_, Option<SystemClipboardSnapshot>> {
        match self.pending_snapshot.lock() {
            Ok(snapshot) => snapshot,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn to_host_representation(
        &self,
        representation: ObservedClipboardRepresentation,
    ) -> Result<HostClipboardRepresentation, HostCapabilityError> {
        let source = representation.source().clone();
        let format = representation.format_id.to_string();
        let mime_type = representation.mime.map(|mime| mime.0);
        match source {
            ClipboardPayloadSource::Inline(bytes) => Ok(HostClipboardRepresentation::Inline {
                format,
                mime_type,
                bytes,
            }),
            ClipboardPayloadSource::LocalFile { path, size_bytes } => {
                let handle = self.file_registry.register_input(path.clone())?;
                let display_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file")
                    .to_string();
                Ok(HostClipboardRepresentation::File {
                    format,
                    handle,
                    display_name,
                    mime_type,
                    size_bytes,
                })
            }
        }
    }
}

struct DesktopClipboardChanges {
    system_clipboard: Arc<dyn SystemClipboard>,
    pending_snapshot: Arc<Mutex<Option<SystemClipboardSnapshot>>>,
    running: Option<RunningDesktopClipboardChanges>,
}

struct RunningDesktopClipboardChanges {
    receiver: tokio::sync::mpsc::Receiver<PlatformEvent>,
    shutdown: ShutdownTx,
    join: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl DesktopClipboardChanges {
    fn start_if_needed(&mut self) -> Result<(), HostCapabilityError> {
        if self.running.is_some() {
            return Ok(());
        }
        let event_loop =
            build_event_loop().map_err(|_| host_io_error("desktop clipboard listener failed"))?;
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        let watcher = ClipboardWatcher::new(Arc::clone(&self.system_clipboard), sender);
        let (shutdown, shutdown_receiver) = shutdown_channel();
        let join = tokio::task::spawn_blocking(move || event_loop.run(watcher, shutdown_receiver));
        self.running = Some(RunningDesktopClipboardChanges {
            receiver,
            shutdown,
            join,
        });
        Ok(())
    }

    fn pending_snapshots(&self) -> MutexGuard<'_, Option<SystemClipboardSnapshot>> {
        match self.pending_snapshot.lock() {
            Ok(snapshot) => snapshot,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl HostClipboardChangeStream for DesktopClipboardChanges {
    async fn next(&mut self) -> Result<HostClipboardChange, HostCapabilityError> {
        self.start_if_needed()?;
        loop {
            let event = match self.running.as_mut() {
                Some(running) => running.receiver.recv().await,
                None => return Ok(HostClipboardChange::Closed),
            };
            match event {
                Some(PlatformEvent::ClipboardChanged { snapshot }) if snapshot.is_empty() => {}
                Some(PlatformEvent::ClipboardChanged { snapshot }) => {
                    *self.pending_snapshots() = Some(snapshot);
                    return Ok(HostClipboardChange::Changed);
                }
                None => return Ok(HostClipboardChange::Closed),
            }
        }
    }

    async fn shutdown(&mut self) -> Result<(), HostCapabilityError> {
        let Some(running) = self.running.take() else {
            return Ok(());
        };
        running.shutdown.signal();
        match tokio::time::timeout(std::time::Duration::from_secs(5), running.join).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => {
                Err(host_io_error("desktop clipboard listener shutdown failed"))
            }
        }
    }
}

impl Drop for DesktopClipboardChanges {
    fn drop(&mut self) {
        if let Some(running) = self.running.as_ref() {
            running.shutdown.signal();
        }
    }
}

#[derive(Default)]
struct DesktopFileRegistry {
    next_id: AtomicU64,
    paths: Mutex<HashMap<String, RegisteredDesktopFile>>,
}

impl DesktopFileRegistry {
    fn register_input(&self, path: PathBuf) -> Result<HostFileHandle, HostCapabilityError> {
        self.register(path, DesktopFileMode::Input)
    }

    fn register_output(&self, path: PathBuf) -> Result<HostFileHandle, HostCapabilityError> {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .map_err(|_| host_io_error("desktop output file creation failed"))?;
        self.register(path, DesktopFileMode::Output)
    }

    fn register(
        &self,
        path: PathBuf,
        mode: DesktopFileMode,
    ) -> Result<HostFileHandle, HostCapabilityError> {
        let id = format!(
            "desktop-file-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        self.paths()
            .insert(id.clone(), RegisteredDesktopFile { path, mode });
        Ok(HostFileHandle::new(id))
    }

    fn resolve(
        &self,
        handle: &HostFileHandle,
    ) -> Result<RegisteredDesktopFile, HostCapabilityError> {
        self.paths().get(handle.as_str()).cloned().ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorCategory::InvalidHandle,
                "unknown desktop file handle",
            )
        })
    }

    fn paths(&self) -> MutexGuard<'_, HashMap<String, RegisteredDesktopFile>> {
        match self.paths.lock() {
            Ok(paths) => paths,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopFileMode {
    Input,
    Output,
}

#[derive(Clone)]
struct RegisteredDesktopFile {
    path: PathBuf,
    mode: DesktopFileMode,
}

#[derive(Clone, Default)]
pub struct DesktopHostFileHandles {
    file_registry: Arc<DesktopFileRegistry>,
}

impl DesktopHostFileHandles {
    pub fn register_input(&self, path: PathBuf) -> Result<HostFileHandle, HostCapabilityError> {
        self.file_registry.register_input(path)
    }

    pub fn register_output(&self, path: PathBuf) -> Result<HostFileHandle, HostCapabilityError> {
        self.file_registry.register_output(path)
    }
}

impl HostFileAccess for DesktopHostFileHandles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        let file = self.file_registry.resolve(handle)?;
        let metadata = std::fs::metadata(&file.path)
            .map_err(|_| host_io_error("desktop file metadata failed"))?;
        Ok(HostFileMetadata {
            display_name: display_name(&file.path),
            size_bytes: metadata.len(),
            mime_type: None,
        })
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        let registered = self.file_registry.resolve(handle)?;
        if registered.mode != DesktopFileMode::Input {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorCategory::InvalidHandle,
                "desktop output handle cannot be read",
            ));
        }
        let mut file = std::fs::File::open(registered.path)
            .map_err(|_| host_io_error("desktop file open failed"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| host_io_error("desktop file seek failed"))?;
        let mut bytes = vec![0; max_bytes as usize];
        let read = file
            .read(&mut bytes)
            .map_err(|_| host_io_error("desktop file read failed"))?;
        bytes.truncate(read);
        Ok(bytes)
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        let registered = self.file_registry.resolve(handle)?;
        if registered.mode != DesktopFileMode::Output {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorCategory::InvalidHandle,
                "desktop input handle cannot be written",
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .open(registered.path)
            .map_err(|_| host_io_error("desktop output file open failed"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| host_io_error("desktop output file seek failed"))?;
        file.write_all(bytes)
            .map_err(|_| host_io_error("desktop output file write failed"))
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        let registered = self.file_registry.resolve(handle)?;
        if registered.mode != DesktopFileMode::Output {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorCategory::InvalidHandle,
                "desktop input handle cannot finish an output",
            ));
        }
        OpenOptions::new()
            .write(true)
            .open(registered.path)
            .and_then(|file| file.sync_all())
            .map_err(|_| host_io_error("desktop output file flush failed"))
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string()
}

fn host_io_error(detail: &'static str) -> HostCapabilityError {
    HostCapabilityError::new(HostCapabilityErrorCategory::Io, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiring::desktop_clipboard_hub::DesktopClipboardHub;
    use uc_platform::clipboard::{FormatId, RepresentationId};

    #[derive(Default)]
    struct RecordingClipboard {
        writes: Mutex<Vec<SystemClipboardSnapshot>>,
    }

    impl SystemClipboard for RecordingClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(empty_system_snapshot())
        }

        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            self.writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(snapshot);
            Ok(())
        }
    }

    struct StaticClipboard {
        snapshot: SystemClipboardSnapshot,
    }

    impl SystemClipboard for StaticClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(self.snapshot.clone())
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn empty_system_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: Vec::new(),
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn clipboard_for_test(
        system_clipboard: Arc<dyn SystemClipboard>,
        mode: DesktopClipboardMode,
    ) -> DesktopClipboard {
        DesktopClipboard {
            system_clipboard,
            file_registry: Arc::new(DesktopFileRegistry::default()),
            pending_snapshot: Arc::new(Mutex::new(None)),
            change_stream_taken: false,
            changes_enabled: true,
            mode,
        }
    }

    #[test]
    fn explicit_profile_clipboards_never_expose_engine_managed_change_streams() {
        let system_clipboard: Arc<dyn SystemClipboard> = Arc::new(StaticClipboard {
            snapshot: empty_system_snapshot(),
        });
        let mut profile_a = clipboard_for_test(
            Arc::clone(&system_clipboard),
            DesktopClipboardMode::ExternalRouter,
        );
        let mut profile_b =
            clipboard_for_test(system_clipboard, DesktopClipboardMode::ExternalRouter);

        assert!(profile_a.take_change_stream().unwrap().is_none());
        assert!(profile_b.take_change_stream().unwrap().is_none());
    }

    #[test]
    fn default_clipboard_keeps_engine_managed_change_stream() {
        let system_clipboard: Arc<dyn SystemClipboard> = Arc::new(StaticClipboard {
            snapshot: empty_system_snapshot(),
        });
        let mut clipboard =
            clipboard_for_test(system_clipboard, DesktopClipboardMode::EngineManaged);

        assert!(clipboard.take_change_stream().unwrap().is_some());
    }

    #[test]
    fn external_router_mode_still_allows_inbound_clipboard_writes() {
        let system_clipboard = Arc::new(RecordingClipboard::default());
        let clipboard = clipboard_for_test(
            system_clipboard.clone(),
            DesktopClipboardMode::ExternalRouter,
        );

        clipboard
            .write(HostClipboardSnapshot {
                observed_at_ms: 7,
                representations: vec![HostClipboardRepresentation::Inline {
                    format: "text".into(),
                    mime_type: Some("text/plain".into()),
                    bytes: b"inbound".to_vec(),
                }],
            })
            .unwrap();

        let writes = system_clipboard
            .writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].representations.len(), 1);
    }

    #[test]
    fn explicit_host_with_hub_uses_staged_snapshot_and_shared_write_path() {
        let temporary = tempfile::tempdir().unwrap();
        let config = DesktopRuntimeProfileConfig::new(
            "019d-profile-hub",
            temporary.path().join("data"),
            temporary.path().join("cache"),
            temporary.path().join("logs"),
        )
        .unwrap();
        let system_clipboard = Arc::new(RecordingClipboard::default());
        let hub = DesktopClipboardHub::from_parts(
            system_clipboard.clone(),
            false,
            Arc::new(|| anyhow::bail!("profile host must not start a watcher")),
        );
        let profile = hub.profile_handle();
        hub.stage_snapshot(
            &profile,
            SystemClipboardSnapshot {
                ts_ms: 9,
                representations: vec![ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("text"),
                    Some(uc_platform::clipboard::MimeType("text/plain".into())),
                    b"staged exact".to_vec(),
                )],
                file_content_digests: Vec::new(),
                file_set_v1_component: None,
            },
        )
        .unwrap();

        let host = prepare_desktop_engine_host_for_profile_with_hub(config, profile).unwrap();
        let (_, capabilities) = host.into_engine_start();

        let observed = capabilities.clipboard().read().unwrap();
        let HostClipboardRepresentation::Inline { bytes, .. } = &observed.representations[0] else {
            panic!("expected inline clipboard representation");
        };
        assert_eq!(bytes, b"staged exact");
        capabilities
            .clipboard()
            .write(HostClipboardSnapshot {
                observed_at_ms: 10,
                representations: vec![HostClipboardRepresentation::Inline {
                    format: "text".into(),
                    mime_type: Some("text/plain".into()),
                    bytes: b"shared write".to_vec(),
                }],
            })
            .unwrap();
        assert_eq!(system_clipboard.writes.lock().unwrap().len(), 1);
    }

    #[test]
    fn default_engine_config_scope_matches_the_raw_engine_baseline() {
        let baseline = EngineConfig::new(env!("CARGO_PKG_VERSION"));
        let actual = default_desktop_engine_config();

        assert_eq!(actual.profile_id(), baseline.profile_id());
    }

    #[test]
    fn explicit_engine_config_scope_uses_each_profile_id() {
        let temporary = tempfile::tempdir().unwrap();
        let config = |profile_id: &str| {
            DesktopRuntimeProfileConfig::new(
                profile_id,
                temporary.path().join(profile_id).join("data"),
                temporary.path().join(profile_id).join("cache"),
                temporary.path().join(profile_id).join("logs"),
            )
            .unwrap()
        };

        assert_eq!(
            explicit_desktop_engine_config(&config("019d-profile-a")).profile_id(),
            "019d-profile-a"
        );
        assert_eq!(
            explicit_desktop_engine_config(&config("019d-profile-b")).profile_id(),
            "019d-profile-b"
        );
    }

    #[test]
    fn engine_directories_use_the_resolved_log_directory() {
        let paths = DesktopHostPaths {
            db_path: "/host/private/uniclipboard.db".into(),
            vault_dir: "/host/private/vault".into(),
            settings_path: "/host/private/settings.json".into(),
            logs_dir: "/host/platform-logs".into(),
            cache_dir: "/host/cache".into(),
            app_data_root_dir: "/host/private".into(),
        };

        let directories = host_directories(&paths, "/host/temporary".into());

        assert_eq!(directories.logs(), Path::new("/host/platform-logs"));
    }

    #[test]
    fn local_file_clipboard_representation_uses_an_opaque_readable_handle() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private-name.txt");
        std::fs::write(&path, b"private file bytes").unwrap();
        let registry = Arc::new(DesktopFileRegistry::default());
        let clipboard = DesktopClipboard {
            system_clipboard: Arc::new(StaticClipboard {
                snapshot: SystemClipboardSnapshot {
                    ts_ms: 1,
                    representations: vec![ObservedClipboardRepresentation::new_local_file(
                        RepresentationId::new(),
                        FormatId::from("files"),
                        Some(uc_platform::clipboard::MimeType("text/plain".into())),
                        path.clone(),
                        18,
                    )],
                    file_content_digests: Vec::new(),
                    file_set_v1_component: None,
                },
            }),
            file_registry: Arc::clone(&registry),
            pending_snapshot: Arc::new(Mutex::new(None)),
            change_stream_taken: false,
            changes_enabled: false,
            mode: DesktopClipboardMode::EngineManaged,
        };

        let snapshot = clipboard.read().unwrap();
        let HostClipboardRepresentation::File {
            handle,
            display_name,
            size_bytes,
            ..
        } = &snapshot.representations[0]
        else {
            panic!("expected file representation");
        };
        assert_eq!(display_name, "private-name.txt");
        assert_eq!(*size_bytes, 18);
        assert!(!handle.as_str().contains("private-name.txt"));
        assert!(!handle
            .as_str()
            .contains(temp.path().to_string_lossy().as_ref()));

        let files = DesktopHostFileHandles {
            file_registry: registry,
        };
        assert_eq!(files.metadata(handle).unwrap().size_bytes, 18);
        assert_eq!(files.read_chunk(handle, 8, 4).unwrap(), b"file");
        assert!(!format!("{handle:?}").contains("private-name.txt"));
    }

    #[test]
    fn pending_platform_snapshot_is_consumed_before_a_fresh_clipboard_read() {
        let pending = SystemClipboardSnapshot {
            ts_ms: 7,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(uc_platform::clipboard::MimeType("text/plain".into())),
                b"event snapshot".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        let fresh = SystemClipboardSnapshot {
            ts_ms: 8,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(uc_platform::clipboard::MimeType("text/plain".into())),
                b"fresh snapshot".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        let clipboard = DesktopClipboard {
            system_clipboard: Arc::new(StaticClipboard { snapshot: fresh }),
            file_registry: Arc::new(DesktopFileRegistry::default()),
            pending_snapshot: Arc::new(Mutex::new(Some(pending))),
            change_stream_taken: false,
            changes_enabled: false,
            mode: DesktopClipboardMode::EngineManaged,
        };

        let first = clipboard.read().unwrap();
        let second = clipboard.read().unwrap();
        let HostClipboardRepresentation::Inline { bytes: first, .. } = &first.representations[0]
        else {
            panic!("expected inline pending snapshot");
        };
        let HostClipboardRepresentation::Inline { bytes: second, .. } = &second.representations[0]
        else {
            panic!("expected inline fresh snapshot");
        };
        assert_eq!(first, b"event snapshot");
        assert_eq!(second, b"fresh snapshot");
    }
}
