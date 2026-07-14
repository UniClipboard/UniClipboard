use std::sync::Arc;

use async_trait::async_trait;
use tracing::warn;
use uc_core::clipboard::MobileConsumableRef;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    ActiveClipboardRegisterError, BackfillMobileConsumableClipboardPort,
    EntryFileSetRepositoryPort, LoadActiveClipboardPort,
};

/// Applies the domain file-set rule to mobile clipboard consumption.
#[derive(Clone)]
pub struct MobileConsumabilityProbe {
    file_sets: Arc<dyn EntryFileSetRepositoryPort>,
}

impl MobileConsumabilityProbe {
    pub fn new(file_sets: Arc<dyn EntryFileSetRepositoryPort>) -> Self {
        Self { file_sets }
    }

    /// Missing or flat manifests remain consumable. Query failures fail closed
    /// so an unknown file-set shape never reaches a mobile client.
    pub async fn is_mobile_consumable(&self, entry_id: &EntryId) -> bool {
        match self.file_sets.load(entry_id).await {
            Ok(None) => true,
            Ok(Some(file_set)) => !file_set.has_directory_structure(),
            Err(err) => {
                warn!(
                    error = %err,
                    entry_id = %entry_id,
                    "mobile consumability probe failed; treating entry as non-consumable"
                );
                false
            }
        }
    }
}

/// Idempotently initializes the mobile-consumable reference after unlock.
pub struct MobileConsumableRefBackfill {
    load_register: Arc<dyn LoadActiveClipboardPort>,
    backfill: Arc<dyn BackfillMobileConsumableClipboardPort>,
    probe: MobileConsumabilityProbe,
}

#[async_trait]
pub trait MobileConsumableBackfill: Send + Sync {
    /// Initialize the mobile-consumable reference from the current register
    /// value when it is absent. Returns whether a reference was written.
    async fn backfill(&self) -> Result<bool, ActiveClipboardRegisterError>;

    /// Fire-and-forget variant for unlock/resume flows: the reference is a
    /// rebuildable shadow of the register, so a failed backfill only logs and
    /// never blocks the unlock itself.
    async fn backfill_best_effort(&self) {
        if let Err(err) = self.backfill().await {
            warn!(error = %err, "mobile-consumable reference backfill failed");
        }
    }
}

impl MobileConsumableRefBackfill {
    pub fn new(
        load_register: Arc<dyn LoadActiveClipboardPort>,
        backfill: Arc<dyn BackfillMobileConsumableClipboardPort>,
        probe: MobileConsumabilityProbe,
    ) -> Self {
        Self {
            load_register,
            backfill,
            probe,
        }
    }

    async fn execute(&self) -> Result<bool, ActiveClipboardRegisterError> {
        let Some(state) = self.load_register.load().await? else {
            return Ok(false);
        };
        if !self.probe.is_mobile_consumable(&state.entry_id).await {
            return Ok(false);
        }
        self.backfill
            .backfill_mobile_consumable_if_current(&MobileConsumableRef::new(
                state.snapshot_hash,
                state.entry_id,
            ))
            .await
    }
}

#[async_trait]
impl MobileConsumableBackfill for MobileConsumableRefBackfill {
    async fn backfill(&self) -> Result<bool, ActiveClipboardRegisterError> {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::clipboard::ActiveClipboardState;
    use uc_core::clipboard::{EntryFileSet, EntryFileSetError};
    use uc_core::ids::DeviceId;

    use crate::test_support::{flat_file_set, nested_file_set, FixedFileSets};

    async fn probe(result: Result<Option<EntryFileSet>, EntryFileSetError>) -> bool {
        MobileConsumabilityProbe::new(Arc::new(FixedFileSets(result)))
            .is_mobile_consumable(&EntryId::from("entry"))
            .await
    }

    #[tokio::test]
    async fn entry_without_file_set_is_mobile_consumable() {
        assert!(probe(Ok(None)).await);
    }

    #[tokio::test]
    async fn flat_file_set_is_mobile_consumable() {
        assert!(probe(Ok(Some(flat_file_set()))).await);
    }

    #[tokio::test]
    async fn directory_file_set_is_not_mobile_consumable() {
        assert!(!probe(Ok(Some(nested_file_set()))).await);
    }

    #[tokio::test]
    async fn file_set_query_failure_is_not_mobile_consumable() {
        assert!(!probe(Err(EntryFileSetError::Storage("boom".into()))).await);
    }

    struct FixedRegister(Option<ActiveClipboardState>);

    #[async_trait]
    impl LoadActiveClipboardPort for FixedRegister {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
            Ok(self.0.clone())
        }
    }

    #[derive(Default)]
    struct RecordingBackfill {
        references: Mutex<Vec<MobileConsumableRef>>,
    }

    #[async_trait]
    impl BackfillMobileConsumableClipboardPort for RecordingBackfill {
        async fn backfill_mobile_consumable_if_current(
            &self,
            reference: &MobileConsumableRef,
        ) -> Result<bool, ActiveClipboardRegisterError> {
            self.references.lock().unwrap().push(reference.clone());
            Ok(true)
        }
    }

    fn active_state() -> ActiveClipboardState {
        ActiveClipboardState::new(
            "blake3v1:legacy",
            EntryId::from("legacy-entry"),
            10,
            DeviceId::new("legacy-device"),
        )
    }

    #[tokio::test]
    async fn ordinary_legacy_register_value_is_backfilled_after_unlock() {
        let recorder = Arc::new(RecordingBackfill::default());
        let backfill = MobileConsumableRefBackfill::new(
            Arc::new(FixedRegister(Some(active_state()))),
            recorder.clone(),
            MobileConsumabilityProbe::new(Arc::new(FixedFileSets::empty())),
        );

        assert!(backfill.backfill().await.unwrap());
        assert_eq!(
            recorder.references.lock().unwrap().as_slice(),
            &[MobileConsumableRef::new(
                "blake3v1:legacy",
                EntryId::from("legacy-entry")
            )]
        );
    }

    #[tokio::test]
    async fn directory_legacy_register_value_is_not_backfilled() {
        let recorder = Arc::new(RecordingBackfill::default());
        let backfill = MobileConsumableRefBackfill::new(
            Arc::new(FixedRegister(Some(active_state()))),
            recorder.clone(),
            MobileConsumabilityProbe::new(Arc::new(FixedFileSets(Ok(Some(nested_file_set()))))),
        );

        assert!(!backfill.backfill().await.unwrap());
        assert!(recorder.references.lock().unwrap().is_empty());
    }
}
