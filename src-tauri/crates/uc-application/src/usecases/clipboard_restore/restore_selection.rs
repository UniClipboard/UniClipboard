//! Reconstruct a system clipboard state from a historical entry. Files走单 rep
//! 文件分支（CF_HDROP / NSPasteboardTypeFileURL），其它内容把所有非文件候选 rep
//! （plain / html / rtf / image 等）一并打包到 `SystemClipboardSnapshot`，由平台
//! 多 rep 写入路径决定哪些能落到系统剪贴板。
//!
//! 历史背景：
//! - 早期实现只挑单一 rep（优先 plain text）写回，导致从 Word 这类富文本源恢复时
//!   RTF / HTML 全部丢失（粘贴只剩纯文本）。
//! - 之后改为多 rep 打包，但仍直接读 `rep.inline_data` 当作完整字节——这在 Staged
//!   状态下取到的是 normalizer 留下的 500-char **预览截断版**，不是完整 RTF / HTML。
//!   Word 等富文本目的地拿到的是被截断的 RTF 头，解析失败 → 粘出空文档。
//!
//! 当前实现：
//! - 用 `ClipboardPayloadResolverPort` 解析每个 rep，由 resolver 根据 `payload_state`
//!   正确路由（Inline 直读 / BlobReady 经 blob_store / Staged|Processing 走 cache+spool）。
//! - paste_rep 解析失败 → 整体报错；secondary rep 解析失败 → 跳过 + warn，不影响其他 rep。

use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use uc_core::{
    blob::ports::BlobReaderPort,
    clipboard::{
        ClipboardIntegrationMode, ObservedClipboardRepresentation, PayloadAvailability,
        PersistedClipboardRepresentation, SystemClipboardSnapshot,
    },
    ids::{EntryId, RepresentationId},
    ports::{
        clipboard::{
            ClipboardPayloadResolverPort, PayloadResolveError, ProcessingUpdateOutcome,
            ResolvedClipboardPayload,
        },
        ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
        ClipboardSelectionRepositoryPort,
    },
};

use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};

use super::file_snapshot::{build_file_snapshot, build_path_list};

pub(crate) struct RestoreClipboardSelectionUseCase {
    clipboard_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    coordinator: Arc<ClipboardWriteCoordinator>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    blob_store: Arc<dyn BlobReaderPort>,
    mode: ClipboardIntegrationMode,
}

