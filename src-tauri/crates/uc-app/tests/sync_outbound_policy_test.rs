use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::mpsc;
use uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase;
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, PairedDevice, PairingState,
};
use uc_core::ports::{
    ClipboardTransportPort, DeviceIdentityPort, EncryptionSessionPort, PairedDeviceRepositoryError,
    PairedDeviceRepositoryPort, PeerDirectoryPort, SettingsPort, SystemClipboardPort,
    TransferCryptoError, TransferPayloadEncryptorPort,
};
use uc_core::security::model::{EncryptionError, MasterKey};
use uc_core::settings::model::{Settings, SyncSettings};
use uc_core::{
    DeviceId, MimeType, ObservedClipboardRepresentation, PeerId, SystemClipboardSnapshot,
};

struct NoopClipboard;

impl SystemClipboardPort for NoopClipboard {
    fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
        Ok(text_snapshot())
    }

    fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NoopClipboardTransport;

#[async_trait]
impl ClipboardTransportPort for NoopClipboardTransport {
    async fn send_clipboard(
        &self,
        _peer_id: &str,
        _encrypted_data: std::sync::Arc<[u8]>,
    ) -> anyhow::Result<()> {
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

    async fn ensure_business_path(&self, _peer_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NoopPeerDirectory;

#[async_trait]
impl PeerDirectoryPort for NoopPeerDirectory {
    async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        Ok(vec![])
    }

    async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
        Ok(vec![])
    }

    fn local_peer_id(&self) -> String {
        "local-peer".to_string()
    }

    async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
        Ok(())
    }
}

struct ReadyEncryptionSession;

#[async_trait]
impl EncryptionSessionPort for ReadyEncryptionSession {
    async fn is_ready(&self) -> bool {
        true
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

struct StaticDeviceIdentity;

impl DeviceIdentityPort for StaticDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        DeviceId::new("device-a")
    }
}

struct PassthroughTransferEncryptor;

impl TransferPayloadEncryptorPort for PassthroughTransferEncryptor {
    fn encrypt(
        &self,
        _master_key: &MasterKey,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, TransferCryptoError> {
        Ok(plaintext.to_vec())
    }
}

struct ConfigurableSettingsPort {
    settings: Arc<Mutex<Settings>>,
}

impl ConfigurableSettingsPort {
    fn new(initial: Settings) -> Self {
        Self {
            settings: Arc::new(Mutex::new(initial)),
        }
    }

    fn set_auto_sync(&self, enabled: bool) {
        let mut settings = self.settings.lock().expect("settings lock");
        settings.sync.auto_sync = enabled;
    }
}

#[async_trait]
impl SettingsPort for ConfigurableSettingsPort {
    async fn load(&self) -> anyhow::Result<Settings> {
        Ok(self.settings.lock().expect("settings lock").clone())
    }

    async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        *self.settings.lock().expect("settings lock") = settings.clone();
        Ok(())
    }
}

struct FailingSettingsPort;

#[async_trait]
impl SettingsPort for FailingSettingsPort {
    async fn load(&self) -> anyhow::Result<Settings> {
        Err(anyhow!("forced settings load failure"))
    }

    async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
        Ok(())
    }
}

struct TrackingPairedDeviceRepo {
    devices_by_peer: Arc<Mutex<HashMap<String, PairedDevice>>>,
    write_count: Arc<AtomicUsize>,
}

impl TrackingPairedDeviceRepo {
    fn new(devices: Vec<PairedDevice>) -> Self {
        let mut by_peer = HashMap::new();
        for device in devices {
            by_peer.insert(device.peer_id.as_str().to_string(), device);
        }
        Self {
            devices_by_peer: Arc::new(Mutex::new(by_peer)),
            write_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn write_count(&self) -> usize {
        self.write_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PairedDeviceRepositoryPort for TrackingPairedDeviceRepo {
    async fn get_by_peer_id(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
        let devices = self.devices_by_peer.lock().expect("repo lock");
        Ok(devices.get(peer_id.as_str()).cloned())
    }

    async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
        let devices = self.devices_by_peer.lock().expect("repo lock");
        Ok(devices.values().cloned().collect())
    }

    async fn upsert(&self, device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        self.devices_by_peer
            .lock()
            .expect("repo lock")
            .insert(device.peer_id.as_str().to_string(), device);
        Ok(())
    }

    async fn set_state(
        &self,
        _peer_id: &PeerId,
        _state: PairingState,
    ) -> Result<(), PairedDeviceRepositoryError> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn update_last_seen(
        &self,
        _peer_id: &PeerId,
        _last_seen_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn delete(&self, peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        self.devices_by_peer
            .lock()
            .expect("repo lock")
            .remove(peer_id.as_str());
        Ok(())
    }

    async fn update_sync_settings(
        &self,
        peer_id: &PeerId,
        settings: Option<SyncSettings>,
    ) -> Result<(), PairedDeviceRepositoryError> {
        self.write_count.fetch_add(1, Ordering::SeqCst);
        if let Some(device) = self
            .devices_by_peer
            .lock()
            .expect("repo lock")
            .get_mut(peer_id.as_str())
        {
            device.sync_settings = settings;
        }
        Ok(())
    }
}

fn build_use_case(
    settings: Arc<dyn SettingsPort>,
    paired_repo: Arc<TrackingPairedDeviceRepo>,
) -> SyncOutboundClipboardUseCase {
    SyncOutboundClipboardUseCase::new(
        Arc::new(NoopClipboard),
        Arc::new(NoopClipboardTransport),
        Arc::new(NoopPeerDirectory),
        Arc::new(ReadyEncryptionSession),
        Arc::new(StaticDeviceIdentity),
        settings,
        Arc::new(PassthroughTransferEncryptor),
        paired_repo,
    )
}

fn peer(peer_id: &str) -> DiscoveredPeer {
    DiscoveredPeer {
        peer_id: peer_id.to_string(),
        device_name: Some(format!("Device {peer_id}")),
        device_id: None,
        addresses: vec![],
        discovered_at: Utc::now(),
        last_seen: Utc::now(),
        is_paired: true,
    }
}

fn peers(ids: &[&str]) -> Vec<DiscoveredPeer> {
    ids.iter().map(|id| peer(id)).collect()
}

fn text_snapshot() -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            b"hello".to_vec(),
        )],
    }
}

