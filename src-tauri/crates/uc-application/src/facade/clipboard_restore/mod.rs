use std::sync::Arc;

use tracing::instrument;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ids::EntryId;
use uc_core::ports::{
    clipboard::{ClipboardPayloadResolverPort, PayloadResolveError},
    ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, ClockPort,
};

use crate::clipboard_write::ClipboardWriteCoordinator;
use crate::usecases::clipboard_restore::{
    RestoreClipboardSelectionUseCase, TouchClipboardEntryUseCase,
};

#[derive(Debug, thiserror::Error)]
pub enum ClipboardRestoreError {
    #[error("clipboard entry not found")]
    NotFound,

    /// Paste representation can no longer be materialized — bytes are gone
    /// from cache and spool, or the representation is in `Lost` state.
    /// This is a known business outcome (resource has logically vanished),
    /// not a server fault. API layer should map this to 410 Gone, **not** 500.
    #[error(
        "clipboard payload unavailable: representation {rep_id} for entry {entry_id} (state={state})"
    )]
    PayloadUnavailable {
        entry_id: String,
        rep_id: String,
        state: String,
    },

    #[error("clipboard restore failed: {0}")]
    Internal(String),
}

/// Dependency bundle for `ClipboardRestoreFacade`. Composition roots build
/// this once from their wiring deps and pass it to
/// `ClipboardRestoreFacade::new`.
pub struct ClipboardRestoreFacadeDeps {
    pub entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub clock: Arc<dyn ClockPort>,
    pub write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub integration_mode: ClipboardIntegrationMode,
}

pub struct ClipboardRestoreFacade {
    restore_uc: RestoreClipboardSelectionUseCase,
    touch_uc: TouchClipboardEntryUseCase,
}

impl ClipboardRestoreFacade {
    pub fn new(deps: ClipboardRestoreFacadeDeps) -> Self {
        let ClipboardRestoreFacadeDeps {
            entry_repo,
            selection_repo,
            representation_repo,
            payload_resolver,
            blob_store,
            clock,
            write_coordinator,
            integration_mode,
        } = deps;

        let restore_uc = RestoreClipboardSelectionUseCase::new(
            entry_repo.clone(),
            write_coordinator,
            selection_repo,
            representation_repo,
            payload_resolver,
            blob_store,
            integration_mode,
        );
        let touch_uc = TouchClipboardEntryUseCase::new(entry_repo, clock);

        Self {
            restore_uc,
            touch_uc,
        }
    }

    #[instrument(skip_all, fields(entry_id = %entry_id))]
    pub async fn restore_entry(&self, entry_id: &str) -> Result<(), ClipboardRestoreError> {
        let parsed_id = EntryId::from(entry_id);

        self.restore_uc.execute(&parsed_id).await.map_err(|err| {
            // Translate the typed `PayloadResolveError` carried inside the
            // anyhow chain into a stable application error. Orphaned / Lost
            // are user-visible "content gone" outcomes (→ 410 at the API
            // layer); Integrity is a data-corruption bug and stays Internal.
            if let Some(payload_err) = err.downcast_ref::<PayloadResolveError>() {
                match payload_err {
                    PayloadResolveError::Orphaned { rep_id, state } => {
                        return ClipboardRestoreError::PayloadUnavailable {
                            entry_id: entry_id.to_string(),
                            rep_id: rep_id.to_string(),
                            state: state.as_str().to_string(),
                        };
                    }
                    PayloadResolveError::Lost { rep_id, .. } => {
                        return ClipboardRestoreError::PayloadUnavailable {
                            entry_id: entry_id.to_string(),
                            rep_id: rep_id.to_string(),
                            state: "Lost".to_string(),
                        };
                    }
                    PayloadResolveError::Integrity { .. } => {
                        // fall through — internal bug, return as Internal(500)
                    }
                }
            }

            let message = err.to_string();
            if message.to_lowercase().contains("not found") {
                ClipboardRestoreError::NotFound
            } else {
                ClipboardRestoreError::Internal(message)
            }
        })?;

        if let Err(err) = self.touch_uc.execute(&parsed_id).await {
            tracing::warn!(
                error = %err,
                entry_id = %entry_id,
                "touch_clipboard_entry failed after restore"
            );
        }

        Ok(())
    }
}
