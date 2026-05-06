//! SyncClipboard 协议的"应用层 wire-shape"类型。
//!
//! 给 4 条 SyncClipboard 协议路由(GET/PUT `/SyncClipboard.json`、
//! GET/PUT `/file/{name}`)在 application ↔ webserver 边界上共享同一份
//! 应用模型,webserver 自己再翻译成 protocol JSON wire schema(见
//! `uc-webserver/src/mobile_lan/routes.rs::SyncClipboardDoc`)。
//!
//! P5a.6 之前本文件还承载过一份 `ClipboardDocStub` 的进程内 Mutex 状态实现,
//! 接管"PUT 后 GET 拿回"的 round-trip。从 P5a.6 起,4 条路由分别走真实
//! use case:
//!
//! | 路由 | use case |
//! |---|---|
//! | GET `/SyncClipboard.json` | [`crate::usecases::mobile_sync::get_latest_doc::GetLatestMobileSyncDocUseCase`] |
//! | PUT `/SyncClipboard.json` | [`crate::usecases::mobile_sync::apply_incoming::ApplyIncomingMobileClipUseCase`] (`SyncDoc` 分支) |
//! | GET `/file/{name}` | [`crate::usecases::mobile_sync::get_file::GetMobileSyncFileUseCase`] |
//! | PUT `/file/{name}` | [`crate::usecases::mobile_sync::apply_incoming::ApplyIncomingMobileClipUseCase`] (`BufferFile` 分支) |
//!
//! stub 类型 / Mutex 状态全部删除;本文件只剩 [`SyncClipboardItemType`] +
//! [`SyncClipboardMeta`] 两个 wire-shape 类型,以及它们与协议字段的对照
//! 文档。
//!
//! ## 协议字段映射
//!
//! [`SyncClipboardMeta`] 是**应用层模型**(按 `uc-application/AGENTS.md` §12.2
//! 与 wire DTO 区分);webserver 拿到它后再翻译成 SyncClipboard 协议的 wire
//! JSON:
//!
//! | 应用模型字段 | 协议 JSON 字段 | 说明 |
//! |---|---|---|
//! | `item_type` | `type` (PascalCase value: Text/Image/File/Group) | wire 协议大小写敏感 |
//! | `text` | `text` | 内容或预览 |
//! | `data_name` | `dataName` | hasData=true 时必填 |
//! | `has_data` | `hasData` | 是否有附件 |
//! | `size` | `size` | 附件大小(字节) |
//! | `hash` | `hash` | SHA-256 hex(daemon 在 PUT 后回填) |

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
    /// `Some(...)`,shortcut 客户端不读它但保留以兼容 SyncClipboard 桌面端。
    pub hash: Option<String>,
}