fn with_global_auto_sync(enabled: bool) -> Settings {
    let mut settings = Settings::default();
    settings.sync.auto_sync = enabled;
    settings
}

fn paired_device(peer_id: &str, auto_sync: bool) -> PairedDevice {
    let mut sync_settings = Settings::default().sync;
    sync_settings.auto_sync = auto_sync;

    PairedDevice {
        peer_id: PeerId::from(peer_id),
        pairing_state: PairingState::Trusted,
        identity_fingerprint: format!("fp-{peer_id}"),
        paired_at: Utc::now(),
        last_seen_at: None,
        device_name: format!("Device {peer_id}"),
        sync_settings: Some(sync_settings),
    }
}

fn peer_ids(peers: &[DiscoveredPeer]) -> Vec<String> {
    peers.iter().map(|p| p.peer_id.clone()).collect()
}

#[tokio::test]
async fn sync_outbound_global_toggle() {
    let settings = Arc::new(ConfigurableSettingsPort::new(with_global_auto_sync(false)));
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![]));
    let use_case = build_use_case(settings, repo);
    let input_peers = peers(&["peer-a", "peer-b", "peer-c"]);

    let result = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;

    assert!(
        result.is_empty(),
        "global auto_sync=false must block all peers"
    );
}

#[tokio::test]
async fn sync_outbound_global_override() {
    let settings = Arc::new(ConfigurableSettingsPort::new(with_global_auto_sync(false)));
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![
        paired_device("peer-a", true),
        paired_device("peer-b", true),
    ]));
    let use_case = build_use_case(settings, repo);
    let input_peers = peers(&["peer-a", "peer-b"]);

    let result = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;

    assert!(
        result.is_empty(),
        "global auto_sync=false must override per-device auto_sync=true"
    );
}

#[tokio::test]
async fn sync_outbound_global_enabled() {
    let settings = Arc::new(ConfigurableSettingsPort::new(with_global_auto_sync(true)));
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![
        paired_device("peer-a", true),
        paired_device("peer-b", false),
    ]));
    let use_case = build_use_case(settings, repo);
    let input_peers = peers(&["peer-a", "peer-b", "peer-c"]);

    let result = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;

    assert_eq!(
        peer_ids(&result),
        vec!["peer-a".to_string(), "peer-c".to_string()]
    );
}

#[tokio::test]
async fn sync_outbound_settings_fallback() {
    let settings = Arc::new(FailingSettingsPort);
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![
        paired_device("peer-a", false),
        paired_device("peer-b", false),
    ]));
    let use_case = build_use_case(settings, repo);
    let input_peers = peers(&["peer-a", "peer-b", "peer-c"]);

    let result = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;

    assert_eq!(peer_ids(&result), vec!["peer-a", "peer-b", "peer-c"]);
}

#[tokio::test]
async fn sync_outbound_no_device_mutation() {
    let settings = Arc::new(ConfigurableSettingsPort::new(with_global_auto_sync(false)));
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![
        paired_device("peer-a", true),
        paired_device("peer-b", false),
    ]));
    let use_case = build_use_case(settings.clone(), repo.clone());
    let input_peers = peers(&["peer-a", "peer-b"]);

    let first = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;
    assert!(first.is_empty());

    settings.set_auto_sync(true);
    let _second = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;

    assert_eq!(
        repo.write_count(),
        0,
        "policy filtering must not mutate paired device settings"
    );
}

#[tokio::test]
async fn sync_outbound_resume() {
    let settings = Arc::new(ConfigurableSettingsPort::new(with_global_auto_sync(false)));
    let repo = Arc::new(TrackingPairedDeviceRepo::new(vec![
        paired_device("peer-a", true),
        paired_device("peer-b", false),
        paired_device("peer-c", true),
    ]));
    let use_case = build_use_case(settings.clone(), repo);
    let input_peers = peers(&["peer-a", "peer-b", "peer-c"]);

    let blocked = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;
    assert!(
        blocked.is_empty(),
        "global auto_sync=false must block outbound sync"
    );

    settings.set_auto_sync(true);
    let resumed = use_case
        .apply_sync_policy(&input_peers, &text_snapshot())
        .await;
    assert_eq!(
        peer_ids(&resumed),
        vec!["peer-a".to_string(), "peer-c".to_string()]
    );
}
