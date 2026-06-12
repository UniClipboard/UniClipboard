use anyhow::Result;
use blake3;
use uc_core::{ports::ContentHashPort, ContentHash};

pub struct Blake3Hasher;

impl ContentHashPort for Blake3Hasher {
    fn hash_bytes(&self, bytes: &[u8]) -> Result<ContentHash> {
        let hash = blake3::hash(bytes);
        Ok(ContentHash {
            alg: uc_core::HashAlgorithm::Blake3V1,
            bytes: hash.into(),
        })
    }
}
