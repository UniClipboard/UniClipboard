use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use uc_engine::{
    Engine, EngineConfig, EngineEvent, EngineState, HostCapabilities, HostCapabilityError,
    HostClipboard, HostClipboardChange, HostClipboardChangeStream, HostClipboardRepresentation,
    HostClipboardSnapshot, HostDirectories, HostFileAccess, HostFileHandle, HostFileMetadata,
    HostSecureStorage,
};

static ENGINE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn next_engine_event_matching(
    events: &mut uc_engine::EventStream,
    predicate: impl Fn(&EngineEvent) -> bool,
) -> EngineEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events.next().await.expect("engine event stream closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for engine event")
}

async fn drain_engine_events(events: &mut uc_engine::EventStream) {
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(1), events.next()).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return,
        }
    }
}

#[derive(Clone, Default)]
struct MemoryHostSecureStorage {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryHostSecureStorage {
    fn values(&self) -> MutexGuard<'_, HashMap<String, Vec<u8>>> {
        match self.values.lock() {
            Ok(values) => values,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl HostSecureStorage for MemoryHostSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Ok(self.values().get(key).cloned())
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.values().insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.values().remove(key);
        Ok(())
    }
}

#[test]
fn secure_storage_adapter_preserves_secret_bytes() {
    let storage = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        MemoryHostSecureStorage::default(),
    ));
    let secret = [0, 1, 2, 127, 128, 255];

    storage.set("identity", &secret).unwrap();
    assert_eq!(
        storage.get("identity").unwrap().as_deref(),
        Some(&secret[..])
    );
    storage.delete("identity").unwrap();
    assert!(storage.get("identity").unwrap().is_none());
}

struct FailingHostSecureStorage {
    category: uc_engine::HostCapabilityErrorCategory,
}

impl HostSecureStorage for FailingHostSecureStorage {
    fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }

    fn set(&self, _key: &str, _value: &[u8]) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }

    fn delete(&self, _key: &str) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }
}

#[test]
fn secure_storage_adapter_preserves_stable_error_categories() {
    use uc_core::ports::SecureStorageError;
    use uc_engine::HostCapabilityErrorCategory;

    let unavailable = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        FailingHostSecureStorage {
            category: HostCapabilityErrorCategory::Unavailable,
        },
    ));
    let denied = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        FailingHostSecureStorage {
            category: HostCapabilityErrorCategory::PermissionDenied,
        },
    ));

    assert!(matches!(
        unavailable.get("identity"),
        Err(SecureStorageError::Unavailable(_))
    ));
    assert!(matches!(
        denied.set("identity", b"secret"),
        Err(SecureStorageError::PermissionDenied(_))
    ));
}

struct StaticHostClipboard {
    snapshot: HostClipboardSnapshot,
}

impl HostClipboard for StaticHostClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Ok(self.snapshot.clone())
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Ok(())
    }
}

struct NotifyingHostClipboard {
    snapshot: HostClipboardSnapshot,
    changes: Mutex<Option<Box<dyn HostClipboardChangeStream>>>,
}

impl HostClipboard for NotifyingHostClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Ok(self.snapshot.clone())
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Ok(())
    }

    fn take_change_stream(
        &mut self,
    ) -> Result<Option<Box<dyn HostClipboardChangeStream>>, HostCapabilityError> {
        Ok(self.changes.lock().unwrap().take())
    }
}

struct ChannelClipboardChanges {
    receiver: tokio::sync::mpsc::UnboundedReceiver<()>,
    stopped: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl HostClipboardChangeStream for ChannelClipboardChanges {
    async fn next(&mut self) -> Result<HostClipboardChange, HostCapabilityError> {
        Ok(match self.receiver.recv().await {
            Some(()) => HostClipboardChange::Changed,
            None => HostClipboardChange::Closed,
        })
    }

    async fn shutdown(&mut self) -> Result<(), HostCapabilityError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn clipboard_adapter_preserves_inline_representation_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let clipboard = uc_engine::internal::host_adapters::adapt_system_clipboard(
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 42,
                representations: vec![HostClipboardRepresentation::Inline {
                    format: "public.utf8-plain-text".into(),
                    mime_type: Some("text/plain;charset=utf-8".into()),
                    bytes: vec![0, 1, 2, 255],
                }],
            },
        }),
        Arc::new(EmptyHostFiles),
        temp.path().join("clipboard-imports"),
    );

    let snapshot = clipboard.read_snapshot().unwrap();
    let representation = &snapshot.representations[0];

    assert_eq!(snapshot.ts_ms, 42);
    assert_eq!(representation.format_id.as_ref(), "public.utf8-plain-text");
    assert_eq!(
        representation.mime.as_ref().map(|mime| mime.as_str()),
        Some("text/plain;charset=utf-8")
    );
    assert_eq!(representation.inline_bytes(), Some(&[0, 1, 2, 255][..]));
}

struct RecordingHostClipboard {
    written: Arc<Mutex<Option<HostClipboardSnapshot>>>,
}

impl HostClipboard for RecordingHostClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Ok(HostClipboardSnapshot {
            observed_at_ms: 0,
            representations: Vec::new(),
        })
    }

    fn write(&self, snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        *self.written.lock().unwrap() = Some(snapshot);
        Ok(())
    }
}

#[test]
fn clipboard_adapter_preserves_inline_representation_on_write() {
    use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use uc_core::ids::{FormatId, RepresentationId};

    let written = Arc::new(Mutex::new(None));
    let temp = tempfile::tempdir().unwrap();
    let clipboard = uc_engine::internal::host_adapters::adapt_system_clipboard(
        Box::new(RecordingHostClipboard {
            written: Arc::clone(&written),
        }),
        Arc::new(EmptyHostFiles),
        temp.path().join("clipboard-imports"),
    );
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 84,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".into())),
            vec![137, 80, 78, 71],
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    clipboard.write_snapshot(snapshot).unwrap();
    let snapshot = written.lock().unwrap().clone().unwrap();

    assert_eq!(snapshot.observed_at_ms, 84);
    assert_eq!(
        snapshot.representations,
        vec![HostClipboardRepresentation::Inline {
            format: "image".into(),
            mime_type: Some("image/png".into()),
            bytes: vec![137, 80, 78, 71],
        }]
    );
}

#[test]
fn clipboard_adapter_imports_file_handles_without_exposing_the_display_name_on_disk() {
    use uc_core::clipboard::{
        ClipboardPayloadSource, FileDisplayMetadata, FILE_DISPLAY_METADATA_MIME,
    };

    let temp = tempfile::tempdir().unwrap();
    let import_root = temp.path().join("clipboard-imports");
    let display_name = "private quarterly report.txt";
    let bytes = vec![42; 70 * 1024];
    let clipboard = uc_engine::internal::host_adapters::adapt_system_clipboard(
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 91,
                representations: vec![HostClipboardRepresentation::File {
                    format: "files".into(),
                    handle: HostFileHandle::new("clipboard-file"),
                    display_name: display_name.into(),
                    mime_type: Some("application/octet-stream".into()),
                    size_bytes: bytes.len() as u64,
                }],
            },
        }),
        Arc::new(ReadableHostFiles {
            handle: "clipboard-file".into(),
            display_name: display_name.into(),
            mime_type: Some("application/octet-stream".into()),
            bytes: bytes.clone(),
            state: Arc::new(RecordingHostFilesState::default()),
        }),
        import_root.clone(),
    );

    let snapshot = clipboard.read_snapshot().unwrap();
    assert_eq!(snapshot.ts_ms, 91);
    assert_eq!(snapshot.representations.len(), 2);
    let file = &snapshot.representations[0];
    let ClipboardPayloadSource::LocalFile { path, size_bytes } = file.source() else {
        panic!("expected a local file representation");
    };
    assert_eq!(file.format_id.as_ref(), "files");
    assert_eq!(
        file.mime.as_ref().map(|mime| mime.as_str()),
        Some("application/octet-stream")
    );
    assert_eq!(*size_bytes, bytes.len() as u64);
    assert!(path.starts_with(&import_root));
    assert!(!path.to_string_lossy().contains(display_name));
    assert_eq!(std::fs::read(path).unwrap(), bytes);

    let metadata_representation = &snapshot.representations[1];
    assert_eq!(
        metadata_representation
            .mime
            .as_ref()
            .map(|mime| mime.as_str()),
        Some(FILE_DISPLAY_METADATA_MIME)
    );
    let metadata = FileDisplayMetadata::decode(
        metadata_representation
            .inline_bytes()
            .expect("display metadata must remain inline"),
    )
    .unwrap();
    let storage_name = path.file_name().unwrap().to_string_lossy();
    assert_eq!(metadata.display_name_for(&storage_name), Some(display_name));
    assert!(!format!("{snapshot:?}").contains(display_name));
}

