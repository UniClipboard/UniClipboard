use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use futures::executor;
use tracing::{debug, info, info_span, warn, Instrument};
use uuid::Uuid;

use uc_core::config::RECEIVE_PLAINTEXT_CAP;
use uc_core::network::paired_device::resolve_sync_settings;
use uc_core::network::protocol::{
    BinaryRepresentation, ClipboardBinaryPayload, ClipboardPayloadVersion,
};
use uc_core::network::{ClipboardMessage, ProtocolMessage};
use uc_core::ports::{
    ClipboardTransportPort, DeviceIdentityPort, EncryptionSessionPort, PairedDeviceRepositoryPort,
    PeerDirectoryPort, SettingsPort, SystemClipboardPort, TransferPayloadEncryptorPort,
};
use uc_core::{ClipboardChangeOrigin, PeerId, SystemClipboardSnapshot};

pub struct SyncOutboundClipboardUseCase {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_network: Arc<dyn ClipboardTransportPort>,
    peer_directory: Arc<dyn PeerDirectoryPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
}

impl SyncOutboundClipboardUseCase {
    pub fn new(
        local_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_network: Arc<dyn ClipboardTransportPort>,
        peer_directory: Arc<dyn PeerDirectoryPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
        transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort>,
        paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
    ) -> Self {
        Self {
            local_clipboard,
            clipboard_network,
            peer_directory,
            encryption_session,
            device_identity,
            settings,
            transfer_encryptor,
            paired_device_repo,
        }
    }

    /// Filter sendable peers by per-device sync policy (auto_sync + content type).
    ///
    /// Peers not found in the paired device table are kept (safety fallback).
    /// Errors from settings/repo loads are logged and the peer is kept.
    /// The snapshot is classified once and the content type check is applied per-peer.
    pub async fn apply_sync_policy(
        &self,
        peers: &[uc_core::network::DiscoveredPeer],
        snapshot: &SystemClipboardSnapshot,
    ) -> Vec<uc_core::network::DiscoveredPeer> {
        use uc_core::settings::content_type_filter::{classify_snapshot, is_content_type_allowed};

        let global_settings = match self.settings.load().await {
            Ok(s) => Some(s),
            Err(err) => {
                warn!(
                    error = %err,
                    "Failed to load global settings for per-device sync policy check; proceeding with all peers"
                );
                None
            }
        };

        // Classify the snapshot once, not per-peer
        let content_category = classify_snapshot(snapshot);

        let mut result = Vec::with_capacity(peers.len());
        for peer in peers {
            let peer_id = PeerId::from(peer.peer_id.as_str());
            match self.paired_device_repo.get_by_peer_id(&peer_id).await {
                Ok(Some(device)) => {
                    if let Some(ref gs) = global_settings {
                        let effective = resolve_sync_settings(&device, &gs.sync);
                        if !effective.auto_sync {
                            debug!(
                                peer_id = %peer.peer_id,
                                "Skipping sync for peer: auto_sync disabled"
                            );
                            continue;
                        }
                        if !is_content_type_allowed(content_category, &effective.content_types) {
                            debug!(
                                peer_id = %peer.peer_id,
                                content_type = ?content_category,
                                "Skipping sync for peer: content type disabled"
                            );
                            continue;
                        }
                    }
                    result.push(peer.clone());
                }
                Ok(None) => {
                    // Peer not in paired_device table yet -- proceed with sync
                    result.push(peer.clone());
                }
                Err(err) => {
                    warn!(
                        peer_id = %peer.peer_id,
                        error = %err,
                        "Failed to load paired device for sync policy check; proceeding with sync"
                    );
                    result.push(peer.clone());
                }
            }
        }
        result
    }

    pub fn execute_current_snapshot(&self, origin: ClipboardChangeOrigin) -> Result<()> {
        let snapshot = self
            .local_clipboard
            .read_snapshot()
            .context("failed to read current clipboard snapshot for outbound sync")?;
        self.execute(snapshot, origin, None)
    }

