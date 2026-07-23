use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;
use uc_core::app_dirs::{AppDirs, AppPaths};
use uc_core::clipboard::{
    normalize_wire_mime, FileDisplayMetadata, FileDisplayMetadataEntry,
    ObservedClipboardRepresentation, SystemClipboardSnapshot, FILE_DISPLAY_METADATA_FORMAT,
    FILE_DISPLAY_METADATA_MIME,
};
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::ports::{
    ClipboardHostEvent, ClipboardOriginKind, DeliveryHostEvent, EmitError, HostEvent,
    HostEventEmitterPort, PlatformClipboardPort, SecureStorageError, SecureStoragePort,
    SystemClipboardPort, TransferHostEvent,
};

use crate::assembly::deps::{BackgroundRuntimeDeps, WiredDependencies, WiringError, WiringResult};
use crate::assembly::platform::SystemClipboardLayer;
use crate::assembly::wire::{wire_dependencies_from_inputs, CoreWiringInputs};
use crate::engine::event_stream::EventSender;
use crate::{
    EngineConfig, EngineEvent, HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory,
    HostClipboard, HostClipboardChangeStream, HostClipboardRepresentation, HostDirectories,
    HostFileAccess, HostSecureStorage, TransferProgress,
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
    files: Arc<dyn HostFileAccess>,
    import_root: PathBuf,
}

impl SystemClipboardPort for HostClipboardAdapter {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        let snapshot = self
            .host
            .read()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let mut representations = Vec::with_capacity(snapshot.representations.len());
        let mut file_metadata = Vec::new();
        let operation_dir = self
            .import_file_representations(
                snapshot.representations,
                &mut representations,
                &mut file_metadata,
            )
            .map_err(|error| anyhow::anyhow!("host clipboard file import failed: {error}"))?;

        if !file_metadata.is_empty() {
            let encoded = FileDisplayMetadata {
                files: file_metadata,
            }
            .encode()
            .map_err(|_| anyhow::anyhow!("host clipboard metadata encoding failed"));
            match encoded {
                Ok(bytes) => representations.push(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from(FILE_DISPLAY_METADATA_FORMAT),
                    normalize_wire_mime(Some(FILE_DISPLAY_METADATA_MIME.to_string())),
                    bytes,
                )),
                Err(error) => {
                    cleanup_import_directory(operation_dir.as_deref());
                    return Err(error);
                }
            }
        }

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

impl HostClipboardAdapter {
    fn import_file_representations(
        &self,
        host_representations: Vec<HostClipboardRepresentation>,
        representations: &mut Vec<ObservedClipboardRepresentation>,
        file_metadata: &mut Vec<FileDisplayMetadataEntry>,
    ) -> anyhow::Result<Option<PathBuf>> {
        let mut operation_dir: Option<PathBuf> = None;
        for representation in host_representations {
            let result = match representation {
                HostClipboardRepresentation::Inline {
                    format,
                    mime_type,
                    bytes,
                } => {
                    representations.push(ObservedClipboardRepresentation::new(
                        RepresentationId::new(),
                        FormatId::from(format),
                        normalize_wire_mime(mime_type),
                        bytes,
                    ));
                    Ok(())
                }
                HostClipboardRepresentation::File {
                    format,
                    handle,
                    display_name,
                    mime_type,
                    size_bytes,
                } => {
                    let directory = match operation_dir.as_ref() {
                        Some(directory) => directory.clone(),
                        None => {
                            let directory =
                                self.import_root.join(RepresentationId::new().to_string());
                            std::fs::create_dir_all(&directory)?;
                            operation_dir = Some(directory.clone());
                            directory
                        }
                    };
                    let storage_name = RepresentationId::new().to_string();
                    let path = directory.join(&storage_name);
                    copy_host_clipboard_file(self.files.as_ref(), &handle, size_bytes, &path)?;
                    representations.push(ObservedClipboardRepresentation::new_local_file(
                        RepresentationId::new(),
                        FormatId::from(format),
                        normalize_wire_mime(mime_type),
                        path,
                        size_bytes,
                    ));
                    file_metadata.push(FileDisplayMetadataEntry {
                        storage_name,
                        display_name,
                    });
                    Ok(())
                }
            };
            if let Err(error) = result {
                cleanup_import_directory(operation_dir.as_deref());
                return Err(error);
            }
        }
        Ok(operation_dir)
    }
}

const HOST_CLIPBOARD_FILE_CHUNK_SIZE: u32 = 64 * 1024;

