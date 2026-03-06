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
}
