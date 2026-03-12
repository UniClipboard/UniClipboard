//! Encryption-related Tauri commands
//! 加密相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::record_trace_fields;
use crate::events::EncryptionEvent;
use std::sync::Arc;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Runtime, State};
use tracing::{info, info_span, warn, Instrument};
use uc_platform::ports::observability::TraceMetadata;

const LOG_CONTEXT: &str = "[initialize_encryption]";
const UNLOCK_CONTEXT: &str = "[unlock_encryption_session]";

/// Event payload for onboarding-password-set event
#[derive(Debug, Clone, serde::Serialize)]
struct OnboardingPasswordSetEvent {
    timestamp: u64,
}

/// Encryption session status payload
/// 加密会话状态载荷
#[derive(Debug, Clone, serde::Serialize)]
pub struct EncryptionSessionStatus {
    initialized: bool,
    session_ready: bool,
}

/// Initialize encryption with passphrase
/// 使用密码短语初始化加密
///
/// This command uses the InitializeEncryption use case through the UseCases accessor.
/// 此命令通过 UseCases 访问器使用 InitializeEncryption 用例。
///
/// ## Architecture / 架构
///
/// - Commands layer (Driving Adapter) → UseCases accessor → Use Case → Ports
/// - Command triggers watcher start via WatcherControlPort after successful init
/// - 命令层（驱动适配器）→ UseCases 访问器 → 用例 → 端口
/// - 加密成功后通过 WatcherControlPort 启动监控器
#[tauri::command]
pub async fn initialize_encryption(
    runtime: State<'_, Arc<AppRuntime>>,
    app_handle: AppHandle,
    passphrase: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.encryption.initialize",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let uc = runtime.usecases().initialize_encryption();
    tracing::debug!("{} Use case created, executing...", LOG_CONTEXT);

    uc.execute(uc_core::security::model::Passphrase(passphrase))
        .instrument(span)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to initialize encryption");
            e.to_string()
        })?;
    tracing::info!("Encryption initialized successfully");

    if let Err(e) = runtime
        .usecases()
        .app_lifecycle_coordinator()
        .ensure_ready()
        .await
    {
        warn!("Failed to boot lifecycle after encryption init: {}", e);
    } else {
        info!("Lifecycle boot completed after encryption init");
    }

    // Emit onboarding-password-set event for frontend
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| format!("Failed to get timestamp: {}", e))?
        .as_millis() as u64;

    let event = OnboardingPasswordSetEvent { timestamp };
    app_handle
        .emit("onboarding-password-set", event)
        .map_err(|e| format!("Failed to emit event: {}", e))?;

    tracing::debug!("{} Event emitted successfully", LOG_CONTEXT);
    tracing::info!("Onboarding: encryption password initialized successfully");

    Ok(())
}

#[cfg(test)]
fn emit_session_ready<R: Runtime>(app_handle: &tauri::AppHandle<R>) -> Result<(), String> {
    app_handle
        .emit("encryption://event", EncryptionEvent::SessionReady)
        .map_err(|e| format!("emit session ready event failed: {}", e))
}

fn emit_session_failed<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    reason: String,
) -> Result<(), String> {
    app_handle
        .emit("encryption://event", EncryptionEvent::Failed { reason })
        .map_err(|e| format!("emit session failed event failed: {}", e))
}

