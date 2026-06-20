use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;
use uc_core::ids::DeviceId;
use uc_observability::FlowId;

use crate::{ApplyInboundClipboardUseCase, ApplyInboundInput, ApplyOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundClipboardNoticeInput {
    pub from_device: String,
    pub snapshot_hash: String,
    pub plaintext: Bytes,
    pub flow_id: Option<FlowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundClipboardApplyOutcome {
    Applied {
        entry_id: String,
    },
    DuplicateSkipped {
        snapshot_hash: String,
        existing_entry_id: String,
    },
    DecodeFailed {
        reason: String,
    },
}

#[derive(Debug, Error)]
pub enum InboundClipboardApplyError {
    #[error("inbound clipboard apply failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait InboundClipboardApplyPort: Send + Sync {
    async fn apply(
        &self,
        input: InboundClipboardApplyInput,
    ) -> Result<InboundClipboardApplyOutcome, InboundClipboardApplyError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundClipboardApplyInput {
    pub from_device: String,
    pub snapshot_hash: String,
    pub plaintext: Bytes,
    pub flow_id: Option<FlowId>,
}

#[async_trait]
impl InboundClipboardApplyPort for ApplyInboundClipboardUseCase {
    async fn apply(
        &self,
        input: InboundClipboardApplyInput,
    ) -> Result<InboundClipboardApplyOutcome, InboundClipboardApplyError> {
        let outcome = self
            .execute(ApplyInboundInput {
                from_device: DeviceId::new(input.from_device),
                snapshot_hash: input.snapshot_hash,
                plaintext: input.plaintext,
                flow_id: input.flow_id,
            })
            .await
            .map_err(|err| InboundClipboardApplyError::Internal(err.to_string()))?;
        Ok(apply_outcome_to_view(outcome))
    }
}

pub struct InboundClipboardFacade {
    apply: Arc<dyn InboundClipboardApplyPort>,
}

impl InboundClipboardFacade {
    pub fn new(apply: Arc<dyn InboundClipboardApplyPort>) -> Self {
        Self { apply }
    }

    pub async fn apply_notice(
        &self,
        input: InboundClipboardNoticeInput,
    ) -> Result<InboundClipboardApplyOutcome, InboundClipboardApplyError> {
        self.apply
            .apply(InboundClipboardApplyInput {
                from_device: input.from_device,
                snapshot_hash: input.snapshot_hash,
                plaintext: input.plaintext,
                flow_id: input.flow_id,
            })
            .await
    }
}

fn apply_outcome_to_view(outcome: ApplyOutcome) -> InboundClipboardApplyOutcome {
    match outcome {
        ApplyOutcome::Applied { entry_id } => InboundClipboardApplyOutcome::Applied {
            entry_id: entry_id.to_string(),
        },
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id,
        } => InboundClipboardApplyOutcome::DuplicateSkipped {
            snapshot_hash,
            existing_entry_id: existing_entry_id.to_string(),
        },
        ApplyOutcome::DecodeFailed { reason } => {
            InboundClipboardApplyOutcome::DecodeFailed { reason }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::ids::EntryId;

    struct FakeApply;

    #[async_trait]
    impl InboundClipboardApplyPort for FakeApply {
        async fn apply(
            &self,
            _input: InboundClipboardApplyInput,
        ) -> Result<InboundClipboardApplyOutcome, InboundClipboardApplyError> {
            Ok(InboundClipboardApplyOutcome::Applied {
                entry_id: EntryId::from("entry-a").to_string(),
            })
        }
    }

    #[tokio::test]
    async fn apply_notice_returns_application_entry_id_string() {
        let facade = InboundClipboardFacade::new(Arc::new(FakeApply));
        let outcome = facade
            .apply_notice(InboundClipboardNoticeInput {
                from_device: "device-a".to_string(),
                snapshot_hash: "hash-a".to_string(),
                plaintext: Bytes::from_static(b"payload"),
                flow_id: None,
            })
            .await
            .unwrap();

        assert_eq!(
            outcome,
            InboundClipboardApplyOutcome::Applied {
                entry_id: "entry-a".to_string()
            }
        );
    }
}
