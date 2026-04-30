use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;
use tracing::{info, warn};
use uc_core::ids::EntryId;
use uc_core::ports::SettingsPort;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

use crate::facade::{
    BlobTransferFacade, ClipboardSyncFacade, PublishBlobCommand, PublishBlobPathCommand,
};
use crate::sync_planner::{FileCandidate, FileSyncIntent, OutboundSyncPlanner};
use crate::V3BlobRef;

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

pub struct ClipboardOutboundDeps {
    pub settings: Arc<dyn SettingsPort>,
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    pub blob_transfer: Arc<BlobTransferFacade>,
}

pub struct ClipboardOutboundDispatcher {
    settings: Arc<dyn SettingsPort>,
    clipboard_sync: Arc<ClipboardSyncFacade>,
    blob_transfer: Arc<BlobTransferFacade>,
}

impl ClipboardOutboundDispatcher {
    pub fn new(deps: ClipboardOutboundDeps) -> Self {
        Self {
            settings: deps.settings,
            clipboard_sync: deps.clipboard_sync,
            blob_transfer: deps.blob_transfer,
        }
    }
}

#[async_trait]
impl ClipboardOutboundPort for ClipboardOutboundDispatcher {
    async fn dispatch_capture(
        &self,
        input: ClipboardOutboundInput,
    ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError> {
        if input.origin == ClipboardChangeOrigin::RemotePush {
            return Ok(ClipboardOutboundOutcome::Skipped {
                reason: "remote_push_echo".to_string(),
            });
        }

        // Phase timing: dispatch_capture 是 outbound 关键路径,从 capture
        // 完成到 dispatch 之间任何阶段卡顿都会让 UI 看起来"复制后没动静"。
        // 拆分阶段计时是为了在用户报"复制后很久才同步"这类问题时,能快速
        // 区分卡在 metadata / plan / publish_files / publish_inline / dispatch
        // 哪一段。详见 GH#487。
        let entry_id_str = input.entry_id.clone();
        let snapshot_rep_count = input.snapshot.representations.len();
        let dispatch_start = Instant::now();

        let resolved_paths = if input.origin == ClipboardChangeOrigin::LocalCapture {
            extract_file_paths_from_snapshot(&input.snapshot)
        } else {
            Vec::new()
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
            file_candidate_count = plan.files.len(),
            total_file_bytes = total_file_metadata_bytes,
            metadata_ms,
            plan_ms,
            "outbound: dispatch_capture entering publish phase"
        );

        let entry_id = EntryId::from(input.entry_id.as_str());

        let publish_files_start = Instant::now();
        let mut blob_refs =
            publish_file_blob_refs(&self.blob_transfer, &plan.files, &entry_id).await?;
        let publish_files_ms = publish_files_start.elapsed().as_millis() as u64;

        let publish_inline_start = Instant::now();
        let mut image_blob_refs = publish_oversized_inline_blob_refs(
            &self.blob_transfer,
            &mut clipboard_intent.snapshot,
            &entry_id,
        )
        .await?;
        let publish_inline_ms = publish_inline_start.elapsed().as_millis() as u64;

        blob_refs.append(&mut image_blob_refs);
        let blob_ref_count = blob_refs.len();

        let dispatch_phase_start = Instant::now();
        let dispatch_result = if blob_refs.is_empty() {
            self.clipboard_sync
                .dispatch_snapshot(clipboard_intent.snapshot, input.origin)
                .await
        } else {
            self.clipboard_sync
                .dispatch_snapshot_with_blob_refs(
                    clipboard_intent.snapshot,
                    blob_refs,
                    input.origin,
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
            "outbound: dispatch_capture completed"
        );

        Ok(ClipboardOutboundOutcome::Dispatched {
            accepted: dispatch_result.total_accepted,
            duplicate: dispatch_result.total_duplicate,
            offline: dispatch_result.total_offline,
            errored: dispatch_result.total_errored,
            blob_ref_count,
        })
    }
}

pub struct ClipboardOutboundFacade {
    dispatcher: Arc<dyn ClipboardOutboundPort>,
}

impl ClipboardOutboundFacade {
    pub fn new(dispatcher: Arc<dyn ClipboardOutboundPort>) -> Self {
        Self { dispatcher }
    }

    pub async fn dispatch_capture(
        &self,
        input: ClipboardOutboundInput,
    ) -> Result<ClipboardOutboundOutcome, ClipboardOutboundError> {
        self.dispatcher.dispatch_capture(input).await
    }
}

#[cfg(target_os = "macos")]
fn resolve_apfs_file_reference(_path: &Path) -> Option<PathBuf> {
    None
}

fn extract_file_paths_from_snapshot(snapshot: &SystemClipboardSnapshot) -> Vec<PathBuf> {
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

        let text = match std::str::from_utf8(&rep.bytes) {
            Ok(text) => text,
            Err(_) => continue,
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Ok(url) = url::Url::parse(line) {
                if url.scheme() == "file" {
                    if let Ok(path) = url.to_file_path() {
                        #[cfg(target_os = "macos")]
                        let resolved = resolve_apfs_file_reference(&path).unwrap_or(path);
                        #[cfg(not(target_os = "macos"))]
                        let resolved = path;
                        paths.push(resolved);
                    }
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// 出向 dispatch 时单条 inline rep 在 envelope 主体里的最大 bytes 数。超过
/// 该值的 image-类 rep 会被剥出来走 blob 通道（receiver 通过 `representation_index`
/// 把 fetched bytes 灌回原 rep），避免撞 wire 层 `MAX_PAYLOAD_SIZE = 2 MiB` 上限。
///
/// 1 MiB 给 envelope 内其他 reps、V3 头、加密 AEAD overhead 留出充足余量；
/// 大多数日常截图（< 1 MiB PNG）仍走 inline 快路径。
const OVERSIZED_REP_THRESHOLD_BYTES: usize = 1024 * 1024;

/// 把 snapshot 中超过 `OVERSIZED_REP_THRESHOLD_BYTES` 的 image-类 rep 上传到
/// blob store，把它们的 `bytes` 字段就地清空（保留 `format_id` / `mime` / `id`），
/// 返回携带 `representation_index` 的 V3BlobRef 列表。
///
/// receiver 端的 `InboundBlobMaterializer` 看到 `representation_index = Some(i)`
/// 时把 fetched bytes 灌回 `representations[i]`，而不是当成独立 file 落到 cache。
///
/// **重要细节**：在清空 `bytes` 之前显式调用一次 `content_hash()`，强制把原内容
/// 哈希写入 OnceLock 缓存，这样 envelope 编码阶段的 `snapshot.snapshot_hash()`
/// 仍反映真实图片内容（receiver 端解码后会拿到一致的 content_hash 用于 dedup）。
///
/// 仅对 `mime` 以 `image/` 开头的 rep 生效。其它类型的大 rep 暂保持 inline；
/// 后续若有非 image 大 rep 撞上限，会在此处扩展并补对应的 receiver 处理。
async fn publish_oversized_inline_blob_refs(
    blob_transfer: &BlobTransferFacade,
    snapshot: &mut SystemClipboardSnapshot,
    entry_id: &EntryId,
) -> Result<Vec<V3BlobRef>, ClipboardOutboundError> {
    let mut blob_refs = Vec::new();

    for (idx, rep) in snapshot.representations.iter_mut().enumerate() {
        if rep.bytes.len() <= OVERSIZED_REP_THRESHOLD_BYTES {
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

        let size_bytes = rep.bytes.len() as u64;
        let plaintext = std::mem::take(&mut rep.bytes);

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

async fn publish_file_blob_refs(
    blob_transfer: &BlobTransferFacade,
    files: &[FileSyncIntent],
    entry_id: &EntryId,
) -> Result<Vec<V3BlobRef>, ClipboardOutboundError> {
    let mut blob_refs = Vec::with_capacity(files.len());

    for file in files {
        // GH#487 P1: 流式 publish。旧路径先 `tokio::fs::read` 把整个文件读到
        // `Vec<u8>`、再 `Bytes::from` 拷贝、再 `add_bytes` 在内存里算 BAO,
        // 1GB 文件 RSS 峰值 ≈ 2GB,且这三步全部串联完成才轮到 dispatch ——
        // 对端因此要等 ~11s 才拿到 envelope。新路径走 iroh-blobs `add_path`,
        // 内部 reflink_or_copy_with_progress 把磁盘文件 stream 到 store(CoW
        // FS 上零拷贝)+ 增量 BAO 编码,内存峰值与文件大小无关。`size_bytes`
        // 改用 plan 透传的 `FileSyncIntent.size`(metadata 阶段已查过)。
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

        blob_refs.push(V3BlobRef {
            ticket: result.ticket,
            entry_id: result.entry_id,
            filename: Some(file.filename.clone()).filter(|name| !name.is_empty()),
            mime: None,
            size_bytes: file.size,
            representation_index: None,
        });
    }

    Ok(blob_refs)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                blob_ref_count: 0,
            })
        }
    }

    #[tokio::test]
    async fn dispatch_capture_accepts_application_entry_id() {
        let facade = ClipboardOutboundFacade::new(Arc::new(FakeOutbound));
        let outcome = facade
            .dispatch_capture(ClipboardOutboundInput {
                entry_id: "entry-a".to_string(),
                snapshot: SystemClipboardSnapshot {
                    representations: Vec::new(),
                    ts_ms: 0,
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
                blob_ref_count: 0,
            }
        );
    }
}
