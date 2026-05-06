//! `ClipboardDocStub` —— Phase 3 子步骤 5f 的桩状态实现。
//!
//! v3 SyncClipboard 协议要求 daemon 暴露:
//!   - `GET /SyncClipboard.json` —— 取当前剪贴板元数据
//!   - `PUT /SyncClipboard.json` —— 上传新元数据
//!   - `GET /file/{dataName}` —— 取附件原始字节
//!   - `PUT /file/{dataName}` —— 上传附件
//!
//! 真正的实现要把这 4 条路由对接到桌面端剪贴板系统(`ApplicationFacade::
//! clipboard_*`),让 iPhone 同步进 / 出的内容直接进入与桌面端 capture
//! 链一致的存储 + 事件流。但 Phase 5 才能开工(`docs/agent/architecture-rules.md`
//! "Implementation Order" 限定先完成 LAN listener / 鉴权再做业务对接);
//! 当前 Phase 3 子步骤 5e/5f 只跑通"路由 + auth 闭环",所以本模块作为
//! Phase 3 stub 落地一份**进程内 Mutex 状态**:
//!
//! - 一份 `latest_doc: Option<SyncClipboardMeta>` —— 最近一次 PUT 的元数据
//! - 一份 `files: HashMap<dataName, (mime, bytes)>` —— 最近 PUT 的附件
//!
//! Phase 5 把这个 stub 整体删掉, 改 facade 的 4 个方法对接 ApplicationFacade
//! 即可, 不影响 webserver 路由层与 use case 类型形态。
//!
//! ## 协议字段映射
//!
//! 本 use case 内的 [`SyncClipboardMeta`] 是**应用层模型**(按
//! `uc-application/AGENTS.md` §12.2 与 wire DTO 区分)。webserver 拿到它后
//! 自己再翻译成 SyncClipboard 协议的 wire JSON:
//!
//! | 应用模型字段 | 协议 JSON 字段 | 说明 |
//! |---|---|---|
//! | `item_type` | `type` (PascalCase value: Text/Image/File/Group) | wire 协议大小写敏感 |
//! | `text` | `text` | 内容或预览 |
//! | `data_name` | `dataName` | hasData=true 时必填 |
//! | `has_data` | `hasData` | 是否有附件 |
//! | `size` | `size` | 附件大小(字节) |
//! | `hash` | `hash` | SHA-256 hex(daemon 在 PUT 后回填) |

use std::collections::HashMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::instrument;

// ─── public-shaped (model / error) ──────────────────────────────────────

/// SyncClipboard 协议里 `type` 字段的 4 个合法值。
///
/// PascalCase 是 wire 形态;Rust 端用枚举, webserver 在 (de)serialize 时与
/// wire 的 PascalCase 字符串一一映射(见 webserver `SyncClipboardDoc`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncClipboardItemType {
    Text,
    Image,
    File,
    Group,
}

