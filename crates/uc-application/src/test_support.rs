//! Shared in-crate test fakes for the mobile-consumability surfaces.
//!
//! Several test modules (local advancer, inbound apply, restore, backfill,
//! unlock facades) need the same trivial stand-ins; keeping them here avoids
//! re-declaring one-off fakes per module.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use uc_core::clipboard::{
    ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetLine, EntryFileSetLineKind,
    FileSetMemberKind, FileSetMemberLocation,
};
use uc_core::ids::{BlobId, EntryId};
use uc_core::ports::clipboard::{ActiveClipboardRegisterError, EntryFileSetRepositoryPort};

use crate::clipboard_write::MobileConsumableBackfill;

/// `EntryFileSetRepositoryPort` fake returning a fixed `load` result. `save`
/// is unreachable because probe-side consumers never write.
pub(crate) struct FixedFileSets(pub(crate) Result<Option<EntryFileSet>, EntryFileSetError>);

impl FixedFileSets {
    /// No file set stored for any entry — the common non-file-class case.
    pub(crate) fn empty() -> Self {
        Self(Ok(None))
    }
}

#[async_trait]
impl EntryFileSetRepositoryPort for FixedFileSets {
    async fn save(
        &self,
        _entry_id: &EntryId,
        _file_set: &EntryFileSet,
    ) -> Result<(), EntryFileSetError> {
        unreachable!()
    }

    async fn load(&self, _entry_id: &EntryId) -> Result<Option<EntryFileSet>, EntryFileSetError> {
        match &self.0 {
            Ok(value) => Ok(value.clone()),
            Err(err) => Err(EntryFileSetError::Storage(err.to_string())),
        }
    }
}

/// One file-class manifest line with the given position and location.
pub(crate) fn file_set_line(
    line_index: i64,
    root_index: i64,
    relative_path: &str,
) -> EntryFileSetLine {
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

/// Flat manifest: two top-level files, no directory structure.
pub(crate) fn flat_file_set() -> EntryFileSet {
    EntryFileSet {
        lines: vec![file_set_line(0, 0, ""), file_set_line(1, 1, "")],
    }
}

/// Directory-shaped manifest: members nested under one root via relative
/// paths.
pub(crate) fn nested_file_set() -> EntryFileSet {
    EntryFileSet {
        lines: vec![file_set_line(0, 0, "a.txt"), file_set_line(2, 0, "b.txt")],
    }
}

/// Directory-shaped manifest: a single explicit empty-directory member.
pub(crate) fn empty_directory_file_set() -> EntryFileSet {
    EntryFileSet {
        lines: vec![EntryFileSetLine {
            line_index: 0,
            original_text: "folder".into(),
            member_location: Some(FileSetMemberLocation {
                root_index: 0,
                root_name: "folder".into(),
                relative_path: "".into(),
                kind: FileSetMemberKind::EmptyDirectory,
            }),
            kind: EntryFileSetLineKind::NonFile,
        }],
    }
}

/// `MobileConsumableBackfill` fake that always no-ops.
pub(crate) struct NoopMobileConsumableBackfill;

#[async_trait]
impl MobileConsumableBackfill for NoopMobileConsumableBackfill {
    async fn backfill(&self) -> Result<bool, ActiveClipboardRegisterError> {
        Ok(false)
    }
}

/// `MobileConsumableBackfill` fake counting invocations.
#[derive(Default)]
pub(crate) struct CountingMobileConsumableBackfill(AtomicUsize);

impl CountingMobileConsumableBackfill {
    pub(crate) fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl MobileConsumableBackfill for CountingMobileConsumableBackfill {
    async fn backfill(&self) -> Result<bool, ActiveClipboardRegisterError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(false)
    }
}
