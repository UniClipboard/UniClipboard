//! Shared mobile LAN route test fixtures.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use uc_engine::{
    ActiveClipboardChanged, EngineError, MobileAuthenticatedSession, MobileCredential,
    MobileSyncDocument, MobileSyncDocumentApplyOutcome, MobileSyncFileReadOutcome,
};

use crate::mobile_lan::core::{MobileLanCore, MobileLanTestBackend};

#[derive(Clone)]
pub(crate) struct TestMobileLanBackend {
    username: String,
    password: String,
    auth_error: bool,
    credential_current: Arc<AtomicBool>,
    argon_hash: Option<String>,
    availability: Arc<Mutex<HashMap<String, bool>>>,
    next_upload: Arc<Mutex<u64>>,
}

impl TestMobileLanBackend {
    pub(crate) fn seeded(username: &str, password: &str) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            auth_error: false,
            credential_current: Arc::new(AtomicBool::new(true)),
            argon_hash: None,
            availability: Arc::new(Mutex::new(HashMap::new())),
            next_upload: Arc::new(Mutex::new(0)),
        }
    }

    fn auth_trap() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            auth_error: true,
            credential_current: Arc::new(AtomicBool::new(false)),
            argon_hash: None,
            availability: Arc::new(Mutex::new(HashMap::new())),
            next_upload: Arc::new(Mutex::new(0)),
        }
    }

    fn with_content_availability(self, snapshot_hash: String, available: bool) -> Self {
        self.availability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(snapshot_hash, available);
        self
    }

    pub(crate) fn revoke(&self) {
        self.credential_current.store(false, Ordering::Release);
    }
}

#[async_trait::async_trait]
impl MobileLanTestBackend for TestMobileLanBackend {
    async fn authenticate(
        &self,
        authorization: String,
    ) -> Result<Option<MobileAuthenticatedSession>, EngineError> {
        if self.auth_error {
            return Err(EngineError::new(
                1901,
                uc_engine::EngineErrorCategory::Internal,
                false,
            ));
        }

        let Some(encoded) = authorization.strip_prefix("Basic ") else {
            return Ok(None);
        };
        let Ok(decoded) = BASE64.decode(encoded) else {
            return Ok(None);
        };
        let Ok(candidate) = String::from_utf8(decoded) else {
            return Ok(None);
        };
        let Some((username, password)) = candidate.split_once(':') else {
            return Ok(None);
        };
        if username != self.username {
            return Ok(None);
        }

        if let Some(phc) = &self.argon_hash {
            let password = password.to_owned();
            let phc = phc.clone();
            let verified = tokio::task::spawn_blocking(move || {
                use argon2::password_hash::{PasswordHash, PasswordVerifier};

                PasswordHash::new(&phc)
                    .ok()
                    .and_then(|parsed| {
                        argon2::Argon2::default()
                            .verify_password(password.as_bytes(), &parsed)
                            .ok()
                    })
                    .is_some()
            })
            .await
            .map_err(|_| EngineError::new(1901, uc_engine::EngineErrorCategory::Internal, false))?;
            if !verified {
                return Ok(None);
            }
        } else if password != self.password {
            return Ok(None);
        }

        Ok(Some(MobileAuthenticatedSession {
            device_id: "did_seed".into(),
            client_type: uc_engine::MobileClientTypeSummary::IosShortcut,
            credential: MobileCredential::new("did_seed", "test-credential"),
        }))
    }

    async fn revalidate(&self, _credential: MobileCredential) -> Result<bool, EngineError> {
        Ok(!self.auth_error && self.credential_current.load(Ordering::Acquire))
    }

    async fn check_content_available(&self, snapshot_hash: String) -> Result<bool, EngineError> {
        Ok(*self
            .availability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&snapshot_hash)
            .unwrap_or(&false))
    }

    async fn latest_document(&self) -> Result<Option<MobileSyncDocument>, EngineError> {
        Ok(None)
    }

    async fn apply_document(
        &self,
        _input: uc_engine::ApplyMobileSyncDocumentInput,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        Err(EngineError::new(
            1901,
            uc_engine::EngineErrorCategory::Internal,
            false,
        ))
    }

    async fn read_file(
        &self,
        _data_name: String,
    ) -> Result<MobileSyncFileReadOutcome, EngineError> {
        Ok(MobileSyncFileReadOutcome::NotFound)
    }

    async fn begin_upload(
        &self,
        _data_name: String,
        _media_type: String,
        _source_device_id: String,
        _transfer_id: String,
        _total_bytes: Option<u64>,
    ) -> Result<u64, EngineError> {
        let mut next = self
            .next_upload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *next += 1;
        Ok(*next)
    }

    async fn append_upload(&self, _handle: u64, _bytes: Vec<u8>) -> Result<(), EngineError> {
        Ok(())
    }

    async fn finish_upload(
        &self,
        _handle: u64,
        _media_type: String,
    ) -> Result<MobileSyncDocumentApplyOutcome, EngineError> {
        Ok(MobileSyncDocumentApplyOutcome::Buffered)
    }

    async fn abort_upload(&self, _handle: u64) -> Result<bool, EngineError> {
        Ok(true)
    }
}

pub(crate) async fn build_test_core_with_seeded_device(
    username: &str,
    password: &str,
) -> MobileLanCore {
    MobileLanCore::with_test_backend(Arc::new(TestMobileLanBackend::seeded(username, password)))
}

pub(crate) async fn build_test_core_with_real_argon2(
    username: &str,
    password: &str,
) -> MobileLanCore {
    let password = password.to_owned();
    let hash = tokio::task::spawn_blocking(move || {
        use argon2::password_hash::{PasswordHasher, SaltString};
        use argon2::{Algorithm, Argon2, Params, Version};

        let salt = SaltString::encode_b64(&[7_u8; 16]).expect("fixed test salt must be valid");
        let params = Params::new(65_536, 3, 4, None).expect("fixed test parameters must be valid");
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password.as_bytes(), &salt)
            .expect("test password hash must succeed")
            .to_string()
    })
    .await
    .expect("test hash task must finish");
    let mut backend = TestMobileLanBackend::seeded(username, "");
    backend.argon_hash = Some(hash);
    MobileLanCore::with_test_backend(Arc::new(backend))
}

pub(crate) async fn build_test_core_with_content_availability(
    username: &str,
    password: &str,
    snapshot_hash: String,
    available: bool,
) -> MobileLanCore {
    MobileLanCore::with_test_backend(Arc::new(
        TestMobileLanBackend::seeded(username, password)
            .with_content_availability(snapshot_hash, available),
    ))
}

pub(crate) async fn build_test_core_with_auth_trap() -> MobileLanCore {
    MobileLanCore::with_test_backend(Arc::new(TestMobileLanBackend::auth_trap()))
}

pub(crate) fn auth_header(username: &str, password: &str) -> String {
    format!("Basic {}", BASE64.encode(format!("{username}:{password}")))
}

pub(crate) fn fake_cancel_token() -> CancellationToken {
    CancellationToken::new()
}

pub(crate) fn fake_sse_source() -> broadcast::Sender<ActiveClipboardChanged> {
    broadcast::channel(8).0
}
