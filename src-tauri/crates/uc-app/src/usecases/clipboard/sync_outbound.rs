use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use futures::executor;
use tracing::{debug, info, info_span, warn, Instrument};
use uuid::Uuid;

use uc_core::network::protocol::ClipboardTextPayloadV1;
use uc_core::network::{ClipboardMessage, ProtocolMessage};
use uc_core::ports::{
    DeviceIdentityPort, EncryptionPort, EncryptionSessionPort, NetworkPort, SettingsPort,
    SystemClipboardPort,
};
use uc_core::security::{aad, model::EncryptionAlgo};
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

pub struct SyncOutboundClipboardUseCase {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    network: Arc<dyn NetworkPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    encryption: Arc<dyn EncryptionPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
}

impl SyncOutboundClipboardUseCase {
    pub fn new(
        local_clipboard: Arc<dyn SystemClipboardPort>,
        network: Arc<dyn NetworkPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        encryption: Arc<dyn EncryptionPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
    ) -> Self {
        Self {
            local_clipboard,
            network,
            encryption_session,
            encryption,
            device_identity,
            settings,
        }
    }

    pub fn execute_current_snapshot(&self, origin: ClipboardChangeOrigin) -> Result<()> {
        let snapshot = self
            .local_clipboard
            .read_snapshot()
            .context("failed to read current clipboard snapshot for outbound sync")?;
        self.execute(snapshot, origin)
    }

    pub fn execute(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
    ) -> Result<()> {
        let span = info_span!(
            "usecase.clipboard.sync_outbound.execute",
            origin = ?origin,
            representation_count = snapshot.representations.len(),
        );

        executor::block_on(self.execute_async(snapshot, origin).instrument(span))
    }

    async fn execute_async(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
    ) -> Result<()> {
        if origin == ClipboardChangeOrigin::RemotePush {
            debug!(origin = ?origin, "Skipping outbound sync for remote-push origin");
            return Ok(());
        }

        if !self.encryption_session.is_ready().await {
            info!(origin = ?origin, "Skipping outbound sync because encryption session is not ready");
            return Ok(());
        }

        let selected_representation = match snapshot.representations.iter().find(|rep| {
            rep.mime
                .as_ref()
                .is_some_and(|mime| is_text_plain_mime(mime.as_str()))
        }) {
            Some(rep) => rep,
            None => {
                debug!(
                    representation_count = snapshot.representations.len(),
                    "Skipping outbound sync because no text/plain representation is available"
                );
                return Ok(());
            }
        };

        let plaintext_text = match std::str::from_utf8(&selected_representation.bytes) {
            Ok(text) => text.to_string(),
            Err(err) => {
                warn!(
                    error = %err,
                    payload_bytes = selected_representation.bytes.len(),
                    "Skipping outbound sync because selected text/plain representation is not valid UTF-8"
                );
                return Ok(());
            }
        };

        let sendable_peers = self
            .network
            .list_sendable_peers()
            .await
            .context("failed to load sendable peers for outbound sync")?;
        let discovered_peer_count = match self.network.get_discovered_peers().await {
            Ok(peers) => peers.len(),
            Err(err) => {
                warn!(
                    error = %err,
                    "get_discovered_peers failed during outbound clipboard peer evaluation"
                );
                0
            }
        };
        info!(
            discovered_peer_count,
            sendable_peer_count = sendable_peers.len(),
            "Evaluated outbound clipboard sendable peers"
        );
        if sendable_peers.is_empty() {
            info!("Skipping outbound sync because there are no sendable peers");
            return Ok(());
        }

        let message_id = Uuid::new_v4().to_string();
        let payload = ClipboardTextPayloadV1::new(plaintext_text, snapshot.ts_ms);
        let payload_bytes = serde_json::to_vec(&payload)
            .context("failed to serialize clipboard text payload for outbound sync")?;

        let master_key = self
            .encryption_session
            .get_master_key()
            .await
            .map_err(anyhow::Error::from)
            .context("failed to access encryption session master key for outbound sync")?;

        let encrypted_blob = self
            .encryption
            .encrypt_blob(
                &master_key,
                &payload_bytes,
                &aad::for_network_clipboard(&message_id),
                EncryptionAlgo::XChaCha20Poly1305,
            )
            .await
            .map_err(anyhow::Error::from)
            .context("failed to encrypt outbound clipboard payload")?;

        let encrypted_content = serde_json::to_vec(&encrypted_blob)
            .context("failed to serialize encrypted outbound clipboard payload")?;

        let origin_device_id = self.device_identity.current_device_id().to_string();
        let origin_device_name = match self.settings.load().await {
            Ok(settings) => settings
                .general
                .device_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "Unknown Device".to_string()),
            Err(err) => {
                warn!(
                    error = %err,
                    "Failed to load settings for outbound sync; using fallback device name"
                );
                "Unknown Device".to_string()
            }
        };

