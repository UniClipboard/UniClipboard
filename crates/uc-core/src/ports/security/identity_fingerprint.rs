//! Identity fingerprint factory port.
//!
//! Derives a stable `IdentityFingerprint` from a raw identity public key.
//! Used during pairing for out-of-band verification. The concrete derivation
//! (SHA-256 + Base32 grouping) lives in `uc-infra`.

use anyhow::Result;

use crate::security::IdentityFingerprint;

pub trait IdentityFingerprintFactoryPort: Send + Sync {
    fn from_public_key(&self, public_key: &[u8]) -> Result<IdentityFingerprint>;
}
