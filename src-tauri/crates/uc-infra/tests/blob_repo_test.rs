//! Blob Repository Tests
//! Blob 仓库测试

use std::path::PathBuf;
use std::sync::Arc;
use uc_core::ports::BlobRepositoryPort;
use uc_core::{blob::BlobStorageLocator, security::EncryptionAlgo, Blob, BlobId, ContentHash};
use uc_infra::db::mappers::blob_mapper::BlobRowMapper;
use uc_infra::db::repositories::DieselBlobRepository;

/// In-memory test executor for testing repositories
struct TestDbExecutor {
    pool: Arc<uc_infra::db::pool::DbPool>,
}

impl TestDbExecutor {
    fn new() -> Self {
        let pool = Arc::new(
            uc_infra::db::pool::init_db_pool(":memory:").expect("Failed to create test DB pool"),
        );
        Self { pool }
    }
}

impl uc_infra::db::ports::DbExecutor for TestDbExecutor {
    fn run<T>(
        &self,
        f: impl FnOnce(&mut diesel::SqliteConnection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut conn = self.pool.get()?;
        f(&mut conn)
    }
}

// Helper function to create valid ContentHash (32 bytes of hex)
fn make_hash(seed: &str) -> String {
    // Convert seed to bytes and pad with zeros to make 32 bytes (64 hex chars)
    let seed_bytes = seed.as_bytes();
    let mut hash_bytes = [0u8; 32];
    for (i, &byte) in seed_bytes.iter().enumerate() {
        if i < 32 {
            hash_bytes[i] = byte;
        }
    }
    format!("blake3v1:{}", hex::encode(hash_bytes))
}

#[tokio::test]
async fn test_insert_and_find_by_hash() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    let blob = Blob::new(
        BlobId::from("test-blob-1"),
        BlobStorageLocator::LocalFs {
            path: PathBuf::from("/tmp/blobs/test-blob-1"),
        },
        1024,
        ContentHash::from(make_hash("abc123")),
        1704067200000,
        None,
    );

    // Insert the blob
    repo.insert_blob(&blob)
        .await
        .expect("Failed to insert blob");

    // Find the blob by hash
    let found = repo
        .find_by_hash(&blob.content_hash)
        .await
        .expect("Failed to find blob by hash");

    assert!(found.is_some(), "Blob should be found by hash");
    let found_blob = found.unwrap();
    assert_eq!(found_blob.blob_id.as_str(), "test-blob-1");
    assert_eq!(found_blob.content_hash.to_string(), make_hash("abc123"));
    assert_eq!(found_blob.size_bytes, 1024);
}

#[tokio::test]
async fn test_find_by_hash_not_found() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    let result = repo
        .find_by_hash(&ContentHash::from(make_hash("nonexistent")))
        .await
        .expect("Failed to execute find");

    assert!(result.is_none(), "Non-existent blob should return None");
}

#[tokio::test]
async fn test_insert_duplicate_hash_fails() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    let blob1 = Blob::new(
        BlobId::from("test-blob-2"),
        BlobStorageLocator::LocalFs {
            path: PathBuf::from("/tmp/blobs/test-blob-2"),
        },
        2048,
        ContentHash::from(make_hash("hash456")),
        1704067200000,
        None,
    );

    let blob2 = Blob::new(
        BlobId::from("test-blob-3"),
        BlobStorageLocator::LocalFs {
            path: PathBuf::from("/tmp/blobs/test-blob-3"),
        },
        4096,
        ContentHash::from(make_hash("hash456")), // Same hash as blob1
        1704067300000,
        None,
    );

    // Insert first blob
    repo.insert_blob(&blob1)
        .await
        .expect("Failed to insert first blob");

    // Insert second blob with same hash should fail due to UNIQUE constraint
    let result = repo.insert_blob(&blob2).await;
    assert!(
        result.is_err(),
        "Inserting blob with duplicate hash should fail"
    );
}

#[tokio::test]
async fn test_insert_encrypted_blob() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    let blob = Blob::new(
        BlobId::from("test-encrypted-blob"),
        BlobStorageLocator::EncryptedFs {
            path: PathBuf::from("/tmp/blobs/encrypted"),
            algo: EncryptionAlgo::XChaCha20Poly1305,
        },
        512,
        ContentHash::from(make_hash("encrypted")),
        1704067200000,
        None,
    );

    // Insert the encrypted blob
    repo.insert_blob(&blob)
        .await
        .expect("Failed to insert encrypted blob");

    // Find the blob by hash
    let found = repo
        .find_by_hash(&blob.content_hash)
        .await
        .expect("Failed to find encrypted blob");

    assert!(found.is_some(), "Encrypted blob should be found");
    let found_blob = found.unwrap();

    // Verify the locator is correctly mapped
    match &found_blob.locator {
        BlobStorageLocator::EncryptedFs { path, algo } => {
            assert_eq!(path, &PathBuf::from("/tmp/blobs/encrypted"));
            assert_eq!(algo, &EncryptionAlgo::XChaCha20Poly1305);
        }
        _ => panic!("Expected EncryptedFs locator"),
    }
}

#[tokio::test]
async fn test_multiple_blobs_different_hash() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    // Insert multiple blobs with different hashes
    for i in 1..=3 {
        let hash_str = make_hash(&format!("hash{}", i));
        let blob = Blob::new(
            BlobId::from(format!("test-blob-{}", i)),
            BlobStorageLocator::LocalFs {
                path: PathBuf::from(format!("/tmp/blobs/test-blob-{}", i)),
            },
            i * 1024,
            ContentHash::from(hash_str.clone()),
            1704067200000 + (i as i64 * 1000),
            None,
        );

        repo.insert_blob(&blob)
            .await
            .expect(&format!("Failed to insert blob {}", i));
    }

    // Verify each can be found by its hash
    for i in 1..=3 {
        let hash_str = make_hash(&format!("hash{}", i));
        let found = repo
            .find_by_hash(&ContentHash::from(hash_str))
            .await
            .expect("Failed to find blob");

        assert!(found.is_some(), "Blob {} should be found", i);
        let found_blob = found.unwrap();
        assert_eq!(found_blob.size_bytes, i * 1024);
    }
}

#[tokio::test]
async fn test_blob_with_zero_size() {
    let executor = TestDbExecutor::new();
    let insert_mapper = BlobRowMapper;
    let row_mapper = BlobRowMapper;
    let repo = DieselBlobRepository::new(executor, insert_mapper, row_mapper);

    let blob = Blob::new(
        BlobId::from("test-empty-blob"),
        BlobStorageLocator::LocalFs {
            path: PathBuf::from("/tmp/blobs/empty"),
        },
        0,
        ContentHash::from(make_hash("empty")),
        1704067200000,
        None,
    );

    repo.insert_blob(&blob)
        .await
        .expect("Failed to insert empty blob");

    let found = repo
        .find_by_hash(&blob.content_hash)
        .await
        .expect("Failed to find empty blob");

    assert!(found.is_some());
    let found_blob = found.unwrap();
    assert_eq!(found_blob.size_bytes, 0);
}