impl RestoreClipboardSelectionUseCase {
    pub(crate) fn new(
        clipboard_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        coordinator: Arc<ClipboardWriteCoordinator>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
        blob_store: Arc<dyn BlobReaderPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            clipboard_repo,
            coordinator,
            selection_repo,
            representation_repo,
            payload_resolver,
            blob_store,
            mode,
        }
    }

    async fn build_snapshot(&self, entry_id: &EntryId) -> Result<SystemClipboardSnapshot> {
        debug!(entry_id = %entry_id, "restore.build_snapshot start");
        let entry = self
            .clipboard_repo
            .get_entry(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Entry not found"))?;

        let selection = self
            .selection_repo
            .get_selection(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Selection not found"))?;

        // 候选 rep 收集顺序：paste_rep 居首（保留"目标应用最优先粘贴"的语义），
        // 然后是 primary / preview / secondary。整体去重后传给后续打包逻辑。
        let mut candidate_ids = Vec::new();
        candidate_ids.push(selection.selection.paste_rep_id.clone());
        candidate_ids.push(selection.selection.primary_rep_id.clone());
        candidate_ids.push(selection.selection.preview_rep_id.clone());
        candidate_ids.extend(selection.selection.secondary_rep_ids.clone());

        let mut seen = HashSet::new();
        candidate_ids.retain(|rep_id| seen.insert(rep_id.clone()));

        let mut candidates = Vec::new();
        for rep_id in &candidate_ids {
            let rep = self
                .representation_repo
                .get_representation(&entry.event_id, rep_id)
                .await?;
            if let Some(rep) = rep {
                candidates.push(rep);
            } else if *rep_id == selection.selection.paste_rep_id {
                return Err(anyhow::anyhow!(
                    "Representation {} not found for event {}",
                    rep_id,
                    entry.event_id
                ));
            }
        }

        // 文件分支：paste_rep 是文件类型（CF_HDROP / NSPasteboardTypeFileURL）时，
        // 走专用的 file snapshot 路径。文件 rep 的语义与文本/图像表示不可混写在
        // 同一个 NSPasteboardItem / clipboard item 中，平台层目前也仅支持文件单独
        // 写入；同时 build_file_snapshot 会校验本地文件存在性。
        let paste_rep = candidates
            .iter()
            .find(|rep| rep.id == selection.selection.paste_rep_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Paste representation {} not found for event {}",
                    selection.selection.paste_rep_id,
                    entry.event_id
                )
            })?;

        if Self::is_file_representation(paste_rep) {
            debug!(
                entry_id = %entry_id,
                paste_rep_id = %paste_rep.id,
                "restore.build_snapshot: detected file entry, using file restore strategy"
            );
            return self.build_file_snapshot(entry_id, paste_rep).await;
        }

        // 非文件分支：把所有非文件候选 rep 都打包成多 rep snapshot，paste_rep 居首。
        // 每条 rep 通过 `ClipboardPayloadResolverPort` 取字节，由 resolver 负责按
        // payload_state 路由（Inline 直读已解密 inline_data / BlobReady 走 blob_store /
        // Staged|Processing 走 cache+spool）。
        //
        // 注意不能直接读 `rep.inline_data`：当 rep 的明文体积超过 inline_threshold
        // （默认 16KB）时，normalizer 会把它标成 Staged 并只在 inline_data 里留
        // 500 字符的 UI preview，真实字节走 spool/blob 异步物化。直接读 inline_data
        // 会拿到截断版，写到 NSPasteboard 上的 RTF / HTML 解析失败 → 粘出空。
        //
        // 平台多 rep 写入路径会原子地把所有支持的 rep 一并写入系统剪贴板：
        // - macOS: NSPasteboard.writeObjects 提交一组 NSPasteboardItem
        // - Windows: 单 OpenClipboard 会话内累加多个 CF_*
        // - Linux: 当前降级为选最优单 rep（platform 层兜底）
        // 平台层不认的 rep（私有格式等）会被静默跳过。
        let mut representations = Vec::with_capacity(candidates.len());
        let mut paste_first = true;
        let mut packed_rep_ids: Vec<RepresentationId> = Vec::new();
        for rep in &candidates {
            if Self::is_file_representation(rep) {
                debug!(
                    entry_id = %entry_id,
                    rep_id = %rep.id,
                    format_id = %rep.format_id,
                    "restore.build_snapshot: skipping file rep when paste_rep is non-file"
                );
                continue;
            }

            let is_paste_rep = rep.id == paste_rep.id;
            let bytes = match self.payload_resolver.resolve(rep).await {
                Ok(ResolvedClipboardPayload::Inline { bytes, .. }) => bytes,
                Ok(ResolvedClipboardPayload::BlobRef { blob_id, .. }) => {
                    match self.blob_store.get(&blob_id).await {
                        Ok(plaintext) => plaintext,
                        Err(err) if is_paste_rep => {
                            return Err(anyhow::anyhow!(
                                "Failed to fetch paste representation blob {}: {}",
                                blob_id,
                                err
                            ));
                        }
                        Err(err) => {
                            warn!(
                                entry_id = %entry_id,
                                rep_id = %rep.id,
                                blob_id = %blob_id,
                                error = %err,
                                "restore.build_snapshot: skipping rep, blob fetch failed"
                            );
                            continue;
                        }
                    }
                }
                Err(resolver_err) if is_paste_rep => {
                    // Active demotion: if the resolver reports an orphaned
                    // representation (cache+spool double miss), demote it to
                    // Lost so subsequent restore attempts return a stable
                    // error instead of repeatedly producing 500s.
                    if let PayloadResolveError::Orphaned {
                        rep_id: orphan_id,
                        state: orphan_state,
                    } = &resolver_err
                    {
                        self.demote_orphaned_to_lost(orphan_id, orphan_state).await;
                    }
                    let context = format!(
                        "Failed to resolve paste representation {} (state={:?})",
                        rep.id, rep.payload_state
                    );
                    return Err(anyhow::Error::new(resolver_err).context(context));
                }
                Err(err) => {
                    warn!(
                        entry_id = %entry_id,
                        rep_id = %rep.id,
                        format_id = %rep.format_id,
                        payload_state = ?rep.payload_state,
                        error = %err,
                        "restore.build_snapshot: skipping rep, resolver failed (likely Staged without cache/spool bytes)"
                    );
                    continue;
                }
            };

            let observed = ObservedClipboardRepresentation::new(
                rep.id.clone(),
                rep.format_id.clone(),
                rep.mime_type.clone(),
                bytes,
            );

            packed_rep_ids.push(rep.id.clone());
            if paste_first {
                representations.insert(0, observed);
                paste_first = false;
            } else {
                representations.push(observed);
            }
        }

        if representations.is_empty() {
            // 极端兜底：候选列表里所有 rep 都被跳过（没有 inline_data 也没有 blob_id）。
            // 上面的循环已经把 paste_rep 缺数据的情况单独 bail 掉，所以走到这里基本
            // 不会发生；但为了不交一个空 snapshot 给平台层，显式报错。
            return Err(anyhow::anyhow!(
                "No restorable representations after packing for entry {}",
                entry_id
            ));
        }

        debug!(
            entry_id = %entry_id,
            event_id = %entry.event_id,
            paste_rep_id = %paste_rep.id,
            packed_rep_count = representations.len(),
            packed_rep_ids = ?packed_rep_ids,
            total_size_bytes = representations.iter().map(|r| r.bytes.len()).sum::<usize>(),
            "restore.build_snapshot packed representations"
        );

        Ok(SystemClipboardSnapshot {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            representations,
        })
    }

    fn is_file_representation(rep: &PersistedClipboardRepresentation) -> bool {
        uc_core::clipboard::is_file_mime_or_format(rep.mime_type.as_ref(), &rep.format_id)
    }

    /// Demote an orphaned representation (cache+spool double miss) to `Lost`.
    ///
    /// Called when the resolver reports `PayloadResolveError::Orphaned` for a
    /// paste-rep. The representation can no longer be materialized — bytes are
    /// gone from both cache and spool, and the worker has no source to retry
    /// from. Marking it `Lost` ensures the next restore attempt routes to the
    /// `Lost` arm in the resolver and the facade returns a stable
    /// `PayloadUnavailable` error instead of producing 500s + Sentry events.
    ///
    /// This is best-effort: any DB failure is logged but does not propagate,
    /// because the original resolve error is what the caller actually returns.
    async fn demote_orphaned_to_lost(
        &self,
        rep_id: &RepresentationId,
        state: &PayloadAvailability,
    ) {
        let last_error = "orphaned at restore: bytes lost before blob materialization";
        match self
            .representation_repo
            .update_processing_result(
                rep_id,
                &[
                    PayloadAvailability::Staged,
                    PayloadAvailability::Processing,
                    PayloadAvailability::Failed {
                        last_error: String::new(),
                    },
                ],
                None,
                PayloadAvailability::Lost,
                Some(last_error),
            )
            .await
        {
            Ok(ProcessingUpdateOutcome::Updated(_)) => {
                info!(
                    representation_id = %rep_id,
                    payload_state = ?state,
                    "Demoted orphaned representation to Lost (cache+spool miss)"
                );
            }
            Ok(ProcessingUpdateOutcome::StateMismatch) => {
                warn!(
                    representation_id = %rep_id,
                    payload_state = ?state,
                    "Skipped Lost demotion due to state mismatch (likely already updated)"
                );
            }
            Ok(ProcessingUpdateOutcome::NotFound) => {
                warn!(
                    representation_id = %rep_id,
                    "Skipped Lost demotion: representation missing from DB"
                );
            }
            Err(err) => {
                warn!(
                    representation_id = %rep_id,
                    error = %err,
                    "Failed to demote orphaned representation to Lost"
                );
            }
        }
    }

    async fn build_file_snapshot(
        &self,
        entry_id: &EntryId,
        rep: &PersistedClipboardRepresentation,
    ) -> Result<SystemClipboardSnapshot> {
        // 与非文件分支同样走 payload_resolver：file URI list 在文件较多时同样会
        // 触发 inline_threshold，rep.inline_data 只剩 500-char 预览截断版，直接
        // clone 会拿到不完整的 URI 列表 → 文件路径解析丢失。resolver 会按
        // payload_state 正确路由（Inline / BlobReady / Staged|Processing）。
        let bytes = match self.payload_resolver.resolve(rep).await {
            Ok(ResolvedClipboardPayload::Inline { bytes, .. }) => bytes,
            Ok(ResolvedClipboardPayload::BlobRef { blob_id, .. }) => {
                self.blob_store.get(&blob_id).await?
            }
            Err(resolver_err) => {
                if let Some(blob_id) = &rep.blob_id {
                    self.blob_store.get(blob_id).await.map_err(|blob_err| {
                        anyhow::anyhow!(
                            "File URI representation resolver failed ({}) and blob fallback failed for entry {}: {}",
                            resolver_err,
                            entry_id,
                            blob_err
                        )
                    })?
                } else {
                    bail!(
                        "File URI representation has no resolvable data for entry {}: {}",
                        entry_id,
                        resolver_err
                    );
                }
            }
        };

        let uri_string = String::from_utf8(bytes)?;

        let mut file_paths = Vec::new();
        for line in uri_string.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("file://") {
                match url::Url::parse(line) {
                    Ok(url) => {
                        let path = url.to_file_path().map_err(|_| {
                            anyhow::anyhow!(
                                "Failed to convert URI to file path for entry {}: {}",
                                entry_id,
                                line
                            )
                        })?;
                        file_paths.push(path);
                    }
                    Err(e) => {
                        bail!(
                            "Failed to parse file URI for entry {}: {} (error: {})",
                            entry_id,
                            line,
                            e
                        );
                    }
                }
            } else {
                file_paths.push(PathBuf::from(line));
            }
        }

        if file_paths.is_empty() {
            bail!("No valid file paths found in entry {}", entry_id);
        }

        for path in &file_paths {
            if !path.exists() {
                bail!("File deleted: {}", path.display());
            }
        }

        let snapshot = build_file_snapshot(&build_path_list(&file_paths));

        info!(
            entry_id = %entry_id,
            file_count = file_paths.len(),
            "restore.build_file_snapshot: files validated and snapshot built"
        );

        Ok(snapshot)
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        info!(entry_id = %entry_id, "restore.execute requested");
        if !self.mode.allow_os_write() {
            return Err(anyhow::anyhow!(
                "System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"
            ));
        }
        let snapshot = self.build_snapshot(entry_id).await?;
        self.coordinator
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await
    }
}
