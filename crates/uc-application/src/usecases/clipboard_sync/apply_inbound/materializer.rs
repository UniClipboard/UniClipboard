//! 入站 blob 本地化抽象 + 默认实现。
//!
//! `InboundBlobMaterializer` 把 V3 envelope 解码出来的 `V3BlobRef` 列表落地:
//! - representation-bound blob 写回 `snapshot.representations[i].bytes`(图片 /
//!   大二进制走这条);
//! - free-standing 文件写到 `cache_dir/iroh-blobs/<entry_id>/<filename>`,
//!   再把 file-list rep 改写成本机 `file://` URI。
//!
//! `InboundBlobFetcher` 是 facade 适配层,生产环境就是 `BlobTransferFacade`,
//! 测试用 mockall 替身。

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{debug, info, warn};
use url::Url;

use uc_app_paths::DIRECTORY_RECEIVE_STAGING_PREFIX;
use uc_core::clipboard::{
    ContentHash, EntryFileSet, EntryFileSetLine, EntryFileSetLineKind, FileSetMemberKind,
    FileSetMemberLocation, HashAlgorithm,
};
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::atomic_publish::{AtomicPublishPort, PublishError};
use uc_core::ports::hidden_path::MarkHiddenPort;
use uc_core::ports::inbound_file_target::{
    ReserveInboundFileTargetPort, ResolveInboundSaveDirPort,
};
use uc_core::ports::{
    AttemptState, ClaimReceiveCommitPort, ClockPort, GetEntryAttemptPort, PublishPhase,
    ReceiveArtifact, ReceiveArtifactOwnership, ReceiveArtifactPhase, ReceiveArtifactRecord,
    ReceiveArtifactResolution, RecordDirectoryPublishPort, RecordReceiveArtifactsPort,
};
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

use crate::facade::blob_transfer::{
    BatchPosition, BlobTransferError, BlobTransferFacade, FetchBlobCommand, FetchBlobResult,
    FetchBlobToPathCommand, FetchBlobToPathResult, FetchTransferContext,
};
use crate::usecases::clipboard_sync::payload_codec::V3BlobRef;

/// 判断 fetcher 返回的 anyhow 错误是否来自 `BlobTransferError::Cancelled`。
/// 仅 `BlobTransferFacade` 这一条生产路径保留 thiserror chain;mock 路径
/// 若想模拟 cancel,可以 `Err(anyhow::Error::from(BlobTransferError::Cancelled))`。
pub fn is_cancel_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<BlobTransferError>()
            .map(|bte| matches!(bte, BlobTransferError::Cancelled))
            .unwrap_or(false)
    })
}

pub(crate) fn is_directory_cancel_error(error: &anyhow::Error) -> bool {
    is_cancel_error(error)
        || error.chain().any(|cause| {
            cause
                .downcast_ref::<DirectoryAttemptGateError>()
                .is_some_and(|gate| matches!(gate, DirectoryAttemptGateError::Cancelled))
        })
}

/// 描述一份未完成 materialize 的 file blob。`reason` 由 file_transfer
/// projection 单独承载(P1-10 落地),此处只表达"这个文件在 partial entry
/// 里是缺失的"+ 元数据,前端按 filename + size 渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFileRef {
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFileSetManifest {
    pub members: Vec<InboundFileSetMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFileSetMember {
    pub root_index: u32,
    pub root_name: String,
    pub root_is_file: bool,
    pub relative_path: String,
    pub kind: FileSetMemberKind,
    pub blob_ref_index: Option<u32>,
}

pub struct ReceiveWorkPlan {
    from_device: DeviceId,
    receiver_entry_id: EntryId,
    snapshot: SystemClipboardSnapshot,
    blob_refs: Vec<V3BlobRef>,
    file_set_manifest: Option<InboundFileSetManifest>,
    attempt_id: Option<String>,
    expected_snapshot_hash: Option<String>,
    original_blob_indices: Vec<usize>,
}

impl ReceiveWorkPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from_device: DeviceId,
        receiver_entry_id: EntryId,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        file_set_manifest: Option<InboundFileSetManifest>,
        attempt_id: Option<String>,
        expected_snapshot_hash: Option<String>,
    ) -> Self {
        let original_blob_indices = (0..blob_refs.len()).collect();
        Self {
            from_device,
            receiver_entry_id,
            snapshot,
            blob_refs,
            file_set_manifest,
            attempt_id,
            expected_snapshot_hash,
            original_blob_indices,
        }
    }

    fn into_parts(
        self,
    ) -> (
        DeviceId,
        EntryId,
        SystemClipboardSnapshot,
        Vec<V3BlobRef>,
        Option<InboundFileSetManifest>,
        Option<String>,
        Option<String>,
    ) {
        debug_assert_eq!(self.original_blob_indices.len(), self.blob_refs.len());
        (
            self.from_device,
            self.receiver_entry_id,
            self.snapshot,
            self.blob_refs,
            self.file_set_manifest,
            self.attempt_id,
            self.expected_snapshot_hash,
        )
    }
}

/// `InboundBlobMaterializer::materialize` 的返回值。
///
/// `is_partial()` 是"该 snapshot 不完整"的权威信号 —— 调用方据此跳过
/// OS clipboard write 与 dedup 登记。`missing` 单独列出 file-rep 阶段
/// 未完成文件的元数据,供前端渲染占位卡片用。
///
/// **关键不变量**:`is_partial()` 不等于 `!missing.is_empty()`。rep-bound
/// blob(图片 / 大二进制)被 cancel 时,materializer 会从 `snapshot.
/// representations` 中删除未完成的 rep —— 这本身已经让 snapshot 半残,
/// 但 `missing` 只用于 *file* 列表,如果 envelope 没有 file_refs,
/// `missing` 仍是空。仅靠 `!missing.is_empty()` 判定会把这种情况误判
/// 为 complete,把半残 snapshot 当真相落入 dedup 表 + OS 剪贴板。
#[derive(Debug)]
pub struct MaterializeResult {
    pub snapshot: SystemClipboardSnapshot,
    pub missing: Vec<MissingFileRef>,
    /// 真正的"是否 partial"标志。`complete()` 置 false,`finalize_partial`
    /// 置 true。`pub(crate)` 让 crate 内的 mock 测试能构造,外部调用方
    /// 统一通过 `is_partial()` 读,避免新调用方又掉进"missing 空 ⇒
    /// complete"陷阱。
    pub(crate) partial: bool,
    pub(crate) outcome: MaterializeOutcome,
    /// Present when this result put directory roots at their final location.
    ///
    /// Those roots are visible but provisional: the caller owes them either a
    /// [`DirectoryPublication::commit`] once the entry is durably recorded, or
    /// a [`DirectoryPublication::rollback`] on any path that abandons the
    /// entry. Take it with [`Self::take_publication`] and settle it on every
    /// branch — dropping it leaves user-visible roots with nothing behind them.
    pub(crate) publication: Option<DirectoryPublication>,
    /// Receiver-local manifest for a fully materialized directory. Paths refer
    /// only to this device's published roots and are sealed by the commit port.
    pub(crate) directory_file_set: Option<EntryFileSet>,
    pub(crate) has_receive_artifacts: bool,
    pub(crate) receive_artifacts: Vec<ReceiveArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializeOutcome {
    Complete,
    PartialCancelled,
    PartialFailed,
}

impl MaterializeResult {
    pub fn complete(snapshot: SystemClipboardSnapshot) -> Self {
        Self {
            snapshot,
            missing: Vec::new(),
            partial: false,
            outcome: MaterializeOutcome::Complete,
            publication: None,
            directory_file_set: None,
            has_receive_artifacts: false,
            receive_artifacts: Vec::new(),
        }
    }

    pub fn is_partial(&self) -> bool {
        self.partial
    }

    pub fn outcome(&self) -> MaterializeOutcome {
        self.outcome
    }

    /// Take ownership of the publication this result is holding, if any.
    pub fn take_publication(&mut self) -> Option<DirectoryPublication> {
        self.publication.take()
    }

    pub(crate) fn take_receive_artifacts(&mut self) -> Vec<ReceiveArtifact> {
        std::mem::take(&mut self.receive_artifacts)
    }
}

#[async_trait]
pub trait InboundBlobMaterializer: Send + Sync {
    async fn materialize_plan(&self, plan: ReceiveWorkPlan) -> Result<MaterializeResult> {
        let (
            from_device,
            receiver_entry_id,
            snapshot,
            blob_refs,
            file_set_manifest,
            attempt_id,
            expected_snapshot_hash,
        ) = plan.into_parts();
        self.materialize(
            from_device,
            receiver_entry_id,
            snapshot,
            blob_refs,
            file_set_manifest,
            attempt_id,
            expected_snapshot_hash,
        )
        .await
    }

    /// `receiver_entry_id` 是 ApplyInbound 在流程入口生成的接收端 entry_id,
    /// 用作所有 blob 拉取的 transfer_id —— 让占位卡片、进度事件和最终
    /// `NewContent` 共享同一个标识,前端无需做合并映射。
    ///
    /// 返回 [`MaterializeResult`]:`missing.is_empty()` 表示全量成功;否则
    /// 是 partial(用户在中途 cancel 了 inbound transfer),`snapshot` 仅包
    /// 含已成功落地的 representation,缺失的 file blob 用 `uniclip-missing://`
    /// URI 表达,`missing` 列出元数据供前端展示与调用方判定。
    async fn materialize(
        &self,
        from_device: DeviceId,
        receiver_entry_id: EntryId,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        file_set_manifest: Option<InboundFileSetManifest>,
        attempt_id: Option<String>,
        expected_snapshot_hash: Option<String>,
    ) -> Result<MaterializeResult>;
}

#[async_trait]
pub trait InboundBlobFetcher: Send + Sync {
    /// Seed durable metadata for a transfer before its fetch becomes active.
    async fn seed_pending_transfer(&self, _context: FetchTransferContext) -> Result<()> {
        Ok(())
    }

    /// In-memory fetch path — used by representation-bound blobs (e.g.
    /// oversized images that we splice back into `snapshot.representations`).
    async fn fetch_blob(&self, command: FetchBlobCommand) -> Result<FetchBlobResult>;

    /// Streaming fetch path — used by free-standing files. The blob is
    /// written directly to `command.target_path` (reflink on CoW
    /// filesystems) so receiving a 1 GiB clipboard transfer no longer
    /// routes the full plaintext through `Bytes`. GH#487 Phase 2.
    async fn fetch_blob_to_path(
        &self,
        command: FetchBlobToPathCommand,
    ) -> Result<FetchBlobToPathResult>;

