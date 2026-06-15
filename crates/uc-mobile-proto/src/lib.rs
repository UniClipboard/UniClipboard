//! `uc-mobile-proto` —— mobile-sync 线协议的纯编解码叶子 crate。
//!
//! 这个 crate 只放「给定输入 → 确定字节输出」的纯逻辑，零内部 workspace 依赖，
//! 因此既能被桌面 daemon（经 `uc-application`）复用，也能编译到 iOS/Android
//! target、被未来的 `uc-mobile` FFI crate 共依赖。
//!
//! ## 当前内容
//! 线协议编解码（M0/M1）：
//! - [`connect_uri`]：`uniclipboard://connect` 深链协议 v1 的编解码。
//! - [`clipboard_doc`]：SyncClipboard 线模型 + SHA-256 + 长文本溢出 + publish 助手。
//! - [`hash`]：内容哈希（大写 hex）。
//! - [`history_record`]：history 线模型、composite/split id、`isDelete` 封装、ISO-8601。
//! - [`multipart`]：RFC 7578 multipart 构造 + history query 编码。
//! - [`net_class`]：URL 形态分类、SSID 归一、候选地址排序。
//!
//! 持久化字节形态 + 纯状态决策（M4，E/F 区）：
//! - [`app_settings`]：`app_settings` blob 编解码 + 前向兼容默认值。
//! - [`server_config`]：`server_config_list` blob + §5.5/§5.2 旧格式迁移。
//! - [`history_log`]：`clipboard_history` blob + 去重 append（cap、direction 升级）。
//! - [`loop_guard`]：自同步环检测状态机（纯函数 over 事件缓冲）。
//! - [`payload_cache`]：LRU 驱逐 **决策**（snapshot in → 待删 key out）+ key 校验。
//! - [`file_state`]：watermark / last_synced_hash / live_urls 的字符串/字节归一。
//!
//! 这些模块从 uc-ios `Shared/` 的 Swift 实现逐字节迁移而来（目标 B M0/M1/M4，见
//! `.planning/research/uc-mobile-goal-b-migration-plan.md`）。Swift 实现及其
//! 测试是规范源，每条 golden vector 在测试里注明来源 Swift 测试名。
//!
//! ## 不在这里
//! - HTTP / 网络 IO、加密、平台 API —— 留在上层 crate。
//! - 持久化的 **I/O**（文件原子写、`UserDefaults`、App Group 容器）留原生/上层；
//!   本 crate 只拥有持久化的 **字节/字符串形态** 与纯决策（snapshot in → 结果 out）。
//!
//! ## 跨语言契约
//! connect-uri 在 Rust / TS（`src/lib/mobileSyncConnectUri.ts`）/ iOS
//! （`ConnectURI.swift`）各有独立实现，**golden vector 是唯一跨语言契约**，
//! 规范单一真相是 `docs/architecture/mobile-sync-connect-uri.md`。

pub mod app_settings;
pub mod clipboard_doc;
pub mod connect_uri;
pub mod file_state;
pub mod hash;
pub mod history_log;
pub mod history_record;
pub mod loop_guard;
pub mod multipart;
pub mod net_class;
pub mod payload_cache;
pub mod persist_keys;
pub mod server_config;
pub mod sync_engine;

pub use app_settings::{
    decode_app_settings, encode_app_settings, AppSettings, AppearanceMode,
    DEFAULT_PAYLOAD_CACHE_MAX_BYTES,
};
pub use clipboard_doc::{
    publish_file, publish_image, publish_text, sanitized_filename, Clipboard, ClipboardKind,
};
pub use connect_uri::{
    build_mobile_sync_connect_uri, parse_mobile_sync_connect_uri, ConnectPayload, ConnectUriError,
    ConnectUriOther, URI_MAX_LEN,
};
pub use file_state::{
    decode_live_urls, encode_live_urls, format_watermark, normalize_synced_hash, parse_watermark,
    update_live_url,
};
pub use hash::{hash_matches, sha256_hex_upper};
pub use history_log::{
    append_history, decode_history, encode_history, touch_history, ClipboardHistoryItem,
    HistoryDirection, DEFAULT_HISTORY_CAP,
};
pub use history_record::{
    composite_profile_id, format_iso8601_utc, parse_iso8601_utc, split_patch_id, HistoryRecord,
    HistoryRecordPatch, IsoTimestampError,
};
pub use loop_guard::{
    record as loop_guard_record, tripped as loop_guard_tripped, LoopDirection, LoopGuardEvent,
    DEFAULT_FLIP_THRESHOLD, DEFAULT_WINDOW_SECS,
};
pub use multipart::{HistoryQuery, MultipartBody, TypeMask};
pub use net_class::{
    classify_url, normalize_ssid, ordered_urls, preferred_urls, NetworkContext, ServerUrlClass,
};
pub use payload_cache::{is_valid_cache_key, plan_eviction, CacheEntry};
pub use server_config::{
    decode_server_list, encode_server_list, load_servers, LegacyServerConfig, ServerConfig,
    ServerConfigList, ServerLoad,
};
pub use sync_engine::{
    acknowledge_loop_detection, advance_watermark, backoff_secs, cadence_secs, commit_apply,
    commit_apply_failed, commit_consent_push, commit_converged, commit_history_sync_done,
    commit_push, commit_push_skipped, commit_stage, commit_tick_failure, commit_tick_success,
    handle_active_server_changed, handle_network_route_changed, hashes_equal, is_cold_start,
    is_history_sync_due, is_probe_conclusion_valid, mark_staged_applied, plan_after_server_get,
    plan_preamble, reset_runtime_state, CommitOutcome, Preamble, PreambleProceed, PreambleSnapshot,
    PushDecision, ServerGetSnapshot, ServerNewPlan, ServerRoute, StopReason, SyncConfig,
    SyncRuntimeState, SyncState, TickErrorKind, TickFailureOutcome,
};
