//! Clipboard representation normalizer with owned config
//! 带有拥有所有权的配置的剪贴板表示规范化器

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::trace;

use crate::config::clipboard_storage_config::ClipboardStorageConfig;
use uc_core::clipboard::{
    MimeType, ObservedClipboardRepresentation, PayloadAvailability,
    PersistedClipboardRepresentation,
};
use uc_core::ports::clipboard::ClipboardRepresentationNormalizerPort;

const PREVIEW_LENGTH_CHARS: usize = 500;

/// Check if MIME type is text-based
/// 检查 MIME 类型是否为文本类型
pub(crate) fn is_text_mime_type(mime_type: &Option<MimeType>) -> bool {
    match mime_type {
        None => false,
        Some(mt) => {
            let mt_str = mt.as_str();
            mt_str.starts_with("text/")
                || mt_str == "text/plain"
                || mt_str.contains("json")
                || mt_str.contains("xml")
                || mt_str.contains("javascript")
                || mt_str.contains("html")
                || mt_str.contains("css")
        }
    }
}

/// UTF-8 safe truncation to first N characters
/// UTF-8 安全截断到前 N 个字符
pub(crate) fn truncate_to_preview(bytes: &[u8]) -> Vec<u8> {
    // UTF-8 safe truncation to first N characters
    std::str::from_utf8(bytes)
        .map(|text| {
            text.chars()
                .take(PREVIEW_LENGTH_CHARS)
                .collect::<String>()
                .into_bytes()
        })
        .unwrap_or_else(|_| {
            // Fallback for invalid UTF-8: truncate bytes
            bytes[..bytes.len().min(PREVIEW_LENGTH_CHARS)].to_vec()
        })
}

/// Clipboard representation normalizer with owned config
/// 带有拥有所有权的配置的剪贴板表示规范化器
///
/// Valid states (per database CHECK constraint after migration 2026-01-18-000001):
/// 1. inline_data = Some(payload), blob_id = None, payload_state = Inline
///    -> inline payload (small content)
/// 2. inline_data = Some(preview), blob_id = None, payload_state = Staged
///    -> staged payload with inline preview (large text content)
/// 3. inline_data = None, blob_id = None, payload_state = Staged
///    -> staged payload without preview (large non-text content)
///
/// Note: CHECK (inline_data IS NULL OR blob_id IS NULL) means blob materialization
/// must clear inline_data when blob_id is set.
pub struct ClipboardRepresentationNormalizer {
    config: Arc<ClipboardStorageConfig>,
}

impl ClipboardRepresentationNormalizer {
    /// Create a new normalizer with the given config
    /// 使用给定配置创建新规范化器
    pub fn new(config: Arc<ClipboardStorageConfig>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ClipboardRepresentationNormalizerPort for ClipboardRepresentationNormalizer {
    async fn normalize(
        &self,
        observed: &ObservedClipboardRepresentation,
    ) -> Result<PersistedClipboardRepresentation> {
        let inline_threshold_bytes = self.config.inline_threshold_bytes;
        let size_bytes = observed.size_bytes();
        // Normalizer 仅处理 Inline source rep —— LocalFile rep 必须在更上游的 capture
        // 流水线里通过 BlobWriterPort.write_path_if_absent 物化到 blob 仓库,直接产出
        // BlobReady 状态的 PersistedClipboardRepresentation,不应进入此路径。
        let bytes = observed.expect_inline_bytes();

        // Decision: inline, preview, or staged for blob materialization
        // 决策：内联、预览还是为 blob 物化创建暂存状态
        if size_bytes <= inline_threshold_bytes {
            // Small content: store full data inline
            trace!(
                representation_id = %observed.id,
                format_id = %observed.format_id,
                size_bytes,
                threshold = inline_threshold_bytes,
                strategy = "inline",
                "Normalizing small content inline"
            );
            Ok(PersistedClipboardRepresentation::new(
                observed.id.clone(),
                observed.format_id.clone(),
                observed.mime.clone(),
                size_bytes,
                Some(bytes.to_vec()),
                None, // blob_id
            ))
        } else {
            // Large content: decide based on type
            if is_text_mime_type(&observed.mime) {
                // Text type: keep a 500-char inline preview but mark as staged so
                // background worker can materialize full payload into blob storage.
                trace!(
                    representation_id = %observed.id,
                    format_id = %observed.format_id,
                    size_bytes,
                    threshold = inline_threshold_bytes,
                    preview_length_chars = PREVIEW_LENGTH_CHARS,
                    strategy = "staged_with_preview",
                    "Normalizing large text as staged with inline preview"
                );
                PersistedClipboardRepresentation::new_with_state(
                    observed.id.clone(),
                    observed.format_id.clone(),
                    observed.mime.clone(),
                    size_bytes,
                    Some(truncate_to_preview(bytes)),
                    None, // blob_id
                    PayloadAvailability::Staged,
                    None,
                )
            } else {
                // Non-text (images, etc.): create staged representation for blob materialization
                trace!(
                    representation_id = %observed.id,
                    format_id = %observed.format_id,
                    size_bytes,
                    threshold = inline_threshold_bytes,
                    strategy = "staged",
                    "Normalizing large non-text as staged (blob materialization pending)"
                );
                Ok(PersistedClipboardRepresentation::new_staged(
                    observed.id.clone(),
                    observed.format_id.clone(),
                    observed.mime.clone(),
                    size_bytes,
                ))
            }
        }
    }
}
