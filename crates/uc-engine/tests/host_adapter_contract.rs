use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use uc_engine::{
    Engine, EngineConfig, EngineEvent, EngineState, HostCapabilities, HostCapabilityError,
    HostClipboard, HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories,
    HostFileAccess, HostFileHandle, HostFileMetadata, HostSecureStorage,
};

#[derive(Default)]
struct MemoryHostSecureStorage {
    values: Mutex<HashMap<String, Vec<u8>>>,
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
    let temp = tempfile::tempdir().unwrap();
    let private = temp.path().join("private");
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
        Box::new(EmptyHostFiles),
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
    assert!(matches!(
        invitation,
        uc_engine::OperationResult::InvitationIssued {
            ref invitation_code
        } if !invitation_code.is_empty()
    ));

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
            EngineState::ShuttingDown,
            EngineState::Stopped,
        ]
    );
}
