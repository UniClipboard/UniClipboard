use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use uc_app::usecases::clipboard::sync_inbound::SyncInboundClipboardUseCase;
use uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase;
use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage, ProtocolMessage,
};
use uc_core::ports::{
    ClipboardChangeOriginPort, ClipboardTransportPort, DeviceIdentityPort, EncryptionPort,
    EncryptionSessionPort, NetworkEventPort, PairingTransportPort, PeerDirectoryPort, SettingsPort,
    SystemClipboardPort,
};
use uc_core::security::model::{
    EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfParams, Kek,
    MasterKey, Passphrase,
};
use uc_core::settings::model::Settings;
use uc_core::{
    ClipboardChangeOrigin, DeviceId, MimeType, ObservedClipboardRepresentation,
    SystemClipboardSnapshot,
};
use uc_infra::clipboard::InMemoryClipboardChangeOrigin;

struct InMemoryClipboard {
    snapshot: Arc<Mutex<SystemClipboardSnapshot>>,
    write_count: Arc<AtomicUsize>,
}

impl InMemoryClipboard {
    fn new(initial: SystemClipboardSnapshot) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(initial)),
            write_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn writes(&self) -> usize {
        self.write_count.load(Ordering::SeqCst)
    }
}

impl SystemClipboardPort for InMemoryClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| anyhow!("snapshot lock poisoned"))?;
        Ok(snapshot.clone())
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let mut guard = self
            .snapshot
            .lock()
            .map_err(|_| anyhow!("snapshot lock poisoned"))?;
        *guard = snapshot;
        self.write_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct StaticDeviceIdentity {
    id: DeviceId,
}

impl DeviceIdentityPort for StaticDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        self.id.clone()
    }
}

struct ReadyEncryptionSession;

#[async_trait]
impl EncryptionSessionPort for ReadyEncryptionSession {
    async fn is_ready(&self) -> bool {
        true
    }

    async fn get_master_key(&self) -> std::result::Result<MasterKey, EncryptionError> {
        Ok(MasterKey([5; 32]))
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

struct PassthroughEncryption;

#[async_trait]
impl EncryptionPort for PassthroughEncryption {
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
        encrypted: &EncryptedBlob,
        _aad: &[u8],
    ) -> std::result::Result<Vec<u8>, EncryptionError> {
        Ok(encrypted.ciphertext.clone())
    }
}

struct StaticSettings {
    settings: Settings,
}

#[async_trait]
impl SettingsPort for StaticSettings {
    async fn load(&self) -> anyhow::Result<Settings> {
        Ok(self.settings.clone())
    }

    async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
        Ok(())
    }
}

struct InProcessNetwork {
    local_peer_id: String,
    remote_peer: ConnectedPeer,
    remote_inbound: Arc<SyncInboundClipboardUseCase>,
    send_count: Arc<AtomicUsize>,
}

#[async_trait]
impl ClipboardTransportPort for InProcessNetwork {
    async fn send_clipboard(&self, peer_id: &str, outbound_bytes: Vec<u8>) -> anyhow::Result<()> {
        self.send_count.fetch_add(1, Ordering::SeqCst);

        if peer_id != self.remote_peer.peer_id {
            return Err(anyhow!(
                "unexpected peer id; expected {}, got {}",
                self.remote_peer.peer_id,
                peer_id
            ));
        }

        // Parse two-segment wire format: [4-byte JSON len LE][JSON header][optional trailing V2 payload]
        if outbound_bytes.len() < 4 {
            return Err(anyhow!("outbound bytes too short for framed format"));
        }
        let json_len = u32::from_le_bytes(outbound_bytes[0..4].try_into().unwrap()) as usize;
        if outbound_bytes.len() < 4 + json_len {
            return Err(anyhow!(
                "outbound bytes truncated: expected {} JSON bytes, have {}",
                json_len,
                outbound_bytes.len() - 4
            ));
        }
        let json_bytes = &outbound_bytes[4..4 + json_len];
        let v2_trailing = &outbound_bytes[4 + json_len..];

        let message = ProtocolMessage::from_bytes(json_bytes)
            .context("failed to decode framed JSON header as ProtocolMessage")?;
        match message {
            ProtocolMessage::Clipboard(mut clipboard_message) => {
                // For V2: the real encrypted payload is in the trailing bytes, not in the JSON header.
                // Re-attach it to encrypted_content so the inbound use case can process it.
                if !v2_trailing.is_empty() {
                    clipboard_message.encrypted_content = v2_trailing.to_vec();
                }
                self.remote_inbound.execute(clipboard_message).await
            }
            _ => Err(anyhow!(
                "expected ProtocolMessage::Clipboard for in-process routing"
            )),
        }
    }

    async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_clipboard(&self) -> anyhow::Result<mpsc::Receiver<ClipboardMessage>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn ensure_business_path(&self, peer_id: &str) -> anyhow::Result<()> {
        if peer_id == self.remote_peer.peer_id {
            return Ok(());
        }

        Err(anyhow!(
            "unexpected peer id for business path; expected {}, got {}",
            self.remote_peer.peer_id,
            peer_id
        ))
    }
}

