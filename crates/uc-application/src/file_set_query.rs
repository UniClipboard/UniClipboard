//! Shared reads over the entry file-set port.

use uc_core::clipboard::EntryFileSetError;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::EntryFileSetRepositoryPort;

/// Whether an entry's file-set manifest describes a directory root.
///
/// Folds the manifest load and its `None` case shared by the search,
/// live-index, and history-list projections: an **absent** manifest is not a
/// directory (`Ok(false)`). The directory rule itself is the single authority
/// `EntryFileSet::has_directory_structure`. A load **error** is propagated so
/// each projection can log it at its own level and degrade to non-directory.
pub(crate) async fn load_has_directory_structure(
    repo: &dyn EntryFileSetRepositoryPort,
    entry_id: &EntryId,
) -> Result<bool, EntryFileSetError> {
    Ok(repo
        .load(entry_id)
        .await?
        .is_some_and(|file_set| file_set.has_directory_structure()))
}