    async fn record_unfetched_failure(&self, _context: FetchTransferContext, _detail: String) {}
}

/// Prefix marking an area where an inbound directory is assembled.
///
/// The area sits on the destination volume, so publication is a same-volume
/// rename. The prefix does two jobs: it makes crash debris recognizable —
/// nothing else creates these, and a live receive only holds one while it runs
/// — and on platforms where a leading dot means "hidden", it is also what
/// keeps the area out of the user's view. Windows needs an explicit attribute
/// for that, which is [`MarkHiddenPort`]'s job.

/// How many times a single root may lose the race for a name before the
/// receive gives up. Each loss means another process took the exact name in
/// the window between choosing it and publishing, so reaching this bound means
/// something is generating names as fast as we are.
const MAX_PUBLISH_ATTEMPTS: u32 = 64;

/// Stand-in for a root name that sanitizes away to nothing.
const FALLBACK_ROOT_NAME: &str = "received-folder";

/// Upper bound on collision-suffix attempts before giving up.
///
/// Matches the bound free-standing inbound files get. Without one, a folder
/// already holding a long run of `name (n)` entries turns every publication
/// into a walk over all of them.
const MAX_COLLISION_ATTEMPTS: u32 = 10_000;

/// Reduce a sender-supplied root name to a usable directory name.
///
/// `validate_manifest_member` has already rejected separators and the `.`/`..`
/// entries, so this handles what is left: characters the local filesystem
/// cannot store, and names that sanitize down to nothing (`"..."` trims to an
/// empty string, which would resolve to the parent directory itself).
///
/// Two known gaps, both shared with the sanitizer free-standing files go
/// through and neither introduced here:
///
/// - Windows rejects more than this replaces (`<>"|?*`, trailing spaces, and
///   the reserved device names `CON`, `NUL`, `COM1`, …). A folder named `a?b`
///   sent from macOS fails the receive on Windows rather than landing under a
///   substituted name.
/// - The rules are duplicated: this and the free-standing file path each carry
///   their own copy, in different crates.
///
/// The two are one problem. Closing the first in one copy would make the two
/// disagree about what a legal name is, and unifying them is not a local edit:
/// the shared home would have to be a crate both layers may depend on, and
/// "what Windows forbids" is platform knowledge that `uc-core` is expressly not
/// allowed to hold. Fix them together, deliberately, or not at all.
fn sanitize_root_name(root_name: &str) -> String {
    let sanitized = sanitize_path_segment(root_name);
    if sanitized.is_empty() {
        FALLBACK_ROOT_NAME.to_string()
    } else {
        sanitized
    }
}

fn staging_dir_name(receiver_entry_id: &EntryId) -> String {
    // The entry id is ours, not the sender's content, so it is safe to spell
    // out on disk.
    format!(
        "{DIRECTORY_RECEIVE_STAGING_PREFIX}{}",
        sanitize_path_segment(receiver_entry_id.as_ref())
    )
}

/// Remove assembly areas that a crash left behind.
///
/// Returns how many were removed. Anything matching the prefix in one of
/// `dirs` is debris: an interrupted receive never gets to publish, and its
/// contents are unreachable from any entry.
///
/// Only the directories passed in are searched. An area left in a folder the
/// user has since stopped using as their save directory is not reachable from
/// here and will not be found.
///
/// The caller owes one precondition: no receive may be running anywhere that
/// can reach `dirs`. An area is only distinguishable as debris by the fact
/// that nothing is currently building in it, and the prefix alone cannot say
/// so. Two processes sharing a save directory — which today means two profiles
/// configured to the same folder — break that: sweeping at one's startup takes
/// the other's in-flight area. The cost is bounded (that receive fails and can
/// be re-sent; nothing already published is touched), which is why the prefix
/// carries no owner. Give the areas an owner tag before relaxing this.
pub async fn sweep_inbound_staging(dirs: &[PathBuf]) -> usize {
    let mut swept = 0;
    for dir in dirs {
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(entries) => entries,
            // A save dir that is gone or unreadable is not an error worth
            // failing startup over; there is simply nothing to sweep.
            Err(_) => continue,
        };
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let matches = entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(DIRECTORY_RECEIVE_STAGING_PREFIX));
                    if !matches {
                        continue;
                    }
                    match tokio::fs::remove_dir_all(entry.path()).await {
                        Ok(()) => swept += 1,
                        Err(err) => {
                            warn!(error = %err, "failed to sweep an inbound staging area")
                        }
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    warn!(error = %err, "failed to enumerate a directory while sweeping");
                    break;
                }
            }
        }
    }
    if swept > 0 {
        info!(count = swept, "swept inbound staging areas left by a crash");
    }
    swept
}

/// Directory roots that are visible at their final location but not yet
/// permanent.
///
/// Publication is not the commit point. A receive is only "all there" once its
/// entry is durably recorded, and verification or persistence can still fail
/// after the roots have landed — so the ability to take them back must outlive
/// publication itself. That is what this handle is: it stays alive until the
/// receive either commits or gives up.
pub struct DirectoryPublication {
    publisher: Arc<dyn AtomicPublishPort>,
    staging: PathBuf,
    mode: PublishMode,
    /// `(final path, the staging path it was published from)`, in publication
    /// order. The staging name is free again once published, so withdrawing is
    /// the same move run backwards.
    published: Vec<(PathBuf, PathBuf)>,
    settled: bool,
}

/// Which guarantee a destination needs when roots land on it.
///
/// The two destinations differ in what lives around them, and that — not the
/// volume — is what decides the guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishMode {
    /// The destination holds the user's own files. A name we did not put there
    /// may appear at any moment, so the move itself has to refuse an occupied
    /// name; checking first and moving second would leave a window in which we
    /// destroy someone's folder.
    NoReplace,
    /// The destination is ours alone and scoped to one receive, so the only
    /// name that can appear is one we chose. Nothing is at risk, and demanding
    /// a guarantee the volume may not offer would cost the receive for nothing.
    IntoFreeName,
}

/// What a rollback managed to achieve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackOutcome {
    /// Every root was withdrawn; the final location holds nothing from this
    /// receive.
    Clean,
    /// Some roots could not be withdrawn and are still visible to the user.
    ///
    /// Carries only a count. The roots' names are the sender's content and
    /// must not travel into plaintext logs or columns.
    PartialPublication { visible_roots: usize },
}

impl std::fmt::Debug for DirectoryPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Names and paths withheld deliberately — see `RollbackOutcome`.
        f.debug_struct("DirectoryPublication")
            .field("root_count", &self.published.len())
            .field("settled", &self.settled)
            .finish()
    }
}

/// Run one move under `mode`'s guarantee.
async fn publish_via(
    publisher: &dyn AtomicPublishPort,
    mode: PublishMode,
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), PublishError> {
    match mode {
        PublishMode::NoReplace => publisher.publish_no_replace(source, destination).await,
        PublishMode::IntoFreeName => publisher.publish_into_free_name(source, destination).await,
    }
}

impl DirectoryPublication {
    fn new(publisher: Arc<dyn AtomicPublishPort>, staging: PathBuf, mode: PublishMode) -> Self {
        Self {
            publisher,
            staging,
            mode,
            published: Vec::new(),
            settled: false,
        }
    }

    fn record(&mut self, final_path: PathBuf, staged_from: PathBuf) {
        self.published.push((final_path, staged_from));
    }

    pub fn root_count(&self) -> usize {
        self.published.len()
    }

    /// Make the publication permanent and drop the assembly area.
    pub async fn commit(mut self) {
        self.settled = true;
        discard_staging(&self.staging).await;
    }

    /// Take every root back out of the final location.
    ///
    /// Withdrawal is a same-volume rename into an assembly area whose names are
    /// free, so it succeeds in all but extreme cases. Whatever is withdrawn is
    /// then discarded along with the staging area: the receive is over and its
    /// content has no entry to belong to.
    pub async fn rollback(mut self) -> RollbackOutcome {
        self.settled = true;
        let mut stuck = 0usize;
        // Reverse order, undoing the most recent first.
        for (final_path, staged_from) in self.published.iter().rev() {
            if let Err(err) =
                publish_via(self.publisher.as_ref(), self.mode, final_path, staged_from).await
            {
                warn!(error = %err, "failed to withdraw a published directory root");
                stuck += 1;
            }
        }
        discard_staging(&self.staging).await;

        if stuck == 0 {
            debug!(
                root_count = self.published.len(),
                "withdrew every published directory root"
            );
            RollbackOutcome::Clean
        } else {
            warn!(
                visible_roots = stuck,
                "some published directory roots could not be withdrawn and remain visible"
            );
            RollbackOutcome::PartialPublication {
                visible_roots: stuck,
            }
        }
    }
}

impl Drop for DirectoryPublication {
    fn drop(&mut self) {
        if !self.settled {
            // Nothing can be undone from here: withdrawal is async and Drop is
            // not. Make the omission visible instead of hiding it — the roots
            // stay visible to the user with no entry behind them.
            warn!(
                root_count = self.published.len(),
                "directory publication dropped without commit or rollback; roots remain visible"
            );
        }
    }
}

/// Remove an assembly area, tolerating its absence.
async fn discard_staging(staging: &std::path::Path) {
    match tokio::fs::remove_dir_all(staging).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warn!(error = %err, "failed to discard an inbound staging area"),
    }
}