#[async_trait]
impl PeerDirectoryPort for InProcessNetwork {
    async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        Ok(vec![DiscoveredPeer {
            peer_id: self.remote_peer.peer_id.clone(),
            device_name: Some(self.remote_peer.device_name.clone()),
            device_id: None,
            addresses: Vec::new(),
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: true,
        }])
    }

    async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
        Ok(vec![self.remote_peer.clone()])
    }

    async fn list_sendable_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        Ok(vec![DiscoveredPeer {
            peer_id: self.remote_peer.peer_id.clone(),
            device_name: Some(self.remote_peer.device_name.clone()),
            device_id: None,
            addresses: Vec::new(),
            discovered_at: self.remote_peer.connected_at,
            last_seen: self.remote_peer.connected_at,
            is_paired: true,
        }])
    }

    fn local_peer_id(&self) -> String {
        self.local_peer_id.clone()
    }

    async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl PairingTransportPort for InProcessNetwork {
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
impl NetworkEventPort for InProcessNetwork {
    async fn subscribe_events(&self) -> anyhow::Result<mpsc::Receiver<NetworkEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }
}

fn text_snapshot(text: &str, ts_ms: i64) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms,
        representations: vec![ObservedClipboardRepresentation {
            id: RepresentationId::new(),
            format_id: FormatId::from("text"),
            mime: Some(MimeType::text_plain()),
            bytes: text.as_bytes().to_vec(),
        }],
    }
}

fn snapshot_text(snapshot: &SystemClipboardSnapshot) -> Result<String> {
    let bytes = &snapshot
        .representations
        .first()
        .ok_or_else(|| anyhow!("expected one representation"))?
        .bytes;
    let text = std::str::from_utf8(bytes).context("snapshot text is not utf8")?;
    Ok(text.to_string())
}

#[tokio::test]
async fn clipboard_sync_e2e_dual_peer_in_process() -> Result<()> {
    let clipboard_a = Arc::new(InMemoryClipboard::new(text_snapshot("", 0)));
    let clipboard_b = Arc::new(InMemoryClipboard::new(text_snapshot("", 0)));

    let origin_a = Arc::new(InMemoryClipboardChangeOrigin::new());
    let origin_b = Arc::new(InMemoryClipboardChangeOrigin::new());

    let encryption_a: Arc<dyn EncryptionPort> = Arc::new(PassthroughEncryption);
    let encryption_b: Arc<dyn EncryptionPort> = Arc::new(PassthroughEncryption);
    let session_a: Arc<dyn EncryptionSessionPort> = Arc::new(ReadyEncryptionSession);
    let session_b: Arc<dyn EncryptionSessionPort> = Arc::new(ReadyEncryptionSession);

    let identity_a: Arc<dyn DeviceIdentityPort> = Arc::new(StaticDeviceIdentity {
        id: DeviceId::new("device-a"),
    });
    let identity_b: Arc<dyn DeviceIdentityPort> = Arc::new(StaticDeviceIdentity {
        id: DeviceId::new("device-b"),
    });

    let settings: Arc<dyn SettingsPort> = Arc::new(StaticSettings {
        settings: Settings::default(),
    });

    let inbound_a = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_a.clone(),
        origin_a.clone(),
        session_a.clone(),
        encryption_a.clone(),
        identity_a.clone(),
    )?);
    let inbound_b = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_b.clone(),
        origin_b.clone(),
        session_b.clone(),
        encryption_b.clone(),
        identity_b.clone(),
    )?);

    let a_send_count = Arc::new(AtomicUsize::new(0));
    let b_send_count = Arc::new(AtomicUsize::new(0));

    let network_a = Arc::new(InProcessNetwork {
        local_peer_id: "peer-a".to_string(),
        remote_peer: ConnectedPeer {
            peer_id: "peer-b".to_string(),
            device_name: "Device B".to_string(),
            connected_at: Utc::now(),
        },
        remote_inbound: inbound_b,
        send_count: a_send_count.clone(),
    });

    let network_b = Arc::new(InProcessNetwork {
        local_peer_id: "peer-b".to_string(),
        remote_peer: ConnectedPeer {
            peer_id: "peer-a".to_string(),
            device_name: "Device A".to_string(),
            connected_at: Utc::now(),
        },
        remote_inbound: inbound_a,
        send_count: b_send_count.clone(),
    });

    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a.clone(),
        network_a.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_a.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_a,
        encryption_a,
        identity_a,
        settings.clone(),
    );
    let outbound_b = SyncOutboundClipboardUseCase::new(
        clipboard_b.clone(),
        network_b.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_b.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_b,
        encryption_b,
        identity_b,
        settings,
    );

    outbound_a.execute(
        text_snapshot("hello from device A", 1_713_000_000_001),
        ClipboardChangeOrigin::LocalCapture,
    )?;

    assert_eq!(a_send_count.load(Ordering::SeqCst), 1);
    assert_eq!(clipboard_b.writes(), 1);

    let snapshot_on_b = clipboard_b.read_snapshot()?;
    assert_eq!(snapshot_on_b.representations.len(), 1);
    assert_eq!(snapshot_text(&snapshot_on_b)?, "hello from device A");

    let b_origin = origin_b
        .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
        .await;
    assert_eq!(b_origin, ClipboardChangeOrigin::RemotePush);

    outbound_b.execute(snapshot_on_b, b_origin)?;

    assert_eq!(b_send_count.load(Ordering::SeqCst), 0);
    assert_eq!(clipboard_a.writes(), 0);

    Ok(())
}
