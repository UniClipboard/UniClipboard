use std::sync::Arc;

use tracing::instrument;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ids::EntryId;
use uc_core::ports::{
    clipboard::ClipboardPayloadResolverPort, ClipboardEntryRepositoryPort,
    ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort, ClockPort,
};

use crate::clipboard_write::ClipboardWriteCoordinator;
use crate::usecases::clipboard_restore::{
    RestoreClipboardSelectionUseCase, TouchClipboardEntryUseCase,
};

#[derive(Debug, thiserror::Error)]
pub enum ClipboardRestoreError {
    #[error("clipboard entry not found")]
    NotFound,
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
