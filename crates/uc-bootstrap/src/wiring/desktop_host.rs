//! Desktop host preparation for `uc-engine`.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use uc_core::clipboard::{ClipboardPayloadSource, ObservedClipboardRepresentation};
use uc_core::config::AppConfig;
use uc_core::ports::{SecureStorageError, SecureStoragePort, SystemClipboardPort};
use uc_engine::{
    EngineConfig, HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory,
    HostClipboard, HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories,
    HostFileAccess, HostFileHandle, HostFileMetadata, HostSecureStorage,
};

use crate::layer::paths::{get_default_app_dirs, resolve_app_paths};
use crate::layer::platform::create_desktop_system_clipboard;
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
    let (_, system_clipboard, _) = create_desktop_system_clipboard()?.into_parts();
    let file_registry = Arc::new(DesktopFileRegistry::default());
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
}

impl HostClipboard for DesktopClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        let snapshot = self
            .system_clipboard
            .read_snapshot()
            .map_err(|_| host_io_error("desktop clipboard read failed"))?;
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
}

impl DesktopClipboard {
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
}
