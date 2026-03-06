//! Clipboard representation normalizer with owned config
//! 带有拥有所有权的配置的剪贴板表示规范化器

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

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
        let size_bytes = observed.bytes.len() as i64;

        // Decision: inline, preview, or staged for blob materialization
        // 决策：内联、预览还是为 blob 物化创建暂存状态
        if size_bytes <= inline_threshold_bytes {
            // Small content: store full data inline
            debug!(
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
                Some(observed.bytes.clone()),
                None, // blob_id
            ))
        } else {
            // Large content: decide based on type
            if is_text_mime_type(&observed.mime) {
                // Text type: keep a 500-char inline preview but mark as staged so
                // background worker can materialize full payload into blob storage.
                debug!(
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
                    Some(truncate_to_preview(&observed.bytes)),
                    None, // blob_id
                    PayloadAvailability::Staged,
                    None,
                )
            } else {
                // Non-text (images, etc.): create staged representation for blob materialization
                debug!(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_is_text_mime_type_with_text_plain() {
        assert!(is_text_mime_type(&Some(MimeType::text_plain())));
    }

    #[test]
    fn test_is_text_mime_type_with_json() {
        assert!(is_text_mime_type(&Some(
            MimeType::from_str("application/json").unwrap()
        )));
    }

    #[test]
    fn test_is_text_mime_type_with_image() {
        assert!(!is_text_mime_type(&Some(
            MimeType::from_str("image/png").unwrap()
        )));
    }

    #[test]
    fn test_is_text_mime_type_with_none() {
        assert!(!is_text_mime_type(&None));
    }

    // truncate_to_preview tests
    #[test]
    fn test_truncate_to_preview_ascii() {
        let input = b"h".repeat(5000); // 5000 bytes
        let result = truncate_to_preview(&input);
        assert_eq!(result.len(), 500); // 500 chars (ASCII)
        assert_eq!(String::from_utf8_lossy(&result), "h".repeat(500));
    }

    #[test]
    fn test_truncate_to_preview_utf8() {
        // Chinese characters are 3 bytes each in UTF-8
        let input = "你".repeat(1000).as_bytes().to_vec(); // 3000 bytes
        let result = truncate_to_preview(&input);
        assert_eq!(String::from_utf8_lossy(&result), "你".repeat(500)); // 500 chars = 500 * 3 = 1500 bytes
        assert_eq!(result.len(), 1500); // 500 chars * 3 bytes each
    }

    #[test]
    fn test_truncate_to_preview_shorter_than_limit() {
        let input = b"short";
        let result = truncate_to_preview(input);
        assert_eq!(result, b"short");
    }

    #[test]
    fn test_truncate_to_preview_invalid_utf8() {
        let input = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
        let result = truncate_to_preview(&input);
        // Fallback to byte truncation
        assert_eq!(result.len(), 3);
    }

    // Normalizer integration tests
    use uc_core::clipboard::PayloadAvailability;
    use uc_core::ids::{FormatId, RepresentationId};

    #[tokio::test]
    async fn test_normalizer_creates_staged_for_large_content() {
        // Large image content (> inline_threshold)
        let large_image_data = vec![0u8; 20 * 1024]; // 20 KB > 16 KB threshold
        let config = Arc::new(ClipboardStorageConfig {
            inline_threshold_bytes: 16 * 1024, // 16 KB
            ..ClipboardStorageConfig::defaults()
        });
        let normalizer = ClipboardRepresentationNormalizer::new(config);

        let observed = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.png"),
            Some(MimeType::from_str("image/png").unwrap()),
            large_image_data,
        );

        let result = normalizer.normalize(&observed).await.unwrap();

        // Verify Staged state: no inline data, no blob_id
        assert_eq!(
            result.payload_state,
            PayloadAvailability::Staged,
            "Large non-text content should have Staged state"
        );
        assert_eq!(
            result.inline_data, None,
            "Staged representation should have inline_data = None"
        );
        assert_eq!(
            result.blob_id, None,
            "Staged representation should have blob_id = None"
        );
        assert_eq!(
            result.size_bytes,
            20 * 1024,
            "Size should reflect original content size"
        );
    }

    #[tokio::test]
    async fn test_normalizer_creates_inline_for_small_content() {
        // Small text content (< inline_threshold)
        let small_text_data = b"Hello, world!".to_vec();
        let config = Arc::new(ClipboardStorageConfig {
            inline_threshold_bytes: 16 * 1024, // 16 KB
            ..ClipboardStorageConfig::defaults()
        });
        let normalizer = ClipboardRepresentationNormalizer::new(config);

        let observed = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            small_text_data.clone(),
        );

        let result = normalizer.normalize(&observed).await.unwrap();

        // Verify Inline state: inline_data contains actual bytes, no blob_id
        assert_eq!(
            result.payload_state,
            PayloadAvailability::Inline,
            "Small content should have Inline state"
        );
        assert_eq!(
            result.inline_data,
            Some(small_text_data),
            "Inline representation should contain actual data bytes"
        );
        assert_eq!(
            result.blob_id, None,
            "Inline representation should have blob_id = None"
        );
        assert_eq!(
            result.size_bytes, 13,
            "Size should match small content size"
        );
    }

    #[tokio::test]
    async fn test_normalizer_creates_staged_with_preview_for_large_text() {
        let large_text = "x".repeat(20 * 1024); // 20 KB > 16 KB threshold
        let config = Arc::new(ClipboardStorageConfig {
            inline_threshold_bytes: 16 * 1024,
            ..ClipboardStorageConfig::defaults()
        });
        let normalizer = ClipboardRepresentationNormalizer::new(config);

        let observed = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            large_text.as_bytes().to_vec(),
        );

        let result = normalizer.normalize(&observed).await.unwrap();

        assert_eq!(
            result.payload_state,
            PayloadAvailability::Staged,
            "Large text should be staged for blob materialization"
        );
        assert_eq!(
            result.blob_id, None,
            "Staged state should not have blob_id yet"
        );
        assert_eq!(
            result.inline_data,
            Some("x".repeat(PREVIEW_LENGTH_CHARS).into_bytes()),
            "Staged large text should keep a preview inline"
        );
    }
}
