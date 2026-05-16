use std::io::Read;
use std::path::{Path, PathBuf};
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

/// 表示一条 rep 的负载来源。
///
/// `Inline` —— 字节已经在内存里(从系统剪贴板高层 API 一次读完、或从 wire 反序列化得到)。
/// `LocalFile` —— 仅在 capture 入站短暂存在,指向用户本机文件路径(典型场景:macOS Finder
/// 复制大图片,本地文件可能数十 MB)。此种 source **绝不允许进入 wire 序列化、spool 队列、
/// envelope 构造**;capture pipeline 必须在持久化之前通过 `BlobWriterPort.write_path_if_absent`
/// 把它物化到 blob 仓库,产出 `BlobReady` 状态的 `PersistedClipboardRepresentation`。
#[derive(Debug, Clone)]
pub enum ClipboardPayloadSource {
    Inline(Vec<u8>),
    LocalFile { path: PathBuf, size_bytes: u64 },
}

pub struct ObservedClipboardRepresentation {
    pub id: RepresentationId,
    pub format_id: FormatId,
    pub mime: Option<MimeType>,
    source: ClipboardPayloadSource,
    /// blake3 content hash —— 首次访问时计算并缓存。
    ///
    /// Clone 时直接拷贝(包括 `LocalFile` 上已算过的 hash),只要 path 指向的文件未变更
    /// 即始终有效;若调用方就地替换字节(参见 `inline_bytes_mut`),必须确保替换后不再
    /// 读取过时的 cached hash —— 当前 `DecryptingClipboardEventRepository` 在初始化
    /// rep 后立即解密、再供下游读 hash,顺序安全。
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
    /// 构造一个内存字节 rep。
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
            source: ClipboardPayloadSource::Inline(bytes),
            cached_hash: OnceLock::new(),
        }
    }

    /// 构造一个 path-backed rep。
    ///
    /// `size_bytes` 应来自调用方 `fs::metadata` 的 `len()`,作为 declared 字段;真正的 hash
    /// 计算会在 `content_hash()` 调用时流式读取文件。
    pub fn new_local_file(
        id: RepresentationId,
        format_id: FormatId,
        mime: Option<MimeType>,
        path: PathBuf,
        size_bytes: u64,
    ) -> Self {
        Self {
            id,
            format_id,
            mime,
            source: ClipboardPayloadSource::LocalFile { path, size_bytes },
            cached_hash: OnceLock::new(),
        }
    }

    /// 获取负载来源(用于显式分流 Inline / LocalFile)。
    pub fn source(&self) -> &ClipboardPayloadSource {
        &self.source
    }

    /// 仅当 source 为 `Inline` 时返回字节切片;`LocalFile` 时返回 `None`。
    pub fn inline_bytes(&self) -> Option<&[u8]> {
        match &self.source {
            ClipboardPayloadSource::Inline(b) => Some(b),
            ClipboardPayloadSource::LocalFile { .. } => None,
        }
    }

    /// 仅当 source 为 `Inline` 时返回可变字节;`LocalFile` 时返回 `None`。
    pub fn inline_bytes_mut(&mut self) -> Option<&mut Vec<u8>> {
        match &mut self.source {
            ClipboardPayloadSource::Inline(b) => Some(b),
            ClipboardPayloadSource::LocalFile { .. } => None,
        }
    }

    /// 强制取 Inline 字节,`LocalFile` 时 panic。仅用于"必然 Inline"的语境
    /// (如系统剪贴板 write_snapshot 出站路径、wire 解码后的 inbound 路径)。
    pub fn expect_inline_bytes(&self) -> &[u8] {
        self.inline_bytes().expect(
            "ObservedClipboardRepresentation: LocalFile source not allowed in this code path; \
             caller must handle path-backed reps explicitly via .source()",
        )
    }

    /// 取走 Inline 字节,留下一个空 `Vec<u8>` 占位(仍为 Inline source);`LocalFile` 时
    /// 返回 `Err`。用于 outbound 路径在把字节交给 blob ingest 后清空本地副本。
    ///
    /// **Note**: 调用方负责在 take 之前显式调用 `content_hash()` 让缓存命中,否则 take
    /// 之后再读 hash 会拿到空字节的 blake3。缓存字段本身不被 take 清空,以便保留预计算
    /// 的"原内容"hash 供后续 snapshot_hash 使用。
    pub fn take_inline_bytes(&mut self) -> anyhow::Result<Vec<u8>> {
        match &mut self.source {
            ClipboardPayloadSource::Inline(b) => Ok(std::mem::take(b)),
            ClipboardPayloadSource::LocalFile { .. } => Err(anyhow::anyhow!(
                "take_inline_bytes not supported for LocalFile source"
            )),
        }
    }

    /// 替换 Inline 负载;`LocalFile` 时返回 `Err`(契约违反)。
    pub fn set_inline_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        match &mut self.source {
            ClipboardPayloadSource::Inline(b) => {
                *b = bytes;
                self.cached_hash = OnceLock::new();
                Ok(())
            }
            ClipboardPayloadSource::LocalFile { .. } => Err(anyhow::anyhow!(
                "set_inline_bytes not supported for LocalFile source"
            )),
        }
    }

    pub fn size_bytes(&self) -> i64 {
        match &self.source {
            ClipboardPayloadSource::Inline(b) => b.len() as i64,
            ClipboardPayloadSource::LocalFile { size_bytes, .. } => *size_bytes as i64,
        }
    }

    /// blake3 content hash, computed lazily and cached.
    ///
    /// `Inline` source 直接对内存字节哈希;`LocalFile` source 流式读取文件计算哈希。
    /// 若 `LocalFile` 路径不可读则 panic(此时 capture pipeline 上游应该已经处理过 stat 失败)。
    pub fn content_hash(&self) -> RepresentationHash {
        self.cached_hash
            .get_or_init(|| {
                let hash = match &self.source {
                    ClipboardPayloadSource::Inline(b) => blake3::hash(b),
                    ClipboardPayloadSource::LocalFile { path, .. } => stream_blake3(path)
                        .unwrap_or_else(|err| {
                            panic!(
                                "ObservedClipboardRepresentation::content_hash: failed to stream-hash {} : {err}",
                                path.display()
                            )
                        }),
                };
                RepresentationHash(ContentHash::from(hash.as_bytes()))
            })
            .clone()
    }
}

