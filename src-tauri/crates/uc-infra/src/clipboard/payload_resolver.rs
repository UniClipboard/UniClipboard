//! Clipboard Payload Resolver Implementation
//!
//! Resolves persisted clipboard representations into usable payloads.
//! Read-only: returns inline data, blob references, or cache/spool bytes.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info_span, warn, Instrument};

use uc_core::clipboard::{PayloadAvailability, PersistedClipboardRepresentation};
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{PayloadResolveError, ResolvedClipboardPayload};
use uc_core::ports::ClipboardPayloadResolverPort;

use crate::clipboard::{RepresentationCache, SpoolManager};

/// Clipboard payload resolver implementation
pub struct ClipboardPayloadResolver {
    cache: Arc<RepresentationCache>,
    spool: Arc<SpoolManager>,
    worker_tx: mpsc::Sender<RepresentationId>,
}

impl ClipboardPayloadResolver {
    pub fn new(
        cache: Arc<RepresentationCache>,
        spool: Arc<SpoolManager>,
        worker_tx: mpsc::Sender<RepresentationId>,
    ) -> Self {
        Self {
            cache,
            spool,
            worker_tx,
        }
    }
}

#[async_trait]
impl ClipboardPayloadResolverPort for ClipboardPayloadResolver {
    async fn resolve(
        &self,
        representation: &PersistedClipboardRepresentation,
    ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
        let span = info_span!(
            "infra.payload.resolve",
            representation_id = %representation.id,
            format_id = %representation.format_id,
        );
        async move {
            let mime = Self::mime_or_default(representation);

            match &representation.payload_state {
                PayloadAvailability::Inline => {
                    let inline_data = match representation.inline_data.as_ref() {
                        Some(bytes) => bytes,
                        None => {
                            let err = PayloadResolveError::Integrity {
                                rep_id: representation.id.clone(),
                                reason: "payload_state Inline but inline_data is None".to_string(),
                            };
                            error!(
                                representation_id = %representation.id,
                                error = %err,
                                "Inline payload is missing inline_data"
                            );
                            return Err(err);
                        }
                    };
                    debug!("Resolving from inline data");
                    Ok(ResolvedClipboardPayload::Inline {
                        mime,
                        bytes: inline_data.clone(),
                    })
                }
                PayloadAvailability::BlobReady => {
                    let blob_id = match representation.blob_id.as_ref() {
                        Some(id) => id,
                        None => {
                            let err = PayloadResolveError::Integrity {
                                rep_id: representation.id.clone(),
                                reason: "payload_state BlobReady but blob_id is None".to_string(),
                            };
                            error!(
                                representation_id = %representation.id,
                                error = %err,
                                "BlobReady payload is missing blob_id"
                            );
                            return Err(err);
                        }
                    };
                    debug!("Resolving from existing blob reference");
                    Ok(ResolvedClipboardPayload::BlobRef {
                        mime,
                        blob_id: blob_id.clone(),
                    })
                }
                PayloadAvailability::Staged
                | PayloadAvailability::Processing
                | PayloadAvailability::Failed { .. } => {
                    if let Some(bytes) = self.cache.get(&representation.id).await {
                        debug!("Resolving from cache bytes");
                        self.try_requeue(&representation.id);
                        return Ok(ResolvedClipboardPayload::Inline { mime, bytes });
                    }

                    match self.spool.read(&representation.id).await {
                        Ok(Some(bytes)) => {
                            debug!("Resolving from spool bytes");
                            self.try_requeue(&representation.id);
                            Ok(ResolvedClipboardPayload::Inline { mime, bytes })
                        }
                        Ok(None) => {
                            warn!(
                                representation_id = %representation.id,
                                payload_state = ?&representation.payload_state,
                                "Bytes not available in cache or spool"
                            );
                            Err(PayloadResolveError::Orphaned {
                                rep_id: representation.id.clone(),
                                state: representation.payload_state.clone(),
                            })
                        }
                        Err(err) => {
                            error!(
                                representation_id = %representation.id,
                                error = %err,
                                "Failed to read bytes from spool"
                            );
                            Err(PayloadResolveError::Integrity {
                                rep_id: representation.id.clone(),
                                reason: format!("spool read failed: {err}"),
                            })
                        }
                    }
                }
                PayloadAvailability::Lost => {
                    let details = representation
                        .last_error
                        .as_deref()
                        .unwrap_or("payload marked as lost");
                    Err(PayloadResolveError::Lost {
                        rep_id: representation.id.clone(),
                        reason: details.to_string(),
                    })
                }
            }
        }
        .instrument(span)
        .await
    }
}

