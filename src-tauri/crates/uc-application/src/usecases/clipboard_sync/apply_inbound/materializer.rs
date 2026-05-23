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

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{debug, info, warn};
use url::Url;

use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

use crate::facade::blob_transfer::{
    BatchPosition, BlobTransferFacade, FetchBlobCommand, FetchBlobResult, FetchBlobToPathCommand,
    FetchBlobToPathResult, FetchTransferContext,
};
use crate::usecases::clipboard_sync::payload_codec::V3BlobRef;

#[async_trait]
pub trait InboundBlobMaterializer: Send + Sync {
    /// `receiver_entry_id` 是 ApplyInbound 在流程入口生成的接收端 entry_id,
    /// 用作所有 blob 拉取的 transfer_id —— 让占位卡片、进度事件和最终
    /// `NewContent` 共享同一个标识,前端无需做合并映射。
    async fn materialize(
        &self,
        from_device: DeviceId,
        receiver_entry_id: EntryId,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
    ) -> Result<SystemClipboardSnapshot>;
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
}

#[async_trait]
impl InboundBlobFetcher for BlobTransferFacade {
    async fn fetch_blob(&self, command: FetchBlobCommand) -> Result<FetchBlobResult> {
        BlobTransferFacade::fetch_blob(self, command)
            .await
            .map_err(|e| anyhow!(e.to_string()))
    }

    async fn fetch_blob_to_path(
        &self,
        command: FetchBlobToPathCommand,
    ) -> Result<FetchBlobToPathResult> {
        BlobTransferFacade::fetch_blob_to_path(self, command)
            .await
            .map_err(|e| anyhow!(e.to_string()))
    }
}

pub struct FileCacheBlobMaterializer {
    fetcher: Arc<dyn InboundBlobFetcher>,
    cache_dir: PathBuf,
}

impl FileCacheBlobMaterializer {
    pub fn new(fetcher: Arc<dyn InboundBlobFetcher>, cache_dir: PathBuf) -> Self {
        Self { fetcher, cache_dir }
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
    ) -> Result<SystemClipboardSnapshot> {
        if blob_refs.is_empty() {
            return Ok(snapshot);
        }

        // Split blob refs by destination:
        //   - `representation_index = Some(i)`: bytes belong to envelope rep i
        //     (image / large binary path). Fetched bytes are written back into
        //     `snapshot.representations[i]` so the rep round-trips with full
        //     content; receiver does NOT spill these to disk.
        //   - `representation_index = None`: free-standing file (legacy
        //     file-URI path). Fetched bytes go to cache_dir, file-list rep is
        //     rewritten with local `file://` URIs.
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

        // 1. Hydrate representation-bound blobs back into the snapshot.
        for blob_ref in rep_refs {
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
                peer_id: from_device.as_str().to_string(),
                total_bytes: Some(advertised_size),
                filename: String::new(),
                outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                outbound_target: Some(from_device.clone()),
                batch_position: position_in_batch(batch_idx, batch_total),
            };
            batch_idx += 1;
            let fetched = self
                .fetcher
                .fetch_blob(FetchBlobCommand {
                    ticket: blob_ref.ticket,
                    entry_id: blob_ref.entry_id.clone(),
                    transfer_context: Some(transfer_context),
                })
                .await
                .map_err(|e| {
                    warn!(
                        entry_id = %entry_id,
                        size_bytes = advertised_size,
                        representation_index = idx,
                        error = %e,
                        "materialize: representation-bound blob fetch failed"
                    );
                    e
                })?;

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

        if file_refs.is_empty() {
            return Ok(snapshot);
        }

        // 2. Free-standing files: existing cache_dir + file-list rewrite path.
        let mut local_paths = Vec::with_capacity(file_refs.len());
        let mut used_names = HashSet::new();
        let blob_ref_total = file_refs.len();

        for (idx, blob_ref) in file_refs.into_iter().enumerate() {
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
                peer_id: from_device.as_str().to_string(),
                total_bytes: Some(advertised_size),
                filename: declared_name.clone().unwrap_or_default(),
                outbound_transfer_id: Some(blob_ref.entry_id.as_ref().to_string()),
                outbound_target: Some(from_device.clone()),
                batch_position: position_in_batch(batch_idx, batch_total),
            };
            batch_idx += 1;

            // GH#487 Phase 2: pre-create cache dir and stream the blob
            // directly to the target file. The previous code did
            // `fetch_blob -> Bytes -> tokio::fs::write`, which on a 800 MB
            // transfer wasted ~20s materialising the full plaintext in
            // memory and writing to disk a second time (the iroh store
            // already had a copy from BAO verification). `fetch_blob_to_path`
            // collapses both into a single `Blobs::export` call (reflink
            // on APFS / Btrfs / ReFS).
            let entry_dir = self
                .cache_dir
                .join("iroh-blobs")
                .join(sanitize_path_segment(blob_ref.entry_id.as_ref()));
            tokio::fs::create_dir_all(&entry_dir).await?;

            let filename = unique_filename(blob_ref.filename.as_deref(), idx, &mut used_names);
            let path = entry_dir.join(filename);

            let fetched = self
                .fetcher
                .fetch_blob_to_path(FetchBlobToPathCommand {
                    ticket: blob_ref.ticket,
                    entry_id: blob_ref.entry_id.clone(),
                    target_path: path.clone(),
                    transfer_context: Some(transfer_context),
                })
                .await
                .map_err(|e| {
                    warn!(
                        idx,
                        total = blob_ref_total,
                        entry_id = %entry_id,
                        size_bytes = advertised_size,
                        error = %e,
                        "materialize: blob fetch failed"
                    );
                    e
                })?;

            info!(
                idx,
                total = blob_ref_total,
                entry_id = %entry_id,
                bytes_written = fetched.bytes_written,
                path = %path.display(),
                "materialize: blob cached to local path (streaming)"
            );
            local_paths.push(path);
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

        Ok(snapshot)
    }
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
