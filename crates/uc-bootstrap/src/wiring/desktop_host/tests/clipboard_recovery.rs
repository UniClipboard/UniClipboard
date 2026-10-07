//! Real Engine startup through DesktopClipboard and DesktopHostFileHandles, with isolated OS snapshots.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uc_engine::observability::{
    DeploymentEnvironment, LocalLogConfig, ObservabilityConfig, ObservabilityResource,
    OperatingSystem, ProcessObservabilityRuntime,
};
use uc_engine::{
    ClipboardRestoreMode, CreateSpaceInput, Engine, ExportDiagnosticLogsInput, Operation,
    OperationResult, RestoreClipboardInput, SecretString, SendTextInput,
};
use uc_platform::clipboard::{FormatId, MimeType, RepresentationId};

use super::super::{
    DesktopClipboard, DesktopHostFileHandles, HostCapabilities, HostCapabilityError,
    HostDirectories, HostSecureStorage, ObservedClipboardRepresentation, SystemClipboard,
    SystemClipboardSnapshot,
};

const CHILD: &str = "UC_DESKTOP_CLIPBOARD_RECOVERY_CHILD";
const SENTINEL: &str = "private-desktop-clipboard-recovery-sentinel";

#[derive(Clone, Default)]
struct Storage(Arc<Mutex<HashMap<String, Vec<u8>>>>);
impl HostSecureStorage for Storage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.0.lock().unwrap().insert(key.into(), value.to_vec());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[derive(Clone)]
struct Platform {
    snapshot: Arc<Mutex<SystemClipboardSnapshot>>,
    denied: Arc<AtomicBool>,
}
impl SystemClipboard for Platform {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        if self.denied.load(Ordering::SeqCst) {
            return Err(anyhow::Error::new(io::Error::new(
                io::ErrorKind::PermissionDenied,
                SENTINEL,
            ))
            .context("isolated platform read"));
        }
        Ok(self.snapshot.lock().unwrap().clone())
    }
    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        *self.snapshot.lock().unwrap() = snapshot;
        Ok(())
    }
}

fn host(
    root: &Path,
    storage: &Storage,
    platform: &Platform,
    files: &DesktopHostFileHandles,
) -> HostCapabilities {
    HostCapabilities::new(
        HostDirectories::new(
            root.join("private"),
            root.join("cache"),
            root.join("temporary"),
            root.join("logs"),
        ),
        Box::new(storage.clone()),
        Box::new(DesktopClipboard {
            system_clipboard: Arc::new(platform.clone()),
            file_registry: Arc::clone(&files.file_registry),
            pending_snapshot: Arc::new(Mutex::new(None)),
            change_stream_taken: false,
            changes_enabled: false,
        }),
        Box::new(files.clone()),
    )
}

async fn start(
    root: &Path,
    storage: &Storage,
    platform: &Platform,
    files: &DesktopHostFileHandles,
) -> Engine {
    Engine::start(
        uc_engine::EngineConfig::new("2.0.0"),
        host(root, storage, platform, files),
    )
    .await
    .unwrap()
    .0
}

async fn activate(engine: &Engine) {
    let OperationResult::EntrySent(saved) = engine
        .execute(Operation::SendText(SendTextInput {
            text: SENTINEL.into(),
            target_devices: vec![],
        }))
        .await
        .unwrap()
    else {
        panic!("expected entry")
    };
    engine
        .execute(Operation::RestoreClipboard(RestoreClipboardInput {
            entry_id: saved.entry_id,
            mode: ClipboardRestoreMode::Standard,
        }))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "isolated subprocess owns the observability runtime"]
