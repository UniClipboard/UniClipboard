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
use uc_core::network::protocol::FileTransferMapping;
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage, ProtocolMessage,
};
use uc_core::network::{PairedDevice, PairingState};
use uc_core::ports::{
    ClipboardChangeOriginPort, ClipboardTransportPort, DeviceIdentityPort, EncryptionPort,
    EncryptionSessionPort, NetworkEventPort, PairedDeviceRepositoryError,
    PairedDeviceRepositoryPort, PairingTransportPort, PeerDirectoryPort, SettingsPort,
    SystemClipboardPort,
};
use uc_core::security::model::{
    EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfParams, Kek,
    MasterKey, Passphrase,
};
use uc_core::settings::model::Settings;
use uc_core::PeerId;
use uc_core::{
    ClipboardChangeOrigin, DeviceId, MimeType, ObservedClipboardRepresentation,
    SystemClipboardSnapshot,
};
use uc_infra::clipboard::{
    InMemoryClipboardChangeOrigin, TransferPayloadDecryptorAdapter, TransferPayloadEncryptorAdapter,
};

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
    async fn send_clipboard(
        &self,
        peer_id: &str,
        outbound_bytes: std::sync::Arc<[u8]>,
    ) -> anyhow::Result<()> {
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
                self.remote_inbound.execute(clipboard_message, None).await
            }
            _ => Err(anyhow!(
                "expected ProtocolMessage::Clipboard for in-process routing"
            )),
        }
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

struct NoopPairedDeviceRepo;

#[async_trait]
impl PairedDeviceRepositoryPort for NoopPairedDeviceRepo {
    async fn get_by_peer_id(
        &self,
        _peer_id: &PeerId,
    ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
        Ok(None)
    }

    async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
        Ok(vec![])
    }

    async fn upsert(&self, _device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
        Ok(())
    }

    async fn set_state(
        &self,
        _peer_id: &PeerId,
        _state: PairingState,
    ) -> Result<(), PairedDeviceRepositoryError> {
        Ok(())
    }

    async fn update_last_seen(
        &self,
        _peer_id: &PeerId,
        _last_seen_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        Ok(())
    }

    async fn delete(&self, _peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
        Ok(())
    }

    async fn update_sync_settings(
        &self,
        _peer_id: &PeerId,
        _settings: Option<uc_core::settings::model::SyncSettings>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        Ok(())
    }
}

/// Build a minimal 2x2 red PNG image for testing.
fn make_test_png() -> Vec<u8> {
    // Minimal valid 2x2 RGBA PNG image (red pixels)
    use std::io::Cursor;
    let mut buf = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(Cursor::new(&mut buf));
        use image::ImageEncoder;
        // 2x2 RGBA red pixels
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, // R
            255, 0, 0, 255, // R
            255, 0, 0, 255, // R
            255, 0, 0, 255, // R
        ];
        encoder
            .write_image(&pixels, 2, 2, image::ExtendedColorType::Rgba8)
            .expect("PNG encode");
    }
    buf
}

fn image_snapshot(png_bytes: Vec<u8>, ts_ms: i64) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            png_bytes,
        )],
    }
}

/// Build a multi-representation snapshot simulating Windows image copy
/// (image + Windows-specific private formats).
fn windows_image_snapshot(png_bytes: Vec<u8>, ts_ms: i64) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms,
        representations: vec![
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("image"),
                Some(MimeType("image/png".to_string())),
                png_bytes,
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("DataObject"),
                None,
                vec![0xDE, 0xAD],
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("Ole Private Data"),
                None,
                vec![0xBE, 0xEF],
            ),
        ],
    }
}