impl ClipboardPayloadResolver {
    fn mime_or_default(representation: &PersistedClipboardRepresentation) -> String {
        representation
            .mime_type
            .clone()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string())
    }

    fn try_requeue(&self, rep_id: &RepresentationId) {
        if let Err(err) = self.worker_tx.try_send(rep_id.clone()) {
            warn!(
                representation_id = %rep_id,
                error = %err,
                "Failed to re-queue representation for background processing"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use uc_core::clipboard::MimeType;
    use uc_core::ids::FormatId;
    use uc_core::BlobId;

    fn make_resolver() -> (
        ClipboardPayloadResolver,
        Arc<RepresentationCache>,
        Arc<SpoolManager>,
        mpsc::Receiver<RepresentationId>,
        TempDir,
    ) {
        let dir = TempDir::new().expect("tempdir");
        let cache = Arc::new(RepresentationCache::new(16, 1024 * 1024));
        let spool = Arc::new(SpoolManager::new(dir.path(), 1024).expect("spool"));
        let (tx, rx) = mpsc::channel(8);
        let resolver = ClipboardPayloadResolver::new(cache.clone(), spool.clone(), tx);
        (resolver, cache, spool, rx, dir)
    }

    fn rep_with_state(
        id: &str,
        state: PayloadAvailability,
        inline_data: Option<Vec<u8>>,
        blob_id: Option<BlobId>,
        last_error: Option<String>,
    ) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation {
            id: RepresentationId::from(id),
            format_id: FormatId::from("public.utf8-plain-text"),
            mime_type: Some(MimeType("text/plain".to_string())),
            size_bytes: 5,
            inline_data,
            blob_id,
            payload_state: state,
            last_error,
        }
    }

    #[tokio::test]
    async fn resolves_inline_state_to_inline_payload() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = PersistedClipboardRepresentation::new(
            RepresentationId::from("rep-i"),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            5,
            Some(b"hello".to_vec()),
            None,
        );
        assert_eq!(rep.payload_state, PayloadAvailability::Inline);

        let resolved = resolver.resolve(&rep).await.expect("resolve");
        match resolved {
            ResolvedClipboardPayload::Inline { mime, bytes } => {
                assert_eq!(mime, "text/plain");
                assert_eq!(bytes, b"hello");
            }
            other => panic!("expected Inline, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn inline_state_without_inline_data_is_integrity_error() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = rep_with_state(
            "rep-broken-inline",
            PayloadAvailability::Inline,
            None,
            None,
            None,
        );

        let err = resolver.resolve(&rep).await.expect_err("must error");
        match err {
            PayloadResolveError::Integrity { rep_id, reason } => {
                assert_eq!(rep_id, RepresentationId::from("rep-broken-inline"));
                assert!(reason.contains("Inline but inline_data is None"));
            }
            other => panic!("expected Integrity, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolves_blob_ready_state_to_blob_ref() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let blob_id = BlobId::from("blob-1");
        let rep = PersistedClipboardRepresentation::new(
            RepresentationId::from("rep-b"),
            FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            10,
            None,
            Some(blob_id.clone()),
        );
        assert_eq!(rep.payload_state, PayloadAvailability::BlobReady);

        let resolved = resolver.resolve(&rep).await.expect("resolve");
        match resolved {
            ResolvedClipboardPayload::BlobRef {
                mime,
                blob_id: returned,
            } => {
                assert_eq!(mime, "image/png");
                assert_eq!(returned, blob_id);
            }
            other => panic!("expected BlobRef, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn blob_ready_without_blob_id_is_integrity_error() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = rep_with_state(
            "rep-broken-blob",
            PayloadAvailability::BlobReady,
            None,
            None,
            None,
        );

        let err = resolver.resolve(&rep).await.expect_err("must error");
        match err {
            PayloadResolveError::Integrity { rep_id, reason } => {
                assert_eq!(rep_id, RepresentationId::from("rep-broken-blob"));
                assert!(reason.contains("BlobReady but blob_id is None"));
            }
            other => panic!("expected Integrity, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn staged_resolves_from_cache_when_present() {
        let (resolver, cache, _spool, mut rx, _dir) = make_resolver();
        let id = RepresentationId::from("rep-cached");
        cache.put(&id, b"cached-bytes".to_vec()).await;

        let rep = rep_with_state("rep-cached", PayloadAvailability::Staged, None, None, None);
        let resolved = resolver.resolve(&rep).await.expect("resolve");
        match resolved {
            ResolvedClipboardPayload::Inline { bytes, .. } => {
                assert_eq!(bytes, b"cached-bytes");
            }
            other => panic!("expected Inline, got {:?}", other),
        }
        // Cache hit 应触发 worker re-queue
        assert_eq!(rx.try_recv().expect("requeued"), id);
    }

    #[tokio::test]
    async fn staged_resolves_from_spool_when_cache_misses() {
        let (resolver, _cache, spool, mut rx, _dir) = make_resolver();
        let id = RepresentationId::from("rep-spool");
        spool.write(&id, b"spool-bytes").await.expect("spool write");

        let rep = rep_with_state(
            "rep-spool",
            PayloadAvailability::Processing,
            None,
            None,
            None,
        );
        let resolved = resolver.resolve(&rep).await.expect("resolve");
        match resolved {
            ResolvedClipboardPayload::Inline { bytes, .. } => {
                assert_eq!(bytes, b"spool-bytes");
            }
            other => panic!("expected Inline, got {:?}", other),
        }
        assert_eq!(rx.try_recv().expect("requeued"), id);
    }

    #[tokio::test]
    async fn staged_with_no_bytes_returns_orphaned_error() {
        let (resolver, _cache, _spool, mut rx, _dir) = make_resolver();
        let rep = rep_with_state("rep-orphan", PayloadAvailability::Staged, None, None, None);
        let err = resolver.resolve(&rep).await.expect_err("must error");
        match err {
            PayloadResolveError::Orphaned { rep_id, state } => {
                assert_eq!(rep_id, RepresentationId::from("rep-orphan"));
                assert_eq!(state, PayloadAvailability::Staged);
            }
            other => panic!("expected Orphaned, got {other:?}"),
        }
        // 双 miss 不 re-queue
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn lost_state_returns_lost_error_with_last_error_text() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = rep_with_state(
            "rep-lost",
            PayloadAvailability::Lost,
            None,
            None,
            Some("orphaned at startup".to_string()),
        );
        let err = resolver.resolve(&rep).await.expect_err("must error");
        match err {
            PayloadResolveError::Lost { rep_id, reason } => {
                assert_eq!(rep_id, RepresentationId::from("rep-lost"));
                assert_eq!(reason, "orphaned at startup");
            }
            other => panic!("expected Lost, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn lost_state_without_last_error_falls_back_to_default_text() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = rep_with_state("rep-lost-bare", PayloadAvailability::Lost, None, None, None);
        let err = resolver.resolve(&rep).await.expect_err("must error");
        match err {
            PayloadResolveError::Lost { reason, .. } => {
                assert_eq!(reason, "payload marked as lost");
            }
            other => panic!("expected Lost, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_mime_type_falls_back_to_octet_stream() {
        let (resolver, _cache, _spool, _rx, _dir) = make_resolver();
        let rep = PersistedClipboardRepresentation {
            id: RepresentationId::from("rep-no-mime"),
            format_id: FormatId::from("public.utf8-plain-text"),
            mime_type: None,
            size_bytes: 1,
            inline_data: Some(b"x".to_vec()),
            blob_id: None,
            payload_state: PayloadAvailability::Inline,
            last_error: None,
        };
        let resolved = resolver.resolve(&rep).await.expect("resolve");
        match resolved {
            ResolvedClipboardPayload::Inline { mime, .. } => {
                assert_eq!(mime, "application/octet-stream");
            }
            other => panic!("expected Inline, got {:?}", other),
        }
    }
}
