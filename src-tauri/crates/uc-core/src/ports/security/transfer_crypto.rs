use crate::security::MasterKey;

/// Error type for transfer payload encryption/decryption operations.
///
/// These variants describe business-level failures without exposing
/// wire-format details (e.g., chunked encoding, magic bytes, header layout).
#[derive(Debug, thiserror::Error)]
pub enum TransferCryptoError {
    #[error("transfer payload encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("transfer payload decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("invalid transfer payload format: {0}")]
    InvalidFormat(String),
}

/// Port for encrypting plaintext into a transfer-ready payload.
///
/// Implementations may use any internal encoding strategy (e.g., chunked AEAD,
/// single-shot encryption). The caller only cares about encrypt/decrypt symmetry.
pub trait TransferPayloadEncryptorPort: Send + Sync {
    fn encrypt(
        &self,
        master_key: &MasterKey,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, TransferCryptoError>;
}

/// Port for decrypting a transfer payload back to plaintext.
pub trait TransferPayloadDecryptorPort: Send + Sync {
    fn decrypt(
        &self,
        encrypted: &[u8],
        master_key: &MasterKey,
    ) -> Result<Vec<u8>, TransferCryptoError>;
}