    pub fn execute(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        origin_flow_id: Option<String>,
    ) -> Result<()> {
        let span = info_span!(
            "usecase.clipboard.sync_outbound.execute",
            origin = ?origin,
            representation_count = snapshot.representations.len(),
        );

        executor::block_on(
            self.execute_async(snapshot, origin, origin_flow_id)
                .instrument(span),
        )
    }

    async fn execute_async(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        origin_flow_id: Option<String>,
    ) -> Result<()> {
        if origin == ClipboardChangeOrigin::RemotePush {
            debug!(origin = ?origin, "Skipping outbound sync for remote-push origin");
            return Ok(());
        }

        if !self.encryption_session.is_ready().await {
            info!(origin = ?origin, "Skipping outbound sync because encryption session is not ready");
            return Ok(());
        }

        // V3: All representations are sent. Return early if there are none.
        if snapshot.representations.is_empty() {
            debug!("Skipping outbound sync because snapshot has no representations");
            return Ok(());
        }

        let all_sendable_peers = self
            .peer_directory
            .list_sendable_peers()
            .await
            .context("failed to load sendable peers for outbound sync")?;

        // Filter out peers whose effective sync policy disallows this content
        let sendable_peers = self.apply_sync_policy(&all_sendable_peers, &snapshot).await;
        let discovered_peer_count = match self.peer_directory.get_discovered_peers().await {
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

        // Extract content_hash and ts_ms BEFORE consuming representations via into_iter().
        let content_hash = snapshot.snapshot_hash().to_string();
        let ts_ms = snapshot.ts_ms;

        // Build V3 binary payload from snapshot representations.
        let binary_reps: Vec<BinaryRepresentation> = snapshot
            .representations
            .into_iter()
            .map(|rep| BinaryRepresentation {
                format_id: rep.format_id.into_inner(),
                mime: rep.mime.map(|m| m.0),
                data: rep.bytes,
            })
            .collect();

        let v3_payload = ClipboardBinaryPayload {
            ts_ms,
            representations: binary_reps,
        };

        let plaintext_bytes = v3_payload
            .encode_to_vec()
            .context("failed to encode V3 clipboard binary payload")?;
        if plaintext_bytes.len() > RECEIVE_PLAINTEXT_CAP {
            bail!(
                "plaintext exceeds receive-side cap: {} > {}",
                plaintext_bytes.len(),
                RECEIVE_PLAINTEXT_CAP
            );
        }

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

        // Build the JSON header (V3: encrypted payload goes as raw trailing bytes)
        let clipboard_header = ClipboardMessage {
            id: message_id,
            content_hash,
            encrypted_content: vec![], // V3 binary is NOT in the JSON
            timestamp: Utc::now(),
            origin_device_id,
            origin_device_name,
            payload_version: ClipboardPayloadVersion::V3,
            origin_flow_id,
        };

        // Clone values needed for parallel encryption block (to avoid &self borrow in tokio::join!)
        let transfer_encryptor = self.transfer_encryptor.clone();
        let encryption_session = self.encryption_session.clone();

        let first_peer = sendable_peers[0].clone();
        let remaining_peers = sendable_peers[1..].to_vec();

        let raw_bytes = plaintext_bytes.len();
        let mut connect_failures = Vec::new();
        let mut connect_success_count = 0usize;

        // Parallel: run prepare path in its own task so CPU-heavy encrypt/frame work
        // cannot starve the business-path ensure branch.
        let prepare_future = async move {
            let master_key = encryption_session
                .get_master_key()
                .await
                .map_err(anyhow::Error::from)
                .context("failed to access encryption session master key for outbound sync")?;

            let encrypted_content = transfer_encryptor
                .encrypt(&master_key, &plaintext_bytes)
                .map_err(|e| {
                    anyhow::anyhow!("failed to encrypt outbound clipboard payload: {e}")
                })?;

            info!(
                raw_bytes,
                encrypted_bytes = encrypted_content.len(),
                "outbound payload encrypted"
            );

            let framed = ProtocolMessage::Clipboard(clipboard_header)
                .frame_to_bytes(Some(&encrypted_content))
                .context("failed to frame outbound V3 clipboard message")?;

            Ok::<Arc<[u8]>, anyhow::Error>(Arc::from(framed.into_boxed_slice()))
        }
        .instrument(info_span!(
            "outbound.prepare",
            raw_bytes,
            stage = uc_observability::stages::OUTBOUND_PREPARE
        ));

        let outbound_bytes = if tokio::runtime::Handle::try_current().is_ok() {
            let prepare_handle = tokio::spawn(prepare_future);
            let (prepare_result, ensure_result) = tokio::join!(
                prepare_handle,
                self.clipboard_network
                    .ensure_business_path(&first_peer.peer_id)
            );
            match ensure_result {
                Ok(()) => {
                    connect_success_count += 1;
                }
                Err(err) => {
                    warn!(
                        peer_id = %first_peer.peer_id,
                        error = %err,
                        "failed to ensure outbound business path for first peer; skipping send"
                    );
                    connect_failures.push(format!("{}: {}", first_peer.peer_id, err));
                }
            }
            let encrypted_result = prepare_result
                .map_err(anyhow::Error::from)
                .context("outbound prepare task join failed")?;
            encrypted_result?
        } else {
            let (encrypted_result, ensure_result) = tokio::join!(
                prepare_future,
                self.clipboard_network
                    .ensure_business_path(&first_peer.peer_id)
            );
            match ensure_result {
                Ok(()) => {
                    connect_success_count += 1;
                }
                Err(err) => {
                    warn!(
                        peer_id = %first_peer.peer_id,
                        error = %err,
                        "failed to ensure outbound business path for first peer; skipping send"
                    );
                    connect_failures.push(format!("{}: {}", first_peer.peer_id, err));
                }
            }
            encrypted_result?
        };

        let mut send_failures = Vec::new();
        let mut sent_count = 0usize;

        if connect_success_count > 0 {
            if let Err(err) = async {
                self.clipboard_network
                    .send_clipboard(&first_peer.peer_id, outbound_bytes.clone())
                    .await
            }
            .instrument(info_span!("outbound.send", peer_id = %first_peer.peer_id, stage = uc_observability::stages::OUTBOUND_SEND))
            .await
            {
                warn!(
                    peer_id = %first_peer.peer_id,
                    error = %err,
                    "failed to send outbound clipboard message to first peer"
                );
                send_failures.push(format!("{}: {}", first_peer.peer_id, err));
            } else {
                sent_count += 1;
            }
        }

        // Serial for remaining peers: ensure + send with Arc clone (zero-copy)
        for peer in &remaining_peers {
            if let Err(err) = self
                .clipboard_network
                .ensure_business_path(&peer.peer_id)
                .await
            {
                warn!(
                    peer_id = %peer.peer_id,
                    error = %err,
                    "failed to ensure outbound business path; skipping send for this peer"
                );
                connect_failures.push(format!("{}: {}", peer.peer_id, err));
                continue;
            }
            connect_success_count += 1;

            if let Err(err) = async {
                self.clipboard_network
                    .send_clipboard(&peer.peer_id, outbound_bytes.clone())
                    .await
            }
            .instrument(info_span!("outbound.send", peer_id = %peer.peer_id, stage = uc_observability::stages::OUTBOUND_SEND))
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
            let failure_count = failures.len();
            warn!(
                sent_count,
                failure_count,
                "outbound clipboard fanout partially failed after best-effort retries"
            );
            info!(
                sent_count,
                connect_success_count, "Outbound clipboard sync sent to sendable peers (partial)"
            );
            return Err(anyhow::anyhow!(
                "outbound clipboard fanout partially failed: {sent_count} sent, {failure_count} failed ({})",
                failures.join(" | ")
            ));
        }

        info!(
            sent_count,
            connect_success_count, "Outbound clipboard sync sent to sendable peers"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use tokio::sync::mpsc;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::network::protocol::ClipboardPayloadVersion;
    use uc_core::network::PairingState;
    use uc_core::network::{
        ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage,
        ProtocolMessage,
    };
    use uc_core::ports::{
        ClipboardTransportPort, NetworkEventPort, PairedDeviceRepositoryError,
        PairedDeviceRepositoryPort, PairingTransportPort, PeerDirectoryPort,
    };
    use uc_core::security::model::{EncryptionError, MasterKey};
    use uc_core::settings::model::Settings;
    use uc_core::{DeviceId, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use uc_infra::clipboard::{ChunkedDecoder, TransferPayloadEncryptorAdapter};

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
    impl ClipboardTransportPort for TestNetwork {
        async fn send_clipboard(
            &self,
            peer_id: &str,
            encrypted_data: std::sync::Arc<[u8]>,
        ) -> anyhow::Result<()> {
            if self.failing_peers.contains(peer_id) {
                return Err(anyhow::anyhow!("simulated send failure for {peer_id}"));
            }

            self.send_calls
                .lock()
                .expect("send calls lock")
                .push((peer_id.to_string(), encrypted_data.to_vec()));
            Ok(())
        }

        async fn broadcast_clipboard(
            &self,
            _encrypted_data: std::sync::Arc<[u8]>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(
            &self,
        ) -> anyhow::Result<mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
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
    }

    #[async_trait]
    impl PeerDirectoryPort for TestNetwork {
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

        fn local_peer_id(&self) -> String {
            "peer-local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairingTransportPort for TestNetwork {
        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(&self, _message: PairingMessage) -> anyhow::Result<()> {
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
    }

    #[async_trait]
    impl NetworkEventPort for TestNetwork {
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

    struct TestDeviceIdentity;

    impl DeviceIdentityPort for TestDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device-1")
        }
    }

    struct TestPairedDeviceRepo;

    #[async_trait]
    impl PairedDeviceRepositoryPort for TestPairedDeviceRepo {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<Option<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: uc_core::network::PairedDevice,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::PeerId,
            _state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &uc_core::PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
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

    /// Parse a two-segment framed wire message, returning (ClipboardMessage, raw_trailing_bytes).
    fn parse_framed(bytes: &[u8]) -> (ClipboardMessage, &[u8]) {
        let json_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let json_bytes = &bytes[4..4 + json_len];
        let trailing = &bytes[4 + json_len..];
        match ProtocolMessage::from_bytes(json_bytes).expect("decode protocol message") {
            ProtocolMessage::Clipboard(msg) => (msg, trailing),
            other => panic!("expected Clipboard, got {:?}", other),
        }
    }

    fn build_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("public.utf8-plain-text"),
                Some(MimeType::text_plain()),
                b"hello world".to_vec(),
            )],
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

        let network = Arc::new(TestNetwork {
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
        });

        let usecase = SyncOutboundClipboardUseCase::new(
            Arc::new(TestSystemClipboard {
                snapshot: build_snapshot(),
            }),
            network.clone(),
            network,
            Arc::new(TestEncryptionSession {
                ready: encryption_ready,
            }),
            Arc::new(TestDeviceIdentity),
            Arc::new(TestSettings {
                settings: Settings::default(),
            }),
            Arc::new(TransferPayloadEncryptorAdapter),
            Arc::new(TestPairedDeviceRepo),
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
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
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
            .execute(build_snapshot(), ClipboardChangeOrigin::RemotePush, None)
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
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalRestore, None)
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
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
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
    fn outbound_bytes_decode_as_v3_protocol_message_clipboard() {
        let test_master_key = MasterKey([7; 32]); // matches TestEncryptionSession
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
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
            .expect("execute local capture");

        let calls = send_calls.lock().expect("send calls lock");
        let (_, outbound_bytes) = calls.first().expect("one outbound send");

        // Parse two-segment wire format
        let (message, v3_raw_payload) = parse_framed(outbound_bytes);

        // V3: payload_version must be V3
        assert_eq!(
            message.payload_version,
            ClipboardPayloadVersion::V3,
            "outbound message must use V3 payload version"
        );
        assert!(
            message.encrypted_content.is_empty(),
            "V3 JSON header must have empty encrypted_content"
        );

        // Decode the raw V3 payload (trailing bytes after JSON header)
        let plaintext = ChunkedDecoder::decode_from(Cursor::new(v3_raw_payload), &test_master_key)
            .expect("V3 chunk decode must succeed");

        // V3: plaintext decodes as ClipboardBinaryPayload
        let v3_payload = ClipboardBinaryPayload::decode_from(&mut Cursor::new(&plaintext))
            .expect("V3 binary payload decode");

        // Must have representations — "hello world" text/plain rep
        assert_eq!(v3_payload.representations.len(), 1);
        assert_eq!(v3_payload.representations[0].data, b"hello world".to_vec());
        assert_eq!(
            v3_payload.representations[0].mime.as_deref(),
            Some("text/plain")
        );
    }

    #[test]
    fn no_op_when_snapshot_has_no_representations() {
        let empty_snapshot = SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![],
        };

        let (usecase, send_calls, list_sendable_peers_calls, ensure_calls, encrypt_calls) =
            build_usecase(
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
            .execute(empty_snapshot, ClipboardChangeOrigin::LocalCapture, None)
            .expect("empty snapshot should no-op without error");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
        // Should return early before peer lookup when there are no representations
        assert_eq!(
            list_sendable_peers_calls.load(Ordering::SeqCst),
            0,
            "should not query peers for empty snapshot"
        );
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 0);
        assert_eq!(encrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn v3_outbound_sends_all_representations_and_uses_snapshot_hash() {
        let test_master_key = MasterKey([7; 32]); // matches TestEncryptionSession
        let multi_rep_snapshot = SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("public.utf8-plain-text"),
                    Some(MimeType::text_plain()),
                    b"hello world".to_vec(),
                ),
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("public.png"),
                    Some(MimeType("image/png".to_string())),
                    vec![0x89, 0x50, 0x4E, 0x47], // PNG header bytes,
                ),
            ],
        };

        let expected_hash = multi_rep_snapshot.snapshot_hash().to_string();

        let (usecase, send_calls, _, _, encrypt_calls) = build_usecase(
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
            .execute(
                multi_rep_snapshot,
                ClipboardChangeOrigin::LocalCapture,
                None,
            )
            .expect("execute multi-rep capture");

        // V3 does NOT call encrypt_blob (uses ChunkedEncoder directly)
        assert_eq!(
            encrypt_calls.load(Ordering::SeqCst),
            0,
            "V3 must not call encrypt_blob"
        );

        let calls = send_calls.lock().expect("send calls lock");
        let (_, outbound_bytes) = calls.first().expect("one outbound send");

        // Parse two-segment wire format
        let (message, v3_raw_payload) = parse_framed(outbound_bytes);

        // content_hash must equal snapshot_hash (covers all representations)
        assert_eq!(
            message.content_hash, expected_hash,
            "content_hash must be snapshot_hash covering all representations"
        );
        assert_eq!(message.payload_version, ClipboardPayloadVersion::V3);
        assert!(
            message.encrypted_content.is_empty(),
            "V3 JSON header must have empty encrypted_content"
        );

        let plaintext = ChunkedDecoder::decode_from(Cursor::new(v3_raw_payload), &test_master_key)
            .expect("V3 chunk decode");
        let v3_payload = ClipboardBinaryPayload::decode_from(&mut Cursor::new(&plaintext))
            .expect("V3 payload decode");

        // Must have BOTH representations
        assert_eq!(v3_payload.representations.len(), 2);
        let mimes: Vec<Option<&str>> = v3_payload
            .representations
            .iter()
            .map(|r| r.mime.as_deref())
            .collect();
        assert!(mimes.contains(&Some("text/plain")));
        assert!(mimes.contains(&Some("image/png")));
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

        let err = usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
            .expect_err("partial fanout failure should be reported");
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("partially failed"),
            "unexpected error message: {err_msg}"
        );
        assert!(
            err_msg.contains("peer-1"),
            "missing peer-1 in error: {err_msg}"
        );

        let calls = send_calls.lock().expect("send calls lock");
        assert_eq!(calls.len(), 1, "peer-2 should still receive payload");
        assert_eq!(calls[0].0, "peer-2");
    }

    #[test]
    fn returns_error_when_all_sendable_peers_fail_business_path_ensure() {
        let (usecase, send_calls, _, ensure_calls, _) = build_usecase(
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
            &[],
            &["peer-1", "peer-2"],
        );

        let err = usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
            .expect_err("all ensure failures should return error");

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
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn returns_error_with_partial_send_when_some_ensure_business_path_fail() {
        let (usecase, send_calls, _, ensure_calls, _) = build_usecase(
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
            &[],
            &["peer-1"],
        );

        let err = usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
            .expect_err("partial ensure failures should return error");

        let err_msg = err.to_string();
        assert!(
            err_msg.contains("partially failed"),
            "unexpected error message: {err_msg}"
        );
        assert!(
            err_msg.contains("peer-1"),
            "missing peer-1 in error: {err_msg}"
        );

        let calls = send_calls.lock().expect("send calls lock");
        assert_eq!(calls.len(), 1, "peer-2 should still receive payload");
        assert_eq!(calls[0].0, "peer-2");
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn no_op_when_no_sendable_peers() {
        let (usecase, send_calls, list_sendable_peers_calls, ensure_calls, encrypt_calls) =
            build_usecase(vec![], true, &[], &[]);

        usecase
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
            .expect("should no-op");

        assert_eq!(send_calls.lock().expect("send calls lock").len(), 0);
        assert_eq!(list_sendable_peers_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ensure_calls.load(Ordering::SeqCst), 0);
        assert_eq!(encrypt_calls.load(Ordering::SeqCst), 0);
    }

    // --- apply_sync_policy content type filtering tests ---

    use uc_core::network::PairedDevice;
    use uc_core::settings::model::{
        ContentTypes, SyncFrequency, SyncSettings as SyncSettingsModel,
    };

    struct ConfigurablePairedDeviceRepo {
        devices: std::collections::HashMap<String, PairedDevice>,
        fail_for: HashSet<String>,
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for ConfigurablePairedDeviceRepo {
        async fn get_by_peer_id(
            &self,
            peer_id: &uc_core::PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            let id = peer_id.as_str().to_string();
            if self.fail_for.contains(&id) {
                return Err(PairedDeviceRepositoryError::Storage(
                    "simulated repo error".to_string(),
                ));
            }
            Ok(self.devices.get(&id).cloned())
        }

        async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(self.devices.values().cloned().collect())
        }

        async fn upsert(&self, _device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::PeerId,
            _state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &uc_core::PeerId,
            _settings: Option<SyncSettingsModel>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    fn make_paired_device(peer_id: &str, sync_settings: Option<SyncSettingsModel>) -> PairedDevice {
        PairedDevice {
            peer_id: PeerId::from(peer_id),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: format!("Device {}", peer_id),
            sync_settings,
        }
    }

    fn build_policy_usecase(
        sendable_peers: Vec<DiscoveredPeer>,
        paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
    ) -> SyncOutboundClipboardUseCase {
        let network = Arc::new(TestNetwork {
            sendable_peers,
            failing_peers: HashSet::new(),
            ensure_failing_peers: HashSet::new(),
            send_calls: Arc::new(Mutex::new(Vec::new())),
            list_sendable_peers_calls: Arc::new(AtomicUsize::new(0)),
            ensure_business_path_calls: Arc::new(AtomicUsize::new(0)),
        });

        SyncOutboundClipboardUseCase::new(
            Arc::new(TestSystemClipboard {
                snapshot: build_snapshot(),
            }),
            network.clone(),
            network,
            Arc::new(TestEncryptionSession { ready: true }),
            Arc::new(TestDeviceIdentity),
            Arc::new(TestSettings {
                settings: Settings::default(),
            }),
            Arc::new(TransferPayloadEncryptorAdapter),
            paired_device_repo,
        )
    }

    fn make_discovered_peer(peer_id: &str) -> DiscoveredPeer {
        DiscoveredPeer {
            peer_id: peer_id.to_string(),
            device_name: Some(format!("Device {}", peer_id)),
            device_id: None,
            addresses: Vec::new(),
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: true,
        }
    }

    fn make_text_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("public.utf8-plain-text"),
                Some(MimeType::text_plain()),
                b"hello".to_vec(),
            )],
        }
    }

    fn make_image_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("public.png"),
                Some(MimeType("image/png".to_string())),
                vec![0x89, 0x50, 0x4E, 0x47],
            )],
        }
    }

    fn make_unknown_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("com.custom.type"),
                Some(MimeType("application/x-custom".to_string())),
                b"custom data".to_vec(),
            )],
        }
    }

    #[tokio::test]
    async fn apply_sync_policy_keeps_peer_when_auto_sync_true_and_content_allowed() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "peer-1".to_string(),
            make_paired_device("peer-1", None), // uses global defaults: auto_sync=true, all content types true
        );
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices,
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_text_snapshot()).await;
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn apply_sync_policy_skips_peer_when_auto_sync_false() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "peer-1".to_string(),
            make_paired_device(
                "peer-1",
                Some(SyncSettingsModel {
                    auto_sync: false,
                    sync_frequency: SyncFrequency::Realtime,
                    content_types: ContentTypes::default(),
                    max_file_size_mb: 100,
                }),
            ),
        );
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices,
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_text_snapshot()).await;
        assert_eq!(
            result.len(),
            0,
            "peer with auto_sync=false should be skipped"
        );
    }

    #[tokio::test]
    async fn apply_sync_policy_skips_peer_when_content_type_disabled() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "peer-1".to_string(),
            make_paired_device(
                "peer-1",
                Some(SyncSettingsModel {
                    auto_sync: true,
                    sync_frequency: SyncFrequency::Realtime,
                    content_types: ContentTypes {
                        text: false, // text disabled
                        image: true,
                        link: true,
                        file: true,
                        code_snippet: true,
                        rich_text: true,
                    },
                    max_file_size_mb: 100,
                }),
            ),
        );
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices,
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_text_snapshot()).await;
        assert_eq!(
            result.len(),
            0,
            "peer with text content type disabled should be skipped for text snapshot"
        );
    }

    #[tokio::test]
    async fn apply_sync_policy_keeps_peer_when_content_type_unknown() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "peer-1".to_string(),
            make_paired_device(
                "peer-1",
                Some(SyncSettingsModel {
                    auto_sync: true,
                    sync_frequency: SyncFrequency::Realtime,
                    content_types: ContentTypes {
                        text: false,
                        image: false,
                        link: false,
                        file: false,
                        code_snippet: false,
                        rich_text: false,
                    },
                    max_file_size_mb: 100,
                }),
            ),
        );
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices,
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_unknown_snapshot()).await;
        assert_eq!(
            result.len(),
            1,
            "unknown content types should always sync regardless of toggles"
        );
    }

    #[tokio::test]
    async fn apply_sync_policy_skips_peer_when_image_content_type_disabled() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut devices = std::collections::HashMap::new();
        devices.insert(
            "peer-1".to_string(),
            make_paired_device(
                "peer-1",
                Some(SyncSettingsModel {
                    auto_sync: true,
                    sync_frequency: SyncFrequency::Realtime,
                    content_types: ContentTypes {
                        text: true,
                        image: false,
                        link: true,
                        file: true,
                        code_snippet: true,
                        rich_text: true,
                    },
                    max_file_size_mb: 100,
                }),
            ),
        );
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices,
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_image_snapshot()).await;
        assert_eq!(
            result.len(),
            0,
            "peer with image content type disabled should be skipped for image snapshot"
        );
    }

    #[tokio::test]
    async fn apply_sync_policy_keeps_peer_not_in_paired_device_table() {
        let peers = vec![make_discovered_peer("peer-1")];
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices: std::collections::HashMap::new(), // empty - peer not found
            fail_for: HashSet::new(),
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_text_snapshot()).await;
        assert_eq!(
            result.len(),
            1,
            "peer not in paired_device table should be kept as safety fallback"
        );
    }

    #[tokio::test]
    async fn apply_sync_policy_keeps_peer_when_repo_returns_error() {
        let peers = vec![make_discovered_peer("peer-1")];
        let mut fail_for = HashSet::new();
        fail_for.insert("peer-1".to_string());
        let repo = Arc::new(ConfigurablePairedDeviceRepo {
            devices: std::collections::HashMap::new(),
            fail_for,
        });
        let uc = build_policy_usecase(peers.clone(), repo);

        let result = uc.apply_sync_policy(&peers, &make_text_snapshot()).await;
        assert_eq!(
            result.len(),
            1,
            "peer should be kept when repo returns error (safety fallback)"
        );
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
            .execute(build_snapshot(), ClipboardChangeOrigin::LocalCapture, None)
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