#[async_trait]
impl InboundBlobFetcher for BlobTransferFacade {
    async fn seed_pending_transfer(&self, context: FetchTransferContext) -> Result<()> {
        BlobTransferFacade::seed_pending_transfer(self, &context)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn fetch_blob(&self, command: FetchBlobCommand) -> Result<FetchBlobResult> {
        // 保留 thiserror 类型链:materializer 用 `is_cancel_error` downcast
        // 判断是否 user-cancel,与真正的 fetch 失败区分对待。
        BlobTransferFacade::fetch_blob(self, command)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn fetch_blob_to_path(
        &self,
        command: FetchBlobToPathCommand,
    ) -> Result<FetchBlobToPathResult> {
        BlobTransferFacade::fetch_blob_to_path(self, command)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn record_unfetched_failure(&self, context: FetchTransferContext, detail: String) {
        BlobTransferFacade::record_unfetched_failure(self, context, detail).await;
    }
}

#[derive(Clone)]
pub(crate) struct DirectoryReceiveAttemptPorts {
    pub get: Arc<dyn GetEntryAttemptPort>,
    pub claim_commit: Arc<dyn ClaimReceiveCommitPort>,
    pub publish_log: Arc<dyn RecordDirectoryPublishPort>,
    pub clock: Arc<dyn ClockPort>,
}

#[derive(Debug, thiserror::Error)]
enum DirectoryAttemptGateError {
    #[error("directory receive attempt was cancelled")]
    Cancelled,
    #[error("directory receive attempt was superseded")]
    Superseded,
    #[error("directory receive attempt is no longer active")]
    NotActive,
}

pub struct FileCacheBlobMaterializer {
    fetcher: Arc<dyn InboundBlobFetcher>,
    cache_dir: PathBuf,
    target_reserver: Option<Arc<dyn ReserveInboundFileTargetPort>>,
    save_dir_resolver: Option<Arc<dyn ResolveInboundSaveDirPort>>,
    publisher: Arc<dyn AtomicPublishPort>,
    hidden_marker: Option<Arc<dyn MarkHiddenPort>>,
    directory_attempts: Option<DirectoryReceiveAttemptPorts>,
    receive_artifact_log: Option<Arc<dyn RecordReceiveArtifactsPort>>,
    /// Per-destination answers from [`AtomicPublishPort::supports_no_replace`],
    /// keyed by the directory the roots land in. Each entry is a shared
    /// [`tokio::sync::OnceCell`] so concurrent receives to the same
    /// destination coalesce onto one probe instead of each observing a cache
    /// miss and issuing its own. See [`Self::supports_no_replace_cached`].
    no_replace_support: tokio::sync::Mutex<BTreeMap<PathBuf, Arc<tokio::sync::OnceCell<bool>>>>,
}

impl FileCacheBlobMaterializer {
    /// `publisher` is not optional: every directory root reaches its final
    /// location through it. Which guarantee each root travels under is
    /// [`PublishMode`]'s decision, and it turns on whose files share the
    /// destination — never on what is convenient.
    pub fn new(
        fetcher: Arc<dyn InboundBlobFetcher>,
        cache_dir: PathBuf,
        publisher: Arc<dyn AtomicPublishPort>,
    ) -> Self {
        Self {
            fetcher,
            cache_dir,
            target_reserver: None,
            save_dir_resolver: None,
            publisher,
            hidden_marker: None,
            directory_attempts: None,
            receive_artifact_log: None,
            no_replace_support: tokio::sync::Mutex::new(BTreeMap::new()),
        }
    }

    pub fn with_directory_receive_attempt_ports(
        mut self,
        get: Arc<dyn GetEntryAttemptPort>,
        claim_commit: Arc<dyn ClaimReceiveCommitPort>,
        publish_log: Arc<dyn RecordDirectoryPublishPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        self.directory_attempts = Some(DirectoryReceiveAttemptPorts {
            get,
            claim_commit,
            publish_log,
            clock,
        });
        self
    }

    pub fn with_receive_artifact_log(
        mut self,
        artifact_log: Arc<dyn RecordReceiveArtifactsPort>,
    ) -> Self {
        self.receive_artifact_log = Some(artifact_log);
        self
    }

    async fn record_receive_artifacts(
        &self,
        entry_id: &EntryId,
        attempt_id: Option<&str>,
        artifacts: &[ReceiveArtifact],
    ) -> Result<bool> {
        let (Some(log), Some(attempt_id), Some(ports)) = (
            self.receive_artifact_log.as_ref(),
            attempt_id,
            self.directory_attempts.as_ref(),
        ) else {
            return Ok(false);
        };
        log.record_receive_artifacts(&ReceiveArtifactRecord {
            entry_id: entry_id.as_ref().to_owned(),
            attempt_id: attempt_id.to_owned(),
            phase: ReceiveArtifactPhase::Preparing,
            resolution: ReceiveArtifactResolution::Pending,
            artifacts: artifacts.to_vec(),
            updated_at_ms: ports.clock.now_ms(),
        })
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
        Ok(true)
    }

    /// Inject the marker that keeps the assembly area out of the user's way.
    ///
    /// Without it the area is only hidden where a leading dot is enough, which
    /// is every platform except Windows. Nothing else changes: the area is
    /// transient either way, and no behavior depends on whether it is seen.
    pub fn with_hidden_marker(mut self, marker: Arc<dyn MarkHiddenPort>) -> Self {
        self.hidden_marker = Some(marker);
        self
    }

    /// Inject the user-configured save-directory resolver. When set and a save
    /// directory is configured, free-standing inbound files are written into
    /// that directory (flat, collision-suffixed) instead of the managed cache
    /// layout. Without it (or when no directory is configured) the managed
    /// `cache_dir/iroh-blobs/<entry_id>/` layout is used.
    pub fn with_target_reserver(mut self, reserver: Arc<dyn ReserveInboundFileTargetPort>) -> Self {
        self.target_reserver = Some(reserver);
        self
    }

    /// Inject the resolver that says which directory the user wants inbound
    /// content in. Directory roots need the directory itself rather than a
    /// reserved path, because their destination must stay untouched until the
    /// whole tree is ready to appear at once.
    pub fn with_save_dir_resolver(mut self, resolver: Arc<dyn ResolveInboundSaveDirPort>) -> Self {
        self.save_dir_resolver = Some(resolver);
        self
    }

    async fn ensure_attempt_committing(&self, entry_id: &EntryId, attempt_id: &str) -> Result<()> {
        let Some(ports) = &self.directory_attempts else {
            return Ok(());
        };
        let current = ports
            .get
            .get_entry_attempt(entry_id.as_ref())
            .await
            .map_err(|error| anyhow!(error.to_string()))?
            .ok_or(DirectoryAttemptGateError::Superseded)?;
        if current.current_attempt_id != attempt_id {
            return Err(DirectoryAttemptGateError::Superseded.into());
        }
        if current.state != AttemptState::Committing {
            return Err(DirectoryAttemptGateError::NotActive.into());
        }
        Ok(())
    }

    async fn ensure_attempt_active(
        &self,
        entry_id: &EntryId,
        attempt_id: Option<&str>,
    ) -> Result<()> {
        let (Some(ports), Some(attempt_id)) = (&self.directory_attempts, attempt_id) else {
            return Ok(());
        };
        let current = ports
            .get
            .get_entry_attempt(entry_id.as_ref())
            .await
            .map_err(|error| anyhow!(error.to_string()))?
            .ok_or(DirectoryAttemptGateError::Superseded)?;
        if current.current_attempt_id != attempt_id {
            return Err(DirectoryAttemptGateError::Superseded.into());
        }
        match current.state {
            AttemptState::Receiving => Ok(()),
            AttemptState::Cancelled | AttemptState::Cancelling => {
                Err(DirectoryAttemptGateError::Cancelled.into())
            }
            AttemptState::Committing
            | AttemptState::Completed
            | AttemptState::Failed
            | AttemptState::Failing => Err(DirectoryAttemptGateError::NotActive.into()),
        }
    }
}

#[async_trait]
impl InboundBlobMaterializer for FileCacheBlobMaterializer {
    async fn materialize(
        &self,
        from_device: DeviceId,
        receiver_entry_id: EntryId,
        mut snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        file_set_manifest: Option<InboundFileSetManifest>,
        attempt_id: Option<String>,
        expected_snapshot_hash: Option<String>,
    ) -> Result<MaterializeResult> {
        if blob_refs.is_empty() && file_set_manifest.is_none() {
            return Ok(MaterializeResult::complete(snapshot));
        }

        // Split blob refs by destination:
        //   - `representation_index = Some(i)`: bytes belong to envelope rep i
        //     (image / large binary path). Fetched bytes are written back into
        //     `snapshot.representations[i]` so the rep round-trips with full
        //     content; receiver does NOT spill these to disk.
        //   - `representation_index = None`: free-standing file (legacy
        //     file-URI path). Fetched bytes go to cache_dir, file-list rep is
        //     rewritten with local `file://` URIs.
        let manifest_blob_refs = file_set_manifest.as_ref().map(|_| blob_refs.clone());
        let (rep_refs, file_refs): (Vec<V3BlobRef>, Vec<V3BlobRef>) = blob_refs
            .into_iter()
            .partition(|r| r.representation_index.is_some());

        // 全部 blob_ref 共享同一个 receiver_entry_id == transfer_id。facade 内
        // 的 lifecycle (seed / start / complete) 是 per-transfer-id 单次状态机,
        // 第二次调用会被 fail-soft 但仍 warn(`upsert_pending_transfer: skipping`
        // / `start lifecycle failed` / `complete lifecycle failed`),且 sender
        // 端会在 batch 第一个 fetch 完成时就提前收到 `OutboundProgressStatus::
        // Completed` —— UI 显示"传输完成"但实际后续 blob 还在拉。
        //
        // 给每次 fetch 一个 `BatchPosition`,让 facade 只在 First/Only 时 seed,
        // 只在 Last/Only 时 complete + 反向通知 sender。
        let batch_total = rep_refs.len() + file_refs.len();
        let mut batch_idx = 0usize;

        if let Some(attempt_id) = attempt_id.as_deref() {
            for (representation_index, blob_ref) in rep_refs.iter().enumerate() {
                let context = directory_representation_transfer_context(
                    &receiver_entry_id,
                    attempt_id,
                    representation_index,
                    blob_ref,
                    &from_device,
                    position_in_batch(representation_index, batch_total),
                );
                self.fetcher.seed_pending_transfer(context).await?;
            }

            if let Some(manifest) = file_set_manifest.as_ref() {
                let all_blob_refs = manifest_blob_refs
                    .as_ref()
                    .ok_or_else(|| anyhow!("directory payload is missing blob references"))?;
                let mut file_offset = rep_refs.len();
                for (member_index, member) in manifest.members.iter().enumerate() {
                    if member.kind == FileSetMemberKind::EmptyDirectory {
                        continue;
                    }
                    let blob_index = member
                        .blob_ref_index
                        .ok_or_else(|| anyhow!("file member is missing its blob reference"))?
                        as usize;
                    let blob_ref = all_blob_refs.get(blob_index).ok_or_else(|| {
                        anyhow!("directory blob reference index {blob_index} is out of bounds")
                    })?;
                    let context = directory_member_transfer_context(
                        &receiver_entry_id,
                        Some(attempt_id),
                        member_index,
                        blob_ref,
                        &from_device,
                        position_in_batch(file_offset, batch_total),
                    );
                    self.fetcher.seed_pending_transfer(context).await?;
                    file_offset += 1;
                }
            } else {
                for (file_index, blob_ref) in file_refs.iter().enumerate() {
                    let context = directory_member_transfer_context(
                        &receiver_entry_id,
                        Some(attempt_id),
                        file_index,
                        blob_ref,
                        &from_device,
                        position_in_batch(rep_refs.len() + file_index, batch_total),
                    );
                    self.fetcher.seed_pending_transfer(context).await?;
                }
            }
        }

        // 收集 partial cancel 时的"未完成"轨迹:
        // - `incomplete_rep_idxs`:rep_refs 阶段被取消的 representation index,
        //   退出循环后从 snapshot.representations 中倒序移除,避免把声明了但
        //   没有真实 bytes 的 rep 落库后导致 renderer 渲染失败。
        // - `missing_files`:file_refs 阶段被取消的文件元数据,用于:
        //   (a) 在 file-list rep 中以 `uniclip-missing://` URI 占位;
        //   (b) 返回给上层做"是否 partial"判定。
        // - `partial`: set when cancel or fetch error occurs; remaining blobs
        //   are marked missing and no further fetches are attempted.
        let mut incomplete_rep_idxs: Vec<usize> = Vec::new();
        let mut missing_files: Vec<MissingFileRef> = Vec::new();
        let mut partial = false;
        let mut partial_outcome = MaterializeOutcome::PartialFailed;

        // 1. Hydrate representation-bound blobs back into the snapshot.
        //
        // Pre-collect rep idx 列表,break 后能把"还没跑到"的 rep 也算进
        // incomplete —— 否则它们留在 snapshot 里就是带空 bytes 的占位 rep,
        // capture 后渲染会失败。
        let pending_rep_idxs = rep_refs
            .iter()
            .map(|blob_ref| {
                blob_ref
                    .representation_index
                    .map(|index| index as usize)
                    .ok_or_else(|| anyhow!("representation blob is missing its index"))
            })
            .collect::<Result<Vec<_>>>()?;
        for (loop_idx, blob_ref) in rep_refs.into_iter().enumerate() {
            let entry_id = blob_ref.entry_id.clone();
            let advertised_size = blob_ref.size_bytes;
            let idx = blob_ref
                .representation_index
                .ok_or_else(|| anyhow!("representation blob is missing its index"))?;
            debug!(
                entry_id = %entry_id,
                size_bytes = advertised_size,
                representation_index = idx,
                mime = blob_ref.mime.as_deref().unwrap_or(""),
                "materialize: fetching representation-bound blob"
            );

            // transfer_id 用接收端的 receiver_entry_id —— 与 file_refs
            // 路径保持一致,确保占位卡片 / 进度事件 / 最终 entry 共享同
            // 一个 ID(协议层 transfer_id == receiver_entry_id)。
            // `blob_ref.entry_id` 是发送端 id,只用于 iroh tag。
            // filename: rep-bound blob 没有显式文件名,留空让 receiver
            // projection 的 filename 字段保持空(dashboard 显示 mime/size
            // 兜底)。
            // outbound_*: 反向进度回报上下文 —— transfer_id 用 sender 的
            // entry_id(V3BlobRef.entry_id),target 用消息来源 device,
            // 两者让 sender UI 能定位本地 entry 并接收实时字节进度。
            let transfer_context = match attempt_id.as_deref() {
                Some(attempt_id) => directory_representation_transfer_context(
                    &receiver_entry_id,
                    attempt_id,
                    loop_idx,
                    &blob_ref,
                    &from_device,
                    position_in_batch(batch_idx, batch_total),
                ),
                None => FetchTransferContext {
                    transfer_id: receiver_entry_id.as_ref().to_string(),
                    entry_id: receiver_entry_id.as_ref().to_string(),
                    attempt_id: None,
                    peer_id: from_device.as_str().to_string(),
                    total_bytes: Some(advertised_size),
                    filename: String::new(),
                    outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                    outbound_target: Some(from_device),
                    batch_position: position_in_batch(batch_idx, batch_total),
                    individual_lifecycle: false,
                },
            };
            batch_idx += 1;
            let fetched = match self
                .fetcher
                .fetch_blob(FetchBlobCommand {
                    ticket: blob_ref.ticket,
                    entry_id: blob_ref.entry_id.clone(),
                    transfer_context: Some(transfer_context),
                })
                .await
            {
                Ok(v) => v,
                Err(e) if is_cancel_error(&e) => {
                    // Cancel:当前 + 后续 rep_refs 都没 fetch,把这部分 idx 全数
                    // 标记为 incomplete,稍后倒序从 snapshot.representations 删除。
                    warn!(
                        entry_id = %entry_id,
                        representation_index = idx,
                        "materialize: representation-bound blob fetch cancelled, marking partial"
                    );
                    incomplete_rep_idxs.extend(pending_rep_idxs[loop_idx..].iter().copied());
                    partial = true;
                    partial_outcome = MaterializeOutcome::PartialCancelled;
                    break;
                }
                Err(e) => {
                    warn!(
                        entry_id = %entry_id,
                        size_bytes = advertised_size,
                        representation_index = idx,
                        error = %e,
                        "materialize: representation-bound blob fetch failed, marking partial"
                    );
                    incomplete_rep_idxs.extend(pending_rep_idxs[loop_idx..].iter().copied());
                    partial = true;
                    break;
                }
            };

            let usize_idx = idx as usize;
            let rep_count = snapshot.representations.len();
            let rep = snapshot.representations.get_mut(usize_idx).ok_or_else(|| {
                anyhow!(
                    "materialize: representation_index {idx} out of bounds (snapshot has {rep_count} reps)"
                )
            })?;
            let fetched_len = fetched.plaintext.len();
            rep.set_inline_bytes(fetched.plaintext.to_vec())
                .map_err(|err| anyhow!("materialize: failed to set inline bytes: {err}"))?;
            info!(
                entry_id = %entry_id,
                representation_index = idx,
                bytes_written = fetched_len,
                "materialize: blob inlined back into representation"
            );
        }

        // Rep-refs stage partial (cancel or error): skip all file_refs fetches,
        // mark them missing, finalize via the partial path.
        if partial {
            if file_set_manifest.is_some() {
                return Err(anyhow!("directory representation blob fetch failed"));
            }
            for blob_ref in file_refs.iter() {
                missing_files.push(MissingFileRef {
                    filename: blob_ref
                        .filename
                        .clone()
                        .unwrap_or_else(|| format!("blob-{}", blob_ref.entry_id.as_ref())),
                    size_bytes: blob_ref.size_bytes,
                });
            }
            return Ok(finalize_partial(
                snapshot,
                &incomplete_rep_idxs,
                Vec::new(),
                missing_files,
                &from_device,
                partial_outcome,
                false,
                Vec::new(),
            ));
        }

        if file_refs.is_empty() && file_set_manifest.is_none() {
            return Ok(MaterializeResult::complete(snapshot));
        }

        if let Some(manifest) = file_set_manifest {
            return self
                .materialize_directory(
                    from_device,
                    receiver_entry_id,
                    snapshot,
                    manifest_blob_refs.unwrap_or_default(),
                    manifest,
                    attempt_id,
                    expected_snapshot_hash.as_deref(),
                    batch_idx,
                    batch_total,
                )
                .await;
        }

        // 2. Free-standing files: existing cache_dir + file-list rewrite path.
        let mut local_paths = Vec::with_capacity(file_refs.len());
        let mut file_content_digests: Vec<[u8; 32]> = Vec::with_capacity(file_refs.len());
        let mut used_names = HashSet::new();
        let blob_ref_total = file_refs.len();
        let mut receive_artifacts = Vec::with_capacity(file_refs.len());
        let mut artifact_journaled = false;

        // 用 indexed access 而非 into_iter().enumerate(),让 cancel break 时
        // 还能回头把 file_refs[idx+1..] 也加进 missing_files —— 否则前端只能
        // 看到当前正在 fetch 的那一个 file 名,丢失批中其他未传文件元数据。
        let mut file_idx = 0usize;
        while file_idx < file_refs.len() {
            let idx = file_idx;
            let blob_ref = file_refs[idx].clone();
            let entry_id = blob_ref.entry_id.clone();
            let advertised_size = blob_ref.size_bytes;
            let declared_name = blob_ref.filename.clone();
            debug!(
                idx,
                total = blob_ref_total,
                entry_id = %entry_id,
                size_bytes = advertised_size,
                filename = declared_name.as_deref().unwrap_or(""),
                "materialize: fetching blob"
            );

            // transfer_id 用接收端的 entry_id ——
            // ApplyInbound 已在流程入口预生成,贯穿到 capture 后的 NewContent。
            // 即便 envelope 含多个 blob_ref,也共享同一 transfer_id:前端按
            // 累计字节数显示总进度即可。`blob_ref.entry_id` 是发送端 id,
            // 仅用于 iroh tag,不参与前端关联。
            // filename: 用 sender 声明的原始文件名(blob_ref.filename),
            // dashboard 直接显示;真正落盘后的去重文件名由 BlobTransferFacade
            // 用 target_path 写进 cached_path,两者职责分离。
            // outbound_*: 反向进度回报。transfer_id 用 sender 的 entry_id
            // 让 sender UI 定位本地 entry,target 是消息来源 device。
            let transfer_context = match attempt_id.as_deref() {
                Some(attempt_id) => directory_member_transfer_context(
                    &receiver_entry_id,
                    Some(attempt_id),
                    idx,
                    &blob_ref,
                    &from_device,
                    position_in_batch(batch_idx, batch_total),
                ),
                None => FetchTransferContext {
                    transfer_id: receiver_entry_id.as_ref().to_string(),
                    entry_id: receiver_entry_id.as_ref().to_string(),
                    attempt_id: None,
                    peer_id: from_device.as_str().to_string(),
                    total_bytes: Some(advertised_size),
                    filename: declared_name.clone().unwrap_or_default(),
                    outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                    outbound_target: Some(from_device),
                    batch_position: position_in_batch(batch_idx, batch_total),
                    individual_lifecycle: false,
                },
            };
            batch_idx += 1;

            // Resolve the destination for this file. When a user save
            // directory is configured and usable, write the file there (flat,
            // collision-suffixed) so the user can operate on it directly;
            // otherwise fall back to the managed cache layout. Files written
            // outside the cache dir are user-owned and never removed by
            // retention cleanup or entry deletion.
            // Pass the declared name through as-is (empty when absent); the
            // adapter owns the single fallback-name source of truth, so we do
            // not duplicate a literal here.
            let reserved = match self.target_reserver.as_ref() {
                Some(reserver) => {
                    reserver
                        .reserve_target(blob_ref.filename.as_deref().unwrap_or_default())
                        .await
                }
                None => None,
            };
            // A reserved path is an empty placeholder the adapter atomically
            // created to win the name; if the fetch never completes we must
            // delete it (see `remove_reserved_placeholder`). When true, `path`
            // below IS that placeholder. Managed-cache paths need no cleanup.
            let from_reserver = reserved.is_some();

            let path = match reserved {
                Some(user_path) => user_path,
                None => {
                    // GH#487 Phase 2: pre-create cache dir and stream the blob
                    // directly to the target file. The previous code did
                    // `fetch_blob -> Bytes -> tokio::fs::write`, which on a
                    // 800 MB transfer wasted ~20s materialising the full
                    // plaintext in memory and writing to disk a second time
                    // (the iroh store already had a copy from BAO
                    // verification). `fetch_blob_to_path` collapses both into a
                    // single `Blobs::export` call (reflink on APFS / Btrfs /
                    // ReFS).
                    let entry_dir = self
                        .cache_dir
                        .join("iroh-blobs")
                        .join(sanitize_path_segment(blob_ref.entry_id.as_ref()));
                    tokio::fs::create_dir_all(&entry_dir).await?;

                    let filename =
                        unique_filename(blob_ref.filename.as_deref(), idx, &mut used_names);
                    entry_dir.join(filename)
                }
            };
            receive_artifacts.push(ReceiveArtifact {
                item_id: transfer_context.transfer_id.clone(),
                staged_path: path.clone(),
                final_path: path.clone(),
                ownership: if from_reserver {
                    ReceiveArtifactOwnership::UserDestination
                } else {
                    ReceiveArtifactOwnership::ManagedStaging
                },
            });
            match self
                .record_receive_artifacts(
                    &receiver_entry_id,
                    attempt_id.as_deref(),
                    &receive_artifacts,
                )
                .await
            {
                Ok(recorded) => artifact_journaled |= recorded,
                Err(error) => {
                    if from_reserver {
                        remove_reserved_placeholder(&path).await;
                    }
                    return Err(error);
                }
            }

            let fetched =
                match self
                    .fetcher
                    .fetch_blob_to_path(FetchBlobToPathCommand {
                        ticket: blob_ref.ticket,
                        entry_id: blob_ref.entry_id.clone(),
                        target_path: path.clone(),
                        transfer_context: Some(transfer_context),
                    })
                    .await
                {
                    Ok(v) => v,
                    Err(e) if is_cancel_error(&e) => {
                        // A cancelled fetch never runs the store-export step, so
                        // a reserved user-dir placeholder stays as a 0-byte file.
                        // The transfer layer does not remove it (cancel only
                        // tears down the connection), so we delete it here to
                        // keep the user's save directory clean. The current +
                        // file_refs[idx+1..] entries go to missing; break to
                        // finalize.
                        if from_reserver {
                            remove_reserved_placeholder(&path).await;
                        }
                        receive_artifacts.pop();
                        artifact_journaled |= self
                            .record_receive_artifacts(
                                &receiver_entry_id,
                                attempt_id.as_deref(),
                                &receive_artifacts,
                            )
                            .await?;
                        warn!(
                            idx,
                            total = blob_ref_total,
                            entry_id = %entry_id,
                            "materialize: blob fetch cancelled, marking partial"
                        );
                        for remaining in &file_refs[idx..] {
                            missing_files.push(MissingFileRef {
                                filename: remaining.filename.clone().unwrap_or_else(|| {
                                    format!("blob-{}", remaining.entry_id.as_ref())
                                }),
                                size_bytes: remaining.size_bytes,
                            });
                        }
                        partial = true;
                        partial_outcome = MaterializeOutcome::PartialCancelled;
                        break;
                    }
                    Err(e) => {
                        // Non-cancel failure (network drop, export error, ...).
                        // Same reasoning as the cancel arm: remove the reserved
                        // placeholder / partial file so it does not linger in
                        // the user's save directory (which retention and entry
                        // deletion never touch) or block the name for a retry.
                        if from_reserver {
                            remove_reserved_placeholder(&path).await;
                        }
                        receive_artifacts.pop();
                        artifact_journaled |= self
                            .record_receive_artifacts(
                                &receiver_entry_id,
                                attempt_id.as_deref(),
                                &receive_artifacts,
                            )
                            .await?;
                        warn!(
                            idx,
                            total = blob_ref_total,
                            entry_id = %entry_id,
                            size_bytes = advertised_size,
                            error = %e,
                            "materialize: blob fetch failed, marking partial"
                        );
                        for remaining in &file_refs[idx..] {
                            missing_files.push(MissingFileRef {
                                filename: remaining.filename.clone().unwrap_or_else(|| {
                                    format!("blob-{}", remaining.entry_id.as_ref())
                                }),
                                size_bytes: remaining.size_bytes,
                            });
                        }
                        partial = true;
                        break;
                    }
                };

            info!(
                idx,
                total = blob_ref_total,
                entry_id = %entry_id,
                bytes_written = fetched.bytes_written,
                "materialize: blob cached to local path (streaming)"
            );
            file_content_digests.push(*fetched.plaintext_hash.as_bytes());
            local_paths.push(path);
            file_idx += 1;
        }

        if partial {
            // The break collected current + remaining file_refs into
            // missing_files. Finalize the partial entry.
            return Ok(finalize_partial(
                snapshot,
                &incomplete_rep_idxs,
                local_paths,
                missing_files,
                &from_device,
                partial_outcome,
                artifact_journaled,
                receive_artifacts,
            ));
        }

        if !file_content_digests.is_empty() {
            snapshot.file_content_digests = file_content_digests;
        }

        let uri_list = local_file_uri_list(&local_paths)?;
        let mut rewritten_rep_count = 0usize;
        for rep in &mut snapshot.representations {
            if is_file_list_representation(rep) {
                rep.set_inline_bytes(uri_list.as_bytes().to_vec())
                    .map_err(|err| anyhow!("materialize: failed to rewrite files rep: {err}"))?;
                rewritten_rep_count += 1;
            }
        }

        if rewritten_rep_count == 0 {
            snapshot
                .representations
                .push(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("files"),
                    Some(MimeType("text/uri-list".to_string())),
                    uri_list.into_bytes(),
                ));
            info!(
                local_path_count = local_paths.len(),
                "materialize: appended synthetic files rep (no file-list rep in payload)"
            );
        } else {
            info!(
                rewritten_rep_count,
                local_path_count = local_paths.len(),
                "materialize: rewrote file-list reps with local paths"
            );
        }

        // 接收端 image rep 合成:对 local_paths 中的图片文件追加一条 LocalFile source
        // image rep,capture pipeline 会在 normalize 阶段同步通过 BlobWriterPort 把它
        // 物化到接收端本机 blob 仓库,产出 BlobReady 状态的持久化 rep。
        //
        // 让接收端 dashboard 通过 /clipboard/blobs/{blob_id} 拿到真实图片字节预览,
        // paste 时 OS pasteboard 同时含 file uri-list 与 image bytes —— 解决"对端粘贴
        // 看到的是 macOS 文件图标缩略图"这条历史回归。仅对第一张图片合成 rep,多文件
        // 选择不重复(对应发送端单 image rep 约定)。
        let mut already_has_image_rep = snapshot.representations.iter().any(|rep| {
            rep.mime
                .as_ref()
                .map(|m| m.as_str().to_ascii_lowercase().starts_with("image/"))
                .unwrap_or(false)
        });
        if !already_has_image_rep {
            for path in &local_paths {
                let Some(image_mime) = image_file_mime_from_path(path) else {
                    continue;
                };
                let Ok(meta) = std::fs::metadata(path) else {
                    continue;
                };
                if meta.len() == 0 {
                    continue;
                }
                snapshot
                    .representations
                    .push(ObservedClipboardRepresentation::new_local_file(
                        RepresentationId::new(),
                        FormatId::from("image-from-file"),
                        Some(MimeType(image_mime.to_string())),
                        path.clone(),
                        meta.len(),
                    ));
                info!(
                    size_bytes = meta.len(),
                    mime = image_mime,
                    "materialize: synthesized LocalFile image rep for inbound image file \
                     (BlobWriter will ingest during capture)"
                );
                already_has_image_rep = true;
                break;
            }
        }
        let _ = already_has_image_rep;

        let mut result = MaterializeResult::complete(snapshot);
        result.has_receive_artifacts = artifact_journaled;
        result.receive_artifacts = receive_artifacts;
        Ok(result)
    }
}

impl FileCacheBlobMaterializer {
    async fn materialize_directory(
        &self,
        from_device: DeviceId,
        receiver_entry_id: EntryId,
        mut snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        manifest: InboundFileSetManifest,
        attempt_id: Option<String>,
        expected_snapshot_hash: Option<&str>,
        mut batch_idx: usize,
        batch_total: usize,
    ) -> Result<MaterializeResult> {
        if attempt_id.is_some() && self.directory_attempts.is_none() {
            return Err(anyhow!(
                "directory receive attempt ports are required for an authoritative attempt"
            ));
        }
        if manifest.members.is_empty() {
            return Err(anyhow!("directory manifest has no members"));
        }

        let plan = self.open_publication(&receiver_entry_id).await?;

        let result = self
            .build_and_publish_directory(
                &from_device,
                &receiver_entry_id,
                &blob_refs,
                &manifest,
                attempt_id.as_deref(),
                expected_snapshot_hash,
                &mut snapshot,
                &plan,
                &mut batch_idx,
                batch_total,
            )
            .await;

        match result {
            Ok((publication, root_paths, member_digests)) => {
                let directory_file_set = match build_received_entry_file_set(
                    &manifest,
                    &member_digests,
                    &blob_refs,
                    &root_paths,
                ) {
                    Ok(file_set) => file_set,
                    Err(error) => {
                        return match publication.rollback().await {
                            RollbackOutcome::Clean => Err(error),
                            RollbackOutcome::PartialPublication { visible_roots } => Err(error
                                .context(format!(
                                    "manifest build rollback left {visible_roots} root(s) visible"
                                ))),
                        };
                    }
                };
                // The staging area stays until the caller settles the
                // publication: rollback needs somewhere to put the roots back.
                Ok(MaterializeResult {
                    snapshot,
                    missing: Vec::new(),
                    partial: false,
                    outcome: MaterializeOutcome::Complete,
                    publication: Some(publication),
                    directory_file_set: Some(directory_file_set),
                    has_receive_artifacts: false,
                    receive_artifacts: Vec::new(),
                })
            }
            Err(err) => {
                // Anything published was already withdrawn inside
                // `build_and_publish_directory`; this only clears the area.
                discard_staging(&plan.staging).await;
                Err(err)
            }
        }
    }

    /// Choose where the roots will land, and open the assembly area beside them.
    ///
    /// The area is a sibling of the final destination, which is what makes
    /// publication a same-volume rename — the atomicity the whole design rests
    /// on.
    ///
    /// The user's save directory is only usable if its volume can refuse an
    /// occupied name, because the user's own folders live there; a volume that
    /// cannot is declined in favour of managed storage. Managed storage itself
    /// asks for no such thing — see [`PublishMode::IntoFreeName`] — so it is
    /// always available and this never fails for want of a volume.
    async fn open_publication(&self, receiver_entry_id: &EntryId) -> Result<PublishPlan> {
        let staging_name = staging_dir_name(receiver_entry_id);

        // Preferred destination: the folder the user configured.
        if let Some(resolver) = &self.save_dir_resolver {
            if let Some(save_dir) = resolver.resolve_save_dir().await {
                let staging = save_dir.join(&staging_name);
                match self
                    .open_staging_area(&staging, PublishMode::NoReplace)
                    .await
                {
                    Ok(true) => {
                        return Ok(PublishPlan {
                            dest_parent: save_dir,
                            staging,
                            mode: PublishMode::NoReplace,
                        })
                    }
                    Ok(false) => {
                        // The volume would let us replace someone's folder
                        // without noticing. Decline it and use managed storage:
                        // the content still lands, just not where the user
                        // asked. This is the same fallback the reserver's
                        // `None` already expresses for free-standing files.
                        info!(
                            "auto-save volume cannot publish without replacing; \
                             using managed storage for this directory"
                        );
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "could not open a staging area in the auto-save dir; \
                             using managed storage for this directory"
                        );
                    }
                }
            }
        }

        // Fallback: the managed cache layout, on the app's own volume. This
        // parent is ours and scoped to this one receive, so no probe gates it:
        // the volume need only be able to move within itself. Requiring more
        // would strand directory sync entirely on exFAT, NFS and the like —
        // which is exactly where a portable install tends to live.
        let dest_parent = self
            .cache_dir
            .join("iroh-blobs")
            .join(sanitize_path_segment(receiver_entry_id.as_ref()));
        tokio::fs::create_dir_all(&dest_parent).await?;
        let staging = dest_parent.join(&staging_name);
        self.open_staging_area(&staging, PublishMode::IntoFreeName)
            .await?;
        Ok(PublishPlan {
            dest_parent,
            staging,
            mode: PublishMode::IntoFreeName,
        })
    }

    /// Create the assembly area and report whether it can carry `mode`.
    ///
    /// [`PublishMode::IntoFreeName`] asks nothing of the volume and always
    /// answers `true`. [`PublishMode::NoReplace`] is probed against the real
    /// filesystem, and the area is removed again when the answer is `false`,
    /// leaving no trace.
    async fn open_staging_area(
        &self,
        staging: &std::path::Path,
        mode: PublishMode,
    ) -> Result<bool> {
        // A same-id leftover can only be debris from an earlier crashed
        // receive: this id belongs to the receive happening right now.
        if tokio::fs::try_exists(staging).await? {
            tokio::fs::remove_dir_all(staging).await?;
        }
        tokio::fs::create_dir_all(staging).await?;
        if let Some(marker) = &self.hidden_marker {
            marker.mark_hidden(staging).await;
        }

        if mode == PublishMode::IntoFreeName {
            return Ok(true);
        }

        // Probe inside the area rather than in the destination folder: same
        // volume, same answer, but the check itself never appears next to the
        // user's files.
        if self.supports_no_replace_cached(staging).await {
            return Ok(true);
        }
        discard_staging(staging).await;
        Ok(false)
    }

    /// Answer the volume-support question once per destination.
    ///
    /// Support is a property of the volume, not of the receive, so asking on
    /// every paste buys nothing — and on a network save directory each ask is
    /// several round trips. The key is the probe area's parent: one save
    /// directory, one answer.
    ///
    /// A wrong answer cached here cannot cost data. `false` only ever costs a
    /// fallback to managed storage, and `true` still runs through
    /// `publish_no_replace`, which refuses an occupied name on its own.
    async fn supports_no_replace_cached(&self, staging: &std::path::Path) -> bool {
        let key = staging
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| staging.to_path_buf());
        // Take (or create) the destination's shared cell under the lock, then
        // release the lock before probing: the probe can be several network
        // round trips, and holding the map lock across it would serialize
        // every destination behind whichever one is probing.
        let cell = {
            let mut support = self.no_replace_support.lock().await;
            Arc::clone(
                support
                    .entry(key)
                    .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new())),
            )
        };
        // Concurrent receives to the same destination all await this one
        // initialization, so the "once per destination" guarantee holds under
        // concurrency and not just in steady state.
        *cell
            .get_or_init(|| async { self.publisher.supports_no_replace(staging).await })
            .await
    }

    async fn build_and_publish_directory(
        &self,
        from_device: &DeviceId,
        receiver_entry_id: &EntryId,
        blob_refs: &[V3BlobRef],
        manifest: &InboundFileSetManifest,
        attempt_id: Option<&str>,
        expected_snapshot_hash: Option<&str>,
        snapshot: &mut SystemClipboardSnapshot,
        plan: &PublishPlan,
        batch_idx: &mut usize,
        batch_total: usize,
    ) -> Result<(
        DirectoryPublication,
        BTreeMap<u32, PathBuf>,
        BTreeMap<u32, [u8; 32]>,
    )> {
        let staging_base = plan.staging.as_path();
        let mut roots = BTreeMap::<u32, (String, bool)>::new();
        let mut root_member_counts = BTreeMap::<u32, usize>::new();
        for member in &manifest.members {
            validate_manifest_member(member)?;
            *root_member_counts.entry(member.root_index).or_default() += 1;
            match roots.get(&member.root_index) {
                Some((existing_name, existing_is_file))
                    if existing_name != &member.root_name
                        || *existing_is_file != member.root_is_file =>
                {
                    return Err(anyhow!(
                        "directory root {} has conflicting names",
                        member.root_index
                    ));
                }
                _ => {
                    roots.insert(
                        member.root_index,
                        (member.root_name.clone(), member.root_is_file),
                    );
                }
            }
        }
        for (root_index, (_, root_is_file)) in &roots {
            if *root_is_file && root_member_counts.get(root_index) != Some(&1) {
                return Err(anyhow!("top-level file root must have exactly one member"));
            }
        }

        for (root_index, (root_name, root_is_file)) in &roots {
            if !root_is_file {
                tokio::fs::create_dir_all(staging_root(staging_base, *root_index, root_name))
                    .await?;
            }
        }

        // Resolve and durably record every staged-to-final root before any
        // member fetch starts. Cancellation and crash recovery can therefore
        // clean this attempt without reconstructing sender-private names.
        let mut claimed = HashSet::new();
        let mut plans = Vec::new();
        for (root_index, (root_name, _)) in &roots {
            let sanitized_name = sanitize_root_name(root_name);
            let desired =
                resolve_nonconflicting_root_path(&plan.dest_parent, &sanitized_name, &claimed)
                    .await?;
            claimed.insert(desired.clone());
            plans.push((
                *root_index,
                staging_root(staging_base, *root_index, root_name),
                desired,
                sanitized_name,
            ));
        }
        if let (Some(ports), Some(attempt_id)) = (&self.directory_attempts, attempt_id) {
            let root_map = plans
                .iter()
                .map(|(_, source, desired, _)| (source.clone(), desired.clone()))
                .collect::<Vec<_>>();
            ports
                .publish_log
                .record_phase(
                    receiver_entry_id.as_ref(),
                    attempt_id,
                    PublishPhase::Staging,
                    &root_map,
                    ports.clock.now_ms(),
                )
                .await
                .map_err(|error| anyhow!(error.to_string()))?;
        }

        let mut member_digests = BTreeMap::new();
        for (member_index, member) in manifest.members.iter().enumerate() {
            self.ensure_attempt_active(receiver_entry_id, attempt_id)
                .await?;
            let root = staging_root(staging_base, member.root_index, &member.root_name);
            let target = if member.root_is_file {
                root.clone()
            } else {
                join_relative_path(&root, &member.relative_path)?
            };
            match member.kind {
                FileSetMemberKind::EmptyDirectory => {
                    if member.blob_ref_index.is_some() {
                        return Err(anyhow!("empty directory member references a blob"));
                    }
                    tokio::fs::create_dir_all(&target).await?;
                }
                FileSetMemberKind::File | FileSetMemberKind::Executable => {
                    let index = member
                        .blob_ref_index
                        .ok_or_else(|| anyhow!("file member is missing its blob reference"))?
                        as usize;
                    let blob_ref = blob_refs.get(index).ok_or_else(|| {
                        anyhow!("directory blob reference index {index} is out of bounds")
                    })?;
                    if blob_ref.representation_index.is_some() {
                        return Err(anyhow!("directory member references a representation blob"));
                    }
                    if let Some(parent) = target.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let context = directory_member_transfer_context(
                        receiver_entry_id,
                        attempt_id,
                        member_index,
                        blob_ref,
                        from_device,
                        position_in_batch(*batch_idx, batch_total),
                    );
                    *batch_idx += 1;
                    let fetched = match self
                        .fetcher
                        .fetch_blob_to_path(FetchBlobToPathCommand {
                            ticket: blob_ref.ticket.clone(),
                            entry_id: blob_ref.entry_id.clone(),
                            target_path: target.clone(),
                            transfer_context: Some(context),
                        })
                        .await
                    {
                        Ok(fetched) => fetched,
                        Err(err) => {
                            if is_cancel_error(&err) {
                                return Err(err);
                            }
                            let detail = err.to_string();
                            for (remaining_index, remaining) in
                                manifest.members.iter().enumerate().skip(member_index + 1)
                            {
                                if remaining.kind == FileSetMemberKind::EmptyDirectory {
                                    continue;
                                }
                                let remaining_blob_index =
                                    remaining.blob_ref_index.ok_or_else(|| {
                                        anyhow!("file member is missing its blob reference")
                                    })? as usize;
                                let remaining_blob = blob_refs.get(remaining_blob_index).ok_or_else(|| {
                                    anyhow!("directory blob reference index {remaining_blob_index} is out of bounds")
                                })?;
                                self.fetcher
                                    .record_unfetched_failure(
                                        directory_member_transfer_context(
                                            receiver_entry_id,
                                            attempt_id,
                                            remaining_index,
                                            remaining_blob,
                                            from_device,
                                            BatchPosition::Middle,
                                        ),
                                        format!("not fetched after another directory member failed: {detail}"),
                                    )
                                    .await;
                            }
                            return Err(err);
                        }
                    };
                    if member.kind == FileSetMemberKind::Executable {
                        restore_executable_permission(&target).await?;
                    }
                    member_digests.insert(index as u32, *fetched.plaintext_hash.as_bytes());
                }
            }
            self.ensure_attempt_active(receiver_entry_id, attempt_id)
                .await?;
        }
        self.ensure_attempt_active(receiver_entry_id, attempt_id)
            .await?;

        let candidate_root_paths = plans
            .iter()
            .map(|(root_index, _, desired, _)| (*root_index, desired.clone()))
            .collect::<BTreeMap<_, _>>();
        let candidate_root_list = candidate_root_paths.values().cloned().collect::<Vec<_>>();
        finalize_directory_snapshot(snapshot, manifest, &member_digests, &candidate_root_list)?;
        if let Some(expected_snapshot_hash) = expected_snapshot_hash {
            verify_file_set_identity(snapshot, expected_snapshot_hash)?;
        }

        let mut logged_root_map = plans
            .iter()
            .map(|(_, source, desired, _)| (source.clone(), desired.clone()))
            .collect::<Vec<_>>();
        let gated_attempt = match (&self.directory_attempts, attempt_id) {
            (Some(ports), Some(attempt_id)) => {
                self.ensure_attempt_active(receiver_entry_id, Some(attempt_id))
                    .await?;
                let now_ms = ports.clock.now_ms();
                ports
                    .publish_log
                    .record_phase(
                        receiver_entry_id.as_ref(),
                        attempt_id,
                        PublishPhase::Publishing,
                        &logged_root_map,
                        now_ms,
                    )
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                if !ports
                    .claim_commit
                    .claim_receive_commit(receiver_entry_id.as_ref(), attempt_id, now_ms)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?
                {
                    return Err(DirectoryAttemptGateError::NotActive.into());
                }
                self.ensure_attempt_committing(receiver_entry_id, attempt_id)
                    .await?;
                Some((ports, attempt_id))
            }
            _ => None,
        };

        let mut publication =
            DirectoryPublication::new(Arc::clone(&self.publisher), plan.staging.clone(), plan.mode);
        let mut published_paths = BTreeMap::new();
        for (root_index, source, desired, sanitized_name) in plans {
            let publish_result = if let Some((ports, attempt_id)) = gated_attempt {
                publish_gated_root(
                    self.publisher.as_ref(),
                    plan.mode,
                    &source,
                    &desired,
                    &plan.dest_parent,
                    &sanitized_name,
                    &mut claimed,
                    &mut logged_root_map,
                    ports,
                    receiver_entry_id,
                    attempt_id,
                )
                .await
            } else {
                publish_root(
                    self.publisher.as_ref(),
                    plan.mode,
                    &source,
                    &desired,
                    &plan.dest_parent,
                    &sanitized_name,
                    &claimed,
                )
                .await
            };
            match publish_result {
                Ok(final_path) => {
                    published_paths.insert(root_index, final_path.clone());
                    publication.record(final_path, source);
                }
                Err(err) => {
                    // Compensating rollback: a paste is all roots or none, so
                    // take back the ones that made it before giving up.
                    let outcome = publication.rollback().await;
                    return Err(match outcome {
                        RollbackOutcome::Clean => err,
                        RollbackOutcome::PartialPublication { visible_roots } => {
                            if let Some((ports, attempt_id)) = gated_attempt {
                                match u32::try_from(visible_roots) {
                                    Ok(visible_roots) => {
                                        if let Err(record_error) = ports
                                            .publish_log
                                            .record_partial(
                                                receiver_entry_id.as_ref(),
                                                attempt_id,
                                                visible_roots,
                                                ports.clock.now_ms(),
                                            )
                                            .await
                                        {
                                            warn!(error = %record_error, "failed to record partial directory publication");
                                        }
                                    }
                                    Err(error) => {
                                        warn!(error = %error, "partial directory root count exceeds u32");
                                    }
                                }
                            }
                            err.context(format!(
                                "partial_publication: {visible_roots} root(s) left visible"
                            ))
                        }
                    });
                }
            }
        }
        Ok((publication, published_paths, member_digests))
    }
}

