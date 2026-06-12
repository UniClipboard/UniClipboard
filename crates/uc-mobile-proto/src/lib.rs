//! `uc-mobile-proto` —— mobile-sync 线协议的纯编解码叶子 crate。
//!
//! 这个 crate 只放「给定输入 → 确定字节输出」的纯逻辑，零内部 workspace 依赖，
//! 因此既能被桌面 daemon（经 `uc-application`）复用，也能编译到 iOS/Android
//! target、被未来的 `uc-mobile` FFI crate 共依赖。
//!
//! ## 当前内容
//! - [`connect_uri`]：`uniclipboard://connect` 深链协议 v1 的编解码。
//!
//! ## 不在这里
//! - HTTP / 网络 IO、加密、持久化、平台 API —— 全部留在上层 crate。
//!
//! ## 跨语言契约
//! connect-uri 在 Rust / TS（`src/lib/mobileSyncConnectUri.ts`）/ iOS
//! （`ConnectURI.swift`）各有独立实现，**golden vector 是唯一跨语言契约**，
//! 规范单一真相是 `docs/architecture/mobile-sync-connect-uri.md`。

pub mod connect_uri;

pub use connect_uri::{
    build_mobile_sync_connect_uri, parse_mobile_sync_connect_uri, ConnectPayload, ConnectUriError,
    ConnectUriOther, URI_MAX_LEN,
};
