//!
//! Secure PIN hashing for pairing verification.
//!

use anyhow::{anyhow, ensure, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use subtle::ConstantTimeEq;

/// Current version of the encoded PIN hash format.
pub const HASH_VERSION: u8 = 0x01;

/// Size of the salt in bytes.
pub const SALT_SIZE: usize = 16;

/// Size of the hash output in bytes.
pub const HASH_SIZE: usize = 32;

/// Total size of the encoded hash (version + salt + hash).
pub const ENCODED_SIZE: usize = 1 + SALT_SIZE + HASH_SIZE;

fn argon_params() -> Result<Params> {
    Params::new(65536, 3, 4, None).map_err(|e| anyhow!("hardcoded Argon2 params are invalid: {e}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPinHash {
    pub version: u8,
    pub salt: [u8; SALT_SIZE],
    pub hash: [u8; HASH_SIZE],
}

impl EncodedPinHash {
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(ENCODED_SIZE);
        encoded.push(self.version);
        encoded.extend_from_slice(&self.salt);
        encoded.extend_from_slice(&self.hash);
        encoded
    }

    pub fn decode(encoded: &[u8]) -> Result<Self> {
        ensure!(
            encoded.len() == ENCODED_SIZE,
            "Invalid encoded hash length: expected {}, got {}",
            ENCODED_SIZE,
            encoded.len()
        );
        let version = encoded[0];
        ensure!(
            version == HASH_VERSION,
            "Unsupported hash version: {} (supported: {})",
            version,
            HASH_VERSION
        );

        let mut salt = [0u8; SALT_SIZE];
        salt.copy_from_slice(&encoded[1..1 + SALT_SIZE]);

        let mut hash = [0u8; HASH_SIZE];
        hash.copy_from_slice(&encoded[1 + SALT_SIZE..]);

        Ok(Self {
            version,
            salt,
            hash,
        })
    }
}

/// Hash a PIN using Argon2id with a random salt.
pub fn hash_pin(pin: &str) -> Result<Vec<u8>> {
    let salt = generate_salt();
    let hash = argon2id_hash(pin, &salt)?;

    Ok(EncodedPinHash {
        version: HASH_VERSION,
        salt,
        hash,
    }
    .encode())
}

/// Verify a PIN against an encoded hash.
pub fn verify_pin(pin: &str, encoded_hash: &[u8]) -> Result<bool> {
    let decoded = EncodedPinHash::decode(encoded_hash)?;
    let computed = argon2id_hash(pin, &decoded.salt)?;
    Ok(computed.ct_eq(&decoded.hash).into())
}

fn generate_salt() -> [u8; SALT_SIZE] {
    let mut salt = [0u8; SALT_SIZE];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut salt);
    salt
}

fn argon2id_hash(pin: &str, salt: &[u8; SALT_SIZE]) -> Result<[u8; HASH_SIZE]> {
    let mut output = [0u8; HASH_SIZE];
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params()?);
    argon
        .hash_password_into(pin.as_bytes(), salt, &mut output)
        .map_err(|e| anyhow!("Argon2id hashing failed: {e}"))?;
    Ok(output)
}
