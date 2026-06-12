//! `uc-mobile-proto` —— mobile-sync 线协议的纯编解码叶子 crate。
//!
//! 这个 crate 只放「给定输入 → 确定字节输出」的纯逻辑，零内部 workspace 依赖，
//! 因此既能被桌面 daemon（经 `uc-application`）复用，也能编译到 iOS/Android
//! target、被未来的 `uc-mobile` FFI crate 共依赖。
//!
//! ## 当前内容
//! - [`connect_uri`]：`uniclipboard://connect` 深链协议 v1 的编解码。
//! - [`clipboard_doc`]：SyncClipboard 线模型 + SHA-256 + 长文本溢出 + publish 助手。
//! - [`hash`]：内容哈希（大写 hex）。
//! - [`history_record`]：history 线模型、composite/split id、`isDelete` 封装、ISO-8601。
//! - [`multipart`]：RFC 7578 multipart 构造 + history query 编码。
//! - [`net_class`]：URL 形态分类、SSID 归一、候选地址排序。
//!
//! 这些模块从 uc-ios `Shared/` 的 Swift 实现逐字节迁移而来（目标 B M0/M1，见
//! `.planning/research/uc-mobile-goal-b-migration-plan.md`）。Swift 实现及其
//! 测试是规范源，每条 golden vector 在测试里注明来源 Swift 测试名。
//!
//! ## 不在这里
//! - HTTP / 网络 IO、加密、持久化、平台 API —— 全部留在上层 crate。
//!
//! ## 跨语言契约
//! connect-uri 在 Rust / TS（`src/lib/mobileSyncConnectUri.ts`）/ iOS
//! （`ConnectURI.swift`）各有独立实现，**golden vector 是唯一跨语言契约**，
//! 规范单一真相是 `docs/architecture/mobile-sync-connect-uri.md`。

pub mod clipboard_doc;
pub mod connect_uri;
pub mod hash;
pub mod history_record;
pub mod multipart;
pub mod net_class;

pub use clipboard_doc::{
    publish_file, publish_image, publish_text, sanitized_filename, Clipboard, ClipboardKind,
};
pub use connect_uri::{
    build_mobile_sync_connect_uri, parse_mobile_sync_connect_uri, ConnectPayload, ConnectUriError,
    ConnectUriOther, URI_MAX_LEN,
};
pub use hash::{hash_matches, sha256_hex_upper};
pub use history_record::{
    composite_profile_id, format_iso8601_utc, parse_iso8601_utc, split_patch_id, HistoryRecord,
    HistoryRecordPatch, IsoTimestampError,
};
pub use multipart::{HistoryQuery, MultipartBody, TypeMask};
pub use net_class::{
    classify_url, normalize_ssid, ordered_urls, preferred_urls, NetworkContext, ServerUrlClass,
};