pub async fn unlock_encryption_session_with_runtime<R: Runtime>(
    runtime: &Arc<AppRuntime>,
    app_handle: &AppHandle<R>,
    trace: Option<TraceMetadata>,
) -> Result<bool, String> {
    let span = info_span!(
        "command.encryption.unlock_session",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &trace);
    let uc = runtime.usecases().auto_unlock_encryption_session();
    info!("{} Attempting keyring unlock", UNLOCK_CONTEXT);
    async {
        match uc.execute().await {
            Ok(true) => {
                info!("{} Keyring unlock completed", UNLOCK_CONTEXT);
                if let Err(e) = runtime
                    .usecases()
                    .app_lifecycle_coordinator()
                    .ensure_ready()
                    .await
                {
                    warn!("{} Auto lifecycle boot failed: {}", UNLOCK_CONTEXT, e);
                } else {
                    info!("{} Auto lifecycle boot completed", UNLOCK_CONTEXT);
                }
                Ok(true)
            }
            Ok(false) => {
                info!(
                    "{} Encryption not initialized, unlock skipped",
                    UNLOCK_CONTEXT
                );
                Ok(false)
            }
            Err(err) => {
                let reason = err.to_string();
                warn!("{} Keyring unlock failed: {}", UNLOCK_CONTEXT, reason);
                if let Err(emit_err) = emit_session_failed(app_handle, reason.clone()) {
                    warn!(
                        "{} Failed to emit session failed event: {}",
                        UNLOCK_CONTEXT, emit_err
                    );
                }
                Err(reason)
            }
        }
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn unlock_encryption_session(
    runtime: State<'_, Arc<AppRuntime>>,
    app_handle: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<bool, String> {
    unlock_encryption_session_with_runtime(runtime.inner(), &app_handle, _trace).await
}

#[cfg(test)]
mod tests {
    use super::{emit_session_ready, unlock_encryption_session_with_runtime};
    use crate::bootstrap::AppRuntime;
    use crate::test_utils::{noop_network_ports, NoopPort};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tauri::Listener;
    use tokio::sync::mpsc;
    use uc_app::AppDeps;
    use uc_core::clipboard::ThumbnailMetadata;
    use uc_core::clipboard::{
        ClipboardSelection, ObservedClipboardRepresentation, PayloadAvailability,
        PersistedClipboardRepresentation, PolicyError, SelectionPolicyVersion,
        SystemClipboardSnapshot,
    };
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::network::{PairedDevice, PairingState};
    use uc_core::ports::clipboard::{
        ProcessingUpdateOutcome, RepresentationCachePort, SpoolQueuePort, SpoolRequest,
    };
    use uc_core::ports::errors::{DeviceRepositoryError, PairedDeviceRepositoryError};
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_scope::KeyScopePort;
    use uc_core::ports::*;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, Kek, KeyScope,
        KeySlot, KeySlotVersion, MasterKey, Passphrase, WrappedMasterKey,
    };
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::{Blob, BlobId, ClipboardChangeOrigin, ContentHash, DeviceId, PeerId};
    use uc_infra::clipboard::InMemoryClipboardChangeOrigin;
    use uc_platform::ports::{AutostartPort, UiPort, WatcherControlError, WatcherControlPort};
    #[tokio::test]
    async fn emit_session_ready_emits_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

        let tx_clone = tx.clone();
        app_handle.listen("encryption://event", move |event: tauri::Event| {
            let _ = tx_clone.try_send(event.payload().to_string());
        });

        emit_session_ready(&app_handle).expect("emit session ready event");

        let payload = rx.recv().await.expect("event payload");
        let value: Value = serde_json::from_str(&payload).expect("json payload");
        assert_eq!(value, serde_json::json!({ "type": "SessionReady" }));
    }

    struct NoopClipboard;
    struct MockDeviceIdentity;

    struct MockEncryptionState;
    struct MockKeyScope;
    struct MockKeyMaterial;
    struct MockEncryption;
    struct MockEncryptionSession;

    struct RecordingNetworkControl {
        calls: Arc<AtomicUsize>,
    }

    impl RecordingNetworkControl {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self { calls }
        }
    }

    fn sample_representation() -> PersistedClipboardRepresentation {
        let rep_id = RepresentationId::new();
        let format_id = FormatId::new();
        PersistedClipboardRepresentation::new_staged(rep_id, format_id, None, 0)
    }

    fn sample_selection() -> ClipboardSelection {
        let rep_id = RepresentationId::new();
        ClipboardSelection {
            primary_rep_id: rep_id.clone(),
            secondary_rep_ids: vec![],
            preview_rep_id: rep_id.clone(),
            paste_rep_id: rep_id,
            policy_version: SelectionPolicyVersion::V1,
        }
    }

    impl SystemClipboardPort for NoopClipboard {
        fn read_snapshot(&self) -> anyhow::Result<uc_core::clipboard::SystemClipboardSnapshot> {
            Ok(uc_core::clipboard::SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(
            &self,
            _snapshot: uc_core::clipboard::SystemClipboardSnapshot,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardEntryRepositoryPort for NoopPort {
        async fn save_entry_and_selection(
            &self,
            _entry: &uc_core::ClipboardEntry,
            _selection: &uc_core::ClipboardSelectionDecision,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_entry(
            &self,
            _entry_id: &uc_core::ids::EntryId,
        ) -> anyhow::Result<Option<uc_core::ClipboardEntry>> {
            Ok(None)
        }

        async fn list_entries(
            &self,
            _limit: usize,
            _offset: usize,
        ) -> anyhow::Result<Vec<uc_core::ClipboardEntry>> {
            Ok(vec![])
        }

        async fn touch_entry(
            &self,
            _entry_id: &uc_core::ids::EntryId,
            _active_time_ms: i64,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn delete_entry(&self, _entry_id: &uc_core::ids::EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardEventWriterPort for NoopPort {
        async fn insert_event(
            &self,
            _event: &uc_core::ClipboardEvent,
            _representations: &Vec<uc_core::PersistedClipboardRepresentation>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete_event_and_representations(
            &self,
            _event_id: &uc_core::ids::EventId,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for NoopPort {
        async fn get_representation(
            &self,
            _event_id: &uc_core::ids::EventId,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> anyhow::Result<ProcessingUpdateOutcome> {
            Ok(ProcessingUpdateOutcome::NotFound)
        }
    }

    #[async_trait]
    impl ClipboardRepresentationNormalizerPort for NoopPort {
        async fn normalize(
            &self,
            _observed: &ObservedClipboardRepresentation,
        ) -> anyhow::Result<PersistedClipboardRepresentation> {
            Ok(sample_representation())
        }
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for NoopPort {
        async fn get_selection(
            &self,
            _entry_id: &uc_core::ids::EntryId,
        ) -> anyhow::Result<Option<uc_core::ClipboardSelectionDecision>> {
            Ok(None)
        }

        async fn delete_selection(&self, _entry_id: &uc_core::ids::EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl SelectRepresentationPolicyPort for NoopPort {
        fn select(
            &self,
            _snapshot: &SystemClipboardSnapshot,
        ) -> Result<ClipboardSelection, PolicyError> {
            Ok(sample_selection())
        }
    }

    #[async_trait]
    impl RepresentationCachePort for NoopPort {
        async fn put(&self, _rep_id: &uc_core::ids::RepresentationId, _data: Vec<u8>) {}

        async fn get(&self, _rep_id: &uc_core::ids::RepresentationId) -> Option<Vec<u8>> {
            None
        }

        async fn mark_completed(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn mark_spooling(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn remove(&self, _rep_id: &uc_core::ids::RepresentationId) {}
    }

    #[async_trait]
    impl SpoolQueuePort for NoopPort {
        async fn enqueue(&self, _request: SpoolRequest) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl EncryptionPort for MockEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf: &uc_core::security::model::KdfParams,
        ) -> Result<Kek, EncryptionError> {
            Kek::from_bytes(&[0u8; 32])
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![0u8; 1],
                aad_fingerprint: None,
            })
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _wrapped: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            MasterKey::from_bytes(&[1u8; 32])
        }

        async fn encrypt_blob(
            &self,
            _key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![0u8; 1],
                aad_fingerprint: None,
            })
        }

        async fn decrypt_blob(
            &self,
            _key: &MasterKey,
            _blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Ok(vec![0u8; 1])
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            true
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn set_master_key(&self, _master_key: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl EncryptionStatePort for MockEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Ok(EncryptionState::Initialized)
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl KeyScopePort for MockKeyScope {
        async fn current_scope(
            &self,
        ) -> Result<KeyScope, uc_core::ports::security::key_scope::ScopeError> {
            Ok(KeyScope {
                profile_id: "default".to_string(),
            })
        }
    }

    #[async_trait]
    impl KeyMaterialPort for MockKeyMaterial {
        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Kek::from_bytes(&[0u8; 32])
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_keyslot(&self, scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
            let kdf = uc_core::security::model::KdfParams::for_initialization();
            let salt_len = kdf.salt_len();
            let wrapped = WrappedMasterKey {
                blob: EncryptedBlob {
                    version: EncryptionFormatVersion::V1,
                    aead: EncryptionAlgo::XChaCha20Poly1305,
                    nonce: vec![0u8; 24],
                    ciphertext: vec![0u8; 1],
                    aad_fingerprint: None,
                },
            };
            Ok(KeySlot {
                version: KeySlotVersion::V1,
                scope: scope.clone(),
                kdf,
                salt: vec![0u8; salt_len],
                wrapped_master_key: Some(wrapped),
            })
        }

        async fn store_keyslot(&self, _keyslot: &KeySlot) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl WatcherControlPort for NoopPort {
        async fn start_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }

        async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }
    }

    #[async_trait]
    impl DeviceRepositoryPort for NoopPort {
        async fn find_by_id(
            &self,
            _id: &DeviceId,
        ) -> Result<Option<uc_core::device::Device>, DeviceRepositoryError> {
            Ok(None)
        }

        async fn save(
            &self,
            _device: uc_core::device::Device,
        ) -> Result<(), DeviceRepositoryError> {
            Ok(())
        }

        async fn delete(&self, _id: &DeviceId) -> Result<(), DeviceRepositoryError> {
            Ok(())
        }

        async fn list_all(&self) -> Result<Vec<uc_core::device::Device>, DeviceRepositoryError> {
            Ok(vec![])
        }
    }

    impl DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("test-device")
        }
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPort {
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
            _last_seen_at: DateTime<Utc>,
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

    #[async_trait]
    impl NetworkControlPort for RecordingNetworkControl {
        async fn start_network(&self) -> anyhow::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait]
    impl uc_core::ports::SetupStatusPort for NoopPort {
        async fn get_status(&self) -> anyhow::Result<uc_core::setup::SetupStatus> {
            Ok(uc_core::setup::SetupStatus::default())
        }

        async fn set_status(&self, _status: &uc_core::setup::SetupStatus) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl SecureStoragePort for NoopPort {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, uc_core::ports::SecureStorageError> {
            Ok(None)
        }

        fn set(&self, _key: &str, _value: &[u8]) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BlobStorePort for NoopPort {
        async fn put(
            &self,
            _blob_id: &BlobId,
            _data: &[u8],
        ) -> anyhow::Result<(std::path::PathBuf, Option<i64>)> {
            Ok((std::path::PathBuf::from("/tmp/noop"), None))
        }

        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl BlobRepositoryPort for NoopPort {
        async fn insert_blob(&self, _blob: &Blob) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_hash(&self, _content_hash: &ContentHash) -> anyhow::Result<Option<Blob>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl BlobWriterPort for NoopPort {
        async fn write_if_absent(
            &self,
            _content_id: &ContentHash,
            _data: &[u8],
        ) -> anyhow::Result<Blob> {
            Err(anyhow::anyhow!("noop writer"))
        }
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for NoopPort {
        async fn get_by_representation_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<ThumbnailMetadata>> {
            Ok(None)
        }

        async fn insert_thumbnail(&self, _metadata: &ThumbnailMetadata) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ThumbnailGeneratorPort for NoopPort {
        async fn generate_thumbnail(
            &self,
            _image_bytes: &[u8],
        ) -> anyhow::Result<uc_core::ports::clipboard::GeneratedThumbnail> {
            Ok(uc_core::ports::clipboard::GeneratedThumbnail {
                thumbnail_bytes: vec![],
                thumbnail_mime_type: uc_core::clipboard::MimeType("image/png".to_string()),
                original_width: 0,
                original_height: 0,
            })
        }

        async fn generate_thumbnail_from_rgba(
            &self,
            _rgba_bytes: &[u8],
            _width: u32,
            _height: u32,
        ) -> anyhow::Result<uc_core::ports::clipboard::GeneratedThumbnail> {
            self.generate_thumbnail(&[]).await
        }
    }

    #[async_trait]
    impl SettingsPort for NoopPort {
        async fn load(&self) -> anyhow::Result<uc_core::settings::model::Settings> {
            Ok(uc_core::settings::model::Settings::default())
        }

        async fn save(&self, _settings: &uc_core::settings::model::Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl UiPort for NoopPort {
        async fn open_settings(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl AutostartPort for NoopPort {
        fn is_enabled(&self) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn enable(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn disable(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl ClockPort for NoopPort {
        fn now_ms(&self) -> i64 {
            0
        }
    }

    impl ContentHashPort for NoopPort {
        fn hash_bytes(&self, _bytes: &[u8]) -> anyhow::Result<ContentHash> {
            Err(anyhow::anyhow!("noop hash"))
        }
    }

    impl uc_core::ports::FileManagerPort for NoopPort {
        fn open_directory(
            &self,
            _path: &std::path::Path,
        ) -> Result<(), uc_core::ports::FileManagerError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl uc_core::ports::CacheFsPort for NoopPort {
        async fn exists(&self, _path: &std::path::Path) -> bool {
            false
        }
        async fn read_dir(
            &self,
            _path: &std::path::Path,
        ) -> anyhow::Result<Vec<uc_core::ports::CacheFsDirEntry>> {
            Ok(vec![])
        }
        async fn remove_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn dir_size(&self, _path: &std::path::Path) -> u64 {
            0
        }
    }

    fn test_storage_paths() -> uc_app::app_paths::AppPaths {
        uc_app::app_paths::AppPaths {
            db_path: std::path::PathBuf::from("/tmp/uniclipboard-test/uniclipboard.db"),
            vault_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/vault"),
            settings_path: std::path::PathBuf::from("/tmp/uniclipboard-test/settings.json"),
            logs_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/logs"),
            cache_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache"),
            app_data_root: std::path::PathBuf::from("/tmp/uniclipboard-test"),
        }
    }

    #[tokio::test]
    async fn unlock_success_triggers_network_start() {
        let start_calls = Arc::new(AtomicUsize::new(0));
        let (worker_tx, _worker_rx) = mpsc::channel(1);
        let origin_port = Arc::new(InMemoryClipboardChangeOrigin::new());
        origin_port
            .set_next_origin(
                ClipboardChangeOrigin::LocalRestore,
                std::time::Duration::from_secs(1),
            )
            .await;

        let deps = AppDeps {
            clipboard: uc_app::ClipboardPorts {
                clipboard: Arc::new(NoopClipboard),
                system_clipboard: Arc::new(NoopClipboard),
                clipboard_entry_repo: Arc::new(NoopPort),
                clipboard_event_repo: Arc::new(NoopPort),
                representation_repo: Arc::new(NoopPort),
                representation_normalizer: Arc::new(NoopPort),
                selection_repo: Arc::new(NoopPort),
                representation_policy: Arc::new(NoopPort),
                representation_cache: Arc::new(NoopPort),
                spool_queue: Arc::new(NoopPort),
                clipboard_change_origin: origin_port,
                worker_tx,
            },
            security: uc_app::SecurityPorts {
                encryption: Arc::new(MockEncryption),
                encryption_session: Arc::new(MockEncryptionSession),
                encryption_state: Arc::new(MockEncryptionState),
                key_scope: Arc::new(MockKeyScope),
                secure_storage: Arc::new(NoopPort),
                key_material: Arc::new(MockKeyMaterial),
            },
            device: uc_app::DevicePorts {
                device_repo: Arc::new(NoopPort),
                device_identity: Arc::new(MockDeviceIdentity),
                paired_device_repo: Arc::new(NoopPort),
            },
            network_ports: noop_network_ports(),
            network_control: Arc::new(RecordingNetworkControl::new(start_calls.clone())),
            setup_status: Arc::new(NoopPort),
            storage: uc_app::StoragePorts {
                blob_store: Arc::new(NoopPort),
                blob_repository: Arc::new(NoopPort),
                blob_writer: Arc::new(NoopPort),
                thumbnail_repo: Arc::new(NoopPort),
                thumbnail_generator: Arc::new(NoopPort),
            },
            settings: Arc::new(NoopPort),
            system: uc_app::SystemPorts {
                clock: Arc::new(NoopPort),
                hash: Arc::new(NoopPort),
                file_manager: Arc::new(NoopPort),
                cache_fs: Arc::new(NoopPort),
            },
        };

        let runtime = Arc::new(AppRuntime::new(deps, test_storage_paths()));
        let app = tauri::test::mock_app();
        let app_handle = app.handle();

        let unlocked = unlock_encryption_session_with_runtime(&runtime, &app_handle, None)
            .await
            .expect("unlock");

        assert!(unlocked, "expected unlock to succeed");
        assert_eq!(
            start_calls.load(Ordering::SeqCst),
            1,
            "network should start once"
        );
    }
}

/// Check if encryption is initialized
/// 检查加密是否已初始化
///
/// This command checks the encryption state port directly.
/// 此命令直接检查加密状态端口。
#[tauri::command]
pub async fn is_encryption_initialized(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<bool, String> {
    let span = info_span!(
        "command.encryption.is_initialized",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let state = runtime.encryption_state().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to check encryption status");
            e
        })?;
        let result = matches!(
            state,
            uc_core::security::state::EncryptionState::Initialized
        );

        tracing::info!(is_initialized = result, "Encryption status checked");
        Ok(result)
    }
    .instrument(span)
    .await
}

/// Get encryption session readiness
/// 获取加密会话就绪状态
///
/// This command reports whether encryption is initialized and whether the session is ready.
/// 此命令返回加密是否已初始化以及会话是否就绪。
#[tauri::command]
pub async fn get_encryption_session_status(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<EncryptionSessionStatus, String> {
    let span = info_span!(
        "command.encryption.session_status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async {
        let state = runtime.encryption_state().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to load encryption state");
            e
        })?;

        let session_ready = runtime.is_encryption_ready().await;
        let initialized = state == uc_core::security::state::EncryptionState::Initialized;

        tracing::info!(
            initialized,
            session_ready,
            "Encryption session status checked"
        );

        Ok(EncryptionSessionStatus {
            initialized,
            session_ready,
        })
    }
    .instrument(span)
    .await
}
