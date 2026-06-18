use std::sync::Arc;

use tracing::instrument;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::MimeType;
use uc_core::ids::{BlobId, EntryId, RepresentationId};
use uc_core::ports::clipboard::{
    GetClipboardEntryPort, GetRepresentationByBlobIdPort, ListRepresentationsForEventPort,
    ThumbnailRepositoryPort,
};

#[derive(Clone)]
pub struct ResourceFacadeDeps {
    pub representation_by_blob_id: Arc<dyn GetRepresentationByBlobIdPort>,
    pub representations_for_event: Arc<dyn ListRepresentationsForEventPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryResourceView {
    pub mime_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// A materialized free-file payload belonging to a clipboard entry.
///
/// `entry_file` resolves an entry's file-list representation to a single
/// on-disk file (the daemon already materialized inbound free-files into a
/// controlled cache directory) and reads its bytes. MVP buffers the whole
/// file in memory; the shape leaves room to switch `bytes` to a stream later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResourceView {
    pub filename: String,
    pub mime: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceFacadeError {
    #[error("resource not found")]
    NotFound,
    #[error("resource mismatch: {0}")]
    Mismatch(String),
    #[error("failed to resolve resource: {0}")]
    Internal(String),
}

pub struct ResourceFacade {
    deps: ResourceFacadeDeps,
}

impl ResourceFacade {
    pub fn new(deps: ResourceFacadeDeps) -> Self {
        Self { deps }
    }

    #[instrument(skip_all, fields(blob_id = %blob_id))]
    pub async fn blob(&self, blob_id: &str) -> Result<BinaryResourceView, ResourceFacadeError> {
        let blob_id = BlobId::from(blob_id);
        let representation = self
            .deps
            .representation_by_blob_id
            .get_representation_by_blob_id(&blob_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?
            .ok_or(ResourceFacadeError::NotFound)?;

        if representation.blob_id.as_ref() != Some(&blob_id) {
            return Err(ResourceFacadeError::Mismatch(
                "representation blob id does not match request".to_string(),
            ));
        }

        let mime_type = representation.mime_type.as_ref().map(mime_to_string);
        let bytes = self
            .deps
            .blob_store
            .get(&blob_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?;

        Ok(BinaryResourceView { mime_type, bytes })
    }

    #[instrument(skip_all, fields(representation_id = %representation_id))]
    pub async fn thumbnail(
        &self,
        representation_id: &str,
    ) -> Result<BinaryResourceView, ResourceFacadeError> {
        let representation_id = RepresentationId::from(representation_id);
        let metadata = self
            .deps
            .thumbnail_repo
            .get_by_representation_id(&representation_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?
            .ok_or(ResourceFacadeError::NotFound)?;

        if metadata.representation_id != representation_id {
            return Err(ResourceFacadeError::Mismatch(
                "thumbnail representation id does not match request".to_string(),
            ));
        }

        let mime_type = Some(mime_to_string(&metadata.thumbnail_mime_type));
        let bytes = self
            .deps
            .blob_store
            .get(&metadata.thumbnail_blob_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?;

        Ok(BinaryResourceView { mime_type, bytes })
    }

    /// Resolve a clipboard entry's first materialized free-file and read its
    /// bytes.
    ///
    /// The daemon materializes an inbound free-file into a controlled cache
    /// directory and rewrites the entry's file-list representation
    /// (`FormatId` `files` / mime `text/uri-list`) so its inline bytes hold a
    /// `file://` uri-list pointing at the cached path(s). This method finds
    /// that representation, takes the **first** `file://` URI, converts it to a
    /// local path, and reads the file.
    ///
    /// Security: only paths embedded in the entry's own file-list
    /// representation are honoured — those paths are written by the daemon's
    /// sanitized materializer, never by an external caller. The returned
    /// `filename` is the URI's last path segment with any path separators
    /// stripped, so callers can safely use it as a basename.
    ///
    /// Errors:
    /// - `NotFound` — no such entry, no file-list representation, or the
    ///   representation carries no usable `file://` URI.
    /// - `Internal` — the cached file could not be read.
    #[instrument(skip_all, fields(entry_id = %entry_id))]
    pub async fn entry_file(
        &self,
        entry_id: &str,
    ) -> Result<FileResourceView, ResourceFacadeError> {
        let entry_id = EntryId::from(entry_id);
        let entry = self
            .deps
            .entry_repo
            .get_entry(&entry_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?
            .ok_or(ResourceFacadeError::NotFound)?;

        let representations = self
            .deps
            .representations_for_event
            .get_representations_for_event(&entry.event_id)
            .await
            .map_err(|err| ResourceFacadeError::Internal(err.to_string()))?;

        let file_rep = representations
            .iter()
            .find(|rep| is_file_list_representation(rep))
            .ok_or(ResourceFacadeError::NotFound)?;

        let uri_list = file_rep
            .inline_data
            .as_deref()
            .ok_or(ResourceFacadeError::NotFound)?;
        let uri_list = std::str::from_utf8(uri_list)
            .map_err(|err| ResourceFacadeError::Internal(format!("uri-list not utf-8: {err}")))?;

        let path = first_local_file_path(uri_list).ok_or(ResourceFacadeError::NotFound)?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_basename)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "uniclip-recv.bin".to_string());

        let mime = file_rep.mime_type.as_ref().map(mime_to_string);

        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|err| ResourceFacadeError::Internal(format!("failed to read file: {err}")))?;

        Ok(FileResourceView {
            filename,
            mime,
            bytes,
        })
    }
}

fn mime_to_string(mime: &MimeType) -> String {
    mime.as_str().to_string()
}

/// A representation carries a file list when its mime is `*/uri-list` or its
/// format id is one of the well-known file-list format ids. Mirrors the
/// materializer's classification so resolution agrees with what was written.
fn is_file_list_representation(rep: &uc_core::PersistedClipboardRepresentation) -> bool {
    rep.mime_type
        .as_ref()
        .map(|mime| {
            mime.as_str().eq_ignore_ascii_case("text/uri-list")
                || mime.as_str().eq_ignore_ascii_case("file/uri-list")
        })
        .unwrap_or(false)
        || rep.format_id.as_str().eq_ignore_ascii_case("files")
        || rep
            .format_id
            .as_str()
            .eq_ignore_ascii_case("public.file-url")
}

/// Parse the first `file://` URI from a uri-list and convert it to a local
/// path. The daemon writes percent-encoded `file://` URLs via
/// `Url::from_file_path`, so we round-trip through `url::Url` to decode them.
fn first_local_file_path(uri_list: &str) -> Option<std::path::PathBuf> {
    uri_list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .find_map(|line| {
            let url = url::Url::parse(line).ok()?;
            if url.scheme() != "file" {
                return None;
            }
            url.to_file_path().ok()
        })
}

/// Strip path separators / NUL from a filename so it is safe to use as a
/// response-header basename and never escapes a target directory.
fn sanitize_basename(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uc_core::clipboard::{
        ClipboardRepositoryError, PersistedClipboardRepresentation, ThumbnailMetadata,
    };
    use uc_core::ids::{EventId, FormatId};
    use uc_core::ClipboardEntry;

    #[derive(Default)]
    struct FakeRepresentationRepo {
        by_blob_id: Mutex<HashMap<BlobId, PersistedClipboardRepresentation>>,
        by_event_id: Mutex<HashMap<EventId, Vec<PersistedClipboardRepresentation>>>,
    }

    #[async_trait]
    impl GetRepresentationByBlobIdPort for FakeRepresentationRepo {
        async fn get_representation_by_blob_id(
            &self,
            blob_id: &BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self
                .by_blob_id
                .lock()
                .expect("representation lock")
                .get(blob_id)
                .cloned())
        }
    }

    #[async_trait]
    impl ListRepresentationsForEventPort for FakeRepresentationRepo {
        async fn get_representations_for_event(
            &self,
            event_id: &EventId,
        ) -> Result<Vec<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self
                .by_event_id
                .lock()
                .expect("event lock")
                .get(event_id)
                .cloned()
                .unwrap_or_default())
        }
    }

    #[derive(Default)]
    struct FakeEntryRepo {
        by_entry_id: Mutex<HashMap<EntryId, ClipboardEntry>>,
    }

    #[async_trait]
    impl GetClipboardEntryPort for FakeEntryRepo {
        async fn get_entry(
            &self,
            entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(self
                .by_entry_id
                .lock()
                .expect("entry lock")
                .get(entry_id)
                .cloned())
        }
    }

    #[derive(Default)]
    struct FakeThumbnailRepo {
        by_representation_id: Mutex<HashMap<RepresentationId, ThumbnailMetadata>>,
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for FakeThumbnailRepo {
        async fn get_by_representation_id(
            &self,
            representation_id: &RepresentationId,
        ) -> Result<Option<ThumbnailMetadata>> {
            Ok(self
                .by_representation_id
                .lock()
                .expect("thumbnail lock")
                .remove(representation_id))
        }

        async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> Result<()> {
            self.by_representation_id
                .lock()
                .expect("thumbnail lock")
                .insert(
                    metadata.representation_id.clone(),
                    ThumbnailMetadata::new(
                        metadata.representation_id.clone(),
                        metadata.thumbnail_blob_id.clone(),
                        metadata.thumbnail_mime_type.clone(),
                        metadata.original_width,
                        metadata.original_height,
                        metadata.original_size_bytes,
                        metadata.created_at_ms,
                    ),
                );
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeBlobStore {
        bytes: Mutex<HashMap<BlobId, Vec<u8>>>,
    }

    #[async_trait]
    impl BlobReaderPort for FakeBlobStore {
        async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
            self.bytes
                .lock()
                .expect("blob lock")
                .get(blob_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("blob not found"))
        }
    }

    struct TestDeps {
        facade: ResourceFacade,
        representation_repo: Arc<FakeRepresentationRepo>,
        thumbnail_repo: Arc<FakeThumbnailRepo>,
        blob_store: Arc<FakeBlobStore>,
        entry_repo: Arc<FakeEntryRepo>,
    }

    fn test_deps() -> TestDeps {
        let representation_repo = Arc::new(FakeRepresentationRepo::default());
        let thumbnail_repo = Arc::new(FakeThumbnailRepo::default());
        let blob_store = Arc::new(FakeBlobStore::default());
        let entry_repo = Arc::new(FakeEntryRepo::default());
        let facade = ResourceFacade::new(ResourceFacadeDeps {
            representation_by_blob_id: representation_repo.clone(),
            representations_for_event: representation_repo.clone(),
            thumbnail_repo: thumbnail_repo.clone(),
            blob_store: blob_store.clone(),
            entry_repo: entry_repo.clone(),
        });
        TestDeps {
            facade,
            representation_repo,
            thumbnail_repo,
            blob_store,
            entry_repo,
        }
    }

    #[tokio::test]
    async fn blob_returns_bytes_and_mime_type() {
        let deps = test_deps();
        let blob_id = BlobId::from("blob-1");
        let representation = PersistedClipboardRepresentation::new(
            RepresentationId::from("rep-1"),
            FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            3,
            None,
            Some(blob_id.clone()),
        );
        deps.representation_repo
            .by_blob_id
            .lock()
            .expect("representation lock")
            .insert(blob_id.clone(), representation);
        deps.blob_store
            .bytes
            .lock()
            .expect("blob lock")
            .insert(blob_id, vec![1, 2, 3]);

        let view = deps.facade.blob("blob-1").await.expect("blob");

        assert_eq!(view.mime_type, Some("image/png".to_string()));
        assert_eq!(view.bytes, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn blob_returns_not_found_when_representation_is_missing() {
        let deps = test_deps();

        let error = deps.facade.blob("missing").await.expect_err("missing");

        assert!(matches!(error, ResourceFacadeError::NotFound));
    }

    #[tokio::test]
    async fn thumbnail_returns_bytes_and_mime_type() {
        let deps = test_deps();
        let representation_id = RepresentationId::from("rep-1");
        let blob_id = BlobId::from("thumb-1");
        deps.thumbnail_repo
            .insert_thumbnail(&ThumbnailMetadata::new(
                representation_id,
                blob_id.clone(),
                MimeType("image/webp".to_string()),
                100,
                80,
                3,
                None,
            ))
            .await
            .expect("insert thumbnail");
        deps.blob_store
            .bytes
            .lock()
            .expect("blob lock")
            .insert(blob_id, vec![4, 5, 6]);

        let view = deps.facade.thumbnail("rep-1").await.expect("thumbnail");

        assert_eq!(view.mime_type, Some("image/webp".to_string()));
        assert_eq!(view.bytes, vec![4, 5, 6]);
    }

    fn seed_entry_with_file_rep(deps: &TestDeps, entry_id: &str, file_path: &std::path::Path) {
        let event_id = EventId::from("event-1");
        let entry = ClipboardEntry::new(EntryId::from(entry_id), event_id.clone(), 0, None, 0);
        deps.entry_repo
            .by_entry_id
            .lock()
            .expect("entry lock")
            .insert(EntryId::from(entry_id), entry);

        let uri = url::Url::from_file_path(file_path).expect("file url");
        let uri_list = format!("{uri}\n");
        let rep = PersistedClipboardRepresentation::new(
            RepresentationId::from("rep-files"),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            uri_list.len() as i64,
            Some(uri_list.into_bytes()),
            None,
        );
        deps.representation_repo
            .by_event_id
            .lock()
            .expect("event lock")
            .insert(event_id, vec![rep]);
    }

    #[tokio::test]
    async fn entry_file_reads_first_local_file() {
        let deps = test_deps();
        let dir = std::env::temp_dir().join(format!("uc-entry-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let file_path = dir.join("hello.txt");
        std::fs::write(&file_path, b"payload-bytes").expect("write");

        seed_entry_with_file_rep(&deps, "entry-1", &file_path);

        let view = deps.facade.entry_file("entry-1").await.expect("entry file");

        assert_eq!(view.filename, "hello.txt");
        assert_eq!(view.mime, Some("text/uri-list".to_string()));
        assert_eq!(view.bytes, b"payload-bytes".to_vec());

        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[tokio::test]
    async fn entry_file_not_found_when_entry_missing() {
        let deps = test_deps();
        let error = deps
            .facade
            .entry_file("missing")
            .await
            .expect_err("missing");
        assert!(matches!(error, ResourceFacadeError::NotFound));
    }

    #[tokio::test]
    async fn entry_file_not_found_when_no_file_representation() {
        let deps = test_deps();
        let event_id = EventId::from("event-text");
        let entry = ClipboardEntry::new(EntryId::from("entry-text"), event_id.clone(), 0, None, 0);
        deps.entry_repo
            .by_entry_id
            .lock()
            .expect("entry lock")
            .insert(EntryId::from("entry-text"), entry);
        // No representations registered for this event.

        let error = deps
            .facade
            .entry_file("entry-text")
            .await
            .expect_err("no file rep");
        assert!(matches!(error, ResourceFacadeError::NotFound));
    }
}