/// Where a directory receive is assembled and where it will land.
struct PublishPlan {
    /// Final parent directory for every root of this receive.
    dest_parent: PathBuf,
    /// Assembly area, a child of `dest_parent` and so on its volume.
    staging: PathBuf,
    /// The guarantee `dest_parent` needs, decided by whose files live there.
    mode: PublishMode,
}

async fn publish_gated_root(
    publisher: &dyn AtomicPublishPort,
    mode: PublishMode,
    source: &std::path::Path,
    desired: &std::path::Path,
    dest_parent: &std::path::Path,
    sanitized_name: &str,
    claimed: &mut HashSet<PathBuf>,
    root_map: &mut Vec<(PathBuf, PathBuf)>,
    ports: &DirectoryReceiveAttemptPorts,
    receiver_entry_id: &EntryId,
    attempt_id: &str,
) -> Result<PathBuf> {
    let mut candidate = desired.to_path_buf();
    for _ in 0..MAX_PUBLISH_ATTEMPTS {
        match publish_via(publisher, mode, source, &candidate).await {
            Ok(()) => return Ok(candidate),
            Err(PublishError::DestinationExists) => {
                candidate =
                    resolve_nonconflicting_root_path(dest_parent, sanitized_name, claimed).await?;
                claimed.insert(candidate.clone());
                let mapping = root_map
                    .iter_mut()
                    .find(|(staged, _)| staged == source)
                    .ok_or_else(|| anyhow!("directory publish root is absent from recovery log"))?;
                mapping.1 = candidate.clone();
                ports
                    .publish_log
                    .record_phase(
                        receiver_entry_id.as_ref(),
                        attempt_id,
                        PublishPhase::Publishing,
                        root_map,
                        ports.clock.now_ms(),
                    )
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
            }
            Err(error) => {
                return Err(anyhow!("directory root publication failed: {error}"));
            }
        }
    }
    Err(anyhow!(
        "directory root publication lost {MAX_PUBLISH_ATTEMPTS} races for a free name"
    ))
}

