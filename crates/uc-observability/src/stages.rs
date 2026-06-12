//! Phase 87: stage constants are now dotted span names (OTel semconv-aligned).
//! Used directly as info_span! names.
//!
//! Each constant represents a discrete stage in the clipboard capture pipeline.
//! Used as tracing span names to provide consistent, queryable stage identifiers
//! across the `uc-app` and `uc-tauri` crates.

pub const DETECT: &str = "clipboard.detect";
pub const NORMALIZE: &str = "clipboard.normalize";
pub const PERSIST_EVENT: &str = "clipboard.persist_event";
pub const CACHE_REPRESENTATIONS: &str = "clipboard.cache_representations";
pub const SELECT_POLICY: &str = "clipboard.select_policy";
pub const PERSIST_ENTRY: &str = "clipboard.persist_entry";
pub const SPOOL_BLOBS: &str = "clipboard.spool_blobs";

pub const OUTBOUND_PREPARE: &str = "clipboard.outbound_prepare";
pub const OUTBOUND_SEND: &str = "clipboard.outbound_send";
pub const INBOUND_DECODE: &str = "clipboard.inbound_decode";
pub const INBOUND_APPLY: &str = "clipboard.inbound_apply";
