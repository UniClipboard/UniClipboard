//! Argon2id-backed implementation of `PinHasherPort`.
//!
//! Thin adapter that delegates to the existing `hash_pin` / `verify_pin`
//! free functions in `pin_hash.rs`.

use anyhow::Result;
use uc_core::ports::security::PinHasherPort;

use crate::security::hashing::pin_hash::{hash_pin, verify_pin};

#[derive(Debug, Default, Clone, Copy)]
pub struct Argon2PinHasher;

impl PinHasherPort for Argon2PinHasher {
    fn hash(&self, pin: &str) -> Result<Vec<u8>> {
        hash_pin(pin)
    }

    fn verify(&self, pin: &str, encoded: &[u8]) -> Result<bool> {
        verify_pin(pin, encoded)
    }
}