/// Publish one root, stepping to the next free name if the chosen one was taken
/// between pre-flight and now.
///
/// The retry is what turns a lost race into a bounded cost, and it only has
/// teeth under [`PublishMode::NoReplace`]: there a name taken in the gap costs
/// another attempt rather than someone else's data.
async fn publish_root(
    publisher: &dyn AtomicPublishPort,
    mode: PublishMode,
    source: &std::path::Path,
    desired: &std::path::Path,
    dest_parent: &std::path::Path,
    sanitized_name: &str,
    claimed: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    let mut candidate = desired.to_path_buf();
    for _ in 0..MAX_PUBLISH_ATTEMPTS {
        match publish_via(publisher, mode, source, &candidate).await {
            Ok(()) => return Ok(candidate),
            Err(PublishError::DestinationExists) => {
                candidate =
                    resolve_nonconflicting_root_path(dest_parent, sanitized_name, claimed).await?;
            }
            Err(err) => return Err(anyhow!("directory root publication failed: {err}")),
        }
    }
    Err(anyhow!(
        "directory root publication lost {MAX_PUBLISH_ATTEMPTS} races for a free name"
    ))
}

fn directory_member_transfer_context(
    receiver_entry_id: &EntryId,
    attempt_id: Option<&str>,
    member_index: usize,
    blob_ref: &V3BlobRef,
    from_device: &DeviceId,
    batch_position: BatchPosition,
) -> FetchTransferContext {
    FetchTransferContext {
        transfer_id: directory_member_transfer_id(receiver_entry_id, attempt_id, member_index),
        entry_id: receiver_entry_id.as_ref().to_owned(),
        attempt_id: attempt_id.map(str::to_owned),
        peer_id: from_device.as_str().to_owned(),
        total_bytes: Some(blob_ref.size_bytes),
        filename: blob_ref.filename.clone().unwrap_or_default(),
        outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_owned()),
        outbound_target: Some(*from_device),
        batch_position,
        individual_lifecycle: true,
    }
}