#[tokio::test]
async fn engine_shutdown_removes_host_clipboard_imports() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    for directory in [&private, &cache, &temporary] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let bytes = b"clipboard file content".to_vec();
    let host = HostCapabilities::new(
        HostDirectories::new(private, cache, temporary.clone()),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 97,
                representations: vec![HostClipboardRepresentation::File {
                    format: "files".into(),
                    handle: HostFileHandle::new("clipboard-file"),
                    display_name: "private report.txt".into(),
                    mime_type: Some("application/octet-stream".into()),
                    size_bytes: bytes.len() as u64,
                }],
            },
        }),
        Box::new(ReadableHostFiles {
            handle: "clipboard-file".into(),
            display_name: "private report.txt".into(),
            mime_type: Some("application/octet-stream".into()),
            bytes,
            state: Arc::new(RecordingHostFilesState::default()),
        }),
    );
    let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Clipboard Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::CaptureCurrentClipboard)
            .await
            .unwrap(),
        uc_engine::OperationResult::ClipboardCaptured { entry_id: Some(_) }
    ));
    let import_root = temporary.join("clipboard-imports");
    assert!(std::fs::read_dir(&import_root).unwrap().next().is_some());

    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();

    assert!(!import_root.exists());
}

#[tokio::test]
async fn host_clipboard_change_is_processed_by_the_engine_and_stops_on_shutdown() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    for directory in [&private, &cache, &temporary] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let probe = "host clipboard change searchable probe".to_string();
    let (change_tx, change_rx) = tokio::sync::mpsc::unbounded_channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let host = HostCapabilities::new(
        HostDirectories::new(private, cache, temporary),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(NotifyingHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 101,
                representations: vec![HostClipboardRepresentation::Inline {
                    format: "text".into(),
                    mime_type: Some("text/plain".into()),
                    bytes: probe.as_bytes().to_vec(),
                }],
            },
            changes: Mutex::new(Some(Box::new(ChannelClipboardChanges {
                receiver: change_rx,
                stopped: Arc::clone(&stopped),
            }))),
        }),
        Box::new(EmptyHostFiles),
    );
    let (engine, mut events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Clipboard Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();

    change_tx.send(()).unwrap();
    let history_entry = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let result = engine
                .execute(uc_engine::Operation::QueryHistory(
                    uc_engine::QueryHistoryInput {
                        cursor: None,
                        limit: 10,
                        query: Some(probe.clone()),
                    },
                ))
                .await
                .unwrap();
            let uc_engine::OperationResult::HistoryPage { entries, .. } = result else {
                panic!("expected history page");
            };
            if let Some(entry) = entries.into_iter().next() {
                break entry;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(history_entry.preview.as_deref(), Some(probe.as_str()));

    assert_eq!(
        next_engine_event_matching(&mut events, |event| matches!(
            event,
            EngineEvent::IncomingEntry(incoming) if incoming.entry_id == history_entry.entry_id
        ))
        .await,
        EngineEvent::IncomingEntry(uc_engine::IncomingEntryEvent {
            entry_id: history_entry.entry_id,
            attempt_id: None,
            preview: "New clipboard content".into(),
            origin: uc_engine::ClipboardOriginSummary::Local,
        })
    );

    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();
    assert!(stopped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn new_engine_does_not_inherit_previous_engine_clipboard_attribution() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let first_temp = tempfile::tempdir().unwrap();
    let first = uc_engine::internal::host_adapters::wire_host_capabilities(
        &EngineConfig::new("1.2.3"),
        HostCapabilities::new(
            HostDirectories::new(
                first_temp.path().join("private"),
                first_temp.path().join("cache"),
                first_temp.path().join("temporary"),
            ),
            Box::new(MemoryHostSecureStorage::default()),
            Box::new(StaticHostClipboard {
                snapshot: HostClipboardSnapshot {
                    observed_at_ms: 0,
                    representations: Vec::new(),
                },
            }),
            Box::new(EmptyHostFiles),
        ),
    )
    .unwrap();
    first
        .wired
        .deps
        .clipboard
        .clipboard_change_origin
        .record_self_write(
            uc_core::ports::clipboard::SelfWriteMatch::ByNextChange("old-write".into()),
            uc_core::ports::clipboard::SelfWriteAttribution::Remote,
            std::time::Duration::from_secs(60),
        )
        .await;

    let second_temp = tempfile::tempdir().unwrap();
    let second = uc_engine::internal::host_adapters::wire_host_capabilities(
        &EngineConfig::new("1.2.3"),
        HostCapabilities::new(
            HostDirectories::new(
                second_temp.path().join("private"),
                second_temp.path().join("cache"),
                second_temp.path().join("temporary"),
            ),
            Box::new(MemoryHostSecureStorage::default()),
            Box::new(StaticHostClipboard {
                snapshot: HostClipboardSnapshot {
                    observed_at_ms: 0,
                    representations: Vec::new(),
                },
            }),
            Box::new(EmptyHostFiles),
        ),
    )
    .unwrap();
    let origin = second
        .wired
        .deps
        .clipboard
        .clipboard_change_origin
        .attribute_observed_change("fresh-local-copy")
        .await;

    assert_eq!(origin, uc_core::ClipboardChangeOrigin::LocalCapture);
}

#[test]
fn host_directories_derive_only_private_and_cache_storage_paths() {
    let directories = HostDirectories::new(
        "/host/private".into(),
        "/host/cache".into(),
        "/host/temporary".into(),
    );

    let paths = uc_engine::internal::host_adapters::derive_app_paths(&directories);

    assert_eq!(
        paths.db_path,
        std::path::Path::new("/host/private/uniclipboard.db")
    );
    assert_eq!(paths.vault_dir, std::path::Path::new("/host/private/vault"));
    assert_eq!(
        paths.settings_path,
        std::path::Path::new("/host/private/settings.json")
    );
    assert_eq!(
        paths.file_cache_dir,
        std::path::Path::new("/host/private/file-cache")
    );
    assert_eq!(paths.cache_dir, std::path::Path::new("/host/cache"));
    assert_eq!(paths.spool_dir, std::path::Path::new("/host/cache/spool"));
}

#[tokio::test]
async fn engine_platform_uses_the_configured_profile() {
    let profile = uc_engine::internal::platform::current_profile_for("mobile-primary");

    assert_eq!(
        profile.current_profile().await.unwrap().as_ref(),
        "mobile-primary"
    );
}

struct EmptyHostFiles;

impl HostFileAccess for EmptyHostFiles {
    fn metadata(&self, _handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        Err(HostCapabilityError::new(
            uc_engine::HostCapabilityErrorCategory::InvalidHandle,
            "missing",
        ))
    }

    fn read_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        Ok(Vec::new())
    }

    fn write_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        Ok(())
    }

    fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        Ok(())
    }
}

struct ReadableHostFiles {
    handle: String,
    display_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
    state: Arc<RecordingHostFilesState>,
}

