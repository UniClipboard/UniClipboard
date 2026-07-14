use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;
use tracing::{debug, info, warn};
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::{
    is_file_mime_or_format, ClipboardPayloadSource, EntryFileSetExcludeReason,
    EntryFileSetLineKind, FileSetMemberKind, FileSetMemberLocation,
};
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    ClipboardPayloadResolverPort, EntryFileSetRepositoryPort, GetClipboardEntryPort,
    GetRepresentationPort, UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::{
    ClipboardEventRepositoryPort, ClipboardSelectionRepositoryPort, DeviceIdentityPort,
    EntryDeliveryRepositoryPort, SettingsPort,
};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

use crate::facade::{
    BlobTransferError, BlobTransferFacade, ClipboardSyncFacade, PublishBlobCommand,
    PublishBlobPathCommand, PublishBlobResult,
};
use crate::sync_planner::{FileCandidate, FileSyncIntent, OutboundSyncPlanner};
use crate::usecases::clipboard_sync::apply_inbound::{
    compute_file_set_component, InboundFileSetManifest, InboundFileSetMember,
};
use crate::usecases::clipboard_sync::resend_entry::{
    ResendEntryDeps, ResendEntryRunner, ResendEntryUseCase,
};
use crate::usecases::clipboard_sync::V3BlobRef;

pub use crate::usecases::clipboard_sync::resend_entry::{
    NotResendableReason, ResendEntryCommand, ResendEntryError, ResendReport,
};

mod file_uri_line;
pub(crate) use file_uri_line::{parse_uri_list_line, UriListLineKind};

/// Crate-internal adapter trait over [`BlobTransferFacade`]'s publish surface.
///
/// 抽出这层只为单测 ergonomics:[`publish_file_blob_refs`] /
/// [`publish_oversized_inline_blob_refs`] 同时被 `dispatch_capture` 与
/// `ResendEntryUseCase` 复用,后者需要在不构造完整 `BlobTransferFacade`(深依赖
/// `PublishBlobUseCase` + `ContentHashPort` + `BlobTransferPort` + `BlobReferenceRepositoryPort`)
/// 的前提下断言 publish 调用入参/计数。trait 不出现在任何对外 API,production wiring
/// 通过 [`OutboundBlobPublishGateway`]-for-[`BlobTransferFacade`] blanket impl 自动满足。
#[async_trait]
pub(crate) trait OutboundBlobPublishGateway: Send + Sync {
    async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError>;

    async fn publish_blob_path(
        &self,
        command: PublishBlobPathCommand,
    ) -> Result<PublishBlobResult, BlobTransferError>;
}