fn text_snapshot(text: &str, ts_ms: i64) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType::text_plain()),
            text.as_bytes().to_vec(),
        )],
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

    let transfer_decryptor: Arc<TransferPayloadDecryptorAdapter> =
        Arc::new(TransferPayloadDecryptorAdapter);
    let inbound_a = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_a.clone(),
        origin_a.clone(),
        session_a.clone(),
        encryption_a.clone(),
        identity_a.clone(),
        transfer_decryptor.clone(),
        settings.clone(),
    )?);
    let inbound_b = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_b.clone(),
        origin_b.clone(),
        session_b.clone(),
        encryption_b.clone(),
        identity_b.clone(),
        transfer_decryptor,
        settings.clone(),
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

    let transfer_encryptor: Arc<TransferPayloadEncryptorAdapter> =
        Arc::new(TransferPayloadEncryptorAdapter);
    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a.clone(),
        network_a.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_a.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_a,
        identity_a,
        settings.clone(),
        transfer_encryptor.clone(),
        Arc::new(NoopPairedDeviceRepo),
    );
    let outbound_b = SyncOutboundClipboardUseCase::new(
        clipboard_b.clone(),
        network_b.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_b.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_b,
        identity_b,
        settings,
        transfer_encryptor,
        Arc::new(NoopPairedDeviceRepo),
    );

    tokio::task::spawn_blocking(move || {
        outbound_a.execute(
            text_snapshot("hello from device A", 1_713_000_000_001),
            ClipboardChangeOrigin::LocalCapture,
            None,
            vec![],
        )
    })
    .await
    .map_err(|e| anyhow!("failed to join outbound A task: {e}"))??;

    assert_eq!(a_send_count.load(Ordering::SeqCst), 1);
    assert_eq!(clipboard_b.writes(), 1);

    let snapshot_on_b = clipboard_b.read_snapshot()?;
    assert_eq!(snapshot_on_b.representations.len(), 1);
    assert_eq!(snapshot_text(&snapshot_on_b)?, "hello from device A");

    let b_origin = origin_b
        .consume_origin_for_snapshot_or_default(
            &snapshot_on_b.snapshot_hash().to_string(),
            ClipboardChangeOrigin::LocalCapture,
        )
        .await;
    assert_eq!(b_origin, ClipboardChangeOrigin::RemotePush);

    tokio::task::spawn_blocking(move || outbound_b.execute(snapshot_on_b, b_origin, None, vec![]))
        .await
        .map_err(|e| anyhow!("failed to join outbound B task: {e}"))??;

    assert_eq!(b_send_count.load(Ordering::SeqCst), 0);
    assert_eq!(clipboard_a.writes(), 0);

    Ok(())
}

/// Image sync E2E: single image representation (image/png) syncs from A to B.
#[tokio::test]
async fn clipboard_sync_e2e_image_single_rep() -> Result<()> {
    let png_bytes = make_test_png();
    let clipboard_a = Arc::new(InMemoryClipboard::new(image_snapshot(vec![], 0)));
    let clipboard_b = Arc::new(InMemoryClipboard::new(image_snapshot(vec![], 0)));

    let origin_b = Arc::new(InMemoryClipboardChangeOrigin::new());

    let _encryption_a: Arc<dyn EncryptionPort> = Arc::new(PassthroughEncryption);
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

    let transfer_decryptor: Arc<TransferPayloadDecryptorAdapter> =
        Arc::new(TransferPayloadDecryptorAdapter);
    let inbound_b = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_b.clone(),
        origin_b.clone(),
        session_b.clone(),
        encryption_b.clone(),
        identity_b.clone(),
        transfer_decryptor,
        settings.clone(),
    )?);

    let a_send_count = Arc::new(AtomicUsize::new(0));

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

    let transfer_encryptor: Arc<TransferPayloadEncryptorAdapter> =
        Arc::new(TransferPayloadEncryptorAdapter);
    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a.clone(),
        network_a.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_a.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_a,
        identity_a,
        settings,
        transfer_encryptor,
        Arc::new(NoopPairedDeviceRepo),
    );

    let png_clone = png_bytes.clone();
    tokio::task::spawn_blocking(move || {
        outbound_a.execute(
            image_snapshot(png_clone, 1_713_000_000_001),
            ClipboardChangeOrigin::LocalCapture,
            None,
            vec![],
        )
    })
    .await
    .map_err(|e| anyhow!("failed to join outbound A task: {e}"))??;

    assert_eq!(a_send_count.load(Ordering::SeqCst), 1);
    assert_eq!(clipboard_b.writes(), 1);

    let snapshot_on_b = clipboard_b.read_snapshot()?;
    assert_eq!(
        snapshot_on_b.representations.len(),
        1,
        "receiver should have exactly one representation"
    );

    let rep = &snapshot_on_b.representations[0];
    assert_eq!(
        rep.mime.as_ref().map(|m| m.as_str()),
        Some("image/png"),
        "receiver representation should be image/png"
    );
    assert_eq!(
        rep.bytes, png_bytes,
        "receiver image bytes should match sender"
    );

    Ok(())
}