/// 流式 blake3:对路径文件做分块哈希,常驻内存仅 64 KiB 缓冲。
fn stream_blake3(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

#[derive(Serialize, Deserialize)]
struct ObservedRepProxy {
    id: RepresentationId,
    format_id: FormatId,
    mime: Option<MimeType>,
    bytes: Vec<u8>,
}

impl serde::Serialize for ObservedClipboardRepresentation {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match &self.source {
            ClipboardPayloadSource::Inline(bytes) => {
                let proxy = ObservedRepProxy {
                    id: self.id.clone(),
                    format_id: self.format_id.clone(),
                    mime: self.mime.clone(),
                    bytes: bytes.clone(),
                };
                proxy.serialize(ser)
            }
            ClipboardPayloadSource::LocalFile { path, .. } => {
                Err(serde::ser::Error::custom(format!(
                    "ObservedClipboardRepresentation: LocalFile source cannot be serialized \
                     (path={}); capture pipeline must materialize to blob storage first",
                    path.display()
                )))
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for ObservedClipboardRepresentation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let proxy = ObservedRepProxy::deserialize(d)?;
        Ok(Self {
            id: proxy.id,
            format_id: proxy.format_id,
            mime: proxy.mime,
            source: ClipboardPayloadSource::Inline(proxy.bytes),
            cached_hash: OnceLock::new(),
        })
    }
}

/// 判断一组 (mime, format_id) 是否表示「纯文本」（`text/plain` 家族）。
///
/// 该判断是领域规则，跨表示类型（`Observed*` / `Persisted*`）共享语义：
/// - `text/plain` 与其参数化变体（`text/plain; charset=utf-8` 等）
/// - macOS UTI `public.utf8-plain-text`
/// - format_id 为 `text` 的兜底分支（无 mime 元信息时）
///
/// 不包含 `text/html` / `text/rtf` / `text/markdown` 等富文本子类型。
pub fn is_plain_text_mime_or_format(mime: Option<&MimeType>, format_id: &FormatId) -> bool {
    if let Some(mime) = mime {
        let mime_str = mime.as_str();
        if mime_str.eq_ignore_ascii_case("text/plain")
            || mime_str.to_ascii_lowercase().starts_with("text/plain;")
            || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
        {
            return true;
        }
    }
    format_id.eq_ignore_ascii_case("text")
}

pub(crate) fn is_plain_text_representation(rep: &ObservedClipboardRepresentation) -> bool {
    is_plain_text_mime_or_format(rep.mime.as_ref(), &rep.format_id)
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

pub(crate) fn is_image_representation(rep: &ObservedClipboardRepresentation) -> bool {
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

pub(crate) fn is_file_representation(rep: &ObservedClipboardRepresentation) -> bool {
    is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id)
}

/// `text/html` / `text/rtf` (rich-text carriers). Caller must check
/// `is_file_representation` *before* this so `text/uri-list` doesn't
/// fall through here.
pub(crate) fn is_rich_text_representation(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(m) = rep.mime.as_ref() {
        let s = m.as_str();
        if s.eq_ignore_ascii_case("text/html") || s.eq_ignore_ascii_case("text/rtf") {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("html") || rep.format_id.eq_ignore_ascii_case("rtf")
}

/// MIME / format-id based link detection (e.g. `text/x-url`, `public.url`).
/// Note: macOS does **not** expose copied URLs through these MIMEs — its
/// system pasteboard surfaces them as plain text only. For that case use
/// `is_link_content_representation` (content-based heuristic).
///
/// Callers must check `is_file_representation` first so `text/uri-list`
/// (file's territory) doesn't get reclassified here.
pub(crate) fn is_link_representation(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(m) = rep.mime.as_ref() {
        let s = m.as_str().to_ascii_lowercase();
        if s == "text/x-uri" || s == "text/x-url" || s == "text/uri" || s.contains("url") {
            return true;
        }
    }
    let f = rep.format_id.to_ascii_lowercase();
    f == "url" || f == "uri" || f == "public.url"
}

/// Catch-all for `text/*` reps that didn't match a more specific bucket
/// (markdown, csv, future subtypes). Caller must check the specific
/// buckets *first* so `text/html` / `text/uri-list` aren't reclassified
/// as plain text via this catch-all.
pub(crate) fn is_any_text_representation(rep: &ObservedClipboardRepresentation) -> bool {
    rep.mime
        .as_ref()
        .is_some_and(|m| m.as_str().to_ascii_lowercase().starts_with("text/"))
}

/// Heuristic: a text-bearing rep whose entire payload (after `trim`) is
/// a single URL/URI literal (e.g. `https://x.com`, `mailto:a@b.c`).
/// Used by `ClipboardContentCategorySet::from_snapshot` to recover the
/// `Link` signal on platforms (notably macOS) where the system
/// pasteboard exposes copied URLs *only* as plain text.
///
/// Bounded by `LINK_HEURISTIC_BYTES_LIMIT` to keep the check cheap.
/// Delegates the URL-shape check to [`crate::clipboard::link_utils::is_single_url`]
/// so URL recognition is consistent across the codebase.
pub(crate) fn is_link_content_representation(rep: &ObservedClipboardRepresentation) -> bool {
    const LINK_HEURISTIC_BYTES_LIMIT: usize = 4096;
    if !(is_plain_text_representation(rep) || is_any_text_representation(rep)) {
        return false;
    }
    // 仅纯文本/富文本 rep 走这条路径,LocalFile source 不应出现在文本类别里;
    // 若上游 mis-classify 把 LocalFile 投到这里,直接当非链接处理(不 panic)。
    let Some(bytes) = rep.inline_bytes() else {
        return false;
    };
    if bytes.is_empty() || bytes.len() > LINK_HEURISTIC_BYTES_LIMIT {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let trimmed = text.trim();
    // First gate: must look like a "real" link — `://` or a known
    // schemeless URI prefix. Without this, `Url::parse` accepts opaque
    // URIs like `python:dict` or `note:hello` as valid URLs, which the
    // user almost certainly intends as plain text.
    let is_url_shape = trimmed.contains("://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
        || trimmed.starts_with("sms:");
    if !is_url_shape {
        return false;
    }
    // Second gate: delegate full URL validation to `link_utils` so URL
    // recognition stays consistent across the codebase.
    crate::clipboard::link_utils::is_single_url(text)
}

impl Clone for ObservedClipboardRepresentation {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            format_id: self.format_id.clone(),
            mime: self.mime.clone(),
            source: self.source.clone(),
            cached_hash: self.cached_hash.clone(),
        }
    }
}

impl std::fmt::Debug for ObservedClipboardRepresentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("ObservedClipboardRepresentation");
        s.field("id", &self.id)
            .field("format_id", &self.format_id)
            .field("mime", &self.mime);
        match &self.source {
            ClipboardPayloadSource::Inline(b) => {
                s.field("source", &"Inline").field("bytes_len", &b.len());
            }
            ClipboardPayloadSource::LocalFile { path, size_bytes } => {
                s.field("source", &"LocalFile")
                    .field("path", &path.display().to_string())
                    .field("size_bytes", size_bytes);
            }
        }
        s.finish()
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
