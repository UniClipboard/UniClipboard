//! PIN hashing port.
//!
//! Abstracts the password-hashing algorithm used to protect pairing PINs
//! at rest. Concrete implementation (Argon2id) lives in `uc-infra`.

use anyhow::Result;

pub trait PinHasherPort: Send + Sync {
    /// Hash a PIN, producing an opaque encoded byte string suitable for
    /// later verification.
    fn hash(&self, pin: &str) -> Result<Vec<u8>>;

    /// Verify a PIN against a previously produced encoded hash.
    fn verify(&self, pin: &str, encoded: &[u8]) -> Result<bool>;
}