#[async_trait]
impl OutboundBlobPublishGateway for BlobTransferFacade {
    async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        BlobTransferFacade::publish_blob(self, command).await
    }

    async fn publish_blob_path(
        &self,
        command: PublishBlobPathCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        BlobTransferFacade::publish_blob_path(self, command).await
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardOutboundInput {
    pub entry_id: String,
    pub snapshot: SystemClipboardSnapshot,
    pub origin: ClipboardChangeOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardOutboundOutcome {
    Dispatched {
        accepted: usize,
        duplicate: usize,
        offline: usize,
        errored: usize,
        /// Peers whose result the main flow didn't wait for (fan-out deadline
        /// hit). Their delivery records are being written by a background
        /// continuation; counted here only for observability.
        pending: usize,
        blob_ref_count: usize,
    },
    Skipped {
        reason: String,
    },
}

#[derive(Debug, Error)]
pub enum ClipboardOutboundError {
    #[error("clipboard outbound dispatch failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait ClipboardOutboundPort: Send + Sync {
    async fn dispatch_capture(
        &self,
        input: ClipboardOutboundInput,
    ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError>;
}

/// Dependencies for [`ClipboardOutboundFacade`]. Bundles the deps for
/// both the `dispatch_capture` path (settings + clipboard_sync +
/// blob_transfer, originally [`ClipboardOutboundDispatcher`]'s deps) and
/// the `resend_entry` path (entry / event / selection / representation /
/// delivery / trusted_peer / payload_resolver / blob_store / device_identity
/// ports — all the things [`ResendEntryUseCase`] needs).
///
/// Bootstrap assembles this once from its wiring deps; the facade
/// constructs the dispatcher + resend use case internally.
pub struct ClipboardOutboundDeps {
    // ── dispatcher path ────────────────────────────────────────────────
    pub settings: Arc<dyn SettingsPort>,
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    pub blob_transfer: Arc<BlobTransferFacade>,
    /// Single source of truth for a file-class entry's member list on the
    /// outbound path (dispatch + resend): the manifest persisted at capture
    /// time, instead of re-parsing raw reps per send. See
    /// [`resolve_outbound_file_set`].
    pub entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,

    // ── resend path ────────────────────────────────────────────────────
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn GetRepresentationPort>,
    pub rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    pub device_identity: Arc<dyn DeviceIdentityPort>,
}

pub struct ClipboardOutboundDispatcher {
    settings: Arc<dyn SettingsPort>,
    clipboard_sync: Arc<ClipboardSyncFacade>,
    blob_transfer: Arc<BlobTransferFacade>,
    entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,
}

impl ClipboardOutboundDispatcher {
    fn from_deps(deps: &ClipboardOutboundDeps) -> Self {
        Self {
            settings: deps.settings.clone(),
            clipboard_sync: deps.clipboard_sync.clone(),
            blob_transfer: deps.blob_transfer.clone(),
            entry_file_set_repo: deps.entry_file_set_repo.clone(),
        }
    }
}

#[async_trait]
impl ClipboardOutboundPort for ClipboardOutboundDispatcher {
    async fn dispatch_capture(
        &self,
        mut input: ClipboardOutboundInput,
    ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError> {
        if input.origin.is_remote_push() {
            return Ok(ClipboardOutboundOutcome::Skipped {
                reason: "remote_push_echo".to_string(),
            });
        }

        // Strip `LocalFile` source reps before envelope construction.
        //
        // capture pipeline 已经把 LocalFile rep 物化到本机 blob 仓库(BlobReady 状态)。
        // 对端无法从远端 path 读字节,LocalFile 在 wire 协议上无意义;且 V3 envelope
        // BinaryRepresentation 只支持 inline 字节,若 LocalFile 留在 snapshot 里会触
        // 发 encode 时的 expect_inline_bytes panic。
        //
        // 真正的"图片字节跨设备传输"路径:同一文件已在 files rep(uri-list)里以路径
        // 形式存在,outbound 通过 publish_file_blob_refs 走 iroh-blobs add_path 流式
        // 上传得到 V3BlobRef,对端 inbound materializer 用 BlobTransferFacade.fetch_blob
        // 把真实文件落到本地 file-cache,完整字节恢复。
        let stripped = input
            .snapshot
            .representations
            .iter()
            .filter(|rep| matches!(rep.source(), ClipboardPayloadSource::LocalFile { .. }))
            .count();
        if stripped > 0 {
            input
                .snapshot
                .representations
                .retain(|rep| !matches!(rep.source(), ClipboardPayloadSource::LocalFile { .. }));
            info!(
                entry_id = %input.entry_id,
                stripped_count = stripped,
                "outbound: stripped LocalFile reps before envelope construction (already in blob store; \
                 peers receive bytes via files rep + iroh-blobs)"
            );
        }

        // Phase timing: dispatch_capture 是 outbound 关键路径,从 capture
        // 完成到 dispatch 之间任何阶段卡顿都会让 UI 看起来"复制后没动静"。
        // 拆分阶段计时是为了在用户报"复制后很久才同步"这类问题时,能快速
        // 区分卡在 metadata / plan / publish_files / publish_inline / dispatch
        // 哪一段。详见 GH#487。
        let entry_id_str = input.entry_id.clone();
        let snapshot_rep_count = input.snapshot.representations.len();
        let dispatch_start = Instant::now();

        let entry_id = EntryId::from(input.entry_id.as_str());

        let resolution = if input.origin == ClipboardChangeOrigin::LocalCapture {
            resolve_outbound_file_set(
                self.entry_file_set_repo.as_ref(),
                &entry_id,
                &input.snapshot,
            )
            .await
        } else {
            OutboundFileSetResolution::NotFileClass
        };
        let (resolved_paths, from_manifest, expected_digests, directory_members) = match resolution
        {
            OutboundFileSetResolution::NotFileClass => (Vec::new(), false, Vec::new(), None),
            OutboundFileSetResolution::Manifest {
                paths,
                expected_digests,
            } => (paths, true, expected_digests, None),
            OutboundFileSetResolution::DirectorySyncable { paths, members } => {
                (paths, true, Vec::new(), Some(members))
            }
            OutboundFileSetResolution::Fallback { paths } => (paths, false, Vec::new(), None),
            OutboundFileSetResolution::Excluded {
                ingest_failed,
                size_cap_exceeded,
                unsupported_member,
            } => {
                // All-or-nothing: the manifest says at least one member never
                // got a content identity, so this entry is keyed on path text.
                // Publishing the readable subset would key the receiver's copy
                // on content digests — a diverged identity and a duplicate
                // entry on the next copy of the same set.
                warn!(
                    entry_id = %entry_id_str,
                    ingest_failed,
                    size_cap_exceeded,
                    unsupported_member,
                    "outbound: file-set manifest has excluded lines; skipping dispatch (all-or-nothing)"
                );
                return Ok(ClipboardOutboundOutcome::Skipped {
                    reason: "file_set_excluded".to_string(),
                });
            }
        };
        let extracted_paths_count = resolved_paths.len();

        let metadata_start = Instant::now();
        let mut file_candidates = Vec::with_capacity(resolved_paths.len());
        let mut total_file_metadata_bytes: u64 = 0;
        for path in resolved_paths {
            match tokio::fs::metadata(&path).await {
                Ok(meta) => {
                    total_file_metadata_bytes =
                        total_file_metadata_bytes.saturating_add(meta.len());
                    file_candidates.push(FileCandidate {
                        path,
                        size: meta.len(),
                    });
                }
                Err(err) if from_manifest => {
                    // A manifest member vanished between capture and dispatch.
                    // Same all-or-nothing rule as above: never sync a subset of
                    // a set whose identity covers all members.
                    warn!(
                        error = %err,
                        file = %path.display(),
                        entry_id = %entry_id_str,
                        "outbound: file-set member unreadable at dispatch; skipping dispatch (all-or-nothing)"
                    );
                    return Ok(ClipboardOutboundOutcome::Skipped {
                        reason: "file_set_member_unavailable".to_string(),
                    });
                }
                Err(err) => warn!(
                    error = %err,
                    file = %path.display(),
                    "排除无法读取元数据的剪贴板文件"
                ),
            }
        }
        let metadata_ms = metadata_start.elapsed().as_millis() as u64;

        let plan_start = Instant::now();
        let planner = OutboundSyncPlanner::new(Arc::clone(&self.settings));
        let plan = planner
            .plan(
                input.snapshot,
                input.origin,
                file_candidates,
                extracted_paths_count,
            )
            .await;
        let plan_ms = plan_start.elapsed().as_millis() as u64;

        let Some(mut clipboard_intent) = plan.clipboard else {
            info!(
                entry_id = %entry_id_str,
                metadata_ms,
                plan_ms,
                "outbound: dispatch_capture skipped (planner suppressed)"
            );
            return Ok(ClipboardOutboundOutcome::Skipped {
                reason: "planner_suppressed".to_string(),
            });
        };

        info!(
            entry_id = %entry_id_str,
            snapshot_rep_count,
            extracted_paths_count,
            file_paths_source = if from_manifest { "manifest" } else { "reps" },
            file_candidate_count = plan.files.len(),
            total_file_bytes = total_file_metadata_bytes,
            metadata_ms,
            plan_ms,
            "outbound: dispatch_capture entering publish phase"
        );

        let publish_files_start = Instant::now();
        let (mut blob_refs, file_content_digests) =
            publish_file_blob_refs(self.blob_transfer.as_ref(), &plan.files, &entry_id).await?;
        let publish_files_ms = publish_files_start.elapsed().as_millis() as u64;

        // Capture→dispatch drift observability: the manifest digests are the
        // identity this entry was persisted under; the publish digests are
        // what actually went on the wire. Only comparable when the planner
        // kept every member (a per-file `max_file_size` exclusion legitimately
        // shrinks the publish set).
        if from_manifest && plan.files.len() == extracted_paths_count {
            let mut wire_digests = file_content_digests.clone();
            wire_digests.sort_unstable();
            wire_digests.dedup();
            let mut capture_digests = expected_digests;
            capture_digests.dedup();
            if wire_digests != capture_digests {
                warn!(
                    entry_id = %entry_id_str,
                    "outbound: file content drifted between capture and dispatch; wire identity keyed on current bytes"
                );
            }
        }

        let publish_inline_start = Instant::now();
        let mut image_blob_refs = publish_oversized_inline_blob_refs(
            self.blob_transfer.as_ref(),
            &mut clipboard_intent.snapshot,
            &entry_id,
        )
        .await?;
        let publish_inline_ms = publish_inline_start.elapsed().as_millis() as u64;

        blob_refs.append(&mut image_blob_refs);
        let blob_ref_count = blob_refs.len();

        let file_set_manifest = match directory_members {
            Some(members) => {
                if plan.files.len() != extracted_paths_count {
                    return Ok(ClipboardOutboundOutcome::Skipped {
                        reason: "file_set_member_unavailable".to_string(),
                    });
                }
                Some(build_transfer_manifest(&members, &plan.files)?)
            }
            None => None,
        };
        if let Some(manifest) = &file_set_manifest {
            let digests = file_content_digests
                .iter()
                .enumerate()
                .map(|(index, digest)| (index as u32, *digest))
                .collect();
            clipboard_intent.snapshot.file_content_digests.clear();
            clipboard_intent.snapshot.file_set_v1_component = Some(
                compute_file_set_component(manifest, &digests)
                    .map_err(|err| ClipboardOutboundError::Internal(err.to_string()))?,
            );
        } else if !file_content_digests.is_empty() {
            clipboard_intent.snapshot.file_content_digests = file_content_digests;
        }

        let dispatch_phase_start = Instant::now();
        // LocalCapture 路径:把 entry_id 透传给 dispatch,fan-out 完成后落盘
        // 每个对端的投递结果(供视图层追踪"这条 entry 同步到了哪些设备")。
        let dispatch_result = if let Some(manifest) = file_set_manifest {
            self.clipboard_sync
                .dispatch_snapshot_with_blob_refs_and_file_set(
                    clipboard_intent.snapshot,
                    blob_refs,
                    manifest,
                    input.origin,
                    Some(entry_id.clone()),
                    None,
                )
                .await
        } else if blob_refs.is_empty() {
            self.clipboard_sync
                .dispatch_snapshot(
                    clipboard_intent.snapshot,
                    input.origin,
                    Some(entry_id.clone()),
                    // LocalCapture 路径走"全 fan-out"语义。resend 路径不经此
                    // 入口,而是直接走 ResendEntryUseCase + DispatchEntryRunner。
                    None,
                )
                .await
        } else {
            self.clipboard_sync
                .dispatch_snapshot_with_blob_refs(
                    clipboard_intent.snapshot,
                    blob_refs,
                    input.origin,
                    Some(entry_id.clone()),
                    None,
                )
                .await
        }
        .map_err(|err| ClipboardOutboundError::Internal(err.to_string()))?;
        let dispatch_ms = dispatch_phase_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %entry_id_str,
            blob_ref_count,
            publish_files_ms,
            publish_inline_ms,
            dispatch_ms,
            total_ms = dispatch_start.elapsed().as_millis() as u64,
            accepted = dispatch_result.total_accepted,
            offline = dispatch_result.total_offline,
            errored = dispatch_result.total_errored,
            pending = dispatch_result.total_pending,
            "outbound: dispatch_capture completed"
        );

        Ok(ClipboardOutboundOutcome::Dispatched {
            accepted: dispatch_result.total_accepted,
            duplicate: dispatch_result.total_duplicate,
            offline: dispatch_result.total_offline,
            errored: dispatch_result.total_errored,
            pending: dispatch_result.total_pending,
            blob_ref_count,
        })
    }
}

pub struct ClipboardOutboundFacade {
    dispatcher: Arc<dyn ClipboardOutboundPort>,
    resend_runner: Arc<dyn ResendEntryRunner>,
}

impl ClipboardOutboundFacade {
    /// Production constructor — bootstrap assembles
    /// [`ClipboardOutboundDeps`] once and the facade builds both the
    /// dispatcher (for `dispatch_capture`) and the resend use case (for
    /// `resend_entry`) internally. Keeps the use-case types
    /// `pub(crate)` per `uc-application/AGENTS.md` §11.4 — bootstrap
    /// never sees the concrete [`ResendEntryUseCase`] / dispatcher
    /// types.
    pub fn new(deps: ClipboardOutboundDeps) -> Self {
        let dispatcher: Arc<dyn ClipboardOutboundPort> =
            Arc::new(ClipboardOutboundDispatcher::from_deps(&deps));
        let dispatch_runner = deps.clipboard_sync.dispatch_runner();
        let blob_publisher: Arc<dyn OutboundBlobPublishGateway> = deps.blob_transfer.clone();
        let resend_uc = ResendEntryUseCase::new(ResendEntryDeps {
            entry_repo: deps.entry_repo,
            event_repo: deps.event_repo,
            selection_repo: deps.selection_repo,
            representation_repo: deps.representation_repo,
            rep_processing_repo: deps.rep_processing_repo,
            payload_resolver: deps.payload_resolver,
            blob_store: deps.blob_store,
            entry_delivery_repo: deps.entry_delivery_repo,
            trusted_peer_repo: deps.trusted_peer_repo,
            device_identity: deps.device_identity,
            settings: deps.settings,
            entry_file_set_repo: deps.entry_file_set_repo,
            blob_publisher,
            dispatch_runner,
        });
        Self {
            dispatcher,
            resend_runner: Arc::new(resend_uc) as Arc<dyn ResendEntryRunner>,
        }
    }

    /// Crate-internal constructor — lets tests inject custom dispatcher
    /// + resend stubs without standing up the full 12-port use case.
    /// Production must go through [`Self::new`] so bootstrap remains the
    /// single source of dep wiring.
    #[cfg(test)]
    pub(crate) fn from_parts(
        dispatcher: Arc<dyn ClipboardOutboundPort>,
        resend_runner: Arc<dyn ResendEntryRunner>,
    ) -> Self {
        Self {
            dispatcher,
            resend_runner,
        }
    }

    pub async fn dispatch_capture(
        &self,
        input: ClipboardOutboundInput,
    ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError> {
        self.dispatcher.dispatch_capture(input).await
    }

    /// 用户主动 resend 一条本机来源的 entry。详细语义见
    /// [`ResendEntryUseCase::execute`] — entry 不存在 → `EntryNotFound`;
    /// 远端来源 / 本机已不持有 plaintext → `EntryNotResendable { reason }`;
    /// 显式 filter 包含未信任设备 → `TargetNotTrusted`;空目标集 →
    /// `NoEligibleTargets`。
    pub async fn resend_entry(
        &self,
        cmd: ResendEntryCommand,
    ) -> Result<ResendReport, ResendEntryError> {
        self.resend_runner.execute(cmd).await
    }
}

pub(crate) fn extract_file_paths_from_snapshot(snapshot: &SystemClipboardSnapshot) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for rep in &snapshot.representations {
        let is_file_rep = rep
            .mime
            .as_ref()
            .map(|m| {
                let s = m.as_str();
                s.eq_ignore_ascii_case("text/uri-list") || s.eq_ignore_ascii_case("file/uri-list")
            })
            .unwrap_or(false)
            || rep.format_id.eq_ignore_ascii_case("files")
            || rep.format_id.eq_ignore_ascii_case("public.file-url");

        if !is_file_rep {
            continue;
        }

        // outbound 路径的 rep 来自 DB reconstruct,必然 Inline source;LocalFile 不可能
        // 出现。保守用 inline_bytes() 而非 expect_inline_bytes(),让契约违反时直接 skip
        // 而不是 panic 在出站路径。
        let Some(rep_bytes) = rep.inline_bytes() else {
            continue;
        };
        let text = match std::str::from_utf8(rep_bytes) {
            Ok(text) => text,
            Err(_) => continue,
        };

        for line in text.lines() {
            if let UriListLineKind::File(path) = parse_uri_list_line(line) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Where an outbound file-class send got its member path list from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutboundFileSetResolution {
    /// The snapshot carries no file-class rep — nothing file-related to
    /// publish for this entry.
    NotFileClass,
    /// Paths recovered from the persisted [`uc_core::clipboard::EntryFileSet`]
    /// manifest — the single source of truth written at capture time.
    Manifest {
        /// Member paths, sorted and deduplicated (publish each distinct file
        /// once even when the manifest lists it on multiple lines).
        paths: Vec<PathBuf>,
        /// The manifest's sorted content-digest contribution — the identity
        /// this entry was persisted under. Lets the dispatch path detect
        /// capture→dispatch content drift after publishing.
        expected_digests: Vec<[u8; 32]>,
    },
    /// No manifest persisted for this entry (pre-manifest entry, a failed
    /// best-effort save at capture, or an unreadable manifest row) — fall
    /// back to re-parsing the snapshot's reps.
    Fallback { paths: Vec<PathBuf> },
    DirectorySyncable {
        paths: Vec<PathBuf>,
        members: Vec<DirectoryMemberSource>,
    },
    /// The manifest exists but contains excluded lines: at least one member
    /// never got a content identity, so the entry's identity fell back to
    /// path text at capture. All-or-nothing — callers must not publish the
    /// readable subset (it would key the receiver's copy on content digests,
    /// diverging from the sender's identity).
    Excluded {
        ingest_failed: usize,
        size_cap_exceeded: usize,
        unsupported_member: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryMemberSource {
    pub location: FileSetMemberLocation,
    pub path: Option<PathBuf>,
    pub root_is_file: bool,
}

/// Resolve the member path list for an outbound file-class send, preferring
/// the persisted file-set manifest over re-parsing raw reps.
///
/// Availability over strictness on the read path: a missing manifest or a
/// storage read failure degrades to the legacy rep-parsing fallback rather
/// than blocking sync. Only an *existing* manifest with excluded lines blocks
/// the send (see [`OutboundFileSetResolution::Excluded`]).
pub(crate) async fn resolve_outbound_file_set(
    entry_file_set_repo: &dyn EntryFileSetRepositoryPort,
    entry_id: &EntryId,
    snapshot: &SystemClipboardSnapshot,
) -> OutboundFileSetResolution {
    let is_file_class = snapshot
        .representations
        .iter()
        .any(|rep| is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id));
    if !is_file_class {
        return OutboundFileSetResolution::NotFileClass;
    }

    let file_set = match entry_file_set_repo.load(entry_id).await {
        Ok(Some(file_set)) => file_set,
        Ok(None) => {
            // Expected for every pre-manifest (legacy) file entry, so keep it
            // at debug: during the migration window this fires on each dispatch
            // /resend of an old file entry and would otherwise flood info.
            debug!(
                entry_id = %entry_id.as_str(),
                "outbound: no file-set manifest for file-class entry; falling back to rep parsing"
            );
            return OutboundFileSetResolution::Fallback {
                paths: extract_file_paths_from_snapshot(snapshot),
            };
        }
        Err(err) => {
            warn!(
                entry_id = %entry_id.as_str(),
                error = %err,
                "outbound: file-set manifest load failed; falling back to rep parsing"
            );
            return OutboundFileSetResolution::Fallback {
                paths: extract_file_paths_from_snapshot(snapshot),
            };
        }
    };

    let mut ingest_failed = 0usize;
    let mut size_cap_exceeded = 0usize;
    let mut unsupported_member = 0usize;
    for line in &file_set.lines {
        if let EntryFileSetLineKind::Excluded { reason } = &line.kind {
            match reason {
                EntryFileSetExcludeReason::IngestFailed => ingest_failed += 1,
                EntryFileSetExcludeReason::SizeCapExceeded => size_cap_exceeded += 1,
                EntryFileSetExcludeReason::UnsupportedMember => unsupported_member += 1,
            }
        }
    }
    if ingest_failed + size_cap_exceeded + unsupported_member > 0 {
        return OutboundFileSetResolution::Excluded {
            ingest_failed,
            size_cap_exceeded,
            unsupported_member,
        };
    }

    if file_set.has_directory_structure() {
        let row_count = file_set.lines.len() as i64;
        let directory_roots: std::collections::HashSet<i64> = file_set
            .lines
            .iter()
            .filter(|line| line.indicates_directory_root(row_count))
            .filter_map(|line| {
                line.member_location
                    .as_ref()
                    .map(|location| location.root_index)
            })
            .collect();
        let mut paths = Vec::new();
        let mut members = Vec::new();
        for line in &file_set.lines {
            let Some(location) = line.member_location.clone() else {
                if matches!(line.kind, EntryFileSetLineKind::NonFile) {
                    continue;
                }
                return OutboundFileSetResolution::Fallback {
                    paths: extract_file_paths_from_snapshot(snapshot),
                };
            };
            let root_is_file = !directory_roots.contains(&location.root_index);
            let path = match (&line.kind, location.kind) {
                (
                    EntryFileSetLineKind::File { .. },
                    FileSetMemberKind::File | FileSetMemberKind::Executable,
                ) => match manifest_line_path(&line.original_text) {
                    Some(root_path) => {
                        let path = if root_is_file {
                            root_path
                        } else {
                            root_path.join(&location.relative_path)
                        };
                        paths.push(path.clone());
                        Some(path)
                    }
                    None => {
                        return OutboundFileSetResolution::Fallback {
                            paths: extract_file_paths_from_snapshot(snapshot),
                        };
                    }
                },
                (EntryFileSetLineKind::NonFile, FileSetMemberKind::EmptyDirectory) => None,
                _ => {
                    return OutboundFileSetResolution::Fallback {
                        paths: extract_file_paths_from_snapshot(snapshot),
                    };
                }
            };
            members.push(DirectoryMemberSource {
                location,
                path,
                root_is_file,
            });
        }
        paths.sort();
        paths.dedup();
        return OutboundFileSetResolution::DirectorySyncable { paths, members };
    }

    let mut paths = Vec::new();
    for line in file_set.file_lines() {
        match manifest_line_path(&line.original_text) {
            Some(path) => paths.push(path),
            None => {
                // A File line whose path can't be recovered would silently
                // shrink the published set — never publish a subset. Degrade
                // to the legacy fallback instead (no worse than pre-manifest
                // behavior).
                warn!(
                    entry_id = %entry_id.as_str(),
                    line_index = line.line_index,
                    "outbound: could not recover path from file-set manifest line; falling back to rep parsing"
                );
                return OutboundFileSetResolution::Fallback {
                    paths: extract_file_paths_from_snapshot(snapshot),
                };
            }
        }
    }
    paths.sort();
    paths.dedup();
    OutboundFileSetResolution::Manifest {
        expected_digests: file_set.content_digest_contribution(),
        paths,
    }
}

pub(crate) fn build_transfer_manifest(
    members: &[DirectoryMemberSource],
    files: &[FileSyncIntent],
) -> Result<InboundFileSetManifest, ClipboardOutboundError> {
    let indexes: HashMap<&std::path::Path, u32> = files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let index = u32::try_from(index).map_err(|_| {
                ClipboardOutboundError::Internal("file-set index cannot fit u32".to_string())
            })?;
            Ok((file.path.as_path(), index))
        })
        .collect::<Result<_, ClipboardOutboundError>>()?;
    let members = members
        .iter()
        .map(|member| {
            let blob_ref_index = match &member.path {
                Some(path) => Some(*indexes.get(path.as_path()).ok_or_else(|| {
                    ClipboardOutboundError::Internal(format!(
                        "directory member was not published: {}",
                        path.display()
                    ))
                })?),
                None => None,
            };
            Ok(InboundFileSetMember {
                root_index: u32::try_from(member.location.root_index).map_err(|_| {
                    ClipboardOutboundError::Internal("negative directory root index".to_string())
                })?,
                root_name: member.location.root_name.clone(),
                root_is_file: member.root_is_file,
                relative_path: member.location.relative_path.clone(),
                kind: member.location.kind,
                blob_ref_index,
            })
        })
        .collect::<Result<Vec<_>, ClipboardOutboundError>>()?;
    Ok(InboundFileSetManifest { members })
}

/// Recover the local path of a manifest `File` line. Capture writes
/// `original_text` in one of two shapes (see the capture side's
/// `build_entry_file_set`): a raw `text/uri-list` line (`file://...`), or a
/// bare filesystem path when the line came from a `LocalFile` rep.
fn manifest_line_path(original_text: &str) -> Option<PathBuf> {
    match parse_uri_list_line(original_text) {
        UriListLineKind::File(path) => Some(path),
        UriListLineKind::NonFile => {
            let bare = original_text.trim();
            (!bare.is_empty() && !bare.starts_with('#')).then(|| PathBuf::from(bare))
        }
    }
}

/// 出向 dispatch 时单条 inline rep 在 envelope 主体里的最大 bytes 数。超过
/// 该值的 image-类 rep 会被剥出来走 blob 通道（receiver 通过 `representation_index`
/// 把 fetched bytes 灌回原 rep），避免撞 wire 层 `MAX_PAYLOAD_SIZE = 2 MiB` 上限。
///
/// 阈值定在 64 KiB 是为了让 placeholder 真正派上用场（#785）。inline 路径下
/// 字节随 V3 envelope 一次性传完，receiver 端 V3 decode 完成那一刻就已经持
/// 有完整图片字节 —— `apply_inbound/usecase.rs:192` 先 emit 的
/// `IncomingPending` 与紧随其后的 `NewContent` 之间只有几 ms 间隔，前端来不
/// 及把"正在接收"占位卡片渲染出来。把阈值压到 64 KiB 让常见截图（百 KB ~
/// 几 MB PNG）走 blob_refs 路径，receiver 端的 materialize 阶段才有真实的
/// 时间窗口承载 placeholder。
///
/// 64 KiB 仍给 `inline_threshold_bytes = 16 KB`（uc-infra `clipboard_storage_config`）
/// 的纯文本 rep 留出 4× 缓冲：emoji / 小 icon 之类的 < 64 KB 图片继续 inline，
/// 不为它们多一次 iroh-blobs round-trip。
const OVERSIZED_REP_THRESHOLD_BYTES: usize = 64 * 1024;

/// 把 snapshot 中超过 `OVERSIZED_REP_THRESHOLD_BYTES` 的 image-类 rep 上传到
/// blob store，把它们的 `bytes` 字段就地清空（保留 `format_id` / `mime` / `id`），
/// 返回携带 `representation_index` 的 V3BlobRef 列表。
///
/// receiver 端的 `InboundBlobMaterializer` 看到 `representation_index = Some(i)`
/// 时把 fetched bytes 灌回 `representations[i]`，而不是当成独立 file 落到 cache。
///
/// **重要细节**：在清空 `bytes` 之前显式调用一次 `content_hash()`，强制把原内容
/// 哈希写入 OnceLock 缓存，这样 envelope 编码阶段的 `snapshot.snapshot_hash()`
/// 仍反映真实图片内容（receiver 端解码后会拿到一致的 snapshot_hash 用于 dedup）。
///
/// 仅对 `mime` 以 `image/` 开头的 rep 生效。其它类型的大 rep 暂保持 inline；
/// 后续若有非 image 大 rep 撞上限，会在此处扩展并补对应的 receiver 处理。
pub(crate) async fn publish_oversized_inline_blob_refs(
    blob_transfer: &dyn OutboundBlobPublishGateway,
    snapshot: &mut SystemClipboardSnapshot,
    entry_id: &EntryId,
) -> Result<Vec<V3BlobRef>, ClipboardOutboundError> {
    let mut blob_refs = Vec::new();

    for (idx, rep) in snapshot.representations.iter_mut().enumerate() {
        // outbound 路径的 rep 必然 Inline source;LocalFile 在 capture 阶段已物化到 blob,
        // 不会进入 outbound dispatch。
        let Some(rep_bytes) = rep.inline_bytes() else {
            continue;
        };
        if rep_bytes.len() <= OVERSIZED_REP_THRESHOLD_BYTES {
            continue;
        }
        let mime_str = rep.mime.as_ref().map(|m| m.as_str().to_string());
        let is_image = mime_str
            .as_deref()
            .map(|m| m.to_ascii_lowercase().starts_with("image/"))
            .unwrap_or(false);
        if !is_image {
            continue;
        }

        // Force-cache content hash with the ORIGINAL bytes before we drain
        // them — `snapshot_hash()` is invoked downstream during V3 encode
        // and must reflect the real image content for cross-device dedup
        // to match.
        let _ = rep.content_hash();

        let size_bytes = rep_bytes.len() as u64;
        let plaintext = rep
            .take_inline_bytes()
            .map_err(|err| ClipboardOutboundError::Internal(err.to_string()))?;

        let publish_start = Instant::now();
        let result = blob_transfer
            .publish_blob(PublishBlobCommand {
                plaintext: Bytes::from(plaintext),
                entry_id: Some(entry_id.clone()),
            })
            .await
            .map_err(|err| ClipboardOutboundError::Internal(err.to_string()))?;
        let publish_ms = publish_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %entry_id.as_str(),
            representation_index = idx,
            size_bytes,
            mime = mime_str.as_deref().unwrap_or("?"),
            reused_existing = result.reused_existing,
            publish_ms,
            "outbound: oversized inline rep published as blob"
        );

        let representation_index = u32::try_from(idx).map_err(|_| {
            ClipboardOutboundError::Internal(format!("representation index {idx} cannot fit u32"))
        })?;

        blob_refs.push(V3BlobRef {
            ticket: result.ticket,
            entry_id: result.entry_id,
            filename: None,
            mime: mime_str,
            size_bytes,
            representation_index: Some(representation_index),
        });
    }

    Ok(blob_refs)
}

/// Returns `(blob_refs, file_content_digests)`.  The digests are the
/// blake3 hashes of the actual file bytes (= `PlaintextHash` from the
/// publish result).  The caller stores them on the snapshot so that
/// `snapshot_hash()` uses file content instead of device-local paths.
pub(crate) async fn publish_file_blob_refs(
    blob_transfer: &dyn OutboundBlobPublishGateway,
    files: &[FileSyncIntent],
    entry_id: &EntryId,
) -> Result<(Vec<V3BlobRef>, Vec<[u8; 32]>), ClipboardOutboundError> {
    let mut blob_refs = Vec::with_capacity(files.len());
    let mut file_content_digests = Vec::with_capacity(files.len());

    for file in files {
        let publish_start = Instant::now();
        let result = blob_transfer
            .publish_blob_path(PublishBlobPathCommand {
                path: file.path.clone(),
                entry_id: Some(entry_id.clone()),
            })
            .await
            .map_err(|err| ClipboardOutboundError::Internal(err.to_string()))?;
        let publish_ms = publish_start.elapsed().as_millis() as u64;

        info!(
            entry_id = %entry_id.as_str(),
            file = %file.path.display(),
            size_bytes = file.size,
            reused_existing = result.reused_existing,
            publish_ms,
            "outbound: file blob published (streaming)"
        );

        file_content_digests.push(*result.plaintext_hash.as_bytes());

        blob_refs.push(V3BlobRef {
            ticket: result.ticket,
            entry_id: result.entry_id,
            filename: Some(file.filename.clone()).filter(|name| !name.is_empty()),
            mime: None,
            size_bytes: file.size,
            representation_index: None,
        });
    }

    Ok((blob_refs, file_content_digests))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use uc_core::clipboard::{
        ContentHash, EntryFileSet, EntryFileSetError, EntryFileSetLine, FileSetMemberKind,
        FileSetMemberLocation, HashAlgorithm, MimeType, ObservedClipboardRepresentation,
    };
    use uc_core::ids::{EntryId, FormatId, RepresentationId};

    struct FakeOutbound;

    #[async_trait]
    impl ClipboardOutboundPort for FakeOutbound {
        async fn dispatch_capture(
            &self,
            input: ClipboardOutboundInput,
        ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError> {
            assert_eq!(input.entry_id, "entry-a");
            Ok(ClipboardOutboundOutcome::Dispatched {
                accepted: 1,
                duplicate: 0,
                offline: 0,
                errored: 0,
                pending: 0,
                blob_ref_count: 0,
            })
        }
    }

    /// Stub runner — every `dispatch_capture` test gets one of these so
    /// the facade can be constructed without standing up the full 12-port
    /// `ResendEntryUseCase`. If a test that exercises only the dispatch
    /// path accidentally calls `resend_entry`, the panic message points
    /// at the wiring mistake.
    struct UnusedResendRunner;

    #[async_trait]
    impl ResendEntryRunner for UnusedResendRunner {
        async fn execute(
            &self,
            _cmd: ResendEntryCommand,
        ) -> Result<ResendReport, ResendEntryError> {
            panic!(
                "UnusedResendRunner.execute should never be called from a dispatch_capture test"
            );
        }
    }

    /// Records the last `ResendEntryCommand` and returns a canned report.
    /// Used by [`resend_entry_forwards_command_to_runner`] to prove the
    /// facade thin-method threads command + result without mutation.
    struct RecordingResendRunner {
        last_cmd: Mutex<Option<ResendEntryCommand>>,
        canned: ResendReport,
    }

    #[async_trait]
    impl ResendEntryRunner for RecordingResendRunner {
        async fn execute(&self, cmd: ResendEntryCommand) -> Result<ResendReport, ResendEntryError> {
            *self.last_cmd.lock().unwrap() = Some(cmd);
            Ok(self.canned.clone())
        }
    }

    // ── resolve_outbound_file_set ───────────────────────────────────────

    /// Fake manifest repo: canned `load` result; `save` must never be called
    /// on the outbound path.
    enum FakeFileSetRepo {
        Found(EntryFileSet),
        Missing,
        Broken,
        /// The resolver must short-circuit before touching the repo.
        MustNotLoad,
    }
    #[async_trait]
    impl EntryFileSetRepositoryPort for FakeFileSetRepo {
        async fn save(
            &self,
            _entry_id: &EntryId,
            _file_set: &EntryFileSet,
        ) -> Result<(), EntryFileSetError> {
            unreachable!("outbound must never save a file-set manifest")
        }
        async fn load(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<EntryFileSet>, EntryFileSetError> {
            match self {
                Self::Found(fs) => Ok(Some(fs.clone())),
                Self::Missing => Ok(None),
                Self::Broken => Err(EntryFileSetError::Storage("synthetic".into())),
                Self::MustNotLoad => {
                    unreachable!("resolver must not load a manifest for a non-file snapshot")
                }
            }
        }
    }

    fn uri_list_snapshot(text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-files"),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                text.as_bytes().to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn text_snapshot() -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-text"),
                FormatId::from("public.utf8-plain-text"),
                Some(MimeType("text/plain".to_string())),
                b"hello".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        }
    }

    fn file_line(index: i64, original_text: &str, hash_byte: u8) -> EntryFileSetLine {
        EntryFileSetLine {
            line_index: index,
            original_text: original_text.to_string(),
            member_location: None,
            kind: EntryFileSetLineKind::File {
                content_hash: ContentHash {
                    alg: HashAlgorithm::Blake3V1,
                    bytes: [hash_byte; 32],
                },
                blob_id: None,
                size_bytes: None,
            },
        }
    }

    fn excluded_line(index: i64, reason: EntryFileSetExcludeReason) -> EntryFileSetLine {
        EntryFileSetLine {
            line_index: index,
            original_text: "file:///excluded".to_string(),
            member_location: None,
            kind: EntryFileSetLineKind::Excluded { reason },
        }
    }

    #[tokio::test]
    async fn resolver_exposes_directory_members_for_wire_publication() {
        let mut line = file_line(1, "file:///tmp/root", 1);
        line.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "root".to_string(),
            relative_path: "child.txt".to_string(),
            kind: FileSetMemberKind::File,
        });
        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::Found(EntryFileSet { lines: vec![line] }),
            &EntryId::from("e1"),
            &uri_list_snapshot("file:///tmp/root\n"),
        )
        .await;

        match resolution {
            OutboundFileSetResolution::DirectorySyncable { paths, members } => {
                assert_eq!(paths, vec![PathBuf::from("/tmp/root/child.txt")]);
                assert_eq!(members.len(), 1);
                assert_eq!(members[0].location.root_name, "root");
                assert_eq!(members[0].path, Some(PathBuf::from("/tmp/root/child.txt")));
                assert!(!members[0].root_is_file);
            }
            other => panic!("expected directory syncable resolution, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_distinguishes_file_roots_in_mixed_selection() {
        let mut loose_file = file_line(0, "file:///tmp/loose.txt", 1);
        loose_file.member_location = Some(FileSetMemberLocation {
            root_index: 0,
            root_name: "loose.txt".to_string(),
            relative_path: "loose.txt".to_string(),
            kind: FileSetMemberKind::File,
        });
        let mut directory_member = file_line(2, "file:///tmp/folder", 2);
        directory_member.member_location = Some(FileSetMemberLocation {
            root_index: 1,
            root_name: "folder".to_string(),
            relative_path: "child.txt".to_string(),
            kind: FileSetMemberKind::File,
        });

        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::Found(EntryFileSet {
                lines: vec![loose_file, directory_member],
            }),
            &EntryId::from("e1"),
            &uri_list_snapshot("file:///tmp/loose.txt\nfile:///tmp/folder\n"),
        )
        .await;

        let OutboundFileSetResolution::DirectorySyncable { paths, members } = resolution else {
            panic!("expected directory syncable resolution");
        };
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/folder/child.txt"),
                PathBuf::from("/tmp/loose.txt"),
            ]
        );
        assert_eq!(members[0].path, Some(PathBuf::from("/tmp/loose.txt")));
        assert!(members[0].root_is_file);
        assert_eq!(
            members[1].path,
            Some(PathBuf::from("/tmp/folder/child.txt"))
        );
        assert!(!members[1].root_is_file);
    }

    #[tokio::test]
    async fn resolver_short_circuits_for_non_file_snapshot() {
        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::MustNotLoad,
            &EntryId::from("e1"),
            &text_snapshot(),
        )
        .await;
        assert_eq!(resolution, OutboundFileSetResolution::NotFileClass);
    }

    /// Manifest is the single source of truth: paths come from its lines
    /// (both `original_text` shapes), sorted + deduplicated — not from the
    /// snapshot rep, which here deliberately lists a different file.
    #[tokio::test]
    async fn resolver_prefers_manifest_over_snapshot_reps() {
        let manifest = EntryFileSet {
            lines: vec![
                // uri-list shape, listed twice (dedup expected).
                file_line(0, "file:///tmp/b.txt", 2),
                file_line(1, "file:///tmp/b.txt", 2),
                // bare-path shape (LocalFile rep origin).
                file_line(2, "/tmp/a.txt", 1),
                EntryFileSetLine {
                    line_index: 3,
                    original_text: "# comment".to_string(),
                    member_location: None,
                    kind: EntryFileSetLineKind::NonFile,
                },
            ],
        };
        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::Found(manifest),
            &EntryId::from("e1"),
            &uri_list_snapshot("file:///tmp/other.txt\n"),
        )
        .await;
        match resolution {
            OutboundFileSetResolution::Manifest {
                paths,
                expected_digests,
            } => {
                assert_eq!(
                    paths,
                    vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
                );
                // Identity contribution keeps duplicate lines (sorted).
                assert_eq!(expected_digests, vec![[1u8; 32], [2u8; 32], [2u8; 32]]);
            }
            other => panic!("expected Manifest resolution, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_falls_back_when_manifest_missing_or_broken() {
        for repo in [FakeFileSetRepo::Missing, FakeFileSetRepo::Broken] {
            let resolution = resolve_outbound_file_set(
                &repo,
                &EntryId::from("e1"),
                &uri_list_snapshot("file:///tmp/legacy.txt\n"),
            )
            .await;
            assert_eq!(
                resolution,
                OutboundFileSetResolution::Fallback {
                    paths: vec![PathBuf::from("/tmp/legacy.txt")]
                }
            );
        }
    }

    #[tokio::test]
    async fn resolver_blocks_on_any_excluded_line() {
        let manifest = EntryFileSet {
            lines: vec![
                file_line(0, "file:///tmp/ok.txt", 1),
                excluded_line(1, EntryFileSetExcludeReason::IngestFailed),
                excluded_line(2, EntryFileSetExcludeReason::SizeCapExceeded),
            ],
        };
        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::Found(manifest),
            &EntryId::from("e1"),
            &uri_list_snapshot("file:///tmp/ok.txt\n"),
        )
        .await;
        assert_eq!(
            resolution,
            OutboundFileSetResolution::Excluded {
                ingest_failed: 1,
                size_cap_exceeded: 1,
                unsupported_member: 0,
            }
        );
    }

    /// A `File` line whose path can't be recovered must degrade the WHOLE
    /// set to the rep-parsing fallback — never publish a silently shrunken
    /// subset of a manifest.
    #[tokio::test]
    async fn resolver_falls_back_when_a_file_line_path_is_unrecoverable() {
        let manifest = EntryFileSet {
            lines: vec![
                file_line(0, "file:///tmp/ok.txt", 1),
                file_line(1, "   ", 2),
            ],
        };
        let resolution = resolve_outbound_file_set(
            &FakeFileSetRepo::Found(manifest),
            &EntryId::from("e1"),
            &uri_list_snapshot("file:///tmp/from-rep.txt\n"),
        )
        .await;
        assert_eq!(
            resolution,
            OutboundFileSetResolution::Fallback {
                paths: vec![PathBuf::from("/tmp/from-rep.txt")]
            }
        );
    }

    #[tokio::test]
    async fn dispatch_capture_accepts_application_entry_id() {
        let facade = ClipboardOutboundFacade::from_parts(
            Arc::new(FakeOutbound),
            Arc::new(UnusedResendRunner),
        );
        let outcome = facade
            .dispatch_capture(ClipboardOutboundInput {
                entry_id: "entry-a".to_string(),
                snapshot: SystemClipboardSnapshot {
                    representations: Vec::new(),
                    ts_ms: 0,

                    file_content_digests: Vec::new(),

                    file_set_v1_component: None,
                },
                origin: ClipboardChangeOrigin::LocalCapture,
            })
            .await
            .unwrap();

        assert_eq!(
            outcome,
            ClipboardOutboundOutcome::Dispatched {
                accepted: 1,
                duplicate: 0,
                offline: 0,
                errored: 0,
                pending: 0,
                blob_ref_count: 0,
            }
        );
    }

    /// Facade thin-method contract: `resend_entry` forwards the exact
    /// command (entry_id + target_filter) to the runner and returns its
    /// report verbatim. mockall-free; the recording runner asserts both
    /// directions.
    #[tokio::test]
    async fn resend_entry_forwards_command_to_runner() {
        let canned = ResendReport {
            accepted: 2,
            duplicate: 0,
            offline: 1,
            errored: 0,
            pending: 0,
        };
        let runner = Arc::new(RecordingResendRunner {
            last_cmd: Mutex::new(None),
            canned: canned.clone(),
        });
        let facade = ClipboardOutboundFacade::from_parts(
            Arc::new(FakeOutbound),
            Arc::clone(&runner) as Arc<dyn ResendEntryRunner>,
        );

        let cmd = ResendEntryCommand {
            entry_id: EntryId::from("entry-xyz"),
            target_filter: Some(vec![
                uc_core::ids::DeviceId::new("peer-a"),
                uc_core::ids::DeviceId::new("peer-b"),
            ]),
        };
        let report = facade.resend_entry(cmd.clone()).await.expect("resend ok");
        assert_eq!(report, canned);

        let captured = runner
            .last_cmd
            .lock()
            .unwrap()
            .clone()
            .expect("runner saw a cmd");
        assert_eq!(captured.entry_id.as_str(), "entry-xyz");
        assert_eq!(
            captured
                .target_filter
                .as_ref()
                .map(|v| v.iter().map(|d| d.as_str().to_string()).collect::<Vec<_>>()),
            Some(vec!["peer-a".to_string(), "peer-b".to_string()])
        );
    }
}
