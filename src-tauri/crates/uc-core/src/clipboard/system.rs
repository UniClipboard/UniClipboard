use std::sync::OnceLock;

use crate::{
    ids::{FormatId, RepresentationId},
    ContentHash, MimeType,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotHash(pub ContentHash);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepresentationHash(pub ContentHash);

/// 从系统剪切板中获取到原始数据的快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemClipboardSnapshot {
    pub ts_ms: i64,
    pub representations: Vec<ObservedClipboardRepresentation>,
}

#[derive(Serialize, Deserialize)]
pub struct ObservedClipboardRepresentation {
    pub id: RepresentationId, // 建议：uuid
    pub format_id: FormatId,
    pub mime: Option<MimeType>,
    pub bytes: Vec<u8>,
    /// Cached blake3 content hash — computed lazily on first access.
    ///
    /// Cloning this type copies `cached_hash` as-is. If callers mutate the cloned
    /// instance's public `bytes` after `content_hash()` has already populated the
    /// cache, the cached hash can become stale. Current assumptions/mitigations:
    /// - Deserialized instances start with an empty cache (`serde(skip)`).
    /// - `DecryptingClipboardEventRepository` mutates bytes before hash access.
    ///
    /// Alternative designs if this trade-off changes:
    /// - clear cache in `Clone`
    /// - make `bytes` non-public and force controlled mutation paths
    #[serde(skip)]
    cached_hash: OnceLock<RepresentationHash>,
}

impl std::ops::Deref for RepresentationHash {
    type Target = ContentHash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for SnapshotHash {
    type Target = ContentHash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ObservedClipboardRepresentation {
    pub fn new(
        id: RepresentationId,
        format_id: FormatId,
        mime: Option<MimeType>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            id,
            format_id,
            mime,
            bytes,
            cached_hash: OnceLock::new(),
        }
    }

    pub fn size_bytes(&self) -> i64 {
        self.bytes.len() as i64
    }

    /// Returns the blake3 content hash, computing it lazily and caching the result.
    pub fn content_hash(&self) -> RepresentationHash {
        self.cached_hash
            .get_or_init(|| {
                let hash = blake3::hash(&self.bytes);
                RepresentationHash(ContentHash::from(hash.as_bytes()))
            })
            .clone()
    }
}

fn is_plain_text_representation(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = rep.mime.as_ref() {
        let mime_str = mime.as_str();
        if mime_str.eq_ignore_ascii_case("text/plain")
            || mime_str.to_ascii_lowercase().starts_with("text/plain;")
            || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
        {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("text")
}

fn is_text_representation(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = rep.mime.as_ref() {
        let mime_str = mime.as_str();
        if mime_str.starts_with("text/") || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
        {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("text")
        || rep.format_id.eq_ignore_ascii_case("html")
        || rep.format_id.eq_ignore_ascii_case("rtf")
}

fn is_image_representation(rep: &ObservedClipboardRepresentation) -> bool {
    rep.mime
        .as_ref()
        .is_some_and(|mime| mime.as_str().starts_with("image/"))
        || rep.format_id.eq_ignore_ascii_case("image")
}

/// Check if a mime type and format ID combination represents a file clipboard entry.
///
/// This is the canonical check used across the codebase — wrappers for specific
/// representation types (`ObservedClipboardRepresentation`, `PersistedClipboardRepresentation`)
/// should delegate to this function.
pub fn is_file_mime_or_format(mime: Option<&MimeType>, format_id: &FormatId) -> bool {
    if let Some(mime) = mime {
        let s = mime.as_str();
        if s.eq_ignore_ascii_case("text/uri-list") || s.eq_ignore_ascii_case("file/uri-list") {
            return true;
        }
    }
    format_id.eq_ignore_ascii_case("files")
        || format_id.eq_ignore_ascii_case("public.file-url")
        || format_id.to_ascii_lowercase().contains("uri-list")
}

fn is_file_representation(rep: &ObservedClipboardRepresentation) -> bool {
    is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id)
}

impl Clone for ObservedClipboardRepresentation {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            format_id: self.format_id.clone(),
            mime: self.mime.clone(),
            bytes: self.bytes.clone(),
            cached_hash: self.cached_hash.clone(),
        }
    }
}

impl std::fmt::Debug for ObservedClipboardRepresentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservedClipboardRepresentation")
            .field("id", &self.id)
            .field("format_id", &self.format_id)
            .field("mime", &self.mime)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

impl SystemClipboardSnapshot {
    /// 返回该快照中所有 representation 的总字节大小
    pub fn total_size_bytes(&self) -> i64 {
        self.representations.iter().map(|r| r.size_bytes()).sum()
    }

    /// 是否为空快照（没有任何 representation）
    pub fn is_empty(&self) -> bool {
        self.representations.is_empty()
    }

    /// representation 数量
    pub fn representation_count(&self) -> usize {
        self.representations.len()
    }

    pub fn snapshot_hash(&self) -> SnapshotHash {
        let mut rep_hashes: Vec<[u8; 32]> = self
            .representations
            .iter()
            .map(|r| r.content_hash().bytes)
            .collect();

        // 顺序无关
        rep_hashes.sort_unstable();

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"snapshot-hash-v1|");

        for h in &rep_hashes {
            hasher.update(h);
        }

        let hash = hasher.finalize();
        SnapshotHash(ContentHash::from(hash.as_bytes()))
    }

    pub fn meaningful_origin_key(&self) -> Option<String> {
        if let Some(rep) = self
            .representations
            .iter()
            .find(|rep| is_file_representation(rep))
        {
            return Some(format!("files:{}", rep.content_hash().0));
        }

        if let Some(rep) = self
            .representations
            .iter()
            .find(|rep| is_plain_text_representation(rep))
        {
            return Some(format!("text:{}", rep.content_hash().0));
        }

        if let Some(rep) = self
            .representations
            .iter()
            .find(|rep| is_text_representation(rep))
        {
            return Some(format!("rich-text:{}", rep.content_hash().0));
        }

        if let Some(rep) = self
            .representations
            .iter()
            .find(|rep| is_image_representation(rep))
        {
            return Some(format!("image:{}", rep.content_hash().0));
        }

        None
    }

    pub fn origin_guard_key(&self) -> String {
        self.meaningful_origin_key()
            .unwrap_or_else(|| self.snapshot_hash().to_string())
    }
}
