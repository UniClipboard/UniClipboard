use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use uc_core::ids::EntryId;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

use crate::clipboard_capture::CaptureClipboardUseCase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedClipboardEntryView {
    pub entry_id: String,
    /// True when the snapshot matched existing content and the existing entry
    /// was resurfaced instead of a new one being created. Callers should
    /// refresh the UI but skip re-indexing / re-dispatching this entry.
    pub deduplicated: bool,
    /// The `snapshot_hash` persisted on this entry — its cross-device identity.
    ///
    /// Consumers that advertise this capture to peers (e.g. the
    /// active-clipboard register) MUST reuse this value rather than recomputing
    /// a hash from a separate copy of the snapshot, otherwise a file copy's
    /// advertised identity diverges from its dispatch identity and the receiver
    /// dedups into two entries.
    pub snapshot_hash: String,
}

#[derive(Debug, Error)]
pub enum ClipboardCaptureFacadeError {
    #[error("clipboard capture failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait ClipboardCapturePort: Send + Sync {
    async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError>;
}

#[async_trait]
impl ClipboardCapturePort for CaptureClipboardUseCase {
    async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
        // Local-capture facade: the snapshot is authoritative for its own hash.
        // Inbound (`RemotePush`) never reaches here — it persists through the
        // `InboundCapture` port, which supplies the wire identity (F-4).
        let outcome = self
            .execute_with_origin(
                snapshot,
                origin,
                preset_entry_id,
                None,
                crate::clipboard_capture::CommitMode::Create,
            )
            .await
            .map_err(|err| ClipboardCaptureFacadeError::Internal(err.to_string()))?;
        Ok(outcome.map(|outcome| CapturedClipboardEntryView {
            entry_id: outcome.entry_id.to_string(),
            deduplicated: outcome.deduplicated,
            snapshot_hash: outcome.snapshot_hash,
        }))
    }
}

pub struct ClipboardCaptureFacade {
    capture: Arc<dyn ClipboardCapturePort>,
}

impl ClipboardCaptureFacade {
    pub fn new(capture: Arc<dyn ClipboardCapturePort>) -> Self {
        Self { capture }
    }

    pub async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
        self.capture
            .capture(snapshot, origin, preset_entry_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::SystemClipboardSnapshot;

    struct FakeCapture;

    #[async_trait]
    impl ClipboardCapturePort for FakeCapture {
        async fn capture(
            &self,
            _snapshot: SystemClipboardSnapshot,
            _origin: ClipboardChangeOrigin,
            _preset_entry_id: Option<EntryId>,
        ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
            Ok(Some(CapturedClipboardEntryView {
                entry_id: "entry-a".to_string(),
                deduplicated: false,
                snapshot_hash: "blake3v1:test".to_string(),
            }))
        }
    }

    #[tokio::test]
    async fn capture_returns_application_entry_id_string() {
        let facade = ClipboardCaptureFacade::new(std::sync::Arc::new(FakeCapture));
        let outcome = facade
            .capture(
                SystemClipboardSnapshot {
                    representations: Vec::new(),
                    ts_ms: 0,

                    file_content_digests: Vec::new(),
                },
                ClipboardChangeOrigin::LocalCapture,
                Some(EntryId::from("entry-preset")),
            )
            .await
            .unwrap();

        assert_eq!(
            outcome,
            Some(CapturedClipboardEntryView {
                entry_id: "entry-a".to_string(),
                deduplicated: false,
                snapshot_hash: "blake3v1:test".to_string(),
            })
        );
    }
}
