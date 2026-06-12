use anyhow::Result;
use std::sync::Arc;

use uc_core::ids::EntryId;
use uc_core::ports::{ClipboardEntryRepositoryPort, ClockPort};

/// Update clipboard entry active time.
pub(crate) struct TouchClipboardEntryUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    clock: Arc<dyn ClockPort>,
}

impl TouchClipboardEntryUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self { entry_repo, clock }
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<bool> {
        let now_ms = self.clock.now_ms();
        self.entry_repo.touch_entry(entry_id, now_ms).await
    }
}