/// 一条 SyncClipboard 元数据(应用层模型)。
///
/// 与 wire 协议的字段语义对齐, 但用 Rust idiomatic 命名;webserver 负责
/// (de)serialize 到 SyncClipboard 协议的 JSON 形态。
#[derive(Debug, Clone)]
pub struct SyncClipboardMeta {
    pub item_type: SyncClipboardItemType,
    pub text: String,
    pub data_name: Option<String>,
    pub has_data: bool,
    pub size: u64,
    /// SHA-256 hex —— daemon 在 PUT 时自己算后填进去。GET 路径上一定是
    /// `Some(...)`(stub 也会填), shortcut 客户端不读它但保留以兼容
    /// SyncClipboard 桌面端。
    pub hash: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncClipboardError {
    /// GET 路径上 daemon 没有任何已上传文档 / 文件 → 路由翻成 404。
    #[error("sync clipboard not found")]
    NotFound,

    /// 内部状态损坏(Mutex poison 等), Phase 3 实际不会出现, 兜底语义。
    #[error("sync clipboard internal failure: {0}")]
    Internal(String),
}

// ─── stub state ─────────────────────────────────────────────────────────

#[derive(Default)]
struct StubState {
    latest_doc: Option<SyncClipboardMeta>,
    /// dataName → (mime, bytes)。Phase 3 stub 不限制大小, Phase 5 接真存储
    /// 时换成 blob_store。
    files: HashMap<String, (String, Vec<u8>)>,
}

// ─── use case ───────────────────────────────────────────────────────────

/// Phase 3 stub: 把"剪贴板同步"映射为进程内 Mutex 状态。
///
/// 类型本身实现 4 个 facade 方法所需的全部状态;Phase 5 用真正的
/// `ApplicationFacade::clipboard_*` 对接版本替换之, 删掉这个文件即可。
pub(crate) struct ClipboardDocStub {
    state: Arc<Mutex<StubState>>,
}

impl ClipboardDocStub {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(StubState::default())),
        }
    }

    /// 取最近一次 PUT 的元数据;无任何 PUT 历史则返回 `NotFound`。
    #[instrument(skip(self))]
    pub(crate) async fn get_latest_doc(&self) -> Result<SyncClipboardMeta, SyncClipboardError> {
        let guard = self.state.lock().await;
        guard.latest_doc.clone().ok_or(SyncClipboardError::NotFound)
    }

    /// 上传一份新元数据。daemon 自动算 `SHA-256(text)` 填进 `hash`, 然后
    /// 落 stub 状态;返回的 `SyncClipboardMeta` 是含 hash 的最终版本(让
    /// 路由可以序列化进 PUT 响应)。
    ///
    /// 完全兼容 SyncClipboard shortcut "不上传 hash" 行为 —— 入参 `hash`
    /// 字段被忽略, 始终用 daemon 自算的值覆盖。
    #[instrument(skip(self, meta), fields(item_type = ?meta.item_type, text_len = meta.text.len()))]
    pub(crate) async fn put_doc(
        &self,
        mut meta: SyncClipboardMeta,
    ) -> Result<SyncClipboardMeta, SyncClipboardError> {
        // SHA-256 over the text bytes —— SyncClipboard 项目对 Text 类型用
        // `sha256(text)` 当 hash, 我们对所有 type 都用 text 一份就够了
        // (File 类型的 text 字段 == dataName, 这是 SyncClipboard 的约定)。
        let mut hasher = Sha256::new();
        hasher.update(meta.text.as_bytes());
        let digest = hasher.finalize();
        meta.hash = Some(hex::encode(digest));

        let mut guard = self.state.lock().await;
        guard.latest_doc = Some(meta.clone());
        Ok(meta)
    }

    /// 取附件二进制 + 它存进来时的 mime。无该 dataName → `NotFound`。
    #[instrument(skip(self))]
    pub(crate) async fn get_file(
        &self,
        data_name: &str,
    ) -> Result<(String, Vec<u8>), SyncClipboardError> {
        let guard = self.state.lock().await;
        guard
            .files
            .get(data_name)
            .cloned()
            .ok_or(SyncClipboardError::NotFound)
    }

    /// 上传附件;同名覆盖。
    #[instrument(skip(self, bytes), fields(name, mime, bytes_len = bytes.len()))]
    pub(crate) async fn put_file(
        &self,
        name: String,
        mime: String,
        bytes: Vec<u8>,
    ) -> Result<(), SyncClipboardError> {
        let mut guard = self.state.lock().await;
        guard.files.insert(name, (mime, bytes));
        Ok(())
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_text(text: &str) -> SyncClipboardMeta {
        SyncClipboardMeta {
            item_type: SyncClipboardItemType::Text,
            text: text.into(),
            data_name: None,
            has_data: false,
            size: 0,
            hash: None,
        }
    }

    #[tokio::test]
    async fn get_before_any_put_returns_not_found() {
        let stub = ClipboardDocStub::new();
        let err = stub.get_latest_doc().await.unwrap_err();
        assert!(matches!(err, SyncClipboardError::NotFound));
    }

    #[tokio::test]
    async fn put_then_get_round_trips_and_fills_sha256_hash() {
        let stub = ClipboardDocStub::new();
        let stored = stub.put_doc(meta_text("hello world")).await.unwrap();

        // SHA-256("hello world") = b94d27b9934d3e08...
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert_eq!(stored.hash.as_deref(), Some(expected));

        let fetched = stub.get_latest_doc().await.unwrap();
        assert_eq!(fetched.text, "hello world");
        assert_eq!(fetched.hash.as_deref(), Some(expected));
    }

    #[tokio::test]
    async fn put_overwrites_previous_doc() {
        let stub = ClipboardDocStub::new();
        stub.put_doc(meta_text("first")).await.unwrap();
        stub.put_doc(meta_text("second")).await.unwrap();
        let fetched = stub.get_latest_doc().await.unwrap();
        assert_eq!(fetched.text, "second");
    }

    #[tokio::test]
    async fn get_unknown_file_returns_not_found() {
        let stub = ClipboardDocStub::new();
        let err = stub.get_file("nope.png").await.unwrap_err();
        assert!(matches!(err, SyncClipboardError::NotFound));
    }

    #[tokio::test]
    async fn put_then_get_file_round_trips() {
        let stub = ClipboardDocStub::new();
        stub.put_file("a.png".into(), "image/png".into(), vec![0xDE, 0xAD])
            .await
            .unwrap();
        let (mime, bytes) = stub.get_file("a.png").await.unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, vec![0xDE, 0xAD]);
    }

    #[tokio::test]
    async fn put_file_overwrites_same_name() {
        let stub = ClipboardDocStub::new();
        stub.put_file("a.png".into(), "image/png".into(), vec![1])
            .await
            .unwrap();
        stub.put_file("a.png".into(), "image/jpeg".into(), vec![2])
            .await
            .unwrap();
        let (mime, bytes) = stub.get_file("a.png").await.unwrap();
        assert_eq!(mime, "image/jpeg");
        assert_eq!(bytes, vec![2]);
    }
}