impl HostFileAccess for ReadableHostFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        if handle.as_str() != self.handle {
            return Err(HostCapabilityError::new(
                uc_engine::HostCapabilityErrorCategory::InvalidHandle,
                "missing",
            ));
        }
        Ok(HostFileMetadata {
            display_name: self.display_name.clone(),
            size_bytes: self.bytes.len() as u64,
            mime_type: self.mime_type.clone(),
        })
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        if handle.as_str() != self.handle {
            return Err(HostCapabilityError::new(
                uc_engine::HostCapabilityErrorCategory::InvalidHandle,
                "missing",
            ));
        }
        let start = usize::try_from(offset).map_err(|_| {
            HostCapabilityError::new(uc_engine::HostCapabilityErrorCategory::Io, "offset")
        })?;
        if start >= self.bytes.len() {
            return Ok(Vec::new());
        }
        let end = start
            .saturating_add(max_bytes as usize)
            .min(self.bytes.len());
        Ok(self.bytes[start..end].to_vec())
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        self.state.writes.lock().unwrap().push((
            handle.as_str().to_string(),
            offset,
            bytes.to_vec(),
        ));
        Ok(())
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        self.state
            .finished
            .lock()
            .unwrap()
            .push(handle.as_str().to_string());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingHostFilesState {
    writes: Mutex<Vec<(String, u64, Vec<u8>)>>,
    finished: Mutex<Vec<String>>,
    contents: Mutex<HashMap<String, Vec<u8>>>,
}

struct RecordingHostFiles {
    state: Arc<RecordingHostFilesState>,
}

impl HostFileAccess for RecordingHostFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        let contents = self.state.contents.lock().unwrap();
        let bytes = contents.get(handle.as_str()).ok_or_else(|| {
            HostCapabilityError::new(
                uc_engine::HostCapabilityErrorCategory::InvalidHandle,
                "missing",
            )
        })?;
        Ok(HostFileMetadata {
            display_name: "opaque-host-file".into(),
            size_bytes: bytes.len() as u64,
            mime_type: None,
        })
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        let contents = self.state.contents.lock().unwrap();
        let bytes = contents.get(handle.as_str()).ok_or_else(|| {
            HostCapabilityError::new(
                uc_engine::HostCapabilityErrorCategory::InvalidHandle,
                "missing",
            )
        })?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = start.saturating_add(max_bytes as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        self.state.writes.lock().unwrap().push((
            handle.as_str().to_string(),
            offset,
            bytes.to_vec(),
        ));
        let mut contents = self.state.contents.lock().unwrap();
        let output = contents.entry(handle.as_str().to_string()).or_default();
        if output.len() as u64 != offset {
            return Err(HostCapabilityError::new(
                uc_engine::HostCapabilityErrorCategory::Io,
                "non-sequential test write",
            ));
        }
        output.extend_from_slice(bytes);
        Ok(())
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        self.state
            .finished
            .lock()
            .unwrap()
            .push(handle.as_str().to_string());
        Ok(())
    }
}

#[tokio::test]
async fn host_capabilities_wire_real_core_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    let host = HostCapabilities::new(
        HostDirectories::new(private.clone(), cache, temporary),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );

    let wiring = uc_engine::internal::host_adapters::wire_host_capabilities(
        &EngineConfig::new("1.2.3").with_profile_id("mobile-primary"),
        host,
    )
    .unwrap();

    assert_eq!(wiring.paths.app_data_root_dir, private);
    assert_eq!(
        wiring
            .wired
            .deps
            .security
            .current_profile
            .current_profile()
            .await
            .unwrap()
            .as_ref(),
        "mobile-primary"
    );
}