fn copy_host_clipboard_file(
    files: &dyn HostFileAccess,
    handle: &crate::HostFileHandle,
    size_bytes: u64,
    destination: &Path,
) -> anyhow::Result<()> {
    let metadata = files
        .metadata(handle)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if metadata.size_bytes != size_bytes {
        return Err(anyhow::anyhow!("host clipboard file size changed"));
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    let mut offset = 0_u64;
    while offset < size_bytes {
        let requested = (size_bytes - offset).min(HOST_CLIPBOARD_FILE_CHUNK_SIZE as u64) as u32;
        let chunk = files
            .read_chunk(handle, offset, requested)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if chunk.is_empty() || chunk.len() > requested as usize {
            return Err(anyhow::anyhow!("host clipboard file read was incomplete"));
        }
        output.write_all(&chunk)?;
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("host clipboard file offset overflow"))?;
    }
    output.sync_all()?;
    Ok(())
}

fn cleanup_import_directory(directory: Option<&Path>) {
    let Some(directory) = directory else {
        return;
    };
    if std::fs::remove_dir_all(directory).is_err() {
        warn!("failed to remove incomplete host clipboard import");
    }
}

#[cfg(test)]
pub fn adapt_system_clipboard(
    host: Box<dyn HostClipboard>,
    files: Arc<dyn HostFileAccess>,
    import_root: PathBuf,
) -> Arc<dyn SystemClipboardPort> {
    Arc::new(HostClipboardAdapter {
        host,
        files,
        import_root,
    })
}

pub fn derive_app_paths(directories: &HostDirectories) -> AppPaths {
    AppPaths::from_app_dirs(&AppDirs {
        app_data_root: directories.private_data().to_path_buf(),
        app_cache_root: directories.cache().to_path_buf(),
        app_log_dir: directories.cache().join("logs"),
    })
}

fn adapt_system_clipboard_layer(
    host: Box<dyn HostClipboard>,
    files: Arc<dyn HostFileAccess>,
    import_root: PathBuf,
) -> SystemClipboardLayer {
    let adapter = Arc::new(HostClipboardAdapter {
        host,
        files,
        import_root,
    });
    let clipboard: Arc<dyn PlatformClipboardPort> = adapter.clone();
    let system_clipboard: Arc<dyn SystemClipboardPort> = adapter;
    SystemClipboardLayer::new(clipboard, system_clipboard)
}

#[cfg(test)]
struct NoopHostEventEmitter;

#[cfg(test)]
impl HostEventEmitterPort for NoopHostEventEmitter {
    fn emit(&self, _event: HostEvent) -> Result<(), EmitError> {
        Ok(())
    }
}

pub(crate) struct EngineHostEventEmitter {
    events: EventSender,
}

impl EngineHostEventEmitter {
    pub(crate) fn new(events: EventSender) -> Self {
        Self { events }
    }
}

impl HostEventEmitterPort for EngineHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        let event = match event {
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id,
                entry_id,
                attempt_id,
                peer_id,
                direction,
                bytes_transferred,
                total_bytes,
            }) => EngineEvent::TransferProgress(TransferProgress {
                transfer_id,
                entry_id,
                attempt_id,
                peer_id,
                direction: match direction {
                    uc_core::file_transfer::FileTransferDirection::Sending => {
                        crate::TransferDirectionSummary::Sending
                    }
                    uc_core::file_transfer::FileTransferDirection::Receiving => {
                        crate::TransferDirectionSummary::Receiving
                    }
                },
                completed_bytes: bytes_transferred,
                total_bytes,
            }),
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id,
                attempt_id,
                preview,
                origin,
            }) => EngineEvent::IncomingEntry(crate::IncomingEntryEvent {
                entry_id,
                attempt_id,
                preview,
                origin: match origin {
                    ClipboardOriginKind::Local => crate::ClipboardOriginSummary::Local,
                    ClipboardOriginKind::Remote => crate::ClipboardOriginSummary::Remote,
                },
            }),
            HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id,
                entry_id,
                attempt_id,
                status,
                reason,
            }) => EngineEvent::TransferStatusChanged(crate::TransferStatusChanged {
                transfer_id,
                entry_id,
                attempt_id,
                status,
                reason,
            }),
            HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
                entry_id,
                target_device_id,
            }) => EngineEvent::DeliveryStatusChanged(crate::DeliveryStatusChanged {
                entry_id,
                target_device_id,
            }),
            HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id,
                attempt_id,
                from_device,
                total_bytes,
                filenames,
            }) => EngineEvent::IncomingPending(crate::IncomingPendingEvent {
                entry_id,
                attempt_id,
                from_device,
                total_bytes,
                filenames,
            }),
            HostEvent::Clipboard(ClipboardHostEvent::ReceiveAttemptStateChanged {
                entry_id,
                attempt_id,
                state,
            }) => EngineEvent::ReceiveAttemptStateChanged(crate::ReceiveAttemptStateChanged {
                entry_id,
                attempt_id,
                state,
            }),
        };
        self.events.send(event);
        Ok(())
    }
}

