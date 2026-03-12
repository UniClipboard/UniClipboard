//! Verify whether macOS Keychain "Always Allow" permission has been granted.
//!
//! This use case performs a lightweight check by calling `load_kek()` and
//! reporting whether the call succeeded silently (no user prompt), which
//! indicates that "Always Allow" was granted for this application.

use std::sync::Arc;
use tracing::{info, info_span, Instrument};

use uc_core::{
    ports::{security::key_scope::KeyScopePort, KeyMaterialPort},
    security::model::EncryptionError,
};

#[derive(Debug, thiserror::Error)]
pub enum VerifyKeychainError {
    #[error("key scope resolution failed: {0}")]
    ScopeFailed(String),

    #[error("KEK not found: encryption may not be properly initialized")]
    KekNotFound,

    #[error("unexpected keyring error: {0}")]
    Unexpected(String),
}

/// Use case for verifying macOS Keychain "Always Allow" permission.
///
/// ## Behavior
///
/// - Calls `load_kek()` to check if Keychain access succeeds silently
/// - `Ok(true)` — Keychain access succeeded (Always Allow granted)
/// - `Ok(false)` — Permission denied or keyring error (Always Allow not yet granted)
/// - `Err(KekNotFound)` — KEK not stored (encryption not properly initialized)
/// - `Err(ScopeFailed)` — Key scope resolution failed
/// - `Err(Unexpected)` — Unexpected error
pub struct VerifyKeychainAccess {
    key_scope: Arc<dyn KeyScopePort>,
    key_material: Arc<dyn KeyMaterialPort>,
}

impl VerifyKeychainAccess {
    pub fn new(key_scope: Arc<dyn KeyScopePort>, key_material: Arc<dyn KeyMaterialPort>) -> Self {
        Self {
            key_scope,
            key_material,
        }
    }

    pub fn from_ports(
        key_scope: Arc<dyn KeyScopePort>,
        key_material: Arc<dyn KeyMaterialPort>,
    ) -> Self {
        Self::new(key_scope, key_material)
    }

    /// Execute the keychain access verification.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` — Keychain access succeeded silently (Always Allow granted)
    /// - `Ok(false)` — Permission denied or keyring error
    /// - `Err(_)` — KEK not found or unexpected error
    pub async fn execute(&self) -> Result<bool, VerifyKeychainError> {
        let span = info_span!("usecase.verify_keychain_access.execute");

        async {
            info!("Verifying Keychain access for Always Allow permission");

            // 1. Get current key scope
            let scope = self
                .key_scope
                .current_scope()
                .await
                .map_err(|e| VerifyKeychainError::ScopeFailed(e.to_string()))?;

            // 2. Attempt to load KEK from keyring
            match self.key_material.load_kek(&scope).await {
                Ok(_) => {
                    info!("Keychain access succeeded silently — Always Allow granted");
                    Ok(true)
                }
                Err(EncryptionError::PermissionDenied) => {
                    info!("Keychain access denied — Always Allow not granted");
                    Ok(false)
                }
                Err(EncryptionError::KeyNotFound) => {
                    info!("KEK not found in keyring — encryption may not be initialized");
                    Err(VerifyKeychainError::KekNotFound)
                }
                Err(EncryptionError::KeyringError(_)) => {
                    info!("Keyring error — treating as Always Allow not granted");
                    Ok(false)
                }
                Err(other) => Err(VerifyKeychainError::Unexpected(other.to_string())),
            }
        }
        .instrument(span)
        .await
    }
}
