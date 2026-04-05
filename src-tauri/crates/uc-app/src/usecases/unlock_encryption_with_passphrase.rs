//! Unlock encryption session using a user-provided passphrase.
//!
//! This use case derives the KEK from the passphrase + stored salt/kdf params,
//! unwraps the MasterKey, and sets it in the EncryptionSessionPort for
//! transparent encryption.
//!
//! ## Difference from AutoUnlockEncryptionSession
//!
//! - **AutoUnlock**: Loads KEK from keyring (no user input required).
//! - **UnlockWithPassphrase**: Derives KEK from user-provided passphrase.

use std::sync::Arc;
use tracing::{info, info_span, Instrument};

use uc_core::{
    ports::{
        security::{
            encryption_state::EncryptionStatePort,
            key_scope::{KeyScopePort, ScopeError},
        },
        EncryptionPort, EncryptionSessionPort, KeyMaterialPort,
    },
    security::{
        model::{EncryptionError, Passphrase},
        state::{EncryptionState, EncryptionStateError},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum UnlockWithPassphraseError {
    #[error("encryption state check failed: {0}")]
    StateCheckFailed(#[source] EncryptionStateError),

    #[error("encryption not initialized — cannot unlock with passphrase")]
    NotInitialized,

    #[error("key scope resolution failed: {0}")]
    ScopeFailed(#[source] ScopeError),

    #[error("failed to load keyslot: {0}")]
    KeySlotLoadFailed(#[source] EncryptionError),

    #[error("keyslot has no wrapped master key")]
    MissingWrappedMasterKey,

    #[error("failed to derive KEK from passphrase: {0}")]
    KekDeriveFailed(#[source] EncryptionError),

    #[error("failed to unwrap master key: {0}")]
    UnwrapFailed(#[source] EncryptionError),

    #[error("failed to set master key in session: {0}")]
    SessionSetFailed(#[source] EncryptionError),
}

/// Use case for unlocking encryption with a user-provided passphrase.
///
/// ## Behavior
///
/// 1. Check encryption state — must be `Initialized` (returns `NotInitialized` if not)
/// 2. Resolve current key scope
/// 3. Load keyslot from storage
/// 4. Derive KEK from the provided passphrase + stored salt + kdf params
/// 5. Unwrap the MasterKey using the derived KEK
/// 6. Set the MasterKey in the encryption session
///
/// Returns `Ok(())` on success.
///
/// ## Errors
///
/// - `NotInitialized` — encryption has not been initialized
/// - `ScopeFailed` — could not resolve key scope
/// - `KeySlotLoadFailed` — could not load keyslot from storage
/// - `MissingWrappedMasterKey` — keyslot exists but has no wrapped master key
/// - `KekDeriveFailed` — KDF failed (wrong passphrase is one cause)
/// - `UnwrapFailed` — unwrap failed (wrong passphrase is one cause)
/// - `SessionSetFailed` — could not set master key in session
pub struct UnlockEncryptionWithPassphrase {
    encryption_state: Arc<dyn EncryptionStatePort>,
    key_scope: Arc<dyn KeyScopePort>,
    key_material: Arc<dyn KeyMaterialPort>,
    encryption: Arc<dyn EncryptionPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
}

impl UnlockEncryptionWithPassphrase {
    pub fn new(
        encryption_state: Arc<dyn EncryptionStatePort>,
        key_scope: Arc<dyn KeyScopePort>,
        key_material: Arc<dyn KeyMaterialPort>,
        encryption: Arc<dyn EncryptionPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
    ) -> Self {
        Self {
            encryption_state,
            key_scope,
            key_material,
            encryption,
            encryption_session,
        }
    }

    pub fn from_ports(
        encryption_state: Arc<dyn EncryptionStatePort>,
        key_scope: Arc<dyn KeyScopePort>,
        key_material: Arc<dyn KeyMaterialPort>,
        encryption: Arc<dyn EncryptionPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
    ) -> Self {
        Self::new(
            encryption_state,
            key_scope,
            key_material,
            encryption,
            encryption_session,
        )
    }

    /// Execute the passphrase unlock flow.
    ///
    /// # Returns
    ///
    /// - `Ok(())` — Session unlocked successfully
    /// - `Err(UnlockWithPassphraseError)` — Unlock failed
    ///
    /// # Note on passphrase errors
    ///
    /// Both `KekDeriveFailed` and `UnwrapFailed` can occur when the wrong
    /// passphrase is provided. The underlying crypto may fail at the KDF stage
    /// (Argon2id password hashing) or at the AEAD unwrap stage (wrong key).
    pub async fn execute(&self, passphrase: Passphrase) -> Result<(), UnlockWithPassphraseError> {
        let span = info_span!("usecase.unlock_encryption_with_passphrase.execute");

        async {
            info!("Checking encryption state for passphrase unlock");

            // 1. Check encryption state
            let state = self
                .encryption_state
                .load_state()
                .await
                .map_err(UnlockWithPassphraseError::StateCheckFailed)?;

            if state == EncryptionState::Uninitialized {
                info!("Encryption not initialized, cannot unlock with passphrase");
                return Err(UnlockWithPassphraseError::NotInitialized);
            }

            info!("Encryption initialized, attempting passphrase unlock");

            // 2. Get key scope
            let scope = self
                .key_scope
                .current_scope()
                .await
                .map_err(UnlockWithPassphraseError::ScopeFailed)?;

            // 3. Load keyslot
            let keyslot = self
                .key_material
                .load_keyslot(&scope)
                .await
                .map_err(UnlockWithPassphraseError::KeySlotLoadFailed)?;

            // 4. Get wrapped master key
            let wrapped_master_key = keyslot
                .wrapped_master_key
                .ok_or(UnlockWithPassphraseError::MissingWrappedMasterKey)?;

            // 5. Derive KEK from passphrase + salt + kdf params
            let kek = self
                .encryption
                .derive_kek(&passphrase, &keyslot.salt, &keyslot.kdf)
                .await
                .map_err(UnlockWithPassphraseError::KekDeriveFailed)?;

            // 6. Unwrap master key
            let master_key = self
                .encryption
                .unwrap_master_key(&kek, &wrapped_master_key.blob)
                .await
                .map_err(UnlockWithPassphraseError::UnwrapFailed)?;

            // 7. Set master key in session
            self.encryption_session
                .set_master_key(master_key)
                .await
                .map_err(UnlockWithPassphraseError::SessionSetFailed)?;

            info!("Passphrase unlock completed successfully");
            Ok(())
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use uc_core::{
        ports::security::key_scope::ScopeError,
        security::model::{
            EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KdfParams, Kek, KeyScope,
            MasterKey, WrappedMasterKey,
        },
        security::state::EncryptionStateError,
    };

    // ---------------------------------------------------------------------------
    // Mock implementations
    // ---------------------------------------------------------------------------

    struct MockEncryptionState {
        state: EncryptionState,
    }

    impl MockEncryptionState {
        fn new(state: EncryptionState) -> Self {
            Self { state }
        }
    }

    #[async_trait]
    impl EncryptionStatePort for MockEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Ok(self.state.clone())
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }

        async fn clear_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    struct MockKeyScope {
        scope: Option<KeyScope>,
    }

    impl MockKeyScope {
        fn succeed_with(scope: KeyScope) -> Self {
            Self { scope: Some(scope) }
        }

        fn fail() -> Self {
            Self { scope: None }
        }
    }

    #[async_trait]
    impl KeyScopePort for MockKeyScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            self.scope
                .clone()
                .ok_or(ScopeError::FailedToGetCurrentScope)
        }
    }

    struct MockKeyMaterial {
        keyslot: Option<uc_core::security::model::KeySlot>,
    }

    impl MockKeyMaterial {
        fn new() -> Self {
            Self { keyslot: None }
        }

        fn with_keyslot(mut self, keyslot: uc_core::security::model::KeySlot) -> Self {
            self.keyslot = Some(keyslot);
            self
        }
    }

    #[async_trait]
    impl KeyMaterialPort for MockKeyMaterial {
        async fn load_keyslot(
            &self,
            _scope: &KeyScope,
        ) -> Result<uc_core::security::model::KeySlot, EncryptionError> {
            self.keyslot.clone().ok_or(EncryptionError::KeyNotFound)
        }

        async fn store_keyslot(
            &self,
            _keyslot: &uc_core::security::model::KeySlot,
        ) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct MockEncryption {
        should_fail_derive: bool,
        should_fail_unwrap: bool,
    }

    impl MockEncryption {
        fn new() -> Self {
            Self {
                should_fail_derive: false,
                should_fail_unwrap: false,
            }
        }

        fn fail_on_unwrap(mut self) -> Self {
            self.should_fail_unwrap = true;
            self
        }
    }

    #[async_trait]
    impl EncryptionPort for MockEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf_params: &KdfParams,
        ) -> Result<Kek, EncryptionError> {
            if self.should_fail_derive {
                return Err(EncryptionError::KdfFailed);
            }
            Ok(Kek([0u8; 32]))
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
                ciphertext: vec![0u8; 32],
                aad_fingerprint: None,
            })
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _blob: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            if self.should_fail_unwrap {
                return Err(EncryptionError::WrongPassphrase);
            }
            MasterKey::from_bytes(&[0u8; 32])
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _algo: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![],
                aad_fingerprint: None,
            })
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Ok(vec![])
        }
    }

    struct MockEncryptionSession {
        should_fail_set: bool,
        master_key_set: Arc<std::sync::atomic::AtomicBool>,
    }

    impl MockEncryptionSession {
        fn new() -> Self {
            Self {
                should_fail_set: false,
                master_key_set: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }

        fn fail_on_set(mut self) -> Self {
            self.should_fail_set = true;
            self
        }

        fn was_master_key_set(&self) -> bool {
            self.master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            if self
                .master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                MasterKey::from_bytes(&[0u8; 32])
            } else {
                Err(EncryptionError::Locked)
            }
        }

        async fn set_master_key(&self, _master_key: MasterKey) -> Result<(), EncryptionError> {
            if self.should_fail_set {
                return Err(EncryptionError::CryptoFailure);
            }
            self.master_key_set
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            self.master_key_set
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    // ---------------------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------------------

    fn create_test_keyslot(scope: KeyScope) -> uc_core::security::model::KeySlot {
        uc_core::security::model::KeySlot {
            version: uc_core::security::model::KeySlotVersion::V1,
            scope,
            kdf: KdfParams::for_initialization(),
            salt: vec![0u8; 16],
            wrapped_master_key: Some(WrappedMasterKey {
                blob: EncryptedBlob {
                    version: EncryptionFormatVersion::V1,
                    aead: EncryptionAlgo::XChaCha20Poly1305,
                    nonce: vec![0u8; 24],
                    ciphertext: vec![0u8; 32],
                    aad_fingerprint: None,
                },
            }),
        }
    }

    fn make_use_case(
        state: Arc<dyn EncryptionStatePort>,
        scope: Arc<dyn KeyScopePort>,
        key_material: Arc<dyn KeyMaterialPort>,
        encryption: Arc<dyn EncryptionPort>,
        session: Arc<dyn EncryptionSessionPort>,
    ) -> UnlockEncryptionWithPassphrase {
        UnlockEncryptionWithPassphrase::new(state, scope, key_material, encryption, session)
    }

    // ---------------------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_unlock_returns_not_initialized_error_when_uninitialized() {
        // When encryption state is Uninitialized, should return NotInitialized error
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Uninitialized));
        let scope = Arc::new(MockKeyScope::succeed_with(KeyScope {
            profile_id: "test".to_string(),
        }));
        let key_material = Arc::new(MockKeyMaterial::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(
            result.is_err(),
            "should fail when encryption is uninitialized"
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::NotInitialized),
            "error should be NotInitialized, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_succeeds_on_happy_path() {
        // When all dependencies succeed, unlock should succeed
        let scope_value = KeyScope {
            profile_id: "test".to_string(),
        };
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::succeed_with(scope_value.clone()));
        let key_material =
            Arc::new(MockKeyMaterial::new().with_keyslot(create_test_keyslot(scope_value)));
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session.clone());
        let result = use_case
            .execute(Passphrase("correct-passphrase".to_string()))
            .await;

        assert!(result.is_ok(), "should succeed on happy path");
        assert!(
            session.was_master_key_set(),
            "master key should be set in session"
        );
    }

    #[tokio::test]
    async fn test_unlock_fails_with_wrong_passphrase() {
        // When the passphrase is wrong, unwrap should fail with WrongPassphrase
        let scope_value = KeyScope {
            profile_id: "test".to_string(),
        };
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::succeed_with(scope_value.clone()));
        let key_material =
            Arc::new(MockKeyMaterial::new().with_keyslot(create_test_keyslot(scope_value)));
        let encryption = Arc::new(MockEncryption::new().fail_on_unwrap());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("wrong-passphrase".to_string()))
            .await;

        assert!(result.is_err(), "should fail with wrong passphrase");
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::UnwrapFailed(_)),
            "error should be UnwrapFailed, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_fails_when_keyslot_load_fails() {
        // When keyslot load fails, should return KeySlotLoadFailed error
        let scope_value = KeyScope {
            profile_id: "test".to_string(),
        };
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::succeed_with(scope_value));
        let key_material = Arc::new(MockKeyMaterial::new()); // No keyslot = load fails
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(result.is_err(), "should fail when keyslot load fails");
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::KeySlotLoadFailed(_)),
            "error should be KeySlotLoadFailed, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_fails_when_wrapped_master_key_is_missing() {
        // When keyslot exists but has no wrapped master key, should return MissingWrappedMasterKey
        let scope_value = KeyScope {
            profile_id: "test".to_string(),
        };
        let mut keyslot = create_test_keyslot(KeyScope {
            profile_id: "test".to_string(),
        });
        keyslot.wrapped_master_key = None; // Remove wrapped master key

        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::succeed_with(scope_value));
        let key_material = Arc::new(MockKeyMaterial::new().with_keyslot(keyslot));
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(
            result.is_err(),
            "should fail when wrapped master key is missing"
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::MissingWrappedMasterKey),
            "error should be MissingWrappedMasterKey, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_fails_when_scope_resolution_fails() {
        // When scope resolution fails, should return ScopeFailed error
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::fail());
        let key_material = Arc::new(MockKeyMaterial::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(result.is_err(), "should fail when scope resolution fails");
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::ScopeFailed(_)),
            "error should be ScopeFailed, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_fails_when_session_set_fails() {
        // When session set fails, should return SessionSetFailed error
        let scope_value = KeyScope {
            profile_id: "test".to_string(),
        };
        let state = Arc::new(MockEncryptionState::new(EncryptionState::Initialized));
        let scope = Arc::new(MockKeyScope::succeed_with(scope_value.clone()));
        let key_material =
            Arc::new(MockKeyMaterial::new().with_keyslot(create_test_keyslot(scope_value)));
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new().fail_on_set());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(result.is_err(), "should fail when session set fails");
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::SessionSetFailed(_)),
            "error should be SessionSetFailed, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_unlock_propagates_state_check_error() {
        // When state check fails, should return StateCheckFailed error
        struct FailingState;

        #[async_trait]
        impl EncryptionStatePort for FailingState {
            async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
                Err(EncryptionStateError::LoadError(
                    "state check failed".to_string(),
                ))
            }

            async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
                Ok(())
            }

            async fn clear_initialized(&self) -> Result<(), EncryptionStateError> {
                Ok(())
            }
        }

        let state = Arc::new(FailingState);
        let scope = Arc::new(MockKeyScope::succeed_with(KeyScope {
            profile_id: "test".to_string(),
        }));
        let key_material = Arc::new(MockKeyMaterial::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let use_case = make_use_case(state, scope, key_material, encryption, session);
        let result = use_case
            .execute(Passphrase("test-passphrase".to_string()))
            .await;

        assert!(result.is_err(), "should fail when state check fails");
        let err = result.unwrap_err();
        assert!(
            matches!(err, UnlockWithPassphraseError::StateCheckFailed(_)),
            "error should be StateCheckFailed, got: {}",
            err
        );
    }
}
