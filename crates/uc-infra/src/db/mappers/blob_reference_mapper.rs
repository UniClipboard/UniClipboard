use anyhow::{anyhow, Result};

use uc_core::ports::blob::{BlobDigest, PlaintextHash};

use crate::db::models::{BlobReferenceRow, NewBlobReferenceRow};

pub struct BlobReferenceRowMapper;

impl BlobReferenceRowMapper {
    pub fn to_row(
        &self,
        hash: PlaintextHash,
        digest: BlobDigest,
        created_at: i64,
    ) -> NewBlobReferenceRow {
        NewBlobReferenceRow {
            plaintext_hash: hex::encode(hash.as_bytes()),
            digest: hex::encode(digest.as_bytes()),
            created_at,
        }
    }

    pub fn digest_from_row(&self, row: &BlobReferenceRow) -> Result<BlobDigest> {
        let bytes = decode_32_byte_hex(&row.digest)?;
        Ok(BlobDigest::from_bytes(bytes))
    }
}

fn decode_32_byte_hex(value: &str) -> Result<[u8; 32]> {
    let decoded = hex::decode(value).map_err(|e| anyhow!("invalid hex value: {e}"))?;
    decoded
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow!("expected 32 bytes, got {}", bytes.len()))
}
