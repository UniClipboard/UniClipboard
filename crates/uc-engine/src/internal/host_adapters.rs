use std::sync::Arc;

use uc_core::app_dirs::{AppDirs, AppPaths};
use uc_core::clipboard::{
    normalize_wire_mime, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::ports::{SecureStorageError, SecureStoragePort, SystemClipboardPort};

use crate::{
    HostCapabilityError, HostCapabilityErrorCategory, HostClipboard, HostClipboardRepresentation,
    HostDirectories, HostSecureStorage,
};

struct HostSecureStorageAdapter {
    host: Box<dyn HostSecureStorage>,
}

impl SecureStoragePort for HostSecureStorageAdapter {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.host.get(key).map_err(map_secure_storage_error)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.host.set(key, value).map_err(map_secure_storage_error)
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.host.delete(key).map_err(map_secure_storage_error)
    }
}

fn map_secure_storage_error(error: HostCapabilityError) -> SecureStorageError {
    let message = error.to_string();
    match error.category() {
        HostCapabilityErrorCategory::Unavailable => SecureStorageError::Unavailable(message),
        HostCapabilityErrorCategory::PermissionDenied => {
            SecureStorageError::PermissionDenied(message)
        }
        HostCapabilityErrorCategory::InvalidHandle | HostCapabilityErrorCategory::Io => {
            SecureStorageError::Other(message)
        }
    }
}

pub fn adapt_secure_storage(host: Box<dyn HostSecureStorage>) -> Arc<dyn SecureStoragePort> {
    Arc::new(HostSecureStorageAdapter { host })
}

struct HostClipboardAdapter {
    host: Box<dyn HostClipboard>,
}

impl SystemClipboardPort for HostClipboardAdapter {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        let snapshot = self
            .host
            .read()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let representations = snapshot
            .representations
            .into_iter()
            .map(|representation| match representation {
                HostClipboardRepresentation::Inline {
                    format,
                    mime_type,
                    bytes,
                } => Ok(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from(format),
                    normalize_wire_mime(mime_type),
                    bytes,
                )),
                HostClipboardRepresentation::File { .. } => Err(anyhow::anyhow!(
                    "host file clipboard representations require file access"
                )),
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(SystemClipboardSnapshot {
            ts_ms: snapshot.observed_at_ms,
            representations,
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        })
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        let representations = snapshot
            .representations
            .into_iter()
            .map(|representation| {
                let bytes = representation
                    .inline_bytes()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "local file clipboard representations cannot reach the host"
                        )
                    })?
                    .to_vec();
                Ok(HostClipboardRepresentation::Inline {
                    format: representation.format_id.to_string(),
                    mime_type: representation.mime.map(|mime| mime.0),
                    bytes,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        self.host
            .write(crate::HostClipboardSnapshot {
                observed_at_ms: snapshot.ts_ms,
                representations,
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

pub fn adapt_system_clipboard(host: Box<dyn HostClipboard>) -> Arc<dyn SystemClipboardPort> {
    Arc::new(HostClipboardAdapter { host })
}

pub fn derive_app_paths(directories: &HostDirectories) -> AppPaths {
    AppPaths::from_app_dirs(&AppDirs {
        app_data_root: directories.private_data().to_path_buf(),
        app_cache_root: directories.cache().to_path_buf(),
        app_log_dir: directories.cache().join("logs"),
    })
}
