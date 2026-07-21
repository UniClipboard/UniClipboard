//! Desktop host preparation for `uc-engine`.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use uc_core::clipboard::{ClipboardPayloadSource, ObservedClipboardRepresentation};
use uc_core::config::AppConfig;
use uc_core::ports::{SecureStorageError, SecureStoragePort, SystemClipboardPort};
use uc_engine::{
    EngineConfig, HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory,
    HostClipboard, HostClipboardChange, HostClipboardChangeStream, HostClipboardRepresentation,
    HostClipboardSnapshot, HostDirectories, HostFileAccess, HostFileHandle, HostFileMetadata,
    HostSecureStorage,
};
use uc_platform::clipboard::watcher::{ClipboardWatcher, PlatformEvent};
use uc_platform::clipboard::{build_event_loop, shutdown_channel, ShutdownTx};

use crate::layer::paths::{get_default_app_dirs, resolve_app_paths};
use crate::layer::platform::{create_desktop_system_clipboard, SystemClipboardWiring};
use crate::wiring::deps::{WiringError, WiringResult};
use crate::wiring::desktop::build_secure_storage_prelude;

pub struct DesktopEngineHost {
    engine_config: EngineConfig,
    capabilities: HostCapabilities,
    storage_paths: uc_application::facade::AppPaths,
}

impl DesktopEngineHost {
    pub fn storage_paths(&self) -> &uc_application::facade::AppPaths {
        &self.storage_paths
    }

    pub fn into_engine_start(self) -> (EngineConfig, HostCapabilities) {
        (self.engine_config, self.capabilities)
    }
}

pub fn prepare_desktop_engine_host(config: &AppConfig) -> WiringResult<DesktopEngineHost> {
    let platform_dirs = get_default_app_dirs()?;
    let paths = resolve_app_paths(&platform_dirs, config)?;
    let secure_storage = build_secure_storage_prelude(&paths)?.secure_storage;
    let (_, system_clipboard, clipboard_wiring) = create_desktop_system_clipboard()?.into_parts();
    let file_registry = Arc::new(DesktopFileRegistry::default());
    let pending_snapshot = Arc::new(Mutex::new(None));
    let temporary_dir = paths.cache_dir.join("engine-tmp");
    std::fs::create_dir_all(&temporary_dir).map_err(|error| {
        WiringError::ConfigInit(format!(
            "failed to create engine temporary directory: {error}"
        ))
    })?;
    let profile_id = std::env::var("UC_PROFILE")
        .ok()
        .filter(|profile| !profile.trim().is_empty())
        .unwrap_or_else(|| "default".to_string());
    let engine_config = EngineConfig::new(env!("CARGO_PKG_VERSION")).with_profile_id(profile_id);
    let capabilities = HostCapabilities::new(
        HostDirectories::new(
            paths.app_data_root_dir.clone(),
            paths.cache_dir.clone(),
            temporary_dir,
        ),
        Box::new(DesktopSecureStorage { secure_storage }),
        Box::new(DesktopClipboard {
            system_clipboard,
            file_registry: Arc::clone(&file_registry),
            pending_snapshot,
            change_stream_taken: false,
            changes_enabled: clipboard_wiring == SystemClipboardWiring::Real,
        }),
        Box::new(DesktopFiles { file_registry }),
    );

    Ok(DesktopEngineHost {
        engine_config,
        capabilities,
        storage_paths: paths,
    })
}

