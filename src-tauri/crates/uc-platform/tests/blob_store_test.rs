use tempfile::TempDir;
use uc_core::ports::BlobStorePort;
use uc_core::BlobId;
use uc_platform::adapters::blob_store::FilesystemBlobStore;

#[tokio::test]
async fn test_put_and_get_blob() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemBlobStore::new(temp_dir.path().to_path_buf());

    let blob_id = BlobId::from("test-blob-1");
    let data = b"hello, world!";

    let (path, compressed_size) = store.put(&blob_id, data).await.unwrap();
    assert!(path.exists());
    assert!(
        compressed_size.is_none(),
        "Raw FS store should return None for compressed_size"
    );

    let retrieved = store.get(&blob_id).await.unwrap();
    assert_eq!(retrieved, data);
}

#[tokio::test]
async fn test_get_nonexistent_blob() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemBlobStore::new(temp_dir.path().to_path_buf());

    let blob_id = BlobId::from("nonexistent");
    let result = store.get(&blob_id).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_put_overwrites_existing() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemBlobStore::new(temp_dir.path().to_path_buf());

    let blob_id = BlobId::from("test-blob-overwrite");
    let data1 = b"first version";
    let data2 = b"second version";

    store.put(&blob_id, data1).await.unwrap();
    store.put(&blob_id, data2).await.unwrap();

    let retrieved = store.get(&blob_id).await.unwrap();
    assert_eq!(retrieved, data2);
}

#[tokio::test]
async fn test_empty_blob() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemBlobStore::new(temp_dir.path().to_path_buf());

    let blob_id = BlobId::from("empty-blob");
    let data = b"";

    let (path, _) = store.put(&blob_id, data).await.unwrap();
    assert!(path.exists());

    let retrieved = store.get(&blob_id).await.unwrap();
    assert_eq!(retrieved, data);
    assert_eq!(retrieved.len(), 0);
}

#[tokio::test]
async fn test_large_blob() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemBlobStore::new(temp_dir.path().to_path_buf());

    let blob_id = BlobId::from("large-blob");
    let data = vec![0u8; 1024 * 1024]; // 1MB of zeros

    let (path, _) = store.put(&blob_id, &data).await.unwrap();
    assert!(path.exists());

    let retrieved = store.get(&blob_id).await.unwrap();
    assert_eq!(retrieved.len(), 1024 * 1024);
    assert_eq!(retrieved, data.as_slice());
}
