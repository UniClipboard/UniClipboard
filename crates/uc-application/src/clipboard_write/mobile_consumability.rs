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
pub struct BackfillMobileConsumableRef {
    load_register: Arc<dyn LoadActiveClipboardPort>,
    backfill: Arc<dyn BackfillMobileConsumableClipboardPort>,
    probe: MobileConsumabilityProbe,
}

#[async_trait]
pub trait MobileConsumableBackfill: Send + Sync {
    async fn backfill(&self) -> Result<bool, ActiveClipboardRegisterError>;
}

impl BackfillMobileConsumableRef {
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
impl MobileConsumableBackfill for BackfillMobileConsumableRef {
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
    use uc_core::clipboard::{
        ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetLine, EntryFileSetLineKind,
        FileSetMemberKind, FileSetMemberLocation,
    };
    use uc_core::ids::BlobId;
    use uc_core::ids::DeviceId;

    struct FixedFileSets(Result<Option<EntryFileSet>, EntryFileSetError>);

    #[async_trait]
    impl EntryFileSetRepositoryPort for FixedFileSets {
        async fn save(
            &self,
            _entry_id: &EntryId,
            _file_set: &EntryFileSet,
        ) -> Result<(), EntryFileSetError> {
            unreachable!()
        }

        async fn load(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<EntryFileSet>, EntryFileSetError> {
            match &self.0 {
                Ok(value) => Ok(value.clone()),
                Err(err) => Err(EntryFileSetError::Storage(err.to_string())),
            }
        }
    }

    fn file_line(line_index: i64, root_index: i64, relative_path: &str) -> EntryFileSetLine {
        EntryFileSetLine {
            line_index,
            original_text: relative_path.to_string(),
            member_location: Some(FileSetMemberLocation {
                root_index,
                root_name: "root".to_string(),
                relative_path: relative_path.to_string(),
                kind: FileSetMemberKind::File,
            }),
            kind: EntryFileSetLineKind::File {
                content_hash: ContentHash::from(&[7; 32]),
                blob_id: Some(BlobId::from("blob")),
                size_bytes: Some(1),
            },
        }
    }

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
        assert!(
            probe(Ok(Some(EntryFileSet {
                lines: vec![file_line(0, 0, ""), file_line(1, 1, "")],
            })))
            .await
        );
    }

    #[tokio::test]
    async fn directory_file_set_is_not_mobile_consumable() {
        assert!(
            !probe(Ok(Some(EntryFileSet {
                lines: vec![file_line(0, 0, "a.txt"), file_line(2, 0, "b.txt")],
            })))
            .await
        );
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
        let backfill = BackfillMobileConsumableRef::new(
            Arc::new(FixedRegister(Some(active_state()))),
            recorder.clone(),
            MobileConsumabilityProbe::new(Arc::new(FixedFileSets(Ok(None)))),
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
        let backfill = BackfillMobileConsumableRef::new(
            Arc::new(FixedRegister(Some(active_state()))),
            recorder.clone(),
            MobileConsumabilityProbe::new(Arc::new(FixedFileSets(Ok(Some(EntryFileSet {
                lines: vec![file_line(0, 0, "a.txt"), file_line(2, 0, "b.txt")],
            }))))),
        );

        assert!(!backfill.backfill().await.unwrap());
        assert!(recorder.references.lock().unwrap().is_empty());
    }
}