struct DesktopSecureStorage {
    secure_storage: Arc<dyn SecureStoragePort>,
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
    system_clipboard: Arc<dyn SystemClipboardPort>,
    file_registry: Arc<DesktopFileRegistry>,
    pending_snapshot: Arc<Mutex<Option<uc_core::SystemClipboardSnapshot>>>,
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
                    uc_core::ids::RepresentationId::new(),
                    uc_core::ids::FormatId::from(format),
                    mime_type.map(uc_core::MimeType),
                    bytes,
                )),
                HostClipboardRepresentation::File { .. } => Err(HostCapabilityError::new(
                    HostCapabilityErrorCategory::InvalidHandle,
                    "file representations cannot be written directly",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.system_clipboard
            .write_snapshot(uc_core::SystemClipboardSnapshot {
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
    fn pending_snapshots(&self) -> MutexGuard<'_, Option<uc_core::SystemClipboardSnapshot>> {
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
                let handle = self.file_registry.register(path.clone())?;
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
    system_clipboard: Arc<dyn SystemClipboardPort>,
    pending_snapshot: Arc<Mutex<Option<uc_core::SystemClipboardSnapshot>>>,
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

    fn pending_snapshots(&self) -> MutexGuard<'_, Option<uc_core::SystemClipboardSnapshot>> {
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
    paths: Mutex<HashMap<String, PathBuf>>,
}

impl DesktopFileRegistry {
    fn register(&self, path: PathBuf) -> Result<HostFileHandle, HostCapabilityError> {
        let id = format!(
            "desktop-file-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        self.paths().insert(id.clone(), path);
        Ok(HostFileHandle::new(id))
    }

    fn resolve(&self, handle: &HostFileHandle) -> Result<PathBuf, HostCapabilityError> {
        self.paths().get(handle.as_str()).cloned().ok_or_else(|| {
            HostCapabilityError::new(
                HostCapabilityErrorCategory::InvalidHandle,
                "unknown desktop file handle",
            )
        })
    }

    fn paths(&self) -> MutexGuard<'_, HashMap<String, PathBuf>> {
        match self.paths.lock() {
            Ok(paths) => paths,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

struct DesktopFiles {
    file_registry: Arc<DesktopFileRegistry>,
}

impl HostFileAccess for DesktopFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        let path = self.file_registry.resolve(handle)?;
        let metadata =
            std::fs::metadata(&path).map_err(|_| host_io_error("desktop file metadata failed"))?;
        Ok(HostFileMetadata {
            display_name: display_name(&path),
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
        let path = self.file_registry.resolve(handle)?;
        let mut file =
            std::fs::File::open(path).map_err(|_| host_io_error("desktop file open failed"))?;
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
        _handle: &HostFileHandle,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(
            HostCapabilityErrorCategory::InvalidHandle,
            "desktop export handle is not registered",
        ))
    }

    fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(
            HostCapabilityErrorCategory::InvalidHandle,
            "desktop export handle is not registered",
        ))
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
    use uc_core::ids::{FormatId, RepresentationId};

    struct StaticClipboard {
        snapshot: uc_core::SystemClipboardSnapshot,
    }

    impl SystemClipboardPort for StaticClipboard {
        fn read_snapshot(&self) -> anyhow::Result<uc_core::SystemClipboardSnapshot> {
            Ok(self.snapshot.clone())
        }

        fn write_snapshot(
            &self,
            _snapshot: uc_core::SystemClipboardSnapshot,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn local_file_clipboard_representation_uses_an_opaque_readable_handle() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private-name.txt");
        std::fs::write(&path, b"private file bytes").unwrap();
        let registry = Arc::new(DesktopFileRegistry::default());
        let clipboard = DesktopClipboard {
            system_clipboard: Arc::new(StaticClipboard {
                snapshot: uc_core::SystemClipboardSnapshot {
                    ts_ms: 1,
                    representations: vec![ObservedClipboardRepresentation::new_local_file(
                        RepresentationId::new(),
                        FormatId::from("files"),
                        Some(uc_core::MimeType("text/plain".into())),
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

        let files = DesktopFiles {
            file_registry: registry,
        };
        assert_eq!(files.metadata(handle).unwrap().size_bytes, 18);
        assert_eq!(files.read_chunk(handle, 8, 4).unwrap(), b"file");
        assert!(!format!("{handle:?}").contains("private-name.txt"));
    }

    #[test]
    fn pending_platform_snapshot_is_consumed_before_a_fresh_clipboard_read() {
        let pending = uc_core::SystemClipboardSnapshot {
            ts_ms: 7,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(uc_core::MimeType("text/plain".into())),
                b"event snapshot".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        let fresh = uc_core::SystemClipboardSnapshot {
            ts_ms: 8,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(uc_core::MimeType("text/plain".into())),
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
