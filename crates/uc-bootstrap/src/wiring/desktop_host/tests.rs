use super::*;
use uc_platform::clipboard::{FormatId, RepresentationId};

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

#[test]
fn engine_directories_use_the_resolved_log_directory() {
    let paths = DesktopHostPaths {
        db_path: "/host/private/uniclipboard.db".into(),
        vault_dir: "/host/private/vault".into(),
        settings_path: "/host/private/settings.json".into(),
        logs_dir: "/host/platform-logs".into(),
        cache_dir: "/host/cache".into(),
        app_data_root_dir: "/host/private".into(),
        upgrade_backups_dir: "/host/recovery".into(),
    };

    let directories = host_directories(&paths, "/host/temporary".into());

    assert_eq!(directories.logs(), Path::new("/host/platform-logs"));
    assert_eq!(directories.upgrade_backups(), Path::new("/host/recovery"));
}

struct DeniedSecureStorage;

impl SecureStorageProvider for DeniedSecureStorage {
    fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        Err(SecureStorageError::PermissionDenied("test denial".into()))
    }
    fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
        panic!("failed startup must not replace keys");
    }
    fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
        panic!("failed startup must not delete keys");
    }
}

#[tokio::test]
async fn desktop_storage_denial_leaves_an_independent_file_backup() {
    let temporary = tempfile::tempdir().unwrap();
    let private = temporary.path().join("userdata");
    let backups = temporary.path().join("recovery");
    let paths = DesktopHostPaths {
        db_path: private.join("uniclipboard.db"),
        vault_dir: private.join("vault"),
        settings_path: private.join("settings.json"),
        logs_dir: temporary.path().join("logs"),
        cache_dir: temporary.path().join("cache"),
        app_data_root_dir: private.clone(),
        upgrade_backups_dir: backups.clone(),
    };
    let original = b"desktop-settings-before-upgrade";
    std::fs::create_dir(&private).unwrap();
    std::fs::write(&paths.settings_path, original).unwrap();
    let files = DesktopHostFileHandles::default();
    let host = HostCapabilities::new(
        host_directories(&paths, temporary.path().join("temporary")),
        Box::new(DesktopSecureStorage {
            secure_storage: Arc::new(DeniedSecureStorage),
        }),
        Box::new(DesktopClipboard {
            system_clipboard: Arc::new(StaticClipboard {
                snapshot: SystemClipboardSnapshot {
                    ts_ms: 0,
                    representations: Vec::new(),
                    file_content_digests: Vec::new(),
                    file_set_v1_component: None,
                },
            }),
            file_registry: Arc::clone(&files.file_registry),
            pending_snapshot: Arc::new(Mutex::new(None)),
            change_stream_taken: false,
            changes_enabled: false,
        }),
        Box::new(files),
    );
    assert!(
        uc_engine::Engine::start(EngineConfig::new("backup-test"), host)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&paths.settings_path).unwrap(), original);
    assert!(!paths.db_path.exists());
    let namespace = std::fs::read_dir(&backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(namespace.join("current").exists());
    assert!(!namespace.join("security-current").exists());
    let archive = std::fs::read_dir(namespace)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "archive")
        })
        .unwrap();
    std::fs::remove_dir_all(&private).unwrap();
    let bytes = std::fs::read(archive).unwrap();
    assert!(bytes.windows(original.len()).any(|part| part == original));
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
    };

    let first = clipboard.read().unwrap();
    let second = clipboard.read().unwrap();
    let HostClipboardRepresentation::Inline { bytes: first, .. } = &first.representations[0] else {
        panic!("expected inline pending snapshot");
    };
    let HostClipboardRepresentation::Inline { bytes: second, .. } = &second.representations[0]
    else {
        panic!("expected inline fresh snapshot");
    };
    assert_eq!(first, b"event snapshot");
    assert_eq!(second, b"fresh snapshot");
}
