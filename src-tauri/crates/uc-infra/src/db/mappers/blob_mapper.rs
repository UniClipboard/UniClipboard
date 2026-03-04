use crate::db::models::blob::NewBlobRow;
use crate::db::models::BlobRow;
use crate::db::ports::{InsertMapper, RowMapper};
use anyhow::Result;
use std::path::PathBuf;
use uc_core::blob::BlobStorageLocator;
use uc_core::{Blob, BlobId, ContentHash};

pub struct BlobRowMapper;

impl InsertMapper<Blob, NewBlobRow> for BlobRowMapper {
    fn to_row(&self, domain: &Blob) -> Result<NewBlobRow> {
        let (storage_backend, storage_path, encryption_algo) = map_locator(&domain.locator);

        Ok(NewBlobRow {
            blob_id: domain.blob_id.to_string(),
            storage_backend,
            storage_path,
            encryption_algo,
            size_bytes: domain.size_bytes,
            content_hash: domain.content_hash.to_string(),
            created_at_ms: domain.created_at_ms,
        })
    }
}

impl RowMapper<BlobRow, Blob> for BlobRowMapper {
    fn to_domain(&self, row: &BlobRow) -> Result<Blob> {
        let locator = match row.storage_backend.as_str() {
            "local_fs" => BlobStorageLocator::LocalFs {
                path: row.storage_path.clone().into(),
            },
            "encrypted_fs" => BlobStorageLocator::EncryptedFs {
                path: row.storage_path.clone().into(),
                algo: row
                    .encryption_algo
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("encryption_algo is missing"))?
                    .into(),
            },
            _ => {
                return Err(anyhow::anyhow!(
                    "unknown storage backend: {}",
                    row.storage_backend
                ))
            }
        };
        Ok(Blob::new(
            BlobId::from(row.blob_id.clone()),
            locator,
            row.size_bytes,
            ContentHash::from(row.content_hash.clone()),
            row.created_at_ms,
            None, // compressed_size: will be mapped from row in Task 2
        ))
    }
}

fn map_locator(locator: &BlobStorageLocator) -> (String, String, Option<String>) {
    match locator {
        BlobStorageLocator::LocalFs { path } => {
            ("local_fs".to_string(), path_to_string(path), None)
        }
        BlobStorageLocator::EncryptedFs { path, algo } => (
            "encrypted_fs".to_string(),
            path_to_string(path),
            Some(algo.to_string()),
        ),
    }
}

fn path_to_string(path: &PathBuf) -> String {
    path.to_str()
        .expect("blob storage path must be valid UTF-8")
        .to_owned()
}
