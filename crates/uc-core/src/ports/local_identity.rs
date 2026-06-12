//! Local device identity lifecycle port.
//!
//! Owns the long-term keypair that identifies this device on the overlay
//! network (iroh Ed25519 in the current adapter). The port only exposes the
//! resulting [`IdentityFingerprint`]; raw secret material stays inside the
//! adapter and never crosses the core boundary.
//!
//! Responsibilities are deliberately split from neighbouring ports:
//!
//! * [`DeviceIdentityPort`](super::DeviceIdentityPort) — UUID business id.
//! * [`IdentityFingerprintFactoryPort`](super::security::identity_fingerprint::IdentityFingerprintFactoryPort)
//!   — pure algorithm (public key → fingerprint).
//! * `LocalIdentityPort` (this file) — persistence/lifecycle of the local
//!   secret + fingerprint derived from it.

use async_trait::async_trait;
use thiserror::Error;

use crate::security::IdentityFingerprint;

/// Errors produced by [`LocalIdentityPort`].
#[derive(Debug, Error)]
pub enum LocalIdentityError {
    /// `create()` called while a local identity already exists.
    ///
    /// A1 (initialize) expects a clean slate; use `ensure()` from the B2
    /// (joiner) path if silent reuse is desired.
    #[error("local identity already exists")]
    AlreadyExists,

    /// Underlying secret-store (keychain / encrypted file) returned an error.
    #[error("local identity store error: {0}")]
    Storage(String),
}

/// Lifecycle owner for this device's long-term network identity.
///
/// Methods differ in their existence semantics:
///
/// | Method                     | When existing identity is found | When absent          |
/// |----------------------------|---------------------------------|----------------------|
/// | `create`                   | `Err(AlreadyExists)`            | generate + persist   |
/// | `ensure`                   | return existing fingerprint     | generate + persist   |
/// | `get_current_fingerprint`  | `Ok(Some(fp))`                  | `Ok(None)`           |
#[async_trait]
pub trait LocalIdentityPort: Send + Sync {
    /// Create a fresh local identity. Fails if one already exists.
    ///
    /// Used by A1 `InitializeSpaceUseCase` where we want loud failure on
    /// double-initialisation.
    async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError>;

    /// Idempotent variant of [`create`](Self::create) — returns the existing
    /// fingerprint if the identity is already persisted, otherwise generates
    /// and persists a new one.
    ///
    /// Used by B2 `RedeemPairingInvitationUseCase` so retries after mid-flow
    /// failures reuse the previously-generated key material.
    async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError>;

    /// Read the current fingerprint, if any. `Ok(None)` means no identity has
    /// been created yet (pre-A1/B2 state). Never generates new material.
    async fn get_current_fingerprint(
        &self,
    ) -> Result<Option<IdentityFingerprint>, LocalIdentityError>;
}
