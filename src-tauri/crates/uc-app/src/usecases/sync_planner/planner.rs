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
        if origin == ClipboardChangeOrigin::RemotePush {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uc_core::{
        ids::{FormatId, RepresentationId},
        settings::model::Settings,
        ObservedClipboardRepresentation, SystemClipboardSnapshot,
    };

    // ── Test helpers ─────────────────────────────────────────────────────────

    struct MockSettings {
        settings: Option<Settings>,
    }

    #[async_trait::async_trait]
    impl SettingsPort for MockSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            match &self.settings {
                Some(s) => Ok(s.clone()),
                None => Err(anyhow::anyhow!("settings load error")),
            }
        }
        async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn make_settings(file_sync_enabled: bool, max_file_size: u64) -> Settings {
        let mut s = Settings::default();
        s.file_sync.file_sync_enabled = file_sync_enabled;
        s.file_sync.max_file_size = max_file_size;
        s
    }

    fn make_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from_str("text/plain"),
                Some("text/plain".parse().unwrap()),
                b"hello world".to_vec(),
            )],
        }
    }

    fn make_candidate(filename: &str, size: u64) -> FileCandidate {
        FileCandidate {
            path: PathBuf::from(format!("/tmp/{filename}")),
            size,
        }
    }

    fn make_planner(settings: Option<Settings>) -> OutboundSyncPlanner {
        OutboundSyncPlanner::new(Arc::new(MockSettings { settings }))
    }

    // ── Test 1: RemotePush → clipboard: None, files: [] ──────────────────────

    #[tokio::test]
    async fn test_remote_push_skips_sync() {
        let planner = make_planner(Some(make_settings(true, 5 * 1024 * 1024 * 1024)));
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::RemotePush,
                vec![],
                0,
            )
            .await;

        assert!(
            plan.clipboard.is_none(),
            "RemotePush must not trigger clipboard sync"
        );
        assert!(
            plan.files.is_empty(),
            "RemotePush must not trigger file sync"
        );
    }

    // ── Test 2: LocalRestore → clipboard: Some, files: [] ────────────────────

    #[tokio::test]
    async fn test_local_restore_triggers_clipboard_sync() {
        let planner = make_planner(Some(make_settings(true, 5 * 1024 * 1024 * 1024)));
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalRestore,
                vec![],
                0,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "LocalRestore must trigger clipboard sync (current behavior)"
        );
        assert!(
            plan.files.is_empty(),
            "LocalRestore must not trigger file sync"
        );
    }

    // ── Test 3: LocalCapture + file_sync_disabled → clipboard: Some, files: [] ──

    #[tokio::test]
    async fn test_local_capture_file_sync_disabled() {
        let planner = make_planner(Some(make_settings(false, 5 * 1024 * 1024 * 1024)));
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                vec![make_candidate("file.txt", 1024)],
                1,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "clipboard sync must be allowed when file_sync is disabled"
        );
        assert!(
            plan.files.is_empty(),
            "file sync must be skipped when file_sync is disabled"
        );
    }

    // ── Test 4: LocalCapture + file_sync_enabled + no files (extracted_paths_count=0) ──

    #[tokio::test]
    async fn test_local_capture_no_files_in_snapshot() {
        let planner = make_planner(Some(make_settings(true, 5 * 1024 * 1024 * 1024)));
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                vec![],
                0, // No paths extracted — snapshot had no files
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "clipboard sync must be allowed when no files existed in snapshot"
        );
        assert!(plan.files.is_empty(), "no files to sync");
    }

    // ── Test 5: All files within max_file_size ──────────────────────────────────

    #[tokio::test]
    async fn test_all_files_within_size_limit() {
        let max_size = 5 * 1024 * 1024 * 1024u64; // 5 GiB
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![
            make_candidate("photo.png", 1024 * 1024), // 1 MiB — within limit
            make_candidate("document.pdf", 50 * 1024 * 1024), // 50 MiB — within limit
        ];
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                2,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "clipboard sync must be allowed when all files are within size limit"
        );
        assert_eq!(plan.files.len(), 2, "both files must be included");

        let intent = plan.clipboard.unwrap();
        assert_eq!(
            intent.file_transfers.len(),
            2,
            "both file transfer mappings must be present in clipboard message"
        );

        // Verify transfer_ids are UUIDs (non-empty)
        for ft in &intent.file_transfers {
            assert!(!ft.transfer_id.is_empty(), "transfer_id must be set");
            assert!(!ft.filename.is_empty(), "filename must be set");
        }
        for fi in &plan.files {
            assert!(
                !fi.transfer_id.is_empty(),
                "file intent transfer_id must be set"
            );
            assert!(!fi.filename.is_empty(), "file intent filename must be set");
        }
    }

    // ── Test 6: All files exceed max_file_size → clipboard: None ───────────────

    #[tokio::test]
    async fn test_all_files_exceed_size_limit() {
        let max_size = 10 * 1024 * 1024u64; // 10 MiB
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![
            make_candidate("big_video.mp4", 500 * 1024 * 1024), // 500 MiB — too big
            make_candidate("huge_archive.zip", 2 * 1024 * 1024 * 1024), // 2 GiB — too big
        ];
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                2,
            )
            .await;

        assert!(
            plan.clipboard.is_none(),
            "clipboard sync must be suppressed when all_files_excluded by size limit"
        );
        assert!(plan.files.is_empty(), "no files must be synced");
    }

    // ── Test 7: Mixed file sizes — only within-limit files included ──────────────

    #[tokio::test]
    async fn test_mixed_file_sizes() {
        let max_size = 100 * 1024 * 1024u64; // 100 MiB
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![
            make_candidate("small.txt", 1024),                  // 1 KiB — OK
            make_candidate("medium.png", 50 * 1024 * 1024),     // 50 MiB — OK
            make_candidate("too_large.mp4", 500 * 1024 * 1024), // 500 MiB — exceeds
        ];
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                3,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "clipboard sync must be allowed when some files are within size limit"
        );
        assert_eq!(
            plan.files.len(),
            2,
            "only within-limit files must be synced"
        );

        let intent = plan.clipboard.unwrap();
        assert_eq!(
            intent.file_transfers.len(),
            2,
            "only within-limit file transfer mappings in clipboard message"
        );

        // Verify filenames
        let synced_names: Vec<&str> = plan.files.iter().map(|f| f.filename.as_str()).collect();
        assert!(synced_names.contains(&"small.txt"));
        assert!(synced_names.contains(&"medium.png"));
        assert!(!synced_names.contains(&"too_large.mp4"));
    }

    // ── Test 8: Settings load failure → safe defaults ────────────────────────────

    #[tokio::test]
    async fn test_settings_failure_safe_defaults() {
        let planner = make_planner(None); // None → settings load will fail
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                vec![],
                0,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "settings failure must default to allowing clipboard sync"
        );
        assert!(
            plan.files.is_empty(),
            "settings failure must default to no file sync"
        );
    }

    // ── Test 9: All files excluded by metadata failure (extracted_paths_count > 0, candidates empty) ──

    #[tokio::test]
    async fn test_all_files_excluded_by_metadata_failure() {
        let planner = make_planner(Some(make_settings(true, 5 * 1024 * 1024 * 1024)));
        // 3 paths were extracted from snapshot, but all metadata() calls failed at runtime
        // — runtime sends empty file_candidates but extracted_paths_count = 3
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                vec![], // All excluded at runtime by metadata() failure
                3,      // But 3 paths were originally extracted
            )
            .await;

        assert!(
            plan.clipboard.is_none(),
            "clipboard sync must be suppressed when all files were excluded by metadata failure"
        );
        assert!(plan.files.is_empty(), "no files to sync");
    }

    // ── Additional: verify file_transfers and files use same transfer_id ──────────

    #[tokio::test]
    async fn test_file_transfers_match_file_intents() {
        let max_size = 1024 * 1024 * 1024u64; // 1 GiB
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![make_candidate("report.pdf", 2 * 1024 * 1024)]; // 2 MiB
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                1,
            )
            .await;

        let intent = plan.clipboard.unwrap();
        assert_eq!(intent.file_transfers.len(), 1);
        assert_eq!(plan.files.len(), 1);

        // transfer_id in clipboard message must match the file intent
        assert_eq!(
            intent.file_transfers[0].transfer_id, plan.files[0].transfer_id,
            "file_transfer mapping and FileSyncIntent must have the same transfer_id"
        );
        assert_eq!(intent.file_transfers[0].filename, plan.files[0].filename);
    }

    // ── Additional: max_file_size boundary (exactly at limit should be included) ──

    #[tokio::test]
    async fn test_file_exactly_at_size_limit_is_included() {
        let max_size = 100 * 1024 * 1024u64; // 100 MiB exactly
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![make_candidate("exact.dat", max_size)]; // exactly at limit
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                1,
            )
            .await;

        assert!(
            plan.clipboard.is_some(),
            "file exactly at size limit must be included"
        );
        assert_eq!(plan.files.len(), 1, "file exactly at limit must be synced");
    }

    // ── Additional: file 1 byte over limit is excluded ─────────────────────────

    #[tokio::test]
    async fn test_file_one_byte_over_limit_is_excluded() {
        let max_size = 100 * 1024 * 1024u64; // 100 MiB
        let planner = make_planner(Some(make_settings(true, max_size)));
        let candidates = vec![make_candidate("over.dat", max_size + 1)]; // 1 byte over
        let plan = planner
            .plan(
                make_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                candidates,
                1,
            )
            .await;

        // only 1 candidate, 1 extracted, all excluded by size → all_files_excluded
        assert!(
            plan.clipboard.is_none(),
            "file 1 byte over limit triggers all_files_excluded"
        );
        assert!(plan.files.is_empty());
    }
}