fn directory_member_transfer_id(
    receiver_entry_id: &EntryId,
    attempt_id: Option<&str>,
    member_index: usize,
) -> String {
    match attempt_id {
        Some(attempt_id) => format!(
            "{}:attempt:{attempt_id}:member:{member_index}",
            receiver_entry_id.as_ref()
        ),
        None => format!("{}:member:{member_index}", receiver_entry_id.as_ref()),
    }
}

fn directory_representation_transfer_id(
    entry_id: &EntryId,
    attempt_id: &str,
    representation_index: usize,
) -> String {
    format!(
        "{}:attempt:{}:representation:{}",
        entry_id.as_ref(),
        attempt_id,
        representation_index
    )
}

fn directory_representation_transfer_context(
    receiver_entry_id: &EntryId,
    attempt_id: &str,
    representation_index: usize,
    blob_ref: &V3BlobRef,
    from_device: &DeviceId,
    batch_position: BatchPosition,
) -> FetchTransferContext {
    FetchTransferContext {
        transfer_id: directory_representation_transfer_id(
            receiver_entry_id,
            attempt_id,
            representation_index,
        ),
        entry_id: receiver_entry_id.as_ref().to_owned(),
        attempt_id: Some(attempt_id.to_owned()),
        peer_id: from_device.as_str().to_owned(),
        total_bytes: Some(blob_ref.size_bytes),
        filename: String::new(),
        outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_owned()),
        outbound_target: Some(*from_device),
        batch_position,
        individual_lifecycle: true,
    }
}

