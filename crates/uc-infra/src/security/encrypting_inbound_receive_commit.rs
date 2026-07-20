//! Encrypting decorator for atomic inbound receive commits.
//!
//! Atomic receive persistence bypasses `ClipboardEventWriterPort`, so inline
//! representation payloads must be encrypted before the settlement reaches
//! the database transaction boundary.

use std::sync::Arc;

use async_trait::async_trait;
use uc_core::clipboard::PersistedClipboardRepresentation;
use uc_core::crypto::aad;
use uc_core::crypto::domain::{Aad, ActiveSpace, Plaintext};
use uc_core::ids::{EventId, SpaceId};
use uc_core::ports::security::BlobCipherPort;
use uc_core::ports::{
    CommitInboundReceivePort, InboundReceiveCommitError, InboundReceiveRecord,
    InboundReceiveSettlement,
};

pub struct EncryptingInboundReceiveCommit {
    inner: Arc<dyn CommitInboundReceivePort>,
    blob_cipher: Arc<dyn BlobCipherPort>,
}

impl EncryptingInboundReceiveCommit {
    pub fn new(
        inner: Arc<dyn CommitInboundReceivePort>,
        blob_cipher: Arc<dyn BlobCipherPort>,
    ) -> Self {
        Self { inner, blob_cipher }
    }

    async fn encrypt_record(
        &self,
        record: &InboundReceiveRecord,
    ) -> Result<InboundReceiveRecord, InboundReceiveCommitError> {
        match record {
            InboundReceiveRecord::Create {
                entry,
                event,
                representations,
                selection,
            } => Ok(InboundReceiveRecord::Create {
                entry: entry.clone(),
                event: event.clone(),
                representations: self
                    .encrypt_representations(&event.event_id, representations)
                    .await?,
                selection: selection.clone(),
            }),
            InboundReceiveRecord::Replace {
                entry_id,
                new_event,
                new_representations,
                new_selection,
                new_total_size,
                new_content_category,
            } => Ok(InboundReceiveRecord::Replace {
                entry_id: entry_id.clone(),
                new_event: new_event.clone(),
                new_representations: self
                    .encrypt_representations(&new_event.event_id, new_representations)
                    .await?,
                new_selection: new_selection.clone(),
                new_total_size: *new_total_size,
                new_content_category: *new_content_category,
            }),
        }
    }

    async fn encrypt_representations(
        &self,
        event_id: &EventId,
        representations: &[PersistedClipboardRepresentation],
    ) -> Result<Vec<PersistedClipboardRepresentation>, InboundReceiveCommitError> {
        let active = ActiveSpace::new(SpaceId::from("space"));
        let mut encrypted = Vec::with_capacity(representations.len());
        for representation in representations {
            let inline_data = match representation.inline_data.as_ref() {
                Some(bytes) => {
                    let associated_data = aad::for_inline(event_id, &representation.id);
                    let ciphertext = self
                        .blob_cipher
                        .encrypt(
                            &active,
                            &Plaintext::new(bytes.clone()),
                            &Aad::from(associated_data.as_slice()),
                        )
                        .await
                        .map_err(|_| {
                            InboundReceiveCommitError::Backend(
                                "failed to encrypt inbound representation".to_owned(),
                            )
                        })?;
                    Some(ciphertext.into_bytes())
                }
                None => None,
            };
            encrypted.push(
                PersistedClipboardRepresentation::new_with_state(
                    representation.id.clone(),
                    representation.format_id.clone(),
                    representation.mime_type.clone(),
                    representation.size_bytes,
                    inline_data,
                    representation.blob_id.clone(),
                    representation.payload_state(),
                    representation.last_error.clone(),
                )
                .map_err(|_| {
                    InboundReceiveCommitError::Backend(
                        "failed to prepare encrypted inbound representation".to_owned(),
                    )
                })?,
            );
        }
        Ok(encrypted)
    }
}

#[async_trait]
impl CommitInboundReceivePort for EncryptingInboundReceiveCommit {
    async fn commit_inbound_receive(
        &self,
        settlement: &InboundReceiveSettlement,
    ) -> Result<(), InboundReceiveCommitError> {
        let encrypted = match settlement {
            InboundReceiveSettlement::Complete {
                record,
                attempt_id,
                file_set,
                artifacts,
                now_ms,
            } => InboundReceiveSettlement::Complete {
                record: self.encrypt_record(record).await?,
                attempt_id: attempt_id.clone(),
                file_set: file_set.clone(),
                artifacts: *artifacts,
                now_ms: *now_ms,
            },
            InboundReceiveSettlement::Partial {
                record,
                attempt_id,
                terminal,
                file_set,
                artifacts,
                now_ms,
            } => InboundReceiveSettlement::Partial {
                record: self.encrypt_record(record).await?,
                attempt_id: attempt_id.clone(),
                terminal: *terminal,
                file_set: file_set.clone(),
                artifacts: *artifacts,
                now_ms: *now_ms,
            },
            InboundReceiveSettlement::NoEntry { .. } => {
                return self.inner.commit_inbound_receive(settlement).await;
            }
        };
        self.inner.commit_inbound_receive(&encrypted).await
    }
}
