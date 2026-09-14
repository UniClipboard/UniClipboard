//! Desktop host preparation for `uc-engine`.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
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
use crate::wiring::error::{WiringError, WiringResult};
use crate::wiring::secure_storage::build_secure_storage_prelude;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopHostProcessPaths {
    app_data_root: PathBuf,
    daemon_pid: PathBuf,
    logs_dir: PathBuf,
}

impl DesktopHostProcessPaths {
    fn from_app_paths(paths: &DesktopHostPaths) -> Self {
        Self {
            app_data_root: paths.app_data_root_dir.clone(),
            daemon_pid: paths.app_data_root_dir.join(".daemon-pid"),
            logs_dir: paths.logs_dir.clone(),
        }
    }

    pub fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    pub fn daemon_pid(&self) -> PathBuf {
        self.daemon_pid.clone()
    }

    pub fn logs_dir(&self) -> &Path {
        &self.logs_dir
    }
}

fn host_directories(paths: &DesktopHostPaths, temporary_dir: PathBuf) -> HostDirectories {
    HostDirectories::new(
        paths.app_data_root_dir.clone(),
        paths.cache_dir.clone(),
        temporary_dir,
        paths.logs_dir.clone(),
    )
    .with_upgrade_backups(paths.upgrade_backups_dir.clone())
}

pub fn prepare_desktop_engine_host() -> WiringResult<DesktopEngineHost> {
    let paths = resolve_desktop_host_paths()?;
    let secure_storage = build_secure_storage_prelude(&paths)?.secure_storage;
    let (_, system_clipboard, clipboard_wiring) = create_desktop_system_clipboard()?.into_parts();
    let file_handles = DesktopHostFileHandles::default();
    let file_registry = Arc::clone(&file_handles.file_registry);
    let pending_snapshot = Arc::new(Mutex::new(None));
    let temporary_dir = paths.cache_dir.join("engine-tmp");
    std::fs::create_dir_all(&temporary_dir).map_err(|error| {
        WiringError::ConfigInit(format!(
            "failed to create engine temporary directory: {error}"
        ))
    })?;
    let engine_config = EngineConfig::new(env!("CARGO_PKG_VERSION"))
        .with_portable_storage(uc_app_paths::is_portable());
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
            changes_enabled: clipboard_wiring == SystemClipboardWiring::Real,
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
        if !self.changes_enabled || self.change_stream_taken {
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
mod tests;
