//! OutboundSyncPlanner — consolidates all outbound sync eligibility decisions.

use std::sync::Arc;

use tracing::warn;
use uc_core::{
    network::protocol::FileTransferMapping, ports::SettingsPort, ClipboardChangeOrigin,
    SystemClipboardSnapshot,
};
use uuid::Uuid;

use super::types::{ClipboardSyncIntent, FileCandidate, FileSyncIntent, OutboundSyncPlan};

/// Consolidates outbound sync eligibility decisions into a single `plan()` call.
///
/// The planner is a pure domain service: it loads settings and applies filtering logic,
/// but performs NO filesystem I/O. All file sizes must be pre-computed by the runtime
/// (via `std::fs::metadata()`) and provided as `Vec<FileCandidate>`.
pub struct OutboundSyncPlanner {
    settings: Arc<dyn SettingsPort>,
}

impl OutboundSyncPlanner {
    /// Create a new planner with the given settings port.
    pub fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self { settings }
    }

    /// Compute the outbound sync plan for a clipboard change event.
    ///
    /// # Parameters
    ///
    /// - `snapshot` — The clipboard snapshot that triggered the change.
    /// - `origin` — Where the change originated (local capture, local restore, or remote push).
    /// - `file_candidates` — Pre-computed file candidates with path and size resolved by the
    ///   runtime. For non-`LocalCapture` origins or when `file_sync_enabled` is false, pass
    ///   an empty `Vec`.
    /// - `extracted_paths_count` — The number of paths that the runtime extracted from the
    ///   snapshot BEFORE any metadata filtering. This allows the planner to detect when ALL
    ///   files were excluded by metadata failures, even when `file_candidates` is empty.
    ///
    /// # Returns
    ///
    /// An `OutboundSyncPlan` that describes what should be synced. This method is infallible:
    /// on settings load failure it returns safe defaults (clipboard sync allowed, no file sync).
    pub async fn plan(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        file_candidates: Vec<FileCandidate>,
        extracted_paths_count: usize,
    ) -> OutboundSyncPlan {
        // Guard: RemotePush is never re-synced outbound.
        if origin.is_remote_push() {
            return OutboundSyncPlan {
                clipboard: None,
                files: vec![],
            };
        }

        // Load settings; on failure use safe defaults.
        let settings = match self.settings.load().await {
            Ok(s) => s,
            Err(err) => {
                warn!(
                    error = %err,
                    "OutboundSyncPlanner: failed to load settings; using safe defaults \
                     (clipboard sync allowed, no file sync)"
                );
                // Safe default: allow clipboard sync, skip file sync.
                return OutboundSyncPlan {
                    clipboard: Some(ClipboardSyncIntent {
                        snapshot,
                        file_transfers: vec![],
                    }),
                    files: vec![],
                };
            }
        };

        // File sync is only applicable for LocalCapture.
        let (eligible_files, file_transfers) = if origin == ClipboardChangeOrigin::LocalCapture
            && settings.file_sync.file_sync_enabled
        {
            let max_file_size = settings.file_sync.max_file_size;

            let mut eligible: Vec<FileSyncIntent> = Vec::new();
            let mut mappings: Vec<FileTransferMapping> = Vec::new();

            for candidate in file_candidates {
                if candidate.size <= max_file_size {
                    let transfer_id = Uuid::new_v4().to_string();
                    let filename = candidate
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();

                    mappings.push(FileTransferMapping {
                        transfer_id: transfer_id.clone(),
                        filename: filename.clone(),
                    });

                    eligible.push(FileSyncIntent {
                        path: candidate.path,
                        transfer_id,
                        filename,
                        size: candidate.size,
                    });
                }
            }

            (eligible, mappings)
        } else {
            (
                Vec::<FileSyncIntent>::new(),
                Vec::<FileTransferMapping>::new(),
            )
        };

        // all_files_excluded guard: only applies when we actually attempted file sync
        // (LocalCapture + file_sync_enabled). If file sync was not attempted, file_candidates
        // and extracted_paths_count are irrelevant.
        let file_sync_attempted =
            origin == ClipboardChangeOrigin::LocalCapture && settings.file_sync.file_sync_enabled;
        let all_files_excluded =
            file_sync_attempted && extracted_paths_count > 0 && eligible_files.is_empty();

        if all_files_excluded {
            return OutboundSyncPlan {
                clipboard: None,
                files: vec![],
            };
        }

        OutboundSyncPlan {
            clipboard: Some(ClipboardSyncIntent {
                snapshot,
                file_transfers,
            }),
            files: eligible_files,
        }
    }
}