/// Image sync E2E: multi-representation Windows snapshot (image + private formats).
/// The receiver should select the image representation and write it to the clipboard.
#[tokio::test]
async fn clipboard_sync_e2e_windows_image_multi_rep() -> Result<()> {
    let png_bytes = make_test_png();
    let clipboard_a = Arc::new(InMemoryClipboard::new(image_snapshot(vec![], 0)));
    let clipboard_b = Arc::new(InMemoryClipboard::new(image_snapshot(vec![], 0)));

    let origin_b = Arc::new(InMemoryClipboardChangeOrigin::new());

    let _encryption_a: Arc<dyn EncryptionPort> = Arc::new(PassthroughEncryption);
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

    let transfer_decryptor: Arc<TransferPayloadDecryptorAdapter> =
        Arc::new(TransferPayloadDecryptorAdapter);
    let inbound_b = Arc::new(SyncInboundClipboardUseCase::new(
        ClipboardIntegrationMode::Full,
        clipboard_b.clone(),
        origin_b.clone(),
        session_b.clone(),
        encryption_b.clone(),
        identity_b.clone(),
        transfer_decryptor,
        settings.clone(),
    )?);

    let a_send_count = Arc::new(AtomicUsize::new(0));

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

    let transfer_encryptor: Arc<TransferPayloadEncryptorAdapter> =
        Arc::new(TransferPayloadEncryptorAdapter);
    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a.clone(),
        network_a.clone() as Arc<dyn uc_core::ports::ClipboardTransportPort>,
        network_a.clone() as Arc<dyn uc_core::ports::PeerDirectoryPort>,
        session_a,
        identity_a,
        settings,
        transfer_encryptor,
        Arc::new(NoopPairedDeviceRepo),
    );

    let png_clone = png_bytes.clone();
    tokio::task::spawn_blocking(move || {
        outbound_a.execute(
            windows_image_snapshot(png_clone, 1_713_000_000_001),
            ClipboardChangeOrigin::LocalCapture,
            None,
            vec![],
        )
    })
    .await
    .map_err(|e| anyhow!("failed to join outbound A task: {e}"))??;

    assert_eq!(a_send_count.load(Ordering::SeqCst), 1);
    assert_eq!(clipboard_b.writes(), 1);

    let snapshot_on_b = clipboard_b.read_snapshot()?;
    assert_eq!(
        snapshot_on_b.representations.len(),
        1,
        "receiver should have exactly ONE representation (highest priority selected)"
    );

    let rep = &snapshot_on_b.representations[0];
    assert_eq!(
        rep.mime.as_ref().map(|m| m.as_str()),
        Some("image/png"),
        "receiver should select image/png as highest priority"
    );
    assert_eq!(
        rep.bytes, png_bytes,
        "receiver image bytes should match the PNG from sender"
    );

    Ok(())
}

/// A network transport that captures sent ClipboardMessages for inspection
/// instead of forwarding them to a remote inbound use case.
struct CapturingNetwork {
    local_peer_id: String,
    remote_peer: ConnectedPeer,
    captured_messages: Arc<Mutex<Vec<ClipboardMessage>>>,
}

