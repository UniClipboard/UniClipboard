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
///
/// # `max_file_size` and `ClipboardChangeOrigin::Resend`
///
/// The user-configurable `settings.file_sync.max_file_size` cap is enforced on
/// `LocalCapture` origins (automatic outbound) but **bypassed** on `Resend`
/// origins. The cap is a bandwidth guard for the implicit capture lane; resend
/// is an explicit user action and is allowed to ignore it. `file_sync_enabled`
/// remains in force on both origins — disabling file sync is a stronger user
/// intent ("never send files") that resend must respect.
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

        // File sync is only applicable for outbound, user-initiated origins
        // (LocalCapture + Resend). RemotePush is already guarded above;
        // LocalRestore writes the snapshot back to the local clipboard with no
        // outbound fan-out, so it has no file-sync side.
        let user_initiated_outbound = matches!(
            origin,
            ClipboardChangeOrigin::LocalCapture | ClipboardChangeOrigin::Resend
        );
        let (eligible_files, file_transfers) =
            if user_initiated_outbound && settings.file_sync.file_sync_enabled {
                let max_file_size = settings.file_sync.max_file_size;
                // Resend is an explicit user action — the user already saw the
                // entry and chose to retry. `max_file_size` exists to keep the
                // automatic LocalCapture lane from spending bandwidth on huge
                // files behind the user's back, and that rationale does not
                // apply when the user clicks "resend". Bypassing here also
                // tightens `PayloadLost` semantics to "we no longer hold it"
                // instead of conflating with "we hold it but it's bigger than
                // your cap". `file_sync_enabled` is *not* bypassed — turning
                // file sync off entirely is a different intent ("I never want
                // to send files") that resend must still respect.
                let bypass_size_limit = matches!(origin, ClipboardChangeOrigin::Resend);

                let mut eligible: Vec<FileSyncIntent> = Vec::new();
                let mut mappings: Vec<FileTransferMapping> = Vec::new();

                for candidate in file_candidates {
                    if bypass_size_limit || candidate.size <= max_file_size {
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
        // (LocalCapture | Resend + file_sync_enabled). If file sync was not attempted,
        // file_candidates and extracted_paths_count are irrelevant.
        let file_sync_attempted = user_initiated_outbound && settings.file_sync.file_sync_enabled;
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
    use std::path::PathBuf;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use uc_core::clipboard::{ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::settings::model::Settings;
    use uc_core::MimeType;

    use super::*;

    struct InMemorySettings(Mutex<Settings>);

    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.0.lock().unwrap().clone())
        }
        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.0.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    fn planner_with_max_file_size(max: u64) -> OutboundSyncPlanner {
        let mut settings = Settings::default();
        settings.file_sync.file_sync_enabled = true;
        settings.file_sync.max_file_size = max;
        OutboundSyncPlanner::new(Arc::new(InMemorySettings(Mutex::new(settings))))
    }

    fn planner_with_file_sync_disabled() -> OutboundSyncPlanner {
        let mut settings = Settings::default();
        settings.file_sync.file_sync_enabled = false;
        // max_file_size value is irrelevant when file_sync is off; pick a
        // hostile zero so a future regression that flips the gate accidentally
        // can't sneak past on a "default cap is huge" technicality.
        settings.file_sync.max_file_size = 0;
        OutboundSyncPlanner::new(Arc::new(InMemorySettings(Mutex::new(settings))))
    }

    fn text_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_700_000_000_000,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"hi".to_vec(),
            )],
            file_content_digests: Vec::new(),
        }
    }

    fn candidate(name: &str, size: u64) -> FileCandidate {
        FileCandidate {
            path: PathBuf::from(format!("/tmp/{name}")),
            size,
        }
    }

    /// Resend origin must bypass `settings.file_sync.max_file_size` while
    /// LocalCapture continues to enforce it. The bypass also has to hold
    /// when the cap is 0 (a deliberately hostile value), to guard against
    /// the predicate being silently inverted in a future edit.
    #[tokio::test]
    async fn resend_origin_bypasses_max_file_size_filter() {
        // --- LocalCapture: cap=1KB, candidate=2KB → fully excluded ---
        let planner = planner_with_max_file_size(1024);
        let plan = planner
            .plan(
                text_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                vec![candidate("big.bin", 2048)],
                1,
            )
            .await;
        assert!(
            plan.clipboard.is_none(),
            "LocalCapture must still respect max_file_size: when the only \
             candidate exceeds the cap the planner must suppress the \
             clipboard intent (all_files_excluded path)"
        );
        assert!(plan.files.is_empty());

        // --- Resend: same setup → candidate slips through ---
        let planner = planner_with_max_file_size(1024);
        let plan = planner
            .plan(
                text_snapshot(),
                ClipboardChangeOrigin::Resend,
                vec![candidate("big.bin", 2048)],
                1,
            )
            .await;
        let intent = plan
            .clipboard
            .expect("Resend bypasses max_file_size → clipboard intent emitted");
        assert_eq!(intent.file_transfers.len(), 1);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].size, 2048);

        // --- Resend with cap=0: defensive — confirm predicate didn't
        // get refactored into `>= max` or `< max` ---
        let planner = planner_with_max_file_size(0);
        let plan = planner
            .plan(
                text_snapshot(),
                ClipboardChangeOrigin::Resend,
                vec![candidate("any.bin", 1)],
                1,
            )
            .await;
        assert!(
            plan.clipboard.is_some(),
            "cap=0 must not block Resend — bypass is unconditional for this origin"
        );
        assert_eq!(plan.files.len(), 1);
    }

    /// Resend bypasses `max_file_size` but MUST still honour
    /// `file_sync_enabled = false`. The doc-comment on the planner claims
    /// the gate "remains in force on both origins" — without an assertion
    /// for the Resend + disabled combo, a future refactor that flips the
    /// `user_initiated_outbound && file_sync_enabled` predicate would slip
    /// through the existing bypass test (which only exercises the enabled
    /// path). Also confirms the text-only clipboard intent still emits
    /// (no `all_files_excluded` short-circuit when file sync isn't even
    /// attempted).
    #[tokio::test]
    async fn resend_origin_respects_file_sync_disabled_gate() {
        let planner = planner_with_file_sync_disabled();
        let plan = planner
            .plan(
                text_snapshot(),
                ClipboardChangeOrigin::Resend,
                // Caller would normally pass empty when sync is off, but
                // exercise the defensive contract: planner must drop these
                // unilaterally regardless of what the caller hands it.
                vec![candidate("ignored.bin", 1)],
                1,
            )
            .await;

        let intent = plan.clipboard.expect(
            "file_sync_enabled = false must NOT suppress the clipboard intent — \
             the all_files_excluded guard only fires when file sync was \
             actually attempted (user_initiated_outbound && file_sync_enabled)",
        );
        assert!(
            intent.file_transfers.is_empty(),
            "no file transfers when file sync is disabled; got {:?}",
            intent.file_transfers
        );
        assert!(
            plan.files.is_empty(),
            "Resend must honour file_sync_enabled = false (stronger user intent \
             than max_file_size); got {} files",
            plan.files.len()
        );
    }
}