#[tokio::test]
async fn engine_start_builds_a_resumable_real_session() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let host_files = Arc::new(RecordingHostFilesState::default());
    let host = HostCapabilities::new(
        HostDirectories::new(
            private.clone(),
            temp.path().join("cache"),
            temp.path().join("temporary"),
        ),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(RecordingHostFiles {
            state: Arc::clone(&host_files),
        }),
    );

    let (engine, mut events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();

    assert!(private.join("uniclipboard.db").is_file());
    assert_eq!(
        events.next().await,
        Some(EngineEvent::StateChanged {
            state: EngineState::Running,
        })
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ListDevices)
            .await
            .unwrap(),
        uc_engine::OperationResult::Devices(Vec::new())
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ListMobileDevices)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileDevices(Vec::new())
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::RevokeMobileDevice(
                uc_engine::MobileDeviceInput {
                    device_id: "missing-mobile-device".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileDeviceRevoked(
            uc_engine::MobileDeviceRevokeOutcome::NotFound,
        )
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::AuthenticateMobileRequest(
                uc_engine::AuthenticateMobileRequestInput {
                    authorization: uc_engine::SecretString::new("invalid authorization"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileAuthentication(
            uc_engine::MobileAuthenticationOutcome::Rejected,
        )
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::RevalidateMobileCredential(
                uc_engine::RevalidateMobileCredentialInput {
                    credential: uc_engine::MobileCredential::new(
                        "missing-mobile-device",
                        "missing-password-proof",
                    ),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileCredentialCurrent { current: false }
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::UpdateMobileSyncSettings(Box::new(
                uc_engine::MobileSyncSettingsPatch {
                    lan_port: Some(Some(0)),
                    ..Default::default()
                },
            )))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncSettingsUpdated(
            uc_engine::MobileSyncSettingsUpdateOutcome::Rejected { .. }
        )
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryMobileSyncSettings)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncSettings(ref settings)
            if !settings.enabled && !settings.lan_listen_enabled
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::UpdateMobileSyncSettings(Box::new(
                uc_engine::MobileSyncSettingsPatch {
                    enabled: Some(true),
                    lan_listen_enabled: Some(true),
                    ..Default::default()
                },
            )))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncSettingsUpdated(
            uc_engine::MobileSyncSettingsUpdateOutcome::Updated(ref settings)
        ) if settings.enabled && settings.lan_listen_enabled && settings.changed
    ));
    assert_eq!(
        next_engine_event_matching(&mut events, |event| {
            matches!(event, EngineEvent::MobileLanSettingsChanged(_))
        })
        .await,
        EngineEvent::MobileLanSettingsChanged(uc_engine::MobileLanSettingsChanged {
            enabled: true,
            lan_listen_enabled: true,
            lan_port: None,
        })
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::UpdateMobileSyncSettings(Box::new(
                uc_engine::MobileSyncSettingsPatch {
                    enabled: Some(true),
                    lan_listen_enabled: Some(true),
                    ..Default::default()
                },
            )))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncSettingsUpdated(
            uc_engine::MobileSyncSettingsUpdateOutcome::Updated(ref settings)
        ) if settings.enabled && settings.lan_listen_enabled && !settings.changed
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryMobileSyncSettings)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncSettings(ref settings)
            if settings.enabled && settings.lan_listen_enabled
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::UpdateMobileLanEndpoint(
                uc_engine::MobileLanEndpointUpdate::Listening {
                    base_url: "http://127.0.0.1:42720".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileLanEndpointUpdated
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::RegisterMobileDevice(
                uc_engine::RegisterMobileDeviceInput {
                    label: "".into(),
                    username: None,
                    password: None,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileDeviceRegistered(
            uc_engine::MobileDeviceRegistrationOutcome::LabelEmpty,
        )
    );
    let registered = engine
        .execute(uc_engine::Operation::RegisterMobileDevice(
            uc_engine::RegisterMobileDeviceInput {
                label: "Test Phone".into(),
                username: Some("test_phone".into()),
                password: Some(uc_engine::SecretString::new("test-password")),
            },
        ))
        .await
        .unwrap();
    let registered_device_id = match registered {
        uc_engine::OperationResult::MobileDeviceRegistered(
            uc_engine::MobileDeviceRegistrationOutcome::Registered(registration),
        ) => {
            assert_eq!(registration.label, "Test Phone");
            assert_eq!(registration.username, "test_phone");
            assert_eq!(registration.password.expose(), "test-password");
            registration.device_id
        }
        other => panic!("expected registered mobile device, got {other:?}"),
    };
    let updated = engine
        .execute(uc_engine::Operation::UpdateMobileDevice(
            uc_engine::UpdateMobileDeviceInput {
                device_id: registered_device_id.clone(),
                label: Some("Renamed Phone".into()),
                username: None,
                password: uc_engine::MobilePasswordUpdate::AutoGenerate,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        updated,
        uc_engine::OperationResult::MobileDeviceUpdated(
            uc_engine::MobileDeviceUpdateOutcome::Updated(ref update)
        ) if update.device_id == registered_device_id
            && update.label == "Renamed Phone"
            && update.username == "test_phone"
            && update.password.is_some()
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ListMobileDevices)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileDevices(ref devices)
            if devices.len() == 1
                && devices[0].device_id == registered_device_id
                && devices[0].label == "Renamed Phone"
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ExportConfig(
                uc_engine::ExportConfigInput {
                    destination: HostFileHandle::new("uninitialized-config"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::ConfigExport(uc_engine::ConfigExportOutcome::NotInitialized,)
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryDiagnostics)
            .await
            .unwrap(),
        uc_engine::OperationResult::DiagnosticsStatus(uc_engine::DiagnosticsStatusSummary {
            debug_mode: false,
            restart_required: false,
            ..
        })
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::UpdateDebugMode(
                uc_engine::UpdateDebugModeInput { enabled: true },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::DebugModeUpdated(uc_engine::DebugModeUpdateSummary {
            debug_mode: true,
            restart_required: true,
        })
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryDiagnostics)
            .await
            .unwrap(),
        uc_engine::OperationResult::DiagnosticsStatus(uc_engine::DiagnosticsStatusSummary {
            debug_mode: true,
            restart_required: false,
            ..
        })
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ExportDiagnosticLogs(
                uc_engine::ExportDiagnosticLogsInput {
                    since_hours: Some(1),
                    destination: HostFileHandle::new("diagnostic-logs"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::DiagnosticLogsExported(_)
    ));
    assert!(host_files
        .writes
        .lock()
        .unwrap()
        .iter()
        .any(|(handle, _, bytes)| handle == "diagnostic-logs" && !bytes.is_empty()));
    assert!(host_files
        .finished
        .lock()
        .unwrap()
        .contains(&"diagnostic-logs".to_string()));
    host_files.writes.lock().unwrap().clear();
    host_files.finished.lock().unwrap().clear();
    engine.suspend().await.unwrap();
    engine.resume().await.unwrap();
    let mismatch = engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Test Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("different phrase"),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        mismatch.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );

    let created = engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Test Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        created,
        uc_engine::OperationResult::SpaceCreated {
            ref space_id,
            ref self_device_id,
            ref identity_fingerprint,
        } if !space_id.is_empty()
            && !self_device_id.is_empty()
            && !identity_fingerprint.is_empty()
    ));
    let self_device_id = match &created {
        uc_engine::OperationResult::SpaceCreated { self_device_id, .. } => self_device_id.clone(),
        other => panic!("expected created space, got {other:?}"),
    };
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ExportConfig(
                uc_engine::ExportConfigInput {
                    destination: HostFileHandle::new("config-bundle"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::ConfigExport(uc_engine::ConfigExportOutcome::Exported)
    );
    let preview = engine
        .execute(uc_engine::Operation::PreviewConfigImport(
            uc_engine::PreviewConfigImportInput {
                source: HostFileHandle::new("config-bundle"),
                password: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        preview,
        uc_engine::OperationResult::ConfigImportPreview(
            uc_engine::ConfigImportPreviewOutcome::Ready(
                uc_engine::ConfigImportPreviewSummary {
                    ref app_version,
                    ref source_mode,
                    ref profile_id,
                    ref device_fingerprint,
                    ..
                }
            )
        ) if app_version == "1.2.3"
            && matches!(
                source_mode,
                uc_engine::ConfigSourceModeSummary::Portable
                    | uc_engine::ConfigSourceModeSummary::Installed
            )
            && !profile_id.is_empty()
            && !device_fingerprint.is_empty()
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::PreviewConfigImport(
                uc_engine::PreviewConfigImportInput {
                    source: HostFileHandle::new("config-bundle"),
                    password: uc_engine::SecretString::new("wrong password"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::ConfigImportPreview(
            uc_engine::ConfigImportPreviewOutcome::InvalidPasswordOrCorrupt,
        )
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::StageConfigImport(
                uc_engine::StageConfigImportInput {
                    source: HostFileHandle::new("config-bundle"),
                    password: uc_engine::SecretString::new("correct horse"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::ConfigImportStaged(
            uc_engine::ConfigImportStageOutcome::Staged { .. }
        )
    ));
    host_files.writes.lock().unwrap().clear();
    host_files.finished.lock().unwrap().clear();
    host_files.contents.lock().unwrap().clear();
    let initial_preferences = engine
        .execute(uc_engine::Operation::QueryMemberSyncPreferences(
            uc_engine::QueryMemberSyncPreferencesInput {
                device_id: self_device_id.clone(),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        initial_preferences,
        uc_engine::OperationResult::MemberSyncPreferences(
            uc_engine::MemberSyncPreferencesSummary {
                send_enabled: true,
                receive_enabled: true,
                ..
            }
        )
    ));
    let updated_preferences = engine
        .execute(uc_engine::Operation::UpdateMemberSyncPreferences(
            uc_engine::UpdateMemberSyncPreferencesInput {
                device_id: self_device_id.clone(),
                patch: uc_engine::MemberSyncPreferencesPatch {
                    send_enabled: Some(false),
                    send_content_types: Some(uc_engine::ContentTypesPatch {
                        text: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        updated_preferences,
        uc_engine::OperationResult::MemberSyncPreferences(
            uc_engine::MemberSyncPreferencesSummary {
                send_enabled: false,
                receive_enabled: true,
                send_content_types: uc_engine::ContentTypesSummary {
                    text: false,
                    image: true,
                    ..
                },
                ..
            }
        )
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryEncryptionState)
            .await
            .unwrap(),
        uc_engine::OperationResult::EncryptionState(uc_engine::EncryptionStateSummary {
            initialized: true,
            session_ready: true,
        })
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::CaptureCurrentClipboard)
            .await
            .unwrap(),
        uc_engine::OperationResult::ClipboardCaptured { entry_id: None }
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::VerifySecureStorageAccess)
            .await
            .unwrap(),
        uc_engine::OperationResult::SecureStorageAccess { granted: true }
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::LockEncryption)
            .await
            .unwrap(),
        uc_engine::OperationResult::EncryptionLocked
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryEncryptionState)
            .await
            .unwrap(),
        uc_engine::OperationResult::EncryptionState(uc_engine::EncryptionStateSummary {
            initialized: true,
            session_ready: false,
        })
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::UnlockSpace(
                uc_engine::UnlockSpaceInput {
                    passphrase: uc_engine::SecretString::new("correct horse"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::SpaceUnlocked { .. }
    ));
    let invitation = engine
        .execute(uc_engine::Operation::IssueInvitation)
        .await
        .unwrap();
    let invitation_code = match invitation {
        uc_engine::OperationResult::InvitationIssued {
            invitation_code,
            expires_at_ms,
            ..
        } => {
            assert!(
                expires_at_ms > 0,
                "invitation expiry must come from the engine"
            );
            invitation_code
        }
        other => panic!("expected invitation, got {other:?}"),
    };
    assert!(!invitation_code.is_empty());
    let invalid_join = engine
        .execute(uc_engine::Operation::JoinSpace(uc_engine::JoinSpaceInput {
            invitation_code,
            device_name: Some("  ".into()),
            passphrase: uc_engine::SecretString::new("correct horse"),
        }))
        .await
        .unwrap_err();
    assert_eq!(
        invalid_join.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );

    let wrong_passphrase = engine
        .execute(uc_engine::Operation::UnlockSpace(
            uc_engine::UnlockSpaceInput {
                passphrase: uc_engine::SecretString::new("wrong phrase"),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        wrong_passphrase.category(),
        uc_engine::EngineErrorCategory::Unauthorized
    );
    let unlocked = engine
        .execute(uc_engine::Operation::UnlockSpace(
            uc_engine::UnlockSpaceInput {
                passphrase: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::SpaceUnlocked { space_id } = unlocked else {
        panic!("expected unlocked space, got {unlocked:?}");
    };
    assert!(!space_id.is_empty(), "unlocked space id must be returned");

    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryHistory(
                uc_engine::QueryHistoryInput {
                    cursor: None,
                    limit: 25,
                    query: None,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryPage {
            entries: Vec::new(),
            next_cursor: None,
        }
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryEntryReceiveProgress(
                uc_engine::EntryReceiveProgressInput {
                    entry_id: "missing-receive".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryReceiveProgress(None)
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ListEntryReceiveProgress)
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryReceiveProgressList(Vec::new())
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::CancelEntryReceive(
                uc_engine::CancelEntryReceiveInput {
                    entry_id: "missing-receive".into(),
                    attempt_id: "attempt-1".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryReceiveCancellation(
            uc_engine::EntryReceiveCancellationOutcome::NotReceiving,
        )
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::CancelInboundTransfer(
                uc_engine::CancelInboundTransferInput {
                    transfer_id: "missing-transfer".into(),
                    reason: uc_engine::TransferCancellationReason::LocalUser,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::InboundTransferCancellation(
            uc_engine::InboundTransferCancellationOutcome::NotInflight,
        )
    );
    let invalid_cursor = engine
        .execute(uc_engine::Operation::QueryHistory(
            uc_engine::QueryHistoryInput {
                cursor: Some("not-an-engine-cursor".into()),
                limit: 25,
                query: None,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        invalid_cursor.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );

    let empty_text = engine
        .execute(uc_engine::Operation::SendText(uc_engine::SendTextInput {
            text: String::new(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(
        empty_text.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );
    let oversized_text = engine
        .execute(uc_engine::Operation::SendText(uc_engine::SendTextInput {
            text: "x".repeat(64 * 1024 + 1),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(
        oversized_text.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );
    let sent = engine
        .execute(uc_engine::Operation::SendText(uc_engine::SendTextInput {
            text: "engine text dispatch".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();
    let sent_entry_id = match sent {
        uc_engine::OperationResult::EntrySent(report) => report.entry_id,
        other => panic!("expected sent entry, got {other:?}"),
    };
    assert!(!sent_entry_id.is_empty());
    let listed = engine
        .execute(uc_engine::Operation::ListHistoryEntries(
            uc_engine::ListHistoryEntriesInput {
                limit: 50,
                offset: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        listed,
        uc_engine::OperationResult::HistoryEntries(ref entries)
            if entries.len() == 1
                && entries[0].entry_id == sent_entry_id
                && entries[0].preview == "engine text dispatch"
                && !entries[0].is_favorited
    ));
    let detail = engine
        .execute(uc_engine::Operation::GetHistoryEntry(
            uc_engine::HistoryEntryInput {
                entry_id: sent_entry_id.clone(),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        detail,
        uc_engine::OperationResult::HistoryEntry(ref entry)
            if entry.entry_id == sent_entry_id && entry.content == "engine text dispatch"
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryEntryDelivery(
                uc_engine::HistoryEntryInput {
                    entry_id: sent_entry_id.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryDelivery(uc_engine::EntryDeliveryViewSummary {
            entry_id: sent_entry_id.clone(),
            source: uc_engine::EntrySourceSummary::Local,
            deliveries: Vec::new(),
        })
    );
    let missing_delivery = engine
        .execute(uc_engine::Operation::QueryEntryDelivery(
            uc_engine::HistoryEntryInput {
                entry_id: "missing-delivery".into(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_delivery.category(),
        uc_engine::EngineErrorCategory::NotFound
    );
    for mode in [
        uc_engine::ClipboardRestoreMode::Standard,
        uc_engine::ClipboardRestoreMode::PlainText,
    ] {
        assert_eq!(
            engine
                .execute(uc_engine::Operation::RestoreClipboard(
                    uc_engine::RestoreClipboardInput {
                        entry_id: sent_entry_id.clone(),
                        mode,
                    },
                ))
                .await
                .unwrap(),
            uc_engine::OperationResult::ClipboardRestored(
                uc_engine::ClipboardRestoreOutcome::Restored,
            )
        );
    }
    assert_eq!(
        engine
            .execute(uc_engine::Operation::RestoreClipboard(
                uc_engine::RestoreClipboardInput {
                    entry_id: sent_entry_id.clone(),
                    mode: uc_engine::ClipboardRestoreMode::FilePaths,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::ClipboardRestored(
            uc_engine::ClipboardRestoreOutcome::NotApplicable {
                reason: "entry has no restorable file paths".into(),
            },
        )
    );
    let missing_restore = engine
        .execute(uc_engine::Operation::RestoreClipboard(
            uc_engine::RestoreClipboardInput {
                entry_id: "missing-restore".into(),
                mode: uc_engine::ClipboardRestoreMode::Standard,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_restore.category(),
        uc_engine::EngineErrorCategory::NotFound
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::SetHistoryEntryFavorite(
                uc_engine::SetHistoryEntryFavoriteInput {
                    entry_id: sent_entry_id.clone(),
                    is_favorited: true,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryEntryFavoriteSet
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryHistoryStats)
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryStats(uc_engine::HistoryStatsSummary {
            total_items: 1,
            total_size,
        }) if total_size > 0
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::GetHistoryEntryResource(
                uc_engine::HistoryEntryInput {
                    entry_id: sent_entry_id.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryEntryResource(
            uc_engine::HistoryEntryResourceSummary {
                inline_data: Some(ref bytes),
                ..
            }
        ) if bytes == b"engine text dispatch"
    ));
    let search_page = engine
        .execute(uc_engine::Operation::SearchEntries(
            uc_engine::SearchEntriesInput {
                query: "engine text dispatch".into(),
                operator: None,
                time_preset: None,
                from_ms: None,
                to_ms: None,
                content_types: None,
                extensions: None,
                source_devices: None,
                tags: None,
                limit: 25,
                offset: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        search_page,
        uc_engine::OperationResult::SearchPage(uc_engine::SearchPageSummary {
            total: 1,
            has_more: false,
            ref items,
            ref state,
        }) if state == "ready"
            && items.len() == 1
            && items[0].entry_id == sent_entry_id
            && items[0].text_preview.as_deref() == Some("engine text dispatch")
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QuerySearchTags)
            .await
            .unwrap(),
        uc_engine::OperationResult::SearchTags(_)
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QuerySearchStatus)
            .await
            .unwrap(),
        uc_engine::OperationResult::SearchStatus(uc_engine::SearchStatusSummary {
            ref state,
            ..
        }) if state == "ready"
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ExportEntry(
                uc_engine::ExportEntryInput {
                    entry_id: sent_entry_id.clone(),
                    destination: HostFileHandle::new("export-text"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryExported
    );
    assert_eq!(
        *host_files.writes.lock().unwrap(),
        vec![(
            "export-text".to_string(),
            0,
            b"engine text dispatch".to_vec(),
        )]
    );
    assert_eq!(
        *host_files.finished.lock().unwrap(),
        vec!["export-text".to_string()]
    );
    let missing_export = engine
        .execute(uc_engine::Operation::ExportEntry(
            uc_engine::ExportEntryInput {
                entry_id: "missing-export".into(),
                destination: HostFileHandle::new("missing-export-target"),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_export.category(),
        uc_engine::EngineErrorCategory::NotFound
    );

    let empty_image = engine
        .execute(uc_engine::Operation::SendImage(uc_engine::SendImageInput {
            bytes: Vec::new(),
            mime_type: "image/png".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(
        empty_image.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );
    let oversized_image = engine
        .execute(uc_engine::Operation::SendImage(uc_engine::SendImageInput {
            bytes: vec![0; 64 * 1024 + 1],
            mime_type: "image/png".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(
        oversized_image.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );
    let sent_image = engine
        .execute(uc_engine::Operation::SendImage(uc_engine::SendImageInput {
            bytes: vec![137, 80, 78, 71],
            mime_type: "image/png".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();
    let sent_image_id = match sent_image {
        uc_engine::OperationResult::EntrySent(report) => report.entry_id,
        other => panic!("expected sent image, got {other:?}"),
    };
    assert_eq!(
        engine
            .execute(uc_engine::Operation::DeleteHistoryEntry(
                uc_engine::HistoryEntryInput {
                    entry_id: sent_image_id.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryEntryDeleted
    );
    let missing_delete = engine
        .execute(uc_engine::Operation::DeleteHistoryEntry(
            uc_engine::HistoryEntryInput {
                entry_id: sent_image_id,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_delete.category(),
        uc_engine::EngineErrorCategory::NotFound
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ClearHistory)
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryCleared(uc_engine::HistoryClearSummary {
            deleted_count: 1,
            ref failed_entry_ids,
        }) if failed_entry_ids.is_empty()
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryHistoryStats)
            .await
            .unwrap(),
        uc_engine::OperationResult::HistoryStats(uc_engine::HistoryStatsSummary {
            total_items: 0,
            total_size: 0,
        })
    );

    assert_eq!(
        engine
            .execute(uc_engine::Operation::ResendEntry(
                uc_engine::ResendEntryInput {
                    entry_id: "missing-entry".into(),
                    target_devices: Vec::new(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryResent(uc_engine::ResendEntryOutcome::EntryNotFound {
            entry_id: "missing-entry".into(),
        },)
    );

    assert_eq!(
        engine
            .execute(uc_engine::Operation::RemoveMember(
                uc_engine::RemoveMemberInput {
                    device_id: self_device_id.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MemberRemoved
    );
    let missing_member = engine
        .execute(uc_engine::Operation::RemoveMember(
            uc_engine::RemoveMemberInput {
                device_id: self_device_id,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_member.category(),
        uc_engine::EngineErrorCategory::NotFound
    );

    assert_eq!(
        engine
            .execute(uc_engine::Operation::FactoryResetSpace)
            .await
            .unwrap(),
        uc_engine::OperationResult::SpaceFactoryReset
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryEncryptionState)
            .await
            .unwrap(),
        uc_engine::OperationResult::EncryptionState(uc_engine::EncryptionStateSummary {
            initialized: false,
            session_ready: false,
        })
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QuerySetupState)
            .await
            .unwrap(),
        uc_engine::OperationResult::SetupState(uc_engine::SetupStateSummary {
            has_completed: false,
            current_invitation: None,
            ..
        })
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::CreateSpace(
                uc_engine::CreateSpaceInput {
                    device_name: Some("Reset Device".into()),
                    passphrase: uc_engine::SecretString::new("new correct horse"),
                    passphrase_confirmation: uc_engine::SecretString::new("new correct horse"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::SpaceCreated { .. }
    ));

    engine.suspend().await.unwrap();
    engine.resume().await.unwrap();
    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();

    let mut states = Vec::new();
    while let Some(event) = events.next().await {
        if let EngineEvent::StateChanged { state } = event {
            states.push(state);
        }
    }
    assert_eq!(
        states,
        vec![
            EngineState::Quiescing,
            EngineState::Quiesced,
            EngineState::Suspended,
            EngineState::Running,
            EngineState::Quiescing,
            EngineState::Quiesced,
            EngineState::Suspended,
            EngineState::Running,
            EngineState::Quiescing,
            EngineState::Quiesced,
            EngineState::ShuttingDown,
            EngineState::Stopped,
        ]
    );
}

#[tokio::test]
async fn engine_mobile_content_round_trips_and_drops_uploads_on_suspend() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let host = HostCapabilities::new(
        HostDirectories::new(
            temp.path().join("private"),
            temp.path().join("cache"),
            temp.path().join("temporary"),
        ),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );
    let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Mobile Content Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();

    assert_eq!(
        engine
            .execute(uc_engine::Operation::QueryLatestMobileSyncDocument)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncDocument(None)
    );
    let empty_hash = engine
        .execute(uc_engine::Operation::CheckMobileContentAvailable(
            uc_engine::MobileContentAvailabilityInput {
                snapshot_hash: "  ".into(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        empty_hash.category(),
        uc_engine::EngineErrorCategory::InvalidInput
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::CheckMobileContentAvailable(
                uc_engine::MobileContentAvailabilityInput {
                    snapshot_hash: "blake3v1:missing".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileContentAvailability { available: false }
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ReadMobileSyncFile(
                uc_engine::ReadMobileSyncFileInput {
                    data_name: "missing.bin".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncFile(uc_engine::MobileSyncFileReadOutcome::NotFound,)
    );

    let applied = engine
        .execute(uc_engine::Operation::ApplyMobileSyncDocument(Box::new(
            uc_engine::ApplyMobileSyncDocumentInput {
                document: uc_engine::MobileSyncDocument {
                    item_type: uc_engine::MobileSyncItemType::Text,
                    text: "mobile engine text".into(),
                    data_name: None,
                    has_data: false,
                    size: 18,
                    hash: None,
                    content_id: None,
                },
                source_device_id: "mobile-source".into(),
            },
        )))
        .await
        .unwrap();
    let text_content_id = match applied {
        uc_engine::OperationResult::MobileSyncDocumentApplied(
            uc_engine::MobileSyncDocumentApplyOutcome::Applied { content_id, .. },
        ) => content_id,
        other => panic!("expected applied mobile text, got {other:?}"),
    };
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::QueryLatestMobileSyncDocument)
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncDocument(Some(ref document))
            if document.item_type == uc_engine::MobileSyncItemType::Text
                && document.text == "mobile engine text"
                && document.content_id.as_deref() == Some(text_content_id.as_str())
    ));
    assert_eq!(
        engine
            .execute(uc_engine::Operation::CheckMobileContentAvailable(
                uc_engine::MobileContentAvailabilityInput {
                    snapshot_hash: text_content_id,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileContentAvailability { available: true }
    );

    let upload = engine
        .execute(uc_engine::Operation::BeginMobileFileUpload(
            uc_engine::BeginMobileFileUploadInput {
                data_name: "mobile-file.txt".into(),
                media_type: "application/octet-stream".into(),
                source_device_id: "mobile-source".into(),
                transfer_id: "mobile-transfer-1".into(),
                total_bytes: Some(19),
            },
        ))
        .await
        .unwrap();
    let upload = match upload {
        uc_engine::OperationResult::MobileFileUploadStarted(handle) => handle,
        other => panic!("expected upload handle, got {other:?}"),
    };
    assert_eq!(format!("{upload:?}"), "MobileFileUploadHandle([REDACTED])");
    for bytes in [b"mobile file ".to_vec(), b"payload".to_vec()] {
        assert_eq!(
            engine
                .execute(uc_engine::Operation::AppendMobileFileUpload(
                    uc_engine::AppendMobileFileUploadInput {
                        handle: upload.clone(),
                        bytes,
                    },
                ))
                .await
                .unwrap(),
            uc_engine::OperationResult::MobileFileUploadChunkAppended
        );
    }
    assert_eq!(
        engine
            .execute(uc_engine::Operation::FinishMobileFileUpload(
                uc_engine::FinishMobileFileUploadInput {
                    handle: upload,
                    media_type: "text/plain".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileFileUploadFinished(
            uc_engine::MobileSyncDocumentApplyOutcome::Buffered,
        )
    );
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ApplyMobileSyncDocument(Box::new(
                uc_engine::ApplyMobileSyncDocumentInput {
                    document: uc_engine::MobileSyncDocument {
                        item_type: uc_engine::MobileSyncItemType::File,
                        text: "mobile-file.txt".into(),
                        data_name: Some("mobile-file.txt".into()),
                        has_data: true,
                        size: 19,
                        hash: None,
                        content_id: None,
                    },
                    source_device_id: "mobile-source".into(),
                },
            )))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncDocumentApplied(
            uc_engine::MobileSyncDocumentApplyOutcome::Applied { .. }
        )
    ));
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ReadMobileSyncFile(
                uc_engine::ReadMobileSyncFileInput {
                    data_name: "mobile-file.txt".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileSyncFile(
            uc_engine::MobileSyncFileReadOutcome::Found(ref file)
        ) if file.media_type == "application/octet-stream"
            && file.bytes == b"mobile file payload"
    ));

    let aborted = engine
        .execute(uc_engine::Operation::BeginMobileFileUpload(
            uc_engine::BeginMobileFileUploadInput {
                data_name: "aborted.bin".into(),
                media_type: "application/octet-stream".into(),
                source_device_id: "mobile-source".into(),
                transfer_id: "mobile-transfer-aborted".into(),
                total_bytes: None,
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::MobileFileUploadStarted(aborted) = aborted else {
        panic!("expected abort upload handle");
    };
    assert_eq!(
        engine
            .execute(uc_engine::Operation::AbortMobileFileUpload(
                uc_engine::AbortMobileFileUploadInput {
                    handle: aborted.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileFileUploadAborted { existed: true }
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::AbortMobileFileUpload(
                uc_engine::AbortMobileFileUploadInput { handle: aborted },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileFileUploadAborted { existed: false }
    );

    let stale = engine
        .execute(uc_engine::Operation::BeginMobileFileUpload(
            uc_engine::BeginMobileFileUploadInput {
                data_name: "stale.bin".into(),
                media_type: "application/octet-stream".into(),
                source_device_id: "mobile-source".into(),
                transfer_id: "mobile-transfer-stale".into(),
                total_bytes: None,
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::MobileFileUploadStarted(stale) = stale else {
        panic!("expected stale upload handle");
    };
    engine.suspend().await.unwrap();
    engine.resume().await.unwrap();
    let stale_error = engine
        .execute(uc_engine::Operation::AppendMobileFileUpload(
            uc_engine::AppendMobileFileUploadInput {
                handle: stale,
                bytes: b"must not resume".to_vec(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        stale_error.category(),
        uc_engine::EngineErrorCategory::NotFound
    );
    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();
}

#[tokio::test]
async fn engine_mobile_upload_owns_transfer_lifecycle_events() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let host = HostCapabilities::new(
        HostDirectories::new(
            temp.path().join("private"),
            temp.path().join("cache"),
            temp.path().join("temporary"),
        ),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );
    let (engine, mut events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Mobile Upload Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    drain_engine_events(&mut events).await;

    let upload = engine
        .execute(uc_engine::Operation::BeginMobileFileUpload(
            uc_engine::BeginMobileFileUploadInput {
                data_name: "lifecycle.bin".into(),
                media_type: "application/octet-stream".into(),
                source_device_id: "mobile-source".into(),
                transfer_id: "mobile-transfer-lifecycle".into(),
                total_bytes: Some(3),
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::MobileFileUploadStarted(upload) = upload else {
        panic!("expected upload handle");
    };
    assert_eq!(
        next_engine_event_matching(&mut events, |event| matches!(
            event,
            EngineEvent::TransferProgress(progress)
                if progress.transfer_id == "mobile-transfer-lifecycle"
        ))
        .await,
        EngineEvent::TransferProgress(uc_engine::TransferProgress {
            transfer_id: "mobile-transfer-lifecycle".into(),
            entry_id: None,
            attempt_id: None,
            peer_id: "mobile:mobile-source".into(),
            direction: uc_engine::TransferDirectionSummary::Receiving,
            completed_bytes: 0,
            total_bytes: Some(3),
        })
    );

    tokio::time::sleep(std::time::Duration::from_millis(260)).await;
    engine
        .execute(uc_engine::Operation::AppendMobileFileUpload(
            uc_engine::AppendMobileFileUploadInput {
                handle: upload.clone(),
                bytes: b"abc".to_vec(),
            },
        ))
        .await
        .unwrap();
    assert_eq!(
        next_engine_event_matching(&mut events, |event| matches!(
            event,
            EngineEvent::TransferProgress(progress)
                if progress.transfer_id == "mobile-transfer-lifecycle"
        ))
        .await,
        EngineEvent::TransferProgress(uc_engine::TransferProgress {
            transfer_id: "mobile-transfer-lifecycle".into(),
            entry_id: None,
            attempt_id: None,
            peer_id: "mobile:mobile-source".into(),
            direction: uc_engine::TransferDirectionSummary::Receiving,
            completed_bytes: 3,
            total_bytes: Some(3),
        })
    );

    assert_eq!(
        engine
            .execute(uc_engine::Operation::FinishMobileFileUpload(
                uc_engine::FinishMobileFileUploadInput {
                    handle: upload,
                    media_type: "application/octet-stream".into(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileFileUploadFinished(
            uc_engine::MobileSyncDocumentApplyOutcome::Buffered,
        )
    );
    assert_eq!(
        next_engine_event_matching(&mut events, |event| matches!(
            event,
            EngineEvent::TransferProgress(progress)
                if progress.transfer_id == "mobile-transfer-lifecycle"
        ))
        .await,
        EngineEvent::TransferProgress(uc_engine::TransferProgress {
            transfer_id: "mobile-transfer-lifecycle".into(),
            entry_id: None,
            attempt_id: None,
            peer_id: "mobile:mobile-source".into(),
            direction: uc_engine::TransferDirectionSummary::Receiving,
            completed_bytes: 3,
            total_bytes: Some(3),
        })
    );

    let aborted = engine
        .execute(uc_engine::Operation::BeginMobileFileUpload(
            uc_engine::BeginMobileFileUploadInput {
                data_name: "aborted-lifecycle.bin".into(),
                media_type: "application/octet-stream".into(),
                source_device_id: "mobile-source".into(),
                transfer_id: "mobile-transfer-aborted-lifecycle".into(),
                total_bytes: None,
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::MobileFileUploadStarted(aborted) = aborted else {
        panic!("expected aborted upload handle");
    };
    assert_eq!(
        next_engine_event_matching(&mut events, |event| matches!(
            event,
            EngineEvent::TransferProgress(progress)
                if progress.transfer_id == "mobile-transfer-aborted-lifecycle"
        ))
        .await,
        EngineEvent::TransferProgress(uc_engine::TransferProgress {
            transfer_id: "mobile-transfer-aborted-lifecycle".into(),
            entry_id: None,
            attempt_id: None,
            peer_id: "mobile:mobile-source".into(),
            direction: uc_engine::TransferDirectionSummary::Receiving,
            completed_bytes: 0,
            total_bytes: None,
        })
    );
    assert_eq!(
        engine
            .execute(uc_engine::Operation::AbortMobileFileUpload(
                uc_engine::AbortMobileFileUploadInput { handle: aborted },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::MobileFileUploadAborted { existed: true }
    );

    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();
}

#[tokio::test]
async fn engine_send_files_imports_opaque_content_and_exports_after_resume() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    for directory in [&private, &cache, &temporary] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let display_name = "uc-sensitive-filename-probe.txt";
    let file_bytes = b"host file payload survives import and resend".to_vec();
    let host_files = Arc::new(RecordingHostFilesState::default());
    let host = HostCapabilities::new(
        HostDirectories::new(private.clone(), cache.clone(), temporary.clone()),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(ReadableHostFiles {
            handle: "picked-file".into(),
            display_name: display_name.into(),
            mime_type: Some("text/plain".into()),
            bytes: file_bytes.clone(),
            state: Arc::clone(&host_files),
        }),
    );
    let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("File Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();

    let sent = engine
        .execute(uc_engine::Operation::SendFiles(uc_engine::SendFilesInput {
            files: vec![HostFileHandle::new("picked-file")],
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();
    let entry_id = match sent {
        uc_engine::OperationResult::EntrySent(report) => report.entry_id,
        other => panic!("expected sent file entry, got {other:?}"),
    };
    assert!(matches!(
        engine
            .execute(uc_engine::Operation::ReadEntryFile(
                uc_engine::HistoryEntryInput {
                    entry_id: entry_id.clone(),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryFileRead(uc_engine::EntryFileResourceSummary {
            bytes,
            file_name,
            ..
        }) if bytes == file_bytes && !file_name.is_empty()
    ));
    let history = engine
        .execute(uc_engine::Operation::QueryHistory(
            uc_engine::QueryHistoryInput {
                cursor: None,
                limit: 10,
                query: None,
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::HistoryPage { entries, .. } = history else {
        panic!("expected history page");
    };
    assert!(
        entries
            .iter()
            .any(|entry| entry.preview.as_deref() == Some(display_name)),
        "the encrypted history must retain the host display name"
    );
    engine.suspend().await.unwrap();
    engine.resume().await.unwrap();
    assert_eq!(
        engine
            .execute(uc_engine::Operation::ExportEntry(
                uc_engine::ExportEntryInput {
                    entry_id,
                    destination: HostFileHandle::new("exported-file"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::EntryExported
    );
    assert_eq!(
        *host_files.writes.lock().unwrap(),
        vec![("exported-file".to_string(), 0, file_bytes.clone())]
    );
    assert_eq!(
        *host_files.finished.lock().unwrap(),
        vec!["exported-file".to_string()]
    );
    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();

    let mut imported_content_found = false;
    let mut pending = vec![private.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_name().to_string_lossy().contains(display_name));
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else if std::fs::read(entry.path()).is_ok_and(|bytes| {
                bytes
                    .windows(file_bytes.len())
                    .any(|part| part == file_bytes)
            }) {
                imported_content_found = true;
            }
        }
    }
    assert!(
        imported_content_found,
        "the imported file bytes were not retained"
    );

    let probe_file = temp.path().join("filename-probe.txt");
    std::fs::write(&probe_file, display_name).unwrap();
    let scanner = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/security/scan-plaintext-probe.sh");
    let output = std::process::Command::new("bash")
        .arg(scanner)
        .arg(probe_file)
        .args([private, cache, temporary])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "filename probe found plaintext: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[tokio::test]
async fn recovering_a_locked_restart_from_secure_storage_restores_keyword_search() {
    use diesel::connection::SimpleConnection;
    use diesel::Connection;

    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let secure_storage = MemoryHostSecureStorage::default();
    let host = HostCapabilities::new(
        HostDirectories::new(
            private.clone(),
            temp.path().join("cache"),
            temp.path().join("temporary"),
        ),
        Box::new(secure_storage.clone()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );
    let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Search Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::SendText(uc_engine::SendTextInput {
            text: "recoverable keyword".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();
    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();

    let database_path = private.join("uniclipboard.db");
    let mut connection = diesel::sqlite::SqliteConnection::establish(
        database_path.to_str().expect("database path must be UTF-8"),
    )
    .unwrap();
    connection
        .batch_execute("UPDATE search_index_meta SET index_version = 'stale', search_blocked = 1;")
        .unwrap();
    drop(connection);

    let restarted_host = HostCapabilities::new(
        HostDirectories::new(
            private,
            temp.path().join("cache"),
            temp.path().join("temporary"),
        ),
        Box::new(secure_storage),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );
    let (restarted, _events) = Engine::start(EngineConfig::new("1.2.3"), restarted_host)
        .await
        .unwrap();
    assert_eq!(
        restarted
            .execute(uc_engine::Operation::RecoverSession(
                uc_engine::RecoverSessionInput {
                    allow_secure_storage_unlock: true,
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::SessionRecovered {
            unlocked: true,
            resumed: true,
        }
    );

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match restarted
            .execute(uc_engine::Operation::QueryHistory(
                uc_engine::QueryHistoryInput {
                    cursor: None,
                    limit: 25,
                    query: Some("recoverable".into()),
                },
            ))
            .await
        {
            Ok(uc_engine::OperationResult::HistoryPage { entries, .. }) => {
                assert_eq!(entries.len(), 1);
                break;
            }
            Err(error)
                if error.category() == uc_engine::EngineErrorCategory::Unavailable
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            other => panic!("keyword search did not recover after unlock: {other:?}"),
        }
    }

    restarted
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();
}

#[tokio::test]
async fn production_engine_restarts_ten_times_with_the_same_network_identity() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    let secure_storage = MemoryHostSecureStorage::default();
    let mut expected_identity = None;

    for cycle in 0..10 {
        let host = HostCapabilities::new(
            HostDirectories::new(private.clone(), cache.clone(), temporary.clone()),
            Box::new(secure_storage.clone()),
            Box::new(StaticHostClipboard {
                snapshot: HostClipboardSnapshot {
                    observed_at_ms: cycle,
                    representations: Vec::new(),
                },
            }),
            Box::new(EmptyHostFiles),
        );
        let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
            .await
            .unwrap_or_else(|error| panic!("engine start failed on cycle {cycle}: {error}"));

        if cycle == 0 {
            engine
                .execute(uc_engine::Operation::CreateSpace(
                    uc_engine::CreateSpaceInput {
                        device_name: Some("Restart Device".into()),
                        passphrase: uc_engine::SecretString::new("correct horse"),
                        passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
                    },
                ))
                .await
                .unwrap();
        } else {
            engine
                .execute(uc_engine::Operation::UnlockSpace(
                    uc_engine::UnlockSpaceInput {
                        passphrase: uc_engine::SecretString::new("correct horse"),
                    },
                ))
                .await
                .unwrap();
        }

        let identity = secure_storage
            .values()
            .get(uc_infra::network::iroh::IDENTITY_STORE_KEY)
            .cloned()
            .expect("network identity must be persisted in secure storage");
        match &expected_identity {
            Some(expected) => assert_eq!(identity, *expected, "identity changed on cycle {cycle}"),
            None => expected_identity = Some(identity),
        }

        engine
            .shutdown(std::time::Duration::from_secs(15))
            .await
            .unwrap_or_else(|error| panic!("engine shutdown failed on cycle {cycle}: {error}"));
    }
}

#[tokio::test]
async fn persisted_engine_text_image_preview_and_logs_do_not_leave_plaintext_on_disk() {
    let _guard = ENGINE_TEST_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
    let cache = temp.path().join("cache");
    let temporary = temp.path().join("temporary");
    let logs = temp.path().join("logs");
    for directory in [&private, &cache, &temporary, &logs] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let log_guard =
        uc_observability::init_tracing_subscriber(&logs, uc_observability::LogProfile::Cli)
            .unwrap();

    let probe = format!(
        "uc-plaintext-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let probe_file = temp.path().join("probe.txt");
    std::fs::write(&probe_file, &probe).unwrap();

    let host = HostCapabilities::new(
        HostDirectories::new(private.clone(), cache.clone(), temporary.clone()),
        Box::new(MemoryHostSecureStorage::default()),
        Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 0,
                representations: Vec::new(),
            },
        }),
        Box::new(EmptyHostFiles),
    );
    let (engine, _events) = Engine::start(EngineConfig::new("1.2.3"), host)
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: Some("Probe Device".into()),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::SendText(uc_engine::SendTextInput {
            text: format!("private payload {probe}"),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();
    engine
        .execute(uc_engine::Operation::SendImage(uc_engine::SendImageInput {
            bytes: probe.as_bytes().to_vec(),
            mime_type: "image/png".into(),
            target_devices: Vec::new(),
        }))
        .await
        .unwrap();

    let history = engine
        .execute(uc_engine::Operation::QueryHistory(
            uc_engine::QueryHistoryInput {
                cursor: None,
                limit: 25,
                query: None,
            },
        ))
        .await
        .unwrap();
    let uc_engine::OperationResult::HistoryPage { entries, .. } = history else {
        panic!("history query returned the wrong result");
    };
    assert!(
        entries
            .iter()
            .filter_map(|entry| entry.preview.as_deref())
            .any(|preview| preview.contains(&probe)),
        "the probe must reach the generated preview before persistence is scanned"
    );

    engine
        .shutdown(std::time::Duration::from_secs(15))
        .await
        .unwrap();
    drop(log_guard);

    let scanner = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/security/scan-plaintext-probe.sh");
    let output = std::process::Command::new("bash")
        .arg(scanner)
        .arg(&probe_file)
        .args([&private, &cache, &temporary, &logs])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "plaintext scan failed: {stderr}");
    assert!(!stdout.contains(&probe));
    assert!(!stderr.contains(&probe));
}
