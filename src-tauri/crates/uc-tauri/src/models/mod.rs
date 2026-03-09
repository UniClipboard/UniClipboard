//! Data Transfer Objects and Projection Models
//!
//! This module contains data structures that are exposed to the frontend.
//! These separate the internal domain models from the API contract.
//!
//! 数据传输对象和投影模型
//!
//! 此模块包含暴露给前端的数据结构。
//! 这些将内部领域模型与 API 契约分离。

use serde::{Deserialize, Serialize};

/// Clipboard entry projection for frontend API.
/// 前端 API 的剪贴板条目投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntryProjection {
    /// Unique identifier for the entry
    pub id: String,
    /// Preview content (truncated for large text, placeholder for images)
    pub preview: String,
    /// Whether full detail is available (has blob or is expandable)
    pub has_detail: bool,
    /// Total size in bytes
    pub size_bytes: i64,
    /// Timestamp when captured (Unix timestamp)
    pub captured_at: i64,
    /// Content type description
    pub content_type: String,
    /// Optional thumbnail URL for image entries
    pub thumbnail_url: Option<String>,
    /// Whether the content is encrypted
    pub is_encrypted: bool,
    /// Whether the entry is favorited
    pub is_favorited: bool,
    /// Timestamp when last updated
    pub updated_at: i64,
    /// Timestamp of last access/use
    pub active_time: i64,
}

/// Clipboard entries response with readiness status
/// 带就绪状态的剪贴板条目响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClipboardEntriesResponse {
    /// Session is ready; entries are available
    Ready {
        entries: Vec<ClipboardEntryProjection>,
    },
    /// Session not ready yet (e.g., awaiting unlock)
    NotReady,
}

/// Full clipboard entry detail
/// 剪贴板条目完整详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntryDetail {
    /// Unique identifier for the entry
    pub id: String,
    /// Full content
    pub content: String,
    /// Total size in bytes
    pub size_bytes: i64,
    /// Content type description
    pub content_type: String,
    /// Whether the entry is favorited
    pub is_favorited: bool,
    /// Timestamp when last updated
    pub updated_at: i64,
    /// Timestamp of last access/use
    pub active_time: i64,
}

/// Clipboard entry resource metadata
/// 剪贴板条目资源元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntryResource {
    /// Blob identifier for the entry payload
    pub blob_id: String,
    /// MIME type for the payload
    pub mime_type: String,
    /// Payload size in bytes
    pub size_bytes: i64,
    /// Custom protocol URL for resource fetching
    pub url: String,
}
