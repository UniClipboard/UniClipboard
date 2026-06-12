use anyhow::Result;

use crate::ContentHash;

pub trait ContentHashPort: Send + Sync {
    fn hash_bytes(&self, bytes: &[u8]) -> Result<ContentHash>;
}
