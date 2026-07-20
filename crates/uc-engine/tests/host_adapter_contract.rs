use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use uc_engine::{
    Engine, EngineConfig, EngineEvent, EngineState, HostCapabilities, HostCapabilityError,
    HostClipboard, HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories,
    HostFileAccess, HostFileHandle, HostFileMetadata, HostSecureStorage,
};

static ENGINE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

#[test]
fn clipboard_adapter_preserves_inline_representation_on_read() {
    let clipboard =
        uc_engine::internal::host_adapters::adapt_system_clipboard(Box::new(StaticHostClipboard {
            snapshot: HostClipboardSnapshot {
                observed_at_ms: 42,
                representations: vec![HostClipboardRepresentation::Inline {
                    format: "public.utf8-plain-text".into(),
                    mime_type: Some("text/plain;charset=utf-8".into()),
                    bytes: vec![0, 1, 2, 255],
                }],
            },
        }));

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
    let clipboard = uc_engine::internal::host_adapters::adapt_system_clipboard(Box::new(
        RecordingHostClipboard {
            written: Arc::clone(&written),
        },
    ));
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

#[derive(Default)]
struct RecordingHostFilesState {
    writes: Mutex<Vec<(String, u64, Vec<u8>)>>,
    finished: Mutex<Vec<String>>,
}

struct RecordingHostFiles {
    state: Arc<RecordingHostFilesState>,
}

impl HostFileAccess for RecordingHostFiles {
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
    engine.suspend().await.unwrap();
    engine.resume().await.unwrap();
    let mismatch = engine
        .execute(uc_engine::Operation::CreateSpace(
            uc_engine::CreateSpaceInput {
                device_name: "Test Device".into(),
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
                device_name: "Test Device".into(),
                passphrase: uc_engine::SecretString::new("correct horse"),
                passphrase_confirmation: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        created,
        uc_engine::OperationResult::SpaceCreated { ref space_id } if !space_id.is_empty()
    ));
    let invitation = engine
        .execute(uc_engine::Operation::IssueInvitation)
        .await
        .unwrap();
    let invitation_code = match invitation {
        uc_engine::OperationResult::InvitationIssued { invitation_code } => invitation_code,
        other => panic!("expected invitation, got {other:?}"),
    };
    assert!(!invitation_code.is_empty());
    let invalid_join = engine
        .execute(uc_engine::Operation::JoinSpace(uc_engine::JoinSpaceInput {
            invitation_code,
            device_name: "  ".into(),
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
    assert_eq!(
        engine
            .execute(uc_engine::Operation::UnlockSpace(
                uc_engine::UnlockSpaceInput {
                    passphrase: uc_engine::SecretString::new("correct horse"),
                },
            ))
            .await
            .unwrap(),
        uc_engine::OperationResult::SpaceUnlocked
    );

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
        uc_engine::OperationResult::EntrySent { entry_id } => entry_id,
        other => panic!("expected sent entry, got {other:?}"),
    };
    assert!(!sent_entry_id.is_empty());
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
    assert!(matches!(
        sent_image,
        uc_engine::OperationResult::EntrySent { ref entry_id } if !entry_id.is_empty()
    ));

    let missing_resend = engine
        .execute(uc_engine::Operation::ResendEntry(
            uc_engine::ResendEntryInput {
                entry_id: "missing-entry".into(),
                target_devices: Vec::new(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(
        missing_resend.category(),
        uc_engine::EngineErrorCategory::NotFound
    );

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
async fn unlocking_a_locked_restart_recovers_keyword_search() {
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
                device_name: "Search Device".into(),
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
    restarted
        .execute(uc_engine::Operation::UnlockSpace(
            uc_engine::UnlockSpaceInput {
                passphrase: uc_engine::SecretString::new("correct horse"),
            },
        ))
        .await
        .unwrap();

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
                        device_name: "Restart Device".into(),
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
