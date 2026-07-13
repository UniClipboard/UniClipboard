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

use uc_core::clipboard::{
    ContentHash, EntryFileSet, EntryFileSetLine, EntryFileSetLineKind, FileSetMemberKind,
    FileSetMemberLocation, HashAlgorithm,
};
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::inbound_file_target::ReserveInboundFileTargetPort;
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
}

impl MaterializeResult {
    pub fn complete(snapshot: SystemClipboardSnapshot) -> Self {
        Self {
            snapshot,
            missing: Vec::new(),
            partial: false,
        }
    }

    pub fn is_partial(&self) -> bool {
        self.partial
    }
}

#[async_trait]
pub trait InboundBlobMaterializer: Send + Sync {
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
    ) -> Result<MaterializeResult>;
}

#[async_trait]
pub trait InboundBlobFetcher: Send + Sync {
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

#[async_trait]
pub(crate) trait RootRenamer: Send + Sync {
    async fn rename(
        &self,
        source: &std::path::Path,
        destination: &std::path::Path,
    ) -> std::io::Result<()>;
}

struct TokioRootRenamer;

#[async_trait]
impl RootRenamer for TokioRootRenamer {
    async fn rename(
        &self,
        source: &std::path::Path,
        destination: &std::path::Path,
    ) -> std::io::Result<()> {
        tokio::fs::rename(source, destination).await
    }
}

#[async_trait]
impl InboundBlobFetcher for BlobTransferFacade {
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

pub struct FileCacheBlobMaterializer {
    fetcher: Arc<dyn InboundBlobFetcher>,
    cache_dir: PathBuf,
    target_reserver: Option<Arc<dyn ReserveInboundFileTargetPort>>,
    root_renamer: Arc<dyn RootRenamer>,
}

impl FileCacheBlobMaterializer {
    pub fn new(fetcher: Arc<dyn InboundBlobFetcher>, cache_dir: PathBuf) -> Self {
        Self {
            fetcher,
            cache_dir,
            target_reserver: None,
            root_renamer: Arc::new(TokioRootRenamer),
        }
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

    #[cfg(test)]
    pub(crate) fn with_root_renamer(mut self, renamer: Arc<dyn RootRenamer>) -> Self {
        self.root_renamer = renamer;
        self
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

        // 1. Hydrate representation-bound blobs back into the snapshot.
        //
        // Pre-collect rep idx 列表,break 后能把"还没跑到"的 rep 也算进
        // incomplete —— 否则它们留在 snapshot 里就是带空 bytes 的占位 rep,
        // capture 后渲染会失败。
        let pending_rep_idxs: Vec<usize> = rep_refs
            .iter()
            .map(|r| r.representation_index.expect("partition guarantees Some") as usize)
            .collect();
        for (loop_idx, blob_ref) in rep_refs.into_iter().enumerate() {
            let entry_id = blob_ref.entry_id.clone();
            let advertised_size = blob_ref.size_bytes;
            let idx = blob_ref
                .representation_index
                .expect("partition guarantees Some");
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
            let transfer_context = FetchTransferContext {
                transfer_id: receiver_entry_id.as_ref().to_string(),
                entry_id: receiver_entry_id.as_ref().to_string(),
                peer_id: from_device.as_str().to_string(),
                total_bytes: Some(advertised_size),
                filename: String::new(),
                outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                outbound_target: Some(from_device.clone()),
                batch_position: position_in_batch(batch_idx, batch_total),
                individual_lifecycle: false,
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
            ));
        }

        if file_refs.is_empty() {
            if file_set_manifest.is_none() {
                return Ok(MaterializeResult::complete(snapshot));
            }
        }

        if let Some(manifest) = file_set_manifest {
            return self
                .materialize_directory(
                    from_device,
                    receiver_entry_id,
                    snapshot,
                    manifest_blob_refs.unwrap_or_default(),
                    manifest,
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
            let transfer_context = FetchTransferContext {
                transfer_id: receiver_entry_id.as_ref().to_string(),
                entry_id: receiver_entry_id.as_ref().to_string(),
                peer_id: from_device.as_str().to_string(),
                total_bytes: Some(advertised_size),
                filename: declared_name.clone().unwrap_or_default(),
                outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                outbound_target: Some(from_device.clone()),
                batch_position: position_in_batch(batch_idx, batch_total),
                individual_lifecycle: false,
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
                path = %path.display(),
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
                    path = %path.display(),
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

        Ok(MaterializeResult::complete(snapshot))
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
        mut batch_idx: usize,
        batch_total: usize,
    ) -> Result<MaterializeResult> {
        if manifest.members.is_empty() {
            return Err(anyhow!("directory manifest has no members"));
        }

        let staging_base = self
            .cache_dir
            .join("iroh-blobs")
            .join("staging")
            .join(sanitize_path_segment(receiver_entry_id.as_ref()));
        if tokio::fs::try_exists(&staging_base).await? {
            tokio::fs::remove_dir_all(&staging_base).await?;
        }
        tokio::fs::create_dir_all(&staging_base).await?;

        let result = self
            .build_and_promote_directory(
                &from_device,
                &receiver_entry_id,
                &blob_refs,
                &manifest,
                &staging_base,
                &mut batch_idx,
                batch_total,
            )
            .await;

        match result {
            Ok((root_paths, member_digests)) => {
                remove_directory_and_empty_parents(&staging_base, &self.cache_dir).await;
                snapshot.file_content_digests.clear();
                snapshot.file_set_v1_component =
                    Some(compute_file_set_component(&manifest, &member_digests)?);
                let uri_list = local_file_uri_list(&root_paths)?;
                rewrite_file_list(&mut snapshot, uri_list, root_paths.len())?;
                Ok(MaterializeResult::complete(snapshot))
            }
            Err(err) => {
                remove_directory_and_empty_parents(&staging_base, &self.cache_dir).await;
                Err(err)
            }
        }
    }

    async fn build_and_promote_directory(
        &self,
        from_device: &DeviceId,
        receiver_entry_id: &EntryId,
        blob_refs: &[V3BlobRef],
        manifest: &InboundFileSetManifest,
        staging_base: &std::path::Path,
        batch_idx: &mut usize,
        batch_total: usize,
    ) -> Result<(Vec<PathBuf>, BTreeMap<u32, [u8; 32]>)> {
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

        let mut member_digests = BTreeMap::new();
        for (member_index, member) in manifest.members.iter().enumerate() {
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
                    let context = FetchTransferContext {
                        transfer_id: directory_member_transfer_id(receiver_entry_id, member_index),
                        entry_id: receiver_entry_id.as_ref().to_string(),
                        peer_id: from_device.as_str().to_string(),
                        total_bytes: Some(blob_ref.size_bytes),
                        filename: blob_ref.filename.clone().unwrap_or_default(),
                        outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                        outbound_target: Some(from_device.clone()),
                        batch_position: position_in_batch(*batch_idx, batch_total),
                        individual_lifecycle: true,
                    };
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
                                        FetchTransferContext {
                                            transfer_id: directory_member_transfer_id(
                                                receiver_entry_id,
                                                remaining_index,
                                            ),
                                            entry_id: receiver_entry_id.as_ref().to_string(),
                                            peer_id: from_device.as_str().to_string(),
                                            total_bytes: Some(remaining_blob.size_bytes),
                                            filename: remaining_blob
                                                .filename
                                                .clone()
                                                .unwrap_or_default(),
                                            outbound_transfer_id: Some(
                                                remaining_blob.entry_id.as_ref().to_string(),
                                            ),
                                            outbound_target: Some(*from_device),
                                            batch_position: BatchPosition::Middle,
                                            individual_lifecycle: true,
                                        },
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
        }

        let managed_parent = self
            .cache_dir
            .join("iroh-blobs")
            .join(sanitize_path_segment(receiver_entry_id.as_ref()));
        tokio::fs::create_dir_all(&managed_parent).await?;
        let mut destinations = Vec::new();
        let mut reserved_destinations = HashSet::new();
        for (root_index, (root_name, _)) in &roots {
            let reserved = match &self.target_reserver {
                Some(reserver) => reserver.reserve_target(root_name).await,
                None => None,
            };
            let destination = match reserved {
                Some(path) => path,
                None => {
                    resolve_nonconflicting_root_path(
                        &managed_parent,
                        root_name,
                        &reserved_destinations,
                    )
                    .await?
                }
            };
            if !reserved_destinations.insert(destination.clone()) {
                return Err(anyhow!(
                    "directory target reserver returned a duplicate path: {}",
                    destination.display()
                ));
            }
            destinations.push((*root_index, root_name.clone(), destination));
        }

        for (_, _, destination) in &destinations {
            if let Ok(metadata) = tokio::fs::metadata(destination).await {
                if metadata.is_file() {
                    tokio::fs::remove_file(destination).await?;
                }
            }
            if tokio::fs::try_exists(destination).await? {
                return Err(anyhow!(
                    "directory destination already exists: {}",
                    destination.display()
                ));
            }
        }

        let mut promoted: Vec<PathBuf> = Vec::new();
        for (root_index, root_name, destination) in &destinations {
            let source = staging_root(staging_base, *root_index, root_name);
            if let Err(err) = promote_root(&source, destination, self.root_renamer.as_ref()).await {
                for path in promoted.iter().rev() {
                    remove_path(path).await?;
                }
                return Err(err);
            }
            promoted.push(destination.clone());
        }
        Ok((promoted, member_digests))
    }
}

fn directory_member_transfer_id(receiver_entry_id: &EntryId, member_index: usize) -> String {
    format!("{}:member:{member_index}", receiver_entry_id.as_ref())
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

async fn resolve_nonconflicting_root_path(
    parent: &std::path::Path,
    desired_name: &str,
    reserved: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    let desired = parent.join(desired_name);
    if !reserved.contains(&desired) && !tokio::fs::try_exists(&desired).await? {
        return Ok(desired);
    }
    let mut suffix = 2u32;
    loop {
        let candidate = parent.join(format!("{desired_name} ({suffix})"));
        if !reserved.contains(&candidate) && !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
        suffix = suffix
            .checked_add(1)
            .ok_or_else(|| anyhow!("directory collision suffix space exhausted"))?;
    }
}

async fn promote_root(
    source: &std::path::Path,
    destination: &std::path::Path,
    renamer: &dyn RootRenamer,
) -> Result<()> {
    match renamer.rename(source, destination).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let parent = destination
                .parent()
                .ok_or_else(|| anyhow!("directory destination has no parent"))?;
            let file_name = destination
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("directory destination name is not valid UTF-8"))?;
            let hidden = parent.join(format!(".uniclip-staging-{file_name}"));
            if tokio::fs::try_exists(&hidden).await? {
                remove_path(&hidden).await?;
            }
            let source = source.to_path_buf();
            let source_copy = source.clone();
            let hidden_copy = hidden.clone();
            tokio::task::spawn_blocking(move || copy_root(&source_copy, &hidden_copy))
                .await
                .map_err(|err| anyhow!("directory copy task failed: {err}"))??;
            if let Err(err) = renamer.rename(&hidden, destination).await {
                let cleanup_error = remove_path(&hidden).await.err();
                return Err(anyhow!(
                    "directory promotion failed after rename error ({rename_error}): {err}; cleanup error: {cleanup_error:?}"
                ));
            }
            remove_path(&source).await?;
            Ok(())
        }
    }
}

fn copy_root(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    let metadata = std::fs::metadata(source)?;
    if metadata.is_dir() {
        return copy_directory_tree(source, destination);
    }
    if metadata.is_file() {
        let copied = std::fs::copy(source, destination)?;
        if copied != metadata.len() {
            return Err(anyhow!(
                "copied byte count mismatch for {}",
                source.display()
            ));
        }
        std::fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }
    Err(anyhow!(
        "staging root has an unsupported type: {}",
        source.display()
    ))
}

async fn remove_path(path: &std::path::Path) -> Result<()> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_dir() => {
            tokio::fs::remove_dir_all(path).await?;
        }
        Ok(_) => {
            tokio::fs::remove_file(path).await?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn copy_directory_tree(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_directory_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            let copied = std::fs::copy(&source_path, &destination_path)?;
            if copied != metadata.len() {
                return Err(anyhow!(
                    "copied byte count mismatch for {}",
                    source_path.display()
                ));
            }
            std::fs::set_permissions(&destination_path, metadata.permissions())?;
        } else {
            return Err(anyhow!(
                "staging tree contains an unsupported member: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
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

async fn remove_directory_and_empty_parents(path: &std::path::Path, stop: &std::path::Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == stop {
            break;
        }
        if tokio::fs::remove_dir(parent).await.is_err() {
            break;
        }
        current = parent.parent();
    }
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
                path = %path.display(),
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
