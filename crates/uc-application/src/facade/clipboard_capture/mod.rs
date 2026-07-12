use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;
use uc_core::clipboard::{EntryFileSetExcludeReason, EntryFileSetLineKind};
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::ports::clipboard::{EntryFileSetRepositoryPort, PlatformClipboardPort};
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use crate::clipboard_capture::CaptureClipboardUseCase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedClipboardEntryView {
    pub entry_id: String,
    /// True when the snapshot matched existing content and the existing entry
    /// was resurfaced instead of a new one being created. Callers should
    /// refresh the UI but skip re-indexing / re-dispatching this entry.
    pub deduplicated: bool,
    /// The `snapshot_hash` persisted on this entry — its cross-device identity.
    ///
    /// Consumers that advertise this capture to peers (e.g. the
    /// active-clipboard register) MUST reuse this value rather than recomputing
    /// a hash from a separate copy of the snapshot, otherwise a file copy's
    /// advertised identity diverges from its dispatch identity and the receiver
    /// dedups into two entries.
    pub snapshot_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFileSetLineView {
    pub line_index: i64,
    pub root_index: Option<i64>,
    pub root_name: Option<String>,
    pub relative_path: Option<String>,
    pub member_kind: Option<String>,
    pub line_kind: String,
    pub exclude_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFileSetView {
    pub entry: CapturedClipboardEntryView,
    pub directory_structure: bool,
    pub content_digest_count: usize,
    pub lines: Vec<CapturedFileSetLineView>,
}

#[derive(Debug, Error)]
pub enum ClipboardCaptureFacadeError {
    #[error("clipboard capture failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait ClipboardCapturePort: Send + Sync {
    async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError>;
}

#[async_trait]
impl ClipboardCapturePort for CaptureClipboardUseCase {
    async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
        // Local-capture facade: the snapshot is authoritative for its own hash.
        // Inbound (`RemotePush`) never reaches here — it persists through the
        // `InboundCapture` port, which supplies the wire identity (F-4).
        let outcome = self
            .execute_with_origin(
                snapshot,
                origin,
                preset_entry_id,
                None,
                crate::clipboard_capture::CommitMode::Create,
            )
            .await
            .map_err(|err| ClipboardCaptureFacadeError::Internal(err.to_string()))?;
        Ok(outcome.map(|outcome| CapturedClipboardEntryView {
            entry_id: outcome.entry_id.to_string(),
            deduplicated: outcome.deduplicated,
            snapshot_hash: outcome.snapshot_hash,
        }))
    }
}

pub struct ClipboardCaptureFacade {
    capture: Arc<dyn ClipboardCapturePort>,
    current_clipboard: Arc<dyn PlatformClipboardPort>,
    entry_file_set_repo: Option<Arc<dyn EntryFileSetRepositoryPort>>,
}

impl ClipboardCaptureFacade {
    pub fn new(
        capture: Arc<dyn ClipboardCapturePort>,
        current_clipboard: Arc<dyn PlatformClipboardPort>,
    ) -> Self {
        Self {
            capture,
            current_clipboard,
            entry_file_set_repo: None,
        }
    }

    pub fn with_entry_file_set_repository(
        mut self,
        repository: Arc<dyn EntryFileSetRepositoryPort>,
    ) -> Self {
        self.entry_file_set_repo = Some(repository);
        self
    }

    pub async fn capture(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
        self.capture
            .capture(snapshot, origin, preset_entry_id)
            .await
    }

    /// Capture whatever is on the OS clipboard right now, without waiting for
    /// a change event. `Ok(None)` when there's nothing to capture (matches
    /// [`Self::capture`]'s "nothing changed" semantics).
    pub async fn capture_current(
        &self,
    ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
        let snapshot = self
            .current_clipboard
            .read_snapshot()
            .map_err(|err| ClipboardCaptureFacadeError::Internal(err.to_string()))?;
        self.capture(snapshot, ClipboardChangeOrigin::LocalCapture, None)
            .await
    }

    /// Capture explicit local paths through the normal file capture pipeline
    /// and return the persisted manifest without exposing absolute source
    /// paths. This is reserved for hidden CLI diagnostics and E2E tests.
    pub async fn capture_file_paths_for_diagnostics(
        &self,
        paths: Vec<PathBuf>,
    ) -> Result<CapturedFileSetView, ClipboardCaptureFacadeError> {
        if paths.is_empty() {
            return Err(ClipboardCaptureFacadeError::Internal(
                "at least one file path is required".to_string(),
            ));
        }

        let current_dir = std::env::current_dir()
            .map_err(|err| ClipboardCaptureFacadeError::Internal(err.to_string()))?;
        let mut uris = Vec::with_capacity(paths.len());
        for path in paths {
            let absolute = if path.is_absolute() {
                path
            } else {
                current_dir.join(path)
            };
            let uri = url::Url::from_file_path(absolute).map_err(|()| {
                ClipboardCaptureFacadeError::Internal(
                    "a path could not be represented as a file URL".to_string(),
                )
            })?;
            uris.push(uri.to_string());
        }

        let snapshot = SystemClipboardSnapshot {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                uris.join("\n").into_bytes(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        };
        let entry = self
            .capture(snapshot, ClipboardChangeOrigin::LocalCapture, None)
            .await?
            .ok_or_else(|| {
                ClipboardCaptureFacadeError::Internal(
                    "file capture did not produce an entry".to_string(),
                )
            })?;
        let repository = self.entry_file_set_repo.as_ref().ok_or_else(|| {
            ClipboardCaptureFacadeError::Internal(
                "file-set diagnostics are unavailable in this runtime".to_string(),
            )
        })?;
        let file_set = repository
            .load(&EntryId::from(entry.entry_id.as_str()))
            .await
            .map_err(|err| ClipboardCaptureFacadeError::Internal(err.to_string()))?
            .ok_or_else(|| {
                ClipboardCaptureFacadeError::Internal(
                    "captured entry has no file-set manifest".to_string(),
                )
            })?;

        let directory_structure = file_set.has_directory_structure();
        let content_digest_count = file_set.content_digest_contribution().len();
        let lines = file_set
            .lines
            .iter()
            .map(|line| {
                let (root_index, root_name, relative_path, member_kind) = line
                    .member_location
                    .as_ref()
                    .map(|location| {
                        (
                            Some(location.root_index),
                            Some(location.root_name.clone()),
                            Some(location.relative_path.clone()),
                            Some(location.kind.as_tag().to_string()),
                        )
                    })
                    .unwrap_or((None, None, None, None));
                let (line_kind, exclude_reason) = match line.kind {
                    EntryFileSetLineKind::File { .. } => ("file", None),
                    EntryFileSetLineKind::NonFile => ("non_file", None),
                    EntryFileSetLineKind::Excluded { reason } => {
                        ("excluded", Some(exclude_reason_tag(reason).to_string()))
                    }
                };
                CapturedFileSetLineView {
                    line_index: line.line_index,
                    root_index,
                    root_name,
                    relative_path,
                    member_kind,
                    line_kind: line_kind.to_string(),
                    exclude_reason,
                }
            })
            .collect();

        Ok(CapturedFileSetView {
            entry,
            directory_structure,
            content_digest_count,
            lines,
        })
    }
}

fn exclude_reason_tag(reason: EntryFileSetExcludeReason) -> &'static str {
    match reason {
        EntryFileSetExcludeReason::SizeCapExceeded => "size_cap_exceeded",
        EntryFileSetExcludeReason::IngestFailed => "ingest_failed",
        EntryFileSetExcludeReason::UnsupportedMember => "unsupported_member",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::clipboard::{
        ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetLine, EntryFileSetLineKind,
        FileSetMemberKind, FileSetMemberLocation, HashAlgorithm,
    };
    use uc_core::ports::clipboard::EntryFileSetRepositoryPort;
    use uc_core::SystemClipboardSnapshot;

    struct FakeCapture;

    #[async_trait]
    impl ClipboardCapturePort for FakeCapture {
        async fn capture(
            &self,
            _snapshot: SystemClipboardSnapshot,
            _origin: ClipboardChangeOrigin,
            _preset_entry_id: Option<EntryId>,
        ) -> Result<Option<CapturedClipboardEntryView>, ClipboardCaptureFacadeError> {
            Ok(Some(CapturedClipboardEntryView {
                entry_id: "entry-a".to_string(),
                deduplicated: false,
                snapshot_hash: "blake3v1:test".to_string(),
            }))
        }
    }

    struct FakePlatformClipboard {
        snapshot: SystemClipboardSnapshot,
    }

    impl PlatformClipboardPort for FakePlatformClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(self.snapshot.clone())
        }
    }

    fn empty_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            representations: Vec::new(),
            ts_ms: 0,
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn facade_with_fakes() -> ClipboardCaptureFacade {
        ClipboardCaptureFacade::new(
            std::sync::Arc::new(FakeCapture),
            std::sync::Arc::new(FakePlatformClipboard {
                snapshot: empty_snapshot(),
            }),
        )
    }

    #[tokio::test]
    async fn capture_returns_application_entry_id_string() {
        let facade = facade_with_fakes();
        let outcome = facade
            .capture(
                empty_snapshot(),
                ClipboardChangeOrigin::LocalCapture,
                Some(EntryId::from("entry-preset")),
            )
            .await
            .unwrap();

        assert_eq!(
            outcome,
            Some(CapturedClipboardEntryView {
                entry_id: "entry-a".to_string(),
                deduplicated: false,
                snapshot_hash: "blake3v1:test".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn capture_current_reads_live_snapshot_then_captures_it() {
        let facade = facade_with_fakes();
        let outcome = facade.capture_current().await.unwrap();

        assert_eq!(
            outcome,
            Some(CapturedClipboardEntryView {
                entry_id: "entry-a".to_string(),
                deduplicated: false,
                snapshot_hash: "blake3v1:test".to_string(),
            })
        );
    }

    struct FakeFileSetRepo;

    #[async_trait]
    impl EntryFileSetRepositoryPort for FakeFileSetRepo {
        async fn save(
            &self,
            _entry_id: &EntryId,
            _file_set: &EntryFileSet,
        ) -> Result<(), EntryFileSetError> {
            panic!("diagnostic capture must not save through its readback dependency")
        }

        async fn load(
            &self,
            entry_id: &EntryId,
        ) -> Result<Option<EntryFileSet>, EntryFileSetError> {
            assert_eq!(entry_id.as_str(), "entry-a");
            Ok(Some(EntryFileSet {
                lines: vec![EntryFileSetLine {
                    line_index: 1,
                    original_text: "file:///private/root".to_string(),
                    member_location: Some(FileSetMemberLocation {
                        root_index: 0,
                        root_name: "root".to_string(),
                        relative_path: "child.txt".to_string(),
                        kind: FileSetMemberKind::File,
                    }),
                    kind: EntryFileSetLineKind::File {
                        content_hash: ContentHash {
                            alg: HashAlgorithm::Blake3V1,
                            bytes: [7; 32],
                        },
                        blob_id: None,
                        size_bytes: None,
                    },
                }],
            }))
        }
    }

    #[tokio::test]
    async fn diagnostic_file_capture_returns_persisted_path_safe_manifest() {
        let facade = facade_with_fakes()
            .with_entry_file_set_repository(std::sync::Arc::new(FakeFileSetRepo));

        let result = facade
            .capture_file_paths_for_diagnostics(vec![std::path::PathBuf::from("/private/root")])
            .await
            .expect("diagnostic capture");

        assert_eq!(result.entry.entry_id, "entry-a");
        assert!(result.directory_structure);
        assert_eq!(result.content_digest_count, 0);
        assert_eq!(
            result.lines,
            vec![CapturedFileSetLineView {
                line_index: 1,
                root_index: Some(0),
                root_name: Some("root".to_string()),
                relative_path: Some("child.txt".to_string()),
                member_kind: Some("f".to_string()),
                line_kind: "file".to_string(),
                exclude_reason: None,
            }]
        );
    }
}