async fn desktop_clipboard_recovery_child() {
    let root = std::path::PathBuf::from(std::env::var(CHILD).unwrap());
    let runtime = ProcessObservabilityRuntime::install(
        ObservabilityConfig::new(
            ObservabilityResource::new(
                "2.0.0",
                DeploymentEnvironment::Test,
                OperatingSystem::Macos,
                "test",
            )
            .unwrap(),
        )
        .with_local_logs(LocalLogConfig::new(root.join("logs"))),
    )
    .unwrap()
    .handle();
    let storage = Storage::default();
    let platform = Platform {
        snapshot: Arc::new(Mutex::new(SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![],
            file_content_digests: vec![],
            file_set_v1_component: None,
        })),
        denied: Arc::new(AtomicBool::new(false)),
    };
    let files = DesktopHostFileHandles::default();
    let engine = start(&root, &storage, &platform, &files).await;
    engine
        .execute(Operation::CreateSpace(CreateSpaceInput {
            device_name: Some("isolated desktop host".into()),
            passphrase: SecretString::new("isolated-desktop-passphrase"),
            passphrase_confirmation: SecretString::new("isolated-desktop-passphrase"),
        }))
        .await
        .unwrap();
    activate(&engine).await;
    engine.shutdown_until_complete().await.unwrap();
    drop(engine);
    let mut cases = vec![];
    for fault in ["platform_denied", "source_missing", "source_denied"] {
        let source = root.join(SENTINEL);
        std::fs::write(&source, vec![42; 128 * 1024]).unwrap();
        platform.snapshot.lock().unwrap().representations =
            vec![ObservedClipboardRepresentation::new_local_file(
                RepresentationId::new(),
                FormatId::from("file"),
                Some(MimeType("application/octet-stream".into())),
                source.clone(),
                128 * 1024,
            )];
        match fault {
            "platform_denied" => platform.denied.store(true, Ordering::SeqCst),
            "source_missing" => std::fs::remove_file(&source).unwrap(),
            "source_denied" => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o000))
                        .unwrap();
                }
            }
            _ => unreachable!(),
        }
        let engine = start(&root, &storage, &platform, &files).await;
        assert!(matches!(
            engine
                .execute(Operation::QueryActiveClipboard)
                .await
                .unwrap(),
            OperationResult::ActiveClipboard(None)
        ));
        let import_root = root.join("temporary/clipboard-imports");
        assert!(!import_root.exists() || std::fs::read_dir(import_root).unwrap().count() == 0);
        platform.denied.store(false, Ordering::SeqCst);
        if source.exists() {
            std::fs::remove_file(source).unwrap();
        }
        activate(&engine).await;
        engine.shutdown_until_complete().await.unwrap();
        drop(engine);
        cases.push(serde_json::json!({ "fault": fault, "startup": "ready", "register": "cleared", "cleanup": "passed" }));
    }
    let engine = start(&root, &storage, &platform, &files).await;
    let archive = files.register_output(root.join("diagnostics.zip")).unwrap();
    engine
        .execute(Operation::ExportDiagnosticLogs(ExportDiagnosticLogsInput {
            since_hours: Some(24),
            destination: archive,
        }))
        .await
        .unwrap();
    engine.shutdown_until_complete().await.unwrap();
    runtime.shutdown(Duration::from_secs(2));
    std::fs::write(root.join("matrix.json"), serde_json::to_vec_pretty(&serde_json::json!({ "pid": std::process::id(), "cases": cases, "boundary": "real Desktop adapters and Engine; isolated platform snapshots, actual missing/unreadable files; no Finder/TCC proof" })).unwrap()).unwrap();
}

#[test]
fn desktop_clipboard_recovery_process() {
    let profile = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "wiring::desktop_host::tests::clipboard_recovery::desktop_clipboard_recovery_child",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD, profile.path())
        .status()
        .unwrap();
    let output = std::env::var_os("UC_TEST_ARTIFACTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/test-artifacts/desktop-clipboard-recovery")
        });
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("process.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({ "exit_code": status.code(), "passed": status.success() }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(status.success());
    for name in ["matrix.json", "diagnostics.zip"] {
        std::fs::copy(profile.path().join(name), output.join(name)).unwrap();
    }
    let logs = std::fs::read_dir(profile.path().join("logs"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(logs.contains("PermissionDenied"));
    assert!(logs.contains("NotFound"));
    assert!(logs.contains("error.chain"));
    assert!(!logs.contains(SENTINEL));
}