        let clipboard_message = ClipboardMessage {
            id: message_id,
            content_hash: selected_representation.content_hash().to_string(),
            encrypted_content,
            timestamp: Utc::now(),
            origin_device_id,
            origin_device_name,
        };

        let outbound_bytes = ProtocolMessage::Clipboard(clipboard_message)
            .to_bytes()
            .context("failed to serialize outbound protocol clipboard message")?;

        let mut send_failures = Vec::new();
        let mut connect_failures = Vec::new();
        let mut connect_success_count = 0usize;
        let mut sent_count = 0usize;

        for peer in sendable_peers {
            if let Err(err) = self.network.ensure_business_path(&peer.peer_id).await {
                warn!(
                    peer_id = %peer.peer_id,
                    error = %err,
                    "failed to ensure outbound business path; skipping send for this peer"
                );
                connect_failures.push(format!("{}: {}", peer.peer_id, err));
                continue;
            }
            connect_success_count += 1;

            if let Err(err) = self
                .network
                .send_clipboard(&peer.peer_id, outbound_bytes.clone())
                .await
            {
                warn!(
                    peer_id = %peer.peer_id,
                    error = %err,
                    "failed to send outbound clipboard message to peer; continuing best-effort fanout"
                );
                send_failures.push(format!("{}: {}", peer.peer_id, err));
                continue;
            }

            sent_count += 1;
        }

        if sent_count == 0 {
            let mut failures = Vec::new();
            failures.extend(connect_failures);
            failures.extend(send_failures);
            return Err(anyhow::anyhow!(
                "outbound clipboard fanout failed: 0 sent, {} failed ({})",
                failures.len(),
                failures.join(" | ")
            ));
        }

        if !connect_failures.is_empty() || !send_failures.is_empty() {
            let mut failures = Vec::new();
            failures.extend(connect_failures);
            failures.extend(send_failures);
            warn!(
                sent_count,
                failure_count = failures.len(),
                "outbound clipboard fanout partially failed after best-effort retries"
            );
            info!(
                sent_count,
                connect_success_count, "Outbound clipboard sync sent to sendable peers (partial)"
            );
            return Ok(());
        }

        info!(
            sent_count,
            connect_success_count, "Outbound clipboard sync sent to sendable peers"
        );
        Ok(())
    }
}