pub struct HostWiring {
    pub wired: WiredDependencies,
    pub background: BackgroundRuntimeDeps,
    pub paths: AppPaths,
    pub temporary_dir: std::path::PathBuf,
    pub clipboard_import_root: std::path::PathBuf,
    pub files: Arc<dyn HostFileAccess>,
    pub clipboard_changes: Option<Box<dyn HostClipboardChangeStream>>,
}

#[cfg(test)]
pub fn wire_host_capabilities(
    config: &EngineConfig,
    host: HostCapabilities,
) -> WiringResult<HostWiring> {
    wire_host_capabilities_with_emitter(config, host, Arc::new(NoopHostEventEmitter))
}

pub(crate) fn wire_host_capabilities_with_emitter(
    config: &EngineConfig,
    host: HostCapabilities,
    host_event_emitter: Arc<dyn HostEventEmitterPort>,
) -> WiringResult<HostWiring> {
    let (directories, secure_storage, mut clipboard, files) = host.into_parts();
    let clipboard_changes = clipboard.take_change_stream().map_err(|_| {
        WiringError::ClipboardInit("failed to open host clipboard change stream".into())
    })?;
    let paths = derive_app_paths(&directories);
    let secure_storage = adapt_secure_storage(secure_storage);
    let app_data_root = paths.app_data_root_dir.clone();
    uc_infra::config_migration::staging::apply_pending_import(
        &app_data_root,
        &paths.db_path,
        &paths.vault_dir,
        &paths.settings_path,
        &app_data_root.join("iroh-identity"),
        secure_storage.as_ref(),
    )
    .map_err(|error| WiringError::SettingsInit(error.to_string()))?;
    let temporary_dir = directories.temporary().to_path_buf();
    let clipboard_import_root = temporary_dir.join("clipboard-imports");
    if let Err(error) = std::fs::remove_dir_all(&clipboard_import_root) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(WiringError::ClipboardInit(
                "failed to clear stale host clipboard imports".into(),
            ));
        }
    }
    std::fs::create_dir_all(&clipboard_import_root).map_err(|_| {
        WiringError::ClipboardInit("failed to create host clipboard import directory".into())
    })?;
    let files: Arc<dyn HostFileAccess> = Arc::from(files);
    let (wired, background) = wire_dependencies_from_inputs(CoreWiringInputs {
        paths: paths.clone(),
        secure_storage,
        profile_id: uc_core::ids::ProfileId::from(config.profile_id()),
        app_version: config.app_version().to_string(),
        config_source_mode: if config.uses_portable_storage() {
            uc_core::ports::ConfigSourceMode::Portable
        } else {
            uc_core::ports::ConfigSourceMode::Installed
        },
        legacy_iroh_identity_dir: app_data_root.join("iroh-identity"),
        iroh_blob_store_dir: app_data_root.join("iroh-blobs"),
        system_clipboard: adapt_system_clipboard_layer(
            clipboard,
            Arc::clone(&files),
            clipboard_import_root.clone(),
        ),
        analytics_sink: Arc::new(uc_observability_contract::analytics::NoopAnalyticsSink),
        analytics_facade: Arc::new(uc_observability_contract::analytics::NoopAnalyticsFacade),
        host_event_emitter,
    })?;

    Ok(HostWiring {
        wired,
        background,
        paths,
        temporary_dir,
        clipboard_import_root,
        files,
        clipboard_changes,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uc_core::file_transfer::FileTransferDirection;
    use uc_core::ports::{
        ClipboardHostEvent, ClipboardOriginKind, DeliveryHostEvent, HostEvent,
        HostEventEmitterPort, TransferHostEvent,
    };

    use crate::engine::event_stream::event_channel;
    use crate::{
        ClipboardOriginSummary, DeliveryStatusChanged, EngineEvent, IncomingPendingEvent,
        ReceiveAttemptStateChanged, TransferDirectionSummary, TransferProgress,
        TransferStatusChanged,
    };

    use super::EngineHostEventEmitter;

    #[tokio::test]
    async fn engine_event_emitter_forwards_transfer_progress() {
        let (events, mut stream) = event_channel(8);
        let emitter: Arc<dyn HostEventEmitterPort> = Arc::new(EngineHostEventEmitter::new(events));

        emitter
            .emit(HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id: "transfer-1".into(),
                entry_id: Some("entry-1".into()),
                attempt_id: Some("attempt-1".into()),
                peer_id: "peer-1".into(),
                direction: FileTransferDirection::Receiving,
                bytes_transferred: 64,
                total_bytes: Some(128),
            }))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::TransferProgress(TransferProgress {
                transfer_id: "transfer-1".into(),
                entry_id: Some("entry-1".into()),
                attempt_id: Some("attempt-1".into()),
                peer_id: "peer-1".into(),
                direction: TransferDirectionSummary::Receiving,
                completed_bytes: 64,
                total_bytes: Some(128),
            }))
        );
    }

    #[tokio::test]
    async fn engine_event_emitter_preserves_clipboard_change_details() {
        let (events, mut stream) = event_channel(8);
        let emitter = EngineHostEventEmitter::new(events);

        emitter
            .emit(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "entry-1".into(),
                attempt_id: None,
                preview: "placeholder".into(),
                origin: ClipboardOriginKind::Remote,
            }))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::IncomingEntry(crate::IncomingEntryEvent {
                entry_id: "entry-1".into(),
                attempt_id: None,
                preview: "placeholder".into(),
                origin: ClipboardOriginSummary::Remote,
            }))
        );
    }

    #[tokio::test]
    async fn engine_event_emitter_preserves_transfer_status_details() {
        let (events, mut stream) = event_channel(8);
        let emitter = EngineHostEventEmitter::new(events);

        emitter
            .emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "transfer-1".into(),
                entry_id: "entry-1".into(),
                attempt_id: Some("attempt-1".into()),
                status: "failed".into(),
                reason: Some("cancelled".into()),
            }))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::TransferStatusChanged(TransferStatusChanged {
                transfer_id: "transfer-1".into(),
                entry_id: "entry-1".into(),
                attempt_id: Some("attempt-1".into()),
                status: "failed".into(),
                reason: Some("cancelled".into()),
            }))
        );
    }

    #[tokio::test]
    async fn engine_event_emitter_preserves_delivery_change_details() {
        let (events, mut stream) = event_channel(8);
        let emitter = EngineHostEventEmitter::new(events);

        emitter
            .emit(HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
                entry_id: "entry-1".into(),
                target_device_id: "peer-1".into(),
            }))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::DeliveryStatusChanged(DeliveryStatusChanged {
                entry_id: "entry-1".into(),
                target_device_id: "peer-1".into(),
            }))
        );
    }

    #[tokio::test]
    async fn engine_event_emitter_preserves_incoming_pending_details() {
        let (events, mut stream) = event_channel(8);
        let emitter = EngineHostEventEmitter::new(events);

        emitter
            .emit(HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id: "entry-1".into(),
                attempt_id: Some("attempt-1".into()),
                from_device: "peer-1".into(),
                total_bytes: Some(128),
                filenames: vec!["private.txt".into()],
            }))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::IncomingPending(IncomingPendingEvent {
                entry_id: "entry-1".into(),
                attempt_id: Some("attempt-1".into()),
                from_device: "peer-1".into(),
                total_bytes: Some(128),
                filenames: vec!["private.txt".into()],
            }))
        );
    }

    #[tokio::test]
    async fn engine_event_emitter_preserves_receive_attempt_state() {
        let (events, mut stream) = event_channel(8);
        let emitter = EngineHostEventEmitter::new(events);

        emitter
            .emit(HostEvent::Clipboard(
                ClipboardHostEvent::ReceiveAttemptStateChanged {
                    entry_id: "entry-1".into(),
                    attempt_id: "attempt-1".into(),
                    state: "failed".into(),
                },
            ))
            .unwrap();

        assert_eq!(
            stream.next().await,
            Some(EngineEvent::ReceiveAttemptStateChanged(
                ReceiveAttemptStateChanged {
                    entry_id: "entry-1".into(),
                    attempt_id: "attempt-1".into(),
                    state: "failed".into(),
                }
            ))
        );
    }
}