#[async_trait]
impl ClipboardTransportPort for CapturingNetwork {
    async fn send_clipboard(
        &self,
        _peer_id: &str,
        outbound_bytes: std::sync::Arc<[u8]>,
    ) -> anyhow::Result<()> {
        if outbound_bytes.len() < 4 {
            return Err(anyhow!("outbound bytes too short for framed format"));
        }
        let json_len = u32::from_le_bytes(outbound_bytes[0..4].try_into().unwrap()) as usize;
        if outbound_bytes.len() < 4 + json_len {
            return Err(anyhow!("outbound bytes truncated"));
        }
        let json_bytes = &outbound_bytes[4..4 + json_len];

        let message = ProtocolMessage::from_bytes(json_bytes)
            .context("failed to decode framed JSON header as ProtocolMessage")?;
        match message {
            ProtocolMessage::Clipboard(clipboard_message) => {
                self.captured_messages
                    .lock()
                    .map_err(|_| anyhow!("lock poisoned"))?
                    .push(clipboard_message);
                Ok(())
            }
            _ => Err(anyhow!("expected ProtocolMessage::Clipboard")),
        }
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

    async fn ensure_business_path(&self, _peer_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl PeerDirectoryPort for CapturingNetwork {
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
impl PairingTransportPort for CapturingNetwork {
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
impl NetworkEventPort for CapturingNetwork {
    async fn subscribe_events(&self) -> anyhow::Result<mpsc::Receiver<NetworkEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }
}

fn file_snapshot(ts_ms: i64) -> SystemClipboardSnapshot {
    // Snapshot with both text/uri-list (file URIs) and text/plain representations.
    // This simulates the common case where clipboard contains file paths alongside
    // a text fallback — classify_snapshot may classify as Text, bypassing the
    // file_sync_enabled guard in sync_policy.
    SystemClipboardSnapshot {
        ts_ms,
        representations: vec![
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"/tmp/test.txt".to_vec(),
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                b"file:///tmp/test.txt\n".to_vec(),
            ),
        ],
    }
}

/// When file_sync_enabled=false, outbound clipboard sync must NOT carry
/// file_transfers metadata (Stage 1 guard). This test verifies that passing
/// empty file_transfers (as the runtime does when the setting is off) results
/// in the sent ClipboardMessage having no file_transfers.
#[tokio::test]
async fn test_outbound_file_transfers_empty_when_file_sync_disabled() -> Result<()> {
    let clipboard_a = Arc::new(InMemoryClipboard::new(text_snapshot("", 0)));

    let session_a: Arc<dyn EncryptionSessionPort> = Arc::new(ReadyEncryptionSession);
    let identity_a: Arc<dyn DeviceIdentityPort> = Arc::new(StaticDeviceIdentity {
        id: DeviceId::new("device-a"),
    });

    let mut settings_with_file_sync_disabled = Settings::default();
    settings_with_file_sync_disabled.file_sync.file_sync_enabled = false;
    let settings: Arc<dyn SettingsPort> = Arc::new(StaticSettings {
        settings: settings_with_file_sync_disabled,
    });

    let captured_messages = Arc::new(Mutex::new(Vec::<ClipboardMessage>::new()));

    let network_a = Arc::new(CapturingNetwork {
        local_peer_id: "peer-a".to_string(),
        remote_peer: ConnectedPeer {
            peer_id: "peer-b".to_string(),
            device_name: "Device B".to_string(),
            connected_at: Utc::now(),
        },
        captured_messages: captured_messages.clone(),
    });

    let transfer_encryptor: Arc<TransferPayloadEncryptorAdapter> =
        Arc::new(TransferPayloadEncryptorAdapter);
    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a,
        network_a.clone() as Arc<dyn ClipboardTransportPort>,
        network_a.clone() as Arc<dyn PeerDirectoryPort>,
        session_a,
        identity_a,
        settings,
        transfer_encryptor,
        Arc::new(NoopPairedDeviceRepo),
    );

    // Simulate what runtime.rs does when file_sync_enabled=false:
    // it passes empty file_transfers even though the snapshot contains file URIs.
    let snapshot = file_snapshot(1_713_000_000_001);
    tokio::task::spawn_blocking(move || {
        outbound_a.execute(
            snapshot,
            ClipboardChangeOrigin::LocalCapture,
            None,
            vec![], // empty — runtime gates this when file_sync_enabled=false
        )
    })
    .await
    .map_err(|e| anyhow!("failed to join outbound A task: {e}"))??;

    let messages = captured_messages
        .lock()
        .map_err(|_| anyhow!("lock poisoned"))?;
    assert!(
        messages.is_empty(),
        "no messages should be sent when file_sync_enabled=false and snapshot is a file copy, got {} messages",
        messages.len()
    );

    Ok(())
}

/// Verify that file_transfers ARE populated when file_sync_enabled=true (default).
/// This is the positive counterpart to the disabled test above.
#[tokio::test]
async fn test_outbound_file_transfers_present_when_file_sync_enabled() -> Result<()> {
    let clipboard_a = Arc::new(InMemoryClipboard::new(text_snapshot("", 0)));

    let session_a: Arc<dyn EncryptionSessionPort> = Arc::new(ReadyEncryptionSession);
    let identity_a: Arc<dyn DeviceIdentityPort> = Arc::new(StaticDeviceIdentity {
        id: DeviceId::new("device-a"),
    });

    // Default settings have file_sync_enabled=true
    let settings: Arc<dyn SettingsPort> = Arc::new(StaticSettings {
        settings: Settings::default(),
    });

    let captured_messages = Arc::new(Mutex::new(Vec::<ClipboardMessage>::new()));

    let network_a = Arc::new(CapturingNetwork {
        local_peer_id: "peer-a".to_string(),
        remote_peer: ConnectedPeer {
            peer_id: "peer-b".to_string(),
            device_name: "Device B".to_string(),
            connected_at: Utc::now(),
        },
        captured_messages: captured_messages.clone(),
    });

    let transfer_encryptor: Arc<TransferPayloadEncryptorAdapter> =
        Arc::new(TransferPayloadEncryptorAdapter);
    let outbound_a = SyncOutboundClipboardUseCase::new(
        clipboard_a,
        network_a.clone() as Arc<dyn ClipboardTransportPort>,
        network_a.clone() as Arc<dyn PeerDirectoryPort>,
        session_a,
        identity_a,
        settings,
        transfer_encryptor,
        Arc::new(NoopPairedDeviceRepo),
    );

    // Simulate what runtime.rs does when file_sync_enabled=true:
    // it extracts file paths and builds file_transfers.
    let file_transfers = vec![FileTransferMapping {
        transfer_id: "tf-001".to_string(),
        filename: "test.txt".to_string(),
    }];
    let snapshot = file_snapshot(1_713_000_000_001);
    tokio::task::spawn_blocking(move || {
        outbound_a.execute(
            snapshot,
            ClipboardChangeOrigin::LocalCapture,
            None,
            file_transfers,
        )
    })
    .await
    .map_err(|e| anyhow!("failed to join outbound A task: {e}"))??;

    let messages = captured_messages
        .lock()
        .map_err(|_| anyhow!("lock poisoned"))?;
    assert_eq!(messages.len(), 1, "expected exactly one sent message");
    assert_eq!(
        messages[0].file_transfers.len(),
        1,
        "file_transfers should contain the mapping when file_sync_enabled=true"
    );
    assert_eq!(messages[0].file_transfers[0].transfer_id, "tf-001");
    assert_eq!(messages[0].file_transfers[0].filename, "test.txt");

    Ok(())
}
