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
