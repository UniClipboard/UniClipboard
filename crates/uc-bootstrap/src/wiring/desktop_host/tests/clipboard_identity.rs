//! Boundary E2E: platform observation -> watcher channel -> DesktopClipboard -> Engine host API.

use super::*;
use uc_engine::HostClipboardIdentity;
use uc_platform::clipboard::ClipboardObservation;

struct VersionedClipboard(Mutex<ClipboardObservation>);

impl SystemClipboard for VersionedClipboard {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        Ok(self.0.lock().unwrap().0.clone())
    }

    fn read_observation(&self) -> anyhow::Result<ClipboardObservation> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        self.write_with_receipt(snapshot).map(|_| ())
    }

    fn write_with_receipt(
        &self,
        snapshot: SystemClipboardSnapshot,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        *self.0.lock().unwrap() = (snapshot, Some(vec![42]));
        Ok(Some(vec![42]))
    }
}

#[tokio::test]
async fn watcher_host_identity_e2e() {
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            "text".into(),
            Some(uc_platform::clipboard::MimeType::text_plain()),
            b"identity fixture".to_vec(),
        )],
        file_content_digests: vec![],
        file_set_v1_component: None,
    };
    let platform = Arc::new(VersionedClipboard(Mutex::new((
        snapshot.clone(),
        Some(vec![1]),
    ))));
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let mut watcher = ClipboardWatcher::new(platform.clone(), sender);
    let pending_snapshot = Arc::new(Mutex::new(None));
    let (shutdown, _) = shutdown_channel();
    let mut changes = DesktopClipboardChanges {
        system_clipboard: platform.clone(),
        pending_snapshot: pending_snapshot.clone(),
        running: Some(RunningDesktopClipboardChanges {
            receiver,
            shutdown,
            join: tokio::spawn(async { Ok(()) }),
        }),
    };
    let clipboard = DesktopClipboard {
        system_clipboard: platform.clone(),
        file_registry: Arc::new(DesktopFileRegistry::default()),
        pending_snapshot,
        changes_enabled: false,
        change_stream_taken: false,
    };
    watcher.notify_change();
    // Ownership changes before the queued snapshot reaches Engine.
    platform.0.lock().unwrap().1 = Some(vec![2]);
    assert_eq!(changes.next().await.unwrap(), HostClipboardChange::Changed);
    let (_, identity) = clipboard.read_with_identity().unwrap();
    assert_eq!(identity, Some(HostClipboardIdentity(vec![1])));
    let (current, identity) = clipboard.read_with_identity().unwrap();
    assert_eq!(identity, Some(HostClipboardIdentity(vec![2])));
    let receipt = clipboard.write_with_receipt(current).unwrap();
    assert_eq!(receipt.identity, Some(HostClipboardIdentity(vec![42])));
    let (_, identity) = clipboard.read_with_identity().unwrap();
    assert_eq!(identity, receipt.identity);
    changes.shutdown().await.unwrap();
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-artifacts/inbound-file-loop");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("host.txt"),
        "PASS: platform -> watcher channel -> desktop host identity; queued snapshot retains original identity after ownership change; write receipt reaches Engine host API.\nNOT RUN: paired-device transport and native GUI.\n").unwrap();
}