fn build_received_entry_file_set(
    manifest: &InboundFileSetManifest,
    member_digests: &BTreeMap<u32, [u8; 32]>,
    blob_refs: &[V3BlobRef],
    root_paths: &BTreeMap<u32, PathBuf>,
) -> Result<EntryFileSet> {
    let row_count = i64::try_from(manifest.members.len())?;
    let lines = manifest
        .members
        .iter()
        .enumerate()
        .map(|(index, member)| {
            let root_path = root_paths
                .get(&member.root_index)
                .ok_or_else(|| anyhow!("published directory root is missing"))?;
            let location = FileSetMemberLocation {
                root_index: i64::from(member.root_index),
                root_name: member.root_name.clone(),
                relative_path: member.relative_path.clone(),
                kind: member.kind,
            };
            let kind = match member.kind {
                FileSetMemberKind::EmptyDirectory => EntryFileSetLineKind::NonFile,
                FileSetMemberKind::File | FileSetMemberKind::Executable => {
                    let blob_index = member
                        .blob_ref_index
                        .ok_or_else(|| anyhow!("file member is missing its blob reference"))?;
                    let digest = member_digests
                        .get(&blob_index)
                        .ok_or_else(|| anyhow!("file member is missing its fetched digest"))?;
                    let size_bytes = i64::try_from(
                        blob_refs
                            .get(blob_index as usize)
                            .ok_or_else(|| anyhow!("file member blob reference is out of bounds"))?
                            .size_bytes,
                    )?;
                    EntryFileSetLineKind::File {
                        content_hash: ContentHash {
                            alg: HashAlgorithm::Blake3V1,
                            bytes: *digest,
                        },
                        blob_id: None,
                        size_bytes: Some(size_bytes),
                    }
                }
            };
            Ok(EntryFileSetLine {
                line_index: row_count + i64::try_from(index)?,
                original_text: root_path.to_string_lossy().into_owned(),
                member_location: Some(location),
                kind,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(EntryFileSet { lines })
}

/// Fold the published roots into the snapshot: clear the per-file digests,
/// recompute the file-set identity component, and rewrite the file list to the
/// roots' local `file://` URIs.
///
/// Split out from [`FileCacheBlobMaterializer::materialize_directory`] so its
/// failure is a single value the caller can react to — the roots are already
/// visible by this point, so a failure here has to withdraw the publication
/// rather than drop it.
pub(crate) fn verify_file_set_identity(
    snapshot: &SystemClipboardSnapshot,
    expected_snapshot_hash: &str,
) -> Result<()> {
    let actual = snapshot.snapshot_hash().to_string();
    if actual != expected_snapshot_hash {
        return Err(anyhow!(
            "directory file-set identity mismatch: expected {expected_snapshot_hash}, got {actual}"
        ));
    }
    Ok(())
}

fn finalize_directory_snapshot(
    snapshot: &mut SystemClipboardSnapshot,
    manifest: &InboundFileSetManifest,
    member_digests: &BTreeMap<u32, [u8; 32]>,
    root_paths: &[PathBuf],
) -> Result<()> {
    snapshot.file_content_digests.clear();
    snapshot.file_set_v1_component = Some(compute_file_set_component(manifest, member_digests)?);
    let uri_list = local_file_uri_list(root_paths)?;
    rewrite_file_list(snapshot, uri_list, root_paths.len())?;
    Ok(())
}

pub(crate) fn compute_file_set_component(
    manifest: &InboundFileSetManifest,
    member_digests: &BTreeMap<u32, [u8; 32]>,
) -> Result<[u8; 32]> {
    let row_count = i64::try_from(manifest.members.len())?;
    let mut lines = Vec::with_capacity(manifest.members.len());
    for (index, member) in manifest.members.iter().enumerate() {
        let location = FileSetMemberLocation {
            root_index: i64::from(member.root_index),
            root_name: member.root_name.clone(),
            relative_path: member.relative_path.clone(),
            kind: member.kind,
        };
        let kind = match member.kind {
            FileSetMemberKind::File | FileSetMemberKind::Executable => {
                let blob_ref_index = member
                    .blob_ref_index
                    .ok_or_else(|| anyhow!("file member is missing its blob reference"))?;
                let bytes = member_digests
                    .get(&blob_ref_index)
                    .ok_or_else(|| anyhow!("file member is missing its fetched content digest"))?;
                EntryFileSetLineKind::File {
                    content_hash: ContentHash {
                        alg: HashAlgorithm::Blake3V1,
                        bytes: *bytes,
                    },
                    blob_id: None,
                    size_bytes: None,
                }
            }
            FileSetMemberKind::EmptyDirectory => EntryFileSetLineKind::NonFile,
        };
        lines.push(EntryFileSetLine {
            line_index: row_count + i64::try_from(index)?,
            original_text: String::new(),
            member_location: Some(location),
            kind,
        });
    }
    EntryFileSet { lines }
        .file_set_v1_component()
        .ok_or_else(|| anyhow!("directory manifest cannot produce a file-set identity"))
}

/// Find a free name under `parent` for `desired_name`, stepping through
/// `name (1)`, `name (2)`, … past anything the filesystem or `reserved`
/// already holds.
///
/// The suffix sequence matches the one free-standing inbound files get: both
/// kinds land in the same folder, and a user seeing `report (1).pdf` next to
/// `report (2)/` for the first collision of each would be reading a difference
/// that means nothing.
async fn resolve_nonconflicting_root_path(
    parent: &std::path::Path,
    desired_name: &str,
    reserved: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    let desired = parent.join(desired_name);
    if !reserved.contains(&desired) && !tokio::fs::try_exists(&desired).await? {
        return Ok(desired);
    }
    for suffix in 1..MAX_COLLISION_ATTEMPTS {
        let candidate = parent.join(format!("{desired_name} ({suffix})"));
        if !reserved.contains(&candidate) && !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }
    Err(anyhow!("directory collision suffix space exhausted"))
}

#[cfg(unix)]
async fn restore_executable_permission(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = tokio::fs::metadata(path).await?.permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    tokio::fs::set_permissions(path, permissions).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn restore_executable_permission(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn validate_manifest_member(member: &InboundFileSetMember) -> Result<()> {
    if member.root_name.is_empty()
        || member.root_name == "."
        || member.root_name == ".."
        || member.root_name.contains(['/', '\\'])
    {
        return Err(anyhow!("invalid directory root name"));
    }
    if member.relative_path.is_empty() {
        return Err(anyhow!("directory member has an empty relative path"));
    }
    if member.root_is_file
        && (member.relative_path != member.root_name
            || member.kind == FileSetMemberKind::EmptyDirectory)
    {
        return Err(anyhow!("invalid top-level file member"));
    }
    Ok(())
}

fn staging_root(base: &std::path::Path, root_index: u32, root_name: &str) -> PathBuf {
    base.join(format!("{root_index}-{}", sanitize_path_segment(root_name)))
}

fn join_relative_path(root: &std::path::Path, relative_path: &str) -> Result<PathBuf> {
    let mut result = root.to_path_buf();
    for component in relative_path.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\\')
        {
            return Err(anyhow!("invalid directory member relative path"));
        }
        result.push(component);
    }
    Ok(result)
}

fn rewrite_file_list(
    snapshot: &mut SystemClipboardSnapshot,
    uri_list: String,
    local_path_count: usize,
) -> Result<()> {
    let mut rewritten = 0usize;
    for rep in &mut snapshot.representations {
        if is_file_list_representation(rep) {
            rep.set_inline_bytes(uri_list.as_bytes().to_vec())
                .map_err(|err| anyhow!("materialize: failed to rewrite files rep: {err}"))?;
            rewritten += 1;
        }
    }
    if rewritten == 0 {
        snapshot
            .representations
            .push(ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                uri_list.into_bytes(),
            ));
    }
    debug!(
        rewritten,
        local_path_count, "materialize: rewrote directory roots"
    );
    Ok(())
}

/// 把 partial cancel 的中间状态收口为 [`MaterializeResult`]:
/// - 删除 `incomplete_rep_idxs` 指向的 representation(它们没有真实 bytes,
///   留在 snapshot 里 capture 后会让 renderer 渲染失败);
/// - 用 `completed_paths`(已落地的 file://) + `missing_files`(占位 URI)
///   拼出 file-list rep 的新内容;若 envelope 原本没有 file-list rep,
///   追加一条合成 rep,保证有 `text/uri-list` 表达;
/// - 若上述全部完成后 snapshot 没有任何 supported representation(极端
///   场景:rep_refs 被全删 + envelope 没有自带任何不需要 fetch 的 rep),
///   mint 一条 `text/plain` 兜底 rep,描述这是一次 cancelled transfer,
///   保证 `CaptureClipboardUseCase::has_supported_representation` 返回 true。
fn finalize_partial(
    mut snapshot: SystemClipboardSnapshot,
    incomplete_rep_idxs: &[usize],
    completed_paths: Vec<PathBuf>,
    missing_files: Vec<MissingFileRef>,
    from_device: &DeviceId,
    outcome: MaterializeOutcome,
    has_receive_artifacts: bool,
    receive_artifacts: Vec<ReceiveArtifact>,
) -> MaterializeResult {
    // 倒序删除 incomplete rep(顺序 -> 倒序保 idx 仍有效)。
    let mut sorted_idxs: Vec<usize> = incomplete_rep_idxs.to_vec();
    sorted_idxs.sort_unstable();
    sorted_idxs.dedup();
    for &i in sorted_idxs.iter().rev() {
        if i < snapshot.representations.len() {
            snapshot.representations.remove(i);
        }
    }

    // 拼 file-list rep 内容:已完成 file:// + 未完成 uniclip-missing://。
    // 这条 rep 始终存在于 partial 路径,既给前端解析依据,也让 OS
    // clipboard write(若意外没被上层短路)拿到的是占位 URI 而非空。
    let mut uri_lines: Vec<String> = Vec::new();
    for path in &completed_paths {
        if let Ok(u) = Url::from_file_path(path) {
            uri_lines.push(u.into());
        }
    }
    for missing in &missing_files {
        uri_lines.push(format_missing_uri(missing));
    }
    let uri_list_body = uri_lines.join("\r\n");

    let mut rewritten = 0usize;
    for rep in &mut snapshot.representations {
        if is_file_list_representation(rep) {
            // set_inline_bytes 失败说明 rep 类型不允许 inline,partial 路径
            // 容忍:跳过该 rep,后面追加一条新合成 rep 兜底。
            if rep
                .set_inline_bytes(uri_list_body.as_bytes().to_vec())
                .is_ok()
            {
                rewritten += 1;
            }
        }
    }
    if rewritten == 0 {
        snapshot
            .representations
            .push(ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                uri_list_body.clone().into_bytes(),
            ));
    }

    // 兜底:若 snapshot 没有任何 supported rep(极端:rep_refs 全删 +
    // envelope 自带的 rep 也都是 unsupported MIME),mint 一条 text/plain
    // 描述这次 cancelled transfer 的元数据。让"取消后 entry 总是保留"
    // 这条用户契约不被 `has_supported_representation=false` 短路掉。
    if !snapshot.representations.iter().any(supported_for_capture) {
        let mut body = String::new();
        body.push_str(&format!(
            "[Cancelled transfer from {}]\n",
            from_device.as_str()
        ));
        for missing in &missing_files {
            body.push_str(&missing.filename);
            body.push('\n');
        }
        snapshot
            .representations
            .push(ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                body.into_bytes(),
            ));
        info!("materialize: minted fallback text/plain rep for empty partial snapshot");
    }

    info!(
        missing = missing_files.len(),
        completed = completed_paths.len(),
        dropped_reps = sorted_idxs.len(),
        "materialize: finalized partial result"
    );
    MaterializeResult {
        snapshot,
        missing: missing_files,
        partial: true,
        outcome,
        // Directory receives are all-or-nothing and never finalize as partial,
        // so this path never carries publication state or a directory manifest.
        publication: None,
        directory_file_set: None,
        has_receive_artifacts,
        receive_artifacts,
    }
}

/// `uniclip-missing:` URI 编码缺失文件元数据,供前端解析渲染。
/// 设计为 opaque path("///{filename}") 形态以避免与 `file://` 解析器冲突;
/// query string 携带可选元数据,渲染层按需读取。
fn format_missing_uri(missing: &MissingFileRef) -> String {
    // URL-encode filename + size into a path-segment-safe form. We deliberately
    // hand-roll the encoder for one segment instead of pulling a full
    // percent-encoding crate.
    let encoded = encode_path_segment(&missing.filename);
    format!(
        "uniclip-missing:///{}?size={}&reason=cancelled",
        encoded, missing.size_bytes
    )
}

fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// 与 `CaptureClipboardUseCase::is_supported_representation` 同义。
/// 此处复制一份的原因:partial 路径在 capture 之前就要知道 snapshot 是否
/// 能通过 `has_supported_representation` 的门,以决定是否 mint 兜底 rep。
/// capture 那个函数是 `pub(crate)` 不能跨模块调,且把它公开会污染 facade
/// 边界;复制一份在 partial finalize 路径里 self-contained。
fn supported_for_capture(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = &rep.mime {
        let mime_str = mime.as_str();
        if mime_str.starts_with("text/")
            || mime_str.starts_with("image/")
            || mime_str.eq_ignore_ascii_case("file/uri-list")
            || mime_str.eq_ignore_ascii_case("text/uri-list")
        {
            return true;
        }
    }
    // format_id may still carry platform-native identifiers (UTIs,
    // NSPasteboard legacy names) — that is the field's documented
    // role. Only the `mime` field is normalized to RFC at the
    // engine boundary (wire + DB read).
    rep.format_id.eq_ignore_ascii_case("text")
        || rep.format_id.eq_ignore_ascii_case("rtf")
        || rep.format_id.eq_ignore_ascii_case("html")
        || rep.format_id.eq_ignore_ascii_case("files")
        || rep.format_id.eq_ignore_ascii_case("image")
        || rep.format_id.eq_ignore_ascii_case("public.utf8-plain-text")
        || rep.format_id.eq_ignore_ascii_case("public.text")
        || rep.format_id.eq_ignore_ascii_case("NSStringPboardType")
}

/// 基于文件后缀推断常见图片 MIME。与 `uc-platform/clipboard/common.rs` 的同名 helper
/// 表项保持一致(打开扩展时两边一起改);该函数刻意复制一份在 application 层,避免把
/// 接收端的 image rep 合成逻辑硬连到 platform crate。
fn image_file_mime_from_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => return None,
    })
}