fn is_text_plain_mime(mime: &str) -> bool {
    let normalized = mime.trim();
    let text_plain_with_params = format!("{};", ClipboardTextPayloadV1::MIME_TEXT_PLAIN);
    normalized.eq_ignore_ascii_case(ClipboardTextPayloadV1::MIME_TEXT_PLAIN)
        || normalized
            .to_ascii_lowercase()
            .starts_with(text_plain_with_params.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use tokio::sync::mpsc;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::network::protocol::ClipboardTextPayloadV1;
    use uc_core::network::{
        ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage,
        ProtocolMessage,
    };
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfParams, Kek,
        MasterKey, Passphrase,
    };
    use uc_core::settings::model::Settings;
    use uc_core::{DeviceId, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

    struct TestSystemClipboard {
        snapshot: SystemClipboardSnapshot,
    }

    impl SystemClipboardPort for TestSystemClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(self.snapshot.clone())
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct TestNetwork {
        sendable_peers: Vec<DiscoveredPeer>,
        failing_peers: HashSet<String>,
        ensure_failing_peers: HashSet<String>,
        send_calls: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
        list_sendable_peers_calls: Arc<AtomicUsize>,
        ensure_business_path_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl NetworkPort for TestNetwork {
        async fn send_clipboard(
            &self,
            peer_id: &str,
            encrypted_data: Vec<u8>,
        ) -> anyhow::Result<()> {
            if self.failing_peers.contains(peer_id) {
                return Err(anyhow::anyhow!("simulated send failure for {peer_id}"));
            }

            self.send_calls
                .lock()
                .expect("send calls lock")
                .push((peer_id.to_string(), encrypted_data));
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(&self) -> anyhow::Result<mpsc::Receiver<ClipboardMessage>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }

        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(Vec::new())
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(Vec::new())
        }

        async fn list_sendable_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            self.list_sendable_peers_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(self.sendable_peers.clone())
        }

        async fn ensure_business_path(&self, peer_id: &str) -> anyhow::Result<()> {
            self.ensure_business_path_calls
                .fetch_add(1, Ordering::SeqCst);
            if self.ensure_failing_peers.contains(peer_id) {
                return Err(anyhow::anyhow!(
                    "simulated ensure business path failure for {peer_id}"
                ));
            }
            Ok(())
        }

        fn local_peer_id(&self) -> String {
            "peer-local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(
            &self,
            _session_id: String,
            _message: PairingMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn close_pairing_session(
            &self,
            _session_id: String,
            _reason: Option<String>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unpair_device(&self, _peer_id: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_events(&self) -> anyhow::Result<mpsc::Receiver<NetworkEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    struct TestEncryptionSession {
        ready: bool,
    }

    #[async_trait]
    impl EncryptionSessionPort for TestEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.ready
        }

        async fn get_master_key(&self) -> std::result::Result<MasterKey, EncryptionError> {
            Ok(MasterKey([7; 32]))
        }

        async fn set_master_key(
            &self,
            _master_key: MasterKey,
        ) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct TestEncryption {
        encrypt_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EncryptionPort for TestEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf: &KdfParams,
        ) -> std::result::Result<Kek, EncryptionError> {
            Err(EncryptionError::UnsupportedKdfAlgorithm)
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> std::result::Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::EncryptFailed)
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _wrapped: &EncryptedBlob,
        ) -> std::result::Result<MasterKey, EncryptionError> {
            Err(EncryptionError::WrongPassphrase)
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            plaintext: &[u8],
            _aad: &[u8],
            _aead: EncryptionAlgo,
        ) -> std::result::Result<EncryptedBlob, EncryptionError> {
            self.encrypt_calls.fetch_add(1, Ordering::SeqCst);
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![1; 24],
                ciphertext: plaintext.to_vec(),
                aad_fingerprint: None,
            })
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _encrypted: &EncryptedBlob,
            _aad: &[u8],
        ) -> std::result::Result<Vec<u8>, EncryptionError> {
            Err(EncryptionError::CorruptedBlob)
        }
    }

    struct TestDeviceIdentity;

    impl DeviceIdentityPort for TestDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device-1")
        }
    }

    struct TestSettings {
        settings: Settings,
    }

    #[async_trait]
    impl SettingsPort for TestSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.settings.clone())
        }

        async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn build_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![ObservedClipboardRepresentation {
                id: RepresentationId::new(),
                format_id: FormatId::from("public.utf8-plain-text"),
                mime: Some(MimeType::text_plain()),
                bytes: b"hello world".to_vec(),
            }],
        }
    }

    fn build_usecase(
        connected_peers: Vec<ConnectedPeer>,
        encryption_ready: bool,
        failing_peers: &[&str],
        ensure_failing_peers: &[&str],
    ) -> (
        SyncOutboundClipboardUseCase,
        Arc<Mutex<Vec<(String, Vec<u8>)>>>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let send_calls = Arc::new(Mutex::new(Vec::new()));
        let list_sendable_peers_calls = Arc::new(AtomicUsize::new(0));
        let ensure_business_path_calls = Arc::new(AtomicUsize::new(0));
        let encrypt_calls = Arc::new(AtomicUsize::new(0));
        let sendable_peers = connected_peers
            .iter()
            .map(|peer| DiscoveredPeer {
                peer_id: peer.peer_id.clone(),
                device_name: Some(peer.device_name.clone()),
                device_id: None,
                addresses: Vec::new(),
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: true,
            })
            .collect();

        let usecase = SyncOutboundClipboardUseCase::new(
            Arc::new(TestSystemClipboard {
                snapshot: build_snapshot(),
            }),
            Arc::new(TestNetwork {
                sendable_peers,
                failing_peers: failing_peers
                    .iter()
                    .map(|peer| (*peer).to_string())
                    .collect(),
                ensure_failing_peers: ensure_failing_peers
                    .iter()
                    .map(|peer| (*peer).to_string())
                    .collect(),
                send_calls: send_calls.clone(),
                list_sendable_peers_calls: list_sendable_peers_calls.clone(),
                ensure_business_path_calls: ensure_business_path_calls.clone(),
            }),
            Arc::new(TestEncryptionSession {
                ready: encryption_ready,
            }),
            Arc::new(TestEncryption {
                encrypt_calls: encrypt_calls.clone(),
            }),
            Arc::new(TestDeviceIdentity),
            Arc::new(TestSettings {
                settings: Settings::default(),
            }),
        );

        (
            usecase,
            send_calls,
            list_sendable_peers_calls,
            ensure_business_path_calls,
            encrypt_calls,
        )
    }

    #[test]
    fn sends_exactly_once_for_local_capture_when_peer_exists() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
                connected_at: Utc::now(),
            }],
            true,
            &[],
            &[],
        );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect("execute local capture");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 1);
    }

    #[test]
    fn does_not_send_for_remote_push() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
                connected_at: Utc::now(),
            }],
            true,
            &[],
            &[],
        );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::RemotePush)
            .expect("remote push should no-op");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
    }

    #[test]
    fn sends_for_local_restore() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
                connected_at: Utc::now(),
            }],
            true,
            &[],
            &[],
        );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalRestore)
            .expect("local restore should fan out");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 1);
    }

    #[test]
    fn no_op_when_encryption_session_not_ready() {
        let (usecase, send_calls, list_sendable_peers_calls, ensure_calls, encrypt_calls) =
            build_usecase(
                vec![ConnectedPeer {
                    peer_id: "peer-1".to_string(),
                    device_name: "Desk".to_string(),
                    connected_at: Utc::now(),
                }],
                false,
                &[],
                &[],
            );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect("execute should no-op");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
        assert_eq!(list_sendable_peers_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 0);
        assert_eq!(encrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn execute_current_snapshot_reads_from_clipboard() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
                connected_at: Utc::now(),
            }],
            true,
            &[],
            &[],
        );

        usecase
            .execute_current_snapshot(ClipboardChangeOrigin::LocalCapture)
            .expect("execute current snapshot");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 1);
    }

    #[test]
    fn outbound_bytes_decode_as_protocol_message_clipboard() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
                connected_at: Utc::now(),
            }],
            true,
            &[],
            &[],
        );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect("execute local capture");

        let calls = send_calls.lock().expect("send calls lock");
        let (_, outbound_bytes) = calls.first().expect("one outbound send");
        let decoded = ProtocolMessage::from_bytes(outbound_bytes).expect("decode protocol message");

        match decoded {
            ProtocolMessage::Clipboard(message) => {
                let encrypted_blob: EncryptedBlob =
                    serde_json::from_slice(&message.encrypted_content)
                        .expect("decode encrypted blob");
                let payload: ClipboardTextPayloadV1 =
                    serde_json::from_slice(&encrypted_blob.ciphertext).expect("decode payload");
                assert_eq!(payload.text, "hello world");
            }
            _ => panic!("expected ProtocolMessage::Clipboard"),
        }
    }

    #[test]
    fn continues_sending_to_other_peers_after_single_peer_failure() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![
                ConnectedPeer {
                    peer_id: "peer-1".to_string(),
                    device_name: "Desk".to_string(),
                    connected_at: Utc::now(),
                },
                ConnectedPeer {
                    peer_id: "peer-2".to_string(),
                    device_name: "Laptop".to_string(),
                    connected_at: Utc::now(),
                },
            ],
            true,
            &["peer-1"],
            &[],
        );

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect("partial failure should still be best-effort success");

        let calls = send_calls.lock().expect("send calls lock");
        assert_eq!(calls.len(), 1, "peer-2 should still receive payload");
        assert_eq!(calls[0].0, "peer-2");
    }

    #[test]
    fn no_op_when_no_sendable_peers() {
        let (usecase, send_calls, list_sendable_peers_calls, ensure_calls, encrypt_calls) =
            build_usecase(vec![], true, &[], &[]);

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect("should no-op");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
        assert_eq!(list_sendable_peers_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 0);
        assert_eq!(encrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn returns_error_when_all_sendable_peers_fail() {
        let (usecase, send_calls, _, _, _) = build_usecase(
            vec![
                ConnectedPeer {
                    peer_id: "peer-1".to_string(),
                    device_name: "Desk".to_string(),
                    connected_at: Utc::now(),
                },
                ConnectedPeer {
                    peer_id: "peer-2".to_string(),
                    device_name: "Laptop".to_string(),
                    connected_at: Utc::now(),
                },
            ],
            true,
            &["peer-1", "peer-2"],
            &[],
        );

        let err = usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture)
            .expect_err("all send failures should return error");

        let err_msg = err.to_string();
        assert!(
            err_msg.contains("fanout failed"),
            "unexpected error message: {err_msg}"
        );
        assert!(
            err_msg.contains("peer-1"),
            "missing peer-1 in error: {err_msg}"
        );
        assert!(
            err_msg.contains("peer-2"),
            "missing peer-2 in error: {err_msg}"
        );
        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
    }
}