fn is_file_list_representation(rep: &ObservedClipboardRepresentation) -> bool {
    rep.mime
        .as_ref()
        .map(|mime| {
            mime.as_str().eq_ignore_ascii_case("text/uri-list")
                || mime.as_str().eq_ignore_ascii_case("file/uri-list")
        })
        .unwrap_or(false)
        || rep.format_id.eq_ignore_ascii_case("files")
        || rep.format_id.eq_ignore_ascii_case("public.file-url")
}

fn unique_filename(
    candidate: Option<&str>,
    idx: usize,
    used_names: &mut HashSet<String>,
) -> String {
    let base = candidate
        .and_then(|name| {
            std::path::Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
        })
        .map(sanitize_path_segment)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("blob-{idx}"));

    if used_names.insert(base.clone()) {
        return base;
    }

    let mut counter = 1usize;
    loop {
        let candidate = format!("{counter}-{base}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

/// 在大小为 `total` 的 fetch batch 里, 把 0-based `idx` 映射到 `BatchPosition`。
/// 用于让 facade 只在第一帧 seed 一次, 只在最后一帧 complete 一次。
fn position_in_batch(idx: usize, total: usize) -> BatchPosition {
    debug_assert!(idx < total, "batch index out of range: {idx} >= {total}");
    if total <= 1 {
        BatchPosition::Only
    } else if idx == 0 {
        BatchPosition::First
    } else if idx + 1 == total {
        BatchPosition::Last
    } else {
        BatchPosition::Middle
    }
}

/// Best-effort removal of a reserved user-directory placeholder after a failed
/// fetch.
///
/// `reserve_target` atomically creates an empty placeholder to claim the name;
/// when the fetch is cancelled or errors before the store-export step, that
/// placeholder (or a cross-volume partial copy) is left behind. Files under the
/// user's save directory are never reclaimed by retention cleanup or entry
/// deletion, so an orphan would linger forever and force the next same-named
/// file to a ` (n)` suffix. `NotFound` is expected (export may already have
/// consumed the placeholder) and stays silent; other errors only warn — this
/// runs on an already-failed path and a second failure should not disturb it.
async fn remove_reserved_placeholder(path: &std::path::Path) {
    if let Err(err) = tokio::fs::remove_file(path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(
                error = %err,
                "materialize: failed to remove reserved placeholder after fetch failure"
            );
        }
    }
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}

fn local_file_uri_list(paths: &[PathBuf]) -> Result<String> {
    let mut out = String::new();
    for path in paths {
        let url = Url::from_file_path(path).map_err(|_| {
            anyhow!(
                "failed to convert cache path to file URL: {}",
                path.display()
            )
        })?;
        out.push_str(url.as_str());
        out.push('\n');
    }
    Ok(out)
}
